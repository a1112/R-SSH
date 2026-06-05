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

use base64::{Engine, engine::general_purpose::STANDARD};
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
use rssh_terminal::{Cell, Color, CursorShape, Terminal};

use crate::{
    cli::{LocalOptions, Osc52Policy},
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
    let osc52_policy = options.osc52_policy;

    let _reader_thread = thread::spawn(move || {
        let result = copy_pty_output(
            &mut reader,
            &terminal_response_sender,
            &output_control_sender,
            output_terminal_size,
            osc52_policy,
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
    osc52_policy: Osc52Policy,
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
                output_filter.write_with_clipboard(
                    &buffer[..count],
                    &mut output,
                    |response| {
                        pty_input_sender.send(response.to_vec()).map_err(|_| {
                            io::Error::new(io::ErrorKind::BrokenPipe, "PTY input closed")
                        })
                    },
                    write_local_clipboard_text,
                    read_local_clipboard_text,
                    osc52_policy,
                )?;
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
    mode_tracker: TerminalModeTracker,
    color_state: TerminalColorState,
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
            query: b"\x1b[>q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x1b[>0q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x9b>q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x9b>0q",
            response: TerminalResponse::XtVersion,
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
            query: b"\x1b[11t",
            response: TerminalResponse::WindowState,
        },
        TerminalQueryResponse {
            query: b"\x9b11t",
            response: TerminalResponse::WindowState,
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
            query: b"\x1b[13t",
            response: TerminalResponse::WindowPosition,
        },
        TerminalQueryResponse {
            query: b"\x9b13t",
            response: TerminalResponse::WindowPosition,
        },
        TerminalQueryResponse {
            query: b"\x1b[15t",
            response: TerminalResponse::ScreenPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x9b15t",
            response: TerminalResponse::ScreenPixelSize,
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
        TerminalQueryResponse {
            query: b"\x1b[20t",
            response: TerminalResponse::IconLabel,
        },
        TerminalQueryResponse {
            query: b"\x9b20t",
            response: TerminalResponse::IconLabel,
        },
        TerminalQueryResponse {
            query: b"\x1b[21t",
            response: TerminalResponse::WindowTitle,
        },
        TerminalQueryResponse {
            query: b"\x9b21t",
            response: TerminalResponse::WindowTitle,
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
            mode_tracker: TerminalModeTracker::default(),
            color_state: TerminalColorState::default(),
        }
    }

    #[cfg(test)]
    fn write(
        &mut self,
        bytes: &[u8],
        output: &mut dyn Write,
        respond: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        self.write_with_clipboard(bytes, output, respond, |_| false, || None, Osc52Policy::Off)
    }

    fn write_with_clipboard(
        &mut self,
        bytes: &[u8],
        output: &mut dyn Write,
        mut respond: impl FnMut(&[u8]) -> io::Result<()>,
        mut write_clipboard: impl FnMut(&str) -> bool,
        mut read_clipboard: impl FnMut() -> Option<String>,
        osc52_policy: Osc52Policy,
    ) -> io::Result<()> {
        self.mode_tracker.process_without_emitting(bytes);
        self.color_state.process(bytes);
        self.pending.extend_from_slice(bytes);

        while let Some((index, response)) = self.find_next_response() {
            output.write_all(&self.pending[..index])?;
            self.feed_mirror_through_output(index);
            let consumed_end = index + response.consumed;
            match response.response {
                TerminalResponse::Osc8Hyperlink => {
                    let sequence = self.pending[index..consumed_end].to_vec();
                    self.feed_mirror_bytes(&sequence);
                }
                TerminalResponse::Osc52Write(text) => {
                    if osc52_policy.allows_write() {
                        let _ = write_clipboard(&text);
                    }
                }
                TerminalResponse::Osc52Query(selection) => {
                    if osc52_policy.allows_query()
                        && let Some(text) = read_clipboard()
                    {
                        let response_bytes = encode_osc52_clipboard_response(&selection, &text);
                        respond(&response_bytes)?;
                    }
                }
                response => {
                    let response_bytes = self.response_bytes(response);
                    respond(&response_bytes)?;
                }
            }
            self.pending.drain(..consumed_end);
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

    fn find_next_response(&self) -> Option<(usize, MatchedTerminalResponse)> {
        let static_response = Self::RESPONSES
            .iter()
            .filter_map(|response| {
                find_subslice(&self.pending, response.query).map(|index| {
                    (
                        index,
                        MatchedTerminalResponse {
                            consumed: response.query.len(),
                            response: response.response.clone(),
                        },
                    )
                })
            })
            .min_by_key(|(index, _)| *index);
        let mode_response = find_private_mode_status_query(&self.pending).map(
            |PrivateModeStatusQuery {
                 index,
                 consumed,
                 mode,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::PrivateModeStatus(mode),
                    },
                )
            },
        );
        let osc_color_response = find_osc_color_query(&self.pending).map(
            |OscColorQuery {
                 index,
                 consumed,
                 query,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::OscColor(query),
                    },
                )
            },
        );
        let decrqss_response = find_decrqss_query(&self.pending).map(
            |DecrqssQuery {
                 index,
                 consumed,
                 response,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::Decrqss(response),
                    },
                )
            },
        );
        let xtgettcap_response = find_xtgettcap_query(&self.pending, self.size.snapshot()).map(
            |XtGetTcapQuery {
                 index,
                 consumed,
                 response,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::XtGetTcap(response),
                    },
                )
            },
        );
        let osc52_response = self.find_osc52_response();
        let osc8_response = find_osc8_hyperlink_sequence(&self.pending).map(
            |Osc8HyperlinkSequence { index, consumed }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::Osc8Hyperlink,
                    },
                )
            },
        );

        static_response
            .into_iter()
            .chain(mode_response)
            .chain(osc_color_response)
            .chain(decrqss_response)
            .chain(xtgettcap_response)
            .chain(osc52_response)
            .chain(osc8_response)
            .min_by_key(|(index, _)| *index)
    }

    fn find_osc52_response(&self) -> Option<(usize, MatchedTerminalResponse)> {
        find_osc52_clipboard_sequence(&self.pending).map(
            |Osc52ClipboardSequence {
                 index,
                 consumed,
                 sequence,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: match sequence {
                            ClipboardSequence::Write(text) => TerminalResponse::Osc52Write(text),
                            ClipboardSequence::Query(selection) => {
                                TerminalResponse::Osc52Query(selection)
                            }
                        },
                    },
                )
            },
        )
    }

    fn suffix_len_matching_query_prefix(pending: &[u8]) -> usize {
        let static_query_suffix = Self::RESPONSES
            .iter()
            .map(|response| suffix_len_matching_prefix(pending, response.query))
            .max()
            .unwrap_or(0);
        static_query_suffix
            .max(private_mode_status_query_suffix_len(pending))
            .max(osc_color_query_suffix_len(pending))
            .max(decrqss_query_suffix_len(pending))
            .max(xtgettcap_query_suffix_len(pending))
            .max(osc52_clipboard_sequence_suffix_len(pending))
            .max(osc8_hyperlink_sequence_suffix_len(pending))
            .max(incomplete_osc_control_sequence_suffix_len(pending))
    }

    fn flush(&mut self, output: &mut dyn Write) -> io::Result<()> {
        if let Some(drop_start) = find_incomplete_control_sequence_start(&self.pending) {
            output.write_all(&self.pending[..drop_start])?;
            self.feed_mirror_through_output(drop_start);
            self.pending.clear();
            return Ok(());
        }

        output.write_all(&self.pending)?;
        self.feed_mirror_through_output(self.pending.len());
        self.pending.clear();
        Ok(())
    }

    fn feed_mirror_through_output(&mut self, end: usize) {
        self.sync_mirror_size();
        self.mirror.feed(&self.pending[..end]);
    }

    fn feed_mirror_bytes(&mut self, bytes: &[u8]) {
        self.sync_mirror_size();
        self.mirror.feed(bytes);
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
            TerminalResponse::WindowState => b"\x1b[1t".to_vec(),
            TerminalResponse::WindowPixelSize => {
                let size = self.size.snapshot();
                format!(
                    "\x1b[4;{};{}t",
                    u32::from(size.rows()) * u32::from(Self::CELL_HEIGHT_PIXELS),
                    u32::from(size.columns()) * u32::from(Self::CELL_WIDTH_PIXELS)
                )
                .into_bytes()
            }
            TerminalResponse::WindowPosition => b"\x1b[3;0;0t".to_vec(),
            TerminalResponse::ScreenPixelSize => {
                let size = self.size.snapshot();
                format!(
                    "\x1b[5;{};{}t",
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
            TerminalResponse::IconLabel => osc_title_response(b'L', self.mirror.title()),
            TerminalResponse::WindowTitle => osc_title_response(b'l', self.mirror.title()),
            TerminalResponse::PrivateModeStatus(mode) => format!(
                "\x1b[?{};{}$y",
                mode,
                self.mode_tracker.private_mode_report_value(mode)
            )
            .into_bytes(),
            TerminalResponse::OscColor(query) => self.color_state.response(query),
            TerminalResponse::Decrqss(query) => query.response(&self.mirror),
            TerminalResponse::XtGetTcap(query) => query.response(),
            TerminalResponse::XtVersion => xtversion_response(),
            TerminalResponse::Osc8Hyperlink
            | TerminalResponse::Osc52Write(_)
            | TerminalResponse::Osc52Query(_) => Vec::new(),
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

struct MatchedTerminalResponse {
    consumed: usize,
    response: TerminalResponse,
}

#[derive(Clone)]
enum TerminalResponse {
    Static(&'static [u8]),
    CursorPosition { private: bool },
    WindowState,
    WindowPixelSize,
    WindowPosition,
    ScreenPixelSize,
    CharacterCellSize,
    TextAreaSize,
    ScreenSize,
    IconLabel,
    WindowTitle,
    PrivateModeStatus(u16),
    OscColor(OscColorResponse),
    Decrqss(DecrqssResponse),
    XtGetTcap(XtGetTcapResponse),
    XtVersion,
    Osc8Hyperlink,
    Osc52Write(String),
    Osc52Query(String),
}

fn xtversion_response() -> Vec<u8> {
    format!("\x1bP>|R-SSH {}\x1b\\", env!("CARGO_PKG_VERSION")).into_bytes()
}

fn encode_osc52_clipboard_response(selection: &str, text: &str) -> Vec<u8> {
    format!(
        "\x1b]52;{};{}\x07",
        selection,
        STANDARD.encode(text.as_bytes())
    )
    .into_bytes()
}

fn read_local_clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn write_local_clipboard_text(text: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
        .is_ok()
}

fn osc_title_response(kind: u8, title: Option<&str>) -> Vec<u8> {
    let mut response = Vec::from([0x1b, b']', kind]);
    response.extend(
        title
            .unwrap_or_default()
            .bytes()
            .filter(|byte| !matches!(byte, 0x00..=0x1f | 0x7f)),
    );
    response.extend_from_slice(b"\x1b\\");
    response
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

struct Osc8HyperlinkSequence {
    index: usize,
    consumed: usize,
}

fn find_osc8_hyperlink_sequence(bytes: &[u8]) -> Option<Osc8HyperlinkSequence> {
    let mut match_sequence = None;
    for (prefix, prefix_len) in [(b"\x1b]8;".as_slice(), 4), (b"\x9d8;".as_slice(), 3)] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(sequence) = parse_osc8_hyperlink_sequence(bytes, index, prefix_len)
                && match_sequence
                    .as_ref()
                    .is_none_or(|current: &Osc8HyperlinkSequence| sequence.index < current.index)
            {
                match_sequence = Some(sequence);
            }
            offset = index.saturating_add(1);
        }
    }
    match_sequence
}

fn parse_osc8_hyperlink_sequence(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<Osc8HyperlinkSequence> {
    let content_start = index + prefix_len;
    let terminator = find_osc_color_terminator(&bytes[content_start..])?;

    Some(Osc8HyperlinkSequence {
        index,
        consumed: content_start + terminator.index + terminator.length - index,
    })
}

fn osc8_hyperlink_sequence_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_osc8_hyperlink_sequence_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_osc8_hyperlink_sequence_prefix(bytes: &[u8]) -> bool {
    [b"\x1b]8;".as_slice(), b"\x9d8;".as_slice()]
        .into_iter()
        .any(|prefix| {
            if prefix.starts_with(bytes) {
                return true;
            }
            bytes.starts_with(prefix) && find_osc_color_terminator(&bytes[prefix.len()..]).is_none()
        })
}

fn find_incomplete_osc8_hyperlink_start(bytes: &[u8]) -> Option<usize> {
    find_incomplete_prefixed_osc_start(bytes, [b"\x1b]8;".as_slice(), b"\x9d8;".as_slice()])
}

fn find_incomplete_osc52_clipboard_start(bytes: &[u8]) -> Option<usize> {
    find_incomplete_prefixed_osc_start(bytes, [b"\x1b]52;".as_slice(), b"\x9d52;".as_slice()])
}

fn find_incomplete_prefixed_osc_start(bytes: &[u8], prefixes: [&[u8]; 2]) -> Option<usize> {
    prefixes
        .into_iter()
        .filter_map(|prefix| find_incomplete_prefixed_sequence_start(bytes, prefix))
        .min()
}

fn find_incomplete_prefixed_sequence_start(bytes: &[u8], prefix: &[u8]) -> Option<usize> {
    if let Some(index) = find_subslice(bytes, prefix)
        && find_osc_color_terminator(&bytes[index + prefix.len()..]).is_none()
    {
        return Some(index);
    }

    let suffix = suffix_len_matching_prefix(bytes, prefix);
    (suffix > 0).then_some(bytes.len() - suffix)
}

fn find_incomplete_control_sequence_start(bytes: &[u8]) -> Option<usize> {
    [
        find_incomplete_osc_control_sequence_start(bytes),
        find_incomplete_osc8_hyperlink_start(bytes),
        find_incomplete_osc52_clipboard_start(bytes),
    ]
    .into_iter()
    .flatten()
    .min()
}

fn incomplete_osc_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_osc_control_sequence_start(bytes)
        .map_or(0, |start| bytes.len() - start)
        .max(suffix_len_matching_prefix(bytes, b"\x1b]"))
}

fn find_incomplete_osc_control_sequence_start(bytes: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((relative_index, prefix_len)) = find_next_osc_start(&bytes[offset..]) else {
            break;
        };
        let index = offset + relative_index;
        let content_start = index + prefix_len;
        let Some(terminator) = find_osc_color_terminator(&bytes[content_start..]) else {
            return Some(index);
        };
        offset = content_start + terminator.index + terminator.length;
    }

    None
}

struct Osc52ClipboardSequence {
    index: usize,
    consumed: usize,
    sequence: ClipboardSequence,
}

enum ClipboardSequence {
    Write(String),
    Query(String),
}

fn find_osc52_clipboard_sequence(bytes: &[u8]) -> Option<Osc52ClipboardSequence> {
    let mut match_sequence = None;
    for (prefix, prefix_len) in [(b"\x1b]52;".as_slice(), 5), (b"\x9d52;".as_slice(), 4)] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(sequence) = parse_osc52_clipboard_sequence(bytes, index, prefix_len) {
                if match_sequence
                    .as_ref()
                    .is_none_or(|current: &Osc52ClipboardSequence| sequence.index < current.index)
                {
                    match_sequence = Some(sequence);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_sequence
}

fn parse_osc52_clipboard_sequence(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<Osc52ClipboardSequence> {
    let content_start = index + prefix_len;
    let terminator = find_osc_color_terminator(&bytes[content_start..])?;
    let content_end = content_start + terminator.index;
    let sequence = parse_osc52_clipboard_content(&bytes[content_start..content_end])?;

    Some(Osc52ClipboardSequence {
        index,
        consumed: content_end + terminator.length - index,
        sequence,
    })
}

fn parse_osc52_clipboard_content(content: &[u8]) -> Option<ClipboardSequence> {
    let separator = content.iter().position(|byte| *byte == b';')?;
    let selection = String::from_utf8(content[..separator].to_vec()).ok()?;
    let payload = &content[separator + 1..];
    if payload == b"?" {
        return Some(ClipboardSequence::Query(selection));
    }

    let decoded = STANDARD.decode(payload).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    Some(ClipboardSequence::Write(text))
}

fn osc52_clipboard_sequence_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_osc52_clipboard_sequence_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_osc52_clipboard_sequence_prefix(bytes: &[u8]) -> bool {
    [b"\x1b]52;".as_slice(), b"\x9d52;".as_slice()]
        .into_iter()
        .any(|prefix| {
            if prefix.starts_with(bytes) {
                return true;
            }
            bytes.starts_with(prefix) && find_osc_color_terminator(&bytes[prefix.len()..]).is_none()
        })
}

struct DecrqssQuery {
    index: usize,
    consumed: usize,
    response: DecrqssResponse,
}

#[derive(Clone)]
struct DecrqssResponse {
    kind: Option<DecrqssKind>,
    terminator: OscResponseTerminator,
}

#[derive(Clone, Copy)]
enum DecrqssKind {
    Sgr,
    CursorShape,
    ScrollRegion,
}

impl DecrqssResponse {
    fn response(&self, terminal: &Terminal) -> Vec<u8> {
        let mut response = if let Some(kind) = self.kind {
            let mut bytes = b"\x1bP1$r".to_vec();
            match kind {
                DecrqssKind::Sgr => append_sgr_state(terminal.active_style(), &mut bytes),
                DecrqssKind::CursorShape => {
                    append_cursor_shape_state(terminal.cursor_shape(), &mut bytes);
                }
                DecrqssKind::ScrollRegion => {
                    append_scroll_region_state(terminal.scroll_region(), &mut bytes);
                }
            }
            bytes
        } else {
            b"\x1bP0$r".to_vec()
        };
        response.extend_from_slice(self.terminator.bytes());
        response
    }
}

fn find_decrqss_query(bytes: &[u8]) -> Option<DecrqssQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [(b"\x1bP".as_slice(), 2), (b"\x90".as_slice(), 1)] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_decrqss_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &DecrqssQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_decrqss_query(bytes: &[u8], index: usize, prefix_len: usize) -> Option<DecrqssQuery> {
    let content_start = index + prefix_len;
    let rest = bytes.get(content_start..)?;
    let body = rest.strip_prefix(b"$q")?;
    let terminator = find_xtgettcap_terminator(body)?;
    let content = &body[..terminator.index];

    Some(DecrqssQuery {
        index,
        consumed: prefix_len + b"$q".len() + terminator.index + terminator.length,
        response: DecrqssResponse {
            kind: parse_decrqss_kind(content),
            terminator: terminator.response_terminator,
        },
    })
}

fn parse_decrqss_kind(content: &[u8]) -> Option<DecrqssKind> {
    match content {
        b"m" => Some(DecrqssKind::Sgr),
        b" q" => Some(DecrqssKind::CursorShape),
        b"r" => Some(DecrqssKind::ScrollRegion),
        _ => None,
    }
}

fn append_sgr_state(style: &Cell, bytes: &mut Vec<u8>) {
    let mut params = Vec::new();
    if style.bold {
        params.push("1".to_owned());
    }
    if style.italic {
        params.push("3".to_owned());
    }
    if style.underline {
        params.push("4".to_owned());
    }
    if style.inverse {
        params.push("7".to_owned());
    }
    append_color_sgr(38, style.foreground, &mut params);
    append_color_sgr(48, style.background, &mut params);

    if params.is_empty() {
        bytes.push(b'0');
    } else {
        bytes.extend_from_slice(params.join(";").as_bytes());
    }
    bytes.push(b'm');
}

fn append_color_sgr(prefix: u8, color: Color, params: &mut Vec<String>) {
    match color {
        Color::Default => {}
        Color::Indexed(index) => {
            params.push(prefix.to_string());
            params.push("5".to_owned());
            params.push(index.to_string());
        }
        Color::Rgb(red, green, blue) => {
            params.push(prefix.to_string());
            params.push("2".to_owned());
            params.push(red.to_string());
            params.push(green.to_string());
            params.push(blue.to_string());
        }
    }
}

fn append_cursor_shape_state(shape: CursorShape, bytes: &mut Vec<u8>) {
    let value = match shape {
        CursorShape::Block => 2,
        CursorShape::Underline => 3,
        CursorShape::Bar => 5,
    };
    bytes.extend_from_slice(value.to_string().as_bytes());
    bytes.extend_from_slice(b" q");
}

fn append_scroll_region_state((top, bottom): (u16, u16), bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(top.saturating_add(1).to_string().as_bytes());
    bytes.push(b';');
    bytes.extend_from_slice(bottom.saturating_add(1).to_string().as_bytes());
    bytes.push(b'r');
}

fn decrqss_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_decrqss_query_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_decrqss_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1bP")
        .or_else(|| bytes.strip_prefix(b"\x90"))
    else {
        return b"\x1bP".starts_with(bytes) || b"\x90".starts_with(bytes);
    };
    if !b"$q".starts_with(rest) && !rest.starts_with(b"$q") {
        return false;
    }
    if let Some(body) = rest.strip_prefix(b"$q") {
        return [b"m".as_slice(), b" q".as_slice(), b"r".as_slice()]
            .into_iter()
            .any(|target| target.starts_with(body));
    }
    true
}

struct XtGetTcapQuery {
    index: usize,
    consumed: usize,
    response: XtGetTcapResponse,
}

#[derive(Clone)]
struct XtGetTcapResponse {
    entries: Vec<XtGetTcapEntry>,
    terminator: OscResponseTerminator,
}

#[derive(Clone)]
struct XtGetTcapEntry {
    name_hex: Vec<u8>,
    value_hex: Vec<u8>,
}

impl XtGetTcapResponse {
    fn response(&self) -> Vec<u8> {
        let mut response = if self.entries.is_empty() {
            b"\x1bP0+r".to_vec()
        } else {
            let mut bytes = b"\x1bP1+r".to_vec();
            for (index, entry) in self.entries.iter().enumerate() {
                if index > 0 {
                    bytes.push(b';');
                }
                bytes.extend_from_slice(&entry.name_hex);
                bytes.push(b'=');
                bytes.extend_from_slice(&entry.value_hex);
            }
            bytes
        };
        response.extend_from_slice(self.terminator.bytes());
        response
    }
}

fn find_xtgettcap_query(bytes: &[u8], size: PtySize) -> Option<XtGetTcapQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [(b"\x1bP".as_slice(), 2), (b"\x90".as_slice(), 1)] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_xtgettcap_query(bytes, index, prefix_len, size) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &XtGetTcapQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_xtgettcap_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
    size: PtySize,
) -> Option<XtGetTcapQuery> {
    let content_start = index + prefix_len;
    let rest = bytes.get(content_start..)?;
    let body = rest.strip_prefix(b"+q")?;
    let terminator = find_xtgettcap_terminator(body)?;
    let content = &body[..terminator.index];
    let entries = content
        .split(|byte| *byte == b';')
        .filter_map(|entry| parse_xtgettcap_entry(entry, size))
        .collect();

    Some(XtGetTcapQuery {
        index,
        consumed: prefix_len + b"+q".len() + terminator.index + terminator.length,
        response: XtGetTcapResponse {
            entries,
            terminator: terminator.response_terminator,
        },
    })
}

fn find_xtgettcap_terminator(bytes: &[u8]) -> Option<OscColorTerminator> {
    let st = find_subslice(bytes, b"\x1b\\").map(|index| OscColorTerminator {
        index,
        length: 2,
        response_terminator: OscResponseTerminator::St,
    });
    let c1_st = bytes
        .iter()
        .position(|byte| *byte == 0x9c)
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::C1St,
        });

    [st, c1_st]
        .into_iter()
        .flatten()
        .min_by_key(|terminator| terminator.index)
}

fn parse_xtgettcap_entry(name_hex: &[u8], size: PtySize) -> Option<XtGetTcapEntry> {
    let name = decode_ascii_hex(name_hex)?;
    let value_hex = xtgettcap_value_hex(&name, size)?;
    Some(XtGetTcapEntry {
        name_hex: name_hex.to_vec(),
        value_hex,
    })
}

fn xtgettcap_value_hex(name: &[u8], size: PtySize) -> Option<Vec<u8>> {
    match name {
        b"Co" | b"colors" => Some(b"323536".to_vec()),
        b"TN" => Some(b"787465726d2d323536636f6c6f72".to_vec()),
        b"RGB" => Some(b"524742".to_vec()),
        b"Ms" => Some(b"1b5d35323b25703125733b257032257307".to_vec()),
        b"co" => Some(decimal_value_hex(size.columns())),
        b"li" => Some(decimal_value_hex(size.rows())),
        _ => None,
    }
}

fn decimal_value_hex(value: u16) -> Vec<u8> {
    encode_ascii_hex(value.to_string().as_bytes())
}

fn encode_ascii_hex(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = Vec::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)]);
        encoded.push(HEX[usize::from(byte & 0x0f)]);
    }
    encoded
}

