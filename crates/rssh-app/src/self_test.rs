use std::{
    error::Error,
    io::{Read, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use rssh_pty::{PtySession, PtySize};
use serde::Serialize;

use crate::cli::SelfTestOptions;

const SELF_TEST_MARKER: &str = "rssh-self-test";
const SELF_TEST_COLUMNS: u16 = 80;
const SELF_TEST_ROWS: u16 = 24;
const SELF_TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SelfTestReport {
    pub ok: bool,
    pub elapsed_ms: u128,
    pub checks: Vec<SelfTestCheck>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SelfTestCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub output_bytes: usize,
}

pub fn print_self_test(options: &SelfTestOptions) -> Result<(), Box<dyn Error>> {
    let report = run_self_test();

    if options.json {
        println!("{}", self_test_json(&report)?);
    } else {
        for line in self_test_text_lines(&report) {
            println!("{line}");
        }
    }

    if report.ok {
        Ok(())
    } else {
        Err("self-test failed".into())
    }
}

fn run_self_test() -> SelfTestReport {
    let started = Instant::now();
    let size = PtySize::try_new(SELF_TEST_COLUMNS, SELF_TEST_ROWS)
        .expect("self-test PTY dimensions are non-zero");
    let output = capture_local_pty_self_test_output(size, SELF_TEST_TIMEOUT);
    let elapsed_ms = started.elapsed().as_millis();

    match output {
        Ok(output) => local_pty_self_test_report_from_output(&output, elapsed_ms),
        Err(error) => SelfTestReport {
            ok: false,
            elapsed_ms,
            checks: vec![SelfTestCheck {
                name: "local-pty".to_owned(),
                ok: false,
                detail: error.to_string(),
                output_bytes: 0,
            }],
        },
    }
}

fn self_test_shell_input() -> String {
    format!("echo {SELF_TEST_MARKER}\r\nexit\r\n")
}

fn capture_local_pty_self_test_output(
    size: PtySize,
    timeout: Duration,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut session = PtySession::spawn(&rssh_pty::PtyCommand::default_shell(), size)?;
    let mut reader = session.take_reader()?;
    let mut writer = session.take_writer()?;
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let mut buffer = [0; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    let _ = sender.send(Ok(Vec::new()));
                    return;
                }
                Ok(count) => {
                    if sender.send(Ok(buffer[..count].to_vec())).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            }
        }
    });

    thread::sleep(Duration::from_millis(300));
    writer.write_all(self_test_shell_input().as_bytes())?;
    writer.flush()?;

    let started = Instant::now();
    let mut output = Vec::new();
    while started.elapsed() < timeout {
        if let Some(status) = session.try_wait()? {
            if !status.success() {
                return Err(
                    format!("self-test shell exited with code {}", status.exit_code()).into(),
                );
            }
            break;
        }

        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(chunk)) => {
                if chunk.is_empty() {
                    break;
                }
                output.extend_from_slice(&chunk);
                if contains_cursor_position_query(&output) {
                    writer.write_all(b"\x1b[1;1R")?;
                    writer.flush()?;
                }
                if String::from_utf8_lossy(&output).contains(SELF_TEST_MARKER) {
                    break;
                }
            }
            Ok(Err(error)) => return Err(Box::new(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if !String::from_utf8_lossy(&output).contains(SELF_TEST_MARKER) {
        let _ = session.kill();
        return Err("PTY marker missing".into());
    }

    let _ = writer.write_all(b"exit\r\n");
    let _ = writer.flush();
    drop(writer);

    Ok(output)
}

fn contains_cursor_position_query(bytes: &[u8]) -> bool {
    bytes
        .windows(b"\x1b[6n".len())
        .any(|window| window == b"\x1b[6n")
}

pub fn self_test_json(report: &SelfTestReport) -> Result<String, Box<dyn Error>> {
    Ok(serde_json::to_string(report)?)
}

pub fn self_test_text_lines(report: &SelfTestReport) -> Vec<String> {
    report
        .checks
        .iter()
        .map(|check| {
            if check.ok {
                format!("ok\t{}\t{}", check.name, check.detail)
            } else {
                format!("missing\t{}", check.name)
            }
        })
        .collect()
}

pub fn local_pty_self_test_report_from_output(output: &[u8], elapsed_ms: u128) -> SelfTestReport {
    let text = String::from_utf8_lossy(output);
    let ok = text.contains(SELF_TEST_MARKER);
    let detail = if ok {
        "captured PTY marker".to_owned()
    } else {
        "PTY marker missing".to_owned()
    };

    SelfTestReport {
        ok,
        elapsed_ms,
        checks: vec![SelfTestCheck {
            name: "local-pty".to_owned(),
            ok,
            detail,
            output_bytes: output.len(),
        }],
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn self_test_json_reports_local_pty_marker_capture() {
        let report =
            super::local_pty_self_test_report_from_output(b"prompt\r\nrssh-self-test\r\n", 12);
        let json = super::self_test_json(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["ok"], true);
        assert_eq!(value["checks"][0]["name"], "local-pty");
        assert_eq!(value["checks"][0]["ok"], true);
        assert_eq!(value["checks"][0]["output_bytes"], 24);
        assert_eq!(value["elapsed_ms"], 12);
    }

    #[test]
    fn self_test_text_reports_failed_local_pty_marker_capture() {
        let report = super::local_pty_self_test_report_from_output(b"prompt\r\n", 9);
        let lines = super::self_test_text_lines(&report);

        assert!(!report.ok);
        assert!(lines.iter().any(|line| line == "missing\tlocal-pty"));
    }

    #[test]
    fn self_test_shell_input_echoes_marker_then_exits() {
        let input = super::self_test_shell_input();

        assert!(input.contains("echo rssh-self-test"));
        assert!(input.contains("exit"));
    }

    #[test]
    fn detects_cursor_position_query_in_pty_output() {
        assert!(super::contains_cursor_position_query(b"before\x1b[6nafter"));
        assert!(!super::contains_cursor_position_query(b"before-after"));
    }
}
