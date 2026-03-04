//! LRU cache for decrypted+decompressed chunks on the read path.

use crate::fingerprint::ChunkHash;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Configuration for the read cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadCacheConfig {
    /// Maximum total bytes of cached chunk data.
    pub capacity_bytes: usize,
    /// Maximum number of cache entries.
    pub max_entries: usize,
}

impl Default for ReadCacheConfig {
    fn default() -> Self {
        Self {
            capacity_bytes: 256 * 1024 * 1024,
            max_entries: 65536,
        }
    }
}

/// Statistics for the read cache.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of evictions.
    pub evictions: u64,
    /// Current number of entries in the cache.
    pub current_entries: usize,
    /// Current total bytes in the cache.
    pub current_bytes: usize,
}

impl CacheStats {
    /// Hit rate as a fraction [0.0, 1.0].
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }
}

/// LRU cache for decoded chunk data.
///
/// Stores the decrypted and decompressed `Vec<u8>` keyed by `ChunkHash`,
/// avoiding repeated decrypt+decompress on re-reads.
pub struct ReadCache {
    config: ReadCacheConfig,
    data: HashMap<ChunkHash, Vec<u8>>,
    lru: VecDeque<ChunkHash>,
    stats: CacheStats,
}

impl ReadCache {
    /// Create a new read cache with the given configuration.
    pub fn new(config: ReadCacheConfig) -> Self {
        Self {
            config,
            data: HashMap::new(),
            lru: VecDeque::new(),
            stats: CacheStats::default(),
        }
    }

