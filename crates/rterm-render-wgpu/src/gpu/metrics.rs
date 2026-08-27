/// Exact owner boundary reached by the staged windowed GPU initializer.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GpuInitializationStage {
    InstanceSurface,
    AdapterDevice,
    ConfiguredSurfaceClear,
    LayerPipelines,
}

impl GpuInitializationStage {
    pub const ORDERED: [Self; 4] = [
        Self::InstanceSurface,
        Self::AdapterDevice,
        Self::ConfiguredSurfaceClear,
        Self::LayerPipelines,
    ];
}

/// Project-owned resources materialized by the R-Term GPU initializer.
///
/// Driver allocations are deliberately excluded. Every field describes an
/// object or explicit byte allocation owned by this crate.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GpuInitializationResourceSnapshot {
    pub instance_count: u64,
    pub surface_count: u64,
    pub adapter_count: u64,
    pub device_count: u64,
    pub queue_count: u64,
    pub surface_configure_count: u64,
    pub surface_acquire_count: u64,
    pub clear_present_count: u64,
    pub pipeline_count: u64,
    pub pipeline_layout_count: u64,
    pub materialized_buffer_count: u64,
    pub instance_buffer_bytes: u64,
    pub upload_buffer_bytes: u64,
    pub total_allocated_buffer_bytes: u64,
    pub total_allocated_texture_bytes: u64,
    pub glyph_atlas_bytes: u64,
    pub raster_cache_bytes: u64,
    pub image_texture_bytes: u64,
    pub base_text_renderer_materialization_count: u64,
    pub cursor_text_renderer_materialization_count: u64,
    pub backend: Option<String>,
    pub adapter_name: Option<String>,
}

impl GpuInitializationResourceSnapshot {
    /// Merges disjoint owner facts into one cumulative snapshot.
    #[must_use]
    pub fn merged(mut self, owned: &Self) -> Self {
        self.instance_count = self.instance_count.saturating_add(owned.instance_count);
        self.surface_count = self.surface_count.saturating_add(owned.surface_count);
        self.adapter_count = self.adapter_count.saturating_add(owned.adapter_count);
        self.device_count = self.device_count.saturating_add(owned.device_count);
        self.queue_count = self.queue_count.saturating_add(owned.queue_count);
        self.surface_configure_count = self
            .surface_configure_count
            .saturating_add(owned.surface_configure_count);
        self.surface_acquire_count = self
            .surface_acquire_count
            .saturating_add(owned.surface_acquire_count);
        self.clear_present_count = self
            .clear_present_count
            .saturating_add(owned.clear_present_count);
        self.pipeline_count = self.pipeline_count.saturating_add(owned.pipeline_count);
        self.pipeline_layout_count = self
            .pipeline_layout_count
            .saturating_add(owned.pipeline_layout_count);
        self.materialized_buffer_count = self
            .materialized_buffer_count
            .saturating_add(owned.materialized_buffer_count);
        self.instance_buffer_bytes = self
            .instance_buffer_bytes
            .saturating_add(owned.instance_buffer_bytes);
        self.upload_buffer_bytes = self
            .upload_buffer_bytes
            .saturating_add(owned.upload_buffer_bytes);
        self.total_allocated_buffer_bytes = self
            .total_allocated_buffer_bytes
            .saturating_add(owned.total_allocated_buffer_bytes);
        self.total_allocated_texture_bytes = self
            .total_allocated_texture_bytes
            .saturating_add(owned.total_allocated_texture_bytes);
        self.glyph_atlas_bytes = self
            .glyph_atlas_bytes
            .saturating_add(owned.glyph_atlas_bytes);
        self.raster_cache_bytes = self
            .raster_cache_bytes
            .saturating_add(owned.raster_cache_bytes);
        self.image_texture_bytes = self
            .image_texture_bytes
            .saturating_add(owned.image_texture_bytes);
        self.base_text_renderer_materialization_count = self
            .base_text_renderer_materialization_count
            .saturating_add(owned.base_text_renderer_materialization_count);
        self.cursor_text_renderer_materialization_count = self
            .cursor_text_renderer_materialization_count
            .saturating_add(owned.cursor_text_renderer_materialization_count);
        if self.backend.is_none() {
            self.backend.clone_from(&owned.backend);
        }
        if self.adapter_name.is_none() {
            self.adapter_name.clone_from(&owned.adapter_name);
        }
        self
    }

