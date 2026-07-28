//! Terminal-oriented row shaping and logical/visual mapping.

use std::collections::HashSet;
use std::fmt;
use std::ops::Range;

use cosmic_text::{
    Attrs, Buffer, Family, FeatureTag, FontFeatures, Metrics, Shaping, Stretch, Style, Weight, Wrap,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::catalog::{FontCatalog, FontId};
use crate::config::{FontConfig, FontStretch, FontStyle};
use crate::diagnostics::{DiagnosticKind, Diagnostics, FontDiagnostic};

/// Logical terminal cell range.
pub type CellSpan = Range<usize>;

/// Logical grapheme-cluster range.
pub type ClusterSpan = Range<usize>;

/// Primary-face metrics resolved for terminal grid geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalFontMetrics {
    /// Shaping font size in logical pixels.
    pub font_size: f32,
    /// Width of one terminal cell after the configured multiplier.
    pub cell_width: f32,
    /// Full row height after the configured multiplier.
    pub line_height: f32,
    /// Distance from row top to the alphabetic baseline.
    pub baseline: f32,
    /// Scaled primary-face ascender.
    pub ascent: f32,
    /// Positive scaled primary-face descender.
    pub descent: f32,
}

/// One terminal-owned grapheme and its authoritative logical cell span.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TerminalCluster {
    /// One complete extended grapheme cluster.
    pub text: String,
    /// Cell geometry established by the terminal model.
    pub cell_span: CellSpan,
}

impl TerminalCluster {
    /// Creates a terminal cluster with an explicit cell span.
    #[must_use]
    pub fn new(text: impl Into<String>, cell_span: CellSpan) -> Self {
        Self {
            text: text.into(),
            cell_span,
        }
    }

    /// Creates a cluster from its starting cell and width.
    #[must_use]
    pub fn with_columns(text: impl Into<String>, start: usize, columns: usize) -> Self {
        Self::new(text, start..start.saturating_add(columns))
    }
}

/// Input error that would otherwise make the shaping backend panic or mis-map cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShapeError {
    /// The isolated catalog contains no usable face.
    NoUsableFont,
    /// A font size, line height, or cell width is zero, non-finite, or negative.
    InvalidMetrics,
    /// A cluster input is empty or contains more than one extended grapheme.
    InvalidCluster,
    /// Authoritative cell spans are empty, overlap, or are not contiguous.
    InvalidCellSpan,
    /// Terminal rows cannot contain CR or LF.
    EmbeddedLineBreak,
}

impl fmt::Display for ShapeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NoUsableFont => "font catalog contains no usable face",
            Self::InvalidMetrics => "font metrics must be finite and greater than zero",
            Self::InvalidCluster => "each input must contain one complete grapheme cluster",
            Self::InvalidCellSpan => "terminal cell spans must be non-empty and contiguous",
            Self::EmbeddedLineBreak => "a terminal row cannot contain CR or LF",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ShapeError {}

/// A glyph positioned for a terminal renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Catalog font face used by the shaping engine.
    pub font_id: FontId,
    /// Glyph identifier within the face.
    pub glyph_id: u16,
    /// UTF-8 source range covered by the shaping cluster.
    pub byte_range: Range<usize>,
    /// Logical grapheme clusters covered by this glyph.
    pub cluster_range: ClusterSpan,
    /// Logical terminal cells covered by this glyph.
    pub cell_span: CellSpan,
    /// Index in renderer draw order.
    pub visual_order: usize,
    /// Terminal-grid x position.
    pub x: f32,
    /// Baseline-relative y position from the shaping engine.
    pub y: f32,
    /// Terminal-grid width; configured cell geometry wins over glyph advance.
    pub width: f32,
    /// Original proportional x position, retained for diagnostics.
    pub shaping_x: f32,
    /// Original proportional hitbox width, retained for diagnostics.
    pub shaping_width: f32,
    /// Outline x offset reported by the shaping engine.
    pub x_offset: f32,
    /// Outline y offset reported by the shaping engine.
    pub y_offset: f32,
    /// Unicode bidi embedding level.
    pub bidi_level: u8,
    /// Whether the selected face has color tables.
    pub is_color: bool,
    /// Whether this glyph represents an uncovered cluster.
    pub is_tofu: bool,
}

