//! Deduplicated font diagnostics.

use std::collections::HashSet;

/// Kind of actionable terminal font diagnostic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticKind {
    /// A configured family is not present in the catalog.
    MissingFamily,
    /// No configured family covers an entire grapheme cluster.
    MissingCluster,
    /// The missing cluster is represented by the selected face's visible notdef glyph.
    VisibleTofu,
}

/// A stable, renderer-independent font diagnostic.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FontDiagnostic {
    /// Diagnostic category.
    pub kind: DiagnosticKind,
    /// Configured or selected family involved in the event.
    pub family: Option<String>,
    /// Grapheme cluster involved in the event.
    pub cluster: Option<String>,
    /// Catalog generation in which this condition was observed.
    pub catalog_generation: u64,
}

/// Insertion-ordered diagnostic set.
#[derive(Debug, Default)]
pub(crate) struct Diagnostics {
    seen: HashSet<FontDiagnostic>,
    items: Vec<FontDiagnostic>,
}

impl Diagnostics {
    pub(crate) fn record(&mut self, diagnostic: FontDiagnostic) {
        if self.seen.insert(diagnostic.clone()) {
            self.items.push(diagnostic);
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<FontDiagnostic> {
        self.items.clone()
    }
}
