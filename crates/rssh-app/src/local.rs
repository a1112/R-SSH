use std::{
    error::Error,
    io::{self, IsTerminal, Read, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute, terminal,
};
use rssh_pty::{PtyExitStatus, PtySession, PtySize};

use crate::{
    cli::LocalOptions,
    terminal_input::{TerminalKey, encode_terminal_key},
};

pub fn run(options: &LocalOptions) -> Result<PtyExitStatus, Box<dyn Error>> {
    let size = resolve_local_size(options.size);
    let mut session = PtySession::spawn(&options.command, size)?;
    let mut reader = session.take_reader()?;
    let mut writer = session.take_writer()?;
    let (reader_done_sender, reader_done_receiver) = mpsc::channel();
    let (writer_done_sender, writer_done_receiver) = mpsc::channel();
    let (pty_input_sender, pty_input_receiver) = mpsc::channel();
    let (control_sender, control_receiver) = mpsc::channel();
    let terminal_response_sender = pty_input_sender.clone();

    let _reader_thread = thread::spawn(move || {
        let result = copy_pty_output(&mut reader, &terminal_response_sender);
        let _ = reader_done_sender.send(result);
    });
    let _writer_thread = thread::spawn(move || {
        let result = copy_pty_input(&mut writer, &pty_input_receiver);
        let _ = writer_done_sender.send(result);
    });

    let _raw_mode = RawMode::enable(options.mouse)?;
    let _input_thread = spawn_input_thread(pty_input_sender.clone(), control_sender, options.mouse);
    let run_result = run_input_loop(
        &mut session,
        &reader_done_receiver,
        &writer_done_receiver,
        &control_receiver,
    );

    drop(pty_input_sender);
    drop(session);

    run_result
}

enum LocalControlEvent {
    Resize(PtySize),
}

fn spawn_input_thread(
    pty_input_sender: mpsc::Sender<Vec<u8>>,
    control_sender: mpsc::Sender<LocalControlEvent>,
    mouse_reporting: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match event::read() {
                Ok(
                    event @ (Event::Key(_)
                    | Event::Paste(_)
                    | Event::Mouse(_)
                    | Event::FocusGained
                    | Event::FocusLost),
                ) => {
                    let Some(bytes) = encode_input_event(event, mouse_reporting) else {
                        continue;
                    };
                    if pty_input_sender.send(bytes).is_err() {
                        return;
                    }
                }
                Ok(Event::Resize(columns, rows)) => {
                    let Ok(size) = PtySize::try_new(columns, rows) else {
                        continue;
                    };
                    if control_sender
                        .send(LocalControlEvent::Resize(size))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    })
}

fn resolve_local_size(explicit: Option<PtySize>) -> PtySize {
    if let Some(size) = explicit {
        return size;
    }

    terminal::size()
        .ok()
        .and_then(|(columns, rows)| PtySize::try_new(columns, rows).ok())
        .unwrap_or_else(fallback_pty_size)
}

fn fallback_pty_size() -> PtySize {
    PtySize::try_new(80, 24).expect("fallback PTY size is valid")
}

struct RawMode {
    bracketed_paste: bool,
    mouse_capture: bool,
    focus_change: bool,
}

impl RawMode {
    fn enable(mouse_capture_requested: bool) -> io::Result<Self> {
        terminal::enable_raw_mode()?;

        let bracketed_paste = if io::stdout().is_terminal() {
            let mut stdout = io::stdout();
            execute!(stdout, EnableBracketedPaste).is_ok()
        } else {
            false
        };

        let (mouse_capture, focus_change) = if mouse_capture_requested && io::stdout().is_terminal()
        {
            let mut stdout = io::stdout();
            (
                execute!(stdout, EnableMouseCapture).is_ok(),
                execute!(stdout, EnableFocusChange).is_ok(),
            )
        } else {
            (false, false)
        };

        Ok(Self {
            bracketed_paste,
            mouse_capture,
            focus_change,
        })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if self.focus_change {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, DisableFocusChange);
        }
        if self.mouse_capture {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, DisableMouseCapture);
        }
        if self.bracketed_paste {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, DisableBracketedPaste);
        }
        let _ = terminal::disable_raw_mode();
    }
}

