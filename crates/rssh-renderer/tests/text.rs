use std::fs;
use std::path::{Path, PathBuf};

use rssh_core::TerminalSize;
use rssh_fonts::{FontCatalog, FontConfig, FontSource, RasterCacheConfig};
use rssh_renderer::{
    CpuTextRenderer, DamageRegion, PixelRenderer, RenderCell, RenderGeometry,
    TerminalRenderSnapshot, TextBackend,
};
use rssh_terminal::{Color, CursorShape, Terminal, UnderlineStyle, VerticalAlign};

const LATIN: &str = "NotoSans-Latin.fixture.ttf";
const CJK: &str = "NotoSansSC-CJK.fixture.ttf";
const ARABIC: &str = "NotoSansArabic.fixture.ttf";
const DEVANAGARI: &str = "NotoSansDevanagari.fixture.ttf";
const HEBREW: &str = "NotoSansHebrew.fixture.ttf";
const EMOJI: &str = "NotoColorEmoji.fixture.ttf";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fonts")
}

fn source(name: &str) -> FontSource {
    FontSource::new(
        name,
        fs::read(fixture_dir().join(name)).expect("read deterministic fixture"),
    )
}

fn font_config(ligatures: bool) -> FontConfig {
    FontConfig::new("Noto Sans")
        .with_fallbacks([
            "Noto Sans SC",
            "Noto Sans Arabic",
            "Noto Sans Devanagari",
            "Noto Sans Hebrew",
            "Noto Color Emoji",
        ])
        .with_font_size(16.0)
        .with_line_height(1.0)
        .with_cell_width(1.0)
        .with_ligatures(ligatures)
}

fn cpu_text(ligatures: bool) -> CpuTextRenderer {
    let catalog = FontCatalog::from_sources(
        "en-US",
        [LATIN, CJK, ARABIC, DEVANAGARI, HEBREW, EMOJI]
            .into_iter()
            .map(source),
    )
    .expect("load isolated fixture catalog");
    CpuTextRenderer::new(
        catalog,
        font_config(ligatures),
        RasterCacheConfig::new(4 * 1024 * 1024),
    )
}

fn snapshot(text: &str, columns: u16) -> TerminalRenderSnapshot {
    let mut terminal = Terminal::new(TerminalSize::new(columns, 1));
    terminal.feed(b"\x1b[?25l");
    terminal.feed(text.as_bytes());
    TerminalRenderSnapshot::from_terminal(&terminal)
}

fn render(
    renderer: &PixelRenderer,
    text_renderer: &mut CpuTextRenderer,
    snapshot: &TerminalRenderSnapshot,
    columns: u32,
) -> Vec<u8> {
    let geometry = RenderGeometry::new(columns * 16, 24, 16, 24);
    let mut target = vec![0; geometry.target_width as usize * 24 * 4];
    renderer.render_shaped(text_renderer, snapshot, &mut target, geometry);
    target
}

fn non_background_pixels(target: &[u8]) -> usize {
    target
        .chunks_exact(4)
        .filter(|pixel| *pixel != [12, 12, 12, 255])
        .count()
}

fn overlay_cell(column: u16, text: &str, foreground: Color, background: Color) -> RenderCell {
    RenderCell {
        row: 0,
        column,
        text: text.to_owned(),
        columns: 1,
        continuation: false,
        ch: text.chars().next().unwrap_or(' '),
        foreground,
        background,
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
    }
}

#[test]
fn text_backends_are_explicit_and_observable() {
    let renderer = PixelRenderer::new();
    assert_eq!(renderer.text_backend(), TextBackend::BitmapEmergency);
    assert_eq!(cpu_text(true).text_backend(), TextBackend::Shaped);
}

#[test]
fn immutable_snapshot_builds_authoritative_terminal_clusters() {
    let snapshot = snapshot("A中e\u{301}", 5);
    let clusters = snapshot.terminal_clusters_for_row(0, 5);

    assert_eq!(clusters[0].text, "A");
    assert_eq!(clusters[0].cell_span, 0..1);
    assert_eq!(clusters[1].text, "中");
    assert_eq!(clusters[1].cell_span, 1..3);
    assert_eq!(clusters[2].text, "e\u{301}");
    assert_eq!(clusters[2].cell_span, 3..4);
    assert_eq!(clusters[3].text, " ");
    assert_eq!(clusters[3].cell_span, 4..5);
}