    /// Look up a chunk by hash.
    ///
    /// Returns `Some(data)` on hit, `None` on miss.
    /// On hit, moves the entry to the back of the LRU queue.
    pub fn get(&mut self, hash: &ChunkHash) -> Option<&Vec<u8>> {
        if self.data.contains_key(hash) {
            // Move to back (most recently used)
            self.lru.retain(|h| h != hash);
            self.lru.push_back(*hash);
            self.stats.hits += 1;
            self.data.get(hash)
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// Insert a chunk into the cache.
    ///
    /// Evicts LRU entries if `capacity_bytes` or `max_entries` would be exceeded.
    pub fn insert(&mut self, hash: ChunkHash, data: Vec<u8>) {
        let entry_bytes = data.len();

        // If already exists, remove old entry first
        if self.data.contains_key(&hash) {
            self.lru.retain(|h| h != &hash);
            self.data.remove(&hash);
        }

        // Evict until we have room
        while (self.total_bytes() + entry_bytes > self.config.capacity_bytes
            || self.data.len() >= self.config.max_entries)
            && !self.lru.is_empty()
        {
            self.evict_lru();
        }

        // Add new entry
        self.data.insert(hash, data);
        self.lru.push_back(hash);
        self.update_stats();
    }

    /// Remove a specific entry.
    ///
    /// Returns `true` if the entry was present and removed.
    pub fn remove(&mut self, hash: &ChunkHash) -> bool {
        if self.data.remove(hash).is_some() {
            self.lru.retain(|h| h != hash);
            self.update_stats();
            true
        } else {
            false
        }
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.data.clear();
        self.lru.clear();
        self.stats = CacheStats::default();
    }

    /// Get current stats snapshot.
    pub fn stats(&self) -> CacheStats {
        self.stats.clone()
    }

    /// Number of entries currently in cache.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// True if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    fn total_bytes(&self) -> usize {
        self.data.values().map(|v| v.len()).sum()
    }

    fn evict_lru(&mut self) {
        if let Some(hash) = self.lru.pop_front() {
            self.data.remove(&hash);
            self.stats.evictions += 1;
        }
    }

    fn update_stats(&mut self) {
        self.stats.current_entries = self.data.len();
        self.stats.current_bytes = self.total_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(n: u8) -> ChunkHash {
        ChunkHash([n; 32])
    }

    fn make_data(n: usize) -> Vec<u8> {
        vec![0xAB; n]
    }

    #[test]
    fn test_empty_cache() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash = make_hash(1);
        assert!(cache.get(&hash).is_none());
    }

    #[test]
    fn test_insert_and_hit() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash = make_hash(1);
        let data = make_data(1024);
        cache.insert(hash, data.clone());
        let result = cache.get(&hash);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), &data);
    }

    #[test]
    fn test_miss_after_never_inserted() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash1 = make_hash(1);
        let hash2 = make_hash(2);
        cache.insert(hash1, make_data(1024));
        assert!(cache.get(&hash2).is_none());
    }

    #[test]
    fn test_evict_by_max_entries() {
        let config = ReadCacheConfig {
            capacity_bytes: 1024 * 1024,
            max_entries: 3,
        };
        let mut cache = ReadCache::new(config);

        cache.insert(make_hash(1), make_data(100));
        cache.insert(make_hash(2), make_data(100));
        cache.insert(make_hash(3), make_data(100));
        cache.insert(make_hash(4), make_data(100));

        // First entry should be evicted
        assert!(cache.get(&make_hash(1)).is_none());
        assert!(cache.get(&make_hash(2)).is_some());
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn test_evict_by_capacity_bytes() {
        let config = ReadCacheConfig {
            capacity_bytes: 500,
            max_entries: 100,
        };
        let mut cache = ReadCache::new(config);

        cache.insert(make_hash(1), make_data(200));
        cache.insert(make_hash(2), make_data(200));
        cache.insert(make_hash(3), make_data(200)); // Should trigger eviction

        // First entry should be evicted due to capacity
        assert!(cache.get(&make_hash(1)).is_none() || cache.stats().evictions >= 1);
    }

    #[test]
    fn test_lru_order_respected() {
        let config = ReadCacheConfig {
            capacity_bytes: 1024,
            max_entries: 3,
        };
        let mut cache = ReadCache::new(config);

        cache.insert(make_hash(1), make_data(300));
        cache.insert(make_hash(2), make_data(300));
        cache.insert(make_hash(3), make_data(300));

        // Access hash1 to make it most recently used
        cache.get(&make_hash(1));

        // Insert another - should evict hash2 (oldest after access)
        cache.insert(make_hash(4), make_data(300));

        // hash1 should still be present (was accessed recently)
        assert!(cache.get(&make_hash(1)).is_some());
    }

    #[test]
    fn test_remove_entry() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash = make_hash(1);
        cache.insert(hash, make_data(1024));
        assert!(cache.remove(&hash));
        assert!(cache.get(&hash).is_none());
    }

    #[test]
    fn test_remove_missing() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash = make_hash(1);
        assert!(!cache.remove(&hash));
    }

    #[test]
    fn test_clear_empties_cache() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        cache.insert(make_hash(1), make_data(1024));
        cache.insert(make_hash(2), make_data(1024));
        cache.clear();
        assert!(cache.is_empty());
        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.evictions, 0);
    }

    #[test]
    fn test_stats_hit_rate_zero() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        cache.get(&make_hash(1));
        cache.get(&make_hash(2));
        let stats = cache.stats();
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_stats_hit_rate_one() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash = make_hash(1);
        cache.insert(hash, make_data(1024));
        cache.get(&hash);
        let stats = cache.stats();
        assert_eq!(stats.hit_rate(), 1.0);
    }

    #[test]
    fn test_stats_hit_rate_mixed() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash1 = make_hash(1);
        let hash2 = make_hash(2);
        let hash3 = make_hash(3);
        let hash4 = make_hash(4);

        cache.insert(hash1, make_data(100));
        cache.insert(hash2, make_data(100));
        cache.insert(hash3, make_data(100));

        cache.get(&hash1);
        cache.get(&hash2);
        cache.get(&hash3);
        cache.get(&hash4);

        let stats = cache.stats();
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate(), 0.75);
    }

    #[test]
    fn test_eviction_count() {
        let config = ReadCacheConfig {
            capacity_bytes: 1024,
            max_entries: 2,
        };
        let mut cache = ReadCache::new(config);

        cache.insert(make_hash(1), make_data(512));
        cache.insert(make_hash(2), make_data(512));
        cache.insert(make_hash(3), make_data(512));
        cache.insert(make_hash(4), make_data(512));

        assert_eq!(cache.stats().evictions, 2);
    }

    #[test]
    fn test_large_capacity() {
        let config = ReadCacheConfig {
            capacity_bytes: 1024 * 1024,
            max_entries: 100,
        };
        let mut cache = ReadCache::new(config);

        for i in 0..100 {
            cache.insert(make_hash(i as u8), make_data(100));
        }

        assert_eq!(cache.len(), 100);
        assert_eq!(cache.stats().evictions, 0);
    }

    #[test]
    fn test_lru_eviction_order() {
        let config = ReadCacheConfig {
            capacity_bytes: 1000,
            max_entries: 3,
        };
        let mut cache = ReadCache::new(config);

        cache.insert(make_hash(1), make_data(300));
        cache.insert(make_hash(2), make_data(300));
        cache.insert(make_hash(3), make_data(300));

        cache.get(&make_hash(1));
        cache.get(&make_hash(2));

        cache.insert(make_hash(4), make_data(300));

        assert!(cache.get(&make_hash(1)).is_some());
        assert!(cache.get(&make_hash(2)).is_some());
    }

    #[test]
    fn test_capacity_bound_enforced() {
        let config = ReadCacheConfig {
            capacity_bytes: 500,
            max_entries: 1000,
        };
        let mut cache = ReadCache::new(config);

        cache.insert(make_hash(1), make_data(300));
        cache.insert(make_hash(2), make_data(300));
        cache.insert(make_hash(3), make_data(300));

        assert!(cache.stats().current_bytes <= 500);
    }

    #[test]
    fn test_stats_under_load() {
        let config = ReadCacheConfig {
            capacity_bytes: 1024,
            max_entries: 10,
        };
        let mut cache = ReadCache::new(config);

        for i in 0..20 {
            cache.insert(make_hash(i as u8), make_data(100));
            cache.get(&make_hash(i as u8));
        }

        let stats = cache.stats();
        assert!(stats.hits > 0);
        assert!(stats.evictions > 0);
    }

    #[test]
    fn test_reinsert_updates_data() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash = make_hash(1);

        cache.insert(hash, vec![1, 2, 3]);
        assert_eq!(cache.get(&hash).unwrap(), &vec![1, 2, 3]);

        cache.insert(hash, vec![4, 5, 6]);
        assert_eq!(cache.get(&hash).unwrap(), &vec![4, 5, 6]);
    }

    #[test]
    fn test_small_capacity_eviction() {
        let config = ReadCacheConfig {
            capacity_bytes: 50,
            max_entries: 10,
        };
        let mut cache = ReadCache::new(config);

        cache.insert(make_hash(1), make_data(100));
        cache.insert(make_hash(2), make_data(100));

        assert!(cache.stats().evictions >= 1);
    }

    #[test]
    fn test_eviction_updates_current_bytes() {
        let config = ReadCacheConfig {
            capacity_bytes: 100,
            max_entries: 10,
        };
        let mut cache = ReadCache::new(config.clone());

        cache.insert(make_hash(1), make_data(50));
        cache.insert(make_hash(2), make_data(60));

        let stats = cache.stats();
        assert!(stats.current_bytes <= config.capacity_bytes);
    }

    #[test]
    fn test_multiple_accesses_same_key() {
        let mut cache = ReadCache::new(ReadCacheConfig::default());
        let hash = make_hash(1);
        cache.insert(hash, make_data(100));

        cache.get(&hash);
        cache.get(&hash);
        cache.get(&hash);

        let stats = cache.stats();
        assert_eq!(stats.hits, 3);
    }
}
