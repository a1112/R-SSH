use std::time::Duration;

use rssh_core::TerminalSize;
use rssh_renderer::gpu::{
    GpuContext, GpuContextOptions, GpuImage, GpuLayer, GpuLayerRenderer, GpuQuad, ImageProtocol,
    PixelRect, RenderGraph, SignedPixelRect,
};
use rssh_renderer::{DamageRegion, PixelRenderer, RenderGeometry, TerminalRenderSnapshot};
use rssh_terminal::Terminal;

fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> [u8; 4] {
    [red, green, blue, alpha]
}

#[test]
fn render_graph_orders_every_terminal_layer_independently_of_insertion_order() {
    let mut graph = RenderGraph::new(8, 8);
    for layer in [
        GpuLayer::Selection,
        GpuLayer::Overlay,
        GpuLayer::TabBar,
        GpuLayer::Cursor,
        GpuLayer::Strikethrough,
        GpuLayer::Underline,
        GpuLayer::PositiveImage,
        GpuLayer::Glyph,
        GpuLayer::NegativeImage,
        GpuLayer::UltraNegativeImage,
        GpuLayer::CellBackground,
        GpuLayer::PaneBackground,
    ] {
        graph.push_quad(GpuQuad::new(
            layer,
            PixelRect::new(0, 0, 1, 1),
            rgba(1, 2, 3, 255),
        ));
    }

    assert_eq!(
        graph.ordered_layers(),
        &[
            GpuLayer::PaneBackground,
            GpuLayer::UltraNegativeImage,
            GpuLayer::CellBackground,
            GpuLayer::NegativeImage,
            GpuLayer::Glyph,
            GpuLayer::PositiveImage,
            GpuLayer::Underline,
            GpuLayer::Strikethrough,
            GpuLayer::Cursor,
            GpuLayer::TabBar,
            GpuLayer::Overlay,
            GpuLayer::Selection,
        ]
    );
    assert_eq!(graph.ordered_content_layers(), graph.ordered_layers());
}

#[test]
fn kitty_iterm_and_sixel_images_share_clipping_and_stable_z_ordering() {
    let mut graph = RenderGraph::new(8, 8);
    let clip = PixelRect::new(2, 1, 4, 5);
    for (protocol, z_index, color) in [
        (ImageProtocol::Kitty, 3, rgba(1, 0, 0, 255)),
        (ImageProtocol::Iterm, -2, rgba(2, 0, 0, 255)),
        (ImageProtocol::Sixel, -2, rgba(3, 0, 0, 255)),
    ] {
        graph.push_image(
            GpuImage::new(protocol, z_index, PixelRect::new(0, 0, 8, 8), color).with_clip(clip),
        );
    }

    let images = graph.ordered_images();
    assert_eq!(
        images
            .iter()
            .map(|image| (image.protocol(), image.z_index(), image.rect()))
            .collect::<Vec<_>>(),
        vec![
            (ImageProtocol::Iterm, -2, clip),
            (ImageProtocol::Sixel, -2, clip),
            (ImageProtocol::Kitty, 3, clip),
        ]
    );
    assert_eq!(images[0].layer(), GpuLayer::NegativeImage);
    assert_eq!(images[2].layer(), GpuLayer::PositiveImage);
}

#[test]
fn extreme_negative_kitty_image_precedes_cell_background() {
    let mut graph = RenderGraph::new(2, 2);
    graph.push_quad(GpuQuad::new(
        GpuLayer::CellBackground,
        PixelRect::new(0, 0, 2, 2),
        rgba(1, 2, 3, 255),
    ));
    graph.push_image(GpuImage::new(
        ImageProtocol::Kitty,
        i32::MIN / 2 - 1,
        PixelRect::new(0, 0, 2, 2),
        rgba(4, 5, 6, 255),
    ));

    assert_eq!(
        graph.ordered_content_layers(),
        vec![GpuLayer::UltraNegativeImage, GpuLayer::CellBackground]
    );
}

