use std::fs;
use std::path::{Path, PathBuf};

use rssh_fonts::{
    DiagnosticKind, FontCatalog, FontConfig, FontSource, ShapeError, TerminalCluster,
    TerminalShaper,
};

const LATIN: &str = "NotoSans-Latin.fixture.ttf";
const CJK: &str = "NotoSansSC-CJK.fixture.ttf";
const ARABIC: &str = "NotoSansArabic.fixture.ttf";
const DEVANAGARI: &str = "NotoSansDevanagari.fixture.ttf";
const HEBREW: &str = "NotoSansHebrew.fixture.ttf";
const SYMBOLS: &str = "NotoSansSymbols2.fixture.ttf";
const EMOJI: &str = "NotoColorEmoji.fixture.ttf";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fonts")
}

fn source(file: &str) -> FontSource {
    FontSource::new(
        file,
        fs::read(fixture_dir().join(file)).expect("read fixture"),
    )
}

fn catalog(files: &[&str]) -> FontCatalog {
    FontCatalog::from_sources("en-US", files.iter().map(|file| source(file)))
        .expect("load deterministic fixtures")
}

fn fixture_config() -> FontConfig {
    FontConfig::new("Noto Sans")
        .with_fallbacks([
            "Noto Sans SC",
            "Noto Sans Arabic",
            "Noto Sans Devanagari",
            "Noto Sans Hebrew",
            "Noto Sans Symbols 2",
            "Noto Color Emoji",
        ])
        .with_font_size(16.0)
        .with_line_height(1.2)
        .with_cell_width(1.0)
}

#[test]
fn configured_primary_and_ordered_whole_cluster_fallback() {
    let mut catalog = catalog(&[LATIN, CJK, EMOJI]);
    let config = FontConfig::new("Noto Sans").with_fallbacks(["Noto Sans SC", "Noto Color Emoji"]);
    let mut shaper = TerminalShaper::new(config);

    let row = shaper.shape_row(&mut catalog, "A中👍🏽").expect("shape row");

    assert_eq!(row.clusters.len(), 3);
    assert_eq!(row.clusters[0].font_family, "Noto Sans");
    assert_eq!(row.clusters[1].font_family, "Noto Sans SC");
    assert_eq!(row.clusters[2].font_family, "Noto Color Emoji");
    for cluster in &row.clusters {
        assert!(
            row.glyphs[cluster.glyph_range.clone()]
                .iter()
                .all(|glyph| glyph.font_id == cluster.font_id),
            "a grapheme cluster must never mix fonts"
        );
    }
}

#[test]
fn fallback_order_is_observable_when_multiple_families_cover_a_cluster() {
    let mut catalog = catalog(&[LATIN, SYMBOLS, EMOJI]);
    let mut text_first = TerminalShaper::new(
        FontConfig::new("Noto Sans").with_fallbacks(["Noto Sans Symbols 2", "Noto Color Emoji"]),
    );
    let mut emoji_first = TerminalShaper::new(
        FontConfig::new("Noto Sans").with_fallbacks(["Noto Color Emoji", "Noto Sans Symbols 2"]),
    );

    let text_row = text_first
        .shape_row(&mut catalog, "✈")
        .expect("shape text-first fallback");
    let emoji_row = emoji_first
        .shape_row(&mut catalog, "✈")
        .expect("shape emoji-first fallback");

    assert_eq!(text_row.clusters[0].font_family, "Noto Sans Symbols 2");
    assert_eq!(emoji_row.clusters[0].font_family, "Noto Color Emoji");
}

#[test]
fn ligature_feature_can_be_enabled_and_disabled() {
    let mut catalog = catalog(&[LATIN]);
    let mut enabled = TerminalShaper::new(FontConfig::new("Noto Sans").with_ligatures(true));
    let mut disabled = TerminalShaper::new(FontConfig::new("Noto Sans").with_ligatures(false));

    let enabled_row = enabled.shape_row(&mut catalog, "fi").expect("shape row");
    let disabled_row = disabled.shape_row(&mut catalog, "fi").expect("shape row");

    assert_eq!(
        enabled_row.glyphs.len(),
        1,
        "fixture retains the fi ligature"
    );
    assert_eq!(disabled_row.glyphs.len(), 2);
    assert_eq!(enabled_row.glyphs[0].byte_range, 0..2);
    assert_eq!(enabled_row.glyphs[0].cluster_range, 0..2);
    assert_eq!(enabled_row.glyphs[0].cell_span, 0..2);
}

