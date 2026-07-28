use std::{error::Error, io, sync::Arc};

use rssh_renderer::gpu::{
    DEFAULT_CPU_FRAME_BYTE_BUDGET, GpuContext, GpuContextOptions, GpuFrameStatus, GpuLayer,
    GpuLayerRenderer, GpuPresentationMetrics, GpuQuad, GpuTextConfig, GpuTextPrepareReport,
    PixelRect, RenderGraph, RgbaFrameLayout,
};
use rssh_renderer::{RenderGeometry, TerminalRenderSnapshot, TextPaintConfig};
use winit::{dpi::PhysicalSize, event_loop::ActiveEventLoop, window::Window};

/// App-owned compatibility bridge from the existing CPU framebuffer to the
/// direct native wgpu surface.
pub(crate) struct WindowGpu {
    context: GpuContext,
    frame: Vec<u8>,
    frame_width: u32,
    frame_height: u32,
    direct_text: Option<Box<DirectGpuText>>,
}

struct DirectGpuText {
    renderer: GpuLayerRenderer,
    graph: RenderGraph,
    report: GpuTextPrepareReport,
    rendered_frames: u64,
}

impl WindowGpu {
    pub(crate) async fn new(
        event_loop: &ActiveEventLoop,
        window: Arc<Window>,
        surface_size: PhysicalSize<u32>,
        frame_width: u32,
        frame_height: u32,
        high_performance: bool,
        force_fallback_adapter: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let options = GpuContextOptions::default()
            .with_high_performance(high_performance)
            .with_force_fallback_adapter(force_fallback_adapter);
        let context = GpuContext::new_windowed(
            event_loop.owned_display_handle(),
            window,
            surface_size.width,
            surface_size.height,
            options,
        )
        .await?;
        let frame = allocate_frame(
            frame_width,
            frame_height,
            context.max_texture_dimension_2d(),
        )?;
        #[cfg(debug_assertions)]
        let direct_text = if std::env::var_os("RSSH_TEST_DIRECT_GPU_TEXT").is_some() {
            Some(Box::new(direct_text_fixture(
                &context,
                surface_size.width,
                surface_size.height,
            )?))
        } else {
            None
        };
        #[cfg(not(debug_assertions))]
        let direct_text = None;
        Ok(Self {
            context,
            frame,
            frame_width,
            frame_height,
            direct_text,
        })
    }

    pub(crate) fn frame_mut(&mut self) -> &mut [u8] {
        &mut self.frame
    }

    pub(crate) fn resize_surface(&mut self, size: PhysicalSize<u32>) -> Result<(), Box<dyn Error>> {
        self.context.resize_surface(size.width, size.height)?;
        #[cfg(debug_assertions)]
        if self.direct_text.is_some() && size.width > 0 && size.height > 0 {
            self.direct_text = Some(Box::new(direct_text_fixture(
                &self.context,
                size.width,
                size.height,
            )?));
        }
        Ok(())
    }

    pub(crate) fn resize_frame(&mut self, width: u32, height: u32) -> Result<(), Box<dyn Error>> {
        if self.frame_width == width && self.frame_height == height {
            return Ok(());
        }
        self.frame = allocate_frame(width, height, self.context.max_texture_dimension_2d())?;
        self.frame_width = width;
        self.frame_height = height;
        Ok(())
    }

    pub(crate) fn present(&mut self, window: &Window) -> Result<GpuFrameStatus, Box<dyn Error>> {
        let status = if let Some(direct) = self.direct_text.as_mut() {
            self.context
                .render_graph(&mut direct.renderer, &direct.graph, || {
                    window.pre_present_notify();
                })?
        } else {
            self.context
                .render_rgba(&self.frame, self.frame_width, self.frame_height, || {
                    window.pre_present_notify();
                })?
        };
        if status == GpuFrameStatus::Presented
            && let Some(direct) = self.direct_text.as_mut()
        {
            direct.rendered_frames = direct.rendered_frames.saturating_add(1);
        }
        Ok(status)
    }

