use std::{
    ffi::OsStr,
    fmt,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const CAPTURE_LIMIT: usize = 256 * 1024;
const OMISSION_RESERVE: usize = 96;

/// The bounded stdout, stderr, and status collected from a child process.
#[derive(Debug)]
pub struct ChildOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// A failure to launch, observe, or terminate a deadline-bound child process.
#[derive(Debug)]
pub enum ChildGuardError {
    /// An operating-system operation failed.
    Io {
        operation: &'static str,
        source: io::Error,
        output: Option<ChildOutput>,
    },
    /// The child exceeded its deadline and was killed and reaped.
    TimedOut {
        timeout: Duration,
        output: ChildOutput,
    },
}

impl ChildGuardError {
    /// Returns any bounded, redacted child output preserved with the error.
    #[must_use]
    pub fn output(&self) -> Option<&ChildOutput> {
        match self {
            Self::Io { output, .. } => output.as_ref(),
            Self::TimedOut { output, .. } => Some(output),
        }
    }
}

impl fmt::Display for ChildGuardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                source,
                output,
            } => {
                write!(formatter, "child process {operation} failed: {source}")?;
                if let Some(output) = output {
                    write_diagnostics(formatter, output)?;
                }
                Ok(())
            }
            Self::TimedOut { timeout, output } => {
                write!(
                    formatter,
                    "child process exceeded its {timeout:?} deadline and was killed and reaped"
                )?;
                write_diagnostics(formatter, output)
            }
        }
    }
}

impl std::error::Error for ChildGuardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::TimedOut { .. } => None,
        }
    }
}

/// Owns a native child process and guarantees bounded waiting plus fallback cleanup.
pub struct ChildGuard {
    child: Option<Child>,
    deadline: Instant,
    timeout: Duration,
    stdout: File,
    stderr: File,
    redactions: Vec<Vec<u8>>,
}

