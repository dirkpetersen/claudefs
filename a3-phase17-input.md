# A3 Phase 17: Object Store Bridge, Chunk Pool, Recovery Scanner

You are implementing Phase 17 of the A3 (Data Reduction) agent for ClaudeFS.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
1235 tests across 61 modules. Phase 17 goal: ~1325 tests.

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/object_store_bridge.rs`

Implement an object store bridge for S3-compatible tiered storage operations.

ClaudeFS uses S3 as backend (D5: cache mode). This module defines the trait and
in-memory implementation for testing the tiering logic.

Requirements:
- `ObjectKey` struct: `bucket: String`, `key: String`
  - `fn full_path(&self) -> String` → format!("{}/{}", bucket, key)
  - Derive Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize
- `ObjectMetadata` struct: `key: ObjectKey`, `size_bytes: u64`, `etag: String`, `uploaded_at_ms: u64`
  - Derive Debug, Clone
- `StoreResult` enum: `Uploaded`, `AlreadyExists`, `Deleted`, `NotFound`
  - Derive Debug, Clone, Copy, PartialEq, Eq
- `ObjectStoreStats` struct: `uploads: u64`, `downloads: u64`, `deletes: u64`, `bytes_uploaded: u64`, `bytes_downloaded: u64`
  - Derive Debug, Clone, Default
- `MemoryObjectStore` struct (in-memory implementation for testing):
  - `fn new() -> Self`
  - `fn put(&mut self, key: ObjectKey, data: Vec<u8>, now_ms: u64) -> StoreResult`
  - `fn get(&mut self, key: &ObjectKey) -> Option<Vec<u8>>`
  - `fn delete(&mut self, key: &ObjectKey) -> StoreResult`
  - `fn head(&self, key: &ObjectKey) -> Option<ObjectMetadata>` — metadata without data
  - `fn list_prefix(&self, bucket: &str, prefix: &str) -> Vec<ObjectMetadata>` — objects matching prefix
  - `fn stats(&self) -> &ObjectStoreStats`
  - `fn object_count(&self) -> usize`
  - `fn total_bytes(&self) -> u64`

Write at least **15 tests**:
1. object_key_full_path
2. put_new_object
3. put_existing_returns_already_exists
4. get_existing_object
5. get_missing_returns_none
6. delete_existing_returns_deleted
7. delete_missing_returns_not_found
8. head_returns_metadata
9. head_missing_returns_none
10. list_prefix_empty
11. list_prefix_matches
12. list_prefix_no_matches
13. stats_uploads_count
14. stats_bytes_uploaded
15. object_count_and_total_bytes

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/chunk_pool.rs`

Implement a chunk pool for reusing Vec<u8> allocations in the hot path.

Frequent small allocations for chunk data are expensive. A pool of pre-allocated
buffers reduces GC pressure.

Requirements:
- `PoolConfig` struct: `max_pooled: usize` (default 64), `chunk_size: usize` (default 16384 = 16KB), `max_chunk_size: usize` (default 65536 = 64KB)
  - Derive Debug, Clone, Serialize, Deserialize
- `PooledBuffer` struct: wraps `Vec<u8>`, returned to pool on drop
  - `fn data(&self) -> &[u8]` — slice of actual data (len, not capacity)
  - `fn as_mut_slice(&mut self) -> &mut Vec<u8>` — mutable access for filling
  - Derive Debug
- `PoolStats` struct: `allocations: u64`, `pool_hits: u64`, `pool_misses: u64`, `returns: u64`
  - `fn hit_rate(&self) -> f64` → pool_hits / allocations, or 0.0
  - Derive Debug, Clone, Default
- `ChunkPool` struct:
  - `fn new(config: PoolConfig) -> Self`
  - `fn acquire(&mut self, size_hint: usize) -> Vec<u8>` — return a Vec from pool if available and size_hint <= max_chunk_size; else allocate new; track hit/miss stats
  - `fn release(&mut self, mut buf: Vec<u8>)` — clear and return to pool if pool not full; drop if full
  - `fn stats(&self) -> &PoolStats`
  - `fn pool_size(&self) -> usize` — current number in pool

