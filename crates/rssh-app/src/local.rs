use std::{
    error::Error,
    fs::File,
    io::{self, IsTerminal, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU64, Ordering},
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
        KeyEventState, KeyModifiers, MediaKeyCode, ModifierKeyCode, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute, terminal,
};
use rssh_core::{
    SessionId, TerminalSize,
    session::{SessionLifecycle, SessionState},
};
use rssh_pty::{PtyBackend, PtyExitStatus, PtySession, PtySize};
use rssh_terminal::{Cell, Color, CursorShape, Terminal, UnderlineStyle, VerticalAlign};
use serde::Serialize;

use crate::{
    cli::{LocalOptions, Osc52Policy},
    diagnostics,
    terminal_input::{TerminalKey, encode_terminal_key},
    terminal_modes::{
        KITTY_KEYBOARD_ALTERNATE_KEYS, KITTY_KEYBOARD_ASSOCIATED_TEXT, KITTY_KEYBOARD_DISAMBIGUATE,
        KITTY_KEYBOARD_REPORT_ALL, KITTY_KEYBOARD_REPORT_EVENTS, KeyModifierOptionsQuery,
        KeyModifierOptionsSequence, KittyKeyboardFlagsQuery, KittyKeyboardModeSequence,
        MouseInputMode, MouseProtocolMode, MouseReportingMode, SynchronizedOutputModeSequence,
        TerminalModeChange, TerminalModeTracker, find_key_modifier_options_query,
        find_key_modifier_options_sequence, find_kitty_keyboard_flags_query,
        find_kitty_keyboard_mode_sequence, find_synchronized_output_mode_sequence,
        key_modifier_options_query_suffix_len, key_modifier_options_sequence_suffix_len,
        kitty_keyboard_flags_query_suffix_len, kitty_keyboard_mode_sequence_suffix_len,
        synchronized_output_mode_sequence_suffix_len,
    },
    visible_output::TerminalVisibleOutputFilter,
};

const LOCAL_CONSOLE_SESSION_ID: SessionId = SessionId::new(1);