impl ChildGuard {
    /// Spawns `command` with stdout and stderr redirected to bounded diagnostics.
    ///
    /// The child inherits no stdin. Environment values explicitly configured on
    /// `command`, plus its current directory, are redacted from error output.
    ///
    /// # Errors
    ///
    /// Returns an error if diagnostic files cannot be created or cloned, or if
    /// the operating system cannot spawn the child.
    pub fn spawn(mut command: Command, timeout: Duration) -> Result<Self, ChildGuardError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| ChildGuardError::Io {
                operation: "deadline setup",
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "child timeout exceeds the platform Instant range",
                ),
                output: None,
            })?;
        let stdout = tempfile::tempfile().map_err(|source| ChildGuardError::Io {
            operation: "stdout capture setup",
            source,
            output: None,
        })?;
        let stderr = tempfile::tempfile().map_err(|source| ChildGuardError::Io {
            operation: "stderr capture setup",
            source,
            output: None,
        })?;
        let stdout_writer = stdout.try_clone().map_err(|source| ChildGuardError::Io {
            operation: "stdout capture clone",
            source,
            output: None,
        })?;
        let stderr_writer = stderr.try_clone().map_err(|source| ChildGuardError::Io {
            operation: "stderr capture clone",
            source,
            output: None,
        })?;
        let redactions = command_redactions(&command);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_writer))
            .stderr(Stdio::from(stderr_writer));
        let child = command.spawn().map_err(|source| ChildGuardError::Io {
            operation: "spawn",
            source,
            output: None,
        })?;

        Ok(Self {
            child: Some(child),
            deadline,
            timeout,
            stdout,
            stderr,
            redactions,
        })
    }

    /// Waits until the child exits or its deadline expires.
    ///
    /// # Errors
    ///
    /// Returns [`ChildGuardError::TimedOut`] only after killing and reaping the
    /// child. Operating-system polling, termination, wait, or capture failures
    /// are returned as [`ChildGuardError::Io`].
    pub fn wait(mut self) -> Result<ChildOutput, ChildGuardError> {
        loop {
            let Some(child) = self.child.as_mut() else {
                return Err(missing_child_error("poll"));
            };
            let try_wait = child.try_wait();
            match try_wait {
                Ok(Some(status)) => {
                    self.child.take();
                    return self.capture_output(status, false);
                }
                Ok(None) if Instant::now() < self.deadline => {
                    thread::sleep(
                        self.deadline
                            .saturating_duration_since(Instant::now())
                            .min(POLL_INTERVAL),
                    );
                }
                Ok(None) => return self.terminate_after_timeout(),
                Err(source) => return self.terminate_after_observation_error(source),
            }
        }
    }

    fn terminate_after_timeout(&mut self) -> Result<ChildOutput, ChildGuardError> {
        let Some(child) = self.child.as_mut() else {
            return Err(missing_child_error("terminate after timeout"));
        };
        let kill_result = child.kill();
        let wait_result = child.wait();
        self.child.take();

        let status = wait_result.map_err(|source| ChildGuardError::Io {
            operation: "wait after timeout",
            source,
            output: None,
        })?;
        let output = self.capture_output(status, true)?;
        if let Err(source) = kill_result {
            return Err(ChildGuardError::Io {
                operation: "kill after timeout",
                source,
                output: Some(output),
            });
        }

        Err(ChildGuardError::TimedOut {
            timeout: self.timeout,
            output,
        })
    }

    fn terminate_after_observation_error(
        &mut self,
        observation_error: io::Error,
    ) -> Result<ChildOutput, ChildGuardError> {
        let Some(child) = self.child.as_mut() else {
            return Err(missing_child_error("clean up after observation error"));
        };
        let kill_result = child.kill();
        let wait_result = child.wait();
        self.child.take();

        let (output, wait_error) = match wait_result {
            Ok(status) => (self.capture_output(status, true).ok(), None),
            Err(source) => (None, Some(source)),
        };
        let source = kill_result
            .err()
            .or(wait_error)
            .unwrap_or(observation_error);
        Err(ChildGuardError::Io {
            operation: "observe and clean up",
            source,
            output,
        })
    }

    fn capture_output(
        &mut self,
        status: ExitStatus,
        redact: bool,
    ) -> Result<ChildOutput, ChildGuardError> {
        let mut stdout = read_bounded(&mut self.stdout).map_err(|source| ChildGuardError::Io {
            operation: "read stdout capture",
            source,
            output: None,
        })?;
        let mut stderr = read_bounded(&mut self.stderr).map_err(|source| ChildGuardError::Io {
            operation: "read stderr capture",
            source,
            output: None,
        })?;
        if redact {
            redact_values(&mut stdout, &self.redactions);
            redact_values(&mut stderr, &self.redactions);
        }
        Ok(ChildOutput {
            status,
            stdout,
            stderr,
        })
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

fn missing_child_error(operation: &'static str) -> ChildGuardError {
    ChildGuardError::Io {
        operation,
        source: io::Error::other("child process ownership was already released"),
        output: None,
    }
}

fn write_diagnostics(formatter: &mut fmt::Formatter<'_>, output: &ChildOutput) -> fmt::Result {
    write!(
        formatter,
        "; status={:?}; stdout={:?}; stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn command_redactions(command: &Command) -> Vec<Vec<u8>> {
    let mut redactions = command
        .get_envs()
        .filter_map(|(_, value)| value)
        .map(OsStr::to_string_lossy)
        .filter(|value| !value.is_empty())
        .map(|value| value.as_bytes().to_vec())
        .collect::<Vec<_>>();
    if let Some(path) = command.get_current_dir() {
        let value = path.as_os_str().to_string_lossy();
        if !value.is_empty() {
            redactions.push(value.as_bytes().to_vec());
        }
    }
    redactions
        .sort_unstable_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    redactions.dedup();
    redactions
}

fn read_bounded(file: &mut File) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let retained = CAPTURE_LIMIT - OMISSION_RESERVE;
    let head_limit = retained / 2;
    let tail_limit = retained - head_limit;
    let mut head = Vec::with_capacity(head_limit);
    let mut tail = Vec::with_capacity(tail_limit);
    let mut total = 0_u64;
    let mut chunk = [0_u8; 8192];

    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        let mut remaining = &chunk[..read];
        if head.len() < head_limit {
            let keep = (head_limit - head.len()).min(remaining.len());
            head.extend_from_slice(&remaining[..keep]);
            remaining = &remaining[keep..];
        }
        if !remaining.is_empty() {
            if remaining.len() >= tail_limit {
                tail.clear();
                tail.extend_from_slice(&remaining[remaining.len() - tail_limit..]);
            } else {
                let excess = tail
                    .len()
                    .saturating_add(remaining.len())
                    .saturating_sub(tail_limit);
                if excess > 0 {
                    tail.drain(..excess);
                }
                tail.extend_from_slice(remaining);
            }
        }
    }

    if total <= CAPTURE_LIMIT as u64 {
        head.extend_from_slice(&tail);
        return Ok(head);
    }

    let omitted = total.saturating_sub((head.len() + tail.len()) as u64);
    head.extend_from_slice(format!("\n... <{omitted} bytes omitted> ...\n").as_bytes());
    head.extend_from_slice(&tail);
    Ok(head)
}

fn redact_values(bytes: &mut Vec<u8>, redactions: &[Vec<u8>]) {
    for redaction in redactions {
        if redaction.is_empty() {
            continue;
        }
        *bytes = replace_all(bytes, redaction, b"<redacted>");
    }
    if bytes.len() > CAPTURE_LIMIT {
        bytes.truncate(CAPTURE_LIMIT);
    }
}

