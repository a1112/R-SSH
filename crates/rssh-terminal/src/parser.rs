use rssh_core::TerminalSize;

use crate::{Cell, Color, TerminalGrid};

#[derive(Debug, Clone)]
pub struct Terminal {
    grid: TerminalGrid,
    cursor_row: u16,
    cursor_column: u16,
    style: Cell,
}

impl Terminal {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            grid: TerminalGrid::new(size),
            cursor_row: 0,
            cursor_column: 0,
            style: Cell::default(),
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

    fn newline(&mut self) {
        self.cursor_column = 0;
        if self.cursor_row + 1 < self.grid.size().rows {
            self.cursor_row += 1;
        }
    }

    fn write_char(&mut self, ch: char) {
        if self.cursor_row >= self.grid.size().rows || self.cursor_column >= self.grid.size().columns
        {
            return;
        }

        let mut cell = self.style.clone();
        cell.ch = ch;

        if self.grid.set(self.cursor_row, self.cursor_column, cell) {
            self.advance_cursor(1);
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
