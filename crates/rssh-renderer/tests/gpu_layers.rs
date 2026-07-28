use std::time::Duration;

use rssh_core::TerminalSize;
use rssh_renderer::gpu::{
    GpuContext, GpuContextOptions, GpuImage, GpuLayer, GpuLayerRenderer, GpuQuad, ImageProtocol,
    PixelRect, RenderGraph, SignedPixelRect,
};
use rssh_renderer::{RenderGeometry, TerminalRenderSnapshot};
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
    let mut renderer =
        GpuLayerRenderer::new(context.device(), wgpu::TextureFormat::Rgba8Unorm, 256)
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

    renderer
        .upload(context.queue(), &graph)
        .expect("first upload");
    let first = renderer.upload_metrics();
    assert!(first.bytes_written > 0);
    assert!(first.capacity_bytes <= 256);

    renderer
        .upload(context.queue(), &graph)
        .expect("unchanged upload");
    assert_eq!(renderer.upload_metrics().bytes_written, 0);

    graph.replace_quad(
        1,
        GpuQuad::new(
            GpuLayer::Cursor,
            PixelRect::new(1, 1, 1, 1),
            rgba(70, 80, 90, 255),
        ),
    );
    renderer
        .upload(context.queue(), &graph)
        .expect("dirty upload");
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
        .upload(context.queue(), &oversized)
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
        GpuLayerRenderer::new(context.device(), wgpu::TextureFormat::Rgba8Unorm, 4096)
            .expect("renderer");
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
        .render_headless_rgba8(
            context.device(),
            context.queue(),
            &graph,
            Duration::from_secs(5),
        )
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
    let mut renderer = GpuLayerRenderer::new_with_budgets(
        context.device(),
        wgpu::TextureFormat::Rgba8Unorm,
        4096,
        16,
    )
    .expect("renderer");
    let actual = renderer
        .render_headless_rgba8(
            context.device(),
            context.queue(),
            &graph,
            Duration::from_secs(5),
        )
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
    assert_eq!(first.retained_bytes, 16);
    assert_eq!(first.uploads, 4);

    renderer
        .render_headless_rgba8(
            context.device(),
            context.queue(),
            &graph,
            Duration::from_secs(5),
        )
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
    let mut renderer = GpuLayerRenderer::new_with_budgets(
        context.device(),
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        4,
    )
    .expect("one-pixel cache");

    for graph in [&red, &green, &red] {
        renderer
            .render_headless_rgba8(
                context.device(),
                context.queue(),
                graph,
                Duration::from_secs(5),
            )
            .expect("bounded cache frame");
        assert!(renderer.texture_cache_metrics().retained_bytes <= 4);
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
        GpuLayerRenderer::new(context.device(), wgpu::TextureFormat::Rgba8Unorm, 256)
            .expect("renderer");
    let mut one = RenderGraph::new(2, 1);
    one.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, 2, 1),
        rgba(0, 200, 0, 255),
    ));
    renderer
        .upload(context.queue(), &one)
        .expect("one instance");
    let initial_capacity = renderer.upload_metrics().capacity_bytes;

    let mut three = RenderGraph::new(2, 1);
    for (layer, color) in [
        (GpuLayer::PaneBackground, rgba(200, 0, 0, 255)),
        (GpuLayer::Cursor, rgba(0, 0, 200, 255)),
        (GpuLayer::Selection, rgba(200, 200, 200, 255)),
    ] {
        three.push_quad(GpuQuad::new(layer, PixelRect::new(1, 0, 1, 1), color));
    }
    renderer.upload(context.queue(), &three).expect("grow");
    let grown_capacity = renderer.upload_metrics().capacity_bytes;
    assert!(grown_capacity > initial_capacity);
    assert!(grown_capacity.is_power_of_two());

    let actual = renderer
        .render_headless_rgba8(
            context.device(),
            context.queue(),
            &one,
            Duration::from_secs(5),
        )
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
    let renderer = GpuLayerRenderer::new(
        context.device(),
        wgpu::TextureFormat::Rgba8Unorm,
        usize::MAX,
    )
    .expect("device-clamped budget");
    assert!(renderer.instance_budget_bytes() as u64 <= context.device().limits().max_buffer_size);
}
