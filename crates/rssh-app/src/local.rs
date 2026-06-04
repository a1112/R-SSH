use std::{
    error::Error,
    io::{self, IsTerminal, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
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
    let output_control_sender = control_sender.clone();
    let input_reporting = InputReporting::default();

    let _reader_thread = thread::spawn(move || {
        let result = copy_pty_output(
            &mut reader,
            &terminal_response_sender,
            &output_control_sender,
        );
        let _ = reader_done_sender.send(result);
    });
    let _writer_thread = thread::spawn(move || {
        let result = copy_pty_input(&mut writer, &pty_input_receiver);
        let _ = writer_done_sender.send(result);
    });

    let mut raw_mode = RawMode::enable()?;
    let _input_thread = spawn_input_thread(
        pty_input_sender.clone(),
        control_sender,
        input_reporting.clone(),
    );
    let run_result = run_input_loop(
        &mut session,
        &reader_done_receiver,
        &writer_done_receiver,
        &control_receiver,
        &mut raw_mode,
        &input_reporting,
        options.mouse,
    );

    drop(pty_input_sender);
    drop(session);

    run_result
}

enum LocalControlEvent {
    Resize(PtySize),
    SetBracketedPaste(bool),
    SetMouseReporting(bool),
    SetFocusReporting(bool),
}

#[derive(Clone, Default)]
struct InputReporting {
    bracketed_paste: Arc<AtomicBool>,
    mouse: Arc<AtomicBool>,
    focus: Arc<AtomicBool>,
}

impl InputReporting {
    fn bracketed_paste_enabled(&self) -> bool {
        self.bracketed_paste.load(Ordering::Relaxed)
    }

    fn mouse_enabled(&self) -> bool {
        self.mouse.load(Ordering::Relaxed)
    }

    fn focus_enabled(&self) -> bool {
        self.focus.load(Ordering::Relaxed)
    }

    fn set_mouse(&self, enabled: bool) {
        self.mouse.store(enabled, Ordering::Relaxed);
    }

    fn set_focus(&self, enabled: bool) {
        self.focus.store(enabled, Ordering::Relaxed);
    }

    fn set_bracketed_paste(&self, enabled: bool) {
        self.bracketed_paste.store(enabled, Ordering::Relaxed);
    }
}

fn spawn_input_thread(
    pty_input_sender: mpsc::Sender<Vec<u8>>,
    control_sender: mpsc::Sender<LocalControlEvent>,
    input_reporting: InputReporting,
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
                    let Some(bytes) = encode_input_event(
                        event,
                        input_reporting.mouse_enabled(),
                        input_reporting.focus_enabled(),
                        input_reporting.bracketed_paste_enabled(),
                    ) else {
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
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;

        let bracketed_paste = if io::stdout().is_terminal() {
            let mut stdout = io::stdout();
            execute!(stdout, EnableBracketedPaste).is_ok()
        } else {
            false
        };

        Ok(Self {
            bracketed_paste,
            mouse_capture: false,
            focus_change: false,
        })
    }

    fn set_mouse_capture(&mut self, enabled: bool) -> io::Result<bool> {
        if enabled == self.mouse_capture {
            return Ok(self.mouse_capture);
        }

        if enabled {
            if !io::stdout().is_terminal() {
                return Ok(false);
            }
            let mut stdout = io::stdout();
            execute!(stdout, EnableMouseCapture)?;
            self.mouse_capture = true;
        } else {
            let mut stdout = io::stdout();
            execute!(stdout, DisableMouseCapture)?;
            self.mouse_capture = false;
        }

        Ok(self.mouse_capture)
    }

    fn set_focus_change(&mut self, enabled: bool) -> io::Result<bool> {
        if enabled == self.focus_change {
            return Ok(self.focus_change);
        }

        if enabled {
            if !io::stdout().is_terminal() {
                return Ok(false);
            }
            let mut stdout = io::stdout();
            execute!(stdout, EnableFocusChange)?;
            self.focus_change = true;
        } else {
            let mut stdout = io::stdout();
            execute!(stdout, DisableFocusChange)?;
            self.focus_change = false;
        }

        Ok(self.focus_change)
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
    control_sender: &mpsc::Sender<LocalControlEvent>,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut buffer = [0; 8192];
    let mut output_filter = TerminalOutputFilter::default();
    let mut mode_tracker = TerminalModeTracker::default();

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                output_filter.flush(&mut stdout)?;
                stdout.flush()?;
                return Ok(());
            }
            Ok(count) => {
                mode_tracker.process(&buffer[..count], |change| {
                    let event = match change {
                        TerminalModeChange::BracketedPaste(enabled) => {
                            LocalControlEvent::SetBracketedPaste(enabled)
                        }
                        TerminalModeChange::Mouse(enabled) => {
                            LocalControlEvent::SetMouseReporting(enabled)
                        }
                        TerminalModeChange::Focus(enabled) => {
                            LocalControlEvent::SetFocusReporting(enabled)
                        }
                    };
                    let _ = control_sender.send(event);
                });
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
    raw_mode: &mut RawMode,
    input_reporting: &InputReporting,
    allow_application_reporting: bool,
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
                LocalControlEvent::SetBracketedPaste(enabled) => {
                    input_reporting.set_bracketed_paste(enabled);
                }
                LocalControlEvent::SetMouseReporting(enabled) => {
                    let enabled = if allow_application_reporting {
                        raw_mode.set_mouse_capture(enabled)?
                    } else {
                        false
                    };
                    input_reporting.set_mouse(enabled);
                }
                LocalControlEvent::SetFocusReporting(enabled) => {
                    let enabled = if allow_application_reporting {
                        raw_mode.set_focus_change(enabled)?
                    } else {
                        false
                    };
                    input_reporting.set_focus(enabled);
                }
            }
        }

        if let Some(status) = session.try_wait()? {
            return Ok(status);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalModeChange {
    BracketedPaste(bool),
    Mouse(bool),
    Focus(bool),
}

#[derive(Default)]
struct TerminalModeTracker {
    pending: Vec<u8>,
    mouse_modes: MouseModes,
    bracketed_paste: bool,
    focus: bool,
}

impl TerminalModeTracker {
    const MODE_PREFIX: &'static [u8] = b"\x1b[?";

    fn process(&mut self, bytes: &[u8], mut emit: impl FnMut(TerminalModeChange)) {
        self.pending.extend_from_slice(bytes);

        loop {
            let Some(index) = find_subslice(&self.pending, Self::MODE_PREFIX) else {
                self.retain_possible_prefix();
                return;
            };
            if index > 0 {
                self.pending.drain(..index);
            }

            match Self::parse_mode_sequence(&self.pending) {
                ModeParse::Complete {
                    modes,
                    enabled,
                    consumed,
                } => {
                    for mode in modes {
                        self.apply_mode(mode, enabled, &mut emit);
                    }
                    self.pending.drain(..consumed);
                }
                ModeParse::Incomplete => return,
                ModeParse::Invalid => {
                    self.pending.drain(..1);
                }
            }
        }
    }

    fn parse_mode_sequence(bytes: &[u8]) -> ModeParse {
        let mut cursor = Self::MODE_PREFIX.len();
        let mut modes = Vec::new();

        loop {
            if cursor >= bytes.len() {
                return ModeParse::Incomplete;
            }

            let start = cursor;
            let mut mode = 0u16;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                mode = mode
                    .saturating_mul(10)
                    .saturating_add(u16::from(bytes[cursor] - b'0'));
                cursor += 1;
            }

            if cursor == start {
                return ModeParse::Invalid;
            }
            modes.push(mode);

            if cursor >= bytes.len() {
                return ModeParse::Incomplete;
            }

            match bytes[cursor] {
                b';' => cursor += 1,
                b'h' | b'l' => {
                    return ModeParse::Complete {
                        modes,
                        enabled: bytes[cursor] == b'h',
                        consumed: cursor + 1,
                    };
                }
                _ => return ModeParse::Invalid,
            }
        }
    }

    fn apply_mode(&mut self, mode: u16, enabled: bool, emit: &mut impl FnMut(TerminalModeChange)) {
        match mode {
            1000 | 1002 | 1003 => self.set_mouse_mode(mode, enabled, emit),
            1004 => {
                if self.focus != enabled {
                    self.focus = enabled;
                    emit(TerminalModeChange::Focus(enabled));
                }
            }
            2004 => {
                if self.bracketed_paste != enabled {
                    self.bracketed_paste = enabled;
                    emit(TerminalModeChange::BracketedPaste(enabled));
                }
            }
            _ => {}
        }
    }

    fn set_mouse_mode(
        &mut self,
        mode: u16,
        enabled: bool,
        emit: &mut impl FnMut(TerminalModeChange),
    ) {
        let before = self.mouse_reporting();
        self.mouse_modes.set(mode, enabled);
        let after = self.mouse_reporting();
        if before != after {
            emit(TerminalModeChange::Mouse(after));
        }
    }

    fn mouse_reporting(&self) -> bool {
        self.mouse_modes.any_enabled()
    }

    fn retain_possible_prefix(&mut self) {
        let retained = Self::MODE_PREFIX.len().saturating_sub(1);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

#[derive(Default)]
struct MouseModes(u8);

impl MouseModes {
    const NORMAL: u8 = 1;
    const BUTTON_EVENT: u8 = 1 << 1;
    const ANY_EVENT: u8 = 1 << 2;

    fn set(&mut self, mode: u16, enabled: bool) {
        let Some(mask) = Self::mask(mode) else {
            return;
        };

        if enabled {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }
    }

    fn any_enabled(&self) -> bool {
        self.0 != 0
    }

    const fn mask(mode: u16) -> Option<u8> {
        match mode {
            1000 => Some(Self::NORMAL),
            1002 => Some(Self::BUTTON_EVENT),
            1003 => Some(Self::ANY_EVENT),
            _ => None,
        }
    }
}

enum ModeParse {
    Complete {
        modes: Vec<u16>,
        enabled: bool,
        consumed: usize,
    },
    Incomplete,
    Invalid,
}

#[derive(Default)]
struct TerminalOutputFilter {
    pending: Vec<u8>,
}

impl TerminalOutputFilter {
    const RESPONSES: &'static [TerminalQueryResponse] = &[
        TerminalQueryResponse {
            query: b"\x1b[6n",
            response: b"\x1b[1;1R",
        },
        TerminalQueryResponse {
            query: b"\x1b[c",
            response: b"\x1b[?1;2c",
        },
        TerminalQueryResponse {
            query: b"\x1b[>c",
            response: b"\x1b[>0;0;0c",
        },
        TerminalQueryResponse {
            query: b"\x1b[5n",
            response: b"\x1b[0n",
        },
    ];

    fn write(
        &mut self,
        bytes: &[u8],
        output: &mut dyn Write,
        mut respond: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        self.pending.extend_from_slice(bytes);

        while let Some((index, response)) = self.find_next_response() {
            output.write_all(&self.pending[..index])?;
            respond(response.response)?;
            self.pending.drain(..index + response.query.len());
        }

        let retained = Self::suffix_len_matching_query_prefix(&self.pending);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            output.write_all(&self.pending[..writable])?;
            self.pending.drain(..writable);
        }

        Ok(())
    }

    fn find_next_response(&self) -> Option<(usize, &'static TerminalQueryResponse)> {
        Self::RESPONSES
            .iter()
            .filter_map(|response| {
                find_subslice(&self.pending, response.query).map(|index| (index, response))
            })
            .min_by_key(|(index, _)| *index)
    }

    fn suffix_len_matching_query_prefix(pending: &[u8]) -> usize {
        Self::RESPONSES
            .iter()
            .map(|response| suffix_len_matching_prefix(pending, response.query))
            .max()
            .unwrap_or(0)
    }

    fn flush(&mut self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(&self.pending)?;
        self.pending.clear();
        Ok(())
    }
}

struct TerminalQueryResponse {
    query: &'static [u8],
    response: &'static [u8],
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn suffix_len_matching_prefix(haystack: &[u8], needle: &[u8]) -> usize {
    let max = haystack.len().min(needle.len().saturating_sub(1));
    (1..=max)
        .rev()
        .find(|&length| haystack[haystack.len() - length..] == needle[..length])
        .unwrap_or(0)
}

fn encode_input_event(
    event: Event,
    mouse_reporting: bool,
    focus_reporting: bool,
    bracketed_paste: bool,
) -> Option<Vec<u8>> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => encode_key(key),
        Event::Paste(text) if bracketed_paste => Some(encode_bracketed_paste(&text)),
        Event::Paste(text) => Some(text.into_bytes()),
        Event::Mouse(event) if mouse_reporting => encode_mouse_event(event),
        Event::FocusGained if focus_reporting => Some(b"\x1b[I".to_vec()),
        Event::FocusLost if focus_reporting => Some(b"\x1b[O".to_vec()),
        _ => None,
    }
}

