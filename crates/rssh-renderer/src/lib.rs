use font8x8::{BASIC_FONTS, UnicodeFonts};
pub use rssh_core::DamageRegion;
use rssh_terminal::{Cell, Color, CursorShape, Terminal, TerminalGrid};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct RenderCell {
    pub row: u16,
    pub column: u16,
    pub ch: char,
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
    pub conceal: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub inverse: bool,
    pub hyperlink: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCursor {
    pub row: u16,
    pub column: u16,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRenderSnapshot {
    cells: Vec<RenderCell>,
    cursor: Option<RenderCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderGeometry {
    pub target_width: u32,
    pub target_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
}

impl RenderGeometry {
    #[must_use]
    pub const fn new(
        target_width: u32,
        target_height: u32,
        cell_width: u32,
        cell_height: u32,
    ) -> Self {
        Self {
            target_width,
            target_height,
            cell_width,
            cell_height,
        }
    }
}

pub const SCROLLBAR_TRACK_COLOR: [u8; 4] = [46, 46, 46, 255];
pub const SCROLLBAR_THUMB_COLOR: [u8; 4] = [172, 172, 172, 255];
pub const SCROLLBAR_WIDTH: u32 = 4;

const MIN_SCROLLBAR_THUMB_HEIGHT: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbackScrollbar {
    pub scrollback_lines: usize,
    pub viewport_rows: u16,
    pub scrollback_offset: usize,
}

impl ScrollbackScrollbar {
    #[must_use]
    pub fn new(
        scrollback_lines: usize,
        viewport_rows: u16,
        scrollback_offset: usize,
    ) -> Option<Self> {
        if scrollback_lines == 0 || viewport_rows == 0 {
            return None;
        }

        Some(Self {
            scrollback_lines,
            viewport_rows,
            scrollback_offset: scrollback_offset.min(scrollback_lines),
        })
    }

    #[must_use]
    pub fn offset_from_pixel_y(self, y: u32, geometry: RenderGeometry) -> usize {
        if geometry.target_height == 0 {
            return self.scrollback_offset;
        }

        let thumb_height = scrollbar_thumb_height(self, geometry.target_height);
        let travel = geometry.target_height.saturating_sub(thumb_height);
        if travel == 0 {
            return 0;
        }

        let y = y.min(geometry.target_height.saturating_sub(1));
        let live_distance = u64::from(y)
            .saturating_mul(self.scrollback_lines as u64)
            .saturating_add(u64::from(travel / 2))
            / u64::from(travel);
        let live_distance = usize::try_from(live_distance).unwrap_or(self.scrollback_lines);
        self.scrollback_lines
            .saturating_sub(live_distance.min(self.scrollback_lines))
    }
}

pub struct PixelRenderer;

impl PixelRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn render(
        &self,
        snapshot: &TerminalRenderSnapshot,
        target: &mut [u8],
        target_width: u32,
        target_height: u32,
        cell_width: u32,
        cell_height: u32,
    ) {
        if target_width == 0 || target_height == 0 || cell_width == 0 || cell_height == 0 {
            return;
        }

        let mut surface = Surface {
            target,
            width: target_width,
            height: target_height,
        };

        surface.fill(default_background());

        for cell in snapshot.cells() {
            render_cell(&mut surface, cell, cell_width, cell_height);
        }

        if let Some(cursor) = snapshot.cursor() {
            render_cursor(&mut surface, cursor, cell_width, cell_height);
        }
    }

    pub fn render_scrollbar(
        &self,
        scrollbar: ScrollbackScrollbar,
        target: &mut [u8],
        geometry: RenderGeometry,
    ) {
        if geometry.target_width == 0 || geometry.target_height == 0 {
            return;
        }

        let mut surface = Surface {
            target,
            width: geometry.target_width,
            height: geometry.target_height,
        };
        let track_width = SCROLLBAR_WIDTH.min(geometry.target_width);
        let track = Rect {
            x: geometry.target_width.saturating_sub(track_width),
            y: 0,
            width: track_width,
            height: geometry.target_height,
        };
        surface.fill_rect(track, SCROLLBAR_TRACK_COLOR);
        surface.fill_rect(
            scrollbar_thumb_rect(scrollbar, geometry, track_width),
            SCROLLBAR_THUMB_COLOR,
        );
    }

    pub fn render_damage(
        &self,
        snapshot: &TerminalRenderSnapshot,
        damage: &[DamageRegion],
        target: &mut [u8],
        geometry: RenderGeometry,
    ) {
        if geometry.target_width == 0
            || geometry.target_height == 0
            || geometry.cell_width == 0
            || geometry.cell_height == 0
            || damage.is_empty()
        {
            return;
        }

        let mut surface = Surface {
            target,
            width: geometry.target_width,
            height: geometry.target_height,
        };

        for region in damage.iter().copied().filter(|region| !region.is_empty()) {
            surface.fill_rect(
                damage_rect(region, geometry.cell_width, geometry.cell_height),
                default_background(),
            );
        }

        for cell in snapshot
            .cells()
            .iter()
            .filter(|cell| damage_contains_cell(damage, cell.row, cell.column))
        {
            render_cell(
                &mut surface,
                cell,
                geometry.cell_width,
                geometry.cell_height,
            );
        }

        if let Some(cursor) = snapshot
            .cursor()
            .filter(|cursor| damage_contains_cell(damage, cursor.row, cursor.column))
        {
            render_cursor(
                &mut surface,
                cursor,
                geometry.cell_width,
                geometry.cell_height,
            );
        }
    }
}

impl Default for PixelRenderer {
    fn default() -> Self {
        Self::new()
    }
}

struct Surface<'a> {
    target: &'a mut [u8],
    width: u32,
    height: u32,
}

#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Surface<'_> {
    fn fill(&mut self, color: [u8; 4]) {
        for pixel in self.target.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
        let max_y = rect.y.saturating_add(rect.height).min(self.height);
        let max_x = rect.x.saturating_add(rect.width).min(self.width);

        for row in rect.y..max_y {
            for column in rect.x..max_x {
                let index = ((row * self.width + column) * 4) as usize;
                if let Some(pixel) = self.target.get_mut(index..index + 4) {
                    pixel.copy_from_slice(&color);
                }
            }
        }
    }
}

