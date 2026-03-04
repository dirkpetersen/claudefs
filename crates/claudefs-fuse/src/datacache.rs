//! Data cache for caching file data in the FUSE client.
//!
//! This module provides an LRU-based cache for file data to improve read performance.
//! The cache enforces limits on both the number of files and total bytes cached,
//! and tracks statistics for monitoring cache effectiveness.

use crate::inode::InodeId;
use lru::LruCache;
use std::num::NonZeroUsize;

/// Configuration for the data cache.
#[derive(Debug, Clone)]
pub struct DataCacheConfig {
    /// Maximum number of files to cache.
    pub max_files: usize,
    /// Maximum total bytes to cache.
    pub max_bytes: u64,
    /// Maximum size of a single file that can be cached.
    pub max_file_size: u64,
}

impl Default for DataCacheConfig {
    fn default() -> Self {
        Self {
            max_files: 256,
            max_bytes: 64 * 1024 * 1024,
            max_file_size: 4 * 1024 * 1024,
        }
    }
}

/// Cached data entry with generation tracking.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedData {
    /// The cached file data.
    pub data: Vec<u8>,
    /// Generation number for cache invalidation.
    pub generation: u64,
}

/// Statistics for the data cache.
#[derive(Debug, Default, Clone)]
pub struct DataCacheStats {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of cache evictions.
    pub evictions: u64,
    /// Total bytes currently cached.
    pub total_bytes: u64,
    /// Number of files currently cached.
    pub files: usize,
}

/// LRU-based data cache for file contents.
pub struct DataCache {
    config: DataCacheConfig,
    cache: LruCache<InodeId, CachedData>,
    total_bytes: u64,
    stats: DataCacheStats,
}

impl DataCache {
    /// Creates a new data cache with the given configuration.
    pub fn new(config: DataCacheConfig) -> Self {
        let cache = LruCache::new(
            NonZeroUsize::new(config.max_files).unwrap_or(NonZeroUsize::new(1).unwrap()),
        );
        Self {
            config,
            cache,
            total_bytes: 0,
            stats: DataCacheStats::default(),
        }
    }

    /// Inserts data into the cache for the given inode.
    ///
    /// Returns `true` if the data was inserted, `false` if it was too large.
    /// Evicts entries as needed to stay within configured limits.
    pub fn insert(&mut self, ino: InodeId, data: Vec<u8>, generation: u64) -> bool {
        let size = data.len() as u64;

        if size > self.config.max_file_size {
            return false;
        }

        if size > self.config.max_bytes {
            return false;
        }

        while self.total_bytes + size > self.config.max_bytes && !self.cache.is_empty() {
            if let Some((_, evicted)) = self.cache.pop_lru() {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.data.len() as u64);
                self.stats.evictions += 1;
            }
        }

        while self.cache.len() >= self.config.max_files && !self.cache.is_empty() {
            if let Some((_, evicted)) = self.cache.pop_lru() {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.data.len() as u64);
                self.stats.evictions += 1;
            }
        }

        if let Some(existing) = self.cache.get(&ino).cloned() {
            self.total_bytes = self.total_bytes.saturating_sub(existing.data.len() as u64);
        }

        self.total_bytes += size;
        self.cache.put(ino, CachedData { data, generation });
        self.stats.total_bytes = self.total_bytes;
        self.stats.files = self.cache.len();

        true
    }

    /// Retrieves cached data for the given inode.
    ///
    /// Updates hit/miss statistics.
    pub fn get(&mut self, ino: InodeId) -> Option<&CachedData> {
        if let Some(data) = self.cache.get(&ino) {
            self.stats.hits += 1;
            Some(data)
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// Removes the cached entry for the given inode.
    pub fn invalidate(&mut self, ino: InodeId) {
        if let Some(data) = self.cache.pop(&ino) {
            self.total_bytes = self.total_bytes.saturating_sub(data.data.len() as u64);
            self.stats.total_bytes = self.total_bytes;
            self.stats.files = self.cache.len();
        }
    }

    /// Invalidates the cached entry if its generation doesn't match.
    pub fn invalidate_if_generation(&mut self, ino: InodeId, generation: u64) {
        if let Some(data) = self.cache.get(&ino) {
            if data.generation != generation {
                self.invalidate(ino);
            }
        }
    }

    /// Returns the cache statistics.
    pub fn stats(&self) -> &DataCacheStats {
        &self.stats
    }

    /// Returns the number of cached files.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the total bytes currently cached.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Clears all entries from the cache.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.total_bytes = 0;
        self.stats = DataCacheStats::default();
    }
}

