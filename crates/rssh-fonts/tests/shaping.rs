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

fn fixture_bytes(file: &str) -> Vec<u8> {
    fs::read(fixture_dir().join(file)).expect("read fixture")
}

fn sfnt_table(bytes: &[u8], wanted: [u8; 4]) -> Option<(usize, usize)> {
    let table_count = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
    (0..table_count).find_map(|index| {
        let record = 12 + index * 16;
        (bytes[record..record + 4] == wanted).then(|| {
            let offset = u32::from_be_bytes(
                bytes[record + 8..record + 12]
                    .try_into()
                    .expect("table offset"),
            ) as usize;
            let length = u32::from_be_bytes(
                bytes[record + 12..record + 16]
                    .try_into()
                    .expect("table length"),
            ) as usize;
            (offset, length)
        })
    })
}

fn with_weight(mut bytes: Vec<u8>, weight: u16) -> Vec<u8> {
    let (offset, _) = sfnt_table(&bytes, *b"OS/2").expect("OS/2 table");
    bytes[offset + 4..offset + 6].copy_from_slice(&weight.to_be_bytes());
    bytes
}

fn without_gsub_and_with_family(mut bytes: Vec<u8>, family: &str, replacement: &str) -> Vec<u8> {
    assert_eq!(
        family.encode_utf16().count(),
        replacement.encode_utf16().count()
    );
    let family_utf16: Vec<_> = family.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let replacement_utf16: Vec<_> = replacement
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let mut replaced = 0;
    for offset in 0..=bytes.len() - family_utf16.len() {
        if bytes[offset..].starts_with(&family_utf16) {
            bytes[offset..offset + family_utf16.len()].copy_from_slice(&replacement_utf16);
            replaced += 1;
        }
    }
    assert!(replaced > 0, "fixture must contain the UTF-16 family name");

    let table_count = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
    let gsub_record = (0..table_count)
        .map(|index| 12 + index * 16)
        .find(|record| &bytes[*record..*record + 4] == b"GSUB")
        .expect("GSUB table");
    bytes[gsub_record..gsub_record + 4].copy_from_slice(b"XSUB");
    bytes
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
fn configured_weight_selects_the_matching_face_within_one_family() {
    let regular = FontSource::new("regular", fixture_bytes(LATIN));
    let bold = FontSource::new("bold", with_weight(fixture_bytes(LATIN), 700));
    let mut catalog =
        FontCatalog::from_sources("en-US", [regular, bold]).expect("load two family faces");

    let regular = TerminalShaper::new(FontConfig::new("Noto Sans").with_weight(400))
        .shape_row(&mut catalog, "A")
        .expect("shape regular");
    let bold = TerminalShaper::new(FontConfig::new("Noto Sans").with_weight(700))
        .shape_row(&mut catalog, "A")
        .expect("shape bold");

    assert!(!regular.clusters[0].is_tofu);
    assert!(!bold.clusters[0].is_tofu);
    assert_ne!(
        regular.clusters[0].font_id, bold.clusters[0].font_id,
        "weight must select a concrete face before whole-cluster planning"
    );
    assert!(bold.glyphs.iter().all(|glyph| glyph.glyph_id != 0));
}

#[test]
fn catalog_faces_outside_the_configured_chain_cannot_leak_into_glyphs() {
    let mut catalog = catalog(&[LATIN, CJK]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));

    let row = shaper
        .shape_row(&mut catalog, "中")
        .expect("shape uncovered cluster");

    assert!(row.clusters[0].is_tofu);
    assert_eq!(row.clusters[0].font_family, "Noto Sans");
    assert!(
        row.glyphs[row.clusters[0].glyph_range.clone()]
            .iter()
            .all(|glyph| glyph.font_id == row.clusters[0].font_id && glyph.is_tofu),
        "cosmic-text fallback must not render an unconfigured catalog face"
    );
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
fn multi_glyph_graphemes_preserve_relative_cosmic_positions_and_widths() {
    let mut catalog = catalog(&[DEVANAGARI]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans Devanagari"));

    let row = shaper
        .shape_clusters(&mut catalog, &[TerminalCluster::new("क्षि", 0..1)])
        .expect("shape complex grapheme");
    assert!(row.glyphs.len() >= 2);
    assert!(
        row.glyphs
            .windows(2)
            .any(|pair| (pair[0].shaping_x - pair[1].shaping_x).abs() > f32::EPSILON)
    );
    assert!(
        row.glyphs
            .windows(2)
            .any(|pair| (pair[0].x - pair[1].x).abs() > f32::EPSILON),
        "terminal snapping must preserve intra-cluster offsets"
    );
    assert!(
        row.glyphs
            .iter()
            .any(|glyph| glyph.width < row.metrics.cell_width),
        "each glyph width must come from its scaled layout width, not the whole EGC"
    );
}

#[test]
fn non_emoji_devanagari_zwj_cluster_allows_multiple_valid_glyphs() {
    let mut catalog = catalog(&[DEVANAGARI]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans Devanagari"));

    let row = shaper
        .shape_clusters(&mut catalog, &[TerminalCluster::new("क्‍ष", 0..1)])
        .expect("shape Devanagari ZWJ conjunct");
    let glyphs = &row.glyphs[row.clusters[0].glyph_range.clone()];

    assert!(!row.clusters[0].is_tofu);
    assert!(!glyphs.is_empty());
    assert!(
        glyphs
            .iter()
            .all(|glyph| glyph.glyph_id != 0 && !glyph.is_tofu)
    );
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
fn pure_rtl_visual_order_follows_layout_positions_not_logical_vec_order() {
    let mut catalog = catalog(&[ARABIC]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans Arabic"));

    let row = shaper.shape_row(&mut catalog, "لا").expect("shape RTL row");

    assert_eq!(row.visual_clusters, [1, 0]);
    assert_eq!(row.clusters[0].cell_span, 0..1);
    assert_eq!(row.clusters[1].cell_span, 1..2);
    assert!(row.clusters[1].visual_index < row.clusters[0].visual_index);
    assert!(row.glyphs[0].x <= row.glyphs[1].x);
}

#[test]
fn uncovered_hebrew_tofu_preserves_backend_rtl_order_and_levels() {
    let mut catalog = catalog(&[LATIN, HEBREW]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));

    let row = shaper
        .shape_row(&mut catalog, "אבג")
        .expect("shape uncovered Hebrew");

    assert_eq!(row.visual_clusters, [2, 1, 0]);
    assert_eq!(
        row.clusters
            .iter()
            .map(|cluster| cluster.cell_span.clone())
            .collect::<Vec<_>>(),
        [0..1, 1..2, 2..3]
    );
    assert!(row.clusters.iter().all(|cluster| cluster.is_tofu));
    assert!(
        row.clusters
            .iter()
            .all(|cluster| cluster.bidi_level % 2 == 1)
    );
    assert!(row.glyphs.iter().all(|glyph| glyph.bidi_level % 2 == 1));
    assert!(row.glyphs.windows(2).all(|pair| pair[0].x <= pair[1].x));
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
    for cluster in &non_space {
        let actual = &row.glyphs[cluster.glyph_range.clone()];
        assert!(!actual.is_empty());
        assert!(actual.iter().all(|glyph| glyph.font_id == cluster.font_id
            && glyph.glyph_id != 0
            && !glyph.is_tofu));
    }
    for sequence in ["👍🏽", "👨‍👩‍👧‍👦", "🇺🇸"] {
        let cluster = non_space
            .iter()
            .find(|cluster| &text[cluster.byte_range.clone()] == sequence)
            .expect("sequence cluster");
        assert_eq!(
            row.glyphs[cluster.glyph_range.clone()].len(),
            1,
            "fixture GSUB must resolve {sequence} as one actual glyph"
        );
    }
}

#[test]
fn broken_emoji_sequence_primary_retries_the_next_configured_family() {
    let broken =
        without_gsub_and_with_family(fixture_bytes(EMOJI), "Noto Color Emoji", "Bad! Color Emoji");
    let mut catalog = FontCatalog::from_sources(
        "en-US",
        [
            FontSource::new("broken emoji primary", broken),
            source(EMOJI),
        ],
    )
    .expect("load broken and valid emoji faces");
    let mut shaper = TerminalShaper::new(
        FontConfig::new("Bad! Color Emoji").with_fallbacks(["Noto Color Emoji"]),
    );

    for sequence in ["👍🏽", "👨‍👩‍👧‍👦", "🇺🇸", "1️⃣"] {
        let row = shaper
            .shape_row(&mut catalog, sequence)
            .expect("shape emoji sequence");
        assert_eq!(row.clusters[0].font_family, "Noto Color Emoji");
        assert!(!row.clusters[0].is_tofu);
        let glyphs = &row.glyphs[row.clusters[0].glyph_range.clone()];
        assert_eq!(glyphs.len(), 1, "{sequence} must resolve as one glyph");
        assert!(glyphs.iter().all(|glyph| glyph.glyph_id != 0));
    }
}

#[test]
fn exhausted_emoji_sequence_candidates_collapse_to_one_atomic_tofu() {
    let broken =
        without_gsub_and_with_family(fixture_bytes(EMOJI), "Noto Color Emoji", "Bad! Color Emoji");
    let mut catalog = FontCatalog::from_sources("en-US", [FontSource::new("broken emoji", broken)])
        .expect("load broken emoji face");
    let mut shaper = TerminalShaper::new(FontConfig::new("Bad! Color Emoji"));

    for sequence in ["👍🏽", "👨‍👩‍👧‍👦", "🇺🇸", "1️⃣"] {
        let row = shaper
            .shape_row(&mut catalog, sequence)
            .expect("shape uncovered emoji sequence");
        let cluster = &row.clusters[0];
        let glyphs = &row.glyphs[cluster.glyph_range.clone()];
        assert!(cluster.is_tofu);
        assert_eq!(glyphs.len(), 1, "{sequence} must be one atomic tofu");
        assert_eq!(glyphs[0].glyph_id, 0);
        assert!(glyphs[0].is_tofu);
        assert_eq!(glyphs[0].cluster_range, 0..1);
        assert_eq!(glyphs[0].byte_range, cluster.byte_range);
        assert_eq!(glyphs[0].cell_span, cluster.cell_span);
    }
}

#[test]
fn default_ignorable_only_cluster_is_not_reported_as_missing() {
    let mut catalog = catalog(&[LATIN]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));

    let row = shaper
        .shape_clusters(&mut catalog, &[TerminalCluster::new("\u{200d}", 0..1)])
        .expect("shape default ignorable");

    assert!(!row.clusters[0].is_tofu);
    assert!(row.glyphs.iter().all(|glyph| !glyph.is_tofu));
    assert!(
        row.diagnostics
            .iter()
            .all(|item| item.kind != DiagnosticKind::MissingCluster)
    );
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
    let first_font = first.clusters[0].font_id;
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
    assert_ne!(third.clusters[0].font_id, first_font);
    assert_eq!(
        third.clusters[0].font_id.catalog_incarnation(),
        catalog.incarnation()
    );
    assert_eq!(
        third.clusters[0].font_id.catalog_generation(),
        catalog.generation()
    );
    assert_eq!(shaper.cache_stats().misses, 2);
}

#[test]
fn equal_generations_from_different_catalog_contents_do_not_share_cache_entries() {
    let mut latin_only = catalog(&[LATIN]);
    let mut latin_and_cjk = catalog(&[LATIN, CJK]);
    assert_eq!(latin_only.generation(), latin_and_cjk.generation());
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));

    shaper
        .shape_row(&mut latin_only, "A")
        .expect("shape first catalog");
    shaper
        .shape_row(&mut latin_and_cjk, "A")
        .expect("shape second catalog");

    assert_eq!(shaper.cache_stats().hits, 0);
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
    assert_ne!(catalog.generation(), first_generation);
    assert_eq!(generations, [catalog.generation()]);
}

#[test]
fn diagnostics_are_bounded_and_only_describe_the_current_row() {
    let mut catalog = catalog(&[LATIN]);
    let mut shaper = TerminalShaper::new(FontConfig::new("Noto Sans"));
    let missing: String = (0x1000..0x1200).filter_map(char::from_u32).collect();

    let noisy = shaper
        .shape_row(&mut catalog, &missing)
        .expect("shape many missing clusters");
    assert!(noisy.diagnostics.len() <= 128);
    assert!(
        noisy
            .diagnostics
            .iter()
            .all(|item| item.catalog_generation == catalog.generation())
    );

    let clean = shaper
        .shape_row(&mut catalog, "A")
        .expect("shape clean next row");
    assert!(clean.diagnostics.is_empty());
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
    shaper.set_config(FontConfig::new("Noto Sans").with_font_size(f32::MAX));
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
    assert!(
        row.linear_index_steps <= 12 * (row.text.len() + row.glyphs.len() + row.clusters.len()),
        "instrumented index-building passes must stay linear: {} steps",
        row.linear_index_steps
    );
}
