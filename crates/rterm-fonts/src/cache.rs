//! Byte-bounded least-recently-used caches and shared instrumentation.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// Cache counters and conservative retained-payload accounting.
///
/// Byte totals charge owned allocation capacities and estimated entry/index
/// metadata. They intentionally exclude allocator bucket slack and temporary
/// shaping/rasterization scratch memory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheMetrics {
    /// Successful lookups.
    pub hits: u64,
    /// Lookups that required work.
    pub misses: u64,
    /// Entries removed to honor the byte budget.
    pub evictions: u64,
    /// Values not retained because one entry exceeded the budget.
    pub oversize_bypasses: u64,
    /// Values successfully retained.
    pub insertions: u64,
    /// Explicit scope/configuration invalidations.
    pub invalidations: u64,
    /// Entries released by explicit invalidation.
    pub invalidated_entries: u64,
    /// Visible glyphs for which the scaler produced no valid image.
    pub raster_failures: u64,
    /// Configured maximum accounted retained bytes.
    pub budget_bytes: usize,
    /// Bytes retained now.
    pub current_bytes: usize,
    /// Highest retained byte count observed.
    pub peak_bytes: usize,
    /// Entries retained now.
    pub current_entries: usize,
    /// Highest retained entry count observed.
    pub peak_entries: usize,
}

#[derive(Debug)]
struct CacheEntry<V> {
    value: V,
    bytes: usize,
    last_used: u64,
}

/// A deterministic LRU with O(log n) hits and eviction.
#[derive(Debug)]
pub(crate) struct BoundedCache<K, V> {
    entries: HashMap<K, CacheEntry<V>>,
    by_age: BTreeMap<u64, K>,
    next_age: u64,
    metrics: CacheMetrics,
}

impl<K, V> BoundedCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    pub(crate) fn new(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            by_age: BTreeMap::new(),
            next_age: 1,
            metrics: CacheMetrics {
                budget_bytes,
                ..CacheMetrics::default()
            },
        }
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<V> {
        let age = self.take_age();
        let Some(entry) = self.entries.get_mut(key) else {
            self.metrics.misses = self.metrics.misses.saturating_add(1);
            return None;
        };
        self.metrics.hits = self.metrics.hits.saturating_add(1);
        self.by_age.remove(&entry.last_used);
        entry.last_used = age;
        self.by_age.insert(age, key.clone());
        Some(entry.value.clone())
    }

    pub(crate) fn insert(&mut self, key: K, value: V, bytes: usize) -> bool {
        if !self.can_retain(bytes) {
            self.record_oversize_bypass();
            return false;
        }

        if let Some(old) = self.entries.remove(&key) {
            self.by_age.remove(&old.last_used);
            self.metrics.current_bytes = self.metrics.current_bytes.saturating_sub(old.bytes);
        }

        while self
            .metrics
            .current_bytes
            .checked_add(bytes)
            .is_none_or(|total| total > self.metrics.budget_bytes)
        {
            let Some((&oldest_age, _)) = self.by_age.first_key_value() else {
                self.metrics.oversize_bypasses = self.metrics.oversize_bypasses.saturating_add(1);
                return false;
            };
            let Some(oldest_key) = self.by_age.remove(&oldest_age) else {
                continue;
            };
            if let Some(oldest) = self.entries.remove(&oldest_key) {
                self.metrics.current_bytes =
                    self.metrics.current_bytes.saturating_sub(oldest.bytes);
                self.metrics.evictions = self.metrics.evictions.saturating_add(1);
            }
        }

        let age = self.take_age();
        self.metrics.current_bytes += bytes;
        self.entries.insert(
            key.clone(),
            CacheEntry {
                value,
                bytes,
                last_used: age,
            },
        );
        self.by_age.insert(age, key);
        self.metrics.insertions = self.metrics.insertions.saturating_add(1);
        self.refresh_live_metrics();
        true
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.by_age.clear();
        self.metrics.current_bytes = 0;
        self.metrics.current_entries = 0;
    }

    pub(crate) fn invalidate(&mut self) {
        self.metrics.invalidations = self.metrics.invalidations.saturating_add(1);
        self.metrics.invalidated_entries = self
            .metrics
            .invalidated_entries
            .saturating_add(u64::try_from(self.entries.len()).unwrap_or(u64::MAX));
        self.clear();
    }

    pub(crate) fn set_budget(&mut self, budget_bytes: usize) {
        self.metrics.budget_bytes = budget_bytes;
        while self.metrics.current_bytes > budget_bytes {
            let Some((&oldest_age, _)) = self.by_age.first_key_value() else {
                break;
            };
            let Some(oldest_key) = self.by_age.remove(&oldest_age) else {
                continue;
            };
            if let Some(oldest) = self.entries.remove(&oldest_key) {
                self.metrics.current_bytes =
                    self.metrics.current_bytes.saturating_sub(oldest.bytes);
                self.metrics.evictions = self.metrics.evictions.saturating_add(1);
            }
        }
        self.refresh_live_metrics();
    }

    pub(crate) const fn metrics(&self) -> CacheMetrics {
        self.metrics
    }

    pub(crate) const fn can_retain(&self, bytes: usize) -> bool {
        bytes != usize::MAX && bytes <= self.metrics.budget_bytes
    }

    pub(crate) fn record_oversize_bypass(&mut self) {
        self.metrics.oversize_bypasses = self.metrics.oversize_bypasses.saturating_add(1);
    }

    pub(crate) fn record_raster_failure(&mut self) {
        self.metrics.raster_failures = self.metrics.raster_failures.saturating_add(1);
    }

    fn refresh_live_metrics(&mut self) {
        self.metrics.current_entries = self.entries.len();
        self.metrics.peak_bytes = self.metrics.peak_bytes.max(self.metrics.current_bytes);
        self.metrics.peak_entries = self.metrics.peak_entries.max(self.metrics.current_entries);
    }

    fn take_age(&mut self) -> u64 {
        if self.next_age == u64::MAX {
            self.rebase_ages();
        }
        let age = self.next_age;
        self.next_age = self.next_age.saturating_add(1);
        age
    }

    fn rebase_ages(&mut self) {
        let keys: Vec<_> = self.by_age.values().cloned().collect();
        self.by_age.clear();
        for (index, key) in keys.into_iter().enumerate() {
            let age = u64::try_from(index).unwrap_or(u64::MAX - 1) + 1;
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.last_used = age;
            }
            self.by_age.insert(age, key);
        }
        self.next_age = u64::try_from(self.by_age.len())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1);
    }
}
