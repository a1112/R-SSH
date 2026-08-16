use std::{error::Error, io, num::NonZeroU32, sync::Arc};

use winit::{
    dpi::PhysicalSize,
    event_loop::{ActiveEventLoop, OwnedDisplayHandle},
    window::Window,
};

#[derive(Debug, Default)]
pub(crate) struct CpuStagingBuffer {
    width: u32,
    height: u32,
    pub(crate) packed: Vec<u32>,
}

impl CpuStagingBuffer {
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        let pixels = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .unwrap_or_default();
        self.packed.resize(pixels, 0);
    }
}

#[cfg(test)]
fn pack_rgba_to_softbuffer(rgba: &[u8]) -> Vec<u32> {
    rgba.chunks_exact(4)
        .map(|pixel| (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]))
        .collect()
}

#[derive(Debug)]
pub(crate) struct WindowBootstrapSurface {
    _context: softbuffer::Context<OwnedDisplayHandle>,
    surface: softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>,
    staging: CpuStagingBuffer,
    size: PhysicalSize<u32>,
}

impl WindowBootstrapSurface {
    pub(crate) fn new(
        event_loop: &ActiveEventLoop,
        window: Arc<Window>,
        size: PhysicalSize<u32>,
    ) -> Result<Self, Box<dyn Error>> {
        let width = NonZeroU32::new(size.width)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zero bootstrap width"))?;
        let height = NonZeroU32::new(size.height)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zero bootstrap height"))?;
        let display = event_loop.owned_display_handle();
        let context = softbuffer::Context::new(display.clone())?;
        let mut surface = softbuffer::Surface::new(&context, window)?;
        surface.resize(width, height)?;
        let mut staging = CpuStagingBuffer::default();
        staging.resize(size.width, size.height);
        Ok(Self {
            _context: context,
            surface,
            staging,
            size,
        })
    }

    pub(crate) fn resize(&mut self, size: PhysicalSize<u32>) -> Result<(), Box<dyn Error>> {
        let width = NonZeroU32::new(size.width)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zero bootstrap width"))?;
        let height = NonZeroU32::new(size.height)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zero bootstrap height"))?;
        self.surface.resize(width, height)?;
        self.staging.resize(size.width, size.height);
        self.size = size;
        Ok(())
    }

    pub(crate) fn present_rgba(&mut self, rgba: &[u8]) -> Result<(), Box<dyn Error>> {
        let expected = usize::try_from(self.size.width)
            .ok()
            .and_then(|width| {
                usize::try_from(self.size.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "bootstrap size overflow")
            })?;
        if rgba.len() < expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RGBA bootstrap frame is smaller than the surface",
            )
            .into());
        }
        for (packed, pixel) in self
            .staging
            .packed
            .iter_mut()
            .zip(rgba[..expected].chunks_exact(4))
        {
            *packed =
                (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
        }
        let mut buffer = self.surface.buffer_mut()?;
        buffer.copy_from_slice(&self.staging.packed);
        buffer.present()?;
        Ok(())
    }

    pub(crate) fn size(&self) -> PhysicalSize<u32> {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_rgba_pixels_into_softbuffer_order() {
        let rgba = [
            0x11, 0x22, 0x33, 0xff, // red, green, blue, alpha
            0xaa, 0xbb, 0xcc, 0x00,
        ];
        assert_eq!(
            pack_rgba_to_softbuffer(&rgba),
            vec![0x0011_2233, 0x00aa_bbcc]
        );
    }

    #[test]
    fn staging_buffer_is_reused_for_same_size_and_cleared_on_resize() {
        let mut staging = CpuStagingBuffer::default();
        staging.resize(2, 1);
        let first = staging.packed.as_ptr();
        staging.resize(2, 1);
        assert_eq!(staging.packed.as_ptr(), first);
        staging.resize(1, 2);
        assert_eq!(staging.packed.len(), 2);
    }
}