    /// Validates the cumulative R-Term ownership matrix at an exact hold.
    ///
    /// # Errors
    ///
    /// Returns every missing, fabricated, or later-stage resource instead of
    /// accepting a partial snapshot.
    pub fn validate_at(&self, stage: GpuInitializationStage) -> Result<(), Vec<String>> {
        let mut violations = Vec::new();
        require_exact(&mut violations, "instance_count", self.instance_count, 1);
        require_exact(&mut violations, "surface_count", self.surface_count, 1);

        let has_device = stage >= GpuInitializationStage::AdapterDevice;
        for (name, actual) in [
            ("adapter_count", self.adapter_count),
            ("device_count", self.device_count),
            ("queue_count", self.queue_count),
        ] {
            require_exact(&mut violations, name, actual, u64::from(has_device));
        }
        if has_device {
            require_identity(&mut violations, "backend", self.backend.as_deref());
            require_identity(
                &mut violations,
                "adapter_name",
                self.adapter_name.as_deref(),
            );
        } else {
            forbid_identity(&mut violations, "backend", self.backend.as_deref());
            forbid_identity(
                &mut violations,
                "adapter_name",
                self.adapter_name.as_deref(),
            );
        }

        let has_clear = stage >= GpuInitializationStage::ConfiguredSurfaceClear;
        for (name, actual) in [
            ("surface_configure_count", self.surface_configure_count),
            ("surface_acquire_count", self.surface_acquire_count),
            ("clear_present_count", self.clear_present_count),
        ] {
            require_exact(&mut violations, name, actual, u64::from(has_clear));
        }

        let has_layers = stage >= GpuInitializationStage::LayerPipelines;
        require_exact(
            &mut violations,
            "pipeline_count",
            self.pipeline_count,
            if has_layers { 2 } else { 0 },
        );
        require_exact(
            &mut violations,
            "pipeline_layout_count",
            self.pipeline_layout_count,
            if has_layers { 2 } else { 0 },
        );
        require_exact(
            &mut violations,
            "materialized_buffer_count",
            self.materialized_buffer_count,
            u64::from(has_layers),
        );
        require_exact(
            &mut violations,
            "total_allocated_buffer_bytes",
            self.total_allocated_buffer_bytes,
            if has_layers { 8 } else { 0 },
        );
        require_exact(
            &mut violations,
            "instance_buffer_bytes",
            self.instance_buffer_bytes,
            0,
        );
        require_exact(
            &mut violations,
            "upload_buffer_bytes",
            self.upload_buffer_bytes,
            0,
        );
        require_exact(
            &mut violations,
            "total_allocated_texture_bytes",
            self.total_allocated_texture_bytes,
            0,
        );
        for (name, actual) in [
            ("glyph_atlas_bytes", self.glyph_atlas_bytes),
            ("raster_cache_bytes", self.raster_cache_bytes),
            ("image_texture_bytes", self.image_texture_bytes),
            (
                "base_text_renderer_materialization_count",
                self.base_text_renderer_materialization_count,
            ),
            (
                "cursor_text_renderer_materialization_count",
                self.cursor_text_renderer_materialization_count,
            ),
        ] {
            require_exact(&mut violations, name, actual, 0);
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }
}

fn require_exact(violations: &mut Vec<String>, name: &str, actual: u64, expected: u64) {
    if actual != expected {
        violations.push(format!("{name} must be {expected}, got {actual}"));
    }
}

fn require_identity(violations: &mut Vec<String>, name: &str, actual: Option<&str>) {
    if actual.is_none_or(str::is_empty) {
        violations.push(format!("{name} is required"));
    }
}

fn forbid_identity(violations: &mut Vec<String>, name: &str, actual: Option<&str>) {
    if actual.is_some() {
        violations.push(format!("{name} must be absent"));
    }
}

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
