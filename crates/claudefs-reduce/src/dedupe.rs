//! Content-defined chunking (FastCDC) and content-addressable deduplication

use crate::fingerprint::{blake3_hash, ChunkHash};
use bytes::Bytes;
use fastcdc::v2020::FastCDC;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A content-defined chunk produced by the FastCDC chunker
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Chunk content
    pub data: Bytes,
    /// BLAKE3 hash of the chunk content (CAS key)
    pub hash: ChunkHash,
    /// Byte offset of this chunk in the original data stream
    pub offset: u64,
}

/// Configuration for the FastCDC chunker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkerConfig {
    /// Minimum chunk size in bytes
    pub min_size: usize,
    /// Average (target) chunk size in bytes
    pub avg_size: usize,
    /// Maximum chunk size in bytes
    pub max_size: usize,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            min_size: 32 * 1024,
            avg_size: 64 * 1024,
            max_size: 512 * 1024,
        }
    }
}

/// Content-defined chunker using the FastCDC algorithm
pub struct Chunker {
    config: ChunkerConfig,
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker {
    /// Create a chunker with default sizes
    pub fn new() -> Self {
        Self {
            config: ChunkerConfig::default(),
        }
    }

    /// Create a chunker with custom configuration
    pub fn with_config(config: ChunkerConfig) -> Self {
        Self { config }
    }

    /// Chunk data using FastCDC and compute BLAKE3 hash per chunk.
    /// Concatenating all chunk.data bytes reconstructs the original data.
    pub fn chunk(&self, data: &[u8]) -> Vec<Chunk> {
        if data.is_empty() {
            return Vec::new();
        }
        FastCDC::new(
            data,
            self.config.min_size as u32,
            self.config.avg_size as u32,
            self.config.max_size as u32,
        )
        .map(|c| {
            let start = c.offset;
            let end = start + c.length;
            let chunk_bytes = Bytes::copy_from_slice(&data[start..end]);
            let hash = blake3_hash(&chunk_bytes);
            Chunk {
                data: chunk_bytes,
                hash,
                offset: start as u64,
            }
        })
        .collect()
    }
}

/// In-memory CAS index for Phase 1 deduplication.
/// Maps ChunkHash to reference count.
#[derive(Debug, Default)]
pub struct CasIndex {
    entries: HashMap<ChunkHash, u64>,
}

impl CasIndex {
    /// Create a new empty CAS index
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if this hash already exists (it's a duplicate)
    pub fn lookup(&self, hash: &ChunkHash) -> bool {
        self.entries.contains_key(hash)
    }

    /// Insert or increment refcount for a hash
    pub fn insert(&mut self, hash: ChunkHash) {
        *self.entries.entry(hash).or_insert(0) += 1;
    }

    /// Decrement refcount. Returns true if refcount hits 0 (block can be reclaimed).
    pub fn release(&mut self, hash: &ChunkHash) -> bool {
        match self.entries.get_mut(hash) {
            Some(count) if *count <= 1 => {
                self.entries.remove(hash);
                true
            }
            Some(count) => {
                *count -= 1;
                false
            }
            None => false,
        }
    }

    /// Get current reference count (0 if not present)
    pub fn refcount(&self, hash: &ChunkHash) -> u64 {
        self.entries.get(hash).copied().unwrap_or(0)
    }

    /// Number of unique chunks tracked
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Is the index empty?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn chunks_reassemble() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let chunker = Chunker::new();
        let chunks = chunker.chunk(&data);
        assert!(!chunks.is_empty());
        let reassembled: Vec<u8> = chunks.iter().flat_map(|c| c.data.iter().copied()).collect();
        assert_eq!(reassembled, data);
    }

    #[test]
    fn empty_data_no_chunks() {
        assert!(Chunker::new().chunk(&[]).is_empty());
    }

    #[test]
    fn cas_refcounting() {
        let mut cas = CasIndex::new();
        let hash = blake3_hash(b"test");
        assert!(!cas.lookup(&hash));
        cas.insert(hash);
        assert_eq!(cas.refcount(&hash), 1);
        cas.insert(hash);
        assert_eq!(cas.refcount(&hash), 2);
        assert!(!cas.release(&hash));
        assert_eq!(cas.refcount(&hash), 1);
        assert!(cas.release(&hash));
        assert!(!cas.lookup(&hash));
    }

    proptest! {
        #[test]
        fn prop_chunks_reassemble(data in prop::collection::vec(0u8..=255, 0..500_000)) {
            let chunks = Chunker::new().chunk(&data);
            let reassembled: Vec<u8> = chunks.iter().flat_map(|c| c.data.iter().copied()).collect();
            prop_assert_eq!(reassembled, data);
        }
    }
}