fn decode_ascii_hex(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| Some(parse_hex_digit(pair[0])? * 16 + parse_hex_digit(pair[1])?))
        .collect()
}

fn xtgettcap_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_xtgettcap_query_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_xtgettcap_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1bP")
        .or_else(|| bytes.strip_prefix(b"\x90"))
    else {
        return b"\x1bP".starts_with(bytes) || b"\x90".starts_with(bytes);
    };
    if !b"+q".starts_with(rest) && !rest.starts_with(b"+q") {
        return false;
    }
    if let Some(body) = rest.strip_prefix(b"+q") {
        return body
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() || *byte == b';');
    }
    true
}

struct OscColorQuery {
    index: usize,
    consumed: usize,
    query: OscColorResponse,
}

#[derive(Clone, Copy)]
struct OscColorResponse {
    kind: OscColorKind,
    terminator: OscResponseTerminator,
}

#[derive(Clone, Copy)]
enum OscColorKind {
    DefaultForeground,
    DefaultBackground,
    Palette(u8),
}

#[derive(Clone, Copy)]
enum OscResponseTerminator {
    Bel,
    St,
    C1St,
}

struct OscColorTerminator {
    index: usize,
    length: usize,
    response_terminator: OscResponseTerminator,
}

fn find_osc_color_query(bytes: &[u8]) -> Option<OscColorQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [(b"\x1b]".as_slice(), 2), (b"\x9d".as_slice(), 1)] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_osc_color_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &OscColorQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_osc_color_query(bytes: &[u8], index: usize, prefix_len: usize) -> Option<OscColorQuery> {
    let content_start = index + prefix_len;
    let terminator = find_osc_color_terminator(&bytes[content_start..])?;
    let content_end = content_start + terminator.index;
    let kind = parse_osc_color_query_content(&bytes[content_start..content_end])?;

    Some(OscColorQuery {
        index,
        consumed: content_end + terminator.length - index,
        query: OscColorResponse {
            kind,
            terminator: terminator.response_terminator,
        },
    })
}

