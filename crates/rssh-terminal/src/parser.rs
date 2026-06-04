use rssh_core::{DamageRegion, TerminalSize};
use unicode_width::UnicodeWidthChar;

use crate::{Cell, Color, TerminalGrid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterSet {
    Ascii,
    DecSpecialGraphics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterWriteMode {
    Replace,
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalModes {
    cursor_visible: bool,
    auto_wrap: bool,
    origin_mode: bool,
    write_mode: CharacterWriteMode,
}

impl Default for TerminalModes {
    fn default() -> Self {
        Self {
            cursor_visible: true,
            auto_wrap: true,
            origin_mode: false,
            write_mode: CharacterWriteMode::Replace,
        }
    }
}

#[derive(Debug, Clone)]
struct TabStops {
    columns: Vec<u16>,
}

impl TabStops {
    fn new(size: TerminalSize) -> Self {
        Self {
            columns: default_tab_stops(size),
        }
    }

    fn resize(&mut self, size: TerminalSize) {
        self.columns.retain(|column| *column < size.columns);
    }

    fn set(&mut self, column: u16, size: TerminalSize) {
        if column >= size.columns || self.columns.binary_search(&column).is_ok() {
            return;
        }

        let index = self.columns.partition_point(|stop| *stop < column);
        self.columns.insert(index, column);
    }

    fn clear(&mut self, column: u16) {
        self.columns.retain(|stop| *stop != column);
    }

    fn clear_all(&mut self) {
        self.columns.clear();
    }

    fn next_after(&self, column: u16, fallback: u16) -> u16 {
        self.columns
            .iter()
            .copied()
            .find(|stop| *stop > column)
            .unwrap_or(fallback)
    }

    fn previous_before(&self, column: u16) -> u16 {
        self.columns
            .iter()
            .rev()
            .copied()
            .find(|stop| *stop < column)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub struct Terminal {
    grid: TerminalGrid,
    cursor_row: u16,
    cursor_column: u16,
    pending_wrap: bool,
    pending_utf8: Vec<u8>,
    pending_control: Vec<char>,
    last_printable: Option<char>,
    saved_cursor: Option<SavedCursor>,
    main_screen: Option<ScreenState>,
    modes: TerminalModes,
    scroll_top: u16,
    scroll_bottom: u16,
    character_set: CharacterSet,
    tab_stops: TabStops,
    style: Cell,
    damage: Vec<DamageRegion>,
}

#[derive(Debug, Clone)]
struct ScreenState {
    grid: TerminalGrid,
    cursor_row: u16,
    cursor_column: u16,
    pending_wrap: bool,
    last_printable: Option<char>,
    saved_cursor: Option<SavedCursor>,
    modes: TerminalModes,
    scroll_top: u16,
    scroll_bottom: u16,
    character_set: CharacterSet,
    style: Cell,
}

#[derive(Debug, Clone)]
struct SavedCursor {
    row: u16,
    column: u16,
    pending_wrap: bool,
    origin_mode: bool,
    character_set: CharacterSet,
    style: Cell,
}

impl Terminal {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            grid: TerminalGrid::new(size),
            cursor_row: 0,
            cursor_column: 0,
            pending_wrap: false,
            pending_utf8: Vec::new(),
            pending_control: Vec::new(),
            last_printable: None,
            saved_cursor: None,
            main_screen: None,
            modes: TerminalModes::default(),
            scroll_top: 0,
            scroll_bottom: size.rows.saturating_sub(1),
            character_set: CharacterSet::Ascii,
            tab_stops: TabStops::new(size),
            style: Cell::default(),
            damage: Vec::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let mut input = std::mem::take(&mut self.pending_utf8);
        input.extend_from_slice(bytes);
        let complete_utf8_len = complete_utf8_prefix_len(&input);
        self.pending_utf8
            .extend_from_slice(&input[complete_utf8_len..]);

        let text = String::from_utf8_lossy(&input[..complete_utf8_len]);
        let mut chars = std::mem::take(&mut self.pending_control);
        chars.extend(text.chars());
        let mut index = 0;

        while index < chars.len() {
            match chars[index] {
                '\u{1b}' if chars.get(index + 1) == Some(&'[') => {
                    if let Some((command, sequence_end)) = parse_csi(&chars, index + 2) {
                        self.apply_csi(command, &chars[index + 2..sequence_end]);
                        index = sequence_end + 1;
                    } else {
                        self.pending_control.extend_from_slice(&chars[index..]);
                        break;
                    }
                }
                '\u{1b}' if chars.get(index + 1) == Some(&']') => {
                    let Some(next_index) = self.skip_osc(&chars, index) else {
                        break;
                    };
                    index = next_index;
                }
                '\u{1b}' if matches!(chars.get(index + 1).copied(), Some('P' | '^' | '_')) => {
                    let Some(next_index) = self.skip_st_control_string(&chars, index) else {
                        break;
                    };
                    index = next_index;
                }
                '\u{1b}' if chars.get(index + 1) == Some(&'(') => {
                    if let Some(selector) = chars.get(index + 2).copied() {
                        if let Some(character_set) = parse_g0_character_set(selector) {
                            self.character_set = character_set;
                        }
                        index += 3;
                    } else {
                        self.pending_control.extend_from_slice(&chars[index..]);
                        break;
                    }
                }
                '\u{1b}' if chars.get(index + 1) == Some(&'7') => {
                    self.save_cursor();
                    index += 2;
                }
                '\u{1b}' if chars.get(index + 1) == Some(&'8') => {
                    self.restore_cursor();
                    index += 2;
                }
                '\u{1b}' if chars.get(index + 1) == Some(&'H') => {
                    self.set_horizontal_tab_stop();
                    index += 2;
                }
                '\u{1b}' if chars.get(index + 1) == Some(&'c') => {
                    self.reset_terminal();
                    index += 2;
                }
                '\u{1b}' if chars.get(index + 1) == Some(&'D') => {
                    self.index_down();
                    index += 2;
                }
                '\u{1b}' if chars.get(index + 1) == Some(&'E') => {
                    self.next_line();
                    index += 2;
                }
                '\u{1b}' if chars.get(index + 1) == Some(&'M') => {
                    self.reverse_index();
                    index += 2;
                }
                '\u{1b}' if chars.get(index + 1).is_none() => {
                    self.pending_control.extend_from_slice(&chars[index..]);
                    break;
                }
                '\u{8}' => {
                    self.backspace();
                    index += 1;
                }
                '\t' => {
                    self.horizontal_tab();
                    index += 1;
                }
                '\n' => {
                    self.newline();
                    index += 1;
                }
                '\r' => {
                    self.cursor_column = 0;
                    self.pending_wrap = false;
                    index += 1;
                }
                ch => {
                    self.write_char(ch);
                    index += 1;
                }
            }
        }
    }

    fn skip_osc(&mut self, chars: &[char], index: usize) -> Option<usize> {
        self.skip_control_string(chars, index, 2, parse_osc)
    }

    fn skip_st_control_string(&mut self, chars: &[char], index: usize) -> Option<usize> {
        self.skip_control_string(chars, index, 2, parse_st_terminated_control_string)
    }

    fn skip_control_string(
        &mut self,
        chars: &[char],
        index: usize,
        content_offset: usize,
        parse: fn(&[char], usize) -> Option<usize>,
    ) -> Option<usize> {
        if let Some(sequence_end) = parse(chars, index + content_offset) {
            Some(sequence_end + 1)
        } else {
            self.pending_control.extend_from_slice(&chars[index..]);
            None
        }
    }

    #[must_use]
    pub const fn grid(&self) -> &TerminalGrid {
        &self.grid
    }

    #[must_use]
    pub const fn cursor(&self) -> (u16, u16) {
        (self.cursor_row, self.cursor_column)
    }

    #[must_use]
    pub const fn cursor_visible(&self) -> bool {
        self.modes.cursor_visible
    }

    pub fn resize(&mut self, size: TerminalSize) {
        self.grid.resize(size);
        self.tab_stops.resize(size);

        if let Some(screen) = self.main_screen.as_mut() {
            screen.grid.resize(size);
            clamp_screen_state(screen, size);
        }

        self.clamp_to_size();
        self.scroll_top = 0;
        self.scroll_bottom = size.rows.saturating_sub(1);
        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    pub fn take_damage(&mut self) -> Vec<DamageRegion> {
        std::mem::take(&mut self.damage)
    }

    fn reset_terminal(&mut self) {
        let size = self.grid.size();
        self.grid = TerminalGrid::new(size);
        self.cursor_row = 0;
        self.cursor_column = 0;
        self.pending_wrap = false;
        self.last_printable = None;
        self.saved_cursor = None;
        self.main_screen = None;
        self.modes = TerminalModes::default();
        self.scroll_top = 0;
        self.scroll_bottom = size.rows.saturating_sub(1);
        self.character_set = CharacterSet::Ascii;
        self.tab_stops = TabStops::new(size);
        self.style = Cell::default();
        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    fn newline(&mut self) {
        self.cursor_column = 0;
        self.index_down();
    }

    fn next_line(&mut self) {
        self.cursor_column = 0;
        self.index_down();
    }

    fn index_down(&mut self) {
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let scroll_bottom = self.scroll_bottom.min(rows - 1);
        if self.cursor_row == scroll_bottom {
            self.scroll_up_region(self.scroll_top, scroll_bottom);
            self.cursor_row = scroll_bottom;
        } else if self.cursor_row + 1 < rows {
            self.cursor_row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let scroll_top = self.scroll_top.min(rows - 1);
        let scroll_bottom = self.scroll_bottom.min(rows - 1);
        if self.cursor_row == scroll_top {
            self.scroll_down_region(scroll_top, scroll_bottom, 1);
            self.cursor_row = scroll_top;
        } else {
            self.cursor_row = self.cursor_row.saturating_sub(1);
        }
    }

    fn backspace(&mut self) {
        self.pending_wrap = false;
        self.cursor_column = self.cursor_column.saturating_sub(1);
    }

    fn horizontal_tab(&mut self) {
        self.move_forward_tabs(1);
    }

    fn set_horizontal_tab_stop(&mut self) {
        self.tab_stops.set(self.cursor_column, self.grid.size());
    }

    fn clear_tab_stop(&mut self, mode: u16) {
        match mode {
            0 => self.tab_stops.clear(self.cursor_column),
            3 => self.tab_stops.clear_all(),
            _ => {}
        }
    }

    fn move_forward_tabs(&mut self, count: u16) {
        self.pending_wrap = false;
        let columns = self.grid.size().columns;
        if columns == 0 {
            return;
        }

        let fallback = columns - 1;
        for _ in 0..count {
            let next = self.tab_stops.next_after(self.cursor_column, fallback);
            if next == self.cursor_column {
                break;
            }
            self.cursor_column = next;
        }
    }

    fn move_backward_tabs(&mut self, count: u16) {
        self.pending_wrap = false;
        if self.grid.size().columns == 0 {
            return;
        }

        for _ in 0..count {
            let previous = self.tab_stops.previous_before(self.cursor_column);
            if previous == self.cursor_column {
                break;
            }
            self.cursor_column = previous;
        }
    }

    fn scroll_up_region(&mut self, top: u16, bottom: u16) {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return;
        }

        let top = top.min(size.rows - 1);
        let bottom = bottom.min(size.rows - 1);
        if top >= bottom {
            return;
        }

        for row in top.saturating_add(1)..=bottom {
            for column in 0..size.columns {
                let cell = self.grid.get(row, column).cloned().unwrap_or_default();
                self.grid.set(row - 1, column, cell);
            }
        }

        for column in 0..size.columns {
            self.grid.set(bottom, column, Cell::default());
        }

        self.record_damage(DamageRegion::new(0, top, size.columns, bottom - top + 1));
    }

    fn write_char(&mut self, ch: char) {
        let ch = self.map_graphic_character(ch);
        let width = display_width(ch);
        if width == 0 {
            return;
        }

        if self.pending_wrap && self.modes.auto_wrap {
            self.newline();
        } else if self.pending_wrap {
            self.pending_wrap = false;
        }

        if self.cursor_column.saturating_add(width) > self.grid.size().columns
            && self.modes.auto_wrap
        {
            self.newline();
        }

        if self.cursor_row >= self.grid.size().rows
            || self.cursor_column >= self.grid.size().columns
            || self.grid.size().columns == 0
        {
            return;
        }

        let available_width = self.grid.size().columns - self.cursor_column;
        let write_width = width.min(available_width);
        if self.modes.write_mode == CharacterWriteMode::Insert {
            self.insert_blank_characters(write_width);
        }

        let column = self.cursor_column;
        let row = self.cursor_row;
        let mut cell = self.style.clone();
        cell.ch = ch;

        if self.grid.set(row, column, cell) {
            if write_width > 1 {
                let mut continuation = self.style.clone();
                continuation.ch = ' ';
                for offset in 1..write_width {
                    self.grid.set(row, column + offset, continuation.clone());
                }
            }

            self.record_damage(DamageRegion::new(column, row, write_width, 1));
            self.advance_cursor(write_width);
            self.last_printable = Some(ch);
        }
    }

    fn advance_cursor(&mut self, width: u16) {
        let next_column = self.cursor_column.saturating_add(width);
        if next_column >= self.grid.size().columns {
            self.cursor_column = self.grid.size().columns.saturating_sub(1);
            self.pending_wrap = self.modes.auto_wrap;
        } else {
            self.cursor_column = next_column;
            self.pending_wrap = false;
        }
    }

    fn apply_csi(&mut self, command: char, params: &[char]) {
        match command {
            '@' => self.insert_blank_characters(csi_count(params)),
            'A' => self.move_cursor_up(csi_count(params)),
            'B' | 'e' => self.move_cursor_down(csi_count(params)),
            'C' | 'a' => self.move_cursor_forward(csi_count(params)),
            'D' => self.move_cursor_back(csi_count(params)),
            'E' => self.move_cursor_next_line(csi_count(params)),
            'F' => self.move_cursor_previous_line(csi_count(params)),
            'G' | '`' => self.position_cursor_column(params),
            'H' | 'f' => self.position_cursor(params),
            'I' => self.move_forward_tabs(csi_count(params)),
            'J' => self.erase_display(csi_mode(params)),
            'K' => self.erase_line(csi_mode(params)),
            'L' => self.insert_lines(csi_count(params)),
            'M' => self.delete_lines(csi_count(params)),
            'P' => self.delete_characters(csi_count(params)),
            'S' => self.scroll_up(csi_count(params)),
            'T' => self.scroll_down(csi_count(params)),
            'X' => self.erase_characters(csi_count(params)),
            'Z' => self.move_backward_tabs(csi_count(params)),
            'b' => self.repeat_previous_character(csi_count(params)),
            'd' => self.position_cursor_row(params),
            'g' => self.clear_tab_stop(csi_mode(params)),
            'm' => self.apply_sgr(params),
            'r' => self.set_scroll_region(params),
            'h' => self.set_mode(params, true),
            'l' => self.set_mode(params, false),
            's' => self.save_cursor(),
            'u' => self.restore_cursor(),
            _ => {}
        }
    }

    fn set_mode(&mut self, params: &[char], enabled: bool) {
        if params.first() == Some(&'?') {
            self.set_private_mode(params, enabled);
        } else {
            self.set_standard_mode(params, enabled);
        }
    }

    fn set_standard_mode(&mut self, params: &[char], enabled: bool) {
        for value in parse_csi_params(params) {
            if value == 4 {
                self.set_insert_mode(enabled);
            }
        }
    }

    fn set_private_mode(&mut self, params: &[char], enabled: bool) {
        let Some(values) = parse_private_csi_params(params) else {
            return;
        };

        for value in values {
            match value {
                6 => self.set_origin_mode(enabled),
                7 => self.set_auto_wrap(enabled),
                25 => self.modes.cursor_visible = enabled,
                47 | 1047 | 1049 => self.set_alternate_screen(enabled),
                1048 => {
                    if enabled {
                        self.save_cursor();
                    } else {
                        self.restore_cursor();
                    }
                }
                _ => {}
            }
        }
    }

    fn set_insert_mode(&mut self, enabled: bool) {
        self.modes.write_mode = if enabled {
            CharacterWriteMode::Insert
        } else {
            CharacterWriteMode::Replace
        };
    }

    fn set_origin_mode(&mut self, enabled: bool) {
        self.modes.origin_mode = enabled;
        self.cursor_home();
    }

    fn set_auto_wrap(&mut self, enabled: bool) {
        self.modes.auto_wrap = enabled;
        if !enabled {
            self.pending_wrap = false;
        }
    }

    fn set_alternate_screen(&mut self, enabled: bool) {
        if enabled {
            if self.main_screen.is_some() {
                return;
            }

            let size = self.grid.size();
            self.main_screen = Some(self.screen_state());
            self.grid = TerminalGrid::new(size);
            self.cursor_row = 0;
            self.cursor_column = 0;
            self.pending_wrap = false;
            self.last_printable = None;
            self.saved_cursor = None;
            self.modes.cursor_visible = true;
            self.modes.origin_mode = false;
            self.scroll_top = 0;
            self.scroll_bottom = size.rows.saturating_sub(1);
            self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
        } else if let Some(screen) = self.main_screen.take() {
            self.restore_screen_state(screen);
            let size = self.grid.size();
            self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
        }
    }

    fn screen_state(&self) -> ScreenState {
        ScreenState {
            grid: self.grid.clone(),
            cursor_row: self.cursor_row,
            cursor_column: self.cursor_column,
            pending_wrap: self.pending_wrap,
            last_printable: self.last_printable,
            saved_cursor: self.saved_cursor.clone(),
            modes: self.modes,
            scroll_top: self.scroll_top,
            scroll_bottom: self.scroll_bottom,
            character_set: self.character_set,
            style: self.style.clone(),
        }
    }

    fn restore_screen_state(&mut self, screen: ScreenState) {
        self.grid = screen.grid;
        self.cursor_row = screen.cursor_row;
        self.cursor_column = screen.cursor_column;
        self.pending_wrap = screen.pending_wrap;
        self.last_printable = screen.last_printable;
        self.saved_cursor = screen.saved_cursor;
        self.modes = screen.modes;
        self.scroll_top = screen.scroll_top;
        self.scroll_bottom = screen.scroll_bottom;
        self.character_set = screen.character_set;
        self.style = screen.style;
        self.clamp_to_size();
    }

    fn clamp_to_size(&mut self) {
        let size = self.grid.size();
        self.cursor_row = clamp_axis(self.cursor_row, size.rows);
        self.cursor_column = clamp_axis(self.cursor_column, size.columns);
        self.scroll_top = clamp_axis(self.scroll_top, size.rows);
        self.scroll_bottom = clamp_axis(self.scroll_bottom, size.rows);
        if self.scroll_top >= self.scroll_bottom {
            self.scroll_top = 0;
            self.scroll_bottom = size.rows.saturating_sub(1);
        }
        if size.columns == 0 || size.rows == 0 {
            self.pending_wrap = false;
        }
    }

    fn cursor_home(&mut self) {
        self.pending_wrap = false;
        self.cursor_column = 0;
        self.cursor_row = if self.modes.origin_mode {
            self.scroll_top.min(self.grid.size().rows.saturating_sub(1))
        } else {
            0
        };
    }

    fn set_scroll_region(&mut self, params: &[char]) {
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let values = parse_csi_params(params);
        let top = param_or_one(values.first().copied()).saturating_sub(1);
        let bottom = values
            .get(1)
            .copied()
            .filter(|value| *value != 0)
            .unwrap_or(rows)
            .saturating_sub(1)
            .min(rows - 1);

        if top >= bottom {
            return;
        }

        self.scroll_top = top;
        self.scroll_bottom = bottom;
        self.cursor_home();
    }

    fn save_cursor(&mut self) {
        self.saved_cursor = Some(SavedCursor {
            row: self.cursor_row,
            column: self.cursor_column,
            pending_wrap: self.pending_wrap,
            origin_mode: self.modes.origin_mode,
            character_set: self.character_set,
            style: self.style.clone(),
        });
    }

    fn restore_cursor(&mut self) {
        let Some(saved) = self.saved_cursor.clone() else {
            return;
        };

        self.modes.origin_mode = saved.origin_mode;
        self.character_set = saved.character_set;
        self.style = saved.style;

        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            self.cursor_row = 0;
            self.cursor_column = 0;
            self.pending_wrap = false;
            return;
        }

        self.cursor_row = saved.row.min(size.rows - 1);
        self.cursor_column = saved.column.min(size.columns - 1);
        self.pending_wrap = saved.pending_wrap;
    }

    fn move_cursor_up(&mut self, count: u16) {
        self.pending_wrap = false;
        self.cursor_row = self.cursor_row.saturating_sub(count);
    }

    fn move_cursor_down(&mut self, count: u16) {
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        self.cursor_row = self.cursor_row.saturating_add(count).min(rows - 1);
    }

    fn move_cursor_forward(&mut self, count: u16) {
        self.pending_wrap = false;
        let columns = self.grid.size().columns;
        if columns == 0 {
            return;
        }

        self.cursor_column = self.cursor_column.saturating_add(count).min(columns - 1);
    }

    fn move_cursor_back(&mut self, count: u16) {
        self.pending_wrap = false;
        self.cursor_column = self.cursor_column.saturating_sub(count);
    }

    fn move_cursor_next_line(&mut self, count: u16) {
        self.move_cursor_down(count);
        self.cursor_column = 0;
    }

    fn move_cursor_previous_line(&mut self, count: u16) {
        self.move_cursor_up(count);
        self.cursor_column = 0;
    }

    fn position_cursor(&mut self, params: &[char]) {
        self.pending_wrap = false;
        let values = parse_csi_params(params);
        let rows = self.grid.size().rows;
        let columns = self.grid.size().columns;

        if rows == 0 || columns == 0 {
            return;
        }

        let row = self.cursor_row_from_position_param(param_or_one(values.first().copied()));
        let column = param_or_one(values.get(1).copied()).saturating_sub(1);

        self.cursor_row = row;
        self.cursor_column = column.min(columns - 1);
    }

    fn cursor_row_from_position_param(&self, param: u16) -> u16 {
        let row = param.saturating_sub(1);
        let rows = self.grid.size().rows;
        if rows == 0 {
            return 0;
        }

        if self.modes.origin_mode {
            let top = self.scroll_top.min(rows - 1);
            let bottom = self.scroll_bottom.min(rows - 1);
            top.saturating_add(row).min(bottom)
        } else {
            row.min(rows - 1)
        }
    }

    fn position_cursor_column(&mut self, params: &[char]) {
        self.pending_wrap = false;
        let columns = self.grid.size().columns;
        if columns == 0 {
            return;
        }

        let column = csi_count(params).saturating_sub(1);
        self.cursor_column = column.min(columns - 1);
    }

    fn position_cursor_row(&mut self, params: &[char]) {
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let row = csi_count(params).saturating_sub(1);
        self.cursor_row = row.min(rows - 1);
    }

    fn erase_display(&mut self, mode: u16) {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return;
        }

        match mode {
            0 => {
                self.clear_cells(self.cursor_row, self.cursor_column, size.columns);
                for row in self.cursor_row.saturating_add(1)..size.rows {
                    self.clear_cells(row, 0, size.columns);
                }
            }
            1 => {
                for row in 0..self.cursor_row {
                    self.clear_cells(row, 0, size.columns);
                }
                self.clear_cells(
                    self.cursor_row,
                    0,
                    self.cursor_column.saturating_add(1).min(size.columns),
                );
            }
            2 | 3 => {
                for row in 0..size.rows {
                    self.clear_cells(row, 0, size.columns);
                }
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let columns = self.grid.size().columns;
        if self.cursor_row >= self.grid.size().rows || columns == 0 {
            return;
        }

        match mode {
            0 => self.clear_cells(self.cursor_row, self.cursor_column, columns),
            1 => self.clear_cells(
                self.cursor_row,
                0,
                self.cursor_column.saturating_add(1).min(columns),
            ),
            2 => self.clear_cells(self.cursor_row, 0, columns),
            _ => {}
        }
    }

    fn insert_lines(&mut self, count: u16) {
        self.pending_wrap = false;
        let Some((top, bottom)) = self.active_scroll_range_from_cursor() else {
            return;
        };

        self.scroll_down_region(top, bottom, count);
    }

    fn delete_lines(&mut self, count: u16) {
        self.pending_wrap = false;
        let Some((top, bottom)) = self.active_scroll_range_from_cursor() else {
            return;
        };

        self.scroll_up_region_by(top, bottom, count);
    }

    fn scroll_up(&mut self, count: u16) {
        self.pending_wrap = false;
        let Some((top, bottom)) = self.active_scroll_range() else {
            return;
        };

        self.scroll_up_region_by(top, bottom, count);
    }

    fn scroll_down(&mut self, count: u16) {
        self.pending_wrap = false;
        let Some((top, bottom)) = self.active_scroll_range() else {
            return;
        };

        self.scroll_down_region(top, bottom, count);
    }

    fn active_scroll_range(&self) -> Option<(u16, u16)> {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return None;
        }

        let scroll_top = self.scroll_top.min(size.rows - 1);
        let scroll_bottom = self.scroll_bottom.min(size.rows - 1);
        if scroll_top > scroll_bottom {
            return None;
        }

        Some((scroll_top, scroll_bottom))
    }

    fn active_scroll_range_from_cursor(&self) -> Option<(u16, u16)> {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return None;
        }

        let scroll_top = self.scroll_top.min(size.rows - 1);
        let scroll_bottom = self.scroll_bottom.min(size.rows - 1);
        if self.cursor_row < scroll_top || self.cursor_row > scroll_bottom {
            return None;
        }

        Some((self.cursor_row, scroll_bottom))
    }

    fn scroll_down_region(&mut self, top: u16, bottom: u16, count: u16) {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 || top > bottom || count == 0 {
            return;
        }

        let height = bottom - top + 1;
        let count = count.min(height);

        if count < height {
            let shift_bottom = bottom - count;
            for row in (top..=shift_bottom).rev() {
                for column in 0..size.columns {
                    let cell = self.grid.get(row, column).cloned().unwrap_or_default();
                    self.grid.set(row + count, column, cell);
                }
            }
        }

        for row in top..top + count {
            for column in 0..size.columns {
                self.grid.set(row, column, Cell::default());
            }
        }

        self.record_damage(DamageRegion::new(0, top, size.columns, height));
    }

    fn scroll_up_region_by(&mut self, top: u16, bottom: u16, count: u16) {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 || top > bottom || count == 0 {
            return;
        }

        let height = bottom - top + 1;
        let count = count.min(height);

        if count < height {
            let shift_bottom = bottom - count;
            for row in top..=shift_bottom {
                for column in 0..size.columns {
                    let cell = self
                        .grid
                        .get(row + count, column)
                        .cloned()
                        .unwrap_or_default();
                    self.grid.set(row, column, cell);
                }
            }
        }

        let blank_start = if count == height {
            top
        } else {
            bottom - count + 1
        };
        for row in blank_start..=bottom {
            for column in 0..size.columns {
                self.grid.set(row, column, Cell::default());
            }
        }

        self.record_damage(DamageRegion::new(0, top, size.columns, height));
    }

    fn insert_blank_characters(&mut self, count: u16) {
        self.pending_wrap = false;
        let size = self.grid.size();
        if self.cursor_row >= size.rows || self.cursor_column >= size.columns || count == 0 {
            return;
        }

        let count = count.min(size.columns - self.cursor_column);
        let shift_end = size.columns - count;
        for column in (self.cursor_column..shift_end).rev() {
            let cell = self
                .grid
                .get(self.cursor_row, column)
                .cloned()
                .unwrap_or_default();
            self.grid.set(self.cursor_row, column + count, cell);
        }

        for column in self.cursor_column..self.cursor_column + count {
            self.grid.set(self.cursor_row, column, Cell::default());
        }

        self.record_damage(DamageRegion::new(
            self.cursor_column,
            self.cursor_row,
            size.columns - self.cursor_column,
            1,
        ));
    }

    fn delete_characters(&mut self, count: u16) {
        self.pending_wrap = false;
        let size = self.grid.size();
        if self.cursor_row >= size.rows || self.cursor_column >= size.columns || count == 0 {
            return;
        }

        let count = count.min(size.columns - self.cursor_column);
        let shift_end = size.columns - count;
        for column in self.cursor_column..shift_end {
            let cell = self
                .grid
                .get(self.cursor_row, column + count)
                .cloned()
                .unwrap_or_default();
            self.grid.set(self.cursor_row, column, cell);
        }

        for column in shift_end..size.columns {
            self.grid.set(self.cursor_row, column, Cell::default());
        }

        self.record_damage(DamageRegion::new(
            self.cursor_column,
            self.cursor_row,
            size.columns - self.cursor_column,
            1,
        ));
    }

    fn erase_characters(&mut self, count: u16) {
        self.pending_wrap = false;
        let size = self.grid.size();
        if self.cursor_row >= size.rows || self.cursor_column >= size.columns || count == 0 {
            return;
        }

        let count = count.min(size.columns - self.cursor_column);
        for column in self.cursor_column..self.cursor_column + count {
            self.grid.set(self.cursor_row, column, Cell::default());
        }

        self.record_damage(DamageRegion::new(
            self.cursor_column,
            self.cursor_row,
            count,
            1,
        ));
    }

    fn repeat_previous_character(&mut self, count: u16) {
        let Some(ch) = self.last_printable else {
            return;
        };

        for _ in 0..count {
            self.write_char(ch);
        }
    }

    fn clear_cells(&mut self, row: u16, start_column: u16, end_column: u16) {
        let columns = self.grid.size().columns;
        if row >= self.grid.size().rows || start_column >= columns {
            return;
        }

        let end_column = end_column.min(columns);
        if start_column >= end_column {
            return;
        }

        for column in start_column..end_column {
            self.grid.set(row, column, Cell::default());
        }

        self.record_damage(DamageRegion::new(
            start_column,
            row,
            end_column - start_column,
            1,
        ));
    }

    fn apply_sgr(&mut self, params: &[char]) {
        let values = parse_sgr_params(params);
        let mut index = 0;

        while index < values.len() {
            match values[index] {
                0 => self.style = Cell::default(),
                1 => self.style.bold = true,
                3 => self.style.italic = true,
                4 => self.style.underline = true,
                7 => self.style.inverse = true,
                22 => self.style.bold = false,
                23 => self.style.italic = false,
                24 => self.style.underline = false,
                27 => self.style.inverse = false,
                30..=37 => {
                    self.style.foreground = Color::Indexed(saturating_u8(values[index] - 30));
                }
                39 => self.style.foreground = Color::Default,
                40..=47 => {
                    self.style.background = Color::Indexed(saturating_u8(values[index] - 40));
                }
                49 => self.style.background = Color::Default,
                90..=97 => {
                    self.style.foreground = Color::Indexed(saturating_u8(values[index] - 90 + 8));
                }
                100..=107 => {
                    self.style.background = Color::Indexed(saturating_u8(values[index] - 100 + 8));
                }
                38 | 48 => {
                    let is_foreground = values[index] == 38;
                    if let Some((color, consumed)) = parse_extended_color(&values[index + 1..]) {
                        if is_foreground {
                            self.style.foreground = color;
                        } else {
                            self.style.background = color;
                        }
                        index += consumed;
                    }
                }
                _ => {}
            }

            index += 1;
        }
    }

    fn record_damage(&mut self, region: DamageRegion) {
        if region.is_empty() {
            return;
        }

        if let Some(last) = self.damage.last_mut() {
            let adjacent =
                last.y == region.y && last.height == region.height && last.right() == region.x;

            if adjacent {
                last.width = last.width.saturating_add(region.width);
                return;
            }
        }

        self.damage.push(region);
    }

    fn map_graphic_character(&self, ch: char) -> char {
        match self.character_set {
            CharacterSet::Ascii => ch,
            CharacterSet::DecSpecialGraphics => map_dec_special_graphics(ch),
        }
    }
}

fn parse_g0_character_set(selector: char) -> Option<CharacterSet> {
    match selector {
        'B' => Some(CharacterSet::Ascii),
        '0' => Some(CharacterSet::DecSpecialGraphics),
        _ => None,
    }
}

fn map_dec_special_graphics(ch: char) -> char {
    match ch {
        '`' => '◆',
        'a' => '▒',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        '~' => '·',
        _ => ch,
    }
}

fn default_tab_stops(size: TerminalSize) -> Vec<u16> {
    let mut stops = Vec::new();
    let mut column = 8;
    while column < size.columns {
        stops.push(column);
        column = column.saturating_add(8);
    }
    stops
}

fn parse_csi(chars: &[char], mut index: usize) -> Option<(char, usize)> {
    while index < chars.len() {
        let ch = chars[index];
        if ('@'..='~').contains(&ch) {
            return Some((ch, index));
        }
        index += 1;
    }

    None
}

fn clamp_screen_state(screen: &mut ScreenState, size: TerminalSize) {
    screen.cursor_row = clamp_axis(screen.cursor_row, size.rows);
    screen.cursor_column = clamp_axis(screen.cursor_column, size.columns);
    screen.scroll_top = clamp_axis(screen.scroll_top, size.rows);
    screen.scroll_bottom = clamp_axis(screen.scroll_bottom, size.rows);
    if screen.scroll_top >= screen.scroll_bottom {
        screen.scroll_top = 0;
        screen.scroll_bottom = size.rows.saturating_sub(1);
    }
    if size.columns == 0 || size.rows == 0 {
        screen.pending_wrap = false;
    }
}

fn clamp_axis(value: u16, limit: u16) -> u16 {
    value.min(limit.saturating_sub(1))
}

fn parse_osc(chars: &[char], mut index: usize) -> Option<usize> {
    while index < chars.len() {
        match chars[index] {
            '\u{7}' => return Some(index),
            '\u{1b}' if chars.get(index + 1) == Some(&'\\') => return Some(index + 1),
            _ => index += 1,
        }
    }

    None
}

fn parse_st_terminated_control_string(chars: &[char], mut index: usize) -> Option<usize> {
    while index < chars.len() {
        match chars[index] {
            '\u{1b}' if chars.get(index + 1) == Some(&'\\') => return Some(index + 1),
            _ => index += 1,
        }
    }

    None
}

fn csi_count(params: &[char]) -> u16 {
    param_or_one(parse_csi_params(params).first().copied())
}

fn csi_mode(params: &[char]) -> u16 {
    parse_csi_params(params).first().copied().unwrap_or(0)
}

fn param_or_one(value: Option<u16>) -> u16 {
    match value {
        Some(0) | None => 1,
        Some(value) => value,
    }
}

fn parse_csi_params(params: &[char]) -> Vec<u16> {
    let raw = params.iter().collect::<String>();
    raw.split(';')
        .map(|part| {
            if part.is_empty() {
                0
            } else {
                part.parse::<u16>().unwrap_or(0)
            }
        })
        .collect()
}

fn parse_private_csi_params(params: &[char]) -> Option<Vec<u16>> {
    let ['?', rest @ ..] = params else {
        return None;
    };

    Some(parse_csi_params(rest))
}

fn parse_sgr_params(params: &[char]) -> Vec<u16> {
    if params.is_empty() {
        return vec![0];
    }

    let raw = params.iter().collect::<String>();
    raw.split(';')
        .map(|part| {
            if part.is_empty() {
                0
            } else {
                part.parse::<u16>().unwrap_or(0)
            }
        })
        .collect()
}

fn parse_extended_color(values: &[u16]) -> Option<(Color, usize)> {
    match values {
        [5, index, ..] => Some((Color::Indexed(saturating_u8(*index)), 2)),
        [2, red, green, blue, ..] => Some((
            Color::Rgb(
                saturating_u8(*red),
                saturating_u8(*green),
                saturating_u8(*blue),
            ),
            4,
        )),
        _ => None,
    }
}

fn complete_utf8_prefix_len(bytes: &[u8]) -> usize {
    let Some(mut start) = bytes.len().checked_sub(1) else {
        return 0;
    };

    while start > 0 && is_utf8_continuation(bytes[start]) {
        start -= 1;
    }

    let first = bytes[start];
    let Some(expected_len) = utf8_sequence_len(first) else {
        return bytes.len();
    };

    let available_len = bytes.len() - start;
    if available_len >= expected_len {
        return bytes.len();
    }

    if bytes[start + 1..]
        .iter()
        .all(|byte| is_utf8_continuation(*byte))
    {
        start
    } else {
        bytes.len()
    }
}

fn utf8_sequence_len(byte: u8) -> Option<usize> {
    match byte {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

fn saturating_u8(value: u16) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

fn display_width(ch: char) -> u16 {
    match UnicodeWidthChar::width(ch) {
        Some(0) => 0,
        Some(width) if width > usize::from(u16::MAX) => u16::MAX,
        Some(width) => u16::try_from(width).unwrap_or(1),
        None => 1,
    }
}
