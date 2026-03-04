//! Metadata cache for FUSE operations.
//!
//! Provides LRU-based caching for file attributes and negative lookups
//! to reduce round-trips to the metadata service.

use crate::attr::FileAttr;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

/// Configuration for the metadata cache.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum number of entries in the cache.
    pub capacity: usize,
    /// Time-to-live for positive cache entries, in seconds.
    pub ttl_secs: u64,
    /// Time-to-live for negative cache entries, in seconds.
    pub negative_ttl_secs: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        CacheConfig {
            capacity: 10_000,
            ttl_secs: 30,
            negative_ttl_secs: 5,
        }
    }
}

/// A cache entry with associated metadata for TTL expiration.
pub struct CacheEntry<V> {
    /// The cached value.
    pub value: V,
    /// Timestamp when this entry was inserted.
    pub inserted_at: Instant,
    /// Time-to-live for this entry, in seconds.
    pub ttl_secs: u64,
}

impl<V> CacheEntry<V> {
    /// Returns `true` if this entry has exceeded its TTL.
    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > Duration::from_secs(self.ttl_secs)
    }
}

/// LRU cache for file metadata with TTL and negative caching support.
pub struct MetadataCache {
    attrs: LruCache<u64, CacheEntry<FileAttr>>,
    negative_cache: LruCache<(u64, String), Instant>,
    config: CacheConfig,
    stats: CacheStats,
}

/// Statistics for cache operations.
#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of entries evicted due to capacity.
    pub evictions: u64,
    /// Current number of entries in the cache.
    pub size: usize,
}

impl MetadataCache {
    /// Creates a new metadata cache with the given configuration.
    pub fn new(config: CacheConfig) -> Self {
        let capacity =
            NonZeroUsize::new(config.capacity).unwrap_or(NonZeroUsize::new(10_000).unwrap());
        MetadataCache {
            attrs: LruCache::new(capacity),
            negative_cache: LruCache::new(capacity),
            config,
            stats: CacheStats::default(),
        }
    }

