use std::{
    error::Error,
    io::{self, Read, Write},
    sync::Arc,
    thread,
};

use pixels::{Pixels, SurfaceTexture};
use rssh_core::TerminalSize;
use rssh_pty::{PtyCommand, PtySession, PtySize};
use rssh_renderer::{PixelRenderer, TerminalRenderSnapshot};
#[cfg(test)]
use rssh_terminal::Terminal;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::Window,
};

use crate::{
    cli::WindowOptions,
    terminal_input::{TerminalKey, encode_terminal_key},
    terminal_runtime::{MouseInputMode, MouseProtocolMode, MouseReportingMode, TerminalRuntime},
};

const TERMINAL_COLUMNS: u16 = 80;
const TERMINAL_ROWS: u16 = 24;
const CELL_WIDTH: u32 = 8;
const CELL_HEIGHT: u32 = 16;
const DEFAULT_WINDOW_TITLE: &str = "R-SSH";
const FRAME_WIDTH: u32 = TERMINAL_COLUMNS as u32 * CELL_WIDTH;
const FRAME_HEIGHT: u32 = TERMINAL_ROWS as u32 * CELL_HEIGHT;

pub fn run(options: &WindowOptions) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<WindowUserEvent>::with_user_event().build()?;
    let event_proxy = event_loop.create_proxy();
    let mut app = NativeWindowApp::with_event_proxy(options.frame_limit, event_proxy);

    event_loop.run_app(&mut app)?;

    Ok(())
}

#[cfg(test)]
pub fn demo_snapshot() -> TerminalRenderSnapshot {
    let mut terminal = Terminal::new(TerminalSize::new(TERMINAL_COLUMNS, TERMINAL_ROWS));
    terminal.feed(b"\x1b[1;32mR-SSH\x1b[0m native renderer\r\n");
    terminal.feed(b"winit window + renderer terminal grid");

    TerminalRenderSnapshot::from_terminal(&terminal)
}

struct NativeWindowApp {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    renderer: PixelRenderer,
    runtime: TerminalRuntime,
    snapshot: TerminalRenderSnapshot,
    window_title: String,
    frame_width: u32,
    frame_height: u32,
    frame_limit: Option<u64>,
    rendered_frames: u64,
    event_proxy: Option<EventLoopProxy<WindowUserEvent>>,
    session: Option<PtySession>,
    writer: Option<Box<dyn Write + Send>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    modifiers: ModifiersState,
    scrollback_offset: usize,
    mouse_position: Option<(u16, u16)>,
    active_mouse_button: Option<MouseButton>,
}

#[derive(Debug)]
enum WindowUserEvent {
    Output(Vec<u8>),
    Exited,
    ReadError(String),
}

impl NativeWindowApp {
    fn new(frame_limit: Option<u64>) -> Self {
        let runtime = TerminalRuntime::new(TerminalSize::new(TERMINAL_COLUMNS, TERMINAL_ROWS));
        let snapshot = TerminalRenderSnapshot::from_terminal(runtime.terminal());

        Self {
            window: None,
            pixels: None,
            renderer: PixelRenderer::new(),
            runtime,
            snapshot,
            window_title: DEFAULT_WINDOW_TITLE.to_owned(),
            frame_width: FRAME_WIDTH,
            frame_height: FRAME_HEIGHT,
            frame_limit,
            rendered_frames: 0,
            event_proxy: None,
            session: None,
            writer: None,
            reader_thread: None,
            modifiers: ModifiersState::empty(),
            scrollback_offset: 0,
            mouse_position: None,
            active_mouse_button: None,
        }
    }

