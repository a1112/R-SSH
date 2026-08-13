use std::{fs, path::PathBuf};

fn repo_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::read_to_string(root.join(path)).unwrap()
}

#[test]
fn windows_helper_uses_send_input_and_targets_only_the_observed_process_window() {
    let source = repo_file("scripts/functional/windows-send-input.ps1");
    for contract in [
        "SendInput",
        "EnumWindows",
        "GetWindowThreadProcessId",
        "SetForegroundWindow",
        "ClientToScreen",
    ] {
        assert!(source.contains(contract), "missing {contract}");
    }
    assert!(!source.contains("Invoke-Expression"));
    assert!(source.contains("[BitConverter]::ToUInt32"));
    assert!(!source.contains("[uint32]([int]$ActionArguments[1] * 120)"));
}

#[test]
fn x11_helper_discovers_the_pid_window_and_calls_xdotool_xtest_actions() {
    let source = repo_file("scripts/functional/x11-xtest-input.sh");
    assert!(source.contains("search --onlyvisible --pid"));
    assert!(source.contains("windowactivate --sync"));
    assert!(source.contains("xdotool"));
    assert!(!source.contains("/dev/stdin"));
}

#[test]
fn macos_helper_refuses_untrusted_accessibility_and_posts_cg_events() {
    let source = repo_file("scripts/functional/macos-cgevent.swift");
    assert!(source.contains("AXIsProcessTrustedWithOptions"));
    assert!(source.contains("CGEventPost"));
    assert!(source.contains("NSRunningApplication"));
    assert!(!source.contains("osascript"));
}

#[test]
fn wayland_harness_is_nested_weston_x11_not_headless_input_emulation() {
    let source = repo_file("scripts/functional/run-wayland-seat.sh");
    assert!(source.contains("--backend=x11-backend.so"));
    assert!(source.contains("RSSH_FUNCTIONAL_WESTON_BACKEND=x11"));
    assert!(source.contains("Xvfb"));
    assert!(!source.contains("--backend=headless-backend.so"));
}
