//! Direct native `wgpu` context and presentation diagnostics.

mod context;
mod images;
mod metrics;
mod quads;
mod render_graph;
mod text;

pub use context::{
    DEFAULT_CPU_FRAME_BYTE_BUDGET, GpuContext, GpuContextError, GpuContextErrorKind,
    GpuContextGeneration, GpuContextOptions, GpuFrameStatus, RgbaFrameLayout,
    WindowedGpuContextBootstrap, WindowedGpuDevice,
};
pub use images::{GpuImage, ImageProtocol};
pub use metrics::{
    GpuInitializationResourceSnapshot, GpuInitializationStage, GpuPresentationMetrics,
    SurfaceFault, SurfaceRecovery, SurfaceRecoveryState, should_abandon_recovered_window_surface,
};
pub use quads::{GpuLayer, GpuLayerError, GpuQuad, PixelRect, SignedPixelRect};
pub(crate) use render_graph::TextureIdentity;
pub use render_graph::{
    DEFAULT_GPU_IMAGE_BYTE_BUDGET, DEFAULT_GPU_INSTANCE_BYTE_BUDGET,
    DEFAULT_GPU_READBACK_BYTE_BUDGET, GpuLayerRenderer, InstanceUploadMetrics, RenderGraph,
    TextureCacheMetrics,
};
pub use text::{GpuTextAtlasMetrics, GpuTextConfig, GpuTextPrepareReport};
