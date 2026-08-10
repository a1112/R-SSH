//! Transport-neutral terminal runtime contracts.

mod api;
mod mailbox;
mod metrics;
mod transport;

pub use api::{
    EffectSequence, EffectSequenceCursor, EffectSequenceError, MetadataChange, PaneGeneration,
    PaneMetadataDelta, PaneToken, PaneTokenAllocator, RuntimeBatch, RuntimeEffect,
    RuntimeEffectKind, RuntimeProgress, RuntimeRevision, SequenceExhausted, SequenceKind,
    SubmitResult, UserVarDelta,
};
pub use mailbox::{
    MailboxItem, MailboxLimits, MailboxLimitsError, MailboxMetrics, MailboxReceiver, MailboxSender,
    RecvError, SendError, TryRecvError, TrySendError, bounded_mailbox,
};
pub use metrics::RuntimeBatchMetrics;
pub use transport::{
    SessionControl, SessionExit, SessionExitSignal, SessionParts, SessionTransport,
};
