use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
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
    input::{PendingPaneCommand, PendingPaneCommandQueue},
};
use rssh_pty::PtySession;
use rssh_runtime::{
    LocalPtyTransport, PaneHandle, PaneMetadataDelta, PaneNotice, PaneToken, PaneWorkerConfig,
    RuntimeBatch, RuntimeBatchMetrics, RuntimeEffect, RuntimeHub, RuntimeRevision, SessionExit,
    SessionTransport, SubmitResult, SystemClock,
};
use rssh_terminal::Terminal;

use crate::terminal_runtime::TerminalRuntime;

type AdoptLocalSession = fn(
    PaneRuntimeRoute,
    PtySession,
    TerminalSize,
    rssh_runtime::TerminalRuntime,
    PaneCapturePolicy,
    Arc<dyn Fn() + Send + Sync>,
) -> Result<SpawnedLocalPane, Box<dyn Error>>;

#[derive(Debug, Clone, Copy)]
pub(crate) struct RuntimeComposition {
    adopt_local_session: AdoptLocalSession,
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
    pub(crate) const fn new() -> Self {
        Self {
            adopt_local_session: WindowPaneRuntime::adopt_local_session,
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
        (self.adopt_local_session)(route, session, size, terminal, capture, notice_waker)
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
        pane: PaneToken,
        exit: Option<SessionExit>,
    },
}

pub(crate) struct WindowPaneRuntime {
    host: WinitHost<RuntimePorts>,
    token: PaneToken,
    closed: bool,
    pending_commands: HashMap<PaneId, PendingPaneCommandQueue>,
    closing: HashSet<PaneToken>,
}

pub(crate) type SpawnedLocalPane = (WindowPaneRuntime, Option<u32>, Option<String>);
pub(crate) type AdoptedLocalPane = (PaneToken, Option<u32>, Option<String>);

pub(crate) struct ActiveWindowRuntime {
    composition: RuntimeComposition,
    presentation: TerminalRuntime,
    worker: Option<WindowPaneRuntime>,
}

impl ActiveWindowRuntime {
    pub(crate) const fn new(presentation: TerminalRuntime) -> Self {
        Self {
            composition: RuntimeComposition::new(),
            presentation,
            worker: None,
        }
    }

    pub(crate) const fn set_composition(&mut self, composition: RuntimeComposition) {
        self.composition = composition;
    }

    pub(crate) const fn composition(&self) -> RuntimeComposition {
        self.composition
    }

    pub(crate) const fn worker(&self) -> Option<&WindowPaneRuntime> {
        self.worker.as_ref()
    }

    pub(crate) fn worker_mut(&mut self) -> Option<&mut WindowPaneRuntime> {
        self.worker.as_mut()
    }

    pub(crate) fn install_worker(&mut self, worker: Option<WindowPaneRuntime>) {
        self.worker = worker;
    }

    pub(crate) fn take_worker(&mut self) -> Option<WindowPaneRuntime> {
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

impl WindowPaneRuntime {
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
            closed: false,
            pending_commands: HashMap::from([(route.pane, PendingPaneCommandQueue::new())]),
            closing: HashSet::new(),
        })
    }

    pub(crate) const fn token(&self) -> PaneToken {
        self.token
    }

    fn add_transport<T: SessionTransport>(
        &mut self,
        pane: PaneId,
        transport: T,
        config: PaneWorkerConfig,
        runtime: rssh_runtime::TerminalRuntime,
    ) -> Result<PaneToken, Box<dyn Error>> {
        let replacing = self.host.state().panes.contains_key(&pane);
        let handle = if replacing {
            self.host
                .ports_mut()
                .hub
                .restart_with_runtime(pane, transport, config, runtime)?
        } else {
            self.host
                .ports_mut()
                .hub
                .open_with_runtime(pane, transport, config, runtime)?
        };
        let token = handle.token();
        self.host.ports_mut().handles.insert(pane, handle);
        self.pending_commands
            .insert(pane, PendingPaneCommandQueue::new());
        self.host
            .handle(PlatformEvent::PaneLifecycle(PaneLifecycleIntent::Opened(
                token,
            )))?;
        self.closing.retain(|closing| closing.pane() != pane);
        self.closed = false;
        Ok(token)
    }