#[test]
fn same_z_kitty_ids_use_cpu_tie_break_and_missing_ids_retain_insertion_order() {
    let mut graph = RenderGraph::new(2, 2);
    graph.push_image(
        GpuImage::new(
            ImageProtocol::Kitty,
            -1,
            PixelRect::new(0, 0, 1, 1),
            rgba(20, 0, 0, 255),
        )
        .with_kitty_id(20),
    );
    graph.push_image(
        GpuImage::new(
            ImageProtocol::Kitty,
            -1,
            PixelRect::new(0, 0, 1, 1),
            rgba(10, 0, 0, 255),
        )
        .with_kitty_id(10),
    );
    graph.push_image(GpuImage::new(
        ImageProtocol::Iterm,
        -1,
        PixelRect::new(0, 0, 1, 1),
        rgba(30, 0, 0, 255),
    ));

    assert_eq!(
        graph
            .ordered_images()
            .iter()
            .map(|image| image.color()[0])
            .collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
}

#[test]
fn signed_half_open_clipping_discards_negative_pixels_before_encoding() {
    let image = GpuImage::new_signed(
        ImageProtocol::Sixel,
        0,
        SignedPixelRect::new(-2, -1, 5, 4),
        rgba(1, 2, 3, 255),
    )
    .with_signed_clip(SignedPixelRect::new(0, 0, 4, 3));

    assert_eq!(image.rect(), PixelRect::new(0, 0, 3, 3));
}

#[test]
fn persistent_instance_uploads_only_the_changed_aligned_range_and_honors_budget() {
    let mut context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 256)
        .expect("bounded renderer");
    let mut graph = RenderGraph::new(4, 4);
    graph.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 4, 4),
        rgba(10, 20, 30, 255),
    ));
    graph.push_quad(GpuQuad::new(
        GpuLayer::Cursor,
        PixelRect::new(1, 1, 1, 1),
        rgba(40, 50, 60, 255),
    ));

    renderer.upload(&graph).expect("first upload");
    let first = renderer.upload_metrics();
    assert!(first.bytes_written > 0);
    assert!(first.capacity_bytes <= 256);

    renderer.upload(&graph).expect("unchanged upload");
    assert_eq!(renderer.upload_metrics().bytes_written, 0);

    graph.replace_quad(
        1,
        GpuQuad::new(
            GpuLayer::Cursor,
            PixelRect::new(1, 1, 1, 1),
            rgba(70, 80, 90, 255),
        ),
    );
    renderer.upload(&graph).expect("dirty upload");
    let dirty = renderer.upload_metrics();
    assert!(dirty.bytes_written > 0);
    assert!(dirty.bytes_written < first.bytes_written);
    assert_eq!(dirty.dirty_offset % wgpu::COPY_BUFFER_ALIGNMENT, 0);
    assert_eq!(dirty.bytes_written as u64 % wgpu::COPY_BUFFER_ALIGNMENT, 0);

    let mut oversized = RenderGraph::new(4, 4);
    for _ in 0..32 {
        oversized.push_quad(GpuQuad::new(
            GpuLayer::CellBackground,
            PixelRect::new(0, 0, 1, 1),
            rgba(0, 0, 0, 255),
        ));
    }
    let error = renderer
        .upload(&oversized)
        .expect_err("instance budget must be enforced");
    assert!(error.to_string().contains("budget"));

    context
        .run_headless_submission_probe(Duration::from_secs(5))
        .expect("writes leave device usable");
}

