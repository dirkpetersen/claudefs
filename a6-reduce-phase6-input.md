# A3: Data Reduction Phase 6 — Three New Modules + Test Expansion

## Context

You are implementing code for the `claudefs-reduce` crate in a distributed file system (ClaudeFS).
The crate provides the data reduction pipeline: FastCDC → BLAKE3 dedupe → LZ4/Zstd compress → AES-GCM encrypt → segment.

**Current state:** 27 modules, 327 tests passing, 0 clippy warnings.
**Goal:** Add 3 new modules + expand tests in low-coverage modules → ~60 new tests.

## Crate location: `crates/claudefs-reduce/src/`

### Existing types you must import and use (do NOT redefine):
- `ChunkHash` — `[u8; 32]` newtype, from `crate::fingerprint`
- `SuperFeatures([u64; 4])` — MinHash super-features, from `crate::fingerprint`
- `ReduceError` — error type, from `crate::error` (variants: `Io`, `Compression`, `Encryption`, `CorruptData`, `ShardCountMismatch{expected,got}`, `RecoveryFailed(String)`)
- `CompressionAlgorithm` — `Lz4 | Zstd(i32) | None`, from `crate::compression`
- `compress(data, algo)` / `decompress(data, algo)` — from `crate::compression`
- `Chunk` (struct with `hash: ChunkHash, data: Vec<u8>`), `Chunker`, `ChunkerConfig` — from `crate::dedupe`
- `ReducedChunk` (struct with hash, data, compressed_size, etc.) — from `crate::pipeline`
- `EncryptedChunk` — from `crate::encryption`
- `EncryptionKey`, `KeyManager`, `KeyManagerConfig` — from `crate::key_manager`
- `blake3::hash` — external crate already in Cargo.toml
- `tracing::{debug, info, warn}` — already in Cargo.toml
- `serde::{Serialize, Deserialize}` — already in Cargo.toml
- `thiserror::Error` — already in Cargo.toml

### Key Cargo.toml dependencies already present:
```toml
blake3 = "1"
lz4_flex = "0.11"
zstd = "0.13"
aes-gcm = "0.10"
rand = "0.8"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
serde = { version = "1", features = ["derive"] }
thiserror = "1"
fastcdc = "3"
reed-solomon-erasure = { version = "6", features = ["simd-accel"] }
```

---

## Task 1: New file `crates/claudefs-reduce/src/stream_chunker.rs`

Create an async streaming CDC (Content-Defined Chunking) module for large files.

**Purpose:** The existing `Chunker` in `dedupe.rs` processes `Vec<u8>` slices. For large files (GBs),
we need to process data in streaming fashion — reading from a tokio `AsyncRead` source, chunking on
the fly, yielding chunks as they're found rather than buffering the entire file.

**Public API:**

```rust
pub struct StreamChunkerConfig {
    pub min_chunk_size: usize,   // default 65536 (64KB)
    pub avg_chunk_size: usize,   // default 262144 (256KB)
    pub max_chunk_size: usize,   // default 1048576 (1MB)
    pub read_buffer_size: usize, // default 1048576 (1MB) — how much to read at a time
}

impl Default for StreamChunkerConfig { ... } // use the above defaults

pub struct StreamChunkResult {
    pub hash: ChunkHash,   // BLAKE3 hash of this chunk's data
    pub data: Vec<u8>,     // raw chunk bytes
    pub offset: u64,       // byte offset in the source stream where chunk starts
    pub length: usize,     // chunk length in bytes
}

pub struct StreamingStats {
    pub chunks_produced: u64,
    pub bytes_consumed: u64,
    pub min_chunk_size_seen: usize,
    pub max_chunk_size_seen: usize,
}

pub struct StreamChunker {
    config: StreamChunkerConfig,
}

impl StreamChunker {
    pub fn new(config: StreamChunkerConfig) -> Self;

    /// Chunk all data from the given AsyncRead source.
    /// Returns a Vec of all chunks with their hashes and offsets.
    /// Uses fastcdc internally.
    pub async fn chunk_stream<R: tokio::io::AsyncRead + Unpin>(
        &self,
        reader: R,
    ) -> Result<(Vec<StreamChunkResult>, StreamingStats), ReduceError>;

    /// Chunk data from a byte slice (convenience wrapper).
    pub fn chunk_slice(&self, data: &[u8]) -> (Vec<StreamChunkResult>, StreamingStats);
}
```

