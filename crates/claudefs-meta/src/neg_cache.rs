//! Negative cache for "file not found" metadata lookups.
//!
//! Caches "does not exist" results to avoid repeated lookups for missing
//! files. This is critical for build systems and package managers that
//! probe many paths before finding the right one.
//! See docs/metadata.md — Negative caching.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::types::*;

/// Configuration for the negative cache.
#[derive(Clone, Debug)]
pub struct NegCacheConfig {
    /// How long a negative entry stays valid.
    pub ttl: Duration,
    /// Maximum number of negative cache entries.
    pub max_entries: usize,
    /// Whether the negative cache is enabled.
    pub enabled: bool,
}

impl Default for NegCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(3),
            max_entries: 8192,
            enabled: true,
        }
    }
}

/// A negative cache entry recording that a name does not exist.
#[derive(Clone, Debug)]
pub struct NegEntry {
    /// Parent directory inode.
    pub parent: InodeId,
    /// Name that was looked up.
    pub name: String,
    /// When the negative result was recorded.
    pub recorded_at: Instant,
}

impl NegEntry {
    /// Checks if this entry has expired.
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.recorded_at.elapsed() > ttl
    }
}

/// Key for a negative cache lookup.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NegKey {
    parent: InodeId,
    name: String,
}

/// Negative cache statistics.
#[derive(Clone, Debug, Default)]
pub struct NegCacheStats {
    /// Number of negative cache hits (avoided a lookup).
    pub hits: u64,
    /// Number of negative cache misses (had to do lookup).
    pub misses: u64,
    /// Number of entries inserted.
    pub inserts: u64,
    /// Number of entries invalidated.
    pub invalidations: u64,
    /// Number of entries expired.
    pub expirations: u64,
}

/// Caches negative (not found) lookup results.
pub struct NegativeCache {
    config: NegCacheConfig,
    entries: HashMap<NegKey, NegEntry>,
    stats: NegCacheStats,
}

