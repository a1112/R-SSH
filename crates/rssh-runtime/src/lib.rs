//! Transport-neutral terminal runtime contracts.

mod api;
mod metrics;
mod transport;

pub use api::{
    EffectSequence, EffectSequenceCursor, EffectSequenceError, MetadataChange, PaneGeneration,
    PaneMetadataDelta, PaneToken, PaneTokenAllocator, RuntimeBatch, RuntimeEffect,
    RuntimeEffectKind, RuntimeProgress, RuntimeRevision, SequenceExhausted, SequenceKind,
    SubmitResult, UserVarDelta,
};
pub use metrics::RuntimeBatchMetrics;
pub use transport::{
    SessionControl, SessionExit, SessionExitSignal, SessionParts, SessionTransport,
};
