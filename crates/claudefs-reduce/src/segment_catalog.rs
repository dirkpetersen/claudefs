//! Segment catalog: in-memory index for O(1) chunk lookups across segments.

use crate::fingerprint::ChunkHash;
use crate::segment::Segment;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Location of a chunk within a specific segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkLocation {
    /// Which segment this chunk is in.
    pub segment_id: u64,
    /// Byte offset within the segment payload.
    pub offset: u32,
    /// Byte length of the chunk payload.
    pub size: u32,
    /// Original uncompressed size.
    pub original_size: u32,
}

/// Configuration for the segment catalog.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CatalogConfig {
    /// Maximum number of entries. When exceeded, oldest entries are evicted (LRU).
    /// 0 means unlimited.
    pub max_entries: usize,
}

/// In-memory index mapping chunk hashes to their location in segments.
/// Used for fast O(1) chunk lookups across many segments without scanning.
pub struct SegmentCatalog {
    config: CatalogConfig,
    index: HashMap<ChunkHash, ChunkLocation>,
    insertion_order: Vec<ChunkHash>,
}

impl SegmentCatalog {
    /// Create a new catalog with the given config.
    pub fn new(config: CatalogConfig) -> Self {
        Self {
            config,
            index: HashMap::new(),
            insertion_order: Vec::new(),
        }
    }

    /// Index all entries from a sealed segment into the catalog.
    /// If max_entries is set and would be exceeded, oldest entries are evicted first.
    pub fn index_segment(&mut self, segment: &Segment) {
        for entry in &segment.entries {
            let location = ChunkLocation {
                segment_id: segment.id,
                offset: entry.offset_in_segment,
                size: entry.payload_size,
                original_size: entry.original_size,
            };

            if self.index.contains_key(&entry.hash) {
                let old_pos = self
                    .insertion_order
                    .iter()
                    .position(|h| h == &entry.hash)
                    .expect("hash in index but not in insertion_order");
                self.insertion_order.remove(old_pos);
            }

            if self.config.max_entries > 0 && self.index.len() >= self.config.max_entries {
                if let Some(evicted_hash) = self.insertion_order.first().copied() {
                    self.insertion_order.remove(0);
                    self.index.remove(&evicted_hash);
                }
            }

            self.index.insert(entry.hash, location);
            self.insertion_order.push(entry.hash);
        }
    }

    /// Look up the location of a chunk by hash.
    pub fn lookup(&self, hash: &ChunkHash) -> Option<&ChunkLocation> {
        self.index.get(hash)
    }

    /// Remove all entries that belong to a given segment.
    /// Used when a segment is garbage collected or replaced.
    pub fn remove_segment(&mut self, segment_id: u64) {
        let hashes_to_remove: Vec<ChunkHash> = self
            .index
            .iter()
            .filter(|(_, loc)| loc.segment_id == segment_id)
            .map(|(h, _)| *h)
            .collect();

        for hash in &hashes_to_remove {
            self.index.remove(hash);
        }

        self.insertion_order
            .retain(|h| !hashes_to_remove.contains(h));
    }