/// One logical extended grapheme cluster in a shaped row.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedCluster {
    /// UTF-8 source range.
    pub byte_range: Range<usize>,
    /// Logical terminal cell span.
    pub cell_span: CellSpan,
    /// Font selected for the complete cluster.
    pub font_id: FontId,
    /// Canonical selected family.
    pub font_family: String,
    /// Glyph slice touching this cluster.
    pub glyph_range: Range<usize>,
    /// Stable logical cluster index.
    pub logical_index: usize,
    /// Index of this cluster in visual order.
    pub visual_index: usize,
    /// Unicode bidi embedding level.
    pub bidi_level: u8,
    /// Whether no configured font covers the complete cluster.
    pub is_tofu: bool,
}

/// Fully shaped terminal row.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedRow {
    /// Original logical UTF-8 row.
    pub text: String,
    /// Renderer-ready glyphs in visual order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Grapheme clusters in logical order.
    pub clusters: Vec<ShapedCluster>,
    /// Logical cluster indexes in renderer visual order.
    pub visual_clusters: Vec<usize>,
    /// Total logical terminal cells.
    pub cell_count: usize,
    /// Number of physical layout lines (always one for a terminal row).
    pub layout_line_count: usize,
    /// Catalog generation used for shaping.
    pub catalog_generation: u64,
    /// Primary-face geometry used by the row.
    pub metrics: TerminalFontMetrics,
    /// Deduplicated diagnostics known when this row was shaped.
    pub diagnostics: Vec<FontDiagnostic>,
}

/// Basic cache counters exposed for metrics and invalidation tests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShapeCacheStats {
    /// Reuses of the most recently shaped row.
    pub hits: u64,
    /// Actual shaping operations.
    pub misses: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct ShapeCacheKey {
    clusters: Vec<TerminalCluster>,
    catalog_generation: u64,
    config: FontConfig,
}

#[derive(Clone, Debug)]
struct ClusterPlan {
    byte_range: Range<usize>,
    cell_span: Range<usize>,
    font_id: FontId,
    font_family: String,
    is_color: bool,
    is_tofu: bool,
}

struct LayoutOutput {
    glyphs: Vec<ShapedGlyph>,
    line_count: usize,
}

/// Shapes logical terminal rows using an isolated [`FontCatalog`].
pub struct TerminalShaper {
    config: FontConfig,
    diagnostics: Diagnostics,
    cache: Option<(ShapeCacheKey, ShapedRow)>,
    stats: ShapeCacheStats,
}

impl TerminalShaper {
    /// Creates a shaper for one effective terminal font configuration.
    #[must_use]
    pub fn new(config: FontConfig) -> Self {
        Self {
            config,
            diagnostics: Diagnostics::default(),
            cache: None,
            stats: ShapeCacheStats::default(),
        }
    }

    /// Replaces the effective configuration and invalidates the row cache.
    pub fn set_config(&mut self, config: FontConfig) {
        if self.config != config {
            self.config = config;
            self.cache = None;
        }
    }

    /// Current cache counters.
    #[must_use]
    pub const fn cache_stats(&self) -> ShapeCacheStats {
        self.stats
    }

    /// Shapes one unwrapped terminal row.
    ///
    /// # Errors
    ///
    /// Returns [`ShapeError`] when metrics, row contents, or the catalog are invalid.
    pub fn shape_row(
        &mut self,
        catalog: &mut FontCatalog,
        text: &str,
    ) -> Result<ShapedRow, ShapeError> {
        if text.contains(['\r', '\n']) {
            return Err(ShapeError::EmbeddedLineBreak);
        }
        let mut cell = 0;
        let clusters: Vec<_> = text
            .graphemes(true)
            .map(|cluster| {
                let width = UnicodeWidthStr::width(cluster);
                let width = if width == 0 && !cluster.is_empty() {
                    1
                } else {
                    width
                };
                let input = TerminalCluster::with_columns(cluster, cell, width);
                cell += width;
                input
            })
            .collect();
        self.shape_clusters(catalog, &clusters)
    }

