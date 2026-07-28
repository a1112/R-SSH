use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use rssh_core::TerminalSize;
use rssh_fonts::{FontCatalog, FontConfig, FontSource, RasterCacheConfig};
use rssh_renderer::{
    DamageRegion, RenderGeometry, TerminalRenderSnapshot,
    gpu::{
        GpuContext, GpuContextOptions, GpuLayer, GpuLayerRenderer, GpuQuad, GpuTextConfig,
        PixelRect, RenderGraph,
    },
};
use rssh_terminal::{CursorShape, Terminal};

static GPU_TEST_LOCK: Mutex<()> = Mutex::new(());

fn gpu_test_guard() -> MutexGuard<'static, ()> {
    GPU_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fonts")
}

fn source(name: &str) -> FontSource {
    FontSource::new(
        name,
        fs::read(fixture_dir().join(name)).expect("read deterministic font fixture"),
    )
}

fn catalog() -> FontCatalog {
    FontCatalog::from_sources(
        "en-US",
        [
            "NotoSans-Latin.fixture.ttf",
            "NotoSansSC-CJK.fixture.ttf",
            "NotoSansArabic.fixture.ttf",
            "NotoSansDevanagari.fixture.ttf",
            "NotoSansHebrew.fixture.ttf",
            "NotoColorEmoji.fixture.ttf",
        ]
        .into_iter()
        .map(source),
    )
    .expect("load isolated fixture catalog")
}

fn font_config() -> FontConfig {
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
}

fn snapshot(text: &str, columns: u16) -> TerminalRenderSnapshot {
    let mut terminal = Terminal::new(TerminalSize::new(columns, 1));
    terminal.feed(b"\x1b[?25l");
    terminal.feed(text.as_bytes());
    TerminalRenderSnapshot::from_terminal(&terminal)
}

fn multiline_snapshot(text: &str, columns: u16, rows: u16) -> TerminalRenderSnapshot {
    let mut terminal = Terminal::new(TerminalSize::new(columns, rows));
    terminal.feed(b"\x1b[?25l");
    terminal.feed(text.as_bytes());
    TerminalRenderSnapshot::from_terminal(&terminal)
}

fn config(atlas_budget_bytes: usize) -> GpuTextConfig {
    GpuTextConfig::new(
        atlas_budget_bytes,
        RasterCacheConfig::new(atlas_budget_bytes),
    )
}

#[test]
fn prepared_gpu_text_uses_shaped_runs_and_mask_and_color_atlas_entries() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096)
        .expect("layer renderer");
    renderer
        .enable_text(catalog(), font_config(), config(4 * 1024 * 1024))
        .expect("enable GPU text");

    let report = renderer
        .prepare_text(
            &snapshot("office 中 مرحبا नमस्ते שלום 😀 █", 48),
            RenderGeometry::new(48 * 16, 24, 16, 24),
            &[],
            1.0,
            1.0,
        )
        .expect("prepare shaped GPU text");

    assert!(report.shaped_glyphs > 0);
    assert!(report.mask_glyphs > 0);
    assert!(report.color_glyphs > 0);
    assert!(report.custom_block_glyphs > 0);
    assert_eq!(report.second_shape_calls, 0);
    renderer
        .upload(&RenderGraph::new(48 * 16, 24))
        .expect("upload generated block glyph quad");
    assert_eq!(renderer.upload_metrics().bytes_written, 32);
}

#[test]
fn atlas_is_bounded_and_repack_retries_at_most_once() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096)
        .expect("layer renderer");
    let error = renderer
        .enable_text(catalog(), font_config(), config(16 * 1024))
        .expect_err("budget below the two initial physical textures must fail");
    assert!(
        error
            .to_string()
            .contains("initial mask and color textures")
    );

    renderer
        .enable_text(catalog(), font_config(), config(340 * 1024))
        .expect("enable minimally bounded GPU text");

    let error = renderer
        .prepare_text(
            &snapshot(
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789中🧑‍💻",
                80,
            ),
            RenderGeometry::new(80 * 32, 48, 32, 48),
            &[],
            2.0,
            1.5,
        )
        .expect_err("tiny atlas budget must fail in a controlled way");

    assert!(error.to_string().contains("glyph atlas budget"));
    let metrics = renderer.text_atlas_metrics().expect("text metrics");
    assert!(metrics.retained_bytes <= metrics.budget_bytes);
    assert_eq!(
        metrics.physical_texture_bytes,
        metrics.mask_dimension as usize * metrics.mask_dimension as usize
            + metrics.color_dimension as usize * metrics.color_dimension as usize * 4
    );
    assert!(metrics.repack_attempts <= 1);
}