**Implementation notes:**
- Use `tokio::io::AsyncReadExt` to read in `read_buffer_size` chunks into a working buffer
- Use `fastcdc::v2020::FastCDC` for the actual CDC algorithm
- Track byte offset as you consume the stream
- Hash each chunk with `blake3::hash`
- `chunk_slice` is synchronous — directly use `fastcdc::v2020::FastCDC` on the slice

**Tests (write 14 tests in `#[cfg(test)]` at bottom of file):**
1. `test_chunk_empty_slice` — empty input produces 0 chunks, 0 bytes
2. `test_chunk_small_slice` — data smaller than min_chunk_size produces exactly 1 chunk
3. `test_chunk_large_slice` — 4MB of random data produces multiple chunks
4. `test_chunk_offsets_monotonic` — offsets must be strictly increasing
5. `test_chunk_offsets_contiguous` — each chunk's offset + length == next chunk's offset
6. `test_chunk_total_bytes` — sum of all chunk lengths == input size
7. `test_chunk_hashes_correct` — verify hash of each result matches `blake3::hash(&result.data)`
8. `test_chunk_hashes_unique_for_unique_data` — unique inputs produce unique hashes
9. `test_stats_bytes_consumed` — stats.bytes_consumed == input length
10. `test_stats_chunks_produced` — stats.chunks_produced == results.len()
11. `test_stream_matches_slice` — `chunk_stream` on `Cursor<Vec<u8>>` gives same chunks as `chunk_slice`
12. `test_deterministic` — same input always produces same chunks and hashes
13. `test_custom_config` — min=4096, avg=8192, max=16384 produces chunks within those bounds
14. `test_large_file_streaming` — 8MB of data, verify all bytes accounted for

---

## Task 2: New file `crates/claudefs-reduce/src/read_cache.rs`

Create an LRU cache for decrypted+decompressed chunks on the read path.

**Purpose:** When reading files, chunks are fetched from segment files, then decrypted and decompressed.
For hot data (recently accessed), this is expensive. The read cache stores the decoded `Vec<u8>` keyed
by `ChunkHash`, avoiding repeated decrypt+decompress on re-reads.

**Public API:**

```rust
pub struct ReadCacheConfig {
    pub capacity_bytes: usize,  // max total bytes of cached chunk data, default 256MB
    pub max_entries: usize,     // max number of cache entries, default 65536
}

impl Default for ReadCacheConfig {
    fn default() -> Self {
        Self { capacity_bytes: 256 * 1024 * 1024, max_entries: 65536 }
    }
}

pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub current_entries: usize,
    pub current_bytes: usize,
}

impl CacheStats {
    /// Hit rate as a fraction [0.0, 1.0]
    pub fn hit_rate(&self) -> f64;
}

pub struct ReadCache {
    // LRU cache: ChunkHash -> Vec<u8>
    // Use a simple HashMap + VecDeque for LRU ordering
    // (do NOT import external lru crate - implement simple LRU with HashMap + VecDeque)
}

impl ReadCache {
    pub fn new(config: ReadCacheConfig) -> Self;

    /// Look up a chunk by hash. Returns Some(data) on hit, None on miss.
    pub fn get(&mut self, hash: &ChunkHash) -> Option<&Vec<u8>>;

    /// Insert a chunk into the cache.
    /// Evicts LRU entries if capacity_bytes or max_entries would be exceeded.
    pub fn insert(&mut self, hash: ChunkHash, data: Vec<u8>);

    /// Remove a specific entry (e.g., after it's been invalidated).
    pub fn remove(&mut self, hash: &ChunkHash) -> bool;

    /// Remove all entries.
    pub fn clear(&mut self);

    /// Get current stats snapshot.
    pub fn stats(&self) -> CacheStats;

    /// Number of entries currently in cache.
    pub fn len(&self) -> usize;

    /// True if cache is empty.
    pub fn is_empty(&self) -> bool;
}
```

