use std::collections::BTreeMap;

use rssh_functional_tests::{
    ActionV1, DriverOperation, InputBackend, PlatformInputDriver, PlatformInputError,
};

fn environment(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn x11_uses_xtest_and_never_falls_back_to_synthetic_stdin() {
    let driver = PlatformInputDriver::from_environment(
        InputBackend::X11Xtest,
        &environment(&[
            ("DISPLAY", ":99"),
            ("RSSH_FUNCTIONAL_XDOTOOL", "/usr/bin/xdotool"),
            ("RSSH_FUNCTIONAL_X11_WINDOW", "4194305"),
        ]),
    )
    .expect("x11 capability");

    let operations = driver
        .plan(&ActionV1::TypeText {
            text: "R-SSH 终端".to_owned(),
        })
        .expect("type plan");
    assert_eq!(operations.len(), 3);
    assert!(matches!(
        &operations[0],
        DriverOperation::Command { program, arguments, .. }
            if program == "/usr/bin/xdotool"
                && arguments == &["windowactivate", "--sync", "4194305"]
    ));
    assert!(matches!(
        &operations[1],
        DriverOperation::Command { arguments, .. }
            if arguments == &["type", "--clearmodifiers", "--delay", "0", "R-SSH 终端"]
    ));
    assert!(matches!(
        &operations[2],
        DriverOperation::Command { arguments, .. }
            if arguments == &["sleep", "0.1"]
    ));
}

#[test]
fn wayland_requires_nested_weston_x11_backend_and_injects_through_its_seat() {
    let headless = PlatformInputDriver::from_environment(
        InputBackend::WaylandWestonSeat,
        &environment(&[
            ("DISPLAY", ":99"),
            ("WAYLAND_DISPLAY", "wayland-rssh"),
            ("RSSH_FUNCTIONAL_WESTON_BACKEND", "headless"),
            ("RSSH_FUNCTIONAL_XDOTOOL", "/usr/bin/xdotool"),
            ("RSSH_FUNCTIONAL_WESTON_WINDOW", "6291457"),
        ]),
    );
    assert!(matches!(
        headless,
        Err(PlatformInputError::CapabilityGate { capability, .. })
            if capability == "wayland_weston_x11_seat"
    ));

    let driver = PlatformInputDriver::from_environment(
        InputBackend::WaylandWestonSeat,
        &environment(&[
            ("DISPLAY", ":99"),
            ("WAYLAND_DISPLAY", "wayland-rssh"),
            ("RSSH_FUNCTIONAL_WESTON_BACKEND", "x11"),
            ("RSSH_FUNCTIONAL_XDOTOOL", "/usr/bin/xdotool"),
            ("RSSH_FUNCTIONAL_WESTON_WINDOW", "6291457"),
        ]),
    )
    .expect("nested Weston seat");
    let operations = driver
        .plan(&ActionV1::MouseClick {
            x: 120,
            y: 80,
            button: rssh_functional_tests::MouseButton::Left,
        })
        .expect("pointer plan");
    assert!(matches!(
        &operations[0],
        DriverOperation::Command { arguments, .. }
            if arguments == &["windowfocus", "--sync", "6291457"]
    ));
    assert!(operations.iter().all(|operation| matches!(
        operation,
        DriverOperation::Command { environment, .. }
            if environment.get("DISPLAY").map(String::as_str) == Some(":99")
    )));
    assert!(operations.iter().any(|operation| matches!(
        operation,
        DriverOperation::Command { arguments, .. }
            if arguments.iter().any(|argument| argument == "mousemove")
    )));
    assert!(operations.iter().all(|operation| matches!(
        operation,
        DriverOperation::Command { arguments, .. }
            if arguments.first().map(String::as_str) != Some("click")
                || !arguments.iter().any(|argument| argument == "--window")
    )));
    assert!(matches!(
        operations.last(),
        Some(DriverOperation::Command { arguments, .. })
            if arguments == &["sleep", "0.1"]
    ));
}

#[test]
fn xtest_clipboard_paste_uses_xclip_before_the_focused_keyboard_binding() {
    let driver = PlatformInputDriver::from_environment(
        InputBackend::WaylandWestonSeat,
        &environment(&[
            ("DISPLAY", ":99"),
            ("WAYLAND_DISPLAY", "wayland-rssh"),
            ("RSSH_FUNCTIONAL_WESTON_BACKEND", "x11"),
            ("RSSH_FUNCTIONAL_XDOTOOL", "/usr/bin/xdotool"),
            ("RSSH_FUNCTIONAL_WESTON_WINDOW", "6291457"),
        ]),
    )
    .expect("nested Weston seat");
    let operations = driver
        .plan(&ActionV1::ClipboardPaste {
            text: "clipboard-probe".to_owned(),
        })
        .expect("clipboard plan");
    assert!(matches!(
        &operations[1],
        DriverOperation::Command { program, arguments, .. }
            if program == "bash"
                && arguments == &["scripts/functional/x11-set-clipboard.sh", "clipboard-probe"]
    ));
    assert!(matches!(
        &operations[2],
        DriverOperation::Command { program, arguments, .. }
            if program == "/usr/bin/xdotool"
                && arguments == &["key", "--clearmodifiers", "ctrl+shift+v"]
    ));
    assert!(matches!(
        operations.last(),
        Some(DriverOperation::Command { arguments, .. })
            if arguments == &["sleep", "0.1"]
    ));
}

#[test]
fn xtest_clipboard_paste_accepts_a_surface_specific_keyboard_binding() {
    let driver = PlatformInputDriver::from_environment(
        InputBackend::WaylandWestonSeat,
        &environment(&[
            ("DISPLAY", ":99"),
            ("WAYLAND_DISPLAY", "wayland-rssh"),
            ("RSSH_FUNCTIONAL_WESTON_BACKEND", "x11"),
            ("RSSH_FUNCTIONAL_XDOTOOL", "/usr/bin/xdotool"),
            ("RSSH_FUNCTIONAL_WESTON_WINDOW", "6291457"),
            ("RSSH_FUNCTIONAL_XTEST_PASTE_KEY", "ctrl+v"),
        ]),
    )
    .expect("nested Weston seat");
    let operations = driver
        .plan(&ActionV1::ClipboardPaste {
            text: "clipboard-probe".to_owned(),
        })
        .expect("clipboard plan");
    assert!(matches!(
        &operations[2],
        DriverOperation::Command { arguments, .. }
            if arguments == &["key", "--clearmodifiers", "ctrl+v"]
    ));
}

#[test]
fn macos_accessibility_is_a_hard_capability_gate() {
    let denied = PlatformInputDriver::from_environment(
        InputBackend::MacosCgEvent,
        &environment(&[(
            "RSSH_FUNCTIONAL_MACOS_CGEVENT_HELPER",
            "/opt/rssh/cgevent-helper",
        )]),
    );
    assert!(matches!(
        denied,
        Err(PlatformInputError::CapabilityGate { capability, .. })
            if capability == "macos_accessibility"
    ));

    let driver = PlatformInputDriver::from_environment(
        InputBackend::MacosCgEvent,
        &environment(&[
            ("RSSH_FUNCTIONAL_MACOS_ACCESSIBILITY", "authorized"),
            (
                "RSSH_FUNCTIONAL_MACOS_CGEVENT_HELPER",
                "/opt/rssh/cgevent-helper",
            ),
        ]),
    )
    .expect("authorized helper");
    assert!(matches!(
        &driver.plan(&ActionV1::FocusWindow).expect("focus plan")[0],
        DriverOperation::Command { program, arguments, .. }
            if program == "/usr/bin/xcrun"
                && arguments == &["swift", "/opt/rssh/cgevent-helper", "focus"]
    ));
}

#[test]
fn xtest_maps_logical_enter_to_the_x11_return_keysym() {
    let driver = PlatformInputDriver::from_environment(
        InputBackend::X11Xtest,
        &environment(&[
            ("DISPLAY", ":99"),
            ("RSSH_FUNCTIONAL_XDOTOOL", "/usr/bin/xdotool"),
            ("RSSH_FUNCTIONAL_X11_WINDOW", "0x4200007"),
        ]),
    )
    .expect("X11 target");
    let operations = driver
        .plan(&ActionV1::Key {
            key: "Enter".to_owned(),
            modifiers: Vec::new(),
        })
        .expect("key plan");

    assert!(matches!(
        &operations[1],
        DriverOperation::Command { arguments, .. }
            if arguments == &["key", "--clearmodifiers", "Return"]
    ));
}

#[test]
fn platform_driver_rejects_non_os_actions_instead_of_faking_them() {
    let driver = PlatformInputDriver::from_environment(
        InputBackend::WindowsSendInput,
        &environment(&[("RSSH_FUNCTIONAL_APP_PID", "4660")]),
    )
    .expect("windows target");
    assert!(matches!(
        driver.plan(&ActionV1::PtyInput {
            bytes_hex: "41".to_owned(),
        }),
        Err(PlatformInputError::UnsupportedAction(_))
    ));
}

#[test]
fn windows_clipboard_paste_uses_the_native_terminal_binding() {
    let driver = PlatformInputDriver::from_environment(
        InputBackend::WindowsSendInput,
        &environment(&[("RSSH_FUNCTIONAL_APP_PID", "4660")]),
    )
    .expect("windows target");
    let operations = driver
        .plan(&ActionV1::ClipboardPaste {
            text: "functional clipboard".to_owned(),
        })
        .expect("paste plan");
    let DriverOperation::Command { arguments, .. } = &operations[0];
    let action = arguments
        .iter()
        .position(|argument| argument == "-Action")
        .expect("explicit action flag");
    let action_arguments = arguments
        .iter()
        .position(|argument| argument == "-ActionArgumentsJson")
        .expect("explicit action-arguments flag");
    assert_eq!(arguments.get(action + 1).map(String::as_str), Some("paste"));
    let encoded = arguments
        .get(action_arguments + 1)
        .expect("encoded arguments");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(encoded).unwrap(),
        ["functional clipboard"]
    );

    let script = include_str!("../../../scripts/functional/windows-send-input.ps1");
    assert!(script.contains("VirtualKey(0x10, $true)"));
    assert!(script.contains("VirtualKey(0x11, $true)"));
    assert!(script.contains("function Set-ClipboardWithRetry"));
    assert!(script.contains("Set-ClipboardWithRetry ($ActionArguments -join \" \")"));
    assert!(script.contains("Send-VirtualKey 0x56"));
    assert!(script.contains("VirtualKey(0x11, $false)"));
    assert!(script.contains("VirtualKey(0x10, $false)"));
}