#[test]
fn repeated_frame_reuses_payloads_and_physical_atlas_without_growth() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096)
        .expect("layer renderer");
    renderer
        .enable_text(catalog(), font_config(), config(4 * 1024 * 1024))
        .expect("enable GPU text");
    let snapshot = snapshot("office 中 😀", 16);
    let geometry = RenderGeometry::new(16 * 16, 24, 16, 24);

    renderer
        .prepare_text(&snapshot, geometry, &[], 1.0, 1.0)
        .expect("first prepare");
    let first = renderer.text_atlas_metrics().expect("first metrics");
    renderer
        .prepare_text(&snapshot, geometry, &[], 1.0, 1.0)
        .expect("repeat prepare");
    let repeated = renderer.text_atlas_metrics().expect("repeat metrics");

    assert_eq!(repeated.uploads, first.uploads);
    assert_eq!(
        repeated.physical_texture_bytes,
        first.physical_texture_bytes
    );
    assert_eq!(repeated.entries, first.entries);
    assert_eq!(repeated.repack_attempts, 0);
}

#[test]
fn scale_and_font_generation_rebuild_the_complete_atlas_scope() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096)
        .expect("layer renderer");
    renderer
        .enable_text(catalog(), font_config(), config(4 * 1024 * 1024))
        .expect("enable GPU text");
    let snapshot = snapshot("scope", 8);
    let geometry = RenderGeometry::new(8 * 16, 24, 16, 24);

    renderer
        .prepare_text(&snapshot, geometry, &[], 1.0, 1.0)
        .expect("initial prepare");
    let initial = renderer.text_atlas_metrics().expect("initial metrics");
    renderer
        .prepare_text(&snapshot, geometry, &[], 2.0, 1.0)
        .expect("DPI prepare");
    let dpi = renderer.text_atlas_metrics().expect("DPI metrics");
    assert_eq!(dpi.scope_generation, initial.scope_generation + 1);

    renderer
        .text_catalog_mut()
        .expect("text catalog")
        .load_source(source("NotoSansHebrew.fixture.ttf"))
        .expect("advance catalog generation");
    renderer
        .prepare_text(&snapshot, geometry, &[], 2.0, 1.0)
        .expect("font generation prepare");
    let font = renderer.text_atlas_metrics().expect("font metrics");
    assert_eq!(font.scope_generation, dpi.scope_generation + 1);
}

#[test]
fn damaged_cell_expands_to_full_shaped_run_and_clips_to_half_open_terminal_bounds() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096)
        .expect("layer renderer");
    renderer
        .enable_text(catalog(), font_config(), config(4 * 1024 * 1024))
        .expect("enable GPU text");

    let report = renderer
        .prepare_text(
            &snapshot("office中", 8),
            RenderGeometry::new(8 * 16, 24, 16, 24),
            &[DamageRegion::new(2, 0, 1, 1)],
            1.0,
            1.0,
        )
        .expect("prepare damaged shaped run");

    assert_eq!(report.prepared_rows, vec![0]);
    assert_eq!(report.damage_bounds, vec![PixelRect::new(0, 0, 8 * 16, 24)]);
    assert!(
        report
            .glyph_bounds
            .iter()
            .all(|bounds| bounds.x + bounds.width <= 8 * 16 && bounds.y + bounds.height <= 24)
    );
}

#[test]
fn local_row_damage_preserves_cached_glyphs_on_untouched_rows() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096)
        .expect("layer renderer");
    renderer
        .enable_text(catalog(), font_config(), config(4 * 1024 * 1024))
        .expect("enable GPU text");
    let snapshot = multiline_snapshot("A\r\nB", 4, 2);
    let geometry = RenderGeometry::new(64, 48, 16, 24);
    renderer
        .prepare_text(&snapshot, geometry, &[], 1.0, 1.0)
        .expect("prepare complete frame");
    let report = renderer
        .prepare_text(
            &snapshot,
            geometry,
            &[DamageRegion::new(0, 0, 1, 1)],
            1.0,
            1.0,
        )
        .expect("prepare only first damaged row");
    assert_eq!(report.prepared_rows, vec![0]);

    let mut graph = RenderGraph::new(64, 48);
    graph.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 64, 48),
        [0, 0, 0, 255],
    ));
    let pixels = renderer
        .render_headless_rgba8(&graph, Duration::from_secs(5))
        .expect("render cached rows");
    assert!(
        pixels[64 * 24 * 4..]
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0),
        "row 1 must remain in glyphon's complete prepared batch after row 0 damage"
    );
}