#[test]
fn complex_script_glyphs_keep_logical_byte_cluster_and_cell_ranges() {
    let mut catalog = catalog(&[LATIN, ARABIC, DEVANAGARI]);
    let config =
        FontConfig::new("Noto Sans").with_fallbacks(["Noto Sans Arabic", "Noto Sans Devanagari"]);
    let mut shaper = TerminalShaper::new(config);
    let text = "سلاّم क्षि";

    let row = shaper.shape_row(&mut catalog, text).expect("shape row");

    for glyph in &row.glyphs {
        assert!(text.is_char_boundary(glyph.byte_range.start));
        assert!(text.is_char_boundary(glyph.byte_range.end));
        assert!(glyph.byte_range.start < glyph.byte_range.end);
        let first = &row.clusters[glyph.cluster_range.start];
        let last = &row.clusters[glyph.cluster_range.end - 1];
        assert!(first.byte_range.start <= glyph.byte_range.start);
        assert!(glyph.byte_range.end <= last.byte_range.end);
        assert_eq!(glyph.cell_span, first.cell_span.start..last.cell_span.end);
    }
    let devanagari = row
        .clusters
        .iter()
        .find(|cluster| &text[cluster.byte_range.clone()] == "क्षि")
        .expect("one Devanagari grapheme");
    assert_eq!(devanagari.font_family, "Noto Sans Devanagari");
    assert!(devanagari.cell_span.end > devanagari.cell_span.start);
}

#[test]
fn bidi_visual_order_retains_logical_cell_mapping() {
    let mut catalog = catalog(&[LATIN, HEBREW, ARABIC]);
    let config =
        FontConfig::new("Noto Sans").with_fallbacks(["Noto Sans Hebrew", "Noto Sans Arabic"]);
    let mut shaper = TerminalShaper::new(config);

    for text in ["A אבג B", "A سلام B"] {
        let row = shaper.shape_row(&mut catalog, text).expect("shape row");
        let logical: Vec<_> = row
            .clusters
            .iter()
            .map(|cluster| cluster.logical_index)
            .collect();
        assert_eq!(logical, (0..row.clusters.len()).collect::<Vec<_>>());
        assert_ne!(
            row.visual_clusters, logical,
            "the RTL run should be reordered visually"
        );
        let mut permutation = row.visual_clusters.clone();
        permutation.sort_unstable();
        assert_eq!(permutation, logical);
        for (visual, logical_index) in row.visual_clusters.iter().copied().enumerate() {
            assert_eq!(row.clusters[logical_index].visual_index, visual);
            assert!(row.clusters[logical_index].cell_span.end <= row.cell_count);
        }
    }
}

#[test]
fn cjk_cluster_maps_to_two_terminal_cells_without_wrapping() {
    let mut catalog = catalog(&[LATIN, CJK]);
    let mut shaper =
        TerminalShaper::new(FontConfig::new("Noto Sans").with_fallbacks(["Noto Sans SC"]));

    let row = shaper.shape_row(&mut catalog, "A中文").expect("shape row");

    assert_eq!(row.cell_count, 5);
    assert_eq!(row.clusters[1].cell_span, 1..3);
    assert_eq!(row.clusters[2].cell_span, 3..5);
    assert_eq!(row.layout_line_count, 1, "terminal shaping uses Wrap::None");
}

#[test]
fn primary_face_metrics_drive_cell_width_baseline_and_line_height() {
    let mut catalog = catalog(&[LATIN]);
    let mut natural = TerminalShaper::new(
        FontConfig::new("Noto Sans")
            .with_font_size(16.0)
            .with_cell_width(1.0)
            .with_line_height(1.0),
    );
    let mut scaled = TerminalShaper::new(
        FontConfig::new("Noto Sans")
            .with_font_size(16.0)
            .with_cell_width(2.0)
            .with_line_height(1.5),
    );

    let natural = natural
        .shape_row(&mut catalog, "A")
        .expect("natural metrics");
    let scaled = scaled.shape_row(&mut catalog, "A").expect("scaled metrics");

    assert!((scaled.metrics.cell_width - natural.metrics.cell_width * 2.0).abs() < 0.001);
    assert!((scaled.metrics.line_height - natural.metrics.line_height * 1.5).abs() < 0.001);
    assert!(scaled.metrics.ascent > 0.0);
    assert!(scaled.metrics.descent >= 0.0);
    assert!(scaled.metrics.baseline > 0.0);
    assert!((scaled.glyphs[0].width - scaled.metrics.cell_width).abs() < 0.001);
}

