use std::process::Command;

const MARKER_ENV: &str = "RSSH_TEST_MARKER";

/// Builds a native command that writes `marker` as exact UTF-8 bytes to stdout.
///
/// The marker is passed through the child environment rather than interpolated
/// into a shell program, avoiding platform quoting and command injection.
#[must_use]
pub fn platform_marker_command(marker: &str) -> Command {
    platform_marker_command_with_delay(marker, None)
}

/// Builds a native command that writes `marker` as exact UTF-8 bytes to stdout
/// and remains alive until the frame-limited parent reaps it.
///
/// Native-window frame probes use this variant so cold GPU or process startup
/// cannot race a short-lived marker process. The command still has a bounded
/// fallback lifetime in case it is run outside the deadline-bounded harness.
#[must_use]
pub fn platform_marker_command_for_window_frames(marker: &str) -> Command {
    platform_marker_command_with_delay(marker, Some(300))
}

fn platform_marker_command_with_delay(marker: &str, delay_seconds: Option<u64>) -> Command {
    #[cfg(windows)]
    {
        let sleep = delay_seconds.map_or_else(String::new, |seconds| {
            format!("; Start-Sleep -Seconds {seconds}")
        });
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
        let program = delay_seconds.map_or_else(
            || "printf '%s' \"$RSSH_TEST_MARKER\"".to_owned(),
            |seconds| format!("printf '%s' \"$RSSH_TEST_MARKER\"; sleep {seconds}"),
        );
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &program]).env(MARKER_ENV, marker);
        command
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Read, process::Stdio, thread, time::Duration};

    use super::platform_marker_command_for_window_frames;

    #[test]
    fn frame_marker_outlives_the_old_five_second_startup_window() {
        let marker = "frame-probe-ready";
        let mut command = platform_marker_command_for_window_frames(marker);
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = command.spawn().expect("spawn frame marker command");
        let mut stdout = child.stdout.take().expect("capture frame marker stdout");
        let mut actual = vec![0; marker.len()];
        stdout
            .read_exact(&mut actual)
            .expect("frame marker is written before waiting");
        assert_eq!(actual, marker.as_bytes());

        thread::sleep(Duration::from_secs(6));
        assert!(
            child.try_wait().expect("poll frame marker child").is_none(),
            "the frame marker child exited before its parent completed the frame probe"
        );

        child.kill().expect("terminate frame marker child");
        child.wait().expect("reap frame marker child");
    }
}
