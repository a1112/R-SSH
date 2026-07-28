use std::{
    borrow::Cow,
    error::Error,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use super::{GpuPresentationMetrics, SurfaceFault, SurfaceRecovery, SurfaceRecoveryState};

pub const DEFAULT_CPU_FRAME_BYTE_BUDGET: usize = 256 * 1024 * 1024;
static NEXT_GPU_CONTEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GpuContextGeneration(u64);

fn next_gpu_context_generation() -> GpuContextGeneration {
    GpuContextGeneration(NEXT_GPU_CONTEXT_GENERATION.fetch_add(1, Ordering::Relaxed))
}

const COMPATIBILITY_SHADER: &str = r"
@group(0) @binding(0)
var frame_texture: texture_2d<f32>;
@group(0) @binding(1)
var frame_sampler: sampler;
struct Layout {
    transform: vec4<f32>,
};
@group(0) @binding(2)
var<uniform> layout_uniform_data: Layout;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let uvs = array(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var output: VertexOutput;
    output.position = vec4<f32>(
        positions[index] * layout_uniform_data.transform.xy + layout_uniform_data.transform.zw,
        0.0,
        1.0,
    );
    output.uv = uvs[index];
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(frame_texture, frame_sampler, input.uv);
}
";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RgbaFrameLayout {
    pub bytes_per_row: u32,
    pub byte_len: usize,
}

