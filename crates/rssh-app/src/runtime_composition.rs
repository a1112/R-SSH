use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    error::Error,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::Duration,
};

use rssh_core::{DamageRegion, PaneId, TerminalSize, WindowId};
use rssh_native::{
    ClipboardEffect, HostPorts, NotificationEffect, PaneLifecycleIntent, PersistenceEffect,
    PlatformEvent, PortError, PortErrorKind, RendererEffect, RuntimeDrain, RuntimePortEffect,
    SpawnEffect, TurnBudget, UriEffect, WindowIntent, WindowPortEffect, WindowState, WinitHost,
};
use rssh_pty::PtySession;
use rssh_runtime::{
    LocalPtyTransport, PaneHandle, PaneMetadataDelta, PaneNotice, PaneToken, PaneWorkerConfig,
    RuntimeBatch, RuntimeBatchMetrics, RuntimeEffect, RuntimeHub, RuntimeRevision, SessionExit,
    SessionTransport, SubmitResult, SystemClock,
};
use rssh_terminal::Terminal;

use crate::{runtime_selection::RuntimeSelection, terminal_runtime::TerminalRuntime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeComposition {
    #[cfg(any(debug_assertions, feature = "runtime-v2-evaluation"))]
    selection: RuntimeSelection,
    #[cfg(all(not(debug_assertions), not(feature = "runtime-v2-evaluation")))]
    legacy: (),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PaneCapturePolicy {
    pub(crate) host_stream: bool,
    pub(crate) visible_output: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PaneRuntimeRoute {
    pub(crate) window: WindowId,
    pub(crate) pane: PaneId,
}

impl RuntimeComposition {
    pub(crate) const fn new(selection: RuntimeSelection) -> Self {
        #[cfg(any(debug_assertions, feature = "runtime-v2-evaluation"))]
        {
            Self { selection }
        }
        #[cfg(all(not(debug_assertions), not(feature = "runtime-v2-evaluation")))]
        {
            let _ = selection;
            Self { legacy: () }
        }
    }

    #[inline]
    pub(crate) const fn selection(self) -> RuntimeSelection {
        #[cfg(any(debug_assertions, feature = "runtime-v2-evaluation"))]
        {
            self.selection
        }
        #[cfg(all(not(debug_assertions), not(feature = "runtime-v2-evaluation")))]
        {
            let () = self.legacy;
            RuntimeSelection::Legacy
        }
    }

    pub(crate) fn adopt_local_session(
        self,
        route: PaneRuntimeRoute,
        session: PtySession,
        size: TerminalSize,
        terminal: rssh_runtime::TerminalRuntime,
        capture: PaneCapturePolicy,
        notice_waker: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<SpawnedLocalPane, Box<dyn Error>> {
        debug_assert_eq!(self.selection(), RuntimeSelection::V2);
        SingleLocalPaneRuntime::adopt_local_session(
            route,
            session,
            size,
            terminal,
            capture,
            notice_waker,
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) enum RuntimeHostEvent {
    Frame {
        pane: PaneToken,
        terminal: Arc<Terminal>,
        damage: Vec<DamageRegion>,
        metadata: PaneMetadataDelta,
        metrics: RuntimeBatchMetrics,
        full_repaint: bool,
    },
    Bell {
        pane: PaneToken,
        count: u64,
    },
    ClipboardWrite {
        pane: PaneToken,
        selection: Option<String>,
        contents: String,
    },
    ClipboardRead {
        pane: PaneToken,
        selection: String,
    },
    Notification {
        pane: PaneToken,
        title: Option<String>,
        body: String,
    },
    Diagnostic {
        pane: Option<PaneToken>,
        message: String,
    },
    HostStream {
        pane: PaneToken,
        bytes: Vec<u8>,
    },
    VisibleOutput {
        pane: PaneToken,
        bytes: Vec<u8>,
    },
    ModeChange {
        pane: PaneToken,
        change: rssh_runtime::TerminalModeChange,
    },
    InputWriteCompleted {
        byte_count: usize,
        elapsed: Duration,
    },
    FirstPtyByte {
        observed_at: std::time::Instant,
    },
    RequestRedraw,
    Closed {
        exit: Option<SessionExit>,
    },
}

pub(crate) struct SingleLocalPaneRuntime {
    host: WinitHost<RuntimePorts>,
    token: PaneToken,
    handle: PaneHandle,
    closed: bool,
    pending_commands: PendingPaneCommandQueue,
}

const MAX_PENDING_INPUT_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone)]
enum PendingPaneCommand {
    Input(Vec<u8>),
    Resize(TerminalSize),
}

impl PendingPaneCommand {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Input(bytes) => bytes.len(),
            Self::Resize(_) => 0,
        }
    }
}

struct PendingPaneCommandQueue {
    commands: VecDeque<PendingPaneCommand>,
    retained_bytes: usize,
}

impl PendingPaneCommandQueue {
    const fn new() -> Self {
        Self {
            commands: VecDeque::new(),
            retained_bytes: 0,
        }
    }

    fn submit_input(
        &mut self,
        bytes: &[u8],
        mut submit: impl FnMut(PendingPaneCommand) -> SubmitResult,
    ) -> std::io::Result<()> {
        let was_empty = self.commands.is_empty();
        for chunk in bytes.chunks(MAX_PENDING_INPUT_CHUNK_BYTES) {
            self.retained_bytes = self
                .retained_bytes
                .checked_add(chunk.len())
                .ok_or_else(|| std::io::Error::other("runtime V2 pending input overflow"))?;
            self.commands
                .push_back(PendingPaneCommand::Input(chunk.to_vec()));
        }
        if was_empty {
            self.flush(&mut submit)
        } else {
            Ok(())
        }
    }

    fn submit_resize(
        &mut self,
        size: TerminalSize,
        mut submit: impl FnMut(PendingPaneCommand) -> SubmitResult,
    ) -> std::io::Result<()> {
        if let Some(PendingPaneCommand::Resize(pending)) = self.commands.back_mut() {
            *pending = size;
            return Ok(());
        }
        let was_empty = self.commands.is_empty();
        self.commands.push_back(PendingPaneCommand::Resize(size));
        if was_empty {
            self.flush(&mut submit)
        } else {
            Ok(())
        }
    }

    fn flush(
        &mut self,
        mut submit: impl FnMut(PendingPaneCommand) -> SubmitResult,
    ) -> std::io::Result<()> {
        while let Some(command) = self.commands.front() {
            match submit(command.clone()) {
                SubmitResult::Accepted => {
                    let command = self.commands.pop_front().expect("front command exists");
                    self.retained_bytes =
                        self.retained_bytes.saturating_sub(command.retained_bytes());
                }
                SubmitResult::Backpressured { .. } => break,
                SubmitResult::Closed => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "runtime V2 pane is closed",
                    ));
                }
            }
        }
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

pub(crate) type SpawnedLocalPane = (SingleLocalPaneRuntime, Option<u32>, Option<String>);

pub(crate) struct ActiveWindowRuntime {
    composition: RuntimeComposition,
    presentation: TerminalRuntime,
    worker: Option<SingleLocalPaneRuntime>,
}

impl ActiveWindowRuntime {
    pub(crate) const fn legacy(presentation: TerminalRuntime) -> Self {
        Self {
            composition: RuntimeComposition::new(RuntimeSelection::Legacy),
            presentation,
            worker: None,
        }
    }

    pub(crate) const fn selection(&self) -> RuntimeSelection {
        self.composition.selection()
    }

    pub(crate) const fn set_composition(&mut self, composition: RuntimeComposition) {
        self.composition = composition;
    }

    pub(crate) const fn composition(&self) -> RuntimeComposition {
        self.composition
    }

    pub(crate) const fn worker(&self) -> Option<&SingleLocalPaneRuntime> {
        self.worker.as_ref()
    }

    pub(crate) fn worker_mut(&mut self) -> Option<&mut SingleLocalPaneRuntime> {
        self.worker.as_mut()
    }

    pub(crate) fn install_worker(&mut self, worker: Option<SingleLocalPaneRuntime>) {
        self.worker = worker;
    }

    pub(crate) fn take_worker(&mut self) -> Option<SingleLocalPaneRuntime> {
        self.worker.take()
    }
}

impl Deref for ActiveWindowRuntime {
    type Target = TerminalRuntime;

    fn deref(&self) -> &Self::Target {
        &self.presentation
    }
}

impl DerefMut for ActiveWindowRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.presentation
    }
}