#[test]
fn shaped_path_rasterizes_ligatures_cjk_and_complex_scripts() {
    for (text, columns) in [
        ("fi", 2),
        ("中", 2),
        ("سلاّم", 5),
        ("क्षि", 1),
        ("e\u{301}", 1),
    ] {
        let renderer = PixelRenderer::new();
        let mut text_renderer = cpu_text(true);
        let snapshot = snapshot(text, columns);
        let target = render(&renderer, &mut text_renderer, &snapshot, u32::from(columns));
        let report = text_renderer.last_report().expect("shaped report");

        assert!(
            non_background_pixels(&target) > 0,
            "{text:?} drew no pixels"
        );
        assert!(
            report.rasterized_glyphs > 0,
            "{text:?} rasterized no glyphs"
        );
        assert!(
            report
                .cluster_bounds
                .iter()
                .all(|cluster| cluster.pixel_bounds.x < u32::from(columns) * 16),
            "{text:?} escaped its terminal row"
        );
    }
}

#[test]
fn ligature_configuration_changes_glyph_count_without_changing_cell_bounds() {
    let snapshot = snapshot("fi", 2);
    let renderer = PixelRenderer::new();
    let mut enabled_renderer = cpu_text(true);
    render(&renderer, &mut enabled_renderer, &snapshot, 2);
    let enabled = enabled_renderer.last_report().expect("enabled report");
    let mut disabled_renderer = cpu_text(false);
    render(&renderer, &mut disabled_renderer, &snapshot, 2);
    let disabled = disabled_renderer.last_report().expect("disabled report");

    assert_eq!(enabled.shaped_glyphs, 1);
    assert_eq!(disabled.shaped_glyphs, 2);
    assert_eq!(enabled.cluster_bounds.first().unwrap().cell_span.start, 0);
    assert_eq!(enabled.cluster_bounds.last().unwrap().cell_span.end, 2);
}