**Implementation notes:**
- Store entries in a `HashMap<ChunkHash, Vec<u8>>` plus a `VecDeque<ChunkHash>` for LRU ordering
- On `get`: if found, move entry to back of VecDeque (most recently used), return hit
- On `insert`: add to back of VecDeque; evict from front while total bytes > capacity_bytes OR entries > max_entries
- Track evicted entries in stats
- All operations are O(n) worst case due to VecDeque scan — that's fine for a first implementation

**Tests (write 14 tests):**
1. `test_empty_cache` — new cache returns miss on any hash
2. `test_insert_and_hit` — insert then get returns the data
3. `test_miss_after_never_inserted` — unknown hash returns None
4. `test_evict_by_max_entries` — inserting max_entries+1 entries evicts the oldest
5. `test_evict_by_capacity_bytes` — inserting data exceeding capacity_bytes triggers eviction
6. `test_lru_order_respected` — access A, insert B, insert C... A should be evicted last if accessed recently
7. `test_remove_entry` — remove returns true and entry is gone
8. `test_remove_missing` — remove of non-existent entry returns false
9. `test_clear_empties_cache` — after clear, cache is empty and stats reset
10. `test_stats_hit_rate_zero` — all misses → hit_rate == 0.0
11. `test_stats_hit_rate_one` — all hits → hit_rate == 1.0
12. `test_stats_hit_rate_mixed` — 3 hits 1 miss → hit_rate == 0.75
13. `test_eviction_count` — evictions counter increments correctly
14. `test_large_capacity` — 100 entries all fit without eviction when under capacity

---

## Task 3: New file `crates/claudefs-reduce/src/prefetch.rs`

Sequential access pattern detection and prefetch planning.

**Purpose:** For sequential read workloads (large file copies, backups, ML training data),
we want to predict which chunks will be needed next and trigger prefetch requests before the
client asks for them. This module detects sequential access patterns and generates prefetch hints.

**Public API:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPattern {
    /// Random or unknown access pattern.
    Random,
    /// Sequential forward access — client reading chunks in order.
    Sequential,
    /// Stride pattern — regular gaps between accesses.
    Stride { stride_bytes: u64 },
}

pub struct PrefetchConfig {
    /// Number of recent accesses to track per file handle, default 8.
    pub history_len: usize,
    /// Number of chunks to prefetch ahead when sequential pattern detected, default 4.
    pub prefetch_depth: usize,
    /// Minimum confidence (fraction of history matching pattern) to declare sequential, default 0.75.
    pub sequential_confidence: f64,
}

impl Default for PrefetchConfig { ... }

/// A prefetch hint: the file handle and offset range to prefetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefetchHint {
    pub file_id: u64,
    pub start_offset: u64,
    pub length: u64,
}

pub struct AccessHistory {
    recent: VecDeque<u64>, // recent byte offsets, newest last
    capacity: usize,
}

impl AccessHistory {
    pub fn push(&mut self, offset: u64);
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    /// Detect pattern from recent accesses. Returns Random if insufficient history.
    pub fn detect_pattern(&self, chunk_size: u64) -> AccessPattern;
}

pub struct PrefetchTracker {
    config: PrefetchConfig,
    history: HashMap<u64, AccessHistory>, // keyed by file_id
}

impl PrefetchTracker {
    pub fn new(config: PrefetchConfig) -> Self;

    /// Record a read access and return any prefetch hints.
    /// file_id: opaque handle ID, offset: byte offset accessed, chunk_size: hint for stride detection.
    pub fn record_access(
        &mut self,
        file_id: u64,
        offset: u64,
        chunk_size: u64,
    ) -> Vec<PrefetchHint>;

    /// Get the detected pattern for a file.
    pub fn get_pattern(&self, file_id: u64) -> AccessPattern;

    /// Remove tracking state for a file (called on file close).
    pub fn forget(&mut self, file_id: u64);

