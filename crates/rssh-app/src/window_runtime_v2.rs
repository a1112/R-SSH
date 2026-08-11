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

        let ActiveV2Close::Closed(exit) = closed else {
            return Ok(None);
        };
        if let Some(mut runtime) = self.runtime.take_worker() {
            runtime.shutdown();
        }
        self.session_process_id = None;
        self.session_tty_name = None;
        self.active_runtime_generation = 0;
        let status = exit.and_then(|exit| exit.status).map(PtyExitStatus::from_exit_code);
        let close_window = self.apply_pane_exit_behavior_after_exit(
            self.app_shell.active_pane_id(),
            status,
        );
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
                self.metrics.record_damage(&damage);
                if full_repaint {
                    self.refresh_snapshot();
                } else {
                    self.refresh_snapshot_after_terminal_damage(&damage);
                }
                self.metrics.record_pty_chunk_process(
                    metrics.parse_duration.saturating_add(metrics.snapshot_duration),
                );
                self.metrics
                    .record_first_rendered_cell(self.snapshot.cells().is_empty());
            }
            RuntimeHostEvent::HostStream { pane, bytes } if pane == token => {
                self.metrics.record_pty_chunk(&bytes);
                self.metrics.record_active_pty_content(&bytes);
            }
            RuntimeHostEvent::VisibleOutput { pane, bytes } if pane == token => {
                self.write_session_log(&bytes)?;
            }
            RuntimeHostEvent::ModeChange { pane, change } if pane == token => {
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
                self.record_pane_bells(pane.pane(), count);
                self.metrics.record_bells(count);
                self.dispatch_bells(pane.pane(), count);
            }
            RuntimeHostEvent::ClipboardWrite {
                pane,
                selection,
                contents,
            } if pane == token => {
                if self.allows_v2_clipboard_write(selection.as_deref()) {
                    self.write_clipboard_text(&contents);
                }
            }
            RuntimeHostEvent::ClipboardRead { pane, selection } if pane == token => {
                if self.osc52_policy.allows_query() {
                    self.answer_clipboard_query(&selection)?;
                }
            }
            RuntimeHostEvent::Notification {
                pane,
                title,
                body,
            } if pane == token => {
                self.dispatch_notification(pane.pane(), &TerminalNotification { title, body });
            }
            RuntimeHostEvent::Diagnostic { pane, message } => {
                self.record_unknown_escape_sequence_warning(
                    pane.map_or(self.app_shell.active_pane_id(), rssh_runtime::PaneToken::pane),
                    &message,
                );
            }
            RuntimeHostEvent::RequestRedraw => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            RuntimeHostEvent::Closed { exit } => *closed = ActiveV2Close::Closed(exit),
            RuntimeHostEvent::Frame { .. }
            | RuntimeHostEvent::HostStream { .. }
            | RuntimeHostEvent::VisibleOutput { .. }
            | RuntimeHostEvent::ModeChange { .. }
            | RuntimeHostEvent::Bell { .. }
            | RuntimeHostEvent::ClipboardWrite { .. }
            | RuntimeHostEvent::ClipboardRead { .. }
            | RuntimeHostEvent::Notification { .. } => {}
        }
        Ok(())
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
            runtime.resize(terminal_size)?;
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
            if let Some(v2_runtime) = runtime.v2_runtime.as_mut() {
                let _ = v2_runtime.resize(terminal_size);
                continue;
            }
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
