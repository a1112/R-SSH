mod common;

use std::{env, fs, path::PathBuf};

#[cfg(target_os = "windows")]
use std::process::Command;

const RSSH_APP_EXECUTABLE: &str = env!("CARGO_BIN_EXE_rssh-app");

#[test]
fn native_window_e2e_presents_ten_frames_from_a_real_pty() {
    let executable = packaged_or_cargo_app_executable();
    let probe = common::run_ten_frame_native_window(&executable);

    common::assert_ten_frame_native_metrics(&probe);
}

#[test]
fn native_window_e2e_preserves_gpu_text_at_windows_scale_factors() {
    let executable = packaged_or_cargo_app_executable();
    for scale_factor in [1.0, 1.25, 1.5, 2.0] {
        let probe = common::run_ten_frame_native_window_at_scale(&executable, Some(scale_factor));
        common::assert_ten_frame_native_metrics(&probe);
    }
}

#[cfg(target_os = "windows")]
#[test]
fn native_window_e2e_uses_borderless_integrated_titlebar() {
    let executable = packaged_or_cargo_app_executable();
    let script = r#"
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class RsshWindowStyleProbe {
  private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
  [StructLayout(LayoutKind.Sequential)] private struct RECT {
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
  }
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
  [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
  [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] private static extern IntPtr GetWindowLongPtr(IntPtr hWnd, int index);
  [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] private static extern bool GetClientRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] private static extern bool ClientToScreen(IntPtr hWnd, ref POINT point);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
  [StructLayout(LayoutKind.Sequential)] private struct POINT {
    public int X;
    public int Y;
  }
  public static bool TryGetMainWindowFrame(uint targetProcessId, out bool clientFillsWindow, out string description) {
    bool fillsWindow = false;
    bool matched = false;
    string foundDescription = "";
    EnumWindows((hWnd, _) => {
      uint processId;
      GetWindowThreadProcessId(hWnd, out processId);
      if (processId != targetProcessId || !IsWindowVisible(hWnd)) {
        return true;
      }
      RECT rect;
      GetWindowRect(hWnd, out rect);
      if (rect.Right - rect.Left < 100 || rect.Bottom - rect.Top < 100) {
        return true;
      }
      RECT clientRect;
      GetClientRect(hWnd, out clientRect);
      var clientOrigin = new POINT();
      ClientToScreen(hWnd, ref clientOrigin);
      var windowWidth = rect.Right - rect.Left;
      var windowHeight = rect.Bottom - rect.Top;
      var clientWidth = clientRect.Right - clientRect.Left;
      var clientHeight = clientRect.Bottom - clientRect.Top;
      fillsWindow = clientOrigin.X == rect.Left
        && clientOrigin.Y == rect.Top
        && clientWidth == windowWidth
        && clientHeight == windowHeight;
      var title = new StringBuilder(512);
      GetWindowText(hWnd, title, title.Capacity);
      var style = GetWindowLongPtr(hWnd, -16).ToInt64();
      foundDescription = string.Format("hwnd=0x{0:x} style=0x{1:x8} title={2} window={3},{4},{5},{6} client-origin={7},{8} client={9},{10}",
        hWnd.ToInt64(), style, title, rect.Left, rect.Top, rect.Right, rect.Bottom,
        clientOrigin.X, clientOrigin.Y, clientWidth, clientHeight);
      matched = true;
      return false;
    }, IntPtr.Zero);
    clientFillsWindow = fillsWindow;
    description = foundDescription;
    return matched;
  }
}
'@
$process = Start-Process -FilePath $env:RSSH_STYLE_PROBE_EXE -ArgumentList @('--skip-config', 'start', '--always-new-process', '--no-auto-connect') -PassThru
try {
  # Native GPU startup can be serialized behind the other real-window tests
  # in this binary. Keep the probe budget aligned with the 30-second native
  # process deadline instead of treating a slow HWND creation as a style bug.
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
    $clientFillsWindow = $false
    $description = ''
    if ([RsshWindowStyleProbe]::TryGetMainWindowFrame([uint32]$process.Id, [ref]$clientFillsWindow, [ref]$description)) {
      if ($clientFillsWindow) {
        exit 0
      }
      if ([DateTime]::UtcNow -ge $deadline) {
        throw ('integrated titlebar window retained a native frame inset: {0}' -f $description)
      }
      Start-Sleep -Milliseconds 50
      continue
    }
    Start-Sleep -Milliseconds 50
  } while ([DateTime]::UtcNow -lt $deadline)
  throw 'native window did not expose an HWND before the probe deadline'
} finally {
  Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
}
"#;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("RSSH_STYLE_PROBE_EXE", executable)
        .output()
        .expect("run native window decoration probe");
    assert!(
        output.status.success(),
        "native window decoration probe failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn packaged_or_cargo_app_executable() -> PathBuf {
    env::var_os("RSSH_TEST_APP_EXECUTABLE")
        .map_or_else(|| PathBuf::from(RSSH_APP_EXECUTABLE), PathBuf::from)
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
    let powershell = format!(
        "{}\n{}",
        read_repo_file("scripts/ci/run-native-window.ps1"),
        read_repo_file("scripts/ci/process-harness.ps1")
    );
    let shell = format!(
        "{}\n{}",
        read_repo_file("scripts/ci/run-native-window.sh"),
        read_repo_file("scripts/ci/process-harness.sh")
    );

    assert_linux_openssh_server_contract([("CI", &ci), ("nightly", &nightly)]);
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
    assert!(powershell.contains("process-harness.ps1"));
    assert!(shell.contains("source \"$script_dir/process-harness.sh\""));
}