fn render_cell(surface: &mut Surface<'_>, cell: &RenderCell, cell_width: u32, cell_height: u32) {
    let origin_x = u32::from(cell.column).saturating_mul(cell_width);
    let origin_y = u32::from(cell.row).saturating_mul(cell_height);
    let foreground = color_to_rgba(cell.foreground, default_foreground());
    let background = color_to_rgba(cell.background, default_background());
    let (foreground, background) = if cell.inverse {
        (background, foreground)
    } else {
        (foreground, background)
    };
    let foreground = if cell.faint {
        dim_foreground(foreground)
    } else {
        foreground
    };

    surface.fill_rect(
        Rect {
            x: origin_x,
            y: origin_y,
            width: cell_width,
            height: cell_height,
        },
        background,
    );

    if cell.conceal {
        return;
    }

    let Some(glyph) = BASIC_FONTS.get(cell.ch) else {
        return;
    };

    let scale_x = cell_width.max(8) / 8;
    let scale_y = cell_height.max(8) / 8;

    for (glyph_y, row_bits) in glyph.iter().enumerate() {
        for glyph_x in 0..8 {
            if row_bits & (1 << glyph_x) == 0 {
                continue;
            }

            let draw_x = origin_x + glyph_x * scale_x;
            let draw_y = origin_y + u32::try_from(glyph_y).unwrap_or(0) * scale_y;
            surface.fill_rect(
                Rect {
                    x: draw_x,
                    y: draw_y,
                    width: scale_x,
                    height: scale_y,
                },
                foreground,
            );
            let bold_x = draw_x.saturating_add(scale_x);
            if cell.bold && bold_x < origin_x.saturating_add(cell_width) {
                surface.fill_rect(
                    Rect {
                        x: bold_x,
                        y: draw_y,
                        width: scale_x,
                        height: scale_y,
                    },
                    foreground,
                );
            }
        }
    }

    if cell.underline {
        let underline_height = (cell_height / 8).max(1);
        surface.fill_rect(
            Rect {
                x: origin_x,
                y: origin_y + cell_height.saturating_sub(underline_height),
                width: cell_width,
                height: underline_height,
            },
            foreground,
        );
    }

    if cell.overline {
        let overline_height = (cell_height / 8).max(1);
        surface.fill_rect(
            Rect {
                x: origin_x,
                y: origin_y,
                width: cell_width,
                height: overline_height,
            },
            foreground,
        );
    }

    if cell.strikethrough {
        let strike_height = (cell_height / 8).max(1);
        let strike_y = origin_y
            .saturating_add(cell_height / 2)
            .saturating_sub(strike_height / 2);
        surface.fill_rect(
            Rect {
                x: origin_x,
                y: strike_y,
                width: cell_width,
                height: strike_height,
            },
            foreground,
        );
    }
}

