use super::{
    ActiveV2Close, Error, NativeWindowApp, PhysicalSize, PtyExitStatus, PtySize, RuntimeHostEvent,
    TerminalNotification, TerminalResizeOutcome, TerminalSize,
    terminal_progress_from_runtime, terminal_size_from_window_pixels_with_padding,
};

impl NativeWindowApp {
    pub(super) fn poll_active_v2_runtime(&mut self) -> Result<Option<bool>, Box<dyn Error>> {
        let Some(runtime) = self.runtime.worker_mut() else {
            return Ok(None);
        };
        let token = runtime.token();
        let events = runtime.poll()?;
        let mut closed = ActiveV2Close::Open;
        for event in events {
            self.apply_active_v2_event(token, event, &mut closed)?;
        }

        let ActiveV2Close::Closed { pane, exit } = closed else {
            return Ok(None);
        };
        let has_remaining_panes = self
            .runtime
            .worker()
            .is_some_and(|runtime| !runtime.pane_tokens().is_empty());
        if !has_remaining_panes
            && let Some(mut runtime) = self.runtime.take_worker()
        {
            runtime.shutdown();
        }
        self.session_process_id = None;
        self.session_tty_name = None;
        self.active_runtime_generation = 0;
        let status = exit.and_then(|exit| exit.status).map(PtyExitStatus::from_exit_code);
        let close_window = self.apply_pane_exit_behavior_after_exit(pane.pane(), status);
        if has_remaining_panes {
            let active = self.app_shell.active_pane_id();
            if let Some(runtime) = self.runtime.worker_mut()
                && runtime.pane_tokens().iter().any(|token| token.pane() == active)
            {
                runtime.activate_pane(active)?;
            }
        }
        Ok(Some(self.defer_automatic_close_for_frame_limit(close_window)))
    }

    fn apply_active_v2_event(
        &mut self,
        token: rssh_runtime::PaneToken,
        event: RuntimeHostEvent,
        closed: &mut ActiveV2Close,
    ) -> Result<(), Box<dyn Error>> {
        match event {
            RuntimeHostEvent::Frame {
                pane,
                terminal,
                damage,
                metadata,
                metrics,
                full_repaint,
                ..
            } if pane == token => {
                self.apply_active_v2_frame(terminal, &damage, metadata, metrics, full_repaint);
            }
            RuntimeHostEvent::Frame {
                pane,
                terminal,
                damage,
                metadata,
                metrics,
                ..
            } => {
                self.apply_inactive_v2_frame(pane, terminal, &damage, &metadata, metrics);
            }
            RuntimeHostEvent::HostStream { pane, bytes } if pane == token => {
                #[cfg(feature = "functional-test-observer")]
                crate::functional_observer::record_effect("host_stream");
                self.metrics.record_pty_chunk(&bytes);
                self.metrics.record_active_pty_content(&bytes);
            }
            RuntimeHostEvent::VisibleOutput { pane, bytes } if pane == token => {
                #[cfg(feature = "functional-test-observer")]
                crate::functional_observer::record_effect("visible_output");
                self.write_session_log(&bytes)?;
            }
            RuntimeHostEvent::ModeChange { pane, change } if pane == token => {
                #[cfg(feature = "functional-test-observer")]
                crate::functional_observer::record_effect("mode_change");
                self.runtime.inner.install_presentation_mode_change(change);
            }
            RuntimeHostEvent::InputWriteCompleted {
                byte_count,
                elapsed,
            } => self.handle_pane_input_write_completed(byte_count, elapsed),
            RuntimeHostEvent::FirstPtyByte { observed_at } => {
                self.metrics.record_first_pty_byte_at(observed_at);
            }
            RuntimeHostEvent::Bell { pane, count } if pane == token => {
                #[cfg(feature = "functional-test-observer")]
                crate::functional_observer::record_effect("bell");
                self.record_pane_bells(pane.pane(), count);
                self.metrics.record_bells(count);
                self.dispatch_bells(pane.pane(), count);
            }
            RuntimeHostEvent::ClipboardWrite {
                pane,
                selection,
                contents,
            } if pane == token => {
                self.apply_v2_clipboard_write(selection.as_deref(), &contents);
            }
            RuntimeHostEvent::ClipboardRead { pane, selection } if pane == token => {
                self.apply_v2_clipboard_read(&selection)?;
            }
            RuntimeHostEvent::Notification {
                pane,
                title,
                body,
            } if pane == token => {
                #[cfg(feature = "functional-test-observer")]
                crate::functional_observer::record_effect("notification");
                self.dispatch_notification(pane.pane(), &TerminalNotification { title, body });
            }
            RuntimeHostEvent::Diagnostic { pane, message } => {
                #[cfg(feature = "functional-test-observer")]
                crate::functional_observer::record_effect("diagnostic");
                self.record_unknown_escape_sequence_warning(
                    pane.map_or(self.app_shell.active_pane_id(), rssh_runtime::PaneToken::pane),
                    &message,
                );
            }
            RuntimeHostEvent::RequestRedraw => {
                self.request_v2_redraw();
            }
            RuntimeHostEvent::Closed { pane, exit } if pane == token => {
                *closed = ActiveV2Close::Closed { pane, exit };
            }
            RuntimeHostEvent::Closed { pane, exit } => {
                let status = exit
                    .and_then(|exit| exit.status)
                    .map(PtyExitStatus::from_exit_code);
                let _ = self.apply_pane_exit_behavior_after_exit(pane.pane(), status);
            }
            RuntimeHostEvent::HostStream { .. }
            | RuntimeHostEvent::VisibleOutput { .. }
            | RuntimeHostEvent::ModeChange { .. }
            | RuntimeHostEvent::Bell { .. }
            | RuntimeHostEvent::ClipboardWrite { .. }
            | RuntimeHostEvent::ClipboardRead { .. }
            | RuntimeHostEvent::Notification { .. } => {}
        }
        Ok(())
    }

