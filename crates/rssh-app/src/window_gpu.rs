use std::{cell::RefCell, error::Error, io, sync::Arc};

#[cfg(feature = "diagnostic-tools")]
use std::time::Instant;

use rssh_diagnostics::{DiagnosticFontResourceSummary, DiagnosticGpuBackend};
use rterm_render_core::{DamageRegion, RenderGeometry, TerminalRenderSnapshot};
use rterm_render_cpu::TextPaintConfig;
use rterm_render_wgpu::gpu::{
    GpuContext, GpuContextError, GpuContextErrorKind, GpuContextOptions, GpuFrameStatus,
    GpuLayerRenderer, GpuPresentationMetrics, GpuTextConfig, GpuTextPrepareReport, RenderGraph,
    WindowedGpuContextBootstrap, should_abandon_recovered_window_surface,
};
use winit::{dpi::PhysicalSize, event_loop::OwnedDisplayHandle, window::Window};

use crate::platform_fonts::{
    CatalogActivation, FontCatalogMode, PlatformFontRepository, production_font_catalog_mode,
};

/// App-owned direct terminal renderer for the native wgpu surface.
pub(crate) struct WindowGpu {
    context: Option<GpuContext>,
    renderer: Option<GpuLayerRenderer>,
    retired_renderers: Vec<GpuLayerRenderer>,
    font_repository: PlatformFontRepository,
    font_catalog_mode: FontCatalogMode,
    recovery: DeviceRecoveryCoordinator,
    report: Option<GpuTextPrepareReport>,
    rendered_frames: u64,
    replaced_device: bool,
    final_metrics: Option<GpuPresentationMetrics>,
    diagnostic_font_resources: Option<DiagnosticFontResourceSummary>,
    #[cfg(test)]
    abandonment_workaround_adapter_match_override: Option<bool>,
    #[cfg(test)]
    current_adapter_metrics_override: Option<GpuPresentationMetrics>,
    #[cfg(test)]
    abandonment_workaround_os_override: Option<&'static str>,
    #[cfg(debug_assertions)]
    test_device_loss_injected: bool,
}

pub(crate) struct PreparedWindowGpu {
    context: WindowedGpuContextBootstrap,
    diagnostic_font_mode: Option<rssh_diagnostics::DiagnosticFontMode>,
    diagnostic_font_specimen: Option<rssh_diagnostics::DiagnosticFontSpecimen>,
}

struct WindowGpuFrame<'a> {
    window: &'a Window,
    snapshot: &'a TerminalRenderSnapshot,
    geometry: RenderGeometry,
    damage: &'a [DamageRegion],
    paint: &'a TextPaintConfig,
    graph: &'a RenderGraph,
    render_mode: rssh_native::RenderMode,
    dpi_scale: f32,
}

pub(crate) enum CatalogFrameAttempt<T> {
    Prepared(T),
    Expanded(u64),
}

pub(crate) fn prepare_catalog_frame_with_one_restart<T>(
    mut expected_generation: u64,
    damage: &[DamageRegion],
    mut prepare: impl FnMut(
        u64,
        &[DamageRegion],
        bool,
    ) -> Result<CatalogFrameAttempt<T>, Box<dyn Error>>,
) -> Result<T, Box<dyn Error>> {
    let mut frame_damage = damage;
    for attempt in 0..=1 {
        match prepare(expected_generation, frame_damage, attempt == 0)? {
            CatalogFrameAttempt::Prepared(report) => return Ok(report),
            CatalogFrameAttempt::Expanded(catalog_generation) if attempt == 0 => {
                expected_generation = catalog_generation;
                frame_damage = &[];
            }
            CatalogFrameAttempt::Expanded(_) => {
                return Err(io::Error::other(
                    "GPU font catalog expanded twice during one presented frame",
                )
                .into());
            }
        }
    }
    unreachable!("bounded catalog frame loop always returns")
}

#[expect(
    clippy::too_many_arguments,
    reason = "one app-owned transaction couples font preflight to one complete GPU text frame"
)]
fn prepare_gpu_text_frame(
    font_repository: &mut PlatformFontRepository,
    renderer: &mut GpuLayerRenderer,
    snapshot: &TerminalRenderSnapshot,
    geometry: RenderGeometry,
    damage: &[DamageRegion],
    paint: &TextPaintConfig,
    dpi_scale: f32,
    zoom: f32,
) -> Result<GpuTextPrepareReport, Box<dyn Error>> {
    font_repository.preflight_snapshot(
        snapshot,
        renderer
            .text_catalog_mut()
            .expect("window GPU text catalog is enabled"),
    )?;
    let expected_generation = renderer
        .text_catalog_generation()
        .expect("window GPU text catalog is enabled");
    prepare_catalog_frame_with_one_restart(
        expected_generation,
        damage,
        |expected_generation, frame_damage, can_expand| {
            let prepared = renderer.prepare_text_for_catalog_generation(
                expected_generation,
                snapshot,
                geometry,
                frame_damage,
                paint,
                dpi_scale,
                zoom,
            )?;
            let expansion = prepared.catalog_expansion();
            if let Some(report) = prepared.into_prepared() {
                if can_expand && !report.missing_glyphs.is_empty() {
                    let activation = match font_repository.activate_missing_glyphs(
                        &report.missing_glyphs,
                        renderer
                            .text_catalog_mut()
                            .expect("window GPU text catalog is enabled"),
                    ) {
                        Ok(activation) => activation,
                        Err(activation_error) => {
                            if let Err(discard_error) = renderer.discard_prepared_text_frame() {
                                return Err(io::Error::other(format!(
                                    "late font activation failed: {activation_error}; discarding the partial GPU text frame also failed: {discard_error}"
                                ))
                                .into());
                            }
                            return Err(io::Error::other(format!(
                                "late font activation failed after discarding the partial GPU text frame: {activation_error}"
                            ))
                            .into());
                        }
                    };
                    if let CatalogActivation::CatalogExpanded {
                        catalog_generation, ..
                    } = activation
                    {
                        renderer.discard_prepared_text_frame()?;
                        return Ok(CatalogFrameAttempt::Expanded(catalog_generation));
                    }
                }
                return Ok(CatalogFrameAttempt::Prepared(report));
            }
            let Some((_, catalog_generation)) = expansion else {
                return Err(io::Error::other("GPU text returned no frame outcome").into());
            };
            Ok(CatalogFrameAttempt::Expanded(catalog_generation))
        },
    )
}

fn retire_lost_renderer_before_rebuild<T>(
    lost_renderer: &mut GpuLayerRenderer,
    rebuild: impl FnOnce(&GpuLayerRenderer) -> Result<T, Box<dyn Error>>,
) -> Result<T, Box<dyn Error>> {
    lost_renderer.retire_text_cpu_font_state();
    rebuild(lost_renderer).map_err(|error| {
        io::Error::other(format!(
            "rebuild GPU renderer after retiring lost CPU font state: {error}"
        ))
        .into()
    })
}

#[derive(Debug, Default)]
struct DeviceRecoveryCoordinator {
    pending: bool,
}

impl DeviceRecoveryCoordinator {
    #[cfg(test)]
    const fn pending(&self) -> bool {
        self.pending
    }

    fn cancel_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one typed recovery transaction owns present, rebuild, classification, and metric callbacks"
    )]
    fn present<F, T, E>(
        &mut self,
        frame: &F,
        mut present: impl FnMut(&F) -> Result<T, E>,
        mut rebuild: impl FnMut() -> Result<(), E>,
        is_device_lost: impl Fn(&E) -> bool,
        is_presented: impl Fn(&T) -> bool,
        mut commit_recovery: impl FnMut(),
        mut record_recovery_failure: impl FnMut(),
    ) -> Result<T, E> {
        if self.pending {
            return match present(frame) {
                Ok(outcome) => {
                    if is_presented(&outcome) {
                        self.pending = false;
                        commit_recovery();
                    }
                    Ok(outcome)
                }
                Err(error) => {
                    self.pending = false;
                    record_recovery_failure();
                    Err(error)
                }
            };
        }

        match present(frame) {
            Err(error) if is_device_lost(&error) => {
                rebuild()?;
                match present(frame) {
                    Ok(outcome) => {
                        if is_presented(&outcome) {
                            commit_recovery();
                        } else {
                            self.pending = true;
                        }
                        Ok(outcome)
                    }
                    Err(error) => {
                        record_recovery_failure();
                        Err(error)
                    }
                }
            }
            outcome => outcome,
        }
    }
}

#[cfg(test)]
fn retry_device_lost_once<F, T, E>(
    frame: &F,
    present: impl FnMut(&F) -> Result<T, E>,
    rebuild: impl FnMut() -> Result<(), E>,
    is_device_lost: impl Fn(&E) -> bool,
    mut commit_recovery: impl FnMut(&T),
    record_retry_failure: impl FnMut(),
) -> Result<T, E> {
    DeviceRecoveryCoordinator::default()
        .present(
            frame,
            present,
            rebuild,
            is_device_lost,
            |_| true,
            || {},
            record_retry_failure,
        )
        .inspect(|outcome| commit_recovery(outcome))
}

fn should_apply_current_adapter_abandonment_workaround(
    os: &str,
    current: &GpuPresentationMetrics,
    shutdown_intent: bool,
    replaced_device: bool,
) -> bool {
    should_abandon_recovered_window_surface(
        os,
        &current.backend,
        current.adapter_vendor_id,
        shutdown_intent,
        replaced_device,
    )
}

fn should_abandon_current_adapter_after_native_close(
    os: &str,
    backend: &str,
    vendor_id: u32,
) -> bool {
    os == "windows" && backend.eq_ignore_ascii_case("vulkan") && vendor_id == 0x10de
}

