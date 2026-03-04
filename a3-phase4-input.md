# A3 Phase 4: Segment Read Path & Catalog

You are implementing Phase 4 of the `claudefs-reduce` crate for ClaudeFS, a distributed POSIX file system. This phase adds the segment read path and a segment catalog for chunk lookups.

## Context

The `claudefs-reduce` crate handles data reduction: chunking, deduplication, compression, and encryption. Currently we have:

- `segment.rs` — writes chunks into 2MB segments (the write path), each segment has a CRC32C checksum
- `write_path.rs` — orchestrates chunk → fingerprint → dedupe → compress → encrypt → segment
- `fingerprint.rs` — BLAKE3 hashing for content-addressable storage
- `error.rs` — `ReduceError` enum with `ChecksumMismatch`, `ChecksumMissing`, `NotFound`, etc.

**The gap**: Once a segment is written and its bytes are stored, there is no way to read a specific chunk back out by its BLAKE3 hash. Phase 4 fills this gap with two new modules.

## Key Existing Types (for reference)

```rust
// From fingerprint.rs
pub type ChunkHash = [u8; 32];  // BLAKE3 hash

// From segment.rs
pub struct Segment {
    pub id: u64,
    pub entries: Vec<SegmentEntry>,
    pub payload: Vec<u8>,
    pub sealed: bool,
    pub created_at_secs: u64,
    pub payload_checksum: Option<DataChecksum>,
}

pub struct SegmentEntry {
    pub hash: ChunkHash,
    pub offset_in_segment: u32,
    pub payload_size: u32,
    pub original_size: u32,
}

// From error.rs
pub enum ReduceError {
    NotFound(String),         // chunk not found
    ChecksumMismatch,         // payload checksum failed
    ChecksumMissing,          // no checksum stored
    // ... others
}
```

## Module 1: `segment_reader.rs`

Implement a `SegmentReader` that extracts chunks from a single sealed `Segment` by hash.

```rust
use crate::fingerprint::ChunkHash;
use crate::segment::Segment;
use crate::error::ReduceError;

/// Reads individual chunks out of a sealed Segment by their BLAKE3 hash.
pub struct SegmentReader<'a> {
    segment: &'a Segment,
}

impl<'a> SegmentReader<'a> {
    /// Create a new reader for the given segment.
    pub fn new(segment: &'a Segment) -> Self;

    /// Look up a chunk by its BLAKE3 hash.
    /// Returns a slice of the payload bytes for that chunk.
    /// Returns `ReduceError::NotFound` if no entry with that hash exists.
    /// Returns `ReduceError::InvalidInput` if the entry's offset+size is out of bounds.
    pub fn get_chunk(&self, hash: &ChunkHash) -> Result<&[u8], ReduceError>;

    /// Look up a chunk and copy it into an owned Vec.
    pub fn get_chunk_owned(&self, hash: &ChunkHash) -> Result<Vec<u8>, ReduceError>;

    /// Check if a chunk exists in this segment.
    pub fn contains(&self, hash: &ChunkHash) -> bool;

    /// Iterate over all (hash, payload) pairs in this segment.
    pub fn iter_chunks(&self) -> impl Iterator<Item = (&ChunkHash, &[u8])>;

    /// Number of chunks in this segment.
    pub fn len(&self) -> usize;

    /// True if this segment has no chunks.
    pub fn is_empty(&self) -> bool;
}
```

### Tests for `segment_reader.rs`

Write at least 12 unit tests covering:
1. `test_get_chunk_found` — create a segment with one chunk, reader returns it correctly
2. `test_get_chunk_not_found` — hash not in segment returns `ReduceError::NotFound`
3. `test_get_chunk_owned` — owned copy matches original payload
4. `test_contains_true` — chunk in segment → `contains` returns true
5. `test_contains_false` — unknown hash → `contains` returns false
6. `test_iter_chunks` — iterate yields all (hash, payload) pairs
7. `test_multiple_chunks` — segment with 3 chunks, look up each one correctly
8. `test_len` — `len()` returns number of entries
9. `test_is_empty_true` — reader on empty segment returns `is_empty() == true`
10. `test_is_empty_false` — reader on non-empty segment returns `is_empty() == false`
11. `test_get_chunk_correct_slice` — verify returned slice has correct length and content
12. `test_iter_chunks_count` — iter yields exactly as many items as `len()`

## Module 2: `segment_catalog.rs`

Implement a `SegmentCatalog` that maintains an in-memory index mapping chunk hashes to their location across multiple segments. This enables fast O(1) chunk lookups without scanning all segments.