fn render_cursor(
    surface: &mut Surface<'_>,
    cursor: RenderCursor,
    cell_width: u32,
    cell_height: u32,
) {
    let origin_x = u32::from(cursor.column).saturating_mul(cell_width);
    let origin_y = u32::from(cursor.row).saturating_mul(cell_height);
    let rect = cursor_rect(cursor.shape, origin_x, origin_y, cell_width, cell_height);
    surface.fill_rect(rect, default_foreground());
}

fn damage_rect(region: DamageRegion, cell_width: u32, cell_height: u32) -> Rect {
    Rect {
        x: u32::from(region.x).saturating_mul(cell_width),
        y: u32::from(region.y).saturating_mul(cell_height),
        width: u32::from(region.width).saturating_mul(cell_width),
        height: u32::from(region.height).saturating_mul(cell_height),
    }
}

fn damage_contains_cell(damage: &[DamageRegion], row: u16, column: u16) -> bool {
    damage.iter().copied().any(|region| {
        !region.is_empty()
            && row >= region.y
            && row < region.y.saturating_add(region.height)
            && column >= region.x
            && column < region.x.saturating_add(region.width)
    })
}

fn scrollbar_thumb_rect(
    scrollbar: ScrollbackScrollbar,
    geometry: RenderGeometry,
    track_width: u32,
) -> Rect {
    let thumb_height = scrollbar_thumb_height(scrollbar, geometry.target_height);
    let travel = geometry.target_height.saturating_sub(thumb_height);
    let scrollback_lines = scrollbar.scrollback_lines as u64;
    let live_distance = scrollbar
        .scrollback_lines
        .saturating_sub(scrollbar.scrollback_offset) as u64;
    let thumb_y = if scrollback_lines == 0 {
        0
    } else {
        u32::try_from(u64::from(travel).saturating_mul(live_distance) / scrollback_lines)
            .unwrap_or(travel)
    };

    Rect {
        x: geometry.target_width.saturating_sub(track_width),
        y: thumb_y.min(travel),
        width: track_width,
        height: thumb_height,
    }
}

fn scrollbar_thumb_height(scrollbar: ScrollbackScrollbar, target_height: u32) -> u32 {
    let viewport_rows = u64::from(scrollbar.viewport_rows);
    let total_rows = viewport_rows.saturating_add(scrollbar.scrollback_lines as u64);
    let target_height_u64 = u64::from(target_height);
    let proportional_height = if total_rows == 0 {
        target_height_u64
    } else {
        target_height_u64.saturating_mul(viewport_rows) / total_rows
    };

    u32::try_from(proportional_height)
        .unwrap_or(target_height)
        .max(MIN_SCROLLBAR_THUMB_HEIGHT)
        .min(target_height)
}

fn color_to_rgba(color: Color, default: [u8; 4]) -> [u8; 4] {
    match color {
        Color::Default => default,
        Color::Indexed(index) => indexed_color(index),
        Color::Rgb(red, green, blue) => [red, green, blue, 255],
    }
}

fn dim_foreground(color: [u8; 4]) -> [u8; 4] {
    [color[0] / 2, color[1] / 2, color[2] / 2, color[3]]
}