    /// Shapes terminal-owned clusters using their authoritative cell geometry.
    ///
    /// # Errors
    ///
    /// Returns [`ShapeError`] before entering the shaping backend when the
    /// catalog, metrics, clusters, or cell spans are invalid.
    pub fn shape_clusters(
        &mut self,
        catalog: &mut FontCatalog,
        clusters: &[TerminalCluster],
    ) -> Result<ShapedRow, ShapeError> {
        self.validate(catalog, clusters)?;
        let key = ShapeCacheKey {
            clusters: clusters.to_vec(),
            catalog_generation: catalog.generation(),
            config: self.config.clone(),
        };
        if let Some((cached_key, row)) = &self.cache
            && cached_key == &key
        {
            self.stats.hits = self.stats.hits.saturating_add(1);
            return Ok(row.clone());
        }
        self.stats.misses = self.stats.misses.saturating_add(1);

        let text: String = clusters
            .iter()
            .map(|cluster| cluster.text.as_str())
            .collect();
        let plans = self.plan_clusters(catalog, clusters);
        let row = self.shape_plans(catalog, text, plans);
        self.cache = Some((key, row.clone()));
        Ok(row)
    }

    fn validate(
        &self,
        catalog: &FontCatalog,
        clusters: &[TerminalCluster],
    ) -> Result<(), ShapeError> {
        if !self.config.metrics_are_valid() {
            return Err(ShapeError::InvalidMetrics);
        }
        if catalog.face_count() == 0 {
            return Err(ShapeError::NoUsableFont);
        }
        let mut expected_cell = 0;
        for cluster in clusters {
            if cluster.text.contains(['\r', '\n']) {
                return Err(ShapeError::EmbeddedLineBreak);
            }
            if cluster.text.graphemes(true).count() != 1 {
                return Err(ShapeError::InvalidCluster);
            }
            if cluster.cell_span.start != expected_cell
                || cluster.cell_span.end <= cluster.cell_span.start
            {
                return Err(ShapeError::InvalidCellSpan);
            }
            expected_cell = cluster.cell_span.end;
        }
        Ok(())
    }

    fn plan_clusters(
        &mut self,
        catalog: &FontCatalog,
        clusters: &[TerminalCluster],
    ) -> Vec<ClusterPlan> {
        let mut plans = Vec::new();
        let mut byte = 0;
        for input in clusters {
            let cluster = input.text.as_str();
            let byte_range = byte..byte + cluster.len();
            byte = byte_range.end;

            let selected = self.config.families().find_map(|family| {
                let Some(record) = catalog.record_for_family(family) else {
                    self.diagnostics.record(FontDiagnostic {
                        kind: DiagnosticKind::MissingFamily,
                        family: Some(family.to_owned()),
                        cluster: None,
                        catalog_generation: catalog.generation(),
                    });
                    return None;
                };
                catalog.supports_cluster(record, cluster).then_some((
                    FontId::from_cosmic(record.id),
                    record.family.clone(),
                    record.is_color,
                ))
            });

            let (font_id, font_family, is_color, is_tofu) =
                if let Some((font_id, family, is_color)) = selected {
                    (font_id, family, is_color, false)
                } else {
                    self.diagnostics.record(FontDiagnostic {
                        kind: DiagnosticKind::MissingCluster,
                        family: None,
                        cluster: Some(cluster.to_owned()),
                        catalog_generation: catalog.generation(),
                    });
                    let tofu = self
                        .config
                        .families()
                        .find_map(|family| catalog.record_for_family(family))
                        .or_else(|| catalog.first_record())
                        .map_or((FontId::MISSING, "<missing>".to_owned(), false), |record| {
                            (
                                FontId::from_cosmic(record.id),
                                record.family.clone(),
                                record.is_color,
                            )
                        });
                    self.diagnostics.record(FontDiagnostic {
                        kind: DiagnosticKind::VisibleTofu,
                        family: Some(tofu.1.clone()),
                        cluster: Some(cluster.to_owned()),
                        catalog_generation: catalog.generation(),
                    });
                    (tofu.0, tofu.1, tofu.2, true)
                };

            plans.push(ClusterPlan {
                byte_range,
                cell_span: input.cell_span.clone(),
                font_id,
                font_family,
                is_color,
                is_tofu,
            });
        }
        plans
    }

