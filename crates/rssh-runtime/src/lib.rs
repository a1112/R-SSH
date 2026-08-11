//! Transport-neutral terminal runtime contracts.

mod api;
pub mod delta;
mod mailbox;
mod metrics;
pub mod modes;
pub mod queries;
pub mod query_dcs;
pub mod terminal;

#[cfg(test)]
#[path = "fixture_trace.rs"]
pub(crate) mod fixture_trace;
#[cfg(test)]
#[path = "frozen_trace_pack.rs"]
pub(crate) mod frozen_trace_pack;
#[cfg(test)]
#[path = "test_body_digest.rs"]
pub(crate) mod test_body_digest;
mod transport;
pub mod visible_output;

pub use api::{
    EffectSequence, EffectSequenceCursor, EffectSequenceError, MetadataChange, PaneGeneration,
    PaneMetadataDelta, PaneToken, PaneTokenAllocator, RuntimeBatch, RuntimeEffect,
    RuntimeEffectKind, RuntimeProgress, RuntimeRevision, SequenceExhausted, SequenceKind,
    SubmitResult, UserVarDelta,
};
pub use delta::{
    MetadataChangeRef, RuntimeBufferCapacities, RuntimeBuffers, RuntimeDelta, RuntimeEffectRef,
    RuntimeMetadataDeltaRef, TerminalSnapshotRef,
};
pub use mailbox::{
    MailboxItem, MailboxLimits, MailboxLimitsError, MailboxMetrics, MailboxReceiver, MailboxSender,
    RecvError, SendError, TryRecvError, TrySendError, bounded_mailbox,
};
pub use metrics::RuntimeBatchMetrics;
pub use modes::{MouseInputMode, MouseProtocolMode, MouseReportingMode, TerminalModeChange};
pub use terminal::TerminalRuntime;
pub use transport::{
    SessionControl, SessionExit, SessionExitSignal, SessionParts, SessionTransport,
};
