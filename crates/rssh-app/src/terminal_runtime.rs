use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

use rssh_core::TerminalSize;
use rssh_runtime::RuntimeBuffers;

pub(crate) use rssh_runtime::terminal::{TerminalNotification, TerminalProgress};

const PROCESS_CWD_PROBE_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) struct TerminalRuntime {
    pub(crate) inner: rssh_runtime::TerminalRuntime,
    pub(crate) storage: TerminalRuntimeStorage,
}

pub(crate) struct TerminalRuntimeStorage {
    pub(crate) buffers: RuntimeBuffers,
    process_cwd_probe: ProcessCwdProbeState,
}

#[derive(Default)]
struct ProcessCwdProbeState {
    process_id: Option<u32>,
    next_probe_at: Option<Instant>,
}

impl TerminalRuntime {
    pub(crate) fn new(size: TerminalSize) -> Self {
        Self {
            inner: rssh_runtime::TerminalRuntime::new(size),
            storage: TerminalRuntimeStorage::new(),
        }
    }

    pub(crate) fn new_with_query_scan_work(size: TerminalSize) -> Self {
        Self {
            inner: rssh_runtime::TerminalRuntime::new_with_query_scan_work(size),
            storage: TerminalRuntimeStorage::new(),
        }
    }

    pub(crate) fn should_probe_process_cwd(&mut self, process_id: u32, now: Instant) -> bool {
        let probe = &mut self.storage.process_cwd_probe;
        if probe.process_id != Some(process_id) {
            probe.process_id = Some(process_id);
            probe.next_probe_at = now.checked_add(PROCESS_CWD_PROBE_INTERVAL);
            return true;
        }

        if probe.next_probe_at.is_none_or(|deadline| now >= deadline) {
            probe.next_probe_at = now.checked_add(PROCESS_CWD_PROBE_INTERVAL);
            return true;
        }

        false
    }

    pub(crate) fn reset_process_cwd_probe(&mut self) {
        self.storage.process_cwd_probe = ProcessCwdProbeState::default();
    }
}

impl TerminalRuntimeStorage {
    fn new() -> Self {
        Self {
            buffers: RuntimeBuffers::with_capacity(8192),
            process_cwd_probe: ProcessCwdProbeState::default(),
        }
    }
}

impl Deref for TerminalRuntime {
    type Target = rssh_runtime::TerminalRuntime;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for TerminalRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_cwd_probe_is_throttled_and_invalidated_by_process_id() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let now = Instant::now();

        assert!(runtime.should_probe_process_cwd(10, now));
        assert!(!runtime.should_probe_process_cwd(10, now));
        assert!(runtime.should_probe_process_cwd(11, now));
        assert!(runtime.should_probe_process_cwd(11, now + PROCESS_CWD_PROBE_INTERVAL));

        runtime.reset_process_cwd_probe();
        assert!(runtime.should_probe_process_cwd(11, now));
    }
}
