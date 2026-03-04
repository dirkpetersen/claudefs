# A3 Phase 16: Ingest Pipeline, Chunk Prefetch Manager, Dedup Index

You are implementing Phase 16 of the A3 (Data Reduction) agent for ClaudeFS.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
1169 tests across 58 modules. Phase 16 goal: ~1260 tests.

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/ingest_pipeline.rs`

Implement the top-level ingest pipeline that orchestrates all reduction stages.

The ingest pipeline processes raw data from clients into reduced/encrypted chunks
ready for segment packing. Combines: buffer → CDC → dedup → compress → encrypt.

Requirements:
- `IngestStage` enum: `Buffering`, `Chunking`, `Deduplicating`, `Compressing`, `Encrypting`, `Packing`
  - Derive Debug, Clone, Copy, PartialEq, Eq
- `IngestConfig` struct: `buffer_threshold_bytes: usize` (default 1MB), `enable_dedup: bool` (default true), `enable_compression: bool` (default true), `enable_encryption: bool` (default true), `compression_level: i32` (default 3)
  - Derive Debug, Clone, Serialize, Deserialize
- `IngestMetrics` struct: `bytes_ingested: u64`, `bytes_deduplicated: u64`, `bytes_compressed: u64`, `bytes_encrypted: u64`, `chunks_new: u64`, `chunks_deduped: u64`, `stage_latencies_us: Vec<u64>` (one per stage)
  - `fn overall_reduction_ratio(&self) -> f64` → if bytes_encrypted > 0 { bytes_ingested / bytes_encrypted } else { 1.0 }
  - `fn dedup_savings_pct(&self) -> f64` → bytes_deduplicated / bytes_ingested * 100.0, or 0.0
  - Derive Debug, Clone, Default
- `IngestPipeline` struct:
  - `fn new(config: IngestConfig) -> Self`
  - `fn ingest(&mut self, inode_id: u64, data: &[u8]) -> Vec<IngestChunk>` — process data through all enabled stages; return list of chunks
  - `fn current_stage(&self) -> IngestStage`
  - `fn metrics(&self) -> &IngestMetrics`
  - `fn reset_metrics(&mut self)`
- `IngestChunk` struct: `hash: [u8; 32]`, `data: Vec<u8>`, `original_size: usize`, `deduped: bool`
  - Derive Debug, Clone

Write at least **14 tests**:
1. ingest_config_default
2. ingest_metrics_default
3. overall_reduction_ratio_no_data
4. dedup_savings_pct_no_savings
5. ingest_empty_data_returns_empty
6. ingest_small_data_returns_chunks
7. ingest_metrics_bytes_ingested
8. ingest_metrics_chunks_new
9. ingest_dedup_disabled — still produces chunks, no dedup tracking
10. ingest_compression_disabled — chunks produced without compression
11. ingest_same_data_twice_deduplicates
12. reset_metrics_clears
13. ingest_large_data_multiple_chunks
14. ingest_chunk_deduped_flag

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/prefetch_manager.rs`

Implement a higher-level prefetch manager that coordinates prefetch requests from multiple files.

Requirements:
- `PrefetchRequest` struct: `inode_id: u64`, `chunk_hashes: Vec<[u8; 32]>`, `priority: u8`, `created_at_ms: u64`
  - Derive Debug, Clone
- `PrefetchStatus` enum: `Pending`, `InFlight`, `Completed`, `Failed`
  - Derive Debug, Clone, Copy, PartialEq, Eq
- `PrefetchEntry` struct: `request: PrefetchRequest`, `status: PrefetchStatus`, `completed_count: usize`
  - Derive Debug, Clone
- `PrefetchManagerConfig` struct: `max_pending: usize` (default 100), `max_inflight: usize` (default 10), `max_chunks_per_request: usize` (default 16)
  - Derive Debug, Clone, Serialize, Deserialize
