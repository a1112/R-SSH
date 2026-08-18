use std::{error::Error, fmt};

/// Integer pixel rectangle used by the GPU primitive graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self
            .x
            .saturating_add(self.width)
            .min(other.x.saturating_add(other.width));
        let bottom = self
            .y
            .saturating_add(self.height)
            .min(other.y.saturating_add(other.height));
        (right > left && bottom > top).then(|| Self::new(left, top, right - left, bottom - top))
    }
}

/// Signed destination rectangle used before viewport clipping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignedPixelRect {
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
}

impl SignedPixelRect {
    #[must_use]
    pub const fn new(x: i64, y: i64, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self
            .x
            .saturating_add(i64::from(self.width))
            .min(other.x.saturating_add(i64::from(other.width)));
        let bottom = self
            .y
            .saturating_add(i64::from(self.height))
            .min(other.y.saturating_add(i64::from(other.height)));
        let width = u32::try_from(right.saturating_sub(left)).ok()?;
        let height = u32::try_from(bottom.saturating_sub(top)).ok()?;
        (width != 0 && height != 0).then(|| Self::new(left, top, width, height))
    }

    pub(crate) fn clipped_to_positive_plane(self) -> Option<PixelRect> {
        let right = self.x.saturating_add(i64::from(self.width));
        let bottom = self.y.saturating_add(i64::from(self.height));
        let left = self.x.max(0);
        let top = self.y.max(0);
        let right = right.min(i64::from(u32::MAX));
        let bottom = bottom.min(i64::from(u32::MAX));
        let x = u32::try_from(left).ok()?;
        let y = u32::try_from(top).ok()?;
        let width = u32::try_from(right.saturating_sub(left)).ok()?;
        let height = u32::try_from(bottom.saturating_sub(top)).ok()?;
        (width != 0 && height != 0).then(|| PixelRect::new(x, y, width, height))
    }
}

/// Stable terminal compositing slots, from back to front.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GpuLayer {
    PaneBackground,
    UltraNegativeImage,
    CellBackground,
    NegativeImage,
    /// Reserved for Task 17's glyph atlas.
    Glyph,
    PositiveImage,
    Underline,
    Strikethrough,
    Cursor,
    TabBar,
    Overlay,
    Selection,
}

impl GpuLayer {
    pub(crate) const ORDERED: [Self; 12] = [
        Self::PaneBackground,
        Self::UltraNegativeImage,
        Self::CellBackground,
        Self::NegativeImage,
        Self::Glyph,
        Self::PositiveImage,
        Self::Underline,
        Self::Strikethrough,
        Self::Cursor,
        Self::TabBar,
        Self::Overlay,
        Self::Selection,
    ];

    #[must_use]
    pub const fn canonical_order() -> &'static [Self] {
        &Self::ORDERED
    }

    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::PaneBackground => 0,
            Self::UltraNegativeImage => 1,
            Self::CellBackground => 2,
            Self::NegativeImage => 3,
            Self::Glyph => 4,
            Self::PositiveImage => 5,
            Self::Underline => 6,
            Self::Strikethrough => 7,
            Self::Cursor => 8,
            Self::TabBar => 9,
            Self::Overlay => 10,
            Self::Selection => 11,
        }
    }
}

/// One colored rectangle in framebuffer pixel coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuQuad {
    layer: GpuLayer,
    rect: PixelRect,
    color: [u8; 4],
}

impl GpuQuad {
    #[must_use]
    pub const fn new(layer: GpuLayer, rect: PixelRect, color: [u8; 4]) -> Self {
        Self { layer, rect, color }
    }

    #[must_use]
    pub const fn layer(self) -> GpuLayer {
        self.layer
    }

    #[must_use]
    pub const fn rect(self) -> PixelRect {
        self.rect
    }

    #[must_use]
    pub const fn color(self) -> [u8; 4] {
        self.color
    }
}

pub(crate) const INSTANCE_SIZE: usize = 32;

#[expect(
    clippy::cast_precision_loss,
    reason = "wgpu vertex attributes are f32 and texture/device dimensions are far below exact-integer range"
)]
pub(crate) fn encode_instance(rect: PixelRect, color: [u8; 4], output: &mut Vec<u8>) {
    for value in [
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
    ] {
        output.extend_from_slice(&value.to_ne_bytes());
    }
    for channel in color {
        output.extend_from_slice(&(f32::from(channel) / 255.0).to_ne_bytes());
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GpuLayerError {
    message: String,
}

impl GpuLayerError {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GpuLayerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GpuLayerError {}
