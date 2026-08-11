use super::{Error, NativeWindowApp, PtyExitStatus, PtySession, report_pane_pty_cleanup};

impl NativeWindowApp {
    pub(super) fn finish_pane_runtime_after_exit(
        &mut self,
        pane_id: rssh_core::PaneId,
        runtime_generation: u64,
    ) -> Option<PtyExitStatus> {
        let observed_status = self
            .metrics
            .observed_pane_exit_statuses
            .remove(&(pane_id, runtime_generation));
        if pane_id == self.app_shell.active_pane_id() {
            return self
                .finish_active_runtime_after_exit()
                .or(observed_status);
        }

        let mut runtime = self.pane_runtimes.remove(&pane_id)?;
        if let Err(error) = self.finish_inactive_pane_output(pane_id, &mut runtime) {
            eprintln!("inactive pane terminal finish failed: {error}");
        }
        let cleanup = runtime.finish_after_exit();
        report_pane_pty_cleanup("inactive pane exit cleanup", &cleanup);
        self.pane_runtimes.insert(pane_id, runtime);
        cleanup.status.or(observed_status)
    }

    pub(super) fn poll_active_legacy_runtime_exit(
        &mut self,
    ) -> Result<Option<bool>, Box<dyn Error>> {
        if self.runtime.worker().is_some() || self.frame_limit.is_some() {
            return Ok(None);
        }
        let now = std::time::Instant::now();
        if self
            .metrics
            .next_legacy_exit_poll
            .is_some_and(|deadline| now < deadline)
        {
            return Ok(None);
        }
        self.metrics.next_legacy_exit_poll = Some(now + super::LEGACY_EXIT_POLL_INTERVAL);
        let exit_key = (
            self.app_shell.active_pane_id(),
            self.active_runtime_generation,
        );
        if self
            .metrics
            .observed_pane_exit_statuses
            .contains_key(&exit_key)
        {
            return Ok(None);
        }
        let Some(observed_status) = self
            .session
            .as_mut()
            .map(PtySession::try_wait)
            .transpose()?
            .flatten()
        else {
            return Ok(None);
        };
        self.metrics
            .observed_pane_exit_statuses
            .insert(exit_key, observed_status);
        if let Some(session) = self.session.as_mut() {
            session.close_master();
        }
        Ok(None)
    }
}