#[test]
fn mask_glyphs_use_terminal_foreground_and_color_emoji_preserves_rgba() {
    let latin = TerminalRenderSnapshot::from_terminal(&{
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25l\x1b[31mA");
        terminal
    });
    let renderer = PixelRenderer::new();
    let mut text_renderer = cpu_text(true);
    let latin_target = render(&renderer, &mut text_renderer, &latin, 1);
    assert!(
        latin_target.chunks_exact(4).any(|pixel| {
            pixel[0] > pixel[1] && pixel[0] > pixel[2] && pixel != [12, 12, 12, 255]
        })
    );

    let emoji = snapshot("😀", 2);
    let emoji_target = render(&renderer, &mut text_renderer, &emoji, 2);
    let report = text_renderer.last_report().expect("emoji report");
    assert!(report.color_glyphs > 0);
    assert!(
        emoji_target
            .chunks_exact(4)
            .any(|pixel| { pixel[3] > 0 && (pixel[0] != pixel[1] || pixel[1] != pixel[2]) })
    );
}

#[test]
fn missing_cluster_draws_visible_mono_tofu() {
    let catalog =
        FontCatalog::from_sources("en-US", [source(LATIN)]).expect("load latin-only catalog");
    let cpu = CpuTextRenderer::new(
        catalog,
        FontConfig::new("Noto Sans").with_font_size(16.0),
        RasterCacheConfig::new(64 * 1024),
    );
    let renderer = PixelRenderer::new();
    let mut cpu = cpu;

    let target = render(&renderer, &mut cpu, &snapshot("Ω", 1), 1);
    let report = cpu.last_report().expect("tofu report");
    assert_eq!(report.fallback_glyphs, 1);
    assert!(non_background_pixels(&target) > 0);
}

#[test]
fn style_selection_cursor_and_ime_overlay_split_shape_runs() {
    let base = snapshot("fi", 4);
    let selected = base.clone().with_selection_colors_overlay(
        |_, column| column == 1,
        Some(Some(Color::Rgb(255, 255, 255))),
        Some(Color::Rgb(20, 40, 80)),
    );
    let renderer = PixelRenderer::new();
    let mut text_renderer = cpu_text(true);
    render(&renderer, &mut text_renderer, &selected, 4);
    let selection_report = text_renderer.last_report().expect("selection report");
    assert_eq!(
        selection_report.shaped_glyphs, 2,
        "selection boundary must prevent a cross-style fi ligature"
    );

    let preedit = selected.with_overlay_cells([
        overlay_cell(2, "k", Color::Rgb(255, 255, 255), Color::Rgb(20, 20, 20)),
        overlay_cell(3, "a", Color::Rgb(255, 255, 255), Color::Rgb(20, 20, 20)),
    ]);
    let target = render(&renderer, &mut text_renderer, &preedit, 4);
    assert!(non_background_pixels(&target) > 0);
}

#[test]
fn damage_conservatively_repaints_the_full_dirty_row() {
    let renderer = PixelRenderer::new();
    let mut text_renderer = cpu_text(true);
    let geometry = RenderGeometry::new(32, 24, 16, 24);
    let old = snapshot("fi", 2);
    let mut damaged = render(&renderer, &mut text_renderer, &old, 2);

    let new = snapshot("X", 2);
    renderer.render_damage_shaped(
        &mut text_renderer,
        &new,
        &[DamageRegion::new(0, 0, 1, 1)],
        &mut damaged,
        geometry,
    );
    assert_eq!(
        text_renderer
            .last_report()
            .expect("damage report")
            .expanded_damage,
        [DamageRegion::new(0, 0, 2, 1)]
    );
    let expected = render(&renderer, &mut text_renderer, &new, 2);

    assert_eq!(
        damaged, expected,
        "old pixels from the second ligature cell must be cleared"
    );
}

#[test]
fn block_cursor_remains_above_shaped_text() {
    let mut terminal = Terminal::new(TerminalSize::new(1, 1));
    terminal.feed(b"A\x1b[1D\x1b[2 q");
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    assert_eq!(snapshot.cursor().unwrap().shape, CursorShape::Block);
    let renderer = PixelRenderer::new();
    let mut text_renderer = cpu_text(true);

    let target = render(&renderer, &mut text_renderer, &snapshot, 1);
    assert!(non_background_pixels(&target) > 0);
}

#[test]
fn rtl_cluster_bounds_use_visual_positions_and_mixed_selection_keeps_one_bidi_paragraph() {
    let renderer = PixelRenderer::new();
    let mut text_renderer = cpu_text(true);
    let rtl = snapshot("אבג", 3);
    render(&renderer, &mut text_renderer, &rtl, 3);
    let rtl_report = text_renderer.last_report().expect("rtl report");
    let rtl_x = rtl_report
        .cluster_bounds
        .iter()
        .map(|cluster| cluster.pixel_bounds.x)
        .collect::<Vec<_>>();
    assert_eq!(rtl_x, [32, 16, 0]);

    let mixed = snapshot("A אבג B", 7).with_selection_colors_overlay(
        |_, column| column == 2,
        Some(Some(Color::Rgb(255, 255, 255))),
        Some(Color::Rgb(40, 60, 80)),
    );
    let pixels = render(&renderer, &mut text_renderer, &mixed, 7);
    let report = text_renderer.last_report().expect("mixed report");
    let hebrew = &report.cluster_bounds[2..5];
    assert!(hebrew[0].pixel_bounds.x > hebrew[1].pixel_bounds.x);
    assert!(hebrew[1].pixel_bounds.x > hebrew[2].pixel_bounds.x);
    assert!(report.shape_runs > 1);
    let pixel = |x: usize, y: usize| {
        let offset = (y * 7 * 16 + x) * 4;
        &pixels[offset..offset + 4]
    };
    assert_eq!(pixel(4 * 16, 23), [40, 60, 80, 255]);
    assert_ne!(pixel(2 * 16, 23), [40, 60, 80, 255]);
}

#[test]
fn block_cursor_redraws_the_shaped_mask_with_cursor_foreground() {
    let mut terminal = Terminal::new(TerminalSize::new(1, 1));
    terminal.feed(b"A\x1b[1D\x1b[2 q");
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    let mut renderer = PixelRenderer::new();
    renderer.set_default_cursor_color([0, 0, 255, 255]);
    renderer.set_default_cursor_foreground(Some([255, 0, 0, 255]));
    let mut text_renderer = cpu_text(true);

    let target = render(&renderer, &mut text_renderer, &snapshot, 1);
    assert!(
        target
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 200 && pixel[1] < 50 && pixel[2] < 50),
        "cursor foreground must redraw the shaped A above its blue block"
    );
}

