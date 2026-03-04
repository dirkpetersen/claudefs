# A3 Phase 12: Snapshot Catalog, Chunk Scheduler, Tier Migration

You are implementing Phase 12 of the A3 (Data Reduction) agent for ClaudeFS.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
831 tests across 46 modules. Phase 12 goal: ~920 tests.

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/snapshot_catalog.rs`

Implement a catalog of snapshots for efficient snapshot management and space accounting.

Requirements:
- `SnapshotId` newtype: `pub struct SnapshotId(pub u64)` — Derive Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize
- `SnapshotRecord` struct: `id: SnapshotId`, `name: String`, `created_at_ms: u64`, `inode_count: u64`, `unique_chunk_count: u64`, `shared_chunk_count: u64`, `total_bytes: u64`, `unique_bytes: u64`
  - `fn shared_bytes(&self) -> u64` → total_bytes - unique_bytes
  - `fn space_amplification(&self) -> f64` → total_bytes / unique_bytes, or 1.0
  - Derive Debug, Clone, Serialize, Deserialize
- `SnapshotCatalog` struct:
  - `fn new() -> Self`
  - `fn add(&mut self, record: SnapshotRecord) -> SnapshotId`
  - `fn get(&self, id: SnapshotId) -> Option<&SnapshotRecord>`
  - `fn get_by_name(&self, name: &str) -> Option<&SnapshotRecord>`
  - `fn list(&self) -> Vec<&SnapshotRecord>` — sorted by created_at_ms ascending
  - `fn delete(&mut self, id: SnapshotId) -> bool`
  - `fn count(&self) -> usize`
  - `fn total_unique_bytes(&self) -> u64` — sum of unique_bytes across all snapshots
  - `fn total_shared_bytes(&self) -> u64` — sum of shared_bytes across all snapshots
  - `fn oldest(&self) -> Option<&SnapshotRecord>`
  - `fn newest(&self) -> Option<&SnapshotRecord>`

Write at least **15 tests**:
1. snapshot_id_equality
2. new_catalog_is_empty
3. add_snapshot_returns_id
4. get_snapshot_found
5. get_snapshot_not_found
6. get_by_name_found
7. get_by_name_not_found
8. list_sorted_by_time
9. delete_snapshot
10. delete_unknown_returns_false
11. count_increments
12. total_unique_bytes_sum
13. total_shared_bytes_sum
14. oldest_empty_catalog
15. oldest_with_snapshots
16. newest_with_snapshots
17. shared_bytes_calculation
18. space_amplification

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/chunk_scheduler.rs`

Implement chunk I/O scheduling that prioritizes reads over background writes.

Requirements:
- `ChunkOp` enum: `Read { chunk_hash: [u8; 32], requester_id: u64 }`, `Write { chunk_hash: [u8; 32], data: Vec<u8> }`, `Prefetch { chunk_hash: [u8; 32] }`
  - Derive Debug, Clone
- `OpPriority` enum: `Interactive` (user-facing reads), `Background` (GC, compaction writes), `Prefetch`
  - Derive Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord
  - Interactive > Prefetch > Background (in terms of priority ordering)
- `ScheduledOp` struct: `op: ChunkOp`, `priority: OpPriority`, `submitted_at_ms: u64`, `op_id: u64`
  - Derive Debug, Clone
- `SchedulerConfig` struct: `max_queue_size: usize` (default 10000), `interactive_quota: u32` (default 10, interactive ops can starve background for this many consecutive ops)
  - Derive Debug, Clone, Serialize, Deserialize
- `ChunkScheduler` struct:
  - `fn new(config: SchedulerConfig) -> Self`
  - `fn submit(&mut self, op: ChunkOp, priority: OpPriority, now_ms: u64) -> Result<u64, SchedulerError>` — enqueue op, return op_id; error if queue full
  - `fn next(&mut self) -> Option<ScheduledOp>` — dequeue highest priority op (Interactive first, then Prefetch, then Background); respect interactive_quota anti-starvation
  - `fn queue_len(&self) -> usize`
  - `fn is_empty(&self) -> bool`
  - `fn clear(&mut self)`
- `SchedulerError` enum with thiserror: `QueueFull`

