//! Logical-to-physical block mapping for inode offset resolution.
//!
//! Files are split into content-defined chunks by FastCDC. Each chunk is stored
//! in a CAS by its hash. The block map tracks, for each file inode, the mapping
//! from logical byte ranges to CAS chunk hashes and their position within the
//! storage segment.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A logical byte range within a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalRange {
    /// Byte offset from the start of the file.
    pub offset: u64,
    /// Length in bytes.
    pub length: u64,
}

impl LogicalRange {
    /// Create a new logical range.
    pub fn new(offset: u64, length: u64) -> Self {
        Self { offset, length }
    }

    /// Returns the end offset (offset + length).
    pub fn end(&self) -> u64 {
        self.offset.saturating_add(self.length)
    }

    /// Returns true if this range overlaps with another range.
    pub fn overlaps(&self, other: &LogicalRange) -> bool {
        self.offset < other.end() && other.offset < self.end()
    }
}

/// A single chunk entry in a file's block map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockEntry {
    /// The logical byte range this chunk covers in the file.
    pub range: LogicalRange,
    /// BLAKE3 hash of the chunk content (CAS key).
    pub chunk_hash: [u8; 32],
    /// Byte offset within the storage segment where the chunk resides.
    pub chunk_offset: u64,
    /// Size of the chunk in bytes.
    pub chunk_size: u32,
}

/// Block map for a single inode, tracking logical-to-physical chunk mappings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMap {
    /// The inode this map belongs to.
    pub inode_id: u64,
    /// Sorted list of block entries (by range.offset).
    entries: Vec<BlockEntry>,
}

impl BlockMap {
    /// Create a new empty block map for the given inode.
    pub fn new(inode_id: u64) -> Self {
        Self {
            inode_id,
            entries: Vec::new(),
        }
    }

    /// Insert a block entry, maintaining sort order by range.offset.
    pub fn insert(&mut self, entry: BlockEntry) {
        let pos = self
            .entries
            .binary_search_by_key(&entry.range.offset, |e| e.range.offset);
        match pos {
            Ok(idx) => self.entries[idx] = entry,
            Err(idx) => self.entries.insert(idx, entry),
        }
    }

    /// Returns all entries that overlap with the given range.
    pub fn lookup_range(&self, range: &LogicalRange) -> Vec<&BlockEntry> {
        self.entries
            .iter()
            .filter(|e| e.range.overlaps(range))
            .collect()
    }

    /// Returns the total logical size (sum of all entry range lengths).
    pub fn total_logical_size(&self) -> u64 {
        self.entries.iter().map(|e| e.range.length).sum()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove entries completely covered by the given range.
    /// Used for truncate/overwrite operations.
    pub fn remove_range(&mut self, range: &LogicalRange) {
        self.entries.retain(|e| !is_fully_covered(&e.range, range));
    }
}

/// Returns true if `entry_range` is fully covered by `cover_range`.
fn is_fully_covered(entry_range: &LogicalRange, cover_range: &LogicalRange) -> bool {
    entry_range.offset >= cover_range.offset && entry_range.end() <= cover_range.end()
}

/// In-memory store of block maps indexed by inode ID.
#[derive(Debug, Default)]
pub struct BlockMapStore {
    maps: HashMap<u64, BlockMap>,
}

impl BlockMapStore {
    /// Create a new empty block map store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a reference to the block map for the given inode.
    pub fn get(&self, inode_id: u64) -> Option<&BlockMap> {
        self.maps.get(&inode_id)
    }

    /// Get a mutable reference to the block map for the given inode.
    pub fn get_mut(&mut self, inode_id: u64) -> Option<&mut BlockMap> {
        self.maps.get_mut(&inode_id)
    }

    /// Get or create a block map for the given inode.
    pub fn get_or_create(&mut self, inode_id: u64) -> &mut BlockMap {
        self.maps
            .entry(inode_id)
            .or_insert_with(|| BlockMap::new(inode_id))
    }

    /// Remove the block map for the given inode.
    pub fn remove(&mut self, inode_id: u64) -> Option<BlockMap> {
        self.maps.remove(&inode_id)
    }

    /// Returns the number of inodes tracked.
    pub fn len(&self) -> usize {
        self.maps.len()
    }

    /// Returns true if no inodes are tracked.
    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }

