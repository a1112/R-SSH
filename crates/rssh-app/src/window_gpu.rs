use std::{error::Error, io, sync::Arc};

use rssh_renderer::gpu::{
    DEFAULT_CPU_FRAME_BYTE_BUDGET, GpuContext, GpuContextOptions, GpuFrameStatus,
    GpuPresentationMetrics, RgbaFrameLayout,
};
use winit::{dpi::PhysicalSize, event_loop::ActiveEventLoop, window::Window};

/// App-owned compatibility bridge from the existing CPU framebuffer to the
/// direct native wgpu surface.
pub(crate) struct WindowGpu {
    context: GpuContext,
    frame: Vec<u8>,
    frame_width: u32,
    frame_height: u32,
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
        Ok(Self {
            context,
            frame,
            frame_width,
            frame_height,
        })
    }

    pub(crate) fn frame_mut(&mut self) -> &mut [u8] {
        &mut self.frame
    }

    pub(crate) fn resize_surface(&mut self, size: PhysicalSize<u32>) -> Result<(), Box<dyn Error>> {
        self.context.resize_surface(size.width, size.height)?;
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
        Ok(self
            .context
            .render_rgba(&self.frame, self.frame_width, self.frame_height, || {
                window.pre_present_notify();
            })?)
    }

    pub(crate) fn metrics(&self) -> &GpuPresentationMetrics {
        self.context.metrics()
    }
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