Write at least **14 tests**:
1. pool_config_default
2. pool_stats_hit_rate_zero
3. acquire_from_empty_pool
4. release_returns_to_pool
5. acquire_from_pool_hit
6. acquire_stats_miss_count
7. acquire_stats_hit_count
8. release_full_pool_drops
9. pool_size_after_release
10. pool_size_after_acquire
11. acquire_large_size_hint_bypasses_pool — size_hint > max_chunk_size
12. stats_total_allocations
13. stats_returns_count
14. hit_rate_after_hits_and_misses

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/recovery_scanner.rs`

Implement a recovery scanner that rebuilds state from segments after a crash.

After a crash, the segment catalog and block maps may be stale. The recovery
scanner reads segment headers to rebuild the metadata index.

Requirements:
- `SegmentHeader` struct: `magic: [u8; 4]` ([0x43, 0x46, 0x53, 0x31] = "CFS1"), `segment_id: u64`, `created_at_ms: u64`, `entry_count: u32`, `total_bytes: u64`, `checksum: u32`
  - `fn is_valid_magic(&self) -> bool` → magic == [0x43, 0x46, 0x53, 0x31]
  - Derive Debug, Clone, Serialize, Deserialize
- `RecoveryEntry` struct: `chunk_hash: [u8; 32]`, `inode_id: u64`, `logical_offset: u64`, `data_offset: u32`, `data_size: u32`
  - Derive Debug, Clone, Serialize, Deserialize
- `RecoveryReport` struct: `segments_scanned: u64`, `segments_valid: u64`, `segments_corrupt: u64`, `chunks_recovered: u64`, `bytes_recovered: u64`, `inodes_recovered: u64`
  - Derive Debug, Clone, Default
- `RecoveryScannerConfig` struct: `stop_on_first_error: bool` (default false), `verify_checksums: bool` (default true)
  - Derive Debug, Clone, Serialize, Deserialize
- `RecoveryScanner` struct:
  - `fn new(config: RecoveryScannerConfig) -> Self`
  - `fn scan_segment(&self, header: &SegmentHeader, entries: &[RecoveryEntry]) -> Result<usize, RecoveryError>` — validate header; return entry count; error if invalid magic
  - `fn build_report(&self, results: &[(SegmentHeader, Vec<RecoveryEntry>)]) -> RecoveryReport` — aggregate scan results
  - `fn unique_inodes(entries: &[RecoveryEntry]) -> usize` — count distinct inode_ids
- `RecoveryError` enum: `InvalidMagic`, `CorruptHeader`, `ChecksumMismatch`
  - thiserror::Error, Derive Debug

Write at least **15 tests**:
1. scanner_config_default
2. segment_header_valid_magic
3. segment_header_invalid_magic
4. scan_segment_valid
5. scan_segment_invalid_magic_returns_error
6. scan_empty_entries
7. build_report_empty
8. build_report_all_valid
9. build_report_some_corrupt
10. chunks_recovered_count
11. bytes_recovered_sum
12. inodes_recovered_count
13. unique_inodes_empty
14. unique_inodes_distinct
15. unique_inodes_with_duplicates
16. recovery_entry_fields

---

## EXPAND TESTS in existing modules

### Expand cache_coherency.rs (17 tests → +8)
Read the file. Add 8 tests for CoherencyTracker edge cases.

### Expand stripe_coordinator.rs (15 tests → +8)
Read the file. Add 8 tests for StripeCoordinator edge cases.

### Expand read_planner.rs (14 tests → +7)
Read the file. Add 7 tests for ReadPlanner edge cases.

---

## Also update lib.rs

Add:
- `pub mod object_store_bridge;`
- `pub mod chunk_pool;`
- `pub mod recovery_scanner;`
- Re-exports for key types

## Goal
- Build: 0 errors, 0 warnings
- Tests: ~1325+ passing
- Clippy: 0 warnings