#[test]
fn windows_mouse_actions_are_one_json_value_not_positional_powershell_arguments() {
    let driver = PlatformInputDriver::from_environment(
        InputBackend::WindowsSendInput,
        &environment(&[("RSSH_FUNCTIONAL_APP_PID", "4660")]),
    )
    .expect("windows target");
    let cases = [
        (
            ActionV1::MouseClick {
                x: 80,
                y: 81,
                button: rssh_functional_tests::MouseButton::Left,
            },
            vec!["80", "81", "left"],
        ),
        (
            ActionV1::MouseDrag {
                from_x: 1,
                from_y: 2,
                to_x: 3,
                to_y: 4,
                button: rssh_functional_tests::MouseButton::Right,
            },
            vec!["1", "2", "3", "4", "right"],
        ),
        (
            ActionV1::MouseWheel {
                delta_x: -2,
                delta_y: 3,
            },
            vec!["-2", "3"],
        ),
    ];
    for (action, expected) in cases {
        let operations = driver.plan(&action).expect("mouse plan");
        let DriverOperation::Command { arguments, .. } = &operations[0];
        let index = arguments
            .iter()
            .position(|argument| argument == "-ActionArgumentsJson")
            .expect("JSON flag");
        let decoded: Vec<String> = serde_json::from_str(&arguments[index + 1]).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(arguments.len(), index + 2);
    }
}