Write at least **15 tests**:
1. scheduler_config_default
2. submit_single_op
3. submit_returns_unique_ids
4. next_returns_highest_priority
5. next_interactive_before_background
6. next_interactive_before_prefetch
7. next_prefetch_before_background
8. queue_full_returns_error
9. next_on_empty_returns_none
10. queue_len_after_submit
11. is_empty_initially
12. clear_empties_queue
13. interactive_quota_anti_starvation — after interactive_quota interactive ops, one background op is returned
14. op_id_monotonically_increasing
15. mixed_priority_order

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/tier_migration.rs`

Implement tier migration policies for moving data between flash and S3.

Requirements:
- `MigrationDirection` enum: `FlashToS3`, `S3ToFlash`
  - Derive Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize
- `MigrationCandidate` struct: `segment_id: u64`, `size_bytes: u64`, `last_access_ms: u64`, `access_count: u64`, `direction: MigrationDirection`, `score: f64`
  - Derive Debug, Clone
- `MigrationConfig` struct: `eviction_age_ms: u64` (default 7 days = 7*24*3600*1000), `promotion_access_count: u32` (default 3, promote S3→flash if accessed 3+ times recently), `batch_size: usize` (default 16)
  - Derive Debug, Clone, Serialize, Deserialize
- `MigrationStats` struct: `flash_to_s3_count: u64`, `flash_to_s3_bytes: u64`, `s3_to_flash_count: u64`, `s3_to_flash_bytes: u64`
  - Derive Debug, Clone, Default
- `TierMigrator` struct:
  - `fn new(config: MigrationConfig) -> Self`
  - `fn evaluate_eviction(&self, segment_id: u64, size_bytes: u64, last_access_ms: u64, now_ms: u64) -> Option<MigrationCandidate>` — if age > eviction_age_ms, return FlashToS3 candidate with score = age_seconds / (1024^2) * size_mb
  - `fn evaluate_promotion(&self, segment_id: u64, size_bytes: u64, access_count: u64) -> Option<MigrationCandidate>` — if access_count >= promotion_access_count, return S3ToFlash candidate
  - `fn select_batch(&self, candidates: &[MigrationCandidate]) -> Vec<MigrationCandidate>` — return up to batch_size candidates, sorted by score descending
  - `fn record_migration(&mut self, candidate: &MigrationCandidate)`
  - `fn stats(&self) -> &MigrationStats`

Write at least **15 tests**:
1. migration_config_default
2. migration_stats_default
3. evaluate_eviction_too_young — age < threshold → None
4. evaluate_eviction_old_enough — age > threshold → Some(FlashToS3)
5. evaluate_eviction_score_increases_with_age
6. evaluate_promotion_too_few_accesses → None
7. evaluate_promotion_enough_accesses → Some(S3ToFlash)
8. select_batch_respects_batch_size
9. select_batch_sorted_by_score
10. select_batch_empty_returns_empty
11. select_batch_fewer_than_batch_size
12. record_migration_flash_to_s3_updates_stats
13. record_migration_s3_to_flash_updates_stats
14. migration_direction_equality
15. evaluate_eviction_at_exactly_threshold

---

## EXPAND TESTS in existing modules

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/tiering.rs`
Read the file first (25 tests). Add 8 more tests.

New tests for `AccessRecord`, `TierClass`, `TierConfig`, `TierTracker`:
1. `test_tier_class_ordering` — S3 < Flash < Memory or similar ordering
2. `test_tier_config_default`
3. `test_tier_tracker_record_access`
4. `test_tier_tracker_promote_hot_data`
5. `test_tier_tracker_demote_cold_data`
6. `test_tier_tracker_multiple_accesses`
7. `test_access_record_fields`
8. `test_tier_tracker_stats`

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/quota_tracker.rs`
Read the file first (19 tests). Add 8 more tests.

New tests for `NamespaceId`, `QuotaConfig`, `QuotaTracker`, `QuotaUsage`, `QuotaViolation`:
1. `test_quota_tracker_multiple_namespaces_isolated`
2. `test_quota_tracker_near_limit`
3. `test_quota_tracker_soft_limit` — if soft limit exists, test it
4. `test_quota_usage_percentage`
5. `test_quota_violation_details`
6. `test_namespace_id_equality`
7. `test_quota_config_default`
8. `test_quota_tracker_reset_usage`

---

## Implementation instructions

1. READ each existing file before editing it
2. For new files: complete, compilable Rust with doc comments
3. For test expansions: append to existing `mod tests` blocks
4. Use `serde::{Serialize, Deserialize}`, `thiserror::Error` as needed
5. No async in new modules
6. Do NOT modify Cargo.toml

## Also update lib.rs

Add:
- `pub mod snapshot_catalog;`
- `pub mod chunk_scheduler;`
- `pub mod tier_migration;`
- Re-exports for key types

## Goal
- `cargo build -p claudefs-reduce` compiles, 0 warnings
- `cargo test -p claudefs-reduce` shows ~920+ tests
- `cargo clippy -p claudefs-reduce -- -D warnings` passes