fn indexed_color(index: u8) -> [u8; 4] {
    const ANSI: [[u8; 4]; 16] = [
        [0, 0, 0, 255],
        [205, 49, 49, 255],
        [13, 188, 121, 255],
        [229, 229, 16, 255],
        [36, 114, 200, 255],
        [188, 63, 188, 255],
        [17, 168, 205, 255],
        [229, 229, 229, 255],
        [102, 102, 102, 255],
        [241, 76, 76, 255],
        [35, 209, 139, 255],
        [245, 245, 67, 255],
        [59, 142, 234, 255],
        [214, 112, 214, 255],
        [41, 184, 219, 255],
        [255, 255, 255, 255],
    ];

    if let Some(color) = ANSI.get(usize::from(index)) {
        return *color;
    }

    if (16..=231).contains(&index) {
        let cube_index = index - 16;
        let red = xterm_color_cube_intensity(cube_index / 36);
        let green = xterm_color_cube_intensity((cube_index / 6) % 6);
        let blue = xterm_color_cube_intensity(cube_index % 6);
        return [red, green, blue, 255];
    }

    let level = 8 + (index - 232) * 10;
    [level, level, level, 255]
}

const fn xterm_color_cube_intensity(value: u8) -> u8 {
    if value == 0 { 0 } else { 55 + value * 40 }
}

const fn default_foreground() -> [u8; 4] {
    [229, 229, 229, 255]
}

const fn default_background() -> [u8; 4] {
    [12, 12, 12, 255]
}

impl TerminalRenderSnapshot {
    #[must_use]
    pub fn from_grid(grid: &TerminalGrid) -> Self {
        Self::from_grid_with_cursor(grid, None)
    }

    #[must_use]
    pub fn from_terminal(terminal: &Terminal) -> Self {
        Self::from_terminal_viewport(terminal, 0)
    }

    #[must_use]
    pub fn from_terminal_viewport(terminal: &Terminal, scrollback_offset: usize) -> Self {
        let grid = terminal.grid();
        let size = grid.size();
        let scrollback = terminal.scrollback();
        let offset = scrollback_offset.min(scrollback.len());
        let first_source_row = scrollback.len().saturating_sub(offset);
        let cursor = render_cursor_from_terminal(terminal, offset);

        let mut cells = Vec::new();
        for viewport_row in 0..size.rows {
            let source_row = first_source_row + usize::from(viewport_row);
            if source_row < scrollback.len() {
                append_render_cells(
                    &mut cells,
                    viewport_row,
                    scrollback[source_row].cells(),
                    size.columns,
                );
            } else {
                let grid_row = source_row - scrollback.len();
                append_grid_row(&mut cells, grid, viewport_row, grid_row, size.columns);
            }
        }

        Self { cells, cursor }
    }

    fn from_grid_with_cursor(grid: &TerminalGrid, cursor: Option<RenderCursor>) -> Self {
        let size = grid.size();
        let mut cells = Vec::new();

        for row in 0..size.rows {
            for column in 0..size.columns {
                let Some(cell) = grid.get(row, column) else {
                    continue;
                };

                if cell.ch == ' ' {
                    continue;
                }

                cells.push(RenderCell {
                    row,
                    column,
                    ch: cell.ch,
                    foreground: cell.foreground,
                    background: cell.background,
                    bold: cell.bold,
                    faint: cell.faint,
                    italic: cell.italic,
                    underline: cell.underline,
                    conceal: cell.conceal,
                    strikethrough: cell.strikethrough,
                    overline: cell.overline,
                    inverse: cell.inverse,
                    hyperlink: cell.hyperlink.clone(),
                });
            }
        }

        Self { cells, cursor }
    }

    #[must_use]
    pub fn cells(&self) -> &[RenderCell] {
        &self.cells
    }

    #[must_use]
    pub const fn cursor(&self) -> Option<RenderCursor> {
        self.cursor
    }

    pub fn update_from_terminal_damage(&mut self, terminal: &Terminal, damage: &[DamageRegion]) {
        let grid = terminal.grid();
        let size = grid.size();
        for region in damage.iter().copied().filter(|region| !region.is_empty()) {
            let start_row = region.y.min(size.rows);
            let end_row = region.y.saturating_add(region.height).min(size.rows);
            let start_column = region.x.min(size.columns);
            let end_column = region.x.saturating_add(region.width).min(size.columns);
            if start_row >= end_row || start_column >= end_column {
                continue;
            }

            self.cells.retain(|cell| {
                cell.row < start_row
                    || cell.row >= end_row
                    || cell.column < start_column
                    || cell.column >= end_column
            });

            for row in start_row..end_row {
                for column in start_column..end_column {
                    let Some(cell) = grid.get(row, column) else {
                        continue;
                    };
                    append_render_cell(&mut self.cells, row, column, cell);
                }
            }
        }

        self.cells.sort_by_key(|cell| (cell.row, cell.column));
        self.cursor = render_cursor_from_terminal(terminal, 0);
    }