impl RgbaFrameLayout {
    /// Validates an RGBA8 framebuffer against GPU and CPU resource limits.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error for zero dimensions, arithmetic
    /// overflow, dimensions beyond `max_texture_dimension_2d`, or a byte size
    /// beyond `cpu_byte_budget`.
    pub fn new(
        width: u32,
        height: u32,
        max_texture_dimension_2d: u32,
        cpu_byte_budget: usize,
    ) -> Result<Self, GpuContextError> {
        if width == 0 || height == 0 {
            return Err(GpuContextError::with_kind(
                GpuContextErrorKind::ResourceLimit,
                "RGBA framebuffer dimensions must be nonzero".to_owned(),
            ));
        }
        if width > max_texture_dimension_2d || height > max_texture_dimension_2d {
            return Err(GpuContextError::with_kind(
                GpuContextErrorKind::ResourceLimit,
                format!(
                    "RGBA framebuffer {width}x{height} exceeds max texture dimension {max_texture_dimension_2d}"
                ),
            ));
        }
        let bytes_per_row = width.checked_mul(4).ok_or_else(|| {
            GpuContextError::with_kind(
                GpuContextErrorKind::ResourceLimit,
                "RGBA framebuffer row pitch overflow".to_owned(),
            )
        })?;
        let byte_len_u64 = u64::from(bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or_else(|| {
                GpuContextError::with_kind(
                    GpuContextErrorKind::ResourceLimit,
                    "RGBA framebuffer byte length overflow".to_owned(),
                )
            })?;
        let byte_len = usize::try_from(byte_len_u64).map_err(|_| {
            GpuContextError::with_kind(
                GpuContextErrorKind::ResourceLimit,
                "RGBA framebuffer does not fit the host address space".to_owned(),
            )
        })?;
        if byte_len > cpu_byte_budget {
            return Err(GpuContextError::with_kind(
                GpuContextErrorKind::ResourceLimit,
                format!(
                    "RGBA framebuffer requires {byte_len} bytes, exceeding the {cpu_byte_budget}-byte CPU budget"
                ),
            ));
        }
        Ok(Self {
            bytes_per_row,
            byte_len,
        })
    }
}

#[derive(Clone, Debug)]
enum GpuRuntimeFault {
    OutOfMemory(String),
    Validation(String),
    Internal(String),
    DeviceLost {
        reason: wgpu::DeviceLostReason,
        message: String,
    },
}

#[derive(Debug, Default)]
struct GpuFaultMonitor {
    pending: Mutex<Option<GpuRuntimeFault>>,
    uncaptured_errors: AtomicU64,
    device_losses: AtomicU64,
}

impl GpuFaultMonitor {
    fn record(&self, fault: GpuRuntimeFault) {
        match &fault {
            GpuRuntimeFault::DeviceLost { .. } => {
                self.device_losses.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                self.uncaptured_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(fault, GpuRuntimeFault::DeviceLost { .. })
            || !matches!(pending.as_ref(), Some(GpuRuntimeFault::DeviceLost { .. }))
                && pending.is_none()
        {
            *pending = Some(fault);
        }
    }

    fn take(&self) -> Option<GpuRuntimeFault> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

fn install_device_fault_handlers(device: &wgpu::Device, monitor: &Arc<GpuFaultMonitor>) {
    let uncaptured_monitor = Arc::clone(monitor);
    device.on_uncaptured_error(Arc::new(move |error| {
        let fault = match error {
            wgpu::Error::OutOfMemory { .. } => GpuRuntimeFault::OutOfMemory(error.to_string()),
            wgpu::Error::Validation { description, .. } => GpuRuntimeFault::Validation(description),
            wgpu::Error::Internal { description, .. } => GpuRuntimeFault::Internal(description),
        };
        uncaptured_monitor.record(fault);
    }));

    let lost_monitor = Arc::clone(monitor);
    device.set_device_lost_callback(move |reason, message| {
        lost_monitor.record(GpuRuntimeFault::DeviceLost { reason, message });
    });
}

fn take_runtime_fault(
    monitor: &GpuFaultMonitor,
    metrics: &mut GpuPresentationMetrics,
    stage: &str,
) -> Result<(), GpuContextError> {
    metrics.uncaptured_errors = monitor.uncaptured_errors.load(Ordering::Relaxed);
    metrics.device_losses = monitor.device_losses.load(Ordering::Relaxed);
    let Some(fault) = monitor.take() else {
        return Ok(());
    };
    match fault {
        GpuRuntimeFault::OutOfMemory(message) => Err(GpuContextError::with_kind(
            GpuContextErrorKind::OutOfMemory,
            format!("GPU out of memory {stage}: {message}"),
        )),
        GpuRuntimeFault::Validation(message) => Err(GpuContextError::with_kind(
            GpuContextErrorKind::Validation,
            format!("GPU validation error {stage}: {message}"),
        )),
        GpuRuntimeFault::Internal(message) => Err(GpuContextError::with_kind(
            GpuContextErrorKind::Internal,
            format!("internal GPU error {stage}: {message}"),
        )),
        GpuRuntimeFault::DeviceLost { reason, message } => Err(GpuContextError::with_kind(
            GpuContextErrorKind::DeviceLost,
            format!("GPU device lost {stage}: {reason:?}: {message}"),
        )),
    }
}

/// Adapter selection controls shared by headless tests and native surfaces.
#[derive(Clone, Copy, Debug)]
pub struct GpuContextOptions {
    pub backends: wgpu::Backends,
    pub power_preference: wgpu::PowerPreference,
    pub force_fallback_adapter: bool,
}

impl Default for GpuContextOptions {
    fn default() -> Self {
        Self {
            backends: native_backends(),
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
        }
    }
}

impl GpuContextOptions {
    #[must_use]
    pub const fn with_high_performance(mut self, high_performance: bool) -> Self {
        self.power_preference = if high_performance {
            wgpu::PowerPreference::HighPerformance
        } else {
            wgpu::PowerPreference::LowPower
        };
        self
    }

    #[must_use]
    pub const fn with_force_fallback_adapter(mut self, force: bool) -> Self {
        self.force_fallback_adapter = force;
        self
    }
}

/// Owns the instance, selected adapter, logical device, and submission queue.
pub struct GpuContext {
    generation: GpuContextGeneration,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_window: Option<Arc<dyn wgpu::WindowHandle>>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    compatibility_pipeline: Option<CompatibilityPipeline>,
    runtime_faults: Arc<GpuFaultMonitor>,
    suspended: bool,
    metrics: GpuPresentationMetrics,
}

impl fmt::Debug for GpuContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GpuContext")
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

impl GpuContext {
    /// Selects a native adapter and device without requiring a display surface.
    ///
    /// # Errors
    ///
    /// Returns an error when no native or software adapter can be selected, or
    /// when the selected adapter cannot create the required device.
    pub async fn new_headless(options: GpuContextOptions) -> Result<Self, GpuContextError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: options.backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = request_adapter(&instance, None, options).await?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rssh-native-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| GpuContextError::new("request device", error))?;
        let runtime_faults = Arc::new(GpuFaultMonitor::default());
        install_device_fault_handlers(&device, &runtime_faults);

        Ok(Self {
            generation: next_gpu_context_generation(),
            instance,
            adapter,
            device,
            queue,
            surface_window: None,
            surface: None,
            surface_config: None,
            compatibility_pipeline: None,
            runtime_faults,
            suspended: false,
            metrics: GpuPresentationMetrics::from_adapter(&info),
        })
    }

    /// Creates a direct presentation surface from the same display and window
    /// handles owned by the active winit event loop.
    ///
    /// # Errors
    ///
    /// Returns an error when surface creation, adapter selection, device
    /// creation, or initial surface configuration fails.
    pub async fn new_windowed<D, W>(
        display: D,
        window: Arc<W>,
        width: u32,
        height: u32,
        options: GpuContextOptions,
    ) -> Result<Self, GpuContextError>
    where
        D: raw_window_handle::HasDisplayHandle + fmt::Debug + Send + Sync + 'static,
        W: wgpu::WindowHandle + 'static,
    {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: options.backends,
            ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display))
        });
        let surface_window: Arc<dyn wgpu::WindowHandle> = window;
        let surface = create_surface(&instance, Arc::clone(&surface_window))?;
        let adapter = request_adapter(&instance, Some(&surface), options).await?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rssh-native-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| GpuContextError::new("request device", error))?;
        let runtime_faults = Arc::new(GpuFaultMonitor::default());
        install_device_fault_handlers(&device, &runtime_faults);

        let mut context = Self {
            generation: next_gpu_context_generation(),
            instance,
            adapter,
            device,
            queue,
            surface_window: Some(surface_window),
            surface: Some(surface),
            surface_config: None,
            compatibility_pipeline: None,
            runtime_faults,
            suspended: width == 0 || height == 0,
            metrics: GpuPresentationMetrics::from_adapter(&info),
        };
        if !context.suspended {
            context.configure_surface(width, height)?;
        }
        Ok(context)
    }

    #[must_use]
    pub const fn metrics(&self) -> &GpuPresentationMetrics {
        &self.metrics
    }

    #[must_use]
    pub const fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    #[must_use]
    pub const fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    #[must_use]
    pub const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    #[must_use]
    pub const fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    #[must_use]
    pub const fn generation(&self) -> GpuContextGeneration {
        self.generation
    }

    #[must_use]
    pub fn max_texture_dimension_2d(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// Validates a compatibility framebuffer against the selected device and
    /// the process-wide CPU framebuffer budget.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error for unsupported or excessive geometry.
    pub fn rgba_frame_layout(
        &self,
        width: u32,
        height: u32,
    ) -> Result<RgbaFrameLayout, GpuContextError> {
        RgbaFrameLayout::new(
            width,
            height,
            self.max_texture_dimension_2d(),
            DEFAULT_CPU_FRAME_BYTE_BUDGET,
        )
    }

    fn check_runtime_faults(&mut self, stage: &str) -> Result<(), GpuContextError> {
        take_runtime_fault(&self.runtime_faults, &mut self.metrics, stage)
    }

    /// Executes a tiny native submission and polls it with a caller-owned deadline.
    ///
    /// # Errors
    ///
    /// Returns an error when device polling fails or the submission does not
    /// complete before `timeout`.
    pub fn run_headless_submission_probe(
        &mut self,
        timeout: Duration,
    ) -> Result<(), GpuContextError> {
        self.check_runtime_faults("before headless submission probe")?;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rssh-headless-probe"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.check_runtime_faults("after headless buffer creation")?;
        self.queue.write_buffer(&buffer, 0, &[1, 2, 3, 4]);
        self.check_runtime_faults("after headless buffer upload")?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rssh-headless-probe-submit"),
            });
        encoder.clear_buffer(&buffer, 0, None);
        self.queue.submit([encoder.finish()]);
        self.check_runtime_faults("after headless submission")?;

        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        loop {
            let status = self
                .device
                .poll(wgpu::PollType::Poll)
                .map_err(|error| GpuContextError::new("poll headless submission", error))?;
            self.check_runtime_faults("while polling headless submission")?;
            if status.is_queue_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(GpuContextError::message(format!(
                    "headless submission did not complete within {timeout:?}"
                )));
            }
            std::thread::yield_now();
        }
    }

    /// Reconfigures the swap chain, or suspends acquisition for a zero-sized window.
    ///
    /// # Errors
    ///
    /// Returns an error if a nonzero surface exposes no usable configuration.
    pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), GpuContextError> {
        self.suspended = width == 0 || height == 0;
        if self.suspended {
            return Ok(());
        }
        self.configure_surface(width, height)
    }

    /// Uploads the compatibility CPU framebuffer and presents it through the
    /// directly owned wgpu surface.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid framebuffer geometry, unrecoverable surface
    /// acquisition failures, or missing presentation state.
    pub fn render_rgba(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        pre_present: impl FnOnce(),
    ) -> Result<GpuFrameStatus, GpuContextError> {
        if self.suspended {
            return Ok(GpuFrameStatus::Skipped);
        }
        self.check_runtime_faults("before frame upload")?;
        let layout = self.rgba_frame_layout(width, height)?;
        validate_frame(rgba, layout)?;
        self.ensure_upload_frame(width, height)?;
        {
            let upload = self
                .compatibility_pipeline
                .as_ref()
                .and_then(|pipeline| pipeline.upload.as_ref())
                .ok_or_else(|| {
                    GpuContextError::message("upload texture is not configured".into())
                })?;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &upload.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.bytes_per_row),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.check_runtime_faults("after frame upload")?;

        let Some((surface_texture, suboptimal)) =
            self.acquire_surface_texture(&mut SurfaceRecoveryState::new())?
        else {
            return Ok(GpuFrameStatus::Skipped);
        };
        let pipeline = self
            .compatibility_pipeline
            .as_ref()
            .ok_or_else(|| GpuContextError::message("surface pipeline is not configured".into()))?;
        let upload = pipeline
            .upload
            .as_ref()
            .ok_or_else(|| GpuContextError::message("upload texture is not configured".into()))?;
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rssh-compatibility-frame"),
            });
        {
            let color_attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &surface_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rssh-compatibility-present"),
                color_attachments: &color_attachments,
                ..wgpu::RenderPassDescriptor::default()
            });
            let (x, y, width, height) = pipeline.clip_rect;
            render_pass.set_scissor_rect(x, y, width, height);
            render_pass.set_pipeline(&pipeline.pipeline);
            render_pass.set_bind_group(0, &upload.bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }
        self.check_runtime_faults("before frame submission")?;
        self.queue.submit([encoder.finish()]);
        self.check_runtime_faults("after frame submission")?;
        self.metrics.rendered_frames = self.metrics.rendered_frames.saturating_add(1);
        self.check_runtime_faults("before frame presentation")?;
        pre_present();
        self.queue.present(surface_texture);
        self.check_runtime_faults("after frame presentation")?;
        self.metrics.presented_frames = self.metrics.presented_frames.saturating_add(1);
        if suboptimal {
            let (width, height) = self.configured_size()?;
            self.configure_surface(width, height)?;
        }
        Ok(GpuFrameStatus::Presented)
    }

    fn configure_surface(&mut self, width: u32, height: u32) -> Result<(), GpuContextError> {
        if width == 0 || height == 0 {
            self.suspended = true;
            return Ok(());
        }
        let max_dimension = self.max_texture_dimension_2d();
        if width > max_dimension || height > max_dimension {
            return Err(GpuContextError::with_kind(
                GpuContextErrorKind::ResourceLimit,
                format!("surface {width}x{height} exceeds max texture dimension {max_dimension}"),
            ));
        }
        let surface = self.surface.as_ref().ok_or_else(|| {
            GpuContextError::message("context has no presentation surface".into())
        })?;
        let capabilities = surface.get_capabilities(&self.adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| GpuContextError::message("surface exposes no formats".into()))?;
        let present_mode = capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
            .then_some(wgpu::PresentMode::Fifo)
            .or_else(|| capabilities.present_modes.first().copied())
            .ok_or_else(|| GpuContextError::message("surface exposes no present modes".into()))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
        };
        surface.configure(&self.device, &config);
        if self
            .surface_config
            .as_ref()
            .is_none_or(|previous| previous.format != config.format)
        {
            self.compatibility_pipeline = Some(CompatibilityPipeline::new(
                &self.device,
                config.format,
                width,
                height,
            ));
        } else if let Some(pipeline) = self.compatibility_pipeline.as_mut() {
            pipeline.set_surface_size(&self.queue, width, height)?;
        }
        self.check_runtime_faults("after surface configuration")?;
        self.metrics.surface_format = Some(format!("{format:?}"));
        self.metrics.present_mode = Some(format!("{present_mode:?}"));
        self.metrics.surface_width = Some(width);
        self.metrics.surface_height = Some(height);
        self.metrics.surface_reconfigurations =
            self.metrics.surface_reconfigurations.saturating_add(1);
        self.surface_config = Some(config);
        self.suspended = false;
        Ok(())
    }

    fn recreate_surface(&mut self) -> Result<(), GpuContextError> {
        let window = self
            .surface_window
            .clone()
            .ok_or_else(|| GpuContextError::message("surface window is unavailable".into()))?;
        self.surface = Some(create_surface(&self.instance, window)?);
        self.metrics.surface_recreations = self.metrics.surface_recreations.saturating_add(1);
        let (width, height) = self.configured_size()?;
        self.configure_surface(width, height)
    }

    fn configured_size(&self) -> Result<(u32, u32), GpuContextError> {
        self.surface_config
            .as_ref()
            .map(|config| (config.width, config.height))
            .ok_or_else(|| GpuContextError::message("surface is not configured".into()))
    }

    fn acquire_surface_texture(
        &mut self,
        recovery: &mut SurfaceRecoveryState,
    ) -> Result<Option<(wgpu::SurfaceTexture, bool)>, GpuContextError> {
        self.check_runtime_faults("before surface acquisition")?;
        let acquisition = self
            .surface
            .as_ref()
            .ok_or_else(|| GpuContextError::message("context has no presentation surface".into()))?
            .get_current_texture();
        self.check_runtime_faults("after surface acquisition")?;
        match acquisition {
            wgpu::CurrentSurfaceTexture::Success(texture) => Ok(Some((texture, false))),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Ok(Some((texture, true))),
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.metrics.surface_timeouts = self.metrics.surface_timeouts.saturating_add(1);
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.metrics.surface_occlusions = self.metrics.surface_occlusions.saturating_add(1);
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Outdated
                if recovery.action(SurfaceFault::Outdated)
                    == SurfaceRecovery::ReconfigureAndRetry =>
            {
                let (width, height) = self.configured_size()?;
                self.configure_surface(width, height)?;
                self.acquire_surface_texture(recovery)
            }
            wgpu::CurrentSurfaceTexture::Lost
                if recovery.action(SurfaceFault::Lost) == SurfaceRecovery::RecreateAndRetry =>
            {
                self.recreate_surface()?;
                self.acquire_surface_texture(recovery)
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.metrics.surface_validation_errors =
                    self.metrics.surface_validation_errors.saturating_add(1);
                Err(GpuContextError::message(
                    "surface acquisition validation error".into(),
                ))
            }
            wgpu::CurrentSurfaceTexture::Outdated => Err(GpuContextError::message(
                "surface remained outdated after one reconfiguration".into(),
            )),
            wgpu::CurrentSurfaceTexture::Lost => Err(GpuContextError::message(
                "surface remained lost after one recreation".into(),
            )),
        }
    }

    fn ensure_upload_frame(&mut self, width: u32, height: u32) -> Result<(), GpuContextError> {
        let pipeline = self
            .compatibility_pipeline
            .as_mut()
            .expect("configured surface must own a compatibility pipeline");
        if pipeline
            .upload
            .as_ref()
            .is_some_and(|upload| upload.width == width && upload.height == height)
        {
            return Ok(());
        }
        pipeline.upload = Some(UploadFrame::new(
            &self.device,
            &pipeline.bind_group_layout,
            &pipeline.sampler,
            &pipeline.layout_uniform,
            width,
            height,
        ));
        pipeline.set_frame_size(&self.queue, width, height)?;
        self.check_runtime_faults("after compatibility texture creation")
    }
}