    fn apply_v2_clipboard_write(&mut self, selection: Option<&str>, contents: &str) {
        #[cfg(feature = "functional-test-observer")]
        crate::functional_observer::record_effect("clipboard_write");
        if self.allows_v2_clipboard_write(selection) {
            self.write_clipboard_text(contents);
        }
    }

    fn request_v2_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn apply_v2_clipboard_read(&mut self, selection: &str) -> Result<(), Box<dyn Error>> {
        #[cfg(feature = "functional-test-observer")]
        crate::functional_observer::record_effect("clipboard_read");
        if self.osc52_policy.allows_query() {
            self.answer_clipboard_query(selection)?;
        }
        Ok(())
    }

    fn apply_active_v2_frame(
        &mut self,
        terminal: std::sync::Arc<rssh_terminal::Terminal>,
        damage: &[rssh_core::DamageRegion],
        metadata: rssh_runtime::PaneMetadataDelta,
        metrics: rssh_runtime::RuntimeBatchMetrics,
        full_repaint: bool,
    ) {
        let previous_dimensions = self.runtime.terminal().stable_dimensions();
        self.runtime.install_presentation_snapshot(terminal);
        let dimensions = self.runtime.terminal().stable_dimensions();
        if dimensions.domain != previous_dimensions.domain
            || dimensions.viewport_rows != previous_dimensions.viewport_rows
        {
            self.retire_active_terminal_identity_state();
        }
        self.reconcile_active_terminal_mutation();
        self.apply_v2_metadata(metadata);
        self.metrics.record_damage(damage);
        if full_repaint {
            self.refresh_snapshot();
        } else {
            self.refresh_snapshot_after_terminal_damage(damage);
        }
        self.metrics.record_pty_chunk_process(
            metrics.parse_duration.saturating_add(metrics.snapshot_duration),
        );
        let snapshot_is_empty = self.snapshot.cells().is_empty();
        self.metrics.record_first_rendered_cell(snapshot_is_empty);
    }

    fn apply_inactive_v2_frame(
        &mut self,
        pane: rssh_runtime::PaneToken,
        terminal: std::sync::Arc<rssh_terminal::Terminal>,
        damage: &[rssh_core::DamageRegion],
        metadata: &rssh_runtime::PaneMetadataDelta,
        metrics: rssh_runtime::RuntimeBatchMetrics,
    ) {
        let pane_id = pane.pane();
        let Some(runtime) = self.pane_runtimes.get_mut(&pane_id) else {
            return;
        };
        let previous_dimensions = runtime.runtime.terminal().stable_dimensions();
        runtime.runtime.install_presentation_snapshot(terminal);
        let dimensions = runtime.runtime.terminal().stable_dimensions();
        if dimensions.domain != previous_dimensions.domain
            || dimensions.viewport_rows != previous_dimensions.viewport_rows
        {
            runtime.ui.retire_terminal_identity();
        }
        runtime.reconcile_terminal_mutation();
        let cwd = metadata
            .working_directory
            .is_some()
            .then(|| runtime.runtime.terminal().current_working_dir().map(str::to_owned))
            .flatten();
        let user_vars = (!metadata.user_vars.is_empty()).then(|| {
            runtime
                .runtime
                .terminal()
                .user_vars()
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect::<Vec<_>>()
        });
        let badge_format = metadata
            .badge_format
            .is_some()
            .then(|| runtime.runtime.terminal().badge_format().map(str::to_owned));
        let progress = metadata
            .progress
            .is_some()
            .then(|| runtime.runtime.progress());
        self.sync_pane_has_unseen_output_from_value(pane_id, true);
        if metadata.working_directory.is_some() {
            self.sync_pane_current_working_dir_from_value(pane_id, cwd);
        }
        if let Some(user_vars) = user_vars {
            self.sync_pane_user_vars_from_pairs(pane_id, user_vars);
        }
        if let Some(badge_format) = badge_format {
            self.sync_pane_badge_format_from_value(pane_id, badge_format);
        }
        if let Some(progress) = progress {
            self.sync_pane_progress_from_value(pane_id, progress);
        }
        self.metrics.record_damage(damage);
        self.metrics.record_pty_chunk_process(
            metrics.parse_duration.saturating_add(metrics.snapshot_duration),
        );
    }