fn gpu_context_options(
    high_performance: bool,
    force_fallback_adapter: bool,
    diagnostic_backend: Option<DiagnosticGpuBackend>,
) -> Result<GpuContextOptions, GpuContextError> {
    let options = GpuContextOptions::default()
        .with_high_performance(high_performance)
        .with_force_fallback_adapter(force_fallback_adapter);
    diagnostic_backend.map_or(Ok(options), |backend| {
        options.with_only_backend_name(backend.as_str())
    })
}

const fn diagnostic_font_catalog_mode(
    mode: Option<rssh_diagnostics::DiagnosticFontMode>,
) -> FontCatalogMode {
    match mode {
        None => production_font_catalog_mode(),
        Some(rssh_diagnostics::DiagnosticFontMode::CurrentCopied) => FontCatalogMode::CurrentCopied,
        Some(rssh_diagnostics::DiagnosticFontMode::SharedAll) => FontCatalogMode::SharedAll,
        Some(rssh_diagnostics::DiagnosticFontMode::Lazy) => FontCatalogMode::Lazy,
    }
}

pub(crate) const fn diagnostic_font_specimen_text(
    specimen: rssh_diagnostics::DiagnosticFontSpecimen,
) -> &'static str {
    match specimen {
        rssh_diagnostics::DiagnosticFontSpecimen::Ascii => "R-SSH Stage 7",
        rssh_diagnostics::DiagnosticFontSpecimen::Cjk => "中文",
        rssh_diagnostics::DiagnosticFontSpecimen::Emoji => "😀",
    }
}

fn sha256_hex(digest: [u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(feature = "diagnostic-tools")]
fn prepare_diagnostic_font_catalog(
    mut repository: PlatformFontRepository,
    mode: Option<rssh_diagnostics::DiagnosticFontMode>,
    specimen: Option<rssh_diagnostics::DiagnosticFontSpecimen>,
) -> Result<
    (
        PlatformFontRepository,
        rssh_fonts::FontCatalog,
        Option<DiagnosticFontResourceSummary>,
    ),
    Box<dyn Error>,
> {
    if mode.is_some() != specimen.is_some() {
        return Err(io::Error::other(
            "diagnostic font mode and specimen must be provided together",
        )
        .into());
    }
    let catalog_mode = diagnostic_font_catalog_mode(mode);
    let mut catalog = repository.build_catalog(catalog_mode)?;
    let Some((mode, specimen)) = mode.zip(specimen) else {
        return Ok((repository, catalog, None));
    };

    let activation_started = Instant::now();
    repository.preflight_text(diagnostic_font_specimen_text(specimen), &mut catalog)?;
    let activation_latency_micros =
        u64::try_from(activation_started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let recovered = repository.rebuild_catalog_from_active(catalog_mode)?;
    let diagnostics = repository.diagnostics();
    let catalog_metrics = catalog.memory_metrics();
    let recovery_metrics = recovered.memory_metrics();
    let exposes_full_inventory = matches!(
        mode,
        rssh_diagnostics::DiagnosticFontMode::CurrentCopied
            | rssh_diagnostics::DiagnosticFontMode::SharedAll
    );
    let summary = DiagnosticFontResourceSummary {
        mode,
        specimen,
        retained_source_bytes: catalog_metrics.retained_source_bytes,
        indexed_source_count: diagnostics.indexed_source_count,
        active_source_count: diagnostics.active_source_count,
        initial_catalog_source_count: diagnostics.initial_catalog_source_count,
        catalog_builds: diagnostics.catalog_builds,
        generation: diagnostics.generation,
        recovery_retained_source_bytes: recovery_metrics.retained_source_bytes,
        recovery_generation: recovered.generation(),
        activation_latency_micros,
        // The proof marker is not emitted until a complete GPU present has
        // replaced this placeholder with evidence from that exact frame.
        tofu_count: 0,
        frame_catalog_generation: None,
        frame_generation_consistent: None,
        index_fingerprint_sha256: sha256_hex(diagnostics.index_fingerprint),
        catalog_fingerprint_sha256: sha256_hex(diagnostics.catalog_fingerprint),
        ordered_catalog_fingerprint_sha256: sha256_hex(catalog.diagnostic_ordered_fingerprint()),
        font_inventory_fingerprint_sha256: exposes_full_inventory
            .then(|| sha256_hex(diagnostics.inventory_fingerprint)),
        font_index_policy_version: exposes_full_inventory.then_some(diagnostics.policy_version),
    };
    Ok((repository, catalog, Some(summary)))
}

fn record_diagnostic_font_frame_evidence(
    summary: &mut DiagnosticFontResourceSummary,
    frame_catalog_generation: u64,
    missing_glyphs: &[char],
) {
    summary.frame_catalog_generation = Some(frame_catalog_generation);
    summary.frame_generation_consistent = Some(frame_catalog_generation == summary.generation);
    let specimen = diagnostic_font_specimen_text(summary.specimen);
    summary.tofu_count = missing_glyphs
        .iter()
        .filter(|codepoint| {
            specimen.contains(**codepoint)
                && !codepoint.is_whitespace()
                && **codepoint != '\u{fe0f}'
        })
        .count();
}

#[cfg(feature = "diagnostic-tools")]
fn finalize_diagnostic_font_resource_summary(
    repository: &PlatformFontRepository,
    catalog_mode: FontCatalogMode,
    renderer: &mut GpuLayerRenderer,
    report: &GpuTextPrepareReport,
    summary: &mut DiagnosticFontResourceSummary,
) -> Result<(), Box<dyn Error>> {
    let diagnostics = repository.diagnostics();
    let catalog = renderer
        .text_catalog_mut()
        .ok_or_else(|| io::Error::other("window GPU text catalog is not enabled"))?;
    let catalog_metrics = catalog.memory_metrics();
    let catalog_generation = catalog.generation();
    let ordered_catalog_fingerprint = catalog.diagnostic_ordered_fingerprint();
    if diagnostics.active_source_count != catalog_metrics.active_source_count
        || diagnostics.catalog_builds != catalog_metrics.catalog_builds
        || diagnostics.generation != catalog_generation
    {
        return Err(io::Error::other(
            "platform font repository and presented GPU catalog epochs diverged",
        )
        .into());
    }
    let recovered = repository.rebuild_catalog_from_active(catalog_mode)?;
    let recovery_metrics = recovered.memory_metrics();

    summary.retained_source_bytes = catalog_metrics.retained_source_bytes;
    summary.indexed_source_count = diagnostics.indexed_source_count;
    summary.active_source_count = catalog_metrics.active_source_count;
    summary.initial_catalog_source_count = diagnostics.initial_catalog_source_count;
    summary.catalog_builds = catalog_metrics.catalog_builds;
    summary.generation = catalog_generation;
    summary.recovery_retained_source_bytes = recovery_metrics.retained_source_bytes;
    summary.recovery_generation = recovered.generation();
    summary.index_fingerprint_sha256 = sha256_hex(diagnostics.index_fingerprint);
    summary.catalog_fingerprint_sha256 = sha256_hex(diagnostics.catalog_fingerprint);
    summary.ordered_catalog_fingerprint_sha256 = sha256_hex(ordered_catalog_fingerprint);
    let exposes_full_inventory = matches!(
        catalog_mode,
        FontCatalogMode::CurrentCopied | FontCatalogMode::SharedAll
    );
    summary.font_inventory_fingerprint_sha256 =
        exposes_full_inventory.then(|| sha256_hex(diagnostics.inventory_fingerprint));
    summary.font_index_policy_version =
        exposes_full_inventory.then_some(diagnostics.policy_version);
    record_diagnostic_font_frame_evidence(
        summary,
        report.catalog_generation,
        &report.missing_glyphs,
    );
    Ok(())
}

#[cfg(feature = "diagnostic-tools")]
fn finalize_diagnostic_font_resource_summary_once(
    summary: &mut Option<DiagnosticFontResourceSummary>,
    finalize: impl FnOnce(&mut DiagnosticFontResourceSummary) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let Some(summary) = summary.as_mut() else {
        return Ok(());
    };
    if summary.frame_catalog_generation.is_some() {
        return Ok(());
    }
    finalize(summary)
}

#[cfg(not(feature = "diagnostic-tools"))]
fn prepare_diagnostic_font_catalog(
    mut repository: PlatformFontRepository,
    mode: Option<rssh_diagnostics::DiagnosticFontMode>,
    specimen: Option<rssh_diagnostics::DiagnosticFontSpecimen>,
) -> Result<
    (
        PlatformFontRepository,
        rssh_fonts::FontCatalog,
        Option<DiagnosticFontResourceSummary>,
    ),
    Box<dyn Error>,
> {
    if mode.is_some() || specimen.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "font proof requires diagnostic-tools",
        )
        .into());
    }
    let catalog = repository.build_catalog(production_font_catalog_mode())?;
    Ok((repository, catalog, None))
}

impl WindowGpu {
    pub(crate) async fn new(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        surface_size: PhysicalSize<u32>,
        high_performance: bool,
        force_fallback_adapter: bool,
    ) -> Result<Self, Box<dyn Error>> {
        Self::new_with_diagnostic_backend(
            display,
            window,
            surface_size,
            high_performance,
            force_fallback_adapter,
            None,
        )
        .await
    }

