//! Deterministic terminal font selection, shaping, fallback, and diagnostics.
//!
//! The catalog is isolated from fonts installed on the host. Applications must
//! load repository-owned or caller-selected font bytes explicitly.

mod catalog;
mod config;
mod diagnostics;
mod shape;

pub use catalog::{CatalogError, FontCatalog, FontId, FontSource};
pub use config::{BidiMode, FontConfig, FontStretch, FontStyle};
pub use diagnostics::{DiagnosticKind, FontDiagnostic};
pub use shape::{
    CellSpan, ClusterSpan, ShapeCacheStats, ShapeError, ShapedCluster, ShapedGlyph, ShapedRow,
    TerminalCluster, TerminalFontMetrics, TerminalShaper,
};
