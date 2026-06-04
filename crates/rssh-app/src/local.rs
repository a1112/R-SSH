use std::{
    error::Error,
    io::{self, Read, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal,
};
use rssh_pty::{PtySession, PtySize};

use crate::{
    cli::LocalOptions,
    terminal_input::{TerminalKey, encode_terminal_key},
};

pub fn run(options: &LocalOptions) -> Result<(), Box<dyn Error>> {
    let size = resolve_local_size(options.size);
    let mut session = PtySession::spawn(&options.command, size)?;
    let mut reader = session.take_reader()?;
    let mut writer = session.take_writer()?;
    let (reader_done_sender, reader_done_receiver) = mpsc::channel();
    let (terminal_response_sender, terminal_response_receiver) = mpsc::channel();

    let reader_thread = thread::spawn(move || {
        let result = copy_pty_output(&mut reader, &terminal_response_sender);
        let _ = reader_done_sender.send(result);
    });

    let _raw_mode = RawMode::enable()?;
    let run_result = run_input_loop(
        &mut session,
        &mut writer,
        &reader_done_receiver,
        &terminal_response_receiver,
    );

    drop(writer);
    let _ = session.wait();
    drop(session);
    let _ = reader_thread.join();

    run_result
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

struct RawMode;

impl RawMode {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn copy_pty_output(
    reader: &mut dyn Read,
    terminal_response_sender: &mpsc::Sender<Vec<u8>>,
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
                    terminal_response_sender
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

fn run_input_loop(
    session: &mut PtySession,
    writer: &mut dyn Write,
    reader_done_receiver: &mpsc::Receiver<io::Result<()>>,
    terminal_response_receiver: &mpsc::Receiver<Vec<u8>>,
) -> Result<(), Box<dyn Error>> {
    loop {
        while let Ok(response) = terminal_response_receiver.try_recv() {
            writer.write_all(&response)?;
            writer.flush()?;
        }

        if let Ok(reader_result) = reader_done_receiver.try_recv() {
            reader_result?;
            return Ok(());
        }

        if session.try_wait()? {
            return Ok(());
        }

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if let Some(bytes) = encode_key(key) {
                    writer.write_all(&bytes)?;
                    writer.flush()?;
                }
            }
            Event::Resize(columns, rows) => {
                if let Ok(size) = PtySize::try_new(columns, rows) {
                    session.resize(size)?;
                }
            }
            _ => {}
        }
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

fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
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

    encode_terminal_key(terminal_key)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{TerminalOutputFilter, encode_key, resolve_local_size};

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
