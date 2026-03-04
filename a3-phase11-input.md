# A3 Phase 11: Write Buffer, Dedup Pipeline Integration, Compaction Scheduler

You are implementing Phase 11 of the A3 (Data Reduction) agent for ClaudeFS, a distributed
scale-out POSIX file system in Rust. You will directly READ and WRITE files in the codebase.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
756 tests passing across 43 modules. Phase 11 goal: reach ~845 tests by:
1. Adding 3 new modules for write buffering, dedup integration, and compaction scheduling
2. Expanding test coverage in segment_reader, read_cache, prefetch, stream_chunker

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/write_buffer.rs`

Implement a write buffer that accumulates small writes before passing to the reduction pipeline.

In ClaudeFS, FUSE writes may be small (< 4KB). Buffering them into 2MB chunks before
running CDC improves dedup effectiveness and reduces metadata overhead.

Requirements:
- `WriteBufferConfig` struct: `flush_threshold_bytes: usize` (default 2MB = 2*1024*1024), `max_pending_writes: usize` (default 1024)
  - Derive Debug, Clone, Serialize, Deserialize
- `PendingWrite` struct: `inode_id: u64`, `offset: u64`, `data: Vec<u8>`, `timestamp_ms: u64`
  - Derive Debug, Clone
- `FlushReason` enum: `ThresholdReached`, `Explicit`, `Timeout`, `InodeClosed`
  - Derive Debug, Clone, Copy, PartialEq, Eq
- `FlushResult` struct: `inode_id: u64`, `writes: Vec<PendingWrite>`, `total_bytes: usize`, `reason: FlushReason`
  - Derive Debug
- `WriteBuffer` struct:
  - `fn new(config: WriteBufferConfig) -> Self`
  - `fn buffer_write(&mut self, write: PendingWrite) -> Option<FlushResult>` — add write to buffer; if threshold reached, return Some(FlushResult) with ThresholdReached
  - `fn flush(&mut self, inode_id: u64, reason: FlushReason) -> Option<FlushResult>` — flush all pending writes for inode
  - `fn flush_all(&mut self, reason: FlushReason) -> Vec<FlushResult>` — flush all pending writes for all inodes
  - `fn pending_count(&self, inode_id: u64) -> usize` — number of pending writes for inode
  - `fn pending_bytes(&self, inode_id: u64) -> usize` — pending bytes for inode
  - `fn total_pending_bytes(&self) -> usize` — all pending bytes across all inodes
  - `fn is_empty(&self) -> bool`

Write at least **15 tests**:
1. default_config_values
2. buffer_single_write
3. buffer_multiple_writes_same_inode
4. buffer_writes_different_inodes
5. buffer_triggers_flush_at_threshold
6. flush_returns_all_writes
7. flush_unknown_inode_returns_none
8. flush_all_returns_all_inodes
9. flush_clears_pending
10. pending_count_after_buffer
11. pending_bytes_accumulate
12. total_pending_bytes
13. is_empty_initially
14. is_empty_after_flush
15. flush_reason_preserved
16. write_buffer_max_pending — buffer fills to max_pending_writes, test behavior

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/dedup_pipeline.rs`

Implement the integrated deduplication pipeline that ties together CDC chunking, CAS lookup,
and fingerprint store for the full write path.

Requirements:
- `DedupResult` enum: `Deduplicated { existing_hash: [u8; 32] }`, `New { hash: [u8; 32], data: Vec<u8> }`
  - Derive Debug, Clone
- `DedupPipelineConfig` struct: `min_chunk_size: u32` (default 4096), `max_chunk_size: u32` (default 65536), `avg_chunk_size: u32` (default 16384), `enable_similarity: bool` (default true)
  - Derive Debug, Clone, Serialize, Deserialize
- `DedupStats` struct: `chunks_total: u64`, `chunks_deduped: u64`, `bytes_in: u64`, `bytes_deduped: u64`
  - `fn dedup_ratio(&self) -> f64` → bytes_deduped as f64 / bytes_in as f64, or 0.0
  - `fn unique_ratio(&self) -> f64` → (bytes_in - bytes_deduped) as f64 / bytes_in as f64, or 1.0
  - Derive Debug, Clone, Default, Serialize, Deserialize
- `DedupPipeline` struct:
  - `fn new(config: DedupPipelineConfig) -> Self`
  - `fn process_chunk(&mut self, data: &[u8]) -> DedupResult` — BLAKE3-hash the data; if hash exists in seen set → Deduplicated; else insert and return New
  - `fn process_data(&mut self, data: &[u8]) -> Vec<DedupResult>` — apply FastCDC to split data, then process_chunk for each piece; use `fastcdc::v2020::FastCDC` with configured min/avg/max sizes
  - `fn stats(&self) -> &DedupStats`
  - `fn reset(&mut self)` — clear seen hashes, reset stats
  - Note: "seen set" is just a `HashSet<[u8; 32]>` of hashes seen in this session