fn copy_pty_output(
    reader: &mut dyn Read,
    pty_input_sender: &mpsc::Sender<Vec<u8>>,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut buffer = [0; 8192];
    let mut output_filter = TerminalOutputFilter::default();

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                output_filter.flush(&mut stdout)?;
                stdout.flush()?;
                return Ok(());
            }
            Ok(count) => {
                output_filter.write(&buffer[..count], &mut stdout, |response| {
                    pty_input_sender
                        .send(response.to_vec())
                        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "PTY input closed"))
                })?;
                stdout.flush()?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

fn copy_pty_input(
    writer: &mut dyn Write,
    pty_input_receiver: &mpsc::Receiver<Vec<u8>>,
) -> io::Result<()> {
    for bytes in pty_input_receiver {
        writer.write_all(&bytes)?;
        writer.flush()?;
    }

    Ok(())
}

fn run_input_loop(
    session: &mut PtySession,
    reader_done_receiver: &mpsc::Receiver<io::Result<()>>,
    writer_done_receiver: &mpsc::Receiver<io::Result<()>>,
    control_receiver: &mpsc::Receiver<LocalControlEvent>,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    loop {
        if let Ok(reader_result) = reader_done_receiver.try_recv() {
            reader_result?;
            return Ok(session.wait()?);
        }

        if let Ok(writer_result) = writer_done_receiver.try_recv() {
            writer_result?;
        }

        while let Ok(control_event) = control_receiver.try_recv() {
            match control_event {
                LocalControlEvent::Resize(size) => session.resize(size)?,
            }
        }

        if let Some(status) = session.try_wait()? {
            return Ok(status);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Default)]
struct TerminalOutputFilter {
    pending: Vec<u8>,
}

impl TerminalOutputFilter {
    const CURSOR_POSITION_QUERY: &'static [u8] = b"\x1b[6n";
    const CURSOR_POSITION_RESPONSE: &'static [u8] = b"\x1b[1;1R";

    fn write(
        &mut self,
        bytes: &[u8],
        output: &mut dyn Write,
        mut respond: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        self.pending.extend_from_slice(bytes);

        while let Some(index) = find_subslice(&self.pending, Self::CURSOR_POSITION_QUERY) {
            output.write_all(&self.pending[..index])?;
            respond(Self::CURSOR_POSITION_RESPONSE)?;
            self.pending
                .drain(..index + Self::CURSOR_POSITION_QUERY.len());
        }

        let retained = Self::CURSOR_POSITION_QUERY.len().saturating_sub(1);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            output.write_all(&self.pending[..writable])?;
            self.pending.drain(..writable);
        }

        Ok(())
    }

    fn flush(&mut self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(&self.pending)?;
        self.pending.clear();
        Ok(())
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn encode_input_event(event: Event, mouse_reporting: bool) -> Option<Vec<u8>> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => encode_key(key),
        Event::Paste(text) => Some(text.into_bytes()),
        Event::Mouse(event) if mouse_reporting => encode_mouse_event(event),
        Event::FocusGained if mouse_reporting => Some(b"\x1b[I".to_vec()),
        Event::FocusLost if mouse_reporting => Some(b"\x1b[O".to_vec()),
        _ => None,
    }
}

fn encode_mouse_event(event: MouseEvent) -> Option<Vec<u8>> {
    let mut code = match event.kind {
        MouseEventKind::Down(button) | MouseEventKind::Up(button) => mouse_button_code(button),
        MouseEventKind::Drag(button) => mouse_button_code(button) + 32,
        MouseEventKind::Moved => 35,
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        MouseEventKind::ScrollLeft => 66,
        MouseEventKind::ScrollRight => 67,
    };

    if event.modifiers.contains(KeyModifiers::SHIFT) {
        code += 4;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        code += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        code += 16;
    }

    let final_byte = if matches!(event.kind, MouseEventKind::Up(_)) {
        b'm'
    } else {
        b'M'
    };
    let column = event.column.checked_add(1)?;
    let row = event.row.checked_add(1)?;

    Some(format!("\x1b[<{code};{column};{row}{}", final_byte as char).into_bytes())
}