#[test]
fn ordinary_glyph_overhang_survives_inside_a_run_but_not_across_a_hard_boundary() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096)
        .expect("layer renderer");
    renderer
        .enable_text(catalog(), font_config(), config(4 * 1024 * 1024))
        .expect("enable GPU text");
    let geometry = RenderGeometry::new(32, 24, 16, 24);

    let overhang = renderer
        .prepare_text(&snapshot("Aj", 2), geometry, &[], 1.0, 1.0)
        .expect("prepare unbroken run");
    assert!(
        overhang
            .glyph_bounds
            .iter()
            .any(|bounds| bounds.x < 16 && bounds.x + bounds.width > 16),
        "the second-cell j must retain its legal left overhang within one shaped run"
    );

    let hard_boundary = snapshot("\x1b[31mA\x1b[39mj", 2);
    let clipped = renderer
        .prepare_text(&hard_boundary, geometry, &[], 1.0, 1.0)
        .expect("prepare style-separated runs");
    assert!(
        clipped
            .glyph_bounds
            .iter()
            .all(|bounds| bounds.x >= 16 || bounds.x + bounds.width <= 16),
        "a style/hard boundary must prevent glyph coverage from leaking into the adjacent run: {:?}",
        clipped.glyph_bounds
    );
}

#[test]
fn block_cursor_foreground_redraw_is_clipped_to_one_visual_cell() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096)
        .expect("layer renderer");
    renderer
        .enable_text(
            catalog(),
            font_config(),
            config(4 * 1024 * 1024).with_cursor_foreground([255, 0, 0, 255]),
        )
        .expect("enable GPU text");
    let mut terminal = Terminal::new(TerminalSize::new(2, 1));
    terminal.feed("中\x1b[2D\x1b[2 q".as_bytes());
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    assert_eq!(
        snapshot.cursor().expect("visible cursor").shape,
        CursorShape::Block
    );
    let geometry = RenderGeometry::new(32, 24, 16, 24);
    let report = renderer
        .prepare_text(&snapshot, geometry, &[], 1.0, 1.0)
        .expect("prepare cursor foreground");
    assert!(report.cursor_foreground_glyphs > 0);
    assert!(
        report
            .cursor_foreground_bounds
            .iter()
            .all(|bounds| bounds.x + bounds.width <= 16),
        "wide glyph cursor redraw must use the one-cell visual cursor clip: {:?}",
        report.cursor_foreground_bounds
    );

    let mut graph = RenderGraph::new(32, 24);
    graph.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 32, 24),
        [0, 0, 0, 255],
    ));
    graph.push_quad(GpuQuad::new(
        GpuLayer::Cursor,
        PixelRect::new(0, 0, 16, 24),
        [0, 0, 255, 255],
    ));
    let pixels = renderer
        .render_headless_rgba8(&graph, Duration::from_secs(5))
        .expect("render cursor redraw after cursor quad");
    let is_red = |pixel: &[u8]| pixel[0] > 200 && pixel[1] < 50 && pixel[2] < 50;
    assert!((0..24).any(|y| {
        pixels[(y * 32) * 4..(y * 32 + 16) * 4]
            .chunks_exact(4)
            .any(is_red)
    }));
    assert!(
        !(0..24).any(|y| pixels[(y * 32 + 16) * 4..(y * 32 + 32) * 4]
            .chunks_exact(4)
            .any(is_red)),
        "cursor foreground redraw must not leak into the second half of a wide glyph"
    );
}

#[test]
fn real_gpu_renders_glyphs_at_the_reserved_layer_slot() {
    let _gpu = gpu_test_guard();
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 64 * 1024)
        .expect("layer renderer");
    renderer
        .enable_text(catalog(), font_config(), config(4 * 1024 * 1024))
        .expect("enable GPU text");
    let geometry = RenderGeometry::new(64, 24, 16, 24);
    renderer
        .prepare_text(&snapshot("A", 4), geometry, &[], 1.0, 1.0)
        .expect("prepare glyph");

    let mut graph = RenderGraph::new(64, 24);
    graph.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 64, 24),
        [0, 0, 0, 255],
    ));
    let pixels = renderer
        .render_headless_rgba8(&graph, Duration::from_secs(5))
        .expect("render real GPU text");

    assert!(
        pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0)
    );
}
