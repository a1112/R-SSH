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
    selection: Option<WindowSelection>,
    selecting: bool,
    search: Option<WindowSearch>,
    clipboard_writer: Box<dyn FnMut(&str) -> bool + Send>,
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
            selection: None,
            selecting: false,
            search: None,
            clipboard_writer: Box::new(write_window_clipboard_text),
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
        for text in self.runtime.take_clipboard_texts() {
            self.write_clipboard_text(&text);
        }
        self.sync_window_title_from_runtime();
        self.refresh_snapshot();

        Ok(())
    }

    fn refresh_snapshot(&mut self) {
        self.scrollback_offset = self
            .scrollback_offset
            .min(self.runtime.terminal().scrollback().len());
        let snapshot = TerminalRenderSnapshot::from_terminal_viewport(
            self.runtime.terminal(),
            self.scrollback_offset,
        );
        let size = self.runtime.terminal().grid().size();
        self.snapshot = if let Some(selection) = self.selection {
            snapshot.with_inverse_overlay(|row, column| selection.contains(row, column, size))
        } else {
            snapshot
        };
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
        self.selection = None;
        self.search = None;
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn set_scrollback_offset(&mut self, offset: usize) {
        let next_offset = offset.min(self.runtime.terminal().scrollback().len());
        if next_offset == self.scrollback_offset {
            return;
        }

        self.scrollback_offset = next_offset;
        self.selection = None;
        self.search = None;
        self.refresh_snapshot();
        self.apply_window_title();
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
            return Ok(self.handle_selection_mouse_input(state, button));
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
            return Ok(self.update_selection_from_mouse_position());
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

    fn handle_selection_mouse_input(&mut self, state: ElementState, button: MouseButton) -> bool {
        if button != MouseButton::Left {
            return false;
        }

        match state {
            ElementState::Pressed => {
                let Some(cell) = self.selection_cell_from_mouse_position() else {
                    return false;
                };
                self.search = None;
                self.selection = Some(WindowSelection::new(cell, cell));
                self.selecting = true;
                self.refresh_snapshot();
                self.apply_window_title();
                true
            }
            ElementState::Released => {
                if !self.selecting {
                    return false;
                }
                self.selecting = false;
                if self.selection.is_some_and(WindowSelection::is_single_cell) {
                    self.selection = None;
                }
                self.refresh_snapshot();
                self.apply_window_title();
                true
            }
        }
    }

    fn update_selection_from_mouse_position(&mut self) -> bool {
        if !self.selecting {
            return false;
        }
        let Some(cell) = self.selection_cell_from_mouse_position() else {
            return false;
        };
        let Some(selection) = self.selection.as_mut() else {
            return false;
        };
        if selection.focus == cell {
            return false;
        }

        selection.set_focus(cell);
        self.refresh_snapshot();
        true
    }

    fn selection_cell_from_mouse_position(&self) -> Option<SelectionCell> {
        let (column, row) = self.mouse_position?;
        let size = self.runtime.terminal().grid().size();
        if row >= size.rows || column >= size.columns {
            return None;
        }

        Some(SelectionCell { row, column })
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
        self.apply_window_title();
    }

    fn effective_window_title(&self) -> String {
        let Some(search) = &self.search else {
            return self.window_title.clone();
        };

        if search.query.is_empty() {
            format!("{} - Search", self.window_title)
        } else if search.current.is_some() {
            format!("{} - Search: {}", self.window_title, search.query)
        } else {
            format!(
                "{} - Search: {} (no match)",
                self.window_title, search.query
            )
        }
    }

    fn apply_window_title(&self) {
        if let Some(window) = &self.window {
            window.set_title(&self.effective_window_title());
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

        if window_search_shortcut(&key.logical_key, self.modifiers) {
            self.enter_search_mode();
            return Ok(());
        }

        if self.search.is_some() {
            self.handle_search_key(key);
            return Ok(());
        }

        if window_copy_shortcut(&key.logical_key, self.modifiers) {
            self.copy_selection_to_clipboard();
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

    fn enter_search_mode(&mut self) {
        self.search = Some(WindowSearch::default());
        self.apply_window_title();
    }

    fn exit_search_mode(&mut self) {
        self.search = None;
        self.apply_window_title();
    }

    fn handle_search_key(&mut self, key: &winit::event::KeyEvent) -> bool {
        match key.logical_key.as_ref() {
            Key::Named(NamedKey::Escape) => {
                self.exit_search_mode();
                true
            }
            Key::Named(NamedKey::F3) if self.modifiers.shift_key() => {
                self.step_search(SearchDirection::Previous)
            }
            Key::Named(NamedKey::Enter | NamedKey::F3) => self.step_search(SearchDirection::Next),
            Key::Named(NamedKey::Backspace) => {
                let Some(search) = self.search.as_ref() else {
                    return false;
                };
                let mut query = search.query.clone();
                if query.pop().is_none() {
                    return false;
                }
                self.update_search_query(&query);
                true
            }
            Key::Character(text) if !self.modifiers.control_key() && !self.modifiers.alt_key() => {
                let Some(search) = self.search.as_ref() else {
                    return false;
                };
                let mut query = search.query.clone();
                query.push_str(text);
                self.update_search_query(&query);
                true
            }
            _ => true,
        }
    }

    fn update_search_query(&mut self, query: &str) -> bool {
        let mut search = WindowSearch {
            query: query.to_owned(),
            current: None,
        };

        if query.is_empty() {
            self.search = Some(search);
            self.selection = None;
            self.refresh_snapshot();
            self.apply_window_title();
            return false;
        }

        let found =
            find_window_search_match(self.runtime.terminal(), query, None, SearchDirection::Next);
        search.current = found;
        self.search = Some(search);

        let Some(found) = found else {
            self.selection = None;
            self.refresh_snapshot();
            self.apply_window_title();
            return false;
        };

        self.apply_search_match(found);
        true
    }

    fn step_search(&mut self, direction: SearchDirection) -> bool {
        let Some(search) = self.search.as_ref() else {
            return false;
        };
        if search.query.is_empty() {
            return false;
        }

        let found = find_window_search_match(
            self.runtime.terminal(),
            &search.query,
            search.current,
            direction,
        );
        let Some(found) = found else {
            return false;
        };

        if let Some(search) = self.search.as_mut() {
            search.current = Some(found);
        }
        self.apply_search_match(found);
        true
    }

    fn apply_search_match(&mut self, search_match: WindowSearchMatch) {
        let scrollback_len = self.runtime.terminal().scrollback().len();
        let (offset, viewport_row) = search_match.viewport_position(scrollback_len);
        self.scrollback_offset = offset;
        self.selection = Some(WindowSelection::new(
            SelectionCell {
                row: viewport_row,
                column: search_match.start_column,
            },
            SelectionCell {
                row: viewport_row,
                column: search_match.end_column,
            },
        ));
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn copy_selection_to_clipboard(&mut self) -> bool {
        let Some(text) = self.selected_text() else {
            return false;
        };

        self.write_clipboard_text(&text)
    }

    fn write_clipboard_text(&mut self, text: &str) -> bool {
        (self.clipboard_writer)(text)
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection?;
        let text =
            selection.text_from_snapshot(&self.snapshot, self.runtime.terminal().grid().size());
        (!text.is_empty()).then_some(text)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelectionCell {
    row: u16,
    column: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowSelection {
    anchor: SelectionCell,
    focus: SelectionCell,
}

impl WindowSelection {
    const fn new(anchor: SelectionCell, focus: SelectionCell) -> Self {
        Self { anchor, focus }
    }

    fn set_focus(&mut self, focus: SelectionCell) {
        self.focus = focus;
    }

    const fn is_single_cell(self) -> bool {
        self.anchor.row == self.focus.row && self.anchor.column == self.focus.column
    }

    fn contains(self, row: u16, column: u16, size: TerminalSize) -> bool {
        if row >= size.rows || column >= size.columns {
            return false;
        }

        let (start, end) = self.normalized();
        if row < start.row || row > end.row {
            return false;
        }

        if start.row == end.row {
            return column >= start.column && column <= end.column;
        }

        if row == start.row {
            column >= start.column
        } else if row == end.row {
            column <= end.column
        } else {
            true
        }
    }

    fn text_from_snapshot(self, snapshot: &TerminalRenderSnapshot, size: TerminalSize) -> String {
        if size.columns == 0 || size.rows == 0 {
            return String::new();
        }

        let (start, end) = self.normalized();
        let mut lines = Vec::new();
        for row in start.row..=end.row.min(size.rows.saturating_sub(1)) {
            let first_column = if row == start.row { start.column } else { 0 };
            let last_column = if row == end.row {
                end.column.min(size.columns.saturating_sub(1))
            } else {
                size.columns.saturating_sub(1)
            };
            if first_column > last_column {
                lines.push(String::new());
                continue;
            }

            let mut line = String::new();
            for column in first_column..=last_column {
                line.push(snapshot_character(snapshot, row, column));
            }
            trim_trailing_spaces(&mut line);
            lines.push(line);
        }

        lines.join("\n")
    }

    const fn normalized(self) -> (SelectionCell, SelectionCell) {
        if self.anchor.row < self.focus.row
            || (self.anchor.row == self.focus.row && self.anchor.column <= self.focus.column)
        {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

fn trim_trailing_spaces(text: &mut String) {
    while text.ends_with(' ') {
        text.pop();
    }
}

fn snapshot_character(snapshot: &TerminalRenderSnapshot, row: u16, column: u16) -> char {
    snapshot
        .cells()
        .iter()
        .find(|cell| cell.row == row && cell.column == column)
        .map_or(' ', |cell| cell.ch)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WindowSearch {
    query: String,
    current: Option<WindowSearchMatch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowSearchMatch {
    source_row: usize,
    start_column: u16,
    end_column: u16,
}

impl WindowSearchMatch {
    fn viewport_position(self, scrollback_len: usize) -> (usize, u16) {
        if self.source_row < scrollback_len {
            (scrollback_len - self.source_row, 0)
        } else {
            let row = self.source_row.saturating_sub(scrollback_len);
            (0, u16::try_from(row).unwrap_or(u16::MAX))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchDirection {
    Next,
    Previous,
}

fn find_window_search_match(
    terminal: &rssh_terminal::Terminal,
    query: &str,
    current: Option<WindowSearchMatch>,
    direction: SearchDirection,
) -> Option<WindowSearchMatch> {
    if query.is_empty() {
        return None;
    }

    let matches = window_search_matches(terminal, query);
    if matches.is_empty() {
        return None;
    }

    let Some(current) = current else {
        return match direction {
            SearchDirection::Next => matches.first().copied(),
            SearchDirection::Previous => matches.last().copied(),
        };
    };

    match direction {
        SearchDirection::Next => matches
            .iter()
            .copied()
            .find(|candidate| search_match_after(*candidate, current))
            .or_else(|| matches.first().copied()),
        SearchDirection::Previous => matches
            .iter()
            .rev()
            .copied()
            .find(|candidate| search_match_after(current, *candidate))
            .or_else(|| matches.last().copied()),
    }
}

fn search_match_after(candidate: WindowSearchMatch, current: WindowSearchMatch) -> bool {
    candidate.source_row > current.source_row
        || (candidate.source_row == current.source_row
            && candidate.start_column > current.start_column)
}

fn window_search_matches(
    terminal: &rssh_terminal::Terminal,
    query: &str,
) -> Vec<WindowSearchMatch> {
    terminal_search_lines(terminal)
        .into_iter()
        .enumerate()
        .flat_map(|(source_row, line)| search_line_matches(source_row, &line, query))
        .collect()
}

fn terminal_search_lines(terminal: &rssh_terminal::Terminal) -> Vec<String> {
    let size = terminal.grid().size();
    let mut lines = Vec::new();

    for line in terminal.scrollback() {
        lines.push(
            line.cells()
                .iter()
                .take(usize::from(size.columns))
                .map(|cell| cell.ch)
                .collect(),
        );
    }

    for row in 0..size.rows {
        let mut line = String::new();
        for column in 0..size.columns {
            line.push(terminal.grid().get(row, column).map_or(' ', |cell| cell.ch));
        }
        lines.push(line);
    }

    lines
}

fn search_line_matches(source_row: usize, line: &str, query: &str) -> Vec<WindowSearchMatch> {
    line.match_indices(query)
        .filter_map(|(byte_index, _)| {
            let start = u16::try_from(line[..byte_index].chars().count()).ok()?;
            let width = u16::try_from(query.chars().count()).ok()?;
            let end = start.checked_add(width.saturating_sub(1))?;
            Some(WindowSearchMatch {
                source_row,
                start_column: start,
                end_column: end,
            })
        })
        .collect()
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

fn window_copy_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    let ctrl_shift_c = modifiers.control_key()
        && modifiers.shift_key()
        && !modifiers.alt_key()
        && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("c"));
    let ctrl_insert =
        modifiers == ModifiersState::CONTROL && matches!(key, Key::Named(NamedKey::Insert));

    ctrl_shift_c || ctrl_insert
}

fn window_search_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers.control_key()
        && !modifiers.shift_key()
        && !modifiers.alt_key()
        && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("f"))
}

fn read_window_clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn write_window_clipboard_text(text: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
        .is_ok()
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
    use std::sync::{Arc, Mutex};

    use winit::dpi::PhysicalPosition;
    use winit::event::{ElementState, MouseButton, MouseScrollDelta};
    use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};

    use crate::terminal_runtime::{MouseInputMode, MouseProtocolMode, MouseReportingMode};

    use super::{
        NativeWindowApp, SearchDirection, SelectionCell, WindowMouseEvent, WindowMouseEventKind,
        WindowSelection, demo_snapshot, encode_window_focus_event, encode_window_key,
        encode_window_mouse_event, encode_window_paste, terminal_size_from_window_pixels,
        window_copy_shortcut, window_paste_shortcut, window_search_shortcut,
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
    fn recognizes_window_copy_shortcuts() {
        assert!(window_copy_shortcut(
            &Key::Character("c".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(window_copy_shortcut(
            &Key::Character("C".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(window_copy_shortcut(
            &Key::Named(NamedKey::Insert),
            ModifiersState::CONTROL
        ));
        assert!(!window_copy_shortcut(
            &Key::Character("c".into()),
            ModifiersState::CONTROL
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
    fn window_selection_extracts_text_across_rows() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"abcd\r\nwxyz").unwrap();

        let selection = WindowSelection::new(
            SelectionCell { row: 0, column: 1 },
            SelectionCell { row: 1, column: 2 },
        );

        assert_eq!(
            selection.text_from_snapshot(&app.snapshot, app.runtime.terminal().grid().size()),
            "bcd\nwxy"
        );

        let reversed = WindowSelection::new(
            SelectionCell { row: 1, column: 2 },
            SelectionCell { row: 0, column: 1 },
        );
        assert_eq!(
            reversed.text_from_snapshot(&app.snapshot, app.runtime.terminal().grid().size()),
            "bcd\nwxy"
        );
    }

    #[test]
    fn window_app_highlights_active_selection() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();
        app.selection = Some(WindowSelection::new(
            SelectionCell { row: 0, column: 1 },
            SelectionCell { row: 0, column: 2 },
        ));

        app.refresh_snapshot();

        assert!(!snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 0, 1).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 0, 2).unwrap().inverse);
        assert!(!snapshot_cell(&app.snapshot, 0, 3).unwrap().inverse);
    }

    #[test]
    fn window_app_updates_selection_from_left_mouse_drag() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();

        app.handle_cursor_moved(PhysicalPosition::new(0.0, 0.0))
            .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        app.handle_cursor_moved(PhysicalPosition::new(f64::from(super::CELL_WIDTH * 2), 0.0))
            .unwrap();

        assert_eq!(
            app.selection,
            Some(WindowSelection::new(
                SelectionCell { row: 0, column: 0 },
                SelectionCell { row: 0, column: 2 },
            ))
        );
        assert!(snapshot_cell(&app.snapshot, 0, 1).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 0, 2).unwrap().inverse);

        assert!(
            app.handle_mouse_input(ElementState::Released, MouseButton::Left)
                .unwrap()
        );
        assert!(!app.selecting);
    }

    #[test]
    fn window_search_finds_matches_in_scrollback() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(6, 2));
        app.handle_pty_output(b"alpha\r\nbeta\r\ngamma").unwrap();

        assert!(app.update_search_query("alpha"));

        assert_eq!(app.selected_text().as_deref(), Some("alpha"));
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(app.scrollback_offset > 0);
    }

    #[test]
    fn window_search_steps_between_matches() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"foo one\r\nmiddle\r\nfoo two")
            .unwrap();

        assert!(app.update_search_query("foo"));
        assert_eq!(app.selected_text().as_deref(), Some("foo"));
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('f'));

        assert!(app.step_search(SearchDirection::Next));

        assert_eq!(app.selected_text().as_deref(), Some("foo"));
        assert_eq!(snapshot_char(&app.snapshot, 2, 0), Some('f'));

        assert!(app.step_search(SearchDirection::Previous));

        assert_eq!(app.selected_text().as_deref(), Some("foo"));
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('f'));
    }

    #[test]
    fn recognizes_window_search_shortcuts() {
        assert!(window_search_shortcut(
            &Key::Character("f".into()),
            ModifiersState::CONTROL
        ));
        assert!(window_search_shortcut(
            &Key::Character("F".into()),
            ModifiersState::CONTROL
        ));
        assert!(!window_search_shortcut(
            &Key::Character("f".into()),
            ModifiersState::empty()
        ));
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
    fn window_app_writes_osc52_clipboard_text_from_pty_output() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&writes);
        let mut app = NativeWindowApp::new(None);
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.handle_pty_output(b"\x1b]52;c;Y29weQ==\x07").unwrap();

        assert_eq!(writes.lock().unwrap().as_slice(), ["copy"]);
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

    fn snapshot_cell(
        snapshot: &rssh_renderer::TerminalRenderSnapshot,
        row: u16,
        column: u16,
    ) -> Option<&rssh_renderer::RenderCell> {
        snapshot
            .cells()
            .iter()
            .find(|cell| cell.row == row && cell.column == column)
    }
}
