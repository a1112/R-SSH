#![cfg(windows)]

use std::{process::Command, time::Duration};

use rssh_test_support::{ChildGuard, ChildOutput};

const PROCESS_DEADLINE: Duration = Duration::from_secs(30);
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
    let output = run_rssh_app(COMMAND_INTENT, &["-n", "window", "--state-json"]);
    let diagnostics = diagnostics(COMMAND_INTENT, &output);

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
    let output = run_rssh_app(COMMAND_INTENT, &["-n", "window", "--frames", "1"]);
    let diagnostics = diagnostics(COMMAND_INTENT, &output);
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

fn run_rssh_app(command_intent: &str, args: &[&str]) -> ChildOutput {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rssh-app"));
    command.args(args);

    let guard = ChildGuard::spawn(command, PROCESS_DEADLINE).unwrap_or_else(|error| {
        panic!(
            "failed to launch `{command_intent}` with a {PROCESS_DEADLINE:?} deadline: \
             {error}\n{CDB_FRAME_EVIDENCE}"
        )
    });
    guard.wait().unwrap_or_else(|error| {
        panic!(
            "`{command_intent}` did not complete within its bounded subprocess harness: \
             {error}\n{CDB_FRAME_EVIDENCE}"
        )
    })
}

fn diagnostics(command_intent: &str, output: &ChildOutput) -> String {
    format!(
        "command intent: `{command_intent}`\n\
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