fn find_osc_color_terminator(bytes: &[u8]) -> Option<OscColorTerminator> {
    let bel = bytes
        .iter()
        .position(|byte| *byte == b'\x07')
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::Bel,
        });
    let st = find_subslice(bytes, b"\x1b\\").map(|index| OscColorTerminator {
        index,
        length: 2,
        response_terminator: OscResponseTerminator::St,
    });
    let c1_st = bytes
        .iter()
        .position(|byte| *byte == 0x9c)
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::C1St,
        });

    [bel, st, c1_st]
        .into_iter()
        .flatten()
        .min_by_key(|terminator| terminator.index)
}

fn parse_osc_color_query_content(content: &[u8]) -> Option<OscColorKind> {
    match content {
        b"10;?" => Some(OscColorKind::DefaultForeground),
        b"11;?" => Some(OscColorKind::DefaultBackground),
        _ => parse_palette_color_query(content),
    }
}

fn parse_palette_color_query(content: &[u8]) -> Option<OscColorKind> {
    let rest = content.strip_prefix(b"4;")?;
    let separator = rest.iter().position(|byte| *byte == b';')?;
    if &rest[separator + 1..] != b"?" {
        return None;
    }
    let index = parse_u8_decimal(&rest[..separator])?;
    Some(OscColorKind::Palette(index))
}

