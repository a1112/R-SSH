//! Direct native `wgpu` context and presentation diagnostics.

mod context;
mod images;
mod metrics;
mod quads;
mod render_graph;

pub use context::{
    DEFAULT_CPU_FRAME_BYTE_BUDGET, GpuContext, GpuContextError, GpuContextErrorKind,
    GpuContextGeneration, GpuContextOptions, GpuFrameStatus, RgbaFrameLayout,
};
pub use images::{GpuImage, ImageProtocol};
pub use metrics::{GpuPresentationMetrics, SurfaceFault, SurfaceRecovery, SurfaceRecoveryState};
pub use quads::{GpuLayer, GpuLayerError, GpuQuad, PixelRect, SignedPixelRect};
pub use render_graph::{
    DEFAULT_GPU_IMAGE_BYTE_BUDGET, DEFAULT_GPU_INSTANCE_BYTE_BUDGET,
    DEFAULT_GPU_READBACK_BYTE_BUDGET, GpuLayerRenderer, InstanceUploadMetrics, RenderGraph,
    TextureCacheMetrics,
};
