//! Distributed-ready dedup index (fingerprint index) for cross-node deduplication.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An entry in the dedup index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupIndexEntry {
    /// BLAKE3 hash of the chunk.
    pub hash: [u8; 32],
    /// Segment containing the chunk.
    pub segment_id: u64,
    /// Offset within the segment.
    pub offset_in_segment: u32,
    /// Size in bytes.
    pub size: u32,
    /// Reference count.
    pub ref_count: u32,
}

/// Statistics for the dedup index.
#[derive(Debug, Clone, Default)]
pub struct DedupIndexStats {
    /// Total unique entries.
    pub total_entries: u64,
    /// Total bytes stored.
    pub total_bytes: u64,
    /// Total references.
    pub total_refs: u64,
    /// Unique ratio (entries / refs).
    pub unique_ratio: f64,
}

/// Configuration for the dedup index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupIndexConfig {
    /// Maximum entries in the index.
    pub max_entries: usize,
    /// Number of shards for distributed routing.
    pub shard_count: u8,
}

impl Default for DedupIndexConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000_000,
            shard_count: 16,
        }
    }
}

/// The dedup index.
#[derive(Debug)]
pub struct DedupIndex {
    config: DedupIndexConfig,
    entries: HashMap<[u8; 32], DedupIndexEntry>,
}