fn parse_u8_decimal(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0u16;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(u16::from(*byte - b'0'));
    }
    u8::try_from(value).ok()
}

fn osc_color_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_osc_color_query_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_osc_color_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b]")
        .or_else(|| bytes.strip_prefix(b"\x9d"))
    else {
        return b"\x1b]".starts_with(bytes) || b"\x9d".starts_with(bytes);
    };

    b"10;?".starts_with(rest) || b"11;?".starts_with(rest) || is_palette_color_query_prefix(rest)
}

fn is_palette_color_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(b"4") else {
        return bytes.is_empty();
    };
    let Some(rest) = rest.strip_prefix(b";") else {
        return rest.is_empty();
    };
    let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return rest.is_empty();
    }
    let tail = &rest[digits..];
    tail.is_empty() || tail == b";" || tail == b";?"
}

struct TerminalColorState {
    foreground: [u8; 3],
    background: [u8; 3],
    palette_overrides: Vec<(u8, [u8; 3])>,
    pending: Vec<u8>,
}

impl Default for TerminalColorState {
    fn default() -> Self {
        Self {
            foreground: DEFAULT_FOREGROUND,
            background: DEFAULT_BACKGROUND,
            palette_overrides: Vec::new(),
            pending: Vec::new(),
        }
    }
}