    pub(crate) async fn new_with_diagnostic_backend(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        surface_size: PhysicalSize<u32>,
        high_performance: bool,
        force_fallback_adapter: bool,
        diagnostic_backend: Option<DiagnosticGpuBackend>,
    ) -> Result<Self, Box<dyn Error>> {
        Self::new_with_diagnostic_options(
            display,
            window,
            surface_size,
            high_performance,
            force_fallback_adapter,
            diagnostic_backend,
            None,
            None,
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "private diagnostics forward independent backend, font mode, and specimen selections"
    )]
    pub(crate) async fn new_with_diagnostic_options(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        surface_size: PhysicalSize<u32>,
        high_performance: bool,
        force_fallback_adapter: bool,
        diagnostic_backend: Option<DiagnosticGpuBackend>,
        diagnostic_font_mode: Option<rssh_diagnostics::DiagnosticFontMode>,
        diagnostic_font_specimen: Option<rssh_diagnostics::DiagnosticFontSpecimen>,
    ) -> Result<Self, Box<dyn Error>> {
        let prepared = Self::prepare_with_diagnostic_options(
            display,
            window,
            surface_size,
            high_performance,
            force_fallback_adapter,
            diagnostic_backend,
            diagnostic_font_mode,
            diagnostic_font_specimen,
        )?;
        Self::finish_prepared(prepared).await
    }

    pub(crate) fn prepare(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        surface_size: PhysicalSize<u32>,
        high_performance: bool,
        force_fallback_adapter: bool,
    ) -> Result<PreparedWindowGpu, Box<dyn Error>> {
        Self::prepare_with_diagnostic_backend(
            display,
            window,
            surface_size,
            high_performance,
            force_fallback_adapter,
            None,
        )
    }

    pub(crate) fn prepare_with_diagnostic_backend(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        surface_size: PhysicalSize<u32>,
        high_performance: bool,
        force_fallback_adapter: bool,
        diagnostic_backend: Option<DiagnosticGpuBackend>,
    ) -> Result<PreparedWindowGpu, Box<dyn Error>> {
        Self::prepare_with_diagnostic_options(
            display,
            window,
            surface_size,
            high_performance,
            force_fallback_adapter,
            diagnostic_backend,
            None,
            None,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "private diagnostics forward independent backend, font mode, and specimen selections"
    )]
    pub(crate) fn prepare_with_diagnostic_options(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        surface_size: PhysicalSize<u32>,
        high_performance: bool,
        force_fallback_adapter: bool,
        diagnostic_backend: Option<DiagnosticGpuBackend>,
        diagnostic_font_mode: Option<rssh_diagnostics::DiagnosticFontMode>,
        diagnostic_font_specimen: Option<rssh_diagnostics::DiagnosticFontSpecimen>,
    ) -> Result<PreparedWindowGpu, Box<dyn Error>> {
        let options =
            gpu_context_options(high_performance, force_fallback_adapter, diagnostic_backend)?;
        let context = GpuContext::prepare_windowed(
            display,
            window,
            surface_size.width,
            surface_size.height,
            options,
        )?;
        Ok(PreparedWindowGpu {
            context,
            diagnostic_font_mode,
            diagnostic_font_specimen,
        })
    }

    pub(crate) async fn finish_prepared(
        prepared: PreparedWindowGpu,
    ) -> Result<Self, Box<dyn Error>> {
        let font_catalog_mode = diagnostic_font_catalog_mode(prepared.diagnostic_font_mode);
        let context = GpuContext::finish_windowed(prepared.context).await?;
        let (font_repository, catalog, diagnostic_font_resources) =
            prepare_diagnostic_font_catalog(
                PlatformFontRepository::production_index(),
                prepared.diagnostic_font_mode,
                prepared.diagnostic_font_specimen,
            )?;
        let renderer = bundled_emergency_text_backend(&context, catalog)?;
        Ok(Self {
            context: Some(context),
            renderer: Some(renderer),
            retired_renderers: Vec::new(),
            font_repository,
            font_catalog_mode,
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device: false,
            final_metrics: None,
            diagnostic_font_resources,
            #[cfg(test)]
            abandonment_workaround_adapter_match_override: None,
            #[cfg(test)]
            current_adapter_metrics_override: None,
            #[cfg(test)]
            abandonment_workaround_os_override: None,
            #[cfg(debug_assertions)]
            test_device_loss_injected: false,
        })
    }

