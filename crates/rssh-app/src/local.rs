use std::{
    error::Error,
    fs::File,
    io::{self, IsTerminal, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyEventState, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute, terminal,
};
use rssh_core::TerminalSize;
use rssh_pty::{PtyExitStatus, PtySession, PtySize};
use rssh_terminal::Terminal;

use crate::{
    cli::LocalOptions,
    terminal_input::{TerminalKey, encode_terminal_key},
    terminal_modes::{
        MouseInputMode, MouseProtocolMode, MouseReportingMode, TerminalModeChange,
        TerminalModeTracker,
    },
    visible_output::TerminalVisibleOutputFilter,
};

pub fn run(options: &LocalOptions) -> Result<PtyExitStatus, Box<dyn Error>> {
    let size = resolve_local_size(options.size);
    let mut session = PtySession::spawn(&options.command, size)?;
    let mut reader = session.take_reader()?;
    let mut writer = session.take_writer()?;
    let mut log_file = match &options.log {
        Some(path) => Some(File::create(path)?),
        None => None,
    };
    let (reader_done_sender, reader_done_receiver) = mpsc::channel();
    let (writer_done_sender, writer_done_receiver) = mpsc::channel();
    let (pty_input_sender, pty_input_receiver) = mpsc::channel();
    let (control_sender, control_receiver) = mpsc::channel();
    let terminal_response_sender = pty_input_sender.clone();
    let output_control_sender = control_sender.clone();
    let runtime_state = LocalRuntimeState::new(size);
    let output_terminal_size = runtime_state.terminal_size.clone();

    let _reader_thread = thread::spawn(move || {
        let result = copy_pty_output(
            &mut reader,
            &terminal_response_sender,
            &output_control_sender,
            output_terminal_size,
            log_file.as_mut().map(|file| file as &mut dyn Write),
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
        runtime_state.input_reporting.clone(),
    );
    let run_result = run_input_loop(
        &mut session,
        &reader_done_receiver,
        &writer_done_receiver,
        &control_receiver,
        &mut raw_mode,
        &runtime_state,
        options.mouse,
    );

    drop(pty_input_sender);
    drop(session);

    run_result
}

#[derive(Clone)]
struct SharedTerminalSize {
    columns: Arc<AtomicU16>,
    rows: Arc<AtomicU16>,
}

impl SharedTerminalSize {
    fn new(size: PtySize) -> Self {
        Self {
            columns: Arc::new(AtomicU16::new(size.columns())),
            rows: Arc::new(AtomicU16::new(size.rows())),
        }
    }

    fn snapshot(&self) -> PtySize {
        PtySize::try_new(
            self.columns.load(Ordering::Relaxed),
            self.rows.load(Ordering::Relaxed),
        )
        .expect("shared terminal size remains valid")
    }

    fn set(&self, size: PtySize) {
        self.columns.store(size.columns(), Ordering::Relaxed);
        self.rows.store(size.rows(), Ordering::Relaxed);
    }
}

impl Default for SharedTerminalSize {
    fn default() -> Self {
        Self::new(fallback_pty_size())
    }
}

enum LocalControlEvent {
    Resize(PtySize),
    SetApplicationCursorKeys(bool),
    SetApplicationKeypad(bool),
    SetBracketedPaste(bool),
    SetMouseReporting(MouseInputMode),
    SetFocusReporting(bool),
}

#[derive(Clone, Default)]
struct InputReporting {
    application_cursor_keys: Arc<AtomicBool>,
    application_keypad: Arc<AtomicBool>,
    bracketed_paste: Arc<AtomicBool>,
    mouse: Arc<AtomicU8>,
    focus: Arc<AtomicBool>,
}

impl InputReporting {
    fn snapshot(&self) -> InputModes {
        InputModes::default()
            .with_application_cursor_keys(self.application_cursor_keys_enabled())
            .with_application_keypad(self.application_keypad_enabled())
            .with_bracketed_paste(self.bracketed_paste_enabled())
            .with_mouse_input_mode(self.mouse_input_mode())
            .with_focus_reporting(self.focus_enabled())
    }

    fn application_cursor_keys_enabled(&self) -> bool {
        self.application_cursor_keys.load(Ordering::Relaxed)
    }

    fn application_keypad_enabled(&self) -> bool {
        self.application_keypad.load(Ordering::Relaxed)
    }

    fn bracketed_paste_enabled(&self) -> bool {
        self.bracketed_paste.load(Ordering::Relaxed)
    }

    fn mouse_input_mode(&self) -> MouseInputMode {
        MouseInputMode::from_bits(self.mouse.load(Ordering::Relaxed))
    }

    fn focus_enabled(&self) -> bool {
        self.focus.load(Ordering::Relaxed)
    }

    fn set_mouse(&self, mode: MouseInputMode) {
        self.mouse.store(mode.bits(), Ordering::Relaxed);
    }

    fn set_focus(&self, enabled: bool) {
        self.focus.store(enabled, Ordering::Relaxed);
    }

    fn set_bracketed_paste(&self, enabled: bool) {
        self.bracketed_paste.store(enabled, Ordering::Relaxed);
    }

    fn set_application_cursor_keys(&self, enabled: bool) {
        self.application_cursor_keys
            .store(enabled, Ordering::Relaxed);
    }

    fn set_application_keypad(&self, enabled: bool) {
        self.application_keypad.store(enabled, Ordering::Relaxed);
    }
}

struct LocalRuntimeState {
    input_reporting: InputReporting,
    terminal_size: SharedTerminalSize,
}

impl LocalRuntimeState {
    fn new(size: PtySize) -> Self {
        Self {
            input_reporting: InputReporting::default(),
            terminal_size: SharedTerminalSize::new(size),
        }
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
                    let Some(bytes) = encode_input_event(event, input_reporting.snapshot()) else {
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
    terminal_size: SharedTerminalSize,
    log: Option<&mut dyn Write>,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut output = SessionLogWriter::new(&mut stdout, log);
    let mut buffer = [0; 8192];
    let mut output_filter = TerminalOutputFilter::with_shared_size(terminal_size);
    let mut mode_tracker = TerminalModeTracker::default();

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                output_filter.flush(&mut output)?;
                output.flush()?;
                return Ok(());
            }
            Ok(count) => {
                mode_tracker.process(&buffer[..count], |change| {
                    let event = match change {
                        TerminalModeChange::ApplicationCursorKeys(enabled) => {
                            LocalControlEvent::SetApplicationCursorKeys(enabled)
                        }
                        TerminalModeChange::ApplicationKeypad(enabled) => {
                            LocalControlEvent::SetApplicationKeypad(enabled)
                        }
                        TerminalModeChange::BracketedPaste(enabled) => {
                            LocalControlEvent::SetBracketedPaste(enabled)
                        }
                        TerminalModeChange::Mouse(mode) => {
                            LocalControlEvent::SetMouseReporting(mode)
                        }
                        TerminalModeChange::Focus(enabled) => {
                            LocalControlEvent::SetFocusReporting(enabled)
                        }
                    };
                    let _ = control_sender.send(event);
                });
                output_filter.write(&buffer[..count], &mut output, |response| {
                    pty_input_sender
                        .send(response.to_vec())
                        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "PTY input closed"))
                })?;
                output.flush()?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

struct SessionLogWriter<'screen, 'log> {
    screen: &'screen mut dyn Write,
    log: Option<&'log mut dyn Write>,
    log_filter: TerminalVisibleOutputFilter,
}

impl<'screen, 'log> SessionLogWriter<'screen, 'log> {
    fn new(screen: &'screen mut dyn Write, log: Option<&'log mut dyn Write>) -> Self {
        Self {
            screen,
            log,
            log_filter: TerminalVisibleOutputFilter::default(),
        }
    }
}

impl Write for SessionLogWriter<'_, '_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.screen.write(buffer)?;
        if count > 0 {
            if let Some(log) = self.log.as_mut() {
                log.write_all(&self.log_filter.process(&buffer[..count]))?;
            }
        }
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.screen.flush()?;
        if let Some(log) = self.log.as_mut() {
            log.flush()?;
        }
        Ok(())
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
    runtime_state: &LocalRuntimeState,
    allow_application_reporting: bool,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    let mut exited_status: Option<(PtyExitStatus, Instant)> = None;

    loop {
        if let Ok(reader_result) = reader_done_receiver.try_recv() {
            reader_result?;
            let status = match exited_status {
                Some((status, _)) => status,
                None => session.wait()?,
            };
            return Ok(status);
        }

        if let Ok(writer_result) = writer_done_receiver.try_recv() {
            writer_result?;
        }

        while let Ok(control_event) = control_receiver.try_recv() {
            match control_event {
                LocalControlEvent::Resize(size) => {
                    session.resize(size)?;
                    runtime_state.terminal_size.set(size);
                }
                LocalControlEvent::SetApplicationCursorKeys(enabled) => {
                    runtime_state
                        .input_reporting
                        .set_application_cursor_keys(enabled);
                }
                LocalControlEvent::SetApplicationKeypad(enabled) => {
                    runtime_state
                        .input_reporting
                        .set_application_keypad(enabled);
                }
                LocalControlEvent::SetBracketedPaste(enabled) => {
                    runtime_state.input_reporting.set_bracketed_paste(enabled);
                }
                LocalControlEvent::SetMouseReporting(mode) => {
                    let mode = if allow_application_reporting
                        && raw_mode.set_mouse_capture(mode.reporting_enabled())?
                    {
                        mode
                    } else {
                        mode.with_reporting(MouseReportingMode::None)
                    };
                    runtime_state.input_reporting.set_mouse(mode);
                }
                LocalControlEvent::SetFocusReporting(enabled) => {
                    let enabled = if allow_application_reporting {
                        raw_mode.set_focus_change(enabled)?
                    } else {
                        false
                    };
                    runtime_state.input_reporting.set_focus(enabled);
                }
            }
        }

        if let Some((status, exited_at)) = &exited_status {
            if exited_at.elapsed() >= Duration::from_millis(100) {
                return Ok(status.clone());
            }
        } else if let Some(status) = session.try_wait()? {
            exited_status = Some((status, Instant::now()));
        }

        thread::sleep(Duration::from_millis(10));
    }
}

struct TerminalOutputFilter {
    pending: Vec<u8>,
    size: SharedTerminalSize,
    mirror: Terminal,
    mirror_size: PtySize,
}

impl TerminalOutputFilter {
    const CELL_HEIGHT_PIXELS: u16 = 16;
    const CELL_WIDTH_PIXELS: u16 = 8;
    const RESPONSES: &'static [TerminalQueryResponse] = &[
        TerminalQueryResponse {
            query: b"\x1b[6n",
            response: TerminalResponse::CursorPosition { private: false },
        },
        TerminalQueryResponse {
            query: b"\x9b6n",
            response: TerminalResponse::CursorPosition { private: false },
        },
        TerminalQueryResponse {
            query: b"\x1b[?6n",
            response: TerminalResponse::CursorPosition { private: true },
        },
        TerminalQueryResponse {
            query: b"\x9b?6n",
            response: TerminalResponse::CursorPosition { private: true },
        },
        TerminalQueryResponse {
            query: b"\x1b[c",
            response: TerminalResponse::Static(b"\x1b[?1;2c"),
        },
        TerminalQueryResponse {
            query: b"\x9bc",
            response: TerminalResponse::Static(b"\x1b[?1;2c"),
        },
        TerminalQueryResponse {
            query: b"\x1b[>c",
            response: TerminalResponse::Static(b"\x1b[>0;0;0c"),
        },
        TerminalQueryResponse {
            query: b"\x9b>c",
            response: TerminalResponse::Static(b"\x1b[>0;0;0c"),
        },
        TerminalQueryResponse {
            query: b"\x1b[5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\x9b5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\x1b[14t",
            response: TerminalResponse::WindowPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x9b14t",
            response: TerminalResponse::WindowPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[16t",
            response: TerminalResponse::CharacterCellSize,
        },
        TerminalQueryResponse {
            query: b"\x9b16t",
            response: TerminalResponse::CharacterCellSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[18t",
            response: TerminalResponse::TextAreaSize,
        },
        TerminalQueryResponse {
            query: b"\x9b18t",
            response: TerminalResponse::TextAreaSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[19t",
            response: TerminalResponse::ScreenSize,
        },
        TerminalQueryResponse {
            query: b"\x9b19t",
            response: TerminalResponse::ScreenSize,
        },
    ];

    #[cfg(test)]
    fn new(size: PtySize) -> Self {
        Self::with_shared_size(SharedTerminalSize::new(size))
    }

    fn with_shared_size(size: SharedTerminalSize) -> Self {
        let mirror_size = size.snapshot();
        Self {
            pending: Vec::new(),
            size,
            mirror: Terminal::new(terminal_size_from_pty(mirror_size)),
            mirror_size,
        }
    }

    fn write(
        &mut self,
        bytes: &[u8],
        output: &mut dyn Write,
        mut respond: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        self.pending.extend_from_slice(bytes);

        while let Some((index, response)) = self.find_next_response() {
            output.write_all(&self.pending[..index])?;
            self.feed_mirror_through_output(index);
            let response_bytes = self.response_bytes(response.response);
            respond(&response_bytes)?;
            self.pending.drain(..index + response.query.len());
        }

        let retained = Self::suffix_len_matching_query_prefix(&self.pending);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            output.write_all(&self.pending[..writable])?;
            self.feed_mirror_through_output(writable);
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
        self.feed_mirror_through_output(self.pending.len());
        self.pending.clear();
        Ok(())
    }

    fn feed_mirror_through_output(&mut self, end: usize) {
        self.sync_mirror_size();
        self.mirror.feed(&self.pending[..end]);
    }

    fn response_bytes(&mut self, response: TerminalResponse) -> Vec<u8> {
        match response {
            TerminalResponse::Static(bytes) => bytes.to_vec(),
            TerminalResponse::CursorPosition { private } => {
                self.sync_mirror_size();
                let (row, column) = self.mirror.cursor();
                if private {
                    format!(
                        "\x1b[?{};{}R",
                        row.saturating_add(1),
                        column.saturating_add(1)
                    )
                    .into_bytes()
                } else {
                    format!(
                        "\x1b[{};{}R",
                        row.saturating_add(1),
                        column.saturating_add(1)
                    )
                    .into_bytes()
                }
            }
            TerminalResponse::WindowPixelSize => {
                let size = self.size.snapshot();
                format!(
                    "\x1b[4;{};{}t",
                    u32::from(size.rows()) * u32::from(Self::CELL_HEIGHT_PIXELS),
                    u32::from(size.columns()) * u32::from(Self::CELL_WIDTH_PIXELS)
                )
                .into_bytes()
            }
            TerminalResponse::CharacterCellSize => format!(
                "\x1b[6;{};{}t",
                Self::CELL_HEIGHT_PIXELS,
                Self::CELL_WIDTH_PIXELS
            )
            .into_bytes(),
            TerminalResponse::TextAreaSize => {
                let size = self.size.snapshot();
                format!("\x1b[8;{};{}t", size.rows(), size.columns()).into_bytes()
            }
            TerminalResponse::ScreenSize => {
                let size = self.size.snapshot();
                format!("\x1b[9;{};{}t", size.rows(), size.columns()).into_bytes()
            }
        }
    }

    fn sync_mirror_size(&mut self) {
        let size = self.size.snapshot();
        if size != self.mirror_size {
            self.mirror.resize(terminal_size_from_pty(size));
            self.mirror_size = size;
        }
    }
}

struct TerminalQueryResponse {
    query: &'static [u8],
    response: TerminalResponse,
}

#[derive(Clone, Copy)]
enum TerminalResponse {
    Static(&'static [u8]),
    CursorPosition { private: bool },
    WindowPixelSize,
    CharacterCellSize,
    TextAreaSize,
    ScreenSize,
}

impl Default for TerminalOutputFilter {
    fn default() -> Self {
        Self::with_shared_size(SharedTerminalSize::default())
    }
}

fn terminal_size_from_pty(size: PtySize) -> TerminalSize {
    TerminalSize::new(size.columns(), size.rows())
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

#[derive(Clone, Copy, Default)]
struct InputModes(u8);

impl InputModes {
    const APPLICATION_CURSOR_KEYS: u8 = 1;
    const APPLICATION_KEYPAD: u8 = 1 << 1;
    const BRACKETED_PASTE: u8 = 1 << 2;
    const FOCUS_REPORTING: u8 = 1 << 3;
    const MOUSE_INPUT_MASK: u8 = 0b0111_0000;
    const MOUSE_REPORTING_SHIFT: u8 = 4;

    fn application_cursor_keys(self) -> bool {
        self.enabled(Self::APPLICATION_CURSOR_KEYS)
    }

    fn bracketed_paste(self) -> bool {
        self.enabled(Self::BRACKETED_PASTE)
    }

    fn application_keypad(self) -> bool {
        self.enabled(Self::APPLICATION_KEYPAD)
    }

    fn mouse_reporting(self) -> bool {
        self.mouse_input_mode().reporting_enabled()
    }

    fn mouse_input_mode(self) -> MouseInputMode {
        MouseInputMode::from_bits((self.0 & Self::MOUSE_INPUT_MASK) >> Self::MOUSE_REPORTING_SHIFT)
    }

    fn focus_reporting(self) -> bool {
        self.enabled(Self::FOCUS_REPORTING)
    }

    fn with_application_cursor_keys(self, enabled: bool) -> Self {
        self.with_flag(Self::APPLICATION_CURSOR_KEYS, enabled)
    }

    fn with_application_keypad(self, enabled: bool) -> Self {
        self.with_flag(Self::APPLICATION_KEYPAD, enabled)
    }

    fn with_bracketed_paste(self, enabled: bool) -> Self {
        self.with_flag(Self::BRACKETED_PASTE, enabled)
    }

    fn with_mouse_input_mode(mut self, mode: MouseInputMode) -> Self {
        self.0 &= !Self::MOUSE_INPUT_MASK;
        self.0 |= mode.bits() << Self::MOUSE_REPORTING_SHIFT;
        self
    }

    fn with_focus_reporting(self, enabled: bool) -> Self {
        self.with_flag(Self::FOCUS_REPORTING, enabled)
    }

    fn enabled(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    fn with_flag(mut self, flag: u8, enabled: bool) -> Self {
        if enabled {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
        self
    }
}

fn encode_input_event(event: Event, modes: InputModes) -> Option<Vec<u8>> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => encode_key_with_mode(
            key,
            modes.application_cursor_keys(),
            modes.application_keypad(),
        ),
        Event::Paste(text) if modes.bracketed_paste() => Some(encode_bracketed_paste(&text)),
        Event::Paste(text) => Some(text.into_bytes()),
        Event::Mouse(event) if modes.mouse_reporting() => {
            encode_mouse_event(event, modes.mouse_input_mode())
        }
        Event::FocusGained if modes.focus_reporting() => Some(b"\x1b[I".to_vec()),
        Event::FocusLost if modes.focus_reporting() => Some(b"\x1b[O".to_vec()),
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

fn encode_mouse_event(event: MouseEvent, mode: MouseInputMode) -> Option<Vec<u8>> {
    if !mode.allows(event.kind) {
        return None;
    }

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

    let column = event.column.checked_add(1)?;
    let row = event.row.checked_add(1)?;

    match mode.protocol() {
        MouseProtocolMode::Sgr => {
            let final_byte = if matches!(event.kind, MouseEventKind::Up(_)) {
                b'm'
            } else {
                b'M'
            };
            Some(format!("\x1b[<{code};{column};{row}{}", final_byte as char).into_bytes())
        }
        MouseProtocolMode::X10 => encode_legacy_mouse_event(event.kind, code, column, row),
    }
}

fn encode_legacy_mouse_event(
    kind: MouseEventKind,
    mut code: u16,
    column: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if matches!(kind, MouseEventKind::Up(_)) {
        code = 3 + (code & !0b11);
    }

    Some(vec![
        0x1b,
        b'[',
        b'M',
        legacy_mouse_byte(code)?,
        legacy_mouse_byte(column)?,
        legacy_mouse_byte(row)?,
    ])
}

fn legacy_mouse_byte(value: u16) -> Option<u8> {
    u8::try_from(value.checked_add(32)?).ok()
}

const fn mouse_button_code(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

#[cfg(test)]
fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    encode_key_with_mode(key, false, false)
}

fn encode_key_with_mode(
    key: KeyEvent,
    application_cursor_keys: bool,
    application_keypad: bool,
) -> Option<Vec<u8>> {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if let Some(bytes) = encode_modified_key(key) {
        return Some(bytes);
    }
    if application_keypad {
        if let Some(bytes) = encode_application_keypad_key(key) {
            return Some(bytes);
        }
    }
    if application_cursor_keys {
        if let Some(bytes) = encode_application_cursor_key(key) {
            return Some(bytes);
        }
    }

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

fn encode_application_keypad_key(key: KeyEvent) -> Option<Vec<u8>> {
    if !key.modifiers.is_empty() {
        return None;
    }
    if !key.state.contains(KeyEventState::KEYPAD) && !matches!(key.code, KeyCode::KeypadBegin) {
        return None;
    }

    let final_byte = match key.code {
        KeyCode::Tab => b'I',
        KeyCode::Enter => b'M',
        KeyCode::Char(' ') => b' ',
        KeyCode::Char('*') => b'j',
        KeyCode::Char('+') => b'k',
        KeyCode::Char(',') => b'l',
        KeyCode::Char('-') => b'm',
        KeyCode::Char('.') => b'n',
        KeyCode::Char('/') => b'o',
        KeyCode::Char('0') => b'p',
        KeyCode::Char('1') => b'q',
        KeyCode::Char('2') => b'r',
        KeyCode::Char('3') => b's',
        KeyCode::Char('4') => b't',
        KeyCode::Char('5') | KeyCode::KeypadBegin => b'u',
        KeyCode::Char('6') => b'v',
        KeyCode::Char('7') => b'w',
        KeyCode::Char('8') => b'x',
        KeyCode::Char('9') => b'y',
        KeyCode::Char('=') => b'X',
        _ => return None,
    };

    Some(vec![0x1b, b'O', final_byte])
}

fn encode_application_cursor_key(key: KeyEvent) -> Option<Vec<u8>> {
    match key.code {
        KeyCode::Up => Some(b"\x1bOA".to_vec()),
        KeyCode::Down => Some(b"\x1bOB".to_vec()),
        KeyCode::Right => Some(b"\x1bOC".to_vec()),
        KeyCode::Left => Some(b"\x1bOD".to_vec()),
        _ => None,
    }
}

fn encode_modified_key(key: KeyEvent) -> Option<Vec<u8>> {
    let modifier = xterm_modifier(key.modifiers)?;

    match key.code {
        KeyCode::Left => Some(format!("\x1b[1;{modifier}D").into_bytes()),
        KeyCode::Right => Some(format!("\x1b[1;{modifier}C").into_bytes()),
        KeyCode::Up => Some(format!("\x1b[1;{modifier}A").into_bytes()),
        KeyCode::Down => Some(format!("\x1b[1;{modifier}B").into_bytes()),
        KeyCode::Home => Some(format!("\x1b[1;{modifier}H").into_bytes()),
        KeyCode::End => Some(format!("\x1b[1;{modifier}F").into_bytes()),
        KeyCode::Insert => Some(format!("\x1b[2;{modifier}~").into_bytes()),
        KeyCode::Delete => Some(format!("\x1b[3;{modifier}~").into_bytes()),
        KeyCode::PageUp => Some(format!("\x1b[5;{modifier}~").into_bytes()),
        KeyCode::PageDown => Some(format!("\x1b[6;{modifier}~").into_bytes()),
        KeyCode::F(1) => Some(format!("\x1b[1;{modifier}P").into_bytes()),
        KeyCode::F(2) => Some(format!("\x1b[1;{modifier}Q").into_bytes()),
        KeyCode::F(3) => Some(format!("\x1b[1;{modifier}R").into_bytes()),
        KeyCode::F(4) => Some(format!("\x1b[1;{modifier}S").into_bytes()),
        KeyCode::F(key) => modified_tilde_function_key(key, modifier),
        _ => None,
    }
}

fn modified_tilde_function_key(key: u8, modifier: u8) -> Option<Vec<u8>> {
    let base = match key {
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        _ => return None,
    };

    Some(format!("\x1b[{base};{modifier}~").into_bytes())
}

fn xterm_modifier(modifiers: KeyModifiers) -> Option<u8> {
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let alt = modifiers.contains(KeyModifiers::ALT);
    let control = modifiers.contains(KeyModifiers::CONTROL);
    if !(shift || alt || control) {
        return None;
    }

    Some(1 + u8::from(shift) + u8::from(alt) * 2 + u8::from(control) * 4)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton,
        MouseEvent, MouseEventKind,
    };

    use crate::terminal_modes::{
        MouseInputMode, MouseProtocolMode, MouseReportingMode, TerminalModeChange,
        TerminalModeTracker,
    };

    use super::{
        InputModes, TerminalOutputFilter, encode_input_event, encode_key, resolve_local_size,
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
    fn encodes_application_cursor_keys_when_enabled() {
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
                InputModes::default().with_application_cursor_keys(true),
            )
            .unwrap(),
            b"\x1bOA"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
                InputModes::default().with_application_cursor_keys(true),
            )
            .unwrap(),
            b"\x1bOC"
        );
    }

    #[test]
    fn encodes_keypad_keys_when_application_keypad_is_enabled() {
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('5'),
                    KeyModifiers::NONE,
                    KeyEventKind::Press,
                    KeyEventState::KEYPAD
                )),
                InputModes::default().with_application_keypad(true),
            )
            .unwrap(),
            b"\x1bOu"
        );
    }

    #[test]
    fn encodes_modified_navigation_keys_as_xterm_sequences() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL)).unwrap(),
            b"\x1b[1;5D"
        );
        assert_eq!(
            encode_key(KeyEvent::new(
                KeyCode::Right,
                KeyModifiers::SHIFT | KeyModifiers::ALT
            ))
            .unwrap(),
            b"\x1b[1;4C"
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::CONTROL)).unwrap(),
            b"\x1b[3;5~"
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT)).unwrap(),
            b"\x1b[15;2~"
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
            encode_input_event(Event::Paste("line 1\n中".to_owned()), InputModes::default())
                .unwrap(),
            "line 1\n中".as_bytes()
        );
    }

    #[test]
    fn encodes_paste_event_as_bracketed_paste_when_enabled() {
        assert_eq!(
            encode_input_event(
                Event::Paste("line 1\n中".to_owned()),
                InputModes::default().with_bracketed_paste(true)
            )
            .unwrap(),
            b"\x1b[200~line 1\n\xe4\xb8\xad\x1b[201~"
        );
    }

    #[test]
    fn ignores_mouse_events_unless_enabled() {
        assert!(encode_input_event(left_mouse_down(), InputModes::default()).is_none());
    }

    #[test]
    fn encodes_mouse_events_as_sgr_sequences_when_enabled() {
        let modes = InputModes::default().with_mouse_input_mode(MouseInputMode::new(
            MouseReportingMode::Normal,
            MouseProtocolMode::Sgr,
        ));

        assert_eq!(
            encode_input_event(left_mouse_down(), modes).unwrap(),
            b"\x1b[<0;1;2M"
        );
        assert_eq!(
            encode_input_event(left_mouse_release(), modes).unwrap(),
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
                modes
            )
            .unwrap(),
            b"\x1b[<81;5;6M"
        );
    }

    #[test]
    fn encodes_mouse_events_as_legacy_sequences_without_sgr_protocol() {
        let modes = InputModes::default().with_mouse_input_mode(MouseInputMode::new(
            MouseReportingMode::Normal,
            MouseProtocolMode::X10,
        ));

        assert_eq!(
            encode_input_event(left_mouse_down(), modes).unwrap(),
            b"\x1b[M !\""
        );
        assert_eq!(
            encode_input_event(left_mouse_release(), modes).unwrap(),
            b"\x1b[M#!\""
        );
    }

    #[test]
    fn normal_mouse_reporting_ignores_motion_without_buttons() {
        let modes = InputModes::default().with_mouse_input_mode(MouseInputMode::new(
            MouseReportingMode::Normal,
            MouseProtocolMode::X10,
        ));

        assert!(encode_input_event(mouse_moved(), modes).is_none());
    }

    #[test]
    fn any_event_mouse_reporting_encodes_motion_without_buttons() {
        let modes = InputModes::default().with_mouse_input_mode(MouseInputMode::new(
            MouseReportingMode::AnyEvent,
            MouseProtocolMode::Sgr,
        ));

        assert_eq!(
            encode_input_event(mouse_moved(), modes).unwrap(),
            b"\x1b[<35;3;4M"
        );
    }

    #[test]
    fn encodes_focus_events_when_focus_reporting_is_enabled() {
        let modes = InputModes::default().with_focus_reporting(true);

        assert_eq!(
            encode_input_event(Event::FocusGained, modes).unwrap(),
            b"\x1b[I"
        );
        assert_eq!(
            encode_input_event(Event::FocusLost, modes).unwrap(),
            b"\x1b[O"
        );
    }

    #[test]
    fn encodes_focus_events_only_when_focus_reporting_is_enabled() {
        assert!(
            encode_input_event(
                Event::FocusGained,
                InputModes::default().with_mouse_input_mode(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::X10,
                ))
            )
            .is_none()
        );
        assert_eq!(
            encode_input_event(
                Event::FocusGained,
                InputModes::default().with_focus_reporting(true)
            )
            .unwrap(),
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
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::Sgr,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::None,
                    MouseProtocolMode::Sgr,
                ))
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
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::ButtonEvent,
                    MouseProtocolMode::Sgr,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::ButtonEvent,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::None,
                    MouseProtocolMode::X10,
                ))
            ]
        );
    }

    #[test]
    fn tracks_sgr_mouse_protocol_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?1000h", |change| changes.push(change));
        tracker.process(b"\x1b[?1006h", |change| changes.push(change));
        tracker.process(b"\x1b[?1006l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::Sgr,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::X10,
                ))
            ]
        );
    }

    #[test]
    fn tracks_mouse_reporting_mode_granularity_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?1000h", |change| changes.push(change));
        tracker.process(b"\x1b[?1002h", |change| changes.push(change));
        tracker.process(b"\x1b[?1003h", |change| changes.push(change));
        tracker.process(b"\x1b[?1003l", |change| changes.push(change));
        tracker.process(b"\x1b[?1002l", |change| changes.push(change));
        tracker.process(b"\x1b[?1000l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::ButtonEvent,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::AnyEvent,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::ButtonEvent,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::None,
                    MouseProtocolMode::X10,
                ))
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

    #[test]
    fn tracks_c1_private_input_modes_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x9b?1;1004;2004h", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::ApplicationCursorKeys(true),
                TerminalModeChange::Focus(true),
                TerminalModeChange::BracketedPaste(true)
            ]
        );
    }

    #[test]
    fn tracks_application_cursor_keys_from_pty_output_modes() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?1h", |change| changes.push(change));
        tracker.process(b"\x1b[?1l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::ApplicationCursorKeys(true),
                TerminalModeChange::ApplicationCursorKeys(false)
            ]
        );
    }

    #[test]
    fn tracks_application_keypad_from_pty_output_modes() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"before\x1b", |change| changes.push(change));
        tracker.process(b"=after\x1b>", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::ApplicationKeypad(true),
                TerminalModeChange::ApplicationKeypad(false)
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

    fn mouse_moved() -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 2,
            row: 3,
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
        assert_eq!(responses, b"\x1b[1;7R");
    }

    #[test]
    fn session_log_writer_records_visible_output() {
        let mut screen = Vec::new();
        let mut log = Vec::new();
        let mut output = super::SessionLogWriter::new(&mut screen, Some(&mut log));

        output.write_all(b"visible").unwrap();
        output.flush().unwrap();

        assert_eq!(screen, b"visible");
        assert_eq!(log, b"visible");
    }

    #[test]
    fn session_log_writer_omits_bell_from_log() {
        let mut screen = Vec::new();
        let mut log = Vec::new();
        let mut output = super::SessionLogWriter::new(&mut screen, Some(&mut log));

        output.write_all(b"before\x07after").unwrap();
        output.flush().unwrap();

        assert_eq!(screen, b"before\x07after");
        assert_eq!(log, b"beforeafter");
    }

    #[test]
    fn session_log_writer_omits_title_sequence_from_log() {
        let mut screen = Vec::new();
        let mut log = Vec::new();
        let mut output = super::SessionLogWriter::new(&mut screen, Some(&mut log));

        output.write_all(b"before\x1b]0;ops\x07after").unwrap();
        output.flush().unwrap();

        assert_eq!(screen, b"before\x1b]0;ops\x07after");
        assert_eq!(log, b"beforeafter");
    }

    #[test]
    fn session_log_writer_omits_split_title_sequence_from_log() {
        let mut screen = Vec::new();
        let mut log = Vec::new();
        let mut output = super::SessionLogWriter::new(&mut screen, Some(&mut log));

        output.write_all(b"before\x1b]0;op").unwrap();
        output.write_all(b"s\x07after").unwrap();
        output.flush().unwrap();

        assert_eq!(screen, b"before\x1b]0;ops\x07after");
        assert_eq!(log, b"beforeafter");
    }

    #[test]
    fn terminal_output_filter_answers_current_cursor_position_query() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"abc\x1b[6n", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"abc");
        assert_eq!(responses, b"\x1b[1;4R");
    }

    #[test]
    fn terminal_output_filter_answers_c1_cursor_position_query() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"abc\x9b6n", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"abc");
        assert_eq!(responses, b"\x1b[1;4R");
    }

    #[test]
    fn terminal_output_filter_answers_private_cursor_position_query() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"abc\x1b[?6n", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"abc");
        assert_eq!(responses, b"\x1b[?1;4R");
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
    fn terminal_output_filter_answers_c1_device_and_status_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"a\x9bc b\x9b>c c\x9b5n d", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"a b c d");
        assert_eq!(responses, b"\x1b[?1;2c\x1b[>0;0;0c\x1b[0n");
    }

    #[test]
    fn terminal_output_filter_answers_text_area_size_query() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[18tafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[8;43;132t");
    }

    #[test]
    fn terminal_output_filter_answers_window_pixel_size_query() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[14tafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[4;688;1056t");
    }

    #[test]
    fn terminal_output_filter_answers_character_cell_size_query() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[16tafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[6;16;8t");
    }

    #[test]
    fn terminal_output_filter_answers_screen_size_query() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[19tafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[9;43;132t");
    }

    #[test]
    fn terminal_output_filter_answers_c1_terminal_size_queries() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x9b18t middle\x9b19tafter",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middleafter");
        assert_eq!(responses, b"\x1b[8;43;132t\x1b[9;43;132t");
    }

    #[test]
    fn terminal_output_filter_answers_c1_window_pixel_and_cell_size_queries() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x9b14t middle\x9b16tafter",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middleafter");
        assert_eq!(responses, b"\x1b[4;688;1056t\x1b[6;16;8t");
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
        assert_eq!(responses, b"\x1b[1;7R");
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
