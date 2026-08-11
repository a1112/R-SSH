use std::collections::HashMap;
use std::fmt;
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rssh_core::PaneId;

use crate::{
    Clock, MailboxLimits, MailboxReceiver, MailboxSender, PaneNotice, PaneToken,
    PaneTokenAllocator, PaneWorkerConfig, RecvError, SequenceExhausted, SessionTransport,
    SystemClock, TryRecvError, bounded_mailbox,
    pane::{ErasedInterrupt, PaneHandle, spawn_pane},
    shutdown::WorkerReaper,
};

const NOTICE_ITEMS: usize = 1024;
const NOTICE_BYTES: usize = 4 * 1024 * 1024;
const NOTICE_LIMITS: MailboxLimits = match MailboxLimits::try_new(NOTICE_ITEMS, NOTICE_BYTES) {
    Ok(limits) => limits,
    Err(_) => panic!("runtime notice limits must be nonzero"),
};

/// Failure to install a new pane generation in a runtime hub.
#[derive(Debug)]
pub enum OpenPaneError {
    /// A live generation already owns the logical pane ID.
    AlreadyOpen(PaneId),
    /// The process-wide pane generation sequence was exhausted.
    GenerationExhausted(SequenceExhausted),
    /// A worker or reader thread could not be spawned.
    Spawn(io::Error),
}

impl fmt::Display for OpenPaneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyOpen(pane) => write!(formatter, "pane {} is already open", pane.get()),
            Self::GenerationExhausted(error) => error.fmt(formatter),
            Self::Spawn(error) => write!(formatter, "failed to spawn pane worker: {error}"),
        }
    }
}

impl std::error::Error for OpenPaneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AlreadyOpen(_) => None,
            Self::GenerationExhausted(error) => Some(error),
            Self::Spawn(error) => Some(error),
        }
    }
}

struct PaneSlot {
    token: PaneToken,
    handle: PaneHandle,
    interrupt: ErasedInterrupt,
    join: Option<JoinHandle<()>>,
    close_deadline: Option<Instant>,
}

/// Owner of live pane generations, notices, deadlines, and worker joins.
pub struct RuntimeHub<C = SystemClock> {
    clock: C,
    allocator: PaneTokenAllocator,
    panes: HashMap<PaneId, PaneSlot>,
    notice_sender: MailboxSender<PaneNotice>,
    notice_receiver: MailboxReceiver<PaneNotice>,
    reaper: WorkerReaper,
    live_threads: Arc<AtomicUsize>,
}

