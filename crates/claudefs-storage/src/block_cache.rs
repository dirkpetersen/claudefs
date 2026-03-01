//! LRU block cache for in-memory caching of hot data.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::block::{BlockId, BlockRef};
use crate::checksum::Checksum;
use crate::error::StorageResult;

/// A cached block entry with access tracking and metadata.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// The block reference identifying this cached block.
    pub block_ref: BlockRef,
    /// The raw data stored in this cache entry.
    pub data: Vec<u8>,
    /// Checksum for data integrity verification.
    pub checksum: Checksum,
    /// Number of times this entry has been accessed.
    pub access_count: u64,
    /// Timestamp of last access in nanoseconds since epoch.
    pub last_access_ns: u64,
    /// Whether this entry has been modified but not flushed to storage.
    pub dirty: bool,
    /// Whether this entry is pinned and cannot be evicted.
    pub pinned: bool,
}

/// Statistics tracking for the block cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of entries inserted.
    pub insertions: u64,
    /// Number of entries evicted.
    pub evictions: u64,
    /// Number of dirty entries written back to storage.
    pub dirty_writebacks: u64,
}

impl CacheStats {
    /// Calculates the cache hit rate as a ratio of hits to total accesses.
    ///
    /// Returns 0.0 if there have been no cache accesses.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Configuration for the block cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockCacheConfig {
    /// Maximum memory usage in bytes (default 256MB).
    pub max_memory_bytes: u64,
    /// Maximum number of cached blocks (default 65536).
    pub max_entries: usize,
    /// Number of entries to evict when the cache is full (default 16).
    pub eviction_batch_size: usize,
    /// Whether writes go through to storage immediately (default true).
    pub write_through: bool,
}

impl Default for BlockCacheConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 256 * 1024 * 1024, // 256MB
            max_entries: 65536,
            eviction_batch_size: 16,
            write_through: true,
        }
    }
}

/// LRU block cache for storing frequently accessed blocks in memory.
pub struct BlockCache {
    lookup: HashMap<(u16, u64), usize>,
    entries: Vec<Option<CacheEntry>>,
    lru_order: VecDeque<(u16, u64)>,
    config: BlockCacheConfig,
    stats: CacheStats,
    current_memory_bytes: u64,
}

impl BlockCache {
    /// Creates a new block cache with the given configuration.
    pub fn new(config: BlockCacheConfig) -> Self {
        debug!(
            max_memory_bytes = config.max_memory_bytes,
            max_entries = config.max_entries,
            "created new block cache"
        );
        Self {
            lookup: HashMap::new(),
            entries: Vec::new(),
            lru_order: VecDeque::new(),
            config,
            stats: CacheStats::default(),
            current_memory_bytes: 0,
        }
    }

    fn now_ns() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    fn entry_memory_size(data_len: usize) -> u64 {
        // Approximate memory: data + overhead for entry metadata
        // This is a rough estimate including Vec<u8> overhead
        data_len as u64 + std::mem::size_of::<CacheEntry>() as u64
    }

    fn block_key(block_id: &BlockId) -> (u16, u64) {
        (block_id.device_idx, block_id.offset)
    }

    fn move_to_front(&mut self, key: &(u16, u64)) {
        if let Some(pos) = self.lru_order.iter().position(|k| k == key) {
            self.lru_order.remove(pos);
            self.lru_order.push_front(*key);
        }
    }

    fn evict_lru_internal(&mut self) -> Option<CacheEntry> {
        // Find the least recently used non-pinned entry from the back
        let key = self.lru_order.iter().rfind(|(device_idx, offset)| {
            if let Some(&idx) = self.lookup.get(&(*device_idx, *offset)) {
                if let Some(ref entry) = self.entries[idx] {
                    return !entry.pinned;
                }
            }
            false
        })?;

        let key = *key;
        self.evict_by_key(&key)
    }