impl TerminalColorState {
    const MAX_PENDING: usize = 1024 * 1024;

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
            return;
        }

        loop {
            let Some((index, prefix_len)) = find_next_osc_start(&self.pending) else {
                self.retain_possible_prefix();
                return;
            };
            if index > 0 {
                self.pending.drain(..index);
            }

            let content_start = prefix_len;
            let Some(terminator) = find_osc_color_terminator(&self.pending[content_start..]) else {
                return;
            };
            let content_end = content_start + terminator.index;
            if let Some(change) = parse_osc_color_change(&self.pending[content_start..content_end])
            {
                self.apply(change);
            }
            self.pending.drain(..content_end + terminator.length);
        }
    }

    fn response(&self, query: OscColorResponse) -> Vec<u8> {
        let mut response = match query.kind {
            OscColorKind::DefaultForeground => {
                format!("\x1b]10;{}", rgb_response(self.foreground)).into_bytes()
            }
            OscColorKind::DefaultBackground => {
                format!("\x1b]11;{}", rgb_response(self.background)).into_bytes()
            }
            OscColorKind::Palette(index) => format!(
                "\x1b]4;{};{}",
                index,
                rgb_response(self.palette_color(index))
            )
            .into_bytes(),
        };
        response.extend_from_slice(query.terminator.bytes());
        response
    }

    fn apply(&mut self, change: OscColorChange) {
        match change {
            OscColorChange::DefaultForeground(color) => self.foreground = color,
            OscColorChange::DefaultBackground(color) => self.background = color,
            OscColorChange::Palette(index, color) => {
                if let Some((_, existing)) = self
                    .palette_overrides
                    .iter_mut()
                    .find(|(palette_index, _)| *palette_index == index)
                {
                    *existing = color;
                } else {
                    self.palette_overrides.push((index, color));
                }
            }
        }
    }

    fn palette_color(&self, index: u8) -> [u8; 3] {
        self.palette_overrides
            .iter()
            .find_map(|(palette_index, color)| (*palette_index == index).then_some(*color))
            .unwrap_or_else(|| indexed_color(index))
    }

    fn retain_possible_prefix(&mut self) {
        let retained = [b"\x1b]".as_slice(), b"\x9d".as_slice()]
            .into_iter()
            .map(|prefix| suffix_len_matching_prefix(&self.pending, prefix))
            .max()
            .unwrap_or(0);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

#[derive(Clone, Copy)]
enum OscColorChange {
    DefaultForeground([u8; 3]),
    DefaultBackground([u8; 3]),
    Palette(u8, [u8; 3]),
}

fn find_next_osc_start(bytes: &[u8]) -> Option<(usize, usize)> {
    [(b"\x1b]".as_slice(), 2), (b"\x9d".as_slice(), 1)]
        .into_iter()
        .filter_map(|(prefix, prefix_len)| {
            find_subslice(bytes, prefix).map(|index| (index, prefix_len))
        })
        .min_by_key(|(index, _)| *index)
}

fn parse_osc_color_change(content: &[u8]) -> Option<OscColorChange> {
    if let Some(color) = content.strip_prefix(b"10;").and_then(parse_rgb_color_spec) {
        return Some(OscColorChange::DefaultForeground(color));
    }
    if let Some(color) = content.strip_prefix(b"11;").and_then(parse_rgb_color_spec) {
        return Some(OscColorChange::DefaultBackground(color));
    }
    parse_palette_color_change(content)
}

fn parse_palette_color_change(content: &[u8]) -> Option<OscColorChange> {
    let rest = content.strip_prefix(b"4;")?;
    let separator = rest.iter().position(|byte| *byte == b';')?;
    let index = parse_u8_decimal(&rest[..separator])?;
    let color = parse_rgb_color_spec(&rest[separator + 1..])?;
    Some(OscColorChange::Palette(index, color))
}

fn parse_rgb_color_spec(value: &[u8]) -> Option<[u8; 3]> {
    let rest = value.strip_prefix(b"rgb:")?;
    let mut components = rest.split(|byte| *byte == b'/');
    let red = parse_rgb_component(components.next()?)?;
    let green = parse_rgb_component(components.next()?)?;
    let blue = parse_rgb_component(components.next()?)?;
    components.next().is_none().then_some([red, green, blue])
}

fn parse_rgb_component(component: &[u8]) -> Option<u8> {
    match component.len() {
        1 => parse_hex_digit(component[0]).map(|value| value * 17),
        2..=4 => parse_hex_byte(&component[..2]),
        _ => None,
    }
}

fn parse_hex_byte(bytes: &[u8]) -> Option<u8> {
    Some(parse_hex_digit(bytes[0])? * 16 + parse_hex_digit(bytes[1])?)
}

fn parse_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl OscResponseTerminator {
    const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Bel => b"\x07",
            Self::St => b"\x1b\\",
            Self::C1St => b"\x9c",
        }
    }
}

