//! LRU cache for dedup hash lookups.

use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
pub struct DedupCacheConfig {
    pub capacity: usize,
}

impl Default for DedupCacheConfig {
    fn default() -> Self {
        Self { capacity: 65536 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DedupCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub insertions: u64,
}

impl DedupCacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

pub struct DedupCache {
    config: DedupCacheConfig,
    map: HashMap<[u8; 32], ()>,
    order: VecDeque<[u8; 32]>,
    stats: DedupCacheStats,
}

impl DedupCache {
    pub fn new(config: DedupCacheConfig) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            stats: DedupCacheStats::default(),
            config,
        }
    }

    pub fn insert(&mut self, hash: [u8; 32]) {
        if self.map.contains_key(&hash) {
            self.order.retain(|h| h != &hash);
            self.order.push_back(hash);
            return;
        }

        if self.map.len() >= self.config.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.map.remove(&evicted);
                self.stats.evictions += 1;
            }
        }

        self.map.insert(hash, ());
        self.order.push_back(hash);
        self.stats.insertions += 1;
    }

    pub fn contains(&mut self, hash: &[u8; 32]) -> bool {
        if self.map.contains_key(hash) {
            self.stats.hits += 1;
            self.order.retain(|h| h != hash);
            self.order.push_back(*hash);
            true
        } else {
            self.stats.misses += 1;
            false
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.map.len() >= self.config.capacity
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    pub fn stats(&self) -> &DedupCacheStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_cache_config_default() {
        let config = DedupCacheConfig::default();
        assert_eq!(config.capacity, 65536);
    }

    #[test]
    fn new_cache_is_empty() {
        let cache = DedupCache::new(DedupCacheConfig::default());
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn insert_increments_len() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.insert([1u8; 32]);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn insert_increments_insertions() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.insert([1u8; 32]);
        assert_eq!(cache.stats().insertions, 1);
    }

    #[test]
    fn contains_hit() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.insert([1u8; 32]);
        assert!(cache.contains(&[1u8; 32]));
    }

    #[test]
    fn contains_miss() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        assert!(!cache.contains(&[1u8; 32]));
    }

    #[test]
    fn contains_increments_hits() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.insert([1u8; 32]);
        cache.contains(&[1u8; 32]);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn contains_increments_misses() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.contains(&[1u8; 32]);
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn hit_rate_zero_when_no_lookups() {
        let cache = DedupCache::new(DedupCacheConfig::default());
        assert_eq!(cache.stats().hit_rate(), 0.0);
    }

    #[test]
    fn hit_rate_one_after_all_hits() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.insert([1u8; 32]);
        cache.contains(&[1u8; 32]);
        assert_eq!(cache.stats().hit_rate(), 1.0);
    }

    #[test]
    fn hit_rate_zero_after_all_misses() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.contains(&[1u8; 32]);
        assert_eq!(cache.stats().hit_rate(), 0.0);
    }

    #[test]
    fn eviction_when_full() {
        let mut cache = DedupCache::new(DedupCacheConfig { capacity: 2 });
        cache.insert([1u8; 32]);
        cache.insert([2u8; 32]);
        cache.insert([3u8; 32]);
        assert!(!cache.contains(&[1u8; 32]));
    }

    #[test]
    fn eviction_increments_counter() {
        let mut cache = DedupCache::new(DedupCacheConfig { capacity: 2 });
        cache.insert([1u8; 32]);
        cache.insert([2u8; 32]);
        cache.insert([3u8; 32]);
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn is_full_when_at_capacity() {
        let cache = DedupCache::new(DedupCacheConfig { capacity: 1 });
        assert!(!cache.is_full());
    }

    #[test]
    fn is_not_full_when_below_capacity() {
        let cache = DedupCache::new(DedupCacheConfig { capacity: 2 });
        assert!(!cache.is_full());
    }

    #[test]
    fn clear_empties_cache() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.insert([1u8; 32]);
        cache.clear();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn insert_duplicate_no_double_insertion() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.insert([1u8; 32]);
        cache.insert([1u8; 32]);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn insert_duplicate_no_eviction() {
        let mut cache = DedupCache::new(DedupCacheConfig { capacity: 1 });
        cache.insert([1u8; 32]);
        cache.insert([1u8; 32]);
        assert_eq!(cache.stats().evictions, 0);
    }

    #[test]
    fn lru_eviction_order() {
        let mut cache = DedupCache::new(DedupCacheConfig { capacity: 2 });
        cache.insert([1u8; 32]);
        cache.insert([2u8; 32]);
        cache.insert([3u8; 32]);
        assert!(!cache.contains(&[1u8; 32]));
        assert!(cache.contains(&[2u8; 32]));
    }

    #[test]
    fn recently_used_not_evicted() {
        let mut cache = DedupCache::new(DedupCacheConfig { capacity: 2 });
        cache.insert([1u8; 32]);
        cache.insert([2u8; 32]);
        cache.contains(&[1u8; 32]);
        cache.insert([3u8; 32]);
        assert!(cache.contains(&[1u8; 32]));
    }

    #[test]
    fn stats_accumulated_across_operations() {
        let mut cache = DedupCache::new(DedupCacheConfig::default());
        cache.insert([1u8; 32]);
        cache.insert([2u8; 32]);
        cache.contains(&[1u8; 32]);
        cache.contains(&[3u8; 32]);
        assert_eq!(cache.stats().insertions, 2);
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn capacity_one_cache() {
        let mut cache = DedupCache::new(DedupCacheConfig { capacity: 1 });
        cache.insert([1u8; 32]);
        cache.insert([2u8; 32]);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(&[2u8; 32]));
        assert!(!cache.contains(&[1u8; 32]));
    }

    #[test]
    fn large_cache_no_evictions() {
        let mut cache = DedupCache::new(DedupCacheConfig { capacity: 200 });
        for i in 0..100 {
            cache.insert([i; 32]);
        }
        assert_eq!(cache.stats().evictions, 0);
    }

    #[test]
    fn contains_updates_lru_order() {
        let mut cache = DedupCache::new(DedupCacheConfig { capacity: 2 });
        cache.insert([1u8; 32]);
        cache.insert([2u8; 32]);
        cache.contains(&[1u8; 32]);
        cache.insert([3u8; 32]);
        assert!(cache.contains(&[1u8; 32]));
    }
}