pub fn run(options: &LocalOptions) -> Result<PtyExitStatus, Box<dyn Error>> {
    if options.console.preflight {
        diagnostics::ensure_console_dependencies()?;
    }

    let metrics_started_at = Instant::now();
    let size = resolve_local_size(options.size);
    let mut lifecycle = SessionLifecycle::new(LOCAL_CONSOLE_SESSION_ID);
    lifecycle.start_connecting()?;
    let mut session = PtySession::spawn(&options.command, size)?;
    lifecycle.mark_connected()?;
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
    let runtime_state = LocalRuntimeState::new(size, options.mouse);
    let metrics = LocalMetricsCounters::default();
    let output_terminal_size = runtime_state.terminal_size.clone();
    let output_metrics = metrics.clone();
    let input_metrics = metrics.clone();
    let osc52_policy = options.osc52_policy;

    let _reader_thread = thread::spawn(move || {
        let result = copy_pty_output(
            &mut reader,
            &terminal_response_sender,
            &output_control_sender,
            output_terminal_size,
            &output_metrics,
            osc52_policy,
            log_file.as_mut().map(|file| file as &mut dyn Write),
        );
        let _ = reader_done_sender.send(result);
    });
    let _writer_thread = thread::spawn(move || {
        let result = copy_pty_input(&mut writer, &pty_input_receiver, &input_metrics);
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
        &metrics,
    );

    drop(pty_input_sender);

    if run_result.is_ok() {
        lifecycle.mark_disconnected()?;
        lifecycle.close()?;
    }

    let session_state = lifecycle.state();
    drop(session);

    if options.console.metrics_json {
        if let Ok(status) = &run_result {
            println!(
                "{}",
                LocalMetricsSnapshot::from_status(
                    &options.command,
                    size,
                    metrics.snapshot(),
                    metrics_started_at.elapsed(),
                    session_state,
                    status
                )
                .json_report()?
            );
        }
    } else if options.console.metrics {
        if let Ok(status) = &run_result {
            print!(
                "{}",
                LocalMetricsSnapshot::from_status(
                    &options.command,
                    size,
                    metrics.snapshot(),
                    metrics_started_at.elapsed(),
                    session_state,
                    status
                )
                .report()
            );
        }
    }

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
    SetKittyKeyboardFlags(u16),
    SetModifyOtherKeys(u8),
}

#[derive(Clone, Default)]
struct InputReporting {
    application_cursor_keys: Arc<AtomicBool>,
    application_keypad: Arc<AtomicBool>,
    bracketed_paste: Arc<AtomicBool>,
    mouse: Arc<AtomicU8>,
    focus: Arc<AtomicBool>,
    kitty_keyboard_flags: Arc<AtomicU16>,
    modify_other_keys: Arc<AtomicU8>,
}

impl InputReporting {
    fn snapshot(&self) -> InputModes {
        InputModes::default()
            .with_application_cursor_keys(self.application_cursor_keys_enabled())
            .with_application_keypad(self.application_keypad_enabled())
            .with_bracketed_paste(self.bracketed_paste_enabled())
            .with_mouse_input_mode(self.mouse_input_mode())
            .with_focus_reporting(self.focus_enabled())
            .with_kitty_keyboard_flags(self.kitty_keyboard_flags())
            .with_modify_other_keys(self.modify_other_keys())
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

    fn kitty_keyboard_flags(&self) -> u16 {
        self.kitty_keyboard_flags.load(Ordering::Relaxed)
    }

    fn modify_other_keys(&self) -> u8 {
        self.modify_other_keys.load(Ordering::Relaxed)
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

    fn set_kitty_keyboard_flags(&self, flags: u16) {
        self.kitty_keyboard_flags.store(flags, Ordering::Relaxed);
    }

    fn set_modify_other_keys(&self, mode: u8) {
        self.modify_other_keys.store(mode, Ordering::Relaxed);
    }
}

struct LocalRuntimeState {
    input_reporting: InputReporting,
    terminal_size: SharedTerminalSize,
    allow_application_reporting: bool,
}

impl LocalRuntimeState {
    fn new(size: PtySize, allow_application_reporting: bool) -> Self {
        Self {
            input_reporting: InputReporting::default(),
            terminal_size: SharedTerminalSize::new(size),
            allow_application_reporting,
        }
    }
}

#[derive(Clone, Default)]
struct LocalMetricsCounters {
    pty_input_bytes: Arc<AtomicU64>,
    pty_output_bytes: Arc<AtomicU64>,
    terminal_output_bytes: Arc<AtomicU64>,
    resize_events: Arc<AtomicU64>,
}

impl LocalMetricsCounters {
    fn add_pty_input(&self, bytes: u64) {
        self.pty_input_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn add_pty_output(&self, bytes: u64) {
        self.pty_output_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn add_terminal_output(&self, bytes: u64) {
        self.terminal_output_bytes
            .fetch_add(bytes, Ordering::Relaxed);
    }

    fn add_resize_event(&self) {
        self.resize_events.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> LocalMetricsCountersSnapshot {
        LocalMetricsCountersSnapshot {
            pty_input_bytes: self.pty_input_bytes.load(Ordering::Relaxed),
            pty_output_bytes: self.pty_output_bytes.load(Ordering::Relaxed),
            terminal_output_bytes: self.terminal_output_bytes.load(Ordering::Relaxed),
            resize_events: self.resize_events.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy)]
struct LocalMetricsCountersSnapshot {
    pty_input_bytes: u64,
    pty_output_bytes: u64,
    terminal_output_bytes: u64,
    resize_events: u64,
}

#[derive(Serialize)]
struct LocalMetricsSnapshot {
    command: String,
    backend: String,
    columns: u16,
    rows: u16,
    session_state: String,
    pty_input_bytes: u64,
    pty_output_bytes: u64,
    terminal_output_bytes: u64,
    resize_events: u64,
    elapsed_ms: u128,
    exit_code: u32,
    signal: Option<String>,
    success: bool,
}

impl LocalMetricsSnapshot {
    fn from_status(
        command: &rssh_pty::PtyCommand,
        size: PtySize,
        counters: LocalMetricsCountersSnapshot,
        elapsed: Duration,
        session_state: SessionState,
        status: &PtyExitStatus,
    ) -> Self {
        Self {
            command: command_line(command),
            backend: format!("{:?}", PtyBackend::current_platform()),
            columns: size.columns(),
            rows: size.rows(),
            session_state: session_state.as_str().to_owned(),
            pty_input_bytes: counters.pty_input_bytes,
            pty_output_bytes: counters.pty_output_bytes,
            terminal_output_bytes: counters.terminal_output_bytes,
            resize_events: counters.resize_events,
            elapsed_ms: elapsed.as_millis(),
            exit_code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
            success: status.success(),
        }
    }

    fn report(&self) -> String {
        format!(
            "\
R-SSH console metrics
command={}
backend={}
columns={}
rows={}
session_state={}
pty_input_bytes={}
pty_output_bytes={}
terminal_output_bytes={}
resize_events={}
elapsed_ms={}
exit_code={}
signal={}
success={}
",
            self.command,
            self.backend,
            self.columns,
            self.rows,
            self.session_state,
            self.pty_input_bytes,
            self.pty_output_bytes,
            self.terminal_output_bytes,
            self.resize_events,
            self.elapsed_ms,
            self.exit_code,
            self.signal.as_deref().unwrap_or("none"),
            self.success
        )
    }

    fn json_report(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

fn command_line(command: &rssh_pty::PtyCommand) -> String {
    std::iter::once(command.program())
        .chain(command.args().iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod metrics_tests {
    use rssh_core::session::SessionState;
    use rssh_pty::{PtyBackend, PtyExitStatus};

    #[test]
    fn console_metrics_report_includes_command_timing_and_exit_status() {
        let command = rssh_pty::PtyCommand::new("cmd.exe").with_args(["/C", "echo hi"]);
        let report = super::LocalMetricsSnapshot::from_status(
            &command,
            rssh_pty::PtySize::try_new(100, 30).unwrap(),
            super::LocalMetricsCountersSnapshot {
                pty_input_bytes: 3,
                pty_output_bytes: 8,
                terminal_output_bytes: 5,
                resize_events: 2,
            },
            std::time::Duration::from_millis(42),
            SessionState::Closed,
            &PtyExitStatus::from_exit_code(0),
        )
        .report();

        let expected_backend = format!("{:?}", PtyBackend::current_platform());

        assert_eq!(
            report,
            format!(
                "R-SSH console metrics\n\
command=cmd.exe /C echo hi\n\
backend={expected_backend}\n\
columns=100\n\
rows=30\n\
session_state=closed\n\
pty_input_bytes=3\n\
pty_output_bytes=8\n\
terminal_output_bytes=5\n\
resize_events=2\n\
elapsed_ms=42\n\
exit_code=0\n\
signal=none\n\
success=true\n"
            )
        );
    }

    #[test]
    fn console_metrics_json_report_is_machine_readable() {
        let command = rssh_pty::PtyCommand::new("cmd.exe").with_args(["/C", "echo hi"]);
        let report = super::LocalMetricsSnapshot::from_status(
            &command,
            rssh_pty::PtySize::try_new(100, 30).unwrap(),
            super::LocalMetricsCountersSnapshot {
                pty_input_bytes: 3,
                pty_output_bytes: 8,
                terminal_output_bytes: 5,
                resize_events: 2,
            },
            std::time::Duration::from_millis(42),
            SessionState::Closed,
            &PtyExitStatus::from_exit_code(0),
        )
        .json_report()
        .unwrap();

        let expected_backend = format!("{:?}", PtyBackend::current_platform());

        assert_eq!(
            report,
            format!(
                "{{\"command\":\"cmd.exe /C echo hi\",\"backend\":\"{expected_backend}\",\"columns\":100,\"rows\":30,\"session_state\":\"closed\",\"pty_input_bytes\":3,\"pty_output_bytes\":8,\"terminal_output_bytes\":5,\"resize_events\":2,\"elapsed_ms\":42,\"exit_code\":0,\"signal\":null,\"success\":true}}"
            )
        );
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
    metrics: &LocalMetricsCounters,
    osc52_policy: Osc52Policy,
    log: Option<&mut dyn Write>,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut output = SessionLogWriter::new(&mut stdout, log, metrics.clone());
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
                metrics.add_pty_output(count as u64);
                mode_tracker.process(&buffer[..count], |change| {
                    let Some(event) = (match change {
                        TerminalModeChange::ApplicationCursorKeys(enabled) => {
                            Some(LocalControlEvent::SetApplicationCursorKeys(enabled))
                        }
                        TerminalModeChange::ApplicationKeypad(enabled) => {
                            Some(LocalControlEvent::SetApplicationKeypad(enabled))
                        }
                        TerminalModeChange::BracketedPaste(enabled) => {
                            Some(LocalControlEvent::SetBracketedPaste(enabled))
                        }
                        TerminalModeChange::Mouse(mode) => {
                            Some(LocalControlEvent::SetMouseReporting(mode))
                        }
                        TerminalModeChange::Focus(enabled) => {
                            Some(LocalControlEvent::SetFocusReporting(enabled))
                        }
                        TerminalModeChange::KittyKeyboardFlags(flags) => {
                            Some(LocalControlEvent::SetKittyKeyboardFlags(flags))
                        }
                        TerminalModeChange::ModifyOtherKeys(mode) => {
                            Some(LocalControlEvent::SetModifyOtherKeys(mode))
                        }
                        TerminalModeChange::SynchronizedOutput(_) => None,
                    }) else {
                        return;
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
    metrics: LocalMetricsCounters,
}

impl<'screen, 'log> SessionLogWriter<'screen, 'log> {
    fn new(
        screen: &'screen mut dyn Write,
        log: Option<&'log mut dyn Write>,
        metrics: LocalMetricsCounters,
    ) -> Self {
        Self {
            screen,
            log,
            log_filter: TerminalVisibleOutputFilter::default(),
            metrics,
        }
    }
}

impl Write for SessionLogWriter<'_, '_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.screen.write(buffer)?;
        if count > 0 {
            self.metrics.add_terminal_output(count as u64);
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
    metrics: &LocalMetricsCounters,
) -> io::Result<()> {
    for bytes in pty_input_receiver {
        writer.write_all(&bytes)?;
        metrics.add_pty_input(bytes.len() as u64);
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
    metrics: &LocalMetricsCounters,
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
                    metrics.add_resize_event();
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
                    let mode = if runtime_state.allow_application_reporting
                        && raw_mode.set_mouse_capture(mode.reporting_enabled())?
                    {
                        mode
                    } else {
                        mode.with_reporting(MouseReportingMode::None)
                    };
                    runtime_state.input_reporting.set_mouse(mode);
                }
                LocalControlEvent::SetFocusReporting(enabled) => {
                    let enabled = if runtime_state.allow_application_reporting {
                        raw_mode.set_focus_change(enabled)?
                    } else {
                        false
                    };
                    runtime_state.input_reporting.set_focus(enabled);
                }
                LocalControlEvent::SetKittyKeyboardFlags(flags) => {
                    runtime_state
                        .input_reporting
                        .set_kitty_keyboard_flags(flags);
                }
                LocalControlEvent::SetModifyOtherKeys(mode) => {
                    runtime_state.input_reporting.set_modify_other_keys(mode);
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
    synchronized_output_buffer: Vec<u8>,
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
            synchronized_output_buffer: Vec::new(),
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
        self.pending.extend_from_slice(bytes);

        while let Some((index, event)) = self.find_next_event() {
            let prefix = self.pending[..index].to_vec();
            self.write_visible_bytes_and_update_state(&prefix, output)?;

            let consumed_end = index + event.consumed;
            let sequence = self.pending[index..consumed_end].to_vec();
            match event.event {
                MatchedTerminalEventKind::Response(response) => match response {
                    TerminalResponse::Osc8Hyperlink => {
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
                },
                MatchedTerminalEventKind::SynchronizedOutputMode { enabled } => {
                    self.mode_tracker.process_without_emitting(&sequence);
                    self.feed_mirror_bytes(&sequence);
                    if !enabled {
                        self.flush_synchronized_output_buffer(output)?;
                    }
                }
                MatchedTerminalEventKind::KittyKeyboardMode
                | MatchedTerminalEventKind::KeyModifierOptions => {
                    self.mode_tracker.process_without_emitting(&sequence);
                }
            }
            self.pending.drain(..consumed_end);
        }

        let retained = Self::suffix_len_matching_query_prefix(&self.pending);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            let visible = self.pending[..writable].to_vec();
            self.write_visible_bytes_and_update_state(&visible, output)?;
            self.pending.drain(..writable);
        }

        Ok(())
    }

    fn write_visible_bytes(&mut self, bytes: &[u8], output: &mut dyn Write) -> io::Result<()> {
        if self.mode_tracker.synchronized_output() {
            self.synchronized_output_buffer.extend_from_slice(bytes);
        } else {
            output.write_all(bytes)?;
        }
        Ok(())
    }

    fn flush_synchronized_output_buffer(&mut self, output: &mut dyn Write) -> io::Result<()> {
        if self.synchronized_output_buffer.is_empty() {
            return Ok(());
        }

        output.write_all(&self.synchronized_output_buffer)?;
        self.synchronized_output_buffer.clear();
        Ok(())
    }

    fn write_visible_bytes_and_update_state(
        &mut self,
        bytes: &[u8],
        output: &mut dyn Write,
    ) -> io::Result<()> {
        let was_synchronized = self.mode_tracker.synchronized_output();
        self.write_visible_bytes(bytes, output)?;
        self.color_state.process(bytes);
        self.mode_tracker.process_without_emitting(bytes);
        if was_synchronized && !self.mode_tracker.synchronized_output() {
            self.flush_synchronized_output_buffer(output)?;
        }
        self.feed_mirror_bytes(bytes);
        Ok(())
    }

    fn find_next_event(&self) -> Option<(usize, MatchedTerminalEvent)> {
        let response = self
            .find_next_response()
            .map(|(index, response)| (index, response.into()));
        let synchronized_output = find_synchronized_output_mode_sequence(&self.pending).map(
            |SynchronizedOutputModeSequence {
                 index,
                 consumed,
                 enabled,
             }| {
                (
                    index,
                    MatchedTerminalEvent {
                        consumed,
                        event: MatchedTerminalEventKind::SynchronizedOutputMode { enabled },
                    },
                )
            },
        );
        let kitty_keyboard_mode = find_kitty_keyboard_mode_sequence(&self.pending).map(
            |KittyKeyboardModeSequence { index, consumed }| {
                (
                    index,
                    MatchedTerminalEvent {
                        consumed,
                        event: MatchedTerminalEventKind::KittyKeyboardMode,
                    },
                )
            },
        );
        let key_modifier_options = find_key_modifier_options_sequence(&self.pending).map(
            |KeyModifierOptionsSequence { index, consumed }| {
                (
                    index,
                    MatchedTerminalEvent {
                        consumed,
                        event: MatchedTerminalEventKind::KeyModifierOptions,
                    },
                )
            },
        );

        response
            .into_iter()
            .chain(synchronized_output)
            .chain(kitty_keyboard_mode)
            .chain(key_modifier_options)
            .min_by_key(|(index, _)| *index)
    }

    #[allow(clippy::too_many_lines)]
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
        let mode_response = self.find_private_mode_response();
        let ansi_mode_response = self.find_ansi_mode_response();
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
        let kitty_keyboard_flags_response = find_kitty_keyboard_flags_query(&self.pending).map(
            |KittyKeyboardFlagsQuery { index, consumed }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::KittyKeyboardFlags,
                    },
                )
            },
        );
        let key_modifier_options_response = find_key_modifier_options_query(&self.pending).map(
            |KeyModifierOptionsQuery {
                 index,
                 consumed,
                 resource,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::KeyModifierOptions(resource),
                    },
                )
            },
        );

        static_response
            .into_iter()
            .chain(mode_response)
            .chain(ansi_mode_response)
            .chain(osc_color_response)
            .chain(decrqss_response)
            .chain(xtgettcap_response)
            .chain(osc52_response)
            .chain(osc8_response)
            .chain(kitty_keyboard_flags_response)
            .chain(key_modifier_options_response)
            .filter(|(index, _)| !is_inside_osc_or_st_control_string(&self.pending, *index))
            .min_by_key(|(index, _)| *index)
    }

    fn find_private_mode_response(&self) -> Option<(usize, MatchedTerminalResponse)> {
        find_private_mode_status_query(&self.pending).map(
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
        )
    }

    fn find_ansi_mode_response(&self) -> Option<(usize, MatchedTerminalResponse)> {
        find_ansi_mode_status_query(&self.pending).map(
            |AnsiModeStatusQuery {
                 index,
                 consumed,
                 mode,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::AnsiModeStatus(mode),
                    },
                )
            },
        )
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
            .max(ansi_mode_status_query_suffix_len(pending))
            .max(synchronized_output_mode_sequence_suffix_len(pending))
            .max(osc_color_query_suffix_len(pending))
            .max(decrqss_query_suffix_len(pending))
            .max(xtgettcap_query_suffix_len(pending))
            .max(osc52_clipboard_sequence_suffix_len(pending))
            .max(osc8_hyperlink_sequence_suffix_len(pending))
            .max(kitty_keyboard_flags_query_suffix_len(pending))
            .max(kitty_keyboard_mode_sequence_suffix_len(pending))
            .max(key_modifier_options_query_suffix_len(pending))
            .max(key_modifier_options_sequence_suffix_len(pending))
            .max(incomplete_osc_control_sequence_suffix_len(pending))
            .max(incomplete_st_control_sequence_suffix_len(pending))
            .max(incomplete_csi_control_sequence_suffix_len(pending))
    }

    fn flush(&mut self, output: &mut dyn Write) -> io::Result<()> {
        if let Some(drop_start) = find_incomplete_control_sequence_start(&self.pending) {
            let visible = self.pending[..drop_start].to_vec();
            self.write_visible_bytes_and_update_state(&visible, output)?;
            self.pending.clear();
            self.flush_synchronized_output_buffer(output)?;
            return Ok(());
        }

        let visible = self.pending.clone();
        self.write_visible_bytes_and_update_state(&visible, output)?;
        self.pending.clear();
        self.flush_synchronized_output_buffer(output)?;
        Ok(())
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
            TerminalResponse::AnsiModeStatus(mode) => format!(
                "\x1b[{};{}$y",
                mode,
                self.mode_tracker.ansi_mode_report_value(mode)
            )
            .into_bytes(),
            TerminalResponse::OscColor(query) => self.color_state.response(query),
            TerminalResponse::Decrqss(query) => query.response(&self.mirror),
            TerminalResponse::XtGetTcap(query) => query.response(),
            TerminalResponse::XtVersion => xtversion_response(),
            TerminalResponse::KittyKeyboardFlags => {
                format!("\x1b[?{}u", self.mode_tracker.kitty_keyboard_flags()).into_bytes()
            }
            TerminalResponse::KeyModifierOptions(resource) => {
                let value = if resource == 4 {
                    self.mode_tracker.modify_other_keys()
                } else {
                    0
                };
                format!("\x1b[>{resource};{value}m").into_bytes()
            }
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

struct MatchedTerminalEvent {
    consumed: usize,
    event: MatchedTerminalEventKind,
}

enum MatchedTerminalEventKind {
    Response(TerminalResponse),
    SynchronizedOutputMode { enabled: bool },
    KittyKeyboardMode,
    KeyModifierOptions,
}

impl From<MatchedTerminalResponse> for MatchedTerminalEvent {
    fn from(response: MatchedTerminalResponse) -> Self {
        Self {
            consumed: response.consumed,
            event: MatchedTerminalEventKind::Response(response.response),
        }
    }
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
    AnsiModeStatus(u16),
    OscColor(OscColorResponse),
    Decrqss(DecrqssResponse),
    XtGetTcap(XtGetTcapResponse),
    XtVersion,
    KittyKeyboardFlags,
    KeyModifierOptions(u16),
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

const UTF8_C1_OSC: &[u8] = b"\xc2\x9d";
const UTF8_C1_ST: &[u8] = b"\xc2\x9c";
const OSC_START_PREFIXES: &[(&[u8], usize)] = &[
    (b"\x1b]".as_slice(), 2),
    (b"\x9d".as_slice(), 1),
    (UTF8_C1_OSC, UTF8_C1_OSC.len()),
];
const OSC8_HYPERLINK_PREFIXES: &[&[u8]] = &[
    b"\x1b]8;".as_slice(),
    b"\x9d8;".as_slice(),
    b"\xc2\x9d8;".as_slice(),
];
const OSC52_CLIPBOARD_PREFIXES: &[&[u8]] = &[
    b"\x1b]52;".as_slice(),
    b"\x9d52;".as_slice(),
    b"\xc2\x9d52;".as_slice(),
];

struct Osc8HyperlinkSequence {
    index: usize,
    consumed: usize,
}

fn find_osc8_hyperlink_sequence(bytes: &[u8]) -> Option<Osc8HyperlinkSequence> {
    let mut match_sequence = None;
    for prefix in OSC8_HYPERLINK_PREFIXES {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(sequence) = parse_osc8_hyperlink_sequence(bytes, index, prefix.len())
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
    OSC8_HYPERLINK_PREFIXES.iter().any(|prefix| {
        if prefix.starts_with(bytes) {
            return true;
        }
        bytes.starts_with(prefix) && find_osc_color_terminator(&bytes[prefix.len()..]).is_none()
    })
}

fn find_incomplete_osc8_hyperlink_start(bytes: &[u8]) -> Option<usize> {
    find_incomplete_prefixed_osc_start(bytes, OSC8_HYPERLINK_PREFIXES)
}

fn find_incomplete_osc52_clipboard_start(bytes: &[u8]) -> Option<usize> {
    find_incomplete_prefixed_osc_start(bytes, OSC52_CLIPBOARD_PREFIXES)
}

fn find_incomplete_prefixed_osc_start(bytes: &[u8], prefixes: &[&[u8]]) -> Option<usize> {
    prefixes
        .iter()
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
        find_incomplete_st_control_sequence_start(bytes),
        find_incomplete_csi_control_sequence_start(bytes),
        find_incomplete_osc8_hyperlink_start(bytes),
        find_incomplete_osc52_clipboard_start(bytes),
    ]
    .into_iter()
    .flatten()
    .min()
}

fn is_inside_osc_or_st_control_string(bytes: &[u8], index: usize) -> bool {
    is_inside_control_string(bytes, index, find_next_osc_start, find_osc_color_terminator)
        || is_inside_control_string(
            bytes,
            index,
            find_next_st_control_string_start,
            find_xtgettcap_terminator,
        )
}

fn is_inside_control_string(
    bytes: &[u8],
    index: usize,
    mut find_next_start: impl FnMut(&[u8]) -> Option<(usize, usize)>,
    mut find_terminator: impl FnMut(&[u8]) -> Option<OscColorTerminator>,
) -> bool {
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((relative_start, prefix_len)) = find_next_start(&bytes[offset..]) else {
            return false;
        };
        let start = offset + relative_start;
        if start >= index {
            return false;
        }

        let content_start = start + prefix_len;
        let Some(terminator) = find_terminator(&bytes[content_start..]) else {
            return true;
        };
        let end = content_start + terminator.index + terminator.length;
        if index < end {
            return true;
        }
        offset = end;
    }

    false
}

fn incomplete_osc_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_osc_control_sequence_start(bytes)
        .map_or(0, |start| bytes.len() - start)
        .max(suffix_len_matching_prefix(bytes, b"\x1b]"))
        .max(suffix_len_matching_prefix(bytes, UTF8_C1_OSC))
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

fn incomplete_st_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_st_control_sequence_start(bytes)
        .map_or(0, |start| bytes.len() - start)
        .max(
            [
                b"\x1bP".as_slice(),
                b"\x1bX".as_slice(),
                b"\x1b^".as_slice(),
                b"\x1b_".as_slice(),
            ]
            .into_iter()
            .map(|prefix| suffix_len_matching_prefix(bytes, prefix))
            .max()
            .unwrap_or(0),
        )
}

fn find_incomplete_st_control_sequence_start(bytes: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((relative_index, prefix_len)) =
            find_next_st_control_string_start(&bytes[offset..])
        else {
            break;
        };
        let index = offset + relative_index;
        let content_start = index + prefix_len;
        let Some(terminator) = find_xtgettcap_terminator(&bytes[content_start..]) else {
            return Some(index);
        };
        offset = content_start + terminator.index + terminator.length;
    }

    None
}

fn find_next_st_control_string_start(bytes: &[u8]) -> Option<(usize, usize)> {
    [
        (b"\x1bP".as_slice(), 2),
        (b"\x1bX".as_slice(), 2),
        (b"\x1b^".as_slice(), 2),
        (b"\x1b_".as_slice(), 2),
        (b"\x90".as_slice(), 1),
        (b"\x98".as_slice(), 1),
        (b"\x9e".as_slice(), 1),
        (b"\x9f".as_slice(), 1),
    ]
    .into_iter()
    .filter_map(|(prefix, prefix_len)| {
        find_subslice(bytes, prefix).map(|index| (index, prefix_len))
    })
    .min_by_key(|(index, _)| *index)
}

fn incomplete_csi_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_csi_control_sequence_start(bytes)
        .map_or(0, |start| bytes.len() - start)
        .max(suffix_len_matching_prefix(bytes, b"\x1b["))
}

fn find_incomplete_csi_control_sequence_start(bytes: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((relative_index, prefix_len)) = find_next_csi_start(&bytes[offset..]) else {
            break;
        };
        let index = offset + relative_index;
        let content_start = index + prefix_len;
        let Some(final_index) = bytes[content_start..]
            .iter()
            .position(|byte| (0x40..=0x7e).contains(byte))
        else {
            return Some(index);
        };
        offset = content_start + final_index + 1;
    }

    None
}

fn find_next_csi_start(bytes: &[u8]) -> Option<(usize, usize)> {
    [(b"\x1b[".as_slice(), 2), (b"\x9b".as_slice(), 1)]
        .into_iter()
        .filter_map(|(prefix, prefix_len)| {
            find_subslice(bytes, prefix).map(|index| (index, prefix_len))
        })
        .min_by_key(|(index, _)| *index)
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
    for prefix in OSC52_CLIPBOARD_PREFIXES {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(sequence) = parse_osc52_clipboard_sequence(bytes, index, prefix.len()) {
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
    OSC52_CLIPBOARD_PREFIXES.iter().any(|prefix| {
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
    ConformanceLevel,
    LeftRightMargins,
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
                DecrqssKind::ConformanceLevel => bytes.extend_from_slice(b"61;1\"p"),
                DecrqssKind::LeftRightMargins => {
                    append_left_right_margin_state(terminal.left_right_margins(), &mut bytes);
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
        b"\"p" => Some(DecrqssKind::ConformanceLevel),
        b"s" => Some(DecrqssKind::LeftRightMargins),
        _ => None,
    }
}

fn append_sgr_state(style: &Cell, bytes: &mut Vec<u8>) {
    let mut params = Vec::new();
    if style.bold {
        params.push("1".to_owned());
    }
    if style.faint {
        params.push("2".to_owned());
    }
    if style.italic {
        params.push("3".to_owned());
    }
    append_underline_style_sgr(style, &mut params);
    if style.blink {
        params.push("5".to_owned());
    }
    if style.inverse {
        params.push("7".to_owned());
    }
    if style.conceal {
        params.push("8".to_owned());
    }
    if style.strikethrough {
        params.push("9".to_owned());
    }
    if style.double_underline {
        params.push("21".to_owned());
    }
    if style.overline {
        params.push("53".to_owned());
    }
    match style.vertical_align {
        VerticalAlign::Baseline => {}
        VerticalAlign::Superscript => params.push("73".to_owned()),
        VerticalAlign::Subscript => params.push("74".to_owned()),
    }
    append_color_sgr(58, style.underline_color, &mut params);
    append_color_sgr(38, style.foreground, &mut params);
    append_color_sgr(48, style.background, &mut params);

    if params.is_empty() {
        bytes.push(b'0');
    } else {
        bytes.extend_from_slice(params.join(";").as_bytes());
    }
    bytes.push(b'm');
}

fn append_underline_style_sgr(style: &Cell, params: &mut Vec<String>) {
    match style.underline_style {
        UnderlineStyle::None if style.double_underline => params.push("21".to_owned()),
        UnderlineStyle::None if style.underline => params.push("4".to_owned()),
        UnderlineStyle::None => {}
        UnderlineStyle::Single => params.push("4".to_owned()),
        UnderlineStyle::Double => params.push("21".to_owned()),
        UnderlineStyle::Curly => params.push("4:3".to_owned()),
        UnderlineStyle::Dotted => params.push("4:4".to_owned()),
        UnderlineStyle::Dashed => params.push("4:5".to_owned()),
    }
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
        Color::Rgba(red, green, blue, alpha) => {
            params.push(prefix.to_string());
            params.push("6".to_owned());
            params.push(red.to_string());
            params.push(green.to_string());
            params.push(blue.to_string());
            params.push(alpha.to_string());
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

fn append_left_right_margin_state((left, right): (u16, u16), bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(left.saturating_add(1).to_string().as_bytes());
    bytes.push(b';');
    bytes.extend_from_slice(right.saturating_add(1).to_string().as_bytes());
    bytes.push(b's');
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
    let utf8_c1_st = find_subslice(bytes, UTF8_C1_ST).map(|index| OscColorTerminator {
        index,
        length: UTF8_C1_ST.len(),
        response_terminator: OscResponseTerminator::C1St,
    });

    [st, c1_st, utf8_c1_st]
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

#[allow(clippy::match_same_arms, clippy::too_many_lines)]
fn xtgettcap_value_hex(name: &[u8], size: PtySize) -> Option<Vec<u8>> {
    match name {
        b"Co" | b"colors" => Some(b"323536".to_vec()),
        b"TN" => Some(b"787465726d2d323536636f6c6f72".to_vec()),
        b"RGB" => Some(b"524742".to_vec()),
        b"Tc" => Some(b"31".to_vec()),
        b"am" => Some(b"31".to_vec()),
        b"bce" => Some(b"31".to_vec()),
        b"ccc" => Some(b"31".to_vec()),
        b"hs" => Some(b"31".to_vec()),
        b"km" => Some(b"31".to_vec()),
        b"mc5i" => Some(b"31".to_vec()),
        b"mir" => Some(b"31".to_vec()),
        b"msgr" => Some(b"31".to_vec()),
        b"npc" => Some(b"31".to_vec()),
        b"Su" => Some(b"31".to_vec()),
        b"xenl" => Some(b"31".to_vec()),
        b"Ms" => Some(b"1b5d35323b25703125733b257032257307".to_vec()),
        b"dsl" => Some(encode_ascii_hex(b"\x1b]2;\x1b\\")),
        b"fsl" => Some(encode_ascii_hex(b"\x1b\\")),
        b"tsl" => Some(encode_ascii_hex(b"\x1b]0;")),
        b"initc" => Some(encode_ascii_hex(
            b"\x1b]4;%p1%d;rgb:%p2%{255}%*%{1000}%/%2.2X/%p3%{255}%*%{1000}%/%2.2X/%p4%{255}%*%{1000}%/%2.2X\x1b\\",
        )),
        b"Smulx" => Some(b"1b5b343a25703125646d".to_vec()),
        b"Setulc" => Some(
            b"1b5b35383a323a3a257031257b36353533367d252f25643a257031257b3235367d252f257b3235357d252625643a257031257b3235357d25262564253b6d"
                .to_vec(),
        ),
        b"Cr" => Some(encode_ascii_hex(b"\x1b]112\x1b\\")),
        b"Cs" => Some(encode_ascii_hex(b"\x1b]12;%p1%s\x1b\\")),
        b"Se" => Some(encode_ascii_hex(b"\x1b[2 q")),
        b"Ss" => Some(encode_ascii_hex(b"\x1b[%p1%d q")),
        b"Sync" => Some(encode_ascii_hex(b"\x1b[?2026%?%p1%{1}%-%tl%eh%;")),
        b"sitm" => Some(b"1b5b336d".to_vec()),
        b"ritm" => Some(b"1b5b32336d".to_vec()),
        b"Smol" => Some(encode_ascii_hex(b"\x1b[53m")),
        b"smxx" => Some(encode_ascii_hex(b"\x1b[9m")),
        b"rmxx" => Some(encode_ascii_hex(b"\x1b[29m")),
        b"flash" => Some(encode_ascii_hex(b"\x1b[?5h$<100/>\x1b[?5l")),
        b"op" => Some(encode_ascii_hex(b"\x1b[39;49m")),
        b"oc" => Some(encode_ascii_hex(b"\x1b]104\x07")),
        b"bel" => Some(encode_ascii_hex(b"\x07")),
        b"cr" => Some(encode_ascii_hex(b"\r")),
        b"ind" => Some(encode_ascii_hex(b"\n")),
        b"ri" => Some(encode_ascii_hex(b"\x1bM")),
        b"sc" => Some(encode_ascii_hex(b"\x1b7")),
        b"rc" => Some(encode_ascii_hex(b"\x1b8")),
        b"u6" => Some(encode_ascii_hex(b"\x1b[%i%d;%dR")),
        b"u7" => Some(encode_ascii_hex(b"\x1b[6n")),
        b"u8" => Some(encode_ascii_hex(b"\x1b[?%[;0123456789]c")),
        b"u9" => Some(encode_ascii_hex(b"\x1b[c")),
        b"clear" => Some(encode_ascii_hex(b"\x1b[H\x1b[2J")),
        b"cup" => Some(encode_ascii_hex(b"\x1b[%i%p1%d;%p2%dH")),
        b"home" => Some(encode_ascii_hex(b"\x1b[H")),
        b"el" => Some(encode_ascii_hex(b"\x1b[K")),
        b"ed" => Some(encode_ascii_hex(b"\x1b[J")),
        b"el1" => Some(encode_ascii_hex(b"\x1b[1K")),
        b"dch" => Some(encode_ascii_hex(b"\x1b[%p1%dP")),
        b"dch1" => Some(encode_ascii_hex(b"\x1b[P")),
        b"ich" => Some(encode_ascii_hex(b"\x1b[%p1%d@")),
        b"ich1" => Some(encode_ascii_hex(b"\x1b[@")),
        b"il" => Some(encode_ascii_hex(b"\x1b[%p1%dL")),
        b"il1" => Some(encode_ascii_hex(b"\x1b[L")),
        b"dl" => Some(encode_ascii_hex(b"\x1b[%p1%dM")),
        b"dl1" => Some(encode_ascii_hex(b"\x1b[M")),
        b"cuu" => Some(encode_ascii_hex(b"\x1b[%p1%dA")),
        b"cuu1" => Some(encode_ascii_hex(b"\x1b[A")),
        b"cud" => Some(encode_ascii_hex(b"\x1b[%p1%dB")),
        b"cud1" => Some(encode_ascii_hex(b"\n")),
        b"cub" => Some(encode_ascii_hex(b"\x1b[%p1%dD")),
        b"cub1" => Some(encode_ascii_hex(b"\x08")),
        b"cuf" => Some(encode_ascii_hex(b"\x1b[%p1%dC")),
        b"cuf1" => Some(encode_ascii_hex(b"\x1b[C")),
        b"hpa" => Some(encode_ascii_hex(b"\x1b[%i%p1%dG")),
        b"vpa" => Some(encode_ascii_hex(b"\x1b[%i%p1%dd")),
        b"cbt" => Some(encode_ascii_hex(b"\x1b[Z")),
        b"ht" => Some(encode_ascii_hex(b"\t")),
        b"hts" => Some(encode_ascii_hex(b"\x1bH")),
        b"tbc" => Some(encode_ascii_hex(b"\x1b[3g")),
        b"ech" => Some(encode_ascii_hex(b"\x1b[%p1%dX")),
        b"rep" => Some(encode_ascii_hex(b"%p1%c\x1b[%p2%{1}%-%db")),
        b"csr" => Some(encode_ascii_hex(b"\x1b[%i%p1%d;%p2%dr")),
        b"indn" => Some(encode_ascii_hex(b"\x1b[%p1%dS")),
        b"rin" => Some(encode_ascii_hex(b"\x1b[%p1%dT")),
        b"kmous" => Some(encode_ascii_hex(b"\x1b[<")),
        b"XM" => Some(encode_ascii_hex(
            b"\x1b[?1006;1000%?%p1%{1}%=%th%el%;",
        )),
        b"xm" => Some(encode_ascii_hex(
            b"\x1b[<%i%p3%d;%p1%d;%p2%d;%?%p4%tM%em%;",
        )),
        b"civis" => Some(encode_ascii_hex(b"\x1b[?25l")),
        b"cnorm" => Some(encode_ascii_hex(b"\x1b[?12l\x1b[?25h")),
        b"cvvis" => Some(encode_ascii_hex(b"\x1b[?12;25h")),
        b"smcup" => Some(encode_ascii_hex(b"\x1b[?1049h\x1b[22;0;0t")),
        b"rmcup" => Some(encode_ascii_hex(b"\x1b[?1049l\x1b[23;0;0t")),
        b"is2" | b"rs2" => Some(encode_ascii_hex(b"\x1b[!p\x1b[?3;4l\x1b[4l\x1b>")),
        b"rs1" => Some(encode_ascii_hex(b"\x1bc\x1b]104\x07")),
        b"smir" => Some(encode_ascii_hex(b"\x1b[4h")),
        b"rmir" => Some(encode_ascii_hex(b"\x1b[4l")),
        b"smam" => Some(encode_ascii_hex(b"\x1b[?7h")),
        b"rmam" => Some(encode_ascii_hex(b"\x1b[?7l")),
        b"smm" => Some(encode_ascii_hex(b"\x1b[?1034h")),
        b"rmm" => Some(encode_ascii_hex(b"\x1b[?1034l")),
        b"mc0" => Some(encode_ascii_hex(b"\x1b[i")),
        b"mc4" => Some(encode_ascii_hex(b"\x1b[4i")),
        b"mc5" => Some(encode_ascii_hex(b"\x1b[5i")),
        b"meml" => Some(encode_ascii_hex(b"\x1bl")),
        b"memu" => Some(encode_ascii_hex(b"\x1bm")),
        b"smkx" => Some(encode_ascii_hex(b"\x1b[?1h\x1b=")),
        b"rmkx" => Some(encode_ascii_hex(b"\x1b[?1l\x1b>")),
        b"sgr0" => Some(encode_ascii_hex(b"\x1b(B\x1b[m")),
        b"sgr" => Some(encode_ascii_hex(
            b"%?%p9%t\x1b(0%e\x1b(B%;\x1b[0%?%p6%t;1%;%?%p5%t;2%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p7%t;8%;m",
        )),
        b"bold" => Some(encode_ascii_hex(b"\x1b[1m")),
        b"dim" => Some(encode_ascii_hex(b"\x1b[2m")),
        b"blink" => Some(encode_ascii_hex(b"\x1b[5m")),
        b"rev" => Some(encode_ascii_hex(b"\x1b[7m")),
        b"smso" => Some(encode_ascii_hex(b"\x1b[7m")),
        b"rmso" => Some(encode_ascii_hex(b"\x1b[27m")),
        b"invis" => Some(encode_ascii_hex(b"\x1b[8m")),
        b"smul" => Some(encode_ascii_hex(b"\x1b[4m")),
        b"rmul" => Some(encode_ascii_hex(b"\x1b[24m")),
        b"setaf" => Some(encode_ascii_hex(
            b"\x1b[%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m",
        )),
        b"setab" => Some(encode_ascii_hex(
            b"\x1b[%?%p1%{8}%<%t4%p1%d%e%p1%{16}%<%t10%p1%{8}%-%d%e48;5;%p1%d%;m",
        )),
        b"kcuu1" => Some(encode_ascii_hex(b"\x1bOA")),
        b"kcud1" => Some(encode_ascii_hex(b"\x1bOB")),
        b"kcuf1" => Some(encode_ascii_hex(b"\x1bOC")),
        b"kcub1" => Some(encode_ascii_hex(b"\x1bOD")),
        b"kb2" => Some(encode_ascii_hex(b"\x1bOE")),
        b"kbs" => Some(encode_ascii_hex(b"\x7f")),
        b"kcbt" => Some(encode_ascii_hex(b"\x1b[Z")),
        b"khome" => Some(encode_ascii_hex(b"\x1bOH")),
        b"kend" => Some(encode_ascii_hex(b"\x1bOF")),
        b"kich1" => Some(encode_ascii_hex(b"\x1b[2~")),
        b"kdch1" => Some(encode_ascii_hex(b"\x1b[3~")),
        b"kpp" => Some(encode_ascii_hex(b"\x1b[5~")),
        b"knp" => Some(encode_ascii_hex(b"\x1b[6~")),
        b"kHOM" => Some(encode_ascii_hex(b"\x1b[1;2H")),
        b"kEND" => Some(encode_ascii_hex(b"\x1b[1;2F")),
        b"kIC" => Some(encode_ascii_hex(b"\x1b[2;2~")),
        b"kDC" => Some(encode_ascii_hex(b"\x1b[3;2~")),
        b"kPRV" => Some(encode_ascii_hex(b"\x1b[5;2~")),
        b"kNXT" => Some(encode_ascii_hex(b"\x1b[6;2~")),
        b"kLFT" => Some(encode_ascii_hex(b"\x1b[1;2D")),
        b"kRIT" => Some(encode_ascii_hex(b"\x1b[1;2C")),
        b"kri" => Some(encode_ascii_hex(b"\x1b[1;2A")),
        b"kind" => Some(encode_ascii_hex(b"\x1b[1;2B")),
        b"kent" => Some(encode_ascii_hex(b"\x1bOM")),
        b"kf1" => Some(encode_ascii_hex(b"\x1bOP")),
        b"kf2" => Some(encode_ascii_hex(b"\x1bOQ")),
        b"kf3" => Some(encode_ascii_hex(b"\x1bOR")),
        b"kf4" => Some(encode_ascii_hex(b"\x1bOS")),
        b"kf5" => Some(encode_ascii_hex(b"\x1b[15~")),
        b"kf6" => Some(encode_ascii_hex(b"\x1b[17~")),
        b"kf7" => Some(encode_ascii_hex(b"\x1b[18~")),
        b"kf8" => Some(encode_ascii_hex(b"\x1b[19~")),
        b"kf9" => Some(encode_ascii_hex(b"\x1b[20~")),
        b"kf10" => Some(encode_ascii_hex(b"\x1b[21~")),
        b"kf11" => Some(encode_ascii_hex(b"\x1b[23~")),
        b"kf12" => Some(encode_ascii_hex(b"\x1b[24~")),
        name if name.starts_with(b"kf") => xtgettcap_modified_function_key_hex(name),
        b"enacs" => Some(encode_ascii_hex(b"\x1b)0")),
        b"smacs" => Some(encode_ascii_hex(b"\x1b(0")),
        b"rmacs" => Some(encode_ascii_hex(b"\x1b(B")),
        b"acsc" => Some(encode_ascii_hex(
            b"``aaffggiijjkkllmmnnooppqqrrssttuuvvwwxxyyzz{{||}}~~",
        )),
        b"co" | b"cols" => Some(decimal_value_hex(size.columns())),
        b"li" | b"lines" => Some(decimal_value_hex(size.rows())),
        b"it" => Some(decimal_value_hex(8)),
        b"pairs" => Some(decimal_value_hex(0x7fff)),
        _ => None,
    }
}

fn xtgettcap_modified_function_key_hex(name: &[u8]) -> Option<Vec<u8>> {
    let number = parse_ascii_decimal_u8(name.strip_prefix(b"kf")?)?;
    let (function_key, modifier) = match number {
        13..=24 => (number - 12, 2),
        25..=36 => (number - 24, 5),
        37..=48 => (number - 36, 6),
        49..=60 => (number - 48, 3),
        61..=63 => (number - 60, 4),
        _ => return None,
    };

    let sequence = match function_key {
        1 => format!("\x1b[1;{modifier}P"),
        2 => format!("\x1b[1;{modifier}Q"),
        3 => format!("\x1b[1;{modifier}R"),
        4 => format!("\x1b[1;{modifier}S"),
        5 => format!("\x1b[15;{modifier}~"),
        6 => format!("\x1b[17;{modifier}~"),
        7 => format!("\x1b[18;{modifier}~"),
        8 => format!("\x1b[19;{modifier}~"),
        9 => format!("\x1b[20;{modifier}~"),
        10 => format!("\x1b[21;{modifier}~"),
        11 => format!("\x1b[23;{modifier}~"),
        12 => format!("\x1b[24;{modifier}~"),
        _ => return None,
    };

    Some(encode_ascii_hex(sequence.as_bytes()))
}

fn parse_ascii_decimal_u8(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() {
        return None;
    }

    let mut value = 0u8;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(byte - b'0')?;
    }
    Some(value)
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

#[derive(Clone)]
struct OscColorResponse {
    kinds: Vec<OscColorKind>,
    terminator: OscResponseTerminator,
}

#[derive(Clone, Copy)]
enum OscColorKind {
    DefaultForeground,
    DefaultBackground,
    Cursor,
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
    for (prefix, prefix_len) in OSC_START_PREFIXES {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_osc_color_query(bytes, index, *prefix_len) {
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
    let kinds = parse_osc_color_query_content(&bytes[content_start..content_end])?;

    Some(OscColorQuery {
        index,
        consumed: content_end + terminator.length - index,
        query: OscColorResponse {
            kinds,
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
    let utf8_c1_st = find_subslice(bytes, UTF8_C1_ST).map(|index| OscColorTerminator {
        index,
        length: UTF8_C1_ST.len(),
        response_terminator: OscResponseTerminator::C1St,
    });

    [bel, st, c1_st, utf8_c1_st]
        .into_iter()
        .flatten()
        .min_by_key(|terminator| terminator.index)
}

fn parse_osc_color_query_content(content: &[u8]) -> Option<Vec<OscColorKind>> {
    match content {
        b"10;?" => Some(vec![OscColorKind::DefaultForeground]),
        b"11;?" => Some(vec![OscColorKind::DefaultBackground]),
        b"12;?" => Some(vec![OscColorKind::Cursor]),
        _ => parse_palette_color_query(content),
    }
}

fn parse_palette_color_query(content: &[u8]) -> Option<Vec<OscColorKind>> {
    let rest = content.strip_prefix(b"4;")?;
    let mut parts = rest.split(|byte| *byte == b';');
    let mut kinds = Vec::new();

    while let Some(index) = parts.next() {
        let marker = parts.next()?;
        if marker != b"?" {
            return None;
        }
        kinds.push(OscColorKind::Palette(parse_u8_decimal(index)?));
    }

    (!kinds.is_empty()).then_some(kinds)
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
        .or_else(|| bytes.strip_prefix(UTF8_C1_OSC))
    else {
        return b"\x1b]".starts_with(bytes)
            || b"\x9d".starts_with(bytes)
            || UTF8_C1_OSC.starts_with(bytes);
    };

    b"10;?".starts_with(rest)
        || b"11;?".starts_with(rest)
        || b"12;?".starts_with(rest)
        || is_palette_color_query_prefix(rest)
}

fn is_palette_color_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(b"4") else {
        return bytes.is_empty();
    };
    let Some(mut rest) = rest.strip_prefix(b";") else {
        return rest.is_empty();
    };

    loop {
        let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
        if digits == 0 {
            return rest.is_empty();
        }
        rest = &rest[digits..];
        if rest.is_empty() {
            return true;
        }
        let Some(after_separator) = rest.strip_prefix(b";") else {
            return false;
        };
        if after_separator.is_empty() {
            return true;
        }
        let Some(after_query_marker) = after_separator.strip_prefix(b"?") else {
            return false;
        };
        rest = after_query_marker;
        if rest.is_empty() {
            return true;
        }
        let Some(after_next_separator) = rest.strip_prefix(b";") else {
            return false;
        };
        rest = after_next_separator;
    }
}

struct TerminalColorState {
    foreground: DynamicColor,
    background: DynamicColor,
    cursor: DynamicColor,
    palette_overrides: Vec<(u8, [u8; 3])>,
    pending: Vec<u8>,
}

impl Default for TerminalColorState {
    fn default() -> Self {
        Self {
            foreground: DynamicColor::rgb8(DEFAULT_FOREGROUND),
            background: DynamicColor::rgb8(DEFAULT_BACKGROUND),
            cursor: DynamicColor::rgb8(DEFAULT_CURSOR),
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
            if is_inside_osc_or_st_control_string(&self.pending, index) {
                self.pending.drain(..index.saturating_add(1));
                continue;
            }
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
        let mut response = Vec::new();
        for kind in query.kinds {
            let mut item = match kind {
                OscColorKind::DefaultForeground => {
                    format!("\x1b]10;{}", color_response(self.foreground)).into_bytes()
                }
                OscColorKind::DefaultBackground => {
                    format!("\x1b]11;{}", color_response(self.background)).into_bytes()
                }
                OscColorKind::Cursor => {
                    format!("\x1b]12;{}", color_response(self.cursor)).into_bytes()
                }
                OscColorKind::Palette(index) => format!(
                    "\x1b]4;{};{}",
                    index,
                    palette_color_response(self.palette_color(index))
                )
                .into_bytes(),
            };
            item.extend_from_slice(query.terminator.bytes());
            response.extend(item);
        }
        response
    }

    fn apply(&mut self, change: OscColorChange) {
        match change {
            OscColorChange::DefaultForeground(color) => self.foreground = color,
            OscColorChange::DefaultBackground(color) => self.background = color,
            OscColorChange::Cursor(color) => self.cursor = color,
            OscColorChange::ResetDefaultForeground => {
                self.foreground = DynamicColor::rgb8(DEFAULT_FOREGROUND);
            }
            OscColorChange::ResetDefaultBackground => {
                self.background = DynamicColor::rgb8(DEFAULT_BACKGROUND);
            }
            OscColorChange::ResetCursor => self.cursor = DynamicColor::rgb8(DEFAULT_CURSOR),
            OscColorChange::ResetPalette(indices) => self
                .palette_overrides
                .retain(|(palette_index, _)| !indices.contains(palette_index)),
            OscColorChange::ResetPaletteAll => self.palette_overrides.clear(),
            OscColorChange::Palette(changes) => {
                for (index, color) in changes {
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
    }

    fn palette_color(&self, index: u8) -> [u8; 3] {
        self.palette_overrides
            .iter()
            .find_map(|(palette_index, color)| (*palette_index == index).then_some(*color))
            .unwrap_or_else(|| indexed_color(index))
    }

    fn retain_possible_prefix(&mut self) {
        let retained = OSC_START_PREFIXES
            .iter()
            .map(|(prefix, _)| suffix_len_matching_prefix(&self.pending, prefix))
            .max()
            .unwrap_or(0);
        let retained = retained
            .max(incomplete_osc_control_sequence_suffix_len(&self.pending))
            .max(incomplete_st_control_sequence_suffix_len(&self.pending));
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

#[derive(Clone)]
enum OscColorChange {
    DefaultForeground(DynamicColor),
    DefaultBackground(DynamicColor),
    Cursor(DynamicColor),
    ResetDefaultForeground,
    ResetDefaultBackground,
    ResetCursor,
    ResetPalette(Vec<u8>),
    ResetPaletteAll,
    Palette(Vec<(u8, [u8; 3])>),
}

fn find_next_osc_start(bytes: &[u8]) -> Option<(usize, usize)> {
    OSC_START_PREFIXES
        .iter()
        .filter_map(|(prefix, prefix_len)| {
            find_subslice(bytes, prefix).map(|index| (index, *prefix_len))
        })
        .min_by_key(|(index, _)| *index)
}

fn parse_osc_color_change(content: &[u8]) -> Option<OscColorChange> {
    if let Some(color) = content.strip_prefix(b"10;").and_then(parse_color_spec) {
        return Some(OscColorChange::DefaultForeground(color));
    }
    if let Some(color) = content.strip_prefix(b"11;").and_then(parse_color_spec) {
        return Some(OscColorChange::DefaultBackground(color));
    }
    if let Some(color) = content.strip_prefix(b"12;").and_then(parse_color_spec) {
        return Some(OscColorChange::Cursor(color));
    }
    if matches!(content, b"110" | b"110;") {
        return Some(OscColorChange::ResetDefaultForeground);
    }
    if matches!(content, b"111" | b"111;") {
        return Some(OscColorChange::ResetDefaultBackground);
    }
    if matches!(content, b"112" | b"112;") {
        return Some(OscColorChange::ResetCursor);
    }
    if let Some(change) = parse_palette_reset_change(content) {
        return Some(change);
    }
    parse_palette_color_change(content)
}

fn parse_palette_reset_change(content: &[u8]) -> Option<OscColorChange> {
    if matches!(content, b"104" | b"104;") {
        return Some(OscColorChange::ResetPaletteAll);
    }
    let rest = content.strip_prefix(b"104;")?;
    let mut indices = Vec::new();
    for index in rest.split(|byte| *byte == b';') {
        indices.push(parse_u8_decimal(index)?);
    }
    (!indices.is_empty()).then_some(OscColorChange::ResetPalette(indices))
}

fn parse_palette_color_change(content: &[u8]) -> Option<OscColorChange> {
    let rest = content.strip_prefix(b"4;")?;
    let mut changes = Vec::new();
    let mut parts = rest.split(|byte| *byte == b';');

    while let Some(index) = parts.next() {
        let color = parts.next()?;
        changes.push((parse_u8_decimal(index)?, parse_color_spec(color)?.to_rgb8()));
    }

    (!changes.is_empty()).then_some(OscColorChange::Palette(changes))
}

fn parse_color_spec(value: &[u8]) -> Option<DynamicColor> {
    if let Some(hex) = value.strip_prefix(b"#") {
        return parse_hex_color_spec(hex);
    }
    if let Some(rest) = value.strip_prefix(b"rgba:") {
        return parse_slash_rgba_color_spec(rest);
    }
    if value.starts_with(b"rgba(") {
        return parse_function_rgba_color_spec(value);
    }

    let rest = value.strip_prefix(b"rgb:")?;
    let mut components = rest.split(|byte| *byte == b'/');
    let red = parse_rgb_component(components.next()?)?;
    let green = parse_rgb_component(components.next()?)?;
    let blue = parse_rgb_component(components.next()?)?;
    components
        .next()
        .is_none()
        .then_some(DynamicColor::rgb(red, green, blue))
}

fn parse_hex_color_spec(hex: &[u8]) -> Option<DynamicColor> {
    match hex.len() {
        3 => Some(DynamicColor::rgb8([
            parse_hex_digit(hex[0])? * 17,
            parse_hex_digit(hex[1])? * 17,
            parse_hex_digit(hex[2])? * 17,
        ])),
        6 => Some(DynamicColor::rgb8([
            parse_hex_byte(&hex[0..2])?,
            parse_hex_byte(&hex[2..4])?,
            parse_hex_byte(&hex[4..6])?,
        ])),
        _ => None,
    }
}

fn parse_slash_rgba_color_spec(value: &[u8]) -> Option<DynamicColor> {
    let mut components = value.split(|byte| *byte == b'/');
    let red = parse_hex_component16(components.next()?)?;
    let green = parse_hex_component16(components.next()?)?;
    let blue = parse_hex_component16(components.next()?)?;
    let alpha = parse_hex_component16(components.next()?)?;
    components
        .next()
        .is_none()
        .then_some(DynamicColor::rgba(red, green, blue, alpha))
}

fn parse_function_rgba_color_spec(value: &[u8]) -> Option<DynamicColor> {
    let inner = value.strip_prefix(b"rgba(")?.strip_suffix(b")")?;
    let mut components = inner.split(|byte| *byte == b',');
    let red = parse_u8_decimal(components.next()?.trim_ascii())?;
    let green = parse_u8_decimal(components.next()?.trim_ascii())?;
    let blue = parse_u8_decimal(components.next()?.trim_ascii())?;
    let alpha = parse_alpha_float_component(components.next()?.trim_ascii())?;
    components
        .next()
        .is_none()
        .then_some(DynamicColor::rgba8(red, green, blue, alpha))
}

fn parse_rgb_component(component: &[u8]) -> Option<u16> {
    match component.len() {
        1 => parse_hex_digit(component[0]).map(|value| u16::from(value) * 0x1111),
        2 => parse_hex_byte(component).map(DynamicColor::expand_byte),
        3 | 4 => parse_hex_component16(component),
        _ => None,
    }
}

fn parse_hex_component16(component: &[u8]) -> Option<u16> {
    match component.len() {
        1 => parse_hex_digit(component[0]).map(|value| u16::from(value) * 0x1111),
        2 => parse_hex_byte(component).map(DynamicColor::expand_byte),
        3 => Some(
            parse_hex_digit(component[0]).map(u16::from)? * 0x1000
                + parse_hex_digit(component[1]).map(u16::from)? * 0x100
                + parse_hex_digit(component[2]).map(u16::from)? * 0x10,
        ),
        4 => Some(
            parse_hex_digit(component[0]).map(u16::from)? * 0x1000
                + parse_hex_digit(component[1]).map(u16::from)? * 0x100
                + parse_hex_digit(component[2]).map(u16::from)? * 0x10
                + parse_hex_digit(component[3]).map(u16::from)?,
        ),
        _ => None,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn parse_alpha_float_component(component: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(component).ok()?;
    let value = text.parse::<f32>().ok()?;
    if !(0.0..=1.0).contains(&value) {
        return None;
    }
    Some((value * f32::from(u16::MAX)).round() as u16)
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
const DEFAULT_CURSOR: [u8; 3] = DEFAULT_FOREGROUND;

#[derive(Clone, Copy)]
struct DynamicColor {
    red: u16,
    green: u16,
    blue: u16,
    alpha: Option<u16>,
}

impl DynamicColor {
    const fn rgb8(color: [u8; 3]) -> Self {
        Self::rgb(
            color[0] as u16 * 0x101,
            color[1] as u16 * 0x101,
            color[2] as u16 * 0x101,
        )
    }

    const fn rgb(red: u16, green: u16, blue: u16) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: None,
        }
    }

    const fn rgba(red: u16, green: u16, blue: u16, alpha: u16) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: Some(alpha),
        }
    }

    const fn rgba8(red: u8, green: u8, blue: u8, alpha: u16) -> Self {
        Self::rgba(
            Self::expand_byte(red),
            Self::expand_byte(green),
            Self::expand_byte(blue),
            alpha,
        )
    }

    const fn expand_byte(value: u8) -> u16 {
        value as u16 * 0x101
    }

    const fn to_rgb8(self) -> [u8; 3] {
        [
            (self.red >> 8) as u8,
            (self.green >> 8) as u8,
            (self.blue >> 8) as u8,
        ]
    }
}

fn color_response(color: DynamicColor) -> String {
    match color.alpha {
        Some(alpha) => format!(
            "rgba:{:04x}/{:04x}/{:04x}/{:04x}",
            color.red, color.green, color.blue, alpha
        ),
        None => format!(
            "rgb:{:04x}/{:04x}/{:04x}",
            color.red, color.green, color.blue
        ),
    }
}

fn palette_color_response(color: [u8; 3]) -> String {
    color_response(DynamicColor::rgb8(color))
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

struct AnsiModeStatusQuery {
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

fn find_ansi_mode_status_query(bytes: &[u8]) -> Option<AnsiModeStatusQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [
        (b"\x1b[".as_slice(), b"\x1b[".len()),
        (b"\x9b".as_slice(), b"\x9b".len()),
    ] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_ansi_mode_status_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &AnsiModeStatusQuery| query.index < current.index)
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

fn parse_ansi_mode_status_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<AnsiModeStatusQuery> {
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
    Some(AnsiModeStatusQuery {
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

fn ansi_mode_status_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_ansi_mode_status_query_prefix(&bytes[bytes.len() - length..]))
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

fn is_ansi_mode_status_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b[")
        .or_else(|| bytes.strip_prefix(b"\x9b"))
    else {
        return b"\x1b[".starts_with(bytes);
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
struct InputModes {
    bits: u8,
    kitty_keyboard_flags: u16,
    modify_other_keys: u8,
}

impl InputModes {
    const APPLICATION_CURSOR_KEYS: u8 = 1;
    const APPLICATION_KEYPAD: u8 = 1 << 1;
    const BRACKETED_PASTE: u8 = 1 << 2;
    const FOCUS_REPORTING: u8 = 1 << 3;
    const MOUSE_INPUT_MASK: u8 = 0b1111_0000;
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
        MouseInputMode::from_bits(
            (self.bits & Self::MOUSE_INPUT_MASK) >> Self::MOUSE_REPORTING_SHIFT,
        )
    }

    fn focus_reporting(self) -> bool {
        self.enabled(Self::FOCUS_REPORTING)
    }

    fn kitty_keyboard_flags(self) -> u16 {
        self.kitty_keyboard_flags
    }

    fn modify_other_keys(self) -> u8 {
        self.modify_other_keys
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
        self.bits &= !Self::MOUSE_INPUT_MASK;
        self.bits |= mode.bits() << Self::MOUSE_REPORTING_SHIFT;
        self
    }

    fn with_focus_reporting(self, enabled: bool) -> Self {
        self.with_flag(Self::FOCUS_REPORTING, enabled)
    }

    fn with_kitty_keyboard_flags(mut self, flags: u16) -> Self {
        self.kitty_keyboard_flags = flags;
        self
    }

    fn with_modify_other_keys(mut self, mode: u8) -> Self {
        self.modify_other_keys = mode;
        self
    }

    fn enabled(self, flag: u8) -> bool {
        self.bits & flag != 0
    }

    fn with_flag(mut self, flag: u8, enabled: bool) -> Self {
        if enabled {
            self.bits |= flag;
        } else {
            self.bits &= !flag;
        }
        self
    }
}

fn encode_input_event(event: Event, modes: InputModes) -> Option<Vec<u8>> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => encode_key_with_mode(key, modes),
        Event::Key(key) if reports_kitty_key_event_type(key.kind, modes.kitty_keyboard_flags()) => {
            encode_key_with_mode(key, modes)
        }
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
        MouseProtocolMode::Utf8 => encode_utf8_mouse_event(event.kind, code, column, row),
        MouseProtocolMode::Urxvt => encode_urxvt_mouse_event(event.kind, code, column, row),
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
        code = legacy_mouse_release_code(code);
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

fn encode_utf8_mouse_event(
    kind: MouseEventKind,
    mut code: u16,
    column: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if matches!(kind, MouseEventKind::Up(_)) {
        code = legacy_mouse_release_code(code);
    }

    let mut bytes = b"\x1b[M".to_vec();
    push_utf8_mouse_value(&mut bytes, code)?;
    push_utf8_mouse_value(&mut bytes, column)?;
    push_utf8_mouse_value(&mut bytes, row)?;
    Some(bytes)
}

fn encode_urxvt_mouse_event(
    kind: MouseEventKind,
    mut code: u16,
    column: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if matches!(kind, MouseEventKind::Up(_)) {
        code = legacy_mouse_release_code(code);
    }

    let encoded_code = code.checked_add(32)?;
    Some(format!("\x1b[{encoded_code};{column};{row}M").into_bytes())
}

fn legacy_mouse_byte(value: u16) -> Option<u8> {
    u8::try_from(value.checked_add(32)?).ok()
}

fn push_utf8_mouse_value(bytes: &mut Vec<u8>, value: u16) -> Option<()> {
    let ch = char::from_u32(u32::from(value.checked_add(32)?))?;
    let mut buffer = [0; 4];
    bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
    Some(())
}

const fn legacy_mouse_release_code(code: u16) -> u16 {
    3 + (code & !0b11)
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
    encode_key_with_mode(key, InputModes::default())
}

fn encode_key_with_mode(key: KeyEvent, modes: InputModes) -> Option<Vec<u8>> {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if key.kind != KeyEventKind::Press {
        return encode_kitty_event_key(key, modes.kitty_keyboard_flags());
    }

    if let Some(bytes) = encode_kitty_modifier_key(key, modes.kitty_keyboard_flags()) {
        return Some(bytes);
    }
    if let Some(bytes) = encode_modified_key(key) {
        return Some(bytes);
    }
    if let Some(bytes) = encode_kitty_keypad_key(key, modes.kitty_keyboard_flags()) {
        return Some(bytes);
    }
    if let Some(bytes) = encode_kitty_functional_key(key, modes.kitty_keyboard_flags()) {
        return Some(bytes);
    }
    if let Some(bytes) = encode_kitty_report_all_key(key, modes.kitty_keyboard_flags()) {
        return Some(bytes);
    }
    if let Some(bytes) = encode_kitty_disambiguated_key(key, modes.kitty_keyboard_flags()) {
        return Some(bytes);
    }
    if let Some(bytes) = encode_xterm_modify_other_key(key, modes.modify_other_keys()) {
        return Some(bytes);
    }
    if modes.application_keypad() {
        if let Some(bytes) = encode_application_keypad_key(key) {
            return Some(bytes);
        }
    }
    if modes.application_cursor_keys() {
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

fn encode_kitty_event_key(key: KeyEvent, kitty_keyboard_flags: u16) -> Option<Vec<u8>> {
    encode_kitty_modifier_key(key, kitty_keyboard_flags)
        .or_else(|| encode_kitty_keypad_key(key, kitty_keyboard_flags))
        .or_else(|| encode_kitty_functional_key(key, kitty_keyboard_flags))
        .or_else(|| encode_kitty_report_all_key(key, kitty_keyboard_flags))
        .or_else(|| encode_kitty_disambiguated_key(key, kitty_keyboard_flags))
}

fn encode_kitty_modifier_key(key: KeyEvent, kitty_keyboard_flags: u16) -> Option<Vec<u8>> {
    let event_type = kitty_event_type(key.kind, kitty_keyboard_flags);
    if kitty_keyboard_flags
        & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_REPORT_EVENTS)
        == 0
    {
        return None;
    }
    if key.kind == KeyEventKind::Press
        && kitty_keyboard_flags & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL) == 0
    {
        return None;
    }

    let key_code = kitty_local_modifier_key_code(key.code)?;
    Some(kitty_csi_u_key_with_event(
        key_code,
        kitty_modifier(key),
        event_type,
        None,
    ))
}

fn encode_kitty_keypad_key(key: KeyEvent, kitty_keyboard_flags: u16) -> Option<Vec<u8>> {
    let event_type = kitty_event_type(key.kind, kitty_keyboard_flags);
    if kitty_keyboard_flags
        & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_REPORT_EVENTS)
        == 0
    {
        return None;
    }
    if key.kind == KeyEventKind::Press
        && kitty_keyboard_flags & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL) == 0
    {
        return None;
    }

    let key_code = kitty_local_keypad_code(key)?;
    Some(kitty_csi_u_key_with_event(
        key_code,
        kitty_modifier(key),
        event_type,
        None,
    ))
}

fn encode_kitty_functional_key(key: KeyEvent, kitty_keyboard_flags: u16) -> Option<Vec<u8>> {
    let event_type = kitty_event_type(key.kind, kitty_keyboard_flags);
    if kitty_keyboard_flags
        & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_REPORT_EVENTS)
        == 0
    {
        return None;
    }
    if key.kind == KeyEventKind::Press
        && kitty_keyboard_flags & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL) == 0
    {
        return None;
    }

    let modifier = kitty_modifier(key);
    match key.code {
        KeyCode::Esc => Some(kitty_csi_u_key_with_event(27, modifier, event_type, None)),
        KeyCode::Enter if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL != 0 => {
            Some(kitty_csi_u_key_with_event(13, modifier, event_type, None))
        }
        KeyCode::Tab if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL != 0 => {
            Some(kitty_csi_u_key_with_event(9, modifier, event_type, None))
        }
        KeyCode::Backspace if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL != 0 => {
            Some(kitty_csi_u_key_with_event(127, modifier, event_type, None))
        }
        KeyCode::Up => Some(kitty_csi_final_key_with_event(b'A', modifier, event_type)),
        KeyCode::Down => Some(kitty_csi_final_key_with_event(b'B', modifier, event_type)),
        KeyCode::Right => Some(kitty_csi_final_key_with_event(b'C', modifier, event_type)),
        KeyCode::Left => Some(kitty_csi_final_key_with_event(b'D', modifier, event_type)),
        KeyCode::End => Some(kitty_csi_final_key_with_event(b'F', modifier, event_type)),
        KeyCode::Home => Some(kitty_csi_final_key_with_event(b'H', modifier, event_type)),
        KeyCode::Insert => Some(kitty_csi_tilde_key_with_event(2, modifier, event_type)),
        KeyCode::Delete => Some(kitty_csi_tilde_key_with_event(3, modifier, event_type)),
        KeyCode::PageUp => Some(kitty_csi_tilde_key_with_event(5, modifier, event_type)),
        KeyCode::PageDown => Some(kitty_csi_tilde_key_with_event(6, modifier, event_type)),
        KeyCode::F(1) => Some(kitty_csi_final_key_with_event(b'P', modifier, event_type)),
        KeyCode::F(2) => Some(kitty_csi_final_key_with_event(b'Q', modifier, event_type)),
        KeyCode::F(3) => Some(kitty_csi_final_key_with_event(b'R', modifier, event_type)),
        KeyCode::F(4) => Some(kitty_csi_final_key_with_event(b'S', modifier, event_type)),
        KeyCode::F(key @ 5..=12) => {
            let number = match key {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => unreachable!(),
            };
            Some(kitty_csi_tilde_key_with_event(number, modifier, event_type))
        }
        KeyCode::F(key @ 13..=35) => Some(kitty_csi_u_key_with_event(
            57376 + u32::from(key - 13),
            modifier,
            event_type,
            None,
        )),
        _ => kitty_local_pua_function_key_code(key.code)
            .map(|key_code| kitty_csi_u_key_with_event(key_code, modifier, event_type, None)),
    }
}

fn kitty_local_pua_function_key_code(code: KeyCode) -> Option<u32> {
    match code {
        KeyCode::CapsLock => Some(57358),
        KeyCode::ScrollLock => Some(57359),
        KeyCode::NumLock => Some(57360),
        KeyCode::PrintScreen => Some(57361),
        KeyCode::Pause => Some(57362),
        KeyCode::Menu => Some(57363),
        KeyCode::Media(media) => Some(kitty_local_media_key_code(media)),
        _ => None,
    }
}

fn kitty_local_media_key_code(media: MediaKeyCode) -> u32 {
    match media {
        MediaKeyCode::Play => 57428,
        MediaKeyCode::Pause => 57429,
        MediaKeyCode::PlayPause => 57430,
        MediaKeyCode::Reverse => 57431,
        MediaKeyCode::Stop => 57432,
        MediaKeyCode::FastForward => 57433,
        MediaKeyCode::Rewind => 57434,
        MediaKeyCode::TrackNext => 57435,
        MediaKeyCode::TrackPrevious => 57436,
        MediaKeyCode::Record => 57437,
        MediaKeyCode::LowerVolume => 57438,
        MediaKeyCode::RaiseVolume => 57439,
        MediaKeyCode::MuteVolume => 57440,
    }
}

fn kitty_local_modifier_key_code(code: KeyCode) -> Option<u32> {
    let KeyCode::Modifier(modifier) = code else {
        return None;
    };

    match modifier {
        ModifierKeyCode::LeftShift => Some(57441),
        ModifierKeyCode::LeftControl => Some(57442),
        ModifierKeyCode::LeftAlt => Some(57443),
        ModifierKeyCode::LeftSuper => Some(57444),
        ModifierKeyCode::LeftHyper => Some(57445),
        ModifierKeyCode::LeftMeta => Some(57446),
        ModifierKeyCode::RightShift => Some(57447),
        ModifierKeyCode::RightControl => Some(57448),
        ModifierKeyCode::RightAlt => Some(57449),
        ModifierKeyCode::RightSuper => Some(57450),
        ModifierKeyCode::RightHyper => Some(57451),
        ModifierKeyCode::RightMeta => Some(57452),
        ModifierKeyCode::IsoLevel3Shift => Some(57453),
        ModifierKeyCode::IsoLevel5Shift => Some(57454),
    }
}

fn encode_kitty_report_all_key(key: KeyEvent, kitty_keyboard_flags: u16) -> Option<Vec<u8>> {
    if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL == 0 {
        return None;
    }

    let key_code = match key.code {
        KeyCode::Char(character) => {
            kitty_local_key_code(character, key.modifiers, kitty_keyboard_flags)
        }
        KeyCode::Enter => 13.to_string(),
        KeyCode::Tab => 9.to_string(),
        KeyCode::Backspace => 127.to_string(),
        KeyCode::Esc => 27.to_string(),
        _ => return None,
    };
    Some(kitty_csi_u_key_with_event(
        key_code,
        kitty_modifier(key),
        kitty_event_type(key.kind, kitty_keyboard_flags),
        associated_text_from_local_key(key, kitty_keyboard_flags).as_deref(),
    ))
}

fn encode_kitty_disambiguated_key(key: KeyEvent, kitty_keyboard_flags: u16) -> Option<Vec<u8>> {
    if kitty_keyboard_flags & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL) == 0 {
        return None;
    }
    if !(key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
        || key.modifiers.contains(KeyModifiers::SUPER)
        || key.modifiers.contains(KeyModifiers::HYPER)
        || key.modifiers.contains(KeyModifiers::META))
    {
        return None;
    }

    let KeyCode::Char(character) = key.code else {
        return None;
    };
    let key_code = if kitty_keyboard_flags & KITTY_KEYBOARD_ALTERNATE_KEYS != 0 {
        kitty_local_key_code(character, key.modifiers, kitty_keyboard_flags)
    } else {
        kitty_ascii_key_code(character)?.to_string()
    };
    let modifier = kitty_modifier(key)?;
    Some(kitty_csi_u_key_with_event(
        key_code,
        Some(modifier),
        kitty_event_type(key.kind, kitty_keyboard_flags),
        None,
    ))
}

fn kitty_ascii_key_code(character: char) -> Option<u32> {
    if character.is_ascii_alphabetic() {
        Some(u32::from(character.to_ascii_lowercase()))
    } else if character.is_ascii_graphic() || character == ' ' {
        Some(u32::from(character))
    } else {
        None
    }
}

fn kitty_key_code(character: char) -> u32 {
    if character.is_ascii_alphabetic() {
        u32::from(character.to_ascii_lowercase())
    } else {
        u32::from(character)
    }
}

fn kitty_local_keypad_code(key: KeyEvent) -> Option<u32> {
    if !key.state.contains(KeyEventState::KEYPAD) && !matches!(key.code, KeyCode::KeypadBegin) {
        return None;
    }

    match key.code {
        KeyCode::Char('0') => Some(57399),
        KeyCode::Char('1') => Some(57400),
        KeyCode::Char('2') => Some(57401),
        KeyCode::Char('3') => Some(57402),
        KeyCode::Char('4') => Some(57403),
        KeyCode::Char('5') => Some(57404),
        KeyCode::Char('6') => Some(57405),
        KeyCode::Char('7') => Some(57406),
        KeyCode::Char('8') => Some(57407),
        KeyCode::Char('9') => Some(57408),
        KeyCode::Char('.') => Some(57409),
        KeyCode::Char('/') => Some(57410),
        KeyCode::Char('*') => Some(57411),
        KeyCode::Char('-') => Some(57412),
        KeyCode::Char('+') => Some(57413),
        KeyCode::Enter => Some(57414),
        KeyCode::Char('=') => Some(57415),
        KeyCode::Char(',') => Some(57416),
        KeyCode::Left => Some(57417),
        KeyCode::Right => Some(57418),
        KeyCode::Up => Some(57419),
        KeyCode::Down => Some(57420),
        KeyCode::PageUp => Some(57421),
        KeyCode::PageDown => Some(57422),
        KeyCode::Home => Some(57423),
        KeyCode::End => Some(57424),
        KeyCode::Insert => Some(57425),
        KeyCode::Delete => Some(57426),
        KeyCode::KeypadBegin => Some(57427),
        _ => None,
    }
}

fn kitty_local_key_code(
    character: char,
    modifiers: KeyModifiers,
    kitty_keyboard_flags: u16,
) -> String {
    let primary = kitty_key_code(character);
    if kitty_keyboard_flags & KITTY_KEYBOARD_ALTERNATE_KEYS == 0
        || !modifiers.contains(KeyModifiers::SHIFT)
    {
        return primary.to_string();
    }

    let shifted = u32::from(character);
    if shifted == primary {
        primary.to_string()
    } else {
        format!("{primary}:{shifted}")
    }
}

fn associated_text_from_local_key(key: KeyEvent, kitty_keyboard_flags: u16) -> Option<String> {
    if kitty_keyboard_flags & (KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_ASSOCIATED_TEXT)
        != (KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_ASSOCIATED_TEXT)
    {
        return None;
    }
    if key.kind == KeyEventKind::Release {
        return None;
    }

    let KeyCode::Char(character) = key.code else {
        return None;
    };
    associated_text_codepoints(std::iter::once(character))
}

fn associated_text_codepoints(characters: impl IntoIterator<Item = char>) -> Option<String> {
    let mut encoded = String::new();
    for character in characters {
        if character.is_control() {
            return None;
        }
        if !encoded.is_empty() {
            encoded.push(':');
        }
        encoded.push_str(&u32::from(character).to_string());
    }

    if encoded.is_empty() {
        None
    } else {
        Some(encoded)
    }
}

fn kitty_csi_u_key_with_event(
    key_code: impl std::fmt::Display,
    modifier: Option<u16>,
    event_type: Option<u8>,
    associated_text: Option<&str>,
) -> Vec<u8> {
    let modifier = match (modifier, event_type) {
        (Some(modifier), Some(event_type)) => Some(format!("{modifier}:{event_type}")),
        (Some(modifier), None) => Some(modifier.to_string()),
        (None, Some(event_type)) => Some(format!("1:{event_type}")),
        (None, None) => None,
    };

    match (modifier, associated_text) {
        (Some(modifier), Some(text)) => format!("\x1b[{key_code};{modifier};{text}u").into_bytes(),
        (Some(modifier), None) => format!("\x1b[{key_code};{modifier}u").into_bytes(),
        (None, Some(text)) => format!("\x1b[{key_code};;{text}u").into_bytes(),
        (None, None) => format!("\x1b[{key_code}u").into_bytes(),
    }
}

fn kitty_csi_final_key_with_event(
    final_byte: u8,
    modifier: Option<u16>,
    event_type: Option<u8>,
) -> Vec<u8> {
    match modifier {
        Some(modifier) => match event_type {
            Some(event_type) => {
                format!("\x1b[1;{}:{}{}", modifier, event_type, final_byte as char).into_bytes()
            }
            None => format!("\x1b[1;{}{}", modifier, final_byte as char).into_bytes(),
        },
        None => match event_type {
            Some(event_type) => {
                format!("\x1b[1;1:{}{}", event_type, final_byte as char).into_bytes()
            }
            None => vec![0x1b, b'[', final_byte],
        },
    }
}

fn kitty_csi_tilde_key_with_event(
    number: u8,
    modifier: Option<u16>,
    event_type: Option<u8>,
) -> Vec<u8> {
    match modifier {
        Some(modifier) => match event_type {
            Some(event_type) => format!("\x1b[{number};{modifier}:{event_type}~").into_bytes(),
            None => format!("\x1b[{number};{modifier}~").into_bytes(),
        },
        None => match event_type {
            Some(event_type) => format!("\x1b[{number};1:{event_type}~").into_bytes(),
            None => format!("\x1b[{number}~").into_bytes(),
        },
    }
}

fn reports_kitty_key_event_type(kind: KeyEventKind, kitty_keyboard_flags: u16) -> bool {
    kind != KeyEventKind::Press && kitty_event_type(kind, kitty_keyboard_flags).is_some()
}

fn kitty_event_type(kind: KeyEventKind, kitty_keyboard_flags: u16) -> Option<u8> {
    if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_EVENTS == 0 {
        return None;
    }

    match kind {
        KeyEventKind::Press => None,
        KeyEventKind::Repeat => Some(2),
        KeyEventKind::Release => Some(3),
    }
}

fn encode_xterm_modify_other_key(key: KeyEvent, modify_other_keys: u8) -> Option<Vec<u8>> {
    if modify_other_keys == 0 {
        return None;
    }
    let modifier = xterm_modifier(key.modifiers)?;
    let key_code = match key.code {
        KeyCode::Char(character) => u32::from(character),
        KeyCode::Enter => 13,
        KeyCode::Tab | KeyCode::BackTab => 9,
        KeyCode::Backspace => 127,
        KeyCode::Esc => 27,
        _ => return None,
    };

    Some(format!("\x1b[27;{modifier};{key_code}~").into_bytes())
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
        KeyCode::Char('5') => b'u',
        KeyCode::KeypadBegin => b'E',
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

fn kitty_modifier(key: KeyEvent) -> Option<u16> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let super_key = key.modifiers.contains(KeyModifiers::SUPER);
    let hyper = key.modifiers.contains(KeyModifiers::HYPER);
    let meta = key.modifiers.contains(KeyModifiers::META);
    let caps_lock = key.state.contains(KeyEventState::CAPS_LOCK);
    let num_lock = key.state.contains(KeyEventState::NUM_LOCK);
    if !(shift || alt || control || super_key || hyper || meta || caps_lock || num_lock) {
        return None;
    }

    Some(
        1 + u16::from(shift)
            + u16::from(alt) * 2
            + u16::from(control) * 4
            + u16::from(super_key) * 8
            + u16::from(hyper) * 16
            + u16::from(meta) * 32
            + u16::from(caps_lock) * 64
            + u16::from(num_lock) * 128,
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MediaKeyCode,
        ModifierKeyCode, MouseButton, MouseEvent, MouseEventKind,
    };

    use crate::terminal_modes::{
        MouseInputMode, MouseProtocolMode, MouseReportingMode, TerminalModeChange,
        TerminalModeTracker,
    };

    use super::{
        InputModes, InputReporting, Osc52Policy, TerminalOutputFilter, encode_input_event,
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
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::KeypadBegin, KeyModifiers::NONE)),
                InputModes::default().with_application_keypad(true),
            )
            .unwrap(),
            b"\x1bOE"
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
    fn encodes_kitty_disambiguated_ascii_keys_when_enabled() {
        let modes = InputModes::default().with_kitty_keyboard_flags(1);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL)),
                modes
            )
            .unwrap(),
            b"\x1b[105;5u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Char('i'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT
                )),
                modes
            )
            .unwrap(),
            b"\x1b[105;6u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT)),
                modes
            )
            .unwrap(),
            b"\x1b[105;3u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE)),
                modes
            )
            .unwrap(),
            b"i"
        );
    }

    #[test]
    fn encodes_kitty_extended_modifier_bits_when_enabled() {
        let disambiguate = InputModes::default().with_kitty_keyboard_flags(1);
        let report_all = InputModes::default().with_kitty_keyboard_flags(8);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('i'),
                    KeyModifiers::SUPER,
                    KeyEventKind::Press,
                    KeyEventState::empty()
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[105;9u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('i'),
                    KeyModifiers::HYPER | KeyModifiers::META,
                    KeyEventKind::Press,
                    KeyEventState::empty()
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[105;49u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Up,
                    KeyModifiers::NONE,
                    KeyEventKind::Press,
                    KeyEventState::NUM_LOCK
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[1;129A"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('a'),
                    KeyModifiers::NONE,
                    KeyEventKind::Press,
                    KeyEventState::CAPS_LOCK | KeyEventState::NUM_LOCK
                )),
                report_all
            )
            .unwrap(),
            b"\x1b[97;193u"
        );
    }

    #[test]
    fn encodes_kitty_modifier_keys_when_enabled() {
        let disambiguate = InputModes::default().with_kitty_keyboard_flags(1);
        let events = InputModes::default().with_kitty_keyboard_flags(2);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Modifier(ModifierKeyCode::LeftShift),
                    KeyModifiers::SHIFT,
                    KeyEventKind::Press,
                    KeyEventState::empty()
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57441;2u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Modifier(ModifierKeyCode::RightSuper),
                    KeyModifiers::SUPER,
                    KeyEventKind::Press,
                    KeyEventState::empty()
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57450;9u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Modifier(ModifierKeyCode::LeftHyper),
                    KeyModifiers::HYPER,
                    KeyEventKind::Press,
                    KeyEventState::empty()
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57445;17u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Modifier(ModifierKeyCode::IsoLevel3Shift),
                    KeyModifiers::NONE,
                    KeyEventKind::Press,
                    KeyEventState::empty()
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57453u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Modifier(ModifierKeyCode::RightMeta),
                    KeyModifiers::NONE,
                    KeyEventKind::Release,
                    KeyEventState::empty()
                )),
                events
            )
            .unwrap(),
            b"\x1b[57452;1:3u"
        );
    }

    #[test]
    fn encodes_kitty_report_all_ascii_and_basic_functional_keys_when_enabled() {
        let modes = InputModes::default().with_kitty_keyboard_flags(8);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
                modes
            )
            .unwrap(),
            b"\x1b[97u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
                modes
            )
            .unwrap(),
            b"\x1b[97;2u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL)),
                modes
            )
            .unwrap(),
            b"\x1b[105;5u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
                modes
            )
            .unwrap(),
            b"\x1b[13u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
                modes
            )
            .unwrap(),
            b"\x1b[9u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
                modes
            )
            .unwrap(),
            b"\x1b[127u"
        );
    }

    #[test]
    fn encodes_kitty_associated_text_when_enabled() {
        let modes = InputModes::default().with_kitty_keyboard_flags(24);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
                modes
            )
            .unwrap(),
            b"\x1b[97;2;65u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
                modes
            )
            .unwrap(),
            b"\x1b[97;;97u"
        );

        let event_modes = InputModes::default().with_kitty_keyboard_flags(26);
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('a'),
                    KeyModifiers::NONE,
                    KeyEventKind::Repeat,
                    KeyEventState::empty()
                )),
                event_modes
            )
            .unwrap(),
            b"\x1b[97;1:2;97u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('a'),
                    KeyModifiers::NONE,
                    KeyEventKind::Release,
                    KeyEventState::empty()
                )),
                event_modes
            )
            .unwrap(),
            b"\x1b[97;1:3u"
        );
    }

    #[test]
    fn encodes_kitty_shifted_alternate_key_when_enabled() {
        let report_all_alternate = InputModes::default().with_kitty_keyboard_flags(12);
        let disambiguate_alternate = InputModes::default().with_kitty_keyboard_flags(5);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
                report_all_alternate
            )
            .unwrap(),
            b"\x1b[97:65;2u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Char('A'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT
                )),
                disambiguate_alternate
            )
            .unwrap(),
            b"\x1b[97:65;6u"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn encodes_kitty_canonical_functional_keys_when_enabled() {
        let disambiguate = InputModes::default().with_kitty_keyboard_flags(1);
        let report_all = InputModes::default().with_kitty_keyboard_flags(8);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)),
                disambiguate
            )
            .unwrap(),
            b"\x1b[P"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
                disambiguate
            )
            .unwrap(),
            b"\x1b[27u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
                disambiguate.with_application_cursor_keys(true)
            )
            .unwrap(),
            b"\x1b[A"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::F(13), KeyModifiers::NONE)),
                report_all
            )
            .unwrap(),
            b"\x1b[57376u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::CapsLock, KeyModifiers::NONE)),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57358u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::ScrollLock, KeyModifiers::NONE)),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57359u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::NumLock, KeyModifiers::NONE)),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57360u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::PrintScreen, KeyModifiers::NONE)),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57361u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Pause, KeyModifiers::NONE)),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57362u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Menu, KeyModifiers::NONE)),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57363u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Media(MediaKeyCode::Play),
                    KeyModifiers::NONE
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57428u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Media(MediaKeyCode::Pause),
                    KeyModifiers::NONE
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57429u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Media(MediaKeyCode::FastForward),
                    KeyModifiers::NONE
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57433u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Media(MediaKeyCode::TrackNext),
                    KeyModifiers::NONE
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57435u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Media(MediaKeyCode::MuteVolume),
                    KeyModifiers::NONE
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57440u"
        );
    }

    #[test]
    fn encodes_kitty_keypad_keys_when_enabled() {
        let disambiguate = InputModes::default().with_kitty_keyboard_flags(1);
        let report_all = InputModes::default().with_kitty_keyboard_flags(8);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    KeyEventKind::Press,
                    KeyEventState::KEYPAD
                )),
                disambiguate
            )
            .unwrap(),
            b"\x1b[57414u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('5'),
                    KeyModifiers::NONE,
                    KeyEventKind::Press,
                    KeyEventState::KEYPAD
                )),
                report_all
            )
            .unwrap(),
            b"\x1b[57404u"
        );
    }

    #[test]
    fn encodes_kitty_event_types_when_enabled() {
        let event_types = InputModes::default().with_kitty_keyboard_flags(2);
        let disambiguate_events = InputModes::default().with_kitty_keyboard_flags(3);
        let report_all_events = InputModes::default().with_kitty_keyboard_flags(10);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Up,
                    KeyModifiers::NONE,
                    KeyEventKind::Repeat,
                    KeyEventState::empty()
                )),
                event_types
            )
            .unwrap(),
            b"\x1b[1;1:2A"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('i'),
                    KeyModifiers::CONTROL,
                    KeyEventKind::Release,
                    KeyEventState::empty()
                )),
                disambiguate_events
            )
            .unwrap(),
            b"\x1b[105;5:3u"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Char('a'),
                    KeyModifiers::NONE,
                    KeyEventKind::Release,
                    KeyEventState::empty()
                )),
                report_all_events
            )
            .unwrap(),
            b"\x1b[97;1:3u"
        );
        assert!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    KeyEventKind::Release,
                    KeyEventState::empty()
                )),
                disambiguate_events
            )
            .is_none()
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new_with_kind_and_state(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    KeyEventKind::Release,
                    KeyEventState::empty()
                )),
                report_all_events
            )
            .unwrap(),
            b"\x1b[13;1:3u"
        );
    }

    #[test]
    fn encodes_xterm_modify_other_keys_when_enabled() {
        let modes = InputModes::default().with_modify_other_keys(2);

        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
                modes
            )
            .unwrap(),
            b"\x1b[27;5;13~"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Char('I'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT
                )),
                modes
            )
            .unwrap(),
            b"\x1b[27;6;73~"
        );
        assert_eq!(
            encode_input_event(
                Event::Key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::ALT)),
                modes
            )
            .unwrap(),
            b"\x1b[27;3;46~"
        );
    }

    #[test]
    fn input_reporting_snapshot_includes_kitty_keyboard_flags() {
        let reporting = InputReporting::default();

        reporting.set_kitty_keyboard_flags(1);

        assert_eq!(reporting.snapshot().kitty_keyboard_flags(), 1);
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
    fn encodes_mouse_events_as_utf8_sequences_when_enabled() {
        let modes = InputModes::default().with_mouse_input_mode(MouseInputMode::new(
            MouseReportingMode::Normal,
            MouseProtocolMode::Utf8,
        ));

        assert_eq!(
            encode_input_event(
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 95,
                    row: 96,
                    modifiers: KeyModifiers::NONE,
                }),
                modes
            )
            .unwrap(),
            b"\x1b[M \xc2\x80\xc2\x81"
        );
        assert_eq!(
            encode_input_event(
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: 95,
                    row: 96,
                    modifiers: KeyModifiers::NONE,
                }),
                modes
            )
            .unwrap(),
            b"\x1b[M#\xc2\x80\xc2\x81"
        );
    }

    #[test]
    fn encodes_mouse_events_as_urxvt_sequences_when_enabled() {
        let modes = InputModes::default().with_mouse_input_mode(MouseInputMode::new(
            MouseReportingMode::Normal,
            MouseProtocolMode::Urxvt,
        ));

        assert_eq!(
            encode_input_event(left_mouse_down(), modes).unwrap(),
            b"\x1b[32;1;2M"
        );
        assert_eq!(
            encode_input_event(left_mouse_release(), modes).unwrap(),
            b"\x1b[35;1;2M"
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
    fn tracks_utf8_and_urxvt_mouse_protocols_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?1000;1005h", |change| changes.push(change));
        tracker.process(b"\x1b[?1005l", |change| changes.push(change));
        tracker.process(b"\x1b[?1015h", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::Utf8,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::Urxvt,
                )),
            ]
        );
    }

    #[test]
    fn reports_extended_mouse_protocol_status_and_leaves_sgr_pixels_unknown() {
        let mut tracker = TerminalModeTracker::default();

        assert_eq!(tracker.private_mode_report_value(1005), 2);
        assert_eq!(tracker.private_mode_report_value(1015), 2);
        assert_eq!(tracker.private_mode_report_value(1016), 0);

        tracker.process(b"\x1b[?1005;1015h", |_| {});

        assert_eq!(tracker.private_mode_report_value(1005), 1);
        assert_eq!(tracker.private_mode_report_value(1015), 1);
        assert_eq!(tracker.private_mode_report_value(1016), 0);

        tracker.process(b"\x1b[?1005;1015l", |_| {});

        assert_eq!(tracker.private_mode_report_value(1005), 2);
        assert_eq!(tracker.private_mode_report_value(1015), 2);
    }

    #[test]
    fn prefers_sgr_then_urxvt_then_utf8_mouse_protocols_when_multiple_are_enabled() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?1000;1005;1015;1006h", |change| {
            changes.push(change);
        });
        tracker.process(b"\x1b[?1006l", |change| changes.push(change));
        tracker.process(b"\x1b[?1015l", |change| changes.push(change));
        tracker.process(b"\x1b[?1005l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::Sgr,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::Urxvt,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::Utf8,
                )),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::Normal,
                    MouseProtocolMode::X10,
                )),
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
    fn ignores_private_input_modes_inside_control_strings() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(
            b"\x1b]0;title \x1b[?1004h\x07\x1bPpayload \x1b[?2004h\x1b\\",
            |change| changes.push(change),
        );

        assert!(changes.is_empty());
        assert!(!tracker.focus_reporting());
        assert!(!tracker.bracketed_paste());
    }

    #[test]
    fn ignores_split_private_input_modes_inside_control_strings() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1bPpayload ", |change| changes.push(change));
        tracker.process(b"\x1b[?1004;2004h\x1b\\", |change| changes.push(change));

        assert!(changes.is_empty());
        assert!(!tracker.focus_reporting());
        assert!(!tracker.bracketed_paste());
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
    fn tracks_synchronized_output_from_pty_output_modes() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?2026h", |change| changes.push(change));
        tracker.process(b"\x1b[?2026l", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::SynchronizedOutput(true),
                TerminalModeChange::SynchronizedOutput(false)
            ]
        );
        assert!(!tracker.synchronized_output());
    }

    #[test]
    fn tracks_c1_private_input_modes_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x9b?1;1004;2004;2026h", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::ApplicationCursorKeys(true),
                TerminalModeChange::Focus(true),
                TerminalModeChange::BracketedPaste(true),
                TerminalModeChange::SynchronizedOutput(true)
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

    #[test]
    fn resets_tracked_modes_on_ris_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(
            b"\x1b[?1;1004;2004;2026h\x1b[?1002;1006h\x1b=\x1b[>1u",
            |_| {},
        );
        tracker.process(b"\x1bc", |change| changes.push(change));

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::ApplicationCursorKeys(false),
                TerminalModeChange::ApplicationKeypad(false),
                TerminalModeChange::BracketedPaste(false),
                TerminalModeChange::Mouse(MouseInputMode::new(
                    MouseReportingMode::None,
                    MouseProtocolMode::X10,
                )),
                TerminalModeChange::Focus(false),
                TerminalModeChange::SynchronizedOutput(false),
                TerminalModeChange::KittyKeyboardFlags(0),
            ]
        );
        assert!(!tracker.application_cursor_keys());
        assert!(!tracker.application_keypad());
        assert!(!tracker.bracketed_paste());
        assert!(!tracker.focus_reporting());
        assert!(!tracker.synchronized_output());
        assert_eq!(tracker.mouse_input_mode(), MouseInputMode::default());
        assert_eq!(tracker.kitty_keyboard_flags(), 0);
    }

    #[test]
    fn soft_reset_restores_insert_and_origin_modes_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[?6h\x1b[4h", |_| {});
        assert_eq!(tracker.private_mode_report_value(6), 1);
        assert_eq!(tracker.ansi_mode_report_value(4), 1);

        tracker.process(b"\x1b[!p", |change| changes.push(change));

        assert!(changes.is_empty());
        assert_eq!(tracker.private_mode_report_value(6), 2);
        assert_eq!(tracker.ansi_mode_report_value(4), 2);
    }

    #[test]
    fn tracks_meta_key_mode_from_pty_output() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        assert_eq!(tracker.private_mode_report_value(1034), 2);

        tracker.process(b"\x1b[?1034h", |change| changes.push(change));

        assert!(changes.is_empty());
        assert_eq!(tracker.private_mode_report_value(1034), 1);

        tracker.process(b"\x1b[?1034l", |change| changes.push(change));

        assert!(changes.is_empty());
        assert_eq!(tracker.private_mode_report_value(1034), 2);
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

    fn mirror_text(filter: &TerminalOutputFilter) -> String {
        let grid = filter.mirror.grid();
        let size = grid.size();
        let mut text = String::new();

        for row in 0..size.rows {
            for column in 0..size.columns {
                text.push(grid.get(row, column).unwrap().ch);
            }
        }

        text
    }

    fn xtgettcap_query(names: &[&[u8]]) -> Vec<u8> {
        let mut query = b"\x1bP+q".to_vec();
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                query.push(b';');
            }
            query.extend_from_slice(&super::encode_ascii_hex(name));
        }
        query.extend_from_slice(b"\x1b\\");
        query
    }

    fn xtgettcap_response(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut response = b"\x1bP1+r".to_vec();
        for (index, (name, value)) in entries.iter().enumerate() {
            if index > 0 {
                response.push(b';');
            }
            response.extend_from_slice(&super::encode_ascii_hex(name));
            response.push(b'=');
            response.extend_from_slice(&super::encode_ascii_hex(value));
        }
        response.extend_from_slice(b"\x1b\\");
        response
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
    fn terminal_output_filter_buffers_synchronized_output_until_mode_resets() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[?2026hmid", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());
        assert!(filter.mode_tracker.synchronized_output());
        assert!(mirror_text(&filter).contains("beforemid"));

        filter
            .write(b"after\x1b[?2026$p", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"before");
        assert_eq!(responses, b"\x1b[?2026;1$y");
        assert!(mirror_text(&filter).contains("beforemidafter"));

        filter
            .write(b"\x1b[?2026l done", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforemidafter done");
        assert_eq!(responses, b"\x1b[?2026;1$y");
        assert!(!filter.mode_tracker.synchronized_output());
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
    fn terminal_output_filter_omits_c1_osc8_hyperlink_sequences() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"a\x9d8;;https://example.com\x9cbc\x9d8;;\x9cd",
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
    fn terminal_output_filter_omits_utf8_c1_osc8_hyperlink_sequences() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                "a\u{9d}8;;https://example.com\u{9c}bc\u{9d}8;;\u{9c}d".as_bytes(),
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
    fn terminal_output_filter_omits_split_c1_osc8_hyperlink_sequences() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"a\x9d8;;https://example.com", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\x9cbc\x9d8;;", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\x9cd", &mut output, |response| {
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
    fn terminal_output_filter_omits_split_utf8_c1_osc8_hyperlink_sequences() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"a\xc2", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\x9d8;;https://example.com", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\xc2", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\x9cbc\xc2\x9d8;;\xc2\x9cd", &mut output, |response| {
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
    fn terminal_output_filter_holds_split_dcs_until_terminated() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1bPignored", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());

        filter
            .write(b"\x1b\\after", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before\x1bPignored\x1b\\after");
        assert!(responses.is_empty());
    }

    #[test]
    fn terminal_output_filter_drops_incomplete_dcs_on_flush() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1bPignored", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());
    }

    #[test]
    fn terminal_output_filter_drops_trailing_escape_prefix_on_flush() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());
    }

    #[test]
    fn terminal_output_filter_ignores_queries_inside_osc_control_strings() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b]0;title \x1b[6n\x07after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before\x1b]0;title \x1b[6n\x07after");
        assert!(responses.is_empty());
    }

    #[test]
    fn terminal_output_filter_holds_split_csi_until_final_byte() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[31", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"before");
        assert!(responses.is_empty());

        filter
            .write(b"mafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before\x1b[31mafter");
        assert!(responses.is_empty());
    }

    #[test]
    fn terminal_output_filter_drops_incomplete_csi_on_flush() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[31", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before");
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
        let metrics = super::LocalMetricsCounters::default();
        let mut output = super::SessionLogWriter::new(&mut screen, Some(&mut log), metrics.clone());

        output.write_all(b"visible").unwrap();
        output.flush().unwrap();

        assert_eq!(screen, b"visible");
        assert_eq!(log, b"visible");
        assert_eq!(metrics.snapshot().terminal_output_bytes, 7);
    }

    #[test]
    fn session_log_writer_omits_bell_from_log() {
        let mut screen = Vec::new();
        let mut log = Vec::new();
        let mut output = super::SessionLogWriter::new(
            &mut screen,
            Some(&mut log),
            super::LocalMetricsCounters::default(),
        );

        output.write_all(b"before\x07after").unwrap();
        output.flush().unwrap();

        assert_eq!(screen, b"before\x07after");
        assert_eq!(log, b"beforeafter");
    }

    #[test]
    fn session_log_writer_omits_title_sequence_from_log() {
        let mut screen = Vec::new();
        let mut log = Vec::new();
        let mut output = super::SessionLogWriter::new(
            &mut screen,
            Some(&mut log),
            super::LocalMetricsCounters::default(),
        );

        output.write_all(b"before\x1b]0;ops\x07after").unwrap();
        output.flush().unwrap();

        assert_eq!(screen, b"before\x1b]0;ops\x07after");
        assert_eq!(log, b"beforeafter");
    }

    #[test]
    fn session_log_writer_omits_split_title_sequence_from_log() {
        let mut screen = Vec::new();
        let mut log = Vec::new();
        let mut output = super::SessionLogWriter::new(
            &mut screen,
            Some(&mut log),
            super::LocalMetricsCounters::default(),
        );

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
    fn terminal_output_filter_answers_kitty_keyboard_protocol_flags_queries_and_tracks_push_pop() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[?u", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        assert_eq!(output, b"before");
        assert_eq!(responses, b"\x1b[?0u");

        filter
            .write(b"\x1b[>1u\x1b[?u", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        assert_eq!(output, b"before");
        assert_eq!(responses, b"\x1b[?0u\x1b[?1u");

        filter
            .write(
                b"\x1b[>9u\x1b[?u\x1b[<u\x1b[?u\x1b[<1u\x1b[?uafter",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[?0u\x1b[?1u\x1b[?9u\x1b[?1u\x1b[?0u");
    }

    #[test]
    fn terminal_output_filter_answers_kitty_keyboard_protocol_flags_queries_and_tracks_set_reset() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b[=1u\x1b[?u\x1b[=8;2u\x1b[?u\x1b[=1;3u\x1b[?uafter",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[?1u\x1b[?9u\x1b[?8u");
    }

    #[test]
    fn terminal_output_filter_answers_modify_other_keys_queries_and_tracks_set_reset() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1b[?4m", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        assert_eq!(output, b"before");
        assert_eq!(responses, b"\x1b[>4;0m");

        filter
            .write(b"\x1b[>4;2m\x1b[?4mafter", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, b"\x1b[>4;0m\x1b[>4;2m");
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
    fn terminal_output_filter_answers_display_private_mode_status_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b[?1034$p \x1b[?1034h\x1b[?1034$p\x1b[?1034l\x1b[?1034$p \
                  \x1b[?7$p \x1b[?7l\x1b[?7$p \
                  \x1b[?25$p \x1b[?25l\x1b[?25$p \
                  \x1b[?6$p \x1b[?6h\x1b[?6$p \
                  \x1b[?47$p \x1b[?47h\x1b[?47$p\x1b[?47l\x1b[?47$p \
                  \x1b[?1048$p \x1b[?1048h\x1b[?1048$p\x1b[?1048l\x1b[?1048$p \
                  \x1b[?1047$p \x1b[?1047h\x1b[?1047$p\x1b[?1047l\x1b[?1047$p \
                  \x1b[?1049$p \x1b[?1049h\x1b[?1049$p\x1b[?1049l\x1b[?1049$p",
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
            b" \x1b[?1034h\x1b[?1034l  \x1b[?7l  \x1b[?25l  \x1b[?6h  \x1b[?47h\x1b[?47l  \x1b[?1048h\x1b[?1048l  \x1b[?1047h\x1b[?1047l  \x1b[?1049h\x1b[?1049l"
        );
        assert_eq!(
            responses,
            b"\x1b[?1034;2$y\x1b[?1034;1$y\x1b[?1034;2$y\
              \x1b[?7;1$y\x1b[?7;2$y\
              \x1b[?25;1$y\x1b[?25;2$y\
              \x1b[?6;2$y\x1b[?6;1$y\
              \x1b[?47;2$y\x1b[?47;1$y\x1b[?47;2$y\
              \x1b[?1048;2$y\x1b[?1048;1$y\x1b[?1048;2$y\
              \x1b[?1047;2$y\x1b[?1047;1$y\x1b[?1047;2$y\
              \x1b[?1049;2$y\x1b[?1049;1$y\x1b[?1049;2$y"
        );
    }

    #[test]
    fn terminal_output_filter_answers_declrmm_mode_status_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b[?69$p\x1b[?69h\x1b[?69$p\x1b[?69l\x1b[?69$p",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x1b[?69h\x1b[?69l");
        assert_eq!(responses, b"\x1b[?69;2$y\x1b[?69;1$y\x1b[?69;2$y");
    }

    #[test]
    fn terminal_output_filter_answers_private_mode_status_defaults_after_terminal_reset() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b[?1;6;25;47;1048;1049;1000;1006;1004;2004h\x1b[?7l\x1b=\x1bc\
                  \x1b[?1$p\x1b[?6$p\x1b[?7$p\x1b[?25$p\x1b[?47$p\x1b[?1048$p\
                  \x1b[?1049$p\x1b[?1000$p\x1b[?1006$p\x1b[?1004$p\x1b[?2004$p",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            responses,
            b"\x1b[?1;2$y\x1b[?6;2$y\x1b[?7;1$y\x1b[?25;1$y\x1b[?47;2$y\x1b[?1048;2$y\x1b[?1049;2$y\x1b[?1000;2$y\x1b[?1006;2$y\x1b[?1004;2$y\x1b[?2004;2$y"
        );
    }

    #[test]
    fn terminal_output_filter_flushes_synchronized_output_on_terminal_reset() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b[?2026hmid\x1bcafter\x1b[?2026$p",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforemid\x1bcafter");
        assert_eq!(responses, b"\x1b[?2026;2$y");
        assert!(!filter.mode_tracker.synchronized_output());
    }

    #[test]
    fn terminal_output_filter_answers_ansi_mode_status_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b[4$p \x1b[4h\x1b[4$p \x1b[4l\x1b[4$p \x1b[999$p",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before \x1b[4h \x1b[4l ");
        assert_eq!(responses, b"\x1b[4;2$y\x1b[4;1$y\x1b[4;2$y\x1b[999;0$y");
        assert!(!mirror_text(&filter).contains("$p"));
    }

    #[test]
    fn terminal_output_filter_answers_c1_ansi_mode_status_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"\x9b4h\x9b4$p", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x9b4h");
        assert_eq!(responses, b"\x1b[4;1$y");
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
    fn terminal_output_filter_answers_utf8_c1_osc_color_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write("\u{9d}4;196;?\u{9c}".as_bytes(), &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert!(output.is_empty());
        assert_eq!(responses, b"\x1b]4;196;rgb:ffff/0000/0000\x9c");
    }

    #[test]
    fn terminal_output_filter_answers_split_utf8_c1_osc_color_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"\xc2", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\x9d4;196;?\xc2", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"\x9c", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert!(output.is_empty());
        assert_eq!(responses, b"\x1b]4;196;rgb:ffff/0000/0000\x9c");
    }

    #[test]
    fn terminal_output_filter_answers_cursor_color_queries_after_changes_and_reset() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b]12;rgb:aa/bb/cc\x07 middle\x1b]12;?\x07 after\x1b]112\x07 reset\x1b]12;?\x1b\\done",
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
            b"before\x1b]12;rgb:aa/bb/cc\x07 middle after\x1b]112\x07 resetdone"
        );
        assert_eq!(
            responses,
            b"\x1b]12;rgb:aaaa/bbbb/cccc\x07\x1b]12;rgb:e5e5/e5e5/e5e5\x1b\\"
        );
    }

    #[test]
    fn terminal_output_filter_answers_c1_cursor_color_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x9d12;rgb:01/02/03\x9c\x9d12;?\x9c",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x9d12;rgb:01/02/03\x9c");
        assert_eq!(responses, b"\x1b]12;rgb:0101/0202/0303\x9c");
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
    fn terminal_output_filter_applies_hex_osc_color_changes() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b]10;#112233\x07\x1b]4;2;#445566\x07\x1b]10;?\x07\x1b]4;2;?\x07",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            responses,
            b"\x1b]10;rgb:1111/2222/3333\x07\x1b]4;2;rgb:4444/5555/6666\x07"
        );
        assert_eq!(output, b"\x1b]10;#112233\x07\x1b]4;2;#445566\x07");
    }

    #[test]
    fn terminal_output_filter_applies_rgba_osc_dynamic_color_changes() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b]10;rgba(127,127,127,0.4)\x07\
                  \x1b]11;rgba:efff/ecff/f4ff/d000\x1b\\\
                  \x1b]12;rgba(1,2,3,1)\x07\
                  \x1b]10;?\x07\x1b]11;?\x1b\\\x1b]12;?\x07",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            responses,
            b"\x1b]10;rgba:7f7f/7f7f/7f7f/6666\x07\x1b]11;rgba:efff/ecff/f4ff/d000\x1b\\\x1b]12;rgba:0101/0202/0303/ffff\x07"
        );
        assert_eq!(
            output,
            b"\x1b]10;rgba(127,127,127,0.4)\x07\x1b]11;rgba:efff/ecff/f4ff/d000\x1b\\\x1b]12;rgba(1,2,3,1)\x07"
        );
    }

    #[test]
    fn terminal_output_filter_applies_multiple_palette_color_changes_from_one_osc4_sequence() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b]4;1;rgb:01/02/03;2;rgb:04/05/06\x07\
                  \x1b]4;1;?\x07\x1b]4;2;?\x07",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            responses,
            b"\x1b]4;1;rgb:0101/0202/0303\x07\x1b]4;2;rgb:0404/0505/0606\x07"
        );
        assert_eq!(output, b"\x1b]4;1;rgb:01/02/03;2;rgb:04/05/06\x07");
    }

    #[test]
    fn terminal_output_filter_answers_multiple_palette_color_queries_from_one_osc4_sequence() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b]4;1;rgb:01/02/03;2;rgb:04/05/06\x07\
                  \x1b]4;1;?;2;?\x07",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            responses,
            b"\x1b]4;1;rgb:0101/0202/0303\x07\x1b]4;2;rgb:0404/0505/0606\x07"
        );
        assert_eq!(output, b"\x1b]4;1;rgb:01/02/03;2;rgb:04/05/06\x07");
    }

    #[test]
    fn terminal_output_filter_resets_dynamic_and_palette_colors() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\
                  \x1b]10;rgb:11/22/33\x07\x1b]11;rgb:44/55/66\x07\
                  \x1b]4;1;rgb:01/02/03\x07\
                  \x1b]10;?\x07\x1b]11;?\x07\x1b]4;1;?\x07\
                  \x1b]110\x07\x1b]111\x07\x1b]104;1\x07\
                  \x1b]10;?\x07\x1b]11;?\x07\x1b]4;1;?\x07after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            responses,
            b"\x1b]10;rgb:1111/2222/3333\x07\
              \x1b]11;rgb:4444/5555/6666\x07\
              \x1b]4;1;rgb:0101/0202/0303\x07\
              \x1b]10;rgb:e5e5/e5e5/e5e5\x07\
              \x1b]11;rgb:0c0c/0c0c/0c0c\x07\
              \x1b]4;1;rgb:cdcd/3131/3131\x07"
        );
        assert!(!String::from_utf8_lossy(&output).contains(";?"));
    }

    #[test]
    fn terminal_output_filter_resets_all_palette_colors() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b]4;1;rgb:01/02/03\x07\x1b]4;2;rgb:04/05/06\x07\
                  \x1b]104\x07\x1b]4;1;?\x07\x1b]4;2;?\x07",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            responses,
            b"\x1b]4;1;rgb:cdcd/3131/3131\x07\x1b]4;2;rgb:0d0d/bcbc/7979\x07"
        );
    }

    #[test]
    fn terminal_output_filter_resets_multiple_palette_colors() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1b]4;1;rgb:01/02/03\x07\x1b]4;2;rgb:04/05/06\x07\x1b]4;3;rgb:07/08/09\x07\
                  \x1b]104;1;2\x07\x1b]4;1;?\x07\x1b]4;2;?\x07\x1b]4;3;?\x07",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(
            responses,
            b"\x1b]4;1;rgb:cdcd/3131/3131\x07\
              \x1b]4;2;rgb:0d0d/bcbc/7979\x07\
              \x1b]4;3;rgb:0707/0808/0909\x07"
        );
    }

    #[test]
    fn terminal_output_filter_ignores_osc_color_changes_inside_st_control_strings() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"\x1bPpayload \x1b]10;rgb:11/22/33\x1b\\ after\x1b]10;?\x07",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x1bPpayload \x1b]10;rgb:11/22/33\x1b\\ after");
        assert_eq!(responses, b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07");
    }

    #[test]
    fn terminal_output_filter_ignores_split_osc_color_changes_inside_st_control_strings() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"\x1bPpayload ", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(
                b"\x1b]10;rgb:11/22/33\x1b\\ after\x1b]10;?\x07",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x1bPpayload \x1b]10;rgb:11/22/33\x1b\\ after");
        assert_eq!(responses, b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07");
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
    fn terminal_output_filter_answers_xtgettcap_official_numeric_capability_names() {
        let mut filter = TerminalOutputFilter::new(rssh_pty::PtySize::try_new(132, 43).unwrap());
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"cols".as_slice(),
            b"lines".as_slice(),
            b"it".as_slice(),
            b"pairs".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"cols".as_slice(), b"132".as_slice()),
                (b"lines".as_slice(), b"43".as_slice()),
                (b"it".as_slice(), b"8".as_slice()),
                (b"pairs".as_slice(), b"32767".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_modern_style_and_color_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"Tc".as_slice(),
            b"Smulx".as_slice(),
            b"Setulc".as_slice(),
            b"sitm".as_slice(),
            b"ritm".as_slice(),
            b"Smol".as_slice(),
            b"smxx".as_slice(),
            b"rmxx".as_slice(),
            b"op".as_slice(),
            b"oc".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"Tc".as_slice(), b"1".as_slice()),
                (b"Smulx".as_slice(), b"\x1b[4:%p1%dm".as_slice()),
                (
                    b"Setulc".as_slice(),
                    b"\x1b[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%d%;m".as_slice()
                ),
                (b"sitm".as_slice(), b"\x1b[3m".as_slice()),
                (b"ritm".as_slice(), b"\x1b[23m".as_slice()),
                (b"Smol".as_slice(), b"\x1b[53m".as_slice()),
                (b"smxx".as_slice(), b"\x1b[9m".as_slice()),
                (b"rmxx".as_slice(), b"\x1b[29m".as_slice()),
                (b"op".as_slice(), b"\x1b[39;49m".as_slice()),
                (b"oc".as_slice(), b"\x1b]104\x07".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_wezterm_official_boolean_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"am".as_slice(),
            b"bce".as_slice(),
            b"ccc".as_slice(),
            b"hs".as_slice(),
            b"mc5i".as_slice(),
            b"mir".as_slice(),
            b"msgr".as_slice(),
            b"npc".as_slice(),
            b"Su".as_slice(),
            b"xenl".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"am".as_slice(), b"1".as_slice()),
                (b"bce".as_slice(), b"1".as_slice()),
                (b"ccc".as_slice(), b"1".as_slice()),
                (b"hs".as_slice(), b"1".as_slice()),
                (b"mc5i".as_slice(), b"1".as_slice()),
                (b"mir".as_slice(), b"1".as_slice()),
                (b"msgr".as_slice(), b"1".as_slice()),
                (b"npc".as_slice(), b"1".as_slice()),
                (b"Su".as_slice(), b"1".as_slice()),
                (b"xenl".as_slice(), b"1".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_wezterm_official_printer_memory_and_reset_templates()
     {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"flash".as_slice(),
            b"mc0".as_slice(),
            b"mc4".as_slice(),
            b"mc5".as_slice(),
            b"meml".as_slice(),
            b"memu".as_slice(),
            b"rs1".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"flash".as_slice(), b"\x1b[?5h$<100/>\x1b[?5l".as_slice()),
                (b"mc0".as_slice(), b"\x1b[i".as_slice()),
                (b"mc4".as_slice(), b"\x1b[4i".as_slice()),
                (b"mc5".as_slice(), b"\x1b[5i".as_slice()),
                (b"meml".as_slice(), b"\x1bl".as_slice()),
                (b"memu".as_slice(), b"\x1bm".as_slice()),
                (b"rs1".as_slice(), b"\x1bc\x1b]104\x07".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_wezterm_title_and_palette_templates() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"dsl".as_slice(),
            b"fsl".as_slice(),
            b"tsl".as_slice(),
            b"initc".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"dsl".as_slice(), b"\x1b]2;\x1b\\".as_slice()),
                (b"fsl".as_slice(), b"\x1b\\".as_slice()),
                (b"tsl".as_slice(), b"\x1b]0;".as_slice()),
                (
                    b"initc".as_slice(),
                    b"\x1b]4;%p1%d;rgb:%p2%{255}%*%{1000}%/%2.2X/%p3%{255}%*%{1000}%/%2.2X/%p4%{255}%*%{1000}%/%2.2X\x1b\\".as_slice()
                ),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_tmux_cursor_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"Cr".as_slice(),
            b"Cs".as_slice(),
            b"Se".as_slice(),
            b"Ss".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"Cr".as_slice(), b"\x1b]112\x1b\\".as_slice()),
                (b"Cs".as_slice(), b"\x1b]12;%p1%s\x1b\\".as_slice()),
                (b"Se".as_slice(), b"\x1b[2 q".as_slice()),
                (b"Ss".as_slice(), b"\x1b[%p1%d q".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_synchronized_output_capability() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[b"Sync".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[(
                b"Sync".as_slice(),
                b"\x1b[?2026%?%p1%{1}%-%tl%eh%;".as_slice()
            )])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_mouse_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[b"kmous".as_slice(), b"XM".as_slice(), b"xm".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"kmous".as_slice(), b"\x1b[<".as_slice()),
                (
                    b"XM".as_slice(),
                    b"\x1b[?1006;1000%?%p1%{1}%=%th%el%;".as_slice()
                ),
                (
                    b"xm".as_slice(),
                    b"\x1b[<%i%p3%d;%p1%d;%p2%d;%?%p4%tM%em%;".as_slice()
                ),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_foundational_terminal_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"clear".as_slice(),
            b"cup".as_slice(),
            b"home".as_slice(),
            b"civis".as_slice(),
            b"cnorm".as_slice(),
            b"cvvis".as_slice(),
            b"smcup".as_slice(),
            b"rmcup".as_slice(),
            b"sgr0".as_slice(),
            b"sgr".as_slice(),
            b"bold".as_slice(),
            b"dim".as_slice(),
            b"blink".as_slice(),
            b"rev".as_slice(),
            b"smso".as_slice(),
            b"rmso".as_slice(),
            b"invis".as_slice(),
            b"smul".as_slice(),
            b"rmul".as_slice(),
            b"setaf".as_slice(),
            b"setab".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"clear".as_slice(), b"\x1b[H\x1b[2J".as_slice()),
                (b"cup".as_slice(), b"\x1b[%i%p1%d;%p2%dH".as_slice()),
                (b"home".as_slice(), b"\x1b[H".as_slice()),
                (b"civis".as_slice(), b"\x1b[?25l".as_slice()),
                (b"cnorm".as_slice(), b"\x1b[?12l\x1b[?25h".as_slice()),
                (b"cvvis".as_slice(), b"\x1b[?12;25h".as_slice()),
                (b"smcup".as_slice(), b"\x1b[?1049h\x1b[22;0;0t".as_slice()),
                (b"rmcup".as_slice(), b"\x1b[?1049l\x1b[23;0;0t".as_slice()),
                (b"sgr0".as_slice(), b"\x1b(B\x1b[m".as_slice()),
                (
                    b"sgr".as_slice(),
                    b"%?%p9%t\x1b(0%e\x1b(B%;\x1b[0%?%p6%t;1%;%?%p5%t;2%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p7%t;8%;m".as_slice()
                ),
                (b"bold".as_slice(), b"\x1b[1m".as_slice()),
                (b"dim".as_slice(), b"\x1b[2m".as_slice()),
                (b"blink".as_slice(), b"\x1b[5m".as_slice()),
                (b"rev".as_slice(), b"\x1b[7m".as_slice()),
                (b"smso".as_slice(), b"\x1b[7m".as_slice()),
                (b"rmso".as_slice(), b"\x1b[27m".as_slice()),
                (b"invis".as_slice(), b"\x1b[8m".as_slice()),
                (b"smul".as_slice(), b"\x1b[4m".as_slice()),
                (b"rmul".as_slice(), b"\x1b[24m".as_slice()),
                (
                    b"setaf".as_slice(),
                    b"\x1b[%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m".as_slice()
                ),
                (
                    b"setab".as_slice(),
                    b"\x1b[%?%p1%{8}%<%t4%p1%d%e%p1%{16}%<%t10%p1%{8}%-%d%e48;5;%p1%d%;m".as_slice()
                ),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_common_control_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"el".as_slice(),
            b"ed".as_slice(),
            b"el1".as_slice(),
            b"dch1".as_slice(),
            b"ich1".as_slice(),
            b"il1".as_slice(),
            b"dl1".as_slice(),
            b"cuu".as_slice(),
            b"cud".as_slice(),
            b"cub".as_slice(),
            b"cuf".as_slice(),
            b"hpa".as_slice(),
            b"vpa".as_slice(),
            b"cbt".as_slice(),
            b"ht".as_slice(),
            b"hts".as_slice(),
            b"tbc".as_slice(),
            b"ech".as_slice(),
            b"rep".as_slice(),
            b"csr".as_slice(),
            b"indn".as_slice(),
            b"rin".as_slice(),
            b"smir".as_slice(),
            b"rmir".as_slice(),
            b"smam".as_slice(),
            b"rmam".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"el".as_slice(), b"\x1b[K".as_slice()),
                (b"ed".as_slice(), b"\x1b[J".as_slice()),
                (b"el1".as_slice(), b"\x1b[1K".as_slice()),
                (b"dch1".as_slice(), b"\x1b[P".as_slice()),
                (b"ich1".as_slice(), b"\x1b[@".as_slice()),
                (b"il1".as_slice(), b"\x1b[L".as_slice()),
                (b"dl1".as_slice(), b"\x1b[M".as_slice()),
                (b"cuu".as_slice(), b"\x1b[%p1%dA".as_slice()),
                (b"cud".as_slice(), b"\x1b[%p1%dB".as_slice()),
                (b"cub".as_slice(), b"\x1b[%p1%dD".as_slice()),
                (b"cuf".as_slice(), b"\x1b[%p1%dC".as_slice()),
                (b"hpa".as_slice(), b"\x1b[%i%p1%dG".as_slice()),
                (b"vpa".as_slice(), b"\x1b[%i%p1%dd".as_slice()),
                (b"cbt".as_slice(), b"\x1b[Z".as_slice()),
                (b"ht".as_slice(), b"\t".as_slice()),
                (b"hts".as_slice(), b"\x1bH".as_slice()),
                (b"tbc".as_slice(), b"\x1b[3g".as_slice()),
                (b"ech".as_slice(), b"\x1b[%p1%dX".as_slice()),
                (b"rep".as_slice(), b"%p1%c\x1b[%p2%{1}%-%db".as_slice()),
                (b"csr".as_slice(), b"\x1b[%i%p1%d;%p2%dr".as_slice()),
                (b"indn".as_slice(), b"\x1b[%p1%dS".as_slice()),
                (b"rin".as_slice(), b"\x1b[%p1%dT".as_slice()),
                (b"smir".as_slice(), b"\x1b[4h".as_slice()),
                (b"rmir".as_slice(), b"\x1b[4l".as_slice()),
                (b"smam".as_slice(), b"\x1b[?7h".as_slice()),
                (b"rmam".as_slice(), b"\x1b[?7l".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_common_key_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"kcuu1".as_slice(),
            b"kcud1".as_slice(),
            b"kcuf1".as_slice(),
            b"kcub1".as_slice(),
            b"kb2".as_slice(),
            b"kbs".as_slice(),
            b"kcbt".as_slice(),
            b"khome".as_slice(),
            b"kend".as_slice(),
            b"kich1".as_slice(),
            b"kdch1".as_slice(),
            b"kpp".as_slice(),
            b"knp".as_slice(),
            b"kHOM".as_slice(),
            b"kEND".as_slice(),
            b"kIC".as_slice(),
            b"kDC".as_slice(),
            b"kPRV".as_slice(),
            b"kNXT".as_slice(),
            b"kLFT".as_slice(),
            b"kRIT".as_slice(),
            b"kri".as_slice(),
            b"kind".as_slice(),
            b"kent".as_slice(),
            b"kf1".as_slice(),
            b"kf2".as_slice(),
            b"kf3".as_slice(),
            b"kf4".as_slice(),
            b"kf5".as_slice(),
            b"kf6".as_slice(),
            b"kf7".as_slice(),
            b"kf8".as_slice(),
            b"kf9".as_slice(),
            b"kf10".as_slice(),
            b"kf11".as_slice(),
            b"kf12".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"kcuu1".as_slice(), b"\x1bOA".as_slice()),
                (b"kcud1".as_slice(), b"\x1bOB".as_slice()),
                (b"kcuf1".as_slice(), b"\x1bOC".as_slice()),
                (b"kcub1".as_slice(), b"\x1bOD".as_slice()),
                (b"kb2".as_slice(), b"\x1bOE".as_slice()),
                (b"kbs".as_slice(), b"\x7f".as_slice()),
                (b"kcbt".as_slice(), b"\x1b[Z".as_slice()),
                (b"khome".as_slice(), b"\x1bOH".as_slice()),
                (b"kend".as_slice(), b"\x1bOF".as_slice()),
                (b"kich1".as_slice(), b"\x1b[2~".as_slice()),
                (b"kdch1".as_slice(), b"\x1b[3~".as_slice()),
                (b"kpp".as_slice(), b"\x1b[5~".as_slice()),
                (b"knp".as_slice(), b"\x1b[6~".as_slice()),
                (b"kHOM".as_slice(), b"\x1b[1;2H".as_slice()),
                (b"kEND".as_slice(), b"\x1b[1;2F".as_slice()),
                (b"kIC".as_slice(), b"\x1b[2;2~".as_slice()),
                (b"kDC".as_slice(), b"\x1b[3;2~".as_slice()),
                (b"kPRV".as_slice(), b"\x1b[5;2~".as_slice()),
                (b"kNXT".as_slice(), b"\x1b[6;2~".as_slice()),
                (b"kLFT".as_slice(), b"\x1b[1;2D".as_slice()),
                (b"kRIT".as_slice(), b"\x1b[1;2C".as_slice()),
                (b"kri".as_slice(), b"\x1b[1;2A".as_slice()),
                (b"kind".as_slice(), b"\x1b[1;2B".as_slice()),
                (b"kent".as_slice(), b"\x1bOM".as_slice()),
                (b"kf1".as_slice(), b"\x1bOP".as_slice()),
                (b"kf2".as_slice(), b"\x1bOQ".as_slice()),
                (b"kf3".as_slice(), b"\x1bOR".as_slice()),
                (b"kf4".as_slice(), b"\x1bOS".as_slice()),
                (b"kf5".as_slice(), b"\x1b[15~".as_slice()),
                (b"kf6".as_slice(), b"\x1b[17~".as_slice()),
                (b"kf7".as_slice(), b"\x1b[18~".as_slice()),
                (b"kf8".as_slice(), b"\x1b[19~".as_slice()),
                (b"kf9".as_slice(), b"\x1b[20~".as_slice()),
                (b"kf10".as_slice(), b"\x1b[21~".as_slice()),
                (b"kf11".as_slice(), b"\x1b[23~".as_slice()),
                (b"kf12".as_slice(), b"\x1b[24~".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_wezterm_keypad_transmit_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[b"smkx".as_slice(), b"rmkx".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"smkx".as_slice(), b"\x1b[?1h\x1b=".as_slice()),
                (b"rmkx".as_slice(), b"\x1b[?1l\x1b>".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_modified_function_key_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let entries: &[(&[u8], &[u8])] = &[
            (b"kf13".as_slice(), b"\x1b[1;2P".as_slice()),
            (b"kf14".as_slice(), b"\x1b[1;2Q".as_slice()),
            (b"kf15".as_slice(), b"\x1b[1;2R".as_slice()),
            (b"kf16".as_slice(), b"\x1b[1;2S".as_slice()),
            (b"kf17".as_slice(), b"\x1b[15;2~".as_slice()),
            (b"kf18".as_slice(), b"\x1b[17;2~".as_slice()),
            (b"kf19".as_slice(), b"\x1b[18;2~".as_slice()),
            (b"kf20".as_slice(), b"\x1b[19;2~".as_slice()),
            (b"kf21".as_slice(), b"\x1b[20;2~".as_slice()),
            (b"kf22".as_slice(), b"\x1b[21;2~".as_slice()),
            (b"kf23".as_slice(), b"\x1b[23;2~".as_slice()),
            (b"kf24".as_slice(), b"\x1b[24;2~".as_slice()),
            (b"kf25".as_slice(), b"\x1b[1;5P".as_slice()),
            (b"kf26".as_slice(), b"\x1b[1;5Q".as_slice()),
            (b"kf27".as_slice(), b"\x1b[1;5R".as_slice()),
            (b"kf28".as_slice(), b"\x1b[1;5S".as_slice()),
            (b"kf29".as_slice(), b"\x1b[15;5~".as_slice()),
            (b"kf30".as_slice(), b"\x1b[17;5~".as_slice()),
            (b"kf31".as_slice(), b"\x1b[18;5~".as_slice()),
            (b"kf32".as_slice(), b"\x1b[19;5~".as_slice()),
            (b"kf33".as_slice(), b"\x1b[20;5~".as_slice()),
            (b"kf34".as_slice(), b"\x1b[21;5~".as_slice()),
            (b"kf35".as_slice(), b"\x1b[23;5~".as_slice()),
            (b"kf36".as_slice(), b"\x1b[24;5~".as_slice()),
            (b"kf37".as_slice(), b"\x1b[1;6P".as_slice()),
            (b"kf38".as_slice(), b"\x1b[1;6Q".as_slice()),
            (b"kf39".as_slice(), b"\x1b[1;6R".as_slice()),
            (b"kf40".as_slice(), b"\x1b[1;6S".as_slice()),
            (b"kf41".as_slice(), b"\x1b[15;6~".as_slice()),
            (b"kf42".as_slice(), b"\x1b[17;6~".as_slice()),
            (b"kf43".as_slice(), b"\x1b[18;6~".as_slice()),
            (b"kf44".as_slice(), b"\x1b[19;6~".as_slice()),
            (b"kf45".as_slice(), b"\x1b[20;6~".as_slice()),
            (b"kf46".as_slice(), b"\x1b[21;6~".as_slice()),
            (b"kf47".as_slice(), b"\x1b[23;6~".as_slice()),
            (b"kf48".as_slice(), b"\x1b[24;6~".as_slice()),
            (b"kf49".as_slice(), b"\x1b[1;3P".as_slice()),
            (b"kf50".as_slice(), b"\x1b[1;3Q".as_slice()),
            (b"kf51".as_slice(), b"\x1b[1;3R".as_slice()),
            (b"kf52".as_slice(), b"\x1b[1;3S".as_slice()),
            (b"kf53".as_slice(), b"\x1b[15;3~".as_slice()),
            (b"kf54".as_slice(), b"\x1b[17;3~".as_slice()),
            (b"kf55".as_slice(), b"\x1b[18;3~".as_slice()),
            (b"kf56".as_slice(), b"\x1b[19;3~".as_slice()),
            (b"kf57".as_slice(), b"\x1b[20;3~".as_slice()),
            (b"kf58".as_slice(), b"\x1b[21;3~".as_slice()),
            (b"kf59".as_slice(), b"\x1b[23;3~".as_slice()),
            (b"kf60".as_slice(), b"\x1b[24;3~".as_slice()),
            (b"kf61".as_slice(), b"\x1b[1;4P".as_slice()),
            (b"kf62".as_slice(), b"\x1b[1;4Q".as_slice()),
            (b"kf63".as_slice(), b"\x1b[1;4R".as_slice()),
        ];
        let names: Vec<&[u8]> = entries.iter().map(|(name, _)| *name).collect();
        let query = xtgettcap_query(&names);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(responses, xtgettcap_response(entries));
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_acs_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"enacs".as_slice(),
            b"smacs".as_slice(),
            b"rmacs".as_slice(),
            b"acsc".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"enacs".as_slice(), b"\x1b)0".as_slice()),
                (b"smacs".as_slice(), b"\x1b(0".as_slice()),
                (b"rmacs".as_slice(), b"\x1b(B".as_slice()),
                (
                    b"acsc".as_slice(),
                    b"``aaffggiijjkkllmmnnooppqqrrssttuuvvwwxxyyzz{{||}}~~".as_slice()
                ),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_wezterm_control_sequence_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"bel".as_slice(),
            b"cr".as_slice(),
            b"ind".as_slice(),
            b"ri".as_slice(),
            b"sc".as_slice(),
            b"rc".as_slice(),
            b"cuu1".as_slice(),
            b"cud1".as_slice(),
            b"cuf1".as_slice(),
            b"cub1".as_slice(),
            b"dch".as_slice(),
            b"ich".as_slice(),
            b"dl".as_slice(),
            b"il".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"bel".as_slice(), b"\x07".as_slice()),
                (b"cr".as_slice(), b"\r".as_slice()),
                (b"ind".as_slice(), b"\n".as_slice()),
                (b"ri".as_slice(), b"\x1bM".as_slice()),
                (b"sc".as_slice(), b"\x1b7".as_slice()),
                (b"rc".as_slice(), b"\x1b8".as_slice()),
                (b"cuu1".as_slice(), b"\x1b[A".as_slice()),
                (b"cud1".as_slice(), b"\n".as_slice()),
                (b"cuf1".as_slice(), b"\x1b[C".as_slice()),
                (b"cub1".as_slice(), b"\x08".as_slice()),
                (b"dch".as_slice(), b"\x1b[%p1%dP".as_slice()),
                (b"ich".as_slice(), b"\x1b[%p1%d@".as_slice()),
                (b"dl".as_slice(), b"\x1b[%p1%dM".as_slice()),
                (b"il".as_slice(), b"\x1b[%p1%dL".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_wezterm_meta_key_capabilities() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[b"km".as_slice(), b"smm".as_slice(), b"rmm".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"km".as_slice(), b"1".as_slice()),
                (b"smm".as_slice(), b"\x1b[?1034h".as_slice()),
                (b"rmm".as_slice(), b"\x1b[?1034l".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_wezterm_reset_templates() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[b"is2".as_slice(), b"rs2".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (
                    b"is2".as_slice(),
                    b"\x1b[!p\x1b[?3;4l\x1b[4l\x1b>".as_slice()
                ),
                (
                    b"rs2".as_slice(),
                    b"\x1b[!p\x1b[?3;4l\x1b[4l\x1b>".as_slice()
                ),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_xtgettcap_wezterm_query_templates() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let query = xtgettcap_query(&[
            b"u6".as_slice(),
            b"u7".as_slice(),
            b"u8".as_slice(),
            b"u9".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        filter
            .write(&input, &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"beforeafter");
        assert_eq!(
            responses,
            xtgettcap_response(&[
                (b"u6".as_slice(), b"\x1b[%i%d;%dR".as_slice()),
                (b"u7".as_slice(), b"\x1b[6n".as_slice()),
                (b"u8".as_slice(), b"\x1b[?%[;0123456789]c".as_slice()),
                (b"u9".as_slice(), b"\x1b[c".as_slice()),
            ])
        );
    }

    #[test]
    fn terminal_output_filter_answers_decrqss_state_queries() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b[1;2;4:3;5;8;9;53;74;58;5;34;38;6;4;5;6;7;48;2;1;2;3m\x1bP$qm\x1b\\ middle\x1b[5 q\x90$q q\x9c after\x1b[2;5r\x1bP$qr\x1b\\done",
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
            b"before\x1b[1;2;4:3;5;8;9;53;74;58;5;34;38;6;4;5;6;7;48;2;1;2;3m middle\x1b[5 q after\x1b[2;5rdone"
        );
        assert_eq!(
            responses,
            b"\x1bP1$r1;2;4:3;5;8;9;53;74;58;5;34;38;6;4;5;6;7;48;2;1;2;3m\x1b\\\x1bP1$r5 q\x9c\x1bP1$r2;5r\x1b\\"
        );
    }

    #[test]
    fn terminal_output_filter_answers_wezterm_decrqss_conformance_and_left_right_margins() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1bP$q\"p\x1b\\ middle\x90$qs\x9c after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middle after");
        assert_eq!(responses, b"\x1bP1$r61;1\"p\x1b\\\x1bP1$r1;80s\x9c");
    }

    #[test]
    fn terminal_output_filter_answers_split_wezterm_decrqss_conformance_and_left_right_margins() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(b"before\x1bP$q\"", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"p\x1b\\ middle\x90$q", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter
            .write(b"s\x9c after", &mut output, |response| {
                responses.extend_from_slice(response);
                Ok(())
            })
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before middle after");
        assert_eq!(responses, b"\x1bP1$r61;1\"p\x1b\\\x1bP1$r1;80s\x9c");
    }

    #[test]
    fn terminal_output_filter_answers_decrqss_left_right_margin_query_from_declrmm_state() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write(
                b"before\x1b[?69h\x1b[3;6s\x1bP$qs\x1b\\after",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"before\x1b[?69h\x1b[3;6safter");
        assert_eq!(responses, b"\x1bP1$r3;6s\x1b\\");
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
    fn terminal_output_filter_writes_c1_osc52_clipboard_text() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x9d52;c;Y29weQ==\x9cafter",
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
    fn terminal_output_filter_writes_utf8_c1_osc52_clipboard_text() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                "before\u{9d}52;c;Y29weQ==\u{9c}after".as_bytes(),
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
    fn terminal_output_filter_ignores_osc52_inside_control_strings() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"\x1b]0;title \x1b]52;c;Y29weQ==\x07\x1bPpayload \x1b]52;c;?\x1b\\done",
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

        assert_eq!(
            output,
            b"\x1b]0;title \x1b]52;c;Y29weQ==\x07\x1bPpayload \x1b]52;c;?\x1b\\done"
        );
        assert!(responses.is_empty());
        assert!(writes.is_empty());
    }

    #[test]
    fn terminal_output_filter_writes_split_c1_osc52_clipboard_text() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x9d52;c;Y2",
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
        assert_eq!(output, b"before");
        assert!(writes.is_empty());

        filter
            .write_with_clipboard(
                b"9weQ==\x9cafter",
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
    fn terminal_output_filter_writes_split_utf8_c1_osc52_clipboard_text() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();
        let mut writes = Vec::new();

        filter
            .write_with_clipboard(
                b"before\xc2",
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
        filter
            .write_with_clipboard(
                b"\x9d52;c;Y29weQ==\xc2",
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
        filter
            .write_with_clipboard(
                b"\x9cafter",
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
    fn terminal_output_filter_answers_c1_osc52_clipboard_query() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write_with_clipboard(
                b"before\x9d52;c;?\x9cafter",
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
    fn terminal_output_filter_answers_utf8_c1_osc52_clipboard_query() {
        let mut filter = TerminalOutputFilter::default();
        let mut output = Vec::new();
        let mut responses = Vec::new();

        filter
            .write_with_clipboard(
                "before\u{9d}52;c;?\u{9c}after".as_bytes(),
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
                b"\x9b?1000;1006h\x1b[?2004;2026h\x9b?1000$p \x9b?1006$p \x9b?2004$p \x1b[?2026$p",
                &mut output,
                |response| {
                    responses.extend_from_slice(response);
                    Ok(())
                },
            )
            .unwrap();
        filter.flush(&mut output).unwrap();

        assert_eq!(output, b"\x9b?1000;1006h   ");
        assert_eq!(
            responses,
            b"\x1b[?1000;1$y\x1b[?1006;1$y\x1b[?2004;1$y\x1b[?2026;1$y"
        );
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