impl SingleLocalPaneRuntime {
    fn adopt_local_session(
        route: PaneRuntimeRoute,
        session: PtySession,
        size: TerminalSize,
        terminal: rssh_runtime::TerminalRuntime,
        capture: PaneCapturePolicy,
        notice_waker: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<SpawnedLocalPane, Box<dyn Error>> {
        let process_id = session.process_id();
        let tty_name = session.tty_name();
        let transport = LocalPtyTransport::from_session(session)?;
        let runtime =
            Self::open_transport(route, transport, size, terminal, capture, notice_waker)?;
        Ok((runtime, process_id, tty_name))
    }

    fn open_transport<T: SessionTransport>(
        route: PaneRuntimeRoute,
        transport: T,
        size: TerminalSize,
        terminal: rssh_runtime::TerminalRuntime,
        capture: PaneCapturePolicy,
        notice_waker: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, Box<dyn Error>> {
        let mut hub = RuntimeHub::new_with_notice_waker(SystemClock, notice_waker);
        let mut config = PaneWorkerConfig {
            size,
            ..PaneWorkerConfig::default()
        };
        config.capture_host_stream = capture.host_stream;
        config.capture_visible_output = capture.visible_output;
        let handle = hub.open_with_runtime(route.pane, transport, config, terminal)?;
        let token = handle.token();
        let ports = RuntimePorts::new(hub, handle.clone());
        let mut state = WindowState::default();
        state.presentation.size = size;
        let mut host = WinitHost::new(
            route.window,
            state,
            ports,
            TurnBudget::new(64, Duration::from_millis(2)),
        );
        host.handle(PlatformEvent::PaneLifecycle(PaneLifecycleIntent::Opened(
            token,
        )))?;
        Ok(Self {
            host,
            token,
            handle,
            closed: false,
            pending_commands: PendingPaneCommandQueue::new(),
        })
    }

    pub(crate) const fn token(&self) -> PaneToken {
        self.token
    }

    pub(crate) fn submit_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let handle = self.handle.clone();
        self.pending_commands
            .submit_input(bytes, move |command| submit_pane_command(&handle, command))
    }

    pub(crate) fn resize(&mut self, size: TerminalSize) -> std::io::Result<()> {
        let handle = self.handle.clone();
        self.pending_commands
            .submit_resize(size, move |command| submit_pane_command(&handle, command))
    }

    pub(crate) fn begin_close(&mut self, grace: Duration) -> bool {
        self.host.ports_mut().hub.begin_close(self.token, grace)
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<RuntimeHostEvent>, Box<dyn Error>> {
        let handle = self.handle.clone();
        self.pending_commands
            .flush(move |command| submit_pane_command(&handle, command))?;
        let continuations = self.host.ports_mut().take_continuations();
        for pane in continuations {
            self.host
                .handle(PlatformEvent::RuntimeContinuation { pane })?;
        }

        while let Ok(notice) = self.host.ports_mut().hub.try_recv_notice() {
            match notice {
                PaneNotice::Ready(_) => {}
                PaneNotice::Wake(pane) => {
                    self.host.handle(PlatformEvent::RuntimeWake { pane })?;
                }
                PaneNotice::InputWriteCompleted {
                    pane: _,
                    byte_count,
                    elapsed,
                } => {
                    self.host
                        .ports_mut()
                        .events
                        .push_back(RuntimeHostEvent::InputWriteCompleted {
                            byte_count,
                            elapsed,
                        });
                }
                PaneNotice::FirstPtyByte { observed_at, .. } => self
                    .host
                    .ports_mut()
                    .events
                    .push_back(RuntimeHostEvent::FirstPtyByte { observed_at }),
                PaneNotice::Closed { pane, exit } => {
                    self.host
                        .handle(PlatformEvent::PaneLifecycle(PaneLifecycleIntent::Closed(
                            pane,
                        )))?;
                    self.closed = true;
                    self.host
                        .ports_mut()
                        .events
                        .push_back(RuntimeHostEvent::Closed { exit });
                }
            }
        }
        Ok(self.host.ports_mut().events.drain(..).collect())
    }

    pub(crate) fn needs_poll(&self) -> bool {
        !self.pending_commands.is_empty() || !self.host.ports().continuations.is_empty()
    }

    pub(crate) fn live_thread_count_for_metrics(&self) -> usize {
        self.host.ports().hub.live_thread_count()
    }

    pub(crate) fn shutdown(&mut self) {
        self.host.ports_mut().hub.shutdown();
        self.closed = true;
    }
}

fn submit_pane_command(handle: &PaneHandle, command: PendingPaneCommand) -> SubmitResult {
    match command {
        PendingPaneCommand::Input(bytes) => handle.submit_input(bytes),
        PendingPaneCommand::Resize(size) => handle.resize(size),
    }
}

impl Drop for SingleLocalPaneRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct PendingFrame {
    terminal: Arc<Terminal>,
    metadata: PaneMetadataDelta,
    metrics: RuntimeBatchMetrics,
    full_repaint: bool,
}

struct DrainBatch {
    frame: Option<rssh_runtime::PresentationFrame>,
    effects: Vec<RuntimeEffect>,
}

struct RuntimePorts {
    hub: RuntimeHub,
    handles: HashMap<PaneId, PaneHandle>,
    pending_frames: HashMap<(PaneToken, RuntimeRevision), PendingFrame>,
    events: VecDeque<RuntimeHostEvent>,
    continuations: VecDeque<PaneToken>,
}

impl RuntimePorts {
    fn new(hub: RuntimeHub, handle: PaneHandle) -> Self {
        Self {
            hub,
            handles: HashMap::from([(handle.token().pane(), handle)]),
            pending_frames: HashMap::new(),
            events: VecDeque::new(),
            continuations: VecDeque::new(),
        }
    }

    fn take_continuations(&mut self) -> Vec<PaneToken> {
        self.continuations.drain(..).collect()
    }

    fn unavailable(message: impl Into<String>) -> PortError {
        PortError::new(PortErrorKind::Unavailable, message)
    }

    fn submit_result(result: SubmitResult) -> Result<(), PortError> {
        match result {
            SubmitResult::Accepted => Ok(()),
            SubmitResult::Backpressured { .. } => Err(PortError::new(
                PortErrorKind::Backpressure,
                "pane command mailbox is full",
            )),
            SubmitResult::Closed => Err(Self::unavailable("pane runtime is closed")),
        }
    }
}

impl HostPorts for RuntimePorts {
    fn runtime(&mut self, effect: &RuntimePortEffect) -> Result<(), PortError> {
        match effect {
            RuntimePortEffect::SubmitInput { pane, bytes } => {
                let handle = self
                    .handles
                    .get(&pane.pane())
                    .filter(|handle| handle.token() == *pane)
                    .ok_or_else(|| Self::unavailable("pane input target is stale"))?;
                Self::submit_result(handle.submit_input(bytes.clone()))
            }
            RuntimePortEffect::ResizePane { pane, size } => {
                let handle = self
                    .handles
                    .get(&pane.pane())
                    .filter(|handle| handle.token() == *pane)
                    .ok_or_else(|| Self::unavailable("pane resize target is stale"))?;
                Self::submit_result(handle.resize(*size))
            }
            RuntimePortEffect::BeginClose { pane } => {
                if self.hub.begin_close(*pane, Duration::from_millis(250)) {
                    Ok(())
                } else {
                    Err(Self::unavailable("pane close target is stale"))
                }
            }
            RuntimePortEffect::WriteTransport { .. } => Err(PortError::new(
                PortErrorKind::Rejected,
                "pane worker owns transport response writes",
            )),
            RuntimePortEffect::ObserveHostStream { context, bytes } => {
                self.events.push_back(RuntimeHostEvent::HostStream {
                    pane: context.pane,
                    bytes: bytes.clone(),
                });
                Ok(())
            }
            RuntimePortEffect::WriteSessionLog { context, bytes } => {
                self.events.push_back(RuntimeHostEvent::VisibleOutput {
                    pane: context.pane,
                    bytes: bytes.clone(),
                });
                Ok(())
            }
            RuntimePortEffect::ApplyModeChange { context, change } => {
                self.events.push_back(RuntimeHostEvent::ModeChange {
                    pane: context.pane,
                    change: *change,
                });
                Ok(())
            }
            RuntimePortEffect::Restart { .. } => Err(Self::unavailable(
                "single-pane V2 selector does not own restart",
            )),
        }
    }

    fn window(&mut self, effect: &WindowPortEffect) -> Result<(), PortError> {
        match effect {
            WindowPortEffect::RequestRedraw => {
                self.events.push_back(RuntimeHostEvent::RequestRedraw);
            }
            WindowPortEffect::ReportDiagnostic(message) => {
                self.events.push_back(RuntimeHostEvent::Diagnostic {
                    pane: None,
                    message: message.clone(),
                });
            }
            WindowPortEffect::RuntimeDiagnostic { context, message } => {
                self.events.push_back(RuntimeHostEvent::Diagnostic {
                    pane: Some(context.pane),
                    message: message.clone(),
                });
            }
            WindowPortEffect::SetFocused(_)
            | WindowPortEffect::SetTitle(_)
            | WindowPortEffect::CloseAfterRuntimes
            | WindowPortEffect::CloseNow => {}
        }
        Ok(())
    }

    fn renderer(&mut self, effect: &RendererEffect) -> Result<(), PortError> {
        match effect {
            RendererEffect::ApplyPane {
                pane,
                revision,
                damage,
                ..
            } => {
                let frame = self
                    .pending_frames
                    .remove(&(*pane, *revision))
                    .ok_or_else(|| Self::unavailable("missing full terminal presentation"))?;
                self.events.push_back(RuntimeHostEvent::Frame {
                    pane: *pane,
                    terminal: frame.terminal,
                    damage: damage.clone(),
                    metadata: frame.metadata,
                    metrics: frame.metrics,
                    full_repaint: frame.full_repaint,
                });
            }
            RendererEffect::Bell { context, count } => {
                self.events.push_back(RuntimeHostEvent::Bell {
                    pane: context.pane,
                    count: count.get(),
                });
            }
            RendererEffect::ResizeSurface(_)
            | RendererEffect::ApplyConfig { .. }
            | RendererEffect::RecoverDevice
            | RendererEffect::Present => {}
        }
        Ok(())
    }

    fn clipboard(&mut self, effect: &ClipboardEffect) -> Result<(), PortError> {
        match effect {
            ClipboardEffect::Write {
                context,
                selection,
                contents,
            } => {
                let pane = context
                    .as_ref()
                    .map(|context| context.pane)
                    .ok_or_else(|| Self::unavailable("runtime clipboard write lacks context"))?;
                self.events.push_back(RuntimeHostEvent::ClipboardWrite {
                    pane,
                    selection: selection.clone(),
                    contents: contents.clone(),
                });
            }
            ClipboardEffect::Read { context, selection } => {
                self.events.push_back(RuntimeHostEvent::ClipboardRead {
                    pane: context.pane,
                    selection: selection.clone(),
                });
            }
        }
        Ok(())
    }

    fn uri(&mut self, _effect: &UriEffect) -> Result<(), PortError> {
        Ok(())
    }

    fn notification(&mut self, effect: &NotificationEffect) -> Result<(), PortError> {
        let NotificationEffect::Show {
            context,
            title,
            body,
        } = effect;
        self.events.push_back(RuntimeHostEvent::Notification {
            pane: context.pane,
            title: title.clone(),
            body: body.clone(),
        });
        Ok(())
    }

    fn persistence(&mut self, _effect: &PersistenceEffect) -> Result<(), PortError> {
        Ok(())
    }

    fn spawn(&mut self, _effect: &SpawnEffect) -> Result<(), PortError> {
        Err(Self::unavailable(
            "single-pane V2 selector does not own spawning",
        ))
    }

    fn drain_runtime(
        &mut self,
        pane: PaneToken,
        max_batches: usize,
    ) -> Result<RuntimeDrain, PortError> {
        let drain = self
            .hub
            .drain_pane(pane, max_batches)
            .ok_or_else(|| Self::unavailable("pane drain target is stale"))?;
        let mut batches = BTreeMap::<RuntimeRevision, DrainBatch>::new();
        for published in drain.effects {
            batches
                .entry(published.revision)
                .or_insert_with(|| DrainBatch {
                    frame: None,
                    effects: Vec::new(),
                })
                .effects
                .push(published.effect);
        }
        if let Some(frame) = drain.frame {
            let revision = frame.revision;
            batches
                .entry(revision)
                .or_insert_with(|| DrainBatch {
                    frame: None,
                    effects: Vec::new(),
                })
                .frame = Some(frame);
        }

        let intents = batches
            .into_iter()
            .map(|(revision, batch)| {
                let (snapshot, damage, metadata, metrics) = if let Some(frame) = batch.frame {
                    self.pending_frames.insert(
                        (pane, revision),
                        PendingFrame {
                            terminal: Arc::clone(&frame.snapshot),
                            metadata: frame.metadata.clone(),
                            metrics: frame.metrics,
                            full_repaint: frame.full_repaint,
                        },
                    );
                    (
                        Some(Arc::new(frame.state)),
                        frame.damage,
                        frame.metadata,
                        frame.metrics,
                    )
                } else {
                    (
                        None,
                        Vec::new(),
                        PaneMetadataDelta::default(),
                        RuntimeBatchMetrics::default(),
                    )
                };
                WindowIntent::RuntimeBatch(RuntimeBatch {
                    pane,
                    revision,
                    snapshot,
                    damage,
                    metadata,
                    effects: batch.effects,
                    metrics,
                })
            })
            .collect();
        Ok(RuntimeDrain {
            intents,
            continuation: drain.continuation,
        })
    }

    fn schedule_runtime_continuation(
        &mut self,
        _window: WindowId,
        pane: PaneToken,
    ) -> Result<(), PortError> {
        if !self.continuations.contains(&pane) {
            self.continuations.push_back(pane);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rssh_runtime::SubmitResult;

    use super::{
        MAX_PENDING_INPUT_CHUNK_BYTES, PendingPaneCommand, PendingPaneCommandQueue, TerminalSize,
    };

    #[test]
    fn pending_input_retries_in_order_after_runtime_backpressure() {
        let mut pending = PendingPaneCommandQueue::new();
        pending
            .submit_input(b"first", |_| SubmitResult::Backpressured {
                retry_after: std::time::Duration::from_millis(1),
            })
            .expect("queue first input");
        pending
            .submit_input(b"second", |_| SubmitResult::Accepted)
            .expect("queue behind pending input");

        let mut delivered = Vec::new();
        pending
            .flush(|command| {
                let PendingPaneCommand::Input(bytes) = command else {
                    panic!("unexpected resize")
                };
                delivered.push(bytes);
                SubmitResult::Accepted
            })
            .expect("flush pending input");

        assert_eq!(delivered, [b"first".to_vec(), b"second".to_vec()]);
        assert!(pending.is_empty());
    }

    #[test]
    fn pending_resizes_coalesce_without_reordering_input() {
        let mut pending = PendingPaneCommandQueue::new();
        pending
            .submit_input(b"input", |_| SubmitResult::Backpressured {
                retry_after: std::time::Duration::from_millis(1),
            })
            .unwrap();
        pending
            .submit_resize(TerminalSize::new(80, 24), |_| SubmitResult::Accepted)
            .unwrap();
        pending
            .submit_resize(TerminalSize::new(120, 40), |_| SubmitResult::Accepted)
            .unwrap();

        let mut delivered = Vec::new();
        pending
            .flush(|command| {
                delivered.push(match command {
                    PendingPaneCommand::Input(bytes) => format!("input:{}", bytes.len()),
                    PendingPaneCommand::Resize(size) => {
                        format!("resize:{}x{}", size.columns, size.rows)
                    }
                });
                SubmitResult::Accepted
            })
            .unwrap();
        assert_eq!(delivered, ["input:5", "resize:120x40"]);
    }

    #[test]
    fn oversized_input_is_chunked_and_deferred_without_blocking_the_ui_thread() {
        let mut pending = PendingPaneCommandQueue::new();
        let input = vec![b'x'; MAX_PENDING_INPUT_CHUNK_BYTES * 2 + 17];
        let mut attempts = 0;
        pending
            .submit_input(&input, |_| {
                attempts += 1;
                SubmitResult::Backpressured {
                    retry_after: std::time::Duration::from_millis(1),
                }
            })
            .expect("oversized input is retained for asynchronous retry");

        assert_eq!(attempts, 1, "the UI thread must never spin or sleep");
        let mut delivered = Vec::new();
        pending
            .flush(|command| {
                let PendingPaneCommand::Input(bytes) = command else {
                    panic!("unexpected resize")
                };
                assert!(bytes.len() <= MAX_PENDING_INPUT_CHUNK_BYTES);
                delivered.extend(bytes);
                SubmitResult::Accepted
            })
            .expect("asynchronous poll drains every chunk");
        assert_eq!(delivered, input);
        assert!(pending.is_empty());
    }
}