#[test]
fn windows_tauri_production_smoke_passes_json_arrays_to_the_input_helper() {
    let script = include_str!("../../../scripts/functional/smoke-production-tauri.ps1");
    for arguments in [
        r#"-Action click -ActionArgumentsJson '["80","80","left"]'"#,
        r#"-ActionArgumentsJson '["exit 7"]'"#,
        r#"-ActionArgumentsJson '["enter"]'"#,
        r#"-ActionArgumentsJson '["close"]'"#,
    ] {
        assert!(
            script.contains(arguments),
            "missing JSON helper argument {arguments}"
        );
    }
    assert!(!script.contains(" -ActionArguments "));
}

#[test]
fn windows_close_uses_focused_system_input_instead_of_an_unobserved_message() {
    let script = include_str!("../../../scripts/functional/windows-send-input.ps1");
    assert!(script.contains("SendMessageTimeout"));
    assert!(script.contains("SendMessageTimeout(WM_CLOSE) failed"));
    assert!(!script.contains("PostMessage(WM_CLOSE)"));
}

#[test]
fn windows_process_target_prefers_the_titled_application_window() {
    let script = include_str!("../../../scripts/functional/windows-send-input.ps1");
    let find_window = script
        .split("public static IntPtr FindWindow(uint expectedProcessId)")
        .nth(1)
        .expect("process window lookup")
        .split("public static IntPtr FindWindowByTitle")
        .next()
        .expect("bounded process window lookup");

    assert!(find_window.contains("GetWindowText"));
    assert!(find_window.contains("title.Length > 0"));
}
