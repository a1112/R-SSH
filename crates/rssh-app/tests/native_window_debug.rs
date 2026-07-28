#![cfg(windows)]

use std::{process::Command, time::Duration};

use rssh_test_support::{ChildGuard, ChildOutput};

const PROCESS_DEADLINE: Duration = Duration::from_secs(30);
const RSSH_APP_EXECUTABLE: &str = env!("CARGO_BIN_EXE_rssh-app");
const STACK_OVERFLOW_MESSAGE: &str = "overflowed its stack";
const CDB_FRAME_EVIDENCE: &str = "\
CDB frame evidence for the existing Windows debug failure:
  window::run: ~617,680 B
  configured_startup_app_with_constructor: ~365,440 B
  validate_cli_config_overrides: ~59,616 B
  combined startup stack: ~1,042,736 B
Root-cause context: large startup state is passed by value through these frames; this test
must keep launching the real debug executable instead of constructing a winit event loop
on the test worker thread.";

#[test]
fn state_json_control_exits_successfully() {
    const COMMAND_INTENT: &str = "rssh-app -n window --state-json";
    const ARGUMENTS: &[&str] = &["-n", "window", "--state-json"];
    let output = run_rssh_app(COMMAND_INTENT, ARGUMENTS);
    let diagnostics = diagnostics(COMMAND_INTENT, ARGUMENTS, &output);

    assert!(
        output.status.success(),
        "state-report control did not exit successfully\n{diagnostics}"
    );
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap_or_else(|error| {
        panic!("state-report control emitted invalid JSON: {error}\n{diagnostics}")
    });
}

#[test]
fn one_frame_native_window_does_not_overflow_the_debug_stack() {
    const COMMAND_INTENT: &str = "rssh-app -n window --frames 1";
    const ARGUMENTS: &[&str] = &["-n", "window", "--frames", "1"];
    let output = run_rssh_app(COMMAND_INTENT, ARGUMENTS);
    let diagnostics = diagnostics(COMMAND_INTENT, ARGUMENTS, &output);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "native window did not exit successfully; expected the current RED failure to report \
         Windows status 0xC00000FD until Task 4 removes the oversized startup frames\n\
         {diagnostics}"
    );
    assert!(
        !stderr.contains(STACK_OVERFLOW_MESSAGE),
        "native window reported a stack overflow\n{diagnostics}"
    );
}

#[test]
fn native_window_reports_real_gpu_presentation_for_one_and_ten_frames() {
    for frames in [1_u64, 10] {
        let frame_text = frames.to_string();
        let arguments = [
            "-n",
            "window",
            "--frames",
            frame_text.as_str(),
            "--metrics-json",
        ];
        let command_intent = format!("rssh-app -n window --frames {frames} --metrics-json");
        let output = run_rssh_app(&command_intent, &arguments);
        let diagnostics = diagnostics(&command_intent, &arguments, &output);

        assert!(
            output.status.success(),
            "native GPU metrics smoke failed\n{diagnostics}"
        );
        let metrics: serde_json::Value =
            serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
                panic!("native window emitted invalid metrics JSON: {error}\n{diagnostics}")
            });
        assert!(
            metrics["gpu_backend"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "missing GPU backend\n{diagnostics}"
        );
        assert!(
            metrics["gpu_adapter_name"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "missing GPU adapter name\n{diagnostics}"
        );
        assert!(metrics["gpu_adapter_type"].is_string(), "{diagnostics}");
        assert!(
            metrics["gpu_software_adapter"].is_boolean(),
            "{diagnostics}"
        );
        assert!(metrics["gpu_surface_format"].is_string(), "{diagnostics}");
        assert!(metrics["gpu_present_mode"].is_string(), "{diagnostics}");
        assert_eq!(metrics["gpu_rendered_frames"], frames, "{diagnostics}");
        assert_eq!(metrics["gpu_presented_frames"], frames, "{diagnostics}");
        assert_eq!(metrics["gpu_uncaptured_errors"], 0, "{diagnostics}");
        assert_eq!(metrics["gpu_device_losses"], 0, "{diagnostics}");
        assert!(metrics["text_backend"].is_string(), "{diagnostics}");
    }
}

#[test]
fn native_window_reconfigures_the_direct_surface_after_resize() {
    const COMMAND_INTENT: &str =
        "rssh-app -n window --frames 2 --metrics-json (resize after first present)";
    const ARGUMENTS: &[&str] = &["-n", "window", "--frames", "2", "--metrics-json"];
    let mut command = Command::new(RSSH_APP_EXECUTABLE);
    command
        .args(ARGUMENTS)
        .env("RSSH_TEST_RESIZE_AFTER_FIRST_PRESENT", "800x480");
    let output = ChildGuard::spawn(command, PROCESS_DEADLINE)
        .expect("spawn deterministic native resize smoke")
        .wait()
        .expect("deterministic native resize smoke completes");
    let diagnostics = diagnostics(COMMAND_INTENT, ARGUMENTS, &output);
    let metrics: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!("native resize smoke emitted invalid metrics JSON: {error}\n{diagnostics}")
        });

    assert!(output.status.success(), "{diagnostics}");
    assert_eq!(metrics["gpu_rendered_frames"], 2, "{diagnostics}");
    assert_eq!(metrics["gpu_presented_frames"], 2, "{diagnostics}");
    assert_eq!(metrics["gpu_surface_width"], 800, "{diagnostics}");
    assert_eq!(metrics["gpu_surface_height"], 480, "{diagnostics}");
    assert!(
        metrics["gpu_surface_reconfigurations"]
            .as_u64()
            .is_some_and(|count| count >= 2),
        "resize did not reconfigure the direct surface\n{diagnostics}"
    );
}

fn run_rssh_app(command_intent: &str, args: &[&str]) -> ChildOutput {
    let mut command = Command::new(RSSH_APP_EXECUTABLE);
    command.args(args);

    let guard = ChildGuard::spawn(command, PROCESS_DEADLINE).unwrap_or_else(|error| {
        panic!(
            "failed to launch `{command_intent}` with a {PROCESS_DEADLINE:?} deadline\n\
             executable: {RSSH_APP_EXECUTABLE:?}\n\
             arguments: {args:?}\n\
             error: {error}\n\
             {CDB_FRAME_EVIDENCE}"
        )
    });
    guard.wait().unwrap_or_else(|error| {
        panic!(
            "`{command_intent}` did not complete within its bounded subprocess harness\n\
             executable: {RSSH_APP_EXECUTABLE:?}\n\
             arguments: {args:?}\n\
             error: {error}\n\
             {CDB_FRAME_EVIDENCE}"
        )
    })
}

fn diagnostics(command_intent: &str, args: &[&str], output: &ChildOutput) -> String {
    format!(
        "command intent: `{command_intent}`\n\
         executable: {RSSH_APP_EXECUTABLE:?}\n\
         arguments: {args:?}\n\
         exit status: {:?} (code: {:?})\n\
         bounded stdout: {:?}\n\
         bounded stderr: {:?}\n\
         {CDB_FRAME_EVIDENCE}",
        output.status,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}