    fn evict_by_key(&mut self, key: &(u16, u64)) -> Option<CacheEntry> {
        if let Some(&idx) = self.lookup.get(key) {
            if let Some(entry) = self.entries[idx].take() {
                self.current_memory_bytes -= Self::entry_memory_size(entry.data.len());
                self.lookup.remove(key);
                self.lru_order.retain(|k| k != key);
                debug!(device_idx = key.0, offset = key.1, "evicted cache entry");
                self.stats.evictions += 1;
                return Some(entry);
            }
        }
        None
    }

    fn evict_until_space(&mut self, required_bytes: u64) {
        while self.current_memory_bytes + required_bytes > self.config.max_memory_bytes
            || self.lookup.len() >= self.config.max_entries
        {
            if self.evict_lru_internal().is_none() {
                break;
            }
        }
    }

    /// Retrieves a cached entry by block ID, updating its LRU position.
    ///
    /// Returns `None` if the block is not in the cache.
    pub fn get(&mut self, block_id: &BlockId) -> Option<&CacheEntry> {
        let key = Self::block_key(block_id);
        if let Some(&idx) = self.lookup.get(&key) {
            if self.entries[idx].is_some() {
                self.move_to_front(&key);
                if let Some(ref mut e) = self.entries[idx] {
                    e.access_count += 1;
                    e.last_access_ns = Self::now_ns();
                }
                self.stats.hits += 1;
                return self.entries[idx].as_ref();
            }
        }
        self.stats.misses += 1;
        None
    }

    /// Convenience method to get just the data portion of a cached block.
    pub fn get_data(&mut self, block_id: &BlockId) -> Option<&[u8]> {
        self.get(block_id).map(|e| e.data.as_slice())
    }

    /// Inserts a new entry into the cache, evicting if necessary.
    pub fn insert(
        &mut self,
        block_ref: BlockRef,
        data: Vec<u8>,
        checksum: Checksum,
    ) -> StorageResult<()> {
        let key = Self::block_key(&block_ref.id);
        let memory_needed = Self::entry_memory_size(data.len());

        // Evict entries if we need space
        self.evict_until_space(memory_needed);

        // Check if we're replacing an existing entry
        let was_present = self.lookup.contains_key(&key);

        if was_present {
            // Remove old entry first
            if let Some(&idx) = self.lookup.get(&key) {
                if let Some(old_entry) = self.entries[idx].take() {
                    self.current_memory_bytes -= Self::entry_memory_size(old_entry.data.len());
                }
            }
        }

        // Create new entry
        let entry = CacheEntry {
            block_ref,
            data,
            checksum,
            access_count: 1,
            last_access_ns: Self::now_ns(),
            dirty: false,
            pinned: false,
        };

        self.current_memory_bytes += memory_needed;

        // Allocate slot
        let idx = if was_present {
            self.lookup[&key]
        } else {
            let idx = self.entries.len();
            self.entries.push(None);
            self.lookup.insert(key, idx);
            self.lru_order.push_front(key);
            idx
        };

        self.entries[idx] = Some(entry);
        self.stats.insertions += 1;

        Ok(())
    }

    /// Inserts a dirty entry into the cache.
    pub fn insert_dirty(
        &mut self,
        block_ref: BlockRef,
        data: Vec<u8>,
        checksum: Checksum,
    ) -> StorageResult<()> {
        let key = Self::block_key(&block_ref.id);
        let memory_needed = Self::entry_memory_size(data.len());

        // Evict entries if we need space
        self.evict_until_space(memory_needed);

        // Check if we're replacing an existing entry
        let was_present = self.lookup.contains_key(&key);

        if was_present {
            if let Some(&idx) = self.lookup.get(&key) {
                if let Some(old_entry) = self.entries[idx].take() {
                    self.current_memory_bytes -= Self::entry_memory_size(old_entry.data.len());
                }
            }
        }

        let entry = CacheEntry {
            block_ref,
            data,
            checksum,
            access_count: 1,
            last_access_ns: Self::now_ns(),
            dirty: true,
            pinned: false,
        };

        self.current_memory_bytes += memory_needed;

        let idx = if was_present {
            self.lookup[&key]
        } else {
            let idx = self.entries.len();
            self.entries.push(None);
            self.lookup.insert(key, idx);
            self.lru_order.push_front(key);
            idx
        };

        self.entries[idx] = Some(entry);
        self.stats.insertions += 1;

        Ok(())
    }

