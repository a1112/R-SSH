use font8x8::{BASIC_FONTS, UnicodeFonts};
pub use rssh_core::DamageRegion;
use rssh_terminal::{Cell, Color, Terminal, TerminalGrid};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct RenderCell {
    pub row: u16,
    pub column: u16,
    pub ch: char,
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCursor {
    pub row: u16,
    pub column: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRenderSnapshot {
    cells: Vec<RenderCell>,
    cursor: Option<RenderCursor>,
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
            let origin_x = u32::from(cell.column).saturating_mul(cell_width);
            let origin_y = u32::from(cell.row).saturating_mul(cell_height);
            let foreground = color_to_rgba(cell.foreground, default_foreground());
            let background = color_to_rgba(cell.background, default_background());
            let (foreground, background) = if cell.inverse {
                (background, foreground)
            } else {
                (foreground, background)
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

            let Some(glyph) = BASIC_FONTS.get(cell.ch) else {
                continue;
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
                }
            }
        }

        if let Some(cursor) = snapshot.cursor() {
            let origin_x = u32::from(cursor.column).saturating_mul(cell_width);
            let origin_y = u32::from(cursor.row).saturating_mul(cell_height);
            surface.fill_rect(
                Rect {
                    x: origin_x,
                    y: origin_y,
                    width: cell_width,
                    height: cell_height,
                },
                default_foreground(),
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

fn color_to_rgba(color: Color, default: [u8; 4]) -> [u8; 4] {
    match color {
        Color::Default => default,
        Color::Indexed(index) => indexed_color(index),
        Color::Rgb(red, green, blue) => [red, green, blue, 255],
    }
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

    ANSI.get(usize::from(index)).copied().unwrap_or(ANSI[15])
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
        let cursor = if terminal.cursor_visible() {
            let (row, column) = terminal.cursor();
            (offset == 0).then_some(RenderCursor { row, column })
        } else {
            None
        };

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
                    italic: cell.italic,
                    underline: cell.underline,
                    inverse: cell.inverse,
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
        italic: cell.italic,
        underline: cell.underline,
        inverse: cell.inverse,
    });
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;
    use rssh_terminal::{Cell, Color, Terminal, TerminalGrid};

    use super::{DamageRegion, PixelRenderer, TerminalRenderSnapshot};

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
                italic: false,
                underline: true,
                inverse: false,
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
                italic: false,
                underline: false,
                inverse: true,
            },
        );

        let snapshot = TerminalRenderSnapshot::from_grid(&grid);

        assert!(snapshot.cells()[0].inverse);
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
                italic: false,
                underline: false,
                inverse: false,
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
                italic: false,
                underline: false,
                inverse: true,
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
}
