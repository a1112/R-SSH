use super::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, PartialEq, Eq)]
struct LocalHostTranscript {
    console: Vec<u8>,
    responses: Vec<Vec<u8>>,
    clipboard_writes: Vec<String>,
    mode_changes: Vec<TerminalModeChange>,
}

fn run_legacy_transcript(chunks: &[&[u8]], finish: bool) -> LocalHostTranscript {
    let size = PtySize::try_new(40, 5).unwrap();
    let mut runtime = TerminalOutputFilter::new(size);
    runtime.set_terminal_name("rssh-test-term");
    let mut console = Vec::new();
    let mut responses = Vec::new();
    let mut clipboard_writes = Vec::new();
    let mut mode_changes = Vec::new();

    for chunk in chunks {
        runtime
            .write_with_clipboard(
                chunk,
                &mut console,
                |response| {
                    responses.push(response.to_vec());
                    Ok(())
                },
                |contents| {
                    clipboard_writes.push(contents.to_owned());
                    true
                },
                || Some("paste".to_owned()),
                Osc52Policy::ReadWrite,
            )
            .unwrap();
        mode_changes.extend(runtime.take_mode_changes());
    }
    if finish {
        runtime.flush(&mut console).unwrap();
    }

    LocalHostTranscript {
        console,
        responses,
        clipboard_writes,
        mode_changes,
    }
}

fn run_v2_transcript(chunks: &[&[u8]], finish: bool) -> LocalHostTranscript {
    let size = PtySize::try_new(40, 5).unwrap();
    let mut runtime = LocalTerminalRuntime::new(
        SharedTerminalSize::new(size),
        "rssh-test-term".to_owned(),
        Osc52Policy::ReadWrite,
    );
    let mut console = Vec::new();
    let mut responses = Vec::new();
    let mut clipboard_writes = Vec::new();
    let mut mode_changes = Vec::new();

    for chunk in chunks {
        runtime
            .write_with_clipboard(
                chunk,
                &mut console,
                |response| {
                    responses.push(response.to_vec());
                    Ok(())
                },
                |contents| {
                    clipboard_writes.push(contents.to_owned());
                    true
                },
                || Some("paste".to_owned()),
                |change| mode_changes.push(change),
            )
            .unwrap();
    }
    if finish {
        runtime
            .finish(
                &mut console,
                |response| {
                    responses.push(response.to_vec());
                    Ok(())
                },
                |contents| {
                    clipboard_writes.push(contents.to_owned());
                    true
                },
                || Some("paste".to_owned()),
                |change| mode_changes.push(change),
            )
            .unwrap();
    }

    LocalHostTranscript {
        console,
        responses,
        clipboard_writes,
        mode_changes,
    }
}

#[test]
fn local_runtime_matches_legacy_host_transcript_across_chunks() {
    let chunks: &[&[u8]] = &[
        b"A\x1b[31mred\x1b[0m\x07\x1b[6",
        b"nB\x1b]8;;https://example.test\x07link\x1b]8;;\x07",
        b"\x1b]52;c;Y29weQ==\x07\x1b]52;c;?\x07\x1b[?2026hheld\x1b[6n",
        b"\x1b[?2026l\x1b[?2004hC\x1b]777;notify;body\x07\x1b[?1;2c\x05",
        b"\x1b[18t\x1bP+q544e\x1b\\\x1b[?2004lD",
    ];

    let legacy = run_legacy_transcript(chunks, true);
    let v2 = run_v2_transcript(chunks, true);

    assert_eq!(v2, legacy);
    assert_eq!(
        v2.console,
        b"A\x1b[31mred\x1b[0m\x07Blinkheld\x1b[?2004hC\x05\x1b[?2004lD"
    );
    assert_eq!(v2.clipboard_writes, ["copy"]);
    assert_eq!(
        v2.mode_changes,
        [
            TerminalModeChange::SynchronizedOutput(true),
            TerminalModeChange::SynchronizedOutput(false),
            TerminalModeChange::BracketedPaste(true),
            TerminalModeChange::BracketedPaste(false),
        ]
    );
    assert!(
        v2.responses
            .iter()
            .any(|response| response == b"\x1b]52;c;cGFzdGU=\x07")
    );
    assert!(
        v2.responses
            .iter()
            .any(|response| response == b"\x1b[8;5;40t")
    );
}

