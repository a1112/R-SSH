/// Stable, machine-readable facts about the selected GPU and presentation path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuPresentationMetrics {
    pub backend: String,
    pub adapter_name: String,
    pub adapter_vendor_id: u32,
    pub adapter_device_id: u32,
    pub adapter_type: String,
    pub software_adapter: bool,
    pub surface_format: Option<String>,
    pub present_mode: Option<String>,
    pub surface_width: Option<u32>,
    pub surface_height: Option<u32>,
    pub rendered_frames: u64,
    pub presented_frames: u64,
    pub surface_reconfigurations: u64,
    pub surface_recreations: u64,
    pub surface_timeouts: u64,
    pub surface_occlusions: u64,
    pub surface_validation_errors: u64,
    pub compatibility_frame_uploads: u64,
    pub uncaptured_errors: u64,
    pub device_losses: u64,
    pub device_recoveries: u64,
    pub device_recovery_failures: u64,
    pub abandoned_lost_surfaces: u64,
}

impl GpuPresentationMetrics {
    pub(super) fn from_adapter(info: &wgpu::AdapterInfo) -> Self {
        Self {
            backend: info.backend.to_string(),
            adapter_name: info.name.clone(),
            adapter_vendor_id: info.vendor,
            adapter_device_id: info.device,
            adapter_type: adapter_type_name(info.device_type).to_owned(),
            software_adapter: matches!(info.device_type, wgpu::DeviceType::Cpu),
            surface_format: None,
            present_mode: None,
            surface_width: None,
            surface_height: None,
            rendered_frames: 0,
            presented_frames: 0,
            surface_reconfigurations: 0,
            surface_recreations: 0,
            surface_timeouts: 0,
            surface_occlusions: 0,
            surface_validation_errors: 0,
            compatibility_frame_uploads: 0,
            uncaptured_errors: 0,
            device_losses: 0,
            device_recoveries: 0,
            device_recovery_failures: 0,
            abandoned_lost_surfaces: 0,
        }
    }

    /// Placeholder used before a native surface is materialized.
    #[must_use]
    pub fn uninitialized() -> Self {
        Self {
            backend: "uninitialized".to_owned(),
            adapter_name: "uninitialized".to_owned(),
            adapter_vendor_id: 0,
            adapter_device_id: 0,
            adapter_type: "unknown".to_owned(),
            software_adapter: false,
            surface_format: None,
            present_mode: None,
            surface_width: None,
            surface_height: None,
            rendered_frames: 0,
            presented_frames: 0,
            surface_reconfigurations: 0,
            surface_recreations: 0,
            surface_timeouts: 0,
            surface_occlusions: 0,
            surface_validation_errors: 0,
            compatibility_frame_uploads: 0,
            uncaptured_errors: 0,
            device_losses: 0,
            device_recoveries: 0,
            device_recovery_failures: 0,
            abandoned_lost_surfaces: 0,
        }
    }
}

/// Returns whether a recovered native window surface needs the narrowly scoped
/// Windows Vulkan NVIDIA abandonment workaround during final shutdown.
#[must_use]
pub fn should_abandon_recovered_window_surface(
    os: &str,
    backend: &str,
    vendor_id: u32,
    shutdown_intent: bool,
    replaced_device: bool,
) -> bool {
    os.eq_ignore_ascii_case("windows")
        && backend.eq_ignore_ascii_case("vulkan")
        && vendor_id == 0x10de
        && shutdown_intent
        && replaced_device
}

fn adapter_type_name(device_type: wgpu::DeviceType) -> &'static str {
    match device_type {
        wgpu::DeviceType::Other => "other",
        wgpu::DeviceType::IntegratedGpu => "integrated-gpu",
        wgpu::DeviceType::DiscreteGpu => "discrete-gpu",
        wgpu::DeviceType::VirtualGpu => "virtual-gpu",
        wgpu::DeviceType::Cpu => "cpu",
    }
}

/// Surface acquisition outcome independent of the backend-specific handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceFault {
    Suboptimal,
    Timeout,
    Occluded,
    Outdated,
    Lost,
    Validation,
}

/// Recovery required for a surface acquisition fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceRecovery {
    PresentThenReconfigure,
    ReconfigureAndRetry,
    RecreateAndRetry,
    SkipFrame,
    Report,
}

impl SurfaceRecovery {
    #[must_use]
    pub const fn for_fault(fault: SurfaceFault) -> Self {
        match fault {
            SurfaceFault::Suboptimal => Self::PresentThenReconfigure,
            SurfaceFault::Outdated => Self::ReconfigureAndRetry,
            SurfaceFault::Lost => Self::RecreateAndRetry,
            SurfaceFault::Timeout | SurfaceFault::Occluded => Self::SkipFrame,
            SurfaceFault::Validation => Self::Report,
        }
    }
}

/// Per-acquisition retry budget. Reconfiguration/recreation is attempted at
/// most once so repeated faults cannot spin the window event loop.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SurfaceRecoveryState {
    retried: bool,
}

impl SurfaceRecoveryState {
    #[must_use]
    pub const fn new() -> Self {
        Self { retried: false }
    }

    pub fn action(&mut self, fault: SurfaceFault) -> SurfaceRecovery {
        let action = SurfaceRecovery::for_fault(fault);
        if matches!(
            action,
            SurfaceRecovery::ReconfigureAndRetry | SurfaceRecovery::RecreateAndRetry
        ) {
            if self.retried {
                return SurfaceRecovery::Report;
            }
            self.retried = true;
        }
        action
    }
}
