use std::{
    collections::HashMap,
    error::Error,
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use pixels::{Pixels, SurfaceTexture};
use rssh_core::{
    DamageRegion, TerminalSize,
    app_shell::{
        AppAction, AppShell, AppShellError, PaneDirection, PaneLaunch, PaneProgress,
        PaneRotationDirection, ResizeDirection, SplitDirection,
    },
};
use rssh_pty::{PtyCommand, PtySession, PtySize};
use rssh_renderer::{
    PixelRenderer, RenderCell, RenderGeometry, SCROLLBAR_WIDTH, ScrollbackScrollbar,
    TerminalRenderSnapshot,
};
use rssh_terminal::{Color, SemanticType, Terminal, UnderlineStyle};
use serde::Serialize;
use unicode_segmentation::UnicodeSegmentation;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::Window,
};

use crate::{
    cli::{Osc52Policy, WindowOptions},
    terminal_input::{TerminalKey, encode_terminal_key},
    terminal_modes::{
        KITTY_KEYBOARD_ALTERNATE_KEYS, KITTY_KEYBOARD_ASSOCIATED_TEXT, KITTY_KEYBOARD_DISAMBIGUATE,
        KITTY_KEYBOARD_REPORT_ALL, KITTY_KEYBOARD_REPORT_EVENTS, MouseInputMode, MouseProtocolMode,
        MouseReportingMode,
    },
    terminal_runtime::{TerminalNotification, TerminalProgress, TerminalRuntime},
};

const TERMINAL_COLUMNS: u16 = 80;
const TERMINAL_ROWS: u16 = 24;
const TAB_BAR_ROWS: u16 = 1;
const CELL_WIDTH: u32 = 8;
const CELL_HEIGHT: u32 = 16;
const DEFAULT_WINDOW_TITLE: &str = "R-SSH";
const FRAME_WIDTH: u32 = TERMINAL_COLUMNS as u32 * CELL_WIDTH;
const FRAME_HEIGHT: u32 = (TERMINAL_ROWS as u32 + TAB_BAR_ROWS as u32) * CELL_HEIGHT;
const DOUBLE_CLICK_MAX_INTERVAL: Duration = Duration::from_millis(500);

pub fn run(options: &WindowOptions) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<WindowUserEvent>::with_user_event().build()?;
    let event_proxy = event_loop.create_proxy();
    let session_log = match &options.log {
        Some(path) => Some(Box::new(File::create(path)?) as Box<dyn Write + Send>),
        None => None,
    };
    let app = NativeWindowApp::with_event_proxy(
        options.frame_limit,
        options.osc52_policy,
        options.command.clone(),
        session_log,
        event_proxy,
    );
    let mut app = NativeWindowManager::new(app);

    event_loop.run_app(&mut app)?;
    if options.metrics_json {
        println!("{}", app.metrics_json_report()?);
    } else if options.metrics {
        print!("{}", app.metrics_report());
    }

    Ok(())
}

#[cfg(test)]
pub fn demo_snapshot() -> TerminalRenderSnapshot {
    let mut terminal = Terminal::new(TerminalSize::new(TERMINAL_COLUMNS, TERMINAL_ROWS));
    terminal.feed(b"\x1b[1;32mR-SSH\x1b[0m native renderer\r\n");
    terminal.feed(b"winit window + renderer terminal grid");

    TerminalRenderSnapshot::from_terminal(&terminal)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeWindowUserVarChange {
    pane: rssh_core::PaneId,
    name: String,
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeWindowBell {
    pane: rssh_core::PaneId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeWindowFocusChange {
    pane: rssh_core::PaneId,
    focused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeWindowResize {
    pane: rssh_core::PaneId,
    pixel_width: u32,
    pixel_height: u32,
    terminal_size: TerminalSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeWindowOpenUri {
    pane: rssh_core::PaneId,
    uri: String,
}

#[allow(clippy::struct_excessive_bools)]
struct NativeWindowApp {
    app_window_id: rssh_core::WindowId,
    window_close_requested: bool,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    renderer: PixelRenderer,
    runtime: TerminalRuntime,
    snapshot: TerminalRenderSnapshot,
    window_title: String,
    frame_width: u32,
    frame_height: u32,
    frame_limit: Option<u64>,
    #[allow(dead_code)]
    startup_command: PtyCommand,
    rendered_frames: u64,
    event_proxy: Option<EventLoopProxy<WindowUserEvent>>,
    session: Option<PtySession>,
    writer: Option<Box<dyn Write + Send>>,
    session_log: Option<Box<dyn Write + Send>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    modifiers: ModifiersState,
    scrollback_offset: usize,
    mouse_pixel_position: Option<PhysicalPosition<f64>>,
    mouse_position: Option<(u16, u16)>,
    active_mouse_button: Option<MouseButton>,
    selection: Option<WindowSelection>,
    selecting: bool,
    scrollbar_dragging: bool,
    split_resize_dragging: Option<PaneSplitResizeDrag>,
    last_left_click: Option<WindowClick>,
    search: Option<WindowSearch>,
    copy_mode: Option<WindowCopyMode>,
    command_palette: Option<WindowCommandPalette>,
    quick_select: Option<WindowQuickSelect>,
    pane_select: Option<WindowPaneSelect>,
    osc52_policy: Osc52Policy,
    clipboard_writer: Box<dyn FnMut(&str) -> bool + Send>,
    clipboard_reader: Box<dyn FnMut() -> Option<String> + Send>,
    primary_selection_writer: Box<dyn FnMut(&str) -> bool + Send>,
    primary_selection_reader: Box<dyn FnMut() -> Option<String> + Send>,
    hyperlink_opener: Box<dyn FnMut(&str) -> bool + Send>,
    open_uri_handler: Box<dyn FnMut(&NativeWindowOpenUri) -> bool + Send>,
    notification_handler: Box<dyn FnMut(&TerminalNotification) -> bool + Send>,
    bell_handler: Box<dyn FnMut(&NativeWindowBell) -> bool + Send>,
    focus_change_handler: Box<dyn FnMut(&NativeWindowFocusChange) -> bool + Send>,
    resize_handler: Box<dyn FnMut(&NativeWindowResize) -> bool + Send>,
    user_var_change_handler: Box<dyn FnMut(&NativeWindowUserVarChange) -> bool + Send>,
    metrics: WindowMetrics,
    pending_frame_damage: Vec<DamageRegion>,
    frame_needs_full_repaint: bool,
    app_shell: AppShell,
    pane_runtimes: HashMap<rssh_core::PaneId, PaneRuntime>,
}

struct NativeWindowManager {
    startup_app: Option<NativeWindowApp>,
    windows: HashMap<winit::window::WindowId, NativeWindowApp>,
    pending_apps: Vec<NativeWindowApp>,
    last_metrics: Option<WindowMetricsSnapshot>,
}

impl NativeWindowManager {
    fn new(startup_app: NativeWindowApp) -> Self {
        Self {
            startup_app: Some(startup_app),
            windows: HashMap::new(),
            pending_apps: Vec::new(),
            last_metrics: None,
        }
    }

    fn metrics_app(&self) -> Option<&NativeWindowApp> {
        self.windows
            .values()
            .next()
            .or(self.startup_app.as_ref())
            .or_else(|| self.pending_apps.first())
    }

    fn metrics_report(&self) -> String {
        self.metrics_app()
            .map_or_else(String::new, NativeWindowApp::metrics_report)
    }

    fn metrics_json_report(&self) -> Result<String, serde_json::Error> {
        if let Some(app) = self.metrics_app() {
            return app.metrics_json_report();
        }
        self.last_metrics
            .unwrap_or_else(|| WindowMetrics::new().snapshot())
            .json_report()
    }

    fn materialize_startup_app(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn Error>> {
        let Some(app) = self.startup_app.take() else {
            return Ok(());
        };
        self.materialize_app(event_loop, app)
    }

    fn materialize_pending_apps(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn Error>> {
        let pending = std::mem::take(&mut self.pending_apps);
        for app in pending {
            self.materialize_app(event_loop, app)?;
        }
        Ok(())
    }

    fn materialize_app(
        &mut self,
        event_loop: &ActiveEventLoop,
        mut app: NativeWindowApp,
    ) -> Result<(), Box<dyn Error>> {
        app.create_window(event_loop)?;
        app.spawn_pty()?;
        let Some(window_id) = app.window_id() else {
            return Err(Box::new(io::Error::other("window was not created")));
        };
        if let Some(window) = &app.window {
            window.request_redraw();
        }
        self.windows.insert(window_id, app);
        Ok(())
    }

    fn collect_pending_window_apps_from_app(&mut self, app: &mut NativeWindowApp) {
        while let Some(detached_app) = app.take_next_pending_window_app() {
            self.pending_apps.push(detached_app);
        }
    }

    fn window_id_for_app_window(
        &self,
        app_window_id: rssh_core::WindowId,
    ) -> Option<winit::window::WindowId> {
        self.windows
            .iter()
            .find_map(|(window_id, app)| (app.app_window_id == app_window_id).then_some(*window_id))
    }

    fn close_window(&mut self, window_id: winit::window::WindowId) -> bool {
        if let Some(app) = self.windows.remove(&window_id) {
            self.last_metrics = Some(app.metrics_snapshot());
            drop(app);
        }
        self.windows.is_empty() && self.startup_app.is_none() && self.pending_apps.is_empty()
    }

    #[cfg(test)]
    fn new_for_test(startup_app: NativeWindowApp) -> Self {
        Self::new(startup_app)
    }

    #[cfg(test)]
    fn collect_pending_window_apps_from_primary_for_test(&mut self) {
        let Some(mut app) = self.startup_app.take() else {
            return;
        };
        self.collect_pending_window_apps_from_app(&mut app);
        self.startup_app = Some(app);
    }

    #[cfg(test)]
    fn pending_app_count_for_test(&self) -> usize {
        self.pending_apps.len()
    }

    #[cfg(test)]
    fn pending_app_for_test(&self, index: usize) -> Option<&NativeWindowApp> {
        self.pending_apps.get(index)
    }
}

#[derive(Debug)]
enum WindowUserEvent {
    Output {
        window_id: rssh_core::WindowId,
        pane_id: rssh_core::PaneId,
        bytes: Vec<u8>,
    },
    Exited {
        window_id: rssh_core::WindowId,
        pane_id: rssh_core::PaneId,
    },
    ReadError {
        window_id: rssh_core::WindowId,
        pane_id: rssh_core::PaneId,
        error: String,
    },
}

impl WindowUserEvent {
    const fn window_id(&self) -> rssh_core::WindowId {
        match self {
            Self::Output { window_id, .. }
            | Self::Exited { window_id, .. }
            | Self::ReadError { window_id, .. } => *window_id,
        }
    }
}

struct PaneRuntime {
    runtime: TerminalRuntime,
    session: Option<PtySession>,
    writer: Option<Box<dyn Write + Send>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    snapshot: TerminalRenderSnapshot,
    scrollback_offset: usize,
}

impl PaneRuntime {
    fn close(&mut self) {
        if let Some(session) = self.session.as_mut() {
            let _ = session.kill();
            let _ = session.wait();
        }

        if let Some(reader_thread) = self.reader_thread.take() {
            let _ = reader_thread.join();
        }

        self.writer = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneRenderRect {
    pane_id: rssh_core::PaneId,
    row: u16,
    column: u16,
    rows: u16,
    columns: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneSeparator {
    row: u16,
    column: u16,
    rows: u16,
    columns: u16,
    source_pane: rssh_core::PaneId,
    new_pane: rssh_core::PaneId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PaneRenderLayout {
    panes: Vec<PaneRenderRect>,
    separators: Vec<PaneSeparator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneMouseCell {
    pane_id: rssh_core::PaneId,
    row: u16,
    column: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneSplitResizeDrag {
    pane_id: rssh_core::PaneId,
    direction: SplitDirection,
    last_row: u16,
    last_column: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameRenderMode {
    Full,
    Damage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct WindowMetricsSnapshot {
    first_pty_byte_ms: Option<u128>,
    first_rendered_cell_ms: Option<u128>,
    pty_chunks: u64,
    pty_bytes: u64,
    pty_chunk_process_p95_us: u128,
    damage_regions: u64,
    damaged_cells: u64,
    snapshot_damage_updates: u64,
    snapshot_rebuilds: u64,
    render_frames: u64,
    full_render_frames: u64,
    dirty_render_frames: u64,
    render_frame_p95_us: u128,
    input_writes: u64,
    input_bytes: u64,
    input_write_p95_us: u128,
    bells: u64,
}

impl WindowMetricsSnapshot {
    fn report(self) -> String {
        format!(
            "\
R-SSH metrics
first_pty_byte_ms={}
first_rendered_cell_ms={}
pty_chunks={}
pty_bytes={}
pty_chunk_process_p95_us={}
damage_regions={}
damaged_cells={}
snapshot_damage_updates={}
snapshot_rebuilds={}
render_frames={}
full_render_frames={}
dirty_render_frames={}
render_frame_p95_us={}
input_writes={}
input_bytes={}
input_write_p95_us={}
bells={}
",
            metric_option(self.first_pty_byte_ms),
            metric_option(self.first_rendered_cell_ms),
            self.pty_chunks,
            self.pty_bytes,
            self.pty_chunk_process_p95_us,
            self.damage_regions,
            self.damaged_cells,
            self.snapshot_damage_updates,
            self.snapshot_rebuilds,
            self.render_frames,
            self.full_render_frames,
            self.dirty_render_frames,
            self.render_frame_p95_us,
            self.input_writes,
            self.input_bytes,
            self.input_write_p95_us,
            self.bells
        )
    }

    fn json_report(self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self)
    }
}

#[derive(Debug)]
struct WindowMetrics {
    spawn_started_at: Instant,
    first_pty_byte: Option<Duration>,
    first_rendered_cell: Option<Duration>,
    pty_chunks: u64,
    pty_bytes: u64,
    pty_chunk_process_times: Vec<Duration>,
    damage_regions: u64,
    damaged_cells: u64,
    snapshot_damage_updates: u64,
    snapshot_rebuilds: u64,
    render_frame_times: Vec<Duration>,
    full_render_frames: u64,
    dirty_render_frames: u64,
    input_writes: u64,
    input_bytes: u64,
    input_write_times: Vec<Duration>,
    bells: u64,
}

impl WindowMetrics {
    fn new() -> Self {
        Self {
            spawn_started_at: Instant::now(),
            first_pty_byte: None,
            first_rendered_cell: None,
            pty_chunks: 0,
            pty_bytes: 0,
            pty_chunk_process_times: Vec::new(),
            damage_regions: 0,
            damaged_cells: 0,
            snapshot_damage_updates: 0,
            snapshot_rebuilds: 0,
            render_frame_times: Vec::new(),
            full_render_frames: 0,
            dirty_render_frames: 0,
            input_writes: 0,
            input_bytes: 0,
            input_write_times: Vec::new(),
            bells: 0,
        }
    }

    fn start_spawn_timer(&mut self) {
        self.spawn_started_at = Instant::now();
        self.first_pty_byte = None;
        self.first_rendered_cell = None;
    }

    fn record_pty_chunk(&mut self, byte_count: usize) {
        if self.first_pty_byte.is_none() {
            self.first_pty_byte = Some(self.spawn_started_at.elapsed());
        }
        self.pty_chunks = self.pty_chunks.saturating_add(1);
        self.pty_bytes = self
            .pty_bytes
            .saturating_add(u64::try_from(byte_count).unwrap_or(u64::MAX));
    }

    fn record_first_rendered_cell(&mut self, snapshot_is_empty: bool) {
        if self.first_rendered_cell.is_none() && !snapshot_is_empty {
            self.first_rendered_cell = Some(self.spawn_started_at.elapsed());
        }
    }

    fn record_pty_chunk_process(&mut self, duration: Duration) {
        self.pty_chunk_process_times.push(duration);
    }

    fn record_damage(&mut self, damage: &[DamageRegion]) {
        self.damage_regions = self
            .damage_regions
            .saturating_add(u64::try_from(damage.len()).unwrap_or(u64::MAX));
        let cells = damage.iter().fold(0_u64, |total, region| {
            total.saturating_add(damage_region_cells(*region))
        });
        self.damaged_cells = self.damaged_cells.saturating_add(cells);
    }

    fn record_snapshot_damage_update(&mut self) {
        self.snapshot_damage_updates = self.snapshot_damage_updates.saturating_add(1);
    }

    fn record_snapshot_rebuild(&mut self) {
        self.snapshot_rebuilds = self.snapshot_rebuilds.saturating_add(1);
    }

    fn record_render_frame(&mut self, duration: Duration) {
        self.render_frame_times.push(duration);
    }

    fn record_frame_render_mode(&mut self, mode: FrameRenderMode) {
        match mode {
            FrameRenderMode::Full => {
                self.full_render_frames = self.full_render_frames.saturating_add(1);
            }
            FrameRenderMode::Damage => {
                self.dirty_render_frames = self.dirty_render_frames.saturating_add(1);
            }
        }
    }

    fn record_input_write(&mut self, byte_count: usize, duration: Duration) {
        self.input_writes = self.input_writes.saturating_add(1);
        self.input_bytes = self
            .input_bytes
            .saturating_add(u64::try_from(byte_count).unwrap_or(u64::MAX));
        self.input_write_times.push(duration);
    }

    fn record_bells(&mut self, count: u64) {
        self.bells = self.bells.saturating_add(count);
    }

    fn snapshot(&self) -> WindowMetricsSnapshot {
        WindowMetricsSnapshot {
            first_pty_byte_ms: self.first_pty_byte.map(|duration| duration.as_millis()),
            first_rendered_cell_ms: self
                .first_rendered_cell
                .map(|duration| duration.as_millis()),
            pty_chunks: self.pty_chunks,
            pty_bytes: self.pty_bytes,
            pty_chunk_process_p95_us: p95_us(&self.pty_chunk_process_times),
            damage_regions: self.damage_regions,
            damaged_cells: self.damaged_cells,
            snapshot_damage_updates: self.snapshot_damage_updates,
            snapshot_rebuilds: self.snapshot_rebuilds,
            render_frames: u64::try_from(self.render_frame_times.len()).unwrap_or(u64::MAX),
            full_render_frames: self.full_render_frames,
            dirty_render_frames: self.dirty_render_frames,
            render_frame_p95_us: p95_us(&self.render_frame_times),
            input_writes: self.input_writes,
            input_bytes: self.input_bytes,
            input_write_p95_us: p95_us(&self.input_write_times),
            bells: self.bells,
        }
    }
}

fn p95_us(samples: &[Duration]) -> u128 {
    if samples.is_empty() {
        return 0;
    }

    let mut values = samples
        .iter()
        .map(std::time::Duration::as_micros)
        .collect::<Vec<_>>();
    values.sort_unstable();
    let index = values
        .len()
        .saturating_mul(95)
        .saturating_add(99)
        .saturating_div(100)
        .saturating_sub(1);

    values[index]
}

fn damage_region_cells(region: DamageRegion) -> u64 {
    u64::from(region.width).saturating_mul(u64::from(region.height))
}

fn metric_option(value: Option<u128>) -> String {
    value.map_or_else(|| "NA".to_owned(), |value| value.to_string())
}

#[allow(clippy::too_many_arguments)]
fn render_framebuffer_with_state(
    renderer: &PixelRenderer,
    snapshot: &TerminalRenderSnapshot,
    scrollbar: Option<ScrollbackScrollbar>,
    pending_frame_damage: &mut Vec<DamageRegion>,
    frame_needs_full_repaint: &mut bool,
    frame: &mut [u8],
    geometry: RenderGeometry,
    damage_row_offset: u16,
) -> FrameRenderMode {
    if *frame_needs_full_repaint || pending_frame_damage.is_empty() {
        renderer.render(
            snapshot,
            frame,
            geometry.target_width,
            geometry.target_height,
            geometry.cell_width,
            geometry.cell_height,
        );
        if let Some(scrollbar) = scrollbar {
            renderer.render_scrollbar(scrollbar, frame, geometry);
            redraw_frame_ui_rows(renderer, snapshot, frame, geometry, damage_row_offset);
        }
        pending_frame_damage.clear();
        *frame_needs_full_repaint = false;
        return FrameRenderMode::Full;
    }

    let damage = offset_damage_regions(std::mem::take(pending_frame_damage), damage_row_offset);
    renderer.render_damage(snapshot, &damage, frame, geometry);
    if let Some(scrollbar) = scrollbar {
        renderer.render_scrollbar(scrollbar, frame, geometry);
        redraw_frame_ui_rows(renderer, snapshot, frame, geometry, damage_row_offset);
    }
    FrameRenderMode::Damage
}

fn redraw_frame_ui_rows(
    renderer: &PixelRenderer,
    snapshot: &TerminalRenderSnapshot,
    frame: &mut [u8],
    geometry: RenderGeometry,
    rows: u16,
) {
    if rows == 0 || geometry.cell_width == 0 {
        return;
    }

    let columns =
        u16::try_from((geometry.target_width / geometry.cell_width).min(u32::from(u16::MAX)))
            .unwrap_or(u16::MAX);
    renderer.render_damage(
        snapshot,
        &[DamageRegion::new(0, 0, columns, rows)],
        frame,
        geometry,
    );
}

fn offset_damage_regions(damage: Vec<DamageRegion>, row_offset: u16) -> Vec<DamageRegion> {
    if row_offset == 0 {
        return damage;
    }

    damage
        .into_iter()
        .map(|region| {
            DamageRegion::new(
                region.x,
                region.y.saturating_add(row_offset),
                region.width,
                region.height,
            )
        })
        .collect()
}

impl NativeWindowApp {
    #[cfg(test)]
    fn new(frame_limit: Option<u64>) -> Self {
        Self::new_with_osc52_policy(frame_limit, Osc52Policy::default())
    }

    #[cfg(test)]
    fn new_with_osc52_policy(frame_limit: Option<u64>, osc52_policy: Osc52Policy) -> Self {
        Self::new_with_command_and_osc52_policy(
            frame_limit,
            osc52_policy,
            PtyCommand::default_shell(),
        )
    }

    #[cfg(test)]
    fn new_with_command(frame_limit: Option<u64>, startup_command: PtyCommand) -> Self {
        Self::new_with_command_and_osc52_policy(
            frame_limit,
            Osc52Policy::default(),
            startup_command,
        )
    }

    fn new_with_command_and_osc52_policy(
        frame_limit: Option<u64>,
        osc52_policy: Osc52Policy,
        startup_command: PtyCommand,
    ) -> Self {
        let runtime = TerminalRuntime::new(TerminalSize::new(TERMINAL_COLUMNS, TERMINAL_ROWS));
        let snapshot = TerminalRenderSnapshot::from_terminal(runtime.terminal());
        let app_shell = app_shell_from_pty_command(&startup_command);

        Self {
            app_window_id: rssh_core::WindowId::new(1),
            window_close_requested: false,
            window: None,
            pixels: None,
            renderer: PixelRenderer::new(),
            runtime,
            snapshot,
            window_title: DEFAULT_WINDOW_TITLE.to_owned(),
            frame_width: FRAME_WIDTH,
            frame_height: FRAME_HEIGHT,
            frame_limit,
            startup_command,
            rendered_frames: 0,
            event_proxy: None,
            session: None,
            writer: None,
            session_log: None,
            reader_thread: None,
            modifiers: ModifiersState::empty(),
            scrollback_offset: 0,
            mouse_pixel_position: None,
            mouse_position: None,
            active_mouse_button: None,
            selection: None,
            selecting: false,
            scrollbar_dragging: false,
            split_resize_dragging: None,
            last_left_click: None,
            search: None,
            copy_mode: None,
            command_palette: None,
            quick_select: None,
            pane_select: None,
            osc52_policy,
            clipboard_writer: Box::new(write_window_clipboard_text),
            clipboard_reader: Box::new(read_window_clipboard_text),
            primary_selection_writer: Box::new(write_window_primary_selection_text),
            primary_selection_reader: Box::new(read_window_primary_selection_text),
            hyperlink_opener: Box::new(open_window_hyperlink),
            open_uri_handler: Box::new(dispatch_window_open_uri),
            notification_handler: Box::new(show_window_notification),
            bell_handler: Box::new(dispatch_window_bell),
            focus_change_handler: Box::new(dispatch_window_focus_change),
            resize_handler: Box::new(dispatch_window_resize),
            user_var_change_handler: Box::new(dispatch_window_user_var_change),
            metrics: WindowMetrics::new(),
            pending_frame_damage: Vec::new(),
            frame_needs_full_repaint: true,
            app_shell,
            pane_runtimes: HashMap::new(),
        }
    }

    #[cfg(test)]
    fn startup_command(&self) -> &PtyCommand {
        &self.startup_command
    }

    #[cfg(test)]
    fn active_workspace_id(&self) -> rssh_core::WorkspaceId {
        self.app_shell.active_workspace_id()
    }

    #[cfg(test)]
    fn active_tab_id(&self) -> rssh_core::TabId {
        self.app_shell.active_tab_id()
    }

    #[cfg(test)]
    fn active_pane_id(&self) -> rssh_core::PaneId {
        self.app_shell.active_pane_id()
    }

    #[cfg(test)]
    fn app_window_id_for_test(&self) -> rssh_core::WindowId {
        self.app_window_id
    }

    #[cfg(test)]
    fn window_close_requested_for_test(&self) -> bool {
        self.window_close_requested
    }

    fn dispatch_app_action(&mut self, action: AppAction) -> Result<(), AppShellError> {
        match action {
            AppAction::CloseTab {
                tab,
                switch_to_last_active,
            } => return self.dispatch_close_tab_action(tab, switch_to_last_active),
            AppAction::ClosePane { pane } => return self.dispatch_close_pane_action(pane),
            _ => {}
        }

        self.dispatch_shell_action(action)
    }

    fn dispatch_shell_action(&mut self, action: AppAction) -> Result<(), AppShellError> {
        let previous_active_pane = self.app_shell.active_pane_id();
        self.app_shell.apply_action(action)?;
        self.sync_pane_runtimes(previous_active_pane);
        self.apply_window_title();
        Ok(())
    }

    fn dispatch_close_tab_action(
        &mut self,
        tab: rssh_core::TabId,
        switch_to_last_active: bool,
    ) -> Result<(), AppShellError> {
        match self.dispatch_shell_action(AppAction::CloseTab {
            tab,
            switch_to_last_active,
        }) {
            Ok(()) => Ok(()),
            Err(AppShellError::CannotCloseLastTab) => {
                self.request_window_close();
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn dispatch_close_pane_action(&mut self, pane: rssh_core::PaneId) -> Result<(), AppShellError> {
        match self.dispatch_shell_action(AppAction::ClosePane { pane }) {
            Ok(()) => Ok(()),
            Err(AppShellError::CannotCloseLastPane | AppShellError::CannotCloseLastTab) => {
                self.request_window_close();
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn request_window_close(&mut self) {
        self.window_close_requested = true;
    }

    fn take_window_close_request(&mut self) -> bool {
        let requested = self.window_close_requested;
        self.window_close_requested = false;
        requested
    }

    #[allow(dead_code)]
    fn take_next_pending_window_app(&mut self) -> Option<Self> {
        let pending_window = self.app_shell.take_next_pending_window()?;
        let app_window_id = pending_window.id();
        let active_pane = pending_window.active_pane_id();
        let launch = pending_window
            .tab()
            .panes()
            .iter()
            .find(|pane| pane.id() == active_pane)
            .map(|pane| pane.launch().clone())?;
        let startup_command = pty_command_from_pane_launch(&launch);
        let runtime = self
            .pane_runtimes
            .remove(&active_pane)
            .unwrap_or_else(|| self.new_inactive_pane_runtime());
        let app_shell = AppShell::from_pending_window(pending_window);
        let mut detached_app = Self::new_with_command_and_osc52_policy(
            self.frame_limit,
            self.osc52_policy,
            startup_command,
        );
        detached_app.app_window_id = app_window_id;
        detached_app.event_proxy.clone_from(&self.event_proxy);
        detached_app.app_shell = app_shell;
        detached_app.install_active_runtime(runtime);
        detached_app.apply_window_title();
        Some(detached_app)
    }

    fn sync_pane_runtimes(&mut self, previous_active_pane: rssh_core::PaneId) {
        let valid_pane_ids = self.app_shell.pane_ids();
        let active_pane = self.app_shell.active_pane_id();
        let active_was_replaced = previous_active_pane != active_pane;

        if active_was_replaced {
            let previous_runtime = self.take_active_runtime();
            if valid_pane_ids.contains(&previous_active_pane) {
                self.pane_runtimes
                    .insert(previous_active_pane, previous_runtime);
            } else {
                let mut previous_runtime = previous_runtime;
                previous_runtime.close();
            }
        }

        if valid_pane_ids.contains(&active_pane) {
            if !self.pane_runtimes.contains_key(&active_pane) {
                self.spawn_active_pane_runtime_if_needed();
            }

            if let Some(runtime) = self.pane_runtimes.remove(&active_pane) {
                self.install_active_runtime(runtime);
            }
        } else if active_was_replaced {
            self.install_active_runtime(self.new_inactive_pane_runtime());
        }

        self.pane_runtimes.retain(|pane_id, runtime| {
            let keep = valid_pane_ids.contains(pane_id);
            if !keep {
                runtime.close();
            }
            keep
        });
    }

    fn take_active_runtime(&mut self) -> PaneRuntime {
        let size = self.runtime.terminal().grid().size();
        let scrollback_offset = self.scrollback_offset;
        let session = self.session.take();
        let writer = self.writer.take();
        let reader_thread = self.reader_thread.take();

        let old_runtime = std::mem::replace(&mut self.runtime, TerminalRuntime::new(size));
        let old_snapshot = TerminalRenderSnapshot::from_terminal_viewport(
            old_runtime.terminal(),
            scrollback_offset,
        );
        self.scrollback_offset = 0;

        PaneRuntime {
            runtime: old_runtime,
            session,
            writer,
            reader_thread,
            snapshot: old_snapshot,
            scrollback_offset,
        }
    }

    fn new_inactive_pane_runtime(&self) -> PaneRuntime {
        let size = self.runtime.terminal().grid().size();
        let runtime = TerminalRuntime::new(size);
        let snapshot = TerminalRenderSnapshot::from_terminal(runtime.terminal());
        PaneRuntime {
            runtime,
            session: None,
            writer: None,
            reader_thread: None,
            snapshot,
            scrollback_offset: 0,
        }
    }

    fn install_active_runtime(&mut self, mut runtime: PaneRuntime) {
        let mut runtime_runtime = TerminalRuntime::new(self.runtime.terminal().grid().size());
        let mut runtime_snapshot = TerminalRenderSnapshot::from_terminal(self.runtime.terminal());

        std::mem::swap(&mut runtime.runtime, &mut runtime_runtime);
        std::mem::swap(&mut runtime.snapshot, &mut runtime_snapshot);

        self.runtime = runtime_runtime;
        self.snapshot = runtime_snapshot;
        self.session = runtime.session.take();
        self.writer = runtime.writer.take();
        self.reader_thread = runtime.reader_thread.take();
        self.scrollback_offset = runtime.scrollback_offset;

        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.flush();
        }

        self.frame_needs_full_repaint = true;
        self.pending_frame_damage.clear();
    }

    fn spawn_active_pane_runtime_if_needed(&mut self) {
        if self.session.is_some() {
            return;
        }

        if self.event_proxy.is_none() {
            self.session = None;
            self.writer = None;
            self.reader_thread = None;
            return;
        }

        match self.spawn_pane_runtime_for_active_pane() {
            Ok(runtime) => self.install_active_runtime(runtime),
            Err(error) => {
                eprintln!("PTY spawn error while syncing pane runtime: {error}");
                self.session = None;
                self.writer = None;
                self.reader_thread = None;
            }
        }
    }

    fn command_palette_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
        modifiers.control_key()
            && modifiers.shift_key()
            && !modifiers.alt_key()
            && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("p"))
    }

    fn enter_command_palette_mode(&mut self) {
        self.search = None;
        self.copy_mode = None;
        self.quick_select = None;
        self.pane_select = None;
        self.command_palette = Some(WindowCommandPalette::default());
        self.apply_window_title();
    }

    fn exit_command_palette_mode(&mut self) {
        self.command_palette = None;
        self.apply_window_title();
    }

    fn command_palette_filtered_commands(&self) -> Vec<WindowCommand> {
        let Some(palette) = self.command_palette.as_ref() else {
            return Vec::new();
        };
        if palette.query.is_empty() {
            return WINDOW_COMMANDS.to_vec();
        }

        if rename_tab_title_from_query(&palette.query).is_some() {
            return vec![WindowCommand::RenameTab];
        }

        let query = palette.query.to_ascii_lowercase();
        let mut matches = WINDOW_COMMANDS
            .iter()
            .copied()
            .filter_map(|command| {
                command
                    .palette_match_score(&query)
                    .map(|score| (command, score))
            })
            .collect::<Vec<_>>();

        matches.sort_by_key(|(_, score)| *score);
        matches.into_iter().map(|(command, _)| command).collect()
    }

    fn command_palette_set_query(&mut self, query: String) {
        let Some(palette) = self.command_palette.as_mut() else {
            return;
        };
        palette.query = query;
        palette.selected = 0;
        self.apply_window_title();
    }

    fn command_palette_move_selection(&mut self, delta: isize) {
        let commands = self.command_palette_filtered_commands();
        if commands.is_empty() {
            return;
        }

        let Some(palette) = self.command_palette.as_mut() else {
            return;
        };

        let len = isize::try_from(commands.len()).unwrap_or(1);
        let current = isize::try_from(palette.selected).unwrap_or(0);
        palette.selected = usize::try_from((current + delta).rem_euclid(len)).unwrap_or(0);
        self.apply_window_title();
    }

    #[allow(clippy::too_many_lines)]
    fn command_palette_apply_command(
        &mut self,
        command: WindowCommand,
    ) -> Result<(), AppShellError> {
        let action = match command {
            WindowCommand::EnterCopyMode => {
                self.enter_copy_mode();
                return Ok(());
            }
            WindowCommand::EnterQuickSelect => {
                self.enter_quick_select_mode();
                return Ok(());
            }
            WindowCommand::EnterPaneSelect => {
                self.enter_pane_select_mode();
                return Ok(());
            }
            WindowCommand::EnterPaneSwap => {
                self.enter_pane_select_mode_with_mode(WindowPaneSelectMode::SwapWithActive);
                return Ok(());
            }
            WindowCommand::EnterPaneSwapKeepFocus => {
                self.enter_pane_select_mode_with_mode(
                    WindowPaneSelectMode::SwapWithActiveKeepFocus,
                );
                return Ok(());
            }
            WindowCommand::EnterPaneMoveToNewTab => {
                self.enter_pane_select_mode_with_mode(WindowPaneSelectMode::MoveToNewTab);
                return Ok(());
            }
            WindowCommand::EnterPaneMoveToNewWindow => {
                self.enter_pane_select_mode_with_mode(WindowPaneSelectMode::MoveToNewWindow);
                return Ok(());
            }
            WindowCommand::EnterSearch => {
                self.enter_search_mode();
                return Ok(());
            }
            WindowCommand::ClearScrollback => {
                self.clear_scrollback();
                return Ok(());
            }
            WindowCommand::ClearScrollbackAndViewport => {
                self.clear_scrollback_and_viewport();
                return Ok(());
            }
            WindowCommand::ClearSelection => {
                self.clear_selection();
                return Ok(());
            }
            WindowCommand::CopyToClipboard => {
                self.copy_selection_to_clipboard();
                return Ok(());
            }
            WindowCommand::CopyToPrimarySelection => {
                self.copy_selection_to_primary_selection();
                return Ok(());
            }
            WindowCommand::CopyToClipboardAndPrimarySelection => {
                self.copy_selection_to_clipboard_and_primary_selection();
                return Ok(());
            }
            WindowCommand::PasteFromClipboard => {
                if let Err(error) = self.handle_window_paste() {
                    eprintln!("paste from clipboard failed: {error}");
                }
                return Ok(());
            }
            WindowCommand::PasteFromPrimarySelection => {
                if let Err(error) = self.handle_window_primary_selection_paste() {
                    eprintln!("paste from primary selection failed: {error}");
                }
                return Ok(());
            }
            WindowCommand::ResetTerminal => {
                if let Err(error) = self.reset_terminal() {
                    eprintln!("reset terminal failed: {error}");
                }
                return Ok(());
            }
            WindowCommand::ScrollToTop => {
                self.set_scrollback_offset(self.runtime.terminal().scrollback().len());
                return Ok(());
            }
            WindowCommand::ScrollToBottom => {
                self.set_scrollback_offset(0);
                return Ok(());
            }
            WindowCommand::ScrollPageUp => {
                self.scroll_viewport_lines(self.viewport_page_rows());
                return Ok(());
            }
            WindowCommand::ScrollPageDown => {
                self.scroll_viewport_lines(-self.viewport_page_rows());
                return Ok(());
            }
            WindowCommand::ScrollLineUp => {
                self.scroll_viewport_lines(1);
                return Ok(());
            }
            WindowCommand::ScrollLineDown => {
                self.scroll_viewport_lines(-1);
                return Ok(());
            }
            WindowCommand::ScrollToPreviousPrompt => {
                self.scroll_to_prompt(-1);
                return Ok(());
            }
            WindowCommand::ScrollToNextPrompt => {
                self.scroll_to_prompt(1);
                return Ok(());
            }
            WindowCommand::NewTab => AppAction::NewTab { launch: None },
            WindowCommand::ActivateLastTab => AppAction::ActivateLastTab,
            WindowCommand::CloseTab => AppAction::CloseTab {
                tab: self.app_shell.active_tab_id(),
                switch_to_last_active: false,
            },
            WindowCommand::NextTabNoWrap => AppAction::ActivateTabRelativeNoWrap { offset: 1 },
            WindowCommand::PreviousTabNoWrap => AppAction::ActivateTabRelativeNoWrap { offset: -1 },
            WindowCommand::NextTab => AppAction::ActivateTabRelative { offset: 1 },
            WindowCommand::PreviousTab => AppAction::ActivateTabRelative { offset: -1 },
            WindowCommand::MoveTabTo1 => AppAction::MoveTab { index: 0 },
            WindowCommand::MoveTabTo2 => AppAction::MoveTab { index: 1 },
            WindowCommand::MoveTabTo3 => AppAction::MoveTab { index: 2 },
            WindowCommand::MoveTabTo4 => AppAction::MoveTab { index: 3 },
            WindowCommand::RotatePanesClockwise => AppAction::RotatePanes {
                direction: PaneRotationDirection::Clockwise,
            },
            WindowCommand::RotatePanesCounterClockwise => AppAction::RotatePanes {
                direction: PaneRotationDirection::CounterClockwise,
            },
            WindowCommand::SplitRight => AppAction::SplitPane {
                pane: self.app_shell.active_pane_id(),
                direction: SplitDirection::Right,
                launch: None,
            },
            WindowCommand::SplitDown => AppAction::SplitPane {
                pane: self.app_shell.active_pane_id(),
                direction: SplitDirection::Down,
                launch: None,
            },
            WindowCommand::ClosePane => AppAction::ClosePane {
                pane: self.app_shell.active_pane_id(),
            },
            WindowCommand::ActivatePaneLeft => AppAction::ActivatePaneDirection {
                direction: PaneDirection::Left,
            },
            WindowCommand::ActivatePaneRight => AppAction::ActivatePaneDirection {
                direction: PaneDirection::Right,
            },
            WindowCommand::ActivatePaneUp => AppAction::ActivatePaneDirection {
                direction: PaneDirection::Up,
            },
            WindowCommand::ActivatePaneDown => AppAction::ActivatePaneDirection {
                direction: PaneDirection::Down,
            },
            WindowCommand::ActivatePane1 => AppAction::ActivatePaneByIndex { index: 0 },
            WindowCommand::ActivatePane2 => AppAction::ActivatePaneByIndex { index: 1 },
            WindowCommand::ActivatePane3 => AppAction::ActivatePaneByIndex { index: 2 },
            WindowCommand::ActivatePane4 => AppAction::ActivatePaneByIndex { index: 3 },
            WindowCommand::NextPane => AppAction::FocusNextPane,
            WindowCommand::PreviousPane => AppAction::FocusPreviousPane,
            WindowCommand::ResizePaneLeft => AppAction::ResizePane {
                pane: self.app_shell.active_pane_id(),
                direction: ResizeDirection::Left,
                amount: 1,
            },
            WindowCommand::ResizePaneRight => AppAction::ResizePane {
                pane: self.app_shell.active_pane_id(),
                direction: ResizeDirection::Right,
                amount: 1,
            },
            WindowCommand::ResizePaneUp => AppAction::ResizePane {
                pane: self.app_shell.active_pane_id(),
                direction: ResizeDirection::Up,
                amount: 1,
            },
            WindowCommand::ResizePaneDown => AppAction::ResizePane {
                pane: self.app_shell.active_pane_id(),
                direction: ResizeDirection::Down,
                amount: 1,
            },
            WindowCommand::TogglePaneZoom => AppAction::TogglePaneZoom {
                pane: self.app_shell.active_pane_id(),
            },
            WindowCommand::ZoomPane => AppAction::SetPaneZoomState {
                pane: self.app_shell.active_pane_id(),
                zoomed: true,
            },
            WindowCommand::UnzoomPane => AppAction::SetPaneZoomState {
                pane: self.app_shell.active_pane_id(),
                zoomed: false,
            },
            WindowCommand::NewWorkspace => AppAction::NewWorkspace {
                name: format!("workspace-{}", self.app_shell.workspaces().len() + 1),
                launch: None,
            },
            WindowCommand::CloseWorkspace => AppAction::CloseWorkspace {
                workspace: self.app_shell.active_workspace_id(),
            },
            WindowCommand::RenameWorkspace => {
                let active_workspace = self.app_shell.active_workspace();
                AppAction::RenameWorkspace {
                    workspace: self.app_shell.active_workspace_id(),
                    name: format!("{} (renamed)", active_workspace.name()),
                }
            }
            WindowCommand::RenameTab => {
                let active_tab = self.app_shell.active_tab();
                let explicit_title = self
                    .command_palette
                    .as_ref()
                    .and_then(|palette| rename_tab_title_from_query(&palette.query));
                let title = explicit_title.unwrap_or_else(|| {
                    let title = self
                        .tab_title_for_tab(active_tab)
                        .unwrap_or_else(|| format!("tab {}", active_tab.id().get()));
                    format!("{title} (renamed)")
                });
                AppAction::SetTabTitle {
                    tab: active_tab.id(),
                    title,
                }
            }
            WindowCommand::NextWorkspace => AppAction::SwitchWorkspaceRelative { offset: 1 },
            WindowCommand::PreviousWorkspace => AppAction::SwitchWorkspaceRelative { offset: -1 },
        };

        self.dispatch_app_action(action)
    }

    fn command_palette_execute(&mut self, command: WindowCommand) -> bool {
        match self.command_palette_apply_command(command) {
            Ok(()) => {
                self.exit_command_palette_mode();
                true
            }
            Err(error) => {
                eprintln!("command palette action failed: {error:?}");
                false
            }
        }
    }

    fn command_palette_status(&self, palette: &WindowCommandPalette) -> String {
        let commands = self.command_palette_filtered_commands();
        if commands.is_empty() {
            if palette.query.is_empty() {
                return "Command Palette: no commands".to_owned();
            }
            return format!("Command Palette: \"{}\" (no match)", palette.query);
        }

        let selected = palette.selected.min(commands.len().saturating_sub(1));
        let command = commands[selected];

        if palette.query.is_empty() {
            format!(
                "Command Palette: [{} / {}] {}",
                selected + 1,
                commands.len(),
                command.label()
            )
        } else {
            format!(
                "Command Palette: \"{}\" [{} / {}] {}",
                palette.query,
                selected + 1,
                commands.len(),
                command.label()
            )
        }
    }

    fn quick_select_status(quick_select: &WindowQuickSelect) -> String {
        if quick_select.matches.is_empty() {
            return "Quick Select: no match".to_owned();
        }

        if !quick_select.input.is_empty() {
            return format!(
                "Quick Select: \"{}\" [{} / {}]",
                quick_select.input,
                quick_select.current + 1,
                quick_select.matches.len()
            );
        }

        format!(
            "Quick Select: [{} / {}]",
            quick_select.current + 1,
            quick_select.matches.len()
        )
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn pane_select_status(pane_select: &WindowPaneSelect) -> String {
        format!("Pane Select: [{} panes]", pane_select.labels.len())
    }

    fn enter_pane_select_mode(&mut self) {
        self.enter_pane_select_mode_with_mode(WindowPaneSelectMode::Activate);
    }

    fn enter_pane_select_mode_with_mode(&mut self, mode: WindowPaneSelectMode) {
        self.command_palette = None;
        self.search = None;
        self.copy_mode = None;
        self.quick_select = None;
        self.selection = None;
        self.pane_select = Some(WindowPaneSelect::from_panes(
            self.app_shell.active_tab().panes(),
            mode,
        ));
        self.frame_needs_full_repaint = true;
        self.apply_window_title();
    }

    fn exit_pane_select_mode(&mut self) {
        self.pane_select = None;
        self.frame_needs_full_repaint = true;
        self.apply_window_title();
    }

    fn handle_pane_select_key(&mut self, key: &Key, modifiers: ModifiersState) -> bool {
        if self.pane_select.is_none() {
            return false;
        }

        match key.as_ref() {
            Key::Named(NamedKey::Escape) => {
                self.exit_pane_select_mode();
                true
            }
            Key::Character(text)
                if modifiers.control_key()
                    && !modifiers.alt_key()
                    && text.eq_ignore_ascii_case("g") =>
            {
                self.exit_pane_select_mode();
                true
            }
            Key::Character(text) if !modifiers.control_key() && !modifiers.alt_key() => {
                let Some(pane_select) = self.pane_select.as_ref() else {
                    return false;
                };
                let mut input = pane_select.input.clone();
                input.push_str(&text.to_ascii_lowercase());

                if let Some(pane) = pane_select.pane_for_label(&input) {
                    let mode = pane_select.mode;
                    let action = match mode {
                        WindowPaneSelectMode::Activate => AppAction::ActivatePane { pane },
                        WindowPaneSelectMode::SwapWithActive => AppAction::SwapPanes {
                            active: self.app_shell.active_pane_id(),
                            selected: pane,
                            keep_focus: false,
                        },
                        WindowPaneSelectMode::SwapWithActiveKeepFocus => AppAction::SwapPanes {
                            active: self.app_shell.active_pane_id(),
                            selected: pane,
                            keep_focus: true,
                        },
                        WindowPaneSelectMode::MoveToNewTab => AppAction::MovePaneToNewTab { pane },
                        WindowPaneSelectMode::MoveToNewWindow => {
                            AppAction::MovePaneToNewWindow { pane }
                        }
                    };
                    if let Err(error) = self.dispatch_app_action(action) {
                        eprintln!("pane select action failed: {error:?}");
                    }
                    self.exit_pane_select_mode();
                    return true;
                }

                if let Some(pane_select) = self.pane_select.as_mut() {
                    if pane_select.has_label_prefix(&input) {
                        pane_select.input = input;
                    } else {
                        pane_select.input.clear();
                    }
                }
                self.apply_window_title();
                true
            }
            _ => true,
        }
    }

    fn quick_select_step(&mut self, direction: SearchDirection) -> bool {
        let Some(quick_select) = self.quick_select.as_mut() else {
            return false;
        };

        if quick_select.matches.is_empty() {
            return false;
        }

        let len = quick_select.matches.len();
        let current = quick_select.current;
        quick_select.current = match direction {
            SearchDirection::Next => (current + 1) % len,
            SearchDirection::Previous => {
                if current == 0 {
                    len - 1
                } else {
                    current - 1
                }
            }
        };

        let Some(active) = quick_select.current_match() else {
            return false;
        };

        self.apply_quick_select_match(active);
        true
    }

    fn quick_select_step_page(&mut self, direction: SearchDirection) -> bool {
        let history_len = self.runtime.terminal().scrollback().len();
        let viewport_rows = usize::from(self.runtime.terminal().grid().size().rows);
        let viewport_top = copy_mode_viewport_top(history_len, self.scrollback_offset);

        let Some(active) = self.quick_select.as_mut().and_then(|quick_select| {
            if quick_select.matches.is_empty() || viewport_rows == 0 {
                return None;
            }

            let current = quick_select.current;
            let last = quick_select.matches.len().saturating_sub(1);
            let target = match direction {
                SearchDirection::Next => {
                    let bottom = viewport_top.saturating_add(viewport_rows);
                    quick_select
                        .matches
                        .iter()
                        .position(|candidate| candidate.source_row >= bottom)
                        .unwrap_or_else(|| current.min(last))
                }
                SearchDirection::Previous => {
                    let top = isize::try_from(viewport_top).unwrap_or(isize::MAX);
                    let prior = top.saturating_sub(isize::try_from(viewport_rows).unwrap_or(0));
                    quick_select
                        .matches
                        .iter()
                        .position(|candidate| {
                            let row = isize::try_from(candidate.source_row).unwrap_or(isize::MAX);
                            row > prior && row < top
                        })
                        .unwrap_or_else(|| current.saturating_sub(1))
                }
            };
            quick_select.current = target;
            quick_select.current_match()
        }) else {
            return false;
        };

        self.apply_quick_select_match(active);
        true
    }

    fn apply_quick_select_match(&mut self, selection: WindowSearchMatch) {
        self.apply_search_match(selection);
        self.apply_window_title();
    }

    fn handle_quick_select_key(&mut self, key: &winit::event::KeyEvent) -> bool {
        self.handle_quick_select_logical_key(&key.logical_key, self.modifiers)
    }

    fn handle_quick_select_logical_key(&mut self, key: &Key, modifiers: ModifiersState) -> bool {
        if self.quick_select.is_none() {
            return false;
        }

        match key.as_ref() {
            Key::Named(NamedKey::Escape) => {
                self.exit_quick_select_mode();
                true
            }
            Key::Named(NamedKey::Tab) => {
                if modifiers.shift_key() {
                    self.quick_select_step(SearchDirection::Previous);
                } else {
                    self.quick_select_step(SearchDirection::Next);
                }
                true
            }
            Key::Named(NamedKey::ArrowDown | NamedKey::ArrowRight) => {
                self.quick_select_step(SearchDirection::Next);
                true
            }
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft | NamedKey::Enter) => {
                self.quick_select_step(SearchDirection::Previous);
                true
            }
            Key::Named(NamedKey::PageDown) if modifiers.is_empty() => {
                self.quick_select_step_page(SearchDirection::Next);
                true
            }
            Key::Named(NamedKey::PageUp) if modifiers.is_empty() => {
                self.quick_select_step_page(SearchDirection::Previous);
                true
            }
            Key::Named(NamedKey::Backspace) if modifiers.is_empty() => {
                if let Some(quick_select) = self.quick_select.as_mut() {
                    quick_select.input.pop();
                }
                self.apply_window_title();
                true
            }
            Key::Character(text)
                if modifiers == ModifiersState::CONTROL && text.eq_ignore_ascii_case("n") =>
            {
                self.quick_select_step(SearchDirection::Next);
                true
            }
            Key::Character(text)
                if modifiers == ModifiersState::CONTROL && text.eq_ignore_ascii_case("p") =>
            {
                self.quick_select_step(SearchDirection::Previous);
                true
            }
            Key::Character(text)
                if modifiers.control_key()
                    && !modifiers.alt_key()
                    && text.eq_ignore_ascii_case("u") =>
            {
                if let Some(quick_select) = self.quick_select.as_mut() {
                    quick_select.input.clear();
                }
                self.apply_window_title();
                true
            }
            Key::Character(text) if !modifiers.control_key() && !modifiers.alt_key() => {
                let Some((input, matched)) = self.quick_select.as_ref().map(|quick_select| {
                    let mut input = quick_select.input.clone();
                    input.push_str(text);
                    let matched = quick_select.match_for_label(&input);
                    (input, matched)
                }) else {
                    return false;
                };

                if let Some(matched) = matched {
                    self.apply_quick_select_match(matched);
                    self.accept_quick_select_match(input != input.to_ascii_lowercase());
                    self.exit_quick_select_mode();
                    return true;
                }

                if let Some(quick_select) = self.quick_select.as_mut() {
                    if quick_select.has_label_prefix(&input) {
                        quick_select.input = input;
                    } else {
                        quick_select.input.clear();
                    }
                }
                self.apply_window_title();
                true
            }
            _ => false,
        }
    }

    fn accept_quick_select_match(&mut self, paste: bool) {
        if paste {
            if let Err(error) = self.paste_selected_text_to_pane() {
                eprintln!("quick-select paste failed: {error}");
            }
        } else {
            self.copy_selection_to_clipboard_and_primary_selection();
        }
    }

    fn enter_quick_select_mode(&mut self) {
        self.command_palette = None;
        self.search = None;
        self.copy_mode = None;
        self.pane_select = None;

        let matches = find_window_quick_select_matches(self.runtime.terminal());
        let labels = quick_select_labels_for_matches(matches.len());
        let quick_select = WindowQuickSelect {
            current: 0,
            matches,
            labels,
            input: String::new(),
        };
        let current = quick_select.current_match();
        self.quick_select = Some(quick_select);

        if let Some(active) = current {
            self.apply_quick_select_match(active);
        } else {
            self.selection = None;
            self.refresh_snapshot();
            self.apply_window_title();
        }
    }

    fn exit_quick_select_mode(&mut self) {
        self.quick_select = None;
        self.selection = None;
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn handle_command_palette_key(&mut self, key: &winit::event::KeyEvent) -> bool {
        let Some(palette_query) = self
            .command_palette
            .as_ref()
            .map(|palette| palette.query.clone())
        else {
            return false;
        };
        let palette_selected = self
            .command_palette
            .as_ref()
            .map_or(0, |palette| palette.selected);

        match key.logical_key.as_ref() {
            Key::Named(NamedKey::Escape) => {
                self.exit_command_palette_mode();
                true
            }
            Key::Named(NamedKey::Enter) => {
                let commands = self.command_palette_filtered_commands();
                if let Some(command) =
                    commands.get(palette_selected.min(commands.len().saturating_sub(1)))
                {
                    self.command_palette_execute(*command)
                } else {
                    false
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.command_palette_move_selection(1);
                false
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.command_palette_move_selection(-1);
                false
            }
            Key::Named(NamedKey::Backspace) => {
                let mut query = palette_query;
                if query.pop().is_some() {
                    self.command_palette_set_query(query);
                } else {
                    self.command_palette_set_query(String::new());
                }
                false
            }
            Key::Character(text)
                if self.modifiers.control_key() && text == "u" && !self.modifiers.alt_key() =>
            {
                self.command_palette_set_query(String::new());
                false
            }
            Key::Character(text)
                if !self.modifiers.control_key()
                    && !self.modifiers.alt_key()
                    && !text.is_empty() =>
            {
                let mut query = palette_query;
                query.push_str(text);
                self.command_palette_set_query(query);
                false
            }
            _ => false,
        }
    }

    fn app_shell_state_id_suffix(&self) -> String {
        format!(
            " [workspace:{} tab:{} pane:{}]",
            self.app_shell.active_workspace_id().get(),
            self.app_shell.active_tab_id().get(),
            self.app_shell.active_pane_id().get()
        )
    }

    #[allow(clippy::too_many_lines)]
    fn app_shell_action_for_key(&self, key: &Key, modifiers: ModifiersState) -> Option<AppAction> {
        if !modifiers.control_key() {
            return None;
        }

        if modifiers.alt_key() {
            if modifiers.shift_key() {
                match key {
                    Key::Named(NamedKey::ArrowLeft) => {
                        return Some(AppAction::ResizePane {
                            pane: self.app_shell.active_pane_id(),
                            direction: ResizeDirection::Left,
                            amount: 1,
                        });
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        return Some(AppAction::ResizePane {
                            pane: self.app_shell.active_pane_id(),
                            direction: ResizeDirection::Right,
                            amount: 1,
                        });
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        return Some(AppAction::ResizePane {
                            pane: self.app_shell.active_pane_id(),
                            direction: ResizeDirection::Up,
                            amount: 1,
                        });
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        return Some(AppAction::ResizePane {
                            pane: self.app_shell.active_pane_id(),
                            direction: ResizeDirection::Down,
                            amount: 1,
                        });
                    }
                    Key::Named(NamedKey::PageUp) => {
                        return Some(AppAction::MoveTabRelative { offset: -1 });
                    }
                    Key::Named(NamedKey::PageDown) => {
                        return Some(AppAction::MoveTabRelative { offset: 1 });
                    }
                    Key::Character(character) if character == "\"" || character == "'" => {
                        return Some(AppAction::SplitPane {
                            pane: self.app_shell.active_pane_id(),
                            direction: SplitDirection::Right,
                            launch: None,
                        });
                    }
                    Key::Character(character) if character == "%" => {
                        return Some(AppAction::SplitPane {
                            pane: self.app_shell.active_pane_id(),
                            direction: SplitDirection::Down,
                            launch: None,
                        });
                    }
                    _ => return None,
                }
            }

            return None;
        }

        if !modifiers.shift_key() {
            match key {
                Key::Character(character) if character.eq_ignore_ascii_case("n") => {
                    return Some(AppAction::SwitchWorkspaceRelative { offset: 1 });
                }
                Key::Character(character) if character.eq_ignore_ascii_case("p") => {
                    return Some(AppAction::SwitchWorkspaceRelative { offset: -1 });
                }
                Key::Named(NamedKey::Tab | NamedKey::PageDown) => {
                    return Some(AppAction::ActivateTabRelative { offset: 1 });
                }
                Key::Named(NamedKey::PageUp) => {
                    return Some(AppAction::ActivateTabRelative { offset: -1 });
                }
                _ => return None,
            }
        }

        let map_index = |character: &str| -> Option<isize> {
            match character {
                "1" | "!" => Some(0),
                "2" | "@" => Some(1),
                "3" | "#" => Some(2),
                "4" | "$" => Some(3),
                "5" | "%" => Some(4),
                "6" | "^" => Some(5),
                "7" | "&" => Some(6),
                "8" | "*" => Some(7),
                "9" | "(" => Some(-1),
                _ => None,
            }
        };

        if let Key::Character(character) = key {
            if let Some(index) = map_index(character) {
                return Some(AppAction::ActivateTabIndex { index });
            }
        }

        match key {
            Key::Character(character) if character.eq_ignore_ascii_case("t") => {
                Some(AppAction::NewTab { launch: None })
            }
            Key::Character(character) if character.eq_ignore_ascii_case("z") => {
                Some(AppAction::TogglePaneZoom {
                    pane: self.app_shell.active_pane_id(),
                })
            }
            Key::Character(character) if character.eq_ignore_ascii_case("w") => {
                Some(AppAction::CloseTab {
                    tab: self.app_shell.active_tab_id(),
                    switch_to_last_active: false,
                })
            }
            Key::Character(character) if character == "]" || character == "}" => {
                Some(AppAction::ActivateTabRelative { offset: 1 })
            }
            Key::Character(character) if character == "[" || character == "{" => {
                Some(AppAction::ActivateTabRelative { offset: -1 })
            }
            Key::Named(NamedKey::Tab)
                if modifiers.control_key() && modifiers.shift_key() && !modifiers.alt_key() =>
            {
                Some(AppAction::ActivateTabRelative { offset: -1 })
            }
            Key::Character(character) if character.eq_ignore_ascii_case("d") => {
                Some(AppAction::SplitPane {
                    pane: self.app_shell.active_pane_id(),
                    direction: SplitDirection::Right,
                    launch: None,
                })
            }
            Key::Character(character) if character.eq_ignore_ascii_case("r") => {
                Some(AppAction::RenameWorkspace {
                    workspace: self.app_shell.active_workspace_id(),
                    name: format!("{} (renamed)", self.app_shell.active_workspace().name()),
                })
            }
            Key::Character(character) if character.eq_ignore_ascii_case("e") => {
                Some(AppAction::SplitPane {
                    pane: self.app_shell.active_pane_id(),
                    direction: SplitDirection::Down,
                    launch: None,
                })
            }
            Key::Named(NamedKey::Tab) => Some(AppAction::ActivateTabRelative { offset: 1 }),
            Key::Named(NamedKey::ArrowLeft) => Some(AppAction::ActivatePaneDirection {
                direction: PaneDirection::Left,
            }),
            Key::Named(NamedKey::ArrowRight) => Some(AppAction::ActivatePaneDirection {
                direction: PaneDirection::Right,
            }),
            Key::Named(NamedKey::ArrowUp) => Some(AppAction::ActivatePaneDirection {
                direction: PaneDirection::Up,
            }),
            Key::Named(NamedKey::ArrowDown) => Some(AppAction::ActivatePaneDirection {
                direction: PaneDirection::Down,
            }),
            Key::Named(NamedKey::PageUp) => Some(AppAction::MoveTabRelative { offset: -1 }),
            Key::Named(NamedKey::PageDown) => Some(AppAction::MoveTabRelative { offset: 1 }),
            Key::Character(character) if character.eq_ignore_ascii_case("k") => {
                Some(AppAction::CloseWorkspace {
                    workspace: self.app_shell.active_workspace_id(),
                })
            }
            _ => None,
        }
    }

    #[cfg(test)]
    fn new_with_session_log(
        frame_limit: Option<u64>,
        session_log: impl Write + Send + 'static,
    ) -> Self {
        let mut app = Self::new(frame_limit);
        app.session_log = Some(Box::new(session_log));
        app
    }

    fn with_event_proxy(
        frame_limit: Option<u64>,
        osc52_policy: Osc52Policy,
        startup_command: PtyCommand,
        session_log: Option<Box<dyn Write + Send>>,
        event_proxy: EventLoopProxy<WindowUserEvent>,
    ) -> Self {
        let mut app =
            Self::new_with_command_and_osc52_policy(frame_limit, osc52_policy, startup_command);
        app.session_log = session_log;
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

    fn window_id(&self) -> Option<winit::window::WindowId> {
        self.window.as_ref().map(|window| window.id())
    }

    fn draw_frame(&mut self, event_loop: &ActiveEventLoop) {
        let scrollbar = self.scrollback_scrollbar();
        let geometry = self.render_geometry();
        let snapshot = self.render_snapshot();
        let damage_row_offset = self.terminal_frame_row_offset();
        if self.has_visible_split_layout() {
            self.frame_needs_full_repaint = true;
        }
        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };

        let started = Instant::now();
        let mode = render_framebuffer_with_state(
            &self.renderer,
            &snapshot,
            scrollbar,
            &mut self.pending_frame_damage,
            &mut self.frame_needs_full_repaint,
            pixels.frame_mut(),
            geometry,
            damage_row_offset,
        );
        self.metrics.record_frame_render_mode(mode);

        if let Err(error) = pixels.render() {
            eprintln!("render error: {error}");
            event_loop.exit();
            return;
        }

        self.rendered_frames = self.rendered_frames.saturating_add(1);
        self.metrics.record_render_frame(started.elapsed());
        if self
            .frame_limit
            .is_some_and(|limit| self.rendered_frames >= limit)
        {
            event_loop.exit();
        }
    }

    #[cfg(test)]
    fn render_framebuffer(&mut self, frame: &mut [u8]) -> FrameRenderMode {
        let scrollbar = self.scrollback_scrollbar();
        let geometry = self.render_geometry();
        let snapshot = self.render_snapshot();
        let damage_row_offset = self.terminal_frame_row_offset();
        if self.has_visible_split_layout() {
            self.frame_needs_full_repaint = true;
        }
        let mode = render_framebuffer_with_state(
            &self.renderer,
            &snapshot,
            scrollbar,
            &mut self.pending_frame_damage,
            &mut self.frame_needs_full_repaint,
            frame,
            geometry,
            damage_row_offset,
        );
        self.metrics.record_frame_render_mode(mode);
        mode
    }

    fn handle_pane_pty_output(
        &mut self,
        pane_id: rssh_core::PaneId,
        bytes: &[u8],
    ) -> io::Result<()> {
        if pane_id == self.app_shell.active_pane_id() {
            return self.handle_active_pane_output(bytes);
        }

        let Some(mut runtime) = self.pane_runtimes.remove(&pane_id) else {
            return Ok(());
        };

        let result = self.handle_inactive_pane_output(pane_id, &mut runtime, bytes);
        self.pane_runtimes.insert(pane_id, runtime);
        result
    }

    fn handle_active_pane_output(&mut self, bytes: &[u8]) -> io::Result<()> {
        let started = Instant::now();
        self.metrics.record_pty_chunk(bytes.len());
        let runtime_output = self.runtime.feed_pty_output_with_display(bytes);
        self.write_session_log(&runtime_output.display)?;
        for response in runtime_output.responses {
            self.write_pty_bytes(&response)?;
        }
        for text in self.runtime.take_clipboard_texts() {
            if self.osc52_policy.allows_write() {
                self.write_clipboard_text(&text);
            }
        }
        for selection in self.runtime.take_clipboard_queries() {
            if self.osc52_policy.allows_query() {
                self.answer_clipboard_query(&selection)?;
            }
        }
        for notification in self.runtime.take_notifications() {
            self.dispatch_notification(&notification);
        }
        self.sync_active_pane_current_working_dir_from_runtime();
        self.sync_active_pane_user_vars_from_runtime();
        self.sync_active_pane_badge_format_from_runtime();
        self.sync_active_pane_progress_from_runtime();
        self.sync_window_title_from_runtime();
        self.metrics.record_damage(&runtime_output.damage);
        self.refresh_snapshot_after_terminal_damage(&runtime_output.damage);
        self.metrics.record_bells(runtime_output.bells);
        self.dispatch_bells(self.app_shell.active_pane_id(), runtime_output.bells);
        self.metrics
            .record_first_rendered_cell(self.snapshot.cells().is_empty());
        self.metrics.record_pty_chunk_process(started.elapsed());

        Ok(())
    }

    fn handle_inactive_pane_output(
        &mut self,
        pane_id: rssh_core::PaneId,
        runtime: &mut PaneRuntime,
        bytes: &[u8],
    ) -> io::Result<()> {
        let started = Instant::now();
        self.metrics.record_pty_chunk(bytes.len());
        let runtime_output = runtime.runtime.feed_pty_output_with_display(bytes);
        for response in runtime_output.responses {
            if let Some(writer) = runtime.writer.as_mut() {
                let started = Instant::now();
                writer.write_all(&response)?;
                writer.flush()?;
                self.metrics
                    .record_input_write(response.len(), started.elapsed());
            }
        }
        for text in runtime.runtime.take_clipboard_texts() {
            if self.osc52_policy.allows_write() {
                self.write_clipboard_text(&text);
            }
        }
        for selection in runtime.runtime.take_clipboard_queries() {
            if self.osc52_policy.allows_query() {
                let Some(text) = self.read_clipboard_text() else {
                    continue;
                };
                let response = encode_osc52_clipboard_response(&selection, &text);
                if let Some(writer) = runtime.writer.as_mut() {
                    let started = Instant::now();
                    writer.write_all(&response)?;
                    writer.flush()?;
                    self.metrics
                        .record_input_write(response.len(), started.elapsed());
                }
            }
        }
        for notification in runtime.runtime.take_notifications() {
            self.dispatch_notification(&notification);
        }
        self.sync_pane_current_working_dir_from_value(
            pane_id,
            runtime
                .runtime
                .terminal()
                .current_working_dir()
                .map(str::to_owned),
        );
        self.sync_pane_user_vars_from_pairs(
            pane_id,
            runtime
                .runtime
                .terminal()
                .user_vars()
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        );
        self.sync_pane_badge_format_from_value(
            pane_id,
            runtime.runtime.terminal().badge_format().map(str::to_owned),
        );
        self.sync_pane_progress_from_value(pane_id, runtime.runtime.progress());
        runtime.scrollback_offset = runtime
            .scrollback_offset
            .min(runtime.runtime.terminal().scrollback().len());
        runtime.snapshot = TerminalRenderSnapshot::from_terminal_viewport(
            runtime.runtime.terminal(),
            runtime.scrollback_offset,
        );
        self.metrics.record_bells(runtime_output.bells);
        self.dispatch_bells(pane_id, runtime_output.bells);
        self.metrics
            .record_first_rendered_cell(self.snapshot.cells().is_empty());
        self.metrics.record_pty_chunk_process(started.elapsed());
        Ok(())
    }

    #[cfg(test)]
    fn handle_pty_output(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.handle_active_pane_output(bytes)
    }

    fn write_session_log(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let Some(log) = self.session_log.as_mut() else {
            return Ok(());
        };

        log.write_all(bytes)?;
        log.flush()
    }

    fn refresh_snapshot(&mut self) {
        self.rebuild_snapshot();
        self.metrics.record_snapshot_rebuild();
        self.frame_needs_full_repaint = true;
        self.pending_frame_damage.clear();
    }

    fn rebuild_snapshot(&mut self) {
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

    fn refresh_snapshot_after_terminal_damage(&mut self, damage: &[DamageRegion]) {
        self.scrollback_offset = self
            .scrollback_offset
            .min(self.runtime.terminal().scrollback().len());
        if self.can_update_snapshot_from_damage() {
            self.snapshot
                .update_from_terminal_damage(self.runtime.terminal(), damage);
            self.pending_frame_damage
                .extend(damage.iter().copied().filter(|region| !region.is_empty()));
            self.metrics.record_snapshot_damage_update();
            return;
        }

        self.refresh_snapshot();
    }

    fn can_update_snapshot_from_damage(&self) -> bool {
        self.scrollback_offset == 0
            && self.selection.is_none()
            && self.search.is_none()
            && self.copy_mode.is_none()
            && self.pane_select.is_none()
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
        self.copy_mode = None;
        self.pane_select = None;
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn scroll_to_prompt(&mut self, amount: isize) {
        let prompt_rows = self.runtime.terminal().semantic_prompt_rows();
        if prompt_rows.is_empty() || amount == 0 {
            return;
        }

        let history_len = self.runtime.terminal().scrollback().len();
        let viewport_top = history_len.saturating_sub(self.scrollback_offset.min(history_len));
        let index = match prompt_rows.binary_search(&viewport_top) {
            Ok(index) | Err(index) => index,
        };
        let target_index = if amount.is_negative() {
            index.saturating_sub(amount.unsigned_abs())
        } else {
            index.saturating_add(usize::try_from(amount).unwrap_or(usize::MAX))
        };
        let Some(prompt_row) = prompt_rows.get(target_index).copied() else {
            return;
        };

        self.set_scrollback_offset(history_len.saturating_sub(prompt_row));
    }

    fn set_scrollback_offset(&mut self, offset: usize) {
        let next_offset = offset.min(self.runtime.terminal().scrollback().len());
        if next_offset == self.scrollback_offset {
            return;
        }

        self.scrollback_offset = next_offset;
        self.selection = None;
        self.search = None;
        self.copy_mode = None;
        self.pane_select = None;
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
        let mouse_cell = self.focus_pane_for_mouse_position();
        let mode = self.runtime.mouse_input_mode();
        if mode.reporting_enabled() {
            if let Some(PaneMouseCell { column, row, .. }) =
                mouse_cell.or_else(|| self.mouse_cell_for_active_pane())
            {
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

        if self.quick_select.is_some() {
            self.exit_quick_select_mode();
        }

        if self.pane_select.is_some() {
            self.exit_pane_select_mode();
        }

        if self.copy_mode.is_some() {
            self.exit_copy_mode();
        }

        if self.handle_tab_bar_mouse_input(state, button) {
            return Ok(true);
        }

        if self.handle_split_resize_mouse_input(state, button) {
            return Ok(true);
        }

        if self.handle_scrollbar_mouse_input(state, button) {
            return Ok(true);
        }

        let mouse_cell = if state == ElementState::Pressed {
            self.focus_pane_for_mouse_position()
        } else {
            self.mouse_cell_for_active_pane()
        };
        let mode = self.runtime.mouse_input_mode();
        if !mode.reporting_enabled() {
            if self.handle_hyperlink_mouse_input(state, button) {
                return Ok(true);
            }
            return Ok(self.handle_selection_mouse_input(state, button));
        }

        let Some(PaneMouseCell { column, row, .. }) = mouse_cell else {
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
        self.mouse_pixel_position = Some(position);
        let next_position = window_mouse_cell(position);
        let mouse_cell_changed = self.mouse_position != next_position;
        self.mouse_position = next_position;

        if self.scrollbar_dragging {
            return Ok(self.scroll_to_scrollbar_position(position));
        }

        if self.split_resize_dragging.is_some() {
            return Ok(self.resize_split_to_mouse_position());
        }

        if !mouse_cell_changed {
            return Ok(false);
        }

        let mode = self.runtime.mouse_input_mode();
        if !mode.reporting_enabled() {
            return Ok(self.update_selection_from_mouse_position());
        }

        let Some(PaneMouseCell { column, row, .. }) = self.mouse_cell_for_active_pane() else {
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

    fn handle_split_resize_mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
    ) -> bool {
        if button != MouseButton::Left {
            return false;
        }

        match state {
            ElementState::Pressed => {
                let Some(drag) = self.split_resize_drag_at_mouse_position() else {
                    return false;
                };

                self.selection = None;
                self.selecting = false;
                self.split_resize_dragging = Some(drag);
                true
            }
            ElementState::Released if self.split_resize_dragging.is_some() => {
                self.split_resize_dragging = None;
                true
            }
            ElementState::Released => false,
        }
    }

    fn resize_split_to_mouse_position(&mut self) -> bool {
        let Some(drag) = self.split_resize_dragging else {
            return false;
        };
        let Some((column, row)) = self.mouse_position else {
            return false;
        };

        let (delta, direction) = match drag.direction {
            SplitDirection::Right => {
                let delta = i32::from(column) - i32::from(drag.last_column);
                let direction = if delta > 0 {
                    ResizeDirection::Right
                } else {
                    ResizeDirection::Left
                };
                (delta, direction)
            }
            SplitDirection::Down => {
                let delta = i32::from(row) - i32::from(drag.last_row);
                let direction = if delta > 0 {
                    ResizeDirection::Down
                } else {
                    ResizeDirection::Up
                };
                (delta, direction)
            }
        };

        let amount = delta.unsigned_abs();
        if amount == 0 {
            return false;
        }
        let amount = u16::try_from(amount).unwrap_or(u16::MAX);
        if let Err(error) = self.dispatch_app_action(AppAction::ResizePane {
            pane: drag.pane_id,
            direction,
            amount,
        }) {
            eprintln!("split resize drag failed: {error:?}");
            return false;
        }

        if let Some(drag) = self.split_resize_dragging.as_mut() {
            drag.last_column = column;
            drag.last_row = row;
        }
        true
    }

    fn split_resize_drag_at_mouse_position(&self) -> Option<PaneSplitResizeDrag> {
        let (column, row) = self.mouse_position?;
        let render_row = row.checked_add(self.terminal_frame_row_offset())?;
        self.pane_render_layout()
            .separators
            .into_iter()
            .find_map(|separator| split_resize_drag(separator, render_row, column))
    }

    fn handle_scrollbar_mouse_input(&mut self, state: ElementState, button: MouseButton) -> bool {
        if button != MouseButton::Left {
            return false;
        }

        match state {
            ElementState::Pressed => {
                let Some(position) = self.mouse_pixel_position else {
                    return false;
                };
                if !self.scrollbar_hit_test(position) {
                    return false;
                }

                self.scrollbar_dragging = true;
                self.scroll_to_scrollbar_position(position)
            }
            ElementState::Released if self.scrollbar_dragging => {
                self.scrollbar_dragging = false;
                true
            }
            ElementState::Released => false,
        }
    }

    fn scrollbar_hit_test(&self, position: PhysicalPosition<f64>) -> bool {
        if self.scrollback_scrollbar().is_none()
            || self.frame_width < SCROLLBAR_WIDTH
            || self.frame_height == 0
            || !position.x.is_finite()
            || !position.y.is_finite()
            || position.x < 0.0
            || position.y < f64::from(tab_bar_pixel_height())
            || position.y >= f64::from(self.frame_height)
        {
            return false;
        }

        let track_left = f64::from(self.frame_width.saturating_sub(SCROLLBAR_WIDTH));
        position.x >= track_left && position.x < f64::from(self.frame_width)
    }

    fn scroll_to_scrollbar_position(&mut self, position: PhysicalPosition<f64>) -> bool {
        let Some(offset) = self.scrollbar_offset_from_pixel_y(position.y) else {
            return false;
        };

        self.selecting = false;
        self.last_left_click = None;

        let old_offset = self.scrollback_offset;
        let had_overlay = self.selection.is_some()
            || self.search.is_some()
            || self.quick_select.is_some()
            || self.pane_select.is_some()
            || self.copy_mode.is_some();
        self.scrollback_offset = offset.min(self.runtime.terminal().scrollback().len());
        self.selection = None;
        self.search = None;
        self.quick_select = None;
        self.pane_select = None;
        self.copy_mode = None;

        if self.scrollback_offset != old_offset || had_overlay {
            self.refresh_snapshot();
            self.apply_window_title();
        }

        true
    }

    fn scrollbar_offset_from_pixel_y(&self, y: f64) -> Option<usize> {
        if !y.is_finite() || self.frame_height == 0 {
            return None;
        }

        let y = y - f64::from(tab_bar_pixel_height());
        if y < 0.0 {
            return None;
        }
        let content_height = self.frame_height.saturating_sub(tab_bar_pixel_height());
        let y = y.clamp(0.0, f64::from(content_height.saturating_sub(1)));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let y = y.floor() as u32;
        let geometry =
            RenderGeometry::new(self.frame_width, content_height, CELL_WIDTH, CELL_HEIGHT);
        Some(
            self.scrollback_scrollbar()?
                .offset_from_pixel_y(y, geometry),
        )
    }

    fn render_geometry(&self) -> RenderGeometry {
        RenderGeometry::new(self.frame_width, self.frame_height, CELL_WIDTH, CELL_HEIGHT)
    }

    #[allow(clippy::unused_self)]
    fn terminal_frame_row_offset(&self) -> u16 {
        TAB_BAR_ROWS
    }

    fn render_snapshot(&self) -> TerminalRenderSnapshot {
        let layout = self.pane_render_layout();
        if layout.panes.len() <= 1 {
            return self
                .snapshot
                .clone()
                .with_row_offset(self.terminal_frame_row_offset())
                .with_overlay_cells(self.pane_select_cells(&layout))
                .with_overlay_cells(self.tab_bar_cells());
        }

        let active_pane = self.app_shell.active_pane_id();
        let mut pane_rects = layout.panes.clone();
        pane_rects.sort_by_key(|rect| rect.pane_id != active_pane);

        let mut snapshot: Option<TerminalRenderSnapshot> = None;
        for rect in pane_rects {
            let Some(pane_snapshot) = self.pane_snapshot(rect.pane_id) else {
                continue;
            };
            let pane_snapshot =
                pane_snapshot
                    .clone()
                    .with_viewport(rect.row, rect.column, rect.rows, rect.columns);
            snapshot = Some(match snapshot {
                Some(current) => current.with_overlay_snapshot(pane_snapshot),
                None => pane_snapshot,
            });
        }

        snapshot
            .unwrap_or_else(|| {
                self.snapshot
                    .clone()
                    .with_row_offset(self.terminal_frame_row_offset())
            })
            .with_overlay_cells(self.pane_separator_cells(&layout))
            .with_overlay_cells(self.pane_select_cells(&layout))
            .with_overlay_cells(self.tab_bar_cells())
    }

    fn has_visible_split_layout(&self) -> bool {
        self.app_shell.active_tab().panes().len() > 1
    }

    fn focus_pane_for_mouse_position(&mut self) -> Option<PaneMouseCell> {
        let mouse_cell = self.pane_cell_at_mouse_position()?;
        if mouse_cell.pane_id != self.app_shell.active_pane_id() {
            if let Err(error) = self.dispatch_app_action(AppAction::ActivatePane {
                pane: mouse_cell.pane_id,
            }) {
                eprintln!("app shell pane focus error: {error:?}");
                return None;
            }
        }

        Some(mouse_cell)
    }

    fn mouse_cell_for_active_pane(&self) -> Option<PaneMouseCell> {
        let mouse_cell = self.pane_cell_at_mouse_position()?;
        (mouse_cell.pane_id == self.app_shell.active_pane_id()).then_some(mouse_cell)
    }

    fn pane_cell_at_mouse_position(&self) -> Option<PaneMouseCell> {
        let (column, row) = self.mouse_position?;
        let render_row = row.checked_add(self.terminal_frame_row_offset())?;
        self.pane_render_layout()
            .panes
            .into_iter()
            .find_map(|rect| pane_mouse_cell(rect, render_row, column))
    }

    fn pane_snapshot(&self, pane_id: rssh_core::PaneId) -> Option<&TerminalRenderSnapshot> {
        if pane_id == self.app_shell.active_pane_id() {
            return Some(&self.snapshot);
        }

        self.pane_runtimes
            .get(&pane_id)
            .map(|runtime| &runtime.snapshot)
    }

    fn pane_render_layout(&self) -> PaneRenderLayout {
        let panes = self.app_shell.active_tab().panes();
        let Some(first_pane) = panes.first() else {
            return PaneRenderLayout::default();
        };

        let size = self.runtime.terminal().grid().size();
        if let Some(zoomed_pane_id) = self.app_shell.active_tab().zoomed_pane_id() {
            return PaneRenderLayout {
                panes: vec![PaneRenderRect {
                    pane_id: zoomed_pane_id,
                    row: self.terminal_frame_row_offset(),
                    column: 0,
                    rows: size.rows,
                    columns: size.columns,
                }],
                separators: Vec::new(),
            };
        }

        let first_rect = PaneRenderRect {
            pane_id: first_pane.id(),
            row: self.terminal_frame_row_offset(),
            column: 0,
            rows: size.rows,
            columns: size.columns,
        };
        let mut rects = HashMap::from([(first_pane.id(), first_rect)]);
        let mut separators = Vec::new();

        for pane in panes.iter().skip(1) {
            let Some(split) = pane.split() else {
                continue;
            };
            let Some(source_rect) = rects.get(&split.source_pane).copied() else {
                continue;
            };
            let Some((next_source, new_rect, separator)) = split_pane_render_rect(
                source_rect,
                pane.id(),
                split.direction,
                split.source_size_delta,
            ) else {
                continue;
            };
            rects.insert(split.source_pane, next_source);
            rects.insert(pane.id(), new_rect);
            separators.push(separator);
        }

        PaneRenderLayout {
            panes: panes
                .iter()
                .filter_map(|pane| rects.get(&pane.id()).copied())
                .collect(),
            separators,
        }
    }

    fn pane_separator_cells(&self, layout: &PaneRenderLayout) -> Vec<RenderCell> {
        let active_pane = self.app_shell.active_pane_id();
        let mut cells = Vec::new();
        for separator in &layout.separators {
            let active = separator.source_pane == active_pane || separator.new_pane == active_pane;
            let foreground = if active {
                Color::Rgb(80, 170, 255)
            } else {
                Color::Rgb(125, 125, 132)
            };
            let background = Color::Rgb(22, 22, 26);
            let ch = if separator.columns == 1 { '|' } else { '-' };
            for row in separator.row..separator.row.saturating_add(separator.rows) {
                for column in separator.column..separator.column.saturating_add(separator.columns) {
                    cells.push(ui_render_cell(
                        row, column, ch, foreground, background, active,
                    ));
                }
            }
        }
        cells
    }

    fn pane_select_cells(&self, layout: &PaneRenderLayout) -> Vec<RenderCell> {
        let Some(pane_select) = self.pane_select.as_ref() else {
            return Vec::new();
        };

        let mut cells = Vec::new();
        for label in &pane_select.labels {
            let Some(rect) = layout
                .panes
                .iter()
                .find(|rect| rect.pane_id == label.pane_id)
                .copied()
            else {
                continue;
            };
            let label_width = u16::try_from(label.label.chars().count()).unwrap_or(u16::MAX);
            if label_width == 0 || rect.rows == 0 || rect.columns == 0 {
                continue;
            }

            let row = rect.row.saturating_add(rect.rows / 2);
            let column = rect
                .column
                .saturating_add((rect.columns / 2).saturating_sub(label_width / 2));
            for (offset, ch) in label.label.chars().enumerate() {
                let offset = u16::try_from(offset).unwrap_or(u16::MAX);
                let column = column.saturating_add(offset);
                if column >= rect.column.saturating_add(rect.columns) {
                    break;
                }
                cells.push(ui_render_cell(
                    row,
                    column,
                    ch,
                    Color::Rgb(12, 12, 14),
                    Color::Rgb(255, 209, 102),
                    true,
                ));
            }
        }

        cells
    }

    fn tab_bar_cells(&self) -> Vec<RenderCell> {
        let columns = self.runtime.terminal().grid().size().columns;
        let mut cells = (0..columns)
            .map(|column| {
                tab_bar_render_cell(
                    column,
                    ' ',
                    Color::Rgb(198, 198, 198),
                    Color::Rgb(34, 34, 38),
                    false,
                )
            })
            .collect::<Vec<_>>();

        let mut column = 0u16;
        write_tab_bar_segment(
            &mut cells,
            &mut column,
            &self.tab_bar_workspace_label(),
            Color::Rgb(228, 228, 228),
            Color::Rgb(18, 18, 22),
            true,
        );

        let active_tab_id = self.app_shell.active_tab_id();
        for (index, tab) in self.app_shell.active_workspace().tabs().iter().enumerate() {
            let active = tab.id() == active_tab_id;
            write_tab_bar_segment(
                &mut cells,
                &mut column,
                &self.tab_bar_label_for_tab(index, tab),
                if active {
                    Color::Rgb(20, 20, 20)
                } else {
                    Color::Rgb(210, 210, 210)
                },
                if active {
                    Color::Rgb(238, 238, 238)
                } else {
                    Color::Rgb(58, 58, 64)
                },
                active,
            );
        }

        write_tab_bar_segment(
            &mut cells,
            &mut column,
            tab_bar_new_tab_label(),
            Color::Rgb(230, 230, 230),
            Color::Rgb(46, 56, 48),
            true,
        );

        cells
    }

    fn handle_tab_bar_mouse_input(&mut self, state: ElementState, button: MouseButton) -> bool {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return false;
        }

        let Some(position) = self.mouse_pixel_position else {
            return false;
        };
        if !position.x.is_finite()
            || !position.y.is_finite()
            || position.x < 0.0
            || position.y < 0.0
            || position.y >= f64::from(tab_bar_pixel_height())
        {
            return false;
        }

        let Some(column) = pixel_axis_to_cell(position.x, CELL_WIDTH) else {
            return false;
        };
        if self.new_tab_button_for_tab_bar_column(column) {
            if let Err(error) = self.dispatch_app_action(AppAction::NewTab { launch: None }) {
                eprintln!("tab bar new tab failed: {error:?}");
                return false;
            }
            return true;
        }

        if let Some(tab) = self.close_tab_for_tab_bar_column(column) {
            if let Err(error) = self.dispatch_app_action(AppAction::CloseTab {
                tab,
                switch_to_last_active: false,
            }) {
                eprintln!("tab bar close failed: {error:?}");
                return false;
            }
            return true;
        }

        let Some(tab) = self.tab_for_tab_bar_column(column) else {
            return false;
        };

        if let Err(error) = self.dispatch_app_action(AppAction::ActivateTab { tab }) {
            eprintln!("tab bar activation failed: {error:?}");
            return false;
        }

        true
    }

    fn tab_for_tab_bar_column(&self, column: u16) -> Option<rssh_core::TabId> {
        let mut cursor = u16::try_from(self.tab_bar_workspace_label().chars().count()).ok()?;
        for (index, tab) in self.app_shell.active_workspace().tabs().iter().enumerate() {
            let label = self.tab_bar_label_for_tab(index, tab);
            let width = u16::try_from(label.chars().count()).ok()?;
            let end = cursor.saturating_add(width);
            if column >= cursor && column < end {
                return Some(tab.id());
            }
            cursor = end;
        }

        None
    }

    fn new_tab_button_for_tab_bar_column(&self, column: u16) -> bool {
        let Some(start) = self.tab_bar_new_tab_column_start() else {
            return false;
        };
        let Ok(width) = u16::try_from(tab_bar_new_tab_label().chars().count()) else {
            return false;
        };
        column >= start && column < start.saturating_add(width)
    }

    fn tab_bar_new_tab_column_start(&self) -> Option<u16> {
        let mut cursor = u16::try_from(self.tab_bar_workspace_label().chars().count()).ok()?;
        for (index, tab) in self.app_shell.active_workspace().tabs().iter().enumerate() {
            let label = self.tab_bar_label_for_tab(index, tab);
            let width = u16::try_from(label.chars().count()).ok()?;
            cursor = cursor.saturating_add(width);
        }
        Some(cursor)
    }

    fn close_tab_for_tab_bar_column(&self, column: u16) -> Option<rssh_core::TabId> {
        let mut cursor = u16::try_from(self.tab_bar_workspace_label().chars().count()).ok()?;
        for (index, tab) in self.app_shell.active_workspace().tabs().iter().enumerate() {
            let label = self.tab_bar_label_for_tab(index, tab);
            let width = u16::try_from(label.chars().count()).ok()?;
            let end = cursor.saturating_add(width);
            if column >= cursor && column < end {
                let offset = usize::from(column.saturating_sub(cursor));
                if label.chars().nth(offset) == Some('x') {
                    return Some(tab.id());
                }
                return None;
            }
            cursor = end;
        }

        None
    }

    fn tab_bar_workspace_label(&self) -> String {
        format!(" ws:{} ", self.app_shell.active_workspace().name())
    }

    fn tab_bar_label_for_tab(&self, position: usize, tab: &rssh_core::app_shell::Tab) -> String {
        let title = self.tab_title_for_tab(tab);
        tab_bar_tab_label(
            position,
            tab.id(),
            tab.panes().len(),
            tab.id() == self.app_shell.active_tab_id(),
            title.as_deref(),
        )
    }

    fn tab_title_for_tab(&self, tab: &rssh_core::app_shell::Tab) -> Option<String> {
        if let Some(title) = tab.title().map(str::trim).filter(|title| !title.is_empty()) {
            return Some(title.to_owned());
        }

        let title = if tab.id() == self.app_shell.active_tab_id() {
            self.runtime.terminal().title()
        } else {
            self.pane_runtimes
                .get(&tab.active_pane_id())
                .and_then(|runtime| runtime.runtime.terminal().title())
        };

        title
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_owned)
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
                self.copy_mode = None;
                let click = self.next_left_click(cell, Instant::now());
                if click.count >= 3 {
                    self.selection = Some(self.line_selection_at_cell(cell));
                    self.selecting = false;
                    self.last_left_click = Some(click);
                    self.refresh_snapshot();
                    self.apply_window_title();
                    return true;
                }
                if click.count == 2 {
                    if let Some(selection) = self.double_click_word_selection(cell) {
                        self.selection = Some(selection);
                        self.selecting = false;
                        self.last_left_click = Some(click);
                        self.refresh_snapshot();
                        self.apply_window_title();
                        return true;
                    }
                }
                self.selection = Some(WindowSelection::new(cell, cell));
                self.selecting = true;
                self.last_left_click = Some(click);
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

    fn handle_hyperlink_mouse_input(&mut self, state: ElementState, button: MouseButton) -> bool {
        if state != ElementState::Pressed
            || button != MouseButton::Left
            || !window_hyperlink_activation_modifiers(self.modifiers)
        {
            return false;
        }

        let Some(url) = self.hyperlink_at_mouse_position() else {
            return false;
        };

        let event = NativeWindowOpenUri {
            pane: self.app_shell.active_pane_id(),
            uri: url.clone(),
        };
        if self.dispatch_open_uri(&event) {
            (self.hyperlink_opener)(&url);
        }
        true
    }

    fn hyperlink_at_mouse_position(&self) -> Option<String> {
        let PaneMouseCell { column, row, .. } = self.mouse_cell_for_active_pane()?;
        self.snapshot
            .cells()
            .iter()
            .find(|cell| cell.row == row && cell.column == column)?
            .hyperlink
            .clone()
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
        self.last_left_click = None;
        self.refresh_snapshot();
        true
    }

    fn selection_cell_from_mouse_position(&self) -> Option<SelectionCell> {
        let PaneMouseCell { column, row, .. } = self.mouse_cell_for_active_pane()?;
        let size = self.runtime.terminal().grid().size();
        if row >= size.rows || column >= size.columns {
            return None;
        }

        Some(SelectionCell { row, column })
    }

    fn next_left_click(&self, cell: SelectionCell, time: Instant) -> WindowClick {
        let count = self
            .last_left_click
            .and_then(|last_click| {
                let elapsed = time.checked_duration_since(last_click.time)?;
                (elapsed <= DOUBLE_CLICK_MAX_INTERVAL).then_some(last_click.count.saturating_add(1))
            })
            .unwrap_or(1);

        WindowClick { cell, time, count }
    }

    fn double_click_word_selection(&self, cell: SelectionCell) -> Option<WindowSelection> {
        let last_click = self.last_left_click?;
        let selection = self.word_selection_at_cell(cell)?;
        let size = self.runtime.terminal().grid().size();
        if selection.contains(last_click.cell.row, last_click.cell.column, size) {
            Some(selection)
        } else {
            None
        }
    }

    fn line_selection_at_cell(&self, cell: SelectionCell) -> WindowSelection {
        let size = self.runtime.terminal().grid().size();
        WindowSelection::new(
            SelectionCell {
                row: cell.row,
                column: 0,
            },
            SelectionCell {
                row: cell.row,
                column: size.columns.saturating_sub(1),
            },
        )
    }

    fn word_selection_at_cell(&self, cell: SelectionCell) -> Option<WindowSelection> {
        let size = self.runtime.terminal().grid().size();
        if cell.row >= size.rows || cell.column >= size.columns {
            return None;
        }
        if !is_word_selection_character(snapshot_character(&self.snapshot, cell.row, cell.column)) {
            return None;
        }

        let mut start_column = cell.column;
        while start_column > 0
            && is_word_selection_character(snapshot_character(
                &self.snapshot,
                cell.row,
                start_column - 1,
            ))
        {
            start_column -= 1;
        }

        let mut end_column = cell.column;
        while end_column + 1 < size.columns
            && is_word_selection_character(snapshot_character(
                &self.snapshot,
                cell.row,
                end_column + 1,
            ))
        {
            end_column += 1;
        }

        Some(WindowSelection::new(
            SelectionCell {
                row: cell.row,
                column: start_column,
            },
            SelectionCell {
                row: cell.row,
                column: end_column,
            },
        ))
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

    fn sync_active_pane_current_working_dir_from_runtime(&mut self) {
        let pane = self.app_shell.active_pane_id();
        let cwd = self
            .runtime
            .terminal()
            .current_working_dir()
            .map(str::to_owned);
        self.sync_pane_current_working_dir_from_value(pane, cwd);
    }

    fn sync_active_pane_user_vars_from_runtime(&mut self) {
        let pane = self.app_shell.active_pane_id();
        let user_vars = self
            .runtime
            .terminal()
            .user_vars()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        self.sync_pane_user_vars_from_pairs(pane, user_vars);
    }

    fn sync_active_pane_badge_format_from_runtime(&mut self) {
        let pane = self.app_shell.active_pane_id();
        let badge_format = self.runtime.terminal().badge_format().map(str::to_owned);
        self.sync_pane_badge_format_from_value(pane, badge_format);
    }

    fn sync_active_pane_progress_from_runtime(&mut self) {
        let pane = self.app_shell.active_pane_id();
        self.sync_pane_progress_from_value(pane, self.runtime.progress());
    }

    fn sync_pane_progress_from_value(
        &mut self,
        pane: rssh_core::PaneId,
        progress: TerminalProgress,
    ) {
        let progress = pane_progress_from_terminal_progress(progress);
        if self.pane_progress(pane) == Some(progress) {
            return;
        }

        if let Err(error) = self
            .app_shell
            .apply_action(AppAction::SetPaneProgress { pane, progress })
        {
            eprintln!("failed to sync pane progress: {error:?}");
        }
    }

    fn sync_pane_badge_format_from_value(
        &mut self,
        pane: rssh_core::PaneId,
        badge_format: Option<String>,
    ) {
        if self.pane_badge_format(pane) == badge_format.as_deref() {
            return;
        }

        if let Err(error) = self
            .app_shell
            .apply_action(AppAction::SetPaneBadgeFormat { pane, badge_format })
        {
            eprintln!("failed to sync pane badge format: {error:?}");
        }
    }

    fn sync_pane_user_vars_from_pairs(
        &mut self,
        pane: rssh_core::PaneId,
        user_vars: Vec<(String, String)>,
    ) {
        for (name, value) in user_vars {
            if self.pane_user_var(pane, &name) == Some(value.as_str()) {
                continue;
            }

            let change = NativeWindowUserVarChange {
                pane,
                name: name.clone(),
                value: value.clone(),
            };
            if let Err(error) =
                self.app_shell
                    .apply_action(AppAction::SetPaneUserVar { pane, name, value })
            {
                eprintln!("failed to sync pane user var: {error:?}");
                continue;
            }

            self.dispatch_user_var_change(&change);
        }
    }

    fn sync_pane_current_working_dir_from_value(
        &mut self,
        pane: rssh_core::PaneId,
        cwd: Option<String>,
    ) {
        if self.pane_launch_current_working_dir(pane) == cwd.as_deref() {
            return;
        }

        if let Err(error) = self
            .app_shell
            .apply_action(AppAction::SetPaneCurrentWorkingDir { pane, cwd })
        {
            eprintln!("failed to sync pane current working directory: {error:?}");
        }
    }

    fn pane_launch_current_working_dir(&self, pane: rssh_core::PaneId) -> Option<&str> {
        self.app_shell
            .workspaces()
            .iter()
            .flat_map(rssh_core::app_shell::Workspace::tabs)
            .flat_map(rssh_core::app_shell::Tab::panes)
            .find(|candidate| candidate.id() == pane)
            .and_then(|candidate| candidate.launch().cwd())
    }

    fn pane_user_var(&self, pane: rssh_core::PaneId, name: &str) -> Option<&str> {
        self.app_shell
            .workspaces()
            .iter()
            .flat_map(rssh_core::app_shell::Workspace::tabs)
            .flat_map(rssh_core::app_shell::Tab::panes)
            .find(|candidate| candidate.id() == pane)
            .and_then(|candidate| candidate.user_vars().get(name))
            .map(String::as_str)
    }

    fn pane_badge_format(&self, pane: rssh_core::PaneId) -> Option<&str> {
        self.app_shell
            .workspaces()
            .iter()
            .flat_map(rssh_core::app_shell::Workspace::tabs)
            .flat_map(rssh_core::app_shell::Tab::panes)
            .find(|candidate| candidate.id() == pane)
            .and_then(rssh_core::app_shell::Pane::badge_format)
    }

    fn pane_progress(&self, pane: rssh_core::PaneId) -> Option<PaneProgress> {
        self.app_shell
            .workspaces()
            .iter()
            .flat_map(rssh_core::app_shell::Workspace::tabs)
            .flat_map(rssh_core::app_shell::Tab::panes)
            .find(|candidate| candidate.id() == pane)
            .map(rssh_core::app_shell::Pane::progress)
    }

    fn effective_window_title(&self) -> String {
        let mut title = self.window_title.clone();
        title.push_str(&self.app_shell_state_id_suffix());

        if let Some(copy_mode) = &self.copy_mode {
            title.push_str(" - ");
            title.push_str(&Self::copy_mode_status(copy_mode));
        }

        if let Some(search) = &self.search {
            title.push_str(" - ");
            title.push_str(&search_status(search));
        }

        if let Some(quick_select) = &self.quick_select {
            title.push_str(" - ");
            title.push_str(&Self::quick_select_status(quick_select));
        }

        if let Some(pane_select) = &self.pane_select {
            title.push_str(" - ");
            title.push_str(&Self::pane_select_status(pane_select));
        }

        if let Some(command_palette) = &self.command_palette {
            title.push_str(" - ");
            title.push_str(&self.command_palette_status(command_palette));
        }

        title
    }

    fn scrollback_scrollbar(&self) -> Option<ScrollbackScrollbar> {
        if self.has_visible_split_layout() {
            return None;
        }

        let history_len = self.runtime.terminal().scrollback().len();
        let rows = self.runtime.terminal().grid().size().rows;
        ScrollbackScrollbar::new(history_len, rows, self.scrollback_offset)
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

        let runtime = self.spawn_pane_runtime_for_active_pane()?;
        self.install_active_runtime(runtime);
        Ok(())
    }

    fn spawn_pane_runtime_for_active_pane(&mut self) -> Result<PaneRuntime, Box<dyn Error>> {
        let Some(event_proxy) = self.event_proxy.clone() else {
            return Err(Box::new(io::Error::other(
                "window event proxy is not configured",
            )));
        };

        let command = pty_command_from_pane_launch(self.app_shell.active_pane().launch());

        let size = self.runtime.terminal().grid().size();
        let pty_size = PtySize::try_new(size.columns, size.rows)?;
        self.metrics.start_spawn_timer();
        let mut session = PtySession::spawn(&command, pty_size)?;
        let mut reader = session.take_reader()?;
        let writer = session.take_writer()?;
        let pane_id = self.app_shell.active_pane_id();
        let app_window_id = self.app_window_id;
        let runtime_size = self.runtime.terminal().grid().size();
        let runtime = TerminalRuntime::new(runtime_size);
        let snapshot = TerminalRenderSnapshot::from_terminal(runtime.terminal());

        let reader_thread = thread::spawn(move || {
            let mut buffer = [0; 8192];

            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = event_proxy.send_event(WindowUserEvent::Exited {
                            window_id: app_window_id,
                            pane_id,
                        });
                        break;
                    }
                    Ok(count) => {
                        if event_proxy
                            .send_event(WindowUserEvent::Output {
                                window_id: app_window_id,
                                pane_id,
                                bytes: buffer[..count].to_vec(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        let _ = event_proxy.send_event(WindowUserEvent::ReadError {
                            window_id: app_window_id,
                            pane_id,
                            error: error.to_string(),
                        });
                        break;
                    }
                }
            }
        });

        Ok(PaneRuntime {
            runtime,
            session: Some(session),
            writer: Some(Box::new(writer)),
            reader_thread: Some(reader_thread),
            snapshot,
            scrollback_offset: 0,
        })
    }

    fn write_pty_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };

        let started = Instant::now();
        writer.write_all(bytes)?;
        writer.flush()?;
        self.metrics
            .record_input_write(bytes.len(), started.elapsed());

        Ok(())
    }

    fn metrics_snapshot(&self) -> WindowMetricsSnapshot {
        self.metrics.snapshot()
    }

    fn metrics_report(&self) -> String {
        self.metrics_snapshot().report()
    }

    fn metrics_json_report(&self) -> Result<String, serde_json::Error> {
        self.metrics_snapshot().json_report()
    }

    #[allow(clippy::too_many_lines)]
    fn handle_keyboard_input(&mut self, key: &winit::event::KeyEvent) -> io::Result<()> {
        let key_event_kind = KittyKeyEventKind::from_winit_key(key);
        if key.state != ElementState::Pressed {
            if key_event_kind == KittyKeyEventKind::Release {
                let bytes = encode_window_key_with_kitty_event(
                    &key.logical_key,
                    key.physical_key,
                    key.text.as_deref(),
                    self.modifiers,
                    self.runtime.application_cursor_keys(),
                    self.runtime.application_keypad(),
                    self.runtime.kitty_keyboard_flags(),
                    self.runtime.modify_other_keys(),
                    key_event_kind,
                );
                if !bytes.is_empty() {
                    self.write_pty_bytes(&bytes)?;
                }
            }
            return Ok(());
        }

        if self.command_palette.is_some() {
            if self.handle_command_palette_key(key) {
                return Ok(());
            }
            return Ok(());
        }

        if self.quick_select.is_some() {
            if self.handle_quick_select_key(key) {
                return Ok(());
            }
            return Ok(());
        }

        if self.pane_select.is_some() {
            if self.handle_pane_select_key(&key.logical_key, self.modifiers) {
                return Ok(());
            }
            return Ok(());
        }

        if self.copy_mode.is_some() {
            if self.handle_copy_mode_key(&key.logical_key, self.modifiers) {
                return Ok(());
            }
            return Ok(());
        }

        if window_clear_scrollback_shortcut(&key.logical_key, self.modifiers) {
            self.clear_scrollback();
            return Ok(());
        }

        if let Some(action) = self.app_shell_action_for_key(&key.logical_key, self.modifiers) {
            if let Err(error) = self.dispatch_app_action(action) {
                eprintln!("app shell action error: {error:?}");
            }
            return Ok(());
        }

        if Self::command_palette_shortcut(&key.logical_key, self.modifiers) {
            self.enter_command_palette_mode();
            return Ok(());
        }

        if window_quick_select_shortcut(&key.logical_key, self.modifiers) {
            self.enter_quick_select_mode();
            return Ok(());
        }

        if window_copy_mode_shortcut(&key.logical_key, self.modifiers) {
            self.enter_copy_mode();
            return Ok(());
        }

        if window_search_shortcut(&key.logical_key, self.modifiers) {
            self.enter_search_mode();
            return Ok(());
        }

        if self.search.is_some() {
            self.handle_search_key(&key.logical_key, self.modifiers);
            return Ok(());
        }

        if let Some(destination) =
            window_copy_destination_for_shortcut(&key.logical_key, self.modifiers)
        {
            self.copy_selection_to(destination);
            return Ok(());
        }

        if let Some(source) = window_paste_source_for_shortcut(&key.logical_key, self.modifiers) {
            self.handle_window_paste_from(source)?;
            return Ok(());
        }

        if self.handle_scrollback_shortcut(&key.logical_key, self.modifiers) {
            return Ok(());
        }

        let bytes = encode_window_key_with_kitty_event(
            &key.logical_key,
            key.physical_key,
            key.text.as_deref(),
            self.modifiers,
            self.runtime.application_cursor_keys(),
            self.runtime.application_keypad(),
            self.runtime.kitty_keyboard_flags(),
            self.runtime.modify_other_keys(),
            key_event_kind,
        );
        if !bytes.is_empty() {
            self.write_pty_bytes(&bytes)?;
        }

        Ok(())
    }

    fn enter_search_mode(&mut self) {
        let initial_query = self
            .selected_text()
            .and_then(|text| text.lines().next().map(str::to_owned))
            .filter(|line| !line.is_empty());
        if self.command_palette.is_some() {
            self.command_palette = None;
        }
        self.quick_select = None;
        self.copy_mode = None;
        self.pane_select = None;
        self.search = Some(WindowSearch::default());
        if let Some(query) = initial_query {
            self.update_search_query(&query);
        } else {
            self.apply_window_title();
        }
    }

    fn clear_scrollback(&mut self) {
        self.scrollback_offset = 0;
        if let Err(error) = self.handle_active_pane_output(b"\x1b[3J") {
            eprintln!("clear scrollback command failed: {error}");
        }
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn clear_scrollback_and_viewport(&mut self) {
        self.scrollback_offset = 0;
        let damage = self.runtime.erase_scrollback_and_viewport();
        self.metrics.record_damage(&damage);
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn reset_terminal(&mut self) -> io::Result<()> {
        self.handle_active_pane_output(b"\x1bc")
    }

    fn copy_mode_status(copy_mode: &WindowCopyMode) -> String {
        match copy_mode.selection_mode {
            WindowCopySelectionMode::Cell => "Copy Mode: Cell".to_owned(),
            WindowCopySelectionMode::Block => "Copy Mode: Block".to_owned(),
            WindowCopySelectionMode::Line => "Copy Mode: Line".to_owned(),
            WindowCopySelectionMode::None => "Copy Mode".to_owned(),
        }
    }

    fn enter_copy_mode(&mut self) {
        self.command_palette = None;
        self.search = None;
        self.quick_select = None;
        self.pane_select = None;

        let size = self.runtime.terminal().grid().size();
        if size.columns == 0 || size.rows == 0 {
            self.copy_mode = None;
            self.selection = None;
            self.refresh_snapshot();
            self.apply_window_title();
            return;
        }

        let (row, column) = self.runtime.terminal().cursor();
        let history_len = self.runtime.terminal().scrollback().len();
        let source_row = history_len.saturating_add(usize::from(row));
        self.copy_mode = Some(WindowCopyMode {
            cursor: SelectionCell {
                row: row.min(size.rows.saturating_sub(1)),
                column: column.min(size.columns.saturating_sub(1)),
            },
            source_cursor: SelectionSourceCell {
                row: source_row,
                column: usize::from(column.min(size.columns.saturating_sub(1))),
            },
            pending_jump: None,
            last_jump: None,
            search_direction: None,
            selection_mode: WindowCopySelectionMode::None,
            anchor: None,
            source_anchor: None,
        });
        self.selection = None;
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn exit_copy_mode(&mut self) {
        self.copy_mode = None;
        self.search = None;
        self.selection = None;
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn scroll_to_bottom_and_exit_copy_mode(&mut self) {
        self.scrollback_offset = 0;
        self.exit_copy_mode();
    }

    #[allow(clippy::too_many_lines)]
    fn handle_copy_mode_key(&mut self, key: &Key, modifiers: ModifiersState) -> bool {
        let pending_jump = {
            let Some(copy_mode) = self.copy_mode.as_mut() else {
                return false;
            };
            copy_mode.pending_jump.take()
        };
        if let Some(pending_jump) = pending_jump {
            return self.complete_copy_mode_jump(key, modifiers, pending_jump);
        }

        let in_copy_mode_search = self
            .copy_mode
            .as_ref()
            .is_some_and(|copy_mode| copy_mode.search_direction.is_some());

        if in_copy_mode_search && Self::copy_mode_search_key_table_handles_key(key, modifiers) {
            return self.handle_copy_mode_search_key(key, modifiers);
        }

        if Self::command_palette_shortcut(key, modifiers) {
            self.enter_command_palette_mode();
            return true;
        }

        if self.handle_copy_mode_app_shell_fallback(key, modifiers) {
            return true;
        }

        if in_copy_mode_search {
            return self.handle_copy_mode_search_key(key, modifiers);
        }

        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return false;
        };

        match key.as_ref() {
            Key::Named(NamedKey::Escape) => {
                self.scroll_to_bottom_and_exit_copy_mode();
                true
            }
            Key::Character("\u{1b}") if modifiers.is_empty() => {
                self.scroll_to_bottom_and_exit_copy_mode();
                true
            }
            Key::Character(character) if character.eq_ignore_ascii_case("q") => {
                self.scroll_to_bottom_and_exit_copy_mode();
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("c") =>
            {
                self.scroll_to_bottom_and_exit_copy_mode();
                true
            }
            Key::Named(NamedKey::Enter) => {
                self.move_copy_mode_cursor_by_lines(1);
                self.move_copy_mode_to_line_start()
            }
            Key::Character("\r") if modifiers.is_empty() => {
                self.move_copy_mode_cursor_by_lines(1);
                self.move_copy_mode_to_line_start()
            }
            Key::Named(NamedKey::PageUp) => {
                let page = isize::try_from(self.runtime.terminal().grid().size().rows).unwrap_or(0);
                self.move_copy_mode_cursor_by_lines(-page)
            }
            Key::Named(NamedKey::PageDown) => {
                let page = isize::try_from(self.runtime.terminal().grid().size().rows).unwrap_or(0);
                self.move_copy_mode_cursor_by_lines(page)
            }
            Key::Character(character) if character.eq_ignore_ascii_case("y") => {
                self.copy_selection_to_clipboard_and_primary_selection();
                self.scroll_to_bottom_and_exit_copy_mode();
                true
            }
            Key::Character(character)
                if (modifiers.shift_key() && character.eq_ignore_ascii_case("v"))
                    || (modifiers.is_empty() && character == "V") =>
            {
                copy_mode.selection_mode = WindowCopySelectionMode::Line;
                copy_mode.anchor = None;
                copy_mode.source_anchor = None;
                self.apply_copy_mode_selection();
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("v") =>
            {
                copy_mode.selection_mode = WindowCopySelectionMode::Block;
                copy_mode.anchor = Some(copy_mode.cursor);
                copy_mode.source_anchor = Some(copy_mode.source_cursor);
                self.apply_copy_mode_selection();
                true
            }
            Key::Character(" ") if modifiers.is_empty() => {
                copy_mode.selection_mode = WindowCopySelectionMode::Cell;
                copy_mode.anchor = Some(copy_mode.cursor);
                copy_mode.source_anchor = Some(copy_mode.source_cursor);
                self.apply_copy_mode_selection();
                true
            }
            Key::Character(character) if character.eq_ignore_ascii_case("v") => {
                copy_mode.selection_mode = WindowCopySelectionMode::Cell;
                copy_mode.anchor = Some(copy_mode.cursor);
                copy_mode.source_anchor = Some(copy_mode.source_cursor);
                self.apply_copy_mode_selection();
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("g") =>
            {
                self.scroll_to_bottom_and_exit_copy_mode();
                true
            }
            Key::Character("/") if modifiers.is_empty() => {
                self.start_copy_mode_search(SearchDirection::Next)
            }
            Key::Character("?") if modifiers.is_empty() || modifiers == ModifiersState::SHIFT => {
                self.start_copy_mode_search(SearchDirection::Previous)
            }
            Key::Character("o") if modifiers.is_empty() => {
                self.move_copy_mode_to_selection_other_end()
            }
            Key::Character("O") if modifiers.is_empty() || modifiers == ModifiersState::SHIFT => {
                self.move_copy_mode_to_selection_other_end_horiz()
            }
            Key::Character(";") if modifiers.is_empty() => self.repeat_copy_mode_jump(false),
            Key::Character(",") if modifiers.is_empty() => self.repeat_copy_mode_jump(true),
            Key::Character("f") if modifiers.is_empty() => self.start_copy_mode_jump(true, false),
            Key::Character("t") if modifiers.is_empty() => self.start_copy_mode_jump(true, true),
            Key::Character("F") if modifiers.is_empty() || modifiers == ModifiersState::SHIFT => {
                self.start_copy_mode_jump(false, false)
            }
            Key::Character("T") if modifiers.is_empty() || modifiers == ModifiersState::SHIFT => {
                self.start_copy_mode_jump(false, true)
            }
            Key::Named(NamedKey::Tab) if modifiers == ModifiersState::SHIFT => {
                self.move_copy_mode_by_word(WindowCopyWordMovement::Backward)
            }
            Key::Named(NamedKey::Tab) if modifiers.is_empty() => {
                self.move_copy_mode_by_word(WindowCopyWordMovement::Forward)
            }
            Key::Named(NamedKey::ArrowLeft) if modifiers == ModifiersState::ALT => {
                self.move_copy_mode_by_word(WindowCopyWordMovement::Backward)
            }
            Key::Named(NamedKey::ArrowRight) if modifiers == ModifiersState::ALT => {
                self.move_copy_mode_by_word(WindowCopyWordMovement::Forward)
            }
            Key::Character(character)
                if !modifiers.control_key()
                    && character.eq_ignore_ascii_case("b")
                    && (modifiers.is_empty() || modifiers == ModifiersState::ALT) =>
            {
                self.move_copy_mode_by_word(WindowCopyWordMovement::Backward)
            }
            Key::Character(character)
                if !modifiers.control_key()
                    && character.eq_ignore_ascii_case("w")
                    && modifiers.is_empty() =>
            {
                self.move_copy_mode_by_word(WindowCopyWordMovement::Forward)
            }
            Key::Character(character)
                if !modifiers.control_key()
                    && character.eq_ignore_ascii_case("f")
                    && modifiers == ModifiersState::ALT =>
            {
                self.move_copy_mode_by_word(WindowCopyWordMovement::Forward)
            }
            Key::Character(character)
                if !modifiers.control_key()
                    && character.eq_ignore_ascii_case("e")
                    && modifiers.is_empty() =>
            {
                self.move_copy_mode_by_word(WindowCopyWordMovement::End)
            }
            Key::Character("m") if modifiers == ModifiersState::ALT => {
                self.move_copy_mode_to_line_content_start()
            }
            Key::Character(character) if modifiers.alt_key() => {
                if let Some(semantic_type) = copy_mode_semantic_zone_type_for_key(character) {
                    let delta = if modifiers.shift_key() { 1 } else { -1 };
                    self.move_copy_mode_by_semantic_zone(delta, Some(semantic_type))
                } else {
                    false
                }
            }
            Key::Character(character)
                if modifiers.shift_key() && character.eq_ignore_ascii_case("z") =>
            {
                self.move_copy_mode_by_semantic_zone(1, None)
            }
            Key::Character("z") => self.move_copy_mode_by_semantic_zone(-1, None),
            Key::Character("G") if modifiers.is_empty() || modifiers == ModifiersState::SHIFT => {
                self.move_copy_mode_to_scrollback_bottom()
            }
            Key::Character("H") if modifiers.is_empty() || modifiers == ModifiersState::SHIFT => {
                self.move_copy_mode_to_viewport_top()
            }
            Key::Character("M") if modifiers.is_empty() || modifiers == ModifiersState::SHIFT => {
                self.move_copy_mode_to_viewport_middle()
            }
            Key::Character("L") if modifiers.is_empty() || modifiers == ModifiersState::SHIFT => {
                self.move_copy_mode_to_viewport_bottom()
            }
            Key::Named(NamedKey::ArrowLeft) => self.move_copy_mode_cursor(0, -1),
            Key::Named(NamedKey::ArrowRight) => self.move_copy_mode_cursor(0, 1),
            Key::Named(NamedKey::ArrowUp) => self.move_copy_mode_cursor(-1, 0),
            Key::Named(NamedKey::ArrowDown) => self.move_copy_mode_cursor(1, 0),
            Key::Character(character) if character.eq_ignore_ascii_case("h") => {
                self.move_copy_mode_cursor(0, -1)
            }
            Key::Character(character) if character.eq_ignore_ascii_case("l") => {
                self.move_copy_mode_cursor(0, 1)
            }
            Key::Character(character) if character.eq_ignore_ascii_case("k") => {
                self.move_copy_mode_cursor(-1, 0)
            }
            Key::Character(character) if character.eq_ignore_ascii_case("j") => {
                self.move_copy_mode_cursor(1, 0)
            }
            Key::Named(NamedKey::Home) | Key::Character("0") => self.move_copy_mode_to_line_start(),
            Key::Character("^") => self.move_copy_mode_to_line_content_start(),
            Key::Named(NamedKey::End) | Key::Character("$") => {
                self.move_copy_mode_to_line_content_end()
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("b") =>
            {
                let page = isize::try_from(self.runtime.terminal().grid().size().rows).unwrap_or(0);
                self.move_copy_mode_cursor_by_lines(-page)
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("f") =>
            {
                let page = isize::try_from(self.runtime.terminal().grid().size().rows).unwrap_or(0);
                self.move_copy_mode_cursor_by_lines(page)
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("u") =>
            {
                let half_page =
                    isize::try_from(self.runtime.terminal().grid().size().rows / 2).unwrap_or(0);
                self.move_copy_mode_cursor_by_lines(-half_page)
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("d") =>
            {
                let half_page =
                    isize::try_from(self.runtime.terminal().grid().size().rows / 2).unwrap_or(0);
                self.move_copy_mode_cursor_by_lines(half_page)
            }
            Key::Character("g") => self.move_copy_mode_to_scrollback_top(),
            _ => false,
        }
    }

    fn handle_copy_mode_app_shell_fallback(
        &mut self,
        key: &Key,
        modifiers: ModifiersState,
    ) -> bool {
        let Some(action) = self.app_shell_action_for_key(key, modifiers) else {
            return false;
        };

        self.exit_copy_mode();
        if let Err(error) = self.dispatch_app_action(action) {
            eprintln!("copy-mode fallback app shell action error: {error:?}");
        }
        true
    }

    fn apply_copy_mode_selection(&mut self) {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return;
        };

        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 || size.columns == 0 {
            return;
        }

        let history_len = self.runtime.terminal().scrollback().len();
        let viewport_top = copy_mode_viewport_top(history_len, self.scrollback_offset);
        self.selection = copy_mode_source_selection(copy_mode, size)
            .and_then(|selection| selection.viewport_selection(viewport_top, size));
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn move_copy_mode_cursor(&mut self, row_delta: isize, col_delta: isize) -> bool {
        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 || size.columns == 0 {
            return false;
        }
        let history_len = self.runtime.terminal().scrollback().len();
        let total_rows = history_len.saturating_add(usize::from(size.rows));
        if total_rows == 0 {
            return false;
        }

        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };

        let max_row = total_rows.saturating_sub(1);
        let max_column = usize::from(size.columns.saturating_sub(1));
        let next_row = apply_isize_delta_to_usize(
            copy_mode.source_cursor.row.min(max_row),
            row_delta,
            max_row,
        );
        let next_column = apply_isize_delta_to_usize(
            copy_mode.source_cursor.column.min(max_column),
            col_delta,
            max_column,
        );

        if next_row == copy_mode.source_cursor.row && next_column == copy_mode.source_cursor.column
        {
            return false;
        }

        self.set_copy_mode_cursor_for_source_position(next_row, next_column)
    }

    fn set_copy_mode_cursor(&mut self, row: u16, column: u16) -> bool {
        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 || size.columns == 0 {
            return false;
        }
        let history_len = self.runtime.terminal().scrollback().len();
        let viewport_top = copy_mode_viewport_top(history_len, self.scrollback_offset);
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return false;
        };

        let target = SelectionCell {
            row: row.min(size.rows.saturating_sub(1)),
            column: column.min(size.columns.saturating_sub(1)),
        };

        if target == copy_mode.cursor {
            return false;
        }

        copy_mode.cursor = target;
        copy_mode.source_cursor = SelectionSourceCell {
            row: viewport_top.saturating_add(usize::from(target.row)),
            column: usize::from(target.column),
        };
        self.apply_copy_mode_selection();
        true
    }

    fn set_copy_mode_cursor_for_source_position(
        &mut self,
        source_row: usize,
        source_column: usize,
    ) -> bool {
        let source_anchor = self
            .copy_mode
            .as_ref()
            .and_then(|copy_mode| copy_mode.source_anchor);
        self.set_copy_mode_cursor_and_anchor_for_source_position(
            SelectionSourceCell {
                row: source_row,
                column: source_column,
            },
            source_anchor,
        )
    }

    fn set_copy_mode_cursor_and_anchor_for_source_position(
        &mut self,
        source_cursor: SelectionSourceCell,
        source_anchor: Option<SelectionSourceCell>,
    ) -> bool {
        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 || size.columns == 0 {
            return false;
        }

        let history_len = self.runtime.terminal().scrollback().len();
        let current_offset = self.scrollback_offset.min(history_len);
        let Some((target_offset, target)) = copy_mode_viewport_cell_for_source_position(
            source_cursor.row,
            source_cursor.column,
            current_offset,
            history_len,
            size,
        ) else {
            return false;
        };
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };

        if target_offset == current_offset
            && target == copy_mode.cursor
            && source_cursor == copy_mode.source_cursor
            && source_anchor == copy_mode.source_anchor
        {
            return false;
        }

        self.scrollback_offset = target_offset;
        if let Some(copy_mode) = self.copy_mode.as_mut() {
            copy_mode.cursor = target;
            copy_mode.source_cursor = source_cursor;
            copy_mode.source_anchor = source_anchor;
            copy_mode.anchor = source_anchor.and_then(|anchor| {
                copy_mode_cell_for_source_position(
                    anchor.row,
                    anchor.column,
                    copy_mode_viewport_top(history_len, target_offset),
                    size,
                )
            });
        }
        self.apply_copy_mode_selection();
        true
    }

    fn move_copy_mode_to_selection_other_end(&mut self) -> bool {
        let Some((source_cursor, source_anchor)) = self.copy_mode.as_ref().and_then(|copy_mode| {
            copy_mode
                .source_anchor
                .map(|anchor| (copy_mode.source_cursor, anchor))
        }) else {
            return false;
        };

        self.set_copy_mode_cursor_and_anchor_for_source_position(source_anchor, Some(source_cursor))
    }

    fn move_copy_mode_to_selection_other_end_horiz(&mut self) -> bool {
        let Some((source_cursor, source_anchor)) = self.copy_mode.as_ref().and_then(|copy_mode| {
            copy_mode
                .source_anchor
                .map(|anchor| (copy_mode.source_cursor, anchor))
        }) else {
            return false;
        };

        self.set_copy_mode_cursor_and_anchor_for_source_position(
            SelectionSourceCell {
                row: source_cursor.row,
                column: source_anchor.column,
            },
            Some(SelectionSourceCell {
                row: source_anchor.row,
                column: source_cursor.column,
            }),
        )
    }

    fn move_copy_mode_cursor_by_lines(&mut self, line_delta: isize) -> bool {
        self.move_copy_mode_cursor(line_delta, 0)
    }

    fn move_copy_mode_by_semantic_zone(
        &mut self,
        delta: isize,
        semantic_type: Option<SemanticType>,
    ) -> bool {
        if delta == 0 {
            return false;
        }

        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };
        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 || size.columns == 0 {
            return false;
        }

        let terminal = self.runtime.terminal();
        let cursor_y = copy_mode.source_cursor.row;
        let cursor_x = copy_mode.source_cursor.column;
        let zones = terminal.semantic_zones();
        let mut index = match zones.binary_search_by(|zone| match zone.start_y.cmp(&cursor_y) {
            std::cmp::Ordering::Equal => zone.start_x.cmp(&cursor_x),
            ordering => ordering,
        }) {
            Ok(index) | Err(index) => index,
        };

        let step = if delta > 0 { 1 } else { -1 };
        let mut remaining = delta;
        while remaining != 0 {
            index = if step > 0 {
                let Some(next) = index.checked_add(1) else {
                    return false;
                };
                next
            } else {
                let Some(previous) = index.checked_sub(1) else {
                    return false;
                };
                previous
            };

            let Some(zone) = zones.get(index).copied() else {
                return false;
            };
            if semantic_type.is_some_and(|semantic_type| zone.semantic_type != semantic_type) {
                continue;
            }

            remaining -= step;
            if remaining == 0 {
                return self.set_copy_mode_cursor_for_source_position(zone.start_y, zone.start_x);
            }
        }

        false
    }

    fn move_copy_mode_by_word(&mut self, movement: WindowCopyWordMovement) -> bool {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };
        let Some(target) =
            copy_mode_word_target(self.runtime.terminal(), copy_mode.source_cursor, movement)
        else {
            return false;
        };

        self.set_copy_mode_cursor_for_source_position(target.row, target.column)
    }

    fn start_copy_mode_jump(&mut self, forward: bool, prev_char: bool) -> bool {
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return false;
        };

        copy_mode.pending_jump = Some(WindowCopyPendingJump { forward, prev_char });
        true
    }

    fn complete_copy_mode_jump(
        &mut self,
        key: &Key,
        modifiers: ModifiersState,
        pending_jump: WindowCopyPendingJump,
    ) -> bool {
        let target = match key.as_ref() {
            Key::Character(character)
                if modifiers.is_empty() || modifiers == ModifiersState::SHIFT =>
            {
                character.chars().next()
            }
            _ => None,
        };

        let Some(target) = target else {
            return true;
        };

        let jump = WindowCopyJump {
            forward: pending_jump.forward,
            prev_char: pending_jump.prev_char,
            target,
        };
        if let Some(copy_mode) = self.copy_mode.as_mut() {
            copy_mode.last_jump = Some(jump);
        }
        self.perform_copy_mode_jump(jump, false)
    }

    fn repeat_copy_mode_jump(&mut self, reverse: bool) -> bool {
        let Some(mut jump) = self
            .copy_mode
            .as_ref()
            .and_then(|copy_mode| copy_mode.last_jump)
        else {
            return false;
        };

        if reverse {
            jump.forward = !jump.forward;
        }
        self.perform_copy_mode_jump(jump, true)
    }

    fn perform_copy_mode_jump(&mut self, jump: WindowCopyJump, repeat: bool) -> bool {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };
        let cursor = copy_mode.source_cursor;
        let Some(target) = copy_mode_jump_target(self.runtime.terminal(), cursor, jump, repeat)
        else {
            return false;
        };

        self.set_copy_mode_cursor_for_source_position(target.row, target.column)
    }

    fn start_copy_mode_search(&mut self, direction: SearchDirection) -> bool {
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return false;
        };

        copy_mode.search_direction = Some(direction);
        self.search = Some(WindowSearch::default());
        self.selection = None;
        self.refresh_snapshot();
        self.apply_window_title();
        true
    }

    fn copy_mode_search_key_table_handles_key(key: &Key, modifiers: ModifiersState) -> bool {
        match key.as_ref() {
            Key::Named(NamedKey::Escape) => true,
            Key::Character("\u{1b}" | "\r")
            | Key::Named(
                NamedKey::ArrowDown
                | NamedKey::ArrowUp
                | NamedKey::Enter
                | NamedKey::PageDown
                | NamedKey::PageUp
                | NamedKey::Backspace,
            ) if modifiers.is_empty() => true,
            Key::Character(character)
                if modifiers == ModifiersState::CONTROL && character.eq_ignore_ascii_case("n") =>
            {
                true
            }
            Key::Character(character)
                if modifiers == ModifiersState::CONTROL && character.eq_ignore_ascii_case("p") =>
            {
                true
            }
            Key::Character(character)
                if modifiers == ModifiersState::CONTROL && character.eq_ignore_ascii_case("r") =>
            {
                true
            }
            Key::Character(character)
                if modifiers == ModifiersState::CONTROL && character.eq_ignore_ascii_case("u") =>
            {
                true
            }
            Key::Character(_) if !modifiers.control_key() && !modifiers.alt_key() => true,
            _ => false,
        }
    }

    fn handle_copy_mode_search_key(&mut self, key: &Key, modifiers: ModifiersState) -> bool {
        match key.as_ref() {
            Key::Named(NamedKey::Escape) => {
                self.exit_copy_mode();
                true
            }
            Key::Character("\u{1b}") if modifiers.is_empty() => {
                self.exit_copy_mode();
                true
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.step_search(SearchDirection::Next);
                true
            }
            Key::Named(NamedKey::ArrowUp | NamedKey::Enter) => {
                self.step_search(SearchDirection::Previous);
                true
            }
            Key::Character("\r") if modifiers.is_empty() => {
                self.step_search(SearchDirection::Previous);
                true
            }
            Key::Named(NamedKey::PageDown) => {
                self.step_search_page(SearchDirection::Next);
                true
            }
            Key::Named(NamedKey::PageUp) => {
                self.step_search_page(SearchDirection::Previous);
                true
            }
            Key::Named(NamedKey::Backspace) => {
                let Some(search) = self.search.as_ref() else {
                    return true;
                };
                let mut query = search.query.clone();
                query.pop();
                let direction = self.copy_mode_search_direction();
                self.update_search_query_with_direction(&query, direction);
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("n") =>
            {
                self.step_search(SearchDirection::Next);
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("p") =>
            {
                self.step_search(SearchDirection::Previous);
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("r") =>
            {
                self.cycle_search_match_type();
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("u") =>
            {
                let direction = self.copy_mode_search_direction();
                self.update_search_query_with_direction("", direction);
                true
            }
            Key::Character(text) if !modifiers.control_key() && !modifiers.alt_key() => {
                let Some(search) = self.search.as_ref() else {
                    return true;
                };
                let mut query = search.query.clone();
                query.push_str(text);
                let direction = self.copy_mode_search_direction();
                self.update_search_query_with_direction(&query, direction);
                true
            }
            _ => true,
        }
    }

    fn copy_mode_search_direction(&self) -> SearchDirection {
        self.copy_mode
            .as_ref()
            .and_then(|copy_mode| copy_mode.search_direction)
            .unwrap_or(SearchDirection::Next)
    }

    fn move_copy_mode_to_viewport_top(&mut self) -> bool {
        self.set_copy_mode_cursor(0, 0)
    }

    fn move_copy_mode_to_viewport_middle(&mut self) -> bool {
        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 {
            return false;
        }

        let middle_row = size.rows / 2;
        self.set_copy_mode_cursor(middle_row, 0)
    }

    fn move_copy_mode_to_viewport_bottom(&mut self) -> bool {
        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 {
            return false;
        }

        self.set_copy_mode_cursor(size.rows.saturating_sub(1), 0)
    }

    fn move_copy_mode_to_scrollback_top(&mut self) -> bool {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };
        self.set_copy_mode_cursor_for_source_position(0, copy_mode.source_cursor.column)
    }

    fn move_copy_mode_to_scrollback_bottom(&mut self) -> bool {
        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 {
            return false;
        }
        let history_len = self.runtime.terminal().scrollback().len();
        let total_rows = history_len.saturating_add(usize::from(size.rows));
        if total_rows == 0 {
            return false;
        }

        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };
        self.set_copy_mode_cursor_for_source_position(
            total_rows.saturating_sub(1),
            copy_mode.source_cursor.column,
        )
    }

    fn move_copy_mode_to_line_start(&mut self) -> bool {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };
        self.set_copy_mode_cursor(copy_mode.cursor.row, 0)
    }

    fn move_copy_mode_to_line_content_start(&mut self) -> bool {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };
        let source_row = copy_mode.source_cursor.row;
        let Some((content_start, _)) =
            copy_mode_line_content_bounds(self.runtime.terminal(), source_row)
        else {
            return false;
        };
        self.set_copy_mode_cursor_for_source_position(source_row, content_start)
    }

    fn move_copy_mode_to_line_content_end(&mut self) -> bool {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return false;
        };
        let source_row = copy_mode.source_cursor.row;
        let Some((_, content_end)) =
            copy_mode_line_content_bounds(self.runtime.terminal(), source_row)
        else {
            return false;
        };
        self.set_copy_mode_cursor_for_source_position(source_row, content_end)
    }

    fn exit_search_mode(&mut self) {
        self.search = None;
        self.apply_window_title();
    }

    fn handle_search_key(&mut self, key: &Key, modifiers: ModifiersState) -> bool {
        match key.as_ref() {
            Key::Named(NamedKey::Escape) => {
                self.exit_search_mode();
                true
            }
            Key::Character("\u{1b}") if modifiers.is_empty() => {
                self.exit_search_mode();
                true
            }
            Key::Named(NamedKey::ArrowDown) if modifiers.is_empty() => {
                self.step_search(SearchDirection::Next);
                true
            }
            Key::Named(NamedKey::ArrowUp) if modifiers.is_empty() => {
                self.step_search(SearchDirection::Previous);
                true
            }
            Key::Named(NamedKey::PageDown) if modifiers.is_empty() => {
                self.step_search_page(SearchDirection::Next);
                true
            }
            Key::Named(NamedKey::PageUp) if modifiers.is_empty() => {
                self.step_search_page(SearchDirection::Previous);
                true
            }
            Key::Named(NamedKey::F3) if modifiers.shift_key() => {
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
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("n") =>
            {
                self.step_search(SearchDirection::Next);
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("p") =>
            {
                self.step_search(SearchDirection::Previous);
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("r") =>
            {
                self.cycle_search_match_type();
                true
            }
            Key::Character(character)
                if modifiers.control_key() && character.eq_ignore_ascii_case("u") =>
            {
                self.update_search_query("");
                true
            }
            Key::Character(text) if !modifiers.control_key() && !modifiers.alt_key() => {
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
        self.update_search_query_with_direction(query, SearchDirection::Next)
    }

    fn cycle_search_match_type(&mut self) -> bool {
        let Some(search) = self.search.as_ref() else {
            return false;
        };
        let query = search.query.clone();
        let match_type = search.match_type.next();
        self.update_search_query_with_type(&query, SearchDirection::Next, match_type)
    }

    fn update_search_query_with_direction(
        &mut self,
        query: &str,
        direction: SearchDirection,
    ) -> bool {
        let match_type = self
            .search
            .as_ref()
            .map_or(WindowSearchMatchType::CaseSensitive, |search| {
                search.match_type
            });
        self.update_search_query_with_type(query, direction, match_type)
    }

    fn update_search_query_with_type(
        &mut self,
        query: &str,
        direction: SearchDirection,
        match_type: WindowSearchMatchType,
    ) -> bool {
        let mut search = WindowSearch {
            query: query.to_owned(),
            current: None,
            match_type,
        };

        if query.is_empty() {
            self.search = Some(search);
            self.selection = None;
            self.refresh_snapshot();
            self.apply_window_title();
            return false;
        }

        let found = find_window_search_match_with_type(
            self.runtime.terminal(),
            query,
            None,
            direction,
            match_type,
        );
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

        let found = find_window_search_match_with_type(
            self.runtime.terminal(),
            &search.query,
            search.current,
            direction,
            search.match_type,
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

    fn step_search_page(&mut self, direction: SearchDirection) -> bool {
        let Some(search) = self.search.as_ref() else {
            return false;
        };
        if search.query.is_empty() {
            return false;
        }

        let size = self.runtime.terminal().grid().size();
        if size.rows == 0 {
            return self.step_search(direction);
        }

        let history_len = self.runtime.terminal().scrollback().len();
        let viewport_top = copy_mode_viewport_top(history_len, self.scrollback_offset);
        let found = find_window_search_page_match_with_type(
            self.runtime.terminal(),
            &search.query,
            viewport_top,
            usize::from(size.rows),
            direction,
            search.match_type,
        )
        .or_else(|| {
            find_window_search_match_with_type(
                self.runtime.terminal(),
                &search.query,
                search.current,
                direction,
                search.match_type,
            )
        });
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
        let (offset, selection) = search_match.viewport_selection(scrollback_len);
        self.scrollback_offset = offset;
        self.selection = Some(selection);
        if let Some(copy_mode) = self.copy_mode.as_mut() {
            copy_mode.cursor = selection.anchor;
            copy_mode.source_cursor = SelectionSourceCell {
                row: search_match.source_row,
                column: usize::from(search_match.start_column),
            };
            copy_mode.anchor = None;
            copy_mode.source_anchor = None;
        }
        self.refresh_snapshot();
        self.apply_window_title();
    }

    fn copy_selection_to_clipboard(&mut self) -> bool {
        self.copy_selection_to(WindowCopyDestination::Clipboard)
    }

    fn copy_selection_to_primary_selection(&mut self) -> bool {
        self.copy_selection_to(WindowCopyDestination::PrimarySelection)
    }

    fn copy_selection_to_clipboard_and_primary_selection(&mut self) -> bool {
        self.copy_selection_to(WindowCopyDestination::ClipboardAndPrimarySelection)
    }

    fn copy_selection_to(&mut self, destination: WindowCopyDestination) -> bool {
        let Some(text) = self.selected_text() else {
            return false;
        };

        match destination {
            WindowCopyDestination::Clipboard => self.write_clipboard_text(&text),
            WindowCopyDestination::PrimarySelection => self.write_primary_selection_text(&text),
            WindowCopyDestination::ClipboardAndPrimarySelection => {
                let clipboard_written = self.write_clipboard_text(&text);
                let primary_written = self.write_primary_selection_text(&text);
                clipboard_written || primary_written
            }
        }
    }

    fn paste_selected_text_to_pane(&mut self) -> io::Result<bool> {
        let Some(text) = self.selected_text() else {
            return Ok(false);
        };
        if text.is_empty() {
            return Ok(false);
        }

        let bytes = encode_window_paste(&text, self.runtime.bracketed_paste());
        self.write_pty_bytes(&bytes)?;
        Ok(true)
    }

    fn write_clipboard_text(&mut self, text: &str) -> bool {
        (self.clipboard_writer)(text)
    }

    fn read_clipboard_text(&mut self) -> Option<String> {
        (self.clipboard_reader)()
    }

    fn write_primary_selection_text(&mut self, text: &str) -> bool {
        (self.primary_selection_writer)(text)
    }

    fn read_primary_selection_text(&mut self) -> Option<String> {
        (self.primary_selection_reader)()
    }

    fn dispatch_notification(&mut self, notification: &TerminalNotification) -> bool {
        (self.notification_handler)(notification)
    }

    fn dispatch_open_uri(&mut self, event: &NativeWindowOpenUri) -> bool {
        (self.open_uri_handler)(event)
    }

    fn dispatch_bells(&mut self, pane: rssh_core::PaneId, count: u64) {
        for _ in 0..count {
            self.dispatch_bell(NativeWindowBell { pane });
        }
    }

    fn dispatch_bell(&mut self, bell: NativeWindowBell) -> bool {
        (self.bell_handler)(&bell)
    }

    fn dispatch_focus_change(&mut self, change: &NativeWindowFocusChange) -> bool {
        (self.focus_change_handler)(change)
    }

    fn dispatch_resize(&mut self, resize: &NativeWindowResize) -> bool {
        (self.resize_handler)(resize)
    }

    fn dispatch_user_var_change(&mut self, change: &NativeWindowUserVarChange) -> bool {
        (self.user_var_change_handler)(change)
    }

    fn answer_clipboard_query(&mut self, selection: &str) -> io::Result<bool> {
        let Some(text) = self.read_clipboard_text() else {
            return Ok(false);
        };

        let response = encode_osc52_clipboard_response(selection, &text);
        self.write_pty_bytes(&response)?;
        Ok(true)
    }

    fn selected_text(&self) -> Option<String> {
        if let Some(copy_mode) = self.copy_mode.as_ref() {
            if let Some(selection) =
                copy_mode_source_selection(copy_mode, self.runtime.terminal().grid().size())
            {
                let text = selection.text_from_terminal(self.runtime.terminal())?;
                return (!text.is_empty()).then_some(text);
            }
        }

        let selection = self.selection?;
        let text =
            selection.text_from_snapshot(&self.snapshot, self.runtime.terminal().grid().size());
        (!text.is_empty()).then_some(text)
    }

    fn handle_window_paste(&mut self) -> io::Result<bool> {
        self.handle_window_paste_from(WindowPasteSource::Clipboard)
    }

    fn handle_window_primary_selection_paste(&mut self) -> io::Result<bool> {
        self.handle_window_paste_from(WindowPasteSource::PrimarySelection)
    }

    fn handle_window_paste_from(&mut self, source: WindowPasteSource) -> io::Result<bool> {
        let text = match source {
            WindowPasteSource::Clipboard => self.read_clipboard_text(),
            WindowPasteSource::PrimarySelection => self.read_primary_selection_text(),
        };
        let Some(text) = text else {
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
        let change = NativeWindowFocusChange {
            pane: self.app_shell.active_pane_id(),
            focused,
        };
        self.dispatch_focus_change(&change);

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
        self.frame_height =
            u32::from(terminal_size.rows.saturating_add(TAB_BAR_ROWS)) * CELL_HEIGHT;

        if let Some(pixels) = self.pixels.as_mut() {
            pixels.resize_buffer(self.frame_width, self.frame_height)?;
        }

        self.runtime.resize(terminal_size);
        for runtime in self.pane_runtimes.values_mut() {
            runtime.runtime.resize(terminal_size);
            if let Some(session) = runtime.session.as_mut() {
                session.resize(PtySize::try_new(terminal_size.columns, terminal_size.rows)?)?;
            }
            runtime.scrollback_offset = runtime
                .scrollback_offset
                .min(runtime.runtime.terminal().scrollback().len());
            runtime.snapshot = TerminalRenderSnapshot::from_terminal_viewport(
                runtime.runtime.terminal(),
                runtime.scrollback_offset,
            );
        }

        if let Some(session) = self.session.as_mut() {
            let pty_size = PtySize::try_new(terminal_size.columns, terminal_size.rows)?;
            session.resize(pty_size)?;
        }
        self.refresh_snapshot();
        let resize = NativeWindowResize {
            pane: self.app_shell.active_pane_id(),
            pixel_width: size.width,
            pixel_height: size.height,
            terminal_size,
        };
        self.dispatch_resize(&resize);

        Ok(())
    }
}

impl Drop for NativeWindowApp {
    fn drop(&mut self) {
        self.stop_active_runtime();

        for runtime in self.pane_runtimes.values_mut() {
            runtime.close();
        }

        self.pane_runtimes.clear();
    }
}

impl NativeWindowApp {
    fn stop_active_runtime(&mut self) {
        if let Some(session) = self.session.as_mut() {
            let _ = session.kill();
            let _ = session.wait();
        }

        if let Some(reader_thread) = self.reader_thread.take() {
            let _ = reader_thread.join();
        }

        self.session = None;
        self.writer = None;
    }
}

#[cfg(test)]
fn encode_window_key(
    key: &Key,
    physical_key: PhysicalKey,
    text: Option<&str>,
    modifiers: ModifiersState,
    application_cursor_keys: bool,
    application_keypad: bool,
) -> Vec<u8> {
    encode_window_key_with_kitty(
        key,
        physical_key,
        text,
        modifiers,
        application_cursor_keys,
        application_keypad,
        0,
        0,
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn encode_window_key_with_kitty(
    key: &Key,
    physical_key: PhysicalKey,
    text: Option<&str>,
    modifiers: ModifiersState,
    application_cursor_keys: bool,
    application_keypad: bool,
    kitty_keyboard_flags: u16,
    modify_other_keys: u8,
) -> Vec<u8> {
    encode_window_key_with_kitty_event(
        key,
        physical_key,
        text,
        modifiers,
        application_cursor_keys,
        application_keypad,
        kitty_keyboard_flags,
        modify_other_keys,
        KittyKeyEventKind::Press,
    )
}

#[allow(clippy::too_many_arguments)]
fn encode_window_key_with_kitty_event(
    key: &Key,
    physical_key: PhysicalKey,
    text: Option<&str>,
    modifiers: ModifiersState,
    application_cursor_keys: bool,
    application_keypad: bool,
    kitty_keyboard_flags: u16,
    modify_other_keys: u8,
    key_event_kind: KittyKeyEventKind,
) -> Vec<u8> {
    let alt = modifiers.alt_key();

    if key_event_kind != KittyKeyEventKind::Press {
        return encode_kitty_event_window_key(
            key,
            physical_key,
            text,
            modifiers,
            kitty_keyboard_flags,
            key_event_kind,
        )
        .unwrap_or_default();
    }

    if let Some(bytes) = encode_kitty_modifier_window_key(
        physical_key,
        modifiers,
        kitty_keyboard_flags,
        key_event_kind,
    ) {
        return bytes;
    }

    if let Some(bytes) = encode_kitty_keypad_window_key(
        physical_key,
        modifiers,
        kitty_keyboard_flags,
        key_event_kind,
    ) {
        return bytes;
    }

    if let Some(bytes) =
        encode_kitty_functional_window_key(key, modifiers, kitty_keyboard_flags, key_event_kind)
    {
        return bytes;
    }

    if let Some(bytes) = encode_kitty_report_all_window_key(
        key,
        physical_key,
        text,
        modifiers,
        kitty_keyboard_flags,
        key_event_kind,
    ) {
        return bytes;
    }

    if let Some(bytes) = encode_kitty_disambiguated_window_key(
        key,
        physical_key,
        modifiers,
        kitty_keyboard_flags,
        key_event_kind,
    ) {
        return bytes;
    }

    if let Some(bytes) = encode_xterm_modify_other_window_key(key, modifiers, modify_other_keys) {
        return bytes;
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KittyKeyEventKind {
    Press,
    Repeat,
    Release,
}

impl KittyKeyEventKind {
    fn from_winit_key(key: &winit::event::KeyEvent) -> Self {
        match key.state {
            ElementState::Released => Self::Release,
            ElementState::Pressed if key.repeat => Self::Repeat,
            ElementState::Pressed => Self::Press,
        }
    }
}

fn encode_kitty_event_window_key(
    key: &Key,
    physical_key: PhysicalKey,
    text: Option<&str>,
    modifiers: ModifiersState,
    kitty_keyboard_flags: u16,
    key_event_kind: KittyKeyEventKind,
) -> Option<Vec<u8>> {
    encode_kitty_modifier_window_key(
        physical_key,
        modifiers,
        kitty_keyboard_flags,
        key_event_kind,
    )
    .or_else(|| {
        encode_kitty_keypad_window_key(
            physical_key,
            modifiers,
            kitty_keyboard_flags,
            key_event_kind,
        )
    })
    .or_else(|| {
        encode_kitty_functional_window_key(key, modifiers, kitty_keyboard_flags, key_event_kind)
    })
    .or_else(|| {
        encode_kitty_report_all_window_key(
            key,
            physical_key,
            text,
            modifiers,
            kitty_keyboard_flags,
            key_event_kind,
        )
    })
    .or_else(|| {
        encode_kitty_disambiguated_window_key(
            key,
            physical_key,
            modifiers,
            kitty_keyboard_flags,
            key_event_kind,
        )
    })
}

fn encode_kitty_modifier_window_key(
    physical_key: PhysicalKey,
    modifiers: ModifiersState,
    kitty_keyboard_flags: u16,
    key_event_kind: KittyKeyEventKind,
) -> Option<Vec<u8>> {
    let event_type = kitty_window_event_type(key_event_kind, kitty_keyboard_flags);
    if kitty_keyboard_flags
        & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_REPORT_EVENTS)
        == 0
    {
        return None;
    }
    if key_event_kind == KittyKeyEventKind::Press
        && kitty_keyboard_flags & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL) == 0
    {
        return None;
    }

    let key_code = kitty_window_modifier_key_code(physical_key)?;
    Some(kitty_csi_u_key_with_event(
        key_code,
        kitty_window_modifier(modifiers),
        event_type,
        None,
    ))
}

fn encode_kitty_keypad_window_key(
    physical_key: PhysicalKey,
    modifiers: ModifiersState,
    kitty_keyboard_flags: u16,
    key_event_kind: KittyKeyEventKind,
) -> Option<Vec<u8>> {
    let event_type = kitty_window_event_type(key_event_kind, kitty_keyboard_flags);
    if kitty_keyboard_flags
        & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_REPORT_EVENTS)
        == 0
    {
        return None;
    }
    if key_event_kind == KittyKeyEventKind::Press
        && kitty_keyboard_flags & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL) == 0
    {
        return None;
    }

    let key_code = kitty_window_keypad_code(physical_key)?;
    Some(kitty_csi_u_key_with_event(
        key_code,
        kitty_window_modifier(modifiers),
        event_type,
        None,
    ))
}

fn encode_kitty_functional_window_key(
    key: &Key,
    modifiers: ModifiersState,
    kitty_keyboard_flags: u16,
    key_event_kind: KittyKeyEventKind,
) -> Option<Vec<u8>> {
    let event_type = kitty_window_event_type(key_event_kind, kitty_keyboard_flags);
    if kitty_keyboard_flags
        & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_REPORT_EVENTS)
        == 0
    {
        return None;
    }
    if key_event_kind == KittyKeyEventKind::Press
        && kitty_keyboard_flags & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL) == 0
    {
        return None;
    }

    let Key::Named(named) = key.as_ref() else {
        return None;
    };
    let modifier = kitty_window_modifier(modifiers);
    match named {
        NamedKey::Escape => Some(kitty_csi_u_key_with_event(27, modifier, event_type, None)),
        NamedKey::Enter if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL != 0 => {
            Some(kitty_csi_u_key_with_event(13, modifier, event_type, None))
        }
        NamedKey::Tab if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL != 0 => {
            Some(kitty_csi_u_key_with_event(9, modifier, event_type, None))
        }
        NamedKey::Backspace if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL != 0 => {
            Some(kitty_csi_u_key_with_event(127, modifier, event_type, None))
        }
        NamedKey::ArrowUp => Some(kitty_csi_final_key_with_event(b'A', modifier, event_type)),
        NamedKey::ArrowDown => Some(kitty_csi_final_key_with_event(b'B', modifier, event_type)),
        NamedKey::ArrowRight => Some(kitty_csi_final_key_with_event(b'C', modifier, event_type)),
        NamedKey::ArrowLeft => Some(kitty_csi_final_key_with_event(b'D', modifier, event_type)),
        NamedKey::End => Some(kitty_csi_final_key_with_event(b'F', modifier, event_type)),
        NamedKey::Home => Some(kitty_csi_final_key_with_event(b'H', modifier, event_type)),
        NamedKey::Insert => Some(kitty_csi_tilde_key_with_event(2, modifier, event_type)),
        NamedKey::Delete => Some(kitty_csi_tilde_key_with_event(3, modifier, event_type)),
        NamedKey::PageUp => Some(kitty_csi_tilde_key_with_event(5, modifier, event_type)),
        NamedKey::PageDown => Some(kitty_csi_tilde_key_with_event(6, modifier, event_type)),
        NamedKey::F1 => Some(kitty_csi_final_key_with_event(b'P', modifier, event_type)),
        NamedKey::F2 => Some(kitty_csi_final_key_with_event(b'Q', modifier, event_type)),
        NamedKey::F3 => Some(kitty_csi_final_key_with_event(b'R', modifier, event_type)),
        NamedKey::F4 => Some(kitty_csi_final_key_with_event(b'S', modifier, event_type)),
        NamedKey::F5 => Some(kitty_csi_tilde_key_with_event(15, modifier, event_type)),
        NamedKey::F6 => Some(kitty_csi_tilde_key_with_event(17, modifier, event_type)),
        NamedKey::F7 => Some(kitty_csi_tilde_key_with_event(18, modifier, event_type)),
        NamedKey::F8 => Some(kitty_csi_tilde_key_with_event(19, modifier, event_type)),
        NamedKey::F9 => Some(kitty_csi_tilde_key_with_event(20, modifier, event_type)),
        NamedKey::F10 => Some(kitty_csi_tilde_key_with_event(21, modifier, event_type)),
        NamedKey::F11 => Some(kitty_csi_tilde_key_with_event(23, modifier, event_type)),
        NamedKey::F12 => Some(kitty_csi_tilde_key_with_event(24, modifier, event_type)),
        _ => kitty_pua_function_key_code(named)
            .map(|key_code| kitty_csi_u_key_with_event(key_code, modifier, event_type, None)),
    }
}

fn encode_kitty_report_all_window_key(
    key: &Key,
    physical_key: PhysicalKey,
    text: Option<&str>,
    modifiers: ModifiersState,
    kitty_keyboard_flags: u16,
    key_event_kind: KittyKeyEventKind,
) -> Option<Vec<u8>> {
    if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL == 0 {
        return None;
    }

    let key_code = match key.as_ref() {
        Key::Character(character) => kitty_window_key_code(
            character.chars().next()?,
            physical_key,
            modifiers,
            kitty_keyboard_flags,
        ),
        Key::Named(NamedKey::Enter) => 13.to_string(),
        Key::Named(NamedKey::Tab) => 9.to_string(),
        Key::Named(NamedKey::Backspace) => 127.to_string(),
        Key::Named(NamedKey::Escape) => 27.to_string(),
        _ => return None,
    };
    Some(kitty_csi_u_key_with_event(
        key_code,
        kitty_window_modifier(modifiers),
        kitty_window_event_type(key_event_kind, kitty_keyboard_flags),
        associated_text_from_window_key(text, kitty_keyboard_flags, key_event_kind).as_deref(),
    ))
}

fn encode_kitty_disambiguated_window_key(
    key: &Key,
    physical_key: PhysicalKey,
    modifiers: ModifiersState,
    kitty_keyboard_flags: u16,
    key_event_kind: KittyKeyEventKind,
) -> Option<Vec<u8>> {
    if kitty_keyboard_flags & (KITTY_KEYBOARD_DISAMBIGUATE | KITTY_KEYBOARD_REPORT_ALL) == 0 {
        return None;
    }
    if !(modifiers.control_key() || modifiers.alt_key() || modifiers.super_key()) {
        return None;
    }

    let Key::Character(character) = key.as_ref() else {
        return None;
    };
    let character = character.chars().next()?;
    let key_code = if kitty_keyboard_flags & KITTY_KEYBOARD_ALTERNATE_KEYS != 0 {
        kitty_window_key_code(character, physical_key, modifiers, kitty_keyboard_flags)
    } else {
        kitty_ascii_key_code(character)?.to_string()
    };
    let modifier = kitty_window_modifier(modifiers)?;
    Some(kitty_csi_u_key_with_event(
        key_code,
        Some(modifier),
        kitty_window_event_type(key_event_kind, kitty_keyboard_flags),
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

fn kitty_window_keypad_code(physical_key: PhysicalKey) -> Option<u32> {
    let PhysicalKey::Code(code) = physical_key else {
        return None;
    };

    match code {
        WinitKeyCode::Numpad0 => Some(57399),
        WinitKeyCode::Numpad1 => Some(57400),
        WinitKeyCode::Numpad2 => Some(57401),
        WinitKeyCode::Numpad3 => Some(57402),
        WinitKeyCode::Numpad4 => Some(57403),
        WinitKeyCode::Numpad5 => Some(57404),
        WinitKeyCode::Numpad6 => Some(57405),
        WinitKeyCode::Numpad7 => Some(57406),
        WinitKeyCode::Numpad8 => Some(57407),
        WinitKeyCode::Numpad9 => Some(57408),
        WinitKeyCode::NumpadDecimal => Some(57409),
        WinitKeyCode::NumpadDivide => Some(57410),
        WinitKeyCode::NumpadMultiply => Some(57411),
        WinitKeyCode::NumpadSubtract => Some(57412),
        WinitKeyCode::NumpadAdd => Some(57413),
        WinitKeyCode::NumpadEnter => Some(57414),
        WinitKeyCode::NumpadEqual => Some(57415),
        WinitKeyCode::NumpadComma => Some(57416),
        _ => None,
    }
}

fn kitty_window_modifier_key_code(physical_key: PhysicalKey) -> Option<u32> {
    let PhysicalKey::Code(code) = physical_key else {
        return None;
    };

    match code {
        WinitKeyCode::ShiftLeft => Some(57441),
        WinitKeyCode::ControlLeft => Some(57442),
        WinitKeyCode::AltLeft => Some(57443),
        WinitKeyCode::SuperLeft => Some(57444),
        WinitKeyCode::ShiftRight => Some(57447),
        WinitKeyCode::ControlRight => Some(57448),
        WinitKeyCode::AltRight => Some(57449),
        WinitKeyCode::SuperRight => Some(57450),
        _ => None,
    }
}

fn kitty_window_key_code(
    character: char,
    physical_key: PhysicalKey,
    modifiers: ModifiersState,
    kitty_keyboard_flags: u16,
) -> String {
    let base_layout = kitty_base_layout_key(physical_key);
    let primary = if modifiers.shift_key() {
        base_layout.map_or_else(|| kitty_key_code(character), u32::from)
    } else {
        kitty_key_code(character)
    };

    if kitty_keyboard_flags & KITTY_KEYBOARD_ALTERNATE_KEYS == 0 {
        return primary.to_string();
    }

    let shifted = modifiers.shift_key().then_some(u32::from(character));
    let base = base_layout.map(u32::from);
    match (shifted, base) {
        (Some(shifted), Some(base)) => format!("{primary}:{shifted}:{base}"),
        (Some(shifted), None) => format!("{primary}:{shifted}"),
        (None, Some(base)) if base != primary => format!("{primary}::{base}"),
        _ => primary.to_string(),
    }
}

fn kitty_base_layout_key(physical_key: PhysicalKey) -> Option<char> {
    let PhysicalKey::Code(code) = physical_key else {
        return None;
    };

    match code {
        WinitKeyCode::Backquote => Some('`'),
        WinitKeyCode::Backslash => Some('\\'),
        WinitKeyCode::BracketLeft => Some('['),
        WinitKeyCode::BracketRight => Some(']'),
        WinitKeyCode::Comma => Some(','),
        WinitKeyCode::Digit0 => Some('0'),
        WinitKeyCode::Digit1 => Some('1'),
        WinitKeyCode::Digit2 => Some('2'),
        WinitKeyCode::Digit3 => Some('3'),
        WinitKeyCode::Digit4 => Some('4'),
        WinitKeyCode::Digit5 => Some('5'),
        WinitKeyCode::Digit6 => Some('6'),
        WinitKeyCode::Digit7 => Some('7'),
        WinitKeyCode::Digit8 => Some('8'),
        WinitKeyCode::Digit9 => Some('9'),
        WinitKeyCode::Equal => Some('='),
        WinitKeyCode::KeyA => Some('a'),
        WinitKeyCode::KeyB => Some('b'),
        WinitKeyCode::KeyC => Some('c'),
        WinitKeyCode::KeyD => Some('d'),
        WinitKeyCode::KeyE => Some('e'),
        WinitKeyCode::KeyF => Some('f'),
        WinitKeyCode::KeyG => Some('g'),
        WinitKeyCode::KeyH => Some('h'),
        WinitKeyCode::KeyI => Some('i'),
        WinitKeyCode::KeyJ => Some('j'),
        WinitKeyCode::KeyK => Some('k'),
        WinitKeyCode::KeyL => Some('l'),
        WinitKeyCode::KeyM => Some('m'),
        WinitKeyCode::KeyN => Some('n'),
        WinitKeyCode::KeyO => Some('o'),
        WinitKeyCode::KeyP => Some('p'),
        WinitKeyCode::KeyQ => Some('q'),
        WinitKeyCode::KeyR => Some('r'),
        WinitKeyCode::KeyS => Some('s'),
        WinitKeyCode::KeyT => Some('t'),
        WinitKeyCode::KeyU => Some('u'),
        WinitKeyCode::KeyV => Some('v'),
        WinitKeyCode::KeyW => Some('w'),
        WinitKeyCode::KeyX => Some('x'),
        WinitKeyCode::KeyY => Some('y'),
        WinitKeyCode::KeyZ => Some('z'),
        WinitKeyCode::Minus => Some('-'),
        WinitKeyCode::Period => Some('.'),
        WinitKeyCode::Quote => Some('\''),
        WinitKeyCode::Semicolon => Some(';'),
        WinitKeyCode::Slash => Some('/'),
        WinitKeyCode::Space => Some(' '),
        _ => None,
    }
}

fn associated_text_from_window_key(
    text: Option<&str>,
    kitty_keyboard_flags: u16,
    key_event_kind: KittyKeyEventKind,
) -> Option<String> {
    if kitty_keyboard_flags & (KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_ASSOCIATED_TEXT)
        != (KITTY_KEYBOARD_REPORT_ALL | KITTY_KEYBOARD_ASSOCIATED_TEXT)
    {
        return None;
    }
    if key_event_kind == KittyKeyEventKind::Release {
        return None;
    }

    associated_text_codepoints(text?.chars())
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
    modifier: Option<u8>,
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
    modifier: Option<u8>,
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
    modifier: Option<u8>,
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

fn kitty_window_event_type(
    key_event_kind: KittyKeyEventKind,
    kitty_keyboard_flags: u16,
) -> Option<u8> {
    if kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_EVENTS == 0 {
        return None;
    }

    match key_event_kind {
        KittyKeyEventKind::Press => None,
        KittyKeyEventKind::Repeat => Some(2),
        KittyKeyEventKind::Release => Some(3),
    }
}

fn kitty_pua_function_key_code(named: NamedKey) -> Option<u32> {
    match named {
        NamedKey::CapsLock => return Some(57358),
        NamedKey::ScrollLock => return Some(57359),
        NamedKey::NumLock => return Some(57360),
        NamedKey::PrintScreen => return Some(57361),
        NamedKey::Pause => return Some(57362),
        NamedKey::ContextMenu => return Some(57363),
        NamedKey::MediaPlay => return Some(57428),
        NamedKey::MediaPause => return Some(57429),
        NamedKey::MediaPlayPause => return Some(57430),
        NamedKey::MediaRewind => return Some(57434),
        NamedKey::MediaStop => return Some(57432),
        NamedKey::MediaFastForward => return Some(57433),
        NamedKey::MediaTrackNext => return Some(57435),
        NamedKey::MediaTrackPrevious => return Some(57436),
        NamedKey::MediaRecord => return Some(57437),
        NamedKey::AudioVolumeDown => return Some(57438),
        NamedKey::AudioVolumeUp => return Some(57439),
        NamedKey::AudioVolumeMute => return Some(57440),
        _ => {}
    }

    let offset = match named {
        NamedKey::F13 => 0,
        NamedKey::F14 => 1,
        NamedKey::F15 => 2,
        NamedKey::F16 => 3,
        NamedKey::F17 => 4,
        NamedKey::F18 => 5,
        NamedKey::F19 => 6,
        NamedKey::F20 => 7,
        NamedKey::F21 => 8,
        NamedKey::F22 => 9,
        NamedKey::F23 => 10,
        NamedKey::F24 => 11,
        NamedKey::F25 => 12,
        NamedKey::F26 => 13,
        NamedKey::F27 => 14,
        NamedKey::F28 => 15,
        NamedKey::F29 => 16,
        NamedKey::F30 => 17,
        NamedKey::F31 => 18,
        NamedKey::F32 => 19,
        NamedKey::F33 => 20,
        NamedKey::F34 => 21,
        NamedKey::F35 => 22,
        _ => return None,
    };
    Some(57376 + offset)
}

fn encode_xterm_modify_other_window_key(
    key: &Key,
    modifiers: ModifiersState,
    modify_other_keys: u8,
) -> Option<Vec<u8>> {
    if modify_other_keys == 0 {
        return None;
    }
    let modifier = xterm_window_modifier(modifiers)?;
    let key_code = match key.as_ref() {
        Key::Character(character) => u32::from(character.chars().next()?),
        Key::Named(NamedKey::Enter) => 13,
        Key::Named(NamedKey::Tab) => 9,
        Key::Named(NamedKey::Backspace) => 127,
        Key::Named(NamedKey::Escape) => 27,
        _ => return None,
    };

    Some(format!("\x1b[27;{modifier};{key_code}~").into_bytes())
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

fn copy_mode_viewport_top(history_len: usize, scrollback_offset: usize) -> usize {
    history_len.saturating_sub(scrollback_offset.min(history_len))
}

fn apply_isize_delta_to_usize(current: usize, delta: isize, max: usize) -> usize {
    if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta.unsigned_abs()).min(max)
    }
}

fn copy_mode_viewport_cell_for_source_position(
    source_row: usize,
    source_column: usize,
    current_offset: usize,
    history_len: usize,
    size: TerminalSize,
) -> Option<(usize, SelectionCell)> {
    let current_viewport_top = copy_mode_viewport_top(history_len, current_offset);
    if let Some(cell) =
        copy_mode_cell_for_source_position(source_row, source_column, current_viewport_top, size)
    {
        return Some((current_offset.min(history_len), cell));
    }

    let target_offset = if source_row < history_len {
        history_len.saturating_sub(source_row)
    } else {
        0
    };
    let target_viewport_top = copy_mode_viewport_top(history_len, target_offset);
    let cell =
        copy_mode_cell_for_source_position(source_row, source_column, target_viewport_top, size)?;

    Some((target_offset, cell))
}

fn copy_mode_cell_for_source_position(
    source_row: usize,
    source_column: usize,
    viewport_top: usize,
    size: TerminalSize,
) -> Option<SelectionCell> {
    let row = source_row.checked_sub(viewport_top)?;
    if row >= usize::from(size.rows) || source_column >= usize::from(size.columns) {
        return None;
    }

    Some(SelectionCell {
        row: u16::try_from(row).ok()?,
        column: u16::try_from(source_column).ok()?,
    })
}

fn copy_mode_semantic_zone_type_for_key(character: &str) -> Option<SemanticType> {
    if character.eq_ignore_ascii_case("z") || character.eq_ignore_ascii_case("o") {
        Some(SemanticType::Output)
    } else if character.eq_ignore_ascii_case("p") {
        Some(SemanticType::Prompt)
    } else if character.eq_ignore_ascii_case("i") {
        Some(SemanticType::Input)
    } else {
        None
    }
}

fn copy_mode_line_content_bounds(terminal: &Terminal, source_row: usize) -> Option<(usize, usize)> {
    let columns = usize::from(terminal.grid().size().columns);
    if columns == 0 {
        return None;
    }

    let line = terminal.text_from_region(0, source_row, columns.saturating_sub(1), source_row)?;
    let mut bounds = None;
    for (column, character) in line.chars().enumerate() {
        if character != ' ' {
            bounds = Some(match bounds {
                Some((start, _)) => (start, column),
                None => (column, column),
            });
        }
    }

    Some(bounds.unwrap_or((0, 0)))
}

fn copy_mode_jump_target(
    terminal: &Terminal,
    cursor: SelectionSourceCell,
    jump: WindowCopyJump,
    repeat: bool,
) -> Option<SelectionSourceCell> {
    let columns = usize::from(terminal.grid().size().columns);
    if columns == 0 {
        return None;
    }

    let line = copy_mode_source_line(terminal, cursor.row)?;
    let mut candidates = line
        .chars()
        .enumerate()
        .filter_map(|(column, character)| (character == jump.target).then_some(column))
        .collect::<Vec<_>>();
    if !jump.forward {
        candidates.reverse();
    }

    let cursor_column = match (jump.prev_char && repeat, jump.forward) {
        (false, _) => cursor.column,
        (true, true) => cursor.column.saturating_add(1),
        (true, false) => cursor.column.saturating_sub(1),
    };

    let target = candidates.into_iter().find(|column| {
        if jump.forward {
            *column > cursor_column
        } else {
            *column < cursor_column
        }
    })?;

    let target_column = match (jump.prev_char, jump.forward) {
        (false, _) => target,
        (true, true) => target.saturating_sub(1),
        (true, false) => target.saturating_add(1),
    }
    .min(columns.saturating_sub(1));

    Some(SelectionSourceCell {
        row: cursor.row,
        column: target_column,
    })
}

fn copy_mode_word_target(
    terminal: &Terminal,
    cursor: SelectionSourceCell,
    movement: WindowCopyWordMovement,
) -> Option<SelectionSourceCell> {
    let columns = usize::from(terminal.grid().size().columns);
    if columns == 0 {
        return None;
    }

    let row_count = terminal
        .scrollback()
        .len()
        .saturating_add(usize::from(terminal.grid().size().rows));
    if cursor.row >= row_count {
        return None;
    }

    match movement {
        WindowCopyWordMovement::Backward => copy_mode_backward_word_target(terminal, cursor),
        WindowCopyWordMovement::Forward => copy_mode_forward_word_target(terminal, cursor),
        WindowCopyWordMovement::End => copy_mode_forward_word_end_target(terminal, cursor),
    }
}

fn copy_mode_backward_word_target(
    terminal: &Terminal,
    cursor: SelectionSourceCell,
) -> Option<SelectionSourceCell> {
    if cursor.column == 0 && cursor.row > 0 {
        let previous_line = copy_mode_source_line(terminal, cursor.row.saturating_sub(1))?;
        let previous_column = previous_line.chars().count().saturating_sub(1);
        return copy_mode_backward_word_target(
            terminal,
            SelectionSourceCell {
                row: cursor.row.saturating_sub(1),
                column: previous_column,
            },
        );
    }

    let line = copy_mode_source_line(terminal, cursor.row)?;
    let line_len = line.chars().count();
    if line_len == 0 {
        return None;
    }

    let cursor_column = cursor.column.min(line_len.saturating_sub(1));
    let segments = copy_mode_word_segments(&line);
    let mut target_column = cursor_column;
    let mut last_was_whitespace = false;

    for (index, segment) in copy_mode_prefix_word_segments(&segments, cursor_column)
        .into_iter()
        .rev()
        .enumerate()
    {
        let width = segment.end.saturating_sub(segment.start);
        if width == 0 {
            continue;
        }

        if segment.is_whitespace {
            target_column = target_column.saturating_sub(width);
            last_was_whitespace = true;
            continue;
        }

        last_was_whitespace = false;
        if index == 0 && width == 1 {
            target_column = target_column.saturating_sub(width);
            continue;
        }

        target_column = target_column.saturating_sub(width.saturating_sub(1));
        break;
    }

    if last_was_whitespace && cursor.row > 0 {
        let previous_line = copy_mode_source_line(terminal, cursor.row.saturating_sub(1))?;
        let previous_column = previous_line.chars().count().saturating_sub(1);
        return copy_mode_backward_word_target(
            terminal,
            SelectionSourceCell {
                row: cursor.row.saturating_sub(1),
                column: previous_column,
            },
        );
    }

    Some(SelectionSourceCell {
        row: cursor.row,
        column: target_column,
    })
}

fn copy_mode_forward_word_target(
    terminal: &Terminal,
    cursor: SelectionSourceCell,
) -> Option<SelectionSourceCell> {
    let line = copy_mode_source_line(terminal, cursor.row)?;
    let line_len = line.chars().count();
    if line_len == 0 {
        return copy_mode_next_line_content_target(terminal, cursor.row);
    }

    let cursor_column = cursor.column.min(line_len);
    let mut target_column = cursor_column;
    let suffix = copy_mode_suffix_word_segments(&copy_mode_word_segments(&line), cursor_column);
    let mut segments = suffix.into_iter();

    if let Some(segment) = segments.next() {
        target_column = target_column.saturating_add(segment.end.saturating_sub(cursor_column));
        if !segment.is_whitespace {
            if let Some(next_segment) = segments.next() {
                if next_segment.is_whitespace {
                    target_column =
                        target_column.saturating_add(next_segment.end - next_segment.start);
                }
            }
        }
    }

    if target_column >= line_len {
        return copy_mode_next_line_content_target(terminal, cursor.row).or(Some(
            SelectionSourceCell {
                row: cursor.row,
                column: line_len.saturating_sub(1),
            },
        ));
    }

    Some(SelectionSourceCell {
        row: cursor.row,
        column: target_column,
    })
}

fn copy_mode_forward_word_end_target(
    terminal: &Terminal,
    cursor: SelectionSourceCell,
) -> Option<SelectionSourceCell> {
    let line = copy_mode_source_line(terminal, cursor.row)?;
    let line_len = line.chars().count();
    if line_len == 0 {
        return copy_mode_next_line_content_target(terminal, cursor.row);
    }

    let cursor_column = cursor.column.min(line_len.saturating_sub(1));
    if cursor_column >= line_len.saturating_sub(1) {
        return copy_mode_next_line_first_word_end_target(terminal, cursor.row).or(Some(
            SelectionSourceCell {
                row: cursor.row,
                column: line_len.saturating_sub(1),
            },
        ));
    }

    let suffix = copy_mode_suffix_word_segments(&copy_mode_word_segments(&line), cursor_column);
    let mut segments = suffix.into_iter();
    let first_segment = segments.next()?;
    let mut word_end = first_segment.end;

    if !first_segment.is_whitespace && cursor_column == word_end.saturating_sub(1) {
        for next_segment in segments.by_ref() {
            word_end = next_segment.end;
            if !next_segment.is_whitespace {
                break;
            }
        }
    }

    for next_segment in segments {
        if next_segment.is_whitespace {
            break;
        }
        word_end = next_segment.end;
    }

    Some(SelectionSourceCell {
        row: cursor.row,
        column: word_end.saturating_sub(1),
    })
}

fn copy_mode_next_line_content_target(
    terminal: &Terminal,
    source_row: usize,
) -> Option<SelectionSourceCell> {
    let next_row = source_row.checked_add(1)?;
    let row_count = terminal
        .scrollback()
        .len()
        .saturating_add(usize::from(terminal.grid().size().rows));
    if next_row >= row_count {
        return None;
    }

    let (column, _) = copy_mode_line_content_bounds(terminal, next_row)?;
    Some(SelectionSourceCell {
        row: next_row,
        column,
    })
}

fn copy_mode_next_line_first_word_end_target(
    terminal: &Terminal,
    source_row: usize,
) -> Option<SelectionSourceCell> {
    let target = copy_mode_next_line_content_target(terminal, source_row)?;
    copy_mode_forward_word_end_target(terminal, target)
}

fn copy_mode_source_line(terminal: &Terminal, source_row: usize) -> Option<String> {
    let columns = usize::from(terminal.grid().size().columns);
    if columns == 0 {
        return None;
    }

    terminal.text_from_region(0, source_row, columns.saturating_sub(1), source_row)
}

fn copy_mode_word_segments(line: &str) -> Vec<WindowCopyWordSegment> {
    let mut column = 0_usize;
    line.split_word_bounds()
        .filter_map(|word| {
            let width = word.chars().count();
            if width == 0 {
                return None;
            }

            let segment = WindowCopyWordSegment {
                start: column,
                end: column.saturating_add(width),
                is_whitespace: is_copy_mode_whitespace_word(word),
            };
            column = segment.end;
            Some(segment)
        })
        .collect()
}

fn copy_mode_prefix_word_segments(
    segments: &[WindowCopyWordSegment],
    cursor_column: usize,
) -> Vec<WindowCopyWordSegment> {
    segments
        .iter()
        .filter_map(|segment| {
            if segment.start > cursor_column {
                return None;
            }

            Some(WindowCopyWordSegment {
                start: segment.start,
                end: segment.end.min(cursor_column.saturating_add(1)),
                is_whitespace: segment.is_whitespace,
            })
            .filter(|segment| segment.start < segment.end)
        })
        .collect()
}

fn copy_mode_suffix_word_segments(
    segments: &[WindowCopyWordSegment],
    cursor_column: usize,
) -> Vec<WindowCopyWordSegment> {
    segments
        .iter()
        .filter_map(|segment| {
            if segment.end <= cursor_column {
                return None;
            }

            Some(WindowCopyWordSegment {
                start: segment.start.max(cursor_column),
                end: segment.end,
                is_whitespace: segment.is_whitespace,
            })
            .filter(|segment| segment.start < segment.end)
        })
        .collect()
}

fn is_copy_mode_whitespace_word(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_whitespace)
}

fn copy_mode_source_selection(
    copy_mode: &WindowCopyMode,
    size: TerminalSize,
) -> Option<WindowSourceSelection> {
    match copy_mode.selection_mode {
        WindowCopySelectionMode::None => None,
        WindowCopySelectionMode::Cell => copy_mode
            .source_anchor
            .map(|anchor| WindowSourceSelection::new(anchor, copy_mode.source_cursor)),
        WindowCopySelectionMode::Block => copy_mode
            .source_anchor
            .map(|anchor| WindowSourceSelection::rectangular(anchor, copy_mode.source_cursor)),
        WindowCopySelectionMode::Line => {
            if size.columns == 0 {
                return None;
            }

            Some(WindowSourceSelection::new(
                SelectionSourceCell {
                    row: copy_mode.source_cursor.row,
                    column: 0,
                },
                SelectionSourceCell {
                    row: copy_mode.source_cursor.row,
                    column: usize::from(size.columns.saturating_sub(1)),
                },
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelectionSourceCell {
    row: usize,
    column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowSourceSelection {
    anchor: SelectionSourceCell,
    focus: SelectionSourceCell,
    rectangular: bool,
}

impl WindowSourceSelection {
    const fn new(anchor: SelectionSourceCell, focus: SelectionSourceCell) -> Self {
        Self {
            anchor,
            focus,
            rectangular: false,
        }
    }

    const fn rectangular(anchor: SelectionSourceCell, focus: SelectionSourceCell) -> Self {
        Self {
            anchor,
            focus,
            rectangular: true,
        }
    }

    fn text_from_terminal(self, terminal: &Terminal) -> Option<String> {
        if self.rectangular {
            let (start, end) = self.normalized_rectangular();
            let mut lines = Vec::new();
            for row in start.row..=end.row {
                lines.push(terminal.text_from_region(start.column, row, end.column, row)?);
            }
            return Some(lines.join("\n"));
        }

        let (start, end) = self.normalized();
        terminal.text_from_region(start.column, start.row, end.column, end.row)
    }

    fn viewport_selection(
        self,
        viewport_top: usize,
        size: TerminalSize,
    ) -> Option<WindowSelection> {
        if size.rows == 0 || size.columns == 0 {
            return None;
        }

        let (start, end) = self.normalized();
        let viewport_bottom = viewport_top.saturating_add(usize::from(size.rows.saturating_sub(1)));
        let first_row = start.row.max(viewport_top);
        let last_row = end.row.min(viewport_bottom);
        if first_row > last_row {
            return None;
        }

        let first_column = if first_row == start.row {
            start.column
        } else {
            0
        };
        let last_column = if last_row == end.row {
            end.column.min(usize::from(size.columns.saturating_sub(1)))
        } else {
            usize::from(size.columns.saturating_sub(1))
        };

        if self.rectangular {
            let (start, end) = self.normalized_rectangular();
            let first_row = start.row.max(viewport_top);
            let last_row = end.row.min(viewport_bottom);
            if first_row > last_row {
                return None;
            }
            let first_column = start
                .column
                .min(usize::from(size.columns.saturating_sub(1)));
            let last_column = end.column.min(usize::from(size.columns.saturating_sub(1)));

            return Some(WindowSelection::rectangular(
                SelectionCell {
                    row: u16::try_from(first_row.saturating_sub(viewport_top)).ok()?,
                    column: u16::try_from(first_column).ok()?,
                },
                SelectionCell {
                    row: u16::try_from(last_row.saturating_sub(viewport_top)).ok()?,
                    column: u16::try_from(last_column).ok()?,
                },
            ));
        }

        Some(WindowSelection::new(
            SelectionCell {
                row: u16::try_from(first_row.saturating_sub(viewport_top)).ok()?,
                column: u16::try_from(first_column).ok()?,
            },
            SelectionCell {
                row: u16::try_from(last_row.saturating_sub(viewport_top)).ok()?,
                column: u16::try_from(last_column).ok()?,
            },
        ))
    }

    const fn normalized(self) -> (SelectionSourceCell, SelectionSourceCell) {
        if self.anchor.row < self.focus.row
            || (self.anchor.row == self.focus.row && self.anchor.column <= self.focus.column)
        {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    const fn normalized_rectangular(self) -> (SelectionSourceCell, SelectionSourceCell) {
        let start_row = if self.anchor.row <= self.focus.row {
            self.anchor.row
        } else {
            self.focus.row
        };
        let end_row = if self.anchor.row >= self.focus.row {
            self.anchor.row
        } else {
            self.focus.row
        };
        let start_column = if self.anchor.column <= self.focus.column {
            self.anchor.column
        } else {
            self.focus.column
        };
        let end_column = if self.anchor.column >= self.focus.column {
            self.anchor.column
        } else {
            self.focus.column
        };

        (
            SelectionSourceCell {
                row: start_row,
                column: start_column,
            },
            SelectionSourceCell {
                row: end_row,
                column: end_column,
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelectionCell {
    row: u16,
    column: u16,
}

#[derive(Clone, Copy, Debug)]
struct WindowClick {
    cell: SelectionCell,
    time: Instant,
    count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowSelection {
    anchor: SelectionCell,
    focus: SelectionCell,
    rectangular: bool,
}

impl WindowSelection {
    const fn new(anchor: SelectionCell, focus: SelectionCell) -> Self {
        Self {
            anchor,
            focus,
            rectangular: false,
        }
    }

    const fn rectangular(anchor: SelectionCell, focus: SelectionCell) -> Self {
        Self {
            anchor,
            focus,
            rectangular: true,
        }
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

        if self.rectangular {
            let (start, end) = self.normalized_rectangular();
            return row >= start.row
                && row <= end.row
                && column >= start.column
                && column <= end.column;
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

        if self.rectangular {
            let (start, end) = self.normalized_rectangular();
            let mut lines = Vec::new();
            for row in start.row..=end.row.min(size.rows.saturating_sub(1)) {
                let mut line = String::new();
                for column in start.column..=end.column.min(size.columns.saturating_sub(1)) {
                    line.push(snapshot_character(snapshot, row, column));
                }
                trim_trailing_spaces(&mut line);
                lines.push(line);
            }
            return lines.join("\n");
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

    const fn normalized_rectangular(self) -> (SelectionCell, SelectionCell) {
        let start_row = if self.anchor.row <= self.focus.row {
            self.anchor.row
        } else {
            self.focus.row
        };
        let end_row = if self.anchor.row >= self.focus.row {
            self.anchor.row
        } else {
            self.focus.row
        };
        let start_column = if self.anchor.column <= self.focus.column {
            self.anchor.column
        } else {
            self.focus.column
        };
        let end_column = if self.anchor.column >= self.focus.column {
            self.anchor.column
        } else {
            self.focus.column
        };

        (
            SelectionCell {
                row: start_row,
                column: start_column,
            },
            SelectionCell {
                row: end_row,
                column: end_column,
            },
        )
    }
}

#[derive(Debug, Clone, Copy)]
enum WindowCopySelectionMode {
    None,
    Cell,
    Block,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowCopyDestination {
    Clipboard,
    PrimarySelection,
    ClipboardAndPrimarySelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowPasteSource {
    Clipboard,
    PrimarySelection,
}

#[derive(Debug)]
struct WindowCopyMode {
    cursor: SelectionCell,
    source_cursor: SelectionSourceCell,
    pending_jump: Option<WindowCopyPendingJump>,
    last_jump: Option<WindowCopyJump>,
    search_direction: Option<SearchDirection>,
    selection_mode: WindowCopySelectionMode,
    anchor: Option<SelectionCell>,
    source_anchor: Option<SelectionSourceCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowCopyPendingJump {
    forward: bool,
    prev_char: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowCopyJump {
    forward: bool,
    prev_char: bool,
    target: char,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowCopyWordMovement {
    Backward,
    Forward,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowCopyWordSegment {
    start: usize,
    end: usize,
    is_whitespace: bool,
}

#[derive(Debug, Default, Clone)]
struct WindowQuickSelect {
    current: usize,
    matches: Vec<WindowSearchMatch>,
    labels: Vec<String>,
    input: String,
}

impl WindowQuickSelect {
    fn current_match(&self) -> Option<WindowSearchMatch> {
        self.matches.get(self.current).copied()
    }

    fn match_for_label(&self, input: &str) -> Option<WindowSearchMatch> {
        let input = input.to_ascii_lowercase();
        self.labels
            .iter()
            .position(|label| label == &input)
            .and_then(|index| self.matches.get(index))
            .copied()
    }

    fn has_label_prefix(&self, input: &str) -> bool {
        let input = input.to_ascii_lowercase();
        self.labels.iter().any(|label| label.starts_with(&input))
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct WindowPaneSelect {
    labels: Vec<WindowPaneSelectLabel>,
    input: String,
    mode: WindowPaneSelectMode,
}

impl WindowPaneSelect {
    fn from_panes(panes: &[rssh_core::app_shell::Pane], mode: WindowPaneSelectMode) -> Self {
        Self {
            labels: pane_select_labels(panes),
            input: String::new(),
            mode,
        }
    }

    fn pane_for_label(&self, input: &str) -> Option<rssh_core::PaneId> {
        self.labels
            .iter()
            .find(|label| label.label == input)
            .map(|label| label.pane_id)
    }

    fn has_label_prefix(&self, input: &str) -> bool {
        self.labels
            .iter()
            .any(|label| label.label.starts_with(input))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum WindowPaneSelectMode {
    #[default]
    Activate,
    SwapWithActive,
    SwapWithActiveKeepFocus,
    MoveToNewTab,
    MoveToNewWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowPaneSelectLabel {
    pane_id: rssh_core::PaneId,
    label: String,
}

const PANE_SELECT_ALPHABET: &str = "asdfqwerzxcvjklmiuopghtybn";

fn pane_select_labels(panes: &[rssh_core::app_shell::Pane]) -> Vec<WindowPaneSelectLabel> {
    let alphabet = PANE_SELECT_ALPHABET.chars().collect::<Vec<_>>();
    panes
        .iter()
        .enumerate()
        .filter_map(|(index, pane)| {
            pane_select_label_for_index(index, &alphabet).map(|label| WindowPaneSelectLabel {
                pane_id: pane.id(),
                label,
            })
        })
        .collect()
}

fn pane_select_label_for_index(index: usize, alphabet: &[char]) -> Option<String> {
    if alphabet.is_empty() {
        return None;
    }

    if let Some(ch) = alphabet.get(index) {
        return Some(ch.to_string());
    }

    let two_char_index = index.saturating_sub(alphabet.len());
    let first = two_char_index / alphabet.len();
    let second = two_char_index % alphabet.len();
    Some(format!("{}{}", alphabet.get(first)?, alphabet.get(second)?))
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

fn is_word_selection_character(character: char) -> bool {
    !character.is_whitespace()
}

fn rename_tab_title_from_query(query: &str) -> Option<String> {
    query
        .trim()
        .strip_prefix("rename tab ")
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowCommand {
    ActivateLastTab,
    ActivatePaneDown,
    ActivatePaneLeft,
    ActivatePane1,
    ActivatePane2,
    ActivatePane3,
    ActivatePane4,
    ActivatePaneRight,
    ActivatePaneUp,
    EnterCopyMode,
    ClearScrollback,
    ClearScrollbackAndViewport,
    ClearSelection,
    CopyToClipboard,
    CopyToPrimarySelection,
    CopyToClipboardAndPrimarySelection,
    PasteFromClipboard,
    PasteFromPrimarySelection,
    ResetTerminal,
    EnterQuickSelect,
    EnterPaneSelect,
    EnterPaneSwap,
    EnterPaneSwapKeepFocus,
    EnterPaneMoveToNewTab,
    EnterPaneMoveToNewWindow,
    EnterSearch,
    ClosePane,
    CloseWorkspace,
    CloseTab,
    MoveTabTo1,
    MoveTabTo2,
    MoveTabTo3,
    MoveTabTo4,
    NextPane,
    PreviousPane,
    NextTab,
    NextTabNoWrap,
    PreviousTab,
    PreviousTabNoWrap,
    NewTab,
    NewWorkspace,
    RenameTab,
    RenameWorkspace,
    ResizePaneDown,
    ResizePaneLeft,
    ResizePaneRight,
    ResizePaneUp,
    RotatePanesClockwise,
    RotatePanesCounterClockwise,
    ScrollLineDown,
    ScrollLineUp,
    ScrollPageDown,
    ScrollPageUp,
    ScrollToBottom,
    ScrollToNextPrompt,
    ScrollToPreviousPrompt,
    ScrollToTop,
    SplitDown,
    SplitRight,
    TogglePaneZoom,
    UnzoomPane,
    ZoomPane,
    NextWorkspace,
    PreviousWorkspace,
}

impl WindowCommand {
    const fn label(self) -> &'static str {
        match self {
            Self::ActivateLastTab => "Activate Last Tab",
            Self::ActivatePaneDown => "Activate Pane Down",
            Self::ActivatePaneLeft => "Activate Pane Left",
            Self::ActivatePane1 => "Activate Pane 1",
            Self::ActivatePane2 => "Activate Pane 2",
            Self::ActivatePane3 => "Activate Pane 3",
            Self::ActivatePane4 => "Activate Pane 4",
            Self::ActivatePaneRight => "Activate Pane Right",
            Self::ActivatePaneUp => "Activate Pane Up",
            Self::ClosePane => "Close Pane",
            Self::CloseWorkspace => "Close Workspace",
            Self::CloseTab => "Close Tab",
            Self::EnterCopyMode => "Enter Copy Mode",
            Self::ClearScrollback => "Clear Scrollback",
            Self::ClearScrollbackAndViewport => "Clear Scrollback And Viewport",
            Self::ClearSelection => "Clear Selection",
            Self::CopyToClipboard => "Copy To Clipboard",
            Self::CopyToPrimarySelection => "Copy To Primary Selection",
            Self::CopyToClipboardAndPrimarySelection => "Copy To Clipboard And Primary Selection",
            Self::PasteFromClipboard => "Paste From Clipboard",
            Self::PasteFromPrimarySelection => "Paste From Primary Selection",
            Self::ResetTerminal => "Reset Terminal",
            Self::EnterQuickSelect => "Enter Quick Select",
            Self::EnterPaneSelect => "Enter Pane Select",
            Self::EnterPaneSwap => "Enter Pane Swap",
            Self::EnterPaneSwapKeepFocus => "Enter Pane Swap Keep Focus",
            Self::EnterPaneMoveToNewTab => "Enter Pane Move To New Tab",
            Self::EnterPaneMoveToNewWindow => "Enter Pane Move To New Window",
            Self::EnterSearch => "Enter Search",
            Self::MoveTabTo1 => "Move Tab To 1",
            Self::MoveTabTo2 => "Move Tab To 2",
            Self::MoveTabTo3 => "Move Tab To 3",
            Self::MoveTabTo4 => "Move Tab To 4",
            Self::NextPane => "Focus Next Pane",
            Self::PreviousPane => "Focus Previous Pane",
            Self::ResizePaneDown => "Resize Pane Down",
            Self::ResizePaneLeft => "Resize Pane Left",
            Self::ResizePaneRight => "Resize Pane Right",
            Self::ResizePaneUp => "Resize Pane Up",
            Self::RotatePanesClockwise => "Rotate Panes Clockwise",
            Self::RotatePanesCounterClockwise => "Rotate Panes Counter Clockwise",
            Self::ScrollLineDown => "Scroll Line Down",
            Self::ScrollLineUp => "Scroll Line Up",
            Self::ScrollPageDown => "Scroll Page Down",
            Self::ScrollPageUp => "Scroll Page Up",
            Self::ScrollToBottom => "Scroll To Bottom",
            Self::ScrollToNextPrompt => "Scroll To Next Prompt",
            Self::ScrollToPreviousPrompt => "Scroll To Previous Prompt",
            Self::ScrollToTop => "Scroll To Top",
            Self::TogglePaneZoom => "Toggle Pane Zoom",
            Self::UnzoomPane => "Unzoom Pane",
            Self::ZoomPane => "Zoom Pane",
            Self::NextTab => "Next Tab",
            Self::NextTabNoWrap => "Next Tab No Wrap",
            Self::PreviousTab => "Previous Tab",
            Self::PreviousTabNoWrap => "Previous Tab No Wrap",
            Self::NewTab => "New Tab",
            Self::NewWorkspace => "New Workspace",
            Self::RenameTab => "Rename Tab",
            Self::RenameWorkspace => "Rename Workspace",
            Self::SplitDown => "Split Pane Down",
            Self::SplitRight => "Split Pane Right",
            Self::NextWorkspace => "Next Workspace",
            Self::PreviousWorkspace => "Previous Workspace",
        }
    }

    fn palette_match_score(self, query: &str) -> Option<(usize, usize)> {
        let label = self.label().to_ascii_lowercase();
        let query = query.to_ascii_lowercase();

        if query.is_empty() {
            return Some((0, 0));
        }

        let label_bytes = label.as_bytes();
        let query_bytes = query.as_bytes();

        let mut query_index = 0usize;
        let mut start = None;
        let mut end = 0usize;

        for (position, character) in label_bytes.iter().enumerate() {
            if query_index >= query_bytes.len() {
                break;
            }
            if character.eq_ignore_ascii_case(&query_bytes[query_index]) {
                if start.is_none() {
                    start = Some(position);
                }
                end = position;
                query_index += 1;
            }
        }

        if query_index < query_bytes.len() {
            return None;
        }

        let start = start.unwrap_or_default();
        let span = (end + 1).saturating_sub(start);
        Some((span, start))
    }
}

const WINDOW_COMMANDS: &[WindowCommand] = &[
    WindowCommand::NewTab,
    WindowCommand::CloseTab,
    WindowCommand::NextTab,
    WindowCommand::PreviousTab,
    WindowCommand::NextTabNoWrap,
    WindowCommand::PreviousTabNoWrap,
    WindowCommand::MoveTabTo1,
    WindowCommand::MoveTabTo2,
    WindowCommand::MoveTabTo3,
    WindowCommand::MoveTabTo4,
    WindowCommand::ActivateLastTab,
    WindowCommand::RotatePanesClockwise,
    WindowCommand::RotatePanesCounterClockwise,
    WindowCommand::SplitRight,
    WindowCommand::SplitDown,
    WindowCommand::EnterCopyMode,
    WindowCommand::ClearScrollback,
    WindowCommand::ClearScrollbackAndViewport,
    WindowCommand::ClearSelection,
    WindowCommand::CopyToClipboard,
    WindowCommand::CopyToPrimarySelection,
    WindowCommand::CopyToClipboardAndPrimarySelection,
    WindowCommand::PasteFromClipboard,
    WindowCommand::PasteFromPrimarySelection,
    WindowCommand::ResetTerminal,
    WindowCommand::ScrollToTop,
    WindowCommand::ScrollToBottom,
    WindowCommand::ScrollPageUp,
    WindowCommand::ScrollPageDown,
    WindowCommand::ScrollLineUp,
    WindowCommand::ScrollLineDown,
    WindowCommand::ScrollToPreviousPrompt,
    WindowCommand::ScrollToNextPrompt,
    WindowCommand::EnterQuickSelect,
    WindowCommand::EnterPaneSelect,
    WindowCommand::EnterPaneSwap,
    WindowCommand::EnterPaneSwapKeepFocus,
    WindowCommand::EnterPaneMoveToNewTab,
    WindowCommand::EnterPaneMoveToNewWindow,
    WindowCommand::EnterSearch,
    WindowCommand::ClosePane,
    WindowCommand::ActivatePaneLeft,
    WindowCommand::ActivatePaneRight,
    WindowCommand::ActivatePaneUp,
    WindowCommand::ActivatePaneDown,
    WindowCommand::ActivatePane1,
    WindowCommand::ActivatePane2,
    WindowCommand::ActivatePane3,
    WindowCommand::ActivatePane4,
    WindowCommand::NextPane,
    WindowCommand::PreviousPane,
    WindowCommand::ResizePaneLeft,
    WindowCommand::ResizePaneRight,
    WindowCommand::ResizePaneUp,
    WindowCommand::ResizePaneDown,
    WindowCommand::TogglePaneZoom,
    WindowCommand::ZoomPane,
    WindowCommand::UnzoomPane,
    WindowCommand::NewWorkspace,
    WindowCommand::CloseWorkspace,
    WindowCommand::RenameTab,
    WindowCommand::RenameWorkspace,
    WindowCommand::NextWorkspace,
    WindowCommand::PreviousWorkspace,
];

#[derive(Debug, Default)]
struct WindowCommandPalette {
    query: String,
    selected: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WindowSearch {
    query: String,
    current: Option<WindowSearchMatch>,
    match_type: WindowSearchMatchType,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WindowSearchMatchType {
    #[default]
    CaseSensitive,
    CaseInsensitive,
    Regex,
}

impl WindowSearchMatchType {
    const fn next(self) -> Self {
        match self {
            Self::CaseSensitive => Self::CaseInsensitive,
            Self::CaseInsensitive => Self::Regex,
            Self::Regex => Self::CaseSensitive,
        }
    }
}

const QUICK_SELECT_PATTERNS: &[WindowQuickSelectPattern] = &[
    WindowQuickSelectPattern::capture(r"\[[^]]*\]\(([^)]+)\)", 1),
    WindowQuickSelectPattern::whole(r"(?:https?://|git@|git://|ssh://|ftp://|file://)\S+"),
    WindowQuickSelectPattern::capture(r"--- a/(\S+)", 1),
    WindowQuickSelectPattern::capture(r"\+\+\+ b/(\S+)", 1),
    WindowQuickSelectPattern::capture(r"sha256:([0-9a-f]{64})", 1),
    WindowQuickSelectPattern::whole(r"(?:[.\w\-@~]+)?(?:/+[.\w\-@]+)+"),
    WindowQuickSelectPattern::whole(r"#[0-9a-fA-F]{6}"),
    WindowQuickSelectPattern::whole(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"),
    WindowQuickSelectPattern::whole(r"Qm[0-9a-zA-Z]{44}"),
    WindowQuickSelectPattern::whole(r"[0-9a-f]{7,40}"),
    WindowQuickSelectPattern::whole(r"\b(?:\d{1,3}\.){3}\d{1,3}\b"),
    WindowQuickSelectPattern::whole(r"[A-f0-9:]+:+[A-f0-9:]+[%\w\d]+"),
    WindowQuickSelectPattern::whole(r"0x[0-9a-fA-F]+"),
    WindowQuickSelectPattern::whole(r"[0-9]{4,}"),
    WindowQuickSelectPattern::whole(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b"),
];

const QUICK_SELECT_ALPHABET: &str = "asdfqwerzxcvjklmiuopghtybn";

#[derive(Clone, Copy)]
struct WindowQuickSelectPattern {
    regex: &'static str,
    capture: Option<usize>,
}

impl WindowQuickSelectPattern {
    const fn whole(regex: &'static str) -> Self {
        Self {
            regex,
            capture: None,
        }
    }

    const fn capture(regex: &'static str, capture: usize) -> Self {
        Self {
            regex,
            capture: Some(capture),
        }
    }
}

fn quick_select_labels_for_matches(num_matches: usize) -> Vec<String> {
    let labels = quick_select_labels_for_alphabet(QUICK_SELECT_ALPHABET, num_matches);
    let mut labels_by_match = vec![String::new(); num_matches];
    for (match_index, label) in (0..num_matches).rev().zip(labels) {
        labels_by_match[match_index] = label;
    }
    labels_by_match
}

fn quick_select_labels_for_alphabet(alphabet: &str, num_matches: usize) -> Vec<String> {
    let alphabet = alphabet
        .chars()
        .map(|character| character.to_lowercase().to_string())
        .collect::<Vec<_>>();
    let mut primary = alphabet.clone();
    let mut secondary = Vec::new();

    while primary.len() + secondary.len() < num_matches {
        let Some(prefix) = primary.pop() else {
            break;
        };

        let remaining = num_matches - primary.len() - secondary.len();
        let prefixed = alphabet
            .iter()
            .take(remaining)
            .map(|character| format!("{prefix}{character}"))
            .collect::<Vec<_>>();
        secondary.splice(0..0, prefixed);
    }

    let secondary_len = secondary.len();
    primary
        .drain(..)
        .take(num_matches.saturating_sub(secondary_len))
        .chain(secondary)
        .collect()
}

fn find_window_quick_select_matches(terminal: &rssh_terminal::Terminal) -> Vec<WindowSearchMatch> {
    let cells = terminal_search_cells(terminal);

    let mut matches = QUICK_SELECT_PATTERNS
        .iter()
        .enumerate()
        .flat_map(|(pattern_index, pattern)| {
            quick_select_regex_window_search_matches(&cells, pattern_index, *pattern).into_iter()
        })
        .collect::<Vec<_>>();

    matches.sort_unstable_by_key(|candidate| {
        (
            candidate.full.source_row,
            candidate.full.start_column,
            candidate.pattern_index,
            candidate.selection.source_row,
            candidate.selection.start_column,
            std::cmp::Reverse(candidate.selection.end_source_row),
            std::cmp::Reverse(candidate.selection.end_column),
        )
    });
    let mut unique = Vec::new();
    for candidate in matches {
        if unique
            .iter()
            .any(|kept| quick_select_matches_overlap(*kept, candidate.selection))
        {
            continue;
        }
        unique.push(candidate.selection);
    }
    unique
}

#[derive(Clone, Copy)]
struct WindowQuickSelectCandidate {
    selection: WindowSearchMatch,
    full: WindowSearchMatch,
    pattern_index: usize,
}

fn quick_select_regex_window_search_matches(
    cells: &[WindowSearchCell],
    pattern_index: usize,
    pattern: WindowQuickSelectPattern,
) -> Vec<WindowQuickSelectCandidate> {
    let Ok(regex) = regex::Regex::new(pattern.regex) else {
        return Vec::new();
    };

    let mut text = String::new();
    let mut byte_to_cell_index = Vec::new();
    let mut previous_source_row = None;
    for (cell_index, cell) in cells.iter().enumerate() {
        if previous_source_row.is_some_and(|source_row| source_row != cell.source_row) {
            byte_to_cell_index.push(None);
            text.push('\n');
        }
        previous_source_row = Some(cell.source_row);

        for _ in 0..cell.character.len_utf8() {
            byte_to_cell_index.push(Some(cell_index));
        }
        text.push(cell.character);
    }

    let mut matches = Vec::new();
    match pattern.capture {
        Some(capture) => {
            for captures in regex.captures_iter(&text) {
                let Some(full) = captures.get(0) else {
                    continue;
                };
                let Some(selection) = captures.get(capture) else {
                    continue;
                };
                if selection.start() == selection.end() {
                    continue;
                }
                let Some(full) =
                    byte_range_to_window_search_match(&byte_to_cell_index, cells, full)
                else {
                    continue;
                };
                let Some(selection) =
                    byte_range_to_window_search_match(&byte_to_cell_index, cells, selection)
                else {
                    continue;
                };
                matches.push(WindowQuickSelectCandidate {
                    selection,
                    full,
                    pattern_index,
                });
            }
        }
        None => {
            for matched in regex.find_iter(&text) {
                if matched.start() == matched.end() {
                    continue;
                }
                let Some(selection) =
                    byte_range_to_window_search_match(&byte_to_cell_index, cells, matched)
                else {
                    continue;
                };
                matches.push(WindowQuickSelectCandidate {
                    selection,
                    full: selection,
                    pattern_index,
                });
            }
        }
    }
    matches
}

fn byte_range_to_window_search_match(
    byte_to_cell_index: &[Option<usize>],
    cells: &[WindowSearchCell],
    matched: regex::Match<'_>,
) -> Option<WindowSearchMatch> {
    let start_index = (*byte_to_cell_index.get(matched.start())?)?;
    let end_byte = matched.end().checked_sub(1)?;
    let end_index = (*byte_to_cell_index.get(end_byte)?)?;
    let start = cells.get(start_index)?;
    let end = cells.get(end_index)?;
    Some(WindowSearchMatch {
        source_row: start.source_row,
        start_column: start.column,
        end_source_row: end.source_row,
        end_column: end.column,
    })
}

fn quick_select_matches_overlap(left: WindowSearchMatch, right: WindowSearchMatch) -> bool {
    let left_start = (left.source_row, left.start_column);
    let left_end = (left.end_source_row, left.end_column);
    let right_start = (right.source_row, right.start_column);
    let right_end = (right.end_source_row, right.end_column);

    left_start <= right_end && right_start <= left_end
}

fn search_status(search: &WindowSearch) -> String {
    if search.query.is_empty() {
        "Search".to_owned()
    } else if search.current.is_some() {
        format!("Search: {}", search.query)
    } else {
        format!("Search: {} (no match)", search.query)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowSearchMatch {
    source_row: usize,
    start_column: u16,
    end_source_row: usize,
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

    fn viewport_selection(self, scrollback_len: usize) -> (usize, WindowSelection) {
        let (offset, start_row) = self.viewport_position(scrollback_len);
        let first_source_row = scrollback_len.saturating_sub(offset);
        let end_row = self.end_source_row.saturating_sub(first_source_row);
        let end_row = u16::try_from(end_row).unwrap_or(u16::MAX);

        (
            offset,
            WindowSelection::new(
                SelectionCell {
                    row: start_row,
                    column: self.start_column,
                },
                SelectionCell {
                    row: end_row,
                    column: self.end_column,
                },
            ),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchDirection {
    Next,
    Previous,
}

fn find_window_search_match_with_type(
    terminal: &rssh_terminal::Terminal,
    query: &str,
    current: Option<WindowSearchMatch>,
    direction: SearchDirection,
    match_type: WindowSearchMatchType,
) -> Option<WindowSearchMatch> {
    if query.is_empty() {
        return None;
    }

    let matches = window_search_matches_with_type(terminal, query, match_type);
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

fn find_window_search_page_match_with_type(
    terminal: &rssh_terminal::Terminal,
    query: &str,
    viewport_top: usize,
    viewport_rows: usize,
    direction: SearchDirection,
    match_type: WindowSearchMatchType,
) -> Option<WindowSearchMatch> {
    if query.is_empty() || viewport_rows == 0 {
        return None;
    }

    let matches = window_search_matches_with_type(terminal, query, match_type);
    if matches.is_empty() {
        return None;
    }

    let (page_start, page_end) = match direction {
        SearchDirection::Next => {
            let start = viewport_top.saturating_add(viewport_rows);
            (start, start.saturating_add(viewport_rows.saturating_sub(1)))
        }
        SearchDirection::Previous => {
            let end = viewport_top.saturating_sub(1);
            (viewport_top.saturating_sub(viewport_rows), end)
        }
    };

    matches
        .into_iter()
        .find(|candidate| candidate.source_row >= page_start && candidate.source_row <= page_end)
}

fn search_match_after(candidate: WindowSearchMatch, current: WindowSearchMatch) -> bool {
    candidate.source_row > current.source_row
        || (candidate.source_row == current.source_row
            && candidate.start_column > current.start_column)
}

fn window_search_matches_with_type(
    terminal: &rssh_terminal::Terminal,
    query: &str,
    match_type: WindowSearchMatchType,
) -> Vec<WindowSearchMatch> {
    let Some(query) = WindowSearchQuery::parse(query, match_type) else {
        return Vec::new();
    };
    let cells = terminal_search_cells(terminal);

    match query {
        WindowSearchQuery::Literal(query) => literal_window_search_matches(&cells, &query, true),
        WindowSearchQuery::CaseInsensitiveLiteral(query) => {
            literal_window_search_matches(&cells, &query, false)
        }
        WindowSearchQuery::Regex(pattern) => regex_window_search_matches(&cells, pattern),
    }
}

enum WindowSearchQuery<'a> {
    Literal(Vec<char>),
    CaseInsensitiveLiteral(Vec<char>),
    Regex(&'a str),
}

impl<'a> WindowSearchQuery<'a> {
    fn parse(query: &'a str, match_type: WindowSearchMatchType) -> Option<Self> {
        if let Some(pattern) = query.strip_prefix("regex:") {
            return (!pattern.is_empty()).then_some(Self::Regex(pattern));
        }

        let (query, force_literal) = match query.strip_prefix("literal:") {
            Some(literal) => (literal, true),
            None => (query, false),
        };
        if match_type == WindowSearchMatchType::Regex && !force_literal {
            return (!query.is_empty()).then_some(Self::Regex(query));
        }

        let query: Vec<char> = query
            .chars()
            .filter(|character| !matches!(character, '\r' | '\n'))
            .collect();
        if query.is_empty() {
            return None;
        }

        Some(match match_type {
            WindowSearchMatchType::CaseSensitive | WindowSearchMatchType::Regex => {
                Self::Literal(query)
            }
            WindowSearchMatchType::CaseInsensitive => Self::CaseInsensitiveLiteral(query),
        })
    }
}

fn literal_window_search_matches(
    cells: &[WindowSearchCell],
    query: &[char],
    case_sensitive: bool,
) -> Vec<WindowSearchMatch> {
    let query: Vec<char> = query
        .iter()
        .copied()
        .filter(|character| !matches!(character, '\r' | '\n'))
        .collect();
    if query.is_empty() {
        return Vec::new();
    }

    if cells.len() < query.len() {
        return Vec::new();
    }

    cells
        .windows(query.len())
        .filter_map(|candidate| {
            if candidate
                .iter()
                .zip(query.iter().copied())
                .all(|(cell, query_character)| {
                    if case_sensitive {
                        cell.character == query_character
                    } else {
                        cell.character.eq_ignore_ascii_case(&query_character)
                    }
                })
            {
                let start = candidate.first()?;
                let end = candidate.last()?;
                Some(WindowSearchMatch {
                    source_row: start.source_row,
                    start_column: start.column,
                    end_source_row: end.source_row,
                    end_column: end.column,
                })
            } else {
                None
            }
        })
        .collect()
}

fn regex_window_search_matches(
    cells: &[WindowSearchCell],
    pattern: &str,
) -> Vec<WindowSearchMatch> {
    let Ok(pattern) = regex::Regex::new(pattern) else {
        return Vec::new();
    };

    let mut text = String::new();
    let mut byte_to_cell_index = Vec::new();
    for (cell_index, cell) in cells.iter().enumerate() {
        for _ in 0..cell.character.len_utf8() {
            byte_to_cell_index.push(cell_index);
        }
        text.push(cell.character);
    }

    pattern
        .find_iter(&text)
        .filter_map(|matched| {
            if matched.start() == matched.end() {
                return None;
            }

            let start_index = *byte_to_cell_index.get(matched.start())?;
            let end_byte = matched.end().checked_sub(1)?;
            let end_index = *byte_to_cell_index.get(end_byte)?;
            let start = cells.get(start_index)?;
            let end = cells.get(end_index)?;
            Some(WindowSearchMatch {
                source_row: start.source_row,
                start_column: start.column,
                end_source_row: end.source_row,
                end_column: end.column,
            })
        })
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

#[derive(Clone, Copy)]
struct WindowSearchCell {
    character: char,
    source_row: usize,
    column: u16,
}

fn terminal_search_cells(terminal: &rssh_terminal::Terminal) -> Vec<WindowSearchCell> {
    terminal_search_lines(terminal)
        .into_iter()
        .enumerate()
        .flat_map(|(source_row, line)| {
            line.trim_end_matches(' ')
                .chars()
                .enumerate()
                .filter_map(move |(column, character)| {
                    Some(WindowSearchCell {
                        character,
                        source_row,
                        column: u16::try_from(column).ok()?,
                    })
                })
                .collect::<Vec<_>>()
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
        MouseProtocolMode::Utf8 => encode_utf8_window_mouse_event(event.kind, code, column, row),
        MouseProtocolMode::Urxvt => encode_urxvt_window_mouse_event(event.kind, code, column, row),
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
        code = legacy_window_mouse_release_code(code);
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

fn encode_utf8_window_mouse_event(
    kind: WindowMouseEventKind,
    mut code: u16,
    column: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if matches!(kind, WindowMouseEventKind::Up(_)) {
        code = legacy_window_mouse_release_code(code);
    }

    let mut bytes = b"\x1b[M".to_vec();
    push_utf8_mouse_value(&mut bytes, code)?;
    push_utf8_mouse_value(&mut bytes, column)?;
    push_utf8_mouse_value(&mut bytes, row)?;
    Some(bytes)
}

fn encode_urxvt_window_mouse_event(
    kind: WindowMouseEventKind,
    mut code: u16,
    column: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if matches!(kind, WindowMouseEventKind::Up(_)) {
        code = legacy_window_mouse_release_code(code);
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

const fn legacy_window_mouse_release_code(code: u16) -> u16 {
    3 + (code & !0b11)
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
    let terminal_y = position.y - f64::from(tab_bar_pixel_height());
    if terminal_y < 0.0 {
        return None;
    }

    Some((
        pixel_axis_to_cell(position.x, CELL_WIDTH)?,
        pixel_axis_to_cell(terminal_y, CELL_HEIGHT)?,
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

const fn tab_bar_pixel_height() -> u32 {
    TAB_BAR_ROWS as u32 * CELL_HEIGHT
}

fn tab_bar_tab_label(
    position: usize,
    tab_id: rssh_core::TabId,
    pane_count: usize,
    active: bool,
    title: Option<&str>,
) -> String {
    let marker = if active { "*" } else { "" };
    match title {
        Some(title) => format!(
            " {}:{}{} panes:{} {} x ",
            position + 1,
            tab_id.get(),
            marker,
            pane_count,
            title
        ),
        None => format!(
            " {}:{}{} panes:{} x ",
            position + 1,
            tab_id.get(),
            marker,
            pane_count
        ),
    }
}

const fn tab_bar_new_tab_label() -> &'static str {
    " + "
}

fn split_pane_render_rect(
    source: PaneRenderRect,
    new_pane_id: rssh_core::PaneId,
    direction: SplitDirection,
    source_size_delta: i16,
) -> Option<(PaneRenderRect, PaneRenderRect, PaneSeparator)> {
    match direction {
        SplitDirection::Right => {
            split_pane_render_rect_right(source, new_pane_id, source_size_delta)
        }
        SplitDirection::Down => split_pane_render_rect_down(source, new_pane_id, source_size_delta),
    }
}

fn split_pane_render_rect_right(
    source: PaneRenderRect,
    new_pane_id: rssh_core::PaneId,
    source_size_delta: i16,
) -> Option<(PaneRenderRect, PaneRenderRect, PaneSeparator)> {
    if source.columns < 3 || source.rows == 0 {
        return None;
    }

    let source_columns = adjusted_split_source_size(
        source.columns,
        source.columns.saturating_sub(1) / 2,
        source_size_delta,
    );
    let new_columns = source
        .columns
        .saturating_sub(source_columns)
        .saturating_sub(1);
    if source_columns == 0 || new_columns == 0 {
        return None;
    }

    let next_source = PaneRenderRect {
        columns: source_columns,
        ..source
    };
    let new_rect = PaneRenderRect {
        pane_id: new_pane_id,
        row: source.row,
        column: source
            .column
            .saturating_add(source_columns)
            .saturating_add(1),
        rows: source.rows,
        columns: new_columns,
    };
    let separator = PaneSeparator {
        row: source.row,
        column: source.column.saturating_add(source_columns),
        rows: source.rows,
        columns: 1,
        source_pane: source.pane_id,
        new_pane: new_pane_id,
    };

    Some((next_source, new_rect, separator))
}

fn split_pane_render_rect_down(
    source: PaneRenderRect,
    new_pane_id: rssh_core::PaneId,
    source_size_delta: i16,
) -> Option<(PaneRenderRect, PaneRenderRect, PaneSeparator)> {
    if source.rows < 3 || source.columns == 0 {
        return None;
    }

    let source_rows = adjusted_split_source_size(
        source.rows,
        source.rows.saturating_sub(1) / 2,
        source_size_delta,
    );
    let new_rows = source.rows.saturating_sub(source_rows).saturating_sub(1);
    if source_rows == 0 || new_rows == 0 {
        return None;
    }

    let next_source = PaneRenderRect {
        rows: source_rows,
        ..source
    };
    let new_rect = PaneRenderRect {
        pane_id: new_pane_id,
        row: source.row.saturating_add(source_rows).saturating_add(1),
        column: source.column,
        rows: new_rows,
        columns: source.columns,
    };
    let separator = PaneSeparator {
        row: source.row.saturating_add(source_rows),
        column: source.column,
        rows: 1,
        columns: source.columns,
        source_pane: source.pane_id,
        new_pane: new_pane_id,
    };

    Some((next_source, new_rect, separator))
}

fn adjusted_split_source_size(total_cells: u16, default_source_cells: u16, delta: i16) -> u16 {
    let max_source_cells = total_cells.saturating_sub(2).max(1);
    let adjusted = i32::from(default_source_cells) + i32::from(delta);
    u16::try_from(adjusted.clamp(1, i32::from(max_source_cells))).unwrap_or(max_source_cells)
}

fn pane_mouse_cell(rect: PaneRenderRect, row: u16, column: u16) -> Option<PaneMouseCell> {
    if row < rect.row
        || row >= rect.row.saturating_add(rect.rows)
        || column < rect.column
        || column >= rect.column.saturating_add(rect.columns)
    {
        return None;
    }

    Some(PaneMouseCell {
        pane_id: rect.pane_id,
        row: row.saturating_sub(rect.row),
        column: column.saturating_sub(rect.column),
    })
}

fn split_resize_drag(
    separator: PaneSeparator,
    row: u16,
    column: u16,
) -> Option<PaneSplitResizeDrag> {
    if row < separator.row
        || row >= separator.row.saturating_add(separator.rows)
        || column < separator.column
        || column >= separator.column.saturating_add(separator.columns)
    {
        return None;
    }

    let direction = if separator.columns == 1 {
        SplitDirection::Right
    } else {
        SplitDirection::Down
    };
    Some(PaneSplitResizeDrag {
        pane_id: separator.new_pane,
        direction,
        last_row: row.saturating_sub(TAB_BAR_ROWS),
        last_column: column,
    })
}

fn write_tab_bar_segment(
    cells: &mut [RenderCell],
    column: &mut u16,
    text: &str,
    foreground: Color,
    background: Color,
    bold: bool,
) {
    for ch in text.chars() {
        let index = usize::from(*column);
        let Some(cell) = cells.get_mut(index) else {
            return;
        };

        *cell = tab_bar_render_cell(*column, ch, foreground, background, bold);
        *column = column.saturating_add(1);
    }
}

fn tab_bar_render_cell(
    column: u16,
    ch: char,
    foreground: Color,
    background: Color,
    bold: bool,
) -> RenderCell {
    ui_render_cell(0, column, ch, foreground, background, bold)
}

fn ui_render_cell(
    row: u16,
    column: u16,
    ch: char,
    foreground: Color,
    background: Color,
    bold: bool,
) -> RenderCell {
    RenderCell {
        row,
        column,
        ch,
        foreground,
        background,
        underline_color: Color::Default,
        underline_style: UnderlineStyle::None,
        bold,
        faint: false,
        italic: false,
        blink: false,
        underline: false,
        double_underline: false,
        conceal: false,
        strikethrough: false,
        overline: false,
        vertical_align: rssh_terminal::VerticalAlign::Baseline,
        inverse: false,
        hyperlink: None,
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

fn kitty_window_modifier(modifiers: ModifiersState) -> Option<u8> {
    let shift = modifiers.shift_key();
    let alt = modifiers.alt_key();
    let control = modifiers.control_key();
    let super_key = modifiers.super_key();
    if !(shift || alt || control || super_key) {
        return None;
    }

    Some(1 + u8::from(shift) + u8::from(alt) * 2 + u8::from(control) * 4 + u8::from(super_key) * 8)
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

fn encode_osc52_clipboard_response(selection: &str, text: &str) -> Vec<u8> {
    format!(
        "\x1b]52;{};{}\x07",
        selection,
        STANDARD.encode(text.as_bytes())
    )
    .into_bytes()
}

fn pane_progress_from_terminal_progress(progress: TerminalProgress) -> PaneProgress {
    match progress {
        TerminalProgress::None => PaneProgress::None,
        TerminalProgress::Percentage(value) => PaneProgress::Percentage(value),
        TerminalProgress::Error(value) => PaneProgress::Error(value),
        TerminalProgress::Indeterminate => PaneProgress::Indeterminate,
    }
}

fn window_paste_source_for_shortcut(
    key: &Key,
    modifiers: ModifiersState,
) -> Option<WindowPasteSource> {
    let ctrl_v = modifiers.control_key()
        && !modifiers.alt_key()
        && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("v"));
    if ctrl_v {
        return Some(WindowPasteSource::Clipboard);
    }

    if modifiers == ModifiersState::SHIFT && matches!(key, Key::Named(NamedKey::Insert)) {
        return Some(WindowPasteSource::PrimarySelection);
    }

    None
}

fn window_copy_destination_for_shortcut(
    key: &Key,
    modifiers: ModifiersState,
) -> Option<WindowCopyDestination> {
    let ctrl_shift_c = modifiers.control_key()
        && modifiers.shift_key()
        && !modifiers.alt_key()
        && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("c"));
    if ctrl_shift_c {
        return Some(WindowCopyDestination::Clipboard);
    }

    if modifiers == ModifiersState::CONTROL && matches!(key, Key::Named(NamedKey::Insert)) {
        return Some(WindowCopyDestination::PrimarySelection);
    }

    None
}

fn window_copy_mode_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers.control_key()
        && modifiers.shift_key()
        && !modifiers.alt_key()
        && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("x"))
}

fn window_quick_select_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers.control_key()
        && modifiers.shift_key()
        && !modifiers.alt_key()
        && matches!(key, Key::Named(NamedKey::Space))
}

fn window_search_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers.control_key()
        && !modifiers.alt_key()
        && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("f"))
}

fn window_clear_scrollback_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers.control_key()
        && modifiers.shift_key()
        && !modifiers.alt_key()
        && matches!(key.as_ref(), Key::Character(character) if character.eq_ignore_ascii_case("k"))
}

fn window_hyperlink_activation_modifiers(modifiers: ModifiersState) -> bool {
    modifiers.control_key() && !modifiers.shift_key() && !modifiers.alt_key()
}

fn read_window_clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn write_window_clipboard_text(text: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
        .is_ok()
}

fn read_window_primary_selection_text() -> Option<String> {
    None
}

fn write_window_primary_selection_text(_text: &str) -> bool {
    false
}

fn show_window_notification(_notification: &TerminalNotification) -> bool {
    false
}

fn dispatch_window_open_uri(_event: &NativeWindowOpenUri) -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn dispatch_window_bell(_bell: &NativeWindowBell) -> bool {
    false
}

fn dispatch_window_focus_change(_change: &NativeWindowFocusChange) -> bool {
    false
}

fn dispatch_window_resize(_resize: &NativeWindowResize) -> bool {
    false
}

fn dispatch_window_user_var_change(_change: &NativeWindowUserVarChange) -> bool {
    false
}

#[cfg(target_os = "windows")]
fn open_window_hyperlink(url: &str) -> bool {
    Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .is_ok()
}

#[cfg(target_os = "macos")]
fn open_window_hyperlink(url: &str) -> bool {
    Command::new("open").arg(url).spawn().is_ok()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_window_hyperlink(url: &str) -> bool {
    Command::new("xdg-open").arg(url).spawn().is_ok()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
fn open_window_hyperlink(_url: &str) -> bool {
    false
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
    let total_rows = height / CELL_HEIGHT;
    let terminal_rows = total_rows.saturating_sub(u32::from(TAB_BAR_ROWS));
    let rows = u16::try_from(terminal_rows.clamp(1, u32::from(u16::MAX)))
        .expect("row count is clamped to u16");

    TerminalSize::new(columns, rows)
}

impl ApplicationHandler<WindowUserEvent> for NativeWindowManager {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.materialize_startup_app(event_loop) {
            eprintln!("window error: {error}");
            event_loop.exit();
            return;
        }

        if let Err(error) = self.materialize_pending_apps(event_loop) {
            eprintln!("window error: {error}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: WindowUserEvent) {
        let Some(window_id) = self.window_id_for_app_window(event.window_id()) else {
            return;
        };

        let Some(mut app) = self.windows.remove(&window_id) else {
            return;
        };
        let mut close_window = false;

        match event {
            WindowUserEvent::Output { pane_id, bytes, .. } => {
                if let Err(error) = app.handle_pane_pty_output(pane_id, &bytes) {
                    eprintln!("PTY write error: {error}");
                    close_window = true;
                } else if pane_id == app.app_shell.active_pane_id() {
                    if let Some(window) = &app.window {
                        window.request_redraw();
                    }
                }
            }
            WindowUserEvent::Exited { pane_id, .. } => {
                if let Some(mut runtime) = app.pane_runtimes.remove(&pane_id) {
                    runtime.close();
                }
                if pane_id == app.app_shell.active_pane_id() {
                    app.stop_active_runtime();
                    close_window = true;
                }
            }
            WindowUserEvent::ReadError { pane_id, error, .. } => {
                if pane_id == app.app_shell.active_pane_id() {
                    eprintln!("PTY read error: {error}");
                    app.stop_active_runtime();
                    close_window = true;
                } else if let Some(mut runtime) = app.pane_runtimes.remove(&pane_id) {
                    runtime.close();
                }
            }
        }

        self.collect_pending_window_apps_from_app(&mut app);
        if close_window {
            self.last_metrics = Some(app.metrics_snapshot());
            drop(app);
            if self.windows.is_empty() && self.pending_apps.is_empty() {
                event_loop.exit();
                return;
            }
        } else {
            self.windows.insert(window_id, app);
        }

        if let Err(error) = self.materialize_pending_apps(event_loop) {
            eprintln!("window error: {error}");
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            if self.close_window(window_id) {
                event_loop.exit();
            }
            return;
        }

        let Some(mut app) = self.windows.remove(&window_id) else {
            return;
        };
        app.window_event(event_loop, window_id, event);
        self.collect_pending_window_apps_from_app(&mut app);
        if app.take_window_close_request() {
            self.last_metrics = Some(app.metrics_snapshot());
            drop(app);
            if self.windows.is_empty() && self.startup_app.is_none() && self.pending_apps.is_empty()
            {
                event_loop.exit();
                return;
            }
        } else {
            self.windows.insert(window_id, app);
        }

        if let Err(error) = self.materialize_pending_apps(event_loop) {
            eprintln!("window error: {error}");
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.materialize_pending_apps(event_loop) {
            eprintln!("window error: {error}");
            event_loop.exit();
            return;
        }

        for app in self.windows.values() {
            if let Some(window) = &app.window {
                window.request_redraw();
            }
        }
    }
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
            WindowUserEvent::Output { pane_id, bytes, .. } => {
                if let Err(error) = self.handle_pane_pty_output(pane_id, &bytes) {
                    eprintln!("PTY write error: {error}");
                    event_loop.exit();
                    return;
                }

                if pane_id == self.app_shell.active_pane_id() && self.window.is_some() {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowUserEvent::Exited { pane_id, .. } => {
                if let Some(mut runtime) = self.pane_runtimes.remove(&pane_id) {
                    runtime.close();
                }
                if pane_id == self.app_shell.active_pane_id() {
                    self.stop_active_runtime();
                    event_loop.exit();
                }
            }
            WindowUserEvent::ReadError { pane_id, error, .. } => {
                if pane_id == self.app_shell.active_pane_id() {
                    eprintln!("PTY read error: {error}");
                    self.stop_active_runtime();
                    event_loop.exit();
                } else if let Some(mut runtime) = self.pane_runtimes.remove(&pane_id) {
                    runtime.close();
                }
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
                    return;
                }
                if self.take_window_close_request() {
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

fn app_shell_from_pty_command(startup_command: &PtyCommand) -> AppShell {
    let mut launch = PaneLaunch::local(startup_command.program())
        .with_args(startup_command.args().iter().cloned());
    if let Some(cwd) = startup_command.cwd() {
        launch = launch.with_cwd(cwd.to_string_lossy());
    }
    AppShell::new(launch)
}

fn pty_command_from_pane_launch(launch: &PaneLaunch) -> PtyCommand {
    let mut command = PtyCommand::new(launch.program()).with_args(launch.args().iter());
    if let Some(cwd) = launch.cwd().and_then(pane_launch_cwd_to_path) {
        command = command.with_cwd(cwd);
    }
    command
}

fn pane_launch_cwd_to_path(cwd: &str) -> Option<PathBuf> {
    if cwd.is_empty() {
        return None;
    }

    if let Some(rest) = cwd.strip_prefix("file://") {
        let path_start = rest.find('/')?;
        let mut path = percent_decode_path_component(&rest[path_start..]);
        if cfg!(windows)
            && path.len() >= 3
            && path.as_bytes().first() == Some(&b'/')
            && path.as_bytes().get(2) == Some(&b':')
        {
            path.remove(0);
        }
        return Some(PathBuf::from(path));
    }

    Some(PathBuf::from(percent_decode_path_component(cwd)))
}

fn percent_decode_path_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push(high << 4 | low);
                index += 3;
                continue;
            }
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use winit::dpi::{PhysicalPosition, PhysicalSize};
    use winit::event::{ElementState, MouseButton, MouseScrollDelta};
    use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};

    use rssh_renderer::SCROLLBAR_THUMB_COLOR;

    use crate::{
        cli::Osc52Policy,
        terminal_modes::{MouseInputMode, MouseProtocolMode, MouseReportingMode},
        terminal_runtime::TerminalNotification,
    };

    use super::{
        AppAction, AppShellError, CELL_WIDTH, DamageRegion, FRAME_HEIGHT, FRAME_WIDTH,
        FrameRenderMode, KittyKeyEventKind, NativeWindowApp, NativeWindowBell,
        NativeWindowFocusChange, NativeWindowManager, NativeWindowOpenUri, NativeWindowResize,
        NativeWindowUserVarChange, PaneLaunch, SearchDirection, SelectionCell, TAB_BAR_ROWS,
        TERMINAL_COLUMNS, WindowCommand, WindowCopyDestination, WindowMouseEvent,
        WindowMouseEventKind, WindowPaneSelectMode, WindowPasteSource, WindowSearchMatchType,
        WindowSelection, demo_snapshot, encode_window_focus_event, encode_window_key,
        encode_window_key_with_kitty, encode_window_key_with_kitty_event,
        encode_window_mouse_event, encode_window_paste, pty_command_from_pane_launch,
        tab_bar_pixel_height, tab_bar_tab_label, terminal_size_from_window_pixels,
        window_clear_scrollback_shortcut, window_copy_destination_for_shortcut,
        window_copy_mode_shortcut, window_paste_source_for_shortcut, window_quick_select_shortcut,
        window_search_shortcut,
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
    fn encodes_window_kitty_disambiguated_ascii_keys_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("i".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                None,
                ModifiersState::CONTROL,
                false,
                false,
                1,
                0
            ),
            b"\x1b[105;5u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("I".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                None,
                ModifiersState::CONTROL | ModifiersState::SHIFT,
                false,
                false,
                1,
                0
            ),
            b"\x1b[105;6u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("i".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                Some("i"),
                ModifiersState::ALT,
                false,
                false,
                1,
                0
            ),
            b"\x1b[105;3u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("i".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                None,
                ModifiersState::SUPER,
                false,
                false,
                8,
                0
            ),
            b"\x1b[105;9u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("I".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                None,
                ModifiersState::SUPER | ModifiersState::SHIFT,
                false,
                false,
                8,
                0
            ),
            b"\x1b[105;10u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("i".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                None,
                ModifiersState::CONTROL,
                false,
                false,
                0,
                0
            ),
            b"\t"
        );
    }

    #[test]
    fn encodes_window_kitty_report_all_ascii_and_basic_functional_keys_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("a".into()),
                PhysicalKey::Code(WinitKeyCode::KeyA),
                Some("a"),
                ModifiersState::empty(),
                false,
                false,
                8,
                0
            ),
            b"\x1b[97u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("A".into()),
                PhysicalKey::Code(WinitKeyCode::KeyA),
                Some("A"),
                ModifiersState::SHIFT,
                false,
                false,
                8,
                0
            ),
            b"\x1b[97;2u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("i".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                None,
                ModifiersState::CONTROL,
                false,
                false,
                8,
                0
            ),
            b"\x1b[105;5u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Enter),
                PhysicalKey::Code(WinitKeyCode::Enter),
                Some("\r"),
                ModifiersState::empty(),
                false,
                false,
                8,
                0
            ),
            b"\x1b[13u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Tab),
                PhysicalKey::Code(WinitKeyCode::Tab),
                Some("\t"),
                ModifiersState::empty(),
                false,
                false,
                8,
                0
            ),
            b"\x1b[9u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Backspace),
                PhysicalKey::Code(WinitKeyCode::Backspace),
                None,
                ModifiersState::empty(),
                false,
                false,
                8,
                0
            ),
            b"\x1b[127u"
        );
    }

    #[test]
    fn encodes_window_kitty_associated_text_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("A".into()),
                PhysicalKey::Code(WinitKeyCode::KeyA),
                Some("A"),
                ModifiersState::SHIFT,
                false,
                false,
                24,
                0
            ),
            b"\x1b[97;2;65u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("a".into()),
                PhysicalKey::Code(WinitKeyCode::KeyA),
                Some("a"),
                ModifiersState::empty(),
                false,
                false,
                24,
                0
            ),
            b"\x1b[97;;97u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("e\u{301}".into()),
                PhysicalKey::Code(WinitKeyCode::KeyE),
                Some("e\u{301}"),
                ModifiersState::empty(),
                false,
                false,
                24,
                0
            ),
            b"\x1b[101;;101:769u"
        );
        assert_eq!(
            encode_window_key_with_kitty_event(
                &Key::Character("a".into()),
                PhysicalKey::Code(WinitKeyCode::KeyA),
                Some("a"),
                ModifiersState::empty(),
                false,
                false,
                26,
                0,
                KittyKeyEventKind::Repeat
            ),
            b"\x1b[97;1:2;97u"
        );
        assert_eq!(
            encode_window_key_with_kitty_event(
                &Key::Character("a".into()),
                PhysicalKey::Code(WinitKeyCode::KeyA),
                Some("a"),
                ModifiersState::empty(),
                false,
                false,
                26,
                0,
                KittyKeyEventKind::Release
            ),
            b"\x1b[97;1:3u"
        );
    }

    #[test]
    fn encodes_window_kitty_alternate_keys_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("+".into()),
                PhysicalKey::Code(WinitKeyCode::Equal),
                Some("+"),
                ModifiersState::CONTROL | ModifiersState::SHIFT,
                false,
                false,
                5,
                0
            ),
            b"\x1b[61:43:61;6u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("\u{441}".into()),
                PhysicalKey::Code(WinitKeyCode::KeyC),
                Some("\u{441}"),
                ModifiersState::CONTROL,
                false,
                false,
                5,
                0
            ),
            b"\x1b[1089::99;5u"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn encodes_window_kitty_canonical_functional_keys_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::F1),
                PhysicalKey::Code(WinitKeyCode::F1),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[P"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Escape),
                PhysicalKey::Code(WinitKeyCode::Escape),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[27u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::ArrowUp),
                PhysicalKey::Code(WinitKeyCode::ArrowUp),
                None,
                ModifiersState::empty(),
                true,
                false,
                1,
                0
            ),
            b"\x1b[A"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::F13),
                PhysicalKey::Code(WinitKeyCode::F13),
                None,
                ModifiersState::empty(),
                false,
                false,
                8,
                0
            ),
            b"\x1b[57376u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::CapsLock),
                PhysicalKey::Code(WinitKeyCode::CapsLock),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57358u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::ScrollLock),
                PhysicalKey::Code(WinitKeyCode::ScrollLock),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57359u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::NumLock),
                PhysicalKey::Code(WinitKeyCode::NumLock),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57360u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::PrintScreen),
                PhysicalKey::Code(WinitKeyCode::PrintScreen),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57361u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Pause),
                PhysicalKey::Code(WinitKeyCode::Pause),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57362u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::ContextMenu),
                PhysicalKey::Code(WinitKeyCode::ContextMenu),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57363u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::MediaPlayPause),
                PhysicalKey::Code(WinitKeyCode::MediaPlayPause),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57430u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::MediaStop),
                PhysicalKey::Code(WinitKeyCode::MediaStop),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57432u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::MediaTrackNext),
                PhysicalKey::Code(WinitKeyCode::MediaTrackNext),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57435u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::AudioVolumeMute),
                PhysicalKey::Code(WinitKeyCode::AudioVolumeMute),
                None,
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57440u"
        );
    }

    #[test]
    fn encodes_window_kitty_keypad_keys_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Enter),
                PhysicalKey::Code(WinitKeyCode::NumpadEnter),
                Some("\r"),
                ModifiersState::empty(),
                false,
                false,
                1,
                0
            ),
            b"\x1b[57414u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("5".into()),
                PhysicalKey::Code(WinitKeyCode::Numpad5),
                Some("5"),
                ModifiersState::empty(),
                false,
                false,
                8,
                0
            ),
            b"\x1b[57404u"
        );
    }

    #[test]
    fn encodes_window_kitty_modifier_keys_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Shift),
                PhysicalKey::Code(WinitKeyCode::ShiftLeft),
                None,
                ModifiersState::SHIFT,
                false,
                false,
                1,
                0
            ),
            b"\x1b[57441;2u"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Super),
                PhysicalKey::Code(WinitKeyCode::SuperLeft),
                None,
                ModifiersState::SUPER,
                false,
                false,
                1,
                0
            ),
            b"\x1b[57444;9u"
        );
        assert_eq!(
            encode_window_key_with_kitty_event(
                &Key::Named(NamedKey::Control),
                PhysicalKey::Code(WinitKeyCode::ControlRight),
                None,
                ModifiersState::empty(),
                false,
                false,
                2,
                0,
                KittyKeyEventKind::Release
            ),
            b"\x1b[57448;1:3u"
        );
    }

    #[test]
    fn encodes_window_kitty_event_types_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty_event(
                &Key::Named(NamedKey::ArrowUp),
                PhysicalKey::Code(WinitKeyCode::ArrowUp),
                None,
                ModifiersState::empty(),
                false,
                false,
                2,
                0,
                KittyKeyEventKind::Repeat
            ),
            b"\x1b[1;1:2A"
        );
        assert_eq!(
            encode_window_key_with_kitty_event(
                &Key::Character("i".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                None,
                ModifiersState::CONTROL,
                false,
                false,
                3,
                0,
                KittyKeyEventKind::Release
            ),
            b"\x1b[105;5:3u"
        );
        assert_eq!(
            encode_window_key_with_kitty_event(
                &Key::Named(NamedKey::ArrowUp),
                PhysicalKey::Code(WinitKeyCode::ArrowUp),
                None,
                ModifiersState::SUPER,
                false,
                false,
                2,
                0,
                KittyKeyEventKind::Repeat
            ),
            b"\x1b[1;9:2A"
        );
        assert_eq!(
            encode_window_key_with_kitty_event(
                &Key::Character("a".into()),
                PhysicalKey::Code(WinitKeyCode::KeyA),
                Some("a"),
                ModifiersState::empty(),
                false,
                false,
                10,
                0,
                KittyKeyEventKind::Release
            ),
            b"\x1b[97;1:3u"
        );
        assert!(
            encode_window_key_with_kitty_event(
                &Key::Named(NamedKey::Enter),
                PhysicalKey::Code(WinitKeyCode::Enter),
                Some("\r"),
                ModifiersState::empty(),
                false,
                false,
                3,
                0,
                KittyKeyEventKind::Release
            )
            .is_empty()
        );
    }

    #[test]
    fn encodes_window_xterm_modify_other_keys_when_enabled() {
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Named(NamedKey::Enter),
                PhysicalKey::Code(WinitKeyCode::Enter),
                Some("\r"),
                ModifiersState::CONTROL,
                false,
                false,
                0,
                2
            ),
            b"\x1b[27;5;13~"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character("I".into()),
                PhysicalKey::Code(WinitKeyCode::KeyI),
                None,
                ModifiersState::CONTROL | ModifiersState::SHIFT,
                false,
                false,
                0,
                2
            ),
            b"\x1b[27;6;73~"
        );
        assert_eq!(
            encode_window_key_with_kitty(
                &Key::Character(".".into()),
                PhysicalKey::Code(WinitKeyCode::Period),
                Some("."),
                ModifiersState::ALT,
                false,
                false,
                0,
                2
            ),
            b"\x1b[27;3;46~"
        );
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
        assert!(
            window_paste_source_for_shortcut(&Key::Character("v".into()), ModifiersState::CONTROL)
                .is_some()
        );
        assert!(
            window_paste_source_for_shortcut(
                &Key::Character("V".into()),
                ModifiersState::CONTROL | ModifiersState::SHIFT
            )
            .is_some()
        );
        assert!(
            window_paste_source_for_shortcut(&Key::Named(NamedKey::Insert), ModifiersState::SHIFT)
                .is_some()
        );
        assert!(
            window_paste_source_for_shortcut(&Key::Character("v".into()), ModifiersState::empty())
                .is_none()
        );
    }

    #[test]
    fn maps_window_paste_shortcuts_to_wezterm_sources() {
        assert_eq!(
            window_paste_source_for_shortcut(&Key::Character("v".into()), ModifiersState::CONTROL),
            Some(WindowPasteSource::Clipboard)
        );
        assert_eq!(
            window_paste_source_for_shortcut(
                &Key::Character("V".into()),
                ModifiersState::CONTROL | ModifiersState::SHIFT
            ),
            Some(WindowPasteSource::Clipboard)
        );
        assert_eq!(
            window_paste_source_for_shortcut(&Key::Named(NamedKey::Insert), ModifiersState::SHIFT),
            Some(WindowPasteSource::PrimarySelection)
        );
        assert_eq!(
            window_paste_source_for_shortcut(&Key::Character("v".into()), ModifiersState::empty()),
            None
        );
    }

    #[test]
    fn window_pane_launch_command_uses_plain_current_working_dir() {
        let launch = PaneLaunch::local("powershell")
            .with_args(["-NoProfile"])
            .with_cwd("/tmp/project");

        let command = pty_command_from_pane_launch(&launch);

        assert_eq!(command.program(), "powershell");
        assert_eq!(command.args(), ["-NoProfile"]);
        assert_eq!(command.cwd(), Some(std::path::Path::new("/tmp/project")));
    }

    #[test]
    fn window_pane_launch_command_decodes_file_uri_current_working_dir() {
        let launch = PaneLaunch::local("powershell").with_cwd("file://host/home/ops%20team");

        let command = pty_command_from_pane_launch(&launch);

        assert_eq!(command.cwd(), Some(std::path::Path::new("/home/ops team")));
    }

    #[test]
    fn recognizes_window_copy_shortcuts() {
        assert!(
            window_copy_destination_for_shortcut(
                &Key::Character("c".into()),
                ModifiersState::CONTROL | ModifiersState::SHIFT
            )
            .is_some()
        );
        assert!(
            window_copy_destination_for_shortcut(
                &Key::Character("C".into()),
                ModifiersState::CONTROL | ModifiersState::SHIFT
            )
            .is_some()
        );
        assert!(
            window_copy_destination_for_shortcut(
                &Key::Named(NamedKey::Insert),
                ModifiersState::CONTROL
            )
            .is_some()
        );
        assert!(
            window_copy_destination_for_shortcut(
                &Key::Character("c".into()),
                ModifiersState::CONTROL
            )
            .is_none()
        );
    }

    #[test]
    fn maps_window_copy_shortcuts_to_wezterm_destinations() {
        assert_eq!(
            window_copy_destination_for_shortcut(
                &Key::Character("c".into()),
                ModifiersState::CONTROL | ModifiersState::SHIFT
            ),
            Some(WindowCopyDestination::Clipboard)
        );
        assert_eq!(
            window_copy_destination_for_shortcut(
                &Key::Character("C".into()),
                ModifiersState::CONTROL | ModifiersState::SHIFT
            ),
            Some(WindowCopyDestination::Clipboard)
        );
        assert_eq!(
            window_copy_destination_for_shortcut(
                &Key::Named(NamedKey::Insert),
                ModifiersState::CONTROL
            ),
            Some(WindowCopyDestination::PrimarySelection)
        );
        assert_eq!(
            window_copy_destination_for_shortcut(
                &Key::Character("c".into()),
                ModifiersState::CONTROL
            ),
            None
        );
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
    fn window_app_dispatches_focus_changed_for_active_pane() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&changes);
        let mut app = NativeWindowApp::new(None);
        app.focus_change_handler = Box::new(move |change| {
            recorded.lock().unwrap().push(*change);
            true
        });
        let active_pane = app.app_shell.active_pane_id();

        app.handle_focus_changed(true).unwrap();
        app.handle_focus_changed(false).unwrap();

        assert_eq!(
            changes.lock().unwrap().as_slice(),
            [
                NativeWindowFocusChange {
                    pane: active_pane,
                    focused: true,
                },
                NativeWindowFocusChange {
                    pane: active_pane,
                    focused: false,
                },
            ]
        );
    }

    #[test]
    fn window_app_dispatches_resize_for_active_pane() {
        let resizes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&resizes);
        let mut app = NativeWindowApp::new(None);
        app.resize_handler = Box::new(move |resize| {
            recorded.lock().unwrap().push(*resize);
            true
        });
        let active_pane = app.app_shell.active_pane_id();

        app.handle_window_resize(PhysicalSize::new(96, 80)).unwrap();

        assert_eq!(
            resizes.lock().unwrap().as_slice(),
            [NativeWindowResize {
                pane: active_pane,
                pixel_width: 96,
                pixel_height: 80,
                terminal_size: rssh_core::TerminalSize::new(12, 4),
            }]
        );
        assert_eq!(
            app.runtime.terminal().grid().size(),
            rssh_core::TerminalSize::new(12, 4)
        );
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
    fn encodes_window_mouse_events_as_utf8_sequences_when_enabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Utf8);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 95,
                    row: 96,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[M \xc2\x80\xc2\x81"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Up(MouseButton::Left),
                    column: 95,
                    row: 96,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[M#\xc2\x80\xc2\x81"
        );
    }

    #[test]
    fn encodes_window_mouse_events_as_urxvt_sequences_when_enabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Urxvt);

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
            b"\x1b[32;1;1M"
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
            b"\x1b[35;1;1M"
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
    fn window_app_updates_live_snapshot_from_terminal_damage() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"live").unwrap();

        let metrics = app.metrics_snapshot();
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('l'));
        assert_eq!(snapshot_char(&app.snapshot, 0, 3), Some('e'));
        assert_eq!(metrics.snapshot_damage_updates, 1);
        assert_eq!(metrics.snapshot_rebuilds, 0);
    }

    #[test]
    fn window_app_renders_pending_terminal_damage_to_framebuffer() {
        let mut app = NativeWindowApp::new(None);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        app.handle_pty_output(b"live").unwrap();

        assert_eq!(
            app.pending_frame_damage,
            vec![DamageRegion::new(0, 0, 4, 1)]
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Damage);
        assert!(app.pending_frame_damage.is_empty());

        let metrics = app.metrics_snapshot();
        assert_eq!(metrics.full_render_frames, 1);
        assert_eq!(metrics.dirty_render_frames, 1);
    }

    #[test]
    fn window_app_renders_scrollback_scrollbar_to_framebuffer() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee").unwrap();
        app.scroll_viewport_lines(99);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                FRAME_WIDTH as usize - 1,
                tab_bar_pixel_height() as usize,
            ),
            SCROLLBAR_THUMB_COLOR
        );
    }

    #[test]
    fn window_app_clicking_scrollback_scrollbar_jumps_viewport() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee").unwrap();

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(FRAME_WIDTH - 1),
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();

        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        assert_eq!(app.scrollback_offset, 3);
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));
        assert!(app.selection.is_none());
        assert!(!app.selecting);
    }

    #[test]
    fn window_app_dragging_scrollback_scrollbar_updates_viewport() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee").unwrap();

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(FRAME_WIDTH - 1),
            f64::from(FRAME_HEIGHT - 1),
        ))
        .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(FRAME_WIDTH - 1),
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();

        assert_eq!(app.scrollback_offset, 3);
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));
        assert!(
            app.handle_mouse_input(ElementState::Released, MouseButton::Left)
                .unwrap()
        );
        assert!(!app.selecting);
    }

    #[test]
    fn window_app_collects_pty_processing_metrics() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"live").unwrap();

        let metrics = app.metrics_snapshot();
        assert_eq!(metrics.pty_chunks, 1);
        assert_eq!(metrics.pty_bytes, 4);
        assert_eq!(metrics.damage_regions, 1);
        assert_eq!(metrics.damaged_cells, 4);
        assert!(metrics.first_pty_byte_ms.is_some());
        assert!(metrics.first_rendered_cell_ms.is_some());
    }

    #[test]
    fn window_app_collects_bell_metrics() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"\x07live\x07").unwrap();

        let metrics = app.metrics_snapshot();
        assert_eq!(metrics.bells, 2);
        assert!(app.metrics_report().contains("bells=2"));
    }

    #[test]
    fn window_app_dispatches_bells_from_active_and_inactive_panes() {
        let bells = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&bells);
        let mut app = NativeWindowApp::new(None);
        app.bell_handler = Box::new(move |bell| {
            recorded.lock().unwrap().push(*bell);
            true
        });
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        let active_pane = app.app_shell.active_pane_id();

        app.handle_pty_output(b"\x07active\x07").unwrap();
        app.handle_pane_pty_output(rssh_core::PaneId::new(1), b"\x07inactive")
            .unwrap();

        assert_eq!(
            bells.lock().unwrap().as_slice(),
            [
                NativeWindowBell { pane: active_pane },
                NativeWindowBell { pane: active_pane },
                NativeWindowBell {
                    pane: rssh_core::PaneId::new(1),
                },
            ]
        );
    }

    #[test]
    fn window_metrics_json_report_is_machine_readable() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"live").unwrap();

        let json = app.metrics_json_report().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["pty_chunks"], 1);
        assert_eq!(value["pty_bytes"], 4);
        assert_eq!(value["damage_regions"], 1);
        assert_eq!(value["damaged_cells"], 4);
        assert_eq!(value["full_render_frames"], 0);
        assert_eq!(value["dirty_render_frames"], 0);
        assert!(value["first_pty_byte_ms"].is_number());
        assert!(value["first_rendered_cell_ms"].is_number());
    }

    #[test]
    fn window_app_collects_input_write_metrics() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut app = NativeWindowApp::new(None);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));

        app.write_pty_bytes(b"abc").unwrap();

        let metrics = app.metrics_snapshot();
        assert_eq!(written.lock().unwrap().as_slice(), b"abc");
        assert_eq!(metrics.input_writes, 1);
        assert_eq!(metrics.input_bytes, 3);
    }

    #[test]
    fn window_app_uses_configured_startup_command() {
        let app = NativeWindowApp::new_with_command(
            None,
            rssh_pty::PtyCommand::new("powershell").with_args([
                "-NoProfile",
                "-Command",
                "Write-Output window-smoke",
            ]),
        );

        assert_eq!(app.startup_command().program(), "powershell");
        assert_eq!(
            app.startup_command().args(),
            ["-NoProfile", "-Command", "Write-Output window-smoke"]
        );
    }

    #[test]
    fn window_app_starts_with_default_shell_state() {
        let app = NativeWindowApp::new_with_command(
            None,
            rssh_pty::PtyCommand::new("powershell").with_args(["-NoProfile"]),
        );

        assert_eq!(app.active_workspace_id(), rssh_core::WorkspaceId::new(1));
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );
    }

    #[test]
    fn window_app_dispatches_new_tab_action() {
        let mut app = NativeWindowApp::new_with_command(
            None,
            rssh_pty::PtyCommand::new("powershell").with_args(["-NoProfile"]),
        );

        let action = app
            .app_shell_action_for_key(
                &Key::Character("T".into()),
                ModifiersState::CONTROL | ModifiersState::SHIFT,
            )
            .expect("expected new tab shortcut");
        app.dispatch_app_action(action).unwrap();

        assert_eq!(app.active_workspace_id(), rssh_core::WorkspaceId::new(1));
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
    }

    #[test]
    fn window_app_records_osc7_current_working_dir_on_active_pane_launch() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"\x1b]7;file://host/home/ops\x07")
            .unwrap();

        assert_eq!(
            app.app_shell.active_pane().launch().cwd(),
            Some("file://host/home/ops")
        );
    }

    #[test]
    fn window_app_records_osc7_current_working_dir_on_inactive_pane_launch() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();

        app.handle_pane_pty_output(
            rssh_core::PaneId::new(1),
            b"\x1b]7;file://host/home/ops\x07",
        )
        .unwrap();

        assert_eq!(
            app.app_shell.active_workspace().tabs()[0].panes()[0]
                .launch()
                .cwd(),
            Some("file://host/home/ops")
        );
    }

    #[test]
    fn window_app_records_iterm_user_var_on_active_pane_metadata() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"\x1b]1337;SetUserVar=WEZTERM_PROG=YmFy\x07")
            .unwrap();

        assert_eq!(
            app.app_shell
                .active_pane()
                .user_vars()
                .get("WEZTERM_PROG")
                .map(String::as_str),
            Some("bar")
        );
    }

    #[test]
    fn window_app_records_iterm_user_var_on_inactive_pane_metadata() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();

        app.handle_pane_pty_output(
            rssh_core::PaneId::new(1),
            b"\x1b]1337;SetUserVar=WEZTERM_PROG=YmFy\x07",
        )
        .unwrap();

        assert_eq!(
            app.app_shell.active_workspace().tabs()[0].panes()[0]
                .user_vars()
                .get("WEZTERM_PROG")
                .map(String::as_str),
            Some("bar")
        );
    }

    #[test]
    fn window_app_records_iterm_badge_format_on_active_pane_metadata() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"\x1b]1337;SetBadgeFormat=aGVsbG8=\x07")
            .unwrap();

        assert_eq!(app.app_shell.active_pane().badge_format(), Some("hello"));
    }

    #[test]
    fn window_app_records_iterm_badge_format_on_inactive_pane_metadata() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();

        app.handle_pane_pty_output(
            rssh_core::PaneId::new(1),
            b"\x1b]1337;SetBadgeFormat=aGVsbG8=\x07",
        )
        .unwrap();

        assert_eq!(
            app.app_shell.active_workspace().tabs()[0].panes()[0].badge_format(),
            Some("hello")
        );
    }

    #[test]
    fn window_app_records_progress_on_active_and_inactive_pane_metadata() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"\x1b]9;4;1;42\x07").unwrap();
        assert_eq!(
            app.app_shell.active_pane().progress(),
            rssh_core::app_shell::PaneProgress::Percentage(42)
        );

        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.handle_pane_pty_output(rssh_core::PaneId::new(1), b"\x9d9;4;2;7\x9c")
            .unwrap();

        assert_eq!(
            app.app_shell.active_workspace().tabs()[0].panes()[0].progress(),
            rssh_core::app_shell::PaneProgress::Error(7)
        );
    }

    #[test]
    fn window_app_renders_tab_bar_above_terminal_snapshot() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"live").unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();

        let snapshot = app.render_snapshot();
        let tab_bar = snapshot_row_text(&snapshot, 0, TERMINAL_COLUMNS);

        assert!(tab_bar.contains("ws:default"));
        assert!(tab_bar.contains("1:1 panes:1"));
        assert!(tab_bar.contains("2:2* panes:1"));
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 0), Some('l'));
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 3), Some('e'));
    }

    #[test]
    fn window_app_tab_bar_uses_active_pane_title() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"\x1b]2;PowerShell\x07").unwrap();

        let snapshot = app.render_snapshot();
        let tab_bar = snapshot_row_text(&snapshot, 0, TERMINAL_COLUMNS);
        assert!(tab_bar.contains("PowerShell"), "tab bar was {tab_bar:?}");
    }

    #[test]
    fn window_app_tab_bar_uses_inactive_tab_active_pane_title() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"\x1b]2;PowerShell\x07").unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();

        let snapshot = app.render_snapshot();
        let tab_bar = snapshot_row_text(&snapshot, 0, TERMINAL_COLUMNS);
        assert!(tab_bar.contains("PowerShell"), "tab bar was {tab_bar:?}");
    }

    #[test]
    fn window_app_tab_bar_prefers_explicit_tab_title() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"\x1b]2;PowerShell\x07").unwrap();
        app.dispatch_app_action(AppAction::SetTabTitle {
            tab: rssh_core::TabId::new(1),
            title: "build".to_owned(),
        })
        .unwrap();

        let snapshot = app.render_snapshot();
        let tab_bar = snapshot_row_text(&snapshot, 0, TERMINAL_COLUMNS);
        assert!(tab_bar.contains("build"), "tab bar was {tab_bar:?}");
        assert!(!tab_bar.contains("PowerShell"), "tab bar was {tab_bar:?}");
    }

    #[test]
    fn window_app_clicking_tab_bar_activates_tab() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));

        let first_tab_column = app.tab_bar_workspace_label().chars().count() + 1;
        let x = u32::try_from(first_tab_column).unwrap_or(0) * CELL_WIDTH;
        app.handle_cursor_moved(PhysicalPosition::new(f64::from(x), 0.0))
            .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
    }

    #[test]
    fn window_app_clicking_tab_bar_close_marker_closes_tab() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));

        let first_tab_width = tab_bar_tab_label(0, rssh_core::TabId::new(1), 1, false, None)
            .chars()
            .count();
        let second_tab_label = tab_bar_tab_label(1, rssh_core::TabId::new(2), 1, true, None);
        let second_tab_start = app.tab_bar_workspace_label().chars().count() + first_tab_width;
        let close_offset = second_tab_label
            .chars()
            .position(|character| character == 'x')
            .expect("tab label should expose close marker");
        let x = u32::try_from(second_tab_start + close_offset).unwrap_or(0) * CELL_WIDTH;

        app.handle_cursor_moved(PhysicalPosition::new(f64::from(x), 0.0))
            .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
        assert_eq!(app.app_shell.active_workspace().tabs().len(), 1);
        assert!(!app.window_close_requested_for_test());
    }

    #[test]
    fn window_app_clicking_last_tab_bar_close_marker_requests_window_close() {
        let mut app = NativeWindowApp::new(None);

        let tab_label = tab_bar_tab_label(0, rssh_core::TabId::new(1), 1, true, None);
        let close_offset = tab_label
            .chars()
            .position(|character| character == 'x')
            .expect("tab label should expose close marker");
        let close_column = app.tab_bar_workspace_label().chars().count() + close_offset;
        let x = u32::try_from(close_column).unwrap_or(0) * CELL_WIDTH;

        app.handle_cursor_moved(PhysicalPosition::new(f64::from(x), 0.0))
            .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        assert!(app.window_close_requested_for_test());
    }

    #[test]
    fn window_app_clicking_tab_bar_new_tab_button_creates_tab() {
        let mut app = NativeWindowApp::new(None);

        let snapshot = app.render_snapshot();
        let tab_bar = snapshot_row_text(&snapshot, 0, TERMINAL_COLUMNS);
        assert!(tab_bar.contains(" + "));

        let tab_width = tab_bar_tab_label(0, rssh_core::TabId::new(1), 1, true, None)
            .chars()
            .count();
        let new_tab_column = app.tab_bar_workspace_label().chars().count() + tab_width + 1;
        let x = u32::try_from(new_tab_column).unwrap_or(0) * CELL_WIDTH;

        app.handle_cursor_moved(PhysicalPosition::new(f64::from(x), 0.0))
            .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
        assert_eq!(app.app_shell.active_workspace().tabs().len(), 2);
    }

    #[test]
    fn window_app_renders_right_split_panes_with_separator() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"left").unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.handle_pty_output(b"right").unwrap();

        let snapshot = app.render_snapshot();

        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 0), Some('l'));
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 39), Some('|'));
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 40), Some('r'));
    }

    #[test]
    fn window_app_renders_down_split_panes_with_separator() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"top").unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Down,
            launch: None,
        })
        .unwrap();
        app.handle_pty_output(b"bottom").unwrap();

        let snapshot = app.render_snapshot();

        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 0), Some('t'));
        assert_eq!(snapshot_char(&snapshot, 12, 0), Some('-'));
        assert_eq!(snapshot_char(&snapshot, 13, 0), Some('b'));
    }

    #[test]
    fn window_app_clicking_split_pane_focuses_that_pane() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(CELL_WIDTH),
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
    }

    #[test]
    fn window_app_mouse_wheel_scrolls_split_pane_under_cursor() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(80, 2));
        app.handle_pty_output(b"aa\nbb\ncc").unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.handle_pty_output(b"right").unwrap();
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(CELL_WIDTH),
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();
        assert!(
            app.handle_window_mouse_wheel(MouseScrollDelta::LineDelta(0.0, 1.0))
                .unwrap()
        );

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));
    }

    #[test]
    fn window_app_resizes_right_split_pane_left() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"left").unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.handle_pty_output(b"right").unwrap();

        app.dispatch_app_action(AppAction::ResizePane {
            pane: rssh_core::PaneId::new(2),
            direction: rssh_core::app_shell::ResizeDirection::Left,
            amount: 5,
        })
        .unwrap();

        let snapshot = app.render_snapshot();

        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 34), Some('|'));
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 35), Some('r'));
    }

    #[test]
    fn window_app_dragging_right_split_separator_resizes_panes() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"left").unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.handle_pty_output(b"right").unwrap();

        let separator_x = 39_u32 * CELL_WIDTH;
        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(separator_x),
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(43_u32 * CELL_WIDTH),
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Released, MouseButton::Left)
                .unwrap()
        );

        let snapshot = app.render_snapshot();
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 43), Some('|'));
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 44), Some('r'));
    }

    #[test]
    fn window_app_zoomed_split_pane_fills_tab_region() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"left").unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.handle_pty_output(b"right").unwrap();

        app.dispatch_app_action(AppAction::TogglePaneZoom {
            pane: rssh_core::PaneId::new(2),
        })
        .unwrap();

        let snapshot = app.render_snapshot();
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 0), Some('r'));
        assert_ne!(snapshot_char(&snapshot, TAB_BAR_ROWS, 39), Some('|'));

        app.dispatch_app_action(AppAction::TogglePaneZoom {
            pane: rssh_core::PaneId::new(2),
        })
        .unwrap();

        let snapshot = app.render_snapshot();
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 39), Some('|'));
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS, 40), Some('r'));
    }

    #[test]
    fn window_title_reports_app_shell_state() {
        let app = NativeWindowApp::new_with_command(
            None,
            rssh_pty::PtyCommand::new("powershell").with_args(["-NoProfile"]),
        );

        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );
    }

    #[test]
    fn window_app_logs_visible_pty_output() {
        let logged = Arc::new(Mutex::new(Vec::new()));
        let mut app =
            NativeWindowApp::new_with_session_log(None, SharedWriter(Arc::clone(&logged)));

        app.handle_pty_output(b"before\x1b[6nafter").unwrap();

        assert_eq!(logged.lock().unwrap().as_slice(), b"beforeafter");
    }

    #[test]
    fn window_app_omits_title_sequence_from_session_log() {
        let logged = Arc::new(Mutex::new(Vec::new()));
        let mut app =
            NativeWindowApp::new_with_session_log(None, SharedWriter(Arc::clone(&logged)));

        app.handle_pty_output(b"before\x1b]0;ops\x07after").unwrap();

        assert_eq!(app.window_title, "ops");
        assert_eq!(logged.lock().unwrap().as_slice(), b"beforeafter");
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
    fn window_title_omits_scrollback_position_after_scrollbar_overlay() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee").unwrap();

        assert_eq!(app.runtime.terminal().scrollback().len(), 3);
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );

        app.scroll_viewport_lines(1);
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );

        app.scroll_viewport_lines(99);
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );

        app.scroll_viewport_lines(-99);
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );
    }

    #[test]
    fn window_title_combines_scrollback_and_search_status() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(6, 2));
        app.handle_pty_output(b"alpha\r\nbeta\r\ngamma").unwrap();

        assert!(app.update_search_query("alpha"));

        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Search: alpha"
        );
    }

    #[test]
    fn window_title_includes_command_palette_status() {
        let mut app = NativeWindowApp::new(None);
        app.enter_command_palette_mode();

        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Command Palette: [1 / 64] New Tab"
        );

        app.command_palette_set_query("split".to_owned());

        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Command Palette: \"split\" [1 / 2] Split Pane Right"
        );

        app.command_palette_set_query("zzz".to_owned());

        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Command Palette: \"zzz\" (no match)"
        );
    }

    #[test]
    fn window_title_includes_quick_select_status() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(40, 1));
        app.handle_pty_output(b"https://example.com 10.0.0.1 test@x.io")
            .unwrap();

        app.enter_quick_select_mode();
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Quick Select: [1 / 3]"
        );
        assert_eq!(app.selected_text().as_deref(), Some("https://example.com"));

        assert!(app.quick_select_step(SearchDirection::Next));
        assert_eq!(app.selected_text().as_deref(), Some("10.0.0.1"));
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Quick Select: [2 / 3]"
        );

        assert!(app.quick_select_step(SearchDirection::Previous));
        assert_eq!(app.selected_text().as_deref(), Some("https://example.com"));

        app.exit_quick_select_mode();
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );
    }

    #[test]
    fn window_quick_select_clears_other_overlays() {
        let mut app = NativeWindowApp::new(None);
        app.update_search_query("example");
        assert!(app.search.is_some());

        app.enter_quick_select_mode();
        assert!(app.search.is_none());
        assert!(app.quick_select.is_some());

        app.enter_command_palette_mode();
        assert!(app.quick_select.is_none());
        assert!(app.command_palette.is_some());
    }

    #[test]
    fn window_quick_select_mode_has_no_match_status() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"no links here").unwrap();

        app.enter_quick_select_mode();
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Quick Select: no match"
        );
        assert_eq!(app.quick_select.as_ref().unwrap().matches.len(), 0);
    }

    #[test]
    fn window_quick_select_label_input_copies_matching_candidate() {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let primary_copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let recorded_primary = Arc::clone(&primary_copied);
        let mut app = NativeWindowApp::new(None);
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });
        app.primary_selection_writer = Box::new(move |text: &str| {
            recorded_primary.lock().unwrap().push(text.to_owned());
            true
        });
        app.runtime.resize(rssh_core::TerminalSize::new(40, 1));
        app.handle_pty_output(b"https://example.com 10.0.0.1 test@x.io")
            .unwrap();

        app.enter_quick_select_mode();
        assert_eq!(app.quick_select.as_ref().unwrap().matches.len(), 3);

        assert!(
            app.handle_quick_select_logical_key(
                &Key::Character("a".into()),
                ModifiersState::empty()
            )
        );

        assert!(app.quick_select.is_none());
        assert!(app.selection.is_none());
        assert_eq!(copied.lock().unwrap().as_slice(), ["test@x.io"]);
        assert_eq!(primary_copied.lock().unwrap().as_slice(), ["test@x.io"]);
    }

    #[test]
    fn window_quick_select_enter_uses_wezterm_prior_match_binding() {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let mut app = NativeWindowApp::new(None);
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });
        app.runtime.resize(rssh_core::TerminalSize::new(40, 1));
        app.handle_pty_output(b"https://example.com 10.0.0.1 test@x.io")
            .unwrap();

        app.enter_quick_select_mode();
        assert_eq!(app.selected_text().as_deref(), Some("https://example.com"));

        assert!(app.handle_quick_select_logical_key(
            &Key::Named(NamedKey::Enter),
            ModifiersState::empty()
        ));

        assert!(app.quick_select.is_some());
        assert!(copied.lock().unwrap().is_empty());
        assert_eq!(app.selected_text().as_deref(), Some("test@x.io"));
    }

    #[test]
    fn window_quick_select_ctrl_n_and_ctrl_p_use_wezterm_match_navigation() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(40, 1));
        app.handle_pty_output(b"https://example.com 10.0.0.1 test@x.io")
            .unwrap();

        app.enter_quick_select_mode();
        assert_eq!(app.selected_text().as_deref(), Some("https://example.com"));

        assert!(
            app.handle_quick_select_logical_key(
                &Key::Character("n".into()),
                ModifiersState::CONTROL
            )
        );
        assert!(app.quick_select.is_some());
        assert_eq!(app.selected_text().as_deref(), Some("10.0.0.1"));

        assert!(
            app.handle_quick_select_logical_key(
                &Key::Character("p".into()),
                ModifiersState::CONTROL
            )
        );
        assert!(app.quick_select.is_some());
        assert_eq!(app.selected_text().as_deref(), Some("https://example.com"));
    }

    #[test]
    fn window_quick_select_page_keys_skip_visible_page_matches() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(20, 3));
        app.handle_pty_output(
            b"u0@example.com\r\nu1@example.com\r\nu2@example.com\r\nu3@example.com\r\nu4@example.com",
        )
        .unwrap();

        app.enter_quick_select_mode();
        assert_eq!(app.quick_select.as_ref().unwrap().matches.len(), 5);
        assert_eq!(app.selected_text().as_deref(), Some("u0@example.com"));

        assert!(app.handle_quick_select_logical_key(
            &Key::Named(NamedKey::PageDown),
            ModifiersState::empty()
        ));
        assert!(app.quick_select.is_some());
        assert_eq!(app.selected_text().as_deref(), Some("u3@example.com"));

        assert!(app.handle_quick_select_logical_key(
            &Key::Named(NamedKey::PageUp),
            ModifiersState::empty()
        ));
        assert!(app.quick_select.is_some());
        assert_eq!(app.selected_text().as_deref(), Some("u0@example.com"));
    }

    #[test]
    fn window_quick_select_uppercase_label_input_pastes_matching_candidate() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let mut app = NativeWindowApp::new(None);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });
        app.runtime.resize(rssh_core::TerminalSize::new(40, 1));
        app.handle_pty_output(b"https://example.com 10.0.0.1 test@x.io")
            .unwrap();

        app.enter_quick_select_mode();
        assert_eq!(app.quick_select.as_ref().unwrap().matches.len(), 3);

        assert!(
            app.handle_quick_select_logical_key(&Key::Character("A".into()), ModifiersState::SHIFT)
        );

        assert!(app.quick_select.is_none());
        assert!(app.selection.is_none());
        assert!(copied.lock().unwrap().is_empty());
        assert_eq!(written.lock().unwrap().as_slice(), b"test@x.io");
    }

    #[test]
    fn window_quick_select_matches_wezterm_non_http_url_schemes() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(48, 1));
        app.handle_pty_output(b"git@github.com:wezterm/wezterm.git")
            .unwrap();

        app.enter_quick_select_mode();

        assert_eq!(app.quick_select.as_ref().unwrap().matches.len(), 1);
        assert_eq!(
            app.selected_text().as_deref(),
            Some("git@github.com:wezterm/wezterm.git")
        );
    }

    #[test]
    fn window_quick_select_matches_wezterm_path_and_hash_patterns() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(64, 1));
        app.handle_pty_output(b"/var/log/rssh.log deadbeef")
            .unwrap();

        app.enter_quick_select_mode();

        assert_eq!(app.quick_select.as_ref().unwrap().matches.len(), 2);
        assert_eq!(app.selected_text().as_deref(), Some("/var/log/rssh.log"));
        assert!(app.quick_select_step(SearchDirection::Next));
        assert_eq!(app.selected_text().as_deref(), Some("deadbeef"));
    }

    #[test]
    fn window_quick_select_matches_wezterm_capture_group_patterns() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(80, 3));
        app.handle_pty_output(
            b"[docs](https://example.com/path)\r\n--- a/src/main.rs\r\nsha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        )
        .unwrap();

        app.enter_quick_select_mode();

        assert_eq!(app.quick_select.as_ref().unwrap().matches.len(), 3);
        assert_eq!(
            app.selected_text().as_deref(),
            Some("https://example.com/path")
        );
        assert!(app.quick_select_step(SearchDirection::Next));
        assert_eq!(app.selected_text().as_deref(), Some("src/main.rs"));
        assert!(app.quick_select_step(SearchDirection::Next));
        assert_eq!(
            app.selected_text().as_deref(),
            Some("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
        );
    }

    #[test]
    fn window_title_includes_pane_select_status() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_pane_select_mode();

        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:2] - Pane Select: [2 panes]"
        );
    }

    #[test]
    fn window_pane_select_activates_labelled_pane() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::ActivatePane {
            pane: rssh_core::PaneId::new(1),
        })
        .unwrap();

        app.enter_pane_select_mode();

        let pane_select = app
            .pane_select
            .as_ref()
            .expect("pane select should be active");
        assert_eq!(pane_select.labels[0].label, "a");
        assert_eq!(pane_select.labels[0].pane_id, rssh_core::PaneId::new(1));
        assert_eq!(pane_select.labels[1].label, "s");
        assert_eq!(pane_select.labels[1].pane_id, rssh_core::PaneId::new(2));

        assert!(app.handle_pane_select_key(&Key::Character("s".into()), ModifiersState::empty()));

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));
        assert!(app.pane_select.is_none());
    }

    #[test]
    fn window_pane_select_renders_labels_over_panes() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(20, 4));
        app.refresh_snapshot();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_pane_select_mode();

        let snapshot = app.render_snapshot();

        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS + 2, 4), Some('a'));
        assert_eq!(snapshot_char(&snapshot, TAB_BAR_ROWS + 2, 15), Some('s'));
    }

    #[test]
    fn window_pane_select_swap_mode_swaps_layout_and_focuses_selected() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(20, 4));
        app.refresh_snapshot();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::ActivatePane {
            pane: rssh_core::PaneId::new(1),
        })
        .unwrap();

        app.enter_pane_select_mode_with_mode(WindowPaneSelectMode::SwapWithActive);
        assert!(app.handle_pane_select_key(&Key::Character("s".into()), ModifiersState::empty()));

        let layout = app.pane_render_layout();
        let selected_rect = layout
            .panes
            .iter()
            .find(|rect| rect.pane_id == rssh_core::PaneId::new(2))
            .expect("selected pane should still render");
        let active_rect = layout
            .panes
            .iter()
            .find(|rect| rect.pane_id == rssh_core::PaneId::new(1))
            .expect("active pane should still render");

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));
        assert_eq!(selected_rect.column, 0);
        assert_eq!(active_rect.column, 10);
        assert!(app.pane_select.is_none());
    }

    #[test]
    fn window_pane_select_swap_keep_focus_keeps_active_pane() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(20, 4));
        app.refresh_snapshot();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::ActivatePane {
            pane: rssh_core::PaneId::new(1),
        })
        .unwrap();

        app.enter_pane_select_mode_with_mode(WindowPaneSelectMode::SwapWithActiveKeepFocus);
        assert!(app.handle_pane_select_key(&Key::Character("s".into()), ModifiersState::empty()));

        let layout = app.pane_render_layout();
        let selected_rect = layout
            .panes
            .iter()
            .find(|rect| rect.pane_id == rssh_core::PaneId::new(2))
            .expect("selected pane should still render");
        let active_rect = layout
            .panes
            .iter()
            .find(|rect| rect.pane_id == rssh_core::PaneId::new(1))
            .expect("active pane should still render");

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
        assert_eq!(selected_rect.column, 0);
        assert_eq!(active_rect.column, 10);
        assert!(app.pane_select.is_none());
    }

    #[test]
    fn window_pane_select_move_to_new_tab_moves_selected_pane_and_activates_tab() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::ActivatePane {
            pane: rssh_core::PaneId::new(1),
        })
        .unwrap();

        app.enter_pane_select_mode_with_mode(WindowPaneSelectMode::MoveToNewTab);
        assert!(app.handle_pane_select_key(&Key::Character("s".into()), ModifiersState::empty()));

        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));
        assert_eq!(app.app_shell.active_workspace().tabs().len(), 2);
        assert_eq!(app.app_shell.active_workspace().tabs()[0].panes().len(), 1);
        assert_eq!(
            app.app_shell.active_workspace().tabs()[0].panes()[0].id(),
            rssh_core::PaneId::new(1)
        );
        assert_eq!(app.app_shell.active_tab().panes().len(), 1);
        assert_eq!(
            app.app_shell.active_tab().panes()[0].id(),
            rssh_core::PaneId::new(2)
        );
        assert!(app.pane_select.is_none());
    }

    #[test]
    fn window_pane_select_move_to_new_window_detaches_selected_pane_and_requests_window() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::ActivatePane {
            pane: rssh_core::PaneId::new(1),
        })
        .unwrap();

        app.enter_pane_select_mode_with_mode(WindowPaneSelectMode::MoveToNewWindow);
        assert!(app.handle_pane_select_key(&Key::Character("s".into()), ModifiersState::empty()));

        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
        assert_eq!(app.app_shell.active_tab().panes().len(), 1);
        assert_eq!(
            app.app_shell.active_tab().panes()[0].id(),
            rssh_core::PaneId::new(1)
        );

        let pending_window = app
            .app_shell
            .pending_windows()
            .first()
            .expect("move should request a new window");
        assert_eq!(pending_window.id(), rssh_core::WindowId::new(2));
        assert_eq!(pending_window.active_tab_id(), rssh_core::TabId::new(2));
        assert_eq!(pending_window.active_pane_id(), rssh_core::PaneId::new(2));
        assert_eq!(pending_window.tab().panes().len(), 1);
        assert!(pending_window.tab().panes()[0].split().is_none());
        assert!(app.pane_select.is_none());
    }

    #[test]
    fn window_app_consumes_pending_new_window_with_detached_pane_runtime() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"left").unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.handle_pty_output(b"right").unwrap();
        app.dispatch_app_action(AppAction::ActivatePane {
            pane: rssh_core::PaneId::new(1),
        })
        .unwrap();

        app.dispatch_app_action(AppAction::MovePaneToNewWindow {
            pane: rssh_core::PaneId::new(2),
        })
        .unwrap();

        let detached_app = app
            .take_next_pending_window_app()
            .expect("pending window should create a detached app");

        assert!(app.app_shell.pending_windows().is_empty());
        assert_eq!(app.app_shell.pane_ids(), vec![rssh_core::PaneId::new(1)]);
        assert!(!app.pane_runtimes.contains_key(&rssh_core::PaneId::new(2)));
        assert_eq!(
            detached_app.active_workspace_id(),
            rssh_core::WorkspaceId::new(1)
        );
        assert_eq!(app.app_window_id_for_test(), rssh_core::WindowId::new(1));
        assert_eq!(
            detached_app.app_window_id_for_test(),
            rssh_core::WindowId::new(2)
        );
        assert_eq!(detached_app.active_tab_id(), rssh_core::TabId::new(2));
        assert_eq!(detached_app.active_pane_id(), rssh_core::PaneId::new(2));
        assert_eq!(snapshot_char(&detached_app.snapshot, 0, 0), Some('r'));
        assert_eq!(snapshot_char(&detached_app.snapshot, 0, 4), Some('t'));
    }

    #[test]
    fn window_manager_collects_detached_app_after_move_to_new_window() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"left").unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.handle_pty_output(b"right").unwrap();
        app.dispatch_app_action(AppAction::MovePaneToNewWindow {
            pane: rssh_core::PaneId::new(2),
        })
        .unwrap();

        let mut manager = NativeWindowManager::new_for_test(app);
        manager.collect_pending_window_apps_from_primary_for_test();

        assert_eq!(manager.pending_app_count_for_test(), 1);
        let detached_app = manager
            .pending_app_for_test(0)
            .expect("manager should hold detached window app");
        assert_eq!(detached_app.active_pane_id(), rssh_core::PaneId::new(2));
        assert_eq!(snapshot_char(&detached_app.snapshot, 0, 0), Some('r'));
        assert_eq!(snapshot_char(&detached_app.snapshot, 0, 4), Some('t'));
    }

    #[test]
    fn window_pane_select_cancel_keys_exit_without_focusing() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::ActivatePane {
            pane: rssh_core::PaneId::new(1),
        })
        .unwrap();

        app.enter_pane_select_mode();
        assert!(app.handle_pane_select_key(&Key::Named(NamedKey::Escape), ModifiersState::empty()));

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
        assert!(app.pane_select.is_none());

        app.enter_pane_select_mode();
        assert!(app.handle_pane_select_key(&Key::Character("g".into()), ModifiersState::CONTROL));

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
        assert!(app.pane_select.is_none());
    }

    #[test]
    fn recognizes_window_copy_mode_shortcut() {
        assert!(window_copy_mode_shortcut(
            &Key::Character("x".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(window_copy_mode_shortcut(
            &Key::Character("X".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(!window_copy_mode_shortcut(
            &Key::Character("x".into()),
            ModifiersState::CONTROL
        ));
    }

    #[test]
    fn window_title_includes_copy_mode_status() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();

        app.enter_copy_mode();
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Copy Mode"
        );

        app.handle_copy_mode_key(&Key::Character("v".into()), ModifiersState::empty());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Copy Mode: Cell"
        );

        app.handle_copy_mode_key(&Key::Named(NamedKey::Escape), ModifiersState::empty());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );
    }

    #[test]
    fn window_copy_mode_escape_character_closes_copy_mode() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();

        app.enter_copy_mode();
        assert!(app.copy_mode.is_some());

        assert!(
            app.handle_copy_mode_key(&Key::Character("\u{1b}".into()), ModifiersState::empty())
        );

        assert!(app.copy_mode.is_none());
        assert!(app.search.is_none());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );
    }

    #[test]
    fn window_copy_mode_allows_command_palette_fallback_shortcut() {
        let mut app = NativeWindowApp::new(None);
        app.enter_copy_mode();

        assert!(app.copy_mode.is_some());

        assert!(app.handle_copy_mode_key(
            &Key::Character("p".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));

        assert!(app.copy_mode.is_none());
        assert!(app.command_palette.is_some());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Command Palette: [1 / 64] New Tab"
        );
    }

    #[test]
    fn window_copy_mode_search_allows_command_palette_fallback_shortcut() {
        let mut app = NativeWindowApp::new(None);
        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("/".into()), ModifiersState::empty()));

        assert!(app.copy_mode.is_some());
        assert!(app.search.is_some());

        assert!(app.handle_copy_mode_key(
            &Key::Character("p".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));

        assert!(app.copy_mode.is_none());
        assert!(app.search.is_none());
        assert!(app.command_palette.is_some());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Command Palette: [1 / 64] New Tab"
        );
    }

    #[test]
    fn window_copy_mode_allows_new_tab_fallback_shortcut() {
        let mut app = NativeWindowApp::new(None);
        app.enter_copy_mode();
        let tab_order = |app: &NativeWindowApp| -> Vec<rssh_core::TabId> {
            app.app_shell
                .active_workspace()
                .tabs()
                .iter()
                .map(rssh_core::app_shell::Tab::id)
                .collect()
        };

        assert_eq!(tab_order(&app), vec![rssh_core::TabId::new(1)]);
        assert!(app.copy_mode.is_some());

        assert!(app.handle_copy_mode_key(
            &Key::Character("t".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));

        assert_eq!(
            tab_order(&app),
            vec![rssh_core::TabId::new(1), rssh_core::TabId::new(2)]
        );
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
        assert!(app.copy_mode.is_none());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:2 pane:2]"
        );
    }

    #[test]
    fn window_copy_mode_search_allows_new_tab_fallback_shortcut() {
        let mut app = NativeWindowApp::new(None);
        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("/".into()), ModifiersState::empty()));
        let tab_order = |app: &NativeWindowApp| -> Vec<rssh_core::TabId> {
            app.app_shell
                .active_workspace()
                .tabs()
                .iter()
                .map(rssh_core::app_shell::Tab::id)
                .collect()
        };

        assert_eq!(tab_order(&app), vec![rssh_core::TabId::new(1)]);
        assert!(app.copy_mode.is_some());
        assert!(app.search.is_some());

        assert!(app.handle_copy_mode_key(
            &Key::Character("t".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));

        assert_eq!(
            tab_order(&app),
            vec![rssh_core::TabId::new(1), rssh_core::TabId::new(2)]
        );
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
        assert!(app.copy_mode.is_none());
        assert!(app.search.is_none());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:2 pane:2]"
        );
    }

    #[test]
    fn window_copy_mode_cell_selection_copies_and_exits() {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_copy_mode();
        app.handle_copy_mode_key(&Key::Character("v".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("y".into()), ModifiersState::empty());

        assert!(app.copy_mode.is_none());
        assert_eq!(copied.lock().unwrap().as_slice(), ["abcd"]);
    }

    #[test]
    fn window_copy_mode_y_copies_to_clipboard_and_primary_then_scrolls_bottom() {
        let clipboard = Arc::new(Mutex::new(Vec::new()));
        let primary = Arc::new(Mutex::new(Vec::new()));
        let recorded_clipboard = Arc::clone(&clipboard);
        let recorded_primary = Arc::clone(&primary);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee\nff").unwrap();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded_clipboard.lock().unwrap().push(text.to_owned());
            true
        });
        app.primary_selection_writer = Box::new(move |text: &str| {
            recorded_primary.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("V".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 4);

        assert!(app.handle_copy_mode_key(&Key::Character("y".into()), ModifiersState::empty()));

        assert!(app.copy_mode.is_none());
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "ee  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ff  ");
        assert_eq!(clipboard.lock().unwrap().as_slice(), ["aa"]);
        assert_eq!(primary.lock().unwrap().as_slice(), ["aa"]);
    }

    #[test]
    fn window_copy_mode_space_uses_wezterm_cell_selection_binding() {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character(" ".into()), ModifiersState::empty()));
        app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("y".into()), ModifiersState::empty());

        assert!(app.copy_mode.is_none());
        assert_eq!(copied.lock().unwrap().as_slice(), ["abcd"]);
    }

    #[test]
    fn window_copy_mode_line_selection() {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(6, 1));
        app.handle_pty_output(b"abcdef").unwrap();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_copy_mode();
        app.handle_copy_mode_key(&Key::Character("V".into()), ModifiersState::SHIFT);
        app.handle_copy_mode_key(&Key::Character("y".into()), ModifiersState::empty());

        assert!(app.copy_mode.is_none());
        assert_eq!(copied.lock().unwrap().as_slice(), ["abcdef"]);
    }

    #[test]
    fn window_copy_mode_uppercase_v_no_modifier_uses_line_selection_binding() {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(6, 1));
        app.handle_pty_output(b"abcdef").unwrap();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("V".into()), ModifiersState::empty()));
        app.handle_copy_mode_key(&Key::Character("y".into()), ModifiersState::empty());

        assert!(app.copy_mode.is_none());
        assert_eq!(copied.lock().unwrap().as_slice(), ["abcdef"]);
    }

    #[test]
    fn window_copy_mode_moves_by_semantic_zone() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(12, 4));
        app.handle_pty_output(
            b"ready\n\x1b]133;A\x07> \x1b]133;B\x07ls -l\n\x1b]133;C\x07file.txt",
        )
        .unwrap();

        app.enter_copy_mode();
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 8 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("Z".into()), ModifiersState::SHIFT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 0 })
        );
    }

    #[test]
    fn window_copy_mode_moves_by_output_semantic_zone_type() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(12, 4));
        app.handle_pty_output(
            b"ready\n\x1b]133;A\x07> \x1b]133;B\x07ls -l\n\x1b]133;C\x07file.txt",
        )
        .unwrap();

        app.enter_copy_mode();
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 8 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::ALT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::ALT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );

        assert!(app.handle_copy_mode_key(
            &Key::Character("Z".into()),
            ModifiersState::ALT | ModifiersState::SHIFT
        ));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 0 })
        );
    }

    #[test]
    fn window_copy_mode_moves_by_prompt_and_input_semantic_zone_types() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(12, 4));
        app.handle_pty_output(
            b"ready\n\x1b]133;A\x07> \x1b]133;B\x07ls -l\n\x1b]133;C\x07file.txt",
        )
        .unwrap();

        app.enter_copy_mode();
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 8 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("p".into()), ModifiersState::ALT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 0 })
        );

        assert!(app.handle_copy_mode_key(
            &Key::Character("I".into()),
            ModifiersState::ALT | ModifiersState::SHIFT
        ));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );
    }

    #[test]
    fn window_copy_mode_semantic_zone_movement_scrolls_into_scrollback() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(12, 2));
        app.handle_pty_output(
            b"\x1b]133;C\x07oldout\n\x1b]133;A\x07> one\n\x1b]133;C\x07midout\n\x1b]133;A\x07> two\n\x1b]133;C\x07live",
        )
        .unwrap();

        assert_eq!(app.runtime.terminal().scrollback().len(), 3);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 12), "> two       ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 12), "live        ");

        app.enter_copy_mode();
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 4 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 1);
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 12), "midout      ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 12), "> two       ");
    }

    #[test]
    fn window_copy_mode_selection_copies_across_scrollback_viewports() {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(12, 2));
        app.handle_pty_output(
            b"\x1b]133;C\x07oldout\n\x1b]133;A\x07> one\n\x1b]133;C\x07midout\n\x1b]133;A\x07> two\n\x1b]133;C\x07live",
        )
        .unwrap();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_copy_mode();
        app.handle_copy_mode_key(&Key::Character("v".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::empty());
        app.handle_copy_mode_key(&Key::Character("z".into()), ModifiersState::empty());

        assert_eq!(app.scrollback_offset, 1);
        assert_eq!(app.selected_text().as_deref(), Some("midout\n> two\nlive"));
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 1, 0).unwrap().inverse);

        app.handle_copy_mode_key(&Key::Character("y".into()), ModifiersState::empty());

        assert!(app.copy_mode.is_none());
        assert_eq!(copied.lock().unwrap().as_slice(), ["midout\n> two\nlive"]);
    }

    #[test]
    fn window_copy_mode_o_moves_to_selection_other_end() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"abcd\r\nefgh\r\nijkl").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("v".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("k".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty()));
        assert_eq!(app.selected_text().as_deref(), Some("gh\nijkl"));

        assert!(app.handle_copy_mode_key(&Key::Character("o".into()), ModifiersState::empty()));

        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| (
                copy_mode.source_cursor.row,
                copy_mode.source_cursor.column,
                copy_mode
                    .source_anchor
                    .map(|anchor| (anchor.row, anchor.column))
            )),
            Some((2, 4, Some((1, 2))))
        );
        assert_eq!(app.selected_text().as_deref(), Some("gh\nijkl"));
    }

    #[test]
    fn window_copy_mode_shift_o_moves_to_selection_other_horizontal_end() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"abcd\r\nefgh\r\nijkl").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("v".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("k".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty()));

        assert!(app.handle_copy_mode_key(&Key::Character("O".into()), ModifiersState::SHIFT));

        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| (
                copy_mode.source_cursor.row,
                copy_mode.source_cursor.column,
                copy_mode
                    .source_anchor
                    .map(|anchor| (anchor.row, anchor.column))
            )),
            Some((1, 4, Some((2, 2))))
        );
    }

    #[test]
    fn window_copy_mode_ctrl_v_uses_block_selection() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(5, 3));
        app.handle_pty_output(b"abcde\r\nfghij\r\nklmno").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("v".into()), ModifiersState::CONTROL));
        assert!(app.handle_copy_mode_key(&Key::Character("k".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("k".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("h".into()), ModifiersState::empty()));

        assert_eq!(app.selected_text().as_deref(), Some("cde\nhij\nmno"));
        assert!(snapshot_cell(&app.snapshot, 0, 2).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 1, 2).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 1, 4).unwrap().inverse);
        assert!(!snapshot_cell(&app.snapshot, 1, 1).unwrap().inverse);
    }

    #[test]
    fn window_copy_mode_vertical_movement_scrolls_across_scrollback() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee").unwrap();

        app.enter_copy_mode();
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("k".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("k".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 1);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "cc  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "dd  ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("j".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 1);
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("j".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "dd  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ee  ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );
    }

    #[test]
    fn window_copy_mode_page_movement_scrolls_across_scrollback() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee\nff").unwrap();

        app.enter_copy_mode();
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "ee  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ff  ");

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::PageUp), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 1);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "dd  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ee  ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::PageDown), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "ee  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ff  ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );
    }

    #[test]
    fn window_copy_mode_g_and_shift_g_move_to_scrollback_extents() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee\nff").unwrap();

        app.enter_copy_mode();
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "ee  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ff  ");

        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 4);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "aa  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "bb  ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("G".into()), ModifiersState::SHIFT));
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "ee  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ff  ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );
    }

    #[test]
    fn window_copy_mode_close_scrolls_to_bottom_before_exiting() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee\nff").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 4);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "aa  ");

        assert!(app.handle_copy_mode_key(&Key::Character("q".into()), ModifiersState::empty()));

        assert!(app.copy_mode.is_none());
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "ee  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ff  ");
    }

    #[test]
    fn window_copy_mode_carriage_return_moves_to_start_of_next_line() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 3));
        app.handle_pty_output(b"abcd\nefgh\nijkl").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("\r".into()), ModifiersState::empty()));

        assert_eq!(
            app.copy_mode
                .as_ref()
                .map(|copy_mode| (copy_mode.source_cursor.row, copy_mode.source_cursor.column)),
            Some((1, 0))
        );
    }

    #[test]
    fn window_copy_mode_uppercase_no_modifier_uses_wezterm_default_bindings() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 4));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee\nff").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 2);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "aa  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 3, 4), "dd  ");

        assert!(app.handle_copy_mode_key(&Key::Character("G".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "cc  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 3, 4), "ff  ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 3, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("H".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("M".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("L".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 3, column: 0 })
        );
    }

    #[test]
    fn window_copy_mode_line_content_movement_uses_non_space_cells() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 2));
        app.handle_pty_output(b"  aa  \n  bb  \n  cc  ").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 1);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "  aa    ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 6 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("^".into()), ModifiersState::SHIFT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("$".into()), ModifiersState::SHIFT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 3 })
        );
    }

    #[test]
    fn window_copy_mode_end_uses_line_content_end_binding() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 2));
        app.handle_pty_output(b"  aa  \n  bb  \n  cc  ").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 6 })
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::End), ModifiersState::empty()));

        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 3 })
        );
    }

    #[test]
    fn window_copy_mode_alt_m_uses_line_content_start_binding() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 2));
        app.handle_pty_output(b"  aa  \n  bb  \n  cc  ").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 6 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("m".into()), ModifiersState::ALT));

        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );
    }

    #[test]
    fn window_copy_mode_word_movement_uses_wezterm_default_bindings() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(24, 1));
        app.handle_pty_output(b"  alpha  beta gamma  ").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("^".into()), ModifiersState::SHIFT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("w".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 9 })
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::Tab), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 14 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("e".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 18 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("b".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 14 })
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::Tab), ModifiersState::SHIFT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 9 })
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::ArrowRight), ModifiersState::ALT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 14 })
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::ArrowLeft), ModifiersState::ALT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 9 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("f".into()), ModifiersState::ALT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 14 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("b".into()), ModifiersState::ALT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 9 })
        );
    }

    #[test]
    fn window_copy_mode_word_movement_crosses_scrollback_rows() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 2));
        app.handle_pty_output(b"aa bb\n  cc dd\n  ee").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("g".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 1);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "aa bb   ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 4 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("w".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("e".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 3 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("b".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 1, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("b".into()), ModifiersState::empty()));
        assert_eq!(app.scrollback_offset, 1);
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 3 })
        );
    }

    #[test]
    fn window_copy_mode_jump_forward_repeat_and_reverse_use_wezterm_bindings() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 1));
        app.handle_pty_output(b"abacad").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("0".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("f".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("a".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character(";".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 4 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character(",".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("t".into()), ModifiersState::empty()));
        assert!(app.handle_copy_mode_key(&Key::Character("d".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 4 })
        );
    }

    #[test]
    fn window_copy_mode_jump_backward_uses_wezterm_bindings() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 1));
        app.handle_pty_output(b"abacad").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("$".into()), ModifiersState::SHIFT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 5 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("F".into()), ModifiersState::SHIFT));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 5 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("a".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 4 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("T".into()), ModifiersState::SHIFT));
        assert!(app.handle_copy_mode_key(&Key::Character("b".into()), ModifiersState::empty()));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 2 })
        );
    }

    #[test]
    fn window_copy_mode_search_keeps_copy_mode_and_steps_matches() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"foo one\r\nmiddle\r\nfoo two")
            .unwrap();

        app.enter_copy_mode();
        assert!(app.copy_mode.is_some());
        assert!(app.handle_copy_mode_key(&Key::Character("/".into()), ModifiersState::empty()));
        assert!(app.copy_mode.is_some());
        assert!(app.search.is_some());

        for character in ["f", "o", "o"] {
            assert!(
                app.handle_copy_mode_key(
                    &Key::Character(character.into()),
                    ModifiersState::empty()
                )
            );
        }

        assert_eq!(app.selected_text().as_deref(), Some("foo"));
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('f'));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );

        assert!(
            app.handle_copy_mode_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty())
        );
        assert_eq!(app.selected_text().as_deref(), Some("foo"));
        assert_eq!(snapshot_char(&app.snapshot, 2, 0), Some('f'));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::ArrowUp), ModifiersState::empty()));
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('f'));

        assert!(app.handle_copy_mode_key(&Key::Character("n".into()), ModifiersState::CONTROL));
        assert_eq!(snapshot_char(&app.snapshot, 2, 0), Some('f'));

        assert!(app.handle_copy_mode_key(&Key::Character("p".into()), ModifiersState::CONTROL));
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('f'));
    }

    #[test]
    fn window_copy_mode_search_carriage_return_uses_prior_match_binding() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"foo one\r\nmiddle\r\nfoo two")
            .unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("/".into()), ModifiersState::empty()));
        for character in ["f", "o", "o"] {
            assert!(
                app.handle_copy_mode_key(
                    &Key::Character(character.into()),
                    ModifiersState::empty()
                )
            );
        }
        assert!(
            app.handle_copy_mode_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty())
        );
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 2, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Character("\r".into()), ModifiersState::empty()));

        assert_eq!(
            app.search.as_ref().map(|search| search.query.as_str()),
            Some("foo")
        );
        assert_eq!(app.selected_text().as_deref(), Some("foo"));
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );
    }

    #[test]
    fn window_copy_mode_search_escape_character_closes_copy_mode() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"foo one\r\nmiddle\r\nfoo two")
            .unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("/".into()), ModifiersState::empty()));
        assert!(app.copy_mode.is_some());
        assert!(app.search.is_some());

        assert!(
            app.handle_copy_mode_key(&Key::Character("\u{1b}".into()), ModifiersState::empty())
        );

        assert!(app.copy_mode.is_none());
        assert!(app.search.is_none());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1]"
        );
    }

    #[test]
    fn window_copy_mode_search_page_navigation_skips_visible_page_matches() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"foo 0\r\nfoo 1\r\nfoo 2\r\nfoo 3\r\nfoo 4")
            .unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("/".into()), ModifiersState::empty()));
        for character in ["f", "o", "o"] {
            assert!(
                app.handle_copy_mode_key(
                    &Key::Character(character.into()),
                    ModifiersState::empty()
                )
            );
        }

        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo 0   ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::PageDown), ModifiersState::empty()));
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo 2   ");
        assert_eq!(
            app.copy_mode
                .as_ref()
                .map(|copy_mode| (copy_mode.source_cursor.row, copy_mode.source_cursor.column)),
            Some((3, 0))
        );

        assert!(app.handle_copy_mode_key(&Key::Named(NamedKey::PageUp), ModifiersState::empty()));
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo 0   ");
        assert_eq!(
            app.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
            Some(SelectionCell { row: 0, column: 0 })
        );
    }

    #[test]
    fn window_copy_mode_search_cycles_match_type() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"foo\r\nFOO\r\nfao").unwrap();

        app.enter_copy_mode();
        assert!(app.handle_copy_mode_key(&Key::Character("/".into()), ModifiersState::empty()));
        for character in ["f", "o", "o"] {
            assert!(
                app.handle_copy_mode_key(
                    &Key::Character(character.into()),
                    ModifiersState::empty()
                )
            );
        }

        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo     ");
        assert_eq!(app.selected_text().as_deref(), Some("foo"));
        assert!(
            app.handle_copy_mode_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty())
        );
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo     ");

        assert!(app.handle_copy_mode_key(&Key::Character("r".into()), ModifiersState::CONTROL));
        assert!(
            app.handle_copy_mode_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty())
        );
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 8), "FOO     ");
        assert_eq!(
            app.copy_mode
                .as_ref()
                .map(|copy_mode| (copy_mode.source_cursor.row, copy_mode.source_cursor.column)),
            Some((1, 0))
        );

        assert!(app.handle_copy_mode_key(&Key::Character("r".into()), ModifiersState::CONTROL));
        assert!(app.handle_copy_mode_key(&Key::Character("u".into()), ModifiersState::CONTROL));
        for character in ["f", ".", "o"] {
            assert!(
                app.handle_copy_mode_key(
                    &Key::Character(character.into()),
                    ModifiersState::empty()
                )
            );
        }

        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo     ");
        assert!(
            app.handle_copy_mode_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty())
        );
        assert_eq!(snapshot_row_text(&app.snapshot, 2, 8), "fao     ");
        assert_eq!(
            app.copy_mode
                .as_ref()
                .map(|copy_mode| (copy_mode.source_cursor.row, copy_mode.source_cursor.column)),
            Some((2, 0))
        );
    }

    #[test]
    fn window_copy_mode_clears_other_overlays() {
        let mut app = NativeWindowApp::new(None);
        app.update_search_query("example");
        assert!(app.search.is_some());
        assert!(app.copy_mode.is_none());

        app.enter_copy_mode();
        assert!(app.search.is_none());
        assert!(app.copy_mode.is_some());

        app.enter_quick_select_mode();
        assert!(app.copy_mode.is_none());
        assert!(app.quick_select.is_some());
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

        app.handle_cursor_moved(PhysicalPosition::new(
            0.0,
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(super::CELL_WIDTH * 2),
            f64::from(tab_bar_pixel_height()),
        ))
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
    fn window_app_double_click_selects_word() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(16, 1));
        app.handle_pty_output(b"run alpha-beta").unwrap();

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(super::CELL_WIDTH * 6),
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );
        assert!(
            app.handle_mouse_input(ElementState::Released, MouseButton::Left)
                .unwrap()
        );
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        assert_eq!(
            app.selection,
            Some(WindowSelection::new(
                SelectionCell { row: 0, column: 4 },
                SelectionCell { row: 0, column: 13 },
            ))
        );
        assert!(snapshot_cell(&app.snapshot, 0, 4).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 0, 13).unwrap().inverse);
    }

    #[test]
    fn window_app_triple_click_selects_line() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(16, 1));
        app.handle_pty_output(b"run alpha-beta").unwrap();

        app.handle_cursor_moved(PhysicalPosition::new(
            f64::from(super::CELL_WIDTH * 6),
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );
        app.handle_mouse_input(ElementState::Released, MouseButton::Left)
            .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );
        app.handle_mouse_input(ElementState::Released, MouseButton::Left)
            .unwrap();
        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );

        assert_eq!(
            app.selection,
            Some(WindowSelection::new(
                SelectionCell { row: 0, column: 0 },
                SelectionCell { row: 0, column: 15 },
            ))
        );
        assert_eq!(app.selected_text().as_deref(), Some("run alpha-beta"));
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(
            app.selection
                .unwrap()
                .contains(0, 15, app.runtime.terminal().grid().size())
        );
    }

    #[test]
    fn window_app_ctrl_click_opens_hyperlink_cell() {
        let opened = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&opened);
        let mut app = NativeWindowApp::new(None);
        app.hyperlink_opener = Box::new(move |url: &str| {
            recorded.lock().unwrap().push(url.to_owned());
            true
        });
        app.runtime.resize(rssh_core::TerminalSize::new(8, 1));
        app.handle_pty_output(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\")
            .unwrap();

        app.modifiers = ModifiersState::CONTROL;
        app.handle_cursor_moved(PhysicalPosition::new(
            0.0,
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();

        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );
        assert_eq!(opened.lock().unwrap().as_slice(), ["https://example.com"]);
        assert!(app.selection.is_none());
        assert!(!app.selecting);
    }

    #[test]
    fn window_app_open_uri_hook_can_prevent_default_hyperlink_open() {
        let open_uris = Arc::new(Mutex::new(Vec::new()));
        let recorded_uri = Arc::clone(&open_uris);
        let opened = Arc::new(Mutex::new(Vec::new()));
        let recorded_open = Arc::clone(&opened);
        let mut app = NativeWindowApp::new(None);
        app.open_uri_handler = Box::new(move |event| {
            recorded_uri.lock().unwrap().push(event.clone());
            false
        });
        app.hyperlink_opener = Box::new(move |url: &str| {
            recorded_open.lock().unwrap().push(url.to_owned());
            true
        });
        let active_pane = app.app_shell.active_pane_id();
        app.runtime.resize(rssh_core::TerminalSize::new(8, 1));
        app.handle_pty_output(b"\x1b]8;;mailto:ops@example.com\x1b\\mail\x1b]8;;\x1b\\")
            .unwrap();

        app.modifiers = ModifiersState::CONTROL;
        app.handle_cursor_moved(PhysicalPosition::new(
            0.0,
            f64::from(tab_bar_pixel_height()),
        ))
        .unwrap();

        assert!(
            app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
                .unwrap()
        );
        assert_eq!(
            open_uris.lock().unwrap().as_slice(),
            [NativeWindowOpenUri {
                pane: active_pane,
                uri: "mailto:ops@example.com".to_owned(),
            }]
        );
        assert!(opened.lock().unwrap().is_empty());
        assert!(app.selection.is_none());
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
    fn window_search_prefills_current_selection_first_line() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"alpha\r\nbeta\r\nalpha 2").unwrap();
        app.selection = Some(WindowSelection::new(
            SelectionCell { row: 0, column: 0 },
            SelectionCell { row: 1, column: 3 },
        ));

        assert_eq!(app.selected_text().as_deref(), Some("alpha\nbeta"));

        app.enter_search_mode();

        assert_eq!(
            app.search.as_ref().map(|search| search.query.as_str()),
            Some("alpha")
        );
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(!snapshot_cell(&app.snapshot, 1, 0).unwrap().inverse);
    }

    #[test]
    fn window_search_finds_match_across_scrollback_and_grid_rows() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(6, 2));
        app.handle_pty_output(b"alpha\r\nbeta\r\ngamma").unwrap();

        assert!(app.update_search_query("habeta"));

        assert_eq!(app.selected_text().as_deref(), Some("ha\nbeta"));
        assert_eq!(snapshot_char(&app.snapshot, 0, 3), Some('h'));
        assert_eq!(snapshot_char(&app.snapshot, 1, 0), Some('b'));
        assert!(snapshot_cell(&app.snapshot, 0, 3).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 1, 3).unwrap().inverse);
        assert_eq!(app.scrollback_offset, 1);
    }

    #[test]
    fn window_search_supports_regex_prefix_across_terminal_rows() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(6, 2));
        app.handle_pty_output(b"alpha\r\nbeta\r\ngamma").unwrap();

        assert!(app.update_search_query("regex:h.*beta"));

        assert_eq!(app.selected_text().as_deref(), Some("ha\nbeta"));
        assert_eq!(snapshot_char(&app.snapshot, 0, 3), Some('h'));
        assert_eq!(snapshot_char(&app.snapshot, 1, 3), Some('a'));
        assert!(snapshot_cell(&app.snapshot, 0, 3).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 1, 3).unwrap().inverse);
        assert_eq!(app.scrollback_offset, 1);
    }

    #[test]
    fn window_search_ignores_zero_width_regex_matches() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(5, 1));
        app.handle_pty_output(b"ab cd").unwrap();

        assert!(!app.update_search_query("regex:\\b"));

        assert!(app.selection.is_none());
        assert_eq!(
            app.effective_window_title(),
            "R-SSH [workspace:1 tab:1 pane:1] - Search: regex:\\b (no match)"
        );
    }

    #[test]
    fn window_search_supports_literal_prefix_for_regex_like_text() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(16, 1));
        app.handle_pty_output(b"regex:h.*beta").unwrap();

        assert!(app.update_search_query("literal:regex:h.*beta"));

        assert_eq!(app.selected_text().as_deref(), Some("regex:h.*beta"));
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 0, 12).unwrap().inverse);
    }

    #[test]
    fn window_search_literal_prefix_stays_literal_in_regex_match_type() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 2));
        app.handle_pty_output(b"foo\r\nf.o").unwrap();

        assert!(app.update_search_query_with_type(
            "literal:f.o",
            SearchDirection::Next,
            WindowSearchMatchType::Regex
        ));

        assert_eq!(app.selected_text().as_deref(), Some("f.o"));
        assert!(snapshot_cell(&app.snapshot, 1, 0).unwrap().inverse);
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
    fn window_search_uses_wezterm_search_mode_navigation_bindings() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"foo one\r\nmiddle\r\nfoo two")
            .unwrap();

        assert!(app.update_search_query("foo"));
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(!snapshot_cell(&app.snapshot, 2, 0).unwrap().inverse);

        assert!(app.handle_search_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty()));
        assert!(!snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 2, 0).unwrap().inverse);

        assert!(app.handle_search_key(&Key::Named(NamedKey::ArrowUp), ModifiersState::empty()));
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(!snapshot_cell(&app.snapshot, 2, 0).unwrap().inverse);

        assert!(app.handle_search_key(&Key::Character("n".into()), ModifiersState::CONTROL));
        assert!(!snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 2, 0).unwrap().inverse);

        assert!(app.handle_search_key(&Key::Character("p".into()), ModifiersState::CONTROL));
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(!snapshot_cell(&app.snapshot, 2, 0).unwrap().inverse);
    }

    #[test]
    fn window_search_uses_wezterm_search_mode_page_bindings() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 3));
        app.handle_pty_output(b"foo 0\r\nfoo 1\r\nfoo 2\r\nfoo 3\r\nfoo 4")
            .unwrap();

        assert!(app.update_search_query("foo"));
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo 0   ");
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);

        assert!(app.handle_search_key(&Key::Named(NamedKey::PageDown), ModifiersState::empty()));
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo 2   ");
        assert!(snapshot_cell(&app.snapshot, 1, 0).unwrap().inverse);

        assert!(app.handle_search_key(&Key::Named(NamedKey::PageUp), ModifiersState::empty()));
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "foo 0   ");
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
    }

    #[test]
    fn window_search_uses_wezterm_search_mode_pattern_bindings() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 2));
        app.handle_pty_output(b"foo\r\nFOO").unwrap();

        app.enter_search_mode();
        assert!(app.handle_search_key(&Key::Character("f".into()), ModifiersState::empty()));
        assert!(app.handle_search_key(&Key::Character("o".into()), ModifiersState::empty()));
        assert!(app.handle_search_key(&Key::Character("o".into()), ModifiersState::empty()));
        assert_eq!(
            app.search.as_ref().map(|search| search.query.as_str()),
            Some("foo")
        );
        assert!(snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(!snapshot_cell(&app.snapshot, 1, 0).unwrap().inverse);

        assert!(app.handle_search_key(&Key::Character("r".into()), ModifiersState::CONTROL));
        assert!(app.handle_search_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty()));
        assert!(!snapshot_cell(&app.snapshot, 0, 0).unwrap().inverse);
        assert!(snapshot_cell(&app.snapshot, 1, 0).unwrap().inverse);

        assert!(app.handle_search_key(&Key::Character("u".into()), ModifiersState::CONTROL));
        assert_eq!(
            app.search.as_ref().map(|search| search.query.as_str()),
            Some("")
        );
        assert!(app.selection.is_none());

        assert!(app.handle_search_key(&Key::Character("\u{1b}".into()), ModifiersState::empty()));
        assert!(app.search.is_none());
    }

    #[test]
    fn recognizes_window_search_shortcuts() {
        assert!(window_search_shortcut(
            &Key::Character("f".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(window_search_shortcut(
            &Key::Character("F".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
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
    fn recognizes_window_clear_scrollback_shortcut() {
        assert!(window_clear_scrollback_shortcut(
            &Key::Character("k".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(window_clear_scrollback_shortcut(
            &Key::Character("K".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(!window_clear_scrollback_shortcut(
            &Key::Character("k".into()),
            ModifiersState::CONTROL
        ));
    }

    #[test]
    fn recognizes_window_quick_select_shortcut() {
        assert!(window_quick_select_shortcut(
            &Key::Named(NamedKey::Space),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(!window_quick_select_shortcut(
            &Key::Named(NamedKey::Space),
            ModifiersState::SHIFT
        ));
    }

    #[test]
    fn recognizes_new_tab_shortcut() {
        let app = NativeWindowApp::new_with_command(
            None,
            rssh_pty::PtyCommand::new("powershell").with_args(["-NoProfile"]),
        );

        assert!(
            app.app_shell_action_for_key(
                &Key::Character("T".into()),
                ModifiersState::CONTROL | ModifiersState::SHIFT
            )
            .is_some()
        );
    }

    #[test]
    fn recognizes_default_tab_navigation_shortcuts() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::ActivateTab {
            tab: rssh_core::TabId::new(2),
        })
        .unwrap();

        let action =
            app.app_shell_action_for_key(&Key::Named(NamedKey::Tab), ModifiersState::CONTROL);
        let AppAction::ActivateTabRelative { offset } = action.expect("expected activate next tab")
        else {
            panic!("expected activate tab relative");
        };
        assert_eq!(offset, 1);

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::Tab),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::ActivateTabRelative { offset } =
            action.expect("expected activate previous tab")
        else {
            panic!("expected activate tab relative");
        };
        assert_eq!(offset, -1);

        let action =
            app.app_shell_action_for_key(&Key::Named(NamedKey::PageUp), ModifiersState::CONTROL);
        let AppAction::ActivateTabRelative { offset } =
            action.expect("expected activate previous tab")
        else {
            panic!("expected activate tab relative");
        };
        assert_eq!(offset, -1);

        let action =
            app.app_shell_action_for_key(&Key::Named(NamedKey::PageDown), ModifiersState::CONTROL);
        let AppAction::ActivateTabRelative { offset } = action.expect("expected activate next tab")
        else {
            panic!("expected activate tab relative");
        };
        assert_eq!(offset, 1);
    }

    #[test]
    fn recognizes_default_tab_move_shortcuts() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::ActivateTab {
            tab: rssh_core::TabId::new(1),
        })
        .unwrap();

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::PageDown),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::MoveTabRelative { offset } = action.expect("expected move tab right") else {
            panic!("expected move tab relative");
        };
        assert_eq!(offset, 1);

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::PageUp),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::MoveTabRelative { offset } = action.expect("expected move tab left") else {
            panic!("expected move tab relative");
        };
        assert_eq!(offset, -1);
    }

    #[test]
    fn recognizes_default_tab_move_shortcuts_with_alt() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::ActivateTab {
            tab: rssh_core::TabId::new(1),
        })
        .unwrap();

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::PageDown),
            ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT,
        );
        let AppAction::MoveTabRelative { offset } = action.expect("expected move tab right") else {
            panic!("expected move tab relative");
        };
        assert_eq!(offset, 1);

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::PageUp),
            ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT,
        );
        let AppAction::MoveTabRelative { offset } = action.expect("expected move tab left") else {
            panic!("expected move tab relative");
        };
        assert_eq!(offset, -1);
    }

    #[test]
    fn recognizes_default_tab_number_shortcuts() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();

        let action = app.app_shell_action_for_key(
            &Key::Character("1".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::ActivateTabIndex { index } = action.expect("expected activate tab 1") else {
            panic!("expected activate tab index");
        };
        assert_eq!(index, 0);

        let action = app.app_shell_action_for_key(
            &Key::Character("2".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::ActivateTabIndex { index } = action.expect("expected activate tab 2") else {
            panic!("expected activate tab index");
        };
        assert_eq!(index, 1);

        let action = app.app_shell_action_for_key(
            &Key::Character("(".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::ActivateTabIndex { index } = action.expect("expected activate last tab")
        else {
            panic!("expected activate tab index");
        };
        assert_eq!(index, -1);
    }

    #[test]
    fn recognizes_default_alt_split_shortcuts() {
        let app = NativeWindowApp::new(None);

        let action = app.app_shell_action_for_key(
            &Key::Character("\"".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT,
        );
        let AppAction::SplitPane { direction, .. } = action.expect("expected split pane action")
        else {
            panic!("expected split pane action");
        };
        assert_eq!(direction, rssh_core::app_shell::SplitDirection::Right);

        let action = app.app_shell_action_for_key(
            &Key::Character("%".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT,
        );
        let AppAction::SplitPane { direction, .. } = action.expect("expected split pane action")
        else {
            panic!("expected split pane action");
        };
        assert_eq!(direction, rssh_core::app_shell::SplitDirection::Down);
    }

    #[test]
    fn recognizes_default_pane_navigation_shortcuts() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::ArrowLeft),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::ActivatePaneDirection { direction } =
            action.expect("expected activate pane left")
        else {
            panic!("expected activate pane direction");
        };
        assert_eq!(direction, rssh_core::app_shell::PaneDirection::Left);

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::ArrowRight),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::ActivatePaneDirection { direction } =
            action.expect("expected activate pane right")
        else {
            panic!("expected activate pane direction");
        };
        assert_eq!(direction, rssh_core::app_shell::PaneDirection::Right);

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::ArrowUp),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::ActivatePaneDirection { direction } =
            action.expect("expected activate pane up")
        else {
            panic!("expected activate pane direction");
        };
        assert_eq!(direction, rssh_core::app_shell::PaneDirection::Up);

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::ArrowDown),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::ActivatePaneDirection { direction } =
            action.expect("expected activate pane down")
        else {
            panic!("expected activate pane direction");
        };
        assert_eq!(direction, rssh_core::app_shell::PaneDirection::Down);
    }

    #[test]
    fn recognizes_default_pane_resize_shortcuts() {
        let app = NativeWindowApp::new(None);

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::ArrowLeft),
            ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT,
        );
        let AppAction::ResizePane {
            direction, amount, ..
        } = action.expect("expected resize pane")
        else {
            panic!("expected resize pane");
        };
        assert_eq!(direction, rssh_core::app_shell::ResizeDirection::Left);
        assert_eq!(amount, 1);

        let action = app.app_shell_action_for_key(
            &Key::Named(NamedKey::ArrowDown),
            ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT,
        );
        let AppAction::ResizePane { direction, .. } = action.expect("expected resize pane") else {
            panic!("expected resize pane");
        };
        assert_eq!(direction, rssh_core::app_shell::ResizeDirection::Down);
    }

    #[test]
    fn recognizes_default_pane_zoom_shortcut() {
        let app = NativeWindowApp::new(None);

        let action = app.app_shell_action_for_key(
            &Key::Character("Z".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );

        let AppAction::TogglePaneZoom { pane } = action.expect("expected toggle pane zoom") else {
            panic!("expected toggle pane zoom");
        };
        assert_eq!(pane, rssh_core::PaneId::new(1));
    }

    #[test]
    fn recognizes_window_command_palette_shortcut() {
        assert!(NativeWindowApp::command_palette_shortcut(
            &Key::Character("p".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ));
        assert!(!NativeWindowApp::command_palette_shortcut(
            &Key::Character("p".into()),
            ModifiersState::CONTROL
        ));
    }

    #[test]
    fn window_app_dispatches_palette_new_tab_command() {
        let mut app = NativeWindowApp::new_with_command(
            None,
            rssh_pty::PtyCommand::new("powershell").with_args(["-NoProfile"]),
        );

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::NewTab);

        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_activate_last_tab_command() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(3));

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ActivateLastTab);

        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_activate_tab_relative_commands() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(3));

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::NextTab);
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::PreviousTab);
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(3));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_activate_tab_no_wrap_commands() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(3));

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::NextTabNoWrap);
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(3));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::PreviousTabNoWrap);
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
        assert!(app.command_palette.is_none());

        app.dispatch_app_action(AppAction::ActivateTab {
            tab: rssh_core::TabId::new(1),
        })
        .unwrap();
        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::PreviousTabNoWrap);
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_move_tab_to_index_commands() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        app.dispatch_app_action(AppAction::ActivateTab {
            tab: rssh_core::TabId::new(1),
        })
        .unwrap();
        let tab_order = |app: &NativeWindowApp| -> Vec<rssh_core::TabId> {
            app.app_shell
                .active_workspace()
                .tabs()
                .iter()
                .map(rssh_core::app_shell::Tab::id)
                .collect()
        };

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::MoveTabTo3);
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
        assert_eq!(
            tab_order(&app),
            vec![
                rssh_core::TabId::new(2),
                rssh_core::TabId::new(3),
                rssh_core::TabId::new(1)
            ]
        );
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::MoveTabTo1);
        assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
        assert_eq!(
            tab_order(&app),
            vec![
                rssh_core::TabId::new(1),
                rssh_core::TabId::new(2),
                rssh_core::TabId::new(3)
            ]
        );
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        assert!(!app.command_palette_execute(WindowCommand::MoveTabTo4));
        assert_eq!(
            tab_order(&app),
            vec![
                rssh_core::TabId::new(1),
                rssh_core::TabId::new(2),
                rssh_core::TabId::new(3)
            ]
        );
        assert!(app.command_palette.is_some());
    }

    #[test]
    fn window_app_dispatches_palette_activate_pane_left_command() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ActivatePaneLeft);

        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_activate_pane_by_index_commands() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(2),
            direction: rssh_core::app_shell::SplitDirection::Down,
            launch: None,
        })
        .unwrap();
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(3));

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ActivatePane1);
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ActivatePane3);
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(3));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ActivatePane4);
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(3));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_rotate_panes_commands() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(2),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::RotatePanesClockwise);
        assert_eq!(
            app.app_shell
                .active_tab()
                .panes()
                .iter()
                .map(rssh_core::app_shell::Pane::id)
                .collect::<Vec<_>>(),
            vec![
                rssh_core::PaneId::new(3),
                rssh_core::PaneId::new(1),
                rssh_core::PaneId::new(2)
            ]
        );
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(3));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::RotatePanesCounterClockwise);
        assert_eq!(
            app.app_shell
                .active_tab()
                .panes()
                .iter()
                .map(rssh_core::app_shell::Pane::id)
                .collect::<Vec<_>>(),
            vec![
                rssh_core::PaneId::new(1),
                rssh_core::PaneId::new(2),
                rssh_core::PaneId::new(3)
            ]
        );
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(3));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_enter_copy_mode_command() {
        let mut app = NativeWindowApp::new(None);

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::EnterCopyMode);

        assert!(app.copy_mode.is_some());
        assert!(app.command_palette.is_none());
        assert!(app.search.is_none());
        assert!(app.quick_select.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_enter_quick_select_command() {
        let mut app = NativeWindowApp::new(None);

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::EnterQuickSelect);

        assert!(app.quick_select.is_some());
        assert!(app.command_palette.is_none());
        assert!(app.search.is_none());
        assert!(app.copy_mode.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_enter_pane_select_command() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::EnterPaneSelect);

        assert!(app.pane_select.is_some());
        assert!(app.command_palette.is_none());
        assert!(app.search.is_none());
        assert!(app.copy_mode.is_none());
        assert!(app.quick_select.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_enter_pane_swap_commands() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::EnterPaneSwap);
        assert_eq!(
            app.pane_select.as_ref().map(|pane_select| pane_select.mode),
            Some(WindowPaneSelectMode::SwapWithActive)
        );

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::EnterPaneSwapKeepFocus);
        assert_eq!(
            app.pane_select.as_ref().map(|pane_select| pane_select.mode),
            Some(WindowPaneSelectMode::SwapWithActiveKeepFocus)
        );
    }

    #[test]
    fn window_app_dispatches_palette_enter_pane_move_to_new_tab_command() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::EnterPaneMoveToNewTab);

        assert_eq!(
            app.pane_select.as_ref().map(|pane_select| pane_select.mode),
            Some(WindowPaneSelectMode::MoveToNewTab)
        );
    }

    #[test]
    fn window_app_dispatches_palette_enter_pane_move_to_new_window_command() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::EnterPaneMoveToNewWindow);

        assert_eq!(
            app.pane_select.as_ref().map(|pane_select| pane_select.mode),
            Some(WindowPaneSelectMode::MoveToNewWindow)
        );
    }

    #[test]
    fn window_app_palette_close_pane_requests_window_close_for_last_pane() {
        let mut app = NativeWindowApp::new(None);

        app.enter_command_palette_mode();

        assert!(app.command_palette_execute(WindowCommand::ClosePane));
        assert!(app.command_palette.is_none());
        assert!(app.window_close_requested_for_test());
    }

    #[test]
    fn window_app_palette_close_tab_requests_window_close_for_last_tab() {
        let mut app = NativeWindowApp::new(None);

        app.enter_command_palette_mode();

        assert!(app.command_palette_execute(WindowCommand::CloseTab));
        assert!(app.command_palette.is_none());
        assert!(app.window_close_requested_for_test());
    }

    #[test]
    fn window_app_dispatches_palette_close_workspace_command() {
        let mut app = NativeWindowApp::new(None);

        app.dispatch_app_action(AppAction::NewWorkspace {
            name: "next".to_owned(),
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::CloseWorkspace);

        assert_eq!(app.app_shell.workspaces().len(), 1);
        assert_eq!(app.active_workspace_id(), rssh_core::WorkspaceId::new(1));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_rejects_palette_close_workspace_with_single_workspace() {
        let mut app = NativeWindowApp::new(None);

        app.enter_command_palette_mode();
        let error = app
            .command_palette_apply_command(WindowCommand::CloseWorkspace)
            .unwrap_err();

        assert_eq!(error, AppShellError::CannotCloseLastWorkspace);
        assert_eq!(app.app_shell.workspaces().len(), 1);
        assert!(app.command_palette.is_some());
    }

    #[test]
    fn window_app_dispatches_palette_enter_search_command() {
        let mut app = NativeWindowApp::new(None);

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::EnterSearch);

        assert!(app.search.is_some());
        assert!(app.command_palette.is_none());
        assert!(app.copy_mode.is_none());
        assert!(app.quick_select.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_clear_scrollback_command() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut app = NativeWindowApp::new(None);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"ab\ncd\nef").unwrap();
        assert_eq!(app.runtime.terminal().scrollback().len(), 1);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "cd  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ef  ");
        app.scrollback_offset = 1;
        app.refresh_snapshot();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ClearScrollback);

        assert!(app.command_palette.is_none());
        assert_eq!(app.scrollback_offset, 0);
        assert!(app.runtime.terminal().scrollback().is_empty());
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "cd  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ef  ");
        assert!(written.lock().unwrap().is_empty());
    }

    #[test]
    fn window_app_dispatches_palette_clear_scrollback_and_viewport_command() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut app = NativeWindowApp::new(None);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"ab\ncd\nef").unwrap();
        assert_eq!(app.runtime.terminal().scrollback().len(), 1);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "cd  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "ef  ");
        assert_eq!(app.runtime.terminal().cursor(), (1, 2));
        app.scrollback_offset = 1;
        app.refresh_snapshot();

        app.enter_command_palette_mode();
        app.command_palette_set_query("clear viewport".to_owned());
        let commands = app.command_palette_filtered_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].label(), "Clear Scrollback And Viewport");
        app.command_palette_execute(commands[0]);

        assert!(app.command_palette.is_none());
        assert_eq!(app.scrollback_offset, 0);
        assert!(app.runtime.terminal().scrollback().is_empty());
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "ef  ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "    ");
        assert_eq!(app.runtime.terminal().cursor(), (0, 2));
        assert!(written.lock().unwrap().is_empty());
    }

    #[test]
    fn window_app_dispatches_palette_reset_terminal_command() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\x1b[31;1m\x1b[?25l")
            .unwrap();
        assert!(!app.runtime.terminal().cursor_visible());
        assert!(!app.runtime.terminal().scrollback().is_empty());
        app.scrollback_offset = 1;
        app.refresh_snapshot();

        app.enter_command_palette_mode();
        app.command_palette_set_query("reset terminal".to_owned());
        let commands = app.command_palette_filtered_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].label(), "Reset Terminal");
        app.command_palette_execute(commands[0]);

        assert!(app.runtime.terminal().scrollback().is_empty());
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 4), "    ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 4), "    ");
        assert_eq!(app.runtime.terminal().cursor(), (0, 0));
        assert!(app.runtime.terminal().cursor_visible());
        let reset_cell = app.runtime.terminal().grid().get(0, 0).unwrap();
        assert_eq!(reset_cell.foreground, rssh_terminal::Color::Default);
        assert!(!reset_cell.bold);
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_scrollback_navigation_commands() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
        app.handle_pty_output(b"aa\nbb\ncc\ndd\nee").unwrap();
        assert_eq!(app.runtime.terminal().scrollback().len(), 3);

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollToTop);
        assert_eq!(app.scrollback_offset, 3);
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('a'));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollToBottom);
        assert_eq!(app.scrollback_offset, 0);
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('d'));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollPageUp);
        assert_eq!(app.scrollback_offset, 2);
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollLineDown);
        assert_eq!(app.scrollback_offset, 1);
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollLineUp);
        assert_eq!(app.scrollback_offset, 2);
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollPageDown);
        assert_eq!(app.scrollback_offset, 0);
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_scroll_to_prompt_commands() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(8, 2));
        app.handle_pty_output(
            b"\x1b]133;A\x07> one\nout1\n\x1b]133;A\x07> two\nout2\n\x1b]133;A\x07> three\nlive",
        )
        .unwrap();

        assert_eq!(app.runtime.terminal().scrollback().len(), 4);
        assert_eq!(app.runtime.terminal().semantic_prompt_rows(), &[0, 2, 4]);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "> three ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 8), "live    ");

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollToPreviousPrompt);

        assert_eq!(app.scrollback_offset, 2);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "> two   ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 8), "out2    ");
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollToPreviousPrompt);

        assert_eq!(app.scrollback_offset, 4);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "> one   ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 8), "out1    ");
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ScrollToNextPrompt);

        assert_eq!(app.scrollback_offset, 2);
        assert_eq!(snapshot_row_text(&app.snapshot, 0, 8), "> two   ");
        assert_eq!(snapshot_row_text(&app.snapshot, 1, 8), "out2    ");
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_clear_selection_command() {
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();
        app.selection = Some(WindowSelection::new(
            SelectionCell { row: 0, column: 1 },
            SelectionCell { row: 0, column: 2 },
        ));
        app.refresh_snapshot();
        assert!(snapshot_cell(&app.snapshot, 0, 1).unwrap().inverse);

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ClearSelection);

        assert!(app.selection.is_none());
        assert!(!snapshot_cell(&app.snapshot, 0, 1).unwrap().inverse);
        assert!(!snapshot_cell(&app.snapshot, 0, 2).unwrap().inverse);
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_copy_to_clipboard_command() {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&copied);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();
        app.selection = Some(WindowSelection::new(
            SelectionCell { row: 0, column: 1 },
            SelectionCell { row: 0, column: 2 },
        ));
        app.refresh_snapshot();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_command_palette_mode();
        app.command_palette_set_query("copy clipboard".to_owned());
        let commands = app.command_palette_filtered_commands();
        let command = commands
            .into_iter()
            .find(|command| command.label() == "Copy To Clipboard")
            .expect("expected clipboard copy command");
        app.command_palette_execute(command);

        assert_eq!(copied.lock().unwrap().as_slice(), ["bc"]);
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_copy_to_primary_selection_command() {
        let primary_copied = Arc::new(Mutex::new(Vec::new()));
        let recorded_primary = Arc::clone(&primary_copied);
        let clipboard_copied = Arc::new(Mutex::new(Vec::new()));
        let recorded_clipboard = Arc::clone(&clipboard_copied);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();
        app.selection = Some(WindowSelection::new(
            SelectionCell { row: 0, column: 1 },
            SelectionCell { row: 0, column: 2 },
        ));
        app.refresh_snapshot();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded_clipboard.lock().unwrap().push(text.to_owned());
            true
        });
        app.primary_selection_writer = Box::new(move |text: &str| {
            recorded_primary.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_command_palette_mode();
        app.command_palette_set_query("copy primary".to_owned());
        let commands = app.command_palette_filtered_commands();
        let command = commands
            .into_iter()
            .find(|command| command.label() == "Copy To Primary Selection")
            .expect("expected primary selection copy command");
        app.command_palette_execute(command);

        assert!(clipboard_copied.lock().unwrap().is_empty());
        assert_eq!(primary_copied.lock().unwrap().as_slice(), ["bc"]);
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_copy_to_clipboard_and_primary_selection_command() {
        let clipboard_copied = Arc::new(Mutex::new(Vec::new()));
        let recorded_clipboard = Arc::clone(&clipboard_copied);
        let primary_copied = Arc::new(Mutex::new(Vec::new()));
        let recorded_primary = Arc::clone(&primary_copied);
        let mut app = NativeWindowApp::new(None);
        app.runtime.resize(rssh_core::TerminalSize::new(4, 1));
        app.handle_pty_output(b"abcd").unwrap();
        app.selection = Some(WindowSelection::new(
            SelectionCell { row: 0, column: 1 },
            SelectionCell { row: 0, column: 2 },
        ));
        app.refresh_snapshot();
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded_clipboard.lock().unwrap().push(text.to_owned());
            true
        });
        app.primary_selection_writer = Box::new(move |text: &str| {
            recorded_primary.lock().unwrap().push(text.to_owned());
            true
        });

        app.enter_command_palette_mode();
        app.command_palette_set_query("copy clipboard primary".to_owned());
        let commands = app.command_palette_filtered_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].label(),
            "Copy To Clipboard And Primary Selection"
        );
        app.command_palette_execute(commands[0]);

        assert_eq!(clipboard_copied.lock().unwrap().as_slice(), ["bc"]);
        assert_eq!(primary_copied.lock().unwrap().as_slice(), ["bc"]);
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_paste_from_clipboard_command() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut app = NativeWindowApp::new(None);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));
        app.clipboard_reader = Box::new(|| Some("paste\ntext".to_owned()));

        app.enter_command_palette_mode();
        app.command_palette_set_query("paste clipboard".to_owned());
        let commands = app.command_palette_filtered_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].label(), "Paste From Clipboard");
        app.command_palette_execute(commands[0]);

        assert_eq!(written.lock().unwrap().as_slice(), b"paste\ntext");
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_paste_from_primary_selection_command() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut app = NativeWindowApp::new(None);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));
        app.clipboard_reader = Box::new(|| Some("clipboard".to_owned()));
        app.primary_selection_reader = Box::new(|| Some("primary\ntext".to_owned()));

        app.enter_command_palette_mode();
        app.command_palette_set_query("paste primary".to_owned());
        let commands = app.command_palette_filtered_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].label(), "Paste From Primary Selection");
        app.command_palette_execute(commands[0]);

        assert_eq!(written.lock().unwrap().as_slice(), b"primary\ntext");
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_rename_workspace_command() {
        let mut app = NativeWindowApp::new(None);

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::RenameWorkspace);

        assert_eq!(app.app_shell.active_workspace().name(), "default (renamed)");
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_rename_tab_command() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"\x1b]2;PowerShell\x07").unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::RenameTab);

        assert_eq!(
            app.app_shell.active_tab().title(),
            Some("PowerShell (renamed)")
        );
        assert!(app.command_palette.is_none());

        let snapshot = app.render_snapshot();
        let tab_bar = snapshot_row_text(&snapshot, 0, TERMINAL_COLUMNS);
        assert!(
            tab_bar.contains("PowerShell (renamed)"),
            "tab bar was {tab_bar:?}"
        );
    }

    #[test]
    fn window_app_dispatches_palette_rename_tab_command_with_query_title() {
        let mut app = NativeWindowApp::new(None);
        app.handle_pty_output(b"\x1b]2;PowerShell\x07").unwrap();

        app.enter_command_palette_mode();
        app.command_palette_set_query("rename tab build-prod".to_owned());
        let commands = app.command_palette_filtered_commands();
        let command = commands.first().copied().expect("expected rename command");
        app.command_palette_execute(command);

        assert_eq!(app.app_shell.active_tab().title(), Some("build-prod"));
        assert!(app.command_palette.is_none());

        let snapshot = app.render_snapshot();
        let tab_bar = snapshot_row_text(&snapshot, 0, TERMINAL_COLUMNS);
        assert!(tab_bar.contains("build-prod"), "tab bar was {tab_bar:?}");
        assert!(!tab_bar.contains("PowerShell"), "tab bar was {tab_bar:?}");
    }

    #[test]
    fn window_app_dispatches_palette_resize_pane_command() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ResizePaneLeft);

        let split = app.app_shell.active_tab().panes()[1]
            .split()
            .expect("split should be present");
        assert_eq!(split.source_size_delta, -1);
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_toggle_pane_zoom_command() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::TogglePaneZoom);

        assert_eq!(
            app.app_shell.active_tab().zoomed_pane_id(),
            Some(rssh_core::PaneId::new(2))
        );
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn window_app_dispatches_palette_set_pane_zoom_state_commands() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: rssh_core::app_shell::SplitDirection::Right,
            launch: None,
        })
        .unwrap();

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ZoomPane);
        assert_eq!(
            app.app_shell.active_tab().zoomed_pane_id(),
            Some(rssh_core::PaneId::new(2))
        );
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::ZoomPane);
        assert_eq!(
            app.app_shell.active_tab().zoomed_pane_id(),
            Some(rssh_core::PaneId::new(2))
        );
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::UnzoomPane);
        assert_eq!(app.app_shell.active_tab().zoomed_pane_id(), None);
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));
        assert!(app.command_palette.is_none());

        app.enter_command_palette_mode();
        app.command_palette_execute(WindowCommand::UnzoomPane);
        assert_eq!(app.app_shell.active_tab().zoomed_pane_id(), None);
        assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));
        assert!(app.command_palette.is_none());
    }

    #[test]
    fn unmodified_t_is_not_shell_shortcut() {
        let app = NativeWindowApp::new_with_command(
            None,
            rssh_pty::PtyCommand::new("powershell").with_args(["-NoProfile"]),
        );

        assert!(
            app.app_shell_action_for_key(&Key::Character("t".into()), ModifiersState::CONTROL)
                .is_none()
        );
    }

    #[test]
    fn recognizes_workspace_rename_shortcut() {
        let app = NativeWindowApp::new(None);

        let action = app.app_shell_action_for_key(
            &Key::Character("r".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );

        let AppAction::RenameWorkspace { workspace, name } = action.unwrap() else {
            panic!("expected rename workspace action");
        };

        assert_eq!(workspace, rssh_core::WorkspaceId::new(1));
        assert_eq!(name, "default (renamed)");
    }

    #[test]
    fn recognizes_workspace_next_shortcut() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewWorkspace {
            name: "zeta".to_owned(),
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::NewWorkspace {
            name: "alpha".to_owned(),
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::SwitchWorkspace {
            workspace: rssh_core::WorkspaceId::new(1),
        })
        .unwrap();

        let action =
            app.app_shell_action_for_key(&Key::Character("n".into()), ModifiersState::CONTROL);
        let AppAction::SwitchWorkspaceRelative { offset } = action.unwrap() else {
            panic!("expected next workspace action");
        };
        assert_eq!(offset, 1);

        app.dispatch_app_action(AppAction::SwitchWorkspaceRelative { offset })
            .unwrap();
        assert_eq!(app.app_shell.active_workspace().name(), "zeta");
    }

    #[test]
    fn recognizes_workspace_previous_shortcut() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewWorkspace {
            name: "zeta".to_owned(),
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::NewWorkspace {
            name: "alpha".to_owned(),
            launch: None,
        })
        .unwrap();
        app.dispatch_app_action(AppAction::SwitchWorkspace {
            workspace: rssh_core::WorkspaceId::new(1),
        })
        .unwrap();

        let action =
            app.app_shell_action_for_key(&Key::Character("p".into()), ModifiersState::CONTROL);
        let AppAction::SwitchWorkspaceRelative { offset } = action.unwrap() else {
            panic!("expected previous workspace action");
        };
        assert_eq!(offset, -1);

        app.dispatch_app_action(AppAction::SwitchWorkspaceRelative { offset })
            .unwrap();
        assert_eq!(app.app_shell.active_workspace().name(), "alpha");
    }

    #[test]
    fn recognizes_window_close_workspace_shortcut() {
        let mut app = NativeWindowApp::new(None);
        app.dispatch_app_action(AppAction::NewWorkspace {
            name: "next".to_owned(),
            launch: None,
        })
        .unwrap();

        let action = app.app_shell_action_for_key(
            &Key::Character("k".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let AppAction::CloseWorkspace { workspace } = action.unwrap() else {
            panic!("expected close workspace action");
        };

        app.dispatch_app_action(AppAction::CloseWorkspace { workspace })
            .unwrap();

        assert_eq!(workspace, rssh_core::WorkspaceId::new(2));
        assert_eq!(app.app_shell.workspaces().len(), 1);
        assert_eq!(app.active_workspace_id(), rssh_core::WorkspaceId::new(1));
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
    fn window_app_writes_iterm_copy_clipboard_text_from_active_pane_output() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&writes);
        let mut app = NativeWindowApp::new(None);
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.handle_pty_output(b"\x1b]1337;Copy=;Y29weQ==\x07")
            .unwrap();

        assert_eq!(writes.lock().unwrap().as_slice(), ["copy"]);
    }

    #[test]
    fn window_app_writes_c1_iterm_copy_clipboard_text_from_active_pane_output() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&writes);
        let mut app = NativeWindowApp::new(None);
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });

        app.handle_pty_output(b"\x9d1337;Copy=;Y29weQ==\x9c")
            .unwrap();

        assert_eq!(writes.lock().unwrap().as_slice(), ["copy"]);
    }

    #[test]
    fn window_app_writes_iterm_copy_clipboard_text_from_inactive_pane_output() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&writes);
        let mut app = NativeWindowApp::new(None);
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded.lock().unwrap().push(text.to_owned());
            true
        });
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();

        app.handle_pane_pty_output(rssh_core::PaneId::new(1), b"\x1b]1337;Copy=;Y29weQ==\x07")
            .unwrap();

        assert_eq!(writes.lock().unwrap().as_slice(), ["copy"]);
    }

    #[test]
    fn window_app_dispatches_wezterm_notifications_from_active_and_inactive_panes() {
        let notifications = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&notifications);
        let mut app = NativeWindowApp::new(None);
        app.notification_handler = Box::new(move |notification| {
            recorded.lock().unwrap().push(notification.clone());
            true
        });
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();

        app.handle_pty_output(b"\x1b]9;active done\x07").unwrap();
        app.handle_pane_pty_output(
            rssh_core::PaneId::new(1),
            b"\x9d777;notify;Inactive;done\x9c",
        )
        .unwrap();

        assert_eq!(
            notifications.lock().unwrap().as_slice(),
            [
                TerminalNotification {
                    title: None,
                    body: "active done".to_owned(),
                },
                TerminalNotification {
                    title: Some("Inactive".to_owned()),
                    body: "done".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn window_app_dispatches_user_var_changed_from_active_and_inactive_panes() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&changes);
        let mut app = NativeWindowApp::new(None);
        app.user_var_change_handler = Box::new(move |change| {
            recorded.lock().unwrap().push(change.clone());
            true
        });
        app.dispatch_app_action(AppAction::NewTab { launch: None })
            .unwrap();
        let active_pane = app.app_shell.active_pane_id();

        app.handle_pty_output(b"\x1b]1337;SetUserVar=WEZTERM_PROG=YmFy\x07")
            .unwrap();
        app.handle_pty_output(b"\x1b]1337;SetUserVar=WEZTERM_PROG=YmFy\x07")
            .unwrap();
        app.handle_pane_pty_output(
            rssh_core::PaneId::new(1),
            b"\x9d1337;SetUserVar=WEZTERM_HOST=YmF6\x9c",
        )
        .unwrap();

        assert_eq!(
            changes.lock().unwrap().as_slice(),
            [
                NativeWindowUserVarChange {
                    pane: active_pane,
                    name: "WEZTERM_PROG".to_owned(),
                    value: "bar".to_owned(),
                },
                NativeWindowUserVarChange {
                    pane: rssh_core::PaneId::new(1),
                    name: "WEZTERM_HOST".to_owned(),
                    value: "baz".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn window_app_answers_osc52_clipboard_query_from_pty_output() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut app = NativeWindowApp::new(None);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));
        app.clipboard_reader = Box::new(|| Some("copy".to_owned()));

        app.handle_pty_output(b"\x1b]52;c;?\x07").unwrap();

        assert_eq!(
            written.lock().unwrap().as_slice(),
            b"\x1b]52;c;Y29weQ==\x07"
        );
    }

    #[test]
    fn window_app_blocks_osc52_when_policy_is_off() {
        let clipboard_writes = Arc::new(Mutex::new(Vec::new()));
        let recorded_clipboard = Arc::clone(&clipboard_writes);
        let pty_writes = Arc::new(Mutex::new(Vec::new()));
        let mut app = NativeWindowApp::new_with_osc52_policy(None, Osc52Policy::Off);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&pty_writes))));
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded_clipboard.lock().unwrap().push(text.to_owned());
            true
        });
        app.clipboard_reader = Box::new(|| Some("copy".to_owned()));

        app.handle_pty_output(b"\x1b]52;c;Y29weQ==\x07").unwrap();
        app.handle_pty_output(b"\x1b]1337;Copy=;Y29weQ==\x07")
            .unwrap();
        app.handle_pty_output(b"\x1b]52;c;?\x07").unwrap();

        assert!(clipboard_writes.lock().unwrap().is_empty());
        assert!(pty_writes.lock().unwrap().is_empty());
    }

    #[test]
    fn window_app_write_only_osc52_policy_blocks_queries() {
        let clipboard_writes = Arc::new(Mutex::new(Vec::new()));
        let recorded_clipboard = Arc::clone(&clipboard_writes);
        let pty_writes = Arc::new(Mutex::new(Vec::new()));
        let mut app = NativeWindowApp::new_with_osc52_policy(None, Osc52Policy::WriteOnly);
        app.writer = Some(Box::new(SharedWriter(Arc::clone(&pty_writes))));
        app.clipboard_writer = Box::new(move |text: &str| {
            recorded_clipboard.lock().unwrap().push(text.to_owned());
            true
        });
        app.clipboard_reader = Box::new(|| Some("copy".to_owned()));

        app.handle_pty_output(b"\x1b]52;c;Y29weQ==\x07").unwrap();
        app.handle_pty_output(b"\x1b]52;c;?\x07").unwrap();

        assert_eq!(clipboard_writes.lock().unwrap().as_slice(), ["copy"]);
        assert!(pty_writes.lock().unwrap().is_empty());
    }

    #[test]
    fn derives_terminal_size_from_window_pixels() {
        assert_eq!(
            terminal_size_from_window_pixels(640, FRAME_HEIGHT),
            rssh_core::TerminalSize::new(80, 24)
        );
        assert_eq!(
            terminal_size_from_window_pixels(640, 384),
            rssh_core::TerminalSize::new(80, 23)
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

    fn snapshot_row_text(
        snapshot: &rssh_renderer::TerminalRenderSnapshot,
        row: u16,
        columns: u16,
    ) -> String {
        (0..columns)
            .map(|column| snapshot_char(snapshot, row, column).unwrap_or(' '))
            .collect()
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

    fn frame_pixel_at(frame: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let index = (y * width + x) * 4;
        [
            frame[index],
            frame[index + 1],
            frame[index + 2],
            frame[index + 3],
        ]
    }

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
