use std::{
    borrow::Cow,
    error::Error,
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use super::{GpuPresentationMetrics, SurfaceFault, SurfaceRecovery, SurfaceRecoveryState};

const COMPATIBILITY_SHADER: &str = r"
@group(0) @binding(0)
var frame_texture: texture_2d<f32>;
@group(0) @binding(1)
var frame_sampler: sampler;

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
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    output.uv = uvs[index];
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(frame_texture, frame_sampler, input.uv);
}
";

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
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_window: Option<Arc<dyn wgpu::WindowHandle>>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    compatibility_pipeline: Option<CompatibilityPipeline>,
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

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface_window: None,
            surface: None,
            surface_config: None,
            compatibility_pipeline: None,
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

        let mut context = Self {
            instance,
            adapter,
            device,
            queue,
            surface_window: Some(surface_window),
            surface: Some(surface),
            surface_config: None,
            compatibility_pipeline: None,
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

    /// Executes a tiny native submission and polls it with a caller-owned deadline.
    ///
    /// # Errors
    ///
    /// Returns an error when device polling fails or the submission does not
    /// complete before `timeout`.
    pub fn run_headless_submission_probe(&self, timeout: Duration) -> Result<(), GpuContextError> {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rssh-headless-probe"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, &[1, 2, 3, 4]);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rssh-headless-probe-submit"),
            });
        encoder.clear_buffer(&buffer, 0, None);
        self.queue.submit([encoder.finish()]);

        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        loop {
            let status = self
                .device
                .poll(wgpu::PollType::Poll)
                .map_err(|error| GpuContextError::new("poll headless submission", error))?;
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
        validate_frame(rgba, width, height)?;
        self.ensure_upload_frame(width, height);
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
                    bytes_per_row: Some(width.saturating_mul(4)),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }

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
            render_pass.set_pipeline(&pipeline.pipeline);
            render_pass.set_bind_group(0, &upload.bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.metrics.rendered_frames = self.metrics.rendered_frames.saturating_add(1);
        pre_present();
        self.queue.present(surface_texture);
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
            self.compatibility_pipeline =
                Some(CompatibilityPipeline::new(&self.device, config.format));
        }
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
        let surface = self.surface.as_ref().ok_or_else(|| {
            GpuContextError::message("context has no presentation surface".into())
        })?;
        match surface.get_current_texture() {
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

    fn ensure_upload_frame(&mut self, width: u32, height: u32) {
        let pipeline = self
            .compatibility_pipeline
            .as_mut()
            .expect("configured surface must own a compatibility pipeline");
        if pipeline
            .upload
            .as_ref()
            .is_some_and(|upload| upload.width == width && upload.height == height)
        {
            return;
        }
        pipeline.upload = Some(UploadFrame::new(
            &self.device,
            &pipeline.bind_group_layout,
            &pipeline.sampler,
            width,
            height,
        ));
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

fn validate_frame(rgba: &[u8], width: u32, height: u32) -> Result<(), GpuContextError> {
    if width == 0 || height == 0 {
        return Err(GpuContextError::message(
            "compatibility framebuffer dimensions must be nonzero".into(),
        ));
    }
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            GpuContextError::message("compatibility framebuffer size overflow".into())
        })?;
    if rgba.len() != expected {
        return Err(GpuContextError::message(format!(
            "compatibility framebuffer length mismatch: expected {expected}, got {}",
            rgba.len()
        )));
    }
    Ok(())
}

struct CompatibilityPipeline {
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    upload: Option<UploadFrame>,
}

impl CompatibilityPipeline {
    fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
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
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rssh-compatibility-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
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
            pipeline,
            upload: None,
        }
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

#[derive(Debug)]
pub struct GpuContextError {
    message: String,
}

impl GpuContextError {
    fn new(context: &str, error: impl fmt::Display) -> Self {
        Self::message(format!("{context}: {error}"))
    }

    fn message(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for GpuContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GpuContextError {}