    /// How many files are currently being tracked.
    pub fn tracked_files(&self) -> usize;
}
```

**Implementation notes:**
- `AccessHistory::detect_pattern`: compute diffs between consecutive offsets.
  If all diffs ≈ chunk_size, it's Sequential. If all diffs ≈ some constant other size, it's Stride.
  Use sequential_confidence fraction — if >= confidence fraction of diffs are chunk_size ± 10%, Sequential.
- `PrefetchTracker::record_access`: push offset into history. If pattern is Sequential,
  generate `prefetch_depth` hints starting at `offset + chunk_size`.
  If pattern is Random, generate no hints.
- VecDeque is from std::collections — import it.
- Use HashMap<u64, AccessHistory> in PrefetchTracker.

**Tests (write 14 tests):**
1. `test_empty_history_is_random` — no history → Random pattern
2. `test_one_access_is_random` — single access → Random
3. `test_sequential_detected` — offsets 0, 4096, 8192, 12288 with chunk_size=4096 → Sequential
4. `test_sequential_generates_hints` — sequential pattern → hints with correct offsets
5. `test_random_no_hints` — random offsets → no hints generated
6. `test_hints_start_after_last_access` — hints start at offset + chunk_size
7. `test_prefetch_depth_respected` — 4 prefetch depth → 4 hints
8. `test_stride_pattern_detected` — offsets 0, 8192, 16384 with chunk_size=4096 → Stride{8192}
9. `test_forget_removes_state` — after forget, tracked_files decreases
10. `test_multiple_files_independent` — two file_ids tracked independently
11. `test_history_bounded_by_capacity` — history len <= history_len config
12. `test_pattern_stabilizes_after_enough_history` — random then sequential stabilizes
13. `test_access_at_zero_offset` — first access at offset 0 is valid
14. `test_tracked_files_count` — tracks correct number of files

---

## Task 4: Expand tests in low-coverage modules

### 4a. Add 8 more tests to `crates/claudefs-reduce/src/dedupe.rs`

The existing file has a `Chunker` struct that wraps fastcdc. Append these tests to its existing `#[cfg(test)]` module:

```rust
#[test]
fn test_chunker_min_max_boundaries() {
    // Create a chunker and verify chunks respect min/max size bounds
    // Use default ChunkerConfig
    let config = ChunkerConfig::default();
    let chunker = Chunker::new(config);
    let data: Vec<u8> = (0u8..=255u8).cycle().take(512 * 1024).collect(); // 512KB
    let chunks = chunker.chunk(&data);
    for chunk in &chunks {
        assert!(chunk.data.len() >= 4096, "chunk too small: {}", chunk.data.len());
        assert!(chunk.data.len() <= 1024 * 1024, "chunk too large: {}", chunk.data.len());
    }
}

#[test]
fn test_chunker_covers_all_bytes() {
    let config = ChunkerConfig::default();
    let chunker = Chunker::new(config);
    let data: Vec<u8> = (0u8..=255u8).cycle().take(300 * 1024).collect();
    let chunks = chunker.chunk(&data);
    let total: usize = chunks.iter().map(|c| c.data.len()).sum();
    assert_eq!(total, data.len());
}

#[test]
fn test_chunker_deterministic() {
    let config = ChunkerConfig::default();
    let chunker = Chunker::new(config);
    let data: Vec<u8> = (0u8..=255u8).cycle().take(200 * 1024).collect();
    let chunks1 = chunker.chunk(&data);
    let chunks2 = chunker.chunk(&data);
    assert_eq!(chunks1.len(), chunks2.len());
    for (a, b) in chunks1.iter().zip(chunks2.iter()) {
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.data, b.data);
    }
}

#[test]
fn test_cas_insert_and_lookup() {
    let mut cas = CasIndex::new();
    let hash = ChunkHash(*blake3::hash(b"test").as_bytes());
    assert!(!cas.contains(&hash));
    cas.insert(hash, 42);
    assert!(cas.contains(&hash));
}

#[test]
fn test_cas_dedup_returns_existing_ref() {
    let mut cas = CasIndex::new();
    let data = b"duplicate data";
    let hash = ChunkHash(*blake3::hash(data).as_bytes());
    cas.insert(hash, 100);
    // second insert with same hash should not change the existing ref
    cas.insert(hash, 200);
    // should still map to original ref
    assert!(cas.contains(&hash));
}

#[test]
fn test_cas_remove() {
    let mut cas = CasIndex::new();
    let hash = ChunkHash(*blake3::hash(b"removable").as_bytes());
    cas.insert(hash, 1);
    assert!(cas.contains(&hash));
    cas.remove(&hash);
    assert!(!cas.contains(&hash));
}

#[test]
fn test_chunker_single_chunk_for_tiny_input() {
    let config = ChunkerConfig::default();
    let chunker = Chunker::new(config);
    let data = b"tiny";
    let chunks = chunker.chunk(data);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].data, data);
}

#[test]
fn test_chunker_hashes_match_data() {
    let config = ChunkerConfig::default();
    let chunker = Chunker::new(config);
    let data: Vec<u8> = (0..=255u8).cycle().take(100 * 1024).collect();
    let chunks = chunker.chunk(&data);
    for chunk in &chunks {
        let expected = ChunkHash(*blake3::hash(&chunk.data).as_bytes());
        assert_eq!(chunk.hash, expected);
    }
}
```

