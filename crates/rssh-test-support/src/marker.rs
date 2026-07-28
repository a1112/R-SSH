use std::process::Command;

const MARKER_ENV: &str = "RSSH_TEST_MARKER";

/// Builds a native command that writes `marker` as exact UTF-8 bytes to stdout.
///
/// The marker is passed through the child environment rather than interpolated
/// into a shell program, avoiding platform quoting and command injection.
#[must_use]
pub fn platform_marker_command(marker: &str) -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$bytes=[Text.Encoding]::UTF8.GetBytes($env:RSSH_TEST_MARKER); \
                 $stdout=[Console]::OpenStandardOutput(); \
                 $stdout.Write($bytes,0,$bytes.Length)",
            ])
            .env(MARKER_ENV, marker);
        command
    }

    #[cfg(not(windows))]
    {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf '%s' \"$RSSH_TEST_MARKER\""])
            .env(MARKER_ENV, marker);
        command
    }
}