Write at least **15 tests**:
1. dedup_pipeline_config_default
2. dedup_stats_default
3. dedup_ratio_zero_bytes_in
4. dedup_ratio_all_deduped
5. unique_ratio_no_dedup
6. process_chunk_new — first time → New
7. process_chunk_dedup — second time same data → Deduplicated
8. process_chunk_different_data — different data → both New
9. process_data_single_chunk_below_min
10. process_data_splits_large_data
11. process_data_deduplicates_repeated_block
12. stats_tracks_chunks_total
13. stats_tracks_bytes_in
14. reset_clears_seen_hashes — after reset, same data is New again
15. reset_clears_stats
16. process_data_empty_returns_empty

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/compaction_scheduler.rs`

Implement compaction scheduling that throttles compaction to avoid impacting foreground I/O.

Compaction rewrites sparse segments, reclaiming space freed by GC. It must be throttled
to avoid saturating disk bandwidth during peak I/O periods.

Requirements:
- `CompactionPriority` enum: `Background` (lowest), `Normal`, `Urgent`, `Emergency` (highest)
  - Derive Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize
- `CompactionJob` struct: `job_id: u64`, `segment_ids: Vec<u64>`, `priority: CompactionPriority`, `estimated_bytes: u64`
  - Derive Debug, Clone
- `CompactionSchedulerConfig` struct: `max_concurrent_jobs: usize` (default 2), `bandwidth_limit_mb_per_sec: u32` (default 100), `urgency_threshold_pct: f64` (default 30.0, compaction urgency when segment waste > 30%)
  - Derive Debug, Clone, Serialize, Deserialize
- `SchedulerStats` struct: `jobs_queued: u64`, `jobs_completed: u64`, `jobs_cancelled: u64`, `bytes_compacted: u64`
  - Derive Debug, Clone, Default
- `CompactionScheduler` struct:
  - `fn new(config: CompactionSchedulerConfig) -> Self`
  - `fn submit(&mut self, job: CompactionJob) -> u64` — enqueue job, return job_id
  - `fn cancel(&mut self, job_id: u64) -> bool` — cancel queued (not running) job; returns true if found
  - `fn next_job(&mut self) -> Option<CompactionJob>` — dequeue highest-priority job; returns None if max_concurrent reached
  - `fn complete_job(&mut self, job_id: u64, bytes_compacted: u64)` — mark job complete, update stats
  - `fn queue_len(&self) -> usize`
  - `fn running_count(&self) -> usize`
  - `fn stats(&self) -> &SchedulerStats`
  - `fn needs_urgent_compaction(&self, waste_pct: f64) -> bool` → waste_pct > urgency_threshold_pct

Write at least **15 tests**:
1. scheduler_config_default
2. scheduler_stats_default
3. submit_returns_job_id
4. submit_increments_queue
5. next_job_returns_highest_priority
6. next_job_respects_max_concurrent
7. next_job_empty_queue_returns_none
8. cancel_removes_job
9. cancel_unknown_returns_false
10. complete_job_updates_stats
11. complete_job_decrements_running_count
12. queue_len_after_submit
13. running_count_after_next_job
14. needs_urgent_compaction_true
15. needs_urgent_compaction_false
16. priority_ordering — Emergency > Urgent > Normal > Background

---

## EXPAND TESTS in existing modules

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/segment_reader.rs`
Read the file first (12 tests). Add 8 more tests.

New tests should cover reading from segments at different offsets, reading partial chunks,
error cases (missing segment), and edge cases like empty segments.

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/read_cache.rs`
Read the file first (14 tests). Add 8 more tests.

New tests covering LRU eviction, capacity bounds, cache stats accuracy under load.

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/prefetch.rs`
Read the file first (14 tests). Add 7 more tests.

New tests for edge cases: prefetch with very small access history, large stride values,
multiple file streams, reset behavior.

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/stream_chunker.rs`
Read the file first (12 tests). Add 7 more tests.

New tests covering: chunk_slice with different sizes, streaming stats accuracy,
chunking very large data, empty stream handling.

---

## Implementation instructions

1. READ each existing file before editing it
2. For new files: complete, compilable Rust with doc comments on all public items
3. For test expansions: append to existing `mod tests` blocks
4. Import `fastcdc` using: `use fastcdc::v2020::FastCDC;` in dedup_pipeline.rs
5. For DedupPipeline.process_data(), use: `FastCDC::new(data, min_size, avg_size, max_size)`
   then iterate: `for chunk in chunker { let data_slice = &data[chunk.offset..chunk.offset+chunk.length]; ... }`
6. No async in new modules
7. Do NOT modify Cargo.toml

## Also update lib.rs

Add:
- `pub mod write_buffer;`
- `pub mod dedup_pipeline;`
- `pub mod compaction_scheduler;`
- Re-exports for key types

## Goal
- `cargo build -p claudefs-reduce` compiles with 0 errors, 0 warnings
- `cargo test -p claudefs-reduce` shows ~845+ tests passing
- `cargo clippy -p claudefs-reduce -- -D warnings` passes cleanly
