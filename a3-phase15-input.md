# A3 Phase 15: Segment GC Integrator, Checksum Store, Pipeline Backpressure

You are implementing Phase 15 of the A3 (Data Reduction) agent for ClaudeFS.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
1091 tests across 55 modules. Phase 15 goal: ~1180 tests.

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/segment_gc.rs`

Implement segment-level garbage collection that integrates the GC engine with the segment catalog.

Requirements:
- `SegmentGcConfig` struct: `min_alive_ratio: f64` (default 0.5, reclaim segment if < 50% of chunks alive), `max_segments_per_cycle: usize` (default 10)
  - Derive Debug, Clone, Serialize, Deserialize
- `SegmentGcReport` struct: `segments_scanned: usize`, `segments_reclaimed: usize`, `segments_compacted: usize`, `bytes_freed: u64`, `bytes_compacted: u64`
  - Derive Debug, Clone, Default
- `SegmentInfo` struct: `segment_id: u64`, `total_chunks: usize`, `alive_chunks: usize`, `total_bytes: u64`, `alive_bytes: u64`
  - `fn alive_ratio(&self) -> f64` → alive_chunks as f64 / total_chunks as f64, or 1.0
  - `fn dead_bytes(&self) -> u64` → total_bytes - alive_bytes
  - `fn should_reclaim(&self, min_alive_ratio: f64) -> bool` → alive_ratio() < min_alive_ratio && alive_chunks == 0
  - `fn should_compact(&self, min_alive_ratio: f64) -> bool` → alive_ratio() < min_alive_ratio && alive_chunks > 0
  - Derive Debug, Clone, Serialize, Deserialize
- `SegmentGc` struct:
  - `fn new(config: SegmentGcConfig) -> Self`
  - `fn evaluate_segment(&self, info: &SegmentInfo) -> SegmentGcAction`
  - `fn run_cycle(&mut self, segments: &[SegmentInfo]) -> SegmentGcReport` — evaluate all segments, count actions
  - `fn top_candidates(&self, segments: &[SegmentInfo], n: usize) -> Vec<&SegmentInfo>` — segments sorted by dead_bytes descending
- `SegmentGcAction` enum: `Keep`, `Reclaim`, `Compact`
  - Derive Debug, Clone, Copy, PartialEq, Eq

Write at least **15 tests**:
1. config_default
2. segment_alive_ratio_full — all chunks alive → 1.0
3. segment_alive_ratio_empty — no chunks → 1.0
4. segment_alive_ratio_half — half alive
5. should_reclaim_true — 0 alive chunks, below threshold
6. should_reclaim_false_has_alive — some alive chunks
7. should_compact_true — partially alive, below threshold
8. should_compact_false_above_threshold
9. evaluate_keep
10. evaluate_reclaim
11. evaluate_compact
12. run_cycle_counts_actions
13. top_candidates_sorted_by_dead_bytes
14. top_candidates_limited
15. run_cycle_empty — no segments → empty report
16. dead_bytes_calculation

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/checksum_store.rs`

Implement a checksum store for tracking end-to-end data integrity.

Requirements:
- `ChecksumEntry` struct: `chunk_hash: [u8; 32]`, `checksum: [u8; 4]` (CRC32C), `verified_at_ms: u64`, `fail_count: u8`
  - `fn is_suspect(&self) -> bool` → fail_count > 0
  - Derive Debug, Clone, Serialize, Deserialize
- `ChecksumStoreConfig` struct: `max_entries: usize` (default 1_000_000), `suspect_threshold: u8` (default 1)
  - Derive Debug, Clone, Serialize, Deserialize
- `ChecksumStore` struct:
  - `fn new(config: ChecksumStoreConfig) -> Self`
  - `fn insert(&mut self, hash: [u8; 32], checksum: [u8; 4], now_ms: u64)`
  - `fn get(&self, hash: &[u8; 32]) -> Option<&ChecksumEntry>`
  - `fn verify(&self, hash: &[u8; 32], data: &[u8]) -> ChecksumVerifyResult`
    - Compute CRC32C of data using the `crc32c` approach: XOR-based simple CRC (you can use a simple Adler32-like or just XOR first 4 bytes as placeholder: `let cs = [data[0], data[1], data[2], data[3]]` for non-empty data, `[0u8;4]` for empty)
    - Actually implement a proper simple checksum: sum all bytes modulo 256, store in [sum as u8, 0, 0, 0]
    - Compare with stored checksum
  - `fn record_failure(&mut self, hash: &[u8; 32]) -> bool` — increment fail_count; return true if found
  - `fn suspect_entries(&self) -> Vec<&ChecksumEntry>` — entries with fail_count > 0
  - `fn entry_count(&self) -> usize`
  - `fn remove(&mut self, hash: &[u8; 32]) -> bool`