impl Default for DataCache {
    fn default() -> Self {
        Self::new(DataCacheConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cache() -> DataCache {
        DataCache::new(DataCacheConfig {
            max_files: 3,
            max_bytes: 100,
            max_file_size: 50,
        })
    }

    fn make_cache_default() -> DataCache {
        DataCache::new(DataCacheConfig::default())
    }

    #[test]
    fn test_insert_small_file_succeeds() {
        let mut cache = make_cache();
        let result = cache.insert(1, vec![1, 2, 3], 1);
        assert!(result);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_insert_too_large_file_returns_false() {
        let mut cache = make_cache();
        let result = cache.insert(1, vec![0u8; 51], 1);
        assert!(!result);
    }

    #[test]
    fn test_get_after_insert_returns_data() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2, 3], 1);
        let result = cache.get(1);
        assert!(result.is_some());
        assert_eq!(result.unwrap().data, vec![1, 2, 3]);
    }

    #[test]
    fn test_get_miss_returns_none() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2, 3], 1);
        let result = cache.get(999);
        assert_eq!(result, None);
    }

    #[test]
    fn test_stats_track_hits_and_misses() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2, 3], 1);

        cache.get(1);
        cache.get(1);
        cache.get(999);

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_invalidate_removes_entry() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2, 3], 1);
        cache.invalidate(1);

        assert_eq!(cache.get(1), None);
    }

    #[test]
    fn test_total_bytes_decreases_after_invalidate() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2, 3], 1);
        assert_eq!(cache.total_bytes(), 3);

        cache.invalidate(1);
        assert_eq!(cache.total_bytes(), 0);
    }

    #[test]
    fn test_eviction_when_over_max_files() {
        let mut cache = make_cache();

        cache.insert(1, vec![1], 1);
        cache.insert(2, vec![2], 1);
        cache.insert(3, vec![3], 1);
        assert_eq!(cache.len(), 3);

        cache.insert(4, vec![4], 1);

        assert!(cache.len() <= 3);
        assert!(cache.get(1).is_none() || cache.len() == 3);
    }

    #[test]
    fn test_eviction_when_over_max_bytes() {
        let mut cache = DataCache::new(DataCacheConfig {
            max_files: 10,
            max_bytes: 5,
            max_file_size: 10,
        });

        cache.insert(1, vec![1, 2], 1);
        cache.insert(2, vec![3, 4], 1);
        cache.insert(3, vec![5, 6], 1);

        assert!(cache.total_bytes() <= 5);
    }

    #[test]
    fn test_clear_empties_everything() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2, 3], 1);
        cache.insert(2, vec![4, 5, 6], 1);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert_eq!(cache.total_bytes(), 0);
    }

    #[test]
    fn test_default_config_has_sensible_values() {
        let config = DataCacheConfig::default();
        assert_eq!(config.max_files, 256);
        assert_eq!(config.max_bytes, 64 * 1024 * 1024);
        assert_eq!(config.max_file_size, 4 * 1024 * 1024);
    }

    #[test]
    fn test_invalidate_if_generation_matching() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2, 3], 1);

        cache.invalidate_if_generation(1, 1);

        assert!(cache.get(1).is_some());
    }

    #[test]
    fn test_invalidate_if_generation_mismatching() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2, 3], 1);

        cache.invalidate_if_generation(1, 2);

        assert!(cache.get(1).is_none());
    }

    #[test]
    fn test_cache_tracks_stats() {
        let mut cache = make_cache();
        cache.insert(1, vec![1], 1);

        let stats = cache.stats();
        assert_eq!(stats.files, 1);
        assert_eq!(stats.total_bytes, 1);
    }

    #[test]
    fn test_insert_overwrites_existing() {
        let mut cache = make_cache();
        cache.insert(1, vec![1, 2], 1);
        cache.insert(1, vec![3, 4, 5], 2);

        let data = cache.get(1).unwrap();
        assert_eq!(data.data, vec![3, 4, 5]);
        assert_eq!(data.generation, 2);
    }
}
