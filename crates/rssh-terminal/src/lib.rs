use rssh_core::TerminalSize;

mod parser;

pub use parser::Terminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            foreground: Color::Default,
            background: Color::Default,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerminalGrid {
    size: TerminalSize,
    cells: Vec<Cell>,
}

impl TerminalGrid {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            size,
            cells: vec![Cell::default(); size.cells()],
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
    fn index(&self, row: u16, column: u16) -> Option<usize> {
        if row >= self.size.rows || column >= self.size.columns {
            return None;
        }

        Some(usize::from(row) * usize::from(self.size.columns) + usize::from(column))
    }
}

#[cfg(test)]
mod tests {
    use rssh_core::{DamageRegion, TerminalSize};

    use super::{Cell, Color, Terminal, TerminalGrid};

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
        assert!(!cell.bold);
        assert!(!cell.italic);
        assert!(!cell.underline);
    }

    #[test]
    fn grid_sets_and_reads_cells_by_position() {
        let mut grid = TerminalGrid::new(TerminalSize::new(3, 2));
        let cell = Cell {
            ch: 'R',
            foreground: Color::Indexed(2),
            background: Color::Rgb(1, 2, 3),
            bold: true,
            italic: false,
            underline: true,
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
        assert_eq!(terminal.grid().get(1, 0).unwrap().ch, 'c');
        assert_eq!(terminal.grid().get(1, 1).unwrap().ch, 'd');
        assert_eq!(terminal.cursor(), (1, 2));
    }

    #[test]
    fn terminal_backspace_moves_cursor_left_without_erasing() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"ab\x08c");

        assert_eq!(row_text(&terminal, 0), "ac  ");
        assert_eq!(terminal.cursor(), (0, 2));
    }

    #[test]
    fn terminal_tab_moves_to_next_eight_column_stop() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 1));

        terminal.feed(b"a\tb");

        assert_eq!(row_text(&terminal, 0), "a       b ");
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
    fn terminal_saves_and_restores_cursor_with_csi_s_and_u() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));

        terminal.feed(b"ab\x1b[s\x1b[2;1Hcd\x1b[uZ");

        assert_eq!(row_text(&terminal, 0), "abZ     ");
        assert_eq!(row_text(&terminal, 1), "cd      ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_scrolls_when_newline_reaches_bottom_row() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"ab\ncd\nef");

        assert_eq!(row_text(&terminal, 0), "cd  ");
        assert_eq!(row_text(&terminal, 1), "ef  ");
        assert_eq!(terminal.cursor(), (1, 2));
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
    fn terminal_places_wide_cjk_character_across_two_columns() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed("中x".as_bytes());

        assert_eq!(terminal.grid().get(0, 0).unwrap().ch, '中');
        assert_eq!(terminal.grid().get(0, 1).unwrap().ch, ' ');
        assert_eq!(terminal.grid().get(0, 2).unwrap().ch, 'x');
        assert_eq!(terminal.cursor(), (0, 3));
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

        terminal.feed(b"ab\ncd\x1b[A\x1b[CX\x1b[B\x1b[DY");

        assert_eq!(terminal.grid().get(0, 3).unwrap().ch, 'X');
        assert_eq!(terminal.grid().get(1, 3).unwrap().ch, 'Y');
        assert_eq!(terminal.cursor(), (1, 4));
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
    fn terminal_erases_line_from_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"abcdef\x1b[3D\x1b[K");

        assert_eq!(row_text(&terminal, 0), "abc     ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_erases_entire_display() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));

        terminal.feed(b"abcd\nef\x1b[2J");

        assert_eq!(row_text(&terminal, 0), "    ");
        assert_eq!(row_text(&terminal, 1), "    ");
        assert_eq!(terminal.cursor(), (1, 2));
    }

    #[test]
    fn terminal_ignores_osc_title_terminated_by_bel() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]0;cmd.exe\x07cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_ignores_osc_title_terminated_by_st() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]2;PowerShell\x1b\\cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_ignores_split_osc_title_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(12, 1));

        terminal.feed(b"ab\x1b]0;cmd");
        terminal.feed(b".exe\x07cd");

        assert_eq!(row_text(&terminal, 0), "abcd        ");
        assert_eq!(terminal.cursor(), (0, 4));
    }

    #[test]
    fn terminal_handles_split_esc_cursor_save_across_feed_calls() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"ab\x1b");
        terminal.feed(b"7cd\x1b8Z");

        assert_eq!(row_text(&terminal, 0), "abZd    ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    fn row_text(terminal: &Terminal, row: u16) -> String {
        let grid = terminal.grid();
        let mut text = String::new();

        for column in 0..grid.size().columns {
            text.push(grid.get(row, column).unwrap().ch);
        }

        text
    }
}