    fn allows_v2_clipboard_write(&self, _selection: Option<&str>) -> bool {
        self.osc52_policy.allows_write()
    }

    fn apply_v2_metadata(&mut self, metadata: rssh_runtime::PaneMetadataDelta) {
        if metadata.working_directory.is_some() {
            self.sync_active_pane_current_working_dir_from_runtime();
        }
        if !metadata.user_vars.is_empty() {
            self.sync_active_pane_user_vars_from_runtime();
        }
        if metadata.badge_format.is_some() {
            self.sync_active_pane_badge_format_from_runtime();
        }
        if let Some(progress) = metadata.progress {
            let progress = match progress {
                rssh_runtime::MetadataChange::Set(progress) => progress,
                rssh_runtime::MetadataChange::Clear => rssh_runtime::RuntimeProgress::None,
            };
            self.sync_pane_progress_from_value(
                self.app_shell.active_pane_id(),
                terminal_progress_from_runtime(progress),
            );
        }
        if metadata.title.is_some() {
            self.sync_window_title_from_runtime();
        }
    }

    pub(super) fn handle_window_resize(
        &mut self,
        size: PhysicalSize<u32>,
    ) -> Result<(), Box<dyn Error>> {
        self.resize_presentation_surface(size)?;
        if self.window.is_some() {
            self.refresh_window_frame_from_window();
        } else {
            self.window_frame.set_size(size);
        }

        let terminal_size = terminal_size_from_window_pixels_with_padding(
            size.width,
            size.height,
            self.cell_width(),
            self.cell_height(),
            self.window_padding,
            self.window_dpi,
        );
        let old_terminal_size = self.runtime.terminal().grid().size();
        let split_resize = self
            .app_shell
            .active_tab()
            .panes()
            .first()
            .map(rssh_core::app_shell::Pane::id)
            .map(|root_pane_id| {
                let old_root =
                    self.padded_terminal_render_rect_for_size(root_pane_id, old_terminal_size);
                let new_root =
                    self.padded_terminal_render_rect_for_size(root_pane_id, terminal_size);
                (
                    old_root.columns,
                    old_root.rows,
                    new_root.columns,
                    new_root.rows,
                )
            });
        self.frame_width = size.width;
        self.frame_height = size.height;

        let pty_size = PtySize::try_new(terminal_size.columns, terminal_size.rows)?;
        if let Some((old_columns, old_rows, new_columns, new_rows)) = split_resize {
            self.app_shell.preserve_split_layout_for_resize(
                old_columns,
                old_rows,
                new_columns,
                new_rows,
            );
        }
        let active_resize_outcome = self.resize_terminal_runtimes(terminal_size)?;
        self.refresh_snapshot_after_terminal_resize(
            active_resize_outcome == TerminalResizeOutcome::AlternateScreenResized,
        );

        for runtime in self.pane_runtimes.values_mut() {
            if let Some(session) = runtime.session.as_mut() {
                session.resize(pty_size)?;
            }
        }
        if let Some(session) = self.session.as_mut() {
            session.resize(pty_size)?;
        }
        let resize = self.native_window_resize_event(size.width, size.height, terminal_size);
        self.dispatch_resize(&resize);
        Ok(())
    }

