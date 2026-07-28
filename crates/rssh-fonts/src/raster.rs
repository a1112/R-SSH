//! Headless glyph rasterization backed by a byte-bounded application cache.

use std::mem;
use std::ops::{BitOr, BitOrAssign};
use std::sync::Arc;

use cosmic_text::{CacheKey, CacheKeyFlags, SwashCache, SwashContent, fontdb};

use crate::cache::{BoundedCache, CacheMetrics};
use crate::catalog::is_default_ignorable;
use crate::{FontCatalog, FontId, ShapedGlyph, ShapedRow};

const MAX_EFFECTIVE_FONT_SIZE: f32 = 1_024.0;
const MAX_RASTER_DIMENSION: u32 = 16_384;

/// Raster flags that participate in cache identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct RasterFlags(u32);

impl RasterFlags {
    /// Algorithmically skew an upright face.
    pub const FAKE_ITALIC: Self = Self(1);
    /// Disable outline hinting.
    pub const DISABLE_HINTING: Self = Self(2);
    /// Snap offsets for bitmap fonts.
    pub const PIXEL_FONT: Self = Self(4);

    const fn cosmic(self) -> CacheKeyFlags {
        CacheKeyFlags::from_bits_retain(self.0)
    }

    pub(crate) const fn from_cosmic(flags: CacheKeyFlags) -> Self {
        Self(flags.bits())
    }
}

impl BitOr for RasterFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for RasterFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Complete renderer-independent request for one glyph bitmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterRequest {
    font_id: FontId,
    glyph_id: u16,
    font_size: f32,
    /// Fractional horizontal raster position.
    pub x: f32,
    /// Fractional vertical raster position.
    pub y: f32,
    weight: u16,
    flags: RasterFlags,
    expected_visible: bool,
    is_tofu: bool,
}

impl RasterRequest {
    /// Creates a request from one row's authoritative shaping and text metadata.
    #[must_use]
    pub fn for_shaped_glyph(row: &ShapedRow, glyph: &ShapedGlyph, x: f32, y: f32) -> Self {
        let expected_visible = glyph.is_tofu
            || row.text.get(glyph.byte_range.clone()).is_some_and(|text| {
                text.chars()
                    .any(|character| !character.is_whitespace() && !is_default_ignorable(character))
            });
        Self {
            font_id: glyph.font_id,
            glyph_id: glyph.glyph_id,
            font_size: glyph.raster_font_size,
            x,
            y,
            weight: glyph.raster_weight,
            flags: glyph.raster_flags,
            expected_visible,
            is_tofu: glyph.is_tofu,
        }
    }
}

/// Pixel storage produced by the font scaler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RasterContent {
    /// One alpha byte per pixel.
    Mask(Vec<u8>),
    /// Four channel coverage bytes per pixel.
    SubpixelMask(Vec<u8>),
    /// Premultiplied or straight RGBA pixels supplied by the font.
    Rgba(Vec<u8>),
}

/// Reason a deterministic monochrome fallback was synthesized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RasterFallback {
    /// Shaping explicitly reported an uncovered/tofu cluster.
    MissingGlyph,
    /// A visible shaped glyph could not be rasterized.
    RasterFailure,
}

impl RasterContent {
    /// Raw tightly-packed pixel bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Mask(bytes) | Self::SubpixelMask(bytes) | Self::Rgba(bytes) => bytes,
        }
    }
}

/// A positioned glyph image suitable for upload to a renderer atlas.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterizedGlyph {
    /// Horizontal offset from the requested origin.
    pub left: i32,
    /// Vertical distance from the baseline to image top.
    pub top: i32,
    /// Image width in physical pixels.
    pub width: u32,
    /// Image height in physical pixels.
    pub height: u32,
    /// Validated tightly-packed pixels.
    pub content: RasterContent,
    /// Present only for a synthesized fallback image.
    pub fallback: Option<RasterFallback>,
}

/// Retained raster-cache settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterCacheConfig {
    /// Maximum accounted retained bytes, including pixel capacity and
    /// conservative entry metadata (but not temporary scaler scratch space).
    pub budget_bytes: usize,
    /// Physical pixels per logical pixel.
    pub dpi_scale: f32,
    /// User-configured font zoom.
    pub zoom: f32,
}