fn create_surface(
    instance: &wgpu::Instance,
    window: Arc<dyn wgpu::WindowHandle>,
) -> Result<wgpu::Surface<'static>, GpuContextError> {
    instance
        .create_surface(wgpu::SurfaceTarget::from_window_without_display(window))
        .map_err(|error| GpuContextError::new("create surface", error))
}

fn validate_frame(rgba: &[u8], layout: RgbaFrameLayout) -> Result<(), GpuContextError> {
    if rgba.len() != layout.byte_len {
        return Err(GpuContextError::with_kind(
            GpuContextErrorKind::InvalidFrame,
            format!(
                "compatibility framebuffer length mismatch: expected {}, got {}",
                layout.byte_len,
                rgba.len()
            ),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct CompatibilityLayout {
    clip_rect: (u32, u32, u32, u32),
    transform: [f32; 4],
}

impl CompatibilityLayout {
    #[allow(clippy::cast_precision_loss)] // Device texture limits stay below f32's exact integer range.
    fn new(
        frame_width: u32,
        frame_height: u32,
        surface_width: u32,
        surface_height: u32,
    ) -> Result<Self, GpuContextError> {
        if frame_width == 0 || frame_height == 0 || surface_width == 0 || surface_height == 0 {
            return Err(GpuContextError::with_kind(
                GpuContextErrorKind::InvalidFrame,
                "compatibility layout dimensions must be nonzero".to_owned(),
            ));
        }
        let integer_scale = (surface_width / frame_width)
            .min(surface_height / frame_height)
            .max(1);
        let scaled_width = frame_width.checked_mul(integer_scale).ok_or_else(|| {
            GpuContextError::with_kind(
                GpuContextErrorKind::ResourceLimit,
                "compatibility layout width overflow".to_owned(),
            )
        })?;
        let scaled_height = frame_height.checked_mul(integer_scale).ok_or_else(|| {
            GpuContextError::with_kind(
                GpuContextErrorKind::ResourceLimit,
                "compatibility layout height overflow".to_owned(),
            )
        })?;
        let clip_width = scaled_width.min(surface_width);
        let clip_height = scaled_height.min(surface_height);
        let clip_x = surface_width.saturating_sub(clip_width) / 2;
        let clip_y = surface_height.saturating_sub(clip_height) / 2;
        let surface_width_f32 = surface_width as f32;
        let surface_height_f32 = surface_height as f32;
        let translation_x = if surface_width % 2 == 0 {
            0.0
        } else {
            0.5 / surface_width_f32
        };
        let translation_y = if surface_height % 2 == 0 {
            0.0
        } else {
            0.5 / surface_height_f32
        };
        Ok(Self {
            clip_rect: (clip_x, clip_y, clip_width, clip_height),
            transform: [
                scaled_width as f32 / surface_width_f32,
                scaled_height as f32 / surface_height_f32,
                translation_x,
                translation_y,
            ],
        })
    }
}

struct CompatibilityPipeline {
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    layout_uniform: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    upload: Option<UploadFrame>,
    surface_size: (u32, u32),
    frame_size: Option<(u32, u32)>,
    clip_rect: (u32, u32, u32, u32),
}

impl CompatibilityPipeline {
    fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        surface_width: u32,
        surface_height: u32,
    ) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rssh-compatibility-bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rssh-compatibility-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        let layout_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rssh-compatibility-layout"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rssh-compatibility-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(COMPATIBILITY_SHADER)),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rssh-compatibility-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let targets = [Some(wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rssh-compatibility-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            bind_group_layout,
            sampler,
            layout_uniform,
            pipeline,
            upload: None,
            surface_size: (surface_width, surface_height),
            frame_size: None,
            clip_rect: (0, 0, surface_width, surface_height),
        }
    }

    fn set_surface_size(
        &mut self,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<(), GpuContextError> {
        self.surface_size = (width, height);
        self.update_layout(queue)
    }

    fn set_frame_size(
        &mut self,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<(), GpuContextError> {
        self.frame_size = Some((width, height));
        self.update_layout(queue)
    }

    fn update_layout(&mut self, queue: &wgpu::Queue) -> Result<(), GpuContextError> {
        let Some((frame_width, frame_height)) = self.frame_size else {
            return Ok(());
        };
        let layout = CompatibilityLayout::new(
            frame_width,
            frame_height,
            self.surface_size.0,
            self.surface_size.1,
        )?;
        let mut bytes = [0_u8; 16];
        for (destination, value) in bytes.chunks_exact_mut(4).zip(layout.transform) {
            destination.copy_from_slice(&value.to_ne_bytes());
        }
        queue.write_buffer(&self.layout_uniform, 0, &bytes);
        self.clip_rect = layout.clip_rect;
        Ok(())
    }
}

struct UploadFrame {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

impl UploadFrame {
    fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        layout_uniform: &wgpu::Buffer,
        width: u32,
        height: u32,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rssh-compatibility-framebuffer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rssh-compatibility-frame-bind-group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: layout_uniform.as_entire_binding(),
                },
            ],
        });
        Self {
            width,
            height,
            texture,
            bind_group,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuFrameStatus {
    Presented,
    Skipped,
}

async fn request_adapter(
    instance: &wgpu::Instance,
    compatible_surface: Option<&wgpu::Surface<'_>>,
    options: GpuContextOptions,
) -> Result<wgpu::Adapter, GpuContextError> {
    let primary = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: options.power_preference,
            force_fallback_adapter: options.force_fallback_adapter,
            compatible_surface,
            apply_limit_buckets: false,
        })
        .await;
    match primary {
        Ok(adapter) => Ok(adapter),
        Err(primary_error) if !options.force_fallback_adapter => instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: options.power_preference,
                force_fallback_adapter: true,
                compatible_surface,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|fallback_error| {
                GpuContextError::message(format!(
                    "request adapter failed: primary={primary_error}; software fallback={fallback_error}"
                ))
            }),
        Err(error) => Err(GpuContextError::new("request fallback adapter", error)),
    }
}