const fn mouse_button_code(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let terminal_key = match key.code {
        KeyCode::Char(character) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            TerminalKey::Control(character)
        }
        KeyCode::Char(character) => TerminalKey::Text(character),
        KeyCode::Enter => TerminalKey::Enter,
        KeyCode::Backspace => TerminalKey::Backspace,
        KeyCode::Tab => TerminalKey::Tab,
        KeyCode::Esc => TerminalKey::Escape,
        KeyCode::Left => TerminalKey::Left,
        KeyCode::Right => TerminalKey::Right,
        KeyCode::Up => TerminalKey::Up,
        KeyCode::Down => TerminalKey::Down,
        KeyCode::Home => TerminalKey::Home,
        KeyCode::End => TerminalKey::End,
        KeyCode::Delete => TerminalKey::Delete,
        KeyCode::Insert => TerminalKey::Insert,
        KeyCode::PageUp => TerminalKey::PageUp,
        KeyCode::PageDown => TerminalKey::PageDown,
        KeyCode::BackTab => TerminalKey::BackTab,
        KeyCode::F(key) => TerminalKey::Function(key),
        _ => return None,
    };

    let mut bytes = encode_terminal_key(terminal_key)?;
    if alt && matches!(key.code, KeyCode::Char(_)) {
        bytes.insert(0, 0x1b);
    }

    Some(bytes)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    use super::{TerminalOutputFilter, encode_input_event, encode_key, resolve_local_size};

    #[test]
    fn encodes_text_input_as_utf8() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Char('中'), KeyModifiers::NONE)).unwrap(),
            "中".as_bytes()
        );
    }

    #[test]
    fn encodes_enter_for_shells() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap(),
            b"\r"
        );
    }

    #[test]
    fn encodes_control_c() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap(),
            vec![3]
        );
    }

    #[test]
    fn encodes_arrow_keys_as_escape_sequences() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)).unwrap(),
            b"\x1b[A"
        );
    }

    #[test]
    fn encodes_backtab_and_function_keys_as_escape_sequences() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)).unwrap(),
            b"\x1b[Z"
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE)).unwrap(),
            b"\x1b[24~"
        );
    }

    #[test]
    fn encodes_alt_text_with_escape_prefix() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT)).unwrap(),
            b"\x1bx"
        );
    }

    #[test]
    fn encodes_paste_event_as_utf8_bytes() {
        assert_eq!(
            encode_input_event(Event::Paste("line 1\n中".to_owned()), false).unwrap(),
            "line 1\n中".as_bytes()
        );
    }

    #[test]
    fn ignores_mouse_events_unless_enabled() {
        assert!(encode_input_event(left_mouse_down(), false).is_none());
    }

    #[test]
    fn encodes_mouse_events_as_sgr_sequences_when_enabled() {
        assert_eq!(
            encode_input_event(left_mouse_down(), true).unwrap(),
            b"\x1b[<0;1;2M"
        );
        assert_eq!(
            encode_input_event(left_mouse_release(), true).unwrap(),
            b"\x1b[<0;1;2m"
        );
        assert_eq!(
            encode_input_event(
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 4,
                    row: 5,
                    modifiers: KeyModifiers::CONTROL,
                }),
                true
            )
            .unwrap(),
            b"\x1b[<81;5;6M"
        );
    }

    #[test]
    fn encodes_focus_events_when_mouse_reporting_is_enabled() {
        assert_eq!(
            encode_input_event(Event::FocusGained, true).unwrap(),
            b"\x1b[I"
        );
        assert_eq!(
            encode_input_event(Event::FocusLost, true).unwrap(),
            b"\x1b[O"
        );
    }

    fn left_mouse_down() -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 1,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn left_mouse_release() -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 1,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn explicit_local_size_overrides_console_size() {
        let size = rssh_pty::PtySize::try_new(101, 31).unwrap();

        let resolved = resolve_local_size(Some(size));

        assert_eq!(resolved.columns(), 101);
        assert_eq!(resolved.rows(), 31);
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

    #[test]
    fn terminal_output_filter_answers_cursor_position_query() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[6nafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[1;1R");
    }

    #[test]
    fn terminal_output_filter_handles_split_cursor_position_query() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"6nafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[1;1R");
    }
}
