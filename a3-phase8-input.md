# A3 Phase 8: Eviction Scoring, Data Classification, Segment Splitting

You are implementing Phase 8 of the A3 (Data Reduction) agent for ClaudeFS, a distributed
scale-out POSIX file system in Rust. You will directly READ and WRITE files in the codebase.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
494 tests passing across 33 modules. Phase 8 goal: reach ~580 tests by:
1. Adding 3 new modules aligned with D5 (S3 tiering), D8 (data placement)
2. Expanding test coverage in background.rs, metrics.rs, snapshot.rs

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/eviction_scorer.rs`

Implement flash tier eviction scoring per architecture decision D5.

D5 states: High watermark 80% → start evicting. Score = `last_access_age × size`.
Old and bulky segments are evicted first. Low watermark 60% → stop evicting.
Segments pinned via xattr (`claudefs.tier=flash`) are never evicted.
Only evict segments confirmed in S3 (cache mode).

Requirements:
- `EvictionConfig` struct: `high_watermark_pct: f64` (default 80.0), `low_watermark_pct: f64` (default 60.0), `age_weight: f64` (default 1.0), `size_weight: f64` (default 1.0)
- `SegmentEvictionInfo` struct: `segment_id: u64`, `size_bytes: u64`, `last_access_age_secs: u64`, `pinned: bool`, `confirmed_in_s3: bool`
- `EvictionCandidate` struct: `segment_id: u64`, `score: f64`, `size_bytes: u64`
- `EvictionScorer` struct with config
- `fn score(&self, info: &SegmentEvictionInfo) -> f64` — computes `age * age_weight * size * size_weight`; returns 0.0 if pinned or not confirmed in S3
- `fn should_evict(&self, usage_pct: f64) -> bool` — true if usage_pct >= high_watermark_pct
- `fn should_stop_evicting(&self, usage_pct: f64) -> bool` — true if usage_pct <= low_watermark_pct
- `fn rank_candidates(&self, segments: &[SegmentEvictionInfo]) -> Vec<EvictionCandidate>` — score all, sort descending, filter out zero-score, return ranked list
- `fn select_eviction_set(&self, candidates: &[EvictionCandidate], target_bytes: u64) -> Vec<EvictionCandidate>` — greedily select highest-scored segments until target_bytes is met
- `EvictionStats` struct: `segments_evicted: u64`, `bytes_evicted: u64`, `segments_pinned: u64`, `segments_not_in_s3: u64`
- All structs: Debug, Clone, Serialize, Deserialize where appropriate
- No async needed

Write at least **16 tests**:
1. default config values
2. score_pinned_segment_is_zero — pinned → score 0.0
3. score_not_in_s3_is_zero — not in S3 → score 0.0
4. score_zero_age_is_zero — age 0 → score 0.0
5. score_positive — age=100, size=1000 → score = 100 * 1.0 * 1000 * 1.0 = 100_000.0
6. score_with_custom_weights — age_weight=2.0, size_weight=0.5
7. should_evict_above_high_watermark — 85.0 >= 80.0 → true
8. should_evict_below_high_watermark — 75.0 < 80.0 → false
9. should_evict_at_watermark — exactly 80.0 → true
10. should_stop_evicting_below_low — 55.0 <= 60.0 → true
11. should_stop_evicting_above_low — 65.0 > 60.0 → false
12. rank_candidates_sorted_descending — verify order
13. rank_candidates_filters_pinned — pinned segments not in result
14. select_eviction_set_meets_target — selects enough bytes
15. select_eviction_set_empty_candidates — returns empty
16. rank_candidates_empty — empty input → empty output
17. select_eviction_set_insufficient_candidates — returns all available even if target not met

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/data_classifier.rs`

Implement content-aware data classification for optimal compression algorithm selection.

The classifier detects data type from content, enabling the pipeline to:
- Skip compression for already-compressed data (video, JPEG, ZIP, etc.)
- Use fast LZ4 for hot data, Zstd for cold data going to S3
- Use delta compression for version-controlled text files

Requirements:
- `DataClass` enum: `Text`, `Binary`, `CompressedMedia` (JPEG/MP4/ZIP/etc.), `Executable`, `StructuredData` (JSON/CSV/XML/Parquet), `Unknown`
  - Derive Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize
- `ClassificationResult` struct: `class: DataClass`, `confidence: f64` (0.0-1.0), `compression_hint: CompressionHint`
- `CompressionHint` enum: `SkipCompression`, `UseLz4`, `UseZstd`, `UseDelta`
  - Derive Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize
- `DataClassifier` struct (stateless, no config needed for basic classification)
- `fn classify(data: &[u8]) -> ClassificationResult` — inspect first 512 bytes for magic bytes and entropy
- `fn entropy(data: &[u8]) -> f64` — Shannon entropy of first 512 bytes (0.0=all same byte, 8.0=max random)
  - Formula: for each unique byte, p = count/total, entropy += -p * log2(p)
- `fn compression_hint_for(class: DataClass) -> CompressionHint` — map class → hint
  - Text → UseZstd, Binary → UseLz4, CompressedMedia → SkipCompression, Executable → UseLz4, StructuredData → UseZstd, Unknown → UseLz4
- Detection logic (checking first 512 bytes or less):
  - JPEG: starts with [0xFF, 0xD8, 0xFF] → CompressedMedia
  - PNG: starts with [0x89, 0x50, 0x4E, 0x47] → CompressedMedia
  - ZIP/JAR: starts with [0x50, 0x4B, 0x03, 0x04] or [0x50, 0x4B, 0x05, 0x06] → CompressedMedia
  - ELF (Linux binary): starts with [0x7F, 0x45, 0x4C, 0x46] → Executable
  - PE (Windows): starts with [0x4D, 0x5A] → Executable
  - JSON: first non-whitespace char is '{' or '[' → StructuredData
  - XML/HTML: first non-whitespace char is '<' → StructuredData
  - High entropy (> 7.5) → likely CompressedMedia or encrypted → CompressedMedia, SkipCompression
  - Low entropy (< 2.5) with printable ASCII → Text
  - Otherwise → Binary
- `fn is_printable_ascii(data: &[u8]) -> bool` — true if >= 80% of bytes are printable ASCII (0x20-0x7E, 0x09, 0x0A, 0x0D)

Write at least **15 tests**:
1. classify_jpeg_magic_bytes → CompressedMedia, SkipCompression
2. classify_png_magic_bytes → CompressedMedia, SkipCompression
3. classify_zip_magic_bytes → CompressedMedia, SkipCompression
4. classify_elf_binary → Executable, UseLz4
5. classify_json_text → StructuredData, UseZstd
6. classify_xml_text → StructuredData, UseZstd
7. classify_plain_text → Text, UseZstd
8. classify_high_entropy → CompressedMedia, SkipCompression
9. classify_empty_data → Unknown
10. entropy_zero_byte_array → 0.0 (all same byte)
11. entropy_uniform_distribution → ~8.0 (all 256 bytes equally)
12. entropy_binary_data → between 0.0 and 8.0
13. compression_hint_for_all_classes — verify each class maps to correct hint
14. is_printable_ascii_true — plain ASCII string
15. is_printable_ascii_false — binary data with non-printable bytes
16. classify_small_data_8_bytes — handles data < 16 bytes gracefully

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/segment_splitter.rs`

Implement segment splitting and merging for EC stripe alignment per D1/D3.

D1: EC unit is 2MB packed segments. D3: Write journal entries packed into 2MB segments.
When a segment is too large (> max_segment_size), it must be split on chunk boundaries.
When segments are too small, they can be merged.

Requirements:
- `SplitterConfig` struct: `max_segment_bytes: u64` (default 2MB = 2*1024*1024), `min_segment_bytes: u64` (default 64*1024 = 64KB), `target_segment_bytes: u64` (default 2MB)
- `ChunkRef` struct: `hash: [u8; 32]`, `offset: u64`, `size: u32` — a reference to a chunk within a segment
  - Derive Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize
- `SegmentPlan` struct: `chunks: Vec<ChunkRef>`, `total_bytes: u64`
  - `fn is_full(&self, config: &SplitterConfig) -> bool` → total_bytes >= max_segment_bytes
  - `fn is_undersized(&self, config: &SplitterConfig) -> bool` → total_bytes < min_segment_bytes
- `SegmentSplitter` struct with config
- `fn split(chunks: &[ChunkRef]) -> Vec<SegmentPlan>` — pack chunks into segments, not exceeding max_segment_bytes per segment; never split a chunk across segments; each segment gets as many chunks as fit
- `fn merge(plans: &[SegmentPlan]) -> Vec<SegmentPlan>` — merge undersized segments into larger ones up to max_segment_bytes; do not merge if already at or above min_segment_bytes; greedy left-to-right
- `fn optimal_split(chunks: &[ChunkRef]) -> Vec<SegmentPlan>` — like split() but tries to produce segments as close to target_segment_bytes as possible (still respects max)
- `SplitStats` struct: `input_chunks: usize`, `output_segments: usize`, `avg_segment_bytes: f64`, `min_segment_bytes: u64`, `max_segment_bytes: u64`
- `fn stats(plans: &[SegmentPlan]) -> SplitStats`

Write at least **15 tests**:
1. default_config_values — verify 2MB max, 64KB min, 2MB target
2. split_single_chunk_fits — one chunk smaller than max → one segment
3. split_multiple_chunks_fit — total < max → one segment
4. split_overflow_creates_two_segments — total > max → two segments
5. split_many_small_chunks — 1000 × 1KB chunks → ~500 chunks per 2MB segment
6. split_chunk_larger_than_max — single chunk > max → one segment still (don't split chunks)
7. split_empty — no chunks → no segments
8. merge_two_small_into_one — two 32KB segments → one 64KB segment
9. merge_three_small_into_two — three 768KB segments (2.25MB total) → one 1.5MB + one 0.75MB
10. merge_already_large — segment >= min_segment_bytes, not merged with smaller
11. merge_empty — no segments → no segments
12. is_full_false — small segment not full
13. is_full_true — segment at max bytes is full
14. is_undersized_true — small segment
15. is_undersized_false — segment at min bytes
16. stats_single_segment — verify avg, min, max all equal
17. stats_multiple_segments

---

## EXPAND TESTS in existing modules

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/background.rs`
Read the file first. It currently has about 6 tokio tests. Add 10 more.