#[test]
fn headless_gpu_readback_matches_cpu_layering_invariants_with_tolerance() {
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096).expect("renderer");
    let mut graph = RenderGraph::new(4, 4);
    graph.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 4, 4),
        rgba(180, 0, 0, 255),
    ));
    graph.push_image(GpuImage::new(
        ImageProtocol::Kitty,
        -1,
        PixelRect::new(0, 0, 3, 3),
        rgba(0, 160, 0, 255),
    ));
    graph.push_quad(GpuQuad::new(
        GpuLayer::Glyph,
        PixelRect::new(1, 1, 1, 1),
        rgba(0, 0, 0, 0),
    ));
    graph.push_image(GpuImage::new(
        ImageProtocol::Sixel,
        1,
        PixelRect::new(1, 1, 3, 3),
        rgba(0, 0, 140, 255),
    ));
    graph.push_quad(GpuQuad::new(
        GpuLayer::Selection,
        PixelRect::new(2, 2, 1, 1),
        rgba(230, 230, 230, 255),
    ));

    let actual = renderer
        .render_headless_rgba8(&graph, Duration::from_secs(5))
        .expect("real wgpu render and readback");
    let expected = [
        rgba(0, 160, 0, 255),
        rgba(0, 160, 0, 255),
        rgba(0, 160, 0, 255),
        rgba(180, 0, 0, 255),
        rgba(0, 160, 0, 255),
        rgba(0, 0, 140, 255),
        rgba(0, 0, 140, 255),
        rgba(0, 0, 140, 255),
        rgba(0, 160, 0, 255),
        rgba(0, 0, 140, 255),
        rgba(230, 230, 230, 255),
        rgba(0, 0, 140, 255),
        rgba(180, 0, 0, 255),
        rgba(0, 0, 140, 255),
        rgba(0, 0, 140, 255),
        rgba(0, 0, 140, 255),
    ]
    .concat();
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            actual.abs_diff(expected) <= 2,
            "channel {index}: GPU={actual}, CPU reference={expected}"
        );
    }
}

#[test]
fn snapshot_fragment_plan_suppresses_parent_and_recomputes_cell_geometry() {
    let mut terminal = Terminal::new(TerminalSize::new(2, 2));
    terminal.feed(b"\x1b_Ga=T,C=1,q=1,i=77,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

    let mut one_pixel_cells = RenderGraph::new(2, 2);
    one_pixel_cells.push_snapshot_images(&snapshot, RenderGeometry::new(2, 2, 1, 1), 0, None);
    assert_eq!(
        one_pixel_cells.planned_image_draw_count(),
        4,
        "the fragmented parent must not also emit a whole-image draw"
    );

    let mut two_pixel_cells = RenderGraph::new(4, 4);
    two_pixel_cells.push_snapshot_images(&snapshot, RenderGeometry::new(4, 4, 2, 2), 0, None);
    assert_eq!(two_pixel_cells.planned_image_draw_count(), 4);
    assert_eq!(
        two_pixel_cells.planned_image_destinations(),
        vec![
            PixelRect::new(0, 0, 2, 2),
            PixelRect::new(2, 0, 2, 2),
            PixelRect::new(0, 2, 2, 2),
            PixelRect::new(2, 2, 2, 2),
        ]
    );
}

#[test]
fn decoded_fragment_textures_have_no_seams_and_are_reused_by_bounded_cache() {
    let mut terminal = Terminal::new(TerminalSize::new(2, 2));
    terminal.feed(b"\x1b_Ga=T,C=1,q=1,i=78,f=24,s=2,v=2,c=2,r=2;/wAAAP8AAAD/////\x1b\\");
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    let mut graph = RenderGraph::new(2, 2);
    graph.push_snapshot_images(&snapshot, RenderGeometry::new(2, 2, 1, 1), 0, None);

    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new_with_budgets(&context, wgpu::TextureFormat::Rgba8Unorm, 4096, 32)
            .expect("renderer");
    let actual = renderer
        .render_headless_rgba8(&graph, Duration::from_secs(5))
        .expect("decoded texture readback");
    assert_eq!(
        actual,
        [
            rgba(255, 0, 0, 255),
            rgba(0, 255, 0, 255),
            rgba(0, 0, 255, 255),
            rgba(255, 255, 255, 255),
        ]
        .concat()
    );
    let first = renderer.texture_cache_metrics();
    assert_eq!(first.retained_bytes, 32);
    assert_eq!(first.uploads, 4);

    renderer
        .render_headless_rgba8(&graph, Duration::from_secs(5))
        .expect("cached decoded texture readback");
    assert_eq!(renderer.texture_cache_metrics().uploads, first.uploads);
}

#[test]
fn texture_cache_evicts_lru_entries_without_exceeding_its_byte_budget() {
    fn one_pixel_graph(id: u32, payload: &str) -> RenderGraph {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(
            format!("\x1b_Ga=T,C=1,q=1,i={id},f=24,s=1,v=1,c=1,r=1;{payload}\x1b\\").as_bytes(),
        );
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let mut graph = RenderGraph::new(1, 1);
        graph.push_snapshot_images(&snapshot, RenderGeometry::new(1, 1, 1, 1), 0, None);
        graph
    }

    let red = one_pixel_graph(81, "/wAA");
    let green = one_pixel_graph(82, "AP8A");
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new_with_budgets(&context, wgpu::TextureFormat::Rgba8Unorm, 64, 8)
            .expect("one-pixel cache");

    for graph in [&red, &green, &red] {
        renderer
            .render_headless_rgba8(graph, Duration::from_secs(5))
            .expect("bounded cache frame");
        assert!(renderer.texture_cache_metrics().retained_bytes <= 8);
    }
    let metrics = renderer.texture_cache_metrics();
    assert_eq!(metrics.entries, 1);
    assert_eq!(metrics.uploads, 3);
    assert_eq!(metrics.evictions, 2);
}

#[test]
fn persistent_capacity_grows_geometrically_reuses_allocation_and_never_draws_stale_tail() {
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 256).expect("renderer");
    let mut one = RenderGraph::new(2, 1);
    one.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 2, 1),
        rgba(0, 200, 0, 255),
    ));
    renderer.upload(&one).expect("one instance");
    let initial_capacity = renderer.upload_metrics().capacity_bytes;

    let mut three = RenderGraph::new(2, 1);
    for (layer, color) in [
        (GpuLayer::PaneBackground, rgba(200, 0, 0, 255)),
        (GpuLayer::Cursor, rgba(0, 0, 200, 255)),
        (GpuLayer::Selection, rgba(200, 200, 200, 255)),
    ] {
        three.push_quad(GpuQuad::new(layer, PixelRect::new(1, 0, 1, 1), color));
    }
    renderer.upload(&three).expect("grow");
    let grown_capacity = renderer.upload_metrics().capacity_bytes;
    assert!(grown_capacity > initial_capacity);
    assert!(grown_capacity.is_power_of_two());

    let actual = renderer
        .render_headless_rgba8(&one, Duration::from_secs(5))
        .expect("shrunken active range");
    assert_eq!(
        actual,
        [rgba(0, 200, 0, 255), rgba(0, 200, 0, 255)].concat(),
        "instances beyond the current active count must not survive a shrink"
    );
    assert_eq!(renderer.upload_metrics().capacity_bytes, grown_capacity);
    assert!(!renderer.upload_metrics().reallocated);
}

