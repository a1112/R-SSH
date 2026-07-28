//! Terminal-oriented row shaping and logical/visual mapping.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;

use cosmic_text::{
    Attrs, Buffer, Family, FeatureTag, FontFeatures, Metrics, Shaping, Stretch, Style, Weight, Wrap,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::catalog::{FontCatalog, FontId, is_default_ignorable};
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
    /// Instrumented steps in the linear byte/cluster/cell index-building passes.
    ///
    /// This excludes backend shaping, visual sorting, and the cost of hash-table operations.
    pub linear_index_steps: usize,
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
    catalog_incarnation: u64,
    catalog_generation: u64,
    catalog_fingerprint: [u8; 32],
    config: FontConfig,
}

#[derive(Clone, Debug)]
struct FaceCandidate {
    font_id: FontId,
    font_family: String,
    is_color: bool,
}

#[derive(Clone, Debug)]
struct ClusterPlan {
    byte_range: Range<usize>,
    cell_span: Range<usize>,
    font_id: FontId,
    font_family: String,
    is_color: bool,
    is_tofu: bool,
    fallback_candidates: Vec<FaceCandidate>,
}

struct LayoutOutput {
    glyphs: Vec<ShapedGlyph>,
    line_count: usize,
    linear_index_steps: usize,
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
            catalog_incarnation: catalog.incarnation(),
            catalog_generation: catalog.generation(),
            catalog_fingerprint: catalog.fingerprint(),
            config: self.config.clone(),
        };
        if let Some((cached_key, row)) = &self.cache
            && cached_key == &key
        {
            self.stats.hits = self.stats.hits.saturating_add(1);
            return Ok(row.clone());
        }
        self.stats.misses = self.stats.misses.saturating_add(1);
        self.diagnostics.begin_row();

        let text: String = clusters
            .iter()
            .map(|cluster| cluster.text.as_str())
            .collect();
        let plans = self.plan_clusters(catalog, clusters);
        let row = self.shape_plans(catalog, text, plans)?;
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

            let config = &self.config;
            let diagnostics = &mut self.diagnostics;
            let mut candidates: Vec<_> = config
                .families()
                .filter_map(|family| {
                    Self::face_candidate(config, diagnostics, catalog, family, cluster)
                })
                .collect();
            let selected = (!candidates.is_empty()).then(|| candidates.remove(0));

            let (font_id, font_family, is_color, is_tofu) = if let Some(candidate) = selected {
                (
                    candidate.font_id,
                    candidate.font_family,
                    candidate.is_color,
                    false,
                )
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
                    .find_map(|family| {
                        catalog.record_for_family(
                            family,
                            self.config.weight,
                            self.config.style,
                            self.config.stretch,
                        )
                    })
                    .or_else(|| catalog.first_record())
                    .map_or(
                        (catalog.missing_font_id(), "<missing>".to_owned(), false),
                        |record| {
                            (
                                catalog.font_id(record.id),
                                record.family.clone(),
                                record.is_color,
                            )
                        },
                    );
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
                fallback_candidates: candidates,
            });
        }
        plans
    }

    fn face_candidate(
        config: &FontConfig,
        diagnostics: &mut Diagnostics,
        catalog: &FontCatalog,
        family: &str,
        cluster: &str,
    ) -> Option<FaceCandidate> {
        let Some(record) =
            catalog.record_for_family(family, config.weight, config.style, config.stretch)
        else {
            diagnostics.record(FontDiagnostic {
                kind: DiagnosticKind::MissingFamily,
                family: Some(family.to_owned()),
                cluster: None,
                catalog_generation: catalog.generation(),
            });
            return None;
        };
        if catalog.face_metrics(record, config.font_size).is_none() {
            diagnostics.record(FontDiagnostic {
                kind: DiagnosticKind::CorruptFont,
                family: Some(family.to_owned()),
                cluster: None,
                catalog_generation: catalog.generation(),
            });
            return None;
        }
        catalog
            .supports_cluster(record, cluster)
            .then_some(FaceCandidate {
                font_id: catalog.font_id(record.id),
                font_family: record.family.clone(),
                is_color: record.is_color,
            })
    }

    fn shape_plans(
        &mut self,
        catalog: &mut FontCatalog,
        text: String,
        mut plans: Vec<ClusterPlan>,
    ) -> Result<ShapedRow, ShapeError> {
        let metrics = self.resolve_metrics(catalog)?;
        let cell_count = plans.last().map_or(0, |plan| plan.cell_span.end);
        if plans.is_empty() {
            return Ok(ShapedRow {
                text,
                glyphs: Vec::new(),
                clusters: Vec::new(),
                visual_clusters: Vec::new(),
                cell_count: 0,
                layout_line_count: 1,
                catalog_generation: catalog.generation(),
                metrics,
                linear_index_steps: 0,
                diagnostics: self.diagnostics.snapshot(),
            });
        }

        let layout = self.layout_glyphs(catalog, &text, &mut plans, metrics);
        Ok(self.finish_row(catalog, text, plans, layout, cell_count, metrics))
    }

    fn resolve_metrics(&self, catalog: &FontCatalog) -> Result<TerminalFontMetrics, ShapeError> {
        let face = self
            .config
            .families()
            .filter_map(|family| {
                catalog.record_for_family(
                    family,
                    self.config.weight,
                    self.config.style,
                    self.config.stretch,
                )
            })
            .find_map(|record| catalog.face_metrics(record, self.config.font_size))
            .or_else(|| {
                catalog
                    .first_record()
                    .and_then(|record| catalog.face_metrics(record, self.config.font_size))
            })
            .ok_or(ShapeError::InvalidMetrics)?;
        let natural_line_height =
            (face.ascent + face.descent + face.line_gap).max(self.config.font_size);
        let line_height = natural_line_height * self.config.line_height;
        let baseline = face.ascent + (line_height - natural_line_height) / 2.0;
        let metrics = TerminalFontMetrics {
            font_size: self.config.font_size,
            cell_width: face.cell_width * self.config.cell_width,
            line_height,
            baseline,
            ascent: face.ascent,
            descent: face.descent,
        };
        if !metrics.cell_width.is_finite()
            || metrics.cell_width <= 0.0
            || !metrics.line_height.is_finite()
            || metrics.line_height <= 0.0
            || !metrics.baseline.is_finite()
            || metrics.baseline <= 0.0
        {
            return Err(ShapeError::InvalidMetrics);
        }
        Ok(metrics)
    }

    fn layout_glyphs(
        &mut self,
        catalog: &mut FontCatalog,
        text: &str,
        plans: &mut [ClusterPlan],
        metrics: TerminalFontMetrics,
    ) -> LayoutOutput {
        let mut linear_index_steps = 0;
        let mut byte_to_cluster = vec![0; text.len()];
        for (logical_index, plan) in plans.iter().enumerate() {
            for slot in &mut byte_to_cluster[plan.byte_range.clone()] {
                *slot = logical_index;
                linear_index_steps += 1;
            }
        }

        loop {
            let buffer = self.build_buffer(catalog, text, plans, metrics);
            let rejected = rejected_cluster_plans(catalog, text, plans, &buffer, &byte_to_cluster);
            let mut retry = false;
            for logical_index in rejected {
                let plan = &mut plans[logical_index];
                if let Some(candidate) = plan.fallback_candidates.first().cloned() {
                    plan.fallback_candidates.remove(0);
                    plan.font_id = candidate.font_id;
                    plan.font_family = candidate.font_family;
                    plan.is_color = candidate.is_color;
                    retry = true;
                } else {
                    mark_plan_tofu(&mut self.diagnostics, plan, text, catalog.generation());
                }
            }
            if retry {
                continue;
            }

            let (glyphs, line_count, glyph_steps) = self.collect_backend_glyphs(
                catalog,
                text,
                plans,
                &buffer,
                &byte_to_cluster,
                metrics,
            );
            return LayoutOutput {
                glyphs,
                line_count,
                linear_index_steps: linear_index_steps + glyph_steps,
            };
        }
    }

    fn build_buffer(
        &self,
        catalog: &mut FontCatalog,
        text: &str,
        plans: &[ClusterPlan],
        metrics: TerminalFontMetrics,
    ) -> Buffer {
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
        let families: Vec<_> = plans.iter().map(|plan| plan.font_family.clone()).collect();
        let spans: Vec<_> = plans
            .iter()
            .zip(&families)
            .map(|(plan, family)| {
                let attrs = Attrs::new()
                    .family(Family::Name(family))
                    .weight(weight)
                    .style(style)
                    .stretch(stretch)
                    .font_features(features.clone());
                (&text[plan.byte_range.clone()], attrs)
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
        buffer
    }

    fn collect_backend_glyphs(
        &mut self,
        catalog: &FontCatalog,
        text: &str,
        plans: &mut [ClusterPlan],
        buffer: &Buffer,
        byte_to_cluster: &[usize],
        metrics: TerminalFontMetrics,
    ) -> (Vec<ShapedGlyph>, usize, usize) {
        let mut linear_index_steps = 0;
        let mut glyphs = Vec::new();
        let mut plan_has_glyph = vec![false; plans.len()];
        let mut layout_line_count = 0;
        for run in buffer.layout_runs() {
            layout_line_count += 1;
            for glyph in run.glyphs {
                let cluster_range =
                    indexed_cluster_range(byte_to_cluster, glyph.start..glyph.end, plans.len());
                let cell_span = cells_for_clusters(plans, cluster_range.clone());
                linear_index_steps += 1;
                let planned_id = plans[cluster_range.start].font_id;
                let actual_id = catalog.font_id(glyph.font_id);
                let same_planned_face = plans[cluster_range.clone()]
                    .iter()
                    .all(|plan| plan.font_id == planned_id);
                linear_index_steps += cluster_range.end - cluster_range.start;
                for logical_index in cluster_range.clone() {
                    plan_has_glyph[logical_index] = true;
                }
                let mut is_tofu = plans[cluster_range.clone()].iter().any(|plan| plan.is_tofu);
                let actual_is_valid =
                    same_planned_face && actual_id == planned_id && glyph.glyph_id != 0 && !is_tofu;
                if !actual_is_valid {
                    for plan in &mut plans[cluster_range.clone()] {
                        mark_plan_tofu(&mut self.diagnostics, plan, text, catalog.generation());
                    }
                    is_tofu = true;
                }
                let is_color = plans[cluster_range.clone()]
                    .iter()
                    .all(|plan| plan.is_color);
                glyphs.push(ShapedGlyph {
                    font_id: if actual_is_valid {
                        actual_id
                    } else {
                        planned_id
                    },
                    glyph_id: if actual_is_valid { glyph.glyph_id } else { 0 },
                    byte_range: glyph.start..glyph.end,
                    cluster_range,
                    cell_span: cell_span.clone(),
                    visual_order: glyphs.len(),
                    x: glyph.x,
                    y: glyph.y,
                    width: glyph.w,
                    shaping_x: glyph.x,
                    shaping_width: glyph.w,
                    x_offset: glyph.x_offset,
                    y_offset: glyph.y_offset,
                    bidi_level: glyph.level.number(),
                    is_color,
                    is_tofu,
                });
            }
        }
        for (logical_index, plan) in plans.iter_mut().enumerate() {
            let cluster = &text[plan.byte_range.clone()];
            let has_visible_scalar = cluster
                .chars()
                .any(|character| !is_default_ignorable(character) && !character.is_whitespace());
            if !plan_has_glyph[logical_index] && has_visible_scalar {
                mark_plan_tofu(&mut self.diagnostics, plan, text, catalog.generation());
                let shaping_x = cell_pixels(plan.cell_span.start, metrics.cell_width);
                glyphs.push(ShapedGlyph {
                    font_id: plan.font_id,
                    glyph_id: 0,
                    byte_range: plan.byte_range.clone(),
                    cluster_range: logical_index..logical_index.saturating_add(1),
                    cell_span: plan.cell_span.clone(),
                    visual_order: glyphs.len(),
                    x: shaping_x,
                    y: 0.0,
                    width: metrics.cell_width,
                    shaping_x,
                    shaping_width: metrics.cell_width,
                    x_offset: 0.0,
                    y_offset: 0.0,
                    bidi_level: 0,
                    is_color: plan.is_color,
                    is_tofu: true,
                });
                linear_index_steps += 1;
            }
        }
        (glyphs, layout_line_count, linear_index_steps)
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
        glyphs.sort_by(|left, right| {
            left.shaping_x
                .total_cmp(&right.shaping_x)
                .then_with(|| left.visual_order.cmp(&right.visual_order))
        });
        let visual_clusters = visual_cluster_order(&glyphs, plans.len());
        let mut visual_indexes = vec![0; plans.len()];
        let mut visual_x = vec![0.0; plans.len()];
        let mut linear_index_steps = layout.linear_index_steps;
        let mut next_x = 0.0;
        for (visual_index, logical_index) in visual_clusters.iter().copied().enumerate() {
            visual_indexes[logical_index] = visual_index;
            visual_x[logical_index] = next_x;
            let cells = plans[logical_index].cell_span.end - plans[logical_index].cell_span.start;
            next_x += cell_pixels(cells, metrics.cell_width);
            linear_index_steps += 1;
        }

        let mut group_bounds: HashMap<(usize, usize), (f32, f32)> = HashMap::new();
        for glyph in &glyphs {
            let key = (glyph.cluster_range.start, glyph.cluster_range.end);
            let bounds = group_bounds
                .entry(key)
                .or_insert((glyph.shaping_x, glyph.shaping_x + glyph.shaping_width));
            bounds.0 = bounds.0.min(glyph.shaping_x);
            bounds.1 = bounds.1.max(glyph.shaping_x + glyph.shaping_width);
        }
        for (visual_order, glyph) in glyphs.iter_mut().enumerate() {
            glyph.visual_order = visual_order;
            let target_x = glyph
                .cluster_range
                .clone()
                .map(|logical_index| visual_x[logical_index])
                .min_by(f32::total_cmp)
                .unwrap_or(0.0);
            let target_width = cell_pixels(
                glyph.cell_span.end - glyph.cell_span.start,
                metrics.cell_width,
            );
            let bounds = group_bounds
                .get(&(glyph.cluster_range.start, glyph.cluster_range.end))
                .copied()
                .unwrap_or((glyph.shaping_x, glyph.shaping_x + glyph.shaping_width));
            let shaping_width = bounds.1 - bounds.0;
            let scale = if shaping_width.is_finite() && shaping_width > 0.0 {
                target_width / shaping_width
            } else {
                1.0
            };
            glyph.x = target_x + (glyph.shaping_x - bounds.0) * scale;
            glyph.width = glyph.shaping_width * scale;
            glyph.x_offset *= scale;
        }

        let mut first_glyph = vec![None; plans.len()];
        let mut last_glyph = vec![None; plans.len()];
        for (glyph_index, glyph) in glyphs.iter().enumerate() {
            for logical_index in glyph.cluster_range.clone() {
                first_glyph[logical_index].get_or_insert(glyph_index);
                last_glyph[logical_index] = Some(glyph_index);
                linear_index_steps += 1;
            }
        }
        let clusters = plans
            .into_iter()
            .enumerate()
            .map(|(logical_index, plan)| {
                let glyph_range = first_glyph[logical_index]
                    .zip(last_glyph[logical_index])
                    .map_or(0..0, |(first, last)| first..last.saturating_add(1));
                let bidi_level =
                    first_glyph[logical_index].map_or(0, |index| glyphs[index].bidi_level);
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
            linear_index_steps,
            diagnostics: self.diagnostics.snapshot(),
        }
    }
}

fn rejected_cluster_plans(
    catalog: &FontCatalog,
    text: &str,
    plans: &[ClusterPlan],
    buffer: &Buffer,
    byte_to_cluster: &[usize],
) -> Vec<usize> {
    let strict: Vec<_> = plans
        .iter()
        .map(|plan| requires_single_glyph_sequence(&text[plan.byte_range.clone()]))
        .collect();
    let mut glyph_counts = vec![0_usize; plans.len()];
    let mut valid = vec![true; plans.len()];
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let cluster_range =
                indexed_cluster_range(byte_to_cluster, glyph.start..glyph.end, plans.len());
            let same_planned_face = plans[cluster_range.clone()]
                .iter()
                .all(|plan| plan.font_id == plans[cluster_range.start].font_id);
            for logical_index in cluster_range.clone() {
                glyph_counts[logical_index] += 1;
                valid[logical_index] &= same_planned_face
                    && catalog.font_id(glyph.font_id) == plans[logical_index].font_id
                    && glyph.glyph_id != 0;
                if strict[logical_index] {
                    valid[logical_index] &= cluster_range.start == logical_index
                        && cluster_range.end == logical_index + 1
                        && glyph.start <= plans[logical_index].byte_range.start
                        && glyph.end >= plans[logical_index].byte_range.end;
                }
            }
        }
    }

    plans
        .iter()
        .enumerate()
        .filter_map(|(logical_index, plan)| {
            if plan.is_tofu {
                return None;
            }
            let cluster = &text[plan.byte_range.clone()];
            let has_visible_scalar = cluster
                .chars()
                .any(|character| !is_default_ignorable(character) && !character.is_whitespace());
            let wrong_glyph_count = if strict[logical_index] {
                glyph_counts[logical_index] != 1
            } else {
                glyph_counts[logical_index] == 0 && has_visible_scalar
            };
            (wrong_glyph_count || !valid[logical_index]).then_some(logical_index)
        })
        .collect()
}

