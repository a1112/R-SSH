//! Deterministic, caller-owned font catalog.

use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use cosmic_text::{FontSystem, fontdb};

/// Font identifier scoped to one [`FontCatalog::generation`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FontId(Option<fontdb::ID>);

impl FontId {
    /// Identifier used when no catalog face is available.
    pub const MISSING: Self = Self(None);

    pub(crate) const fn from_cosmic(id: fontdb::ID) -> Self {
        Self(Some(id))
    }
}

/// Caller-provided font bytes and a diagnostic label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontSource {
    /// Human-readable source name or path.
    pub label: String,
    /// Complete font bytes.
    bytes: Arc<[u8]>,
}

impl FontSource {
    /// Creates an in-memory source.
    #[must_use]
    pub fn new(label: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            label: label.into(),
            bytes: bytes.into(),
        }
    }

    /// Returns the immutable font bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
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
    generation: u64,
}

impl fmt::Debug for FontCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FontCatalog")
            .field("locale", &self.locale)
            .field("sources", &self.sources)
            .field("records", &self.records)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl FontCatalog {
    /// Creates an empty catalog backed by a fresh `fontdb::Database`.
    #[must_use]
    pub fn new(locale: impl Into<String>) -> Self {
        let locale = locale.into();
        Self {
            font_system: FontSystem::new_with_locale_and_db(
                locale.clone(),
                fontdb::Database::new(),
            ),
            locale,
            sources: Vec::new(),
            records: Vec::new(),
            generation: 0,
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
        let locale = locale.into();
        let sources: Vec<_> = sources.into_iter().collect();
        let (font_system, records) = Self::build(&locale, &sources)?;
        Ok(Self {
            locale,
            sources,
            records,
            font_system,
            generation: 1,
        })
    }

    /// Adds a caller-provided source, rebuilds shaping state, and advances generation.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidFont`] without changing the catalog when the
    /// prospective source set contains an invalid font.
    pub fn load_source(&mut self, source: FontSource) -> Result<u64, CatalogError> {
        let mut sources = self.sources.clone();
        sources.push(source);
        let (font_system, records) = Self::build(&self.locale, &sources)?;
        self.sources = sources;
        self.records = records;
        self.font_system = font_system;
        self.generation = self.generation.wrapping_add(1);
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

    /// Number of faces available to this isolated catalog.
    #[must_use]
    pub fn face_count(&self) -> usize {
        self.records.len()
    }

    fn build(
        locale: &str,
        sources: &[FontSource],
    ) -> Result<(FontSystem, Vec<FontRecord>), CatalogError> {
        let mut db = fontdb::Database::new();
        let mut records = Vec::new();

        for (source_index, source) in sources.iter().enumerate() {
            let before: std::collections::HashSet<_> = db.faces().map(|face| face.id).collect();
            db.load_font_data(source.bytes.to_vec());
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

    pub(crate) fn record_for_family(&self, family: &str) -> Option<&FontRecord> {
        self.records
            .iter()
            .find(|record| record.matches_family(family))
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
            is_variation_selector(character) || face.glyph_index(character).is_some()
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

    pub(crate) fn font_system_mut(&mut self) -> &mut FontSystem {
        &mut self.font_system
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
        Some(FaceMetrics {
            cell_width: advance,
            ascent: f32::from(face.ascender()) * scale,
            descent: -f32::from(face.descender()) * scale,
            line_gap: f32::from(face.line_gap()) * scale,
        })
    }
}

fn is_variation_selector(character: char) -> bool {
    matches!(character, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
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
