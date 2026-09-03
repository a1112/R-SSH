//! Deterministic terminal font selection, shaping, fallback, and diagnostics.
//!
//! The catalog is isolated from fonts installed on the host. Applications must
//! load repository-owned or caller-selected font bytes explicitly. Normal
//! catalogs share each immutable source allocation with the shaping database;
//! the historical copied-allocation path is available only to diagnostics.

mod cache;
mod catalog;
mod config;
mod diagnostics;
mod raster;
mod shape;

pub use cache::CacheMetrics;
pub use catalog::{CatalogError, CatalogMemoryMetrics, FontCatalog, FontId, FontSource};
pub use config::{BidiMode, FontConfig, FontStretch, FontStyle};
pub use diagnostics::{DiagnosticKind, FontDiagnostic};
pub use raster::{
    PositionedRaster, RasterCache, RasterCacheConfig, RasterContent, RasterFallback, RasterFlags,
    RasterRequest, RasterizedGlyph,
};
pub use shape::{
    CellSpan, ClusterSpan, ShapeCacheStats, ShapeError, ShapedCluster, ShapedGlyph, ShapedRow,
    TerminalCluster, TerminalFontMetrics, TerminalShaper,
};