    /// Total number of indexed chunks.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// True if no chunks are indexed.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Returns segment IDs currently indexed (sorted).
    pub fn indexed_segment_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.index.values().map(|loc| loc.segment_id).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Returns number of chunks from a specific segment.
    pub fn chunks_in_segment(&self, segment_id: u64) -> usize {
        self.index
            .values()
            .filter(|loc| loc.segment_id == segment_id)
            .count()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.index.clear();
        self.insertion_order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::blake3_hash;
    use crate::segment::SegmentEntry;

    fn make_test_segment(id: u64, chunks: &[&[u8]]) -> Segment {
        let mut entries = Vec::new();
        let mut payload = Vec::new();
        let mut offset = 0u32;

        for data in chunks {
            let hash = blake3_hash(data);
            entries.push(SegmentEntry {
                hash,
                offset_in_segment: offset,
                payload_size: data.len() as u32,
                original_size: data.len() as u32,
            });
            payload.extend_from_slice(data);
            offset += data.len() as u32;
        }

        Segment {
            id,
            entries,
            payload,
            sealed: true,
            created_at_secs: 0,
            payload_checksum: None,
        }
    }

    #[test]
    fn test_index_segment_basic() {
        let segment = make_test_segment(1, &[b"hello", b"world"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);

        let hash1 = blake3_hash(b"hello");
        let hash2 = blake3_hash(b"world");

        assert!(catalog.lookup(&hash1).is_some());
        assert!(catalog.lookup(&hash2).is_some());
    }

    #[test]
    fn test_lookup_not_found() {
        let segment = make_test_segment(1, &[b"hello"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);

        let unknown_hash = blake3_hash(b"unknown");
        assert!(catalog.lookup(&unknown_hash).is_none());
    }

    #[test]
    fn test_remove_segment() {
        let segment = make_test_segment(1, &[b"hello", b"world"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);
        assert_eq!(catalog.len(), 2);

        catalog.remove_segment(1);
        assert!(catalog.is_empty());
    }

    #[test]
    fn test_multiple_segments() {
        let segment1 = make_test_segment(1, &[b"hello"]);
        let segment2 = make_test_segment(2, &[b"world"]);

        let mut catalog = SegmentCatalog::new(CatalogConfig::default());
        catalog.index_segment(&segment1);
        catalog.index_segment(&segment2);

        let hash1 = blake3_hash(b"hello");
        let hash2 = blake3_hash(b"world");

        let loc1 = catalog.lookup(&hash1).expect("hash1 should exist");
        assert_eq!(loc1.segment_id, 1);

        let loc2 = catalog.lookup(&hash2).expect("hash2 should exist");
        assert_eq!(loc2.segment_id, 2);
    }

    #[test]
    fn test_len_after_index() {
        let segment = make_test_segment(1, &[b"a", b"b", b"c"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);
        assert_eq!(catalog.len(), 3);
    }

    #[test]
    fn test_is_empty_true() {
        let catalog = SegmentCatalog::new(CatalogConfig::default());
        assert!(catalog.is_empty());
    }

    #[test]
    fn test_is_empty_false() {
        let segment = make_test_segment(1, &[b"hello"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);
        assert!(!catalog.is_empty());
    }

    #[test]
    fn test_indexed_segment_ids() {
        let segment1 = make_test_segment(5, &[b"a"]);
        let segment2 = make_test_segment(2, &[b"b"]);
        let segment3 = make_test_segment(9, &[b"c"]);

        let mut catalog = SegmentCatalog::new(CatalogConfig::default());
        catalog.index_segment(&segment1);
        catalog.index_segment(&segment2);
        catalog.index_segment(&segment3);

        let ids = catalog.indexed_segment_ids();
        assert_eq!(ids, vec![2, 5, 9]);
    }

    #[test]
    fn test_chunks_in_segment() {
        let segment1 = make_test_segment(1, &[b"a", b"b"]);
        let segment2 = make_test_segment(2, &[b"c"]);

        let mut catalog = SegmentCatalog::new(CatalogConfig::default());
        catalog.index_segment(&segment1);
        catalog.index_segment(&segment2);

        assert_eq!(catalog.chunks_in_segment(1), 2);
        assert_eq!(catalog.chunks_in_segment(2), 1);
    }

    #[test]
    fn test_clear() {
        let segment = make_test_segment(1, &[b"hello"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);
        assert!(!catalog.is_empty());

        catalog.clear();
        assert!(catalog.is_empty());
    }

    #[test]
    fn test_location_fields() {
        let segment = make_test_segment(42, &[b"test data here"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);

        let hash = blake3_hash(b"test data here");
        let loc = catalog.lookup(&hash).expect("should exist");

        assert_eq!(loc.segment_id, 42);
        assert_eq!(loc.offset, 0);
        assert_eq!(loc.size, 14);
        assert_eq!(loc.original_size, 14);
    }

    #[test]
    fn test_max_entries_eviction() {
        let config = CatalogConfig { max_entries: 1 };
        let mut catalog = SegmentCatalog::new(config);

        let data1 = b"first";
        let data2 = b"second";

        let segment1 = make_test_segment(1, &[data1]);
        let segment2 = make_test_segment(2, &[data2]);

        catalog.index_segment(&segment1);
        assert_eq!(catalog.len(), 1);

        catalog.index_segment(&segment2);
        assert_eq!(catalog.len(), 1);

        let hash1 = blake3_hash(data1);
        let hash2 = blake3_hash(data2);

        assert!(catalog.lookup(&hash1).is_none(), "first should be evicted");
        assert!(catalog.lookup(&hash2).is_some(), "second should exist");
    }

    #[test]
    fn test_remove_nonexistent_segment() {
        let segment = make_test_segment(1, &[b"hello"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);
        let len_before = catalog.len();

        catalog.remove_segment(999);
        assert_eq!(catalog.len(), len_before);
    }

    #[test]
    fn test_index_same_hash_twice() {
        let data = b"duplicate";

        let segment1 = make_test_segment(1, &[data]);
        let segment2 = make_test_segment(2, &[data]);

        let mut catalog = SegmentCatalog::new(CatalogConfig::default());
        catalog.index_segment(&segment1);
        catalog.index_segment(&segment2);

        let hash = blake3_hash(data);
        let loc = catalog.lookup(&hash).expect("should exist");

        assert_eq!(loc.segment_id, 2, "second write should win");
    }

    #[test]
    fn test_zero_max_entries_unlimited() {
        let config = CatalogConfig { max_entries: 0 };
        let mut catalog = SegmentCatalog::new(config);

        for i in 0..100 {
            let data = vec![i as u8; 10];
            let segment = make_test_segment(i, &[&data]);
            catalog.index_segment(&segment);
        }

        assert_eq!(catalog.len(), 100);
    }

    #[test]
    fn test_indexed_segment_ids_sorted() {
        let segment1 = make_test_segment(100, &[b"a"]);
        let segment2 = make_test_segment(1, &[b"b"]);
        let segment3 = make_test_segment(50, &[b"c"]);

        let mut catalog = SegmentCatalog::new(CatalogConfig::default());
        catalog.index_segment(&segment1);
        catalog.index_segment(&segment2);
        catalog.index_segment(&segment3);

        let ids = catalog.indexed_segment_ids();
        assert_eq!(ids, vec![1, 50, 100], "IDs should be sorted ascending");
    }

    #[test]
    fn test_chunks_in_segment_zero() {
        let segment = make_test_segment(1, &[b"hello"]);
        let mut catalog = SegmentCatalog::new(CatalogConfig::default());

        catalog.index_segment(&segment);

        assert_eq!(catalog.chunks_in_segment(999), 0);
    }

    #[test]
    fn test_remove_partial() {
        let segment1 = make_test_segment(1, &[b"a", b"b"]);
        let segment2 = make_test_segment(2, &[b"c", b"d"]);

        let mut catalog = SegmentCatalog::new(CatalogConfig::default());
        catalog.index_segment(&segment1);
        catalog.index_segment(&segment2);

        assert_eq!(catalog.len(), 4);

        catalog.remove_segment(1);

        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog.chunks_in_segment(1), 0);
        assert_eq!(catalog.chunks_in_segment(2), 2);

        let hash_c = blake3_hash(b"c");
        let hash_d = blake3_hash(b"d");
        assert!(catalog.lookup(&hash_c).is_some());
        assert!(catalog.lookup(&hash_d).is_some());
    }
}
