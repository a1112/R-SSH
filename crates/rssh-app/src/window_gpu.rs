use std::{error::Error, io, sync::Arc};

use rssh_renderer::gpu::{GpuContext, GpuContextOptions, GpuFrameStatus, GpuPresentationMetrics};
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
        let frame = allocate_frame(frame_width, frame_height)?;
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
        self.frame = allocate_frame(width, height)?;
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

fn allocate_frame(width: u32, height: u32) -> Result<Vec<u8>, io::Error> {
    let len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| io::Error::other("compatibility framebuffer size overflow"))?;
    Ok(vec![0; len])
}