impl NegativeCache {
    /// Creates a new negative cache.
    pub fn new(config: NegCacheConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
            stats: NegCacheStats::default(),
        }
    }

    /// Records a negative lookup result.
    pub fn insert(&mut self, parent: InodeId, name: String) {
        if !self.config.enabled {
            return;
        }

        // Evict if at capacity
        if self.entries.len() >= self.config.max_entries {
            self.evict_expired();
            if self.entries.len() >= self.config.max_entries {
                self.evict_oldest();
            }
        }

        let key = NegKey {
            parent,
            name: name.clone(),
        };
        let entry = NegEntry {
            parent,
            name,
            recorded_at: Instant::now(),
        };
        self.entries.insert(key, entry);
        self.stats.inserts += 1;
    }

    /// Checks if a name is known to not exist.
    /// Returns true if the name is in the negative cache and not expired.
    pub fn is_negative(&mut self, parent: &InodeId, name: &str) -> bool {
        if !self.config.enabled {
            return false;
        }

        let key = NegKey {
            parent: *parent,
            name: name.to_string(),
        };

        match self.entries.get(&key) {
            Some(entry) => {
                if entry.is_expired(self.config.ttl) {
                    self.entries.remove(&key);
                    self.stats.expirations += 1;
                    self.stats.misses += 1;
                    false
                } else {
                    self.stats.hits += 1;
                    true
                }
            }
            None => {
                self.stats.misses += 1;
                false
            }
        }
    }

    /// Invalidates a specific entry (e.g., when a file is created).
    pub fn invalidate(&mut self, parent: &InodeId, name: &str) {
        let key = NegKey {
            parent: *parent,
            name: name.to_string(),
        };
        if self.entries.remove(&key).is_some() {
            self.stats.invalidations += 1;
        }
    }

    /// Invalidates all entries for a directory (e.g., directory was modified).
    pub fn invalidate_dir(&mut self, parent: &InodeId) {
        let before = self.entries.len();
        self.entries.retain(|key, _| key.parent != *parent);
        let removed = before - self.entries.len();
        self.stats.invalidations += removed as u64;
    }

    /// Returns the number of cached entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the cache statistics.
    pub fn stats(&self) -> &NegCacheStats {
        &self.stats
    }

    /// Returns the cache hit ratio (0.0-1.0).
    pub fn hit_ratio(&self) -> f64 {
        let total = self.stats.hits + self.stats.misses;
        if total == 0 {
            return 0.0;
        }
        self.stats.hits as f64 / total as f64
    }

    /// Removes all expired entries.
    pub fn cleanup_expired(&mut self) -> usize {
        let ttl = self.config.ttl;
        let before = self.entries.len();
        self.entries.retain(|_, entry| !entry.is_expired(ttl));
        let removed = before - self.entries.len();
        self.stats.expirations += removed as u64;
        removed
    }

    /// Clears the entire cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn evict_expired(&mut self) {
        let ttl = self.config.ttl;
        self.entries.retain(|_, entry| !entry.is_expired(ttl));
    }

    fn evict_oldest(&mut self) {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.recorded_at)
            .map(|(key, _)| key.clone());

        if let Some(key) = oldest {
            self.entries.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cache() -> NegativeCache {
        NegativeCache::new(NegCacheConfig::default())
    }

    #[test]
    fn test_insert_and_check() {
        let mut cache = make_cache();
        cache.insert(InodeId::new(1), "missing.txt".to_string());
        assert!(cache.is_negative(&InodeId::new(1), "missing.txt"));
    }

    #[test]
    fn test_miss_for_unknown() {
        let mut cache = make_cache();
        assert!(!cache.is_negative(&InodeId::new(1), "unknown.txt"));
    }

    #[test]
    fn test_invalidate_specific() {
        let mut cache = make_cache();
        cache.insert(InodeId::new(1), "missing.txt".to_string());
        cache.invalidate(&InodeId::new(1), "missing.txt");
        assert!(!cache.is_negative(&InodeId::new(1), "missing.txt"));
    }

    #[test]
    fn test_invalidate_dir() {
        let mut cache = make_cache();
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(1), "b.txt".to_string());
        cache.insert(InodeId::new(2), "c.txt".to_string());

        cache.invalidate_dir(&InodeId::new(1));
        assert!(!cache.is_negative(&InodeId::new(1), "a.txt"));
        assert!(!cache.is_negative(&InodeId::new(1), "b.txt"));
        assert!(cache.is_negative(&InodeId::new(2), "c.txt"));
    }

    #[test]
    fn test_entry_count() {
        let mut cache = make_cache();
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(1), "b.txt".to_string());
        assert_eq!(cache.entry_count(), 2);
    }

    #[test]
    fn test_ttl_expiration() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            ttl: Duration::from_millis(10),
            ..Default::default()
        });
        cache.insert(InodeId::new(1), "temp.txt".to_string());
        std::thread::sleep(Duration::from_millis(15));
        assert!(!cache.is_negative(&InodeId::new(1), "temp.txt"));
    }

    #[test]
    fn test_stats_tracking() {
        let mut cache = make_cache();
        cache.insert(InodeId::new(1), "missing.txt".to_string());
        cache.is_negative(&InodeId::new(1), "missing.txt"); // hit
        cache.is_negative(&InodeId::new(1), "other.txt"); // miss

        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().inserts, 1);
    }

    #[test]
    fn test_hit_ratio() {
        let mut cache = make_cache();
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.is_negative(&InodeId::new(1), "a.txt"); // hit
        cache.is_negative(&InodeId::new(1), "b.txt"); // miss
        assert!((cache.hit_ratio() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_disabled_cache() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            enabled: false,
            ..Default::default()
        });
        cache.insert(InodeId::new(1), "missing.txt".to_string());
        assert!(!cache.is_negative(&InodeId::new(1), "missing.txt"));
    }

    #[test]
    fn test_clear() {
        let mut cache = make_cache();
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(2), "b.txt".to_string());
        cache.clear();
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_max_entries_eviction() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            max_entries: 3,
            ..Default::default()
        });
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(1), "b.txt".to_string());
        cache.insert(InodeId::new(1), "c.txt".to_string());
        cache.insert(InodeId::new(1), "d.txt".to_string());
        assert!(cache.entry_count() <= 3);
    }

    #[test]
    fn test_overwrite_existing() {
        let mut cache = make_cache();
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(1), "a.txt".to_string()); // overwrite
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn test_cleanup_expired() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            ttl: Duration::from_millis(10),
            ..Default::default()
        });
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(1), "b.txt".to_string());
        std::thread::sleep(Duration::from_millis(15));
        let removed = cache.cleanup_expired();
        assert_eq!(removed, 2);
        assert_eq!(cache.entry_count(), 0);
    }
}
