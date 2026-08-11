use std::{cell::RefCell, error::Error, io, sync::Arc};

use rssh_renderer::gpu::{
    GpuContext, GpuContextError, GpuContextErrorKind, GpuContextOptions, GpuFrameStatus,
    GpuLayerRenderer, GpuPresentationMetrics, GpuTextConfig, GpuTextPrepareReport, RenderGraph,
    should_abandon_recovered_window_surface,
};
use rssh_renderer::{DamageRegion, RenderGeometry, TerminalRenderSnapshot, TextPaintConfig};
use winit::{dpi::PhysicalSize, event_loop::ActiveEventLoop, window::Window};

/// App-owned direct terminal renderer for the native wgpu surface.
pub(crate) struct WindowGpu {
    context: Option<GpuContext>,
    renderer: Option<GpuLayerRenderer>,
    retired_renderers: Vec<GpuLayerRenderer>,
    recovery: DeviceRecoveryCoordinator,
    report: Option<GpuTextPrepareReport>,
    rendered_frames: u64,
    replaced_device: bool,
    final_metrics: Option<GpuPresentationMetrics>,
    #[cfg(test)]
    abandonment_workaround_adapter_match_override: Option<bool>,
    #[cfg(test)]
    current_adapter_metrics_override: Option<GpuPresentationMetrics>,
    #[cfg(test)]
    abandonment_workaround_os_override: Option<&'static str>,
    #[cfg(debug_assertions)]
    test_device_loss_injected: bool,
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

impl WindowGpu {
    pub(crate) async fn new(
        event_loop: &ActiveEventLoop,
        window: Arc<Window>,
        surface_size: PhysicalSize<u32>,
        high_performance: bool,
        force_fallback_adapter: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let options = GpuContextOptions::default()
            .with_high_performance(high_performance)
            .with_force_fallback_adapter(force_fallback_adapter);
        let display = event_loop.owned_display_handle();
        let context = GpuContext::new_windowed(
            display.clone(),
            Arc::clone(&window),
            surface_size.width,
            surface_size.height,
            options,
        )
        .await?;
        let renderer = bundled_emergency_text_backend(&context)?;
        Ok(Self {
            context: Some(context),
            renderer: Some(renderer),
            retired_renderers: Vec::new(),
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device: false,
            final_metrics: None,
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
            || state.borrow_mut().rebuild_device_and_layers(),
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
        let report = self
            .renderer
            .as_mut()
            .expect("window GPU renderer is available before shutdown")
            .prepare_text(
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
        let renderer = match bundled_emergency_text_backend(self.context()) {
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
        context.record_abandoned_lost_surface();
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

    #[cfg(test)]
    pub(crate) fn for_manager_close_test(
        abandonment_workaround_adapter_match: bool,
        replaced_device: bool,
    ) -> Self {
        let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
            .expect("manager close test context");
        Self {
            context: Some(context),
            renderer: None,
            retired_renderers: Vec::new(),
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device,
            final_metrics: None,
            abandonment_workaround_adapter_match_override: Some(
                abandonment_workaround_adapter_match,
            ),
            current_adapter_metrics_override: None,
            abandonment_workaround_os_override: None,
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

    pub(crate) fn direct_text_metrics(&self) -> Option<(&GpuTextPrepareReport, u64)> {
        self.report
            .as_ref()
            .map(|report| (report, self.rendered_frames))
    }

    fn context(&self) -> &GpuContext {
        self.context
            .as_ref()
            .expect("window GPU context is available before shutdown")
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
        // No resource is forgotten here. Ordinary window closure always
        // releases active and retired wgpu objects through their normal Drop.
    }
}

fn bundled_emergency_text_backend(
    context: &GpuContext,
) -> Result<GpuLayerRenderer, Box<dyn Error>> {
    use rssh_fonts::RasterCacheConfig;

    let catalog = bundled_emergency_font_catalog()?;
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

fn bundled_emergency_font_catalog() -> Result<rssh_fonts::FontCatalog, Box<dyn Error>> {
    use rssh_fonts::{FontCatalog, FontSource};

    let mut catalog = FontCatalog::from_sources(
        "en-US",
        [
            FontSource::new(
                "NotoSans-Latin.fixture.ttf",
                include_bytes!("../../../tests/fixtures/fonts/NotoSans-Latin.fixture.ttf").to_vec(),
            ),
            FontSource::new(
                "NotoSansSC-CJK.fixture.ttf",
                include_bytes!("../../../tests/fixtures/fonts/NotoSansSC-CJK.fixture.ttf").to_vec(),
            ),
            FontSource::new(
                "NotoSansArabic.fixture.ttf",
                include_bytes!("../../../tests/fixtures/fonts/NotoSansArabic.fixture.ttf").to_vec(),
            ),
            FontSource::new(
                "NotoSansDevanagari.fixture.ttf",
                include_bytes!("../../../tests/fixtures/fonts/NotoSansDevanagari.fixture.ttf")
                    .to_vec(),
            ),
            FontSource::new(
                "NotoSansHebrew.fixture.ttf",
                include_bytes!("../../../tests/fixtures/fonts/NotoSansHebrew.fixture.ttf").to_vec(),
            ),
            FontSource::new(
                "NotoSansSymbols2.fixture.ttf",
                include_bytes!("../../../tests/fixtures/fonts/NotoSansSymbols2.fixture.ttf")
                    .to_vec(),
            ),
            FontSource::new(
                "NotoColorEmoji.fixture.ttf",
                include_bytes!("../../../tests/fixtures/fonts/NotoColorEmoji.fixture.ttf").to_vec(),
            ),
        ],
    )?;
    load_platform_font_sources(&mut catalog);
    Ok(catalog)
}

fn load_platform_font_sources(catalog: &mut rssh_fonts::FontCatalog) {
    use rssh_fonts::FontSource;

    #[cfg(target_os = "windows")]
    const CANDIDATES: &[(&str, &str)] = &[
        (
            "CascadiaMono.system.ttf",
            r"C:\Windows\Fonts\CascadiaMono.ttf",
        ),
        (
            "CascadiaCode.system.ttf",
            r"C:\Windows\Fonts\CascadiaCode.ttf",
        ),
        (
            "SourceCodePro.system.ttf",
            r"C:\Windows\Fonts\SourceCodePro-Regular.ttf",
        ),
        ("Consolas.system.ttf", r"C:\Windows\Fonts\consola.ttf"),
        (
            "NotoSansSC.system.ttf",
            r"C:\Windows\Fonts\NotoSansSC-VF.ttf",
        ),
        (
            "NotoSansJP.system.ttf",
            r"C:\Windows\Fonts\NotoSansJP-VF.ttf",
        ),
        ("MicrosoftYaHei.system.ttc", r"C:\Windows\Fonts\msyh.ttc"),
        ("Meiryo.system.ttc", r"C:\Windows\Fonts\meiryo.ttc"),
        ("MalgunGothic.system.ttf", r"C:\Windows\Fonts\malgun.ttf"),
        ("SegoeUI.system.ttf", r"C:\Windows\Fonts\segoeui.ttf"),
        ("NirmalaUI.system.ttc", r"C:\Windows\Fonts\Nirmala.ttc"),
        ("SegoeUIEmoji.system.ttf", r"C:\Windows\Fonts\seguiemj.ttf"),
    ];
    #[cfg(target_os = "linux")]
    const CANDIDATES: &[(&str, &str)] = &[
        (
            "NotoSansMono.system.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
        ),
        (
            "DejaVuSansMono.system.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        ),
        (
            "NotoSansCJK.system.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ),
        (
            "NotoSansArabic.system.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
        ),
        (
            "NotoSansDevanagari.system.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
        ),
        (
            "NotoColorEmoji.system.ttf",
            "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
        ),
    ];
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[(&str, &str)] = &[
        ("Menlo.system.ttc", "/System/Library/Fonts/Menlo.ttc"),
        ("Monaco.system.dfont", "/System/Library/Fonts/Monaco.dfont"),
        (
            "HiraginoSansGB.system.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
        ),
        (
            "HiraginoSans.system.ttc",
            "/System/Library/Fonts/Hiragino Sans.ttc",
        ),
        (
            "AppleSDGothicNeo.system.ttc",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        ),
        (
            "ArialUnicode.system.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ),
        (
            "AppleColorEmoji.system.ttc",
            "/System/Library/Fonts/Apple Color Emoji.ttc",
        ),
    ];
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    const CANDIDATES: &[(&str, &str)] = &[];

    for &(label, path) in CANDIDATES {
        let Ok(source) = FontSource::from_file(path) else {
            continue;
        };
        let _ = catalog.load_source(FontSource::new(label, source.bytes().to_vec()));
    }
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
    use std::cell::{Cell, RefCell};

    use rssh_core::TerminalSize;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    use rssh_fonts::TerminalShaper;
    use rssh_renderer::terminal_snapshot_content_digest;
    use rssh_terminal::Terminal;

    #[test]
    fn native_presentation_mode_is_the_only_gpu_damage_selector() {
        let damage = [DamageRegion::new(1, 2, 3, 4)];

        assert!(presentation_damage(rssh_native::RenderMode::Full, &damage).is_empty());
        assert_eq!(
            presentation_damage(rssh_native::RenderMode::Damage, &damage),
            damage
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
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device: true,
            final_metrics: None,
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
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device: true,
            final_metrics: None,
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
            recovery: DeviceRecoveryCoordinator::default(),
            report: None,
            rendered_frames: 0,
            replaced_device: true,
            final_metrics: None,
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
