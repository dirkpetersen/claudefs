# A3 Phase 7: New Modules + Test Expansion for claudefs-reduce

You are implementing Phase 7 of the A3 (Data Reduction) agent for ClaudeFS, a distributed
scale-out POSIX file system in Rust. You will directly READ and WRITE files in the codebase.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
393 tests passing across 30 modules. Phase 7 goal: reach ~490 tests by:
1. Adding 3 new modules (chunk_verifier.rs, pipeline_monitor.rs, write_amplification.rs)
2. Expanding test coverage in 6 undertested modules

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/chunk_verifier.rs`

Implement a background data integrity verifier that detects bitrot and data corruption.

Requirements:
- `ChunkVerifier` struct with config `ChunkVerifierConfig`
- `VerificationResult` enum: `Ok`, `Corrupted { hash: ChunkHash, expected: ChunkHash }`, `Missing { hash: ChunkHash }`
- `VerificationStats` struct tracking: chunks_verified, chunks_ok, chunks_corrupted, chunks_missing, bytes_verified
- `fn verify_chunk(data: &[u8], expected_hash: &ChunkHash) -> VerificationResult` — recompute BLAKE3, compare
- `fn verify_batch(chunks: &[(ChunkHash, Vec<u8>)]) -> Vec<VerificationResult>` — verify multiple chunks
- `fn schedule_verification(hashes: Vec<ChunkHash>) -> VerificationSchedule` — priority queue of chunks to check
- `VerificationSchedule` struct: ordered list of hashes to verify, tracks last_verified timestamps
- `fn next_batch(&mut self, n: usize) -> Vec<ChunkHash>` on VerificationSchedule
- `ChunkVerifierConfig`: `batch_size: usize` (default 64), `interval_secs: u64` (default 3600), `priority: VerificationPriority`
- `VerificationPriority` enum: `Low`, `Normal`, `High`

Write at least **15 tests** covering:
1. verify_chunk with correct data → Ok
2. verify_chunk with corrupted data → Corrupted
3. verify_chunk with empty data
4. verify_batch with mixed results
5. verify_batch all ok
6. verify_batch all corrupted
7. schedule_verification creates schedule
8. next_batch returns up to n items
9. next_batch on empty schedule returns empty
10. VerificationStats default values
11. stats accumulation across multiple verifications
12. ChunkVerifierConfig default values
13. VerificationResult Debug formatting
14. Large batch verification (100+ chunks)
15. Duplicate hash in schedule

Use `#[cfg(test)]` test module. Import `crate::fingerprint::{blake3_hash, ChunkHash}`.
No async needed — all functions are sync. Use `use serde::{Deserialize, Serialize};` for config structs.

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/pipeline_monitor.rs`

Implement real-time monitoring and alerting for the reduction pipeline.

Requirements:
- `PipelineMonitor` struct that aggregates stats from multiple pipeline stages
- `StageMetrics` struct: `stage_name: String`, `chunks_in: u64`, `chunks_out: u64`, `bytes_in: u64`, `bytes_out: u64`, `errors: u64`, `latency_sum_us: u64`, `latency_count: u64`
- `StageMetrics::avg_latency_us()` → f64 (latency_sum / latency_count, or 0.0)
- `StageMetrics::reduction_ratio()` → f64 (bytes_in / bytes_out, or 1.0)
- `StageMetrics::error_rate()` → f64 (errors / chunks_in, or 0.0)
- `PipelineMetrics` struct containing: `stages: Vec<StageMetrics>`, `total_chunks: u64`, `total_bytes_in: u64`, `total_bytes_out: u64`, `overall_reduction_ratio() -> f64`
- `PipelineMonitor::new() -> Self`
- `PipelineMonitor::record_stage(metrics: StageMetrics)` — add or update stage metrics
- `PipelineMonitor::snapshot() -> PipelineMetrics` — return aggregated metrics snapshot
- `PipelineMonitor::reset()` — clear all metrics
- `AlertThreshold` struct: `max_error_rate: f64`, `min_reduction_ratio: f64`, `max_latency_us: u64`
- `PipelineMonitor::check_alerts(&self, threshold: &AlertThreshold) -> Vec<PipelineAlert>`
- `PipelineAlert` enum: `HighErrorRate { stage: String, rate: f64 }`, `LowReduction { stage: String, ratio: f64 }`, `HighLatency { stage: String, latency_us: u64 }`
- All structs derive Debug, Clone, Default where appropriate
- Use serde for config structs

Write at least **15 tests** covering:
1. new monitor has no stages
2. record_stage adds stage
3. snapshot with no stages returns empty metrics
4. snapshot aggregates multiple stages correctly
5. total_bytes_in/out are summed correctly
6. overall_reduction_ratio with data
7. overall_reduction_ratio with zero bytes_out returns 1.0
8. StageMetrics::avg_latency_us with zero count
9. StageMetrics::avg_latency_us with data
10. StageMetrics::reduction_ratio
11. StageMetrics::error_rate
12. check_alerts no alerts on healthy pipeline
13. check_alerts detects high error rate
14. check_alerts detects low reduction
15. check_alerts detects high latency
16. reset clears all metrics
17. record_stage multiple times (updates existing)

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/write_amplification.rs`

