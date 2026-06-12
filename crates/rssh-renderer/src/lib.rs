use std::io::Cursor;

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

const MIN_SCROLLBAR_THUMB_HEIGHT: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbackScrollbar {
    pub scrollback_lines: usize,
    pub viewport_rows: u16,
    pub scrollback_offset: usize,
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
        })
    }

    #[must_use]
    pub fn offset_from_pixel_y(self, y: u32, geometry: RenderGeometry) -> usize {
        if geometry.target_height == 0 {
            return self.scrollback_offset;
        }

        let thumb_height = scrollbar_thumb_height(self, geometry.target_height);
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
    animation_frame: usize,
    animation_elapsed_ms: Option<u64>,
}

impl PixelRenderer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            blink_visible: true,
            animation_frame: 0,
            animation_elapsed_ms: None,
        }
    }

    #[must_use]
    pub const fn with_blink_visible(blink_visible: bool) -> Self {
        Self {
            blink_visible,
            animation_frame: 0,
            animation_elapsed_ms: None,
        }
    }

    #[must_use]
    pub const fn with_animation_frame(animation_frame: usize) -> Self {
        Self {
            blink_visible: true,
            animation_frame,
            animation_elapsed_ms: None,
        }
    }

    #[must_use]
    pub const fn with_animation_elapsed_ms(animation_elapsed_ms: u64) -> Self {
        Self {
            blink_visible: true,
            animation_frame: 0,
            animation_elapsed_ms: Some(animation_elapsed_ms),
        }
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

        surface.fill(default_background());

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
            render_cell_background(&mut surface, cell, cell_width, cell_height);
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
                self.blink_visible,
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
            render_cursor(&mut surface, cursor, cell_width, cell_height);
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
            scrollbar_thumb_rect(scrollbar, geometry, track_width),
            SCROLLBAR_THUMB_COLOR,
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
            surface.fill_rect(
                damage_rect(region, geometry.cell_width, geometry.cell_height),
                default_background(),
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
                self.blink_visible,
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
            render_cursor(
                &mut surface,
                cursor,
                geometry.cell_width,
                geometry.cell_height,
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
) {
    let origin_x = u32::from(cell.column).saturating_mul(cell_width);
    let origin_y = u32::from(cell.row).saturating_mul(cell_height);
    let (_, background) = effective_cell_colors(cell);
    if background == default_background() {
        return;
    }

    surface.fill_rect(
        Rect {
            x: origin_x,
            y: origin_y,
            width: cell_width,
            height: cell_height,
        },
        background,
    );
}

fn render_cell_foreground(
    surface: &mut Surface<'_>,
    cell: &RenderCell,
    cell_width: u32,
    cell_height: u32,
    blink_visible: bool,
) {
    let origin_x = u32::from(cell.column).saturating_mul(cell_width);
    let origin_y = u32::from(cell.row).saturating_mul(cell_height);
    let (foreground, _) = effective_cell_colors(cell);

    if cell.conceal || (cell.blink && !blink_visible) {
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
            surface.fill_rect(
                Rect {
                    x: draw_x,
                    y: draw_y,
                    width,
                    height: scale_y,
                },
                foreground,
            );
            let bold_x = draw_x.saturating_add(scale_x);
            if cell.bold && bold_x < origin_x.saturating_add(cell_width) {
                surface.fill_rect(
                    Rect {
                        x: bold_x,
                        y: draw_y,
                        width: scale_x,
                        height: scale_y,
                    },
                    foreground,
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
        color_to_rgba(cell.underline_color, foreground),
    );
}

fn effective_cell_colors(cell: &RenderCell) -> ([u8; 4], [u8; 4]) {
    let foreground = color_to_rgba(cell.foreground, default_foreground());
    let background = color_to_rgba(cell.background, default_background());
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
) {
    render_underline_style(surface, cell, cell_rect, underline_color);

    if cell.overline {
        let overline_height = (cell_rect.height / 8).max(1);
        surface.fill_rect(
            Rect {
                x: cell_rect.x,
                y: cell_rect.y,
                width: cell_rect.width,
                height: overline_height,
            },
            foreground,
        );
    }

    if cell.strikethrough {
        let strike_height = (cell_rect.height / 8).max(1);
        let strike_y = cell_rect
            .y
            .saturating_add(cell_rect.height / 2)
            .saturating_sub(strike_height / 2);
        surface.fill_rect(
            Rect {
                x: cell_rect.x,
                y: strike_y,
                width: cell_rect.width,
                height: strike_height,
            },
            foreground,
        );
    }
}

fn render_underline_style(
    surface: &mut Surface<'_>,
    cell: &RenderCell,
    cell_rect: Rect,
    underline_color: [u8; 4],
) {
    let style = effective_underline_style(cell);
    if style == UnderlineStyle::None {
        return;
    }

    let underline_height = (cell_rect.height / 8).max(1);
    let lower_y = cell_rect.y + cell_rect.height.saturating_sub(underline_height);
    let lower_rect = Rect {
        x: cell_rect.x,
        y: lower_y,
        width: cell_rect.width,
        height: underline_height,
    };

    match style {
        UnderlineStyle::None => {}
        UnderlineStyle::Single => surface.fill_rect(lower_rect, underline_color),
        UnderlineStyle::Double => {
            surface.fill_rect(lower_rect, underline_color);
            surface.fill_rect(
                Rect {
                    y: lower_y.saturating_sub(underline_height.saturating_mul(2)),
                    ..lower_rect
                },
                underline_color,
            );
        }
        UnderlineStyle::Curly => render_curly_underline(surface, lower_rect, underline_color),
        UnderlineStyle::Dotted => {
            render_patterned_underline(surface, lower_rect, underline_color, 1, 1);
        }
        UnderlineStyle::Dashed => {
            render_patterned_underline(surface, lower_rect, underline_color, 3, 2);
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
    stroke_width: u32,
    gap_width: u32,
) {
    let cycle = stroke_width.saturating_add(gap_width).max(1);
    let mut offset = 0;
    while offset < rect.width {
        let segment_width = stroke_width.min(rect.width - offset);
        surface.fill_rect(
            Rect {
                x: rect.x + offset,
                width: segment_width,
                ..rect
            },
            color,
        );
        offset = offset.saturating_add(cycle);
    }
}

fn render_curly_underline(surface: &mut Surface<'_>, rect: Rect, color: [u8; 4]) {
    let upper_y = rect.y.saturating_sub(rect.height);
    for offset in 0..rect.width {
        let y = if (offset / 2) % 2 == 0 {
            upper_y
        } else {
            rect.y
        };
        surface.fill_rect(
            Rect {
                x: rect.x + offset,
                y,
                width: 1,
                height: rect.height,
            },
            color,
        );
    }
}

fn render_cursor(
    surface: &mut Surface<'_>,
    cursor: RenderCursor,
    cell_width: u32,
    cell_height: u32,
) {
    let origin_x = u32::from(cursor.column).saturating_mul(cell_width);
    let origin_y = u32::from(cursor.row).saturating_mul(cell_height);
    let rect = cursor_rect(cursor.shape, origin_x, origin_y, cell_width, cell_height);
    surface.fill_rect(rect, default_foreground());
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
) -> Rect {
    let thumb_height = scrollbar_thumb_height(scrollbar, geometry.target_height);
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

fn scrollbar_thumb_height(scrollbar: ScrollbackScrollbar, target_height: u32) -> u32 {
    let viewport_rows = u64::from(scrollbar.viewport_rows);
    let total_rows = viewport_rows.saturating_add(scrollbar.scrollback_lines as u64);
    let target_height_u64 = u64::from(target_height);
    let proportional_height = if total_rows == 0 {
        target_height_u64
    } else {
        target_height_u64.saturating_mul(viewport_rows) / total_rows
    };

    u32::try_from(proportional_height)
        .unwrap_or(target_height)
        .max(MIN_SCROLLBAR_THUMB_HEIGHT)
        .min(target_height)
}

fn color_to_rgba(color: Color, default: [u8; 4]) -> [u8; 4] {
    match color {
        Color::Default => default,
        Color::Indexed(index) => indexed_color(index),
        Color::Rgb(red, green, blue) => [red, green, blue, 255],
        Color::Rgba(red, green, blue, alpha) => [red, green, blue, alpha],
    }
}

fn dim_foreground(color: [u8; 4]) -> [u8; 4] {
    [color[0] / 2, color[1] / 2, color[2] / 2, color[3]]
}

fn indexed_color(index: u8) -> [u8; 4] {
    const ANSI: [[u8; 4]; 16] = [
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

    if let Some(color) = ANSI.get(usize::from(index)) {
        return *color;
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
                );
            } else {
                let grid_row = source_row - scrollback.len();
                append_grid_row(&mut cells, grid, viewport_row, grid_row, size.columns);
            }
        }

        let inline_images =
            render_inline_images_from_terminal(terminal, first_source_row, size.rows, size.columns);

        Self {
            cells,
            cursor,
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
            inline_images: Vec::new(),
        }
    }

    #[must_use]
    pub fn cells(&self) -> &[RenderCell] {
        &self.cells
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
                    append_render_cell(&mut self.cells, row, column, cell);
                }
            }
        }

        self.cells.sort_by_key(|cell| (cell.row, cell.column));
        self.inline_images =
            render_inline_images_from_terminal(terminal, 0, size.rows, size.columns);
        self.cursor = render_cursor_from_terminal(terminal, 0);
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
    })
}

fn cursor_rect(
    shape: CursorShape,
    origin_x: u32,
    origin_y: u32,
    cell_width: u32,
    cell_height: u32,
) -> Rect {
    match shape {
        CursorShape::Block => Rect {
            x: origin_x,
            y: origin_y,
            width: cell_width,
            height: cell_height,
        },
        CursorShape::Underline => {
            let height = (cell_height / 6).max(1);
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
            width: (cell_width / 4).max(1),
            height: cell_height,
        },
    }
}

fn append_grid_row(
    cells: &mut Vec<RenderCell>,
    grid: &TerminalGrid,
    viewport_row: u16,
    grid_row: usize,
    columns: u16,
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
        append_render_cell(cells, viewport_row, column, cell);
    }
}