const DEFAULT_FOREGROUND: [u8; 3] = [229, 229, 229];
const DEFAULT_BACKGROUND: [u8; 3] = [12, 12, 12];

fn rgb_response(color: [u8; 3]) -> String {
    format!(
        "rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}",
        color[0], color[1], color[2]
    )
}

fn indexed_color(index: u8) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];

    if let Some(color) = ANSI.get(usize::from(index)) {
        return *color;
    }

    if (16..=231).contains(&index) {
        let cube_index = index - 16;
        return [
            xterm_color_cube_intensity(cube_index / 36),
            xterm_color_cube_intensity((cube_index / 6) % 6),
            xterm_color_cube_intensity(cube_index % 6),
        ];
    }

    let level = 8 + (index - 232) * 10;
    [level, level, level]
}

const fn xterm_color_cube_intensity(value: u8) -> u8 {
    if value == 0 { 0 } else { 55 + value * 40 }
}

struct PrivateModeStatusQuery {
    index: usize,
    consumed: usize,
    mode: u16,
}

fn find_private_mode_status_query(bytes: &[u8]) -> Option<PrivateModeStatusQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [
        (b"\x1b[?".as_slice(), b"\x1b[?".len()),
        (b"\x9b?".as_slice(), b"\x9b?".len()),
    ] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_private_mode_status_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &PrivateModeStatusQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_private_mode_status_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<PrivateModeStatusQuery> {
    let mut cursor = index + prefix_len;
    let start = cursor;
    let mut mode = 0u16;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
        mode = mode
            .saturating_mul(10)
            .saturating_add(u16::from(bytes[cursor] - b'0'));
        cursor += 1;
    }
    if cursor == start || bytes.get(cursor..cursor + 2) != Some(b"$p") {
        return None;
    }
    Some(PrivateModeStatusQuery {
        index,
        consumed: cursor + 2 - index,
        mode,
    })
}