    fn with_event_proxy(
        frame_limit: Option<u64>,
        event_proxy: EventLoopProxy<WindowUserEvent>,
    ) -> Self {
        let mut app = Self::new(frame_limit);
        app.event_proxy = Some(event_proxy);
        app
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(self.window_title.clone())
                    .with_inner_size(LogicalSize::new(
                        f64::from(FRAME_WIDTH),
                        f64::from(FRAME_HEIGHT),
                    )),
            )?,
        );
        let size = window.inner_size();
        let surface_texture = SurfaceTexture::new(size.width, size.height, window.clone());
        let pixels = Pixels::new(self.frame_width, self.frame_height, surface_texture)?;

        self.window = Some(window);
        self.pixels = Some(pixels);

        Ok(())
    }

    fn draw_frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };

        self.renderer.render(
            &self.snapshot,
            pixels.frame_mut(),
            self.frame_width,
            self.frame_height,
            CELL_WIDTH,
            CELL_HEIGHT,
        );

        if let Err(error) = pixels.render() {
            eprintln!("render error: {error}");
            event_loop.exit();
            return;
        }

        self.rendered_frames = self.rendered_frames.saturating_add(1);
        if self
            .frame_limit
            .is_some_and(|limit| self.rendered_frames >= limit)
        {
            event_loop.exit();
        }
    }

    fn handle_pty_output(&mut self, bytes: &[u8]) -> io::Result<()> {
        for response in self.runtime.feed_pty_output(bytes) {
            self.write_pty_bytes(&response)?;
        }
        self.sync_window_title_from_runtime();
        self.refresh_snapshot();

        Ok(())
    }

    fn refresh_snapshot(&mut self) {
        self.scrollback_offset = self
            .scrollback_offset
            .min(self.runtime.terminal().scrollback().len());
        self.snapshot = TerminalRenderSnapshot::from_terminal_viewport(
            self.runtime.terminal(),
            self.scrollback_offset,
        );
    }

    fn scroll_viewport_lines(&mut self, lines: isize) {
        let history_len = self.runtime.terminal().scrollback().len();
        let next_offset = if lines.is_positive() {
            self.scrollback_offset
                .saturating_add(lines.unsigned_abs())
                .min(history_len)
        } else {
            self.scrollback_offset.saturating_sub(lines.unsigned_abs())
        };

        if next_offset == self.scrollback_offset {
            return;
        }

        self.scrollback_offset = next_offset;
        self.refresh_snapshot();
    }

    fn set_scrollback_offset(&mut self, offset: usize) {
        let next_offset = offset.min(self.runtime.terminal().scrollback().len());
        if next_offset == self.scrollback_offset {
            return;
        }

        self.scrollback_offset = next_offset;
        self.refresh_snapshot();
    }

    fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        let lines = scrollback_lines_from_mouse_delta(delta);
        if lines == 0 {
            return false;
        }

        self.scroll_viewport_lines(lines);
        true
    }

    fn handle_window_mouse_wheel(&mut self, delta: MouseScrollDelta) -> io::Result<bool> {
        let mode = self.runtime.mouse_input_mode();
        if mode.reporting_enabled() {
            if let Some((column, row)) = self.mouse_position {
                let Some(kind) = window_mouse_wheel_kind(delta) else {
                    return Ok(false);
                };
                if let Some(bytes) = encode_window_mouse_event(
                    WindowMouseEvent {
                        kind,
                        column,
                        row,
                        modifiers: self.modifiers,
                    },
                    mode,
                ) {
                    self.write_pty_bytes(&bytes)?;
                    return Ok(true);
                }
            }
            return Ok(false);
        }

        Ok(self.handle_mouse_wheel(delta))
    }

    fn handle_mouse_input(&mut self, state: ElementState, button: MouseButton) -> io::Result<bool> {
        let kind = match state {
            ElementState::Pressed => WindowMouseEventKind::Down(button),
            ElementState::Released => WindowMouseEventKind::Up(button),
        };
        self.update_active_mouse_button(state, button);

        let mode = self.runtime.mouse_input_mode();
        if !mode.reporting_enabled() {
            return Ok(false);
        }

        let Some((column, row)) = self.mouse_position else {
            return Ok(false);
        };

        let Some(bytes) = encode_window_mouse_event(
            WindowMouseEvent {
                kind,
                column,
                row,
                modifiers: self.modifiers,
            },
            mode,
        ) else {
            return Ok(false);
        };

        self.write_pty_bytes(&bytes)?;
        Ok(true)
    }

    fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) -> io::Result<bool> {
        let next_position = window_mouse_cell(position);
        if self.mouse_position == next_position {
            return Ok(false);
        }
        self.mouse_position = next_position;

        let mode = self.runtime.mouse_input_mode();
        if !mode.reporting_enabled() {
            return Ok(false);
        }

        let Some((column, row)) = self.mouse_position else {
            return Ok(false);
        };
        let kind = match self.active_mouse_button {
            Some(button) => WindowMouseEventKind::Drag(button),
            None => WindowMouseEventKind::Moved,
        };

        let Some(bytes) = encode_window_mouse_event(
            WindowMouseEvent {
                kind,
                column,
                row,
                modifiers: self.modifiers,
            },
            mode,
        ) else {
            return Ok(false);
        };

        self.write_pty_bytes(&bytes)?;
        Ok(true)
    }

    fn update_active_mouse_button(&mut self, state: ElementState, button: MouseButton) {
        if window_mouse_button_code(button).is_none() {
            return;
        }

        match state {
            ElementState::Pressed => self.active_mouse_button = Some(button),
            ElementState::Released if self.active_mouse_button == Some(button) => {
                self.active_mouse_button = None;
            }
            ElementState::Released => {}
        }
    }

    fn handle_scrollback_shortcut(&mut self, key: &Key, modifiers: ModifiersState) -> bool {
        if modifiers != ModifiersState::SHIFT {
            return false;
        }

        let Key::Named(named) = key else {
            return false;
        };

        match named {
            NamedKey::PageUp => {
                self.scroll_viewport_lines(self.viewport_page_rows());
                true
            }
            NamedKey::PageDown => {
                self.scroll_viewport_lines(-self.viewport_page_rows());
                true
            }
            NamedKey::Home => {
                self.set_scrollback_offset(self.runtime.terminal().scrollback().len());
                true
            }
            NamedKey::End => {
                self.set_scrollback_offset(0);
                true
            }
            _ => false,
        }
    }

    fn viewport_page_rows(&self) -> isize {
        isize::try_from(i32::from(self.runtime.terminal().grid().size().rows)).unwrap_or(isize::MAX)
    }

    fn sync_window_title_from_runtime(&mut self) {
        let Some(title) = self.runtime.terminal().title().map(str::to_owned) else {
            return;
        };

        if self.window_title == title {
            return;
        }

        self.window_title = title;
        if let Some(window) = &self.window {
            window.set_title(&self.window_title);
        }
    }

    fn spawn_pty(&mut self) -> Result<(), Box<dyn Error>> {
        if self.session.is_some() {
            return Ok(());
        }

        let Some(event_proxy) = self.event_proxy.clone() else {
            return Err(Box::new(io::Error::other(
                "window event proxy is not configured",
            )));
        };

        let size = PtySize::try_new(TERMINAL_COLUMNS, TERMINAL_ROWS)?;
        let mut session = PtySession::spawn(&PtyCommand::default_shell(), size)?;
        let mut reader = session.take_reader()?;
        let writer = session.take_writer()?;

        let reader_thread = thread::spawn(move || {
            let mut buffer = [0; 8192];

            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = event_proxy.send_event(WindowUserEvent::Exited);
                        break;
                    }
                    Ok(count) => {
                        if event_proxy
                            .send_event(WindowUserEvent::Output(buffer[..count].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        let _ =
                            event_proxy.send_event(WindowUserEvent::ReadError(error.to_string()));
                        break;
                    }
                }
            }
        });

        self.writer = Some(writer);
        self.reader_thread = Some(reader_thread);
        self.session = Some(session);

        Ok(())
    }

    fn write_pty_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };

        writer.write_all(bytes)?;
        writer.flush()?;

        Ok(())
    }

    fn handle_keyboard_input(&mut self, key: &winit::event::KeyEvent) -> io::Result<()> {
        if key.state != ElementState::Pressed {
            return Ok(());
        }

        if window_paste_shortcut(&key.logical_key, self.modifiers) {
            self.handle_window_paste()?;
            return Ok(());
        }

        if self.handle_scrollback_shortcut(&key.logical_key, self.modifiers) {
            return Ok(());
        }

        let bytes = encode_window_key(
            &key.logical_key,
            key.physical_key,
            key.text.as_deref(),
            self.modifiers,
            self.runtime.application_cursor_keys(),
            self.runtime.application_keypad(),
        );
        if !bytes.is_empty() {
            self.write_pty_bytes(&bytes)?;
        }

        Ok(())
    }

    fn handle_window_paste(&mut self) -> io::Result<bool> {
        let Some(text) = read_window_clipboard_text() else {
            return Ok(false);
        };
        if text.is_empty() {
            return Ok(false);
        }

        let bytes = encode_window_paste(&text, self.runtime.bracketed_paste());
        self.write_pty_bytes(&bytes)?;
        Ok(true)
    }

    fn handle_focus_changed(&mut self, focused: bool) -> io::Result<()> {
        if let Some(bytes) = encode_window_focus_event(focused, self.runtime.focus_reporting()) {
            self.write_pty_bytes(&bytes)?;
        }

        Ok(())
    }

    fn handle_window_resize(&mut self, size: PhysicalSize<u32>) -> Result<(), Box<dyn Error>> {
        if let Some(pixels) = self.pixels.as_mut() {
            pixels.resize_surface(size.width, size.height)?;
        }

        let terminal_size = terminal_size_from_window_pixels(size.width, size.height);
        self.frame_width = u32::from(terminal_size.columns) * CELL_WIDTH;
        self.frame_height = u32::from(terminal_size.rows) * CELL_HEIGHT;

        if let Some(pixels) = self.pixels.as_mut() {
            pixels.resize_buffer(self.frame_width, self.frame_height)?;
        }

        self.runtime.resize(terminal_size);
        if let Some(session) = self.session.as_mut() {
            let pty_size = PtySize::try_new(terminal_size.columns, terminal_size.rows)?;
            session.resize(pty_size)?;
        }
        self.refresh_snapshot();

        Ok(())
    }
}