    pub(crate) fn metrics(&self) -> &GpuPresentationMetrics {
        self.context.metrics()
    }

    pub(crate) fn direct_text_metrics(&self) -> Option<(&GpuTextPrepareReport, u64)> {
        self.direct_text
            .as_ref()
            .map(|direct| (&direct.report, direct.rendered_frames))
    }
}

#[cfg(debug_assertions)]
fn direct_text_fixture(
    context: &GpuContext,
    width: u32,
    height: u32,
) -> Result<DirectGpuText, Box<dyn Error>> {
    use std::{fs, path::Path};

    use rssh_core::TerminalSize;
    use rssh_fonts::{FontCatalog, FontConfig, FontSource, RasterCacheConfig};
    use rssh_terminal::Terminal;

    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fonts");
    let source = |name: &str| -> Result<FontSource, io::Error> {
        Ok(FontSource::new(name, fs::read(fixture_dir.join(name))?))
    };
    let catalog = FontCatalog::from_sources(
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
        .map(source)
        .collect::<Result<Vec<_>, _>>()?,
    )?;
    let font_config = FontConfig::new("Noto Sans")
        .with_fallbacks([
            "Noto Sans SC",
            "Noto Sans Arabic",
            "Noto Sans Devanagari",
            "Noto Sans Hebrew",
            "Noto Color Emoji",
        ])
        .with_font_size(16.0);
    let format = context
        .surface_format()
        .ok_or_else(|| io::Error::other("direct text fixture requires a surface format"))?;
    let mut renderer = GpuLayerRenderer::new(context, format, 64 * 1024)?;
    renderer.enable_text(
        catalog,
        font_config,
        GpuTextConfig::new(4 * 1024 * 1024, RasterCacheConfig::new(4 * 1024 * 1024)),
    )?;
    let columns = u16::try_from((width / 16).max(1)).unwrap_or(u16::MAX);
    let mut terminal = Terminal::new(TerminalSize::new(columns, 1));
    terminal.feed(b"\x1b[?25l");
    terminal.feed("office 中 مرحبا नमस्ते שלום 😀 █".as_bytes());
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    let geometry = RenderGeometry::new(width, height, 16, 24);
    let report = renderer.prepare_text(
        &snapshot,
        geometry,
        &[],
        &TextPaintConfig::default(),
        1.0,
        1.0,
    )?;
    let mut graph = RenderGraph::new(width, height);
    graph.push_quad(GpuQuad::new(
        GpuLayer::PaneBackground,
        PixelRect::new(0, 0, width, height),
        [12, 12, 12, 255],
    ));
    Ok(DirectGpuText {
        renderer,
        graph,
        report,
        rendered_frames: 0,
    })
}

fn allocate_frame(
    width: u32,
    height: u32,
    max_texture_dimension_2d: u32,
) -> Result<Vec<u8>, io::Error> {
    let layout = RgbaFrameLayout::new(
        width,
        height,
        max_texture_dimension_2d,
        DEFAULT_CPU_FRAME_BYTE_BUDGET,
    )
    .map_err(io::Error::other)?;
    let mut frame = Vec::new();
    frame.try_reserve_exact(layout.byte_len).map_err(|error| {
        io::Error::other(format!("compatibility framebuffer allocation: {error}"))
    })?;
    frame.resize(layout.byte_len, 0);
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    #[test]
    fn compatibility_frame_allocation_accepts_4k_and_rejects_oversized_requests() {
        let frame = allocate_frame(3_840, 2_160, 8_192).expect("4K frame fits the budget");
        assert_eq!(frame.len(), 33_177_600);

        let oversized = catch_unwind(AssertUnwindSafe(|| allocate_frame(8_193, 8_192, 16_384)))
            .expect("oversized frame allocation must not panic");
        assert!(oversized.is_err());
    }
}
