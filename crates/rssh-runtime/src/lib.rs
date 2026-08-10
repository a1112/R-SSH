//! Transport-neutral terminal runtime contracts.

mod api;
mod metrics;
mod transport;

pub use api::{
    EffectSequence, MetadataChange, PaneGeneration, PaneMetadataDelta, PaneToken,
    PaneTokenAllocator, RuntimeBatch, RuntimeEffect, RuntimeEffectKind, RuntimeRevision,
    SequenceExhausted, SequenceKind, SubmitResult, UserVarDelta,
};
pub use metrics::RuntimeBatchMetrics;
pub use transport::{SessionControl, SessionExit, SessionParts, SessionTransport};