```rust
use crate::fingerprint::ChunkHash;
use crate::error::ReduceError;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogConfig {
    /// Maximum number of entries. When exceeded, oldest entries are evicted (LRU).
    /// 0 means unlimited.
    pub max_entries: usize,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self { max_entries: 0 }
    }
}

/// In-memory index mapping chunk hashes to their location in segments.
/// Used for fast O(1) chunk lookups across many segments without scanning.
pub struct SegmentCatalog {
    config: CatalogConfig,
    index: HashMap<ChunkHash, ChunkLocation>,
    /// Ordered by insertion for eviction (segment_id order).
    insertion_order: Vec<ChunkHash>,
}

impl SegmentCatalog {
    /// Create a new catalog with the given config.
    pub fn new(config: CatalogConfig) -> Self;

    /// Index all entries from a sealed segment into the catalog.
    /// If max_entries is set and would be exceeded, oldest entries are evicted first.
    pub fn index_segment(&mut self, segment: &crate::segment::Segment);

    /// Look up the location of a chunk by hash.
    pub fn lookup(&self, hash: &ChunkHash) -> Option<&ChunkLocation>;

    /// Remove all entries that belong to a given segment.
    /// Used when a segment is garbage collected or replaced.
    pub fn remove_segment(&mut self, segment_id: u64);

    /// Total number of indexed chunks.
    pub fn len(&self) -> usize;

    /// True if no chunks are indexed.
    pub fn is_empty(&self) -> bool;

    /// Returns segment IDs currently indexed (sorted).
    pub fn indexed_segment_ids(&self) -> Vec<u64>;

    /// Returns number of chunks from a specific segment.
    pub fn chunks_in_segment(&self, segment_id: u64) -> usize;

    /// Clear all entries.
    pub fn clear(&mut self);
}
```

### Tests for `segment_catalog.rs`

Write at least 18 unit tests covering:
1. `test_index_segment_basic` — index one segment, then lookup its chunks
2. `test_lookup_not_found` — unknown hash returns None
3. `test_remove_segment` — after removing, chunks no longer in catalog
4. `test_multiple_segments` — index two segments, look up chunks from each
5. `test_len_after_index` — `len()` increases correctly after indexing
6. `test_is_empty_true` — new catalog is empty
7. `test_is_empty_false` — non-empty catalog
8. `test_indexed_segment_ids` — returns correct segment IDs
9. `test_chunks_in_segment` — correct count per segment
10. `test_clear` — after clear, catalog is empty
11. `test_location_fields` — ChunkLocation has correct segment_id, offset, size, original_size
12. `test_max_entries_eviction` — when max_entries is 1 and a second chunk is added, first is evicted
13. `test_remove_nonexistent_segment` — removing a segment_id not in catalog does nothing
14. `test_index_same_hash_twice` — duplicate hash in two segments — second write wins (overwrite)
15. `test_zero_max_entries_unlimited` — max_entries=0 allows unlimited entries
16. `test_indexed_segment_ids_sorted` — returned IDs are sorted ascending
17. `test_chunks_in_segment_zero` — segment not indexed returns 0
18. `test_remove_partial` — two segments indexed, remove one, other remains intact

## Integration Notes

- Both modules live in `crates/claudefs-reduce/src/`
- Use `use crate::fingerprint::ChunkHash;` — ChunkHash is `[u8; 32]`
- Use `use crate::error::ReduceError;`
- Use `use crate::segment::{Segment, SegmentEntry};` as needed
- Follow existing style: `thiserror`, `serde`, `tracing` crate
- `SegmentReader::get_chunk` should check bounds (offset + size <= payload.len()) before slicing
- `SegmentCatalog` uses `HashMap<ChunkHash, ChunkLocation>` for O(1) lookup; `ChunkHash` is `[u8; 32]` which implements `Hash + Eq`
- For eviction in `SegmentCatalog` with `max_entries > 0`: when `insertion_order.len() >= max_entries` before adding a new entry, pop the front of `insertion_order` and remove that hash from `index`

## Output Format

Output ONLY the complete Rust source code for two files:

1. First: complete `segment_reader.rs` with `// FILE: segment_reader.rs` header
2. Second: complete `segment_catalog.rs` with `// FILE: segment_catalog.rs` header

No explanation needed. Just the two complete Rust files with `#[cfg(test)]` modules included.

Make sure the code compiles with standard Rust (2021 edition). Use HashMap for the catalog index. For `iter_chunks` in SegmentReader, returning an iterator that yields `(&ChunkHash, &[u8])` pairs by mapping over `self.segment.entries`.