fn encode_bracketed_paste(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(b"\x1b[200~".len() + text.len() + b"\x1b[201~".len());
    bytes.extend_from_slice(b"\x1b[200~");
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
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

    use super::{
        TerminalModeChange, TerminalModeTracker, TerminalOutputFilter, encode_input_event,
        encode_key, resolve_local_size,
    };

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
            encode_input_event(Event::Paste("line 1\n中".to_owned()), false, false, false).unwrap(),
            "line 1\n中".as_bytes()
        );
    }

    #[test]
    fn encodes_paste_event_as_bracketed_paste_when_enabled() {
        assert_eq!(
            encode_input_event(Event::Paste("line 1\n中".to_owned()), false, false, true).unwrap(),
            b"\x1b[200~line 1\n\xe4\xb8\xad\x1b[201~"
        );
    }

    #[test]
    fn ignores_mouse_events_unless_enabled() {
        assert!(encode_input_event(left_mouse_down(), false, false, false).is_none());
    }

    #[test]
    fn encodes_mouse_events_as_sgr_sequences_when_enabled() {
        assert_eq!(
            encode_input_event(left_mouse_down(), true, false, false).unwrap(),
            b"\x1b[<0;1;2M"
        );
        assert_eq!(
            encode_input_event(left_mouse_release(), true, false, false).unwrap(),
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
                true,
                false,
                false
            )
            .unwrap(),
            b"\x1b[<81;5;6M"
        );
    }

    #[test]
    fn encodes_focus_events_when_focus_reporting_is_enabled() {
        assert_eq!(
            encode_input_event(Event::FocusGained, false, true, false).unwrap(),
            b"\x1b[I"
        );
        assert_eq!(
            encode_input_event(Event::FocusLost, false, true, false).unwrap(),
            b"\x1b[O"
        );
    }

    #[test]
    fn encodes_focus_events_only_when_focus_reporting_is_enabled() {
        assert!(encode_input_event(Event::FocusGained, true, false, false).is_none());
        assert_eq!(
            encode_input_event(Event::FocusGained, false, true, false).unwrap(),
            b"\x1b[I"
        );
    }

    #[test]
    fn tracks_mouse_reporting_from_pty_output_modes() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?1006h", |change| changes.push(change));
        assert!(changes.is_empty());

        tracker.process(b"\x1b[?1000h", |change| changes.push(change));
        tracker.process(b"\x1b[?1000l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::Mouse(true),
                TerminalModeChange::Mouse(false)
            ]
        );
    }

    #[test]
    fn tracks_combined_mouse_reporting_modes_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?1002;1006h", |change| changes.push(change));
        tracker.process(b"\x1b[?1006l", |change| changes.push(change));
        tracker.process(b"\x1b[?1002l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::Mouse(true),
                TerminalModeChange::Mouse(false)
            ]
        );
    }

    #[test]
    fn tracks_split_focus_reporting_from_pty_output_modes() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"before\x1b[?", |change| changes.push(change));
        tracker.process(b"1004hafter\x1b[?1004l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::Focus(true),
                TerminalModeChange::Focus(false)
            ]
        );
    }

    #[test]
    fn tracks_bracketed_paste_from_pty_output_modes() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?2004h", |change| changes.push(change));
        tracker.process(b"\x1b[?2004l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::BracketedPaste(true),
                TerminalModeChange::BracketedPaste(false)
            ]
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
    fn terminal_output_filter_does_not_hold_unrelated_tail_bytes() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"console-smoke", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"console-smoke");
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
    fn terminal_output_filter_answers_device_and_status_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"a\x1b[c b\x1b[>c c\x1b[5n d", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"a b c d");
        assert_eq!(responses, b"\x1b[?1;2c\x1b[>0;0;0c\x1b[0n");
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

    #[test]
    fn terminal_output_filter_handles_split_device_attribute_query() {
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
            .write(b">cafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[>0;0;0c");
    }
}