#[test]
fn instance_budget_is_clamped_to_the_device_max_buffer_size() {
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, usize::MAX)
        .expect("device-clamped budget");
    assert!(renderer.instance_budget_bytes() as u64 <= context.device().limits().max_buffer_size);
}

#[test]
fn inline_image_alpha_replaces_cpu_style_instead_of_blending() {
    fn render_pixel(payload: &str) -> [u8; 4] {
        let mut terminal = Terminal::new(TerminalSize::new(1, 1));
        terminal.feed(format!("\x1b_Ga=T,q=1,i=90,f=32,s=1,v=1;{payload}\x1b\\").as_bytes());
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let mut graph = RenderGraph::new(1, 1);
        graph.push_quad(GpuQuad::new(
            GpuLayer::PaneBackground,
            PixelRect::new(0, 0, 1, 1),
            rgba(200, 0, 0, 255),
        ));
        graph.push_snapshot_images(&snapshot, RenderGeometry::new(1, 1, 1, 1), 0, None);
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("headless adapter");
        let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 256)
            .expect("renderer");
        let bytes = renderer
            .render_headless_rgba8(&graph, Duration::from_secs(5))
            .expect("readback");
        bytes.try_into().expect("one RGBA pixel")
    }

    assert_eq!(
        render_pixel("AAD/gA=="),
        rgba(0, 0, 255, 128),
        "nonzero image alpha is authoritative, not source-over blended"
    );
    assert_eq!(
        render_pixel("AAD/AA=="),
        rgba(200, 0, 0, 255),
        "fully transparent image pixels leave the lower layer unchanged"
    );
}