#[test]
fn local_runtime_finish_matches_legacy_sync_release_and_incomplete_discard() {
    let chunks: &[&[u8]] = &[b"before\x1b[?2026hheld\x1b[6"];

    let legacy = run_legacy_transcript(chunks, true);
    let v2 = run_v2_transcript(chunks, true);

    assert_eq!(v2, legacy);
    assert_eq!(v2.console, b"beforeheld");
    assert!(v2.responses.is_empty());
}

#[derive(Debug, PartialEq, Eq)]
struct LocalErrorTranscript {
    error: Option<io::ErrorKind>,
    console: Vec<u8>,
    responses: Vec<Vec<u8>>,
    clipboard_writes: Vec<String>,
    mode_changes: Vec<TerminalModeChange>,
}

struct FailOnFragmentWriter<'a> {
    bytes: Vec<u8>,
    fragment: Option<&'a [u8]>,
}

impl Write for FailOnFragmentWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .fragment
            .is_some_and(|fragment| bytes.windows(fragment.len()).any(|part| part == fragment))
        {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected console failure",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

const LOCAL_ERROR_SCRIPT: &[u8] =
    b"A\x1b[?2004h\x1b[6nB\x1b]52;c;Y29weQ==\x07C\x1b]52;c;?\x07D\x1b[?2004lE";

fn run_legacy_error_transcript(
    console_failure: Option<&[u8]>,
    response_failure: Option<usize>,
    clipboard_accepts_write: bool,
) -> LocalErrorTranscript {
    let mut runtime = TerminalOutputFilter::default();
    let mut console = FailOnFragmentWriter {
        bytes: Vec::new(),
        fragment: console_failure,
    };
    let mut responses = Vec::new();
    let mut response_index = 0;
    let mut clipboard_writes = Vec::new();
    let mut mode_changes = Vec::new();
    let result = runtime.write_with_clipboard(
        LOCAL_ERROR_SCRIPT,
        &mut console,
        |response| {
            let current = response_index;
            response_index += 1;
            if response_failure == Some(current) {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "injected response failure",
                ));
            }
            responses.push(response.to_vec());
            Ok(())
        },
        |contents| {
            clipboard_writes.push(contents.to_owned());
            clipboard_accepts_write
        },
        || Some("paste".to_owned()),
        Osc52Policy::ReadWrite,
    );
    if result.is_ok() {
        mode_changes.extend(runtime.take_mode_changes());
    }

    LocalErrorTranscript {
        error: result.err().map(|error| error.kind()),
        console: console.bytes,
        responses,
        clipboard_writes,
        mode_changes,
    }
}

fn run_v2_error_transcript(
    console_failure: Option<&[u8]>,
    response_failure: Option<usize>,
    clipboard_accepts_write: bool,
) -> LocalErrorTranscript {
    let mut runtime = LocalTerminalRuntime::new(
        SharedTerminalSize::default(),
        DEFAULT_TERMINAL_NAME.to_owned(),
        Osc52Policy::ReadWrite,
    );
    let mut console = FailOnFragmentWriter {
        bytes: Vec::new(),
        fragment: console_failure,
    };
    let mut responses = Vec::new();
    let mut response_index = 0;
    let mut clipboard_writes = Vec::new();
    let mut mode_changes = Vec::new();
    let result = runtime.write_with_clipboard(
        LOCAL_ERROR_SCRIPT,
        &mut console,
        |response| {
            let current = response_index;
            response_index += 1;
            if response_failure == Some(current) {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "injected response failure",
                ));
            }
            responses.push(response.to_vec());
            Ok(())
        },
        |contents| {
            clipboard_writes.push(contents.to_owned());
            clipboard_accepts_write
        },
        || Some("paste".to_owned()),
        |change| mode_changes.push(change),
    );

    LocalErrorTranscript {
        error: result.err().map(|error| error.kind()),
        console: console.bytes,
        responses,
        clipboard_writes,
        mode_changes,
    }
}

#[test]
fn local_runtime_matches_legacy_effect_prefix_when_console_write_fails() {
    let legacy = run_legacy_error_transcript(Some(b"C"), None, true);
    let v2 = run_v2_error_transcript(Some(b"C"), None, true);

    assert_eq!(v2, legacy);
    assert_eq!(v2.error, Some(io::ErrorKind::BrokenPipe));
    assert_eq!(v2.console, b"A\x1b[?2004hB");
    assert_eq!(v2.responses.len(), 1);
    assert_eq!(v2.clipboard_writes, ["copy"]);
    assert!(v2.mode_changes.is_empty());
}