impl Drop for NativeWindowApp {
    fn drop(&mut self) {
        self.writer.take();

        if let Some(session) = self.session.as_mut() {
            let _ = session.kill();
            let _ = session.wait();
        }
        self.session.take();

        if let Some(reader_thread) = self.reader_thread.take() {
            let _ = reader_thread.join();
        }
    }
}

fn encode_window_key(
    key: &Key,
    physical_key: PhysicalKey,
    text: Option<&str>,
    modifiers: ModifiersState,
    application_cursor_keys: bool,
    application_keypad: bool,
) -> Vec<u8> {
    let alt = modifiers.alt_key();

    if modifiers.control_key() {
        if let Key::Character(character) = key.as_ref() {
            if let Some(character) = character.chars().next() {
                let mut bytes =
                    encode_terminal_key(TerminalKey::Control(character)).unwrap_or_default();
                if alt {
                    bytes.insert(0, 0x1b);
                }
                return bytes;
            }
        }
    }

    if let Some(bytes) = encode_modified_window_key(key, modifiers) {
        return bytes;
    }

    if application_keypad {
        if let Some(bytes) = encode_application_keypad_key(physical_key) {
            return bytes;
        }
    }

    if application_cursor_keys {
        if let Some(bytes) = encode_application_cursor_key(key) {
            return bytes;
        }
    }

    if modifiers.shift_key() && matches!(key, Key::Named(NamedKey::Tab)) {
        return encode_terminal_key(TerminalKey::BackTab).unwrap_or_default();
    }

    if let Some(key) = named_terminal_key(key) {
        return encode_terminal_key(key).unwrap_or_default();
    }

    let mut bytes: Vec<u8> = text
        .unwrap_or_default()
        .chars()
        .filter_map(|character| encode_terminal_key(TerminalKey::Text(character)))
        .flatten()
        .collect();
    if alt && !bytes.is_empty() {
        bytes.insert(0, 0x1b);
    }

    bytes
}