    fn resize_terminal_runtimes(
        &mut self,
        terminal_size: TerminalSize,
    ) -> Result<TerminalResizeOutcome, Box<dyn Error>> {
        let active_height_changed =
            self.runtime.terminal().grid().size().rows != terminal_size.rows;
        let active_resize_outcome = if let Some(runtime) = self.runtime.worker_mut() {
            runtime.resize_all(terminal_size)?;
            TerminalResizeOutcome::Unchanged
        } else {
            self.runtime.resize(terminal_size)
        };
        if active_height_changed {
            self.retire_active_terminal_identity_state();
        }
        if active_resize_outcome == TerminalResizeOutcome::MainScreenReflowed {
            self.reconcile_active_ui_after_main_screen_reflow();
        } else {
            self.reconcile_active_terminal_resize(
                active_resize_outcome == TerminalResizeOutcome::AlternateScreenResized,
            );
        }
        self.resize_inactive_terminal_runtimes(terminal_size);
        Ok(active_resize_outcome)
    }

    fn resize_inactive_terminal_runtimes(&mut self, terminal_size: TerminalSize) {
        for runtime in self.pane_runtimes.values_mut() {
            let height_changed =
                runtime.runtime.terminal().grid().size().rows != terminal_size.rows;
            let resize_outcome = runtime.runtime.resize(terminal_size);
            if height_changed {
                runtime.ui.retire_terminal_identity();
            }
            if resize_outcome == TerminalResizeOutcome::MainScreenReflowed {
                runtime.reconcile_after_main_screen_reflow();
            } else {
                runtime.reconcile_terminal_resize(
                    resize_outcome == TerminalResizeOutcome::AlternateScreenResized,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::*;

    #[test]
    fn inactive_v2_frame_updates_its_owned_presentation_and_metadata() {
        let mut app = NativeWindowApp::new(None);
        let inactive = app.active_pane_id();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: inactive,
            direction: SplitDirection::Right,
            launch: Some(PaneLaunch::local("active")),
        })
        .unwrap();
        let active = app.active_pane_id();
        let mut allocator = rssh_runtime::PaneTokenAllocator::new();
        let inactive_token = allocator.issue(inactive).expect("inactive token");
        let active_token = allocator.issue(active).expect("active token");
        let size = app
            .pane_runtimes
            .get(&inactive)
            .unwrap()
            .runtime
            .terminal()
            .grid()
            .size();
        let mut terminal = Terminal::new(size);
        terminal.feed(b"\x1b]2;inactive-v2-title\x07inactive-v2-body");
        let mut closed = ActiveV2Close::Open;

        app.apply_active_v2_event(
            active_token,
            RuntimeHostEvent::Frame {
                pane: inactive_token,
                terminal: Arc::new(terminal),
                damage: vec![rssh_core::DamageRegion::new(0, 0, 1, 1)],
                metadata: rssh_runtime::PaneMetadataDelta {
                    title: Some(rssh_runtime::MetadataChange::Set(
                        "inactive-v2-title".to_owned(),
                    )),
                    ..rssh_runtime::PaneMetadataDelta::default()
                },
                metrics: rssh_runtime::RuntimeBatchMetrics::default(),
                full_repaint: false,
            },
            &mut closed,
        )
        .expect("apply inactive V2 frame");

        let inactive_runtime = app.pane_runtimes.get(&inactive).unwrap();
        assert_eq!(
            inactive_runtime.runtime.terminal().window_title(),
            Some("inactive-v2-title")
        );
        assert_eq!(app.active_pane_id(), active);
        assert!(matches!(closed, ActiveV2Close::Open));
    }

    #[test]
    fn inactive_v2_close_does_not_close_the_active_runtime_owner() {
        let mut app = NativeWindowApp::new(None);
        let inactive = app.active_pane_id();
        app.dispatch_app_action(AppAction::SplitPane {
            pane: inactive,
            direction: SplitDirection::Right,
            launch: Some(PaneLaunch::local("active")),
        })
        .unwrap();
        let active = app.active_pane_id();
        let mut allocator = rssh_runtime::PaneTokenAllocator::new();
        let inactive_token = allocator.issue(inactive).expect("inactive token");
        let active_token = allocator.issue(active).expect("active token");
        let mut closed = ActiveV2Close::Open;

        app.apply_active_v2_event(
            active_token,
            RuntimeHostEvent::Closed {
                pane: inactive_token,
                exit: Some(rssh_runtime::SessionExit {
                    status: Some(0),
                    signal: None,
                }),
            },
            &mut closed,
        )
        .expect("apply inactive close");

        assert_eq!(app.active_pane_id(), active);
        assert_eq!(app.app_shell.pane_ids(), vec![active]);
        assert!(matches!(closed, ActiveV2Close::Open));
    }
}
