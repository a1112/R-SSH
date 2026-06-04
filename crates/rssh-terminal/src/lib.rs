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
#[allow(clippy::struct_excessive_bools)]
pub struct Cell {
    pub ch: char,
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
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
            inverse: false,
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

    pub fn resize(&mut self, size: TerminalSize) {
        let old_size = self.size;
        let old_cells = std::mem::replace(&mut self.cells, vec![Cell::default(); size.cells()]);
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
        assert!(!cell.inverse);
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
            inverse: false,
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
    fn terminal_sets_custom_tab_stop_with_hts() {
        let mut terminal = Terminal::new(TerminalSize::new(10, 1));

        terminal.feed(b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;1Ha\tb");

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

        terminal.feed(b"ab\ncd\nef");

        assert_eq!(row_text(&terminal, 0), "cd  ");
        assert_eq!(row_text(&terminal, 1), "ef  ");
        assert_eq!(terminal.cursor(), (1, 2));
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
        terminal.feed(b"abcd\nef");
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

        terminal.feed(b"ab\ncd\x1b[A\x1b[CX\x1b[B\x1b[DY");

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
    fn terminal_erases_line_from_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 1));

        terminal.feed(b"abcdef\x1b[3D\x1b[K");

        assert_eq!(row_text(&terminal, 0), "abc     ");
        assert_eq!(terminal.cursor(), (0, 3));
    }

    #[test]
    fn terminal_inserts_blank_characters_with_ich() {
        let mut terminal = Terminal::new(TerminalSize::new(6, 1));

        terminal.feed(b"abcdef\x1b[4D\x1b[2@");

        assert_eq!(row_text(&terminal, 0), "a  bcd");
        assert_eq!(terminal.cursor(), (0, 1));
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
}