fn replace_all(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(index) = rest
        .windows(needle.len())
        .position(|candidate| candidate == needle)
    {
        result.extend_from_slice(&rest[..index]);
        result.extend_from_slice(replacement);
        rest = &rest[index + needle.len()..];
    }
    result.extend_from_slice(rest);
    result
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        ffi::OsStr,
        io::Write,
        process::Command,
        time::{Duration, Instant},
    };

    use super::{CAPTURE_LIMIT, ChildGuard, ChildGuardError, read_bounded};
    use crate::{TempHome, platform_marker_command};

    const HELPER_MODE: &str = "RSSH_TEST_HELPER_MODE";

    fn helper_command(mode: &str) -> Command {
        let mut command = Command::new(env::current_exe().expect("current test executable"));
        command
            .args([
                "--exact",
                "process::tests::child_process_helper",
                "--nocapture",
            ])
            .env(HELPER_MODE, mode);
        command
    }

    #[cfg(windows)]
    fn timeout_command() -> Command {
        let mut command = Command::new("cmd.exe");
        command.args([
            "/D",
            "/Q",
            "/C",
            "(echo child-started)& \
             (echo diagnostic-before-timeout 1>&2)& \
             (echo %RSSH_TEST_SECRET% 1>&2)& \
             for /L %i in (0,0,1) do @rem",
        ]);
        command
    }

    #[cfg(not(windows))]
    fn timeout_command() -> Command {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "printf 'child-started\\n'; \
             printf 'diagnostic-before-timeout\\n' >&2; \
             printf '%s\\n' \"$RSSH_TEST_SECRET\" >&2; \
             while :; do :; done",
        ]);
        command
    }

    #[test]
    fn child_process_helper() {
        if let Ok("environment") = env::var(HELPER_MODE).as_deref() {
            let home = env::var_os("HOME").expect("HOME is set");
            let userprofile = env::var_os("USERPROFILE").expect("USERPROFILE is set");
            println!("HOME={}", home.to_string_lossy());
            println!("USERPROFILE={}", userprofile.to_string_lossy());
        }
    }

    #[test]
    fn child_guard_returns_output_before_deadline() {
        let output = ChildGuard::spawn(
            platform_marker_command("ready-before-deadline"),
            Duration::from_secs(5),
        )
        .expect("spawn marker command")
        .wait()
        .expect("marker command completes");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"ready-before-deadline");
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn child_guard_kills_and_reaps_timed_out_child() {
        let started = Instant::now();
        let secret = "super-sensitive-test-value";
        let mut command = timeout_command();
        command.env("RSSH_TEST_SECRET", secret);
        let error = ChildGuard::spawn(command, Duration::from_millis(250))
            .expect("spawn timeout helper")
            .wait()
            .expect_err("helper must time out");

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!error.to_string().contains(secret));
        assert!(error.to_string().contains("<redacted>"));
        let ChildGuardError::TimedOut { output, .. } = error else {
            panic!("expected timeout error");
        };
        assert!(!output.status.success());
        assert!(
            output
                .stdout
                .windows(b"child-started".len())
                .any(|part| part == b"child-started")
        );
        assert!(
            output
                .stderr
                .windows(b"diagnostic-before-timeout".len())
                .any(|part| part == b"diagnostic-before-timeout")
        );
        assert!(
            output
                .stderr
                .windows(b"<redacted>".len())
                .any(|part| part == b"<redacted>")
        );
        assert!(
            !output
                .stderr
                .windows(secret.len())
                .any(|part| part == secret.as_bytes())
        );
    }

    #[test]
    fn temp_home_isolates_home_and_userprofile() {
        let original_home = env::var_os("HOME");
        let original_userprofile = env::var_os("USERPROFILE");
        let temp_home = TempHome::new().expect("create isolated home");
        let mut command = helper_command("environment");
        temp_home.apply_to(&mut command);

        let output = ChildGuard::spawn(command, Duration::from_secs(5))
            .expect("spawn environment helper")
            .wait()
            .expect("environment helper completes");
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 helper output");
        let path = temp_home.path().to_string_lossy();

        assert!(stdout.contains(&format!("HOME={path}")));
        assert!(stdout.contains(&format!("USERPROFILE={path}")));
        assert_eq!(env::var_os("HOME"), original_home);
        assert_eq!(env::var_os("USERPROFILE"), original_userprofile);
        assert_eq!(
            temp_home
                .environment()
                .get(OsStr::new("HOME"))
                .map(AsRef::as_ref),
            Some(temp_home.path().as_os_str())
        );
    }

    #[test]
    fn marker_command_emits_exact_utf8_marker() {
        let marker = "R-SSH-终端-🦀";
        let output = ChildGuard::spawn(platform_marker_command(marker), Duration::from_secs(5))
            .expect("spawn UTF-8 marker command")
            .wait()
            .expect("UTF-8 marker command completes");

        assert!(output.status.success());
        assert_eq!(output.stdout, marker.as_bytes());
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn bounded_capture_retains_the_start_and_end() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"start").unwrap();
        file.write_all(&vec![b'x'; CAPTURE_LIMIT]).unwrap();
        file.write_all(b"end").unwrap();

        let captured = read_bounded(&mut file).unwrap();

        assert!(captured.starts_with(b"start"));
        assert!(captured.ends_with(b"end"));
        assert!(captured.len() <= CAPTURE_LIMIT);
        assert!(
            captured
                .windows(b"bytes omitted".len())
                .any(|part| part == b"bytes omitted")
        );
    }
}