fn scrollback_lines_from_mouse_delta(delta: MouseScrollDelta) -> isize {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => signed_scroll_direction(y),
        MouseScrollDelta::PixelDelta(position) => {
            signed_scroll_direction(position.y / f64::from(CELL_HEIGHT))
        }
    }
}

fn signed_scroll_direction(value: impl Into<f64>) -> isize {
    let value = value.into();
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

#[derive(Clone, Copy)]
struct WindowMouseEvent {
    kind: WindowMouseEventKind,
    column: u16,
    row: u16,
    modifiers: ModifiersState,
}

#[derive(Clone, Copy)]
enum WindowMouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

fn encode_window_mouse_event(event: WindowMouseEvent, mode: MouseInputMode) -> Option<Vec<u8>> {
    if !window_mouse_reporting_allows(event.kind, mode.reporting()) {
        return None;
    }

    let mut code = match event.kind {
        WindowMouseEventKind::Down(button) | WindowMouseEventKind::Up(button) => {
            window_mouse_button_code(button)?
        }
        WindowMouseEventKind::Drag(button) => window_mouse_button_code(button)? + 32,
        WindowMouseEventKind::Moved => 35,
        WindowMouseEventKind::ScrollUp => 64,
        WindowMouseEventKind::ScrollDown => 65,
        WindowMouseEventKind::ScrollLeft => 66,
        WindowMouseEventKind::ScrollRight => 67,
    };

    if event.modifiers.shift_key() {
        code += 4;
    }
    if event.modifiers.alt_key() {
        code += 8;
    }
    if event.modifiers.control_key() {
        code += 16;
    }

    let column = event.column.checked_add(1)?;
    let row = event.row.checked_add(1)?;

    match mode.protocol() {
        MouseProtocolMode::Sgr => {
            let final_byte = if matches!(event.kind, WindowMouseEventKind::Up(_)) {
                b'm'
            } else {
                b'M'
            };
            Some(format!("\x1b[<{code};{column};{row}{}", final_byte as char).into_bytes())
        }
        MouseProtocolMode::X10 => encode_legacy_window_mouse_event(event.kind, code, column, row),
    }
}

fn window_mouse_reporting_allows(
    kind: WindowMouseEventKind,
    reporting: MouseReportingMode,
) -> bool {
    match reporting {
        MouseReportingMode::None => false,
        MouseReportingMode::Normal => matches!(
            kind,
            WindowMouseEventKind::Down(_)
                | WindowMouseEventKind::Up(_)
                | WindowMouseEventKind::ScrollUp
                | WindowMouseEventKind::ScrollDown
                | WindowMouseEventKind::ScrollLeft
                | WindowMouseEventKind::ScrollRight
        ),
        MouseReportingMode::ButtonEvent => !matches!(kind, WindowMouseEventKind::Moved),
        MouseReportingMode::AnyEvent => true,
    }
}

fn encode_legacy_window_mouse_event(
    kind: WindowMouseEventKind,
    mut code: u16,
    column: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if matches!(kind, WindowMouseEventKind::Up(_)) {
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

const fn window_mouse_button_code(button: MouseButton) -> Option<u16> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

fn window_mouse_wheel_kind(delta: MouseScrollDelta) -> Option<WindowMouseEventKind> {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => wheel_kind_from_axes(f64::from(x), f64::from(y)),
        MouseScrollDelta::PixelDelta(position) => wheel_kind_from_axes(position.x, position.y),
    }
}

fn wheel_kind_from_axes(x: f64, y: f64) -> Option<WindowMouseEventKind> {
    if y > 0.0 {
        Some(WindowMouseEventKind::ScrollUp)
    } else if y < 0.0 {
        Some(WindowMouseEventKind::ScrollDown)
    } else if x > 0.0 {
        Some(WindowMouseEventKind::ScrollRight)
    } else if x < 0.0 {
        Some(WindowMouseEventKind::ScrollLeft)
    } else {
        None
    }
}

fn window_mouse_cell(position: PhysicalPosition<f64>) -> Option<(u16, u16)> {
    Some((
        pixel_axis_to_cell(position.x, CELL_WIDTH)?,
        pixel_axis_to_cell(position.y, CELL_HEIGHT)?,
    ))
}

fn pixel_axis_to_cell(value: f64, cell_size: u32) -> Option<u16> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }

    let cell = (value / f64::from(cell_size)).floor();
    if cell > f64::from(u16::MAX) {
        return None;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(cell as u16)
}

fn encode_modified_window_key(key: &Key, modifiers: ModifiersState) -> Option<Vec<u8>> {
    let modifier = xterm_window_modifier(modifiers)?;

    let Key::Named(named) = key else {
        return None;
    };

    match named {
        NamedKey::ArrowLeft => Some(format!("\x1b[1;{modifier}D").into_bytes()),
        NamedKey::ArrowRight => Some(format!("\x1b[1;{modifier}C").into_bytes()),
        NamedKey::ArrowUp => Some(format!("\x1b[1;{modifier}A").into_bytes()),
        NamedKey::ArrowDown => Some(format!("\x1b[1;{modifier}B").into_bytes()),
        NamedKey::Home => Some(format!("\x1b[1;{modifier}H").into_bytes()),
        NamedKey::End => Some(format!("\x1b[1;{modifier}F").into_bytes()),
        NamedKey::Insert => Some(format!("\x1b[2;{modifier}~").into_bytes()),
        NamedKey::Delete => Some(format!("\x1b[3;{modifier}~").into_bytes()),
        NamedKey::PageUp => Some(format!("\x1b[5;{modifier}~").into_bytes()),
        NamedKey::PageDown => Some(format!("\x1b[6;{modifier}~").into_bytes()),
        NamedKey::F1 => Some(format!("\x1b[1;{modifier}P").into_bytes()),
        NamedKey::F2 => Some(format!("\x1b[1;{modifier}Q").into_bytes()),
        NamedKey::F3 => Some(format!("\x1b[1;{modifier}R").into_bytes()),
        NamedKey::F4 => Some(format!("\x1b[1;{modifier}S").into_bytes()),
        NamedKey::F5 => Some(format!("\x1b[15;{modifier}~").into_bytes()),
        NamedKey::F6 => Some(format!("\x1b[17;{modifier}~").into_bytes()),
        NamedKey::F7 => Some(format!("\x1b[18;{modifier}~").into_bytes()),
        NamedKey::F8 => Some(format!("\x1b[19;{modifier}~").into_bytes()),
        NamedKey::F9 => Some(format!("\x1b[20;{modifier}~").into_bytes()),
        NamedKey::F10 => Some(format!("\x1b[21;{modifier}~").into_bytes()),
        NamedKey::F11 => Some(format!("\x1b[23;{modifier}~").into_bytes()),
        NamedKey::F12 => Some(format!("\x1b[24;{modifier}~").into_bytes()),
        _ => None,
    }
}

fn xterm_window_modifier(modifiers: ModifiersState) -> Option<u8> {
    let shift = modifiers.shift_key();
    let alt = modifiers.alt_key();
    let control = modifiers.control_key();
    if !(shift || alt || control) {
        return None;
    }

    Some(1 + u8::from(shift) + u8::from(alt) * 2 + u8::from(control) * 4)
}

fn encode_application_keypad_key(physical_key: PhysicalKey) -> Option<Vec<u8>> {
    let PhysicalKey::Code(code) = physical_key else {
        return None;
    };

    let final_byte = match code {
        WinitKeyCode::NumpadEnter => b'M',
        WinitKeyCode::NumpadMultiply => b'j',
        WinitKeyCode::NumpadAdd => b'k',
        WinitKeyCode::NumpadComma => b'l',
        WinitKeyCode::NumpadSubtract => b'm',
        WinitKeyCode::NumpadDecimal => b'n',
        WinitKeyCode::NumpadDivide => b'o',
        WinitKeyCode::Numpad0 => b'p',
        WinitKeyCode::Numpad1 => b'q',
        WinitKeyCode::Numpad2 => b'r',
        WinitKeyCode::Numpad3 => b's',
        WinitKeyCode::Numpad4 => b't',
        WinitKeyCode::Numpad5 => b'u',
        WinitKeyCode::Numpad6 => b'v',
        WinitKeyCode::Numpad7 => b'w',
        WinitKeyCode::Numpad8 => b'x',
        WinitKeyCode::Numpad9 => b'y',
        WinitKeyCode::NumpadEqual => b'X',
        _ => return None,
    };

    Some(vec![0x1b, b'O', final_byte])
}

fn encode_application_cursor_key(key: &Key) -> Option<Vec<u8>> {
    let Key::Named(named) = key else {
        return None;
    };

    match named {
        NamedKey::ArrowUp => Some(b"\x1bOA".to_vec()),
        NamedKey::ArrowDown => Some(b"\x1bOB".to_vec()),
        NamedKey::ArrowRight => Some(b"\x1bOC".to_vec()),
        NamedKey::ArrowLeft => Some(b"\x1bOD".to_vec()),
        _ => None,
    }
}

fn encode_window_focus_event(focused: bool, focus_reporting: bool) -> Option<Vec<u8>> {
    if !focus_reporting {
        return None;
    }

    Some(if focused {
        b"\x1b[I".to_vec()
    } else {
        b"\x1b[O".to_vec()
    })
}

fn encode_window_paste(text: &str, bracketed_paste: bool) -> Vec<u8> {
    if !bracketed_paste {
        return text.as_bytes().to_vec();
    }

    let mut bytes = Vec::with_capacity(b"\x1b[200~".len() + text.len() + b"\x1b[201~".len());
    bytes.extend_from_slice(b"\x1b[200~");
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

fn window_paste_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    let ctrl_v = modifiers.control_key()
        && !modifiers.alt_key()
        && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("v"));
    let shift_insert =
        modifiers == ModifiersState::SHIFT && matches!(key, Key::Named(NamedKey::Insert));

    ctrl_v || shift_insert
}