    #[must_use]
    pub fn with_inverse_overlay(mut self, mut selected: impl FnMut(u16, u16) -> bool) -> Self {
        for cell in &mut self.cells {
            if selected(cell.row, cell.column) {
                cell.inverse = !cell.inverse;
            }
        }

        self
    }
}

fn render_cursor_from_terminal(
    terminal: &Terminal,
    scrollback_offset: usize,
) -> Option<RenderCursor> {
    if !terminal.cursor_visible() || scrollback_offset != 0 {
        return None;
    }

    let (row, column) = terminal.cursor();
    Some(RenderCursor {
        row,
        column,
        shape: terminal.cursor_shape(),
    })
}

fn cursor_rect(
    shape: CursorShape,
    origin_x: u32,
    origin_y: u32,
    cell_width: u32,
    cell_height: u32,
) -> Rect {
    match shape {
        CursorShape::Block => Rect {
            x: origin_x,
            y: origin_y,
            width: cell_width,
            height: cell_height,
        },
        CursorShape::Underline => {
            let height = (cell_height / 6).max(1);
            Rect {
                x: origin_x,
                y: origin_y + cell_height.saturating_sub(height),
                width: cell_width,
                height,
            }
        }
        CursorShape::Bar => Rect {
            x: origin_x,
            y: origin_y,
            width: (cell_width / 4).max(1),
            height: cell_height,
        },
    }
}

fn append_grid_row(
    cells: &mut Vec<RenderCell>,
    grid: &TerminalGrid,
    viewport_row: u16,
    grid_row: usize,
    columns: u16,
) {
    let Ok(grid_row) = u16::try_from(grid_row) else {
        return;
    };

    if grid_row >= grid.size().rows {
        return;
    }

    for column in 0..columns {
        let Some(cell) = grid.get(grid_row, column) else {
            continue;
        };
        append_render_cell(cells, viewport_row, column, cell);
    }
}

fn append_render_cells(
    cells: &mut Vec<RenderCell>,
    viewport_row: u16,
    source_cells: &[Cell],
    columns: u16,
) {
    for (column, cell) in source_cells.iter().take(usize::from(columns)).enumerate() {
        let Ok(column) = u16::try_from(column) else {
            continue;
        };
        append_render_cell(cells, viewport_row, column, cell);
    }
}

