//! Deterministic, caller-owned font catalog.

use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cosmic_text::{FontSystem, fontdb};
use sha2::{Digest, Sha256};

use crate::config::{FontStretch, FontStyle};

static NEXT_CATALOG_INCARNATION: AtomicU64 = AtomicU64::new(1);

/// Font identifier scoped to one [`FontCatalog::generation`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FontId {
    raw: Option<fontdb::ID>,
    catalog_incarnation: u64,
    catalog_generation: u64,
}

impl FontId {
    /// Catalog instance that owns the underlying database identifier.
    #[must_use]
    pub const fn catalog_incarnation(self) -> u64 {
        self.catalog_incarnation
    }

    /// Catalog generation that owns the underlying database identifier.
    #[must_use]
    pub const fn catalog_generation(self) -> u64 {
        self.catalog_generation
    }

    pub(crate) const fn raw(self) -> Option<fontdb::ID> {
        self.raw
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FontBytes(Box<[u8]>);

impl AsRef<[u8]> for FontBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Caller-provided font bytes and a diagnostic label.
#[derive(Clone, Eq, PartialEq)]
pub struct FontSource {
    /// Human-readable source name or path.
    pub label: String,
    /// Complete font bytes.
    bytes: Arc<FontBytes>,
}

impl fmt::Debug for FontSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FontSource")
            .field("label", &self.label)
            .field("bytes", &self.bytes())
            .finish()
    }
}

impl FontSource {
    /// Creates an in-memory source.
    #[must_use]
    pub fn new(label: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            label: label.into(),
            bytes: Arc::new(FontBytes(bytes.into_boxed_slice())),
        }
    }

    /// Returns the immutable font bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_ref().as_ref()
    }

    /// Returns the number of owners of the source allocation for diagnostics.
    #[cfg(feature = "diagnostic-tools")]
    #[must_use]
    pub fn diagnostic_allocation_strong_count(&self) -> usize {
        Arc::strong_count(&self.bytes)
    }

    /// Reads a caller-selected font file.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::Read`] when the file cannot be read.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| CatalogError::Read {
            path: path.to_owned(),
            source,
        })?;
        Ok(Self::new(path.display().to_string(), bytes))
    }
}

/// Memory retained by the active font catalog and its backing database.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CatalogMemoryMetrics {
    /// Bytes retained for active sources, including database copies when present.
    pub retained_source_bytes: usize,
    /// Number of caller-selected active sources.
    pub active_source_count: usize,
    /// Number of successful catalog builds committed by this instance.
    pub catalog_builds: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceOwnership {
    Copied,
    #[cfg(feature = "diagnostic-tools")]
    Shared,
}

/// Error returned while building the isolated catalog.
#[derive(Debug)]
pub enum CatalogError {
    /// A caller-provided file could not be read.
    Read {
        /// Failed path.
        path: std::path::PathBuf,
        /// Operating-system error.
        source: std::io::Error,
    },
    /// The source contains no parseable font face.
    InvalidFont {
        /// Diagnostic source label.
        label: String,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read font {}: {source}",
                    path.display()
                )
            }
            Self::InvalidFont { label } => write!(formatter, "invalid font source {label}"),
        }
    }
}

impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::InvalidFont { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FontRecord {
    pub(crate) id: fontdb::ID,
    pub(crate) family: String,
    aliases: Vec<String>,
    source_index: usize,
    face_index: u32,
    pub(crate) is_color: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FaceMetrics {
    pub(crate) cell_width: f32,
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
    pub(crate) line_gap: f32,
}

impl FontRecord {
    fn matches_family(&self, requested: &str) -> bool {
        self.aliases
            .iter()
            .any(|family| family.eq_ignore_ascii_case(requested))
    }
}

/// An isolated font catalog that never loads host fonts.
pub struct FontCatalog {
    locale: String,
    sources: Vec<FontSource>,
    records: Vec<FontRecord>,
    font_system: FontSystem,
    incarnation: u64,
    generation: u64,
    fingerprint: [u8; 32],
    memory_metrics: CatalogMemoryMetrics,
    source_ownership: SourceOwnership,
}

struct CatalogCandidate {
    sources: Vec<FontSource>,
    records: Vec<FontRecord>,
    font_system: FontSystem,
    generation: u64,
    fingerprint: [u8; 32],
    memory_metrics: CatalogMemoryMetrics,
}

impl fmt::Debug for FontCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FontCatalog")
            .field("locale", &self.locale)
            .field("sources", &self.sources)
            .field("records", &self.records)
            .field("generation", &self.generation)
            .field("incarnation", &self.incarnation)
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl FontCatalog {
    /// Creates an empty catalog backed by a fresh `fontdb::Database`.
    #[must_use]
    pub fn new(locale: impl Into<String>) -> Self {
        let locale = locale.into();
        let sources = Vec::new();
        let fingerprint = content_fingerprint(&locale, &sources);
        Self {
            font_system: FontSystem::new_with_locale_and_db(
                locale.clone(),
                fontdb::Database::new(),
            ),
            locale,
            fingerprint,
            sources,
            records: Vec::new(),
            incarnation: next_catalog_incarnation(),
            generation: 0,
            memory_metrics: CatalogMemoryMetrics::default(),
            source_ownership: SourceOwnership::Copied,
        }
    }

    /// Creates a catalog from caller-provided sources.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidFont`] when any source has no parseable face.
    pub fn from_sources<I>(locale: impl Into<String>, sources: I) -> Result<Self, CatalogError>
    where
        I: IntoIterator<Item = FontSource>,
    {
        Self::from_sources_with_ownership(locale, sources, SourceOwnership::Copied)
    }

    /// Creates a shared-allocation catalog for non-production memory diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidFont`] when any source has no parseable face.
    #[cfg(feature = "diagnostic-tools")]
    pub fn from_sources_shared_for_diagnostics<I>(
        locale: impl Into<String>,
        sources: I,
    ) -> Result<Self, CatalogError>
    where
        I: IntoIterator<Item = FontSource>,
    {
        Self::from_sources_with_ownership(locale, sources, SourceOwnership::Shared)
    }

    /// Creates a copied-allocation catalog for non-production memory diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidFont`] when any source has no parseable face.
    #[cfg(feature = "diagnostic-tools")]
    pub fn from_sources_copied_for_diagnostics<I>(
        locale: impl Into<String>,
        sources: I,
    ) -> Result<Self, CatalogError>
    where
        I: IntoIterator<Item = FontSource>,
    {
        Self::from_sources_with_ownership(locale, sources, SourceOwnership::Copied)
    }

    /// Adds a caller-provided source, rebuilds shaping state, and advances generation.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidFont`] without changing the catalog when the
    /// prospective source set contains an invalid font.
    pub fn load_source(&mut self, source: FontSource) -> Result<u64, CatalogError> {
        self.load_sources([source])
    }

    /// Adds a batch of caller-provided sources in one transactional rebuild.
    /// An empty batch is a no-op and returns the current generation.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidFont`] without changing the catalog when any
    /// source in the prospective active set has no parseable face.
    pub fn load_sources<I>(&mut self, sources: I) -> Result<u64, CatalogError>
    where
        I: IntoIterator<Item = FontSource>,
    {
        let sources: Vec<_> = sources.into_iter().collect();
        if sources.is_empty() {
            return Ok(self.generation);
        }
        let mut candidate_sources = self.sources.clone();
        candidate_sources.extend(sources);
        let candidate = Self::build_candidate(
            &self.locale,
            candidate_sources,
            self.source_ownership,
            self.generation.wrapping_add(1),
            self.memory_metrics.catalog_builds.wrapping_add(1),
        )?;
        self.commit(candidate);
        Ok(self.generation)
    }

    /// Loads a caller-selected font file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or contains no parseable face.
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<u64, CatalogError> {
        self.load_source(FontSource::from_file(path)?)
    }

    /// Current generation used to invalidate shape and raster caches.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Current memory and committed-build counters for this catalog.
    #[must_use]
    pub const fn memory_metrics(&self) -> CatalogMemoryMetrics {
        self.memory_metrics
    }

    /// Process-unique identity of this catalog instance.
    #[must_use]
    pub const fn incarnation(&self) -> u64 {
        self.incarnation
    }

    pub(crate) const fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// Returns the order-sensitive content fingerprint for diagnostics.
    #[cfg(feature = "diagnostic-tools")]
    #[must_use]
    pub const fn diagnostic_ordered_fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// Returns a fingerprint of every active face record for diagnostics.
    #[cfg(feature = "diagnostic-tools")]
    #[must_use]
    pub fn diagnostic_face_records_fingerprint(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(self.records.len().to_le_bytes());
        for record in &self.records {
            let id = record.id.to_string();
            digest.update(id.len().to_le_bytes());
            digest.update(id.as_bytes());
            digest.update(record.family.len().to_le_bytes());
            digest.update(record.family.as_bytes());
            digest.update(record.aliases.len().to_le_bytes());
            for alias in &record.aliases {
                digest.update(alias.len().to_le_bytes());
                digest.update(alias.as_bytes());
            }
            digest.update(record.source_index.to_le_bytes());
            digest.update(record.face_index.to_le_bytes());
            digest.update([u8::from(record.is_color)]);
        }
        digest.finalize().into()
    }

    pub(crate) const fn font_id(&self, raw: fontdb::ID) -> FontId {
        FontId {
            raw: Some(raw),
            catalog_incarnation: self.incarnation,
            catalog_generation: self.generation,
        }
    }

    pub(crate) const fn missing_font_id(&self) -> FontId {
        FontId {
            raw: None,
            catalog_incarnation: self.incarnation,
            catalog_generation: self.generation,
        }
    }

    /// Number of faces available to this isolated catalog.
    #[must_use]
    pub fn face_count(&self) -> usize {
        self.records.len()
    }

    fn from_sources_with_ownership<I>(
        locale: impl Into<String>,
        sources: I,
        source_ownership: SourceOwnership,
    ) -> Result<Self, CatalogError>
    where
        I: IntoIterator<Item = FontSource>,
    {
        let locale = locale.into();
        let candidate = Self::build_candidate(
            &locale,
            sources.into_iter().collect(),
            source_ownership,
            1,
            1,
        )?;
        Ok(Self {
            locale,
            sources: candidate.sources,
            records: candidate.records,
            font_system: candidate.font_system,
            incarnation: next_catalog_incarnation(),
            generation: candidate.generation,
            fingerprint: candidate.fingerprint,
            memory_metrics: candidate.memory_metrics,
            source_ownership,
        })
    }

    fn build_candidate(
        locale: &str,
        sources: Vec<FontSource>,
        source_ownership: SourceOwnership,
        generation: u64,
        catalog_builds: u64,
    ) -> Result<CatalogCandidate, CatalogError> {
        let fingerprint = content_fingerprint(locale, &sources);
        let (font_system, records) = Self::build(locale, &sources, source_ownership)?;
        let retained_multiplier = match source_ownership {
            SourceOwnership::Copied => 2,
            #[cfg(feature = "diagnostic-tools")]
            SourceOwnership::Shared => 1,
        };
        let retained_source_bytes = sources.iter().fold(0usize, |retained, source| {
            retained.saturating_add(source.bytes().len().saturating_mul(retained_multiplier))
        });
        Ok(CatalogCandidate {
            memory_metrics: CatalogMemoryMetrics {
                retained_source_bytes,
                active_source_count: sources.len(),
                catalog_builds,
            },
            sources,
            records,
            font_system,
            generation,
            fingerprint,
        })
    }

    fn commit(&mut self, candidate: CatalogCandidate) {
        self.sources = candidate.sources;
        self.records = candidate.records;
        self.font_system = candidate.font_system;
        self.generation = candidate.generation;
        self.fingerprint = candidate.fingerprint;
        self.memory_metrics = candidate.memory_metrics;
    }

    fn build(
        locale: &str,
        sources: &[FontSource],
        source_ownership: SourceOwnership,
    ) -> Result<(FontSystem, Vec<FontRecord>), CatalogError> {
        let mut db = fontdb::Database::new();
        let mut records = Vec::new();

        for (source_index, source) in sources.iter().enumerate() {
            let before: std::collections::HashSet<_> = db.faces().map(|face| face.id).collect();
            match source_ownership {
                SourceOwnership::Copied => db.load_font_data(source.bytes().to_vec()),
                #[cfg(feature = "diagnostic-tools")]
                SourceOwnership::Shared => {
                    let bytes: Arc<dyn AsRef<[u8]> + Send + Sync> = source.bytes.clone();
                    db.load_font_source(fontdb::Source::Binary(bytes));
                }
            }
            let new_faces: Vec<_> = db
                .faces()
                .filter(|face| !before.contains(&face.id))
                .map(|face| {
                    let aliases: Vec<String> = face
                        .families
                        .iter()
                        .map(|(name, _language)| name.clone())
                        .collect();
                    let family = aliases
                        .first()
                        .cloned()
                        .unwrap_or_else(|| face.post_script_name.clone());
                    FontRecord {
                        id: face.id,
                        family,
                        aliases,
                        source_index,
                        face_index: face.index,
                        is_color: has_color_tables(source.bytes(), face.index),
                    }
                })
                .collect();
            if new_faces.is_empty() {
                return Err(CatalogError::InvalidFont {
                    label: source.label.clone(),
                });
            }
            records.extend(new_faces);
        }

        Ok((
            FontSystem::new_with_locale_and_db(locale.to_owned(), db),
            records,
        ))
    }

    pub(crate) fn record_for_family(
        &self,
        family: &str,
        weight: u16,
        style: FontStyle,
        stretch: FontStretch,
    ) -> Option<&FontRecord> {
        let canonical_family = self
            .records
            .iter()
            .find(|record| record.matches_family(family))?
            .aliases
            .iter()
            .find(|alias| alias.eq_ignore_ascii_case(family))?;
        let families = [fontdb::Family::Name(canonical_family)];
        let id = self.font_system.db().query(&fontdb::Query {
            families: &families,
            weight: fontdb::Weight(weight),
            style: match style {
                FontStyle::Normal => fontdb::Style::Normal,
                FontStyle::Italic => fontdb::Style::Italic,
                FontStyle::Oblique => fontdb::Style::Oblique,
            },
            stretch: match stretch {
                FontStretch::Condensed => fontdb::Stretch::Condensed,
                FontStretch::Normal => fontdb::Stretch::Normal,
                FontStretch::Expanded => fontdb::Stretch::Expanded,
            },
        })?;
        self.records.iter().find(|record| record.id == id)
    }

    pub(crate) fn supports_cluster(&self, record: &FontRecord, cluster: &str) -> bool {
        let source = &self.sources[record.source_index];
        let Ok(face) = ttf_parser::Face::parse(source.bytes(), record.face_index) else {
            return false;
        };
        let wants_emoji = cluster.contains('\u{fe0f}');
        let wants_text = cluster.contains('\u{fe0e}');
        if (wants_emoji && !record.is_color) || (wants_text && record.is_color) {
            return false;
        }
        let covered = cluster.chars().all(|character| {
            is_default_ignorable(character) || face.glyph_index(character).is_some()
        });
        if !covered {
            return false;
        }
        if wants_emoji {
            let mut characters = cluster.chars();
            let Some(base) = characters.find(|character| !is_variation_selector(*character)) else {
                return false;
            };
            return face.glyph_variation_index(base, '\u{fe0f}').is_some();
        }
        true
    }

    /// Borrows the isolated shaping system for renderer integrations that must
    /// share this catalog's exact font database.
    ///
    /// Callers must not add system fonts or independently shape terminal text.
    /// Terminal shaping remains the responsibility of [`crate::TerminalShaper`].
    pub fn font_system_mut(&mut self) -> &mut FontSystem {
        &mut self.font_system
    }

    /// Reports whether every database face for a source uses its exact allocation.
    #[cfg(feature = "diagnostic-tools")]
    #[must_use]
    pub fn diagnostic_fontdb_shares_source_allocation(&self, source_index: usize) -> bool {
        let Some(source) = self.sources.get(source_index) else {
            return false;
        };
        let expected: Arc<dyn AsRef<[u8]> + Send + Sync> = source.bytes.clone();
        let mut found = false;
        for record in self
            .records
            .iter()
            .filter(|record| record.source_index == source_index)
        {
            found = true;
            let Some(face) = self.font_system.db().face(record.id) else {
                return false;
            };
            let fontdb::Source::Binary(actual) = &face.source else {
                return false;
            };
            if !Arc::ptr_eq(&expected, actual) {
                return false;
            }
        }
        found
    }

    pub(crate) fn owns(&self, id: FontId) -> bool {
        id.catalog_incarnation == self.incarnation
            && id.catalog_generation == self.generation
            && id
                .raw
                .is_some_and(|raw| self.records.iter().any(|record| record.id == raw))
    }

    pub(crate) fn first_record(&self) -> Option<&FontRecord> {
        self.records.first()
    }

    pub(crate) fn face_metrics(&self, record: &FontRecord, font_size: f32) -> Option<FaceMetrics> {
        let source = &self.sources[record.source_index];
        let face = ttf_parser::Face::parse(source.bytes(), record.face_index).ok()?;
        let units_per_em = f32::from(face.units_per_em());
        let scale = font_size / units_per_em;
        let cell_glyph = ['M', '0', ' ']
            .into_iter()
            .find_map(|character| face.glyph_index(character));
        let advance = cell_glyph
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map_or(font_size * 0.6, |advance| f32::from(advance) * scale);
        let metrics = FaceMetrics {
            cell_width: advance,
            ascent: f32::from(face.ascender()) * scale,
            descent: -f32::from(face.descender()) * scale,
            line_gap: f32::from(face.line_gap()) * scale,
        };
        let metric_limit = font_size * 10.0;
        (metrics.cell_width.is_finite()
            && metrics.cell_width > 0.0
            && metrics.cell_width <= metric_limit
            && metrics.ascent.is_finite()
            && metrics.ascent > 0.0
            && metrics.ascent <= metric_limit
            && metrics.descent.is_finite()
            && metrics.descent >= 0.0
            && metrics.descent <= metric_limit
            && metrics.line_gap.is_finite()
            && metrics.line_gap.abs() <= metric_limit)
            .then_some(metrics)
    }
}

fn next_catalog_incarnation() -> u64 {
    NEXT_CATALOG_INCARNATION.fetch_add(1, Ordering::Relaxed)
}

fn content_fingerprint(locale: &str, sources: &[FontSource]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(locale.len().to_le_bytes());
    digest.update(locale.as_bytes());
    digest.update(sources.len().to_le_bytes());
    for source in sources {
        digest.update(source.bytes().len().to_le_bytes());
        digest.update(source.bytes());
    }
    digest.finalize().into()
}

fn is_variation_selector(character: char) -> bool {
    matches!(character, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}

pub(crate) fn is_default_ignorable(character: char) -> bool {
    is_variation_selector(character)
        || matches!(
            character,
            '\u{00ad}'
                | '\u{034f}'
                | '\u{061c}'
                | '\u{115f}'..='\u{1160}'
                | '\u{17b4}'..='\u{17b5}'
                | '\u{180b}'..='\u{180f}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{3164}'
                | '\u{feff}'
                | '\u{ffa0}'
                | '\u{1bca0}'..='\u{1bca3}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{e0000}'..='\u{e0fff}'
        )
}

fn has_color_tables(bytes: &[u8], face_index: u32) -> bool {
    let Ok(face) = ttf_parser::Face::parse(bytes, face_index) else {
        return false;
    };
    face.tables().colr.is_some()
        || face.tables().sbix.is_some()
        || face.tables().svg.is_some()
        || face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(b"CBDT"))
            .is_some()
}