fn read_window_clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn named_terminal_key(key: &Key) -> Option<TerminalKey> {
    let Key::Named(named) = key else {
        return None;
    };

    match named {
        NamedKey::Enter => Some(TerminalKey::Enter),
        NamedKey::Backspace => Some(TerminalKey::Backspace),
        NamedKey::Tab => Some(TerminalKey::Tab),
        NamedKey::Escape => Some(TerminalKey::Escape),
        NamedKey::ArrowLeft => Some(TerminalKey::Left),
        NamedKey::ArrowRight => Some(TerminalKey::Right),
        NamedKey::ArrowUp => Some(TerminalKey::Up),
        NamedKey::ArrowDown => Some(TerminalKey::Down),
        NamedKey::Home => Some(TerminalKey::Home),
        NamedKey::End => Some(TerminalKey::End),
        NamedKey::Delete => Some(TerminalKey::Delete),
        NamedKey::Insert => Some(TerminalKey::Insert),
        NamedKey::PageUp => Some(TerminalKey::PageUp),
        NamedKey::PageDown => Some(TerminalKey::PageDown),
        NamedKey::F1 => Some(TerminalKey::Function(1)),
        NamedKey::F2 => Some(TerminalKey::Function(2)),
        NamedKey::F3 => Some(TerminalKey::Function(3)),
        NamedKey::F4 => Some(TerminalKey::Function(4)),
        NamedKey::F5 => Some(TerminalKey::Function(5)),
        NamedKey::F6 => Some(TerminalKey::Function(6)),
        NamedKey::F7 => Some(TerminalKey::Function(7)),
        NamedKey::F8 => Some(TerminalKey::Function(8)),
        NamedKey::F9 => Some(TerminalKey::Function(9)),
        NamedKey::F10 => Some(TerminalKey::Function(10)),
        NamedKey::F11 => Some(TerminalKey::Function(11)),
        NamedKey::F12 => Some(TerminalKey::Function(12)),
        _ => None,
    }
}