fn requires_single_glyph_sequence(cluster: &str) -> bool {
    let regional_indicators = cluster
        .chars()
        .filter(|character| matches!(character, '\u{1f1e6}'..='\u{1f1ff}'))
        .count();
    cluster.chars().any(|character| {
        matches!(
            character,
            '\u{1f3fb}'..='\u{1f3ff}'
                | '\u{200d}'
                | '\u{20e3}'
                | '\u{fe00}'..='\u{fe0f}'
                | '\u{e0100}'..='\u{e01ef}'
        )
    }) || regional_indicators >= 2
}

fn mark_plan_tofu(
    diagnostics: &mut Diagnostics,
    plan: &mut ClusterPlan,
    text: &str,
    catalog_generation: u64,
) {
    if !plan.is_tofu {
        let cluster = &text[plan.byte_range.clone()];
        diagnostics.record(FontDiagnostic {
            kind: DiagnosticKind::MissingCluster,
            family: Some(plan.font_family.clone()),
            cluster: Some(cluster.to_owned()),
            catalog_generation,
        });
        diagnostics.record(FontDiagnostic {
            kind: DiagnosticKind::VisibleTofu,
            family: Some(plan.font_family.clone()),
            cluster: Some(cluster.to_owned()),
            catalog_generation,
        });
    }
    plan.is_tofu = true;
}

fn indexed_cluster_range(
    byte_to_cluster: &[usize],
    bytes: Range<usize>,
    cluster_count: usize,
) -> Range<usize> {
    if cluster_count == 0 {
        return 0..0;
    }
    let first = byte_to_cluster
        .get(bytes.start)
        .copied()
        .unwrap_or(cluster_count - 1);
    let last_byte = bytes.end.saturating_sub(1);
    let last = byte_to_cluster.get(last_byte).copied().unwrap_or(first);
    first.min(last)..first.max(last).saturating_add(1)
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