- `ChecksumVerifyResult` enum: `Ok`, `Mismatch { stored: [u8; 4], computed: [u8; 4] }`, `NotFound`
  - Derive Debug, Clone, PartialEq, Eq

Write at least **15 tests**:
1. config_default
2. insert_and_get
3. get_missing_returns_none
4. verify_ok — correct data
5. verify_mismatch — wrong data
6. verify_not_found
7. record_failure_increments
8. record_failure_unknown_returns_false
9. is_suspect_false — fail_count 0
10. is_suspect_true — fail_count > 0
11. suspect_entries_empty
12. suspect_entries_after_failure
13. entry_count_increments
14. remove_existing
15. remove_missing_returns_false
16. checksum_empty_data

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/pipeline_backpressure.rs`

Implement backpressure for the reduction pipeline to prevent memory exhaustion.

When the pipeline processes faster than it can write, chunks accumulate in memory.
Backpressure signals the ingestion layer to slow down.

Requirements:
- `BackpressureConfig` struct: `high_watermark_bytes: usize` (default 256MB), `low_watermark_bytes: usize` (default 64MB), `high_watermark_chunks: usize` (default 10000)
  - Derive Debug, Clone, Serialize, Deserialize
- `BackpressureState` enum: `Normal`, `Warning`, `Throttled`, `Stalled`
  - Derive Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord
- `PipelineBackpressure` struct:
  - `fn new(config: BackpressureConfig) -> Self`
  - `fn add_bytes(&mut self, n: usize)` — add bytes to in-flight counter
  - `fn remove_bytes(&mut self, n: usize)` — subtract bytes (min 0)
  - `fn add_chunks(&mut self, n: usize)`
  - `fn remove_chunks(&mut self, n: usize)` — min 0
  - `fn state(&self) -> BackpressureState`
    - Stalled: bytes >= high_watermark_bytes * 2 OR chunks >= high_watermark_chunks * 2
    - Throttled: bytes >= high_watermark_bytes OR chunks >= high_watermark_chunks
    - Warning: bytes >= low_watermark_bytes
    - Normal: otherwise
  - `fn should_accept(&self) -> bool` → state() <= BackpressureState::Warning
  - `fn in_flight_bytes(&self) -> usize`
  - `fn in_flight_chunks(&self) -> usize`
  - `fn stats(&self) -> BackpressureStats`
- `BackpressureStats` struct: `current_bytes: usize`, `current_chunks: usize`, `peak_bytes: usize`, `peak_chunks: usize`, `throttle_events: u64`
  - Derive Debug, Clone, Default

Write at least **15 tests**:
1. config_default
2. initial_state_normal
3. add_bytes_increments
4. remove_bytes_decrements
5. remove_bytes_min_zero
6. state_normal — below low watermark
7. state_warning — above low watermark
8. state_throttled — above high watermark
9. state_stalled — above 2x high watermark
10. should_accept_normal
11. should_accept_warning
12. should_not_accept_throttled
13. should_not_accept_stalled
14. in_flight_bytes
15. in_flight_chunks
16. peak_bytes_tracked

---

## EXPAND TESTS in existing modules

### Expand eviction_scorer.rs (18 tests → +8)
Read the file. Add 8 tests for edge cases in EvictionScorer and EvictionStats.

### Expand data_classifier.rs (26 tests → +7)
Read the file. Add 7 tests for more content types and edge cases.

### Expand segment_splitter.rs (23 tests → +7)
Read the file. Add 7 tests for edge cases in segment splitting/merging.

---

## Also update lib.rs

Add:
- `pub mod segment_gc;`
- `pub mod checksum_store;`
- `pub mod pipeline_backpressure;`
- Re-exports for key types

## Goal
- Build: 0 errors, 0 warnings
- Tests: ~1180+ passing
- Clippy: 0 warnings