- `PrefetchManager` struct:
  - `fn new(config: PrefetchManagerConfig) -> Self`
  - `fn submit(&mut self, request: PrefetchRequest) -> Result<u64, PrefetchError>` — enqueue, return request_id
  - `fn next_request(&mut self) -> Option<PrefetchRequest>` — dequeue highest-priority pending request; mark as InFlight
  - `fn complete(&mut self, request_id: u64, success: bool)` — mark Completed or Failed
  - `fn pending_count(&self) -> usize`
  - `fn inflight_count(&self) -> usize`
  - `fn drain_completed(&mut self) -> Vec<PrefetchEntry>` — remove and return all completed entries
- `PrefetchError` enum: `QueueFull`, `TooManyChunks`
  - thiserror::Error, Derive Debug

Write at least **14 tests**:
1. manager_config_default
2. submit_returns_id
3. pending_count_after_submit
4. next_request_dequeues_highest_priority
5. next_request_marks_inflight
6. inflight_count_after_next
7. complete_success
8. complete_failure
9. drain_completed_returns_completed
10. drain_completed_clears
11. submit_full_queue_returns_error
12. submit_too_many_chunks_returns_error
13. next_request_empty_returns_none
14. priority_ordering — higher priority dequeued first

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/dedup_index.rs`

Implement a distributed-ready dedup index (fingerprint index) for cross-node deduplication.

Requirements:
- `DedupIndexEntry` struct: `hash: [u8; 32]`, `segment_id: u64`, `offset_in_segment: u32`, `size: u32`, `ref_count: u32`
  - Derive Debug, Clone, Serialize, Deserialize
- `DedupIndexStats` struct: `total_entries: u64`, `total_bytes: u64`, `total_refs: u64`, `unique_ratio: f64`
  - Derive Debug, Clone, Default
- `DedupIndexConfig` struct: `max_entries: usize` (default 10_000_000), `shard_count: u8` (default 16, for sharding in distributed deployments)
  - Derive Debug, Clone, Serialize, Deserialize
- `DedupIndex` struct:
  - `fn new(config: DedupIndexConfig) -> Self`
  - `fn insert(&mut self, hash: [u8; 32], segment_id: u64, offset: u32, size: u32) -> bool` — true if new entry, false if existed (increments ref_count)
  - `fn lookup(&self, hash: &[u8; 32]) -> Option<&DedupIndexEntry>`
  - `fn release(&mut self, hash: &[u8; 32]) -> Option<u32>` — decrement ref_count, remove if 0, return new ref_count
  - `fn entry_count(&self) -> usize`
  - `fn total_refs(&self) -> u64` — sum of all ref_counts
  - `fn stats(&self) -> DedupIndexStats`
  - `fn shard_for(&self, hash: &[u8; 32]) -> u8` → hash[0] % shard_count (for routing in distributed mode)
  - `fn entries_in_shard(&self, shard: u8) -> Vec<&DedupIndexEntry>` — entries where shard_for(hash) == shard
  - `fn is_full(&self) -> bool` → entry_count() >= max_entries

Write at least **15 tests**:
1. config_default
2. insert_new_entry_returns_true
3. insert_existing_increments_ref_count
4. lookup_found
5. lookup_not_found
6. release_decrements_ref
7. release_to_zero_removes_entry
8. release_unknown_returns_none
9. entry_count_increments
10. total_refs_sums_all
11. shard_for_deterministic
12. entries_in_shard_filtered
13. stats_empty_index
14. stats_with_entries
15. is_full_false
16. is_full_true

---

## EXPAND TESTS in existing modules

### Expand block_map.rs (17 tests → +7 more)
Read the file. Add 7 tests for edge cases in BlockMap and BlockMapStore.

### Expand journal_segment.rs (17 tests → +7 more)
Read the file. Add 7 tests for edge cases in JournalSegment and JournalEntry.

### Expand tenant_isolator.rs (18 tests → +7 more)
Read the file. Add 7 tests for edge cases in TenantIsolator and TenantPolicy.

---

## Also update lib.rs

Add:
- `pub mod ingest_pipeline;`
- `pub mod prefetch_manager;`
- `pub mod dedup_index;`
- Re-exports for key types

## Goal
- Build: 0 errors, 0 warnings
- Tests: ~1260+ passing
- Clippy: 0 warnings