#[test]
fn same_z_fragment_group_follows_whole_group_in_both_insertion_directions() {
    const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    let mut terminal = Terminal::new(TerminalSize::new(1, 1));
    terminal.feed(b"\x1b_Ga=T,q=1,i=91,f=24,s=1,v=1;AP8A\x1b\\");
    terminal.feed(format!("\x1b]1337;File=inline=1;width=1px;height=1px:{RED_PNG}\x07").as_bytes());
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    let mut graph = RenderGraph::new(1, 1);
    graph.push_snapshot_images(&snapshot, RenderGeometry::new(1, 1, 1, 1), 0, None);
    assert_eq!(graph.planned_image_draw_count(), 2);

    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 256).expect("renderer");
    assert_eq!(
        renderer
            .render_headless_rgba8(&graph, Duration::from_secs(5),)
            .expect("same-z readback"),
        rgba(0, 255, 0, 255),
        "the Kitty placement fragment group must follow the iTerm whole group"
    );

    let mut reversed = Terminal::new(TerminalSize::new(1, 1));
    reversed.feed(format!("\x1b]1337;File=inline=1;width=1px;height=1px:{RED_PNG}\x07").as_bytes());
    reversed.feed(b"\x1b[H");
    reversed.feed(b"\x1b_Ga=T,q=1,i=91,f=24,s=1,v=1;AP8A\x1b\\");
    let reversed_snapshot = TerminalRenderSnapshot::from_terminal(&reversed);
    let mut reversed_graph = RenderGraph::new(1, 1);
    reversed_graph.push_snapshot_images(
        &reversed_snapshot,
        RenderGeometry::new(1, 1, 1, 1),
        0,
        None,
    );
    assert_eq!(
        renderer
            .render_headless_rgba8(&reversed_graph, Duration::from_secs(5))
            .expect("reverse insertion readback"),
        rgba(0, 255, 0, 255),
        "whole/fragment grouping must not depend on snapshot insertion"
    );
}

#[test]
fn mixed_whole_and_fragment_images_match_cpu_full_and_damage_on_real_gpu() {
    const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    let mut terminal = Terminal::new(TerminalSize::new(1, 1));
    terminal.feed(format!("\x1b]1337;File=inline=1;width=1px;height=1px:{RED_PNG}\x07").as_bytes());
    terminal.feed(b"\x1b[H");
    terminal.feed(b"\x1b_Ga=T,C=1,q=1,i=1,f=24,s=1,v=1,c=1,r=1;AAD/\x1b\\");
    terminal.feed(b"\x1b[?25l");
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    let geometry = RenderGeometry::new(1, 1, 1, 1);

    let mut cpu_renderer = PixelRenderer::new();
    cpu_renderer.set_default_background(rgba(8, 9, 10, 255));
    cpu_renderer.set_cursor_opacity(0.0);
    let mut cpu_full = vec![0; 4];
    cpu_renderer.render(&snapshot, &mut cpu_full, 1, 1, 1, 1);
    let mut cpu_damage = rgba(99, 98, 97, 255).to_vec();
    cpu_renderer.render_damage(
        &snapshot,
        &[DamageRegion::new(0, 0, 1, 1)],
        &mut cpu_damage,
        geometry,
    );
    assert_eq!(cpu_damage, cpu_full);

    let mut graph = RenderGraph::new(1, 1);
    graph.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 1, 1),
        rgba(8, 9, 10, 255),
    ));
    graph.push_snapshot_images(&snapshot, geometry, 0, None);
    assert_eq!(graph.planned_image_draw_count(), 2);
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut gpu_renderer =
        GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 256).expect("renderer");
    let gpu = gpu_renderer
        .render_headless_rgba8(&graph, Duration::from_secs(5))
        .expect("mixed whole/fragment real GPU readback");

    assert_eq!(cpu_full, rgba(0, 0, 255, 255));
    assert_eq!(gpu, cpu_full);
}