    /// Returns the total number of chunks across all block maps.
    pub fn total_chunks(&self) -> usize {
        self.maps.values().map(|m| m.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_range_end() {
        let range = LogicalRange::new(100, 50);
        assert_eq!(range.end(), 150);
    }

    #[test]
    fn logical_range_overlaps_yes() {
        let r1 = LogicalRange::new(0, 100);
        let r2 = LogicalRange::new(50, 100);
        assert!(r1.overlaps(&r2));
        assert!(r2.overlaps(&r1));
    }

    #[test]
    fn logical_range_overlaps_no() {
        let r1 = LogicalRange::new(0, 100);
        let r2 = LogicalRange::new(200, 100);
        assert!(!r1.overlaps(&r2));
        assert!(!r2.overlaps(&r1));
    }

    #[test]
    fn logical_range_overlaps_adjacent() {
        let r1 = LogicalRange::new(0, 100);
        let r2 = LogicalRange::new(100, 50);
        assert!(!r1.overlaps(&r2));
        assert!(!r2.overlaps(&r1));
    }

    #[test]
    fn block_map_new_is_empty() {
        let map = BlockMap::new(1);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn block_map_insert_single() {
        let mut map = BlockMap::new(1);
        let entry = BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        };
        map.insert(entry);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn block_map_insert_maintains_order() {
        let mut map = BlockMap::new(1);

        let entry2 = BlockEntry {
            range: LogicalRange::new(8192, 4096),
            chunk_hash: [2u8; 32],
            chunk_offset: 8192,
            chunk_size: 4096,
        };
        let entry1 = BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        };
        let entry3 = BlockEntry {
            range: LogicalRange::new(16384, 4096),
            chunk_hash: [3u8; 32],
            chunk_offset: 16384,
            chunk_size: 4096,
        };

        map.insert(entry2);
        map.insert(entry1);
        map.insert(entry3);

        assert_eq!(map.entries[0].range.offset, 0);
        assert_eq!(map.entries[1].range.offset, 8192);
        assert_eq!(map.entries[2].range.offset, 16384);
    }

    #[test]
    fn block_map_lookup_range_single_match() {
        let mut map = BlockMap::new(1);
        map.insert(BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        });

        let query = LogicalRange::new(100, 50);
        let results = map.lookup_range(&query);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn block_map_lookup_range_no_match() {
        let mut map = BlockMap::new(1);
        map.insert(BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        });

        let query = LogicalRange::new(5000, 100);
        let results = map.lookup_range(&query);
        assert!(results.is_empty());
    }

    #[test]
    fn block_map_lookup_range_multiple_matches() {
        let mut map = BlockMap::new(1);
        map.insert(BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        });
        map.insert(BlockEntry {
            range: LogicalRange::new(4096, 4096),
            chunk_hash: [2u8; 32],
            chunk_offset: 4096,
            chunk_size: 4096,
        });
        map.insert(BlockEntry {
            range: LogicalRange::new(8192, 4096),
            chunk_hash: [3u8; 32],
            chunk_offset: 8192,
            chunk_size: 4096,
        });

        let query = LogicalRange::new(3000, 4000);
        let results = map.lookup_range(&query);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn block_map_total_logical_size() {
        let mut map = BlockMap::new(1);
        map.insert(BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        });
        map.insert(BlockEntry {
            range: LogicalRange::new(4096, 8192),
            chunk_hash: [2u8; 32],
            chunk_offset: 4096,
            chunk_size: 8192,
        });

        assert_eq!(map.total_logical_size(), 12288);
    }

    #[test]
    fn block_map_remove_range_partial() {
        let mut map = BlockMap::new(1);
        map.insert(BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        });
        map.insert(BlockEntry {
            range: LogicalRange::new(4096, 4096),
            chunk_hash: [2u8; 32],
            chunk_offset: 4096,
            chunk_size: 4096,
        });
        map.insert(BlockEntry {
            range: LogicalRange::new(8192, 4096),
            chunk_hash: [3u8; 32],
            chunk_offset: 8192,
            chunk_size: 4096,
        });

        let remove = LogicalRange::new(4096, 4096);
        map.remove_range(&remove);

        assert_eq!(map.len(), 2);
        assert_eq!(map.entries[0].range.offset, 0);
        assert_eq!(map.entries[1].range.offset, 8192);
    }

    #[test]
    fn block_map_store_new_is_empty() {
        let store = BlockMapStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn block_map_store_get_or_create() {
        let mut store = BlockMapStore::new();
        let map = store.get_or_create(42);
        assert_eq!(map.inode_id, 42);
        assert!(map.is_empty());

        let map2 = store.get_or_create(42);
        assert_eq!(map2.inode_id, 42);
    }

    #[test]
    fn block_map_store_total_chunks() {
        let mut store = BlockMapStore::new();

        let map1 = store.get_or_create(1);
        map1.insert(BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        });
        map1.insert(BlockEntry {
            range: LogicalRange::new(4096, 4096),
            chunk_hash: [2u8; 32],
            chunk_offset: 4096,
            chunk_size: 4096,
        });

        let map2 = store.get_or_create(2);
        map2.insert(BlockEntry {
            range: LogicalRange::new(0, 8192),
            chunk_hash: [3u8; 32],
            chunk_offset: 0,
            chunk_size: 8192,
        });

        assert_eq!(store.total_chunks(), 3);
    }

    #[test]
    fn block_map_store_remove() {
        let mut store = BlockMapStore::new();
        store.get_or_create(1);
        store.get_or_create(2);

        assert_eq!(store.len(), 2);

        let removed = store.remove(1);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().inode_id, 1);
        assert_eq!(store.len(), 1);

        let not_found = store.remove(999);
        assert!(not_found.is_none());
    }

    #[test]
    fn block_map_len_and_is_empty() {
        let mut map = BlockMap::new(1);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);

        map.insert(BlockEntry {
            range: LogicalRange::new(0, 4096),
            chunk_hash: [1u8; 32],
            chunk_offset: 0,
            chunk_size: 4096,
        });
        assert!(!map.is_empty());
        assert_eq!(map.len(), 1);
    }
}