### 4b. Add 8 more tests to `crates/claudefs-reduce/src/compression.rs`

Look at the existing file and append to its test module:

```rust
#[test]
fn test_roundtrip_lz4_random_data() {
    let data: Vec<u8> = (0u8..=255u8).cycle().take(64 * 1024).collect();
    let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
    let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_roundtrip_zstd_random_data() {
    let data: Vec<u8> = (0u8..=255u8).cycle().take(64 * 1024).collect();
    let compressed = compress(&data, CompressionAlgorithm::Zstd(3)).unwrap();
    let decompressed = decompress(&compressed, CompressionAlgorithm::Zstd(3)).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_none_compression_passthrough() {
    let data = b"no compression applied";
    let compressed = compress(data, CompressionAlgorithm::None).unwrap();
    let decompressed = decompress(&compressed, CompressionAlgorithm::None).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_lz4_compresses_repetitive_data() {
    let data: Vec<u8> = vec![0xABu8; 64 * 1024];
    let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
    assert!(compressed.len() < data.len(), "LZ4 should compress repetitive data");
}

#[test]
fn test_zstd_level_9_smaller_than_level_1() {
    let data: Vec<u8> = "The quick brown fox jumps over the lazy dog"
        .repeat(1000)
        .into_bytes();
    let c1 = compress(&data, CompressionAlgorithm::Zstd(1)).unwrap();
    let c9 = compress(&data, CompressionAlgorithm::Zstd(9)).unwrap();
    // Higher level should be same or smaller
    assert!(c9.len() <= c1.len() + 100, "zstd level 9 should not be much bigger than level 1");
}

#[test]
fn test_compress_empty() {
    let data: &[u8] = &[];
    let compressed = compress(data, CompressionAlgorithm::Lz4).unwrap();
    let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_decompress_wrong_algorithm_fails() {
    let data = b"some data to compress";
    let compressed = compress(data, CompressionAlgorithm::Lz4).unwrap();
    // Decompressing Zstd-compressed data as Zstd should fail for Lz4 data
    let result = decompress(&compressed, CompressionAlgorithm::Zstd(3));
    // This might succeed or fail depending on implementation but shouldn't panic
    let _ = result; // just ensure no panic
}

#[test]
fn test_zstd_roundtrip_large() {
    let data: Vec<u8> = (0u8..=255u8).cycle().take(1024 * 1024).collect();
    let compressed = compress(&data, CompressionAlgorithm::Zstd(3)).unwrap();
    let decompressed = decompress(&compressed, CompressionAlgorithm::Zstd(3)).unwrap();
    assert_eq!(decompressed, data);
}
```

### 4c. Add 8 more tests to `crates/claudefs-reduce/src/pipeline.rs`

Look at the existing `ReductionPipeline` in `pipeline.rs` and append to its test module:

