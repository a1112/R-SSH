use rssh_core::{DamageRegion, TerminalSize};
use unicode_width::UnicodeWidthChar;

use crate::{Cell, Color, TerminalGrid};

#[derive(Debug, Clone)]
pub struct Terminal {
    grid: TerminalGrid,
    cursor_row: u16,
    cursor_column: u16,
    pending_wrap: bool,
    style: Cell,
    damage: Vec<DamageRegion>,
}

impl Terminal {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            grid: TerminalGrid::new(size),
            cursor_row: 0,
            cursor_column: 0,
            pending_wrap: false,
            style: Cell::default(),
            damage: Vec::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let chars: Vec<char> = text.chars().collect();
        let mut index = 0;

        while index < chars.len() {
            match chars[index] {
                '\u{1b}' if chars.get(index + 1) == Some(&'[') => {
                    if let Some((command, sequence_end)) = parse_csi(&chars, index + 2) {
                        self.apply_csi(command, &chars[index + 2..sequence_end]);
                        index = sequence_end + 1;
                    } else {
                        index += 1;
                    }
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

    #[must_use]
    pub const fn grid(&self) -> &TerminalGrid {
        &self.grid
    }

    #[must_use]
    pub const fn cursor(&self) -> (u16, u16) {
        (self.cursor_row, self.cursor_column)
    }

    pub fn take_damage(&mut self) -> Vec<DamageRegion> {
        std::mem::take(&mut self.damage)
    }

    fn newline(&mut self) {
        self.cursor_column = 0;
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        if self.cursor_row + 1 < rows {
            self.cursor_row += 1;
        } else {
            self.scroll_up_one_line();
            self.cursor_row = rows - 1;
        }
    }

    fn scroll_up_one_line(&mut self) {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return;
        }

        for row in 1..size.rows {
            for column in 0..size.columns {
                let cell = self.grid.get(row, column).cloned().unwrap_or_default();
                self.grid.set(row - 1, column, cell);
            }
        }

        let bottom_row = size.rows - 1;
        for column in 0..size.columns {
            self.grid.set(bottom_row, column, Cell::default());
        }

        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    fn write_char(&mut self, ch: char) {
        let width = display_width(ch);
        if width == 0 {
            return;
        }

        if self.pending_wrap {
            self.newline();
        }

        if self.cursor_column.saturating_add(width) > self.grid.size().columns {
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
        }
    }

    fn advance_cursor(&mut self, width: u16) {
        let next_column = self.cursor_column.saturating_add(width);
        if next_column >= self.grid.size().columns {
            self.cursor_column = self.grid.size().columns.saturating_sub(1);
            self.pending_wrap = true;
        } else {
            self.cursor_column = next_column;
            self.pending_wrap = false;
        }
    }

    fn apply_csi(&mut self, command: char, params: &[char]) {
        match command {
            'A' => self.move_cursor_up(csi_count(params)),
            'B' => self.move_cursor_down(csi_count(params)),
            'C' => self.move_cursor_forward(csi_count(params)),
            'D' => self.move_cursor_back(csi_count(params)),
            'H' | 'f' => self.position_cursor(params),
            'J' => self.erase_display(csi_mode(params)),
            'K' => self.erase_line(csi_mode(params)),
            'm' => self.apply_sgr(params),
            _ => {}
        }
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

    fn position_cursor(&mut self, params: &[char]) {
        self.pending_wrap = false;
        let values = parse_csi_params(params);
        let rows = self.grid.size().rows;
        let columns = self.grid.size().columns;

        if rows == 0 || columns == 0 {
            return;
        }

        let row = param_or_one(values.first().copied()).saturating_sub(1);
        let column = param_or_one(values.get(1).copied()).saturating_sub(1);

        self.cursor_row = row.min(rows - 1);
        self.cursor_column = column.min(columns - 1);
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
                22 => self.style.bold = false,
                23 => self.style.italic = false,
                24 => self.style.underline = false,
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