impl RasterCacheConfig {
    /// Creates a config with an explicit byte budget.
    #[must_use]
    pub const fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            dpi_scale: 1.0,
            zoom: 1.0,
        }
    }

    /// Sets the initial DPI and zoom scope.
    #[must_use]
    pub const fn with_scale(mut self, dpi_scale: f32, zoom: f32) -> Self {
        self.dpi_scale = dpi_scale;
        self.zoom = zoom;
        self
    }
}

impl Default for RasterCacheConfig {
    fn default() -> Self {
        Self::new(32 * 1024 * 1024)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RasterKey {
    font_id: FontId,
    catalog_fingerprint: [u8; 32],
    cosmic: CacheKey,
    dpi_bits: u32,
    zoom_bits: u32,
}

/// Bounded, instrumented glyph raster cache.
///
/// The underlying scaler is used only through its uncached API so that every
/// retained pixel remains governed by this type's byte budget.
pub struct RasterCache {
    scaler: SwashCache,
    entries: BoundedCache<RasterKey, Arc<RasterizedGlyph>>,
    catalog_scope: Option<(u64, u64, [u8; 32])>,
    scale_scope: Option<(u32, u32)>,
}

impl RasterCache {
    /// Creates an empty cache.
    #[must_use]
    pub fn new(config: RasterCacheConfig) -> Self {
        Self {
            scaler: SwashCache::new(),
            entries: BoundedCache::new(config.budget_bytes),
            catalog_scope: None,
            scale_scope: valid_scale(config.dpi_scale, config.zoom)
                .map(|()| (config.dpi_scale.to_bits(), config.zoom.to_bits())),
        }
    }

    /// Detailed cache instrumentation.
    #[must_use]
    pub const fn metrics(&self) -> CacheMetrics {
        self.entries.metrics()
    }

    /// Changes the byte budget and evicts least-recently-used glyphs as needed.
    pub fn set_budget(&mut self, budget_bytes: usize) {
        self.entries.set_budget(budget_bytes);
    }

    /// Establishes a new DPI/zoom scope and releases images from the old scale.
    pub fn set_scale(&mut self, dpi_scale: f32, zoom: f32) {
        let scope = valid_scale(dpi_scale, zoom).map(|()| (dpi_scale.to_bits(), zoom.to_bits()));
        if self.scale_scope != scope {
            self.entries.invalidate();
            self.scale_scope = scope;
            self.scaler = SwashCache::new();
        }
    }

    /// Rasterizes or reuses one glyph.
    ///
    /// Invalid requests, stale/tofu font identifiers, missing glyph images,
    /// and malformed scaler output return `None` rather than panicking.
    pub fn rasterize(
        &mut self,
        catalog: &mut FontCatalog,
        request: RasterRequest,
    ) -> Option<Arc<RasterizedGlyph>> {
        let catalog_scope = (
            catalog.incarnation(),
            catalog.generation(),
            catalog.fingerprint(),
        );
        if self.catalog_scope != Some(catalog_scope) {
            if self.catalog_scope.is_some() {
                self.entries.invalidate();
            }
            self.catalog_scope = Some(catalog_scope);
            self.scaler = SwashCache::new();
        }
        if !request.font_size.is_finite()
            || request.font_size <= 0.0
            || !request.x.is_finite()
            || !request.y.is_finite()
            || !catalog.owns(request.font_id)
        {
            return None;
        }
        let (dpi_bits, zoom_bits) = self.scale_scope?;
        let dpi_scale = f32::from_bits(dpi_bits);
        let zoom = f32::from_bits(zoom_bits);
        let effective_size = request
            .font_size
            .checked_mul(dpi_scale)?
            .checked_mul(zoom)?;
        if !effective_size.is_finite()
            || effective_size <= 0.0
            || effective_size > MAX_EFFECTIVE_FONT_SIZE
        {
            return None;
        }

        let raw_font = request.font_id.raw()?;
        let (cosmic, _, _) = CacheKey::new(
            raw_font,
            request.glyph_id,
            effective_size,
            (request.x, request.y),
            fontdb::Weight(request.weight),
            request.flags.cosmic(),
        );
        let key = RasterKey {
            font_id: request.font_id,
            catalog_fingerprint: catalog.fingerprint(),
            cosmic,
            dpi_bits,
            zoom_bits,
        };
        if let Some(image) = self.entries.get(&key) {
            return Some(image);
        }

        if request.is_tofu {
            let raster = Arc::new(synthetic_tofu(
                effective_size,
                RasterFallback::MissingGlyph,
            )?);
            self.retain(key, &raster);
            return Some(raster);
        }

        let image = self
            .scaler
            .get_image_uncached(catalog.font_system_mut(), cosmic);
        let raster = match image.map(decode_swash_image).transpose() {
            Ok(Some(Some(raster))) => Arc::new(raster),
            Ok(Some(None) | None) | Err(()) if !request.expected_visible => return None,
            Ok(Some(None) | None) | Err(()) => {
                self.entries.record_raster_failure();
                Arc::new(synthetic_tofu(
                    effective_size,
                    RasterFallback::RasterFailure,
                )?)
            }
        };
        self.retain(key, &raster);
        Some(raster)
    }

    fn retain(&mut self, key: RasterKey, raster: &Arc<RasterizedGlyph>) {
        let entry_bytes = mem::size_of::<RasterKey>()
            .saturating_add(mem::size_of::<Arc<RasterizedGlyph>>())
            .saturating_add(mem::size_of::<RasterizedGlyph>())
            .saturating_add(mem::size_of::<(u64, RasterKey)>())
            .saturating_add(mem::size_of::<usize>() * 4)
            .saturating_add(raster.content.capacity());
        self.entries.insert(key, Arc::clone(raster), entry_bytes);
    }
}

fn valid_scale(dpi_scale: f32, zoom: f32) -> Option<()> {
    (dpi_scale.is_finite() && dpi_scale > 0.0 && zoom.is_finite() && zoom > 0.0).then_some(())
}

impl RasterContent {
    fn capacity(&self) -> usize {
        match self {
            Self::Mask(bytes) | Self::SubpixelMask(bytes) | Self::Rgba(bytes) => bytes.capacity(),
        }
    }
}

fn synthetic_tofu(font_size: f32, fallback: RasterFallback) -> Option<RasterizedGlyph> {
    let width = clamped_dimension(font_size * 0.6);
    let height = clamped_dimension(font_size);
    let len = (width as usize).checked_mul(height as usize)?;
    let mut mask = vec![0; len];
    for y in 0..height {
        for x in 0..width {
            if x == 0 || y == 0 || x + 1 == width || y + 1 == height {
                let index = (y as usize)
                    .checked_mul(width as usize)?
                    .checked_add(x as usize)?;
                mask[index] = u8::MAX;
            }
        }
    }
    Some(RasterizedGlyph {
        left: 0,
        top: i32::try_from(height).unwrap_or(i32::MAX),
        width,
        height,
        content: RasterContent::Mask(mask),
        fallback: Some(fallback),
    })
}

fn decode_swash_image(image: cosmic_text::SwashImage) -> Result<Option<RasterizedGlyph>, ()> {
    let width = image.placement.width;
    let height = image.placement.height;
    if width > MAX_RASTER_DIMENSION || height > MAX_RASTER_DIMENSION {
        return Err(());
    }
    if width == 0 || height == 0 {
        return Ok(None);
    }
    let pixels = (width as usize).checked_mul(height as usize).ok_or(())?;
    let (expected_len, content) = match image.content {
        SwashContent::Mask => (pixels, RasterContent::Mask(image.data)),
        SwashContent::SubpixelMask => (
            pixels.checked_mul(4).ok_or(())?,
            RasterContent::SubpixelMask(image.data),
        ),
        SwashContent::Color => (
            pixels.checked_mul(4).ok_or(())?,
            RasterContent::Rgba(image.data),
        ),
    };
    if content.bytes().len() != expected_len {
        return Err(());
    }
    Ok(Some(RasterizedGlyph {
        left: image.placement.left,
        top: image.placement.top,
        width,
        height,
        content,
        fallback: None,
    }))
}

fn clamped_dimension(value: f32) -> u32 {
    let value = value.ceil().clamp(3.0, 1_024.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        value as u32
    }
}

trait CheckedFloatMul {
    fn checked_mul(self, rhs: Self) -> Option<Self>
    where
        Self: Sized;
}

impl CheckedFloatMul for f32 {
    fn checked_mul(self, rhs: Self) -> Option<Self> {
        let product = self * rhs;
        product.is_finite().then_some(product)
    }
}