#[test]
fn local_runtime_matches_legacy_effect_prefix_for_each_response_failure() {
    for failure_index in 0..2 {
        let legacy = run_legacy_error_transcript(None, Some(failure_index), true);
        let v2 = run_v2_error_transcript(None, Some(failure_index), true);

        assert_eq!(v2, legacy, "response failure {failure_index}");
        assert_eq!(v2.error, Some(io::ErrorKind::BrokenPipe));
        assert!(v2.mode_changes.is_empty());
    }
}

#[test]
fn local_runtime_matches_legacy_when_clipboard_or_mode_sink_rejects_work() {
    let legacy = run_legacy_error_transcript(None, None, false);
    let v2 = run_v2_error_transcript(None, None, false);

    assert_eq!(v2, legacy);
    assert_eq!(v2.error, None);
    assert_eq!(v2.console, b"A\x1b[?2004hBCD\x1b[?2004lE");
    assert_eq!(v2.clipboard_writes, ["copy"]);
    assert_eq!(
        v2.mode_changes,
        [
            TerminalModeChange::BracketedPaste(true),
            TerminalModeChange::BracketedPaste(false),
        ]
    );
}

#[test]
fn local_runtime_rejects_feed_and_finish_after_a_host_io_error() {
    let mut runtime = LocalTerminalRuntime::new(
        SharedTerminalSize::default(),
        DEFAULT_TERMINAL_NAME.to_owned(),
        Osc52Policy::Off,
    );
    let mut first_console = Vec::new();
    let first = runtime.write_with_clipboard(
        b"A\x1b[6nB\x1b[?2004hC",
        &mut first_console,
        |_| {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected response failure",
            ))
        },
        |_| true,
        || None,
        |_| panic!("mode changes must not publish after an I/O error"),
    );
    assert_eq!(first.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(first_console, b"A");

    let mut retry_console = Vec::new();
    let retry = runtime.write_with_clipboard(
        b"retry",
        &mut retry_console,
        |_| panic!("poisoned runtime must not respond"),
        |_| panic!("poisoned runtime must not write clipboard"),
        || panic!("poisoned runtime must not read clipboard"),
        |_| panic!("poisoned runtime must not publish modes"),
    );
    assert_eq!(retry.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert!(retry_console.is_empty());

    let finish = runtime.finish(
        &mut retry_console,
        |_| panic!("poisoned runtime must not respond while finishing"),
        |_| panic!("poisoned runtime must not write clipboard while finishing"),
        || panic!("poisoned runtime must not read clipboard while finishing"),
        |_| panic!("poisoned runtime must not publish modes while finishing"),
    );
    assert_eq!(finish.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert!(retry_console.is_empty());
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalHostEvent {
    Console(Vec<u8>),
    Transport(Vec<u8>),
    ClipboardWrite(String),
    ClipboardRead,
    Mode(TerminalModeChange),
    Flush,
}

struct EventWriter(Rc<RefCell<Vec<LocalHostEvent>>>);

impl Write for EventWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .borrow_mut()
            .push(LocalHostEvent::Console(bytes.to_vec()));
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.borrow_mut().push(LocalHostEvent::Flush);
        Ok(())
    }
}

const ORDERED_LOCAL_SCRIPT: &[u8] = b"A\x1b[6nB\x1b]52;c;aGVsbG8=\x07C\x1b]52;c;?\x07D\x1b[?1hE";

fn legacy_ordered_host_events() -> Vec<LocalHostEvent> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut output = EventWriter(Rc::clone(&events));
    let mut runtime = TerminalOutputFilter::default();
    runtime
        .write_with_clipboard(
            ORDERED_LOCAL_SCRIPT,
            &mut output,
            {
                let events = Rc::clone(&events);
                move |response| {
                    events
                        .borrow_mut()
                        .push(LocalHostEvent::Transport(response.to_vec()));
                    Ok(())
                }
            },
            {
                let events = Rc::clone(&events);
                move |contents| {
                    events
                        .borrow_mut()
                        .push(LocalHostEvent::ClipboardWrite(contents.to_owned()));
                    false
                }
            },
            {
                let events = Rc::clone(&events);
                move || {
                    events.borrow_mut().push(LocalHostEvent::ClipboardRead);
                    Some("clip".to_owned())
                }
            },
            Osc52Policy::ReadWrite,
        )
        .unwrap();
    for change in runtime.take_mode_changes() {
        events.borrow_mut().push(LocalHostEvent::Mode(change));
    }
    output.flush().unwrap();
    drop(output);
    Rc::try_unwrap(events).unwrap().into_inner()
}