fn append_render_cell(cells: &mut Vec<RenderCell>, row: u16, column: u16, cell: &Cell) {
    if cell.ch == ' ' {
        return;
    }

    cells.push(RenderCell {
        row,
        column,
        ch: cell.ch,
        foreground: cell.foreground,
        background: cell.background,
        bold: cell.bold,
        faint: cell.faint,
        italic: cell.italic,
        underline: cell.underline,
        conceal: cell.conceal,
        strikethrough: cell.strikethrough,
        overline: cell.overline,
        inverse: cell.inverse,
        hyperlink: cell.hyperlink.clone(),
    });
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;
    use rssh_terminal::{Cell, Color, CursorShape, Terminal, TerminalGrid};

    use super::{
        DamageRegion, PixelRenderer, RenderGeometry, SCROLLBAR_THUMB_COLOR, SCROLLBAR_TRACK_COLOR,
        ScrollbackScrollbar, TerminalRenderSnapshot,
    };

    #[test]
    fn zero_width_region_is_empty() {
        assert!(DamageRegion::new(0, 0, 0, 1).is_empty());
    }

    #[test]
    fn render_snapshot_contains_non_blank_terminal_cells() {
        let mut grid = TerminalGrid::new(TerminalSize::new(3, 2));
        grid.set(
            1,
            2,
            Cell {
                ch: 'R',
                foreground: Color::Indexed(2),
                background: Color::Rgb(1, 2, 3),
                bold: true,
                faint: false,
                italic: false,
                underline: true,
                conceal: false,
                strikethrough: false,
                overline: false,
                inverse: false,
                hyperlink: None,
            },
        );

        let snapshot = TerminalRenderSnapshot::from_grid(&grid);

        assert_eq!(snapshot.cells().len(), 1);
        assert_eq!(snapshot.cells()[0].row, 1);
        assert_eq!(snapshot.cells()[0].column, 2);
        assert_eq!(snapshot.cells()[0].ch, 'R');
        assert_eq!(snapshot.cells()[0].foreground, Color::Indexed(2));
        assert_eq!(snapshot.cells()[0].background, Color::Rgb(1, 2, 3));
        assert!(snapshot.cells()[0].bold);
        assert!(snapshot.cells()[0].underline);
        assert!(!snapshot.cells()[0].inverse);
    }

    #[test]
    fn render_snapshot_preserves_inverse_style() {
        let mut grid = TerminalGrid::new(TerminalSize::new(1, 1));
        grid.set(
            0,
            0,
            Cell {
                ch: 'I',
                foreground: Color::Default,
                background: Color::Default,
                bold: false,
                faint: false,
                italic: false,
                underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                inverse: true,
                hyperlink: None,
            },
        );

        let snapshot = TerminalRenderSnapshot::from_grid(&grid);

        assert!(snapshot.cells()[0].inverse);
    }

    #[test]
    fn render_snapshot_preserves_strikethrough_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[9mS");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].strikethrough);
    }

    #[test]
    fn render_snapshot_preserves_faint_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[2mF");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].faint);
    }

    #[test]
    fn render_snapshot_preserves_conceal_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[8mC");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].conceal);
    }

    #[test]
    fn render_snapshot_preserves_overline_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[53mO");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].overline);
    }

    #[test]
    fn render_snapshot_preserves_hyperlink_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));
        terminal.feed(b"\x1b]8;;https://example.com\x1b\\ab\x1b]8;;\x1b\\");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert_eq!(
            snapshot.cells()[0].hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            snapshot.cells()[1].hyperlink.as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn render_snapshot_can_apply_inverse_overlay() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));
        terminal.feed(b"abc");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal)
            .with_inverse_overlay(|row, column| row == 0 && column == 1);

        assert!(!snapshot.cells()[0].inverse);
        assert!(snapshot.cells()[1].inverse);
        assert!(!snapshot.cells()[2].inverse);
    }

    #[test]
    fn render_snapshot_updates_cells_from_damage_regions() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));
        terminal.feed(b"abc");
        terminal.take_damage();
        let mut snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        terminal.feed(b"\rZ");
        let damage = terminal.take_damage();

        snapshot.update_from_terminal_damage(&terminal, &damage);

        assert_eq!(snapshot_char(&snapshot, 0, 0), Some('Z'));
        assert_eq!(snapshot_char(&snapshot, 0, 1), Some('b'));
        assert_eq!(snapshot_char(&snapshot, 0, 2), Some('c'));
    }

    #[test]
    fn render_snapshot_removes_cells_cleared_by_damage_regions() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));
        terminal.feed(b"abc");
        terminal.take_damage();
        let mut snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        terminal.feed(b"\r ");
        let damage = terminal.take_damage();

        snapshot.update_from_terminal_damage(&terminal, &damage);

        assert_eq!(snapshot_char(&snapshot, 0, 0), None);
        assert_eq!(snapshot_char(&snapshot, 0, 1), Some('b'));
        assert_eq!(snapshot_char(&snapshot, 0, 2), Some('c'));
    }

    #[test]
    fn pixel_renderer_draws_glyph_foreground_pixels() {
        let mut grid = TerminalGrid::new(TerminalSize::new(1, 1));
        grid.set(
            0,
            0,
            Cell {
                ch: 'A',
                foreground: Color::Rgb(255, 0, 0),
                background: Color::Default,
                bold: false,
                faint: false,
                italic: false,
                underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                inverse: false,
                hyperlink: None,
            },
        );
        let snapshot = TerminalRenderSnapshot::from_grid(&grid);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [255, 0, 0, 255]),
            "renderer did not draw a red glyph pixel"
        );
    }

    #[test]
    fn pixel_renderer_updates_only_damage_regions() {
        let mut grid = TerminalGrid::new(TerminalSize::new(2, 1));
        grid.set(
            0,
            0,
            Cell {
                ch: 'A',
                foreground: Color::Default,
                background: Color::Rgb(20, 0, 0),
                bold: false,
                faint: false,
                italic: false,
                underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                inverse: false,
                hyperlink: None,
            },
        );
        grid.set(
            0,
            1,
            Cell {
                ch: 'B',
                foreground: Color::Default,
                background: Color::Rgb(0, 20, 0),
                bold: false,
                faint: false,
                italic: false,
                underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                inverse: false,
                hyperlink: None,
            },
        );
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(
            &TerminalRenderSnapshot::from_grid(&grid),
            &mut target,
            16,
            8,
            8,
            8,
        );
        let untouched_second_cell = pixel_at(&target, 16, 8, 0);

        grid.set(
            0,
            0,
            Cell {
                ch: 'Z',
                foreground: Color::Rgb(0, 0, 20),
                background: Color::Rgb(0, 0, 20),
                bold: false,
                faint: false,
                italic: false,
                underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                inverse: false,
                hyperlink: None,
            },
        );

        renderer.render_damage(
            &TerminalRenderSnapshot::from_grid(&grid),
            &[DamageRegion::new(0, 0, 1, 1)],
            &mut target,
            RenderGeometry::new(16, 8, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 0, 0), [0, 0, 20, 255]);
        assert_eq!(pixel_at(&target, 16, 8, 0), untouched_second_cell);
    }

    #[test]
    fn pixel_renderer_draws_scrollback_scrollbar_at_bottom_for_live_viewport() {
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 32 * 4];

        renderer.render_scrollbar(
            ScrollbackScrollbar::new(3, 1, 0).unwrap(),
            &mut target,
            RenderGeometry::new(16, 32, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 15, 0), SCROLLBAR_TRACK_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 31), SCROLLBAR_THUMB_COLOR);
    }

    #[test]
    fn pixel_renderer_moves_scrollback_scrollbar_thumb_up_for_history_viewport() {
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 32 * 4];

        renderer.render_scrollbar(
            ScrollbackScrollbar::new(3, 1, 3).unwrap(),
            &mut target,
            RenderGeometry::new(16, 32, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 15, 0), SCROLLBAR_THUMB_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 31), SCROLLBAR_TRACK_COLOR);
    }

    #[test]
    fn scrollback_scrollbar_maps_pixel_y_to_viewport_offset() {
        let geometry = RenderGeometry::new(8, 100, 1, 1);
        let scrollbar = ScrollbackScrollbar::new(10, 10, 0).unwrap();

        assert_eq!(scrollbar.offset_from_pixel_y(0, geometry), 10);
        assert_eq!(scrollbar.offset_from_pixel_y(99, geometry), 0);
    }

    #[test]
    fn indexed_color_maps_xterm_256_color_palette() {
        assert_eq!(
            super::color_to_rgba(Color::Indexed(16), [1, 2, 3, 255]),
            [0, 0, 0, 255]
        );
        assert_eq!(
            super::color_to_rgba(Color::Indexed(196), [1, 2, 3, 255]),
            [255, 0, 0, 255]
        );
        assert_eq!(
            super::color_to_rgba(Color::Indexed(232), [1, 2, 3, 255]),
            [8, 8, 8, 255]
        );
        assert_eq!(
            super::color_to_rgba(Color::Indexed(255), [1, 2, 3, 255]),
            [238, 238, 238, 255]
        );
    }

    #[test]
    fn pixel_renderer_draws_xterm_256_color_from_terminal_output() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[38;5;196mR");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert_eq!(snapshot.cells()[0].foreground, Color::Indexed(196));
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [255, 0, 0, 255]),
            "renderer did not draw xterm indexed red"
        );
    }

    #[test]
    fn pixel_renderer_draws_underlined_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[4;38;2;255;0;0mA");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].underline);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_strikethrough_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[9;38;2;255;0;0m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].strikethrough);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 4), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 4), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_dims_faint_foreground_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[2;4;38;2;200;100;50m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].faint);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [100, 50, 25, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [100, 50, 25, 255]);
    }

    #[test]
    fn pixel_renderer_hides_concealed_foreground_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[8;4;38;2;255;0;0;48;2;3;4;5m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].conceal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [3, 4, 5, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [3, 4, 5, 255]);
        assert_eq!(count_pixels(&target, [255, 0, 0, 255]), 0);
    }

    #[test]
    fn pixel_renderer_draws_overlined_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[53;38;2;255;0;0m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].overline);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_bold_text_with_more_foreground_pixels() {
        let renderer = PixelRenderer::new();
        let mut normal = Terminal::new(TerminalSize::new(2, 1));
        normal.feed(b"\x1b[38;2;255;0;0mA");
        let normal_snapshot = TerminalRenderSnapshot::from_terminal(&normal);
        let mut normal_target = vec![0; 16 * 8 * 4];

        renderer.render(&normal_snapshot, &mut normal_target, 16, 8, 8, 8);

        let mut bold = Terminal::new(TerminalSize::new(2, 1));
        bold.feed(b"\x1b[1;38;2;255;0;0mA");
        let bold_snapshot = TerminalRenderSnapshot::from_terminal(&bold);
        assert!(bold_snapshot.cells()[0].bold);
        let mut bold_target = vec![0; 16 * 8 * 4];

        renderer.render(&bold_snapshot, &mut bold_target, 16, 8, 8, 8);

        assert!(
            count_pixels(&bold_target, [255, 0, 0, 255])
                > count_pixels(&normal_target, [255, 0, 0, 255])
        );
    }

    #[test]
    fn pixel_renderer_swaps_foreground_and_background_for_inverse_cells() {
        let mut grid = TerminalGrid::new(TerminalSize::new(1, 1));
        grid.set(
            0,
            0,
            Cell {
                ch: 'A',
                foreground: Color::Rgb(255, 0, 0),
                background: Color::Rgb(0, 0, 255),
                bold: false,
                faint: false,
                italic: false,
                underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                inverse: true,
                hyperlink: None,
            },
        );
        let snapshot = TerminalRenderSnapshot::from_grid(&grid);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [255, 0, 0, 255]),
            "renderer did not use the original foreground as inverse background"
        );
        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [0, 0, 255, 255]),
            "renderer did not use the original background as inverse foreground"
        );
    }

    #[test]
    fn render_snapshot_exposes_terminal_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"ab\nc");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        let cursor = snapshot.cursor().expect("cursor should be visible");
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.column, 1);
    }

    #[test]
    fn render_snapshot_can_show_scrollback_viewport() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"ab\ncd\nef");

        let snapshot = TerminalRenderSnapshot::from_terminal_viewport(&terminal, 1);

        assert_eq!(
            snapshot
                .cells()
                .iter()
                .map(|cell| (cell.row, cell.column, cell.ch))
                .collect::<Vec<_>>(),
            vec![(0, 0, 'a'), (0, 1, 'b'), (1, 0, 'c'), (1, 1, 'd')]
        );
        assert!(snapshot.cursor().is_none());
    }

    #[test]
    fn render_snapshot_omits_hidden_terminal_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"\x1b[?25l");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cursor().is_none());
    }

    #[test]
    fn pixel_renderer_draws_blank_cursor_cell() {
        let terminal = Terminal::new(TerminalSize::new(1, 1));
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [229, 229, 229, 255]),
            "renderer did not draw a visible cursor block"
        );
    }

    #[test]
    fn pixel_renderer_draws_bar_cursor_shape() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[6 q");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert_eq!(snapshot.cursor().unwrap().shape, CursorShape::Bar);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 0), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_underline_cursor_shape() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[4 q");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert_eq!(snapshot.cursor().unwrap().shape, CursorShape::Underline);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 7), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 0, 0), [12, 12, 12, 255]);
    }

    fn snapshot_char(snapshot: &TerminalRenderSnapshot, row: u16, column: u16) -> Option<char> {
        snapshot
            .cells()
            .iter()
            .find(|cell| cell.row == row && cell.column == column)
            .map(|cell| cell.ch)
    }

    fn pixel_at(target: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let index = (y * width + x) * 4;
        [
            target[index],
            target[index + 1],
            target[index + 2],
            target[index + 3],
        ]
    }

    fn count_pixels(target: &[u8], color: [u8; 4]) -> usize {
        target
            .chunks_exact(4)
            .filter(|pixel| *pixel == color)
            .count()
    }
}