#[test]
fn mixed_texture_and_legacy_image_nodes_have_one_real_gpu_order() {
    const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    let geometry = RenderGeometry::new(1, 1, 1, 1);
    let mut fragment_terminal = Terminal::new(TerminalSize::new(1, 1));
    fragment_terminal.feed(b"\x1b_Ga=T,C=1,q=1,i=1,f=24,s=1,v=1,c=1,r=1;AAD/\x1b\\");
    let fragment_snapshot = TerminalRenderSnapshot::from_terminal(&fragment_terminal);
    let mut whole_terminal = Terminal::new(TerminalSize::new(1, 1));
    whole_terminal
        .feed(format!("\x1b]1337;File=inline=1;width=1px;height=1px:{RED_PNG}\x07").as_bytes());
    let whole_snapshot = TerminalRenderSnapshot::from_terminal(&whole_terminal);

    let mut graph = RenderGraph::new(1, 1);
    graph.push_snapshot_images(&fragment_snapshot, geometry, 0, None);
    graph.push_image(GpuImage::new(
        ImageProtocol::Iterm,
        0,
        PixelRect::new(0, 0, 1, 1),
        rgba(0, 255, 0, 255),
    ));
    graph.push_snapshot_images(&whole_snapshot, geometry, 0, None);

    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 256).expect("renderer");
    assert_eq!(
        renderer
            .render_headless_rgba8(&graph, Duration::from_secs(5))
            .expect("mixed GraphNode readback"),
        rgba(0, 0, 255, 255),
        "the fragment group must follow both legacy and texture whole-image nodes"
    );
}

#[test]
fn gpu_image_materialization_budget_errors_before_mutating_renderer_state() {
    const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    let mut terminal = Terminal::new(TerminalSize::new(1, 1));
    terminal.feed(format!("\x1b]1337;File=inline=1;width=2px;height=2px:{RED_PNG}\x07").as_bytes());
    terminal.feed(b"\x1b[H");
    terminal.feed(b"\x1b_Ga=T,C=1,q=1,i=1,f=24,s=1,v=1,c=1,r=1;AAD/\x1b\\");
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    let mut graph = RenderGraph::new(2, 2);
    graph.push_snapshot_images(&snapshot, RenderGeometry::new(2, 2, 2, 2), 0, None);
    assert_eq!(graph.planned_image_draw_count(), 2);

    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new_with_budgets(&context, wgpu::TextureFormat::Rgba8Unorm, 256, 40)
            .expect("renderer");
    let upload_before = renderer.upload_metrics();
    let cache_before = renderer.texture_cache_metrics();
    let error = renderer
        .upload(&graph)
        .expect_err("two 2x2 draws require 64 retained bytes");
    assert!(error.to_string().contains("budget"));
    assert_eq!(renderer.upload_metrics(), upload_before);
    assert_eq!(renderer.texture_cache_metrics(), cache_before);
}

#[test]
fn every_adjacent_layer_pair_is_submitted_in_canonical_order() {
    fn push_layer(graph: &mut RenderGraph, layer: GpuLayer, rect: PixelRect, color: [u8; 4]) {
        match layer {
            GpuLayer::UltraNegativeImage => graph.push_image(GpuImage::new(
                ImageProtocol::Kitty,
                i32::MIN / 2 - 1,
                rect,
                color,
            )),
            GpuLayer::NegativeImage => {
                graph.push_image(GpuImage::new(ImageProtocol::Kitty, -1, rect, color));
            }
            GpuLayer::PositiveImage => {
                graph.push_image(GpuImage::new(ImageProtocol::Kitty, 0, rect, color));
            }
            _ => graph.push_quad(GpuQuad::new(layer, rect, color)),
        }
    }

    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 4096).expect("renderer");
    for pair in GpuLayer::canonical_order().windows(2) {
        let lower = pair[0];
        let upper = pair[1];
        let mut graph = RenderGraph::new(2, 1);
        // Reverse insertion proves the explicit graph order is authoritative.
        push_layer(
            &mut graph,
            upper,
            PixelRect::new(1, 0, 1, 1),
            rgba(0, 0, 220, 255),
        );
        push_layer(
            &mut graph,
            lower,
            PixelRect::new(0, 0, 2, 1),
            rgba(220, 0, 0, 255),
        );
        assert_eq!(graph.ordered_content_layers(), vec![lower, upper]);
        let actual = renderer
            .render_headless_rgba8(&graph, Duration::from_secs(5))
            .expect("adjacent layer readback");
        let lower_pixel = if lower == GpuLayer::Glyph {
            rgba(0, 0, 0, 0)
        } else {
            rgba(220, 0, 0, 255)
        };
        let upper_pixel = if upper == GpuLayer::Glyph {
            lower_pixel
        } else {
            rgba(0, 0, 220, 255)
        };
        assert_eq!(
            actual,
            [lower_pixel, upper_pixel].concat(),
            "incorrect GPU ordering for {lower:?} -> {upper:?}"
        );
    }
}

