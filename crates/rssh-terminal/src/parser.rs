use rssh_core::{DamageRegion, TerminalSize};
use unicode_width::UnicodeWidthChar;

use crate::{Cell, Color, TerminalGrid};

#[derive(Debug, Clone)]
pub struct Terminal {
    grid: TerminalGrid,
    cursor_row: u16,
    cursor_column: u16,
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
                        if command == 'm' {
                            self.apply_sgr(&chars[index + 2..sequence_end]);
                        }
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
        if self.cursor_row + 1 < self.grid.size().rows {
            self.cursor_row += 1;
        }
    }

    fn write_char(&mut self, ch: char) {
        let width = display_width(ch);
        if width == 0 {
            return;
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
        self.cursor_column = self.cursor_column.saturating_add(width);
        if self.cursor_column >= self.grid.size().columns {
            self.cursor_column = 0;
            if self.cursor_row + 1 < self.grid.size().rows {
                self.cursor_row += 1;
            }
        }
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
                30..=37 => self.style.foreground = Color::Indexed((values[index] - 30) as u8),
                39 => self.style.foreground = Color::Default,
                40..=47 => self.style.background = Color::Indexed((values[index] - 40) as u8),
                49 => self.style.background = Color::Default,
                90..=97 => self.style.foreground = Color::Indexed((values[index] - 90 + 8) as u8),
                100..=107 => {
                    self.style.background = Color::Indexed((values[index] - 100 + 8) as u8);
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
            let adjacent = last.y == region.y
                && last.height == region.height
                && last.right() == region.x;

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
        [5, index, ..] => Some((Color::Indexed((*index).min(u16::from(u8::MAX)) as u8), 2)),
        [2, red, green, blue, ..] => Some((
            Color::Rgb(
                (*red).min(u16::from(u8::MAX)) as u8,
                (*green).min(u16::from(u8::MAX)) as u8,
                (*blue).min(u16::from(u8::MAX)) as u8,
            ),
            4,
        )),
        _ => None,
    }
}

fn display_width(ch: char) -> u16 {
    match UnicodeWidthChar::width(ch) {
        Some(0) => 0,
        Some(width) if width > usize::from(u16::MAX) => u16::MAX,
        Some(width) => u16::try_from(width).unwrap_or(1),
        None => 1,
    }
}
