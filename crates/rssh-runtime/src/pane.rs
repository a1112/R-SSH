use std::fmt;
use std::io::{self, Read, Write};
use std::num::NonZeroU64;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rssh_core::{DamageRegion, TerminalSize};

use crate::{
    MailboxItem, MailboxLimits, MailboxReceiver, MailboxSender, MetadataChange, MetadataChangeRef,
    PaneMetadataDelta, PaneToken, RuntimeBuffers, RuntimeEffectKind, RuntimeRevision, SessionExit,
    SessionInterrupt, SessionTransport, SubmitResult, TerminalRuntime, TrySendError, UserVarDelta,
    bounded_mailbox, delta::RuntimeEffectRef,
};

const BACKPRESSURE_RETRY: Duration = Duration::from_millis(1);
const READER_CHUNK_BYTES: usize = 8 * 1024;

/// Bounded resources and initial terminal state for one pane worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneWorkerConfig {
    /// Initial terminal grid size.
    pub size: TerminalSize,
    /// Shared bounded inbox used by controller commands and reader events.
    pub inbox_limits: MailboxLimits,
}

impl Default for PaneWorkerConfig {
    fn default() -> Self {
        Self {
            size: TerminalSize::new(80, 24),
            inbox_limits: MailboxLimits::try_new(256, 2 * 1024 * 1024)
                .expect("default pane mailbox limits are nonzero"),
        }
    }
}

/// Lossless lifecycle or terminal-progress notice emitted by a pane worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneNotice {
    /// The worker owns all session resources and accepts commands.
    Ready(PaneToken),
    /// One terminal event advanced this pane generation.
    Advanced {
        /// Pane generation that produced the event.
        pane: PaneToken,
        /// Strictly increasing revision within the pane generation.
        revision: RuntimeRevision,
        /// Whether a new presentation snapshot is required.
        snapshot_changed: bool,
        /// Normalized terminal damage.
        damage: Vec<DamageRegion>,
        /// Final-only metadata changes from this event.
        metadata: PaneMetadataDelta,
        /// Ordered host effects not already consumed by the transport writer.
        effects: Vec<RuntimeEffectKind>,
    },
    /// The worker and its blocking reader have stopped.
    Closed {
        /// Pane generation that stopped.
        pane: PaneToken,
        /// Transport exit status when it was available.
        exit: Option<SessionExit>,
    },
}

impl PaneNotice {
    /// Returns the pane generation that emitted this notice.
    #[must_use]
    pub const fn pane(&self) -> PaneToken {
        match self {
            Self::Ready(pane) | Self::Advanced { pane, .. } | Self::Closed { pane, .. } => *pane,
        }
    }
}

impl MailboxItem for PaneNotice {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Ready(_) => 0,
            Self::Closed { exit, .. } => exit.as_ref().map_or(0, |exit| {
                exit.signal.as_ref().map_or(0, |signal| {
                    signal
                        .name
                        .capacity()
                        .saturating_add(signal.error_message.capacity())
                        .saturating_add(signal.lang_tag.capacity())
                })
            }),
            Self::Advanced {
                damage,
                metadata,
                effects,
                ..
            } => damage
                .capacity()
                .saturating_mul(std::mem::size_of::<DamageRegion>())
                .saturating_add(metadata_retained_bytes(metadata))
                .saturating_add(
                    effects
                        .capacity()
                        .saturating_mul(std::mem::size_of::<RuntimeEffectKind>()),
                )
                .saturating_add(effects.iter().map(effect_retained_bytes).sum::<usize>()),
        }
    }
}

fn metadata_retained_bytes(metadata: &PaneMetadataDelta) -> usize {
    change_capacity(metadata.title.as_ref())
        .saturating_add(change_capacity(metadata.working_directory.as_ref()))
        .saturating_add(change_capacity(metadata.badge_format.as_ref()))
        .saturating_add(
            metadata
                .user_vars
                .capacity()
                .saturating_mul(std::mem::size_of::<UserVarDelta>()),
        )
        .saturating_add(metadata.user_vars.iter().fold(0usize, |bytes, change| {
            bytes
                .saturating_add(change.name.capacity())
                .saturating_add(metadata_change_capacity(&change.value))
        }))
}

