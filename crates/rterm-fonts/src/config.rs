//! Terminal font configuration.

/// Slant used to match configured font faces.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum FontStyle {
    /// Upright glyphs.
    #[default]
    Normal,
    /// Italic glyphs.
    Italic,
    /// Algorithmically slanted glyphs.
    Oblique,
}

/// Width class used to match configured font faces.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum FontStretch {
    /// A condensed face.
    Condensed,
    /// The face's normal width.
    #[default]
    Normal,
    /// An expanded face.
    Expanded,
}

/// Bidirectional handling for a terminal row.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum BidiMode {
    /// Apply the Unicode bidirectional algorithm while preserving logical maps.
    #[default]
    Auto,
}

/// Font selection and shaping controls for a terminal.
#[derive(Clone, Debug, PartialEq)]
pub struct FontConfig {
    pub(crate) primary: String,
    pub(crate) fallbacks: Vec<String>,
    pub(crate) font_size: f32,
    pub(crate) line_height: f32,
    pub(crate) cell_width: f32,
    pub(crate) weight: u16,
    pub(crate) style: FontStyle,
    pub(crate) stretch: FontStretch,
    pub(crate) ligatures: bool,
    pub(crate) features: Vec<([u8; 4], u32)>,
    pub(crate) bidi: BidiMode,
}

impl FontConfig {
    /// Creates a terminal font configuration with a primary family.
    #[must_use]
    pub fn new(primary: impl Into<String>) -> Self {
        Self {
            primary: primary.into(),
            fallbacks: Vec::new(),
            font_size: 14.0,
            line_height: 1.0,
            cell_width: 1.0,
            weight: 400,
            style: FontStyle::Normal,
            stretch: FontStretch::Normal,
            ligatures: true,
            features: Vec::new(),
            bidi: BidiMode::Auto,
        }
    }

    /// Replaces the ordered fallback family list.
    #[must_use]
    pub fn with_fallbacks<I, S>(mut self, fallbacks: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fallbacks = fallbacks.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the shaping font size in logical pixels.
    #[must_use]
    pub const fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// Sets the terminal line-height multiplier over the primary face metrics.
    #[must_use]
    pub const fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    /// Sets the terminal cell-width multiplier over the primary face advance.
    #[must_use]
    pub const fn with_cell_width(mut self, cell_width: f32) -> Self {
        self.cell_width = cell_width;
        self
    }

    /// Sets the OpenType weight value.
    #[must_use]
    pub const fn with_weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    /// Sets the font slant.
    #[must_use]
    pub const fn with_style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the font width class.
    #[must_use]
    pub const fn with_stretch(mut self, stretch: FontStretch) -> Self {
        self.stretch = stretch;
        self
    }

    /// Enables or disables standard and contextual ligatures.
    #[must_use]
    pub const fn with_ligatures(mut self, ligatures: bool) -> Self {
        self.ligatures = ligatures;
        self
    }

    /// Sets an arbitrary four-byte OpenType feature.
    #[must_use]
    pub fn with_feature(mut self, tag: [u8; 4], value: u32) -> Self {
        if let Some((_, existing)) = self.features.iter_mut().find(|(item, _)| *item == tag) {
            *existing = value;
        } else {
            self.features.push((tag, value));
        }
        self
    }

    /// Returns the primary family.
    #[must_use]
    pub fn primary(&self) -> &str {
        &self.primary
    }

    /// Returns families in exact selection order, primary first.
    pub(crate) fn families(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.primary.as_str()).chain(self.fallbacks.iter().map(String::as_str))
    }

    pub(crate) fn metrics_are_valid(&self) -> bool {
        self.font_size.is_finite()
            && self.font_size > 0.0
            && self.line_height.is_finite()
            && self.line_height > 0.0
            && self.cell_width.is_finite()
            && self.cell_width > 0.0
    }
}
