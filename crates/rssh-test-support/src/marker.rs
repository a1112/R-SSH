use std::process::Command;

const MARKER_ENV: &str = "RSSH_TEST_MARKER";

/// Builds a native command that writes `marker` as exact UTF-8 bytes to stdout.
///
/// The marker is passed through the child environment rather than interpolated
/// into a shell program, avoiding platform quoting and command injection.
#[must_use]
pub fn platform_marker_command(marker: &str) -> Command {
    platform_marker_command_with_lifetime(marker, false)
}

/// Builds a native command that writes `marker` as exact UTF-8 bytes to stdout
/// and remains alive until the caller closes its terminal session.
///
/// Native-window frame probes use this variant so their rendering assertions do
/// not race a deliberately short-lived marker process during window startup.
#[must_use]
pub fn platform_marker_command_hold_open(marker: &str) -> Command {
    platform_marker_command_with_lifetime(marker, true)
}

fn platform_marker_command_with_lifetime(marker: &str, hold_open: bool) -> Command {
    #[cfg(windows)]
    {
        let sleep = if hold_open {
            "; Start-Sleep -Seconds 300"
        } else {
            ""
        };
        let script = format!(
            "$bytes=[Text.Encoding]::UTF8.GetBytes($env:RSSH_TEST_MARKER); \
             $stdout=[Console]::OpenStandardOutput(); \
             $stdout.Write($bytes,0,$bytes.Length){sleep}"
        );
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .env(MARKER_ENV, marker);
        command
    }

    #[cfg(not(windows))]
    {
        let program = if hold_open {
            "printf '%s' \"$RSSH_TEST_MARKER\"; sleep 300"
        } else {
            "printf '%s' \"$RSSH_TEST_MARKER\""
        };
        let mut command = Command::new("/bin/sh");
        command.args(["-c", program]).env(MARKER_ENV, marker);
        command
    }
}
