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
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::Window,
};

use crate::{
    cli::WindowOptions,
    terminal_input::{TerminalKey, encode_terminal_key},
    terminal_runtime::TerminalRuntime,
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

    fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        let lines = scrollback_lines_from_mouse_delta(delta);
        if lines == 0 {
            return false;
        }

        self.scroll_viewport_lines(lines);
        true
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
            WindowEvent::MouseWheel { delta, .. } => {
                if self.handle_mouse_wheel(delta) {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
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
    use winit::event::MouseScrollDelta;
    use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};

    use super::{
        NativeWindowApp, demo_snapshot, encode_window_focus_event, encode_window_key,
        terminal_size_from_window_pixels,
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