fn terminal_size_from_window_pixels(width: u32, height: u32) -> TerminalSize {
    let columns = u16::try_from((width / CELL_WIDTH).clamp(1, u32::from(u16::MAX)))
        .expect("column count is clamped to u16");
    let rows = u16::try_from((height / CELL_HEIGHT).clamp(1, u32::from(u16::MAX)))
        .expect("row count is clamped to u16");

    TerminalSize::new(columns, rows)
}

impl ApplicationHandler<WindowUserEvent> for NativeWindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        if let Err(error) = self.create_window(event_loop) {
            eprintln!("window error: {error}");
            event_loop.exit();
            return;
        }

        if let Err(error) = self.spawn_pty() {
            eprintln!("PTY error: {error}");
            event_loop.exit();
            return;
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: WindowUserEvent) {
        match event {
            WindowUserEvent::Output(bytes) => {
                if let Err(error) = self.handle_pty_output(&bytes) {
                    eprintln!("PTY write error: {error}");
                    event_loop.exit();
                    return;
                }

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowUserEvent::Exited => event_loop.exit(),
            WindowUserEvent::ReadError(error) => {
                eprintln!("PTY read error: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let Err(error) = self.handle_keyboard_input(&event) {
                    eprintln!("PTY input error: {error}");
                    event_loop.exit();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Focused(focused) => {
                if let Err(error) = self.handle_focus_changed(focused) {
                    eprintln!("PTY focus error: {error}");
                    event_loop.exit();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Err(error) = self.handle_cursor_moved(position) {
                    eprintln!("PTY mouse error: {error}");
                    event_loop.exit();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Err(error) = self.handle_mouse_input(state, button) {
                    eprintln!("PTY mouse error: {error}");
                    event_loop.exit();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => match self.handle_window_mouse_wheel(delta) {
                Ok(true) => {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    eprintln!("PTY mouse error: {error}");
                    event_loop.exit();
                }
            },
            WindowEvent::Resized(size) => {
                if let Err(error) = self.handle_window_resize(size) {
                    eprintln!("resize error: {error}");
                    event_loop.exit();
                }
            }
            WindowEvent::RedrawRequested => {
                self.draw_frame(event_loop);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use winit::event::{MouseButton, MouseScrollDelta};
    use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};

    use crate::terminal_runtime::{MouseInputMode, MouseProtocolMode, MouseReportingMode};

    use super::{
        NativeWindowApp, WindowMouseEvent, WindowMouseEventKind, demo_snapshot,
        encode_window_focus_event, encode_window_key, encode_window_mouse_event,
        encode_window_paste, terminal_size_from_window_pixels, window_paste_shortcut,
    };

    #[test]
    fn demo_snapshot_contains_visible_terminal_cells() {
        let snapshot = demo_snapshot();

        assert!(!snapshot.cells().is_empty());
    }

    #[test]
    fn encodes_window_text_input_for_pty() {
        let bytes = encode_window_key(
            &Key::Character("中".into()),
            PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified),
            Some("中"),
            ModifiersState::empty(),
            false,
            false,
        );

        assert_eq!(bytes, "中".as_bytes());
    }

    #[test]
    fn encodes_window_control_input_for_pty() {
        let bytes = encode_window_key(
            &Key::Character("c".into()),
            PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified),
            None,
            ModifiersState::CONTROL,
            false,
            false,
        );

        assert_eq!(bytes, vec![3]);
    }

    #[test]
    fn encodes_window_alt_text_with_escape_prefix() {
        let bytes = encode_window_key(
            &Key::Character("x".into()),
            PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified),
            Some("x"),
            ModifiersState::ALT,
            false,
            false,
        );

        assert_eq!(bytes, b"\x1bx");
    }

    #[test]
    fn encodes_window_paste_as_raw_or_bracketed_bytes() {
        assert_eq!(encode_window_paste("plain\ntext", false), b"plain\ntext");
        assert_eq!(
            encode_window_paste("plain\ntext", true),
            b"\x1b[200~plain\ntext\x1b[201~"
        );
    }

    #[test]
    fn recognizes_window_paste_shortcuts() {
        assert!(window_paste_shortcut(
            &Key::Character("v".into()),
            ModifiersState::CONTROL
        ));
        assert!(window_paste_shortcut(
            &Key::Character("V".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(window_paste_shortcut(
            &Key::Named(NamedKey::Insert),
            ModifiersState::SHIFT
        ));
        assert!(!window_paste_shortcut(
            &Key::Character("v".into()),
            ModifiersState::empty()
        ));
    }

    #[test]
    fn encodes_window_named_keys_for_pty() {
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::ArrowUp),
                PhysicalKey::Code(WinitKeyCode::ArrowUp),
                None,
                ModifiersState::empty(),
                false,
                false
            ),
            b"\x1b[A"
        );
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::Enter),
                PhysicalKey::Code(WinitKeyCode::Enter),
                Some("\r"),
                ModifiersState::empty(),
                false,
                false
            ),
            b"\r"
        );
    }

    #[test]
    fn encodes_window_modified_navigation_and_function_keys() {
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::ArrowLeft),
                PhysicalKey::Code(WinitKeyCode::ArrowLeft),
                None,
                ModifiersState::CONTROL,
                false,
                false
            ),
            b"\x1b[1;5D"
        );
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::Delete),
                PhysicalKey::Code(WinitKeyCode::Delete),
                None,
                ModifiersState::SHIFT | ModifiersState::ALT,
                false,
                false
            ),
            b"\x1b[3;4~"
        );
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::F5),
                PhysicalKey::Code(WinitKeyCode::F5),
                None,
                ModifiersState::SHIFT,
                false,
                false
            ),
            b"\x1b[15;2~"
        );
    }

    #[test]
    fn encodes_window_application_cursor_keys_when_enabled() {
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::ArrowUp),
                PhysicalKey::Code(WinitKeyCode::ArrowUp),
                None,
                ModifiersState::empty(),
                true,
                false
            ),
            b"\x1bOA"
        );
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::ArrowUp),
                PhysicalKey::Code(WinitKeyCode::ArrowUp),
                None,
                ModifiersState::CONTROL,
                true,
                false
            ),
            b"\x1b[1;5A"
        );
    }

    #[test]
    fn encodes_window_focus_events_when_enabled() {
        assert_eq!(
            encode_window_focus_event(true, true),
            Some(b"\x1b[I".to_vec())
        );
        assert_eq!(
            encode_window_focus_event(false, true),
            Some(b"\x1b[O".to_vec())
        );
        assert_eq!(encode_window_focus_event(true, false), None);
    }

    #[test]
    fn encodes_window_mouse_events_as_sgr_sequences_when_enabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Sgr);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[<0;3;2M"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Up(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[<0;3;2m"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::ScrollDown,
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[<65;3;2M"
        );
    }

    #[test]
    fn encodes_window_mouse_events_as_legacy_sequences_when_sgr_is_disabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::X10);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[M !!"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Up(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[M#!!"
        );
    }

    #[test]
    fn encodes_window_mouse_drag_and_motion_events_when_enabled() {
        let button_event_mode =
            MouseInputMode::new(MouseReportingMode::ButtonEvent, MouseProtocolMode::Sgr);
        let any_event_mode =
            MouseInputMode::new(MouseReportingMode::AnyEvent, MouseProtocolMode::Sgr);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Drag(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                button_event_mode,
            )
            .unwrap(),
            b"\x1b[<32;3;2M"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Moved,
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                any_event_mode,
            )
            .unwrap(),
            b"\x1b[<35;3;2M"
        );
    }

    #[test]
    fn ignores_window_mouse_motion_events_outside_matching_reporting_modes() {
        let normal_mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Sgr);
        let button_event_mode =
            MouseInputMode::new(MouseReportingMode::ButtonEvent, MouseProtocolMode::Sgr);

        assert!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Drag(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                normal_mode,
            )
            .is_none()
        );
        assert!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Moved,
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                button_event_mode,
            )
            .is_none()
        );
    }

    #[test]
    fn ignores_window_mouse_events_when_reporting_is_disabled() {
        assert!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: ModifiersState::empty(),
                },
                MouseInputMode::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn encodes_window_application_keypad_keys_when_enabled() {
        assert_eq!(
            encode_window_key(
                &Key::Character("1".into()),
                PhysicalKey::Code(WinitKeyCode::Numpad1),
                Some("1"),
                ModifiersState::empty(),
                false,
                true
            ),
            b"\x1bOq"
        );
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::Enter),
                PhysicalKey::Code(WinitKeyCode::NumpadEnter),
                Some("\r"),
                ModifiersState::empty(),
                false,
                true
            ),
            b"\x1bOM"
        );
        assert!(
            encode_window_key(
                &Key::Character("1".into()),
                PhysicalKey::Code(WinitKeyCode::Numpad1),
                Some("1"),
                ModifiersState::CONTROL,
                false,
                true
            )
            .is_empty()
        );
    }

    #[test]
    fn encodes_window_backtab_and_function_keys_for_pty() {
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::Tab),
                PhysicalKey::Code(WinitKeyCode::Tab),
                Some("\t"),
                ModifiersState::SHIFT,
                false,
                false
            ),
            b"\x1b[Z"
        );
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::F12),
                PhysicalKey::Code(WinitKeyCode::F12),
                None,
                ModifiersState::empty(),
                false,
                false
            ),
            b"\x1b[24~"
        );
    }

    #[test]
    fn window_app_rebuilds_snapshot_from_pty_output() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"live").unwrap();

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('l'));
        assert_eq!(snapshot_char(&app.snapshot, 0, 3), Some('e'));
    }

    #[test]
    fn window_app_scrolls_snapshot_to_scrollback_lines() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));

        app.handle_pty_output(b"ab\ncd\nef").unwrap();
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('c'));

        app.scroll_viewport_lines(1);

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));
        assert_eq!(snapshot_char(&app.snapshot, 1, 0), Some('c'));
        assert!(app.snapshot.cursor().is_none());

        app.scroll_viewport_lines(-1);

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('c'));
        assert!(app.snapshot.cursor().is_some());
    }

    #[test]
    fn window_app_clamps_scrollback_viewport_to_available_history() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"ab\ncd\nef").unwrap();

        app.scroll_viewport_lines(99);
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));

        app.scroll_viewport_lines(-99);
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('c'));
    }

    #[test]
    fn window_app_mouse_wheel_scrolls_scrollback_viewport() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"ab\ncd\nef").unwrap();

        assert!(app.handle_mouse_wheel(MouseScrollDelta::LineDelta(0.0, 1.0)));

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));

        assert!(app.handle_mouse_wheel(MouseScrollDelta::LineDelta(0.0, -1.0)));

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('c'));
    }

    #[test]
    fn window_app_shift_page_keys_scroll_scrollback_viewport() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee").unwrap();

        assert!(
            app.handle_scrollback_shortcut(&Key::Named(NamedKey::PageUp), ModifiersState::SHIFT)
        );

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('b'));

        assert!(
            app.handle_scrollback_shortcut(&Key::Named(NamedKey::PageDown), ModifiersState::SHIFT)
        );

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('d'));
    }

    #[test]
    fn window_app_shift_home_end_jump_scrollback_viewport() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee").unwrap();

        assert!(app.handle_scrollback_shortcut(&Key::Named(NamedKey::Home), ModifiersState::SHIFT));

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));

        assert!(app.handle_scrollback_shortcut(&Key::Named(NamedKey::End), ModifiersState::SHIFT));

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('d'));
        assert!(app.snapshot.cursor().is_some());
    }

    #[test]
    fn window_app_unmodified_page_key_stays_available_for_pty() {
        let mut app = NativeWindowApp::new(None);

        assert!(
            !app.handle_scrollback_shortcut(&Key::Named(NamedKey::PageUp), ModifiersState::empty())
        );
    }

    #[test]
    fn window_app_tracks_runtime_title_from_pty_output() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"\x1b]2;PowerShell\x07").unwrap();

        assert_eq!(app.window_title, "PowerShell");
    }

    #[test]
    fn derives_terminal_size_from_window_pixels() {
        assert_eq!(
            terminal_size_from_window_pixels(640, 384),
            rssh_core::TerminalSize::new(80, 24)
        );
        assert_eq!(
            terminal_size_from_window_pixels(1, 1),
            rssh_core::TerminalSize::new(1, 1)
        );
    }

    fn snapshot_char(
        snapshot: &rssh_renderer::TerminalRenderSnapshot,
        row: u16,
        column: u16,
    ) -> Option<char> {
        snapshot
            .cells()
            .iter()
            .find(|cell| cell.row == row && cell.column == column)
            .map(|cell| cell.ch)
    }
}