fn v2_ordered_host_events() -> Vec<LocalHostEvent> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut output = EventWriter(Rc::clone(&events));
    let mut runtime = LocalTerminalRuntime::new(
        SharedTerminalSize::default(),
        DEFAULT_TERMINAL_NAME.to_owned(),
        Osc52Policy::ReadWrite,
    );
    runtime
        .write_with_clipboard(
            ORDERED_LOCAL_SCRIPT,
            &mut output,
            {
                let events = Rc::clone(&events);
                move |response| {
                    events
                        .borrow_mut()
                        .push(LocalHostEvent::Transport(response.to_vec()));
                    Ok(())
                }
            },
            {
                let events = Rc::clone(&events);
                move |contents| {
                    events
                        .borrow_mut()
                        .push(LocalHostEvent::ClipboardWrite(contents.to_owned()));
                    false
                }
            },
            {
                let events = Rc::clone(&events);
                move || {
                    events.borrow_mut().push(LocalHostEvent::ClipboardRead);
                    Some("clip".to_owned())
                }
            },
            {
                let events = Rc::clone(&events);
                move |change| events.borrow_mut().push(LocalHostEvent::Mode(change))
            },
        )
        .unwrap();
    output.flush().unwrap();
    drop(output);
    Rc::try_unwrap(events).unwrap().into_inner()
}

#[test]
fn local_runtime_matches_legacy_cross_effect_order_and_outer_flush() {
    let legacy = legacy_ordered_host_events();
    let v2 = v2_ordered_host_events();

    assert_eq!(v2, legacy);
    assert_eq!(
        v2,
        [
            LocalHostEvent::Console(b"A".to_vec()),
            LocalHostEvent::Transport(b"\x1b[1;2R".to_vec()),
            LocalHostEvent::Console(b"B".to_vec()),
            LocalHostEvent::ClipboardWrite("hello".to_owned()),
            LocalHostEvent::Console(b"C".to_vec()),
            LocalHostEvent::ClipboardRead,
            LocalHostEvent::Transport(b"\x1b]52;c;Y2xpcA==\x07".to_vec()),
            LocalHostEvent::Console(b"D".to_vec()),
            LocalHostEvent::Console(b"\x1b[?1h".to_vec()),
            LocalHostEvent::Console(b"E".to_vec()),
            LocalHostEvent::Mode(TerminalModeChange::ApplicationCursorKeys(true)),
            LocalHostEvent::Flush,
        ]
    );
}

#[test]
fn local_runtime_keeps_console_controls_but_logs_only_visible_text() {
    let mut runtime = LocalTerminalRuntime::new(
        SharedTerminalSize::default(),
        DEFAULT_TERMINAL_NAME.to_owned(),
        Osc52Policy::Off,
    );
    let mut screen = Vec::new();
    let mut log = Vec::new();
    let metrics = LocalMetricsCounters::default();
    let input = b"A\x1b[31mred\x1b[0m\x07\x1b]2;title\x07B\x1b[6nC";
    let mut responses = Vec::new();
    {
        let mut output = SessionLogWriter::new(&mut screen, Some(&mut log), metrics.clone());
        runtime
            .write_with_clipboard(
                input,
                &mut output,
                |response| {
                    responses.push(response.to_vec());
                    Ok(())
                },
                |_| false,
                || None,
                |_| {},
            )
            .unwrap();
        output.flush().unwrap();
    }

    assert_eq!(screen, b"A\x1b[31mred\x1b[0m\x07\x1b]2;title\x07BC");
    assert_eq!(log, b"AredBC");
    assert_eq!(responses, [b"\x1b[1;6R".to_vec()]);
    assert_eq!(
        metrics.snapshot().terminal_output_bytes,
        u64::try_from(screen.len()).unwrap()
    );
}

#[test]
fn terminal_output_filter_passes_plain_output() {
    let mut filter = TerminalOutputFilter::default();
    let mut output = Vec::new();
    let mut responses = Vec::new();

    filter
        .write(b"hello", &mut output, |response| {
            responses.extend_from_slice(response);
            Ok(())
        })
        .unwrap();
    filter.flush(&mut output).unwrap();

    assert_eq!(output, b"hello");
    assert!(responses.is_empty());
}