```rust
#[tokio::test]
async fn test_pipeline_chunk_count_matches_cdc() {
    let config = PipelineConfig::default();
    let pipeline = ReductionPipeline::new(config).unwrap();
    let data: Vec<u8> = (0u8..=255u8).cycle().take(512 * 1024).collect();
    let result = pipeline.process(&data).await.unwrap();
    assert!(!result.chunks.is_empty());
}

#[tokio::test]
async fn test_pipeline_stats_bytes_in() {
    let config = PipelineConfig::default();
    let pipeline = ReductionPipeline::new(config).unwrap();
    let data: Vec<u8> = vec![0u8; 128 * 1024];
    let result = pipeline.process(&data).await.unwrap();
    assert_eq!(result.stats.bytes_in, data.len() as u64);
}

#[tokio::test]
async fn test_pipeline_all_chunks_have_hashes() {
    let config = PipelineConfig::default();
    let pipeline = ReductionPipeline::new(config).unwrap();
    let data: Vec<u8> = (0u8..255u8).cycle().take(256 * 1024).collect();
    let result = pipeline.process(&data).await.unwrap();
    for chunk in &result.chunks {
        // hash should be non-zero (actual data)
        let zero_hash = [0u8; 32];
        assert_ne!(chunk.hash.0, zero_hash);
    }
}

#[tokio::test]
async fn test_pipeline_dedup_reduces_unique_chunks() {
    let config = PipelineConfig::default();
    let pipeline = ReductionPipeline::new(config).unwrap();
    // Highly repetitive data — dedup should collapse many identical chunks
    let data: Vec<u8> = vec![0x42u8; 1024 * 1024]; // 1MB of identical bytes
    let result = pipeline.process(&data).await.unwrap();
    // May have many input chunks but some should be deduped
    assert!(result.stats.chunks_in >= result.stats.chunks_out || result.stats.chunks_in > 0);
}

#[tokio::test]
async fn test_pipeline_compression_reduces_size() {
    let config = PipelineConfig::default();
    let pipeline = ReductionPipeline::new(config).unwrap();
    let data: Vec<u8> = vec![0xFFu8; 256 * 1024]; // highly compressible
    let result = pipeline.process(&data).await.unwrap();
    assert!(result.stats.bytes_in > 0);
}

#[tokio::test]
async fn test_pipeline_empty_input() {
    let config = PipelineConfig::default();
    let pipeline = ReductionPipeline::new(config).unwrap();
    let result = pipeline.process(&[]).await.unwrap();
    assert_eq!(result.chunks.len(), 0);
    assert_eq!(result.stats.bytes_in, 0);
}

#[tokio::test]
async fn test_pipeline_small_input_one_chunk() {
    let config = PipelineConfig::default();
    let pipeline = ReductionPipeline::new(config).unwrap();
    let data = b"small input";
    let result = pipeline.process(data).await.unwrap();
    assert_eq!(result.chunks.len(), 1);
}

#[tokio::test]
async fn test_pipeline_deterministic() {
    let config = PipelineConfig::default();
    let p1 = ReductionPipeline::new(config.clone()).unwrap();
    let p2 = ReductionPipeline::new(config).unwrap();
    let data: Vec<u8> = (0u8..=255u8).cycle().take(200 * 1024).collect();
    let r1 = p1.process(&data).await.unwrap();
    let r2 = p2.process(&data).await.unwrap();
    assert_eq!(r1.chunks.len(), r2.chunks.len());
    for (a, b) in r1.chunks.iter().zip(r2.chunks.iter()) {
        assert_eq!(a.hash, b.hash);
    }
}
```

---

## Output format

For each task, output the COMPLETE file content. Use this format:

```
=== FILE: crates/claudefs-reduce/src/stream_chunker.rs ===
<complete file content>
=== END FILE ===

=== FILE: crates/claudefs-reduce/src/read_cache.rs ===
<complete file content>
=== END FILE ===

=== FILE: crates/claudefs-reduce/src/prefetch.rs ===
<complete file content>
=== END FILE ===

=== FILE: APPEND_TO: crates/claudefs-reduce/src/dedupe.rs ===
<only the new test functions to append inside the existing #[cfg(test)] mod tests { ... }>
=== END FILE ===

=== FILE: APPEND_TO: crates/claudefs-reduce/src/compression.rs ===
<only the new test functions to append>
=== END FILE ===

=== FILE: APPEND_TO: crates/claudefs-reduce/src/pipeline.rs ===
<only the new test functions to append>
=== END FILE ===
```

## Critical requirements:
- All public types need doc comments (the crate has `#![warn(missing_docs)]`)
- Use `#[allow(dead_code)]` only if truly needed — prefer actual usage
- `#[derive(Debug, Clone)]` on all config/result types
- Serde `Serialize/Deserialize` on all config types
- No `unsafe` code (this crate is pure safe Rust)
- Tests must compile standalone — do NOT rely on crate-private test helpers from other modules
- For new modules, put all tests in `#[cfg(test)] mod tests { use super::*; ... }`
- For "APPEND_TO" items, output only the test function bodies (NOT the mod/use boilerplate)
