use super::{GpuLayer, PixelRect, SignedPixelRect};

const KITTY_NON_DEFAULT_BACKGROUND_Z_CUTOFF: i32 = i32::MIN / 2;

pub(crate) const fn image_layer(z_index: i32) -> GpuLayer {
    if z_index < KITTY_NON_DEFAULT_BACKGROUND_Z_CUTOFF {
        GpuLayer::UltraNegativeImage
    } else if z_index < 0 {
        GpuLayer::NegativeImage
    } else {
        GpuLayer::PositiveImage
    }
}

/// Protocol origin retained in the graph for diagnostics and parity checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageProtocol {
    Kitty,
    Iterm,
    Sixel,
}

/// One already-decoded image primitive.
///
/// Task 16 establishes clipping and z-order slots. The color is a deterministic
/// specimen input; decoded texture sampling can use the same placement metadata
/// without changing graph ordering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuImage {
    protocol: ImageProtocol,
    z_index: i32,
    signed_rect: SignedPixelRect,
    color: [u8; 4],
    kitty_id: Option<u32>,
}

impl GpuImage {
    #[must_use]
    pub fn new(protocol: ImageProtocol, z_index: i32, rect: PixelRect, color: [u8; 4]) -> Self {
        Self {
            protocol,
            z_index,
            signed_rect: SignedPixelRect::new(
                i64::from(rect.x),
                i64::from(rect.y),
                rect.width,
                rect.height,
            ),
            color,
            kitty_id: None,
        }
    }

    #[must_use]
    pub const fn new_signed(
        protocol: ImageProtocol,
        z_index: i32,
        rect: SignedPixelRect,
        color: [u8; 4],
    ) -> Self {
        Self {
            protocol,
            z_index,
            signed_rect: rect,
            color,
            kitty_id: None,
        }
    }

    /// Applies the same destination clipping rule for Kitty, iTerm, and Sixel.
    #[must_use]
    pub fn with_clip(mut self, clip: PixelRect) -> Self {
        self.signed_rect = self
            .signed_rect
            .intersection(SignedPixelRect::new(
                i64::from(clip.x),
                i64::from(clip.y),
                clip.width,
                clip.height,
            ))
            .unwrap_or_else(|| SignedPixelRect::new(0, 0, 0, 0));
        self
    }

    #[must_use]
    pub fn with_signed_clip(mut self, clip: SignedPixelRect) -> Self {
        self.signed_rect = self
            .signed_rect
            .intersection(clip)
            .unwrap_or_else(|| SignedPixelRect::new(0, 0, 0, 0));
        self
    }

    #[must_use]
    pub const fn with_kitty_id(mut self, kitty_id: u32) -> Self {
        self.kitty_id = Some(kitty_id);
        self
    }

    #[must_use]
    pub const fn protocol(self) -> ImageProtocol {
        self.protocol
    }

    #[must_use]
    pub const fn z_index(self) -> i32 {
        self.z_index
    }

    #[must_use]
    pub fn rect(self) -> PixelRect {
        self.signed_rect
            .clipped_to_positive_plane()
            .unwrap_or_else(|| PixelRect::new(0, 0, 0, 0))
    }

    #[must_use]
    pub const fn kitty_id(self) -> Option<u32> {
        self.kitty_id
    }

    #[must_use]
    pub const fn color(self) -> [u8; 4] {
        self.color
    }

    #[must_use]
    pub const fn layer(self) -> GpuLayer {
        image_layer(self.z_index)
    }
}