#[test]
fn emoji_variations_and_sequences_select_one_expected_font_per_cluster() {
    let mut catalog = catalog(&[LATIN, SYMBOLS, EMOJI]);
    let config =
        FontConfig::new("Noto Sans").with_fallbacks(["Noto Sans Symbols 2", "Noto Color Emoji"]);
    let mut shaper = TerminalShaper::new(config);
    let text = "✈︎ ✈️ 👍🏽 👨‍👩‍👧‍👦 🇺🇸";

    let row = shaper.shape_row(&mut catalog, text).expect("shape row");
    let non_space: Vec<_> = row
        .clusters
        .iter()
        .filter(|cluster| &text[cluster.byte_range.clone()] != " ")
        .collect();

    assert_eq!(non_space.len(), 5);
    assert_eq!(non_space[0].font_family, "Noto Sans Symbols 2");
    assert!(
        non_space[1..]
            .iter()
            .all(|cluster| cluster.font_family == "Noto Color Emoji")
    );
    assert!(
        row.glyphs[non_space[0].glyph_range.clone()]
            .iter()
            .all(|glyph| !glyph.is_color)
    );
    assert!(non_space[1..].iter().all(|cluster| {
        row.glyphs[cluster.glyph_range.clone()]
            .iter()
            .all(|glyph| glyph.is_color)
    }));
    for cluster in non_space {
        assert!(
            row.glyphs[cluster.glyph_range.clone()]
                .iter()
                .all(|glyph| glyph.font_id == cluster.font_id)
        );
    }
}

#[test]
fn missing_family_and_visible_tofu_are_deduplicated_diagnostics() {
    let mut catalog = catalog(&[LATIN]);
    let config = FontConfig::new("Definitely Missing").with_fallbacks(["Noto Sans"]);
    let mut shaper = TerminalShaper::new(config);

    let first = shaper.shape_row(&mut catalog, "AΩΩ").expect("shape row");
    let second = shaper.shape_row(&mut catalog, "AΩΩ").expect("shape row");

    assert_eq!(first.clusters[0].font_family, "Noto Sans");
    assert!(first.clusters[1].is_tofu);
    assert!(first.clusters[2].is_tofu);
    assert!(first.glyphs.iter().any(|glyph| glyph.is_tofu));
    assert_eq!(first.diagnostics, second.diagnostics);
    assert_eq!(
        first
            .diagnostics
            .iter()
            .filter(|item| item.kind == DiagnosticKind::MissingFamily)
            .count(),
        1
    );
    assert_eq!(
        first
            .diagnostics
            .iter()
            .filter(|item| item.kind == DiagnosticKind::MissingCluster)
            .count(),
        1
    );
    assert_eq!(
        first
            .diagnostics
            .iter()
            .filter(|item| item.kind == DiagnosticKind::VisibleTofu)
            .count(),
        1
    );
}

#[test]
fn catalog_generation_invalidates_the_shape_cache() {
    let mut catalog = catalog(&[LATIN]);
    let mut shaper = TerminalShaper::new(fixture_config());

    let first = shaper.shape_row(&mut catalog, "abc").expect("shape row");
    let second = shaper.shape_row(&mut catalog, "abc").expect("shape row");
    assert_eq!(first, second);
    assert_eq!(shaper.cache_stats().misses, 1);
    assert_eq!(shaper.cache_stats().hits, 1);

    let generation = catalog.generation();
    catalog
        .load_source(source(CJK))
        .expect("add configured font");
    assert!(catalog.generation() > generation);

    let third = shaper.shape_row(&mut catalog, "abc").expect("shape row");
    assert_eq!(third.catalog_generation, catalog.generation());
    assert_eq!(shaper.cache_stats().misses, 2);
}

