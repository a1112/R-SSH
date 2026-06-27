use std::{collections::HashSet, io::Cursor};

use font8x8::{BASIC_FONTS, UnicodeFonts};
use image::AnimationDecoder;
pub use rssh_core::DamageRegion;
use rssh_terminal::{
    Cell, Color, CursorShape, InlineImageFormat, ItermInlineImage, Terminal, TerminalGrid,
    UnderlineStyle, VerticalAlign,
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct RenderCell {
    pub row: u16,
    pub column: u16,
    pub ch: char,
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
    pub hyperlink: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCursor {
    pub row: u16,
    pub column: u16,
    pub shape: CursorShape,
    pub blinking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderCellColorRole {
    Foreground,
    Background,
    Underline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderInlineImage {
    pub row: u16,
    pub column: u16,
    pub name: Option<String>,
    pub kitty_image_id: Option<u32>,
    pub kitty_placement_id: Option<u32>,
    pub kitty_z_index: Option<i32>,
    pub size: Option<usize>,
    pub width: Option<String>,
    pub height: Option<String>,
    pub preserve_aspect_ratio: Option<bool>,
    pub image_format: InlineImageFormat,
    pub pixel_width: Option<u32>,
    pub pixel_height: Option<u32>,
    pub source_x: Option<u32>,
    pub source_y: Option<u32>,
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub target_x: Option<u32>,
    pub target_y: Option<u32>,
    pub data: Vec<u8>,
}

const KITTY_NON_DEFAULT_BACKGROUND_Z_CUTOFF: i32 = i32::MIN / 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRenderSnapshot {
    cells: Vec<RenderCell>,
    cursor: Option<RenderCursor>,
    cursor_color: Option<Color>,
    inline_images: Vec<RenderInlineImage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderGeometry {
    pub target_width: u32,
    pub target_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
}

impl RenderGeometry {
    #[must_use]
    pub const fn new(
        target_width: u32,
        target_height: u32,
        cell_width: u32,
        cell_height: u32,
    ) -> Self {
        Self {
            target_width,
            target_height,
            cell_width,
            cell_height,
        }
    }
}

pub const SCROLLBAR_TRACK_COLOR: [u8; 4] = [46, 46, 46, 255];
pub const SCROLLBAR_THUMB_COLOR: [u8; 4] = [172, 172, 172, 255];
pub const SCROLLBAR_WIDTH: u32 = 4;
const DEFAULT_DPI: u32 = 96;
pub type RenderIndexedPalette = [Option<[u8; 4]>; 256];
const DEFAULT_ANSI_PALETTE: [[u8; 4]; 16] = [
    [0, 0, 0, 255],
    [205, 49, 49, 255],
    [13, 188, 121, 255],
    [229, 229, 16, 255],
    [36, 114, 200, 255],
    [188, 63, 188, 255],
    [17, 168, 205, 255],
    [229, 229, 229, 255],
    [102, 102, 102, 255],
    [241, 76, 76, 255],
    [35, 209, 139, 255],
    [245, 245, 67, 255],
    [59, 142, 234, 255],
    [214, 112, 214, 255],
    [41, 184, 219, 255],
    [255, 255, 255, 255],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbackScrollbar {
    pub scrollback_lines: usize,
    pub viewport_rows: u16,
    pub scrollback_offset: usize,
    pub min_thumb_height: Option<RenderScrollbarThumbSize>,
    pub thumb_color: Option<[u8; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderScrollbarThumbSize {
    Pixels(u32),
    Points(u32),
    CellFractionPerMille(u32),
    Percent(u32),
}

impl ScrollbackScrollbar {
    #[must_use]
    pub fn new(
        scrollback_lines: usize,
        viewport_rows: u16,
        scrollback_offset: usize,
    ) -> Option<Self> {
        if scrollback_lines == 0 || viewport_rows == 0 {
            return None;
        }

        Some(Self {
            scrollback_lines,
            viewport_rows,
            scrollback_offset: scrollback_offset.min(scrollback_lines),
            min_thumb_height: None,
            thumb_color: None,
        })
    }

    #[must_use]
    pub const fn with_min_thumb_height_px(mut self, min_thumb_height_px: u32) -> Self {
        self.min_thumb_height = Some(RenderScrollbarThumbSize::Pixels(min_thumb_height_px));
        self
    }

    #[must_use]
    pub const fn with_min_thumb_height(
        mut self,
        min_thumb_height: RenderScrollbarThumbSize,
    ) -> Self {
        self.min_thumb_height = Some(min_thumb_height);
        self
    }

    #[must_use]
    pub const fn with_min_thumb_height_points(mut self, points: u32) -> Self {
        self.min_thumb_height = Some(RenderScrollbarThumbSize::Points(points));
        self
    }

    #[must_use]
    pub const fn with_min_thumb_height_cell_fraction_per_mille(mut self, per_mille: u32) -> Self {
        self.min_thumb_height = Some(RenderScrollbarThumbSize::CellFractionPerMille(per_mille));
        self
    }

    #[must_use]
    pub const fn with_min_thumb_height_percent(mut self, percent: u32) -> Self {
        self.min_thumb_height = Some(RenderScrollbarThumbSize::Percent(percent));
        self
    }

    #[must_use]
    pub const fn with_thumb_color(mut self, thumb_color: [u8; 4]) -> Self {
        self.thumb_color = Some(thumb_color);
        self
    }

    #[must_use]
    pub fn offset_from_pixel_y(self, y: u32, geometry: RenderGeometry) -> usize {
        self.offset_from_pixel_y_with_dpi(y, geometry, DEFAULT_DPI)
    }

    #[must_use]
    pub fn offset_from_pixel_y_with_dpi(
        self,
        y: u32,
        geometry: RenderGeometry,
        window_dpi: u32,
    ) -> usize {
        if geometry.target_height == 0 {
            return self.scrollback_offset;
        }

        let thumb_height = scrollbar_thumb_height(self, geometry, window_dpi);
        let travel = geometry.target_height.saturating_sub(thumb_height);
        if travel == 0 {
            return 0;
        }

        let y = y.min(geometry.target_height.saturating_sub(1));
        let live_distance = u64::from(y)
            .saturating_mul(self.scrollback_lines as u64)
            .saturating_add(u64::from(travel / 2))
            / u64::from(travel);
        let live_distance = usize::try_from(live_distance).unwrap_or(self.scrollback_lines);
        self.scrollback_lines
            .saturating_sub(live_distance.min(self.scrollback_lines))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelRenderer {
    blink_visible: bool,
    cursor_opacity_alpha: u8,
    text_blink_opacity_alpha: u8,
    rapid_text_blink_opacity_alpha: u8,
    bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
    ansi_palette: Option<[[u8; 4]; 16]>,
    indexed_palette: Option<RenderIndexedPalette>,
    cursor_thickness: Option<RenderCursorThickness>,
    underline_thickness: Option<RenderUnderlineThickness>,
    underline_position: Option<RenderUnderlinePosition>,
    strikethrough_position: Option<RenderStrikethroughPosition>,
    force_reverse_video_cursor: bool,
    reverse_video_cursor_min_contrast: Option<u16>,
    default_foreground: [u8; 4],
    default_background: [u8; 4],
    default_background_gradient: Option<RenderBackgroundGradient>,
    default_background_image: Option<RenderBackgroundImage>,
    default_cursor_color: [u8; 4],
    default_cursor_border: Option<[u8; 4]>,
    default_cursor_foreground: Option<[u8; 4]>,
    window_dpi: u32,
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RenderBoldBrightensAnsiColors {
    No,
    #[default]
    BrightAndBold,
    BrightOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderCursorThickness {
    Pixels(u32),
    Points(u32),
    Percent(u32),
    CellFractionPerMille(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderUnderlineThickness {
    Pixels(u32),
    Points(u32),
    Percent(u32),
    CellFractionPerMille(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderUnderlinePosition {
    Pixels(i32),
    Points(i32),
    Percent(i32),
    CellFractionPerMille(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStrikethroughPosition {
    Pixels(u32),
    Points(u32),
    Percent(u32),
    CellFractionPerMille(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBackgroundGradientOrientation {
    Horizontal,
    Vertical,
    Linear {
        angle_millidegrees: i32,
    },
    Radial {
        cx_millis: u32,
        cy_millis: u32,
        radius_millis: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBackgroundGradientInterpolation {
    Linear,
    Basis,
    CatmullRom,
}

impl RenderBackgroundGradientInterpolation {
    const fn to_colorgrad(self) -> colorgrad::Interpolation {
        match self {
            Self::Linear => colorgrad::Interpolation::Linear,
            Self::Basis => colorgrad::Interpolation::Basis,
            Self::CatmullRom => colorgrad::Interpolation::CatmullRom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBackgroundGradientBlend {
    Rgb,
    LinearRgb,
    Hsv,
    Oklab,
}

impl RenderBackgroundGradientBlend {
    const fn to_colorgrad(self) -> colorgrad::BlendMode {
        match self {
            Self::Rgb => colorgrad::BlendMode::Rgb,
            Self::LinearRgb => colorgrad::BlendMode::LinearRgb,
            Self::Hsv => colorgrad::BlendMode::Hsv,
            Self::Oklab => colorgrad::BlendMode::Oklab,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderBackgroundGradientSegment {
    pub size: usize,
    pub smoothness_millis: u32,
}

impl RenderBackgroundGradientSegment {
    fn smoothness(self) -> f64 {
        f64::from(self.smoothness_millis) / 1_000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBackgroundGradientPreset {
    Blues,
    BrBg,
    BuGn,
    BuPu,
    Cividis,
    Cool,
    CubeHelixDefault,
    GnBu,
    Greens,
    Greys,
    Inferno,
    Magma,
    OrRd,
    Oranges,
    PiYg,
    Plasma,
    PrGn,
    PuBu,
    PuBuGn,
    PuOr,
    PuRd,
    Purples,
    Rainbow,
    RdBu,
    RdGy,
    RdPu,
    RdYlBu,
    RdYlGn,
    Reds,
    Sinebow,
    Spectral,
    Turbo,
    Viridis,
    Warm,
    YlGn,
    YlGnBu,
    YlOrBr,
    YlOrRd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderBackgroundGradient {
    pub orientation: RenderBackgroundGradientOrientation,
    pub interpolation: RenderBackgroundGradientInterpolation,
    pub blend: RenderBackgroundGradientBlend,
    pub noise: Option<usize>,
    pub segment: Option<RenderBackgroundGradientSegment>,
    pub preset: Option<RenderBackgroundGradientPreset>,
    pub opacity_alpha: u8,
    pub blend_with_default_background: bool,
    pub hsb: RenderBackgroundGradientHsb,
    pub colors: Vec<[u8; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderBackgroundImage {
    pub data: Vec<u8>,
    pub opacity_alpha: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderBackgroundGradientHsb {
    pub hue: u16,
    pub saturation: u16,
    pub brightness: u16,
}

impl RenderBackgroundGradientHsb {
    pub const IDENTITY: Self = Self {
        hue: 1_000,
        saturation: 1_000,
        brightness: 1_000,
    };

    const fn is_identity(self) -> bool {
        self.hue == Self::IDENTITY.hue
            && self.saturation == Self::IDENTITY.saturation
            && self.brightness == Self::IDENTITY.brightness
    }
}

pub fn background_gradient_color_strings(
    gradient: &RenderBackgroundGradient,
    count: usize,
) -> Option<Vec<String>> {
    let sampler = BackgroundGradientSampler::from_gradient(gradient);
    if sampler.is_empty() {
        return None;
    }

    Some(
        (0..count)
            .map(|index| {
                let position = if count <= 1 {
                    0.0
                } else {
                    index as f64 / (count - 1) as f64
                };
                let color = sampler.color_at(position);
                format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
            })
            .collect(),
    )
}

impl PixelRenderer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            blink_visible: true,
            cursor_opacity_alpha: u8::MAX,
            text_blink_opacity_alpha: u8::MAX,
            rapid_text_blink_opacity_alpha: u8::MAX,
            bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors::BrightAndBold,
            ansi_palette: None,
            indexed_palette: None,
            cursor_thickness: None,
            underline_thickness: None,
            underline_position: None,
            strikethrough_position: None,
            force_reverse_video_cursor: false,
            reverse_video_cursor_min_contrast: None,
            default_foreground: default_foreground(),
            default_background: default_background(),
            default_background_gradient: None,
            default_background_image: None,
            default_cursor_color: default_foreground(),
            default_cursor_border: None,
            default_cursor_foreground: None,
            window_dpi: DEFAULT_DPI,
            animation_frame: 0,
            animation_elapsed_ms: None,
        }
    }

    #[must_use]
    pub const fn with_blink_visible(blink_visible: bool) -> Self {
        Self {
            blink_visible,
            cursor_opacity_alpha: if blink_visible { u8::MAX } else { 0 },
            text_blink_opacity_alpha: if blink_visible { u8::MAX } else { 0 },
            rapid_text_blink_opacity_alpha: if blink_visible { u8::MAX } else { 0 },
            bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors::BrightAndBold,
            ansi_palette: None,
            indexed_palette: None,
            cursor_thickness: None,
            underline_thickness: None,
            underline_position: None,
            strikethrough_position: None,
            force_reverse_video_cursor: false,
            reverse_video_cursor_min_contrast: None,
            default_foreground: default_foreground(),
            default_background: default_background(),
            default_background_gradient: None,
            default_background_image: None,
            default_cursor_color: default_foreground(),
            default_cursor_border: None,
            default_cursor_foreground: None,
            window_dpi: DEFAULT_DPI,
            animation_frame: 0,
            animation_elapsed_ms: None,
        }
    }

    #[must_use]
    pub fn with_cursor_opacity(opacity: f32) -> Self {
        let mut renderer = Self::new();
        renderer.set_cursor_opacity(opacity);
        renderer
    }

    pub fn set_cursor_opacity(&mut self, opacity: f32) {
        let alpha = opacity_alpha(opacity);
        self.blink_visible = alpha > 0;
        self.cursor_opacity_alpha = alpha;
    }

    #[must_use]
    pub fn with_text_blink_opacity(opacity: f32) -> Self {
        let mut renderer = Self::new();
        renderer.set_text_blink_opacity(opacity);
        renderer
    }

    pub fn set_text_blink_opacity(&mut self, opacity: f32) {
        self.text_blink_opacity_alpha = opacity_alpha(opacity);
    }

    #[must_use]
    pub fn with_rapid_text_blink_opacity(opacity: f32) -> Self {
        let mut renderer = Self::new();
        renderer.set_rapid_text_blink_opacity(opacity);
        renderer
    }

    pub fn set_rapid_text_blink_opacity(&mut self, opacity: f32) {
        self.rapid_text_blink_opacity_alpha = opacity_alpha(opacity);
    }

    #[must_use]
    pub fn with_bold_brightens_ansi_colors(
        bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
    ) -> Self {
        let mut renderer = Self::new();
        renderer.set_bold_brightens_ansi_colors(bold_brightens_ansi_colors);
        renderer
    }

    pub fn set_bold_brightens_ansi_colors(
        &mut self,
        bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
    ) {
        self.bold_brightens_ansi_colors = bold_brightens_ansi_colors;
    }

    pub fn set_ansi_palette(&mut self, ansi_palette: Option<[[u8; 4]; 16]>) {
        self.ansi_palette = ansi_palette;
    }

    pub fn set_indexed_palette(&mut self, indexed_palette: Option<RenderIndexedPalette>) {
        self.indexed_palette = indexed_palette;
    }

    #[must_use]
    pub fn with_cursor_thickness_px(cursor_thickness_px: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_cursor_thickness(Some(RenderCursorThickness::Pixels(cursor_thickness_px)));
        renderer
    }

    pub fn set_cursor_thickness_px(&mut self, cursor_thickness_px: Option<u32>) {
        self.set_cursor_thickness(cursor_thickness_px.map(RenderCursorThickness::Pixels));
    }

    #[must_use]
    pub fn with_cursor_thickness_points(points: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_cursor_thickness(Some(RenderCursorThickness::Points(points)));
        renderer
    }

    #[must_use]
    pub fn with_cursor_thickness_percent(percent: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_cursor_thickness(Some(RenderCursorThickness::Percent(percent)));
        renderer
    }

    #[must_use]
    pub fn with_cursor_thickness_cell_fraction_per_mille(per_mille: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_cursor_thickness(Some(RenderCursorThickness::CellFractionPerMille(per_mille)));
        renderer
    }

    pub fn set_cursor_thickness(&mut self, cursor_thickness: Option<RenderCursorThickness>) {
        self.cursor_thickness = cursor_thickness;
    }

    #[must_use]
    pub fn with_underline_thickness_px(underline_thickness_px: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_underline_thickness(Some(RenderUnderlineThickness::Pixels(
            underline_thickness_px,
        )));
        renderer
    }

    #[must_use]
    pub fn with_underline_thickness_points(points: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_underline_thickness(Some(RenderUnderlineThickness::Points(points)));
        renderer
    }

    #[must_use]
    pub fn with_underline_thickness_percent(percent: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_underline_thickness(Some(RenderUnderlineThickness::Percent(percent)));
        renderer
    }

    #[must_use]
    pub fn with_underline_thickness_cell_fraction_per_mille(per_mille: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_underline_thickness(Some(RenderUnderlineThickness::CellFractionPerMille(
            per_mille,
        )));
        renderer
    }

    pub fn set_underline_thickness(
        &mut self,
        underline_thickness: Option<RenderUnderlineThickness>,
    ) {
        self.underline_thickness = underline_thickness;
    }

    #[must_use]
    pub fn with_underline_position_px(underline_position_px: i32) -> Self {
        let mut renderer = Self::new();
        renderer
            .set_underline_position(Some(RenderUnderlinePosition::Pixels(underline_position_px)));
        renderer
    }

    #[must_use]
    pub fn with_underline_position_points(points: i32) -> Self {
        let mut renderer = Self::new();
        renderer.set_underline_position(Some(RenderUnderlinePosition::Points(points)));
        renderer
    }

    #[must_use]
    pub fn with_underline_position_percent(percent: i32) -> Self {
        let mut renderer = Self::new();
        renderer.set_underline_position(Some(RenderUnderlinePosition::Percent(percent)));
        renderer
    }

    #[must_use]
    pub fn with_underline_position_cell_fraction_per_mille(per_mille: i32) -> Self {
        let mut renderer = Self::new();
        renderer.set_underline_position(Some(RenderUnderlinePosition::CellFractionPerMille(
            per_mille,
        )));
        renderer
    }

    pub fn set_underline_position(&mut self, underline_position: Option<RenderUnderlinePosition>) {
        self.underline_position = underline_position;
    }

    #[must_use]
    pub fn with_strikethrough_position_px(strikethrough_position_px: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_strikethrough_position(Some(RenderStrikethroughPosition::Pixels(
            strikethrough_position_px,
        )));
        renderer
    }

    #[must_use]
    pub fn with_strikethrough_position_points(points: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_strikethrough_position(Some(RenderStrikethroughPosition::Points(points)));
        renderer
    }

    #[must_use]
    pub fn with_strikethrough_position_percent(percent: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_strikethrough_position(Some(RenderStrikethroughPosition::Percent(percent)));
        renderer
    }

    #[must_use]
    pub fn with_strikethrough_position_cell_fraction_per_mille(per_mille: u32) -> Self {
        let mut renderer = Self::new();
        renderer.set_strikethrough_position(Some(
            RenderStrikethroughPosition::CellFractionPerMille(per_mille),
        ));
        renderer
    }

    pub fn set_strikethrough_position(
        &mut self,
        strikethrough_position: Option<RenderStrikethroughPosition>,
    ) {
        self.strikethrough_position = strikethrough_position;
    }

    pub fn set_window_dpi(&mut self, dpi: u32) {
        self.window_dpi = dpi.max(1);
    }

    pub fn set_default_background(&mut self, background: [u8; 4]) {
        self.default_background = background;
    }

    pub fn set_default_background_gradient(&mut self, gradient: Option<RenderBackgroundGradient>) {
        self.default_background_gradient =
            gradient.filter(|gradient| gradient.preset.is_some() || !gradient.colors.is_empty());
    }

    pub fn set_default_background_image(&mut self, image: Option<RenderBackgroundImage>) {
        self.default_background_image = image.filter(|image| !image.data.is_empty());
    }

    pub fn set_default_foreground(&mut self, foreground: [u8; 4]) {
        self.default_foreground = foreground;
    }

    pub fn set_default_cursor_color(&mut self, color: [u8; 4]) {
        self.default_cursor_color = color;
    }

    pub fn set_default_cursor_border(&mut self, color: Option<[u8; 4]>) {
        self.default_cursor_border = color;
    }

    pub fn set_default_cursor_foreground(&mut self, color: Option<[u8; 4]>) {
        self.default_cursor_foreground = color;
    }

    #[must_use]
    pub const fn with_force_reverse_video_cursor(force_reverse_video_cursor: bool) -> Self {
        Self {
            blink_visible: true,
            cursor_opacity_alpha: u8::MAX,
            text_blink_opacity_alpha: u8::MAX,
            rapid_text_blink_opacity_alpha: u8::MAX,
            bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors::BrightAndBold,
            ansi_palette: None,
            indexed_palette: None,
            cursor_thickness: None,
            underline_thickness: None,
            underline_position: None,
            strikethrough_position: None,
            force_reverse_video_cursor,
            reverse_video_cursor_min_contrast: None,
            default_foreground: default_foreground(),
            default_background: default_background(),
            default_background_gradient: None,
            default_background_image: None,
            default_cursor_color: default_foreground(),
            default_cursor_border: None,
            default_cursor_foreground: None,
            window_dpi: DEFAULT_DPI,
            animation_frame: 0,
            animation_elapsed_ms: None,
        }
    }

    pub fn set_force_reverse_video_cursor(&mut self, force_reverse_video_cursor: bool) {
        self.force_reverse_video_cursor = force_reverse_video_cursor;
    }

    #[must_use]
    pub fn with_reverse_video_cursor_min_contrast(mut self, min_contrast: f64) -> Self {
        self.set_reverse_video_cursor_min_contrast(Some(min_contrast));
        self
    }

    pub fn set_reverse_video_cursor_min_contrast(&mut self, min_contrast: Option<f64>) {
        self.reverse_video_cursor_min_contrast = min_contrast.and_then(contrast_ratio_to_centi);
    }

    #[must_use]
    pub const fn with_animation_frame(animation_frame: usize) -> Self {
        Self {
            blink_visible: true,
            cursor_opacity_alpha: u8::MAX,
            text_blink_opacity_alpha: u8::MAX,
            rapid_text_blink_opacity_alpha: u8::MAX,
            bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors::BrightAndBold,
            ansi_palette: None,
            indexed_palette: None,
            cursor_thickness: None,
            underline_thickness: None,
            underline_position: None,
            strikethrough_position: None,
            force_reverse_video_cursor: false,
            reverse_video_cursor_min_contrast: None,
            default_foreground: default_foreground(),
            default_background: default_background(),
            default_background_gradient: None,
            default_background_image: None,
            default_cursor_color: default_foreground(),
            default_cursor_border: None,
            default_cursor_foreground: None,
            window_dpi: DEFAULT_DPI,
            animation_frame,
            animation_elapsed_ms: None,
        }
    }

    #[must_use]
    pub const fn with_animation_elapsed_ms(animation_elapsed_ms: u64) -> Self {
        Self {
            blink_visible: true,
            cursor_opacity_alpha: u8::MAX,
            text_blink_opacity_alpha: u8::MAX,
            rapid_text_blink_opacity_alpha: u8::MAX,
            bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors::BrightAndBold,
            ansi_palette: None,
            indexed_palette: None,
            cursor_thickness: None,
            underline_thickness: None,
            underline_position: None,
            strikethrough_position: None,
            force_reverse_video_cursor: false,
            reverse_video_cursor_min_contrast: None,
            default_foreground: default_foreground(),
            default_background: default_background(),
            default_background_gradient: None,
            default_background_image: None,
            default_cursor_color: default_foreground(),
            default_cursor_border: None,
            default_cursor_foreground: None,
            window_dpi: DEFAULT_DPI,
            animation_frame: 0,
            animation_elapsed_ms: Some(animation_elapsed_ms),
        }
    }

    pub fn set_animation_elapsed_ms(&mut self, animation_elapsed_ms: u64) {
        self.animation_elapsed_ms = Some(animation_elapsed_ms);
    }

    pub fn render(
        &self,
        snapshot: &TerminalRenderSnapshot,
        target: &mut [u8],
        target_width: u32,
        target_height: u32,
        cell_width: u32,
        cell_height: u32,
    ) {
        if target_width == 0 || target_height == 0 || cell_width == 0 || cell_height == 0 {
            return;
        }

        let mut surface = Surface {
            target,
            width: target_width,
            height: target_height,
        };

        fill_default_background(
            &mut surface,
            self.default_background,
            self.default_background_gradient.as_ref(),
        );
        render_background_image(
            &mut surface,
            self.default_background_image.as_ref(),
            Rect {
                x: 0,
                y: 0,
                width: target_width,
                height: target_height,
            },
            self.animation_frame,
            self.animation_elapsed_ms,
        );

        render_inline_images_in_z_order(
            &mut surface,
            snapshot
                .inline_images()
                .iter()
                .filter(|image| image_below_non_default_background(image)),
            cell_width,
            cell_height,
            self.animation_frame,
            self.animation_elapsed_ms,
        );

        for cell in snapshot.cells() {
            render_cell_background(
                &mut surface,
                cell,
                cell_width,
                cell_height,
                self.bold_brightens_ansi_colors,
                self.default_foreground,
                self.default_background,
                self.ansi_palette.as_ref(),
                self.indexed_palette.as_ref(),
            );
        }

        render_inline_images_in_z_order(
            &mut surface,
            snapshot.inline_images().iter().filter(|image| {
                image_below_text(image) && !image_below_non_default_background(image)
            }),
            cell_width,
            cell_height,
            self.animation_frame,
            self.animation_elapsed_ms,
        );

        for cell in snapshot.cells() {
            render_cell_foreground(
                &mut surface,
                cell,
                cell_width,
                cell_height,
                self.text_blink_opacity_alpha,
                self.rapid_text_blink_opacity_alpha,
                self.underline_thickness,
                self.underline_position,
                self.strikethrough_position,
                self.window_dpi,
                self.bold_brightens_ansi_colors,
                self.default_foreground,
                self.default_background,
                self.ansi_palette.as_ref(),
                self.indexed_palette.as_ref(),
            );
        }

        render_inline_images_in_z_order(
            &mut surface,
            snapshot
                .inline_images()
                .iter()
                .filter(|image| !image_below_text(image)),
            cell_width,
            cell_height,
            self.animation_frame,
            self.animation_elapsed_ms,
        );

        if let Some(cursor) = snapshot.cursor() {
            let cursor_cell = snapshot
                .cells()
                .iter()
                .find(|cell| cell.row == cursor.row && cell.column == cursor.column);
            let cursor_colors = cursor_colors(
                snapshot,
                cursor,
                self.force_reverse_video_cursor,
                self.reverse_video_cursor_min_contrast,
                self.bold_brightens_ansi_colors,
                self.default_foreground,
                self.default_background,
                self.ansi_palette.as_ref(),
                self.indexed_palette.as_ref(),
                cursor_shape_default_color(
                    cursor,
                    self.default_cursor_color,
                    self.default_cursor_border,
                ),
                self.default_cursor_foreground,
            );
            render_cursor(
                &mut surface,
                cursor,
                cursor_cell,
                cell_width,
                cell_height,
                CursorRenderStyle {
                    blink_visible: self.blink_visible,
                    opacity_alpha: self.cursor_opacity_alpha,
                    thickness: self.cursor_thickness,
                    window_dpi: self.window_dpi,
                    color: cursor_colors.color,
                    foreground: cursor_colors.foreground,
                    border: configured_cursor_border(
                        snapshot,
                        self.force_reverse_video_cursor,
                        self.default_cursor_border,
                    ),
                },
            );
        }
    }

    pub fn render_scrollbar(
        &self,
        scrollbar: ScrollbackScrollbar,
        target: &mut [u8],
        geometry: RenderGeometry,
    ) {
        if geometry.target_width == 0 || geometry.target_height == 0 {
            return;
        }

        let mut surface = Surface {
            target,
            width: geometry.target_width,
            height: geometry.target_height,
        };
        let track_width = SCROLLBAR_WIDTH.min(geometry.target_width);
        let track = Rect {
            x: geometry.target_width.saturating_sub(track_width),
            y: 0,
            width: track_width,
            height: geometry.target_height,
        };
        surface.fill_rect(track, SCROLLBAR_TRACK_COLOR);
        surface.fill_rect(
            scrollbar_thumb_rect(scrollbar, geometry, track_width, self.window_dpi),
            scrollbar.thumb_color.unwrap_or(SCROLLBAR_THUMB_COLOR),
        );
    }

    pub fn render_damage(
        &self,
        snapshot: &TerminalRenderSnapshot,
        damage: &[DamageRegion],
        target: &mut [u8],
        geometry: RenderGeometry,
    ) {
        if geometry.target_width == 0
            || geometry.target_height == 0
            || geometry.cell_width == 0
            || geometry.cell_height == 0
            || damage.is_empty()
        {
            return;
        }

        let mut surface = Surface {
            target,
            width: geometry.target_width,
            height: geometry.target_height,
        };

        for region in damage.iter().copied().filter(|region| !region.is_empty()) {
            let rect = damage_rect(region, geometry.cell_width, geometry.cell_height);
            fill_default_background_rect(
                &mut surface,
                rect,
                self.default_background,
                self.default_background_gradient.as_ref(),
            );
            render_background_image(
                &mut surface,
                self.default_background_image.as_ref(),
                rect,
                self.animation_frame,
                self.animation_elapsed_ms,
            );
        }

        let damaged_cells = snapshot
            .cells()
            .iter()
            .filter(|cell| damage_contains_cell(damage, cell.row, cell.column))
            .collect::<Vec<_>>();

        render_damaged_inline_images_in_z_order(
            &mut surface,
            snapshot
                .inline_images()
                .iter()
                .filter(|image| image_below_non_default_background(image)),
            damage,
            geometry,
            self.animation_frame,
            self.animation_elapsed_ms,
        );

        for cell in &damaged_cells {
            render_cell_background(
                &mut surface,
                cell,
                geometry.cell_width,
                geometry.cell_height,
                self.bold_brightens_ansi_colors,
                self.default_foreground,
                self.default_background,
                self.ansi_palette.as_ref(),
                self.indexed_palette.as_ref(),
            );
        }

        render_damaged_inline_images_in_z_order(
            &mut surface,
            snapshot.inline_images().iter().filter(|image| {
                image_below_text(image) && !image_below_non_default_background(image)
            }),
            damage,
            geometry,
            self.animation_frame,
            self.animation_elapsed_ms,
        );

        for cell in &damaged_cells {
            render_cell_foreground(
                &mut surface,
                cell,
                geometry.cell_width,
                geometry.cell_height,
                self.text_blink_opacity_alpha,
                self.rapid_text_blink_opacity_alpha,
                self.underline_thickness,
                self.underline_position,
                self.strikethrough_position,
                self.window_dpi,
                self.bold_brightens_ansi_colors,
                self.default_foreground,
                self.default_background,
                self.ansi_palette.as_ref(),
                self.indexed_palette.as_ref(),
            );
        }

        render_damaged_inline_images_in_z_order(
            &mut surface,
            snapshot
                .inline_images()
                .iter()
                .filter(|image| !image_below_text(image)),
            damage,
            geometry,
            self.animation_frame,
            self.animation_elapsed_ms,
        );

        if let Some(cursor) = snapshot
            .cursor()
            .filter(|cursor| damage_contains_cell(damage, cursor.row, cursor.column))
        {
            let cursor_cell = snapshot
                .cells()
                .iter()
                .find(|cell| cell.row == cursor.row && cell.column == cursor.column);
            let cursor_colors = cursor_colors(
                snapshot,
                cursor,
                self.force_reverse_video_cursor,
                self.reverse_video_cursor_min_contrast,
                self.bold_brightens_ansi_colors,
                self.default_foreground,
                self.default_background,
                self.ansi_palette.as_ref(),
                self.indexed_palette.as_ref(),
                cursor_shape_default_color(
                    cursor,
                    self.default_cursor_color,
                    self.default_cursor_border,
                ),
                self.default_cursor_foreground,
            );
            render_cursor(
                &mut surface,
                cursor,
                cursor_cell,
                geometry.cell_width,
                geometry.cell_height,
                CursorRenderStyle {
                    blink_visible: self.blink_visible,
                    opacity_alpha: self.cursor_opacity_alpha,
                    thickness: self.cursor_thickness,
                    window_dpi: self.window_dpi,
                    color: cursor_colors.color,
                    foreground: cursor_colors.foreground,
                    border: configured_cursor_border(
                        snapshot,
                        self.force_reverse_video_cursor,
                        self.default_cursor_border,
                    ),
                },
            );
        }
    }
}

#[derive(Debug, Clone)]
struct DecodedImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Default for PixelRenderer {
    fn default() -> Self {
        Self::new()
    }
}

struct Surface<'a> {
    target: &'a mut [u8],
    width: u32,
    height: u32,
}

#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy)]
struct CursorRenderStyle {
    blink_visible: bool,
    opacity_alpha: u8,
    thickness: Option<RenderCursorThickness>,
    window_dpi: u32,
    color: [u8; 4],
    foreground: Option<[u8; 4]>,
    border: Option<[u8; 4]>,
}

fn fill_default_background(
    surface: &mut Surface<'_>,
    color: [u8; 4],
    gradient: Option<&RenderBackgroundGradient>,
) {
    if let Some(gradient) = gradient {
        if gradient.blend_with_default_background {
            surface.fill(color);
        }
        fill_default_background_gradient(
            surface,
            Rect {
                x: 0,
                y: 0,
                width: surface.width,
                height: surface.height,
            },
            gradient,
        );
    } else {
        surface.fill(color);
    }
}

fn fill_default_background_rect(
    surface: &mut Surface<'_>,
    rect: Rect,
    color: [u8; 4],
    gradient: Option<&RenderBackgroundGradient>,
) {
    if let Some(gradient) = gradient {
        if gradient.blend_with_default_background {
            surface.fill_rect(rect, color);
        }
        fill_default_background_gradient(surface, rect, gradient);
    } else {
        surface.fill_rect(rect, color);
    }
}

fn fill_default_background_gradient(
    surface: &mut Surface<'_>,
    rect: Rect,
    gradient: &RenderBackgroundGradient,
) {
    let max_y = rect.y.saturating_add(rect.height).min(surface.height);
    let max_x = rect.x.saturating_add(rect.width).min(surface.width);
    if max_y <= rect.y || max_x <= rect.x {
        return;
    }

    let sampler = BackgroundGradientSampler::from_gradient(gradient);
    let noise_amount = background_gradient_noise_amount(gradient);
    for row in rect.y..max_y {
        for column in rect.x..max_x {
            let position = background_gradient_position_at(
                gradient,
                column,
                row,
                surface.width,
                surface.height,
                noise_amount,
            );
            let color =
                background_gradient_color_with_hsb(sampler.color_at(position), gradient.hsb);
            let color = background_gradient_color_with_opacity(color, gradient.opacity_alpha);
            let index = ((row * surface.width + column) * 4) as usize;
            if let Some(pixel) = surface.target.get_mut(index..index + 4) {
                if gradient.blend_with_default_background {
                    let background = [pixel[0], pixel[1], pixel[2], pixel[3]];
                    pixel.copy_from_slice(&source_over_rgba(background, color));
                } else {
                    pixel.copy_from_slice(&color);
                }
            }
        }
    }
}

fn source_over_rgba(background: [u8; 4], foreground: [u8; 4]) -> [u8; 4] {
    let foreground_alpha = u32::from(foreground[3]);
    let background_alpha = u32::from(background[3]);
    let inverse_alpha = u32::from(u8::MAX) - foreground_alpha;
    let alpha =
        foreground_alpha + background_alpha.saturating_mul(inverse_alpha) / u32::from(u8::MAX);
    if alpha == 0 {
        return [0, 0, 0, 0];
    }

    let channel = |index: usize| {
        let foreground_weight = u32::from(foreground[index]).saturating_mul(foreground_alpha);
        let background_weight = u32::from(background[index])
            .saturating_mul(background_alpha)
            .saturating_mul(inverse_alpha)
            / u32::from(u8::MAX);
        let value = (foreground_weight + background_weight) / alpha;
        u8::try_from(value).unwrap_or(u8::MAX)
    };

    [
        channel(0),
        channel(1),
        channel(2),
        u8::try_from(alpha.min(u32::from(u8::MAX))).unwrap_or(u8::MAX),
    ]
}

enum BackgroundGradientSampler {
    Empty,
    Single([u8; 4]),
    Gradient(colorgrad::Gradient),
}

impl BackgroundGradientSampler {
    fn from_gradient(gradient: &RenderBackgroundGradient) -> Self {
        if let Some(preset) = gradient.preset {
            return Self::Gradient(segment_colorgrad_gradient(
                colorgrad_gradient_for_preset(preset),
                gradient.segment,
            ));
        }

        match gradient.colors.as_slice() {
            [] => Self::Empty,
            [color] => Self::Single(*color),
            colors => {
                let colors = colors
                    .iter()
                    .copied()
                    .map(colorgrad_color_from_rgba)
                    .collect::<Vec<_>>();
                colorgrad::CustomGradient::new()
                    .colors(&colors)
                    .interpolation(gradient.interpolation.to_colorgrad())
                    .mode(gradient.blend.to_colorgrad())
                    .build()
                    .map(|base| segment_colorgrad_gradient(base, gradient.segment))
                    .map_or(Self::Empty, Self::Gradient)
            }
        }
    }

    const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    fn color_at(&self, position: f64) -> [u8; 4] {
        let position = position.clamp(0.0, 1.0);
        match self {
            Self::Empty => default_background(),
            Self::Single(color) => *color,
            Self::Gradient(gradient) => gradient.at(position).to_rgba8(),
        }
    }
}

fn background_gradient_color_with_hsb(
    mut color: [u8; 4],
    hsb: RenderBackgroundGradientHsb,
) -> [u8; 4] {
    if !hsb.is_identity() {
        let [red, green, blue] = transform_rgb_hsb(color[0], color[1], color[2], hsb);
        color[0] = red;
        color[1] = green;
        color[2] = blue;
    }
    color
}

fn background_gradient_color_with_opacity(mut color: [u8; 4], opacity_alpha: u8) -> [u8; 4] {
    if opacity_alpha != u8::MAX {
        color[3] = ((u16::from(color[3]) * u16::from(opacity_alpha)) / u16::from(u8::MAX)) as u8;
    }
    color
}

fn transform_rgb_hsb(red: u8, green: u8, blue: u8, hsb: RenderBackgroundGradientHsb) -> [u8; 3] {
    let red_channel = red;
    let green_channel = green;
    let max_channel = red.max(green).max(blue);
    let red = f64::from(red) / 255.0;
    let green = f64::from(green) / 255.0;
    let blue = f64::from(blue) / 255.0;

    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let delta = max - min;
    let hue = if delta <= f64::EPSILON {
        0.0
    } else if max_channel == red_channel {
        60.0 * ((green - blue) / delta).rem_euclid(6.0)
    } else if max_channel == green_channel {
        60.0 * (((blue - red) / delta) + 2.0)
    } else {
        60.0 * (((red - green) / delta) + 4.0)
    };
    let saturation = if max <= f64::EPSILON {
        0.0
    } else {
        delta / max
    };
    let value = max;

    let hue = (hue * (f64::from(hsb.hue) / 1_000.0)).rem_euclid(360.0);
    let saturation = (saturation * (f64::from(hsb.saturation) / 1_000.0)).clamp(0.0, 1.0);
    let value = (value * (f64::from(hsb.brightness) / 1_000.0)).clamp(0.0, 1.0);

    hsv_to_rgb(hue, saturation, value)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> [u8; 3] {
    let chroma = value * saturation;
    let hue_sector = hue / 60.0;
    let x = chroma * (1.0 - (hue_sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = if hue_sector < 1.0 {
        (chroma, x, 0.0)
    } else if hue_sector < 2.0 {
        (x, chroma, 0.0)
    } else if hue_sector < 3.0 {
        (0.0, chroma, x)
    } else if hue_sector < 4.0 {
        (0.0, x, chroma)
    } else if hue_sector < 5.0 {
        (x, 0.0, chroma)
    } else {
        (chroma, 0.0, x)
    };
    let m = value - chroma;

    [
        round_rgb_component(red + m),
        round_rgb_component(green + m),
        round_rgb_component(blue + m),
    ]
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn round_rgb_component(component: f64) -> u8 {
    (component.mul_add(255.0, 1e-9)).round().clamp(0.0, 255.0) as u8
}

fn segment_colorgrad_gradient(
    gradient: colorgrad::Gradient,
    segment: Option<RenderBackgroundGradientSegment>,
) -> colorgrad::Gradient {
    match segment {
        Some(segment) => gradient.sharp(segment.size, segment.smoothness()),
        None => gradient,
    }
}

fn colorgrad_color_from_rgba(color: [u8; 4]) -> colorgrad::Color {
    colorgrad::Color::new(
        f64::from(color[0]) / 255.0,
        f64::from(color[1]) / 255.0,
        f64::from(color[2]) / 255.0,
        f64::from(color[3]) / 255.0,
    )
}

fn background_gradient_position_at(
    gradient: &RenderBackgroundGradient,
    column: u32,
    row: u32,
    width: u32,
    height: u32,
    noise_amount: usize,
) -> f64 {
    match gradient.orientation {
        RenderBackgroundGradientOrientation::Horizontal => {
            gradient_axis_position_with_noise(column, width, column, row, noise_amount)
        }
        RenderBackgroundGradientOrientation::Vertical => {
            1.0 - gradient_axis_position_with_noise(row, height, column, row, noise_amount)
        }
        RenderBackgroundGradientOrientation::Linear { angle_millidegrees } => {
            linear_gradient_axis_position(
                column,
                row,
                width,
                height,
                angle_millidegrees,
                noise_amount,
            )
        }
        RenderBackgroundGradientOrientation::Radial {
            cx_millis,
            cy_millis,
            radius_millis,
        } => radial_gradient_axis_position(
            column,
            row,
            width,
            height,
            cx_millis,
            cy_millis,
            radius_millis,
            noise_amount,
        ),
    }
}

fn gradient_axis_position(value: u32, extent: u32) -> f64 {
    if extent <= 1 {
        return 0.0;
    }

    f64::from(value.min(extent - 1)) / f64::from(extent - 1)
}

fn gradient_axis_position_with_noise(
    value: u32,
    extent: u32,
    column: u32,
    row: u32,
    noise_amount: usize,
) -> f64 {
    if extent <= 1 {
        return 0.0;
    }

    let noise = background_gradient_noise_offset(column, row, noise_amount);
    (f64::from(value.min(extent - 1)) + noise) / f64::from(extent - 1)
}

fn linear_gradient_axis_position(
    column: u32,
    row: u32,
    width: u32,
    height: u32,
    angle_millidegrees: i32,
    noise_amount: usize,
) -> f64 {
    let x = gradient_axis_position(column, width);
    let y = gradient_axis_position(row, height);
    let radians = (f64::from(angle_millidegrees) / 1_000.0).to_radians();
    let axis_x = radians.cos();
    let axis_y = -radians.sin();
    let pixel_noise = background_gradient_noise_offset(column, row, noise_amount);
    let noise = if width <= 1 {
        0.0
    } else {
        pixel_noise / f64::from(width - 1)
    };
    let projection = x.mul_add(axis_x, y * axis_y) + noise;
    let corners = [0.0, axis_x, axis_y, axis_x + axis_y];
    let min = corners.iter().copied().fold(f64::INFINITY, f64::min);
    let max = corners.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    if (max - min).abs() <= f64::EPSILON {
        return 0.0;
    }

    (projection - min) / (max - min)
}

fn radial_gradient_axis_position(
    column: u32,
    row: u32,
    width: u32,
    height: u32,
    cx_millis: u32,
    cy_millis: u32,
    radius_millis: u32,
    noise_amount: usize,
) -> f64 {
    if radius_millis == 0 {
        return 0.0;
    }

    if noise_amount > 0 {
        return radial_gradient_axis_position_with_noise(
            column,
            row,
            width,
            height,
            cx_millis,
            cy_millis,
            radius_millis,
            noise_amount,
        );
    }

    let x = gradient_axis_position(column, width);
    let y = gradient_axis_position(row, height);
    let cx = f64::from(cx_millis) / 1_000.0;
    let cy = f64::from(cy_millis) / 1_000.0;
    let radius = f64::from(radius_millis) / 1_000.0;
    let dx = x - cx;
    let dy = y - cy;

    dx.hypot(dy) / radius
}

fn radial_gradient_axis_position_with_noise(
    column: u32,
    row: u32,
    width: u32,
    height: u32,
    cx_millis: u32,
    cy_millis: u32,
    radius_millis: u32,
    noise_amount: usize,
) -> f64 {
    let width = width.max(1);
    let height = height.max(1);
    let radius = (f64::from(width) * f64::from(radius_millis) / 1_000.0).max(f64::EPSILON);
    let cx = f64::from(width) * f64::from(cx_millis) / 1_000.0;
    let cy = f64::from(height) * f64::from(cy_millis) / 1_000.0;
    let x = f64::from(column.min(width - 1));
    let y = f64::from(row.min(height - 1));
    let noise_limit = noise_amount as f64;
    let nx = if (cx - x).abs() < noise_limit {
        0.0
    } else {
        background_gradient_noise_offset(column, row, noise_amount)
    };
    let ny = if (cy - y).abs() < noise_limit {
        0.0
    } else {
        background_gradient_noise_offset(row, column, noise_amount)
    };
    let value = nx + (x - cx).powi(2) + (ny + y - cy).powi(2);

    if value <= 0.0 {
        0.0
    } else {
        value.sqrt() / radius
    }
}

fn background_gradient_noise_amount(gradient: &RenderBackgroundGradient) -> usize {
    gradient.noise.unwrap_or(match gradient.orientation {
        RenderBackgroundGradientOrientation::Radial { .. } => 16,
        RenderBackgroundGradientOrientation::Horizontal
        | RenderBackgroundGradientOrientation::Vertical
        | RenderBackgroundGradientOrientation::Linear { .. } => 64,
    })
}

fn background_gradient_noise_offset(column: u32, row: u32, noise_amount: usize) -> f64 {
    if noise_amount == 0 {
        return 0.0;
    }

    let amount = u64::try_from(noise_amount).unwrap_or(u64::MAX);
    let hash =
        splitmix64(u64::from(column) ^ u64::from(row).rotate_left(21) ^ 0x9e37_79b9_7f4a_7c15);
    -((hash % amount) as f64)
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[allow(clippy::too_many_lines)]
fn colorgrad_gradient_for_preset(preset: RenderBackgroundGradientPreset) -> colorgrad::Gradient {
    match preset {
        RenderBackgroundGradientPreset::Blues => colorgrad::blues(),
        RenderBackgroundGradientPreset::BrBg => colorgrad::br_bg(),
        RenderBackgroundGradientPreset::BuGn => colorgrad::bu_gn(),
        RenderBackgroundGradientPreset::BuPu => colorgrad::bu_pu(),
        RenderBackgroundGradientPreset::Cividis => colorgrad::cividis(),
        RenderBackgroundGradientPreset::Cool => colorgrad::cool(),
        RenderBackgroundGradientPreset::CubeHelixDefault => colorgrad::cubehelix_default(),
        RenderBackgroundGradientPreset::GnBu => colorgrad::gn_bu(),
        RenderBackgroundGradientPreset::Greens => colorgrad::greens(),
        RenderBackgroundGradientPreset::Greys => colorgrad::greys(),
        RenderBackgroundGradientPreset::Inferno => colorgrad::inferno(),
        RenderBackgroundGradientPreset::Magma => colorgrad::magma(),
        RenderBackgroundGradientPreset::OrRd => colorgrad::or_rd(),
        RenderBackgroundGradientPreset::Oranges => colorgrad::oranges(),
        RenderBackgroundGradientPreset::PiYg => colorgrad::pi_yg(),
        RenderBackgroundGradientPreset::Plasma => colorgrad::plasma(),
        RenderBackgroundGradientPreset::PrGn => colorgrad::pr_gn(),
        RenderBackgroundGradientPreset::PuBu => colorgrad::pu_bu(),
        RenderBackgroundGradientPreset::PuBuGn => colorgrad::pu_bu_gn(),
        RenderBackgroundGradientPreset::PuOr => colorgrad::pu_or(),
        RenderBackgroundGradientPreset::PuRd => colorgrad::pu_rd(),
        RenderBackgroundGradientPreset::Purples => colorgrad::purples(),
        RenderBackgroundGradientPreset::Rainbow => colorgrad::rainbow(),
        RenderBackgroundGradientPreset::RdBu => colorgrad::rd_bu(),
        RenderBackgroundGradientPreset::RdGy => colorgrad::rd_gy(),
        RenderBackgroundGradientPreset::RdPu => colorgrad::rd_pu(),
        RenderBackgroundGradientPreset::RdYlBu => colorgrad::rd_yl_bu(),
        RenderBackgroundGradientPreset::RdYlGn => colorgrad::rd_yl_gn(),
        RenderBackgroundGradientPreset::Reds => colorgrad::reds(),
        RenderBackgroundGradientPreset::Sinebow => colorgrad::sinebow(),
        RenderBackgroundGradientPreset::Spectral => colorgrad::spectral(),
        RenderBackgroundGradientPreset::Turbo => colorgrad::turbo(),
        RenderBackgroundGradientPreset::Viridis => colorgrad::viridis(),
        RenderBackgroundGradientPreset::Warm => colorgrad::warm(),
        RenderBackgroundGradientPreset::YlGn => colorgrad::yl_gn(),
        RenderBackgroundGradientPreset::YlGnBu => colorgrad::yl_gn_bu(),
        RenderBackgroundGradientPreset::YlOrBr => colorgrad::yl_or_br(),
        RenderBackgroundGradientPreset::YlOrRd => colorgrad::yl_or_rd(),
    }
}

impl Surface<'_> {
    fn fill(&mut self, color: [u8; 4]) {
        for pixel in self.target.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
        let max_y = rect.y.saturating_add(rect.height).min(self.height);
        let max_x = rect.x.saturating_add(rect.width).min(self.width);

        for row in rect.y..max_y {
            for column in rect.x..max_x {
                let index = ((row * self.width + column) * 4) as usize;
                if let Some(pixel) = self.target.get_mut(index..index + 4) {
                    pixel.copy_from_slice(&color);
                }
            }
        }
    }

    fn fill_rect_alpha(&mut self, rect: Rect, color: [u8; 4], alpha: u8) {
        if alpha == u8::MAX {
            self.fill_rect(rect, color);
            return;
        }
        if alpha == 0 {
            return;
        }

        let max_y = rect.y.saturating_add(rect.height).min(self.height);
        let max_x = rect.x.saturating_add(rect.width).min(self.width);
        let alpha = u16::from(alpha);
        let inverse_alpha = u16::from(u8::MAX).saturating_sub(alpha);

        for row in rect.y..max_y {
            for column in rect.x..max_x {
                let index = ((row * self.width + column) * 4) as usize;
                if let Some(pixel) = self.target.get_mut(index..index + 4) {
                    pixel[0] = blend_channel(color[0], pixel[0], alpha, inverse_alpha);
                    pixel[1] = blend_channel(color[1], pixel[1], alpha, inverse_alpha);
                    pixel[2] = blend_channel(color[2], pixel[2], alpha, inverse_alpha);
                    pixel[3] = u8::MAX;
                }
            }
        }
    }

    fn stroke_rect(&mut self, rect: Rect, color: [u8; 4], alpha: u8) {
        if rect.width == 0 || rect.height == 0 || alpha == 0 {
            return;
        }
        if rect.width == 1 || rect.height == 1 {
            self.fill_rect_alpha(rect, color, alpha);
            return;
        }

        self.fill_rect_alpha(Rect { height: 1, ..rect }, color, alpha);
        if rect.height > 1 {
            self.fill_rect_alpha(
                Rect {
                    y: rect.y + rect.height - 1,
                    height: 1,
                    ..rect
                },
                color,
                alpha,
            );
        }
        if rect.width > 1 {
            self.fill_rect_alpha(Rect { width: 1, ..rect }, color, alpha);
            self.fill_rect_alpha(
                Rect {
                    x: rect.x + rect.width - 1,
                    width: 1,
                    ..rect
                },
                color,
                alpha,
            );
        }
    }

    fn put_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }

        let index = ((y * self.width + x) * 4) as usize;
        if let Some(pixel) = self.target.get_mut(index..index + 4) {
            pixel.copy_from_slice(&color);
        }
    }
}

fn blend_channel(foreground: u8, background: u8, alpha: u16, inverse_alpha: u16) -> u8 {
    let blended = u16::from(foreground)
        .saturating_mul(alpha)
        .saturating_add(u16::from(background).saturating_mul(inverse_alpha))
        / u16::from(u8::MAX);
    u8::try_from(blended).unwrap_or(u8::MAX)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn opacity_alpha(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * f32::from(u8::MAX)) as u8
}

fn render_inline_image(
    surface: &mut Surface<'_>,
    image: &RenderInlineImage,
    cell_width: u32,
    cell_height: u32,
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
) {
    let Some(decoded) = decode_inline_image(image, animation_frame, animation_elapsed_ms) else {
        return;
    };
    let rect = inline_image_rect(image, cell_width, cell_height);
    let Some(source_rect) = inline_image_source_rect(image, decoded.width, decoded.height) else {
        return;
    };
    if rect.width == 0 || rect.height == 0 || source_rect.width == 0 || source_rect.height == 0 {
        return;
    }

    for target_y in 0..rect.height {
        let source_y = source_rect.y + target_y.saturating_mul(source_rect.height) / rect.height;
        for target_x in 0..rect.width {
            let source_x = source_rect.x + target_x.saturating_mul(source_rect.width) / rect.width;
            if let Some(pixel) = rgba_pixel(&decoded, source_x, source_y) {
                if pixel[3] == 0 {
                    continue;
                }
                surface.put_pixel(rect.x + target_x, rect.y + target_y, pixel);
            }
        }
    }
}

fn render_background_image(
    surface: &mut Surface<'_>,
    image: Option<&RenderBackgroundImage>,
    rect: Rect,
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
) {
    let Some(image) = image else {
        return;
    };
    let Some(decoded) = decode_image_rgba(&image.data, animation_frame, animation_elapsed_ms)
    else {
        return;
    };
    if decoded.width == 0 || decoded.height == 0 {
        return;
    }
    let max_y = rect.y.saturating_add(rect.height).min(surface.height);
    let max_x = rect.x.saturating_add(rect.width).min(surface.width);
    if max_y <= rect.y || max_x <= rect.x || surface.width == 0 || surface.height == 0 {
        return;
    }

    let scale = (f64::from(surface.width) / f64::from(decoded.width))
        .max(f64::from(surface.height) / f64::from(decoded.height));
    if !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let scaled_width = f64::from(decoded.width) * scale;
    let scaled_height = f64::from(decoded.height) * scale;
    let offset_x = (scaled_width - f64::from(surface.width)) / 2.0;
    let offset_y = (scaled_height - f64::from(surface.height)) / 2.0;

    for target_y in rect.y..max_y {
        let source_y =
            (((f64::from(target_y) + offset_y) / scale).floor() as u32).min(decoded.height - 1);
        for target_x in rect.x..max_x {
            let source_x =
                (((f64::from(target_x) + offset_x) / scale).floor() as u32).min(decoded.width - 1);
            if let Some(mut pixel) = rgba_pixel(&decoded, source_x, source_y) {
                pixel[3] = u8::try_from(
                    u16::from(pixel[3]).saturating_mul(u16::from(image.opacity_alpha))
                        / u16::from(u8::MAX),
                )
                .unwrap_or(u8::MAX);
                if pixel[3] == 0 {
                    continue;
                }
                let index = ((target_y * surface.width + target_x) * 4) as usize;
                if let Some(background) = surface.target.get_mut(index..index + 4) {
                    let background_pixel =
                        [background[0], background[1], background[2], background[3]];
                    background.copy_from_slice(&source_over_rgba(background_pixel, pixel));
                }
            }
        }
    }
}

fn render_inline_images_in_z_order<'a>(
    surface: &mut Surface<'_>,
    images: impl Iterator<Item = &'a RenderInlineImage>,
    cell_width: u32,
    cell_height: u32,
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
) {
    let mut images = images.enumerate().collect::<Vec<_>>();
    images.sort_by(|(left_index, left), (right_index, right)| {
        image_z_index(left)
            .cmp(&image_z_index(right))
            .then_with(|| match (left.kitty_image_id, right.kitty_image_id) {
                (Some(left_id), Some(right_id)) => left_id.cmp(&right_id),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| left_index.cmp(right_index))
    });

    for (_, image) in images {
        render_inline_image(
            surface,
            image,
            cell_width,
            cell_height,
            animation_frame,
            animation_elapsed_ms,
        );
    }
}

fn render_damaged_inline_images_in_z_order<'a>(
    surface: &mut Surface<'_>,
    images: impl Iterator<Item = &'a RenderInlineImage>,
    damage: &[DamageRegion],
    geometry: RenderGeometry,
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
) {
    render_inline_images_in_z_order(
        surface,
        images.filter(|image| {
            damage_intersects_inline_image(damage, image, geometry.cell_width, geometry.cell_height)
        }),
        geometry.cell_width,
        geometry.cell_height,
        animation_frame,
        animation_elapsed_ms,
    );
}

fn image_below_text(image: &RenderInlineImage) -> bool {
    image.kitty_z_index.is_some_and(|z_index| z_index < 0)
}

fn image_below_non_default_background(image: &RenderInlineImage) -> bool {
    image
        .kitty_z_index
        .is_some_and(|z_index| z_index < KITTY_NON_DEFAULT_BACKGROUND_Z_CUTOFF)
}

fn image_z_index(image: &RenderInlineImage) -> i32 {
    image.kitty_z_index.unwrap_or(0)
}

fn decode_inline_image(
    image: &RenderInlineImage,
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
) -> Option<DecodedImage> {
    match image.image_format {
        InlineImageFormat::Encoded => {
            decode_image_rgba(&image.data, animation_frame, animation_elapsed_ms)
        }
        InlineImageFormat::Rgb => {
            decode_raw_rgb(&image.data, image.pixel_width?, image.pixel_height?)
        }
        InlineImageFormat::Rgba => {
            decode_raw_rgba(&image.data, image.pixel_width?, image.pixel_height?)
        }
    }
}

fn decode_image_rgba(
    data: &[u8],
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
) -> Option<DecodedImage> {
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return decode_gif_frame_rgba(data, animation_frame, animation_elapsed_ms);
    }

    let image = image::load_from_memory(data).ok()?.to_rgba8();
    let width = image.width();
    let height = image.height();

    Some(DecodedImage {
        width,
        height,
        pixels: image.into_raw(),
    })
}

fn decode_gif_frame_rgba(
    data: &[u8],
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
) -> Option<DecodedImage> {
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(data)).ok()?;
    let frames = decoder.into_frames().collect_frames().ok()?;
    let frame_count = frames.len();
    if frame_count == 0 {
        return None;
    }

    let frame_index = animation_elapsed_ms.map_or(animation_frame % frame_count, |elapsed_ms| {
        gif_frame_index_for_elapsed_ms(&frames, elapsed_ms)
    });
    let image = frames.into_iter().nth(frame_index)?.into_buffer();
    let width = image.width();
    let height = image.height();

    Some(DecodedImage {
        width,
        height,
        pixels: image.into_raw(),
    })
}

fn gif_frame_index_for_elapsed_ms(frames: &[image::Frame], elapsed_ms: u64) -> usize {
    let total_duration_ms = frames.iter().fold(0_u64, |total, frame| {
        total.saturating_add(gif_frame_delay_ms(frame))
    });
    if total_duration_ms == 0 {
        return 0;
    }

    let elapsed_ms = elapsed_ms % total_duration_ms;
    let mut frame_start_ms = 0_u64;
    for (index, frame) in frames.iter().enumerate() {
        frame_start_ms = frame_start_ms.saturating_add(gif_frame_delay_ms(frame));
        if elapsed_ms < frame_start_ms {
            return index;
        }
    }

    0
}

fn gif_frame_delay_ms(frame: &image::Frame) -> u64 {
    let (numerator, denominator) = frame.delay().numer_denom_ms();
    if denominator == 0 {
        return 0;
    }

    u64::from(numerator) / u64::from(denominator)
}

fn decode_raw_rgb(data: &[u8], width: u32, height: u32) -> Option<DecodedImage> {
    validate_raw_image_len(data.len(), width, height, 3)?;

    let mut pixels = Vec::with_capacity(
        usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?
            .checked_mul(4)?,
    );
    for rgb in data.chunks_exact(3) {
        pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }

    Some(DecodedImage {
        width,
        height,
        pixels,
    })
}

fn decode_raw_rgba(data: &[u8], width: u32, height: u32) -> Option<DecodedImage> {
    validate_raw_image_len(data.len(), width, height, 4)?;

    Some(DecodedImage {
        width,
        height,
        pixels: data.to_vec(),
    })
}

fn validate_raw_image_len(
    len: usize,
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
) -> Option<()> {
    let expected_len = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(bytes_per_pixel)?;
    (len == expected_len).then_some(())
}

fn rgba_pixel(image: &DecodedImage, x: u32, y: u32) -> Option<[u8; 4]> {
    if x >= image.width || y >= image.height {
        return None;
    }

    let index = usize::try_from((y * image.width + x) * 4).ok()?;
    let pixel = image.pixels.get(index..index + 4)?;
    Some([pixel[0], pixel[1], pixel[2], pixel[3]])
}

fn inline_image_rect(image: &RenderInlineImage, cell_width: u32, cell_height: u32) -> Rect {
    Rect {
        x: u32::from(image.column)
            .saturating_mul(cell_width)
            .saturating_add(image.target_x.unwrap_or(0)),
        y: u32::from(image.row)
            .saturating_mul(cell_height)
            .saturating_add(image.target_y.unwrap_or(0)),
        width: inline_image_axis_pixels(image.width.as_deref(), cell_width),
        height: inline_image_axis_pixels(image.height.as_deref(), cell_height),
    }
}

fn inline_image_source_rect(
    image: &RenderInlineImage,
    decoded_width: u32,
    decoded_height: u32,
) -> Option<Rect> {
    if decoded_width == 0 || decoded_height == 0 {
        return None;
    }

    let x = image.source_x.unwrap_or(0);
    let y = image.source_y.unwrap_or(0);
    if x >= decoded_width || y >= decoded_height {
        return None;
    }

    let available_width = decoded_width - x;
    let available_height = decoded_height - y;
    let width = image
        .source_width
        .unwrap_or(available_width)
        .min(available_width);
    let height = image
        .source_height
        .unwrap_or(available_height)
        .min(available_height);

    (width > 0 && height > 0).then_some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn inline_image_axis_pixels(value: Option<&str>, cell_pixels: u32) -> u32 {
    let Some(value) = value else {
        return cell_pixels;
    };
    if let Some(pixels) = value.strip_suffix("px").and_then(parse_positive_u32) {
        return pixels;
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .map_or(cell_pixels, |cells| cells.saturating_mul(cell_pixels))
}

fn parse_positive_u32(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|value| *value > 0)
}

fn render_cell_background(
    surface: &mut Surface<'_>,
    cell: &RenderCell,
    cell_width: u32,
    cell_height: u32,
    bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
    default_foreground: [u8; 4],
    default_background: [u8; 4],
    ansi_palette: Option<&[[u8; 4]; 16]>,
    indexed_palette: Option<&RenderIndexedPalette>,
) {
    let origin_x = u32::from(cell.column).saturating_mul(cell_width);
    let origin_y = u32::from(cell.row).saturating_mul(cell_height);
    let (_, background) = effective_cell_colors(
        cell,
        bold_brightens_ansi_colors,
        default_foreground,
        default_background,
        ansi_palette,
        indexed_palette,
    );
    if background == default_background {
        return;
    }

    let rect = Rect {
        x: origin_x,
        y: origin_y,
        width: cell_width,
        height: cell_height,
    };
    if background[3] == u8::MAX {
        surface.fill_rect(rect, background);
    } else {
        surface.fill_rect_alpha(rect, background, background[3]);
    }
}

fn render_cell_foreground(
    surface: &mut Surface<'_>,
    cell: &RenderCell,
    cell_width: u32,
    cell_height: u32,
    text_blink_opacity_alpha: u8,
    rapid_text_blink_opacity_alpha: u8,
    underline_thickness: Option<RenderUnderlineThickness>,
    underline_position: Option<RenderUnderlinePosition>,
    strikethrough_position: Option<RenderStrikethroughPosition>,
    window_dpi: u32,
    bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
    default_foreground: [u8; 4],
    default_background: [u8; 4],
    ansi_palette: Option<&[[u8; 4]; 16]>,
    indexed_palette: Option<&RenderIndexedPalette>,
) {
    let origin_x = u32::from(cell.column).saturating_mul(cell_width);
    let origin_y = u32::from(cell.row).saturating_mul(cell_height);
    let (foreground, _) = effective_cell_colors(
        cell,
        bold_brightens_ansi_colors,
        default_foreground,
        default_background,
        ansi_palette,
        indexed_palette,
    );
    let foreground_alpha = text_foreground_alpha(
        cell,
        text_blink_opacity_alpha,
        rapid_text_blink_opacity_alpha,
    );

    if cell.conceal || foreground_alpha == 0 {
        return;
    }

    let Some(glyph) = BASIC_FONTS.get(cell.ch) else {
        return;
    };

    let scale_x = cell_width.max(8) / 8;
    let scale_y = cell_height.max(8) / 8;

    for (glyph_y, row_bits) in glyph.iter().enumerate() {
        let row_offset = italic_row_offset(glyph_y, scale_x, cell.italic);
        for glyph_x in 0..8 {
            if row_bits & (1 << glyph_x) == 0 {
                continue;
            }

            let draw_x = origin_x + glyph_x * scale_x + row_offset;
            let Some(draw_y) = vertical_aligned_y(
                origin_y,
                cell_height,
                u32::try_from(glyph_y).unwrap_or(0) * scale_y,
                cell.vertical_align,
            ) else {
                continue;
            };
            let Some(width) = clipped_cell_width(draw_x, origin_x, cell_width, scale_x) else {
                continue;
            };
            surface.fill_rect_alpha(
                Rect {
                    x: draw_x,
                    y: draw_y,
                    width,
                    height: scale_y,
                },
                foreground,
                foreground_alpha,
            );
            let bold_x = draw_x.saturating_add(scale_x);
            if cell_draws_bold(cell, bold_brightens_ansi_colors)
                && bold_x < origin_x.saturating_add(cell_width)
            {
                surface.fill_rect_alpha(
                    Rect {
                        x: bold_x,
                        y: draw_y,
                        width: scale_x,
                        height: scale_y,
                    },
                    foreground,
                    foreground_alpha,
                );
            }
        }
    }

    render_text_decorations(
        surface,
        cell,
        Rect {
            x: origin_x,
            y: origin_y,
            width: cell_width,
            height: cell_height,
        },
        foreground,
        color_to_rgba_with_palette(
            cell.underline_color,
            foreground,
            ansi_palette,
            indexed_palette,
        ),
        foreground_alpha,
        underline_thickness,
        underline_position,
        strikethrough_position,
        window_dpi,
    );
}

fn text_foreground_alpha(
    cell: &RenderCell,
    text_blink_opacity_alpha: u8,
    rapid_text_blink_opacity_alpha: u8,
) -> u8 {
    if !cell.blink {
        return u8::MAX;
    }

    if cell.rapid_blink {
        rapid_text_blink_opacity_alpha
    } else {
        text_blink_opacity_alpha
    }
}

fn effective_cell_colors(
    cell: &RenderCell,
    bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
    default_foreground: [u8; 4],
    default_background: [u8; 4],
    ansi_palette: Option<&[[u8; 4]; 16]>,
    indexed_palette: Option<&RenderIndexedPalette>,
) -> ([u8; 4], [u8; 4]) {
    let foreground = color_to_rgba_with_palette(
        effective_cell_foreground(cell, bold_brightens_ansi_colors),
        default_foreground,
        ansi_palette,
        indexed_palette,
    );
    let background = color_to_rgba_with_palette(
        cell.background,
        default_background,
        ansi_palette,
        indexed_palette,
    );
    let (foreground, background) = if cell.inverse {
        (background, foreground)
    } else {
        (foreground, background)
    };
    let foreground = if cell.faint {
        dim_foreground(foreground)
    } else {
        foreground
    };

    (foreground, background)
}

fn effective_cell_foreground(
    cell: &RenderCell,
    bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
) -> Color {
    let Color::Indexed(index @ 0..=7) = cell.foreground else {
        return cell.foreground;
    };

    if cell.bold && bold_brightens_ansi_colors != RenderBoldBrightensAnsiColors::No {
        Color::Indexed(index.saturating_add(8))
    } else {
        cell.foreground
    }
}

fn cell_draws_bold(
    cell: &RenderCell,
    bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
) -> bool {
    if !cell.bold {
        return false;
    }

    if bold_brightens_ansi_colors != RenderBoldBrightensAnsiColors::BrightOnly {
        return true;
    }

    !matches!(cell.foreground, Color::Indexed(0..=7))
}

fn italic_row_offset(glyph_y: usize, scale_x: u32, italic: bool) -> u32 {
    if italic {
        u32::try_from(7usize.saturating_sub(glyph_y)).unwrap_or(0) / 3 * scale_x
    } else {
        0
    }
}

fn vertical_aligned_y(
    origin_y: u32,
    cell_height: u32,
    glyph_y: u32,
    vertical_align: VerticalAlign,
) -> Option<u32> {
    let offset = (cell_height / 4).max(1);
    match vertical_align {
        VerticalAlign::Baseline => Some(origin_y.saturating_add(glyph_y)),
        VerticalAlign::Superscript => {
            let y = glyph_y.saturating_sub(offset);
            Some(origin_y.saturating_add(y))
        }
        VerticalAlign::Subscript => {
            let y = glyph_y.saturating_add(offset);
            (y < cell_height).then_some(origin_y.saturating_add(y))
        }
    }
}

fn clipped_cell_width(draw_x: u32, origin_x: u32, cell_width: u32, width: u32) -> Option<u32> {
    let cell_right = origin_x.saturating_add(cell_width);
    if draw_x >= cell_right {
        None
    } else {
        Some(width.min(cell_right - draw_x))
    }
}

fn render_text_decorations(
    surface: &mut Surface<'_>,
    cell: &RenderCell,
    cell_rect: Rect,
    foreground: [u8; 4],
    underline_color: [u8; 4],
    foreground_alpha: u8,
    underline_thickness: Option<RenderUnderlineThickness>,
    underline_position: Option<RenderUnderlinePosition>,
    strikethrough_position: Option<RenderStrikethroughPosition>,
    window_dpi: u32,
) {
    render_underline_style(
        surface,
        cell,
        cell_rect,
        underline_color,
        foreground_alpha,
        underline_thickness,
        underline_position,
        window_dpi,
    );

    if cell.overline {
        let overline_height = (cell_rect.height / 8).max(1);
        surface.fill_rect_alpha(
            Rect {
                x: cell_rect.x,
                y: cell_rect.y,
                width: cell_rect.width,
                height: overline_height,
            },
            foreground,
            foreground_alpha,
        );
    }

    if cell.strikethrough {
        let strike_height = (cell_rect.height / 8).max(1);
        let strike_y = cell_rect.y.saturating_add(strikethrough_position_px(
            strikethrough_position,
            cell_rect.height,
            strike_height,
            window_dpi,
        ));
        surface.fill_rect_alpha(
            Rect {
                x: cell_rect.x,
                y: strike_y,
                width: cell_rect.width,
                height: strike_height,
            },
            foreground,
            foreground_alpha,
        );
    }
}

fn render_underline_style(
    surface: &mut Surface<'_>,
    cell: &RenderCell,
    cell_rect: Rect,
    underline_color: [u8; 4],
    foreground_alpha: u8,
    underline_thickness: Option<RenderUnderlineThickness>,
    underline_position: Option<RenderUnderlinePosition>,
    window_dpi: u32,
) {
    let style = effective_underline_style(cell);
    if style == UnderlineStyle::None {
        return;
    }

    let underline_height =
        underline_thickness_px(underline_thickness, cell_rect.height, window_dpi);
    let lower_y = cell_rect.y.saturating_add(underline_position_px(
        underline_position,
        cell_rect.height,
        underline_height,
        window_dpi,
    ));
    let lower_rect = Rect {
        x: cell_rect.x,
        y: lower_y,
        width: cell_rect.width,
        height: underline_height,
    };

    match style {
        UnderlineStyle::None => {}
        UnderlineStyle::Single => {
            surface.fill_rect_alpha(lower_rect, underline_color, foreground_alpha);
        }
        UnderlineStyle::Double => {
            surface.fill_rect_alpha(lower_rect, underline_color, foreground_alpha);
            surface.fill_rect_alpha(
                Rect {
                    y: lower_y.saturating_sub(underline_height.saturating_mul(2)),
                    ..lower_rect
                },
                underline_color,
                foreground_alpha,
            );
        }
        UnderlineStyle::Curly => {
            render_curly_underline(surface, lower_rect, underline_color, foreground_alpha);
        }
        UnderlineStyle::Dotted => {
            render_patterned_underline(
                surface,
                lower_rect,
                underline_color,
                foreground_alpha,
                1,
                1,
            );
        }
        UnderlineStyle::Dashed => {
            render_patterned_underline(
                surface,
                lower_rect,
                underline_color,
                foreground_alpha,
                3,
                2,
            );
        }
    }
}

fn effective_underline_style(cell: &RenderCell) -> UnderlineStyle {
    match cell.underline_style {
        UnderlineStyle::None if cell.double_underline => UnderlineStyle::Double,
        UnderlineStyle::None if cell.underline => UnderlineStyle::Single,
        style => style,
    }
}

fn render_patterned_underline(
    surface: &mut Surface<'_>,
    rect: Rect,
    color: [u8; 4],
    alpha: u8,
    stroke_width: u32,
    gap_width: u32,
) {
    let cycle = stroke_width.saturating_add(gap_width).max(1);
    let mut offset = 0;
    while offset < rect.width {
        let segment_width = stroke_width.min(rect.width - offset);
        surface.fill_rect_alpha(
            Rect {
                x: rect.x + offset,
                width: segment_width,
                ..rect
            },
            color,
            alpha,
        );
        offset = offset.saturating_add(cycle);
    }
}

fn render_curly_underline(surface: &mut Surface<'_>, rect: Rect, color: [u8; 4], alpha: u8) {
    let upper_y = rect.y.saturating_sub(rect.height);
    for offset in 0..rect.width {
        let y = if (offset / 2) % 2 == 0 {
            upper_y
        } else {
            rect.y
        };
        surface.fill_rect_alpha(
            Rect {
                x: rect.x + offset,
                y,
                width: 1,
                height: rect.height,
            },
            color,
            alpha,
        );
    }
}

fn render_cursor(
    surface: &mut Surface<'_>,
    cursor: RenderCursor,
    cell: Option<&RenderCell>,
    cell_width: u32,
    cell_height: u32,
    style: CursorRenderStyle,
) {
    if cursor.blinking && !style.blink_visible {
        return;
    }

    let origin_x = u32::from(cursor.column).saturating_mul(cell_width);
    let origin_y = u32::from(cursor.row).saturating_mul(cell_height);
    let rect = cursor_rect(
        cursor.shape,
        origin_x,
        origin_y,
        cell_width,
        cell_height,
        style.thickness,
        style.window_dpi,
    );
    let cursor_alpha = if cursor.blinking {
        style.opacity_alpha
    } else {
        u8::MAX
    };
    if cursor_alpha < u8::MAX {
        surface.fill_rect_alpha(rect, style.color, cursor_alpha);
    } else {
        surface.fill_rect(rect, style.color);
    }

    if cursor.shape == CursorShape::Block
        && let Some(border) = style.border
    {
        surface.stroke_rect(rect, border, cursor_alpha);
    }

    if cursor.shape == CursorShape::Block
        && let (Some(cell), Some(foreground)) = (cell, style.foreground)
    {
        render_cursor_cell_foreground(surface, cell, cell_width, cell_height, foreground);
    }
}

fn render_cursor_cell_foreground(
    surface: &mut Surface<'_>,
    cell: &RenderCell,
    cell_width: u32,
    cell_height: u32,
    foreground: [u8; 4],
) {
    if cell.conceal {
        return;
    }
    let Some(glyph) = BASIC_FONTS.get(cell.ch) else {
        return;
    };

    let origin_x = u32::from(cell.column).saturating_mul(cell_width);
    let origin_y = u32::from(cell.row).saturating_mul(cell_height);
    let scale_x = cell_width.max(8) / 8;
    let scale_y = cell_height.max(8) / 8;

    for (glyph_y, row_bits) in glyph.iter().enumerate() {
        let row_offset = italic_row_offset(glyph_y, scale_x, cell.italic);
        for glyph_x in 0..8 {
            if row_bits & (1 << glyph_x) == 0 {
                continue;
            }

            let draw_x = origin_x + glyph_x * scale_x + row_offset;
            let Some(draw_y) = vertical_aligned_y(
                origin_y,
                cell_height,
                u32::try_from(glyph_y).unwrap_or(0) * scale_y,
                cell.vertical_align,
            ) else {
                continue;
            };
            let Some(width) = clipped_cell_width(draw_x, origin_x, cell_width, scale_x) else {
                continue;
            };
            surface.fill_rect(
                Rect {
                    x: draw_x,
                    y: draw_y,
                    width,
                    height: scale_y,
                },
                foreground,
            );
        }
    }
}

#[derive(Clone, Copy)]
struct CursorColors {
    color: [u8; 4],
    foreground: Option<[u8; 4]>,
}

fn cursor_colors(
    snapshot: &TerminalRenderSnapshot,
    cursor: RenderCursor,
    force_reverse_video_cursor: bool,
    reverse_video_cursor_min_contrast: Option<u16>,
    bold_brightens_ansi_colors: RenderBoldBrightensAnsiColors,
    default_foreground: [u8; 4],
    default_background: [u8; 4],
    ansi_palette: Option<&[[u8; 4]; 16]>,
    indexed_palette: Option<&RenderIndexedPalette>,
    default_cursor_color: [u8; 4],
    default_cursor_foreground: Option<[u8; 4]>,
) -> CursorColors {
    if let Some(color) = snapshot.cursor_color() {
        return CursorColors {
            color: color_to_rgba_with_palette(
                color,
                default_foreground,
                ansi_palette,
                indexed_palette,
            ),
            foreground: None,
        };
    }

    if !force_reverse_video_cursor {
        return CursorColors {
            color: default_cursor_color,
            foreground: default_cursor_foreground,
        };
    }

    let Some(cell) = snapshot
        .cells()
        .iter()
        .find(|cell| cell.row == cursor.row && cell.column == cursor.column)
    else {
        return CursorColors {
            color: default_foreground,
            foreground: None,
        };
    };
    let (reverse_background, reverse_foreground) = effective_cell_colors(
        cell,
        bold_brightens_ansi_colors,
        default_foreground,
        default_background,
        ansi_palette,
        indexed_palette,
    );

    if reverse_video_cursor_min_contrast.is_some_and(|minimum| {
        contrast_ratio(reverse_foreground, reverse_background) < f64::from(minimum) / 100.0
    }) {
        CursorColors {
            color: default_cursor_color,
            foreground: default_cursor_foreground,
        }
    } else {
        CursorColors {
            color: reverse_background,
            foreground: None,
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn contrast_ratio_to_centi(value: f64) -> Option<u16> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some((value * 100.0).round().min(f64::from(u16::MAX)) as u16)
}

fn contrast_ratio(foreground: [u8; 4], background: [u8; 4]) -> f64 {
    let foreground_luminance = relative_luminance(foreground);
    let background_luminance = relative_luminance(background);
    let light = foreground_luminance.max(background_luminance);
    let dark = foreground_luminance.min(background_luminance);
    (light + 0.05) / (dark + 0.05)
}

fn relative_luminance(color: [u8; 4]) -> f64 {
    let red = linear_srgb_component(color[0]);
    let green = linear_srgb_component(color[1]);
    let blue = linear_srgb_component(color[2]);
    0.2126 * red + 0.7152 * green + 0.0722 * blue
}

fn linear_srgb_component(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.03928 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn cursor_shape_default_color(
    cursor: RenderCursor,
    default_cursor_color: [u8; 4],
    default_cursor_border: Option<[u8; 4]>,
) -> [u8; 4] {
    if cursor.shape == CursorShape::Block {
        default_cursor_color
    } else {
        default_cursor_border.unwrap_or(default_cursor_color)
    }
}

fn configured_cursor_border(
    snapshot: &TerminalRenderSnapshot,
    force_reverse_video_cursor: bool,
    default_cursor_border: Option<[u8; 4]>,
) -> Option<[u8; 4]> {
    if force_reverse_video_cursor || snapshot.cursor_color().is_some() {
        None
    } else {
        default_cursor_border
    }
}

fn damage_rect(region: DamageRegion, cell_width: u32, cell_height: u32) -> Rect {
    Rect {
        x: u32::from(region.x).saturating_mul(cell_width),
        y: u32::from(region.y).saturating_mul(cell_height),
        width: u32::from(region.width).saturating_mul(cell_width),
        height: u32::from(region.height).saturating_mul(cell_height),
    }
}

fn damage_contains_cell(damage: &[DamageRegion], row: u16, column: u16) -> bool {
    damage.iter().copied().any(|region| {
        !region.is_empty()
            && row >= region.y
            && row < region.y.saturating_add(region.height)
            && column >= region.x
            && column < region.x.saturating_add(region.width)
    })
}

fn damage_intersects_inline_image(
    damage: &[DamageRegion],
    image: &RenderInlineImage,
    cell_width: u32,
    cell_height: u32,
) -> bool {
    let image_rect = inline_image_rect(image, cell_width, cell_height);
    damage.iter().copied().any(|region| {
        !region.is_empty()
            && rects_intersect(image_rect, damage_rect(region, cell_width, cell_height))
    })
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    let a_right = a.x.saturating_add(a.width);
    let a_bottom = a.y.saturating_add(a.height);
    let b_right = b.x.saturating_add(b.width);
    let b_bottom = b.y.saturating_add(b.height);

    a.x < b_right && a_right > b.x && a.y < b_bottom && a_bottom > b.y
}

fn scrollbar_thumb_rect(
    scrollbar: ScrollbackScrollbar,
    geometry: RenderGeometry,
    track_width: u32,
    window_dpi: u32,
) -> Rect {
    let thumb_height = scrollbar_thumb_height(scrollbar, geometry, window_dpi);
    let travel = geometry.target_height.saturating_sub(thumb_height);
    let scrollback_lines = scrollbar.scrollback_lines as u64;
    let live_distance = scrollbar
        .scrollback_lines
        .saturating_sub(scrollbar.scrollback_offset) as u64;
    let thumb_y = if scrollback_lines == 0 {
        0
    } else {
        u32::try_from(u64::from(travel).saturating_mul(live_distance) / scrollback_lines)
            .unwrap_or(travel)
    };

    Rect {
        x: geometry.target_width.saturating_sub(track_width),
        y: thumb_y.min(travel),
        width: track_width,
        height: thumb_height,
    }
}

fn scrollbar_thumb_height(
    scrollbar: ScrollbackScrollbar,
    geometry: RenderGeometry,
    window_dpi: u32,
) -> u32 {
    let viewport_rows = u64::from(scrollbar.viewport_rows);
    let total_rows = viewport_rows.saturating_add(scrollbar.scrollback_lines as u64);
    let target_height = geometry.target_height;
    let target_height_u64 = u64::from(target_height);
    let proportional_height = if total_rows == 0 {
        target_height_u64
    } else {
        target_height_u64.saturating_mul(viewport_rows) / total_rows
    };
    let min_thumb_height =
        scrollbar_min_thumb_height(scrollbar.min_thumb_height, geometry, window_dpi).max(1);

    u32::try_from(proportional_height)
        .unwrap_or(target_height)
        .max(min_thumb_height)
        .min(target_height)
}

fn scrollbar_min_thumb_height(
    min_thumb_height: Option<RenderScrollbarThumbSize>,
    geometry: RenderGeometry,
    window_dpi: u32,
) -> u32 {
    match min_thumb_height {
        Some(RenderScrollbarThumbSize::Pixels(pixels)) => pixels,
        Some(RenderScrollbarThumbSize::Points(points)) => points_to_pixels(points, window_dpi),
        Some(RenderScrollbarThumbSize::CellFractionPerMille(per_mille)) => {
            geometry.cell_height.saturating_mul(per_mille) / 1_000
        }
        Some(RenderScrollbarThumbSize::Percent(percent)) => {
            geometry.target_height.saturating_mul(percent) / 100
        }
        None => geometry.cell_height.div_ceil(2),
    }
}

#[must_use]
pub fn color_to_rgba(color: Color, default: [u8; 4]) -> [u8; 4] {
    color_to_rgba_with_palette(color, default, None, None)
}

fn color_to_rgba_with_palette(
    color: Color,
    default: [u8; 4],
    ansi_palette: Option<&[[u8; 4]; 16]>,
    indexed_palette: Option<&RenderIndexedPalette>,
) -> [u8; 4] {
    match color {
        Color::Default => default,
        Color::Indexed(index) => indexed_color(index, ansi_palette, indexed_palette),
        Color::Rgb(red, green, blue) => [red, green, blue, 255],
        Color::Rgba(red, green, blue, alpha) => [red, green, blue, alpha],
    }
}

fn dim_foreground(color: [u8; 4]) -> [u8; 4] {
    [color[0] / 2, color[1] / 2, color[2] / 2, color[3]]
}

fn indexed_color(
    index: u8,
    ansi_palette: Option<&[[u8; 4]; 16]>,
    indexed_palette: Option<&RenderIndexedPalette>,
) -> [u8; 4] {
    let ansi_palette = ansi_palette.unwrap_or(&DEFAULT_ANSI_PALETTE);
    if let Some(color) = ansi_palette.get(usize::from(index)) {
        return *color;
    }

    if let Some(color) = indexed_palette
        .and_then(|palette| palette.get(usize::from(index)))
        .copied()
        .flatten()
    {
        return color;
    }

    if (16..=231).contains(&index) {
        let cube_index = index - 16;
        let red = xterm_color_cube_intensity(cube_index / 36);
        let green = xterm_color_cube_intensity((cube_index / 6) % 6);
        let blue = xterm_color_cube_intensity(cube_index % 6);
        return [red, green, blue, 255];
    }

    let level = 8 + (index - 232) * 10;
    [level, level, level, 255]
}

const fn xterm_color_cube_intensity(value: u8) -> u8 {
    if value == 0 { 0 } else { 55 + value * 40 }
}

const fn default_foreground() -> [u8; 4] {
    [229, 229, 229, 255]
}

const fn default_background() -> [u8; 4] {
    [12, 12, 12, 255]
}

impl TerminalRenderSnapshot {
    #[must_use]
    pub fn from_grid(grid: &TerminalGrid) -> Self {
        Self::from_grid_with_cursor(grid, None)
    }

    #[must_use]
    pub fn from_terminal(terminal: &Terminal) -> Self {
        Self::from_terminal_viewport(terminal, 0)
    }

    #[must_use]
    pub fn from_terminal_viewport(terminal: &Terminal, scrollback_offset: usize) -> Self {
        let grid = terminal.grid();
        let size = grid.size();
        let scrollback = terminal.scrollback();
        let offset = scrollback_offset.min(scrollback.len());
        let first_source_row = scrollback.len().saturating_sub(offset);
        let cursor = render_cursor_from_terminal(terminal, offset);

        let mut cells = Vec::new();
        for viewport_row in 0..size.rows {
            let source_row = first_source_row + usize::from(viewport_row);
            if source_row < scrollback.len() {
                append_render_cells(
                    &mut cells,
                    viewport_row,
                    scrollback[source_row].cells(),
                    size.columns,
                    terminal.screen_reverse_video(),
                );
            } else {
                let grid_row = source_row - scrollback.len();
                append_grid_row(
                    &mut cells,
                    grid,
                    viewport_row,
                    grid_row,
                    size.columns,
                    terminal.screen_reverse_video(),
                );
            }
        }

        let inline_images =
            render_inline_images_from_terminal(terminal, first_source_row, size.rows, size.columns);

        Self {
            cells,
            cursor,
            cursor_color: None,
            inline_images,
        }
    }

    fn from_grid_with_cursor(grid: &TerminalGrid, cursor: Option<RenderCursor>) -> Self {
        let size = grid.size();
        let mut cells = Vec::new();

        for row in 0..size.rows {
            for column in 0..size.columns {
                let Some(cell) = grid.get(row, column) else {
                    continue;
                };

                if !cell_has_renderable_content(cell) {
                    continue;
                }

                cells.push(RenderCell {
                    row,
                    column,
                    ch: cell.ch,
                    foreground: cell.foreground,
                    background: cell.background,
                    underline_color: cell.underline_color,
                    underline_style: cell.underline_style,
                    bold: cell.bold,
                    faint: cell.faint,
                    italic: cell.italic,
                    blink: cell.blink,
                    rapid_blink: cell.rapid_blink,
                    underline: cell.underline,
                    double_underline: cell.double_underline,
                    conceal: cell.conceal,
                    strikethrough: cell.strikethrough,
                    overline: cell.overline,
                    vertical_align: cell.vertical_align,
                    inverse: cell.inverse,
                    hyperlink: cell.hyperlink.clone(),
                });
            }
        }

        Self {
            cells,
            cursor,
            cursor_color: None,
            inline_images: Vec::new(),
        }
    }

    #[must_use]
    pub fn cells(&self) -> &[RenderCell] {
        &self.cells
    }

    #[must_use]
    pub fn missing_glyphs(&self) -> Vec<char> {
        let mut missing = Vec::new();
        for cell in &self.cells {
            if BASIC_FONTS.get(cell.ch).is_none() && !missing.contains(&cell.ch) {
                missing.push(cell.ch);
            }
        }
        missing
    }

    #[must_use]
    pub fn inline_images(&self) -> &[RenderInlineImage] {
        &self.inline_images
    }

    #[must_use]
    pub const fn cursor(&self) -> Option<RenderCursor> {
        self.cursor
    }

    #[must_use]
    pub const fn cursor_color(&self) -> Option<Color> {
        self.cursor_color
    }

    #[must_use]
    pub const fn with_cursor_color(mut self, cursor_color: Option<Color>) -> Self {
        self.cursor_color = cursor_color;
        self
    }

    pub fn set_cursor_color(&mut self, cursor_color: Option<Color>) {
        self.cursor_color = cursor_color;
    }

    #[must_use]
    pub fn with_row_offset(mut self, offset: u16) -> Self {
        if offset == 0 {
            return self;
        }

        for cell in &mut self.cells {
            cell.row = cell.row.saturating_add(offset);
        }
        for image in &mut self.inline_images {
            image.row = image.row.saturating_add(offset);
        }
        if let Some(cursor) = self.cursor.as_mut() {
            cursor.row = cursor.row.saturating_add(offset);
        }

        self
    }

    #[must_use]
    pub fn with_viewport(
        mut self,
        origin_row: u16,
        origin_column: u16,
        rows: u16,
        columns: u16,
    ) -> Self {
        self.cells
            .retain(|cell| cell.row < rows && cell.column < columns);
        self.inline_images
            .retain(|image| image.row < rows && image.column < columns);
        for cell in &mut self.cells {
            cell.row = cell.row.saturating_add(origin_row);
            cell.column = cell.column.saturating_add(origin_column);
        }
        for image in &mut self.inline_images {
            image.row = image.row.saturating_add(origin_row);
            image.column = image.column.saturating_add(origin_column);
        }

        self.cursor = self.cursor.and_then(|mut cursor| {
            if cursor.row >= rows || cursor.column >= columns {
                return None;
            }
            cursor.row = cursor.row.saturating_add(origin_row);
            cursor.column = cursor.column.saturating_add(origin_column);
            Some(cursor)
        });

        self
    }

    #[must_use]
    pub fn with_overlay_cells(mut self, cells: impl IntoIterator<Item = RenderCell>) -> Self {
        let cells = cells.into_iter().collect::<Vec<_>>();
        if cells.is_empty() {
            return self;
        }
        let overlay_positions = cells
            .iter()
            .map(|cell| (cell.row, cell.column))
            .collect::<HashSet<_>>();
        self.cells
            .retain(|cell| !overlay_positions.contains(&(cell.row, cell.column)));
        self.cells.extend(cells);
        self.cells.sort_by_key(|cell| (cell.row, cell.column));
        self
    }

    #[must_use]
    pub fn with_overlay_snapshot(mut self, snapshot: Self) -> Self {
        self.cells.extend(snapshot.cells);
        self.cells.sort_by_key(|cell| (cell.row, cell.column));
        self.inline_images.extend(snapshot.inline_images);
        self.inline_images
            .sort_by_key(|image| (image.row, image.column));
        self
    }

    #[must_use]
    pub fn with_cells_mapped(mut self, mut map_cell: impl FnMut(RenderCell) -> RenderCell) -> Self {
        self.cells = self.cells.into_iter().map(&mut map_cell).collect();
        self
    }

    #[must_use]
    pub fn with_cell_colors_mapped(
        mut self,
        mut map_color: impl FnMut(RenderCellColorRole, Color) -> Color,
    ) -> Self {
        for cell in &mut self.cells {
            cell.foreground = map_color(RenderCellColorRole::Foreground, cell.foreground);
            cell.background = map_color(RenderCellColorRole::Background, cell.background);
            cell.underline_color = map_color(RenderCellColorRole::Underline, cell.underline_color);
        }
        self
    }

    pub fn update_from_terminal_damage(&mut self, terminal: &Terminal, damage: &[DamageRegion]) {
        let grid = terminal.grid();
        let size = grid.size();
        for region in damage.iter().copied().filter(|region| !region.is_empty()) {
            let start_row = region.y.min(size.rows);
            let end_row = region.y.saturating_add(region.height).min(size.rows);
            let start_column = region.x.min(size.columns);
            let end_column = region.x.saturating_add(region.width).min(size.columns);
            if start_row >= end_row || start_column >= end_column {
                continue;
            }

            self.cells.retain(|cell| {
                cell.row < start_row
                    || cell.row >= end_row
                    || cell.column < start_column
                    || cell.column >= end_column
            });

            for row in start_row..end_row {
                for column in start_column..end_column {
                    let Some(cell) = grid.get(row, column) else {
                        continue;
                    };
                    append_render_cell(
                        &mut self.cells,
                        row,
                        column,
                        cell,
                        terminal.screen_reverse_video(),
                    );
                }
            }
        }

        self.cells.sort_by_key(|cell| (cell.row, cell.column));
        self.inline_images =
            render_inline_images_from_terminal(terminal, 0, size.rows, size.columns);
        self.cursor = render_cursor_from_terminal(terminal, 0);
    }

    pub fn update_cursor_from_terminal(&mut self, terminal: &Terminal, scrollback_offset: usize) {
        self.cursor = render_cursor_from_terminal(terminal, scrollback_offset);
    }

    #[must_use]
    pub fn with_inverse_overlay(mut self, mut selected: impl FnMut(u16, u16) -> bool) -> Self {
        for cell in &mut self.cells {
            if selected(cell.row, cell.column) {
                cell.inverse = !cell.inverse;
            }
        }

        self
    }

    #[must_use]
    pub fn with_selection_colors_overlay(
        mut self,
        mut selected: impl FnMut(u16, u16) -> bool,
        selection_foreground: Option<Option<Color>>,
        selection_background: Option<Color>,
    ) -> Self {
        if selection_foreground.is_none() && selection_background.is_none() {
            return self.with_inverse_overlay(selected);
        }

        for cell in &mut self.cells {
            if selected(cell.row, cell.column) {
                let inverse_foreground = cell.background;
                let inverse_background = cell.foreground;
                cell.foreground = match selection_foreground {
                    Some(Some(color)) => color,
                    Some(None) => cell.foreground,
                    None => inverse_foreground,
                };
                cell.background = selection_background
                    .map(|background| blend_selection_background(background, cell.background))
                    .unwrap_or(inverse_background);
                cell.inverse = false;
            }
        }

        self
    }
}

fn blend_selection_background(selection_background: Color, cell_background: Color) -> Color {
    let Color::Rgba(red, green, blue, alpha) = selection_background else {
        return selection_background;
    };
    match cell_background {
        Color::Rgb(base_red, base_green, base_blue)
        | Color::Rgba(base_red, base_green, base_blue, _) => {
            let alpha = u16::from(alpha);
            let inverse_alpha = u16::from(u8::MAX).saturating_sub(alpha);
            Color::Rgb(
                blend_channel(red, base_red, alpha, inverse_alpha),
                blend_channel(green, base_green, alpha, inverse_alpha),
                blend_channel(blue, base_blue, alpha, inverse_alpha),
            )
        }
        Color::Default | Color::Indexed(_) => selection_background,
    }
}

fn render_inline_images_from_terminal(
    terminal: &Terminal,
    first_source_row: usize,
    rows: u16,
    columns: u16,
) -> Vec<RenderInlineImage> {
    let last_source_row = first_source_row.saturating_add(usize::from(rows));
    let mut images = terminal
        .inline_images()
        .iter()
        .filter_map(|image| {
            render_inline_image_item(image, first_source_row, last_source_row, columns)
        })
        .collect::<Vec<_>>();
    images.sort_by_key(|image| (image.row, image.column));
    images
}

fn render_inline_image_item(
    image: &ItermInlineImage,
    first_source_row: usize,
    last_source_row: usize,
    columns: u16,
) -> Option<RenderInlineImage> {
    if image.row < first_source_row || image.row >= last_source_row || image.column >= columns {
        return None;
    }

    let row = u16::try_from(image.row - first_source_row).ok()?;
    Some(RenderInlineImage {
        row,
        column: image.column,
        name: image.name.clone(),
        kitty_image_id: image.kitty_image_id,
        kitty_placement_id: image.kitty_placement_id,
        kitty_z_index: image.kitty_z_index,
        size: image.size,
        width: image.width.clone(),
        height: image.height.clone(),
        preserve_aspect_ratio: image.preserve_aspect_ratio,
        image_format: image.image_format,
        pixel_width: image.pixel_width,
        pixel_height: image.pixel_height,
        source_x: image.source_x,
        source_y: image.source_y,
        source_width: image.source_width,
        source_height: image.source_height,
        target_x: image.target_x,
        target_y: image.target_y,
        data: image.data.clone(),
    })
}

fn render_cursor_from_terminal(
    terminal: &Terminal,
    scrollback_offset: usize,
) -> Option<RenderCursor> {
    if !terminal.cursor_visible() || scrollback_offset != 0 {
        return None;
    }

    let (row, column) = terminal.cursor();
    Some(RenderCursor {
        row,
        column,
        shape: terminal.cursor_shape(),
        blinking: terminal.cursor_blinking(),
    })
}

fn cursor_rect(
    shape: CursorShape,
    origin_x: u32,
    origin_y: u32,
    cell_width: u32,
    cell_height: u32,
    cursor_thickness: Option<RenderCursorThickness>,
    window_dpi: u32,
) -> Rect {
    match shape {
        CursorShape::Block => Rect {
            x: origin_x,
            y: origin_y,
            width: cell_width,
            height: cell_height,
        },
        CursorShape::Underline => {
            let height =
                cursor_thickness_px(cursor_thickness, cell_height, cell_height, window_dpi);
            Rect {
                x: origin_x,
                y: origin_y + cell_height.saturating_sub(height),
                width: cell_width,
                height,
            }
        }
        CursorShape::Bar => Rect {
            x: origin_x,
            y: origin_y,
            width: cursor_thickness_px(cursor_thickness, cell_width, cell_height, window_dpi),
            height: cell_height,
        },
    }
}

fn cursor_thickness_px(
    cursor_thickness: Option<RenderCursorThickness>,
    max_thickness: u32,
    cell_height: u32,
    window_dpi: u32,
) -> u32 {
    let default_thickness = (cell_height / 6).max(1);
    let thickness = match cursor_thickness {
        Some(RenderCursorThickness::Pixels(pixels)) => pixels,
        Some(RenderCursorThickness::Points(points)) => points_to_pixels(points, window_dpi),
        Some(RenderCursorThickness::Percent(percent)) => {
            default_thickness.saturating_mul(percent) / 100
        }
        Some(RenderCursorThickness::CellFractionPerMille(per_mille)) => {
            cell_height.saturating_mul(per_mille) / 1_000
        }
        None => default_thickness,
    };

    thickness.max(1).min(max_thickness)
}

fn underline_thickness_px(
    underline_thickness: Option<RenderUnderlineThickness>,
    cell_height: u32,
    window_dpi: u32,
) -> u32 {
    let default_thickness = (cell_height / 8).max(1);
    let thickness = match underline_thickness {
        Some(RenderUnderlineThickness::Pixels(pixels)) => pixels,
        Some(RenderUnderlineThickness::Points(points)) => points_to_pixels(points, window_dpi),
        Some(RenderUnderlineThickness::Percent(percent)) => {
            default_thickness.saturating_mul(percent) / 100
        }
        Some(RenderUnderlineThickness::CellFractionPerMille(per_mille)) => {
            cell_height.saturating_mul(per_mille) / 1_000
        }
        None => default_thickness,
    };

    thickness.max(1).min(cell_height)
}

fn underline_position_px(
    underline_position: Option<RenderUnderlinePosition>,
    cell_height: u32,
    underline_height: u32,
    window_dpi: u32,
) -> u32 {
    let default_position = cell_height.saturating_sub(underline_height);
    let offset = match underline_position {
        Some(RenderUnderlinePosition::Pixels(pixels)) => pixels,
        Some(RenderUnderlinePosition::Points(points)) => {
            signed_points_to_pixels(points, window_dpi)
        }
        Some(RenderUnderlinePosition::Percent(percent)) => {
            signed_scaled(default_position, percent.saturating_sub(100), 100)
        }
        Some(RenderUnderlinePosition::CellFractionPerMille(per_mille)) => {
            signed_scaled(cell_height, per_mille, 1_000)
        }
        None => return default_position,
    };

    shifted_line_position(default_position, offset, cell_height, underline_height)
}

fn strikethrough_position_px(
    strikethrough_position: Option<RenderStrikethroughPosition>,
    cell_height: u32,
    strike_height: u32,
    window_dpi: u32,
) -> u32 {
    let default_position = cell_height
        .saturating_div(2)
        .saturating_sub(strike_height.saturating_div(2));
    let position = match strikethrough_position {
        Some(RenderStrikethroughPosition::Pixels(pixels)) => pixels,
        Some(RenderStrikethroughPosition::Points(points)) => points_to_pixels(points, window_dpi),
        Some(RenderStrikethroughPosition::Percent(percent)) => {
            default_position.saturating_mul(percent) / 100
        }
        Some(RenderStrikethroughPosition::CellFractionPerMille(per_mille)) => {
            cell_height.saturating_mul(per_mille) / 1_000
        }
        None => default_position,
    };

    position.min(cell_height.saturating_sub(strike_height.min(cell_height)))
}

fn shifted_line_position(
    default_position: u32,
    offset: i32,
    cell_height: u32,
    line_height: u32,
) -> u32 {
    let max_position = i64::from(cell_height.saturating_sub(line_height.min(cell_height)));
    let position = i64::from(default_position).saturating_add(i64::from(offset));
    u32::try_from(position.clamp(0, max_position)).unwrap_or(0)
}

fn signed_scaled(value: u32, factor: i32, divisor: i32) -> i32 {
    let scaled = i64::from(value).saturating_mul(i64::from(factor)) / i64::from(divisor.max(1));
    i32::try_from(scaled).unwrap_or(if scaled.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

fn signed_points_to_pixels(points: i32, dpi: u32) -> i32 {
    let numerator = i64::from(points).saturating_mul(i64::from(dpi));
    let rounded = if numerator.is_negative() {
        numerator.saturating_sub(36) / 72
    } else {
        numerator.saturating_add(36) / 72
    };
    i32::try_from(rounded).unwrap_or(if rounded.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

fn points_to_pixels(points: u32, dpi: u32) -> u32 {
    let numerator = u64::from(points).saturating_mul(u64::from(dpi));
    u32::try_from((numerator + 36) / 72).unwrap_or(u32::MAX)
}

fn append_grid_row(
    cells: &mut Vec<RenderCell>,
    grid: &TerminalGrid,
    viewport_row: u16,
    grid_row: usize,
    columns: u16,
    screen_reverse: bool,
) {
    let Ok(grid_row) = u16::try_from(grid_row) else {
        return;
    };

    if grid_row >= grid.size().rows {
        return;
    }

    for column in 0..columns {
        let Some(cell) = grid.get(grid_row, column) else {
            continue;
        };
        append_render_cell(cells, viewport_row, column, cell, screen_reverse);
    }
}

fn append_render_cells(
    cells: &mut Vec<RenderCell>,
    viewport_row: u16,
    source_cells: &[Cell],
    columns: u16,
    screen_reverse: bool,
) {
    for (column, cell) in source_cells.iter().take(usize::from(columns)).enumerate() {
        let Ok(column) = u16::try_from(column) else {
            continue;
        };
        append_render_cell(cells, viewport_row, column, cell, screen_reverse);
    }
}

fn append_render_cell(
    cells: &mut Vec<RenderCell>,
    row: u16,
    column: u16,
    cell: &Cell,
    screen_reverse: bool,
) {
    if !screen_reverse && !cell_has_renderable_content(cell) {
        return;
    }

    let inverse = cell.inverse ^ screen_reverse;
    cells.push(RenderCell {
        row,
        column,
        ch: cell.ch,
        foreground: cell.foreground,
        background: cell.background,
        underline_color: cell.underline_color,
        underline_style: cell.underline_style,
        bold: cell.bold,
        faint: cell.faint,
        italic: cell.italic,
        blink: cell.blink,
        rapid_blink: cell.rapid_blink,
        underline: cell.underline,
        double_underline: cell.double_underline,
        conceal: cell.conceal,
        strikethrough: cell.strikethrough,
        overline: cell.overline,
        vertical_align: cell.vertical_align,
        inverse,
        hyperlink: cell.hyperlink.clone(),
    });
}

fn cell_has_renderable_content(cell: &Cell) -> bool {
    cell.ch != ' '
        || cell.background != Color::Default
        || cell.inverse
        || cell.underline
        || cell.double_underline
        || cell.strikethrough
        || cell.overline
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use rssh_core::TerminalSize;
    use rssh_terminal::{
        Cell, Color, CursorShape, InlineImageFormat, SemanticType, Terminal, TerminalGrid,
        UnderlineStyle, VerticalAlign,
    };

    use super::{
        DamageRegion, PixelRenderer, RenderBoldBrightensAnsiColors, RenderCell, RenderGeometry,
        RenderInlineImage, SCROLLBAR_THUMB_COLOR, SCROLLBAR_TRACK_COLOR, ScrollbackScrollbar,
        TerminalRenderSnapshot,
    };

    #[test]
    fn zero_width_region_is_empty() {
        assert!(DamageRegion::new(0, 0, 0, 1).is_empty());
    }

    #[test]
    fn render_snapshot_contains_non_blank_terminal_cells() {
        let mut grid = TerminalGrid::new(TerminalSize::new(3, 2));
        grid.set(
            1,
            2,
            Cell {
                ch: 'R',
                foreground: Color::Indexed(2),
                background: Color::Rgb(1, 2, 3),
                underline_color: Color::Default,
                underline_style: UnderlineStyle::None,
                bold: true,
                faint: false,
                italic: false,
                blink: false,
                rapid_blink: false,
                underline: true,
                double_underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
                protected: false,
                hyperlink: None,
                semantic_type: SemanticType::Output,
            },
        );

        let snapshot = TerminalRenderSnapshot::from_grid(&grid);

        assert_eq!(snapshot.cells().len(), 1);
        assert_eq!(snapshot.cells()[0].row, 1);
        assert_eq!(snapshot.cells()[0].column, 2);
        assert_eq!(snapshot.cells()[0].ch, 'R');
        assert_eq!(snapshot.cells()[0].foreground, Color::Indexed(2));
        assert_eq!(snapshot.cells()[0].background, Color::Rgb(1, 2, 3));
        assert!(snapshot.cells()[0].bold);
        assert!(snapshot.cells()[0].underline);
        assert!(!snapshot.cells()[0].inverse);
    }

    #[test]
    fn render_snapshot_reports_missing_glyph_codepoints_once() {
        let mut grid = TerminalGrid::new(TerminalSize::new(3, 1));
        for (column, ch) in [(0, 'R'), (1, '中'), (2, '中')] {
            grid.set(
                0,
                column,
                Cell {
                    ch,
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
                    vertical_align: VerticalAlign::Baseline,
                    inverse: false,
                    protected: false,
                    hyperlink: None,
                    semantic_type: SemanticType::Output,
                },
            );
        }

        let snapshot = TerminalRenderSnapshot::from_grid(&grid);

        assert_eq!(snapshot.missing_glyphs(), vec!['中']);
    }

    #[test]
    fn render_snapshot_preserves_inverse_style() {
        let mut grid = TerminalGrid::new(TerminalSize::new(1, 1));
        grid.set(
            0,
            0,
            Cell {
                ch: 'I',
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
                vertical_align: VerticalAlign::Baseline,
                inverse: true,
                protected: false,
                hyperlink: None,
                semantic_type: SemanticType::Output,
            },
        );

        let snapshot = TerminalRenderSnapshot::from_grid(&grid);

        assert!(snapshot.cells()[0].inverse);
    }

    #[test]
    fn render_snapshot_applies_screen_reverse_video_mode() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));

        terminal.feed(b"A\x1b[?5hB");
        let reversed = TerminalRenderSnapshot::from_terminal(&terminal);

        assert_eq!(snapshot_char(&reversed, 0, 0), Some('A'));
        assert_eq!(snapshot_char(&reversed, 0, 1), Some('B'));
        assert!(reversed.cells().iter().all(|cell| cell.inverse));
        assert_eq!(
            reversed.cells().len(),
            4,
            "reverse video should render the full visible screen"
        );

        terminal.feed(b"\x1b[?5lC");
        let normal = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(normal.cells().iter().all(|cell| !cell.inverse));
        assert!(normal.cells().len() < 4);
    }

    #[test]
    fn render_snapshot_preserves_strikethrough_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[9mS");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].strikethrough);
    }

    #[test]
    fn render_snapshot_preserves_faint_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[2mF");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].faint);
    }

    #[test]
    fn render_snapshot_preserves_conceal_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[8mC");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].conceal);
    }

    #[test]
    fn render_snapshot_preserves_overline_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[53mO");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].overline);
    }

    #[test]
    fn render_snapshot_preserves_blink_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[5mB");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].blink);
    }

    #[test]
    fn render_snapshot_preserves_double_underline_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[21mD");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cells()[0].double_underline);
    }

    #[test]
    fn render_snapshot_preserves_underline_color() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[4;58;2;1;2;3mU");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert_eq!(snapshot.cells()[0].underline_color, Color::Rgb(1, 2, 3));
    }

    #[test]
    fn render_snapshot_preserves_colon_separated_underline_style() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[4:4mD");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert_eq!(
            snapshot.cells()[0].underline_style,
            rssh_terminal::UnderlineStyle::Dotted
        );
    }

    #[test]
    fn render_snapshot_preserves_hyperlink_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));
        terminal.feed(b"\x1b]8;;https://example.com\x1b\\ab\x1b]8;;\x1b\\");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert_eq!(
            snapshot.cells()[0].hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            snapshot.cells()[1].hyperlink.as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn render_snapshot_preserves_iterm_inline_image_metadata() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));
        terminal.feed(
            b"ab\x1b]1337;File=inline=1;name=aW1nLnBuZw==;size=4;width=3;height=2;preserveAspectRatio=0:QUJDRA==\x07cd",
        );

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert_eq!(
            snapshot.inline_images(),
            &[RenderInlineImage {
                row: 0,
                column: 2,
                name: Some("img.png".to_owned()),
                kitty_image_id: None,
                kitty_placement_id: None,
                kitty_z_index: None,
                size: Some(4),
                width: Some("3".to_owned()),
                height: Some("2".to_owned()),
                preserve_aspect_ratio: Some(false),
                image_format: InlineImageFormat::Encoded,
                pixel_width: None,
                pixel_height: None,
                source_x: None,
                source_y: None,
                source_width: None,
                source_height: None,
                target_x: None,
                target_y: None,
                data: b"ABCD".to_vec(),
            }]
        );
    }

    #[test]
    fn render_snapshot_places_inline_images_after_scrollback_exists() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));
        terminal.feed(b"one\r\ntwo\r\n");
        terminal.feed(b"\x1b]1337;File=inline=1:QQ==\x07");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert_eq!(snapshot.inline_images().len(), 1);
        assert_eq!(snapshot.inline_images()[0].row, 1);
        assert_eq!(snapshot.inline_images()[0].column, 0);
    }

    #[test]
    fn render_snapshot_can_view_inline_images_in_scrollback() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));
        terminal.feed(b"\x1b]1337;File=inline=1:QQ==\x07one\r\ntwo\r\n");

        let live_snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let scrolled_snapshot = TerminalRenderSnapshot::from_terminal_viewport(&terminal, 1);

        assert!(live_snapshot.inline_images().is_empty());
        assert_eq!(scrolled_snapshot.inline_images().len(), 1);
        assert_eq!(scrolled_snapshot.inline_images()[0].row, 0);
    }

    #[test]
    fn render_snapshot_can_apply_inverse_overlay() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));
        terminal.feed(b"abc");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal)
            .with_inverse_overlay(|row, column| row == 0 && column == 1);

        assert!(!snapshot.cells()[0].inverse);
        assert!(snapshot.cells()[1].inverse);
        assert!(!snapshot.cells()[2].inverse);
    }

    #[test]
    fn render_snapshot_blends_selection_background_alpha_over_cell_background() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[48;2;10;20;30mA");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal)
            .with_selection_colors_overlay(
                |row, column| row == 0 && column == 0,
                Some(None),
                Some(Color::Rgba(110, 120, 130, 127)),
            );

        assert_eq!(snapshot.cells()[0].background, Color::Rgb(59, 69, 79));
    }

    #[test]
    fn render_snapshot_can_offset_rows_and_overlay_cells() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));
        terminal.feed(b"abc");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal)
            .with_row_offset(1)
            .with_overlay_cells([RenderCell {
                row: 0,
                column: 0,
                ch: 'T',
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
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
                hyperlink: None,
            }])
            .with_overlay_cells([RenderCell {
                row: 1,
                column: 0,
                ch: 'O',
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
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
                hyperlink: None,
            }]);

        assert_eq!(snapshot_char(&snapshot, 0, 0), Some('T'));
        assert_eq!(snapshot_char(&snapshot, 1, 0), Some('O'));
        assert_eq!(snapshot_char(&snapshot, 1, 2), Some('c'));
    }

    #[test]
    fn render_snapshot_can_overlay_another_snapshot_with_inline_images() {
        let mut base_terminal = Terminal::new(TerminalSize::new(4, 1));
        base_terminal.feed(b"base");
        let mut overlay_terminal = Terminal::new(TerminalSize::new(4, 1));
        overlay_terminal.feed(b"\x1b]1337;File=inline=1:QQ==\x07");

        let snapshot = TerminalRenderSnapshot::from_terminal(&base_terminal).with_overlay_snapshot(
            TerminalRenderSnapshot::from_terminal(&overlay_terminal).with_viewport(2, 3, 1, 4),
        );

        assert_eq!(snapshot.inline_images().len(), 1);
        assert_eq!(snapshot.inline_images()[0].row, 2);
        assert_eq!(snapshot.inline_images()[0].column, 3);
    }

    #[test]
    fn render_snapshot_can_clip_and_position_viewport() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"abcd\r\nefgh");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal).with_viewport(3, 5, 1, 2);

        assert_eq!(snapshot.cells().len(), 2);
        assert_eq!(snapshot_char(&snapshot, 3, 5), Some('a'));
        assert_eq!(snapshot_char(&snapshot, 3, 6), Some('b'));
        assert_eq!(snapshot_char(&snapshot, 4, 5), None);
        assert_eq!(snapshot_char(&snapshot, 3, 7), None);
    }

    #[test]
    fn render_snapshot_updates_cells_from_damage_regions() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));
        terminal.feed(b"abc");
        terminal.take_damage();
        let mut snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        terminal.feed(b"\rZ");
        let damage = terminal.take_damage();

        snapshot.update_from_terminal_damage(&terminal, &damage);

        assert_eq!(snapshot_char(&snapshot, 0, 0), Some('Z'));
        assert_eq!(snapshot_char(&snapshot, 0, 1), Some('b'));
        assert_eq!(snapshot_char(&snapshot, 0, 2), Some('c'));
    }

    #[test]
    fn render_snapshot_removes_cells_cleared_by_damage_regions() {
        let mut terminal = Terminal::new(TerminalSize::new(3, 1));
        terminal.feed(b"abc");
        terminal.take_damage();
        let mut snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        terminal.feed(b"\r ");
        let damage = terminal.take_damage();

        snapshot.update_from_terminal_damage(&terminal, &damage);

        assert_eq!(snapshot_char(&snapshot, 0, 0), None);
        assert_eq!(snapshot_char(&snapshot, 0, 1), Some('b'));
        assert_eq!(snapshot_char(&snapshot, 0, 2), Some('c'));
    }

    #[test]
    fn pixel_renderer_draws_glyph_foreground_pixels() {
        let mut grid = TerminalGrid::new(TerminalSize::new(1, 1));
        grid.set(
            0,
            0,
            Cell {
                ch: 'A',
                foreground: Color::Rgb(255, 0, 0),
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
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
                protected: false,
                hyperlink: None,
                semantic_type: SemanticType::Output,
            },
        );
        let snapshot = TerminalRenderSnapshot::from_grid(&grid);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [255, 0, 0, 255]),
            "renderer did not draw a red glyph pixel"
        );
    }

    #[test]
    fn pixel_renderer_draws_iterm_inline_png_image_payload() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_inline_png(&mut terminal, "width=1;height=1");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_iterm_inline_jpeg_image_payload() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_inline_jpeg(&mut terminal, "width=1;height=1");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [254, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [254, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_iterm_inline_gif_first_frame() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_inline_gif(&mut terminal, "width=1;height=1");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_iterm_inline_gif_animation_frame() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_green_inline_gif(&mut terminal, "width=1;height=1");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::with_animation_frame(1);
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [0, 255, 0, 255]);

        let renderer = PixelRenderer::with_animation_elapsed_ms(250);
        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_selects_iterm_inline_gif_frame_by_elapsed_time() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_green_inline_gif(&mut terminal, "width=1;height=1");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::with_animation_elapsed_ms(150);
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [0, 255, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_kitty_rgb_direct_inline_image() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_kitty_rgb_image(&mut terminal);
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_compressed_kitty_rgb_direct_inline_image() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_compressed_red_kitty_rgb_image(&mut terminal);
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_kitty_rgb_simple_file_transfer() {
        let file = KittyTestFile::new(&[255, 0, 0]);
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_kitty_rgb_file_image(&mut terminal, &file.path, "");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_kitty_rgb_simple_file_transfer_slice() {
        let file = KittyTestFile::new(&[0, 0, 255, 255, 0, 0]);
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_kitty_rgb_file_image(&mut terminal, &file.path, ",O=3,S=3");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_kitty_rgb_temporary_file_transfer_and_deletes_safe_temp_file() {
        let file = KittyTestFile::new_with_prefix("tty-graphics-protocol-rssh", &[255, 0, 0]);
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_kitty_rgb_temporary_file_image(&mut terminal, &file.path, "");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
        assert!(
            !file.path.exists(),
            "safe kitty temporary file should be deleted after reading"
        );
    }

    #[test]
    fn pixel_renderer_preserves_kitty_temporary_file_without_safe_name() {
        let file = KittyTestFile::new(&[255, 0, 0]);
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_kitty_rgb_temporary_file_image(&mut terminal, &file.path, "");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
        assert!(
            file.path.exists(),
            "unsafe kitty temporary file name should not be deleted"
        );
    }

    #[test]
    fn pixel_renderer_draws_chunked_kitty_rgb_direct_inline_image() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_chunked_red_green_kitty_rgb_image(&mut terminal);
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_basic_sixel_image() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25l");
        terminal.feed(b"\x1bPq\"1;1;1;6#1;2;100;0;0#1~\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 0, 5), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 1, 0), [12, 12, 12, 255]);
        assert_eq!(pixel_at(&target, 8, 0, 6), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_sixel_repeat_and_newline() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 2));
        terminal.feed(b"\x1b[?25l");
        terminal.feed(b"\x1bPq\"1;1;2;12#1;2;100;0;0#1!2@-!2@\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 16 * 4];

        renderer.render(&snapshot, &mut target, 8, 16, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 1, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 0, 6), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 1, 6), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 2, 0), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_sixel_hls_color_definition() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25l");
        terminal.feed(b"\x1bPq#1;1;240;50;100#1~\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 0, 5), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 1, 0), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_kitty_horizontal_source_rectangle() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=2,v=1,c=1,r=1,x=1,w=1;/wAAAP8A\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [0, 255, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_kitty_vertical_source_rectangle() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=2,c=1,r=1,y=1,h=1;/wAAAP8A\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [0, 255, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_kitty_target_pixel_offset() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 2));
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1,X=2,Y=3;/wAA\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 16 * 4];

        renderer.render(&snapshot, &mut target, 16, 16, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 0), [12, 12, 12, 255]);
        assert_eq!(pixel_at(&target, 16, 8, 3), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 2, 10), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 1, 3), [12, 12, 12, 255]);
        assert_eq!(pixel_at(&target, 16, 2, 2), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_stored_kitty_source_rectangle() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=2,v=1,c=1,r=1;/wAAAP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,x=1,w=1\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [0, 255, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_stored_kitty_target_pixel_offset() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 2));
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,X=2,Y=3\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 16 * 4];

        renderer.render(&snapshot, &mut target, 16, 16, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 0), [12, 12, 12, 255]);
        assert_eq!(pixel_at(&target, 16, 8, 3), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 2, 10), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 1, 3), [12, 12, 12, 255]);
        assert_eq!(pixel_at(&target, 16, 2, 2), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_stored_kitty_rgb_direct_inline_image() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_stored_red_kitty_rgb_image(&mut terminal);
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_stacks_kitty_images_by_z_index() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_overlapping_kitty_rgb_images(&mut terminal, 5, 1);
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_uses_kitty_image_id_as_same_z_index_tiebreaker() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_overlapping_kitty_rgb_images_high_id_first(&mut terminal);
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [0, 255, 0, 255]);
    }

    #[test]
    fn pixel_renderer_places_negative_z_kitty_images_below_text() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25lA\x1b[1;1H");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,z=-1\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 3, 0), [229, 229, 229, 255]);
    }

    #[test]
    fn pixel_renderer_places_extreme_negative_z_kitty_images_below_non_default_backgrounds() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25l\x1b[48;2;0;0;255mA\x1b[0m\x1b[1;1H");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,z=-1073741825\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel_at(&target, 8, 3, 0), [229, 229, 229, 255]);
    }

    #[test]
    fn pixel_renderer_places_extreme_negative_z_kitty_images_below_non_default_space_backgrounds() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25l\x1b[48;2;0;0;255m \x1b[0m\x1b[1;1H");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7,z=-1073741825\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [0, 0, 255, 255]);
    }

    #[test]
    fn pixel_renderer_omits_deleted_kitty_placements() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_stored_red_kitty_rgb_image(&mut terminal);
        terminal.feed(b"\x1b_Ga=d\x1b\\");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [12, 12, 12, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_inline_image_from_damage_region() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_inline_png(&mut terminal, "width=1;height=1");
        let damage = terminal.take_damage();
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render_damage(
            &snapshot,
            &damage,
            &mut target,
            RenderGeometry::new(8, 8, 8, 8),
        );

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_redraws_inline_image_when_damage_hits_covered_cell() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        feed_red_inline_png(&mut terminal, "width=2;height=1");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render_damage(
            &snapshot,
            &[DamageRegion::new(1, 0, 1, 1)],
            &mut target,
            RenderGeometry::new(16, 8, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 8, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 15, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_respects_inline_image_pixel_dimensions() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_inline_png(&mut terminal, "width=4px;height=2px");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 3, 1), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 4, 1), [12, 12, 12, 255]);
        assert_eq!(pixel_at(&target, 8, 3, 2), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_updates_only_damage_regions() {
        let mut grid = TerminalGrid::new(TerminalSize::new(2, 1));
        grid.set(
            0,
            0,
            Cell {
                ch: 'A',
                foreground: Color::Default,
                background: Color::Rgb(20, 0, 0),
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
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
                protected: false,
                hyperlink: None,
                semantic_type: SemanticType::Output,
            },
        );
        grid.set(
            0,
            1,
            Cell {
                ch: 'B',
                foreground: Color::Default,
                background: Color::Rgb(0, 20, 0),
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
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
                protected: false,
                hyperlink: None,
                semantic_type: SemanticType::Output,
            },
        );
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(
            &TerminalRenderSnapshot::from_grid(&grid),
            &mut target,
            16,
            8,
            8,
            8,
        );
        let untouched_second_cell = pixel_at(&target, 16, 8, 0);

        grid.set(
            0,
            0,
            Cell {
                ch: 'Z',
                foreground: Color::Rgb(0, 0, 20),
                background: Color::Rgb(0, 0, 20),
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
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
                protected: false,
                hyperlink: None,
                semantic_type: SemanticType::Output,
            },
        );

        renderer.render_damage(
            &TerminalRenderSnapshot::from_grid(&grid),
            &[DamageRegion::new(0, 0, 1, 1)],
            &mut target,
            RenderGeometry::new(16, 8, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 0, 0), [0, 0, 20, 255]);
        assert_eq!(pixel_at(&target, 16, 8, 0), untouched_second_cell);
    }

    #[test]
    fn pixel_renderer_draws_scrollback_scrollbar_at_bottom_for_live_viewport() {
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 32 * 4];

        renderer.render_scrollbar(
            ScrollbackScrollbar::new(3, 1, 0).unwrap(),
            &mut target,
            RenderGeometry::new(16, 32, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 15, 0), SCROLLBAR_TRACK_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 31), SCROLLBAR_THUMB_COLOR);
    }

    #[test]
    fn pixel_renderer_moves_scrollback_scrollbar_thumb_up_for_history_viewport() {
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 32 * 4];

        renderer.render_scrollbar(
            ScrollbackScrollbar::new(3, 1, 3).unwrap(),
            &mut target,
            RenderGeometry::new(16, 32, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 15, 0), SCROLLBAR_THUMB_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 31), SCROLLBAR_TRACK_COLOR);
    }

    #[test]
    fn pixel_renderer_uses_half_cell_default_minimum_scrollbar_thumb_height() {
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 32 * 4];

        renderer.render_scrollbar(
            ScrollbackScrollbar::new(1_000, 1, 0).unwrap(),
            &mut target,
            RenderGeometry::new(16, 32, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 15, 27), SCROLLBAR_TRACK_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 28), SCROLLBAR_THUMB_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 31), SCROLLBAR_THUMB_COLOR);
    }

    #[test]
    fn pixel_renderer_applies_percent_minimum_scrollbar_thumb_height() {
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 32 * 4];

        renderer.render_scrollbar(
            ScrollbackScrollbar::new(1_000, 1, 0)
                .unwrap()
                .with_min_thumb_height_percent(50),
            &mut target,
            RenderGeometry::new(16, 32, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 15, 15), SCROLLBAR_TRACK_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 16), SCROLLBAR_THUMB_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 31), SCROLLBAR_THUMB_COLOR);
    }

    #[test]
    fn pixel_renderer_scales_point_minimum_scrollbar_thumb_height_by_window_dpi() {
        let mut renderer = PixelRenderer::new();
        renderer.set_window_dpi(144);
        let mut target = vec![0; 16 * 32 * 4];

        renderer.render_scrollbar(
            ScrollbackScrollbar::new(1_000, 1, 0)
                .unwrap()
                .with_min_thumb_height_points(3),
            &mut target,
            RenderGeometry::new(16, 32, 8, 8),
        );

        assert_eq!(pixel_at(&target, 16, 15, 25), SCROLLBAR_TRACK_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 26), SCROLLBAR_THUMB_COLOR);
        assert_eq!(pixel_at(&target, 16, 15, 31), SCROLLBAR_THUMB_COLOR);
    }

    #[test]
    fn scrollback_scrollbar_maps_pixel_y_to_viewport_offset() {
        let geometry = RenderGeometry::new(8, 100, 1, 1);
        let scrollbar = ScrollbackScrollbar::new(10, 10, 0).unwrap();

        assert_eq!(scrollbar.offset_from_pixel_y(0, geometry), 10);
        assert_eq!(scrollbar.offset_from_pixel_y(99, geometry), 0);
    }

    #[test]
    fn indexed_color_maps_xterm_256_color_palette() {
        assert_eq!(
            super::color_to_rgba(Color::Indexed(16), [1, 2, 3, 255]),
            [0, 0, 0, 255]
        );
        assert_eq!(
            super::color_to_rgba(Color::Indexed(196), [1, 2, 3, 255]),
            [255, 0, 0, 255]
        );
        assert_eq!(
            super::color_to_rgba(Color::Indexed(232), [1, 2, 3, 255]),
            [8, 8, 8, 255]
        );
        assert_eq!(
            super::color_to_rgba(Color::Indexed(255), [1, 2, 3, 255]),
            [238, 238, 238, 255]
        );
    }

    #[test]
    fn color_to_rgba_preserves_terminal_rgba_alpha() {
        assert_eq!(
            super::color_to_rgba(Color::Rgba(1, 2, 3, 4), [9, 9, 9, 255]),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn pixel_renderer_draws_xterm_256_color_from_terminal_output() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[38;5;196mR");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert_eq!(snapshot.cells()[0].foreground, Color::Indexed(196));
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [255, 0, 0, 255]),
            "renderer did not draw xterm indexed red"
        );
    }

    #[test]
    fn pixel_renderer_draws_underlined_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[4;38;2;255;0;0mA");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].underline);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_underlines_with_underline_color() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[4;38;2;255;0;0;58;2;0;255;0mA");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert_eq!(snapshot.cells()[0].underline_color, Color::Rgb(0, 255, 0));
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [0, 255, 0, 255]);
        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [255, 0, 0, 255]),
            "glyph foreground should still use the foreground color"
        );
    }

    #[test]
    fn pixel_renderer_applies_underline_thickness_override() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[4;38;2;255;0;0m ");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].underline);
        let renderer = PixelRenderer::with_underline_thickness_px(3);
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 5), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 5), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 0, 4), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_applies_underline_position_override() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[4;38;2;255;0;0m ");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].underline);
        let renderer = PixelRenderer::with_underline_position_px(-3);
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 4), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 4), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 0, 7), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_applies_strikethrough_position_override() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[9;38;2;255;0;0m ");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].strikethrough);
        let renderer = PixelRenderer::with_strikethrough_position_cell_fraction_per_mille(250);
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 2), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 2), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 0, 4), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_dotted_underlines_with_gaps() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[4:4;58;2;0;255;0mA");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert_eq!(
            snapshot.cells()[0].underline_style,
            rssh_terminal::UnderlineStyle::Dotted
        );
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [0, 255, 0, 255]);
        assert_ne!(pixel_at(&target, 16, 1, 7), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 2, 7), [0, 255, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_double_underlined_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[21;38;2;255;0;0mA");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].double_underline);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 5), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 5), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 0, 7), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_draws_strikethrough_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[9;38;2;255;0;0m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].strikethrough);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 4), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 4), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_dims_faint_foreground_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[2;4;38;2;200;100;50m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].faint);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [100, 50, 25, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [100, 50, 25, 255]);
    }

    #[test]
    fn pixel_renderer_hides_concealed_foreground_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[8;4;38;2;255;0;0;48;2;3;4;5m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].conceal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [3, 4, 5, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [3, 4, 5, 255]);
        assert_eq!(count_pixels(&target, [255, 0, 0, 255]), 0);
    }

    #[test]
    fn pixel_renderer_draws_overlined_text() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[53;38;2;255;0;0m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].overline);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_hides_blinking_foreground_when_phase_is_hidden() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[5;4;38;2;255;0;0;48;2;3;4;5m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].blink);
        let renderer = PixelRenderer::with_blink_visible(false);
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(pixel_at(&target, 16, 0, 7), [3, 4, 5, 255]);
        assert_eq!(pixel_at(&target, 16, 7, 7), [3, 4, 5, 255]);
        assert_eq!(count_pixels(&target, [255, 0, 0, 255]), 0);
    }

    #[test]
    fn pixel_renderer_fades_blinking_foreground_toward_background() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[5;38;2;255;0;0;48;2;3;4;5m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].blink);
        let renderer = PixelRenderer::with_text_blink_opacity(0.5);
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert!(count_pixels(&target, [128, 2, 2, 255]) > 0);
        assert_eq!(count_pixels(&target, [255, 0, 0, 255]), 0);
    }

    #[test]
    fn pixel_renderer_uses_rapid_text_blink_opacity_for_sgr6_cells() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[6;38;2;255;0;0;48;2;3;4;5m.");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].blink);
        assert!(snapshot.cells()[0].rapid_blink);
        let renderer = PixelRenderer::with_rapid_text_blink_opacity(0.0);
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert_eq!(count_pixels(&target, [255, 0, 0, 255]), 0);
        assert_eq!(count_pixels(&target, [128, 2, 2, 255]), 0);
    }

    #[test]
    fn pixel_renderer_draws_bold_text_with_more_foreground_pixels() {
        let renderer = PixelRenderer::new();
        let mut normal = Terminal::new(TerminalSize::new(2, 1));
        normal.feed(b"\x1b[38;2;255;0;0mA");
        let normal_snapshot = TerminalRenderSnapshot::from_terminal(&normal);
        let mut normal_target = vec![0; 16 * 8 * 4];

        renderer.render(&normal_snapshot, &mut normal_target, 16, 8, 8, 8);

        let mut bold = Terminal::new(TerminalSize::new(2, 1));
        bold.feed(b"\x1b[1;38;2;255;0;0mA");
        let bold_snapshot = TerminalRenderSnapshot::from_terminal(&bold);
        assert!(bold_snapshot.cells()[0].bold);
        let mut bold_target = vec![0; 16 * 8 * 4];

        renderer.render(&bold_snapshot, &mut bold_target, 16, 8, 8, 8);

        assert!(
            count_pixels(&bold_target, [255, 0, 0, 255])
                > count_pixels(&normal_target, [255, 0, 0, 255])
        );
    }

    #[test]
    fn pixel_renderer_brightens_bold_ansi_foreground_by_default() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[1;31mA");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert!(snapshot.cells()[0].bold);
        assert_eq!(snapshot.cells()[0].foreground, Color::Indexed(1));
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert!(count_pixels(&target, [241, 76, 76, 255]) > 0);
        assert_eq!(count_pixels(&target, [205, 49, 49, 255]), 0);
    }

    #[test]
    fn pixel_renderer_can_disable_bold_ansi_brightening() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[1;31mA");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer =
            PixelRenderer::with_bold_brightens_ansi_colors(RenderBoldBrightensAnsiColors::No);
        let mut target = vec![0; 16 * 8 * 4];

        renderer.render(&snapshot, &mut target, 16, 8, 8, 8);

        assert!(count_pixels(&target, [205, 49, 49, 255]) > 0);
        assert_eq!(count_pixels(&target, [241, 76, 76, 255]), 0);
    }

    #[test]
    fn pixel_renderer_bright_only_ansi_bold_uses_bright_color_without_bold_weight() {
        let renderer = PixelRenderer::with_bold_brightens_ansi_colors(
            RenderBoldBrightensAnsiColors::BrightOnly,
        );
        let mut normal = Terminal::new(TerminalSize::new(2, 1));
        normal.feed(b"\x1b[91mA");
        let normal_snapshot = TerminalRenderSnapshot::from_terminal(&normal);
        let mut normal_target = vec![0; 16 * 8 * 4];

        renderer.render(&normal_snapshot, &mut normal_target, 16, 8, 8, 8);

        let mut bold = Terminal::new(TerminalSize::new(2, 1));
        bold.feed(b"\x1b[1;31mA");
        let bold_snapshot = TerminalRenderSnapshot::from_terminal(&bold);
        assert!(bold_snapshot.cells()[0].bold);
        let mut bold_target = vec![0; 16 * 8 * 4];

        renderer.render(&bold_snapshot, &mut bold_target, 16, 8, 8, 8);

        let normal_bright_pixels = count_pixels(&normal_target, [241, 76, 76, 255]);
        assert!(normal_bright_pixels > 0);
        assert_eq!(
            count_pixels(&bold_target, [241, 76, 76, 255]),
            normal_bright_pixels
        );
        assert_eq!(count_pixels(&bold_target, [205, 49, 49, 255]), 0);
    }

    #[test]
    fn pixel_renderer_slants_italic_text() {
        let renderer = PixelRenderer::new();
        let mut normal = Terminal::new(TerminalSize::new(2, 1));
        normal.feed(b"\x1b[38;2;255;0;0mI");
        let normal_snapshot = TerminalRenderSnapshot::from_terminal(&normal);
        let mut normal_target = vec![0; 16 * 8 * 4];

        renderer.render(&normal_snapshot, &mut normal_target, 16, 8, 8, 8);

        let mut italic = Terminal::new(TerminalSize::new(2, 1));
        italic.feed(b"\x1b[3;38;2;255;0;0mI");
        let italic_snapshot = TerminalRenderSnapshot::from_terminal(&italic);
        assert!(italic_snapshot.cells()[0].italic);
        let mut italic_target = vec![0; 16 * 8 * 4];

        renderer.render(&italic_snapshot, &mut italic_target, 16, 8, 8, 8);

        assert_ne!(italic_target, normal_target);
        assert_eq!(
            count_pixels(&italic_target, [255, 0, 0, 255]),
            count_pixels(&normal_target, [255, 0, 0, 255])
        );
    }

    #[test]
    fn pixel_renderer_offsets_subscript_text_baseline() {
        let renderer = PixelRenderer::new();
        let mut baseline = Terminal::new(TerminalSize::new(2, 1));
        baseline.feed(b"\x1b[38;2;255;0;0mA");
        let baseline_snapshot = TerminalRenderSnapshot::from_terminal(&baseline);
        assert_eq!(
            baseline_snapshot.cells()[0].vertical_align,
            rssh_terminal::VerticalAlign::Baseline
        );
        let mut baseline_target = vec![0; 16 * 16 * 4];

        renderer.render(&baseline_snapshot, &mut baseline_target, 16, 16, 8, 16);

        let mut subscript = Terminal::new(TerminalSize::new(2, 1));
        subscript.feed(b"\x1b[74;38;2;255;0;0mA");
        let subscript_snapshot = TerminalRenderSnapshot::from_terminal(&subscript);
        assert_eq!(
            subscript_snapshot.cells()[0].vertical_align,
            rssh_terminal::VerticalAlign::Subscript
        );
        let mut subscript_target = vec![0; 16 * 16 * 4];

        renderer.render(&subscript_snapshot, &mut subscript_target, 16, 16, 8, 16);

        assert!(
            first_pixel_y(&subscript_target, 16, [255, 0, 0, 255])
                > first_pixel_y(&baseline_target, 16, [255, 0, 0, 255])
        );
    }

    #[test]
    fn pixel_renderer_swaps_foreground_and_background_for_inverse_cells() {
        let mut grid = TerminalGrid::new(TerminalSize::new(1, 1));
        grid.set(
            0,
            0,
            Cell {
                ch: 'A',
                foreground: Color::Rgb(255, 0, 0),
                background: Color::Rgb(0, 0, 255),
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
                vertical_align: VerticalAlign::Baseline,
                inverse: true,
                protected: false,
                hyperlink: None,
                semantic_type: SemanticType::Output,
            },
        );
        let snapshot = TerminalRenderSnapshot::from_grid(&grid);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [255, 0, 0, 255]),
            "renderer did not use the original foreground as inverse background"
        );
        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [0, 0, 255, 255]),
            "renderer did not use the original background as inverse foreground"
        );
    }

    #[test]
    fn render_snapshot_exposes_terminal_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"ab\r\nc");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        let cursor = snapshot.cursor().expect("cursor should be visible");
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.column, 1);
        assert!(!cursor.blinking);
    }

    #[test]
    fn render_snapshot_marks_blinking_terminal_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?12h");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(
            snapshot
                .cursor()
                .expect("cursor should be visible")
                .blinking
        );
    }

    #[test]
    fn pixel_renderer_hides_blinking_cursor_when_phase_is_hidden() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?12h");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::with_blink_visible(false);
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert!(
            !target
                .chunks_exact(4)
                .any(|pixel| pixel == [229, 229, 229, 255]),
            "renderer drew a cursor during the hidden blink phase"
        );
    }

    #[test]
    fn pixel_renderer_applies_blinking_cursor_opacity() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?12h");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::with_cursor_opacity(0.5);
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(&target[0..4], &[120, 120, 120, 255]);
    }

    #[test]
    fn pixel_renderer_cursor_opacity_preserves_animation_frame() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        feed_red_green_inline_gif(&mut terminal, "width=1;height=1");
        terminal.feed(b"\r\x1b[?25h\x1b[?12h");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let mut renderer = PixelRenderer::with_animation_frame(1);
        renderer.set_cursor_opacity(0.5);
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(&target[0..4], &[114, 242, 114, 255]);
    }

    #[test]
    fn render_snapshot_can_show_scrollback_viewport() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"ab\r\ncd\r\nef");

        let snapshot = TerminalRenderSnapshot::from_terminal_viewport(&terminal, 1);

        assert_eq!(
            snapshot
                .cells()
                .iter()
                .map(|cell| (cell.row, cell.column, cell.ch))
                .collect::<Vec<_>>(),
            vec![(0, 0, 'a'), (0, 1, 'b'), (1, 0, 'c'), (1, 1, 'd')]
        );
        assert!(snapshot.cursor().is_none());
    }

    #[test]
    fn render_snapshot_omits_hidden_terminal_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"\x1b[?25l");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert!(snapshot.cursor().is_none());
    }

    #[test]
    fn pixel_renderer_draws_blank_cursor_cell() {
        let terminal = Terminal::new(TerminalSize::new(1, 1));
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert!(
            target
                .chunks_exact(4)
                .any(|pixel| pixel == [229, 229, 229, 255]),
            "renderer did not draw a visible cursor block"
        );
    }

    #[test]
    fn pixel_renderer_draws_configured_block_cursor_border() {
        let terminal = Terminal::new(TerminalSize::new(1, 1));
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let mut renderer = PixelRenderer::new();
        renderer.set_default_cursor_color([7, 8, 9, 255]);
        renderer.set_default_cursor_border(Some([1, 2, 3, 255]));
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [1, 2, 3, 255]);
        assert_eq!(pixel_at(&target, 8, 1, 1), [7, 8, 9, 255]);
    }

    #[test]
    fn pixel_renderer_draws_bar_cursor_shape() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[6 q");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert_eq!(snapshot.cursor().unwrap().shape, CursorShape::Bar);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 0), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_draws_underline_cursor_shape() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[4 q");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        assert_eq!(snapshot.cursor().unwrap().shape, CursorShape::Underline);
        let renderer = PixelRenderer::new();
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 7), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 0, 0), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_applies_cursor_thickness_override_to_line_cursors() {
        let mut bar_terminal = Terminal::new(TerminalSize::new(1, 1));
        bar_terminal.feed(b"\x1b[6 q");
        let bar_snapshot = TerminalRenderSnapshot::from_terminal(&bar_terminal);
        let renderer = PixelRenderer::with_cursor_thickness_px(3);
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&bar_snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 2, 0), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 3, 0), [12, 12, 12, 255]);

        let mut underline_terminal = Terminal::new(TerminalSize::new(1, 1));
        underline_terminal.feed(b"\x1b[4 q");
        let underline_snapshot = TerminalRenderSnapshot::from_terminal(&underline_terminal);
        target.fill(0);

        renderer.render(&underline_snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 5), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 0, 4), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_applies_cursor_thickness_percent_to_line_cursors() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[4 q");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::with_cursor_thickness_percent(200);
        let mut target = vec![0; 8 * 12 * 4];

        renderer.render(&snapshot, &mut target, 8, 12, 8, 12);

        assert_eq!(pixel_at(&target, 8, 0, 8), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 0, 7), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_applies_cursor_thickness_cell_fraction_to_line_cursors() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[6 q");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::with_cursor_thickness_cell_fraction_per_mille(250);
        let mut target = vec![0; 8 * 12 * 4];

        renderer.render(&snapshot, &mut target, 8, 12, 8, 12);

        assert_eq!(pixel_at(&target, 8, 2, 0), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 3, 0), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_applies_cursor_thickness_points_to_line_cursors() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[6 q");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::with_cursor_thickness_points(2);
        let mut target = vec![0; 8 * 12 * 4];

        renderer.render(&snapshot, &mut target, 8, 12, 8, 12);

        assert_eq!(pixel_at(&target, 8, 2, 0), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 3, 0), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_scales_cursor_thickness_points_by_window_dpi() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[6 q");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let mut renderer = PixelRenderer::with_cursor_thickness_points(2);
        renderer.set_window_dpi(144);
        let mut target = vec![0; 8 * 12 * 4];

        renderer.render(&snapshot, &mut target, 8, 12, 8, 12);

        assert_eq!(pixel_at(&target, 8, 3, 0), [229, 229, 229, 255]);
        assert_eq!(pixel_at(&target, 8, 4, 0), [12, 12, 12, 255]);
    }

    #[test]
    fn pixel_renderer_force_reverse_video_cursor_uses_cell_foreground() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[38;2;255;0;0;48;2;0;0;255mA\x1b[1;1H");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let renderer = PixelRenderer::with_force_reverse_video_cursor(true);
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn pixel_renderer_reverse_video_cursor_min_contrast_uses_default_cursor_colors() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[38;2;17;17;17;48;2;16;16;16mA\x1b[1;1H");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let mut renderer = PixelRenderer::with_force_reverse_video_cursor(true)
            .with_reverse_video_cursor_min_contrast(2.5);
        renderer.set_default_cursor_color([7, 8, 9, 255]);
        renderer.set_default_cursor_foreground(Some([1, 2, 3, 255]));
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert!(count_pixels(&target, [1, 2, 3, 255]) > 0);
        assert!(count_pixels(&target, [7, 8, 9, 255]) > 0);
    }

    #[test]
    fn pixel_renderer_cursor_color_overrides_force_reverse_video_cursor() {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[38;2;255;0;0;48;2;0;0;255mA\x1b[1;1H");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal)
            .with_cursor_color(Some(Color::Rgb(0, 255, 0)));
        let renderer = PixelRenderer::with_force_reverse_video_cursor(true);
        let mut target = vec![0; 8 * 8 * 4];

        renderer.render(&snapshot, &mut target, 8, 8, 8, 8);

        assert_eq!(pixel_at(&target, 8, 0, 0), [0, 255, 0, 255]);
        assert_eq!(pixel_at(&target, 8, 7, 7), [0, 255, 0, 255]);
    }

    fn snapshot_char(snapshot: &TerminalRenderSnapshot, row: u16, column: u16) -> Option<char> {
        snapshot
            .cells()
            .iter()
            .find(|cell| cell.row == row && cell.column == column)
            .map(|cell| cell.ch)
    }

    fn feed_red_inline_png(terminal: &mut Terminal, params: &str) {
        const RED_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
        feed_inline_image(terminal, params, RED_PNG_BASE64);
    }

    fn feed_red_inline_jpeg(terminal: &mut Terminal, params: &str) {
        const RED_JPEG_BASE64: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/2wBDAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/wAARCAABAAEDAREAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD8X6/ynP8Av4P/2Q==";
        feed_inline_image(terminal, params, RED_JPEG_BASE64);
    }

    fn feed_red_inline_gif(terminal: &mut Terminal, params: &str) {
        const RED_GIF_BASE64: &str = "R0lGODdhAQABAIEAAP8AAAAAAAAAAAAAACwAAAAAAQABAAAIBAABBAQAOw==";
        feed_inline_image(terminal, params, RED_GIF_BASE64);
    }

    fn feed_red_green_inline_gif(terminal: &mut Terminal, params: &str) {
        const RED_GREEN_GIF_BASE64: &str = "R0lGODlhAQABAIEAAP8AAAAAAAAAAAAAACH/C05FVFNDQVBFMi4wAwEAAAAh+QQICgAAACwAAAAAAQABAAAIBAABBAQAIfkECAoAAAAsAAAAAAEAAQCBAP8AAAAAAAAAAAAACAQAAQQEADs=";
        feed_inline_image(terminal, params, RED_GREEN_GIF_BASE64);
    }

    fn feed_red_kitty_rgb_image(terminal: &mut Terminal) {
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
    }

    fn feed_compressed_red_kitty_rgb_image(terminal: &mut Terminal) {
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1,o=z;eJz7z8AAAAMAAQA=\x1b\\");
    }

    fn feed_chunked_red_green_kitty_rgb_image(terminal: &mut Terminal) {
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=T,f=24,s=2,v=1,c=1,r=1,m=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Gm=0;AP8A\x1b\\");
    }

    fn feed_kitty_rgb_file_image(terminal: &mut Terminal, path: &Path, extra_params: &str) {
        feed_kitty_rgb_local_file_image(terminal, path, 'f', extra_params);
    }

    fn feed_kitty_rgb_temporary_file_image(
        terminal: &mut Terminal,
        path: &Path,
        extra_params: &str,
    ) {
        feed_kitty_rgb_local_file_image(terminal, path, 't', extra_params);
    }

    fn feed_kitty_rgb_local_file_image(
        terminal: &mut Terminal,
        path: &Path,
        medium: char,
        extra_params: &str,
    ) {
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        let encoded_path = base64_standard(path.as_os_str().to_string_lossy().as_bytes());
        let sequence =
            format!("\x1b_Ga=T,t={medium},f=24,s=1,v=1,c=1,r=1{extra_params};{encoded_path}\x1b\\");
        terminal.feed(sequence.as_bytes());
    }

    fn feed_stored_red_kitty_rgb_image(terminal: &mut Terminal) {
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=7\x1b\\");
    }

    fn feed_overlapping_kitty_rgb_images(
        terminal: &mut Terminal,
        red_z_index: i32,
        green_z_index: i32,
    ) {
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(format!("\x1b_Ga=p,i=7,z={red_z_index}\x1b\\").as_bytes());
        terminal.feed(b"\x1b[1;1H");
        terminal.feed(format!("\x1b_Ga=p,i=8,z={green_z_index}\x1b\\").as_bytes());
    }

    fn feed_overlapping_kitty_rgb_images_high_id_first(terminal: &mut Terminal) {
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(b"\x1b_Ga=t,i=8,f=24,s=1,v=1,c=1,r=1;AP8A\x1b\\");
        terminal.feed(b"\x1b_Ga=p,i=8,z=2\x1b\\");
        terminal.feed(b"\x1b[1;1H");
        terminal.feed(b"\x1b_Ga=p,i=7,z=2\x1b\\");
    }

    fn feed_inline_image(terminal: &mut Terminal, params: &str, payload_base64: &str) {
        terminal.feed(b"\x1b[?25l");
        terminal.take_damage();
        let sequence = format!("\x1b]1337;File=inline=1;{params}:{payload_base64}\x07");
        terminal.feed(sequence.as_bytes());
    }

    struct KittyTestFile {
        path: PathBuf,
    }

    impl KittyTestFile {
        fn new(data: &[u8]) -> Self {
            Self::new_with_prefix("rssh-kitty-file", data)
        }

        fn new_with_prefix(prefix: &str, data: &[u8]) -> Self {
            static NEXT_TEST_FILE_ID: AtomicUsize = AtomicUsize::new(0);

            let suffix = NEXT_TEST_FILE_ID.fetch_add(1, Ordering::Relaxed);
            let mut path = std::env::temp_dir();
            path.push(format!("{prefix}-{}-{suffix}.rgb", std::process::id()));
            fs::write(&path, data).expect("write kitty test image file");
            Self { path }
        }
    }

    impl Drop for KittyTestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn base64_standard(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        let mut encoded = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let first = usize::from(chunk[0]);
            let second = usize::from(*chunk.get(1).unwrap_or(&0));
            let third = usize::from(*chunk.get(2).unwrap_or(&0));

            encoded.push(char::from(TABLE[first >> 2]));
            encoded.push(char::from(
                TABLE[((first & 0b0000_0011) << 4) | (second >> 4)],
            ));

            if chunk.len() > 1 {
                encoded.push(char::from(
                    TABLE[((second & 0b0000_1111) << 2) | (third >> 6)],
                ));
            } else {
                encoded.push('=');
            }

            if chunk.len() > 2 {
                encoded.push(char::from(TABLE[third & 0b0011_1111]));
            } else {
                encoded.push('=');
            }
        }

        encoded
    }

    fn pixel_at(target: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let index = (y * width + x) * 4;
        [
            target[index],
            target[index + 1],
            target[index + 2],
            target[index + 3],
        ]
    }

    fn count_pixels(target: &[u8], color: [u8; 4]) -> usize {
        target
            .chunks_exact(4)
            .filter(|pixel| *pixel == color)
            .count()
    }

    fn first_pixel_y(target: &[u8], width: usize, color: [u8; 4]) -> usize {
        target
            .chunks_exact(4)
            .position(|pixel| pixel == color)
            .map(|index| index / width)
            .expect("expected color pixel")
    }
}