New tests:
1. `test_background_config_default` — verify default channel_capacity=1000, etc.
2. `test_background_stats_default` — all zeros
3. `test_handle_is_running_true` — handle is_running() after start()
4. `test_gc_cycle_reclaims_chunks` — put chunks in CAS with refcount 0, run GC, verify reclaimed
5. `test_process_chunk_increments_counter` — send 5 ProcessChunk tasks, verify chunks_processed=5
6. `test_gc_cycles_counter` — send 3 RunGc tasks, verify gc_cycles=3
7. `test_stats_initial_similarity_hits_zero` — starts at 0
8. `test_background_handle_send_after_shutdown` — sending after shutdown returns error
9. `test_config_custom_channel_capacity` — create with capacity=10
10. `test_process_multiple_gc_and_chunks_interleaved` — mix of tasks

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/metrics.rs`
Read the file first. Add 10 more tests.

(Read the full file to understand `ReductionMetrics`, `MetricsHandle`, `MetricKind`, `MetricValue`, `ReduceMetric`, `MetricsSnapshot`)

New tests should cover:
1. Default values
2. Record metric, verify snapshot
3. Multiple metrics recorded
4. MetricKind variants
5. MetricValue comparison
6. Snapshot correctness
7. Reset/clear
8. Concurrent metric recording (if thread-safe)
9. ReduceMetric fields
10. MetricsHandle operations

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/snapshot.rs`
Read the file first. Add 8 more tests.

(Read full file to understand `Snapshot`, `SnapshotConfig`, `SnapshotInfo`, `SnapshotManager`)

New tests should cover:
1. Default config values
2. Create snapshot, verify info fields
3. List snapshots empty
4. List snapshots after creation
5. Delete snapshot
6. Snapshot with custom name
7. SnapshotInfo metadata (created_at, size_bytes, etc.)
8. Multiple snapshots ordered

---

## Implementation instructions

1. READ each existing file before editing it
2. For new files: write complete, compilable Rust files with proper imports
3. For expanded tests: append new test functions to the existing `#[cfg(test)] mod tests` blocks
4. All public items need doc comments (satisfy `#![warn(missing_docs)]`)
5. Use `serde::{Serialize, Deserialize}` for config/data structs
6. No async needed in new modules — all functions are synchronous
7. Import from crate root: `use crate::fingerprint::ChunkHash;` etc.
8. Do NOT modify Cargo.toml

## Also update lib.rs

Add to `/home/cfs/claudefs/crates/claudefs-reduce/src/lib.rs`:
- `pub mod eviction_scorer;`
- `pub mod data_classifier;`
- `pub mod segment_splitter;`
- Re-exports for key public types from each module

## Goal
- `cargo build -p claudefs-reduce` compiles with 0 errors, 0 warnings
- `cargo test -p claudefs-reduce` shows ~580+ tests passing
- `cargo clippy -p claudefs-reduce -- -D warnings` passes cleanly