    pub(crate) fn adopt_additional_local_session(
        &mut self,
        pane: PaneId,
        session: PtySession,
        size: TerminalSize,
        terminal: rssh_runtime::TerminalRuntime,
        capture: PaneCapturePolicy,
    ) -> Result<AdoptedLocalPane, Box<dyn Error>> {
        let process_id = session.process_id();
        let tty_name = session.tty_name();
        let transport = LocalPtyTransport::from_session(session)?;
        let config = PaneWorkerConfig {
            size,
            capture_host_stream: capture.host_stream,
            capture_visible_output: capture.visible_output,
            ..PaneWorkerConfig::default()
        };
        let token = self.add_transport(pane, transport, config, terminal)?;
        Ok((token, process_id, tty_name))
    }

    pub(crate) fn pane_tokens(&self) -> Vec<PaneToken> {
        self.host
            .state()
            .pane_order
            .iter()
            .filter_map(|pane| self.host.state().panes.get(pane).map(|state| state.token))
            .collect()
    }

    #[cfg(test)]
    fn active_token(&self) -> PaneToken {
        self.token
    }

    pub(crate) fn activate(&mut self, token: PaneToken) -> Result<(), Box<dyn Error>> {
        self.host.handle(PlatformEvent::PaneLifecycle(
            PaneLifecycleIntent::Activated(token),
        ))?;
        if self.host.state().active_pane == Some(token.pane()) {
            self.token = token;
        }
        Ok(())
    }

