//! Renderer-neutral terminal snapshots, geometry, damage, and stable digests.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use font8x8::{BASIC_FONTS, UnicodeFonts as _};
use rssh_terminal::{
    Cell, Color, CursorShape, InlineImageFormat, InlineImageFragment, ItermInlineImage, Terminal,
    TerminalGrid, UnderlineStyle, VerticalAlign,
};
pub use rterm_types::DamageRegion;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct RenderCell {
    pub row: u16,
    pub column: u16,
    /// Complete terminal grapheme. Empty for continuation cells.
    pub text: String,
    /// Logical width stored on a grapheme leader.
    pub columns: u8,
    pub continuation: bool,
    /// Temporary first-scalar compatibility field for the bitmap renderer.
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
    pub hyperlink: Option<Arc<str>>,
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

/// Pixel rectangle for one cell-granular inline-image fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderInlineImageFragment {
    pub parent_image_index: usize,
    pub cell_attachment: bool,
    pub row: u16,
    pub column: u16,
    pub source_row: i64,
    pub source_column: i64,
    pub destination_x: u32,
    pub destination_y: u32,
    pub destination_width: u32,
    pub destination_height: u32,
    pub source_x: u32,
    pub source_y: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub sampling_source_x: u32,
    pub sampling_source_y: u32,
    pub sampling_source_width: u32,
    pub sampling_source_height: u32,
    pub source_destination_x: u32,
    pub source_destination_y: u32,
    pub source_destination_width: u32,
    pub source_destination_height: u32,
}

