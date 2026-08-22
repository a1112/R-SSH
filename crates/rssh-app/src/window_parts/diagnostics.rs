impl NativeWindowApp {
    fn set_diagnostic_gpu_backend(
        &mut self,
        backend: Option<rssh_diagnostics::DiagnosticGpuBackend>,
    ) {
        self.diagnostic_gpu_backend = backend;
    }

    fn is_benchmark_startup(&self) -> bool {
        matches!(self.startup_mode, NativeStartupMode::Benchmark)
    }

    fn diagnostic_gui(&self) -> Option<&NativeDiagnosticGuiState> {
        match &self.startup_mode {
            NativeStartupMode::Diagnostic(diagnostic) => Some(diagnostic),
            NativeStartupMode::Normal | NativeStartupMode::Benchmark => None,
        }
    }

    fn diagnostic_gui_mut(&mut self) -> Option<&mut NativeDiagnosticGuiState> {
        match &mut self.startup_mode {
            NativeStartupMode::Diagnostic(diagnostic) => Some(diagnostic),
            NativeStartupMode::Normal | NativeStartupMode::Benchmark => None,
        }
    }

    fn has_diagnostic_gui(&self) -> bool {
        self.diagnostic_gui().is_some()
    }

    fn suppresses_transport_start(&self) -> bool {
        self.diagnostic_gui()
            .is_some_and(|diagnostic| diagnostic.scenario == DiagnosticScenario::EmptyWindow)
    }

    fn set_diagnostic_gui(
        &mut self,
        markers: DiagnosticMarkerHandle,
        scenario: DiagnosticScenario,
        hold_duration: Duration,
        pending_secret: Option<String>,
    ) {
        self.startup_mode = NativeStartupMode::Diagnostic(NativeDiagnosticGuiState {
            markers,
            scenario,
            hold_duration,
            hold_deadline: None,
            absolute_deadline: Instant::now()
                + Duration::from_secs(10).saturating_add(hold_duration),
            pending_secret,
            secret_prompt_presented: false,
        });
    }

    fn diagnostic_scale_override_enabled(&self) -> bool {
        self.is_benchmark_startup() || self.has_diagnostic_gui()
    }

    fn diagnostic_hold_deadline(&self) -> Option<Instant> {
        self.diagnostic_gui().map(|diagnostic| {
            diagnostic
                .hold_deadline
                .unwrap_or(diagnostic.absolute_deadline)
                .min(diagnostic.absolute_deadline)
        })
    }

    fn emit_diagnostic_marker(
        &self,
        kind: DiagnosticMarkerKind,
        renderer: Option<DiagnosticRendererKind>,
        connection_state: Option<DiagnosticConnectionState>,
    ) {
        if let Some(diagnostic) = self.diagnostic_gui()
            && let Err(error) = diagnostic.markers.emit(kind, renderer, connection_state)
        {
            eprintln!("failed to emit diagnostic marker {kind:?}: {error}");
        }
    }

    fn mark_diagnostic_window_created(&self) {
        self.emit_diagnostic_marker(DiagnosticMarkerKind::WindowCreated, None, None);
    }

    fn mark_diagnostic_config_ready(&self) {
        self.emit_diagnostic_marker(DiagnosticMarkerKind::ConfigReady, None, None);
    }

    fn mark_diagnostic_transport_started(&self) {
        if self
            .diagnostic_gui()
            .is_some_and(|diagnostic| diagnostic.scenario == DiagnosticScenario::Ssh1)
        {
            self.emit_diagnostic_marker(
                DiagnosticMarkerKind::TransportStarted,
                None,
                Some(DiagnosticConnectionState::Pending),
            );
        }
    }

    fn mark_transport_start_requested(&mut self) {
        self.transport_start_requested = true;
        self.mark_diagnostic_transport_started();
    }

    fn advance_ssh1_diagnostic_after_present(
        &mut self,
        renderer: DiagnosticRendererKind,
        snapshot: &TerminalRenderSnapshot,
    ) {
        if !self
            .diagnostic_gui()
            .is_some_and(|diagnostic| diagnostic.scenario == DiagnosticScenario::Ssh1)
        {
            return;
        }
        let pane_id = self.app_shell.active_pane_id();
        let state = self.ssh_connection_state_for_pane(pane_id);
        if state == ConnectionState::AwaitingSecret {
            let secret = self.diagnostic_gui_mut().and_then(|diagnostic| {
                diagnostic.secret_prompt_presented = true;
                diagnostic.pending_secret.take()
            });
            if secret.is_some() {
                self.resolve_secret_prompt_for_pane(pane_id, secret);
            }
            return;
        }
        if state != ConnectionState::Connected {
            return;
        }

        let visible_cell_count = visible_snapshot_cell_count(snapshot);
        let mut extra = HashMap::new();
        extra.insert(
            "visible_connection_state".to_owned(),
            serde_json::json!("connected"),
        );
        extra.insert(
            "visible_cell_count".to_owned(),
            serde_json::json!(visible_cell_count),
        );
        let Some(diagnostic) = self.diagnostic_gui_mut() else {
            return;
        };
        extra.insert(
            "secret_prompt_presented".to_owned(),
            serde_json::json!(diagnostic.secret_prompt_presented),
        );
        if let Err(error) = diagnostic.markers.emit(
            DiagnosticMarkerKind::TransportReady,
            Some(renderer),
            Some(DiagnosticConnectionState::Connected),
        ) {
            eprintln!("failed to emit diagnostic transport readiness: {error}");
        }
        if diagnostic.hold_deadline.is_none() {
            if let Err(error) = diagnostic.markers.emit_with_extra(
                DiagnosticMarkerKind::ScenarioReady,
                Some(renderer),
                Some(DiagnosticConnectionState::Connected),
                extra,
            ) {
                eprintln!("failed to emit diagnostic scenario readiness: {error}");
            }
            diagnostic.hold_deadline = Some(Instant::now() + diagnostic.hold_duration);
        }
    }

    fn mark_diagnostic_first_present(
        &mut self,
        renderer: DiagnosticRendererKind,
        visible_cell_count: usize,
    ) {
        if let Some(diagnostic) = self.diagnostic_gui() {
            let mut extra = HashMap::new();
            extra.insert(
                "visible_cell_count".to_owned(),
                serde_json::json!(visible_cell_count),
            );
            if let Err(error) = diagnostic.markers.emit_with_extra(
                DiagnosticMarkerKind::FirstPresent,
                Some(renderer),
                Some(diagnostic_connection_state(
                    self.ssh_connection_state_for_pane(self.app_shell.active_pane_id()),
                )),
                extra,
            ) {
                eprintln!("failed to emit diagnostic first present: {error}");
            }
        }
        if renderer == DiagnosticRendererKind::Gpu {
            self.emit_diagnostic_marker(
                DiagnosticMarkerKind::GpuReady,
                Some(DiagnosticRendererKind::Gpu),
                None,
            );
        }
        if let Some(diagnostic) = self.diagnostic_gui_mut()
            && diagnostic.scenario == DiagnosticScenario::EmptyWindow
            && diagnostic.hold_deadline.is_none()
        {
            if let Err(error) = diagnostic.markers.emit(
                DiagnosticMarkerKind::ScenarioReady,
                Some(renderer),
                Some(DiagnosticConnectionState::NotStarted),
            ) {
                eprintln!("failed to emit diagnostic scenario readiness: {error}");
            }
            diagnostic.hold_deadline = Some(Instant::now() + diagnostic.hold_duration);
        }
    }

    fn mark_diagnostic_gpu_ready(&self) {
        self.emit_diagnostic_marker(
            DiagnosticMarkerKind::GpuReady,
            Some(DiagnosticRendererKind::Gpu),
            None,
        );
    }

    fn record_first_present(
        &mut self,
        renderer: RendererKind,
        snapshot: &TerminalRenderSnapshot,
    ) {
        let diagnostic_renderer = match renderer {
            RendererKind::Cpu => {
                self.metrics.record_first_present(RendererKind::Cpu);
                DiagnosticRendererKind::Cpu
            }
            RendererKind::Gpu => {
                self.metrics.record_first_present(RendererKind::Gpu);
                DiagnosticRendererKind::Gpu
            }
        };
        self.mark_diagnostic_first_present(
            diagnostic_renderer,
            visible_snapshot_cell_count(snapshot),
        );
        self.metrics
            .record_first_frame_private_bytes(current_process_private_bytes());
    }

    fn frame_limit_redraw_pending(&self) -> bool {
        if test_ssh_gui_frame_limit().is_some()
            && self.presentation_owner == PresentationOwner::GpuInitializing
            && self.gpu.is_none()
        {
            return false;
        }
        self.frame_limit.is_some_and(|limit| {
            let target = if self.metrics.pty_linkage_enabled
                && !self.metrics.terminal_linkage_nonce_found
            {
                limit.saturating_sub(1)
            } else {
                limit
            };
            self.rendered_frames < target
        })
    }

    fn frame_limit_refresh_pending(&self) -> bool {
        self.frame_limit_redraw_pending() || self.final_linkage_frame_is_reserved()
    }

    fn final_linkage_frame_is_reserved(&self) -> bool {
        self.metrics.pty_linkage_enabled
            && !self.metrics.terminal_linkage_nonce_found
            && self
                .frame_limit
                .is_some_and(|limit| self.rendered_frames.saturating_add(1) >= limit)
    }

    fn frame_limit_reached(&self) -> bool {
        self.frame_limit
            .is_some_and(|limit| self.rendered_frames >= limit)
    }

    fn frame_limit_probe_ready(&self) -> bool {
        self.frame_limit_reached()
            && (!self.metrics.pty_linkage_enabled
                || self.metrics.terminal_linkage_nonce_found)
    }

    fn frame_limit_probe_pending(&self) -> bool {
        self.frame_limit.is_some() && !self.frame_limit_probe_ready()
    }

    fn frame_limit_redraw_deadline(&self, now: Instant) -> Option<Instant> {
        self.frame_limit_refresh_pending().then(|| {
            self.last_redraw_request_at
                .map_or(now, |last| last + self.redraw_request_interval())
        })
    }
}