fn change_capacity(change: Option<&MetadataChange<String>>) -> usize {
    match change {
        Some(MetadataChange::Set(value)) => value.capacity(),
        Some(MetadataChange::Clear) | None => 0,
    }
}

fn metadata_change_capacity(change: &MetadataChange<String>) -> usize {
    match change {
        MetadataChange::Set(value) => value.capacity(),
        MetadataChange::Clear => 0,
    }
}

fn effect_retained_bytes(effect: &RuntimeEffectKind) -> usize {
    match effect {
        RuntimeEffectKind::TransportWrite(bytes) => bytes.capacity(),
        RuntimeEffectKind::Bell { .. } => 0,
        RuntimeEffectKind::ClipboardWrite {
            selection,
            contents,
        } => selection.as_ref().map_or(0, String::capacity) + contents.capacity(),
        RuntimeEffectKind::ClipboardRead { selection } => selection.capacity(),
        RuntimeEffectKind::Notification { title, body } => {
            title.as_ref().map_or(0, String::capacity) + body.capacity()
        }
        RuntimeEffectKind::Diagnostic { message } => message.capacity(),
    }
}

#[derive(Debug)]
enum PaneMessage {
    Input(Vec<u8>),
    Resize(TerminalSize),
    Output(Vec<u8>),
    ReaderEof,
    ReaderError,
    Close,
}

impl MailboxItem for PaneMessage {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Input(bytes) | Self::Output(bytes) => bytes.capacity(),
            Self::Resize(_) | Self::ReaderEof | Self::ReaderError | Self::Close => 0,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ErasedInterrupt {
    call: Arc<dyn Fn() -> io::Result<()> + Send + Sync>,
}

impl fmt::Debug for ErasedInterrupt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ErasedInterrupt(..)")
    }
}

impl ErasedInterrupt {
    fn new<I: SessionInterrupt>(interrupt: I) -> Self {
        Self {
            call: Arc::new(move || interrupt.interrupt()),
        }
    }

    pub(crate) fn interrupt(&self) -> io::Result<()> {
        (self.call)()
    }
}

/// Cloneable controller endpoint for one pane generation.
#[derive(Clone)]
pub struct PaneHandle {
    token: PaneToken,
    sender: MailboxSender<PaneMessage>,
    accepting: Arc<AtomicBool>,
}

