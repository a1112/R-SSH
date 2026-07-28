use smol_str::SmolStr;

use crate::{Color, SemanticType, UnderlineStyle, VerticalAlign};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellContent {
    Blank,
    Text { grapheme: SmolStr, columns: u8 },
    Continuation { leader_delta: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cell {
    pub(crate) content: CellContent,
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

impl Cell {
    #[must_use]
    pub fn with_char(ch: char) -> Self {
        let mut cell = Self::default();
        // A standalone public cell cannot carry the continuation slots needed
        // for a wider span. Terminal writes construct real spans internally.
        cell.set_text(ch.to_string(), 1);
        cell
    }

    #[must_use]
    pub const fn content(&self) -> &CellContent {
        &self.content
    }

    #[must_use]
    pub fn text(&self) -> &str {
        match &self.content {
            CellContent::Blank => " ",
            CellContent::Text { grapheme, .. } => grapheme.as_str(),
            CellContent::Continuation { .. } => "",
        }
    }

    #[must_use]
    pub fn primary_char(&self) -> char {
        self.text().chars().next().unwrap_or(' ')
    }

    #[must_use]
    pub const fn columns(&self) -> u8 {
        match self.content {
            CellContent::Blank => 1,
            CellContent::Text { columns, .. } => columns,
            CellContent::Continuation { .. } => 0,
        }
    }

    #[must_use]
    pub const fn is_continuation(&self) -> bool {
        matches!(self.content, CellContent::Continuation { .. })
    }

    #[must_use]
    pub const fn is_blank(&self) -> bool {
        matches!(self.content, CellContent::Blank)
    }

    pub(crate) fn set_blank(&mut self) {
        self.content = CellContent::Blank;
    }

    pub(crate) fn set_text(&mut self, grapheme: impl Into<SmolStr>, columns: u8) {
        self.content = CellContent::Text {
            grapheme: grapheme.into(),
            columns,
        };
    }

    pub(crate) fn set_continuation(&mut self, leader_delta: u8) {
        self.content = CellContent::Continuation { leader_delta };
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            content: CellContent::Blank,
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
