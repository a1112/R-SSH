use rssh_core::TerminalSize;

mod parser;

pub use parser::{DEFAULT_SCROLLBACK_LIMIT, Terminal, TerminalUnknownEscapeSequence};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
    Rgba(u8, u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CursorStyle {
    #[default]
    SteadyBlock,
    BlinkingBlock,
    SteadyUnderline,
    BlinkingUnderline,
    SteadyBar,
    BlinkingBar,
}

impl CursorStyle {
    #[must_use]
    pub const fn shape(self) -> CursorShape {
        match self {
            Self::SteadyBlock | Self::BlinkingBlock => CursorShape::Block,
            Self::SteadyUnderline | Self::BlinkingUnderline => CursorShape::Underline,
            Self::SteadyBar | Self::BlinkingBar => CursorShape::Bar,
        }
    }

    #[must_use]
    pub const fn blinking(self) -> bool {
        matches!(
            self,
            Self::BlinkingBlock | Self::BlinkingUnderline | Self::BlinkingBar
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderlineStyle {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Superscript,
    Subscript,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SemanticType {
    #[default]
    Output,
    Prompt,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticZone {
    pub start_y: usize,
    pub start_x: usize,
    pub end_y: usize,
    pub end_x: usize,
    pub semantic_type: SemanticType,
}

impl SemanticZone {
    #[must_use]
    pub const fn new(
        start_y: usize,
        start_x: usize,
        end_y: usize,
        end_x: usize,
        semantic_type: SemanticType,
    ) -> Self {
        Self {
            start_y,
            start_x,
            end_y,
            end_x,
            semantic_type,
        }
    }

    #[must_use]
    pub const fn contains(&self, x: usize, y: usize) -> bool {
        if y < self.start_y || y > self.end_y {
            return false;
        }
        if y == self.start_y && x < self.start_x {
            return false;
        }
        if y == self.end_y && x > self.end_x {
            return false;
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCommandExit {
    pub row: usize,
    pub exit_code: Option<i32>,
    pub aid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineImageFormat {
    Encoded,
    Rgb,
    Rgba,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItermInlineImage {
    pub row: usize,
    pub column: u16,
    pub name: Option<String>,
    pub kitty_image_id: Option<u32>,
    pub kitty_placement_id: Option<u32>,
    pub kitty_z_index: Option<i32>,
    pub size: Option<usize>,
    pub width: Option<String>,
    pub height: Option<String>,
    pub preserve_aspect_ratio: Option<bool>,
    pub image_format: InlineImageFormat,
    pub pixel_width: Option<u32>,
    pub pixel_height: Option<u32>,
    pub source_x: Option<u32>,
    pub source_y: Option<u32>,
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub target_x: Option<u32>,
    pub target_y: Option<u32>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cell {
    pub ch: char,
    pub foreground: Color,
    pub background: Color,
    pub underline_color: Color,
    pub underline_style: UnderlineStyle,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub blink: bool,
    pub rapid_blink: bool,
    pub underline: bool,
    pub double_underline: bool,
    pub conceal: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub vertical_align: VerticalAlign,
    pub inverse: bool,
    pub protected: bool,
    pub hyperlink: Option<String>,
    pub semantic_type: SemanticType,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            foreground: Color::Default,
            background: Color::Default,
            underline_color: Color::Default,
            underline_style: UnderlineStyle::None,
            bold: false,
            faint: false,
            italic: false,
            blink: false,
            rapid_blink: false,
            underline: false,
            double_underline: false,
            conceal: false,
            strikethrough: false,
            overline: false,
            vertical_align: VerticalAlign::default(),
            inverse: false,
            protected: false,
            hyperlink: None,
            semantic_type: SemanticType::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerminalGrid {
    size: TerminalSize,
    cells: Vec<Cell>,
    row_wrapped: Vec<bool>,
}

impl TerminalGrid {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            size,
            cells: vec![Cell::default(); size.cells()],
            row_wrapped: vec![false; usize::from(size.rows)],
        }
    }

    #[must_use]
    pub const fn size(&self) -> TerminalSize {
        self.size
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    #[must_use]
    pub fn get(&self, row: u16, column: u16) -> Option<&Cell> {
        self.index(row, column)
            .and_then(|index| self.cells.get(index))
    }

    pub fn set(&mut self, row: u16, column: u16, cell: Cell) -> bool {
        let Some(index) = self.index(row, column) else {
            return false;
        };

        self.cells[index] = cell;
        true
    }

    #[must_use]
    pub(crate) fn row_wrapped(&self, row: u16) -> bool {
        self.row_wrapped
            .get(usize::from(row))
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn set_row_wrapped(&mut self, row: u16, wrapped: bool) {
        if let Some(row_wrapped) = self.row_wrapped.get_mut(usize::from(row)) {
            *row_wrapped = wrapped;
        }
    }

    pub(crate) fn copy_row_wrapped(&mut self, from: u16, to: u16) {
        let wrapped = self.row_wrapped(from);
        self.set_row_wrapped(to, wrapped);
    }

    pub fn resize(&mut self, size: TerminalSize) {
        let old_size = self.size;
        let old_cells = std::mem::replace(&mut self.cells, vec![Cell::default(); size.cells()]);
        let old_row_wrapped =
            std::mem::replace(&mut self.row_wrapped, vec![false; usize::from(size.rows)]);
        self.size = size;

        let rows = old_size.rows.min(size.rows);
        let columns = old_size.columns.min(size.columns);
        for row in 0..rows {
            for column in 0..columns {
                let old_index =
                    usize::from(row) * usize::from(old_size.columns) + usize::from(column);
                let new_index = usize::from(row) * usize::from(size.columns) + usize::from(column);
                if let Some(cell) = old_cells.get(old_index) {
                    self.cells[new_index] = cell.clone();
                }
            }
            self.row_wrapped[usize::from(row)] = old_row_wrapped
                .get(usize::from(row))
                .copied()
                .unwrap_or(false);
        }
    }

    #[must_use]
    fn index(&self, row: u16, column: u16) -> Option<usize> {
        if row >= self.size.rows || column >= self.size.columns {
            return None;
        }

        Some(usize::from(row) * usize::from(self.size.columns) + usize::from(column))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbackLine {
    cells: Vec<Cell>,
    wrapped: bool,
}

impl ScrollbackLine {
    #[must_use]
    pub(crate) const fn from_cells_wrapped(cells: Vec<Cell>, wrapped: bool) -> Self {
        Self { cells, wrapped }
    }

    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    #[must_use]
    pub const fn is_wrapped(&self) -> bool {
        self.wrapped
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use rssh_core::{DamageRegion, TerminalSize};

    use super::{
        Cell, Color, CursorShape, CursorStyle, InlineImageFormat, ItermInlineImage,
        SemanticCommandExit, SemanticType, SemanticZone, Terminal, TerminalGrid, UnderlineStyle,
        VerticalAlign,
    };

    #[test]
    fn grid_allocates_one_cell_per_terminal_slot() {
        let grid = TerminalGrid::new(TerminalSize::new(80, 24));

        assert_eq!(grid.size(), TerminalSize::new(80, 24));
        assert_eq!(grid.len(), 1920);
        assert!(!grid.is_empty());
    }

    #[test]
    fn default_cell_has_terminal_defaults() {
        let cell = Cell::default();

        assert_eq!(cell.ch, ' ');
        assert_eq!(cell.foreground, Color::Default);
        assert_eq!(cell.background, Color::Default);
        assert_eq!(cell.underline_color, Color::Default);
        assert_eq!(cell.underline_style, UnderlineStyle::None);
        assert!(!cell.bold);
        assert!(!cell.faint);
        assert!(!cell.italic);
        assert!(!cell.blink);
        assert!(!cell.underline);
        assert!(!cell.double_underline);
        assert!(!cell.conceal);
        assert!(!cell.strikethrough);
        assert!(!cell.overline);
        assert_eq!(cell.vertical_align, VerticalAlign::Baseline);
        assert!(!cell.inverse);
        assert!(!cell.protected);
        assert_eq!(cell.hyperlink, None);
        assert_eq!(cell.semantic_type, SemanticType::Output);
    }

    #[test]
    fn grid_sets_and_reads_cells_by_position() {
        let mut grid = TerminalGrid::new(TerminalSize::new(3, 2));
        let cell = Cell {
            ch: 'R',
            foreground: Color::Indexed(2),
            background: Color::Rgb(1, 2, 3),
            underline_color: Color::Default,
            underline_style: UnderlineStyle::Single,
            bold: true,
            faint: false,
            italic: false,
            blink: false,
            underline: true,
            double_underline: false,
            conceal: false,
            strikethrough: false,
            overline: false,
            vertical_align: VerticalAlign::Baseline,
            inverse: false,
            hyperlink: None,
            semantic_type: SemanticType::Output,
            ..Cell::default()
        };

        assert!(grid.set(1, 2, cell.clone()));

        assert_eq!(grid.get(1, 2), Some(&cell));
    }

    #[test]
    fn grid_returns_none_for_out_of_bounds_reads() {
        let grid = TerminalGrid::new(TerminalSize::new(3, 2));

        assert_eq!(grid.get(2, 0), None);
        assert_eq!(grid.get(0, 3), None);
    }

    #[test]
    fn grid_rejects_out_of_bounds_writes() {
        let mut grid = TerminalGrid::new(TerminalSize::new(3, 2));

        assert!(!grid.set(2, 0, Cell::default()));
        assert!(!grid.set(0, 3, Cell::default()));
    }

    #[test]
    fn terminal_writes_plain_text_into_grid() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 2));

        terminal.feed(b"abc");

        assert_eq!(terminal.grid().get(0, 0).unwrap().ch, 'a');
        assert_eq!(terminal.grid().get(0, 1).unwrap().ch, 'b');
        assert_eq!(terminal.grid().get(0, 2).unwrap().ch, 'c');
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_moves_to_next_row_on_newline() {
        let mut terminal = Terminal::new(TerminalSize::new(5, 2));

        terminal.feed(b"ab\ncd");

        assert_eq!(terminal.grid().get(0, 0).unwrap().ch, 'a');
        assert_eq!(terminal.grid().get(0, 1).unwrap().ch, 'b');
        assert_eq!(terminal.grid().get(1, 2).unwrap().ch, 'c');
        assert_eq!(terminal.grid().get(1, 3).unwrap().ch, 'd');
        assert_eq!(terminal.cursor(), (1, 4));
    }

    #[test]
    fn terminal_vertical_tab_and_form_feed_preserve_column() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 3));

        terminal.feed(b"ab\x0bcd\x0cef");

        assert_eq!(terminal.grid().get(0, 0).unwrap().ch, 'a');
        assert_eq!(terminal.grid().get(0, 1).unwrap().ch, 'b');
        assert_eq!(terminal.grid().get(1, 2).unwrap().ch, 'c');
        assert_eq!(terminal.grid().get(1, 3).unwrap().ch, 'd');
        assert_eq!(terminal.grid().get(2, 4).unwrap().ch, 'e');
        assert_eq!(terminal.grid().get(2, 5).unwrap().ch, 'f');
        assert_eq!(terminal.cursor(), (2, 6));
    }

    #[test]
    fn terminal_records_bell_without_advancing_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"ab\x07cd");

        assert_eq!(row_text(&terminal, 0), "abcd  ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.take_bell_count(), 1);
        assert_eq!(terminal.take_bell_count(), 0);
    }

    #[test]
    fn terminal_records_unknown_escape_sequences_without_rendering_them() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"ab\x1bzcd\x1b[?999zef");

        let unknown = terminal.take_unknown_escape_sequences();
        let sequences = unknown
            .iter()
            .map(|sequence| sequence.sequence.as_str())
            .collect::<Vec<_>>();
        assert_eq!(sequences, ["ESC z", "CSI ?999z"]);
        assert_eq!(row_text(&terminal, 0), "abcdef  ");
        assert!(terminal.take_unknown_escape_sequences().is_empty());
    }

    #[test]
    fn terminal_ignores_non_printing_c0_controls() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(
            b"a\x00\x01\x02\x03\x04\x05\x06\x0e\x0f\x10\x11\x12\x13\x14\
              \x15\x16\x17\x18\x19\x1a\x1c\x1d\x1e\x1fb",
        );

        assert_eq!(row_text(&terminal, 0), "ab    ");
        assert_eq!(terminal.cursor(), (0, 2));
    }

    #[test]
    fn terminal_index_moves_down_without_carriage_return() {
        let mut terminal = Terminal::new(TerminalSize::new(5, 2));

        terminal.feed(b"ab\x1bDcd");

        assert_eq!(row_text(&terminal, 0), "ab   ");
        assert_eq!(row_text(&terminal, 1), "  cd ");
        assert_eq!(terminal.cursor(), (1, 4));
    }

    #[test]
    fn terminal_next_line_moves_down_to_first_column() {
        let mut terminal = Terminal::new(TerminalSize::new(5, 2));

        terminal.feed(b"ab\x1bEcd");

        assert_eq!(row_text(&terminal, 0), "ab   ");
        assert_eq!(row_text(&terminal, 1), "cd   ");
        assert_eq!(terminal.cursor(), (1, 2));
    }

    #[test]
    fn terminal_reverse_index_moves_up_without_carriage_return() {
        let mut terminal = Terminal::new(TerminalSize::new(5, 2));

        terminal.feed(b"\x1b[2;3H\x1bMZ");

        assert_eq!(row_text(&terminal, 0), "  Z  ");
        assert_eq!(row_text(&terminal, 1), "     ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_c1_ind_nel_and_ri_match_escape_controls() {
        let mut terminal = Terminal::new(TerminalSize::new(5, 3));

        terminal.feed(b"ab\x84cd\x85ef\x9b3;3H\x8dZ");

        assert_eq!(row_text(&terminal, 0), "ab   ");
        assert_eq!(row_text(&terminal, 1), "  Zd ");
        assert_eq!(row_text(&terminal, 2), "ef   ");
        assert_eq!(terminal.cursor(), (1, 3));
    }

    #[test]
    fn terminal_backspace_moves_cursor_left_without_erasing() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"ab\x08c");

        assert_eq!(row_text(&terminal, 0), "ac  ");
        assert_eq!(terminal.cursor(), (0, 2));
    }

    #[test]
    fn terminal_backspace_stops_at_left_margin_when_declrmm_is_enabled() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"abcdefgh\x1b[?69h\x1b[3;6s\x1b[1;3H\x08Z");

        assert_eq!(row_text(&terminal, 0), "abZdefgh");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_carriage_return_moves_to_left_margin_when_declrmm_is_enabled() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"abcdefgh\x1b[?69h\x1b[3;6s\x1b[1;5H\rZ");

        assert_eq!(row_text(&terminal, 0), "abZdefgh");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_tab_moves_to_next_eight_column_stop() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 1));

        terminal.feed(b"a\tb");

        assert_eq!(row_text(&terminal, 0), "a       b ");
        assert_eq!(terminal.cursor(), (0, 9));
    }

    #[test]
    fn terminal_sets_custom_tab_stop_with_hts() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 1));

        terminal.feed(b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;1Ha\tb");

        assert_eq!(row_text(&terminal, 0), "a   b     ");
        assert_eq!(terminal.cursor(), (0, 5));
    }

    #[test]
    fn terminal_c1_hts_sets_custom_tab_stop() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 1));

        terminal.feed(b"\x1b[3g\x1b[1;5H\x88\x1b[1;1Ha\tb");

        assert_eq!(row_text(&terminal, 0), "a   b     ");
        assert_eq!(terminal.cursor(), (0, 5));
    }

    #[test]
    fn terminal_clears_tab_stops_with_tbc() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 1));

        terminal.feed(b"\x1b[3g\x1b[1;5H\x1bH\x1b[g\x1b[1;1Ha\tb");

        assert_eq!(row_text(&terminal, 0), "a        b");
        assert_eq!(terminal.cursor(), (0, 9));
    }

    #[test]
    fn terminal_moves_forward_and_backward_between_tab_stops() {
        let mut terminal = Terminal::new(TerminalSize::new(20, 1));

        terminal.feed(b"a\x1b[2Ib\x1b[10G\x1b[Zc");

        assert_eq!(row_text(&terminal, 0), "a       c       b   ");
        assert_eq!(terminal.cursor(), (0, 9));
    }

    #[test]
    fn terminal_saves_and_restores_cursor_with_esc_7_and_8() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"ab\x1b7cd\x1b8Z");

        assert_eq!(row_text(&terminal, 0), "abZd    ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_esc_save_restore_restores_style_and_character_set() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"\x1b[31;1mA\x1b7\x1b[0m\x1b(0q\x1b8q");

        assert_eq!(row_text(&terminal, 0), "Aq    ");
        assert_eq!(terminal.cursor(), (0, 2));

        let restored = terminal.grid().get(0, 1).unwrap();
        assert_eq!(restored.ch, 'q');
        assert_eq!(restored.foreground, Color::Indexed(1));
        assert!(restored.bold);
    }

    #[test]
    fn terminal_esc_save_restore_restores_origin_mode() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[2;3r\x1b[?6h\x1b[1;1H\x1b7\x1b[?6l\x1b8\x1b[1;1HZ");

        assert_eq!(row_text(&terminal, 0), "    ");
        assert_eq!(row_text(&terminal, 1), "Z   ");
        assert_eq!(terminal.cursor(), (1, 1));
    }

    #[test]
    fn terminal_saves_and_restores_cursor_with_csi_s_and_u() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"ab\x1b[s\x1b[2;1Hcd\x1b[uZ");

        assert_eq!(row_text(&terminal, 0), "abZ     ");
        assert_eq!(row_text(&terminal, 1), "cd      ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_csi_save_restore_restores_style_and_character_set() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"\x1b[32mA\x1b[s\x1b[0m\x1b(0q\x1b[uq");

        assert_eq!(row_text(&terminal, 0), "Aq    ");

        let restored = terminal.grid().get(0, 1).unwrap();
        assert_eq!(restored.ch, 'q');
        assert_eq!(restored.foreground, Color::Indexed(2));
    }

    #[test]
    fn terminal_scrolls_when_newline_reaches_bottom_row() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"ab\r\ncd\r\nef");

        assert_eq!(row_text(&terminal, 0), "cd  ");
        assert_eq!(row_text(&terminal, 1), "ef  ");
        assert_eq!(terminal.cursor(), (1, 2));
    }

    #[test]
    fn terminal_records_full_screen_scrolled_lines_in_scrollback() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"ab\r\ncd\r\nef");

        assert_eq!(terminal.scrollback().len(), 1);
        assert_eq!(scrollback_text(&terminal, 0), "ab  ");
        assert_eq!(row_text(&terminal, 0), "cd  ");
        assert_eq!(row_text(&terminal, 1), "ef  ");
    }

    #[test]
    fn terminal_honors_configured_scrollback_limit() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.set_scrollback_limit(2);

        terminal.feed(b"aa\r\nbb\r\ncc\r\ndd\r\nee");

        assert_eq!(terminal.scrollback().len(), 2);
        assert_eq!(scrollback_text(&terminal, 0), "bb  ");
        assert_eq!(scrollback_text(&terminal, 1), "cc  ");
        assert_eq!(row_text(&terminal, 0), "dd  ");
        assert_eq!(row_text(&terminal, 1), "ee  ");
    }

    #[test]
    fn terminal_erase_display_mode_3_clears_scrollback_only() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"ab\r\ncd\r\nef");
        assert_eq!(terminal.scrollback().len(), 1);

        terminal.feed(b"\x1b[3J");

        assert!(terminal.scrollback().is_empty());
        assert_eq!(row_text(&terminal, 0), "cd  ");
        assert_eq!(row_text(&terminal, 1), "ef  ");
    }

    #[test]
    fn terminal_erase_display_mode_3_removes_scrollback_inline_images() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");
        terminal.feed(b"\n\n");
        assert_eq!(terminal.scrollback().len(), 1);
        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[3J");

        assert!(terminal.scrollback().is_empty());
        assert!(terminal.inline_images().is_empty());
    }

    #[test]
    fn terminal_erase_display_mode_3_rebases_visible_inline_images() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"ab\ncd\nef");
        assert_eq!(terminal.scrollback().len(), 1);
        terminal.feed(b"\x1b[1;1H");
        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");
        assert_eq!(terminal.inline_images()[0].row, 1);

        terminal.feed(b"\x1b[3J");

        assert!(terminal.scrollback().is_empty());
        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].row, 0);
    }

    #[test]
    fn terminal_selective_erase_display_mode_2_clears_visible_grid() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"abcd\x1b[2;1Hefgh\x1b[1;2H\x1b[?2J");

        assert_eq!(row_text(&terminal, 0), "    ");
        assert_eq!(row_text(&terminal, 1), "    ");
    }

    #[test]
    fn terminal_selective_erase_display_preserves_protected_cells() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"\x1b[1\"qab\x1b[0\"qcd\x1b[2;1Hefgh\x1b[1;1H\x1b[?2J");

        assert_eq!(row_text(&terminal, 0), "ab  ");
        assert_eq!(row_text(&terminal, 1), "    ");
    }

    #[test]
    fn terminal_selective_erase_line_mode_2_clears_current_line() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"abcd\x1b[1;2H\x1b[?2K");

        assert_eq!(row_text(&terminal, 0), "    ");
    }

    #[test]
    fn terminal_selective_erase_line_preserves_protected_cells() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"\x1b[1\"qab\x1b[2\"qcd\x1b[1;1H\x1b[?2K");

        assert_eq!(row_text(&terminal, 0), "ab  ");
    }

    #[test]
    fn terminal_tracks_osc133_prompt_rows_across_scrollback() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"\x1b]133;A\x07> one\r\noutput\r\n\x1b]133;A\x07> two\r\nnext");

        assert_eq!(terminal.scrollback().len(), 2);
        assert_eq!(scrollback_text(&terminal, 0), "> one   ");
        assert_eq!(scrollback_text(&terminal, 1), "output  ");
        assert_eq!(row_text(&terminal, 0), "> two   ");
        assert_eq!(row_text(&terminal, 1), "next    ");
        assert_eq!(terminal.semantic_prompt_rows(), &[0, 2]);
    }

    #[test]
    fn terminal_tracks_osc133_semantic_zones() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 4));

        terminal.feed(b"ready\r\n\x1b]133;A\x07> \x1b]133;B\x07ls -l\r\n\x1b]133;C\x07file.txt");

        assert_eq!(row_text(&terminal, 0), "ready       ");
        assert_eq!(row_text(&terminal, 1), "> ls -l     ");
        assert_eq!(row_text(&terminal, 2), "file.txt    ");
        assert_eq!(
            terminal.semantic_zones(),
            &[
                SemanticZone::new(0, 0, 0, 4, SemanticType::Output),
                SemanticZone::new(1, 0, 1, 1, SemanticType::Prompt),
                SemanticZone::new(1, 2, 1, 6, SemanticType::Input),
                SemanticZone::new(2, 0, 2, 7, SemanticType::Output),
            ]
        );
        assert_eq!(
            terminal.semantic_zone_at(0, 1),
            Some(SemanticZone::new(1, 0, 1, 1, SemanticType::Prompt))
        );
        assert_eq!(
            terminal.semantic_zone_at(4, 1),
            Some(SemanticZone::new(1, 2, 1, 6, SemanticType::Input))
        );
        assert_eq!(
            terminal.semantic_zone_at(2, 2),
            Some(SemanticZone::new(2, 0, 2, 7, SemanticType::Output))
        );
    }

    #[test]
    fn terminal_tracks_osc133_command_status() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 3));

        terminal.feed(b"\x1b]133;C\x07build\nok\x1b]133;D;7;aid=42\x07");

        assert_eq!(
            terminal.semantic_command_exits(),
            &[SemanticCommandExit {
                row: 1,
                exit_code: Some(7),
                aid: Some("42".to_owned()),
            }]
        );
    }

    #[test]
    fn terminal_resets_osc133_line_input_after_newline() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 3));

        terminal.feed(b"\x1b]133;A\x07> \x1b]133;I\x07one\r\noutput");

        assert_eq!(row_text(&terminal, 0), "> one       ");
        assert_eq!(row_text(&terminal, 1), "output      ");
        assert_eq!(
            terminal.semantic_zones(),
            &[
                SemanticZone::new(0, 0, 0, 1, SemanticType::Prompt),
                SemanticZone::new(0, 2, 0, 4, SemanticType::Input),
                SemanticZone::new(1, 0, 1, 5, SemanticType::Output),
            ]
        );
    }

    #[test]
    fn terminal_extracts_text_from_semantic_zone() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 4));

        terminal.feed(b"\x1b]133;A\x07> \x1b]133;B\x07cargo test\r\n\x1b]133;C\x07ok");

        let input_zone = terminal.semantic_zone_at(3, 0).expect("input zone");
        assert_eq!(input_zone.semantic_type, SemanticType::Input);
        assert_eq!(
            terminal.text_from_semantic_zone(input_zone).as_deref(),
            Some("cargo test")
        );

        let output_zone = terminal.semantic_zone_at(0, 1).expect("output zone");
        assert_eq!(output_zone.semantic_type, SemanticType::Output);
        assert_eq!(
            terminal.text_from_semantic_zone(output_zone).as_deref(),
            Some("ok")
        );
    }

    #[test]
    fn terminal_extracts_multiline_semantic_zone_from_scrollback() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"\x1b]133;C\x07alpha\r\nbeta\r\ngamma");

        assert_eq!(terminal.scrollback().len(), 1);
        let output_zone = terminal.semantic_zone_at(0, 0).expect("output zone");
        assert_eq!(output_zone.semantic_type, SemanticType::Output);
        assert_eq!(output_zone.start_y, 0);
        assert_eq!(output_zone.end_y, 2);
        assert_eq!(
            terminal.text_from_semantic_zone(output_zone).as_deref(),
            Some("alpha\nbeta\ngamma")
        );
    }

    #[test]
    fn terminal_text_from_region_unwraps_soft_wrapped_lines_across_scrollback() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"abc defghij\r\nz");

        assert_eq!(terminal.scrollback().len(), 2);
        assert_eq!(scrollback_text(&terminal, 0), "abc ");
        assert_eq!(scrollback_text(&terminal, 1), "defg");
        assert_eq!(row_text(&terminal, 0), "hij ");
        assert_eq!(row_text(&terminal, 1), "z   ");
        assert_eq!(
            terminal.text_from_region(0, 0, 3, 3).as_deref(),
            Some("abc defghij\nz")
        );
    }

    #[test]
    fn terminal_does_not_record_scroll_region_lines_in_scrollback() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444");
        terminal.feed(b"\x1b[2;3r\x1b[3;1H\nzz");

        assert!(terminal.scrollback().is_empty());
        assert_eq!(row_text(&terminal, 1), "3333");
        assert_eq!(row_text(&terminal, 2), "zz  ");
    }

    #[test]
    fn terminal_does_not_record_alternate_screen_scrolls_in_scrollback() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"main\x1b[?1049halt\nmore\ndone\x1b[?1049l");

        assert!(terminal.scrollback().is_empty());
        assert_eq!(row_text(&terminal, 0), "main");
    }

    #[test]
    fn terminal_scroll_region_limits_linefeed_scrolling() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444");
        terminal.feed(b"\x1b[2;3r\x1b[3;1H\nzz");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "3333");
        assert_eq!(row_text(&terminal, 2), "zz  ");
        assert_eq!(row_text(&terminal, 3), "4444");
        assert_eq!(terminal.cursor(), (2, 2));
    }

    #[test]
    fn terminal_index_scrolls_up_at_scroll_region_bottom() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444");
        terminal.feed(b"\x1b[2;3r\x1b[3;1H\x1bDzz");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "3333");
        assert_eq!(row_text(&terminal, 2), "zz  ");
        assert_eq!(row_text(&terminal, 3), "4444");
        assert_eq!(terminal.cursor(), (2, 2));
    }

    #[test]
    fn terminal_reverse_index_scrolls_down_at_scroll_region_top() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444");
        terminal.feed(b"\x1b[2;3r\x1b[2;1H\x1bMzz");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "zz  ");
        assert_eq!(row_text(&terminal, 2), "2222");
        assert_eq!(row_text(&terminal, 3), "4444");
        assert_eq!(terminal.cursor(), (1, 2));
    }

    #[test]
    fn terminal_reset_scroll_region_restores_full_screen_scrolling() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 3));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333");
        terminal.feed(b"\x1b[2;2r\x1b[r\x1b[3;1H\nzz");

        assert_eq!(row_text(&terminal, 0), "2222");
        assert_eq!(row_text(&terminal, 1), "3333");
        assert_eq!(row_text(&terminal, 2), "zz  ");
        assert_eq!(terminal.cursor(), (2, 2));
    }

    #[test]
    fn terminal_ris_resets_visible_state_and_modes() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 3));

        terminal.feed(b"dirty\x1b[31;1m\x1b[?25l\x1b[?7l\x1b(0\x1b[3g\x1b[1;5H\x1bH");
        terminal.feed(b"\x1bcq\tB");

        assert_eq!(row_text(&terminal, 0), "q       B ");
        assert_eq!(row_text(&terminal, 1), "          ");
        assert_eq!(row_text(&terminal, 2), "          ");
        assert_eq!(terminal.cursor(), (0, 9));
        assert!(terminal.cursor_visible());

        let reset_cell = terminal.grid().get(0, 0).unwrap();
        assert_eq!(reset_cell.ch, 'q');
        assert_eq!(reset_cell.foreground, Color::Default);
        assert!(!reset_cell.bold);
    }

    #[test]
    fn terminal_ris_resets_insert_mode_and_scroll_region() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 3));

        terminal.feed(b"\x1b[2;3r\x1b[4h\x1bcabcd\x1b[1;2HX\x1b[3;1H\nZ");

        assert_eq!(row_text(&terminal, 0), "    ");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(row_text(&terminal, 2), "Z   ");
        assert_eq!(terminal.cursor(), (2, 1));
    }

    #[test]
    fn terminal_decaln_fills_visible_grid_with_e() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 3));

        terminal.feed(b"ab\ncd");
        terminal.take_damage();
        terminal.feed(b"\x1b#8");

        assert_eq!(row_text(&terminal, 0), "EEEE");
        assert_eq!(row_text(&terminal, 1), "EEEE");
        assert_eq!(row_text(&terminal, 2), "EEEE");
        assert_eq!(terminal.cursor(), (0, 0));
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 4, 3)]);
    }

    #[test]
    fn terminal_decaln_resets_scroll_region_and_origin_mode() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 5));

        terminal.feed(b"\x1b[2;4r\x1b[?6h\x1b#8");

        assert_eq!(terminal.scroll_region(), (0, 4));
        assert_eq!(terminal.cursor(), (0, 0));

        terminal.feed(b"\x1b[2;4r");
        assert_eq!(terminal.cursor(), (0, 0));
    }

    #[test]
    fn terminal_origin_mode_positions_cursor_relative_to_scroll_region() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 5));

        terminal.feed(b"\x1b[2;4r\x1b[?6h\x1b[1;1HZ\x1b[3;4HQ");

        assert_eq!(row_text(&terminal, 0), "    ");
        assert_eq!(row_text(&terminal, 1), "Z   ");
        assert_eq!(row_text(&terminal, 2), "    ");
        assert_eq!(row_text(&terminal, 3), "   Q");
        assert_eq!(row_text(&terminal, 4), "    ");
        assert_eq!(terminal.cursor(), (3, 3));
    }

    #[test]
    fn terminal_origin_mode_positions_cursor_relative_to_left_right_margins() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 5));

        terminal.feed(b"\x1b[2;4r\x1b[?69h\x1b[3;6s\x1b[?6h\x1b[1;1HZ\x1b[3;4HQ");

        assert_eq!(row_text(&terminal, 0), "        ");
        assert_eq!(row_text(&terminal, 1), "  Z     ");
        assert_eq!(row_text(&terminal, 2), "        ");
        assert_eq!(row_text(&terminal, 3), "     Q  ");
        assert_eq!(row_text(&terminal, 4), "        ");
        assert_eq!(terminal.cursor(), (3, 6));
    }

    #[test]
    fn terminal_origin_mode_reset_restores_absolute_cursor_positioning() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 5));

        terminal.feed(b"\x1b[2;4r\x1b[?6h\x1b[1;1HZ\x1b[?6l\x1b[1;1HQ");

        assert_eq!(row_text(&terminal, 0), "Q   ");
        assert_eq!(row_text(&terminal, 1), "Z   ");
        assert_eq!(row_text(&terminal, 2), "    ");
        assert_eq!(row_text(&terminal, 3), "    ");
        assert_eq!(row_text(&terminal, 4), "    ");
        assert_eq!(terminal.cursor(), (0, 1));
    }

    #[test]
    fn terminal_inserts_lines_with_il() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444");
        terminal.feed(b"\x1b[2;1H\x1b[2L");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(row_text(&terminal, 2), "    ");
        assert_eq!(row_text(&terminal, 3), "2222");
        assert_eq!(terminal.cursor(), (1, 0));
    }

    #[test]
    fn terminal_deletes_lines_with_dl() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444");
        terminal.feed(b"\x1b[2;1H\x1b[2M");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "4444");
        assert_eq!(row_text(&terminal, 2), "    ");
        assert_eq!(row_text(&terminal, 3), "    ");
        assert_eq!(terminal.cursor(), (1, 0));
    }

    #[test]
    fn terminal_inserts_lines_only_within_scroll_region() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 5));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444\x1b[5;1H5555");
        terminal.feed(b"\x1b[2;4r\x1b[3;1H\x1b[L");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "2222");
        assert_eq!(row_text(&terminal, 2), "    ");
        assert_eq!(row_text(&terminal, 3), "3333");
        assert_eq!(row_text(&terminal, 4), "5555");
        assert_eq!(terminal.cursor(), (2, 0));
    }

    #[test]
    fn terminal_deletes_lines_only_within_scroll_region() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 5));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444\x1b[5;1H5555");
        terminal.feed(b"\x1b[2;4r\x1b[2;1H\x1b[M");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "3333");
        assert_eq!(row_text(&terminal, 2), "4444");
        assert_eq!(row_text(&terminal, 3), "    ");
        assert_eq!(row_text(&terminal, 4), "5555");
        assert_eq!(terminal.cursor(), (1, 0));
    }

    #[test]
    fn terminal_scrolls_up_with_su() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444");
        terminal.feed(b"\x1b[2S");

        assert_eq!(row_text(&terminal, 0), "3333");
        assert_eq!(row_text(&terminal, 1), "4444");
        assert_eq!(row_text(&terminal, 2), "    ");
        assert_eq!(row_text(&terminal, 3), "    ");
        assert_eq!(terminal.cursor(), (3, 3));
    }

    #[test]
    fn terminal_scrolls_up_kitty_inline_images_with_text() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 3));

        terminal.feed(b"\x1b[1;1H11111111\x1b[2;1H22222222\x1b[3;1H33333333");
        terminal.feed(b"\x1b[2;3H");
        terminal.feed(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");

        terminal.feed(b"\x1b[S");

        assert_eq!(row_text(&terminal, 0), "22222222");
        assert_eq!(row_text(&terminal, 1), "33333333");
        assert_eq!(row_text(&terminal, 2), "        ");
        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 2);
    }

    #[test]
    fn terminal_scrolls_up_drops_kitty_inline_images_scrolled_out() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 3));

        terminal.feed(b"\x1b[1;1H11111111\x1b[2;1H22222222\x1b[3;1H33333333");
        terminal.feed(b"\x1b[1;3H");
        terminal.feed(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");

        terminal.feed(b"\x1b[S");

        assert!(terminal.inline_images().is_empty());
    }

    #[test]
    fn terminal_scrolls_down_with_sd() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 4));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444");
        terminal.feed(b"\x1b[2T");

        assert_eq!(row_text(&terminal, 0), "    ");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(row_text(&terminal, 2), "1111");
        assert_eq!(row_text(&terminal, 3), "2222");
        assert_eq!(terminal.cursor(), (3, 3));
    }

    #[test]
    fn terminal_scrolls_down_kitty_inline_images_with_text() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 3));

        terminal.feed(b"\x1b[1;1H11111111\x1b[2;1H22222222\x1b[3;1H33333333");
        terminal.feed(b"\x1b[1;3H");
        terminal.feed(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");

        terminal.feed(b"\x1b[T");

        assert_eq!(row_text(&terminal, 0), "        ");
        assert_eq!(row_text(&terminal, 1), "11111111");
        assert_eq!(row_text(&terminal, 2), "22222222");
        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].row, 1);
        assert_eq!(terminal.inline_images()[0].column, 2);
    }

    #[test]
    fn terminal_scrolls_down_drops_kitty_inline_images_scrolled_out() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 3));

        terminal.feed(b"\x1b[1;1H11111111\x1b[2;1H22222222\x1b[3;1H33333333");
        terminal.feed(b"\x1b[3;3H");
        terminal.feed(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");

        terminal.feed(b"\x1b[T");

        assert!(terminal.inline_images().is_empty());
    }

    #[test]
    fn terminal_scrolls_up_and_down_only_within_scroll_region() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 5));

        terminal.feed(b"\x1b[1;1H1111\x1b[2;1H2222\x1b[3;1H3333\x1b[4;1H4444\x1b[5;1H5555");
        terminal.feed(b"\x1b[2;4r\x1b[S");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "3333");
        assert_eq!(row_text(&terminal, 2), "4444");
        assert_eq!(row_text(&terminal, 3), "    ");
        assert_eq!(row_text(&terminal, 4), "5555");

        terminal.feed(b"\x1b[T");

        assert_eq!(row_text(&terminal, 0), "1111");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(row_text(&terminal, 2), "3333");
        assert_eq!(row_text(&terminal, 3), "4444");
        assert_eq!(row_text(&terminal, 4), "5555");
    }

    #[test]
    fn terminal_scroll_region_moves_only_kitty_inline_images_inside_region() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 5));

        terminal.feed(b"\x1b[1;1H11111111\x1b[2;1H22222222\x1b[3;1H33333333\x1b[4;1H44444444\x1b[5;1H55555555");
        terminal.feed(b"\x1b[1;3H");
        terminal.feed(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");
        terminal.feed(b"\x1b[3;3H");
        terminal.feed(b"\x1b_Ga=T,i=8,f=24,s=1,v=1,c=1,r=1,C=1;AP8A\x1b\\");

        terminal.feed(b"\x1b[2;4r\x1b[S");

        let outside = terminal
            .inline_images()
            .iter()
            .find(|image| image.kitty_image_id == Some(7))
            .expect("outside image should remain placed");
        let inside = terminal
            .inline_images()
            .iter()
            .find(|image| image.kitty_image_id == Some(8))
            .expect("inside image should remain placed");

        assert_eq!(outside.row, 0);
        assert_eq!(outside.column, 2);
        assert_eq!(inside.row, 1);
        assert_eq!(inside.column, 2);
    }

    #[test]
    fn terminal_switches_to_alternate_screen_and_restores_main_screen() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 2));

        terminal.feed(b"main\x1b[?1049halt\x1b[?1049l!");

        assert_eq!(row_text(&terminal, 0), "main! ");
        assert_eq!(row_text(&terminal, 1), "      ");
        assert_eq!(terminal.cursor(), (0, 5));
    }

    #[test]
    fn terminal_alternate_screen_starts_clear_and_is_discarded_on_exit() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 2));

        terminal.feed(b"main\x1b[?1049h");

        assert_eq!(row_text(&terminal, 0), "      ");
        assert_eq!(terminal.cursor(), (0, 0));

        terminal.feed(b"alt\x1b[?1049l");

        assert_eq!(row_text(&terminal, 0), "main  ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_alternate_screen_hides_and_restores_main_inline_images() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");
        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[?1049h");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b[?1049l");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 0);
    }

    #[test]
    fn terminal_alternate_screen_discards_alternate_inline_images_on_exit() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"main\x1b[?1049h");
        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1,C=1;/wAA\x1b\\");
        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[?1049l");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(row_text(&terminal, 0), "main    ");
    }

    #[test]
    fn terminal_supports_1047_alternate_screen_mode() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 2));

        terminal.feed(b"main\x1b[?1047halt\x1b[?1047l!");

        assert_eq!(row_text(&terminal, 0), "main! ");
        assert_eq!(row_text(&terminal, 1), "      ");
        assert_eq!(terminal.cursor(), (0, 5));
    }

    #[test]
    fn terminal_supports_legacy_47_alternate_screen_mode() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 2));

        terminal.feed(b"main\x1b[?47halt\x1b[?47l!");

        assert_eq!(row_text(&terminal, 0), "main! ");
        assert_eq!(row_text(&terminal, 1), "      ");
        assert_eq!(terminal.cursor(), (0, 5));
    }

    #[test]
    fn terminal_private_1048_saves_and_restores_cursor_without_alternate_screen() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 2));

        terminal.feed(b"ab\x1b[?1048hcd\x1b[2;1Hef\x1b[?1048lZ");

        assert_eq!(row_text(&terminal, 0), "abZd  ");
        assert_eq!(row_text(&terminal, 1), "ef    ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_tracks_cursor_visibility_private_mode() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        assert!(terminal.cursor_visible());

        terminal.feed(b"\x1b[?25l");
        assert!(!terminal.cursor_visible());

        terminal.feed(b"\x1b[?25h");
        assert!(terminal.cursor_visible());
    }

    #[test]
    fn terminal_tracks_cursor_blinking_private_mode_and_decscusr() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        assert!(!terminal.cursor_blinking());

        terminal.feed(b"\x1b[?12h");
        assert!(terminal.cursor_blinking());

        terminal.feed(b"\x1b[?12l");
        assert!(!terminal.cursor_blinking());

        terminal.feed(b"\x1b[5 q");
        assert_eq!(terminal.cursor_shape(), CursorShape::Bar);
        assert!(terminal.cursor_blinking());

        terminal.feed(b"\x1b[6 q");
        assert_eq!(terminal.cursor_shape(), CursorShape::Bar);
        assert!(!terminal.cursor_blinking());
    }

    #[test]
    fn terminal_soft_reset_restores_modes_without_clearing_cells() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 3));

        terminal.feed(b"abc\x1b[2;3r\x1b[?6h\x1b[4h\x1b(0\x1b[!p\x1b[1;2HZq");

        assert_eq!(row_text(&terminal, 0), "aZq   ");
        assert_eq!(terminal.scroll_region(), (0, 2));
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_soft_reset_restores_default_style_for_subsequent_cells() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"\x1b[1;31mA\x1b[!pB");

        let styled = terminal.grid().get(0, 0).unwrap();
        assert_eq!(styled.foreground, Color::Indexed(1));
        assert!(styled.bold);

        let reset = terminal.grid().get(0, 1).unwrap();
        assert_eq!(reset.ch, 'B');
        assert_eq!(reset.foreground, Color::Default);
        assert!(!reset.bold);
    }

    #[test]
    fn terminal_soft_reset_removes_kitty_placements_and_data() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,C=1\x1b\\");
        terminal.take_kitty_graphics_responses();
        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[!p");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b_Ga=p,i=7,p=2\x1b\\");
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7,p=2;ENOENT:No image with id 7\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_soft_reset_exits_alternate_screen() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"main\x1b[?1049halt\x1b[!p!");

        assert_eq!(row_text(&terminal, 0), "main! ");
        assert_eq!(terminal.cursor(), (0, 5));
    }

    #[test]
    fn terminal_tracks_decscusr_cursor_shape() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        assert_eq!(terminal.cursor_shape(), CursorShape::Block);

        terminal.feed(b"\x1b[6 q");
        assert_eq!(terminal.cursor_shape(), CursorShape::Bar);

        terminal.feed(b"\x1b[4 q");
        assert_eq!(terminal.cursor_shape(), CursorShape::Underline);

        terminal.feed(b"\x1b[0 q");
        assert_eq!(terminal.cursor_shape(), CursorShape::Block);
    }

    #[test]
    fn terminal_uses_configured_default_cursor_style_for_decscusr_reset() {
        let mut terminal = Terminal::new_with_default_cursor_style(
            TerminalSize::new(4, 1),
            CursorStyle::BlinkingUnderline,
        );

        assert_eq!(terminal.cursor_shape(), CursorShape::Underline);
        assert!(terminal.cursor_blinking());

        terminal.feed(b"\x1b[6 q");
        assert_eq!(terminal.cursor_shape(), CursorShape::Bar);
        assert!(!terminal.cursor_blinking());

        terminal.feed(b"\x1b[0 q");
        assert_eq!(terminal.cursor_shape(), CursorShape::Underline);
        assert!(terminal.cursor_blinking());
    }

    #[test]
    fn terminal_full_reset_restores_configured_default_cursor_style() {
        let mut terminal = Terminal::new_with_default_cursor_style(
            TerminalSize::new(4, 1),
            CursorStyle::SteadyBar,
        );

        terminal.feed(b"\x1b[3 q\x1bc");

        assert_eq!(terminal.cursor_shape(), CursorShape::Bar);
        assert!(!terminal.cursor_blinking());
    }

    #[test]
    fn terminal_delays_auto_wrap_until_next_printable_character() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"abcd");

        assert_eq!(row_text(&terminal, 0), "abcd");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(terminal.cursor(), (0, 3));

        terminal.feed(b"e");

        assert_eq!(row_text(&terminal, 0), "abcd");
        assert_eq!(row_text(&terminal, 1), "e   ");
        assert_eq!(terminal.cursor(), (1, 1));
    }

    #[test]
    fn terminal_auto_wrap_scrolls_at_bottom_row() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"abcdefghi");

        assert_eq!(row_text(&terminal, 0), "efgh");
        assert_eq!(row_text(&terminal, 1), "i   ");
        assert_eq!(terminal.cursor(), (1, 1));
    }

    #[test]
    fn terminal_wrap_mode_can_disable_auto_wrap_at_right_edge() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"abc\x1b[?7ldef");

        assert_eq!(row_text(&terminal, 0), "abcf");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_wrap_mode_can_reenable_auto_wrap_at_right_edge() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"ab\x1b[?7lcd\x1b[?7hef");

        assert_eq!(row_text(&terminal, 0), "abce");
        assert_eq!(row_text(&terminal, 1), "f   ");
        assert_eq!(terminal.cursor(), (1, 1));
    }

    #[test]
    fn terminal_soft_reset_reenables_auto_wrap_at_right_edge() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"ab\x1b[?7lcd\x1b[!pef");

        assert_eq!(row_text(&terminal, 0), "abce");
        assert_eq!(row_text(&terminal, 1), "f   ");
        assert_eq!(terminal.cursor(), (1, 1));
    }

    #[test]
    fn terminal_reverse_wrap_mode_wraps_backspace_to_previous_line() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 3));

        terminal.feed(b"abcd\x1b[2;1H\x1b[?45h\x08Z");

        assert_eq!(row_text(&terminal, 0), "abcZ");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_soft_reset_disables_reverse_wrap_at_left_edge() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"abcd\x1b[2;1H\x1b[?45h\x1b[!p\x08Z");

        assert_eq!(row_text(&terminal, 0), "abcd");
        assert_eq!(row_text(&terminal, 1), "Z   ");
        assert_eq!(terminal.cursor(), (1, 1));
    }

    #[test]
    fn terminal_soft_reset_disables_screen_reverse_video() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"\x1b[?5h");
        assert!(terminal.screen_reverse_video());

        terminal.feed(b"\x1b[!p");
        assert!(!terminal.screen_reverse_video());
    }

    #[test]
    fn terminal_soft_reset_restores_cursor_visibility() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"\x1b[?25l");
        assert!(!terminal.cursor_visible());

        terminal.feed(b"\x1b[!p");
        assert!(terminal.cursor_visible());
    }

    #[test]
    fn terminal_consumes_application_keypad_escape_sequences() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"ab\x1b=cd\x1b>ef");

        assert_eq!(row_text(&terminal, 0), "abcdef  ");
        assert_eq!(terminal.cursor(), (0, 6));
    }

    #[test]
    fn terminal_applies_basic_sgr_colors_and_styles() {
        let mut terminal = Terminal::new(TerminalSize::new(5, 1));

        terminal.feed(b"\x1b[1;31mR\x1b[0mD");

        let red = terminal.grid().get(0, 0).unwrap();
        assert_eq!(red.ch, 'R');
        assert_eq!(red.foreground, Color::Indexed(1));
        assert!(red.bold);

        let default = terminal.grid().get(0, 1).unwrap();
        assert_eq!(default.ch, 'D');
        assert_eq!(default.foreground, Color::Default);
        assert!(!default.bold);
    }

    #[test]
    fn terminal_applies_colon_separated_sgr_extended_colors() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));

        terminal.feed(b"\x1b[38:5:196mF\x1b[48:2::1:2:3mB");

        let foreground = terminal.grid().get(0, 0).unwrap();
        assert_eq!(foreground.ch, 'F');
        assert_eq!(foreground.foreground, Color::Indexed(196));

        let background = terminal.grid().get(0, 1).unwrap();
        assert_eq!(background.ch, 'B');
        assert_eq!(background.background, Color::Rgb(1, 2, 3));
    }

    #[test]
    fn terminal_applies_wezterm_sgr_rgba_colors() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[38:6::1:2:3:4mF\x1b[48:6:5:6:7:8mB\x1b[58;6;9;10;11;12mU");

        let foreground = terminal.grid().get(0, 0).unwrap();
        assert_eq!(foreground.ch, 'F');
        assert_eq!(foreground.foreground, Color::Rgba(1, 2, 3, 4));

        let background = terminal.grid().get(0, 1).unwrap();
        assert_eq!(background.ch, 'B');
        assert_eq!(background.background, Color::Rgba(5, 6, 7, 8));

        let underline = terminal.grid().get(0, 2).unwrap();
        assert_eq!(underline.ch, 'U');
        assert_eq!(underline.underline_color, Color::Rgba(9, 10, 11, 12));
    }

    #[test]
    fn terminal_keeps_sgr_params_after_semicolon_truecolor() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));

        terminal.feed(b"\x1b[38;2;1;2;3;1mA");

        let cell = terminal.grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'A');
        assert_eq!(cell.foreground, Color::Rgb(1, 2, 3));
        assert!(cell.bold);
    }

    #[test]
    fn terminal_applies_sgr_inverse_video() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[7mA\x1b[27mB");

        let inverse = terminal.grid().get(0, 0).unwrap();
        assert_eq!(inverse.ch, 'A');
        assert!(inverse.inverse);

        let normal = terminal.grid().get(0, 1).unwrap();
        assert_eq!(normal.ch, 'B');
        assert!(!normal.inverse);
    }

    #[test]
    fn terminal_applies_sgr_strikethrough() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[9mA\x1b[29mB");

        let struck = terminal.grid().get(0, 0).unwrap();
        assert_eq!(struck.ch, 'A');
        assert!(struck.strikethrough);

        let normal = terminal.grid().get(0, 1).unwrap();
        assert_eq!(normal.ch, 'B');
        assert!(!normal.strikethrough);
    }

    #[test]
    fn terminal_applies_sgr_faint() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[2mA\x1b[22mB");

        let faint = terminal.grid().get(0, 0).unwrap();
        assert_eq!(faint.ch, 'A');
        assert!(faint.faint);

        let normal = terminal.grid().get(0, 1).unwrap();
        assert_eq!(normal.ch, 'B');
        assert!(!normal.faint);
    }

    #[test]
    fn terminal_applies_sgr_conceal() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[8mA\x1b[28mB");

        let concealed = terminal.grid().get(0, 0).unwrap();
        assert_eq!(concealed.ch, 'A');
        assert!(concealed.conceal);

        let normal = terminal.grid().get(0, 1).unwrap();
        assert_eq!(normal.ch, 'B');
        assert!(!normal.conceal);
    }

    #[test]
    fn terminal_applies_sgr_overline() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[53mA\x1b[55mB");

        let overlined = terminal.grid().get(0, 0).unwrap();
        assert_eq!(overlined.ch, 'A');
        assert!(overlined.overline);

        let normal = terminal.grid().get(0, 1).unwrap();
        assert_eq!(normal.ch, 'B');
        assert!(!normal.overline);
    }

    #[test]
    fn terminal_applies_wezterm_sgr_vertical_align() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[73mS\x1b[74mD\x1b[75mB");

        let superscript = terminal.grid().get(0, 0).unwrap();
        assert_eq!(superscript.ch, 'S');
        assert_eq!(superscript.vertical_align, VerticalAlign::Superscript);

        let subscript = terminal.grid().get(0, 1).unwrap();
        assert_eq!(subscript.ch, 'D');
        assert_eq!(subscript.vertical_align, VerticalAlign::Subscript);

        let baseline = terminal.grid().get(0, 2).unwrap();
        assert_eq!(baseline.ch, 'B');
        assert_eq!(baseline.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn terminal_applies_sgr_blink() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[5mA\x1b[25mB");

        let blinking = terminal.grid().get(0, 0).unwrap();
        assert_eq!(blinking.ch, 'A');
        assert!(blinking.blink);

        let normal = terminal.grid().get(0, 1).unwrap();
        assert_eq!(normal.ch, 'B');
        assert!(!normal.blink);
    }

    #[test]
    fn terminal_applies_sgr_rapid_blink_as_blink() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[6mA\x1b[25mB");

        let blinking = terminal.grid().get(0, 0).unwrap();
        assert_eq!(blinking.ch, 'A');
        assert!(blinking.blink);

        let normal = terminal.grid().get(0, 1).unwrap();
        assert_eq!(normal.ch, 'B');
        assert!(!normal.blink);
    }

    #[test]
    fn terminal_preserves_sgr_rapid_blink_attribute() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"\x1b[5mA\x1b[6mB\x1b[25mC");

        let normal_blink = terminal.grid().get(0, 0).unwrap();
        let rapid_blink = terminal.grid().get(0, 1).unwrap();
        let plain = terminal.grid().get(0, 2).unwrap();

        assert!(normal_blink.blink);
        assert!(!normal_blink.rapid_blink);
        assert!(rapid_blink.blink);
        assert!(rapid_blink.rapid_blink);
        assert!(!plain.blink);
        assert!(!plain.rapid_blink);
    }

    #[test]
    fn terminal_applies_sgr_double_underline() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"\x1b[4mA\x1b[21mB\x1b[24mC");

        let single = terminal.grid().get(0, 0).unwrap();
        assert_eq!(single.ch, 'A');
        assert!(single.underline);
        assert!(!single.double_underline);

        let double = terminal.grid().get(0, 1).unwrap();
        assert_eq!(double.ch, 'B');
        assert!(!double.underline);
        assert!(double.double_underline);

        let normal = terminal.grid().get(0, 2).unwrap();
        assert_eq!(normal.ch, 'C');
        assert!(!normal.underline);
        assert!(!normal.double_underline);
    }

    #[test]
    fn terminal_applies_sgr_underline_color() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));

        terminal.feed(b"\x1b[4;58;5;196mA\x1b[59mB");

        let colored = terminal.grid().get(0, 0).unwrap();
        assert_eq!(colored.ch, 'A');
        assert!(colored.underline);
        assert_eq!(colored.underline_color, Color::Indexed(196));

        let default = terminal.grid().get(0, 1).unwrap();
        assert_eq!(default.ch, 'B');
        assert_eq!(default.underline_color, Color::Default);
    }

    #[test]
    fn terminal_applies_colon_separated_sgr_underline_styles() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"\x1b[4:2mD\x1b[4:0mN\x1b[4:3mC\x1b[4:4mO\x1b[4:5mA\x1b[4:1mS");

        let double = terminal.grid().get(0, 0).unwrap();
        assert_eq!(double.ch, 'D');
        assert_eq!(double.underline_style, UnderlineStyle::Double);
        assert!(double.double_underline);
        assert!(!double.faint, "SGR 4:2 must not leak the 2 as faint");

        let none = terminal.grid().get(0, 1).unwrap();
        assert_eq!(none.ch, 'N');
        assert_eq!(none.underline_style, UnderlineStyle::None);
        assert!(!none.underline);
        assert!(!none.double_underline);

        let curly = terminal.grid().get(0, 2).unwrap();
        assert_eq!(curly.underline_style, UnderlineStyle::Curly);
        assert!(curly.underline);

        let dotted = terminal.grid().get(0, 3).unwrap();
        assert_eq!(dotted.underline_style, UnderlineStyle::Dotted);
        assert!(dotted.underline);

        let dashed = terminal.grid().get(0, 4).unwrap();
        assert_eq!(dashed.underline_style, UnderlineStyle::Dashed);
        assert!(dashed.underline);

        let single = terminal.grid().get(0, 5).unwrap();
        assert_eq!(single.underline_style, UnderlineStyle::Single);
        assert!(single.underline);
        assert!(!single.bold, "SGR 4:1 must not leak the 1 as bold");
    }

    #[test]
    fn terminal_places_wide_cjk_character_across_two_columns() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed("中x".as_bytes());

        assert_eq!(terminal.grid().get(0, 0).unwrap().ch, '中');
        assert_eq!(terminal.grid().get(0, 1).unwrap().ch, ' ');
        assert_eq!(terminal.grid().get(0, 2).unwrap().ch, 'x');
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_handles_split_utf8_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));
        let bytes = "中".as_bytes();

        terminal.feed(&bytes[..1]);

        assert_eq!(row_text(&terminal, 0), "    ");
        assert_eq!(terminal.cursor(), (0, 0));
        assert!(terminal.take_damage().is_empty());

        terminal.feed(&bytes[1..]);

        assert_eq!(row_text(&terminal, 0), "中   ");
        assert_eq!(terminal.cursor(), (0, 2));
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 2, 1)]);
    }

    #[test]
    fn terminal_resize_expands_grid_and_preserves_visible_cells() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"abcd\r\nef");
        terminal.take_damage();

        terminal.resize(TerminalSize::new(6, 3));

        assert_eq!(terminal.grid().size(), TerminalSize::new(6, 3));
        assert_eq!(row_text(&terminal, 0), "abcd  ");
        assert_eq!(row_text(&terminal, 1), "ef    ");
        assert_eq!(row_text(&terminal, 2), "      ");
        assert_eq!(terminal.cursor(), (1, 2));
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 6, 3)]);
    }

    #[test]
    fn terminal_resize_shrinks_grid_and_clamps_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(5, 3));
        terminal.feed(b"abcde\x1b[2;1Hfghij\x1b[3;5HZ");
        terminal.take_damage();

        terminal.resize(TerminalSize::new(3, 2));

        assert_eq!(terminal.grid().size(), TerminalSize::new(3, 2));
        assert_eq!(row_text(&terminal, 0), "abc");
        assert_eq!(row_text(&terminal, 1), "fgh");
        assert_eq!(terminal.cursor(), (1, 2));
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 3, 2)]);
    }

    #[test]
    fn terminal_reports_merged_damage_for_written_text() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 1));

        terminal.feed(b"abc");

        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 3, 1)]);
        assert!(terminal.take_damage().is_empty());
    }

    #[test]
    fn terminal_reports_wide_character_damage() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed("中".as_bytes());

        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 2, 1)]);
    }

    #[test]
    fn terminal_positions_cursor_with_cup_and_hvp() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 3));

        terminal.feed(b"\x1b[2;3HZ\x1b[3;1fQ");

        assert_eq!(terminal.grid().get(1, 2).unwrap().ch, 'Z');
        assert_eq!(terminal.grid().get(2, 0).unwrap().ch, 'Q');
        assert_eq!(terminal.cursor(), (2, 1));
    }

    #[test]
    fn terminal_moves_cursor_with_relative_csi_commands() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 3));

        terminal.feed(b"ab\r\ncd\x1b[A\x1b[CX\x1b[B\x1b[DY");

        assert_eq!(terminal.grid().get(0, 3).unwrap().ch, 'X');
        assert_eq!(terminal.grid().get(1, 3).unwrap().ch, 'Y');
        assert_eq!(terminal.cursor(), (1, 4));
    }

    #[test]
    fn terminal_moves_cursor_with_additional_csi_absolute_controls() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 4));

        terminal.feed(b"abcdef\x1b[4GZ\x1b[3dQ\x1b[2Ers\x1b[Ft");

        assert_eq!(row_text(&terminal, 0), "abcZef  ");
        assert_eq!(row_text(&terminal, 1), "        ");
        assert_eq!(row_text(&terminal, 2), "t   Q   ");
        assert_eq!(row_text(&terminal, 3), "rs      ");
        assert_eq!(terminal.cursor(), (2, 1));
    }

    #[test]
    fn terminal_moves_cursor_with_additional_csi_relative_controls() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 4));

        terminal.feed(b"\x1b[2;2H\x1b[3aX\x1b[2eY\x1b[1`Z");

        assert_eq!(row_text(&terminal, 0), "        ");
        assert_eq!(row_text(&terminal, 1), "    X   ");
        assert_eq!(row_text(&terminal, 2), "        ");
        assert_eq!(row_text(&terminal, 3), "Z    Y  ");
        assert_eq!(terminal.cursor(), (3, 1));
    }

    #[test]
    fn terminal_handles_split_csi_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"ab\x1b[");
        terminal.feed(b"2Dcd");

        assert_eq!(row_text(&terminal, 0), "cd    ");
        assert_eq!(terminal.cursor(), (0, 2));
    }

    #[test]
    fn terminal_can_cancels_csi_sequence() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"ab\x1b[2\x18cd");

        assert_eq!(row_text(&terminal, 0), "abcd    ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_sub_cancels_split_csi_sequence() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"ab\x1b[2");
        terminal.feed(b"\x1acd");

        assert_eq!(row_text(&terminal, 0), "abcd    ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_erases_line_from_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"abcdef\x1b[3D\x1b[K");

        assert_eq!(row_text(&terminal, 0), "abc     ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_erase_line_uses_current_background_color() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b[42mabcdef\x1b[3D\x1b[K");

        assert_eq!(row_text(&terminal, 0), "abc     ");
        assert_eq!(
            terminal.grid().get(0, 3).unwrap().background,
            Color::Indexed(2)
        );
        assert_eq!(
            terminal.grid().get(0, 7).unwrap().background,
            Color::Indexed(2)
        );
    }

    #[test]
    fn terminal_erase_characters_use_current_background_color() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"\x1b[41mabcdef\x1b[3D\x1b[2X");

        assert_eq!(row_text(&terminal, 0), "ab  ef");
        assert_eq!(
            terminal.grid().get(0, 2).unwrap().background,
            Color::Indexed(1)
        );
        assert_eq!(
            terminal.grid().get(0, 3).unwrap().background,
            Color::Indexed(1)
        );
    }

    #[test]
    fn terminal_scrolling_uses_current_background_color_for_new_rows() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"\x1b[44mab\r\ncd\r\n");

        assert_eq!(row_text(&terminal, 0), "cd  ");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(
            terminal.grid().get(1, 0).unwrap().background,
            Color::Indexed(4)
        );
        assert_eq!(
            terminal.grid().get(1, 3).unwrap().background,
            Color::Indexed(4)
        );
    }

    #[test]
    fn terminal_inserts_blank_characters_with_ich() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"abcdef\x1b[4D\x1b[2@");

        assert_eq!(row_text(&terminal, 0), "a  bcd");
        assert_eq!(terminal.cursor(), (0, 1));
    }

    #[test]
    fn terminal_insert_blank_characters_use_current_background_color() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"\x1b[45mabcdef\x1b[4D\x1b[2@");

        assert_eq!(row_text(&terminal, 0), "a  bcd");
        assert_eq!(
            terminal.grid().get(0, 1).unwrap().background,
            Color::Indexed(5)
        );
        assert_eq!(
            terminal.grid().get(0, 2).unwrap().background,
            Color::Indexed(5)
        );
    }

    #[test]
    fn terminal_insert_mode_shifts_printable_characters() {
        let mut terminal = Terminal::new(TerminalSize::new(7, 1));

        terminal.feed(b"abcd\x1b[1;2H\x1b[4hXY\x1b[4lZ");

        assert_eq!(row_text(&terminal, 0), "aXYZcd ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_deletes_characters_with_dch() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"abcdef\x1b[3D\x1b[2P");

        assert_eq!(row_text(&terminal, 0), "abef  ");
        assert_eq!(terminal.cursor(), (0, 2));
    }

    #[test]
    fn terminal_delete_characters_use_current_background_color_for_exposed_blanks() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"\x1b[46mabcdef\x1b[3D\x1b[2P");

        assert_eq!(row_text(&terminal, 0), "abef  ");
        assert_eq!(
            terminal.grid().get(0, 4).unwrap().background,
            Color::Indexed(6)
        );
        assert_eq!(
            terminal.grid().get(0, 5).unwrap().background,
            Color::Indexed(6)
        );
    }

    #[test]
    fn terminal_erases_characters_with_ech() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"abcdef\x1b[3D\x1b[2X");

        assert_eq!(row_text(&terminal, 0), "ab  ef");
        assert_eq!(terminal.cursor(), (0, 2));
    }

    #[test]
    fn terminal_repeats_previous_character_with_rep() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"A\x1b[3bZ");

        assert_eq!(row_text(&terminal, 0), "AAAAZ   ");
        assert_eq!(terminal.cursor(), (0, 5));
    }

    #[test]
    fn terminal_repeats_dec_special_graphics_with_rep() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b(0q\x1b[4b\x1b(Bx");

        assert_eq!(row_text(&terminal, 0), "─────x  ");
        assert_eq!(terminal.cursor(), (0, 6));
    }

    #[test]
    fn terminal_erases_entire_display() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"abcd\r\nef\x1b[2J");

        assert_eq!(row_text(&terminal, 0), "    ");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(terminal.cursor(), (1, 2));
    }

    #[test]
    fn terminal_erase_display_mode_2_removes_visible_inline_images_but_keeps_stored_data() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,C=1\x1b\\");
        terminal.take_kitty_graphics_responses();
        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[2J");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=7,C=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(7));
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_tracks_osc_title_terminated_by_bel_without_rendering_it() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]0;cmd.exe\x07cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.title(), Some("cmd.exe"));
    }

    #[test]
    fn terminal_tracks_osc_title_terminated_by_st_without_rendering_it() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]2;PowerShell\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.title(), Some("PowerShell"));
    }

    #[test]
    fn terminal_tracks_osc1_icon_title_without_rendering_it() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]1;tab-title\x07cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.title(), Some("tab-title"));
    }

    #[test]
    fn terminal_tracks_icon_and_window_titles_separately() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"\x1b]1;icon-name\x07\x1b]2;window-name\x07");

        assert_eq!(terminal.icon_title(), Some("icon-name"));
        assert_eq!(terminal.window_title(), Some("window-name"));
        assert_eq!(terminal.title(), Some("window-name"));
    }

    #[test]
    fn terminal_tracks_sun_osc_title_aliases_without_rendering_them() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]Ltab-title\x1b\\cd");
        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.title(), Some("tab-title"));

        terminal.feed(b"\x1b]lwin-title\x07ef");
        assert_eq!(row_text(&terminal, 0), "abcdef      ");
        assert_eq!(terminal.cursor(), (0, 6));
        assert_eq!(terminal.title(), Some("win-title"));
    }

    #[test]
    fn terminal_saves_and_restores_title_stack_without_rendering_controls() {
        let mut terminal = Terminal::new(TerminalSize::new(16, 1));

        terminal.feed(b"ab\x1b]0;main\x07cd\x1b[22;0;0t\x1b]0;alternate\x07ef\x1b[23;0;0t");

        assert_eq!(row_text(&terminal, 0), "abcdef          ");
        assert_eq!(terminal.cursor(), (0, 6));
        assert_eq!(terminal.title(), Some("main"));
    }

    #[test]
    fn terminal_tracks_osc7_current_working_directory() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab\x1b]7;file://host/home/ops\x07cd");

        assert_eq!(row_text(&terminal, 0), "abcd                    ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.current_working_dir(), Some("file://host/home/ops"));
    }

    #[test]
    fn terminal_tracks_iterm_current_dir_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab\x1b]1337;CurrentDir=/tmp/project\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd                    ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.current_working_dir(), Some("/tmp/project"));
    }

    #[test]
    fn terminal_tracks_iterm_set_user_var_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab\x1b]1337;SetUserVar=WEZTERM_PROG=YmFy\x07cd");

        assert_eq!(row_text(&terminal, 0), "abcd                    ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(
            terminal.user_vars().get("WEZTERM_PROG").map(String::as_str),
            Some("bar")
        );
    }

    #[test]
    fn terminal_tracks_iterm_set_badge_format_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab\x1b]1337;SetBadgeFormat=aGVsbG8=\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd                    ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.badge_format(), Some("hello"));
    }

    #[test]
    fn terminal_tracks_iterm_inline_image_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(
            b"ab\x1b]1337;File=inline=1;name=aW1nLnBuZw==;size=4;width=10px;height=2;preserveAspectRatio=0:QUJDRA==\x07cd",
        );

        assert_eq!(row_text(&terminal, 0), "abcd                    ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(
            terminal.inline_images(),
            &[ItermInlineImage {
                row: 0,
                column: 2,
                name: Some("img.png".to_owned()),
                kitty_image_id: None,
                kitty_placement_id: None,
                kitty_z_index: None,
                size: Some(4),
                width: Some("10px".to_owned()),
                height: Some("2".to_owned()),
                preserve_aspect_ratio: Some(false),
                image_format: InlineImageFormat::Encoded,
                pixel_width: None,
                pixel_height: None,
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: b"ABCD".to_vec(),
            }]
        );
    }

    #[test]
    fn terminal_tracks_kitty_rgb_inline_image_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert_eq!(
            terminal.inline_images(),
            &[ItermInlineImage {
                row: 0,
                column: 2,
                name: None,
                kitty_image_id: Some(0),
                kitty_placement_id: None,
                kitty_z_index: Some(0),
                size: Some(3),
                width: Some("1".to_owned()),
                height: Some("1".to_owned()),
                preserve_aspect_ratio: None,
                image_format: InlineImageFormat::Rgb,
                pixel_width: Some(1),
                pixel_height: Some(1),
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: vec![255, 0, 0],
            }]
        );
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(2, 0, 1, 1)]);

        terminal.feed(b"cd");
        assert_eq!(row_text(&terminal, 0), "ab cd                   ");
    }

    #[test]
    fn terminal_decompresses_kitty_zlib_rgb_inline_image() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1,o=z;eJz7z8AAAAMAAQA=\x1b\\");

        assert_eq!(
            terminal.inline_images(),
            &[ItermInlineImage {
                row: 0,
                column: 2,
                name: None,
                kitty_image_id: Some(0),
                kitty_placement_id: None,
                kitty_z_index: Some(0),
                size: Some(3),
                width: Some("1".to_owned()),
                height: Some("1".to_owned()),
                preserve_aspect_ratio: None,
                image_format: InlineImageFormat::Rgb,
                pixel_width: Some(1),
                pixel_height: Some(1),
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: vec![255, 0, 0],
            }]
        );
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(2, 0, 1, 1)]);
    }

    #[test]
    fn terminal_moves_cursor_after_kitty_direct_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b[2;3H");
        terminal.feed(b"\x1b_Ga=T,f=24,s=3,v=2,c=3,r=2;/wAAAP8AAAD//wAAAP8AAAD/\x1b\\");

        assert_eq!(terminal.cursor(), (3, 5));
    }

    #[test]
    fn terminal_respects_kitty_no_cursor_movement_flag() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b[2;3H");
        terminal.feed(b"\x1b_Ga=T,f=24,s=3,v=2,c=3,r=2,C=1;/wAAAP8AAAD//wAAAP8AAAD/\x1b\\");

        assert_eq!(terminal.cursor(), (1, 2));
    }

    #[test]
    fn terminal_moves_cursor_after_kitty_stored_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=3,v=2,c=3,r=2;/wAAAP8AAAD//wAAAP8AAAD/\x1b\\");
        terminal.feed(b"\x1b[2;3H");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert_eq!(terminal.cursor(), (3, 5));
    }

    #[test]
    fn terminal_accumulates_kitty_chunked_rgb_until_final_chunk() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=2,v=1,c=2,r=1,m=1;/wAA\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert!(terminal.take_damage().is_empty());

        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Gm=0;AP8A\x1b\\");

        assert_eq!(
            terminal.inline_images(),
            &[ItermInlineImage {
                row: 0,
                column: 4,
                name: None,
                kitty_image_id: Some(0),
                kitty_placement_id: None,
                kitty_z_index: Some(0),
                size: Some(6),
                width: Some("2".to_owned()),
                height: Some("1".to_owned()),
                preserve_aspect_ratio: None,
                image_format: InlineImageFormat::Rgb,
                pixel_width: Some(2),
                pixel_height: Some(1),
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: vec![255, 0, 0, 0, 255, 0],
            }]
        );
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(4, 0, 2, 1)]);
    }

    #[test]
    fn terminal_answers_chunked_kitty_query_after_final_chunk_without_storing_image() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=q,i=31,f=24,s=2,v=1,c=2,r=1,m=1;/wAA\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert!(terminal.take_kitty_graphics_responses().is_empty());

        terminal.feed(b"\x1b_Gm=0;AP8A\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=31;OK\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b_Ga=p,i=31\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=31;ENOENT:No image with id 31\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_places_stored_kitty_rgb_image_by_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert!(terminal.take_damage().is_empty());

        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert_eq!(
            terminal.inline_images(),
            &[ItermInlineImage {
                row: 0,
                column: 4,
                name: None,
                kitty_image_id: Some(7),
                kitty_placement_id: None,
                kitty_z_index: Some(0),
                size: Some(3),
                width: Some("1".to_owned()),
                height: Some("1".to_owned()),
                preserve_aspect_ratio: None,
                image_format: InlineImageFormat::Rgb,
                pixel_width: Some(1),
                pixel_height: Some(1),
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: vec![255, 0, 0],
            }]
        );
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(4, 0, 1, 1)]);
    }

    #[test]
    fn terminal_derives_kitty_display_rows_from_columns_and_image_aspect_ratio() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(
            b"\x1b_Ga=T,C=1,q=1,i=70,f=24,s=4,v=2,c=4;/wAA/wAA/wAA/wAA/wAA/wAA/wAA/wAA\x1b\\",
        );

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].width, Some("4".to_owned()));
        assert_eq!(terminal.inline_images()[0].height, Some("2".to_owned()));
    }

    #[test]
    fn terminal_derives_kitty_display_columns_from_rows_and_image_aspect_ratio() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(
            b"\x1b_Ga=T,C=1,q=1,i=71,f=24,s=4,v=2,r=3;/wAA/wAA/wAA/wAA/wAA/wAA/wAA/wAA\x1b\\",
        );

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].width, Some("6".to_owned()));
        assert_eq!(terminal.inline_images()[0].height, Some("3".to_owned()));
    }

    #[test]
    fn terminal_displays_kitty_virtual_placement_from_unicode_placeholder() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,q=1,i=42,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b_Ga=p,q=1,U=1,i=42,c=2,r=2\x1b\\");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b[2;3H\x1b[38;5;42m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());

        assert_eq!(
            terminal.inline_images(),
            &[ItermInlineImage {
                row: 1,
                column: 2,
                name: None,
                kitty_image_id: Some(42),
                kitty_placement_id: None,
                kitty_z_index: Some(0),
                size: Some(12),
                width: Some("2".to_owned()),
                height: Some("2".to_owned()),
                preserve_aspect_ratio: None,
                image_format: InlineImageFormat::Rgb,
                pixel_width: Some(2),
                pixel_height: Some(2),
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
            }]
        );
    }

    #[test]
    fn terminal_combines_kitty_upload_with_virtual_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,i=43,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=43;OK\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b[3;4H\x1b[38;5;43m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(43));
        assert_eq!(terminal.inline_images()[0].row, 2);
        assert_eq!(terminal.inline_images()[0].column, 3);
        assert_eq!(terminal.inline_images()[0].width, Some("2".to_owned()));
        assert_eq!(terminal.inline_images()[0].height, Some("2".to_owned()));
        assert_eq!(
            terminal.inline_images()[0].data,
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]
        );
    }

    #[test]
    fn terminal_displays_kitty_virtual_placement_with_placeholder_image_id_msb() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));
        let image_id = (2_u32 << 24) | 0x2c;

        terminal.feed(
            format!("\x1b_Ga=T,U=1,q=1,i={image_id},f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\")
                .as_bytes(),
        );

        terminal.feed(b"\x1b[2;3H\x1b[38;5;44m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}\u{030e}".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(image_id));
        assert_eq!(terminal.inline_images()[0].row, 1);
        assert_eq!(terminal.inline_images()[0].column, 2);
    }

    #[test]
    fn terminal_displays_kitty_virtual_placement_from_row_only_first_column_placeholder() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=46,f=24,s=2,v=2,c=3,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[1;1H\x1b[38;5;46m");
        terminal.feed("\u{10eeee}\u{0305}\u{10eeee}\u{10eeee}".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(46));
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 0);
        assert_eq!(terminal.inline_images()[0].width, Some("3".to_owned()));
        assert_eq!(terminal.inline_images()[0].height, Some("2".to_owned()));
    }

    #[test]
    fn terminal_renders_kitty_virtual_placement_from_non_origin_placeholder_cell() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=47,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[1;2H\x1b[38;5;47m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}\u{10eeee}x".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(47));
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 0);
    }

    #[test]
    fn terminal_inherits_kitty_placeholder_coordinates_from_left_cell() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=48,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[1;2H\x1b[38;5;48m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}".as_bytes());
        terminal.feed(b"\x1b_Ga=d,d=a\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed("\u{10eeee}x".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(48));
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 0);
    }

    #[test]
    fn terminal_inherits_kitty_placeholder_coordinates_from_stored_left_cell() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=50,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[1;2H\x1b[38;5;50m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}".as_bytes());
        terminal.feed(b"\x1b[2;1Hx\x1b_Ga=d,d=a\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[1;3H\x1b[38;5;50m");
        terminal.feed("\u{10eeee}x".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(50));
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 0);
    }

    #[test]
    fn terminal_keeps_kitty_placeholder_render_after_graphics_delete_until_cell_overwritten() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=49,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[1;1H\x1b[38;5;49m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());
        terminal.feed(b"\x1b_Ga=d,d=a\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(49));

        terminal.feed(b"\x1b[1;1Hx");

        assert!(terminal.inline_images().is_empty());
    }

    #[test]
    fn terminal_clears_kitty_placeholder_metadata_when_cell_is_erased() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=53,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[1;2H\x1b[38;5;53m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}".as_bytes());
        terminal.feed(b"\x1b[1;2H\x1b[K\x1b_Ga=d,d=a\x1b\\");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b[1;3H\x1b[38;5;53m");
        terminal.feed("\u{10eeee}x".as_bytes());

        assert!(terminal.inline_images().is_empty());
    }

    #[test]
    fn terminal_clears_kitty_placeholder_metadata_on_reset() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=54,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[1;2H\x1b[38;5;54m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}".as_bytes());
        terminal.feed(b"\x1bc");
        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=54,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");

        terminal.feed(b"\x1b[1;3H\x1b[38;5;54m");
        terminal.feed("\u{10eeee}x".as_bytes());

        assert!(terminal.inline_images().is_empty());
    }

    #[test]
    fn terminal_scrolls_kitty_placeholder_metadata_with_region_cells() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=55,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[3;2H\x1b[38;5;55m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}".as_bytes());
        terminal.feed(b"\x1b[2;4r\x1b[4;1H\n\x1b_Ga=d,d=a\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[2;3H\x1b[38;5;55m");
        terminal.feed("\u{10eeee}x".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(55));
        assert_eq!(terminal.inline_images()[0].row, 1);
        assert_eq!(terminal.inline_images()[0].column, 0);
    }

    #[test]
    fn terminal_scrolls_kitty_placeholder_metadata_down_with_inserted_lines() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=56,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[2;2H\x1b[38;5;56m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}".as_bytes());
        terminal.feed(b"\x1b[1;1H\x1b[L\x1b_Ga=d,d=a\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[3;3H\x1b[38;5;56m");
        terminal.feed("\u{10eeee}x".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(56));
        assert_eq!(terminal.inline_images()[0].row, 2);
        assert_eq!(terminal.inline_images()[0].column, 0);
    }

    #[test]
    fn terminal_rebases_kitty_placeholder_metadata_when_scrollback_is_erased() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"one\ntwo\nthree\nfour\nfive\n");
        assert!(!terminal.scrollback().is_empty());

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=57,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[3;2H\x1b[38;5;57m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}".as_bytes());
        terminal.feed(b"\x1b[3J\x1b_Ga=d,d=a\x1b\\");

        assert!(terminal.scrollback().is_empty());
        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[3;3H\x1b[38;5;57m");
        terminal.feed("\u{10eeee}x".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(57));
        assert_eq!(terminal.inline_images()[0].row, 2);
        assert_eq!(terminal.inline_images()[0].column, 0);
    }

    #[test]
    fn terminal_restores_kitty_placeholder_metadata_after_alternate_screen() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=58,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b[1;2H\x1b[38;5;58m");
        terminal.feed("\u{10eeee}\u{0305}\u{030d}".as_bytes());
        terminal.feed(b"\x1b[?1049h\x1b[1;3H\x1b[38;5;58m");
        terminal.feed("\u{10eeee}x".as_bytes());

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b[?1049l\x1b_Ga=d,d=a\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);

        terminal.feed(b"\x1b[1;3H\x1b[38;5;58m");
        terminal.feed("\u{10eeee}x".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(58));
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 0);
    }

    #[test]
    fn terminal_deletes_kitty_virtual_placement_by_image_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=45,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b_Ga=d,d=i,i=45\x1b\\");

        terminal.feed(b"\x1b[2;3H\x1b[38;5;45m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b_Ga=q,i=45\x1b\\");

        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=45;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_deletes_kitty_virtual_placement_by_image_id_range() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=51,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b_Ga=d,d=r,x=50,y=52\x1b\\");

        terminal.feed(b"\x1b[2;3H\x1b[38;5;51m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b_Ga=q,i=51\x1b\\");

        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=51;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_keeps_kitty_image_data_when_visible_delete_leaves_virtual_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,q=1,i=52,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b_Ga=p,q=1,i=52\x1b\\");
        terminal.feed(b"\x1b_Ga=p,q=1,U=1,i=52,c=2,r=2\x1b\\");
        terminal.feed(b"\x1b[1;1H\x1b_Ga=d,d=C\x1b\\");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b[2;3H\x1b[38;5;52m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(52));
    }

    #[test]
    fn terminal_keeps_kitty_image_data_when_uppercase_all_delete_leaves_virtual_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,q=1,i=53,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
        terminal.feed(b"\x1b_Ga=p,q=1,i=53\x1b\\");
        terminal.feed(b"\x1b_Ga=p,q=1,U=1,i=53,c=2,r=2\x1b\\");
        terminal.feed(b"\x1b_Ga=d,d=A\x1b\\");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b[2;3H\x1b[38;5;53m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(53));
    }

    #[test]
    fn terminal_keeps_scrollback_kitty_placements_when_deleting_visible_placements() {
        for delete_target in ['a', 'A'] {
            let mut terminal = Terminal::new(TerminalSize::new(8, 2));

            terminal.feed(b"\x1b_Ga=T,C=1,q=1,i=60,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
            terminal.feed(b"one\ntwo\n");

            assert_eq!(terminal.scrollback().len(), 1);
            assert_eq!(terminal.inline_images().len(), 1);
            assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(60));
            assert_eq!(terminal.inline_images()[0].row, 0);

            terminal.feed(b"\x1b_Ga=T,C=1,q=1,i=61,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
            assert_eq!(terminal.inline_images().len(), 2);

            let delete = format!("\x1b_Ga=d,d={delete_target}\x1b\\");
            terminal.feed(delete.as_bytes());

            assert_eq!(terminal.inline_images().len(), 1);
            assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(60));
            assert!(terminal.inline_images()[0].row < terminal.scrollback().len());
        }
    }

    #[test]
    fn terminal_acknowledges_stored_kitty_image_upload_by_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_defaults_kitty_graphics_action_to_stored_upload() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Gi=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;OK\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(7));
        assert_eq!(terminal.inline_images()[0].data, vec![255, 0, 0]);
    }

    #[test]
    fn terminal_suppresses_stored_kitty_image_upload_ok_when_quiet_is_one() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,q=1,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert!(terminal.take_kitty_graphics_responses().is_empty());
    }

    #[test]
    fn terminal_reports_invalid_stored_kitty_image_upload_by_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;not-base64!\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;EINVAL:Invalid base64 payload\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;ENOENT:No image with id 7\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_reports_unsupported_stored_kitty_image_format_by_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=99,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;EINVAL:Unsupported image format\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;ENOENT:No image with id 7\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_retransmitted_kitty_image_id_deletes_existing_placements() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].data, vec![255, 0, 0]);

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(7));
        assert_eq!(terminal.inline_images()[0].data, vec![0, 255, 0]);
    }

    #[test]
    fn terminal_suppresses_kitty_ok_response_when_quiet_is_one() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=q,q=1,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert!(terminal.take_kitty_graphics_responses().is_empty());
    }

    #[test]
    fn terminal_suppresses_kitty_error_response_when_quiet_is_two() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=p,q=2,i=404,p=3\x1b\\");

        assert!(terminal.take_kitty_graphics_responses().is_empty());
    }

    #[test]
    fn terminal_still_reports_kitty_error_response_when_quiet_is_one() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=p,q=1,i=404,p=3\x1b\\");

        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=404,p=3;ENOENT:No image with id 404\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_queries_stored_kitty_image_by_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=q,i=7\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_queries_kitty_file_payload_without_storing_or_replacing_image() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));
        let file = KittyTestFile::new(&[0, 255, 0]);
        let encoded_path = STANDARD.encode(file.path.as_os_str().to_string_lossy().as_bytes());

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.take_kitty_graphics_responses();

        let query = format!("\x1b_Ga=q,t=f,i=7,f=24,s=1,v=1,c=1,r=1;{encoded_path}\x1b\\");
        terminal.feed(query.as_bytes());

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;OK\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].data, vec![255, 0, 0]);
    }

    #[test]
    fn terminal_queries_kitty_temporary_file_payload_and_deletes_safe_file() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));
        let file = KittyTestFile::new_with_prefix("tty-graphics-protocol-rssh-query", &[0, 255, 0]);
        let encoded_path = STANDARD.encode(file.path.as_os_str().to_string_lossy().as_bytes());

        let query = format!("\x1b_Ga=q,t=t,i=40,f=24,s=1,v=1,c=1,r=1;{encoded_path}\x1b\\");
        terminal.feed(query.as_bytes());

        assert!(terminal.inline_images().is_empty());
        assert!(
            !file.path.exists(),
            "safe kitty temporary query file should be deleted after reading"
        );
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=40;OK\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b_Ga=p,i=40\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=40;ENOENT:No image with id 40\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_reports_missing_kitty_image_query_by_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=q,i=404\x1b\\");

        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=404;ENOENT:No image with id 404\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_assigns_kitty_image_id_for_image_number_upload() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,I=13,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=1,I=13;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_places_latest_kitty_image_by_image_number() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,I=13,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,I=13,p=2\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(1));
        assert_eq!(terminal.inline_images()[0].kitty_placement_id, Some(2));
        assert_eq!(terminal.inline_images()[0].column, 4);
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=1,I=13,p=2;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_reports_missing_kitty_image_number_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=p,I=13,p=2\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_GI=13,p=2;ENOENT:No image with number 13\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_reports_missing_kitty_relative_parent_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=p,i=7,p=2,P=30,Q=4,H=1,V=0\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7,p=2;ENOPARENT:No parent placement with id 30,p=4\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_places_kitty_image_relative_to_existing_parent_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,i=30,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b[2;4H");
        terminal.feed(b"\x1b_Ga=p,i=30,p=4,c=1,r=1\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.take_damage();

        terminal.feed(b"\x1b[1;1H");
        terminal.feed(b"\x1b_Ga=p,i=7,p=2,P=30,Q=4,H=2,V=1,c=1,r=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 2);
        let child = &terminal.inline_images()[1];
        assert_eq!(child.kitty_image_id, Some(7));
        assert_eq!(child.kitty_placement_id, Some(2));
        assert_eq!(child.row, 2);
        assert_eq!(child.column, 5);
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(5, 2, 1, 1)]);
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7,p=2;OK\x1b\\".to_vec()]
        );
        assert_eq!(terminal.cursor(), (0, 0));

        terminal.feed(b"X");

        assert_eq!(terminal.grid().get(0, 0).unwrap().ch, 'X');
    }

    #[test]
    fn terminal_places_kitty_image_relative_to_virtual_parent_placeholder_bounds() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=T,U=1,q=1,i=30,p=4,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b[1;3H\x1b[38;5;30m\x1b[58;5;4m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());
        terminal.feed(b"\x1b[3;8H\x1b[38;5;30m\x1b[58;5;4m");
        terminal.feed("\u{10eeee}\u{0305}\u{0305}".as_bytes());

        terminal.feed(b"\x1b_Ga=p,i=7,p=2,P=30,Q=4,H=2,V=1,c=1,r=1\x1b\\");

        let child = terminal
            .inline_images()
            .iter()
            .find(|image| image.kitty_image_id == Some(7))
            .unwrap();
        assert_eq!(child.kitty_placement_id, Some(2));
        assert_eq!(child.row, 1);
        assert_eq!(child.column, 4);
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7,p=2;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_rejects_virtual_kitty_relative_placement() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,i=30,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=p,i=30,p=4,c=1,r=1\x1b\\");
        terminal.take_kitty_graphics_responses();

        terminal.feed(b"\x1b_Ga=p,U=1,i=7,p=2,P=30,Q=4,H=1,V=0,c=1,r=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(30));
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7,p=2;EINVAL:Virtual placements cannot be relative\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_deletes_kitty_relative_child_when_parent_placement_is_deleted() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,i=30,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=p,i=30,p=4,c=1,r=1\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,p=2,P=30,Q=4,H=1,V=0,c=1,r=1\x1b\\");
        terminal.take_kitty_graphics_responses();

        assert_eq!(terminal.inline_images().len(), 2);

        terminal.feed(b"\x1b_Ga=d,d=i,i=30,p=4\x1b\\");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b_Ga=p,i=7,p=3\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7,p=3;ENOENT:No image with id 7\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_deletes_kitty_relative_child_when_parent_scrolls_out() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 3));

        terminal.feed(b"\x1b_Ga=t,i=30,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b[1;1H");
        terminal.feed(b"\x1b_Ga=p,i=30,p=4,c=1,r=1\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,p=2,P=30,Q=4,H=0,V=1,c=1,r=1\x1b\\");
        terminal.take_kitty_graphics_responses();

        assert_eq!(terminal.inline_images().len(), 2);

        terminal.feed(b"\x1b[1;2r\x1b[2;1H\n");

        assert!(terminal.inline_images().is_empty());

        terminal.feed(b"\x1b_Ga=p,i=7,p=3\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7,p=3;ENOENT:No image with id 7\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_rejects_kitty_relative_placement_cycle() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,i=30,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=p,i=30,p=4,c=1,r=1\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,p=2,P=30,Q=4,H=1,V=0,c=1,r=1\x1b\\");
        terminal.take_kitty_graphics_responses();

        terminal.feed(b"\x1b_Ga=p,i=30,p=4,P=7,Q=2,H=1,V=0,c=1,r=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 2);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(30));
        assert_eq!(terminal.inline_images()[0].kitty_placement_id, Some(4));
        assert_eq!(terminal.inline_images()[0].column, 0);
        assert_eq!(terminal.inline_images()[1].kitty_image_id, Some(7));
        assert_eq!(terminal.inline_images()[1].kitty_placement_id, Some(2));
        assert_eq!(terminal.inline_images()[1].column, 1);
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=30,p=4;ECYCLE:Relative placement cycle\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_moves_kitty_relative_child_when_parent_placement_moves() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,i=30,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=p,i=30,p=4,c=1,r=1\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,p=2,P=30,Q=4,H=2,V=1,c=1,r=1\x1b\\");
        terminal.take_kitty_graphics_responses();

        terminal.feed(b"\x1b[2;5H");
        terminal.feed(b"\x1b_Ga=p,i=30,p=4,c=1,r=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 2);
        let parent = terminal
            .inline_images()
            .iter()
            .find(|image| image.kitty_image_id == Some(30))
            .unwrap();
        let child = terminal
            .inline_images()
            .iter()
            .find(|image| image.kitty_image_id == Some(7))
            .unwrap();
        assert_eq!(parent.kitty_placement_id, Some(4));
        assert_eq!(parent.row, 1);
        assert_eq!(parent.column, 4);
        assert_eq!(child.kitty_placement_id, Some(2));
        assert_eq!(child.row, 2);
        assert_eq!(child.column, 6);
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=30,p=4;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_rejects_kitty_relative_chain_deeper_than_eight() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 4));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=p,i=7,p=1,c=1,r=1\x1b\\");
        for placement_id in 2..=9 {
            let parent_id = placement_id - 1;
            let command =
                format!("\x1b_Ga=p,i=7,p={placement_id},P=7,Q={parent_id},H=1,V=0,c=1,r=1\x1b\\");
            terminal.feed(command.as_bytes());
        }
        terminal.take_kitty_graphics_responses();

        terminal.feed(b"\x1b_Ga=p,i=7,p=10,P=7,Q=9,H=1,V=0,c=1,r=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 9);
        assert!(terminal.inline_images().iter().all(|image| {
            image
                .kitty_placement_id
                .is_some_and(|placement_id| (1..=9).contains(&placement_id))
        }));
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7,p=10;ETOODEEP:Relative placement chain too deep\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_queries_stored_kitty_image_by_number() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,I=13,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=q,I=13\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=1,I=13;OK\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_reports_missing_kitty_image_query_by_number() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=q,I=13\x1b\\");

        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_GI=13;ENOENT:No image with number 13\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_rejects_kitty_image_id_and_number_together() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=q,i=7,I=13,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");

        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![
                b"\x1b_Gi=7,I=13;EINVAL:Image id and image number are mutually exclusive\x1b\\"
                    .to_vec()
            ]
        );
    }

    #[test]
    fn terminal_preserves_transparent_sixel_raster_attribute_pixel_size() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab");
        terminal.feed(b"\x1bP0;1q\"1;1;4;8#1;2;100;0;0#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        let image = &terminal.inline_images()[0];
        assert_eq!(image.row, 0);
        assert_eq!(image.column, 2);
        assert_eq!(image.image_format, InlineImageFormat::Rgba);
        assert_eq!(image.pixel_width, Some(4));
        assert_eq!(image.pixel_height, Some(8));
        assert_eq!(image.width.as_deref(), Some("4px"));
        assert_eq!(image.height.as_deref(), Some("8px"));
        assert_eq!(image.data.len(), 4 * 8 * 4);
        assert_eq!(&image.data[0..4], &[255, 0, 0, 255]);
        assert_eq!(&image.data[4..8], &[0, 0, 0, 0]);
        assert_eq!(&image.data[80..84], &[255, 0, 0, 255]);
        assert_eq!(&image.data[96..100], &[0, 0, 0, 0]);
    }

    #[test]
    fn terminal_moves_cursor_below_sixel_image() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 3));

        terminal.feed(b"ab");
        terminal.feed(b"\x1bP0;1q\"1;1;2;6#1;2;100;0;0#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 2);
        assert_eq!(terminal.cursor(), (1, 2));

        terminal.feed(b"cd");

        assert_eq!(row_text(&terminal, 0), "ab                      ");
        assert_eq!(row_text(&terminal, 1), "  cd                    ");
    }

    #[test]
    fn terminal_sixel_display_mode_set_starts_image_at_graphics_origin() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 3));

        terminal.feed(b"ab\x1b[?80h");
        terminal.feed(b"\x1bP0;1q\"1;1;2;6#1;2;100;0;0#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 0);
        assert_eq!(terminal.cursor(), (0, 2));

        terminal.feed(b"cd");

        assert_eq!(row_text(&terminal, 0), "abcd                    ");
        assert_eq!(row_text(&terminal, 1), "                        ");
    }

    #[test]
    fn terminal_sixel_scrolls_right_mode_moves_cursor_below_right_edge() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 3));

        terminal.feed(b"ab\x1b[?8452h");
        terminal.feed(b"\x1bP0;1q\"1;1;2;6#1;2;100;0;0#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].row, 0);
        assert_eq!(terminal.inline_images()[0].column, 2);
        assert_eq!(terminal.cursor(), (1, 3));

        terminal.feed(b"cd");

        assert_eq!(row_text(&terminal, 0), "ab                      ");
        assert_eq!(row_text(&terminal, 1), "   cd                   ");
    }

    #[test]
    fn terminal_sixel_scrolls_right_uses_rendered_pixel_width() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 3));

        terminal.feed(b"ab\x1b[?8452h");
        terminal.feed(b"\x1bP0;1q\"1;1;24;6#1;2;100;0;0#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].width.as_deref(), Some("24px"));
        assert_eq!(terminal.cursor(), (1, 5));

        terminal.feed(b"cd");

        assert_eq!(row_text(&terminal, 1), "     cd                 ");
    }

    #[test]
    fn terminal_fills_default_sixel_background_opaque() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1bPq\"1;1;2;6#1;2;100;0;0#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        let image = &terminal.inline_images()[0];
        assert_eq!(image.pixel_width, Some(2));
        assert_eq!(image.pixel_height, Some(6));
        assert_eq!(&image.data[0..4], &[255, 0, 0, 255]);
        assert_eq!(&image.data[4..8], &[0, 0, 0, 255]);
    }

    #[test]
    fn terminal_uses_vt340_default_sixel_palette() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1bP0;1q\"1;1;1;6#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        let image = &terminal.inline_images()[0];
        assert_eq!(image.pixel_width, Some(1));
        assert_eq!(image.pixel_height, Some(6));
        let vt340_blue = [51, 51, 204, 255];
        assert_eq!(&image.data[0..4], &vt340_blue);
        assert!(image.data.chunks_exact(4).all(|pixel| pixel == vt340_blue));
    }

    #[test]
    fn terminal_uses_dec_hls_primary_hues_for_sixel() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal
            .feed(b"\x1bP0;1q\"1;1;3;6#1;1;0;50;100#1~#2;1;120;50;100#2~#3;1;240;50;100#3~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        let image = &terminal.inline_images()[0];
        assert_eq!(image.pixel_width, Some(3));
        assert_eq!(image.pixel_height, Some(6));
        assert_eq!(&image.data[0..4], &[0, 0, 255, 255]);
        assert_eq!(&image.data[4..8], &[255, 0, 0, 255]);
        assert_eq!(&image.data[8..12], &[0, 255, 0, 255]);
    }

    #[test]
    fn terminal_applies_sixel_raster_pixel_aspect_ratio() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1bP0;1q\"2;1;1;6#1;2;100;0;0#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        let image = &terminal.inline_images()[0];
        assert_eq!(image.pixel_width, Some(1));
        assert_eq!(image.pixel_height, Some(12));
        assert_eq!(image.width.as_deref(), Some("1px"));
        assert_eq!(image.height.as_deref(), Some("12px"));
        assert_eq!(image.data.len(), 12 * 4);
        assert!(
            image
                .data
                .chunks_exact(4)
                .all(|pixel| pixel == [255, 0, 0, 255])
        );
    }

    #[test]
    fn terminal_applies_sixel_dcs_macro_pixel_aspect_ratio() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1bPq#1;2;100;0;0#1~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        let image = &terminal.inline_images()[0];
        assert_eq!(image.pixel_width, Some(1));
        assert_eq!(image.pixel_height, Some(12));
        assert_eq!(image.width.as_deref(), Some("1px"));
        assert_eq!(image.height.as_deref(), Some("12px"));
        assert_eq!(image.data.len(), 12 * 4);
        assert!(
            image
                .data
                .chunks_exact(4)
                .all(|pixel| pixel == [255, 0, 0, 255])
        );
    }

    #[test]
    fn terminal_honors_sixel_opaque_background_parameter_without_drawn_pixels() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1bP0;2q\"1;1;2;6?\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        let image = &terminal.inline_images()[0];
        assert_eq!(image.pixel_width, Some(2));
        assert_eq!(image.pixel_height, Some(6));
        assert_eq!(image.data.len(), 2 * 6 * 4);
        assert!(
            image
                .data
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        );
    }

    #[test]
    fn terminal_allows_sixel_pixels_beyond_raster_attribute_size() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1bP0;1q\"1;1;1;6#1;2;100;0;0#1~~\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        let image = &terminal.inline_images()[0];
        assert_eq!(image.pixel_width, Some(2));
        assert_eq!(image.pixel_height, Some(6));
        assert_eq!(image.width.as_deref(), Some("2px"));
        assert_eq!(image.height.as_deref(), Some("6px"));
        assert_eq!(image.data.len(), 2 * 6 * 4);
        assert!(
            image
                .data
                .chunks_exact(4)
                .all(|pixel| pixel == [255, 0, 0, 255])
        );
    }

    #[test]
    fn terminal_does_not_treat_decrqss_dcs_as_sixel_image() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1bP$qm\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert!(terminal.take_damage().is_empty());
    }

    #[test]
    fn terminal_does_not_treat_tmux_control_dcs_as_sixel_image() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1bP1000q~\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert!(terminal.take_damage().is_empty());
    }

    #[test]
    fn terminal_deletes_all_visible_kitty_placements_but_keeps_stored_image() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 24, 1)]);

        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert_eq!(
            terminal.inline_images(),
            &[ItermInlineImage {
                row: 0,
                column: 4,
                name: None,
                kitty_image_id: Some(7),
                kitty_placement_id: None,
                kitty_z_index: Some(0),
                size: Some(3),
                width: Some("1".to_owned()),
                height: Some("1".to_owned()),
                preserve_aspect_ratio: None,
                image_format: InlineImageFormat::Rgb,
                pixel_width: Some(1),
                pixel_height: Some(1),
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: vec![255, 0, 0],
            }]
        );
    }

    #[test]
    fn terminal_deletes_visible_kitty_placements_by_image_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=8\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=i,i=7\x1b\\");

        assert_eq!(
            terminal.inline_images(),
            &[ItermInlineImage {
                row: 0,
                column: 4,
                name: None,
                kitty_image_id: Some(8),
                kitty_placement_id: None,
                kitty_z_index: Some(0),
                size: Some(3),
                width: Some("1".to_owned()),
                height: Some("1".to_owned()),
                preserve_aspect_ratio: None,
                image_format: InlineImageFormat::Rgb,
                pixel_width: Some(1),
                pixel_height: Some(1),
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: vec![0, 255, 0],
            }]
        );
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 24, 1)]);
    }

    #[test]
    fn terminal_replaces_kitty_placement_with_same_image_and_placement_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,p=2\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=7,p=2\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].column, 4);
    }

    #[test]
    fn terminal_deletes_latest_kitty_placement_by_image_number() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,I=13,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b[1;1H");
        terminal.feed(b"\x1b_Ga=p,I=13,C=1\x1b\\");
        terminal.feed(b"\x1b_Ga=t,I=13,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,I=13,C=1\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=n,I=13\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(1));
        assert_eq!(terminal.inline_images()[0].column, 0);
        assert_eq!(terminal.inline_images()[0].data, vec![255, 0, 0]);
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 24, 1)]);
    }

    #[test]
    fn terminal_deletes_kitty_placement_by_image_and_placement_id() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,p=1\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=7,p=2\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=i,i=7,p=2\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].column, 0);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(7));
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 24, 1)]);
    }

    #[test]
    fn terminal_deletes_kitty_placement_at_cursor_cell() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=2,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=8\x1b\\");
        terminal.feed(b"\x1b[1;2H");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=c\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(8));
        assert_eq!(terminal.inline_images()[0].column, 4);
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 2)]);
    }

    #[test]
    fn terminal_deletes_kitty_placement_at_explicit_cell() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=2,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");
        terminal.feed(b"\x1b[2;5H");
        terminal.feed(b"\x1b_Ga=p,i=8\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=p,x=2,y=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(8));
        assert_eq!(terminal.inline_images()[0].row, 1);
        assert_eq!(terminal.inline_images()[0].column, 4);
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 2)]);
    }

    #[test]
    fn terminal_deletes_kitty_placements_by_visible_column_and_row() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=2,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=9,f=24,s=1,v=1,c=1,r=1;AAD/\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=8\x1b\\");
        terminal.feed(b"\x1b[2;5H");
        terminal.feed(b"\x1b_Ga=p,i=9\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=x,x=2\x1b\\");

        assert_eq!(terminal.inline_images().len(), 2);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(8));
        assert_eq!(terminal.inline_images()[1].kitty_image_id, Some(9));

        terminal.feed(b"\x1b_Ga=d,d=y,y=2\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(8));
    }

    #[test]
    fn terminal_deletes_kitty_placements_by_image_id_range() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=9,f=24,s=1,v=1,c=1,r=1;AAD/\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,C=1\x1b\\");
        terminal.feed(b"\x1b[1;3H");
        terminal.feed(b"\x1b_Ga=p,i=8,C=1\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=9,C=1\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=r,x=8,y=9\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(7));
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 1)]);

        terminal.feed(b"\x1b[1;7H");
        terminal.feed(b"\x1b_Ga=p,i=8,C=1\x1b\\");
        terminal.feed(b"\x1b[1;8H");
        terminal.feed(b"\x1b_Ga=p,i=9,C=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 3);
        assert_eq!(terminal.inline_images()[1].kitty_image_id, Some(8));
        assert_eq!(terminal.inline_images()[2].kitty_image_id, Some(9));
    }

    #[test]
    fn terminal_uppercase_cursor_delete_drops_unreferenced_stored_kitty_image() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b[1;1H");
        terminal.feed(b"\x1b_Ga=d,d=C\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 1)]);
    }

    #[test]
    fn terminal_uppercase_image_id_delete_drops_unplaced_stored_kitty_image() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=d,d=I,i=7\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=7;ENOENT:No image with id 7\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_uppercase_image_number_delete_drops_unplaced_stored_kitty_image() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,I=13,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.feed(b"\x1b_Ga=d,d=N,I=13\x1b\\");
        terminal.feed(b"\x1b_Ga=p,I=13\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_GI=13;ENOENT:No image with number 13\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_uppercase_range_delete_drops_unreferenced_stored_kitty_images() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=9,f=24,s=1,v=1,c=1,r=1;AAD/\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=8,C=1\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=R,x=8,y=9\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 1)]);

        terminal.feed(b"\x1b_Ga=p,i=7,C=1\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=8,C=1\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=9,C=1\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(7));
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![
                b"\x1b_Gi=7;OK\x1b\\".to_vec(),
                b"\x1b_Gi=8;ENOENT:No image with id 8\x1b\\".to_vec(),
                b"\x1b_Gi=9;ENOENT:No image with id 9\x1b\\".to_vec(),
            ]
        );
    }

    #[test]
    fn terminal_uppercase_image_number_delete_drops_unreferenced_stored_kitty_image() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,I=13,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,I=13,C=1\x1b\\");
        terminal.take_kitty_graphics_responses();
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=N,I=13\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 1)]);

        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=1\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(
            terminal.take_kitty_graphics_responses(),
            vec![b"\x1b_Gi=1;ENOENT:No image with id 1\x1b\\".to_vec()]
        );
    }

    #[test]
    fn terminal_deletes_kitty_placements_by_z_index() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,z=2\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=8,z=-3\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=z,z=2\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(8));
        assert_eq!(terminal.inline_images()[0].column, 4);
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 1)]);
    }

    #[test]
    fn terminal_deletes_kitty_placement_by_cell_and_z_index() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,z=2\x1b\\");
        terminal.feed(b"\x1b[1;1H");
        terminal.feed(b"\x1b_Ga=p,i=8,z=-3\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=q,x=1,y=1,z=2\x1b\\");

        assert_eq!(terminal.inline_images().len(), 1);
        assert_eq!(terminal.inline_images()[0].kitty_image_id, Some(8));
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 1)]);
    }

    #[test]
    fn terminal_uppercase_z_delete_drops_unreferenced_stored_kitty_image() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,z=2\x1b\\");
        terminal.take_damage();

        terminal.feed(b"\x1b_Ga=d,d=Z,z=2\x1b\\");
        terminal.feed(b"\x1b[1;5H");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");

        assert!(terminal.inline_images().is_empty());
        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 8, 1)]);
    }

    #[test]
    fn terminal_marks_inline_image_origin_damaged() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"\x1b]1337;File=inline=1;width=3;height=2:QUJDRA==\x07");

        assert_eq!(terminal.take_damage(), vec![DamageRegion::new(0, 0, 3, 1)]);
    }

    #[test]
    fn terminal_ignores_non_inline_iterm_file_uploads() {
        let mut terminal = Terminal::new(TerminalSize::new(24, 1));

        terminal.feed(b"ab\x1b]1337;File=name=aW1nLnBuZw==:QUJDRA==\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd                    ");
        assert!(terminal.inline_images().is_empty());
    }

    #[test]
    fn terminal_tracks_osc8_hyperlink_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"a\x1b]8;;https://example.com\x1b\\bc\x1b]8;;\x1b\\d");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(
            terminal.grid().get(0, 1).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            terminal.grid().get(0, 2).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(terminal.grid().get(0, 0).unwrap().hyperlink, None);
        assert_eq!(terminal.grid().get(0, 3).unwrap().hyperlink, None);
    }

    #[test]
    fn terminal_tracks_c1_osc8_hyperlink_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"a\x9d8;;https://example.com\x9cbc\x9d8;;\x9cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(
            terminal.grid().get(0, 1).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            terminal.grid().get(0, 2).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(terminal.grid().get(0, 0).unwrap().hyperlink, None);
        assert_eq!(terminal.grid().get(0, 3).unwrap().hyperlink, None);
    }

    #[test]
    fn terminal_preserves_active_hyperlink_across_sgr_reset() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b]8;;https://example.com\x1b\\a\x1b[0mb\x1b]8;;\x1b\\c");

        assert_eq!(
            terminal.grid().get(0, 0).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            terminal.grid().get(0, 1).unwrap().hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(terminal.grid().get(0, 2).unwrap().hyperlink, None);
    }

    #[test]
    fn terminal_tracks_split_osc_title_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]0;cmd");
        terminal.feed(b".exe\x07cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.title(), Some("cmd.exe"));
    }

    #[test]
    fn terminal_can_cancels_osc_title_sequence() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]0;title\x18cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_ignores_dcs_terminated_by_st() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1bP$qm\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_ignores_split_dcs_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1bP$q");
        terminal.feed(b"m\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_sub_cancels_split_dcs_sequence() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1bP$q");
        terminal.feed(b"\x1acd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_ignores_pm_and_apc_terminated_by_st() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b^private\x1b\\\x1b_app\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_ignores_split_apc_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b_app");
        terminal.feed(b"-data\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_ignores_sos_terminated_by_st() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1bXstring\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_ignores_st_controls_without_rendering() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"ab\x1b\\cd\x9cef");

        assert_eq!(row_text(&terminal, 0), "abcdef  ");
        assert_eq!(terminal.cursor(), (0, 6));
    }

    #[test]
    fn terminal_ignores_split_sos_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1bXstr");
        terminal.feed(b"ing\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_supports_c1_csi_sequences() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 3));

        terminal.feed(b"ab\x9b2;3HZ");

        assert_eq!(row_text(&terminal, 0), "ab    ");
        assert_eq!(row_text(&terminal, 1), "  Z   ");
        assert_eq!(terminal.cursor(), (1, 3));
    }

    #[test]
    fn terminal_handles_split_c1_csi_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 3));

        terminal.feed(b"ab\x9b2;");
        terminal.feed(b"3HZ");

        assert_eq!(row_text(&terminal, 0), "ab    ");
        assert_eq!(row_text(&terminal, 1), "  Z   ");
        assert_eq!(terminal.cursor(), (1, 3));
    }

    #[test]
    fn terminal_tracks_c1_osc_title_and_ignores_other_control_strings() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x9d0;title\x9c\x90$qm\x9ccd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.title(), Some("title"));
    }

    #[test]
    fn terminal_tracks_split_c1_osc_title_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x9d0;ti");
        terminal.feed(b"tle\x9ccd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
        assert_eq!(terminal.title(), Some("title"));
    }

    #[test]
    fn terminal_handles_split_esc_cursor_save_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"ab\x1b");
        terminal.feed(b"7cd\x1b8Z");

        assert_eq!(row_text(&terminal, 0), "abZd    ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_maps_dec_special_graphics_line_drawing() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"\x1b(0lqk\x1b(Babc");

        assert_eq!(row_text(&terminal, 0), "┌─┐abc  ");
        assert_eq!(terminal.cursor(), (0, 6));
    }

    #[test]
    fn terminal_handles_split_dec_special_graphics_sequence() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"\x1b(");
        terminal.feed(b"0x\x1b(Bx");

        assert_eq!(row_text(&terminal, 0), "│x  ");
        assert_eq!(terminal.cursor(), (0, 2));
    }

    fn row_text(terminal: &Terminal, row: u16) -> String {
        let grid = terminal.grid();
        let mut text = String::new();

        for column in 0..grid.size().columns {
            text.push(grid.get(row, column).unwrap().ch);
        }

        text
    }

    fn scrollback_text(terminal: &Terminal, index: usize) -> String {
        terminal.scrollback()[index]
            .cells()
            .iter()
            .map(|cell| cell.ch)
            .collect()
    }

    struct KittyTestFile {
        path: PathBuf,
    }

    impl KittyTestFile {
        fn new(data: &[u8]) -> Self {
            Self::new_with_prefix("rssh-kitty-file-query", data)
        }

        fn new_with_prefix(prefix: &str, data: &[u8]) -> Self {
            static NEXT_TEST_FILE_ID: AtomicUsize = AtomicUsize::new(0);

            let suffix = NEXT_TEST_FILE_ID.fetch_add(1, Ordering::Relaxed);
            let mut path = std::env::temp_dir();
            path.push(format!("{prefix}-{}-{suffix}.rgb", std::process::id()));
            fs::write(&path, data).expect("write kitty query test image file");
            Self { path }
        }
    }

    impl Drop for KittyTestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
