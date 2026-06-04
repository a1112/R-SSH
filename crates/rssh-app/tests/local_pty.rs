use std::{
    io::{Read, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use rssh_core::TerminalSize;
use rssh_pty::{PtyCommand, PtySession, PtySize};
use rssh_terminal::Terminal;

#[test]
#[ignore = "spawns a real platform shell"]
fn local_pty_output_feeds_terminal_grid() {
    let marker = "rssh-terminal-grid-smoke";
    let command = PtyCommand::default_shell();
    let mut session = PtySession::spawn(&command, PtySize::try_new(160, 30).unwrap()).unwrap();
    let mut reader = session.take_reader().unwrap();
    let mut writer = session.take_writer().unwrap();
    let (sender, receiver) = mpsc::channel();

    let reader_thread = thread::spawn(move || {
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
    writer
        .write_all(format!("echo {marker}\r\n").as_bytes())
        .unwrap();
    writer.flush().unwrap();

    let mut terminal = Terminal::new(TerminalSize::new(160, 30));
    let mut cursor_query_probe = Vec::new();
    let mut saw_marker = false;
    let started = Instant::now();
    let timeout = Duration::from_secs(5);

    while started.elapsed() < timeout {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(chunk)) => {
                answer_cursor_position_queries(&chunk, &mut cursor_query_probe, &mut writer);
                terminal.feed(&chunk);
                if terminal_text(&terminal).contains(marker) {
                    saw_marker = true;
                    break;
                }
            }
            Ok(Err(error)) => panic!("failed to read PTY output: {error}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = writer.write_all(b"exit\r\n");
    let _ = writer.flush();
    drop(writer);
    wait_or_kill(&mut session, Duration::from_secs(2));
    drop(session);
    reader_thread.join().unwrap();

    assert!(
        saw_marker,
        "terminal grid did not receive marker; grid: {:?}",
        terminal_text(&terminal)
    );
}

fn answer_cursor_position_queries(chunk: &[u8], probe: &mut Vec<u8>, writer: &mut dyn Write) {
    const QUERY: &[u8] = b"\x1b[6n";
    const RESPONSE: &[u8] = b"\x1b[1;1R";

    probe.extend_from_slice(chunk);
    while let Some(index) = find_subslice(probe, QUERY) {
        writer.write_all(RESPONSE).unwrap();
        writer.flush().unwrap();
        probe.drain(..index + QUERY.len());
    }

    let retained = QUERY.len().saturating_sub(1);
    let removable = probe.len().saturating_sub(retained);
    if removable > 0 {
        probe.drain(..removable);
    }
}

fn wait_or_kill(session: &mut PtySession, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if session.try_wait().unwrap().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = session.kill();
}

fn terminal_text(terminal: &Terminal) -> String {
    let size = terminal.grid().size();
    let mut text = String::new();

    for row in 0..size.rows {
        for column in 0..size.columns {
            text.push(terminal.grid().get(row, column).unwrap().ch);
        }
        text.push('\n');
    }

    text
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
