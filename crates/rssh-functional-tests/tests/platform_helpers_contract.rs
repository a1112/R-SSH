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
        "MovePointer",
        "MOUSEEVENTF_ABSOLUTE",
        "MOUSEEVENTF_VIRTUALDESK",
        "GetSystemMetrics",
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
    let production_smoke = repo_file("scripts/functional/smoke-production-tauri.sh");
    assert!(source.contains("search --onlyvisible --pid"));
    assert!(source.contains("find_visible_window"));
    assert!(source.contains("pgrep -P"));
    assert!(source.contains("for _ in {1..100}"));
    assert!(source.contains("windowactivate --sync"));
    assert!(source.contains("xdotool"));
    assert!(source.contains("type --clearmodifiers --delay 0 -- \"$*\""));
    assert!(source.contains("Enter|enter) key=Return"));
    assert!(source.contains("xdotool key --clearmodifiers \"$key\""));
    assert!(source.contains("click \"$button\""));
    assert!(source.contains("sleep 0.1"));
    assert!(!source.contains("type --clearmodifiers --delay 0 --window"));
    assert!(!source.contains("click --window"));
    assert!(!source.contains("/dev/stdin"));
    assert!(production_smoke.contains("bash scripts/functional/x11-xtest-input.sh"));
}

#[test]
fn x11_clipboard_helper_owns_one_real_selection_request() {
    let source = repo_file("scripts/functional/x11-set-clipboard.sh");
    assert!(source.contains("xclip -selection clipboard -loops 1"));
    assert!(source.contains("printf '%s' \"$1\""));
    assert!(!source.contains("eval"));
}

#[test]
fn windows_helper_restores_before_focusing_and_verifies_foreground_ownership() {
    let source = repo_file("scripts/functional/windows-send-input.ps1");
    assert!(source.contains("GetForegroundWindow"));
    assert!(source.contains("IsIconic"));
    let restore = source
        .find("ShowWindow($window, 9)")
        .expect("restore before focus");
    let focus = source[restore..]
        .find("SetForegroundWindow($window)")
        .map(|offset| restore + offset)
        .expect("focus after restore");
    assert!(restore < focus);
    assert!(source.contains("GetForegroundWindow() -eq $window"));
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
    assert!(source.contains("--shell=kiosk-shell.so"));
    assert!(source.contains("RSSH_FUNCTIONAL_WESTON_BACKEND=x11"));
    assert!(source.contains("Xvfb"));
    assert!(source.contains("wait_for_x11_display"));
    assert!(source.contains("wait_for_weston_socket"));
    assert!(source.contains("wait_for_weston_window"));
    assert!(source.contains("dump_startup_logs"));
    assert!(source.contains("export RSSH_FUNCTIONAL_XDOTOOL="));
    let weston_socket_wait = source
        .split("wait_for_weston_socket()")
        .nth(1)
        .and_then(|body| body.split("wait_for_weston_window()").next())
        .expect("Weston socket readiness function");
    let weston_window_wait = source
        .split("wait_for_weston_window()")
        .nth(1)
        .and_then(|body| body.split("Xvfb ").next())
        .expect("Weston window readiness function");
    assert!(weston_socket_wait.contains("for _ in {1..300}"));
    assert!(weston_window_wait.contains("for _ in {1..300}"));
    assert!(!source.contains("--backend=headless-backend.so"));
}

#[test]
fn x11_harness_owns_runtime_dbus_display_and_window_manager_lifetimes() {
    let source = repo_file("scripts/functional/run-x11-seat.sh");
    for contract in [
        "mkdir -m 700",
        "export XDG_RUNTIME_DIR=",
        "export RSSH_FUNCTIONAL_XDOTOOL=",
        "dbus-run-session",
        "xvfb-run --auto-servernum",
        "openbox",
        "trap cleanup EXIT",
    ] {
        assert!(
            source.contains(contract),
            "missing X11 harness contract {contract}"
        );
    }
    assert!(source.contains("if ! rm -rf -- \"$runtime\""));
    assert!(!source.contains("eval"));
}