fn append_render_cells(
    cells: &mut Vec<RenderCell>,
    viewport_row: u16,
    source_cells: &[Cell],
    columns: u16,
) {
    for (column, cell) in source_cells.iter().take(usize::from(columns)).enumerate() {
        let Ok(column) = u16::try_from(column) else {
            continue;
        };
        append_render_cell(cells, viewport_row, column, cell);
    }
}

fn append_render_cell(cells: &mut Vec<RenderCell>, row: u16, column: u16, cell: &Cell) {
    if !cell_has_renderable_content(cell) {
        return;
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
        DamageRegion, PixelRenderer, RenderCell, RenderGeometry, RenderInlineImage,
        SCROLLBAR_THUMB_COLOR, SCROLLBAR_TRACK_COLOR, ScrollbackScrollbar, TerminalRenderSnapshot,
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
                underline: true,
                double_underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
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
                underline: false,
                double_underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                vertical_align: VerticalAlign::Baseline,
                inverse: true,
                hyperlink: None,
                semantic_type: SemanticType::Output,
            },
        );

        let snapshot = TerminalRenderSnapshot::from_grid(&grid);

        assert!(snapshot.cells()[0].inverse);
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
        terminal.feed(b"one\ntwo\n");
        terminal.feed(b"\x1b]1337;File=inline=1:QQ==\x07");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        assert_eq!(snapshot.inline_images().len(), 1);
        assert_eq!(snapshot.inline_images()[0].row, 1);
        assert_eq!(snapshot.inline_images()[0].column, 0);
    }

    #[test]
    fn render_snapshot_can_view_inline_images_in_scrollback() {
        let mut terminal = Terminal::new(TerminalSize::new(8, 2));
        terminal.feed(b"\x1b]1337;File=inline=1:QQ==\x07one\ntwo\n");

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
        assert_eq!(snapshot_char(&snapshot, 1, 0), Some('a'));
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
                underline: false,
                double_underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
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
                underline: false,
                double_underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
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
                underline: false,
                double_underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
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
                underline: false,
                double_underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                vertical_align: VerticalAlign::Baseline,
                inverse: false,
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
                underline: false,
                double_underline: false,
                conceal: false,
                strikethrough: false,
                overline: false,
                vertical_align: VerticalAlign::Baseline,
                inverse: true,
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
        terminal.feed(b"ab\nc");

        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        let cursor = snapshot.cursor().expect("cursor should be visible");
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.column, 1);
    }

    #[test]
    fn render_snapshot_can_show_scrollback_viewport() {
        let mut terminal = Terminal::new(TerminalSize::new(4, 2));
        terminal.feed(b"ab\ncd\nef");

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