#[doc(hidden)]
pub const KITTY_NON_DEFAULT_BACKGROUND_Z_CUTOFF: i32 = i32::MIN / 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRenderSnapshot {
    #[doc(hidden)]
    pub cells: Vec<RenderCell>,
    #[doc(hidden)]
    pub cursor: Option<RenderCursor>,
    #[doc(hidden)]
    pub cursor_color: Option<Color>,
    #[doc(hidden)]
    pub inline_images: Vec<RenderInlineImage>,
    #[doc(hidden)]
    pub inline_image_fragments: Vec<RenderInlineImageFragment>,
    #[doc(hidden)]
    pub inline_image_parent_origins: Vec<(i64, i64)>,
    /// Render-parent indices for logical placements that have no surviving
    /// cells. Kept private so protocol/image metadata remains stable.
    #[doc(hidden)]
    pub empty_inline_image_attachment_parents: HashSet<usize>,
    #[doc(hidden)]
    pub inline_image_attachment_viewport_offsets: HashMap<(usize, i64, i64), (i64, i64)>,
    #[doc(hidden)]
    pub inline_image_attachment_viewport_clips: HashMap<(usize, i64, i64), AttachmentViewportClip>,
    #[doc(hidden)]
    pub scrollback_offset: usize,
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct RuntimeInlineImageFragment {
    pub fragment: RenderInlineImageFragment,
    pub row_offset: i64,
    pub column_offset: i64,
    pub clip: Option<AttachmentViewportClip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct AttachmentViewportClip {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl AttachmentViewportClip {
    #[doc(hidden)]
    #[must_use]
    pub fn translated(self, row: u16, column: u16) -> Self {
        Self {
            left: self.left.saturating_add(i64::from(column)),
            top: self.top.saturating_add(i64::from(row)),
            right: self.right.saturating_add(i64::from(column)),
            bottom: self.bottom.saturating_add(i64::from(row)),
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn intersection(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderGeometry {
    pub target_width: u32,
    pub target_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub content_x: u32,
    pub content_y: u32,
    pub content_width: u32,
    pub content_height: u32,
    /// Optional one-pixel frame chrome painted outside the content viewport.
    ///
    /// This is deliberately part of the surface geometry rather than the
    /// terminal snapshot so the physical frame never changes PTY dimensions,
    /// cell placement, or hit testing.
    pub frame_border_color: Option<[u8; 4]>,
    /// Optional horizontal chrome separator in target-surface pixel space.
    pub frame_separator: Option<(u32, [u8; 4])>,
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
            content_x: 0,
            content_y: 0,
            content_width: target_width,
            content_height: target_height,
            frame_border_color: None,
            frame_separator: None,
        }
    }

    #[must_use]
    pub fn with_content_rect(mut self, x: u32, y: u32, width: u32, height: u32) -> Self {
        self.content_x = x.min(self.target_width);
        self.content_y = y.min(self.target_height);
        self.content_width = width.min(self.target_width.saturating_sub(self.content_x));
        self.content_height = height.min(self.target_height.saturating_sub(self.content_y));
        self
    }

    #[must_use]
    pub fn with_frame_border(mut self, color: [u8; 4]) -> Self {
        self.frame_border_color = Some(color);
        self
    }

    #[must_use]
    pub fn with_frame_separator(mut self, y: u32, color: [u8; 4]) -> Self {
        self.frame_separator = Some((y, color));
        self
    }
}

pub const SCROLLBAR_TRACK_COLOR: [u8; 4] = [0x10, 0x18, 0x27, 0xff];
pub const SCROLLBAR_THUMB_COLOR: [u8; 4] = [0x47, 0x55, 0x69, 0xff];
pub const SCROLLBAR_WIDTH: u32 = 4;
#[doc(hidden)]
pub const DEFAULT_DPI: u32 = 96;
pub type RenderIndexedPalette = [Option<[u8; 4]>; 256];

pub type TerminalContentDigest = [u8; 32];

#[must_use]
pub fn terminal_bytes_content_digest(bytes: &[u8]) -> TerminalContentDigest {
    Sha256::digest(bytes).into()
}

/// Hashes the exact ordered terminal render plan, including cell placement and
/// grapheme span, with SHA-256.
#[must_use]
pub fn terminal_snapshot_content_digest(
    snapshot: &TerminalRenderSnapshot,
) -> TerminalContentDigest {
    let mut digest = Sha256::new();
    digest.update(b"rssh-terminal-render-plan-v1\0");
    digest.update(
        u64::try_from(snapshot.cells().len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for cell in snapshot.cells() {
        digest.update(cell.row.to_le_bytes());
        digest.update(cell.column.to_le_bytes());
        digest.update([cell.columns, u8::from(cell.continuation)]);
        digest.update(
            u64::try_from(cell.text.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        digest.update(cell.text.as_bytes());
    }
    digest.finalize().into()
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
            if let Some(line) = scrollback.get(source_row) {
                append_render_cells(
                    &mut cells,
                    viewport_row,
                    line.cells(),
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

        let (
            inline_images,
            inline_image_fragments,
            inline_image_parent_origins,
            empty_inline_image_attachment_parents,
            inline_image_attachment_viewport_offsets,
        ) = render_inline_images_from_terminal(terminal, first_source_row, size.rows, size.columns);

        Self {
            cells,
            cursor,
            cursor_color: None,
            inline_images,
            inline_image_fragments,
            inline_image_parent_origins,
            empty_inline_image_attachment_parents,
            inline_image_attachment_viewport_offsets,
            inline_image_attachment_viewport_clips: std::collections::HashMap::new(),
            scrollback_offset: offset,
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
                    text: cell.text().to_owned(),
                    columns: cell.columns(),
                    continuation: cell.is_continuation(),
                    ch: cell.primary_char(),
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
            inline_image_fragments: Vec::new(),
            inline_image_parent_origins: Vec::new(),
            empty_inline_image_attachment_parents: HashSet::new(),
            inline_image_attachment_viewport_offsets: HashMap::new(),
            inline_image_attachment_viewport_clips: std::collections::HashMap::new(),
            scrollback_offset: 0,
        }
    }

    #[must_use]
    pub fn cells(&self) -> &[RenderCell] {
        &self.cells
    }

    /// Reconstructs one immutable row as complete graphemes with terminal-owned spans.
    ///
    /// Default blank cells are represented explicitly so the result is contiguous
    /// and can be passed directly to the shaping engine.
    #[must_use]
    pub fn terminal_clusters_for_row(
        &self,
        row: u16,
        columns: u16,
    ) -> Vec<rssh_fonts::TerminalCluster> {
        let mut by_column = self
            .cells
            .iter()
            .filter(|cell| cell.row == row)
            .map(|cell| (cell.column, cell))
            .collect::<HashMap<_, _>>();
        let mut clusters = Vec::new();
        let mut column = 0_u16;
        while column < columns {
            match by_column.remove(&column) {
                Some(cell) if cell.continuation => {
                    clusters.push(rssh_fonts::TerminalCluster::new(
                        " ",
                        usize::from(column)..usize::from(column.saturating_add(1)),
                    ));
                    column = column.saturating_add(1);
                }
                Some(cell) => {
                    let width = u16::from(cell.columns.max(1))
                        .min(columns.saturating_sub(column))
                        .max(1);
                    clusters.push(rssh_fonts::TerminalCluster::new(
                        cell.text.clone(),
                        usize::from(column)..usize::from(column.saturating_add(width)),
                    ));
                    column = column.saturating_add(width);
                }
                None => {
                    clusters.push(rssh_fonts::TerminalCluster::new(
                        " ",
                        usize::from(column)..usize::from(column.saturating_add(1)),
                    ));
                    column = column.saturating_add(1);
                }
            }
        }
        clusters
    }

    #[must_use]
    pub fn missing_glyphs(&self) -> Vec<char> {
        let mut missing = Vec::new();
        for cell in &self.cells {
            // The native GPU path supplies the modern terminal UI symbols
            // from its configured fallback catalog.  Keep the compatibility
            // warning focused on characters that neither the legacy 8x8
            // renderer nor the guaranteed UI fallback set can represent.
            if BASIC_FONTS.get(cell.ch).is_none()
                && !modern_ui_fallback_glyph(cell.ch)
                && !missing.contains(&cell.ch)
            {
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
    pub fn inline_image_fragments(&self) -> &[RenderInlineImageFragment] {
        &self.inline_image_fragments
    }

    #[must_use]
    pub const fn scrollback_offset(&self) -> usize {
        self.scrollback_offset
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
        for fragment in &mut self.inline_image_fragments {
            fragment.row = fragment.row.saturating_add(offset);
            if !fragment.cell_attachment {
                fragment.source_row = fragment.source_row.saturating_add(i64::from(offset));
            }
        }
        for (_, row) in &mut self.inline_image_parent_origins {
            *row = row.saturating_add(i64::from(offset));
        }
        for clip in self.inline_image_attachment_viewport_clips.values_mut() {
            clip.top = clip.top.saturating_add(i64::from(offset));
            clip.bottom = clip.bottom.saturating_add(i64::from(offset));
        }
        if let Some(cursor) = self.cursor.as_mut() {
            cursor.row = cursor.row.saturating_add(offset);
        }

        self
    }

    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "viewport projection atomically remaps cells, images, fragments, and attachment metadata"
    )]
    pub fn with_viewport(
        mut self,
        origin_row: u16,
        origin_column: u16,
        rows: u16,
        columns: u16,
    ) -> Self {
        self.cells
            .retain(|cell| cell.row < rows && cell.column < columns);
        let old_inline_images = std::mem::take(&mut self.inline_images);
        let old_inline_image_parent_origins = std::mem::take(&mut self.inline_image_parent_origins);
        let old_inline_image_fragments = std::mem::take(&mut self.inline_image_fragments);
        let old_empty_attachment_parents =
            std::mem::take(&mut self.empty_inline_image_attachment_parents);
        let old_attachment_offsets =
            std::mem::take(&mut self.inline_image_attachment_viewport_offsets);
        let old_attachment_clips = std::mem::take(&mut self.inline_image_attachment_viewport_clips);
        let mut attachment_parents = HashSet::new();
        let mut visible_attachment_parents = HashSet::new();
        let mut visible_attachment_keys = HashSet::new();
        let mut visible_fragments = Vec::new();
        for fragment in old_inline_image_fragments {
            let visible = if fragment.cell_attachment {
                attachment_parents.insert(fragment.parent_image_index);
                let key = (
                    fragment.parent_image_index,
                    fragment.source_row,
                    fragment.source_column,
                );
                let (row_offset, column_offset) =
                    old_attachment_offsets.get(&key).copied().unwrap_or((0, 0));
                let row = i64::from(fragment.row).saturating_add(row_offset);
                let column = i64::from(fragment.column).saturating_add(column_offset);
                let visible =
                    row >= 0 && row < i64::from(rows) && column >= 0 && column < i64::from(columns);
                if visible {
                    visible_attachment_parents.insert(fragment.parent_image_index);
                    visible_attachment_keys.insert(key);
                }
                visible
            } else {
                fragment.row < rows && fragment.column < columns
            };
            if visible {
                visible_fragments.push(fragment);
            }
        }
        let mut parent_indices = vec![None; old_inline_images.len()];
        for (old_index, image) in old_inline_images.into_iter().enumerate() {
            let keep = if attachment_parents.contains(&old_index) {
                visible_attachment_parents.contains(&old_index)
            } else {
                image.row < rows && image.column < columns
            };
            if keep {
                let new_index = self.inline_images.len();
                parent_indices[old_index] = Some(new_index);
                if old_empty_attachment_parents.contains(&old_index) {
                    self.empty_inline_image_attachment_parents.insert(new_index);
                }
                self.inline_images.push(image);
                self.inline_image_parent_origins.push(
                    old_inline_image_parent_origins
                        .get(old_index)
                        .copied()
                        .unwrap_or((0, 0)),
                );
            }
        }
        self.inline_image_fragments = visible_fragments
            .into_iter()
            .filter_map(|mut fragment| {
                let Some(Some(parent_image_index)) =
                    parent_indices.get(fragment.parent_image_index)
                else {
                    return None;
                };
                fragment.parent_image_index = *parent_image_index;
                Some(fragment)
            })
            .collect();
        for cell in &mut self.cells {
            cell.row = cell.row.saturating_add(origin_row);
            cell.column = cell.column.saturating_add(origin_column);
        }
        for image in &mut self.inline_images {
            image.row = image.row.saturating_add(origin_row);
            image.column = image.column.saturating_add(origin_column);
        }
        for fragment in &mut self.inline_image_fragments {
            fragment.row = fragment.row.saturating_add(origin_row);
            fragment.column = fragment.column.saturating_add(origin_column);
            if !fragment.cell_attachment {
                fragment.source_row = fragment.source_row.saturating_add(i64::from(origin_row));
                fragment.source_column = fragment
                    .source_column
                    .saturating_add(i64::from(origin_column));
            }
        }
        self.inline_image_attachment_viewport_offsets = old_attachment_offsets
            .into_iter()
            .filter_map(|((old_parent, source_row, source_column), offset)| {
                visible_attachment_keys
                    .contains(&(old_parent, source_row, source_column))
                    .then_some(())?;
                let new_parent = *parent_indices.get(old_parent)?.as_ref()?;
                Some(((new_parent, source_row, source_column), offset))
            })
            .collect();
        let viewport_clip = AttachmentViewportClip {
            left: i64::from(origin_column),
            top: i64::from(origin_row),
            right: i64::from(origin_column).saturating_add(i64::from(columns)),
            bottom: i64::from(origin_row).saturating_add(i64::from(rows)),
        };
        self.inline_image_attachment_viewport_clips = visible_attachment_keys
            .into_iter()
            .filter_map(|(old_parent, source_row, source_column)| {
                let new_parent = *parent_indices.get(old_parent)?.as_ref()?;
                let clip = old_attachment_clips
                    .get(&(old_parent, source_row, source_column))
                    .copied()
                    .map_or(viewport_clip, |clip| {
                        clip.translated(origin_row, origin_column)
                    })
                    .intersection(viewport_clip);
                Some(((new_parent, source_row, source_column), clip))
            })
            .collect();
        for (column, row) in &mut self.inline_image_parent_origins {
            *column = column.saturating_add(i64::from(origin_column));
            *row = row.saturating_add(i64::from(origin_row));
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
        let parent_index_offset = self.inline_images.len();
        self.inline_images.extend(snapshot.inline_images);
        self.empty_inline_image_attachment_parents.extend(
            snapshot
                .empty_inline_image_attachment_parents
                .into_iter()
                .map(|parent| parent.saturating_add(parent_index_offset)),
        );
        self.inline_image_parent_origins
            .extend(snapshot.inline_image_parent_origins);
        self.inline_image_fragments
            .extend(
                snapshot
                    .inline_image_fragments
                    .into_iter()
                    .map(|mut fragment| {
                        fragment.parent_image_index = fragment
                            .parent_image_index
                            .saturating_add(parent_index_offset);
                        fragment
                    }),
            );
        self.inline_image_attachment_viewport_offsets.extend(
            snapshot
                .inline_image_attachment_viewport_offsets
                .into_iter()
                .map(|((parent, source_row, source_column), offset)| {
                    (
                        (
                            parent.saturating_add(parent_index_offset),
                            source_row,
                            source_column,
                        ),
                        offset,
                    )
                }),
        );
        self.inline_image_attachment_viewport_clips.extend(
            snapshot
                .inline_image_attachment_viewport_clips
                .into_iter()
                .map(|((parent, source_row, source_column), clip)| {
                    (
                        (
                            parent.saturating_add(parent_index_offset),
                            source_row,
                            source_column,
                        ),
                        clip,
                    )
                }),
        );
        self.sort_inline_images_and_remap_fragments();
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
        (
            self.inline_images,
            self.inline_image_fragments,
            self.inline_image_parent_origins,
            self.empty_inline_image_attachment_parents,
            self.inline_image_attachment_viewport_offsets,
        ) = render_inline_images_from_terminal(terminal, 0, size.rows, size.columns);
        self.inline_image_attachment_viewport_clips.clear();
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
                cell.background = selection_background.map_or(inverse_background, |background| {
                    blend_selection_background(background, cell.background)
                });
                cell.inverse = false;
            }
        }

        self
    }

    fn sort_inline_images_and_remap_fragments(&mut self) {
        let old_inline_image_parent_origins = std::mem::take(&mut self.inline_image_parent_origins);
        let old_empty_attachment_parents =
            std::mem::take(&mut self.empty_inline_image_attachment_parents);
        let mut indexed_images = std::mem::take(&mut self.inline_images)
            .into_iter()
            .enumerate()
            .map(|(index, image)| {
                (
                    index,
                    image,
                    old_inline_image_parent_origins
                        .get(index)
                        .copied()
                        .unwrap_or((0, 0)),
                )
            })
            .collect::<Vec<_>>();
        indexed_images.sort_by_key(|(_, image, _)| (image.row, image.column));
        let mut parent_indices = vec![0; indexed_images.len()];
        let mut inline_images = Vec::with_capacity(indexed_images.len());
        let mut inline_image_parent_origins = Vec::with_capacity(indexed_images.len());
        for (new_index, (old_index, image, origin)) in indexed_images.into_iter().enumerate() {
            parent_indices[old_index] = new_index;
            inline_images.push(image);
            inline_image_parent_origins.push(origin);
        }
        self.inline_images = inline_images;
        self.inline_image_parent_origins = inline_image_parent_origins;
        self.empty_inline_image_attachment_parents = old_empty_attachment_parents
            .into_iter()
            .filter_map(|old_parent| parent_indices.get(old_parent).copied())
            .collect();
        self.inline_image_fragments.retain_mut(|fragment| {
            let Some(parent_image_index) = parent_indices.get(fragment.parent_image_index) else {
                return false;
            };
            fragment.parent_image_index = *parent_image_index;
            true
        });
        let old_attachment_offsets =
            std::mem::take(&mut self.inline_image_attachment_viewport_offsets);
        self.inline_image_attachment_viewport_offsets = old_attachment_offsets
            .into_iter()
            .filter_map(|((old_parent, source_row, source_column), offset)| {
                let new_parent = *parent_indices.get(old_parent)?;
                Some(((new_parent, source_row, source_column), offset))
            })
            .collect();
        let old_attachment_clips = std::mem::take(&mut self.inline_image_attachment_viewport_clips);
        self.inline_image_attachment_viewport_clips = old_attachment_clips
            .into_iter()
            .filter_map(|((old_parent, source_row, source_column), clip)| {
                let new_parent = *parent_indices.get(old_parent)?;
                Some(((new_parent, source_row, source_column), clip))
            })
            .collect();
    }
}

/// Glyphs used by the modern tab bar and native window chrome.
///
/// These are intentionally kept small and explicit: the GPU font catalog
/// carries the actual outlines, while this predicate prevents the legacy
/// compatibility diagnostic from reporting known fallback-backed UI glyphs
/// as missing before the GPU pass has shaped them.
fn modern_ui_fallback_glyph(character: char) -> bool {
    matches!(character, '×' | '▾' | '—' | '□')
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

#[doc(hidden)]
pub type AttachmentViewportOffset = ((usize, i64, i64), (i64, i64));
#[doc(hidden)]
pub type TerminalInlineImageProjection = (
    Vec<RenderInlineImage>,
    Vec<RenderInlineImageFragment>,
    Vec<(i64, i64)>,
    HashSet<usize>,
    HashMap<(usize, i64, i64), (i64, i64)>,
);

#[doc(hidden)]
#[must_use]
pub fn render_inline_images_from_terminal(
    terminal: &Terminal,
    first_source_row: usize,
    rows: u16,
    columns: u16,
) -> TerminalInlineImageProjection {
    let last_source_row = first_source_row.saturating_add(usize::from(rows));
    let terminal_fragments = terminal.inline_image_fragments();
    let fragment_parent_indices = terminal_fragments
        .iter()
        .filter(|fragment| {
            (fragment.cell_attachment
                && ((fragment.row >= first_source_row && fragment.row < last_source_row)
                    || terminal
                        .inline_images()
                        .get(fragment.image_index)
                        .is_some_and(|image| image.target_y.is_some())))
                || (fragment.row >= first_source_row
                    && fragment.row < last_source_row
                    && fragment.column < columns)
        })
        .map(|fragment| fragment.image_index)
        .collect::<HashSet<_>>();
    let mut images = terminal
        .inline_images()
        .iter()
        .enumerate()
        .filter_map(|(image_index, image)| {
            render_inline_image_item(image, first_source_row, last_source_row, columns)
                .or_else(|| {
                    fragment_parent_indices
                        .contains(&image_index)
                        .then(|| {
                            render_inline_image_item(image, image.row, image.row + 1, u16::MAX)
                        })
                        .flatten()
                })
                .and_then(|render_image| {
                    Some((
                        image_index,
                        render_image,
                        (
                            i64::from(image.column),
                            i64::try_from(image.row).ok()?
                                - i64::try_from(first_source_row).ok()?,
                        ),
                    ))
                })
        })
        .collect::<Vec<_>>();
    images.sort_by_key(|(_, image, _)| (image.row, image.column));
    let parent_indices = images
        .iter()
        .enumerate()
        .map(|(render_index, (terminal_index, _, _))| (*terminal_index, render_index))
        .collect::<HashMap<_, _>>();
    let empty_inline_image_attachment_parents = images
        .iter()
        .enumerate()
        .filter_map(|(render_index, (terminal_index, _, _))| {
            terminal
                .inline_image_attachment_parent_is_empty(*terminal_index)
                .then_some(render_index)
        })
        .collect::<HashSet<_>>();
    let (fragments, attachment_viewport_offsets) = terminal_fragments
        .into_iter()
        .filter_map(|fragment| {
            render_inline_image_fragment_item(
                &fragment,
                &parent_indices,
                first_source_row,
                last_source_row,
                columns,
            )
        })
        .unzip();
    (
        images.iter().map(|(_, image, _)| image.clone()).collect(),
        fragments,
        images.into_iter().map(|(_, _, origin)| origin).collect(),
        empty_inline_image_attachment_parents,
        attachment_viewport_offsets,
    )
}

fn render_inline_image_fragment_item(
    fragment: &InlineImageFragment,
    parent_indices: &HashMap<usize, usize>,
    first_source_row: usize,
    last_source_row: usize,
    columns: u16,
) -> Option<(RenderInlineImageFragment, AttachmentViewportOffset)> {
    if fragment.column >= columns
        || (!fragment.cell_attachment
            && (fragment.row < first_source_row || fragment.row >= last_source_row))
    {
        return None;
    }
    let parent_image_index = *parent_indices.get(&fragment.image_index)?;
    let relative_row = i64::try_from(fragment.row).ok()? - i64::try_from(first_source_row).ok()?;
    let row = u16::try_from(relative_row.max(0)).ok()?;
    let render_fragment = RenderInlineImageFragment {
        parent_image_index,
        cell_attachment: fragment.cell_attachment,
        row,
        column: fragment.column,
        source_row: if fragment.cell_attachment {
            i64::try_from(fragment.source_row).ok()?
        } else {
            i64::try_from(fragment.source_row).ok()? - i64::try_from(first_source_row).ok()?
        },
        source_column: i64::from(fragment.source_column),
        destination_x: fragment.destination_x,
        destination_y: fragment.destination_y,
        destination_width: fragment.destination_width,
        destination_height: fragment.destination_height,
        source_x: fragment.source_x,
        source_y: fragment.source_y,
        source_width: fragment.source_width,
        source_height: fragment.source_height,
        sampling_source_x: fragment.sampling_source_x,
        sampling_source_y: fragment.sampling_source_y,
        sampling_source_width: fragment.sampling_source_width,
        sampling_source_height: fragment.sampling_source_height,
        source_destination_x: fragment.source_destination_x,
        source_destination_y: fragment.source_destination_y,
        source_destination_width: fragment.source_destination_width,
        source_destination_height: fragment.source_destination_height,
    };
    let attachment_key = (
        parent_image_index,
        render_fragment.source_row,
        render_fragment.source_column,
    );
    Some((
        render_fragment,
        (attachment_key, (relative_row - i64::from(row), 0)),
    ))
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
        text: cell.text().to_owned(),
        columns: cell.columns(),
        continuation: cell.is_continuation(),
        ch: cell.primary_char(),
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
    cell.is_continuation()
        || (!cell.is_blank() && cell.text() != " ")
        || cell.background != Color::Default
        || cell.inverse
        || cell.underline
        || cell.double_underline
        || cell.strikethrough
        || cell.overline
}

fn blend_channel(foreground: u8, background: u8, alpha: u16, inverse_alpha: u16) -> u8 {
    let blended = u16::from(foreground)
        .saturating_mul(alpha)
        .saturating_add(u16::from(background).saturating_mul(inverse_alpha))
        / u16::from(u8::MAX);
    u8::try_from(blended).unwrap_or(u8::MAX)
}