#[test]
fn renderer_rejects_a_foreign_device_and_queue_before_mutating_upload_state() {
    let context_a = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("context A");
    let mut context_b = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("context B");
    let mut renderer = GpuLayerRenderer::new(&context_a, wgpu::TextureFormat::Rgba8Unorm, 256)
        .expect("renderer A");
    let mut graph = RenderGraph::new(1, 1);
    graph.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 1, 1),
        rgba(1, 2, 3, 255),
    ));
    let before = renderer.upload_metrics();
    let error = renderer
        .upload_from(&context_b, &graph)
        .expect_err("foreign context must be rejected");
    assert!(error.to_string().contains("different GPU context"));
    assert_eq!(renderer.upload_metrics(), before);
    context_b
        .run_headless_submission_probe(Duration::from_secs(5))
        .expect("rejection must not poison the foreign device");
}

#[test]
fn headless_readback_rejects_device_and_host_resource_limits_before_gpu_creation() {
    let mut context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 256).expect("renderer");
    let max_dimension = context.device().limits().max_texture_dimension_2d;
    let oversized = RenderGraph::new(max_dimension.saturating_add(1), 1);
    assert!(
        renderer
            .render_headless_rgba8(&oversized, Duration::from_secs(5),)
            .expect_err("max dimension must be validated")
            .to_string()
            .contains("limit")
    );
    let budget_square = RenderGraph::new(max_dimension, max_dimension);
    assert!(
        renderer
            .render_headless_rgba8(&budget_square, Duration::from_secs(5),)
            .expect_err("host readback budget must be validated")
            .to_string()
            .contains("budget")
    );
    context
        .run_headless_submission_probe(Duration::from_secs(5))
        .expect("preflight rejection must not create an uncaptured GPU fault");
}

#[test]
fn non_power_of_two_instance_budget_accepts_legal_active_bytes_at_boundary() {
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer = GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 100)
        .expect("100-byte renderer");
    let mut graph = RenderGraph::new(1, 1);
    for _ in 0..3 {
        graph.push_quad(GpuQuad::new(
            GpuLayer::CellBackground,
            PixelRect::new(0, 0, 1, 1),
            rgba(0, 0, 0, 255),
        ));
    }
    renderer
        .upload(&graph)
        .expect("96 active bytes fit a 100-byte budget");
    graph.push_quad(GpuQuad::new(
        GpuLayer::CellBackground,
        PixelRect::new(0, 0, 1, 1),
        rgba(0, 0, 0, 255),
    ));
    assert!(
        renderer
            .upload(&graph)
            .expect_err("128 active bytes exceed a 100-byte budget")
            .to_string()
            .contains("budget")
    );
}

#[test]
fn instance_budget_counts_only_visible_non_glyph_draws() {
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let mut renderer =
        GpuLayerRenderer::new(&context, wgpu::TextureFormat::Rgba8Unorm, 64).expect("renderer");

    let mut glyphs = RenderGraph::new(1, 1);
    for _ in 0..3 {
        glyphs.push_quad(GpuQuad::new(
            GpuLayer::Glyph,
            PixelRect::new(0, 0, 1, 1),
            rgba(1, 2, 3, 255),
        ));
    }
    renderer
        .upload(&glyphs)
        .expect("reserved glyph slots consume no Task 16 instances");
    assert_eq!(renderer.upload_metrics().bytes_written, 0);

    let mut offscreen = RenderGraph::new(1, 1);
    for _ in 0..3 {
        offscreen.push_quad(GpuQuad::new(
            GpuLayer::CellBackground,
            PixelRect::new(10, 10, 1, 1),
            rgba(1, 2, 3, 255),
        ));
    }
    renderer
        .upload(&offscreen)
        .expect("clipped nodes consume no instance budget");
    assert_eq!(renderer.upload_metrics().bytes_written, 0);
}