Track and analyze write amplification from dedup, compression, and erasure coding operations.

Requirements:
- `WriteAmplificationTracker` struct
- `WriteEvent` struct: `logical_bytes: u64`, `physical_bytes: u64`, `dedup_bytes_saved: u64`, `compression_bytes_saved: u64`, `ec_overhead_bytes: u64`, `timestamp_ms: u64`
- `WriteAmplificationStats` struct:
  - `total_logical_bytes: u64`
  - `total_physical_bytes: u64`
  - `total_dedup_saved: u64`
  - `total_compression_saved: u64`
  - `total_ec_overhead: u64`
  - `event_count: u64`
  - `write_amplification() -> f64` (physical / logical, or 1.0)
  - `effective_reduction() -> f64` (logical / physical, or 1.0)
  - `dedup_ratio() -> f64` (dedup_saved / logical, or 0.0)
  - `compression_ratio() -> f64` (compression_saved / (logical - dedup_saved), or 0.0)
  - `ec_overhead_pct() -> f64` (ec_overhead / physical * 100.0)
- `WriteAmplificationTracker::new() -> Self`
- `WriteAmplificationTracker::record(event: WriteEvent)`
- `WriteAmplificationTracker::stats() -> WriteAmplificationStats`
- `WriteAmplificationTracker::reset()`
- `WriteAmplificationTracker::window_stats(last_n: usize) -> WriteAmplificationStats` — stats for last N events
- `WriteAmplificationConfig`: `max_events: usize` (default 10000, circular buffer)
- All structs: Debug, Clone, Default, Serialize, Deserialize where appropriate

Write at least **15 tests** covering:
1. new tracker has zero stats
2. record single event, verify stats
3. write_amplification calculation
4. effective_reduction calculation
5. dedup_ratio calculation
6. compression_ratio calculation
7. ec_overhead_pct calculation
8. record multiple events accumulates correctly
9. reset clears stats
10. window_stats with fewer events than window
11. window_stats with exactly window size
12. window_stats with more events than window (circular)
13. zero logical bytes edge case
14. event with no dedup or compression
15. event with full dedup (all logical bytes deduped)
16. config default values

---

## EXPAND TESTS in existing modules

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/fingerprint.rs`
Currently has 5 tests. Add 10 more tests in the `#[cfg(test)]` mod tests block.
READ the file first, then append to the existing tests block.

New tests to add:
1. `test_chunk_hash_to_hex` — verify hex string length is 64, all hex chars
2. `test_chunk_hash_display` — verify Display formatting matches to_hex()
3. `test_chunk_hash_as_bytes` — verify as_bytes returns same underlying array
4. `test_super_features_is_similar_true` — identical features → is_similar true
5. `test_super_features_is_similar_false` — completely different → is_similar false
6. `test_super_features_similarity_0` — all different features → 0
7. `test_super_features_similarity_4` — identical features → 4
8. `test_super_features_exactly_4_bytes` — exactly 4 byte input
9. `test_super_features_large_data` — 1MB data produces consistent features
10. `test_blake3_hash_empty` — empty data produces non-zero hash

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/gc.rs`
Currently has 6 tests. Add 9 more tests.

New tests:
1. `test_gc_empty_cas` — sweep on empty CAS → 0 reclaimed
2. `test_gc_all_referenced` — all chunks have refcount > 0 → 0 reclaimed
3. `test_gc_multiple_cycles` — run 3 cycles, check stats accumulate
4. `test_mark_reachable_multiple` — mark 10 hashes, verify all marked
5. `test_sweep_only_zeros` — verify only refcount=0 chunks are swept
6. `test_gc_with_high_refcount` — chunk with refcount 5, not reclaimed
7. `test_gc_stats_bytes_reclaimed` — bytes_reclaimed field (currently 0, verify 0)
8. `test_is_marked_false_initially` — newly created gc has no marks
9. `test_run_cycle_empty_reachable` — empty reachable list, all zero-refcount swept

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/similarity.rs`
Currently has 8 tests. Add 8 more tests.