impl fmt::Debug for PaneHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PaneHandle")
            .field("token", &self.token)
            .field("accepting", &self.accepting.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl PaneHandle {
    /// Returns the exact pane generation owned by this handle.
    #[must_use]
    pub const fn token(&self) -> PaneToken {
        self.token
    }

    /// Submits ordered user input without silently dropping it.
    pub fn submit_input(&self, bytes: Vec<u8>) -> SubmitResult {
        self.submit(PaneMessage::Input(bytes))
    }

    /// Submits a terminal resize to the same one-owner worker.
    pub fn resize(&self, size: TerminalSize) -> SubmitResult {
        self.submit(PaneMessage::Resize(size))
    }

    fn submit(&self, message: PaneMessage) -> SubmitResult {
        if !self.accepting.load(Ordering::Acquire) {
            return SubmitResult::Closed;
        }
        match self.sender.try_send(message) {
            Ok(()) => SubmitResult::Accepted,
            Err(TrySendError::Closed(_)) => SubmitResult::Closed,
            Err(TrySendError::Full { .. } | TrySendError::Oversize { .. }) => {
                SubmitResult::Backpressured {
                    retry_after: BACKPRESSURE_RETRY,
                }
            }
        }
    }

    pub(crate) fn stop_accepting(&self) {
        self.accepting.store(false, Ordering::Release);
    }

    pub(crate) fn request_close(&self) {
        self.stop_accepting();
        let _ = self.sender.try_send(PaneMessage::Close);
    }
}

pub(crate) struct SpawnedPane {
    pub handle: PaneHandle,
    pub interrupt: ErasedInterrupt,
    pub join: JoinHandle<()>,
}

struct LiveThreadGuard {
    count: Arc<AtomicUsize>,
}

impl LiveThreadGuard {
    fn new(count: Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        Self { count }
    }
}

impl Drop for LiveThreadGuard {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) fn spawn_pane<T: SessionTransport>(
    token: PaneToken,
    transport: T,
    config: PaneWorkerConfig,
    notices: MailboxSender<PaneNotice>,
    live_threads: &Arc<AtomicUsize>,
) -> io::Result<SpawnedPane> {
    let parts = transport.split();
    let interrupt = ErasedInterrupt::new(parts.interrupt.clone());
    let (sender, receiver) = bounded_mailbox(config.inbox_limits);
    let accepting = Arc::new(AtomicBool::new(true));

    let reader_sender = sender.clone();
    let reader_interrupt = interrupt.clone();
    let reader_live_threads = Arc::clone(live_threads);
    let reader_join = thread::Builder::new()
        .name(format!("rssh-pane-reader-{}", token.pane().get()))
        .spawn(move || {
            let _guard = LiveThreadGuard::new(reader_live_threads);
            run_reader(parts.reader, &reader_sender);
        })?;

    let worker_interrupt = interrupt.clone();
    let worker_live_threads = Arc::clone(live_threads);
    let worker_accepting = Arc::clone(&accepting);
    let join = match thread::Builder::new()
        .name(format!("rssh-pane-worker-{}", token.pane().get()))
        .spawn(move || {
            let _guard = LiveThreadGuard::new(worker_live_threads);
            run_worker(
                token,
                config.size,
                receiver,
                parts.writer,
                parts.control,
                &worker_interrupt,
                &worker_accepting,
                reader_join,
                &notices,
            );
        }) {
        Ok(join) => join,
        Err(error) => {
            let _ = reader_interrupt.interrupt();
            return Err(error);
        }
    };

    Ok(SpawnedPane {
        handle: PaneHandle {
            token,
            sender,
            accepting,
        },
        interrupt,
        join,
    })
}

fn run_reader<R: Read>(mut reader: R, sender: &MailboxSender<PaneMessage>) {
    let mut buffer = vec![0; READER_CHUNK_BYTES];
    loop {
        let message = match reader.read(&mut buffer) {
            Ok(0) => PaneMessage::ReaderEof,
            Ok(count) => PaneMessage::Output(buffer[..count].to_vec()),
            Err(_) => PaneMessage::ReaderError,
        };
        let terminal = matches!(message, PaneMessage::ReaderEof | PaneMessage::ReaderError);
        if sender.send(message).is_err() || terminal {
            break;
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "worker entry receives each uniquely owned session resource exactly once"
)]
fn run_worker<W: Write, C: crate::SessionControl>(
    token: PaneToken,
    size: TerminalSize,
    mut receiver: MailboxReceiver<PaneMessage>,
    mut writer: W,
    mut control: C,
    interrupt: &ErasedInterrupt,
    accepting: &Arc<AtomicBool>,
    reader_join: JoinHandle<()>,
    notices: &MailboxSender<PaneNotice>,
) {
    let mut runtime = TerminalRuntime::new(size);
    let mut buffers = RuntimeBuffers::default();
    let mut revision = RuntimeRevision::FIRST;
    if notices.send(PaneNotice::Ready(token)).is_err() {
        accepting.store(false, Ordering::Release);
        let _ = interrupt.interrupt();
        let _ = reader_join.join();
        return;
    }

    while let Ok(message) = receiver.recv() {
        let result = match message {
            PaneMessage::Input(bytes) => writer.write_all(&bytes),
            PaneMessage::Resize(new_size) => control.resize(new_size).and_then(|()| {
                let (_, delta) = runtime.resize_into(new_size, &mut buffers);
                publish_delta(token, revision, delta, &mut writer, notices, true)?;
                revision = next_revision(revision)?;
                Ok(())
            }),
            PaneMessage::Output(bytes) => advance_terminal(
                token,
                revision,
                &bytes,
                &mut runtime,
                &mut buffers,
                &mut writer,
                notices,
            )
            .and_then(|()| {
                revision = next_revision(revision)?;
                Ok(())
            }),
            PaneMessage::ReaderEof | PaneMessage::ReaderError | PaneMessage::Close => break,
        };
        if result.is_err() {
            break;
        }
    }

    accepting.store(false, Ordering::Release);
    let _ = receiver.close();
    let _ = interrupt.interrupt();
    let _ = control.begin_close();
    let delta = runtime.finish_into(&mut buffers);
    let _ = publish_delta(token, revision, delta, &mut writer, notices, false);
    let _ = reader_join.join();
    let exit = control.poll_exit().ok().flatten();
    let _ = notices.send(PaneNotice::Closed { pane: token, exit });
}

fn advance_terminal<W: Write>(
    token: PaneToken,
    revision: RuntimeRevision,
    bytes: &[u8],
    runtime: &mut TerminalRuntime,
    buffers: &mut RuntimeBuffers,
    writer: &mut W,
    notices: &MailboxSender<PaneNotice>,
) -> io::Result<()> {
    let delta = runtime.feed_into(bytes, buffers);
    publish_delta(token, revision, delta, writer, notices, true)
}

fn publish_delta<W: Write>(
    token: PaneToken,
    revision: RuntimeRevision,
    delta: crate::RuntimeDelta<'_>,
    writer: &mut W,
    notices: &MailboxSender<PaneNotice>,
    always_publish: bool,
) -> io::Result<()> {
    let transport_result = write_transport_effects(delta, writer);
    let damage = delta.damage().to_vec();
    let metadata = owned_metadata(delta.metadata());
    let effects = owned_host_effects(delta);
    let should_publish = always_publish
        || delta.snapshot_changed()
        || !damage.is_empty()
        || metadata != PaneMetadataDelta::default()
        || !effects.is_empty();
    if !should_publish {
        return transport_result;
    }
    let notice = PaneNotice::Advanced {
        pane: token,
        revision,
        snapshot_changed: delta.snapshot_changed(),
        damage,
        metadata,
        effects,
    };
    let notice_result = notices
        .send(notice)
        .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe));
    transport_result.and(notice_result)
}