const fn native_backends() -> wgpu::Backends {
    #[cfg(target_os = "windows")]
    {
        wgpu::Backends::DX12
            .union(wgpu::Backends::VULKAN)
            .union(wgpu::Backends::GL)
    }
    #[cfg(target_os = "macos")]
    {
        wgpu::Backends::METAL
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        wgpu::Backends::VULKAN.union(wgpu::Backends::GL)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        wgpu::Backends::all()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuContextErrorKind {
    Initialization,
    Surface,
    InvalidFrame,
    ResourceLimit,
    Validation,
    OutOfMemory,
    Internal,
    DeviceLost,
}

#[derive(Debug)]
pub struct GpuContextError {
    kind: GpuContextErrorKind,
    message: String,
}

impl GpuContextError {
    fn new(context: &str, error: impl fmt::Display) -> Self {
        Self::with_kind(
            GpuContextErrorKind::Initialization,
            format!("{context}: {error}"),
        )
    }

    fn message(message: String) -> Self {
        Self::with_kind(GpuContextErrorKind::Surface, message)
    }

    fn with_kind(kind: GpuContextErrorKind, message: String) -> Self {
        Self { kind, message }
    }

    #[must_use]
    pub const fn kind(&self) -> GpuContextErrorKind {
        self.kind
    }
}

impl fmt::Display for GpuContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GpuContextError {}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() <= f32::EPSILON);
    }

    #[test]
    fn compatibility_layout_matches_pixels_integer_scaling_and_letterbox() {
        let odd_surface = CompatibilityLayout::new(640, 400, 641, 401).expect("valid odd surface");
        assert_eq!(odd_surface.clip_rect, (0, 0, 640, 400));
        assert_close(odd_surface.transform[0], 640.0 / 641.0);
        assert_close(odd_surface.transform[1], 400.0 / 401.0);

        let letterboxed = CompatibilityLayout::new(640, 400, 960, 600).expect("valid letterbox");
        assert_eq!(letterboxed.clip_rect, (160, 100, 640, 400));
        assert_close(letterboxed.transform[0], 640.0 / 960.0);
        assert_close(letterboxed.transform[1], 400.0 / 600.0);

        let doubled =
            CompatibilityLayout::new(640, 400, 1_280, 800).expect("valid integer upscale");
        assert_eq!(doubled.clip_rect, (0, 0, 1_280, 800));
        assert_close(doubled.transform[0], 1.0);
        assert_close(doubled.transform[1], 1.0);
    }

    #[test]
    fn compatibility_layout_centers_and_clips_frames_larger_than_the_surface() {
        let cropped = CompatibilityLayout::new(640, 400, 320, 200).expect("valid crop");
        assert_eq!(cropped.clip_rect, (0, 0, 320, 200));
        assert!(cropped.transform[0] > 1.0);
        assert!(cropped.transform[1] > 1.0);
    }

    #[test]
    fn rgba_layout_accepts_4k_and_rejects_all_resource_limit_edges_without_panicking() {
        let four_k = RgbaFrameLayout::new(3_840, 2_160, 8_192, DEFAULT_CPU_FRAME_BYTE_BUDGET)
            .expect("4K frame fits the native texture and CPU budget");
        assert_eq!(four_k.bytes_per_row, 15_360);
        assert_eq!(four_k.byte_len, 33_177_600);

        let failures = catch_unwind(AssertUnwindSafe(|| {
            [
                RgbaFrameLayout::new(0, 1, 8_192, DEFAULT_CPU_FRAME_BYTE_BUDGET),
                RgbaFrameLayout::new(8_193, 1, 8_192, DEFAULT_CPU_FRAME_BYTE_BUDGET),
                RgbaFrameLayout::new(u32::MAX, 1, u32::MAX, DEFAULT_CPU_FRAME_BYTE_BUDGET),
                RgbaFrameLayout::new(8_192, 8_192, 8_192, 32 * 1024 * 1024),
            ]
        }))
        .expect("invalid resource requests must return errors, not panic");
        assert!(failures.into_iter().all(|result| result.is_err()));
    }

    #[test]
    fn injected_validation_and_device_loss_are_structured_and_do_not_count_frames() {
        for (fault, expected_uncaptured, expected_losses) in [
            (
                GpuRuntimeFault::Validation("injected validation".to_owned()),
                1,
                0,
            ),
            (
                GpuRuntimeFault::DeviceLost {
                    reason: wgpu::DeviceLostReason::Unknown,
                    message: "injected device loss".to_owned(),
                },
                0,
                1,
            ),
        ] {
            let monitor = GpuFaultMonitor::default();
            let mut metrics = GpuPresentationMetrics::uninitialized();
            monitor.record(fault);
            let result = catch_unwind(AssertUnwindSafe(|| {
                take_runtime_fault(&monitor, &mut metrics, "before upload")
            }))
            .expect("fault observation must return an error instead of panicking");
            let error = result.expect_err("injected fault must stop presentation");

            assert!(matches!(
                error.kind(),
                GpuContextErrorKind::Validation | GpuContextErrorKind::DeviceLost
            ));
            assert_eq!(metrics.rendered_frames, 0);
            assert_eq!(metrics.presented_frames, 0);
            assert_eq!(metrics.uncaptured_errors, expected_uncaptured);
            assert_eq!(metrics.device_losses, expected_losses);
        }
    }

    #[test]
    fn device_loss_keeps_prior_uncaptured_fault_metrics_while_taking_precedence() {
        let monitor = GpuFaultMonitor::default();
        let mut metrics = GpuPresentationMetrics::uninitialized();
        monitor.record(GpuRuntimeFault::Validation(
            "validation before loss".to_owned(),
        ));
        monitor.record(GpuRuntimeFault::DeviceLost {
            reason: wgpu::DeviceLostReason::Unknown,
            message: "device removed".to_owned(),
        });

        let error = take_runtime_fault(&monitor, &mut metrics, "after submit")
            .expect_err("device loss must stop the frame");
        assert_eq!(error.kind(), GpuContextErrorKind::DeviceLost);
        assert_eq!(metrics.uncaptured_errors, 1);
        assert_eq!(metrics.device_losses, 1);
        assert_eq!(metrics.rendered_frames, 0);
        assert_eq!(metrics.presented_frames, 0);
    }
}