impl<C: fmt::Debug> fmt::Debug for RuntimeHub<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeHub")
            .field("clock", &self.clock)
            .field("live_panes", &self.panes.len())
            .field("live_threads", &self.live_threads.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl<C: Clock> RuntimeHub<C> {
    /// Creates an empty hub with a bounded lossless notice queue.
    #[must_use]
    pub fn new(clock: C) -> Self {
        let (notice_sender, notice_receiver) = bounded_mailbox(NOTICE_LIMITS);
        Self {
            clock,
            allocator: PaneTokenAllocator::new(),
            panes: HashMap::new(),
            notice_sender,
            notice_receiver,
            reaper: WorkerReaper::default(),
            live_threads: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Opens one logical pane with a fresh process-wide generation.
    ///
    /// # Errors
    ///
    /// Returns an error if the logical pane is already open, generation space
    /// is exhausted, or the worker threads cannot be created.
    pub fn open<T: SessionTransport>(
        &mut self,
        pane: PaneId,
        transport: T,
        config: PaneWorkerConfig,
    ) -> Result<PaneHandle, OpenPaneError> {
        if self.panes.contains_key(&pane) {
            return Err(OpenPaneError::AlreadyOpen(pane));
        }
        let token = self
            .allocator
            .issue(pane)
            .map_err(OpenPaneError::GenerationExhausted)?;
        let spawned = spawn_pane(
            token,
            transport,
            config,
            self.notice_sender.clone(),
            &self.live_threads,
        )
        .map_err(OpenPaneError::Spawn)?;
        let handle = spawned.handle.clone();
        self.panes.insert(
            pane,
            PaneSlot {
                token,
                handle: spawned.handle,
                interrupt: spawned.interrupt,
                join: Some(spawned.join),
                close_deadline: None,
            },
        );
        Ok(handle)
    }

    /// Replaces a logical pane with a fresh generation and rejects old handles.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::open`] for the replacement session.
    pub fn restart<T: SessionTransport>(
        &mut self,
        pane: PaneId,
        transport: T,
        config: PaneWorkerConfig,
    ) -> Result<PaneHandle, OpenPaneError> {
        if let Some(token) = self.panes.get(&pane).map(|slot| slot.token) {
            let _ = self.begin_close(token, Duration::ZERO);
            let _ = self.reap_expired();
        }
        self.open(pane, transport, config)
    }

    /// Begins orderly close and records the external-interrupt deadline.
    #[must_use]
    pub fn begin_close(&mut self, token: PaneToken, grace: Duration) -> bool {
        let Some(slot) = self.panes.get_mut(&token.pane()) else {
            return false;
        };
        if slot.token != token {
            return false;
        }
        slot.handle.request_close();
        let now = self.clock.now();
        slot.close_deadline = Some(now.checked_add(grace).unwrap_or(now));
        true
    }

    /// Interrupts expired sessions and transfers their joins to the reaper.
    #[must_use]
    pub fn reap_expired(&mut self) -> usize {
        let now = self.clock.now();
        let expired = self
            .panes
            .iter()
            .filter_map(|(pane, slot)| {
                slot.close_deadline
                    .filter(|deadline| *deadline <= now)
                    .map(|_| *pane)
            })
            .collect::<Vec<_>>();
        for pane in &expired {
            if let Some(mut slot) = self.panes.remove(pane) {
                slot.handle.stop_accepting();
                let _ = slot.interrupt.interrupt();
                if let Some(join) = slot.join.take() {
                    self.reaper.handoff(join);
                }
            }
        }
        self.reaper.reap_finished();
        expired.len()
    }

    /// Waits for the next notice from the current pane generation.
    ///
    /// # Errors
    ///
    /// Returns [`RecvError::Closed`] after hub shutdown and queue drain.
    pub fn recv_notice(&mut self) -> Result<PaneNotice, RecvError> {
        loop {
            let notice = self.notice_receiver.recv()?;
            if self.is_current(notice.pane()) {
                self.finish_closed_slot(&notice);
                return Ok(notice);
            }
        }
    }

    /// Attempts to receive one notice, discarding stale generations.
    ///
    /// # Errors
    ///
    /// Returns empty or closed status from the bounded notice mailbox.
    pub fn try_recv_notice(&mut self) -> Result<PaneNotice, TryRecvError> {
        loop {
            let notice = self.notice_receiver.try_recv()?;
            if self.is_current(notice.pane()) {
                self.finish_closed_slot(&notice);
                return Ok(notice);
            }
        }
    }

    /// Returns the number of worker and blocking-reader threads still alive.
    #[must_use]
    pub fn live_thread_count(&self) -> usize {
        self.live_threads.load(Ordering::Acquire)
    }

    /// Interrupts and joins every pane generation and deadline reaper.
    pub fn shutdown(&mut self) {
        let _ = self.notice_receiver.close();
        for slot in self.panes.values_mut() {
            slot.handle.stop_accepting();
            let _ = slot.interrupt.interrupt();
        }
        for (_, mut slot) in self.panes.drain() {
            if let Some(join) = slot.join.take() {
                let _ = join.join();
            }
        }
        self.reaper.join_all();
    }

    fn is_current(&self, token: PaneToken) -> bool {
        self.panes
            .get(&token.pane())
            .is_some_and(|slot| slot.token == token)
    }

    fn finish_closed_slot(&mut self, notice: &PaneNotice) {
        if !matches!(notice, PaneNotice::Closed { .. }) {
            return;
        }
        let pane = notice.pane().pane();
        if let Some(mut slot) = self.panes.remove(&pane) {
            if let Some(join) = slot.join.take() {
                self.reaper.handoff(join);
            }
            self.reaper.reap_finished();
        }
    }
}

impl<C> Drop for RuntimeHub<C> {
    fn drop(&mut self) {
        let _ = self.notice_receiver.close();
        for slot in self.panes.values_mut() {
            slot.handle.stop_accepting();
            let _ = slot.interrupt.interrupt();
        }
        for (_, mut slot) in self.panes.drain() {
            if let Some(join) = slot.join.take() {
                let _ = join.join();
            }
        }
        self.reaper.join_all();
    }
}