fn next_revision(revision: RuntimeRevision) -> io::Result<RuntimeRevision> {
    revision
        .next()
        .map_err(|_| io::Error::other("pane runtime revision exhausted"))
}

fn write_transport_effects<W: Write>(
    delta: crate::RuntimeDelta<'_>,
    writer: &mut W,
) -> io::Result<()> {
    for effect in delta.effects() {
        if let RuntimeEffectRef::TransportWrite(bytes) = effect {
            writer.write_all(bytes)?;
        }
    }
    Ok(())
}

fn owned_host_effects(delta: crate::RuntimeDelta<'_>) -> Vec<RuntimeEffectKind> {
    delta
        .effects()
        .filter_map(|effect| match effect {
            RuntimeEffectRef::TransportWrite(_)
            | RuntimeEffectRef::ConsoleWrite(_)
            | RuntimeEffectRef::ModeChange(_) => None,
            RuntimeEffectRef::Bell { count } => {
                NonZeroU64::new(count).map(|count| RuntimeEffectKind::Bell { count })
            }
            RuntimeEffectRef::ClipboardWrite {
                selection,
                contents,
            } => Some(RuntimeEffectKind::ClipboardWrite {
                selection: selection.map(str::to_owned),
                contents: contents.to_owned(),
            }),
            RuntimeEffectRef::ClipboardRead { selection } => {
                Some(RuntimeEffectKind::ClipboardRead {
                    selection: selection.to_owned(),
                })
            }
            RuntimeEffectRef::Notification { title, body } => {
                Some(RuntimeEffectKind::Notification {
                    title: title.map(str::to_owned),
                    body: body.to_owned(),
                })
            }
            RuntimeEffectRef::Diagnostic { message } => Some(RuntimeEffectKind::Diagnostic {
                message: message.to_owned(),
            }),
        })
        .collect()
}

fn owned_metadata(metadata: crate::RuntimeMetadataDeltaRef<'_>) -> PaneMetadataDelta {
    PaneMetadataDelta {
        title: owned_change(metadata.title()),
        working_directory: owned_change(metadata.working_directory()),
        badge_format: owned_change(metadata.badge_format()),
        progress: metadata.progress().map(MetadataChange::Set),
        user_vars: metadata
            .user_vars()
            .map(|(name, value)| UserVarDelta {
                name: name.to_owned(),
                value: owned_change(Some(value)).expect("user variable always has a change"),
            })
            .collect(),
    }
}

fn owned_change(change: Option<MetadataChangeRef<'_>>) -> Option<MetadataChange<String>> {
    change.map(|change| match change {
        MetadataChangeRef::Set(value) => MetadataChange::Set(value.to_owned()),
        MetadataChangeRef::Clear => MetadataChange::Clear,
    })
}