    pub(crate) fn resize_surface(&mut self, size: PhysicalSize<u32>) -> Result<(), Box<dyn Error>> {
        self.context_mut().resize_surface(size.width, size.height)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn present(
        &mut self,
        window: &Window,
        snapshot: &TerminalRenderSnapshot,
        geometry: RenderGeometry,
        damage: &[DamageRegion],
        paint: &TextPaintConfig,
        graph: &RenderGraph,
        render_mode: rssh_native::RenderMode,
        dpi_scale: f32,
    ) -> Result<GpuFrameStatus, Box<dyn Error>> {
        #[cfg(debug_assertions)]
        if !self.test_device_loss_injected
            && std::env::var_os("RSSH_TEST_GPU_DEVICE_LOSS").is_some()
        {
            self.context_mut().inject_device_loss_for_test();
            self.test_device_loss_injected = true;
        }
        let frame = WindowGpuFrame {
            window,
            snapshot,
            geometry,
            damage,
            paint,
            graph,
            render_mode,
            dpi_scale,
        };
        let mut recovery = std::mem::take(&mut self.recovery);
        let state = RefCell::new(self);
        let outcome = recovery.present(
            &frame,
            |borrowed| state.borrow_mut().present_once(borrowed),
            || {
                state
                    .borrow_mut()
                    .execute_host_renderer_effect(&rssh_native::RendererEffect::RecoverDevice)
            },
            |error| {
                error
                    .as_ref()
                    .downcast_ref::<GpuContextError>()
                    .is_some_and(|error| error.kind() == GpuContextErrorKind::DeviceLost)
            },
            |outcome| outcome.0 == GpuFrameStatus::Presented,
            || {
                state
                    .borrow_mut()
                    .context_mut()
                    .commit_windowed_device_recovery();
            },
            || {
                state
                    .borrow_mut()
                    .context_mut()
                    .record_device_recovery_failure();
            },
        );
        let this = state.into_inner();
        this.recovery = recovery;
        let (status, report) = outcome?;
        if status == GpuFrameStatus::Presented {
            #[cfg(feature = "diagnostic-tools")]
            finalize_diagnostic_font_resource_summary_once(
                &mut this.diagnostic_font_resources,
                |resources| {
                    finalize_diagnostic_font_resource_summary(
                        &this.font_repository,
                        this.font_catalog_mode,
                        this.renderer
                            .as_mut()
                            .expect("window GPU renderer is available before shutdown"),
                        &report,
                        resources,
                    )
                },
            )?;
            #[cfg(not(feature = "diagnostic-tools"))]
            debug_assert!(this.diagnostic_font_resources.is_none());
            this.report = Some(report);
            this.rendered_frames = this.rendered_frames.saturating_add(1);
        }
        Ok(status)
    }

    fn present_once(
        &mut self,
        frame: &WindowGpuFrame<'_>,
    ) -> Result<(GpuFrameStatus, GpuTextPrepareReport), Box<dyn Error>> {
        let damage = presentation_damage(frame.render_mode, frame.damage);
        let renderer = self
            .renderer
            .as_mut()
            .expect("window GPU renderer is available before shutdown");
        let report = prepare_gpu_text_frame(
            &mut self.font_repository,
            renderer,
            frame.snapshot,
            frame.geometry,
            damage,
            frame.paint,
            frame.dpi_scale,
            1.0,
        )?;
        let status = self
            .context
            .as_mut()
            .expect("window GPU context is available before shutdown")
            .render_graph(
                self.renderer
                    .as_mut()
                    .expect("window GPU renderer is available before shutdown"),
                frame.graph,
                || {
                    frame.window.pre_present_notify();
                },
            )?;
        Ok((status, report))
    }

    fn rebuild_device_and_layers(&mut self) -> Result<(), Box<dyn Error>> {
        pollster::block_on(self.context_mut().recover_device())?;
        self.replaced_device = true;
        let font_catalog_mode = self.font_catalog_mode;
        let font_repository = &self.font_repository;
        let context = self
            .context
            .as_ref()
            .expect("window GPU context is available before shutdown");
        let lost_renderer = self
            .renderer
            .as_mut()
            .expect("lost renderer is available before recovery");
        let renderer = match retire_lost_renderer_before_rebuild(lost_renderer, |_| {
            let catalog = font_repository.rebuild_catalog_from_active(font_catalog_mode)?;
            bundled_emergency_text_backend(context, catalog)
        }) {
            Ok(renderer) => renderer,
            Err(error) => {
                self.context_mut().record_device_recovery_failure();
                return Err(error);
            }
        };
        let lost_renderer = self
            .renderer
            .replace(renderer)
            .expect("lost renderer is retained until shutdown");
        self.retired_renderers.push(lost_renderer);
        self.report = None;
        Ok(())
    }

    fn execute_host_renderer_effect(
        &mut self,
        effect: &rssh_native::RendererEffect,
    ) -> Result<(), Box<dyn Error>> {
        match effect {
            rssh_native::RendererEffect::RecoverDevice => self.rebuild_device_and_layers(),
            _ => Err(io::Error::other("renderer effect requires the frame adapter").into()),
        }
    }

    /// Applies the narrowly-scoped NVIDIA Vulkan window-close workaround.
    ///
    /// The manager calls this before removing every window. Unrecovered or
    /// non-matching windows keep their resources for normal Drop.
    pub(crate) fn shutdown_for_window_close(&mut self) -> bool {
        if self.recovery.cancel_pending()
            && let Some(context) = self.context.as_mut()
        {
            context.record_device_recovery_failure();
        }
        if !self.should_apply_abandonment_workaround() {
            return false;
        }
        let Some(mut context) = self.context.take() else {
            return false;
        };
        if self.replaced_device {
            context.record_abandoned_lost_surface();
        }
        self.final_metrics = Some(context.metrics().clone());
        if let Some(renderer) = self.renderer.take() {
            std::mem::forget(renderer);
        }
        for renderer in self.retired_renderers.drain(..) {
            std::mem::forget(renderer);
        }
        std::mem::forget(context);
        true
    }

    /// Avoids a validation-layer crash after the final native close request
    /// on the affected Windows NVIDIA Vulkan stack.
    pub(crate) fn shutdown_after_native_window_close(&mut self) -> bool {
        #[cfg(test)]
        let os = self
            .abandonment_workaround_os_override
            .unwrap_or(std::env::consts::OS);
        #[cfg(not(test))]
        let os = std::env::consts::OS;
        let should_abandon = self.context.as_ref().is_some_and(|context| {
            #[cfg(test)]
            let metrics = self
                .current_adapter_metrics_override
                .as_ref()
                .unwrap_or_else(|| context.metrics());
            #[cfg(not(test))]
            let metrics = context.metrics();
            should_abandon_current_adapter_after_native_close(
                os,
                &metrics.backend,
                metrics.adapter_vendor_id,
            )
        });
        if !should_abandon {
            return self.shutdown_for_window_close();
        }
        if self.recovery.cancel_pending()
            && let Some(context) = self.context.as_mut()
        {
            context.record_device_recovery_failure();
        }
        let Some(mut context) = self.context.take() else {
            return false;
        };
        if self.replaced_device {
            context.record_abandoned_lost_surface();
        }
        self.final_metrics = Some(context.metrics().clone());
        if let Some(renderer) = self.renderer.take() {
            std::mem::forget(renderer);
        }
        for renderer in self.retired_renderers.drain(..) {
            std::mem::forget(renderer);
        }
        std::mem::forget(context);
        eprintln!(
            "preserving Windows NVIDIA Vulkan GPU resources after native window close to avoid an unsafe driver shutdown"
        );
        true
    }

    #[cfg(test)]
    pub(crate) fn for_manager_close_test(
        abandonment_workaround_adapter_match: bool,
        replaced_device: bool,
    ) -> Self {
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("manager close test context");
        let mut current = GpuPresentationMetrics::uninitialized();
        current.backend = "Vulkan".to_owned();
        current.adapter_vendor_id = if abandonment_workaround_adapter_match {
            0x10de
        } else {
            0x1002
        };
        Self {
            context: Some(context),
            renderer: None,
            retired_renderers: Vec::new(),
            font_repository: PlatformFontRepository::production_index_for_os("test"),
            font_catalog_mode: production_font_catalog_mode(),
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device,
            final_metrics: None,
            diagnostic_font_resources: None,
            abandonment_workaround_adapter_match_override: Some(
                abandonment_workaround_adapter_match,
            ),
            current_adapter_metrics_override: Some(current),
            abandonment_workaround_os_override: Some("windows"),
            #[cfg(debug_assertions)]
            test_device_loss_injected: false,
        }
    }

    pub(crate) fn metrics(&self) -> &GpuPresentationMetrics {
        if let Some(context) = self.context.as_ref() {
            context.metrics()
        } else {
            self.final_metrics
                .as_ref()
                .expect("final GPU metrics are retained after window-close abandonment")
        }
    }

    pub(crate) fn diagnostic_font_resources(&self) -> Option<&DiagnosticFontResourceSummary> {
        self.diagnostic_font_resources.as_ref()
    }

    pub(crate) fn direct_text_metrics(&self) -> Option<(&GpuTextPrepareReport, u64)> {
        self.report
            .as_ref()
            .map(|report| (report, self.rendered_frames))
    }

    fn context_mut(&mut self) -> &mut GpuContext {
        self.context
            .as_mut()
            .expect("window GPU context is available before shutdown")
    }

    fn should_apply_abandonment_workaround(&self) -> bool {
        let Some(context) = self.context.as_ref() else {
            return false;
        };
        #[cfg(test)]
        if let Some(workaround_match) = self.abandonment_workaround_adapter_match_override {
            return workaround_match && self.replaced_device;
        }
        #[cfg(test)]
        let current = self
            .current_adapter_metrics_override
            .as_ref()
            .unwrap_or_else(|| context.metrics());
        #[cfg(not(test))]
        let current = context.metrics();
        #[cfg(test)]
        let os = self
            .abandonment_workaround_os_override
            .unwrap_or(std::env::consts::OS);
        #[cfg(not(test))]
        let os = std::env::consts::OS;
        should_apply_current_adapter_abandonment_workaround(os, current, true, self.replaced_device)
    }
}

fn presentation_damage(mode: rssh_native::RenderMode, damage: &[DamageRegion]) -> &[DamageRegion] {
    match mode {
        rssh_native::RenderMode::Full => &[],
        rssh_native::RenderMode::Damage => damage,
    }
}

impl Drop for WindowGpu {
    fn drop(&mut self) {
        if self.recovery.cancel_pending()
            && let Some(context) = self.context.as_mut()
        {
            context.record_device_recovery_failure();
        }
        release_renderers_before_context(
            &mut self.renderer,
            &mut self.retired_renderers,
            &mut self.context,
        );
    }
}

fn release_renderers_before_context<Renderer, Context>(
    renderer: &mut Option<Renderer>,
    retired_renderers: &mut Vec<Renderer>,
    context: &mut Option<Context>,
) {
    drop(renderer.take());
    retired_renderers.clear();
    drop(context.take());
}

fn bundled_emergency_text_backend(
    context: &GpuContext,
    catalog: rssh_fonts::FontCatalog,
) -> Result<GpuLayerRenderer, Box<dyn Error>> {
    use rssh_fonts::RasterCacheConfig;

    let font_config = bundled_emergency_font_config();
    let format = context
        .surface_format()
        .ok_or_else(|| io::Error::other("direct text fixture requires a surface format"))?;
    let mut renderer = GpuLayerRenderer::new(context, format, 64 * 1024)?;
    renderer.enable_text(
        catalog,
        font_config,
        GpuTextConfig::new(4 * 1024 * 1024, RasterCacheConfig::new(4 * 1024 * 1024)),
    )?;
    Ok(renderer)
}

#[cfg(test)]
fn bundled_emergency_font_catalog() -> Result<rssh_fonts::FontCatalog, Box<dyn Error>> {
    let mut repository = PlatformFontRepository::production_index();
    repository.build_catalog(production_font_catalog_mode())
}

fn bundled_emergency_font_config() -> rssh_fonts::FontConfig {
    rssh_fonts::FontConfig::new("Cascadia Mono")
        .with_fallbacks([
            "Cascadia Code",
            "Source Code Pro",
            "Consolas",
            "Menlo",
            "Monaco",
            "Noto Sans Mono",
            "DejaVu Sans Mono",
            "Noto Sans",
            "Noto Sans SC",
            "Noto Sans JP",
            "Hiragino Sans GB",
            "Arial Unicode MS",
            "Apple SD Gothic Neo",
            "Noto Sans Arabic",
            "Noto Sans Devanagari",
            "Noto Sans Hebrew",
            "Noto Sans Symbols 2",
            "Noto Color Emoji",
            "Microsoft YaHei",
            "Meiryo",
            "Malgun Gothic",
            "Segoe UI",
            "Nirmala UI",
            "Segoe UI Emoji",
        ])
        .with_font_size(17.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use rssh_core::TerminalSize;
    use rssh_diagnostics::DiagnosticGpuBackend;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    use rssh_fonts::TerminalShaper;
    use rssh_terminal::Terminal;
    use rterm_render_core::terminal_snapshot_content_digest;
    use winit::event_loop::OwnedDisplayHandle;

    fn construct_window_gpu_from_owned_display(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        size: PhysicalSize<u32>,
    ) {
        drop(WindowGpu::new(display, window, size, false, false));
    }

    fn prepare_window_gpu_from_owned_display(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        size: PhysicalSize<u32>,
    ) {
        drop(WindowGpu::prepare(display, window, size, false, false));
    }

    fn prepare_window_gpu_with_diagnostic_backend_from_owned_display(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        size: PhysicalSize<u32>,
        backend: Option<DiagnosticGpuBackend>,
    ) {
        drop(WindowGpu::prepare_with_diagnostic_backend(
            display, window, size, false, false, backend,
        ));
    }

    fn prepare_window_gpu_with_diagnostic_font_options_from_owned_display(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        size: PhysicalSize<u32>,
        backend: Option<DiagnosticGpuBackend>,
        font_mode: Option<rssh_diagnostics::DiagnosticFontMode>,
        font_specimen: Option<rssh_diagnostics::DiagnosticFontSpecimen>,
    ) {
        drop(WindowGpu::prepare_with_diagnostic_options(
            display,
            window,
            size,
            false,
            false,
            backend,
            font_mode,
            font_specimen,
        ));
    }

    fn finish_prepared_window_gpu(prepared: PreparedWindowGpu) {
        drop(WindowGpu::finish_prepared(prepared));
    }

    #[test]
    fn window_gpu_constructor_accepts_owned_display_handle() {
        std::hint::black_box(
            construct_window_gpu_from_owned_display
                as fn(OwnedDisplayHandle, Arc<Window>, PhysicalSize<u32>),
        );
    }

    #[test]
    fn window_gpu_surface_preparation_and_worker_initialization_are_split() {
        fn assert_send<T: Send>() {}

        assert_send::<PreparedWindowGpu>();
        std::hint::black_box(
            prepare_window_gpu_from_owned_display
                as fn(OwnedDisplayHandle, Arc<Window>, PhysicalSize<u32>),
        );
        std::hint::black_box(finish_prepared_window_gpu as fn(PreparedWindowGpu));
    }

    #[test]
    fn diagnostic_gpu_backend_prepare_accepts_explicit_selection_without_running_hardware() {
        std::hint::black_box(
            prepare_window_gpu_with_diagnostic_backend_from_owned_display
                as fn(
                    OwnedDisplayHandle,
                    Arc<Window>,
                    PhysicalSize<u32>,
                    Option<DiagnosticGpuBackend>,
                ),
        );
    }

    #[test]
    fn diagnostic_gpu_backend_builds_hardware_free_context_options() {
        let native_default = GpuContextOptions::default();
        let unselected = gpu_context_options(true, true, None).expect("native options");
        assert_eq!(unselected.backends, native_default.backends);
        assert_eq!(
            unselected.power_preference,
            native_default.with_high_performance(true).power_preference
        );
        assert!(unselected.force_fallback_adapter);

        for backend in [
            DiagnosticGpuBackend::Dx12,
            DiagnosticGpuBackend::Vulkan,
            DiagnosticGpuBackend::Gl,
        ] {
            let selected = gpu_context_options(false, false, Some(backend))
                .expect("supported diagnostic backend");
            let expected = native_default
                .with_only_backend_name(backend.as_str())
                .expect("known backend");
            assert_eq!(selected.backends, expected.backends);
            assert_eq!(selected.backends.bits().count_ones(), 1);
            assert_eq!(selected.power_preference, native_default.power_preference);
            assert!(!selected.force_fallback_adapter);
        }
    }

    #[test]
    fn diagnostic_font_mode_maps_exactly_without_changing_production_default() {
        use rssh_diagnostics::DiagnosticFontMode;

        assert_eq!(
            diagnostic_font_catalog_mode(None),
            production_font_catalog_mode()
        );
        assert_eq!(
            diagnostic_font_catalog_mode(Some(DiagnosticFontMode::CurrentCopied)),
            FontCatalogMode::CurrentCopied
        );
        assert_eq!(
            diagnostic_font_catalog_mode(Some(DiagnosticFontMode::SharedAll)),
            FontCatalogMode::SharedAll
        );
        assert_eq!(
            diagnostic_font_catalog_mode(Some(DiagnosticFontMode::Lazy)),
            FontCatalogMode::Lazy
        );
    }

    #[test]
    fn diagnostic_font_mode_prepare_accepts_typed_options_without_running_hardware() {
        std::hint::black_box(
            prepare_window_gpu_with_diagnostic_font_options_from_owned_display
                as fn(
                    OwnedDisplayHandle,
                    Arc<Window>,
                    PhysicalSize<u32>,
                    Option<DiagnosticGpuBackend>,
                    Option<rssh_diagnostics::DiagnosticFontMode>,
                    Option<rssh_diagnostics::DiagnosticFontSpecimen>,
                ),
        );
    }

    #[test]
    fn diagnostic_font_specimens_activate_one_bounded_batch_without_tofu() {
        use rssh_diagnostics::{DiagnosticFontMode, DiagnosticFontSpecimen};

        for specimen in [DiagnosticFontSpecimen::Cjk, DiagnosticFontSpecimen::Emoji] {
            let repository = PlatformFontRepository::production_index_for_os("test");
            let (_repository, _catalog, summary) = prepare_diagnostic_font_catalog(
                repository,
                Some(DiagnosticFontMode::Lazy),
                Some(specimen),
            )
            .expect("prepare bounded specimen");
            let summary = summary.expect("diagnostic resource summary");
            assert_eq!(summary.mode, DiagnosticFontMode::Lazy);
            assert_eq!(summary.specimen, specimen);
            assert_eq!(summary.catalog_builds, 2);
            assert_eq!(summary.generation, 2);
            assert_eq!(summary.tofu_count, 0, "specimen {specimen:?}");
            assert_eq!(
                summary.recovery_retained_source_bytes,
                summary.retained_source_bytes
            );
            assert_eq!(summary.recovery_generation, summary.generation);
        }
        assert_eq!(
            diagnostic_font_specimen_text(DiagnosticFontSpecimen::Emoji),
            "😀"
        );
    }

    #[test]
    fn diagnostic_font_resource_summary_serializes_irreversible_identity_without_paths() {
        use rssh_diagnostics::{DiagnosticFontMode, DiagnosticFontSpecimen};

        let repository = PlatformFontRepository::production_index_for_os("test");
        let (_, _, summary) = prepare_diagnostic_font_catalog(
            repository,
            Some(DiagnosticFontMode::SharedAll),
            Some(DiagnosticFontSpecimen::Ascii),
        )
        .expect("prepare shared proof catalog");
        let value = serde_json::to_value(summary.expect("resource summary")).unwrap();
        let rendered = value.to_string();

        assert_eq!(value["mode"], "shared");
        assert_eq!(value["specimen"], "ascii");
        for key in ["index_fingerprint_sha256", "catalog_fingerprint_sha256"] {
            assert_eq!(value[key].as_str().expect("digest").len(), 64);
        }
        assert!(!rendered.contains(r"C:\Windows\Fonts"));
        assert!(!rendered.to_ascii_lowercase().contains("path"));
    }

    #[test]
    fn diagnostic_font_frame_evidence_is_derived_from_the_presented_specimen_frame() {
        use rssh_diagnostics::{DiagnosticFontMode, DiagnosticFontSpecimen};

        let repository = PlatformFontRepository::production_index_for_os("test");
        let (_, _, summary) = prepare_diagnostic_font_catalog(
            repository,
            Some(DiagnosticFontMode::Lazy),
            Some(DiagnosticFontSpecimen::Cjk),
        )
        .expect("prepare summary");
        let mut summary = summary.expect("font summary");

        let generation = summary.generation;
        record_diagnostic_font_frame_evidence(&mut summary, generation, &['中']);
        assert_eq!(summary.frame_catalog_generation, Some(summary.generation));
        assert_eq!(summary.frame_generation_consistent, Some(true));
        assert_eq!(summary.tofu_count, 1);
        record_diagnostic_font_frame_evidence(&mut summary, generation, &[]);
        assert_eq!(summary.tofu_count, 0);
        let mixed_generation = summary.generation.saturating_add(1);
        record_diagnostic_font_frame_evidence(&mut summary, mixed_generation, &[]);
        assert_eq!(summary.frame_generation_consistent, Some(false));
    }

    #[test]
    fn diagnostic_font_summary_finalizes_only_the_first_presented_proof_frame() {
        use std::cell::Cell;

        use rssh_diagnostics::{DiagnosticFontMode, DiagnosticFontSpecimen};

        let (_, _, summary) = prepare_diagnostic_font_catalog(
            PlatformFontRepository::production_index_for_os("test"),
            Some(DiagnosticFontMode::Lazy),
            Some(DiagnosticFontSpecimen::Ascii),
        )
        .expect("prepare diagnostic catalog");
        let mut summary = summary;
        let finalizations = Cell::new(0_u32);

        for generation in [1_u64, 2] {
            finalize_diagnostic_font_resource_summary_once(&mut summary, |resources| {
                finalizations.set(finalizations.get() + 1);
                resources.frame_catalog_generation = Some(generation);
                Ok(())
            })
            .expect("finalize proof summary");
        }

        assert_eq!(finalizations.get(), 1);
        assert_eq!(
            summary
                .as_ref()
                .and_then(|resources| resources.frame_catalog_generation),
            Some(1)
        );
    }

    #[test]
    fn diagnostic_font_summary_is_rederived_from_the_actual_presented_catalog_epoch() {
        use rssh_diagnostics::{DiagnosticFontMode, DiagnosticFontSpecimen};

        let mode = DiagnosticFontMode::Lazy;
        let specimen = DiagnosticFontSpecimen::Emoji;
        let (mut repository, catalog, summary) = prepare_diagnostic_font_catalog(
            PlatformFontRepository::production_index_for_os("test"),
            Some(mode),
            Some(specimen),
        )
        .expect("prepare diagnostic catalog");
        let mut summary = summary.expect("diagnostic resource summary");
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("headless adapter");
        let mut renderer =
            GpuLayerRenderer::new_headless(&context, 64 * 1024).expect("GPU layer renderer");
        renderer
            .enable_text(
                catalog,
                bundled_emergency_font_config(),
                GpuTextConfig::new(
                    4 * 1024 * 1024,
                    rssh_fonts::RasterCacheConfig::new(4 * 1024 * 1024),
                ),
            )
            .expect("GPU text");
        let mut terminal = Terminal::new(TerminalSize::new(16, 1));
        terminal.feed(diagnostic_font_specimen_text(specimen).as_bytes());
        let report = prepare_gpu_text_frame(
            &mut repository,
            &mut renderer,
            &TerminalRenderSnapshot::from_terminal(&terminal),
            RenderGeometry::new(16 * 16, 24, 16, 24),
            &[],
            &TextPaintConfig::default(),
            1.0,
            1.0,
        )
        .expect("prepare actual specimen frame");

        summary.retained_source_bytes = 0;
        summary.active_source_count = 0;
        summary.catalog_builds = 0;
        summary.generation = 0;
        summary.recovery_retained_source_bytes = 0;
        summary.recovery_generation = 0;
        summary.catalog_fingerprint_sha256.clear();
        summary.ordered_catalog_fingerprint_sha256.clear();
        finalize_diagnostic_font_resource_summary(
            &repository,
            FontCatalogMode::Lazy,
            &mut renderer,
            &report,
            &mut summary,
        )
        .expect("derive final evidence from the presented catalog epoch");

        let diagnostics = repository.diagnostics();
        let catalog = renderer.text_catalog_mut().expect("actual catalog");
        let metrics = catalog.memory_metrics();
        assert_eq!(summary.retained_source_bytes, metrics.retained_source_bytes);
        assert_eq!(summary.active_source_count, metrics.active_source_count);
        assert_eq!(summary.initial_catalog_source_count, 1);
        assert_eq!(summary.catalog_builds, metrics.catalog_builds);
        assert_eq!(summary.generation, catalog.generation());
        assert_eq!(
            summary.indexed_source_count,
            diagnostics.indexed_source_count
        );
        assert_eq!(summary.frame_catalog_generation, Some(catalog.generation()));
        assert_eq!(summary.frame_generation_consistent, Some(true));
        assert_eq!(summary.tofu_count, 0);
        assert_eq!(
            summary.recovery_retained_source_bytes,
            summary.retained_source_bytes
        );
        assert_eq!(summary.recovery_generation, summary.generation);
        assert_eq!(summary.catalog_fingerprint_sha256.len(), 64);
        assert_eq!(summary.ordered_catalog_fingerprint_sha256.len(), 64);
    }

    #[test]
    fn native_presentation_mode_is_the_only_gpu_damage_selector() {
        let damage = [DamageRegion::new(1, 2, 3, 4)];

        assert!(presentation_damage(rssh_native::RenderMode::Full, &damage).is_empty());
        assert_eq!(
            presentation_damage(rssh_native::RenderMode::Damage, &damage),
            damage
        );
    }

    #[test]
    fn gpu_text_frame_late_missing_activates_next_source_and_restarts_without_tofu() {
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("headless adapter");
        let mut repository = PlatformFontRepository::late_missing_fixture();
        let catalog = repository
            .build_catalog(FontCatalogMode::Lazy)
            .expect("primary catalog");
        let mut renderer =
            GpuLayerRenderer::new_headless(&context, 64 * 1024).expect("GPU layer renderer");
        renderer
            .enable_text(
                catalog,
                bundled_emergency_font_config(),
                GpuTextConfig::new(
                    4 * 1024 * 1024,
                    rssh_fonts::RasterCacheConfig::new(4 * 1024 * 1024),
                ),
            )
            .expect("GPU text");
        let geometry = RenderGeometry::new(8 * 16, 2 * 24, 16, 24);
        let paint = TextPaintConfig::default();

        let mut priming_terminal = Terminal::new(TerminalSize::new(8, 2));
        priming_terminal.feed(b"ASCII\r\nrow");
        prepare_gpu_text_frame(
            &mut repository,
            &mut renderer,
            &TerminalRenderSnapshot::from_terminal(&priming_terminal),
            geometry,
            &[],
            &paint,
            1.0,
            1.0,
        )
        .expect("prime generation one rows");

        let mut terminal = Terminal::new(TerminalSize::new(8, 2));
        terminal.feed("ASCII\r\n中文".as_bytes());
        let report = prepare_gpu_text_frame(
            &mut repository,
            &mut renderer,
            &TerminalRenderSnapshot::from_terminal(&terminal),
            geometry,
            &[DamageRegion::new(0, 1, 2, 1)],
            &paint,
            1.0,
            1.0,
        )
        .expect("late missing source expansion and one full-frame retry");

        assert_eq!(report.catalog_generation, 3);
        assert_eq!(report.prepared_rows, [0, 1]);
        assert!(report.missing_glyphs.is_empty());
        let diagnostics = repository.diagnostics();
        assert_eq!(diagnostics.active_source_count, 3);
        assert_eq!(diagnostics.generation, 3);
    }

    #[test]
    fn gpu_text_frame_second_late_missing_stays_tofu_without_a_second_expansion() {
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("headless adapter");
        let mut repository = PlatformFontRepository::repeated_late_missing_fixture();
        let catalog = repository
            .build_catalog(FontCatalogMode::Lazy)
            .expect("primary catalog");
        let mut renderer =
            GpuLayerRenderer::new_headless(&context, 64 * 1024).expect("GPU layer renderer");
        renderer
            .enable_text(
                catalog,
                bundled_emergency_font_config(),
                GpuTextConfig::new(
                    4 * 1024 * 1024,
                    rssh_fonts::RasterCacheConfig::new(4 * 1024 * 1024),
                ),
            )
            .expect("GPU text");
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));
        terminal.feed("中文".as_bytes());

        let report = prepare_gpu_text_frame(
            &mut repository,
            &mut renderer,
            &TerminalRenderSnapshot::from_terminal(&terminal),
            RenderGeometry::new(4 * 16, 24, 16, 24),
            &[],
            &TextPaintConfig::default(),
            1.0,
            1.0,
        )
        .expect("one late expansion followed by stable tofu");

        assert_eq!(report.catalog_generation, 3);
        assert_eq!(report.missing_glyphs, ['中', '文']);
        let diagnostics = repository.diagnostics();
        assert_eq!(diagnostics.active_source_count, 3);
        assert_eq!(diagnostics.generation, 3);
    }