#[test]
fn faint_and_blink_opacity_dim_color_emoji_without_foreground_tinting() {
    let renderer = PixelRenderer::new();
    let mut text_renderer = cpu_text(true);
    let normal = snapshot("😀", 2);
    let faint = TerminalRenderSnapshot::from_terminal(&{
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed("\u{1b}[?25l\u{1b}[2m😀".as_bytes());
        terminal
    });

    let normal = render(&renderer, &mut text_renderer, &normal, 2);
    let faint = render(&renderer, &mut text_renderer, &faint, 2);
    let brightness = |pixels: &[u8]| {
        pixels
            .chunks_exact(4)
            .map(|pixel| u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]))
            .sum::<u64>()
    };
    assert!(brightness(&faint) < brightness(&normal));
    assert!(
        faint
            .chunks_exact(4)
            .any(|pixel| { pixel[0] != pixel[1] || pixel[1] != pixel[2] })
    );

    let blinking = TerminalRenderSnapshot::from_terminal(&{
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed("\u{1b}[?25l\u{1b}[5m😀".as_bytes());
        terminal
    });
    let invisible_blink = render(
        &PixelRenderer::with_text_blink_opacity(0.0),
        &mut text_renderer,
        &blinking,
        2,
    );
    assert_eq!(non_background_pixels(&invisible_blink), 0);
}

#[test]
fn bold_and_italic_cells_reach_shaping_attributes() {
    let styled = TerminalRenderSnapshot::from_terminal(&{
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"\x1b[?25l\x1b[1mA\x1b[22;3mB");
        terminal
    });
    let renderer = PixelRenderer::new();
    let mut text_renderer = cpu_text(true);

    render(&renderer, &mut text_renderer, &styled, 2);
    let report = text_renderer.last_report().expect("styled report");
    assert_eq!(report.bold_glyphs, 1);
    assert_eq!(report.italic_glyphs, 1);
}

#[test]
fn orphan_continuation_is_projected_as_a_blank_without_span_gaps() {
    let orphan = RenderCell {
        continuation: true,
        text: String::new(),
        columns: 0,
        ..overlay_cell(1, " ", Color::Default, Color::Default)
    };
    let snapshot = snapshot("", 3).with_overlay_cells([orphan]);

    let clusters = snapshot.terminal_clusters_for_row(0, 3);
    assert_eq!(
        clusters
            .iter()
            .map(|cluster| cluster.cell_span.clone())
            .collect::<Vec<_>>(),
        [0..1, 1..2, 2..3]
    );
    assert!(clusters.iter().all(|cluster| cluster.text == " "));
}

#[test]
fn shaped_text_keeps_negative_images_below_and_positive_images_above() {
    fn with_image(z: i32) -> TerminalRenderSnapshot {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(b"\x1b[?25lA\x1b[1;1H");
        terminal.take_damage();
        terminal.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        terminal.feed(format!("\x1b_Ga=p,i=7,z={z}\x1b\\").as_bytes());
        TerminalRenderSnapshot::from_terminal(&terminal)
    }

    let renderer = PixelRenderer::new();
    let mut text_renderer = cpu_text(true);
    let below = render(&renderer, &mut text_renderer, &with_image(-1), 1);
    assert!(below.chunks_exact(4).any(|pixel| pixel == [255, 0, 0, 255]));
    assert!(below.chunks_exact(4).any(|pixel| pixel != [255, 0, 0, 255]));

    let above = render(&renderer, &mut text_renderer, &with_image(1), 1);
    assert!(above.chunks_exact(4).all(|pixel| pixel == [255, 0, 0, 255]));
}