fn assert_linux_openssh_server_contract(workflows: [(&str, &str); 2]) {
    let install_marker = "- name: Install Linux native E2E dependencies";
    for (name, workflow) in workflows {
        let native_e2e_job = workflow_job(workflow, "native-terminal-e2e")
            .unwrap_or_else(|| panic!("{name} is missing the native-terminal-e2e job"));
        let install_start = native_e2e_job
            .find(install_marker)
            .unwrap_or_else(|| panic!("{name} is missing the Linux dependency step"));
        let after_marker = &native_e2e_job[install_start + install_marker.len()..];
        let install_end = after_marker
            .find("\n      - name: ")
            .map_or(native_e2e_job.len(), |offset| {
                install_start + install_marker.len() + offset
            });
        let install_step = &native_e2e_job[install_start..install_end];

        assert!(install_step.contains("if: runner.os == 'Linux'"));
        for package in ["openssh-client", "openssh-server", "xvfb", "weston"] {
            assert!(
                install_step.contains(package),
                "{name} Linux dependency step is missing {package}"
            );
        }
        for service_guard in [
            "policy-rc.d",
            "exit 101",
            "trap restore_policy_rc EXIT",
            "sudo cp -a \"$backup_path\" \"$policy_path\"",
            "sudo chmod 0755 \"$policy_path\"",
        ] {
            assert!(
                install_step.contains(service_guard),
                "{name} Linux dependency step can start system ssh service: missing {service_guard}"
            );
        }
        let mut previous = None;
        for ordered_marker in [
            "sudo cp -a \"$policy_path\" \"$backup_path\"",
            "trap restore_policy_rc EXIT",
            "sudo rm -f -- \"$policy_path\"",
            "sudo tee \"$policy_path\"",
            "apt-get install --yes",
        ] {
            let position = install_step.find(ordered_marker).unwrap_or_else(|| {
                panic!("{name} Linux dependency step is missing {ordered_marker}")
            });
            if let Some(previous) = previous {
                assert!(
                    previous < position,
                    "{name} Linux dependency step orders {ordered_marker} before its prerequisite"
                );
            }
            previous = Some(position);
        }
        for e2e_step in [
            "- name: Native E2E with Xvfb/X11",
            "- name: Native E2E with Weston/Wayland",
        ] {
            let e2e_start = native_e2e_job
                .find(e2e_step)
                .unwrap_or_else(|| panic!("{name} is missing {e2e_step}"));
            assert!(
                install_start < e2e_start,
                "{name} Linux dependencies must be installed before {e2e_step}"
            );
        }
    }

    let native_ssh = read_repo_file("crates/rssh-ssh/tests/loopback_native.rs");
    for contract in [
        concat!(
            "#[cfg(target_os = \"linux\")]\n",
            "#[test]\n",
            "fn native_client_interoperates_with_an_isolated_real_openssh_sshd()"
        ),
        "(\"sshd\", \"-V\")",
        "required Linux OpenSSH fixture tool {tool} missing",
    ] {
        assert!(
            native_ssh.contains(contract),
            "required Linux real-sshd probe is missing {contract}"
        );
    }
    let function_start = native_ssh
        .find("fn native_client_interoperates_with_an_isolated_real_openssh_sshd()")
        .expect("required Linux real-sshd test function");
    let attribute_start = native_ssh[..function_start]
        .rfind("\n\n")
        .map_or(0, |offset| offset + 2);
    let attributes = &native_ssh[attribute_start..function_start];
    assert!(
        !attributes.lines().any(|line| {
            let attribute = line.trim_start();
            attribute.starts_with("#[") && attribute.contains("ignore")
        }),
        "required Linux real-sshd probe must not be ignored"
    );
}

fn workflow_job<'a>(workflow: &'a str, job_name: &str) -> Option<&'a str> {
    let marker = format!("\n  {job_name}:\n");
    let start = workflow.find(&marker)? + 1;
    let after_header = start + marker.len() - 1;
    let remainder = &workflow[after_header..];
    let end = remainder
        .match_indices('\n')
        .find_map(|(offset, _)| {
            let next_line = &remainder[offset + 1..];
            let line = next_line.lines().next()?;
            (line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
                .then_some(after_header + offset)
        })
        .unwrap_or(workflow.len());
    Some(&workflow[start..end])
}

#[test]
fn linux_openssh_contract_is_scoped_to_the_native_e2e_job() {
    let workflow = r"
jobs:
  quality:
    steps:
      - name: Install Linux native E2E dependencies
  native-terminal-e2e:
    steps:
      - name: Native E2E with Xvfb/X11
";

    let job = workflow_job(workflow, "native-terminal-e2e").expect("native E2E job");
    assert!(!job.contains("Install Linux native E2E dependencies"));
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
