use std::{fmt::Write as _, path::Path, process::Command, time::Duration};

use rssh_test_support::{ChildGuard, ChildOutput, platform_marker_command};

const PROCESS_DEADLINE: Duration = Duration::from_secs(30);
const DETERMINISTIC_PAYLOAD: &str = "rssh-task24-native-e2e|office 中 مرحبا नमस्ते שלום 😀 █";
const PTY_LINK_BEGIN: &str = "RSSH-LINK-BEGIN|";
const PTY_LINK_END: &str = "|RSSH-LINK-END";
const STACK_OVERFLOW_MESSAGE: &str = "overflowed its stack";

pub struct NativeWindowProbe {
    pub output: ChildOutput,
    pub metrics: serde_json::Value,
}

pub fn run_ten_frame_native_window(executable: impl AsRef<Path>) -> NativeWindowProbe {
    run_ten_frame_native_window_at_scale(executable, None)
}

pub fn run_ten_frame_native_window_at_scale(
    executable: impl AsRef<Path>,
    scale_factor: impl Into<Option<f64>>,
) -> NativeWindowProbe {
    let executable = executable.as_ref();
    let framed_marker = format!("{PTY_LINK_BEGIN}{DETERMINISTIC_PAYLOAD}{PTY_LINK_END}");
    let marker_command = platform_marker_command(&framed_marker);
    let mut command = Command::new(executable);
    command
        .args(["-n", "window", "--frames", "10", "--metrics-json", "--"])
        .arg(marker_command.get_program())
        .args(marker_command.get_args())
        .env("RSSH_TEST_DIRECT_GPU_TEXT", "1")
        .env("RSSH_TEST_PTY_LINKAGE", "1");
    if let Some(scale_factor) = scale_factor.into() {
        command.env(
            "RSSH_TEST_WINDOW_SCALE_FACTOR",
            format!("{scale_factor:.2}"),
        );
    }
    for (name, value) in marker_command.get_envs() {
        if let Some(value) = value {
            command.env(name, value);
        }
    }

    let output = ChildGuard::spawn(command, PROCESS_DEADLINE)
        .expect("launch deadline-bounded ten-frame native window")
        .wait()
        .expect("ten-frame native window exits within its deadline");
    let diagnostics = diagnostics(executable, &output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "ten-frame native window did not exit cleanly\n{diagnostics}"
    );
    assert!(
        !stderr.contains(STACK_OVERFLOW_MESSAGE),
        "ten-frame native window reported a stack overflow\n{diagnostics}"
    );
    let metrics = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!("ten-frame native window emitted invalid metrics JSON: {error}\n{diagnostics}")
    });

    NativeWindowProbe { output, metrics }
}

pub fn assert_ten_frame_native_metrics(probe: &NativeWindowProbe) {
    let diagnostics = diagnostics("rssh-app", &probe.output);
    let metrics = &probe.metrics;

    assert_eq!(metrics["gpu_rendered_frames"], 10, "{diagnostics}");
    assert_eq!(metrics["gpu_presented_frames"], 10, "{diagnostics}");
    assert_eq!(metrics["gpu_text_rendered_frames"], 10, "{diagnostics}");
    assert_eq!(
        metrics["gpu_compatibility_frame_uploads"], 0,
        "{diagnostics}"
    );
    assert!(
        metrics["pty_bytes"].as_u64().is_some_and(|bytes| bytes > 0),
        "deterministic marker produced no PTY bytes\n{diagnostics}"
    );
    assert!(metrics["first_pty_byte_ms"].is_number(), "{diagnostics}");
    assert!(
        metrics["first_rendered_cell_ms"].is_number(),
        "{diagnostics}"
    );
    for field in [
        "gpu_backend",
        "gpu_adapter_name",
        "gpu_adapter_type",
        "gpu_surface_format",
        "gpu_present_mode",
    ] {
        assert!(
            metrics[field]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "missing {field}\n{diagnostics}"
        );
    }
    assert!(
        metrics["gpu_software_adapter"].is_boolean(),
        "{diagnostics}"
    );
    assert!(
        metrics["gpu_surface_width"]
            .as_u64()
            .is_some_and(|width| width > 0),
        "{diagnostics}"
    );
    assert!(
        metrics["gpu_surface_height"]
            .as_u64()
            .is_some_and(|height| height > 0),
        "{diagnostics}"
    );
    assert_eq!(metrics["gpu_uncaptured_errors"], 0, "{diagnostics}");
    assert_eq!(metrics["gpu_device_losses"], 0, "{diagnostics}");
    assert_eq!(metrics["text_backend"], "shaped-gpu-atlas", "{diagnostics}");
    for field in [
        "gpu_text_prepared_glyphs",
        "gpu_text_mask_glyphs",
        "gpu_text_color_glyphs",
        "gpu_text_block_glyphs",
    ] {
        assert!(
            metrics[field].as_u64().is_some_and(|glyphs| glyphs > 0),
            "font fixture specimen did not produce {field}\n{diagnostics}"
        );
    }

    assert_eq!(metrics["pty_linkage_found"], true, "{diagnostics}");
    assert_eq!(
        metrics["pty_linkage_digest"],
        digest_hex(rssh_renderer::terminal_bytes_content_digest(
            DETERMINISTIC_PAYLOAD.as_bytes(),
        )),
        "{diagnostics}"
    );
    assert_eq!(
        metrics["terminal_linkage_nonce_found"], true,
        "{diagnostics}"
    );
}

fn digest_hex(digest: rssh_renderer::TerminalContentDigest) -> String {
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn diagnostics(executable: impl std::fmt::Debug, output: &ChildOutput) -> String {
    format!(
        "executable: {executable:?}\nexit status: {:?} (code: {:?})\nstdout: {:?}\nstderr: {:?}",
        output.status,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}
