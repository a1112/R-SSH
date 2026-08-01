mod common;

use std::{fs, path::PathBuf};

const RSSH_APP_EXECUTABLE: &str = env!("CARGO_BIN_EXE_rssh-app");

#[test]
fn native_window_e2e_presents_ten_frames_from_a_real_pty() {
    let probe = common::run_ten_frame_native_window(RSSH_APP_EXECUTABLE);

    common::assert_ten_frame_native_metrics(&probe);
}

#[test]
fn workflow_contract_has_exact_pr_and_supplemental_runner_sets() {
    let ci = read_repo_file(".github/workflows/ci.yml");
    let nightly = read_repo_file(".github/workflows/nightly.yml");

    assert_eq!(
        runner_labels(&ci),
        ["windows-2025", "ubuntu-24.04", "macos-15"]
    );
    assert_eq!(
        runner_labels(&nightly),
        ["windows-11-arm", "ubuntu-24.04-arm", "macos-15-intel"]
    );
    assert!(ci.contains("permissions:\n  contents: read"));
    assert!(nightly.contains("permissions:\n  contents: read"));
    assert!(nightly.contains("tags:\n      - \"v*\""));
    for workflow in [&ci, &nightly] {
        assert_action_refs_are_pinned(workflow);
        assert!(workflow.contains("version --json"));
        assert!(workflow.contains("pty_backend"));
        assert!(workflow.contains("target"));
        assert!(workflow.contains("run-native-window.ps1"));
        assert!(workflow.contains("run-native-window.sh"));
    }
}

#[test]
fn linux_display_and_strict_script_contracts_are_explicit() {
    let ci = read_repo_file(".github/workflows/ci.yml");
    let nightly = read_repo_file(".github/workflows/nightly.yml");
    let powershell = read_repo_file("scripts/ci/run-native-window.ps1");
    let shell = read_repo_file("scripts/ci/run-native-window.sh");

    for workflow in [&ci, &nightly] {
        assert!(workflow.contains("xvfb-run"), "missing X11 native E2E");
        assert!(workflow.contains("weston"), "missing Wayland native E2E");
        assert!(
            workflow.contains("XDG_RUNTIME_DIR"),
            "missing Wayland runtime"
        );
        assert!(workflow.contains("phase_status=$?"));
        assert!(workflow.contains("wait \"$weston_pid\" || true"));
    }
    for contract in [
        "$ErrorActionPreference = \"Stop\"",
        "$PSNativeCommandUseErrorActionPreference = $true",
        "WaitForExit",
        "job.Dispose()",
        "Complete-StreamBeforeDeadline",
        "Assert-WindowsCommandLineQuoting",
        "HarnessSelfTest",
        "RsshCiJobObject",
        "KILL_ON_JOB_CLOSE",
        "CreateProcessW",
        "CREATE_SUSPENDED",
        "ResumeThread",
        "FailAssignmentForTest",
        "timeout-stdout-marker",
        "timeout-stderr-marker",
        "C:\\path with space\\",
        "--locked",
        "--all-targets",
        "OpenSSH client probe",
        "-FilePath \"ssh\"",
        "@(\"-V\")",
        "\"rssh-ssh\", \"--all-targets\"",
        "\"openssh_loopback\"",
        "RSSH_REQUIRE_OPENSSH = \"1\"",
        "version",
        "--json",
    ] {
        assert!(
            powershell.contains(contract),
            "PowerShell script is missing {contract}"
        );
    }
    assert!(!powershell.contains("GetAwaiter().GetResult"));
    for contract in [
        "set -euo pipefail",
        "--harness-self-test",
        "start_new_session=True",
        "os.killpg(process_group, 0)",
        "signal.SIGKILL",
        "leader-exit process-group self-test",
        "process-group timeout self-test",
        "timeout-stdout-marker",
        "timeout-stderr-marker",
        "timeout",
        "trap",
        "kill",
        "--locked",
        "--all-targets",
        "ssh -V",
        "rssh-ssh --all-targets",
        "openssh_loopback",
        "RSSH_REQUIRE_OPENSSH=1",
        "version --json",
    ] {
        assert!(
            shell.contains(contract),
            "shell script is missing {contract}"
        );
    }
}

#[test]
fn workflow_contract_normalizes_windows_line_endings() {
    assert_eq!(
        normalize_line_endings("permissions:\r\n  contents: read\r\n"),
        "permissions:\n  contents: read\n"
    );
}

fn read_repo_file(path: &str) -> String {
    let path = repo_root().join(path);
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    normalize_line_endings(&contents)
}

fn normalize_line_endings(contents: &str) -> String {
    contents.replace("\r\n", "\n").replace('\r', "\n")
}

fn runner_labels(workflow: &str) -> Vec<&str> {
    workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- runner: "))
        .collect()
}

fn assert_action_refs_are_pinned(workflow: &str) {
    for action in workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- uses: "))
    {
        let revision = action
            .split_once('@')
            .unwrap_or_else(|| panic!("action has no revision: {action}"))
            .1
            .split_whitespace()
            .next()
            .expect("action revision after @");
        assert_eq!(
            revision.len(),
            40,
            "action is not pinned to a full SHA: {action}"
        );
        assert!(
            revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "action revision is not hexadecimal: {action}"
        );
        assert!(
            action.contains(" # "),
            "pinned action lacks version comment: {action}"
        );
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root above rssh-app")
        .to_owned()
}