    #[test]
    fn gpu_text_frame_invalid_late_fallback_discards_partial_state_before_error() {
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("headless adapter");
        let mut repository = PlatformFontRepository::invalid_late_missing_fixture();
        let catalog = repository
            .build_catalog(FontCatalogMode::Lazy)
            .expect("primary catalog");
        let mut renderer =
            GpuLayerRenderer::new_headless(&context, 64 * 1024).expect("GPU layer renderer");
        renderer
            .enable_text(
                catalog,
                bundled_emergency_font_config(),
                GpuTextConfig::new(
                    4 * 1024 * 1024,
                    rssh_fonts::RasterCacheConfig::new(4 * 1024 * 1024),
                ),
            )
            .expect("GPU text");
        let mut terminal = Terminal::new(TerminalSize::new(4, 1));
        terminal.feed("中文".as_bytes());
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

        for _ in 0..2 {
            let error = prepare_gpu_text_frame(
                &mut repository,
                &mut renderer,
                &snapshot,
                RenderGeometry::new(4 * 16, 24, 16, 24),
                &[],
                &TextPaintConfig::default(),
                1.0,
                1.0,
            )
            .expect_err("readable invalid late fallback must fail");
            let cpu = renderer.text_cpu_font_metrics().expect("font metrics");
            assert_eq!(cpu.row_cache_entries, 0);
            assert_eq!(cpu.payload_bytes, 0);
            let atlas = renderer.text_atlas_metrics().expect("atlas metrics");
            assert_eq!(atlas.entries, 0);
            assert_eq!(atlas.payload_bytes, 0);
            assert!(error.to_string().contains("late font activation failed"));
        }
    }