    fn shape_plans(
        &self,
        catalog: &mut FontCatalog,
        text: String,
        plans: Vec<ClusterPlan>,
    ) -> ShapedRow {
        let metrics = self.resolve_metrics(catalog);
        let cell_count = plans.last().map_or(0, |plan| plan.cell_span.end);
        if plans.is_empty() {
            return ShapedRow {
                text,
                glyphs: Vec::new(),
                clusters: Vec::new(),
                visual_clusters: Vec::new(),
                cell_count: 0,
                layout_line_count: 1,
                catalog_generation: catalog.generation(),
                metrics,
                diagnostics: self.diagnostics.snapshot(),
            };
        }

        let layout = self.layout_glyphs(catalog, &text, &plans, metrics);
        self.finish_row(catalog, text, plans, layout, cell_count, metrics)
    }

    fn resolve_metrics(&self, catalog: &FontCatalog) -> TerminalFontMetrics {
        let record = self
            .config
            .families()
            .find_map(|family| catalog.record_for_family(family))
            .or_else(|| catalog.first_record());
        let face = record
            .and_then(|record| catalog.face_metrics(record, self.config.font_size))
            .unwrap_or(crate::catalog::FaceMetrics {
                cell_width: self.config.font_size * 0.6,
                ascent: self.config.font_size * 0.8,
                descent: self.config.font_size * 0.2,
                line_gap: 0.0,
            });
        let natural_line_height =
            (face.ascent + face.descent + face.line_gap).max(self.config.font_size);
        let line_height = natural_line_height * self.config.line_height;
        let baseline = face.ascent + (line_height - natural_line_height) / 2.0;
        TerminalFontMetrics {
            font_size: self.config.font_size,
            cell_width: face.cell_width * self.config.cell_width,
            line_height,
            baseline,
            ascent: face.ascent,
            descent: face.descent,
        }
    }

    fn layout_glyphs(
        &self,
        catalog: &mut FontCatalog,
        text: &str,
        plans: &[ClusterPlan],
        metrics: TerminalFontMetrics,
    ) -> LayoutOutput {
        let mut features = FontFeatures::new();
        if !self.config.ligatures {
            features.disable(FeatureTag::STANDARD_LIGATURES);
            features.disable(FeatureTag::CONTEXTUAL_LIGATURES);
        }
        for (tag, value) in &self.config.features {
            features.set(FeatureTag::new(tag), *value);
        }

        let weight = Weight(self.config.weight);
        let style = match self.config.style {
            FontStyle::Normal => Style::Normal,
            FontStyle::Italic => Style::Italic,
            FontStyle::Oblique => Style::Oblique,
        };
        let stretch = match self.config.stretch {
            FontStretch::Condensed => Stretch::Condensed,
            FontStretch::Normal => Stretch::Normal,
            FontStretch::Expanded => Stretch::Expanded,
        };
        let spans: Vec<_> = plans
            .iter()
            .map(|plan| {
                (
                    &text[plan.byte_range.clone()],
                    Attrs::new()
                        .family(Family::Name(&plan.font_family))
                        .weight(weight)
                        .style(style)
                        .stretch(stretch)
                        .font_features(features.clone()),
                )
            })
            .collect();
        let default_attrs = Attrs::new()
            .weight(weight)
            .style(style)
            .stretch(stretch)
            .font_features(features);
        let mut buffer = Buffer::new_empty(Metrics::new(metrics.font_size, metrics.line_height));
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, None);
        buffer.set_rich_text(spans, &default_attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(catalog.font_system_mut(), false);

        let mut glyphs = Vec::new();
        let mut layout_line_count = 0;
        for run in buffer.layout_runs() {
            layout_line_count += 1;
            glyphs.extend(run.glyphs.iter().enumerate().map(|(run_order, glyph)| {
                let cluster_range = overlapping_clusters(plans, glyph.start..glyph.end);
                let cell_span = cells_for_clusters(plans, cluster_range.clone());
                let is_tofu = plans[cluster_range.clone()].iter().any(|plan| plan.is_tofu);
                let is_color = plans[cluster_range.clone()]
                    .iter()
                    .all(|plan| plan.is_color);
                ShapedGlyph {
                    font_id: FontId::from_cosmic(glyph.font_id),
                    glyph_id: glyph.glyph_id,
                    byte_range: glyph.start..glyph.end,
                    cluster_range,
                    cell_span: cell_span.clone(),
                    visual_order: run_order,
                    x: glyph.x,
                    y: glyph.y,
                    width: cell_pixels(cell_span.end - cell_span.start, metrics.cell_width),
                    shaping_x: glyph.x,
                    shaping_width: glyph.w,
                    x_offset: glyph.x_offset,
                    y_offset: glyph.y_offset,
                    bidi_level: glyph.level.number(),
                    is_color,
                    is_tofu,
                }
            }));
        }
        LayoutOutput {
            glyphs,
            line_count: layout_line_count,
        }
    }