#[test]
fn diagnostic_deduplication_is_scoped_to_catalog_generation() {
    let mut catalog = catalog(&[LATIN]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));

    shaper.shape_row(&mut catalog, "Ω").expect("shape tofu");
    let first_generation = catalog.generation();
    catalog.load_source(source(CJK)).expect("reload catalog");
    let row = shaper
        .shape_row(&mut catalog, "Ω")
        .expect("shape tofu again");

    let generations: Vec<_> = row
        .diagnostics
        .iter()
        .filter(|item| item.kind == DiagnosticKind::MissingCluster)
        .map(|item| item.catalog_generation)
        .collect();
    assert_eq!(generations, [first_generation, catalog.generation()]);
}

#[test]
fn configuration_and_authoritative_spans_participate_in_shape_cache_keys() {
    let mut catalog = catalog(&[LATIN]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));

    shaper.shape_row(&mut catalog, "fi").expect("first shape");
    shaper.shape_row(&mut catalog, "fi").expect("cached shape");
    assert_eq!(
        shaper.cache_stats(),
        rssh_fonts::ShapeCacheStats { hits: 1, misses: 1 }
    );

    shaper.set_config(
        FontConfig::new("Noto Sans")
            .with_ligatures(false)
            .with_feature(*b"kern", 0)
            .with_cell_width(11.0),
    );
    shaper.shape_row(&mut catalog, "fi").expect("new config");
    assert_eq!(shaper.cache_stats().misses, 2);

    shaper
        .shape_clusters(
            &mut catalog,
            &[
                TerminalCluster::new("f", 0..2),
                TerminalCluster::new("i", 2..3),
            ],
        )
        .expect("new terminal geometry");
    assert_eq!(shaper.cache_stats().misses, 3);
}

#[test]
fn terminal_owned_cell_spans_override_unicode_width() {
    let mut catalog = catalog(&[LATIN, CJK]);
    let mut shaper =
        TerminalShaper::new(FontConfig::new("Noto Sans").with_fallbacks(["Noto Sans SC"]));
    let inputs = [
        TerminalCluster::new("A", 0..2),
        TerminalCluster::new("中", 2..3),
    ];

    let row = shaper
        .shape_clusters(&mut catalog, &inputs)
        .expect("shape terminal clusters");

    assert_eq!(row.cell_count, 3);
    assert_eq!(row.clusters[0].cell_span, 0..2);
    assert_eq!(row.clusters[1].cell_span, 2..3);
    assert_eq!(row.glyphs[0].cell_span, 0..2);
}

#[test]
fn invalid_inputs_fail_before_entering_cosmic_text() {
    let mut empty = FontCatalog::new("en-US");
    let mut normal = catalog(&[LATIN]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));
    assert_eq!(
        shaper.shape_row(&mut empty, "A"),
        Err(ShapeError::NoUsableFont)
    );
    assert_eq!(
        shaper.shape_row(&mut normal, "A\nB"),
        Err(ShapeError::EmbeddedLineBreak)
    );
    assert_eq!(
        shaper.shape_clusters(&mut normal, &[TerminalCluster::new("AB", 0..1)]),
        Err(ShapeError::InvalidCluster)
    );
    assert_eq!(
        shaper.shape_clusters(
            &mut normal,
            &[
                TerminalCluster::new("A", 0..1),
                TerminalCluster::new("B", 2..3),
            ],
        ),
        Err(ShapeError::InvalidCellSpan)
    );

    shaper.set_config(FontConfig::new("Noto Sans").with_font_size(f32::NAN));
    assert_eq!(
        shaper.shape_row(&mut normal, "A"),
        Err(ShapeError::InvalidMetrics)
    );
}

#[test]
fn empty_and_extremely_long_rows_remain_one_unwrapped_layout_line() {
    let mut catalog = catalog(&[LATIN]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));

    let empty = shaper.shape_row(&mut catalog, "").expect("shape empty row");
    assert_eq!(empty.layout_line_count, 1);
    assert!(empty.glyphs.is_empty());

    let long = "A".repeat(20_000);
    let row = shaper
        .shape_row(&mut catalog, &long)
        .expect("shape long row");
    assert_eq!(row.layout_line_count, 1);
    assert_eq!(row.cell_count, 20_000);
}
