//! Direct native `wgpu` context and presentation diagnostics.

mod context;
mod metrics;

pub use context::{GpuContext, GpuContextError, GpuContextOptions, GpuFrameStatus};
pub use metrics::{GpuPresentationMetrics, SurfaceFault, SurfaceRecovery, SurfaceRecoveryState};