    pub(crate) fn activate_pane(&mut self, pane: PaneId) -> Result<(), Box<dyn Error>> {
        let token = self
            .host
            .state()
            .panes
            .get(&pane)
            .map(|state| state.token)
            .ok_or_else(|| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("runtime V2 pane {} is not open", pane.get()),
                )) as Box<dyn Error>
            })?;
        self.activate(token)
    }

    pub(crate) fn submit_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let handle = self
            .host
            .ports()
            .handles
            .get(&self.token.pane())
            .filter(|handle| handle.token() == self.token)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        self.pending_commands
            .get_mut(&self.token.pane())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?
            .submit_input(bytes, move |command| submit_pane_command(&handle, command))
    }

    pub(crate) fn resize_all(&mut self, size: TerminalSize) -> std::io::Result<()> {
        let panes = self.pane_tokens();
        for token in panes {
            let handle = self
                .host
                .ports()
                .handles
                .get(&token.pane())
                .filter(|handle| handle.token() == token)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
            self.pending_commands
                .get_mut(&token.pane())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?
                .submit_resize(size, move |command| submit_pane_command(&handle, command))?;
        }
        Ok(())
    }

    pub(crate) fn begin_close(&mut self, grace: Duration) -> bool {
        self.begin_close_pane(self.token, grace)
    }

    fn begin_close_pane(&mut self, token: PaneToken, grace: Duration) -> bool {
        if self.closing.contains(&token) || !self.host.ports_mut().hub.begin_close(token, grace) {
            return false;
        }
        self.closing.insert(token);
        true
    }

    pub(crate) fn begin_close_by_pane(&mut self, pane: PaneId, grace: Duration) -> bool {
        let Some(token) = self.host.state().panes.get(&pane).map(|state| state.token) else {
            return false;
        };
        self.begin_close_pane(token, grace)
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<RuntimeHostEvent>, Box<dyn Error>> {
        let pending_panes = self.pending_commands.keys().copied().collect::<Vec<_>>();
        for pane in pending_panes {
            let Some(handle) = self.host.ports().handles.get(&pane).cloned() else {
                continue;
            };
            if let Some(pending) = self.pending_commands.get_mut(&pane) {
                pending.flush(move |command| submit_pane_command(&handle, command))?;
            }
        }
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
                    self.host.ports_mut().handles.remove(&pane.pane());
                    self.pending_commands.remove(&pane.pane());
                    self.closing.remove(&pane);
                    if self.token == pane
                        && let Some(active) = self.host.state().active_pane.and_then(|active| {
                            self.host
                                .state()
                                .panes
                                .get(&active)
                                .map(|state| state.token)
                        })
                    {
                        self.token = active;
                    }
                    self.closed = self.host.state().panes.is_empty();
                    self.host
                        .ports_mut()
                        .events
                        .push_back(RuntimeHostEvent::Closed { pane, exit });
                }
            }
        }
        Ok(self.host.ports_mut().events.drain(..).collect())
    }

    pub(crate) fn needs_poll(&self) -> bool {
        self.pending_commands
            .values()
            .any(|pending| !pending.is_empty())
            || !self.host.ports().continuations.is_empty()
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

impl Drop for WindowPaneRuntime {
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
                "runtime selector does not own pane restart",
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
            "runtime selector does not own pane spawning",
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
    use std::sync::Arc;

    use rssh_core::{PaneId, WindowId};
    use rssh_runtime::{
        PaneWorkerConfig, TerminalRuntime,
        testing::{ReadAction, ScriptedTransport, WriteAction},
    };

    use super::{PaneCapturePolicy, PaneRuntimeRoute, TerminalSize, WindowPaneRuntime};

    #[test]
    fn window_runtime_owns_multiple_pane_workers_and_active_selection() {
        let size = TerminalSize::new(80, 24);
        let (first_transport, first_driver) =
            ScriptedTransport::new([ReadAction::Block], [WriteAction::accept(usize::MAX)], []);
        let mut runtime = WindowPaneRuntime::open_transport(
            PaneRuntimeRoute {
                window: WindowId::new(1),
                pane: PaneId::new(201),
            },
            first_transport,
            size,
            TerminalRuntime::new(size),
            PaneCapturePolicy {
                host_stream: false,
                visible_output: false,
            },
            Arc::new(|| {}),
        )
        .expect("open first pane");
        let first = runtime.token();
        let (second_transport, second_driver) =
            ScriptedTransport::new([ReadAction::Block], [WriteAction::accept(usize::MAX)], []);
        let second = runtime
            .add_transport(
                PaneId::new(202),
                second_transport,
                PaneWorkerConfig::default(),
                TerminalRuntime::new(size),
            )
            .expect("open second pane");

        assert_eq!(runtime.pane_tokens(), vec![first, second]);
        runtime.activate(second).expect("activate second pane");
        assert_eq!(runtime.active_token(), second);
        let resized = TerminalSize::new(120, 40);
        runtime.resize_all(resized).expect("resize all panes");
        first_driver.wait_until_control_call_count(1);
        second_driver.wait_until_control_call_count(1);
        assert_eq!(first_driver.control_log().resizes, vec![resized]);
        assert_eq!(second_driver.control_log().resizes, vec![resized]);
        assert!(runtime.begin_close_pane(first, std::time::Duration::ZERO));
        assert!(!runtime.begin_close_pane(first, std::time::Duration::ZERO));
        runtime.shutdown();
        assert_eq!(runtime.live_thread_count_for_metrics(), 0);
    }

    #[test]
    fn v2_worker_ownership_is_window_scoped_instead_of_pane_scoped() {
        let window_source = [
            include_str!("window.rs"),
            include_str!("window_parts/part10.rs"),
            include_str!("window_parts/part15.rs"),
        ]
        .join("\n");
        assert!(
            !window_source.contains("v2_runtime: Option<WindowPaneRuntime>"),
            "pane-local ownership would create one RuntimeHub per pane"
        );
        assert!(
            !window_source.contains("self.app_shell.pane_ids().len() == 1"),
            "V2 ownership must not silently fall back after the first pane"
        );
    }
}