New tests:
1. `test_insert_multiple_chunks` — insert 5 chunks, entry_count = 5
2. `test_find_similar_empty_index` — empty index returns None
3. `test_delta_compress_empty_data` — empty data compresses (non-empty reference)
4. `test_delta_compress_empty_reference` — empty reference returns error
5. `test_delta_compress_identical` — identical data and reference
6. `test_delta_compress_large_data` — 64KB data compression roundtrip
7. `test_similarity_index_thread_safety` — clone Arc<SimilarityIndex> across thread (use std::sync::Arc, std::thread::spawn)
8. `test_remove_nonexistent` — removing hash not in index is safe (no panic)

Note: `SimilarityIndex` uses `Arc<RwLock<...>>` internally so you need to wrap in Arc to clone:
`let idx = Arc::new(SimilarityIndex::new());`

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/recompressor.rs`
Currently has 7 tests. Add 8 more tests.

READ the file at `/home/cfs/claudefs/crates/claudefs-reduce/src/recompressor.rs` first
to understand the full implementation. Then add tests.

New tests:
1. `test_recompressor_default_config` — verify default zstd_level=3, min_improvement_pct=5
2. `test_stats_compression_ratio_no_data` — ratio with 0 bytes is 1.0
3. `test_stats_bytes_saved_positive` — bytes_before > bytes_after → positive
4. `test_stats_bytes_saved_negative` — bytes_after > bytes_before → negative
5. `test_recompress_incompressible` — random data, may not improve
6. `test_recompress_highly_compressible` — repeated data, Zstd wins
7. `test_recompress_batch_empty` — empty batch returns empty stats
8. `test_stats_chunks_processed_count` — verify count increments per call

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs`
Currently has 7 tests. Add 8 more tests.

READ the file at `/home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs` first.

New tests:
1. `test_write_path_stats_default` — default stats all zeros
2. `test_total_input_bytes` — matches pipeline.input_bytes
3. `test_overall_reduction_ratio_no_data` — 0 bytes stored → 1.0
4. `test_write_path_empty_data` — process empty slice
5. `test_write_path_small_data` — process 1KB data
6. `test_write_path_large_data` — process 1MB data
7. `test_write_path_with_dedup` — same data twice → 2nd should dedup
8. `test_write_path_stats_segments_produced` — verify segments_produced tracks sealed segments

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/encryption.rs`
Currently has 6 tests. Add 9 more tests.

READ the file at `/home/cfs/claudefs/crates/claudefs-reduce/src/encryption.rs` first.

New tests (look at what's exported: EncryptedChunk, EncryptionAlgorithm, EncryptionKey):
1. `test_encryption_key_generate_different` — two generated keys are different
2. `test_encryption_aes_gcm_roundtrip_empty` — encrypt/decrypt empty data
3. `test_encryption_aes_gcm_roundtrip_large` — encrypt/decrypt 1MB data
4. `test_encryption_chacha20_roundtrip` — if ChaCha20 is supported
5. `test_encrypted_chunk_metadata` — verify chunk_id/size fields after encrypt
6. `test_wrong_key_fails_decrypt` — decrypting with wrong key returns error
7. `test_wrong_nonce_fails_decrypt` — tampered ciphertext returns error
8. `test_encryption_algorithm_variants` — verify all algorithm variants exist
9. `test_encryption_key_as_bytes` — key bytes are correct length

---

## Implementation instructions

1. READ each file before editing it (use file I/O, not just your memory)
2. For new files: write complete, compilable Rust files with proper imports
3. For expanded tests: append new test functions to existing `mod tests` blocks
4. Keep all code `#![warn(missing_docs)]` compatible — add doc comments to public items
5. Use `thiserror` for error types (already in dependencies)
6. Use `serde::{Serialize, Deserialize}` for config/stats structs
7. Keep all functions pure/sync where possible (no async in new modules)
8. Do NOT modify Cargo.toml — all needed crates are already included

## After writing all files

Also update `/home/cfs/claudefs/crates/claudefs-reduce/src/lib.rs`:
- Add `pub mod chunk_verifier;`
- Add `pub mod pipeline_monitor;`
- Add `pub mod write_amplification;`
- Add re-exports for key types from each new module

The current lib.rs ends at line 70. Add the new module declarations and re-exports after line 38
(which has `pub mod worm_reducer;`) in the module list, and corresponding re-exports at the bottom.

## Goal
After all edits:
- `cargo build -p claudefs-reduce` compiles with 0 errors, 0 warnings
- `cargo test -p claudefs-reduce` shows ~490+ tests passing
- `cargo clippy -p claudefs-reduce -- -D warnings` passes cleanly