    /// Removes an entry from the cache by block ID.
    ///
    /// Returns the removed entry if it existed.
    pub fn remove(&mut self, block_id: &BlockId) -> Option<CacheEntry> {
        let key = Self::block_key(block_id);
        if let Some(&idx) = self.lookup.get(&key) {
            if let Some(entry) = self.entries[idx].take() {
                self.current_memory_bytes -= Self::entry_memory_size(entry.data.len());
                self.lookup.remove(&key);
                self.lru_order.retain(|k| k != &key);
                return Some(entry);
            }
        }
        None
    }

    /// Pins a block in the cache, preventing eviction.
    ///
    /// Returns `false` if the block is not in the cache.
    pub fn pin(&mut self, block_id: &BlockId) -> bool {
        let key = Self::block_key(block_id);
        if let Some(&idx) = self.lookup.get(&key) {
            if let Some(ref mut entry) = self.entries[idx] {
                entry.pinned = true;
                debug!(
                    device_idx = block_id.device_idx,
                    offset = block_id.offset,
                    "pinned cache entry"
                );
                return true;
            }
        }
        false
    }

    /// Unpins a block in the cache, allowing eviction.
    ///
    /// Returns `false` if the block is not in the cache.
    pub fn unpin(&mut self, block_id: &BlockId) -> bool {
        let key = Self::block_key(block_id);
        if let Some(&idx) = self.lookup.get(&key) {
            if let Some(ref mut entry) = self.entries[idx] {
                entry.pinned = false;
                debug!(
                    device_idx = block_id.device_idx,
                    offset = block_id.offset,
                    "unpinned cache entry"
                );
                return true;
            }
        }
        false
    }

    /// Marks a dirty entry as clean.
    pub fn mark_clean(&mut self, block_id: &BlockId) {
        let key = Self::block_key(block_id);
        if let Some(&idx) = self.lookup.get(&key) {
            if let Some(ref mut entry) = self.entries[idx] {
                if entry.dirty {
                    entry.dirty = false;
                    self.stats.dirty_writebacks += 1;
                }
            }
        }
    }

    /// Returns all dirty entries in the cache.
    pub fn dirty_entries(&self) -> Vec<&CacheEntry> {
        self.entries
            .iter()
            .filter_map(|e| e.as_ref())
            .filter(|e| e.dirty)
            .collect()
    }

    /// Checks if a block is contained in the cache.
    pub fn contains(&self, block_id: &BlockId) -> bool {
        let key = Self::block_key(block_id);
        self.lookup.contains_key(&key)
    }