impl DedupIndex {
    /// Create a new dedup index.
    pub fn new(config: DedupIndexConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
        }
    }

    /// Insert a new entry or increment ref_count if exists.
    /// Returns true if new entry, false if existing (increments ref_count).
    pub fn insert(&mut self, hash: [u8; 32], segment_id: u64, offset: u32, size: u32) -> bool {
        if let Some(entry) = self.entries.get_mut(&hash) {
            entry.ref_count = entry.ref_count.saturating_add(1);
            false
        } else {
            let entry = DedupIndexEntry {
                hash,
                segment_id,
                offset_in_segment: offset,
                size,
                ref_count: 1,
            };
            self.entries.insert(hash, entry);
            true
        }
    }

    /// Look up an entry by hash.
    pub fn lookup(&self, hash: &[u8; 32]) -> Option<&DedupIndexEntry> {
        self.entries.get(hash)
    }

    /// Release a reference to an entry.
    /// Decrements ref_count, removes if zero, returns new ref_count.
    pub fn release(&mut self, hash: &[u8; 32]) -> Option<u32> {
        if let Some(entry) = self.entries.get_mut(hash) {
            if entry.ref_count <= 1 {
                self.entries.remove(hash);
                Some(0)
            } else {
                entry.ref_count -= 1;
                Some(entry.ref_count)
            }
        } else {
            None
        }
    }

    /// Return the number of entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Return the sum of all ref_counts.
    pub fn total_refs(&self) -> u64 {
        self.entries.values().map(|e| e.ref_count as u64).sum()
    }

    /// Return statistics.
    pub fn stats(&self) -> DedupIndexStats {
        let total_entries = self.entries.len() as u64;
        let total_bytes: u64 = self.entries.values().map(|e| e.size as u64).sum();
        let total_refs = self.total_refs();
        let unique_ratio = if total_refs > 0 {
            total_entries as f64 / total_refs as f64
        } else {
            1.0
        };

        DedupIndexStats {
            total_entries,
            total_bytes,
            total_refs,
            unique_ratio,
        }
    }

    /// Compute which shard a hash belongs to.
    pub fn shard_for(&self, hash: &[u8; 32]) -> u8 {
        hash[0] % self.config.shard_count
    }

    /// Return entries in a specific shard.
    pub fn entries_in_shard(&self, shard: u8) -> Vec<&DedupIndexEntry> {
        self.entries
            .values()
            .filter(|e| self.shard_for(&e.hash) == shard)
            .collect()
    }

    /// Check if the index is at capacity.
    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.config.max_entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(val: u8) -> [u8; 32] {
        let mut hash = [0u8; 32];
        hash[0] = val;
        hash
    }

    #[test]
    fn config_default() {
        let config = DedupIndexConfig::default();
        assert_eq!(config.max_entries, 10_000_000);
        assert_eq!(config.shard_count, 16);
    }

    #[test]
    fn insert_new_entry_returns_true() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        let hash = make_hash(1);
        let result = index.insert(hash, 100, 0, 4096);
        assert!(result);
    }

    #[test]
    fn insert_existing_increments_ref_count() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        let hash = make_hash(1);
        index.insert(hash, 100, 0, 4096);
        let result = index.insert(hash, 100, 0, 4096);
        assert!(!result);
        let entry = index.lookup(&hash).unwrap();
        assert_eq!(entry.ref_count, 2);
    }

    #[test]
    fn lookup_found() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        let hash = make_hash(1);
        index.insert(hash, 100, 0, 4096);
        let entry = index.lookup(&hash);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().segment_id, 100);
    }

    #[test]
    fn lookup_not_found() {
        let index = DedupIndex::new(DedupIndexConfig::default());
        let hash = make_hash(1);
        assert!(index.lookup(&hash).is_none());
    }

    #[test]
    fn release_decrements_ref() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        let hash = make_hash(1);
        index.insert(hash, 100, 0, 4096);
        index.insert(hash, 100, 0, 4096);
        let new_ref = index.release(&hash);
        assert_eq!(new_ref, Some(1));
    }

    #[test]
    fn release_to_zero_removes_entry() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        let hash = make_hash(1);
        index.insert(hash, 100, 0, 4096);
        let new_ref = index.release(&hash);
        assert_eq!(new_ref, Some(0));
        assert!(index.lookup(&hash).is_none());
    }

    #[test]
    fn release_unknown_returns_none() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        let hash = make_hash(1);
        let result = index.release(&hash);
        assert!(result.is_none());
    }

    #[test]
    fn entry_count_increments() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        assert_eq!(index.entry_count(), 0);
        index.insert(make_hash(1), 100, 0, 4096);
        assert_eq!(index.entry_count(), 1);
        index.insert(make_hash(2), 100, 4096, 4096);
        assert_eq!(index.entry_count(), 2);
    }

    #[test]
    fn total_refs_sums_all() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        index.insert(make_hash(1), 100, 0, 4096);
        index.insert(make_hash(1), 100, 0, 4096);
        index.insert(make_hash(2), 100, 4096, 4096);
        index.insert(make_hash(3), 100, 8192, 4096);
        assert_eq!(index.total_refs(), 4);
    }

    #[test]
    fn shard_for_deterministic() {
        let index = DedupIndex::new(DedupIndexConfig::default());
        let hash1 = make_hash(17);
        let hash2 = make_hash(33);
        assert_eq!(index.shard_for(&hash1), 17 % 16);
        assert_eq!(index.shard_for(&hash2), 33 % 16);
    }

    #[test]
    fn entries_in_shard_filtered() {
        let config = DedupIndexConfig {
            shard_count: 4,
            ..Default::default()
        };
        let mut index = DedupIndex::new(config);

        for i in 0..16u8 {
            let mut hash = [0u8; 32];
            hash[0] = i;
            index.insert(hash, 100, 0, 4096);
        }

        let shard0 = index.entries_in_shard(0);
        assert_eq!(shard0.len(), 4);

        let shard1 = index.entries_in_shard(1);
        assert_eq!(shard1.len(), 4);
    }

    #[test]
    fn stats_empty_index() {
        let index = DedupIndex::new(DedupIndexConfig::default());
        let stats = index.stats();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.total_bytes, 0);
        assert_eq!(stats.total_refs, 0);
        assert_eq!(stats.unique_ratio, 1.0);
    }

    #[test]
    fn stats_with_entries() {
        let mut index = DedupIndex::new(DedupIndexConfig::default());
        index.insert(make_hash(1), 100, 0, 4096);
        index.insert(make_hash(1), 100, 0, 4096);
        index.insert(make_hash(2), 100, 4096, 2048);

        let stats = index.stats();
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.total_bytes, 4096 + 2048);
        assert_eq!(stats.total_refs, 3);
        assert!((stats.unique_ratio - 0.6666666).abs() < 0.01);
    }

    #[test]
    fn is_full_false() {
        let config = DedupIndexConfig {
            max_entries: 100,
            ..Default::default()
        };
        let mut index = DedupIndex::new(config);
        for i in 0..50u8 {
            index.insert(make_hash(i), 100, 0, 4096);
        }
        assert!(!index.is_full());
    }

    #[test]
    fn is_full_true() {
        let config = DedupIndexConfig {
            max_entries: 3,
            ..Default::default()
        };
        let mut index = DedupIndex::new(config);
        index.insert(make_hash(1), 100, 0, 4096);
        index.insert(make_hash(2), 100, 0, 4096);
        index.insert(make_hash(3), 100, 0, 4096);
        assert!(index.is_full());
    }
}