fn private_mode_status_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_private_mode_status_query_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_private_mode_status_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b[?")
        .or_else(|| bytes.strip_prefix(b"\x9b?"))
    else {
        return b"\x1b[".starts_with(bytes)
            || b"\x1b[?".starts_with(bytes)
            || b"\x9b?".starts_with(bytes);
    };

    let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return rest.is_empty();
    }
    let tail = &rest[digits..];
    tail.is_empty() || tail == b"$"
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
        InputModes, Osc52Policy, TerminalOutputFilter, encode_input_event, encode_key,
        resolve_local_size,
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
    fn terminal_output_filter_omits_osc8_hyperlink_sequences() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"a\x1b]8;;https://example.com\x1b\\bc\x1b]8;;\x1b\\d",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"abcd");
        assert!(responses.is_empty());
        assert_eq!(
            filter.mirror.grid().get(0, 1).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            filter.mirror.grid().get(0, 2).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(filter.mirror.grid().get(0, 0).unwrap().hyperlink, None);
        assert_eq!(filter.mirror.grid().get(0, 3).unwrap().hyperlink, None);
    }

    #[test]
    fn terminal_output_filter_omits_split_osc8_hyperlink_sequences() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"a\x1b]8;;https://example.com", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\x1b\\bc\x1b]8;;", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\x1b\\d", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"abcd");
        assert!(responses.is_empty());
        assert_eq!(
            filter.mirror.grid().get(0, 1).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            filter.mirror.grid().get(0, 2).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(filter.mirror.grid().get(0, 3).unwrap().hyperlink, None);
    }

    #[test]
    fn terminal_output_filter_drops_incomplete_osc8_on_flush() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"a\x1b]8;;https://example.com", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"a");
        assert!(responses.is_empty());
        assert_eq!(filter.mirror.grid().get(0, 0).unwrap().ch, 'a');
        assert_eq!(filter.mirror.grid().get(0, 0).unwrap().hyperlink, None);
    }

    #[test]
    fn terminal_output_filter_drops_partial_osc8_prefix_on_flush() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"a\x1b]8", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"a");
        assert!(responses.is_empty());
        assert_eq!(filter.mirror.grid().get(0, 0).unwrap().ch, 'a');
    }

    #[test]
    fn terminal_output_filter_holds_split_osc_title_until_terminated() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b]0;op", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());
        assert_eq!(filter.mirror.title(), None);

        filter
            .write(b"s\x07after", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before\x1b]0;ops\x07after");
        assert!(responses.is_empty());
        assert_eq!(filter.mirror.title(), Some("ops"));
    }

    #[test]
    fn terminal_output_filter_drops_incomplete_osc_title_on_flush() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b]0;ops", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());
        assert_eq!(filter.mirror.title(), None);
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
    fn terminal_output_filter_answers_window_position_query() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[13tafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[3;0;0t");
    }

    #[test]
    fn terminal_output_filter_answers_screen_pixel_size_query() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[15tafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[5;688;1056t");
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
    fn terminal_output_filter_answers_window_state_query() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[11tafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[1t");
    }

    #[test]
    fn terminal_output_filter_answers_window_title_queries() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b]0;ops\x07before\x1b[20t middle\x1b[21tafter",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x1b]0;ops\x07before middleafter");
        assert_eq!(responses, b"\x1b]Lops\x1b\\\x1b]lops\x1b\\");
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
    fn terminal_output_filter_answers_c1_window_position_and_screen_pixel_size_queries() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x9b13t middle\x9b15tafter",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middleafter");
        assert_eq!(responses, b"\x1b[3;0;0t\x1b[5;688;1056t");
    }

    #[test]
    fn terminal_output_filter_answers_c1_window_state_and_title_queries() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b]0;ops\x07before\x9b11t middle\x9b20t after\x9b21t",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x1b]0;ops\x07before middle after");
        assert_eq!(responses, b"\x1b[1t\x1b]Lops\x1b\\\x1b]lops\x1b\\");
    }

    #[test]
    fn terminal_output_filter_answers_private_mode_status_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b[?1h\x1b[?1$p middle\x1b[?1004$p after\x1b[?9999$p",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before\x1b[?1h middle after");
        assert_eq!(responses, b"\x1b[?1;1$y\x1b[?1004;2$y\x1b[?9999;0$y");
    }

    #[test]
    fn terminal_output_filter_answers_osc_color_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b]10;?\x07 middle\x1b]11;?\x1b\\ after\x1b]4;1;?\x07done",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middle afterdone");
        assert_eq!(
            responses,
            b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\\x1b]4;1;rgb:cdcd/3131/3131\x07"
        );
    }

    #[test]
    fn terminal_output_filter_answers_c1_osc_color_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"\x9d4;196;?\x9c", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert!(output.is_empty());
        assert_eq!(responses, b"\x1b]4;196;rgb:ffff/0000/0000\x9c");
    }

    #[test]
    fn terminal_output_filter_answers_osc_color_queries_after_color_changes() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b]10;rgb:11/22/33\x07 middle\x1b]10;?\x07 after\x1b]4;1;rgb:01/02/03\x1b\\ done\x1b]4;1;?\x1b\\",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            output,
            b"before\x1b]10;rgb:11/22/33\x07 middle after\x1b]4;1;rgb:01/02/03\x1b\\ done"
        );
        assert_eq!(
            responses,
            b"\x1b]10;rgb:1111/2222/3333\x07\x1b]4;1;rgb:0101/0202/0303\x1b\\"
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1bP+q436f\x1b\\ middle\x90+q544e;524742\x9c after\x1bP+q626164\x1b\\done",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middle afterdone");
        assert_eq!(
            responses,
            b"\x1bP1+r436f=323536\x1b\\\x1bP1+r544e=787465726d2d323536636f6c6f72;524742=524742\x9c\x1bP0+r\x1b\\"
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_size_queries_from_current_size() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1bP+q636f;6c69\x1b\\after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1bP1+r636f=313332;6c69=3433\x1b\\");
    }

    #[test]
    fn terminal_output_filter_answers_decrqss_state_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b[1;4;38;5;196;48;2;1;2;3m\x1bP$qm\x1b\\ middle\x1b[5 q\x90$q q\x9c after\x1b[2;5r\x1bP$qr\x1b\\done",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            output,
            b"before\x1b[1;4;38;5;196;48;2;1;2;3m middle\x1b[5 q after\x1b[2;5rdone"
        );
        assert_eq!(
            responses,
            b"\x1bP1$r1;4;38;5;196;48;2;1;2;3m\x1b\\\x1bP1$r5 q\x9c\x1bP1$r2;5r\x1b\\"
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtversion_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b[>q middle\x1b[>0q after\x9b>q done",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middle after done");
        assert_eq!(
            responses,
            b"\x1bP>|R-SSH 0.1.0\x1b\\\x1bP>|R-SSH 0.1.0\x1b\\\x1bP>|R-SSH 0.1.0\x1b\\"
        );
    }

    #[test]
    fn terminal_output_filter_writes_osc52_clipboard_text() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x1b]52;c;Y29weQ==\x07after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
                |text| {
                    writes.push(text.to_owned());
                    true
                },
                || Some("ignored".to_owned()),
                Osc52Policy::ReadWrite,
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert!(responses.is_empty());
        assert_eq!(writes, vec!["copy"]);
    }

    #[test]
    fn terminal_output_filter_answers_osc52_clipboard_query() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x1b]52;c;?\x07after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
                |_| true,
                || Some("copy".to_owned()),
                Osc52Policy::ReadWrite,
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b]52;c;Y29weQ==\x07");
    }

    #[test]
    fn terminal_output_filter_drops_incomplete_osc52_on_flush() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x1b]52;c;Y29weQ==",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
                |text| {
                    writes.push(text.to_owned());
                    true
                },
                || Some("copy".to_owned()),
                Osc52Policy::ReadWrite,
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());
        assert!(writes.is_empty());
    }

    #[test]
    fn terminal_output_filter_drops_partial_osc52_prefix_on_flush() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x1b]52",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
                |text| {
                    writes.push(text.to_owned());
                    true
                },
                || Some("copy".to_owned()),
                Osc52Policy::ReadWrite,
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());
        assert!(writes.is_empty());
    }

    #[test]
    fn terminal_output_filter_blocks_osc52_when_policy_is_off() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x1b]52;c;Y29weQ==\x07 middle\x1b]52;c;?\x07after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
                |text| {
                    writes.push(text.to_owned());
                    true
                },
                || Some("copy".to_owned()),
                Osc52Policy::Off,
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middleafter");
        assert!(responses.is_empty());
        assert!(writes.is_empty());
    }

    #[test]
    fn terminal_output_filter_write_only_osc52_policy_blocks_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x1b]52;c;Y29weQ==\x07 middle\x1b]52;c;?\x07after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
                |text| {
                    writes.push(text.to_owned());
                    true
                },
                || Some("copy".to_owned()),
                Osc52Policy::WriteOnly,
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middleafter");
        assert!(responses.is_empty());
        assert_eq!(writes, vec!["copy"]);
    }

    #[test]
    fn terminal_output_filter_answers_c1_private_mode_status_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x9b?1000;1006h\x1b[?2004h\x9b?1000$p \x9b?1006$p \x9b?2004$p",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x9b?1000;1006h\x1b[?2004h  ");
        assert_eq!(responses, b"\x1b[?1000;1$y\x1b[?1006;1$y\x1b[?2004;1$y");
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