    /// Retrieves a file attribute from the cache if present and not expired.
    ///
    /// Updates hit/miss statistics. Returns `None` if the entry is missing
    /// or has exceeded its TTL.
    pub fn get_attr(&mut self, ino: u64) -> Option<FileAttr> {
        if let Some(entry) = self.attrs.get(&ino) {
            if entry.is_expired() {
                self.attrs.pop(&ino);
                self.stats.misses += 1;
                None
            } else {
                self.stats.hits += 1;
                Some(entry.value.clone())
            }
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// Inserts a file attribute into the cache with the configured TTL.
    ///
    /// If the cache is at capacity, the least-recently-used entry is evicted.
    pub fn insert_attr(&mut self, ino: u64, attr: FileAttr) {
        let prev_len = self.attrs.len();
        self.attrs.push(
            ino,
            CacheEntry {
                value: attr,
                inserted_at: Instant::now(),
                ttl_secs: self.config.ttl_secs,
            },
        );
        if self.attrs.len() <= prev_len {
            self.stats.evictions += 1;
        }
    }

    /// Removes the entry for the given inode from the cache.
    pub fn invalidate(&mut self, ino: u64) {
        self.attrs.pop(&ino);
    }

    /// Clears all entries from the attribute cache.
    ///
    /// Note: This is a conservative implementation that clears the entire
    /// cache rather than filtering by parent inode.
    pub fn invalidate_children(&mut self, _parent_ino: u64) {
        self.attrs.clear();
    }

    /// Records a negative cache entry for a nonexistent name in a directory.
    pub fn insert_negative(&mut self, parent_ino: u64, name: &str) {
        self.negative_cache
            .push((parent_ino, name.to_string()), Instant::now());
    }

    /// Returns `true` if a negative cache entry exists and has not expired.
    pub fn is_negative(&mut self, parent_ino: u64, name: &str) -> bool {
        if let Some(instant) = self.negative_cache.get(&(parent_ino, name.to_string())) {
            if instant.elapsed() > Duration::from_secs(self.config.negative_ttl_secs) {
                self.negative_cache.pop(&(parent_ino, name.to_string()));
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    /// Returns a snapshot of current cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.stats.hits,
            misses: self.stats.misses,
            evictions: self.stats.evictions,
            size: self.attrs.len(),
        }
    }

    /// Clears both the attribute cache and negative cache.
    pub fn clear(&mut self) {
        self.attrs.clear();
        self.negative_cache.clear();
    }

    /// Returns the number of entries in the attribute cache.
    pub fn len(&self) -> usize {
        self.attrs.len()
    }

    /// Returns `true` if the attribute cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }
}

impl Default for MetadataCache {
    fn default() -> Self {
        Self::new(CacheConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get_within_ttl() {
        let mut cache = MetadataCache::new(CacheConfig {
            capacity: 100,
            ttl_secs: 60,
            negative_ttl_secs: 5,
        });
        let attr = FileAttr::new_file(1, 100, 0o644, 0, 0);
        cache.insert_attr(1, attr.clone());

        let result = cache.get_attr(1);
        assert!(result.is_some());
        assert_eq!(result.unwrap().ino, 1);
    }

    #[test]
    fn test_get_after_ttl_expiry() {
        let mut cache = MetadataCache::new(CacheConfig {
            capacity: 100,
            ttl_secs: 0,
            negative_ttl_secs: 0,
        });
        let attr = FileAttr::new_file(1, 100, 0o644, 0, 0);
        cache.insert_attr(1, attr);

        std::thread::sleep(Duration::from_millis(10));

        let result = cache.get_attr(1);
        assert!(result.is_none());
    }

    #[test]
    fn test_negative_cache_hit() {
        let mut cache = MetadataCache::new(CacheConfig::default());
        cache.insert_negative(1, "nonexistent");

        assert!(cache.is_negative(1, "nonexistent"));
    }

    #[test]
    fn test_negative_cache_miss() {
        let mut cache = MetadataCache::new(CacheConfig::default());
        assert!(!cache.is_negative(1, "nonexistent"));
    }

    #[test]
    fn test_invalidate_removes_entry() {
        let mut cache = MetadataCache::new(CacheConfig::default());
        let attr = FileAttr::new_file(1, 100, 0o644, 0, 0);
        cache.insert_attr(1, attr);

        cache.invalidate(1);

        assert!(cache.get_attr(1).is_none());
    }

    #[test]
    fn test_clear_empties_cache() {
        let mut cache = MetadataCache::new(CacheConfig::default());
        cache.insert_attr(1, FileAttr::new_file(1, 100, 0o644, 0, 0));
        cache.insert_attr(2, FileAttr::new_file(2, 200, 0o644, 0, 0));

        cache.clear();

        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_stats_track_hits_and_misses() {
        let mut cache = MetadataCache::new(CacheConfig::default());

        cache.get_attr(1);
        assert_eq!(cache.stats().misses, 1);

        cache.insert_attr(2, FileAttr::new_file(2, 100, 0o644, 0, 0));
        cache.get_attr(2);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn test_capacity_eviction() {
        let config = CacheConfig {
            capacity: 2,
            ttl_secs: 60,
            negative_ttl_secs: 5,
        };
        let mut cache = MetadataCache::new(config);

        cache.insert_attr(1, FileAttr::new_file(1, 100, 0o644, 0, 0));
        cache.insert_attr(2, FileAttr::new_file(2, 200, 0o644, 0, 0));
        cache.insert_attr(3, FileAttr::new_file(3, 300, 0o644, 0, 0));

        assert!(cache.len() <= 2);
    }

    #[test]
    fn test_default_config_sensible_values() {
        let config = CacheConfig::default();
        assert_eq!(config.capacity, 10_000);
        assert_eq!(config.ttl_secs, 30);
        assert_eq!(config.negative_ttl_secs, 5);
    }
}