    fn finish_row(
        &self,
        catalog: &FontCatalog,
        text: String,
        plans: Vec<ClusterPlan>,
        layout: LayoutOutput,
        cell_count: usize,
        metrics: TerminalFontMetrics,
    ) -> ShapedRow {
        let mut glyphs = layout.glyphs;
        let visual_clusters = visual_cluster_order(&glyphs, plans.len());
        let mut visual_indexes = vec![0; plans.len()];
        let mut visual_x = vec![0.0; plans.len()];
        let mut next_x = 0.0;
        for (visual_index, logical_index) in visual_clusters.iter().copied().enumerate() {
            visual_indexes[logical_index] = visual_index;
            visual_x[logical_index] = next_x;
            let cells = plans[logical_index].cell_span.end - plans[logical_index].cell_span.start;
            next_x += cell_pixels(cells, metrics.cell_width);
        }
        for (visual_order, glyph) in glyphs.iter_mut().enumerate() {
            glyph.visual_order = visual_order;
            glyph.x = glyph
                .cluster_range
                .clone()
                .map(|logical_index| visual_x[logical_index])
                .min_by(f32::total_cmp)
                .unwrap_or(0.0);
        }

        let clusters = plans
            .into_iter()
            .enumerate()
            .map(|(logical_index, plan)| {
                let touching: Vec<_> = glyphs
                    .iter()
                    .enumerate()
                    .filter(|(_, glyph)| glyph.cluster_range.contains(&logical_index))
                    .map(|(index, _)| index)
                    .collect();
                let glyph_range = touching
                    .first()
                    .zip(touching.last())
                    .map_or(0..0, |(first, last)| *first..last.saturating_add(1));
                let bidi_level = touching
                    .first()
                    .map_or(0, |index| glyphs[*index].bidi_level);
                ShapedCluster {
                    byte_range: plan.byte_range,
                    cell_span: plan.cell_span,
                    font_id: plan.font_id,
                    font_family: plan.font_family,
                    glyph_range,
                    logical_index,
                    visual_index: visual_indexes[logical_index],
                    bidi_level,
                    is_tofu: plan.is_tofu,
                }
            })
            .collect();

        ShapedRow {
            text,
            glyphs,
            clusters,
            visual_clusters,
            cell_count,
            layout_line_count: layout.line_count,
            catalog_generation: catalog.generation(),
            metrics,
            diagnostics: self.diagnostics.snapshot(),
        }
    }
}

fn overlapping_clusters(plans: &[ClusterPlan], bytes: Range<usize>) -> Range<usize> {
    let first = plans
        .iter()
        .position(|plan| ranges_overlap(&plan.byte_range, &bytes))
        .unwrap_or(0);
    let last = plans
        .iter()
        .rposition(|plan| ranges_overlap(&plan.byte_range, &bytes))
        .unwrap_or(first);
    first..last.saturating_add(1)
}

#[allow(clippy::cast_precision_loss)]
fn cell_pixels(cells: usize, cell_width: f32) -> f32 {
    cells as f32 * cell_width
}

fn cells_for_clusters(plans: &[ClusterPlan], clusters: Range<usize>) -> Range<usize> {
    let Some(first) = plans.get(clusters.start) else {
        return 0..0;
    };
    let last = &plans[clusters.end - 1];
    first.cell_span.start..last.cell_span.end
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn visual_cluster_order(glyphs: &[ShapedGlyph], cluster_count: usize) -> Vec<usize> {
    let mut result = Vec::with_capacity(cluster_count);
    let mut seen = HashSet::new();
    for glyph in glyphs {
        let logical: Box<dyn Iterator<Item = usize>> = if glyph.bidi_level % 2 == 0 {
            Box::new(glyph.cluster_range.clone())
        } else {
            Box::new(glyph.cluster_range.clone().rev())
        };
        for cluster in logical {
            if seen.insert(cluster) {
                result.push(cluster);
            }
        }
    }
    for cluster in 0..cluster_count {
        if seen.insert(cluster) {
            result.push(cluster);
        }
    }
    result
}