    #[test]
    fn recovery_retires_real_cpu_font_state_before_building_the_replacement() {
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("headless adapter");
        let mut repository = PlatformFontRepository::late_missing_fixture();
        let catalog = repository
            .build_catalog(FontCatalogMode::CurrentCopied)
            .expect("active copied catalog");
        let mut lost = GpuLayerRenderer::new_headless(&context, 64 * 1024).expect("lost renderer");
        lost.enable_text(
            catalog,
            bundled_emergency_font_config(),
            GpuTextConfig::new(
                4 * 1024 * 1024,
                rssh_fonts::RasterCacheConfig::new(4 * 1024 * 1024),
            ),
        )
        .expect("lost text state");
        let old_bytes = lost
            .text_cpu_font_metrics()
            .expect("lost font metrics")
            .catalog
            .retained_source_bytes;
        assert!(old_bytes > 0);
        let peak = Cell::new(old_bytes);

        let replacement = retire_lost_renderer_before_rebuild(&mut lost, |retired| {
            let retired_metrics = retired.text_cpu_font_metrics().expect("retired metrics");
            assert_eq!(retired_metrics.catalog.retained_source_bytes, 0);
            let recovered_catalog = repository
                .rebuild_catalog_from_active(FontCatalogMode::CurrentCopied)
                .expect("recovered active catalog");
            let mut replacement =
                GpuLayerRenderer::new_headless(&context, 64 * 1024).expect("replacement renderer");
            replacement
                .enable_text(
                    recovered_catalog,
                    bundled_emergency_font_config(),
                    GpuTextConfig::new(
                        4 * 1024 * 1024,
                        rssh_fonts::RasterCacheConfig::new(4 * 1024 * 1024),
                    ),
                )
                .expect("replacement text state");
            let replacement_bytes = replacement
                .text_cpu_font_metrics()
                .expect("replacement metrics")
                .catalog
                .retained_source_bytes;
            peak.set(peak.get().max(replacement_bytes));
            Ok::<_, Box<dyn Error>>(replacement)
        })
        .expect("retire then rebuild");

        assert_eq!(
            lost.text_cpu_font_metrics()
                .expect("retired metrics")
                .catalog
                .retained_source_bytes,
            0
        );
        assert!(
            replacement
                .text_cpu_font_metrics()
                .expect("replacement metrics")
                .catalog
                .retained_source_bytes
                > 0
        );
        assert_eq!(peak.get(), old_bytes, "old and new bytes must not overlap");

        let mut failed = replacement;
        let error = retire_lost_renderer_before_rebuild(&mut failed, |retired| {
            assert_eq!(
                retired
                    .text_cpu_font_metrics()
                    .expect("failed retired metrics")
                    .catalog
                    .retained_source_bytes,
                0
            );
            Err::<GpuLayerRenderer, _>(io::Error::other("injected replacement failure").into())
        })
        .expect_err("replacement failure must remain fail closed");
        assert!(error.to_string().contains("injected replacement failure"));
        assert_eq!(
            failed
                .text_cpu_font_metrics()
                .expect("failed renderer metrics")
                .catalog
                .active_source_count,
            0
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn emergency_font_catalog_covers_common_cli_ui_scripts() {
        let mut catalog = bundled_emergency_font_catalog().expect("fixture font catalog");
        let mut shaper = TerminalShaper::new(bundled_emergency_font_config());
        let row = shaper
            .shape_row(
                &mut catalog,
                "中文显示测试 日本語 한국어 العربية हिन्दी ×▾—□…",
            )
            .expect("shape common CLI UI scripts");

        assert!(
            row.clusters.iter().all(|cluster| !cluster.is_tofu),
            "emergency GPU font catalog must not render common UI scripts as tofu: {:?}",
            row.clusters
                .iter()
                .filter(|cluster| cluster.is_tofu)
                .map(|cluster| &row.text[cluster.byte_range.clone()])
                .collect::<Vec<_>>()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn emergency_font_catalog_covers_mac_cjk_fallback() {
        let mut catalog = bundled_emergency_font_catalog().expect("fixture font catalog");
        let mut shaper = TerminalShaper::new(bundled_emergency_font_config());
        let row = shaper
            .shape_row(&mut catalog, "中文显示测试 日本語繁體字")
            .expect("shape macOS CJK terminal sample");

        assert!(
            row.clusters.iter().all(|cluster| !cluster.is_tofu),
            "macOS system CJK fallback must not render Chinese or Japanese as tofu: {:?}",
            row.clusters
                .iter()
                .filter(|cluster| cluster.is_tofu)
                .map(|cluster| &row.text[cluster.byte_range.clone()])
                .collect::<Vec<_>>()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn modern_default_font_prefers_cascadia_mono_and_preserves_terminal_cells() {
        let config = bundled_emergency_font_config();
        assert_eq!(config.primary(), "Cascadia Mono");

        let mut catalog = bundled_emergency_font_catalog().expect("fixture font catalog");
        let mut shaper = TerminalShaper::new(config);
        let row = shaper
            .shape_row(&mut catalog, "R-SSH 你好 😀")
            .expect("shape modern terminal sample");

        assert!((row.metrics.font_size - 17.0).abs() < f32::EPSILON);
        assert_eq!(
            row.clusters
                .iter()
                .map(|cluster| cluster.cell_span.clone())
                .collect::<Vec<_>>(),
            vec![
                0..1,
                1..2,
                2..3,
                3..4,
                4..5,
                5..6,
                6..8,
                8..10,
                10..11,
                11..13,
            ],
            "shaping must preserve the terminal's logical cell spans"
        );

        for cluster in &row.clusters {
            let text = &row.text[cluster.byte_range.clone()];
            if text
                .chars()
                .all(|character| character.is_ascii() && character != ' ')
            {
                assert_eq!(
                    cluster.font_family, "Cascadia Mono",
                    "Latin terminal glyphs should use the preferred monospace face"
                );
            }
            if text.chars().any(|character| "你好".contains(character)) {
                assert!(
                    !cluster.is_tofu,
                    "CJK fallback must resolve a visible glyph"
                );
            }
        }
    }

    #[test]
    fn native_app_manifest_has_no_pixels_runtime_dependency() {
        let manifest: toml::Value =
            toml::from_str(include_str!("../Cargo.toml")).expect("parse rssh-app manifest");
        assert!(
            manifest["dependencies"].get("pixels").is_none(),
            "the promoted native renderer must not depend on the Pixels compatibility frontend"
        );
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestPresentError {
        DeviceLost,
        Validation,
    }

    #[test]
    fn window_gpu_releases_every_renderer_before_its_context() {
        #[derive(Debug)]
        struct TrackedDrop {
            label: &'static str,
            drops: Rc<RefCell<Vec<&'static str>>>,
        }

        impl Drop for TrackedDrop {
            fn drop(&mut self) {
                self.drops.borrow_mut().push(self.label);
            }
        }

        let drops = Rc::new(RefCell::new(Vec::new()));
        let mut renderer = Some(TrackedDrop {
            label: "active-renderer",
            drops: Rc::clone(&drops),
        });
        let mut retired_renderers = vec![TrackedDrop {
            label: "retired-renderer",
            drops: Rc::clone(&drops),
        }];
        let mut context = Some(TrackedDrop {
            label: "context",
            drops: Rc::clone(&drops),
        });

        release_renderers_before_context(&mut renderer, &mut retired_renderers, &mut context);

        assert_eq!(
            drops.borrow().as_slice(),
            ["active-renderer", "retired-renderer", "context"]
        );
        assert!(renderer.is_none());
        assert!(retired_renderers.is_empty());
        assert!(context.is_none());
    }

    #[test]
    fn abandonment_workaround_requires_proven_adapter_and_explicit_final_shutdown() {
        assert!(should_abandon_recovered_window_surface(
            "windows", "Vulkan", 0x10de, true, true
        ));
        assert!(should_abandon_recovered_window_surface(
            "windows", "vulkan", 0x10de, true, true
        ));
        for (os, backend, vendor, shutdown, replaced) in [
            ("linux", "Vulkan", 0x10de, true, true),
            ("windows", "Dx12", 0x10de, true, true),
            ("windows", "Vulkan", 0x1002, true, true),
            ("windows", "Vulkan", 0x10de, false, true),
            ("windows", "Vulkan", 0x10de, true, false),
        ] {
            assert!(
                !should_abandon_recovered_window_surface(os, backend, vendor, shutdown, replaced),
                "{os}/{backend}/{vendor:#x} shutdown={shutdown} replaced={replaced}"
            );
        }
    }

    #[test]
    fn native_close_abandons_only_windows_nvidia_vulkan_devices() {
        assert!(should_abandon_current_adapter_after_native_close(
            "windows", "Vulkan", 0x10de
        ));
        for (os, backend, vendor) in [
            ("linux", "Vulkan", 0x10de),
            ("windows", "Dx12", 0x10de),
            ("windows", "Vulkan", 0x1002),
        ] {
            assert!(!should_abandon_current_adapter_after_native_close(
                os, backend, vendor
            ));
        }
    }

    #[test]
    fn abandonment_eligibility_tracks_the_recovered_current_adapter() {
        let mut current = GpuPresentationMetrics::uninitialized();
        current.backend = "Vulkan".to_owned();
        current.adapter_vendor_id = 0x10de;
        assert!(should_apply_current_adapter_abandonment_workaround(
            "windows", &current, true, true
        ));

        current.adapter_vendor_id = 0x1002;
        assert!(
            !should_apply_current_adapter_abandonment_workaround("windows", &current, true, true),
            "initial NVIDIA must not remain cached after recovery selects non-NVIDIA"
        );

        let mut current = GpuPresentationMetrics::uninitialized();
        current.backend = "Vulkan".to_owned();
        current.adapter_vendor_id = 0x1002;
        assert!(!should_apply_current_adapter_abandonment_workaround(
            "windows", &current, true, true
        ));

        current.adapter_vendor_id = 0x10de;
        assert!(
            should_apply_current_adapter_abandonment_workaround("windows", &current, true, true),
            "initial non-NVIDIA must not suppress a recovered NVIDIA adapter"
        );
    }

    #[test]
    fn window_gpu_records_abandonment_only_when_exit_shutdown_actually_forgets_resources() {
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("headless context");
        let mut gpu = WindowGpu {
            context: Some(context),
            renderer: None,
            retired_renderers: Vec::new(),
            font_repository: PlatformFontRepository::production_index_for_os("test"),
            font_catalog_mode: production_font_catalog_mode(),
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device: true,
            final_metrics: None,
            diagnostic_font_resources: None,
            abandonment_workaround_adapter_match_override: Some(true),
            current_adapter_metrics_override: None,
            abandonment_workaround_os_override: None,
            #[cfg(debug_assertions)]
            test_device_loss_injected: false,
        };
        assert_eq!(gpu.metrics().abandoned_lost_surfaces, 0);
        gpu.recovery.pending = true;

        assert!(gpu.shutdown_for_window_close());
        assert_eq!(gpu.metrics().abandoned_lost_surfaces, 1);
        assert_eq!(gpu.metrics().device_recoveries, 0);
        assert_eq!(gpu.metrics().device_recovery_failures, 1);
        assert!(!gpu.shutdown_for_window_close());
        assert_eq!(gpu.metrics().abandoned_lost_surfaces, 1);

        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("second headless context");
        let mut ordinary_close = WindowGpu {
            context: Some(context),
            renderer: None,
            retired_renderers: Vec::new(),
            font_repository: PlatformFontRepository::production_index_for_os("test"),
            font_catalog_mode: production_font_catalog_mode(),
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device: true,
            final_metrics: None,
            diagnostic_font_resources: None,
            abandonment_workaround_adapter_match_override: Some(false),
            current_adapter_metrics_override: None,
            abandonment_workaround_os_override: None,
            #[cfg(debug_assertions)]
            test_device_loss_injected: false,
        };
        assert!(!ordinary_close.shutdown_for_window_close());
        assert_eq!(ordinary_close.metrics().abandoned_lost_surfaces, 0);
        assert!(ordinary_close.context.is_some());
        assert!(ordinary_close.final_metrics.is_none());
    }

    #[test]
    fn current_adapter_shutdown_path_is_idempotent_after_context_is_taken() {
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("headless context");
        let mut recovered_metrics = GpuPresentationMetrics::uninitialized();
        recovered_metrics.backend = "Vulkan".to_owned();
        recovered_metrics.adapter_vendor_id = 0x10de;
        let mut gpu = WindowGpu {
            context: Some(context),
            renderer: None,
            retired_renderers: Vec::new(),
            font_repository: PlatformFontRepository::production_index_for_os("test"),
            font_catalog_mode: production_font_catalog_mode(),
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device: true,
            final_metrics: None,
            diagnostic_font_resources: None,
            abandonment_workaround_adapter_match_override: None,
            current_adapter_metrics_override: Some(recovered_metrics),
            abandonment_workaround_os_override: Some("windows"),
            #[cfg(debug_assertions)]
            test_device_loss_injected: false,
        };

        assert!(gpu.shutdown_for_window_close());
        assert!(!gpu.shutdown_for_window_close());
        assert_eq!(gpu.metrics().abandoned_lost_surfaces, 1);
    }

    #[test]
    fn skipped_recovery_stays_pending_until_a_later_presented_frame_commits_once() {
        let frame = ();
        let attempts = Cell::new(0);
        let rebuilds = Cell::new(0);
        let recoveries = Cell::new(0);
        let failures = Cell::new(0);
        let mut coordinator = DeviceRecoveryCoordinator::default();
        let outcomes = RefCell::new(
            [
                Err(TestPresentError::DeviceLost),
                Ok(GpuFrameStatus::Skipped),
                Ok(GpuFrameStatus::Presented),
            ]
            .into_iter(),
        );

        let first = coordinator
            .present(
                &frame,
                |()| {
                    attempts.set(attempts.get() + 1);
                    outcomes.borrow_mut().next().expect("scripted outcome")
                },
                || {
                    rebuilds.set(rebuilds.get() + 1);
                    Ok(())
                },
                |error| *error == TestPresentError::DeviceLost,
                |status| *status == GpuFrameStatus::Presented,
                || recoveries.set(recoveries.get() + 1),
                || failures.set(failures.get() + 1),
            )
            .expect("skipped retry");
        assert_eq!(first, GpuFrameStatus::Skipped);
        assert!(coordinator.pending());
        assert_eq!(recoveries.get(), 0);
        assert_eq!(failures.get(), 0);

        let second = coordinator
            .present(
                &frame,
                |()| {
                    attempts.set(attempts.get() + 1);
                    outcomes.borrow_mut().next().expect("scripted outcome")
                },
                || {
                    rebuilds.set(rebuilds.get() + 1);
                    Ok(())
                },
                |error| *error == TestPresentError::DeviceLost,
                |status| *status == GpuFrameStatus::Presented,
                || recoveries.set(recoveries.get() + 1),
                || failures.set(failures.get() + 1),
            )
            .expect("later presented frame");
        assert_eq!(second, GpuFrameStatus::Presented);
        assert!(!coordinator.pending());
        assert_eq!(attempts.get(), 3);
        assert_eq!(rebuilds.get(), 1);
        assert_eq!(recoveries.get(), 1);
        assert_eq!(failures.get(), 0);
    }

    #[test]
    fn pending_recovery_fails_once_without_starting_a_second_rebuild() {
        let frame = ();
        let rebuilds = Cell::new(0);
        let recoveries = Cell::new(0);
        let failures = Cell::new(0);
        let mut coordinator = DeviceRecoveryCoordinator::default();
        let outcomes = RefCell::new(
            [
                Err(TestPresentError::DeviceLost),
                Ok(GpuFrameStatus::Skipped),
                Err(TestPresentError::DeviceLost),
            ]
            .into_iter(),
        );

        coordinator
            .present(
                &frame,
                |()| outcomes.borrow_mut().next().expect("scripted outcome"),
                || {
                    rebuilds.set(rebuilds.get() + 1);
                    Ok(())
                },
                |error| *error == TestPresentError::DeviceLost,
                |status| *status == GpuFrameStatus::Presented,
                || recoveries.set(recoveries.get() + 1),
                || failures.set(failures.get() + 1),
            )
            .expect("skipped retry");
        let failed = coordinator.present(
            &frame,
            |()| outcomes.borrow_mut().next().expect("scripted outcome"),
            || {
                rebuilds.set(rebuilds.get() + 1);
                Ok(())
            },
            |error| *error == TestPresentError::DeviceLost,
            |status| *status == GpuFrameStatus::Presented,
            || recoveries.set(recoveries.get() + 1),
            || failures.set(failures.get() + 1),
        );

        assert_eq!(failed, Err(TestPresentError::DeviceLost));
        assert!(!coordinator.pending());
        assert_eq!(rebuilds.get(), 1);
        assert_eq!(recoveries.get(), 0);
        assert_eq!(failures.get(), 1);
    }

    #[test]
    fn device_loss_recovery_rebuilds_once_and_retries_the_same_snapshot() {
        let mut terminal = Terminal::new(TerminalSize::new(2, 1));
        terminal.feed(b"ok");
        let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
        let expected_digest = terminal_snapshot_content_digest(&snapshot);
        let expected_identity = std::ptr::from_ref(&snapshot);
        let attempts = Cell::new(0_u32);
        let rebuilds = Cell::new(0_u32);
        let recoveries = Cell::new(0_u32);
        let retry_failures = Cell::new(0_u32);
        let observed = RefCell::new(Vec::new());

        let status = retry_device_lost_once(
            &snapshot,
            |borrowed| {
                observed.borrow_mut().push((
                    std::ptr::from_ref(borrowed),
                    terminal_snapshot_content_digest(borrowed),
                ));
                let attempt = attempts.get();
                attempts.set(attempt + 1);
                if attempt == 0 {
                    Err(TestPresentError::DeviceLost)
                } else {
                    Ok(GpuFrameStatus::Presented)
                }
            },
            || {
                rebuilds.set(rebuilds.get() + 1);
                Ok(())
            },
            |error| *error == TestPresentError::DeviceLost,
            |status| {
                if *status == GpuFrameStatus::Presented {
                    recoveries.set(recoveries.get() + 1);
                }
            },
            || retry_failures.set(retry_failures.get() + 1),
        )
        .expect("one device-loss retry");

        assert_eq!(status, GpuFrameStatus::Presented);
        assert_eq!(attempts.get(), 2);
        assert_eq!(rebuilds.get(), 1);
        assert_eq!(recoveries.get(), 1);
        assert_eq!(retry_failures.get(), 0);
        assert_eq!(
            observed.into_inner(),
            vec![
                (expected_identity, expected_digest),
                (expected_identity, expected_digest),
            ]
        );
    }

    #[test]
    fn device_loss_recovery_never_attempts_a_third_present_or_swallows_other_faults() {
        let snapshot =
            TerminalRenderSnapshot::from_terminal(&Terminal::new(TerminalSize::new(1, 1)));
        let attempts = Cell::new(0_u32);
        let rebuilds = Cell::new(0_u32);
        let recoveries = Cell::new(0_u32);
        let recovery_failures = Cell::new(0_u32);
        let repeated = retry_device_lost_once(
            &snapshot,
            |_| {
                attempts.set(attempts.get() + 1);
                Err::<GpuFrameStatus, _>(TestPresentError::DeviceLost)
            },
            || {
                rebuilds.set(rebuilds.get() + 1);
                Ok(())
            },
            |error| *error == TestPresentError::DeviceLost,
            |_| recoveries.set(recoveries.get() + 1),
            || recovery_failures.set(recovery_failures.get() + 1),
        );
        assert_eq!(repeated, Err(TestPresentError::DeviceLost));
        assert_eq!(attempts.get(), 2);
        assert_eq!(rebuilds.get(), 1);
        assert_eq!(recoveries.get(), 0);
        assert_eq!(recovery_failures.get(), 1);

        attempts.set(0);
        rebuilds.set(0);
        recovery_failures.set(0);
        let validation = retry_device_lost_once(
            &snapshot,
            |_| {
                attempts.set(attempts.get() + 1);
                Err::<GpuFrameStatus, _>(TestPresentError::Validation)
            },
            || {
                rebuilds.set(rebuilds.get() + 1);
                Ok(())
            },
            |error| *error == TestPresentError::DeviceLost,
            |_| recoveries.set(recoveries.get() + 1),
            || recovery_failures.set(recovery_failures.get() + 1),
        );
        assert_eq!(validation, Err(TestPresentError::Validation));
        assert_eq!(attempts.get(), 1);
        assert_eq!(rebuilds.get(), 0);
        assert_eq!(recoveries.get(), 0);
        assert_eq!(recovery_failures.get(), 0);

        attempts.set(0);
        rebuilds.set(0);
        recovery_failures.set(0);
        let layer_rebuild_failure = retry_device_lost_once(
            &snapshot,
            |_| {
                attempts.set(attempts.get() + 1);
                Err::<GpuFrameStatus, _>(TestPresentError::DeviceLost)
            },
            || {
                rebuilds.set(rebuilds.get() + 1);
                recovery_failures.set(recovery_failures.get() + 1);
                Err(TestPresentError::Validation)
            },
            |error| *error == TestPresentError::DeviceLost,
            |_| recoveries.set(recoveries.get() + 1),
            || recovery_failures.set(recovery_failures.get() + 1),
        );
        assert_eq!(layer_rebuild_failure, Err(TestPresentError::Validation));
        assert_eq!(attempts.get(), 1);
        assert_eq!(rebuilds.get(), 1);
        assert_eq!(recoveries.get(), 0);
        assert_eq!(recovery_failures.get(), 1);
    }
}