    /// Returns the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.lookup.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.lookup.is_empty()
    }

    /// Returns the current memory usage in bytes.
    pub fn memory_usage(&self) -> u64 {
        self.current_memory_bytes
    }

    /// Returns a reference to the cache statistics.
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Clears all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lookup.clear();
        self.lru_order.clear();
        self.current_memory_bytes = 0;
        debug!("cleared block cache");
    }

    /// Evicts the least recently used non-pinned entry.
    ///
    /// Returns the evicted entry if one was available.
    pub fn evict_lru(&mut self) -> Option<CacheEntry> {
        self.evict_lru_internal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockSize;
    use crate::checksum::{compute, ChecksumAlgorithm};

    fn create_test_block_ref(device_idx: u16, offset: u64, size: BlockSize) -> BlockRef {
        BlockRef {
            id: BlockId::new(device_idx, offset),
            size,
        }
    }

    fn create_test_data(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 256) as u8).collect()
    }

    #[test]
    fn test_insert_and_get_roundtrip() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 100, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        let retrieved = cache.get(&block_ref.id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().data, data);
    }

    #[test]
    fn test_cache_miss_returns_none() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_id = BlockId::new(0, 999);
        let result = cache.get(&block_id);

        assert!(result.is_none());
    }

    #[test]
    fn test_lru_eviction_order() {
        let mut config = BlockCacheConfig::default();
        config.max_memory_bytes = 10000;
        config.max_entries = 100;
        config.eviction_batch_size = 1;
        let mut cache = BlockCache::new(config);

        // Insert multiple blocks
        for i in 0..5 {
            let block_ref = create_test_block_ref(0, i, BlockSize::B4K);
            let data = create_test_data(1000);
            let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
            cache.insert(block_ref, data, checksum).unwrap();
        }

        // Access them in order to establish LRU
        for i in 0..5 {
            let block_id = BlockId::new(0, i);
            let _ = cache.get(&block_id);
        }

        // Access block 0 to make it most recent
        let _ = cache.get(&BlockId::new(0, 0));

        // Now insert more blocks to trigger eviction
        for i in 100..110 {
            let block_ref = create_test_block_ref(0, i, BlockSize::B4K);
            let data = create_test_data(1000);
            let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
            cache.insert(block_ref, data, checksum).unwrap();
        }

        // Block 1 (least recently used after block 0 was accessed) should be evicted
        assert!(!cache.contains(&BlockId::new(0, 1)));
    }

    #[test]
    fn test_pinned_blocks_not_evicted() {
        let mut config = BlockCacheConfig::default();
        config.max_memory_bytes = 5000;
        config.max_entries = 10;
        config.eviction_batch_size = 1;
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(1000);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        // Pin the block
        assert!(cache.pin(&block_ref.id));

        // Try to evict
        let evicted = cache.evict_lru();
        assert!(evicted.is_none());

        // Unpin and try again
        assert!(cache.unpin(&block_ref.id));
        let evicted = cache.evict_lru();
        assert!(evicted.is_some());
    }

    #[test]
    fn test_memory_limit_enforcement() {
        let mut config = BlockCacheConfig::default();
        config.max_memory_bytes = 4096 + 100; // Just over one block
        config.max_entries = 100;
        config.eviction_batch_size = 1;
        let mut cache = BlockCache::new(config);

        // Insert first block
        let block_ref1 = create_test_block_ref(0, 1, BlockSize::B4K);
        let data1 = create_test_data(4096);
        let checksum1 = compute(ChecksumAlgorithm::Crc32c, &data1);
        cache
            .insert(block_ref1.clone(), data1.clone(), checksum1)
            .unwrap();

        // Insert second block - should trigger eviction
        let block_ref2 = create_test_block_ref(0, 2, BlockSize::B4K);
        let data2 = create_test_data(4096);
        let checksum2 = compute(ChecksumAlgorithm::Crc32c, &data2);
        cache
            .insert(block_ref2.clone(), data2.clone(), checksum2)
            .unwrap();

        // One of them should still be in cache
        assert!(cache.len() >= 1);
    }

    #[test]
    fn test_entry_count_limit_enforcement() {
        let mut config = BlockCacheConfig::default();
        config.max_memory_bytes = 1000000;
        config.max_entries = 3;
        config.eviction_batch_size = 1;
        let mut cache = BlockCache::new(config);

        // Insert more than max_entries
        for i in 0..5 {
            let block_ref = create_test_block_ref(0, i, BlockSize::B4K);
            let data = create_test_data(100);
            let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
            cache.insert(block_ref, data, checksum).unwrap();
        }

        // Should be limited to max_entries
        assert!(cache.len() <= 3);
    }

    #[test]
    fn test_dirty_tracking() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        cache
            .insert_dirty(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        let entry = cache.get(&block_ref.id).unwrap();
        assert!(entry.dirty);
    }

    #[test]
    fn test_mark_clean_works() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        cache
            .insert_dirty(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        cache.mark_clean(&block_ref.id);

        let entry = cache.get(&block_ref.id).unwrap();
        assert!(!entry.dirty);
    }

    #[test]
    fn test_dirty_entries_returns_correct_set() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        // Insert clean entry
        let block_ref1 = create_test_block_ref(0, 1, BlockSize::B4K);
        let data1 = create_test_data(4096);
        let checksum1 = compute(ChecksumAlgorithm::Crc32c, &data1);
        cache
            .insert(block_ref1.clone(), data1.clone(), checksum1)
            .unwrap();

        // Insert dirty entry
        let block_ref2 = create_test_block_ref(0, 2, BlockSize::B4K);
        let data2 = create_test_data(4096);
        let checksum2 = compute(ChecksumAlgorithm::Crc32c, &data2);
        cache
            .insert_dirty(block_ref2.clone(), data2.clone(), checksum2)
            .unwrap();

        let dirty = cache.dirty_entries();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].block_ref.id, block_ref2.id);
    }

    #[test]
    fn test_remove_entry() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();
        assert!(cache.contains(&block_ref.id));

        let removed = cache.remove(&block_ref.id);
        assert!(removed.is_some());
        assert!(!cache.contains(&block_ref.id));
    }

    #[test]
    fn test_clear_all_entries() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        for i in 0..5 {
            let block_ref = create_test_block_ref(0, i, BlockSize::B4K);
            let data = create_test_data(4096);
            let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
            cache.insert(block_ref, data, checksum).unwrap();
        }

        assert!(!cache.is_empty());
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_stats_tracking() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        // Access the block (hit)
        let _ = cache.get(&block_ref.id);

        // Try to get non-existent block (miss)
        let _ = cache.get(&BlockId::new(0, 999));

        let stats = cache.stats();
        assert_eq!(stats.insertions, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_hit_rate_calculation() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        // 3 hits, 1 miss
        let _ = cache.get(&block_ref.id);
        let _ = cache.get(&block_ref.id);
        let _ = cache.get(&block_ref.id);
        let _ = cache.get(&BlockId::new(0, 999));

        let stats = cache.stats();
        assert!((stats.hit_rate() - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_hit_rate_empty_cache() {
        let config = BlockCacheConfig::default();
        let cache = BlockCache::new(config);

        assert_eq!(cache.stats().hit_rate(), 0.0);
    }

    #[test]
    fn test_insert_replaces_existing_entry() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);

        // First insert
        let data1 = create_test_data(4096);
        let checksum1 = compute(ChecksumAlgorithm::Crc32c, &data1);
        cache
            .insert(block_ref.clone(), data1.clone(), checksum1)
            .unwrap();

        // Second insert (replace)
        let data2 = vec![0xFF; 4096];
        let checksum2 = compute(ChecksumAlgorithm::Crc32c, &data2);
        cache
            .insert(block_ref.clone(), data2.clone(), checksum2)
            .unwrap();

        let entry = cache.get(&block_ref.id).unwrap();
        assert_eq!(entry.data, data2);
    }

    #[test]
    fn test_pin_unpin_operations() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        // Pin
        assert!(cache.pin(&block_ref.id));
        let entry = cache.get(&block_ref.id).unwrap();
        assert!(entry.pinned);

        // Unpin
        assert!(cache.unpin(&block_ref.id));
        let entry = cache.get(&block_ref.id).unwrap();
        assert!(!entry.pinned);
    }

    #[test]
    fn test_pin_nonexistent_returns_false() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let result = cache.pin(&BlockId::new(0, 999));
        assert!(!result);
    }

    #[test]
    fn test_unpin_nonexistent_returns_false() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let result = cache.unpin(&BlockId::new(0, 999));
        assert!(!result);
    }

    #[test]
    fn test_get_updates_lru_position() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref1 = create_test_block_ref(0, 1, BlockSize::B4K);
        let block_ref2 = create_test_block_ref(0, 2, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        cache
            .insert(block_ref1.clone(), data.clone(), checksum)
            .unwrap();
        cache
            .insert(block_ref2.clone(), data.clone(), checksum)
            .unwrap();

        // Access block 1 to make it recently used
        let _ = cache.get(&block_ref1.id);

        // Trigger eviction
        cache.evict_lru();

        // Block 2 should be evicted (least recently used)
        assert!(!cache.contains(&block_ref2.id));
        // Block 1 should still be there
        assert!(cache.contains(&block_ref1.id));
    }

    #[test]
    fn test_empty_cache_operations() {
        let config = BlockCacheConfig::default();
        let cache = BlockCache::new(config);

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        assert!(!cache.contains(&BlockId::new(0, 1)));
        assert_eq!(cache.memory_usage(), 0);
    }

    #[test]
    fn test_config_defaults() {
        let config = BlockCacheConfig::default();

        assert_eq!(config.max_memory_bytes, 256 * 1024 * 1024);
        assert_eq!(config.max_entries, 65536);
        assert_eq!(config.eviction_batch_size, 16);
        assert!(config.write_through);
    }

    #[test]
    fn test_multiple_evictions_in_sequence() {
        let mut config = BlockCacheConfig::default();
        config.max_memory_bytes = 5000;
        config.max_entries = 10;
        config.eviction_batch_size = 1;
        let mut cache = BlockCache::new(config);

        // Insert enough blocks to exceed limits
        for i in 0..20 {
            let block_ref = create_test_block_ref(0, i, BlockSize::B4K);
            let data = create_test_data(1000);
            let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
            cache.insert(block_ref, data, checksum).unwrap();
        }

        // Evict multiple times
        let mut eviction_count = 0;
        for _ in 0..10 {
            if cache.evict_lru().is_some() {
                eviction_count += 1;
            }
        }

        assert!(eviction_count > 0);
    }

    #[test]
    fn test_contains_check() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        assert!(!cache.contains(&block_ref.id));

        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        assert!(cache.contains(&block_ref.id));
        assert!(!cache.contains(&BlockId::new(0, 999)));
    }

    #[test]
    fn test_get_data_convenience() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        let retrieved_data = cache.get_data(&block_ref.id);
        assert!(retrieved_data.is_some());
        assert_eq!(retrieved_data.unwrap(), data.as_slice());
    }

    #[test]
    fn test_dirty_writeback_increments_stats() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        cache
            .insert_dirty(block_ref.clone(), data.clone(), checksum)
            .unwrap();
        cache.mark_clean(&block_ref.id);

        assert_eq!(cache.stats().dirty_writebacks, 1);
    }

    #[test]
    fn test_different_device_indices() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        for device_idx in 0..3u16 {
            let block_ref = create_test_block_ref(device_idx, 1, BlockSize::B4K);
            let data = create_test_data(4096);
            let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
            cache.insert(block_ref, data, checksum).unwrap();
        }

        for device_idx in 0..3u16 {
            assert!(cache.contains(&BlockId::new(device_idx, 1)));
        }
    }

    #[test]
    fn test_access_count_increments() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = create_test_block_ref(0, 1, BlockSize::B4K);
        let data = create_test_data(4096);
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);

        cache
            .insert(block_ref.clone(), data.clone(), checksum)
            .unwrap();

        let _ = cache.get(&block_ref.id);
        let _ = cache.get(&block_ref.id);
        let _ = cache.get(&block_ref.id);

        let entry = cache.get(&block_ref.id).unwrap();
        assert!(entry.access_count >= 3);
    }
}
