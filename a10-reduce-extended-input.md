# Task: Write reduce_extended_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-reduce` crate focusing on write path stats, WORM policy enforcement, key rotation scheduling, GC mark phase, and segment packing integrity.

## File location
`crates/claudefs-security/src/reduce_extended_security_tests.rs`

## Module structure
```rust
//! Extended security tests for claudefs-reduce: write path, WORM, key rotation, GC, segments.
//!
//! Part of A10 Phase 10: Reduce extended security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs)

```rust
use claudefs_reduce::{
    // Write path stats
    PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats,
    // GC
    GcConfig, GcEngine, GcStats,
    // Segment
    Segment, SegmentEntry, SegmentPacker, SegmentPackerConfig,
    // Snapshot
    Snapshot, SnapshotConfig, SnapshotInfo, SnapshotManager,
    // Encryption
    EncryptionAlgorithm, EncryptionKey,
    // Key management
    DataKey, KeyManager, KeyManagerConfig, KeyVersion, VersionedKey, WrappedKey,
    // Dedup
    CasIndex, Chunk, Chunker, ChunkerConfig, ChunkHash,
    // Compression
    CompressionAlgorithm,
    // Checksum
    ChecksumAlgorithm, ChecksummedBlock, DataChecksum,
    // Error
    ReduceError,
};
// Module-path imports:
use claudefs_reduce::worm_reducer::{WormMode, WormReducer, RetentionPolicy};
use claudefs_reduce::key_rotation_scheduler::{KeyRotationScheduler, RotationStatus};
use claudefs_reduce::write_path::{WritePathConfig, WritePathStats};
use claudefs_reduce::fingerprint::{blake3_hash, super_features};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating
- `reduce_security_tests.rs`: basic encryption, chunker, CAS index, compression, checksum, pipeline, GC basics, segment basics, snapshot basics
- `reduce_deep_security_tests.rs`: deterministic DEK, key rotation, nonce uniqueness, refcount, drain unreferenced, compression roundtrip, checksum corruption, pipeline dedup, GC sweep, snapshot max

DO NOT duplicate these. Focus on WORM enforcement, write path stats, key rotation scheduler, GC mark edge cases, and extended segment/snapshot tests.

## Test categories (25 tests total, 5 per category)

### Category 1: WORM Policy Enforcement (5 tests)

1. **test_worm_none_always_expired** — Create RetentionPolicy::none(). Call is_expired(0). Verify true. Call is_expired(u64::MAX). Verify true.

2. **test_worm_legal_hold_never_expires** — Create RetentionPolicy::legal_hold(). Call is_expired(0). Verify false. Call is_expired(u64::MAX). Verify false.

3. **test_worm_immutable_expiry_boundary** — Create RetentionPolicy::immutable_until(100). Call is_expired(99). Verify false. Call is_expired(100). Verify true (at exactly retain_until). Call is_expired(101). Verify true.

4. **test_worm_reducer_policy_upgrade** — Create WormReducer. Register hash with RetentionPolicy::none(). Register same hash with RetentionPolicy::legal_hold(). Verify the policy is upgraded (legal_hold wins). (FINDING: if downgrade allowed, compliance violation).

5. **test_worm_reducer_active_count** — Create WormReducer. Register 3 chunks: one with none (expired), one with immutable_until(1000), one with legal_hold. Call active_count(500). Verify returns 2 (none expired, immutable not yet, legal hold active).

### Category 2: Key Rotation Scheduler (5 tests)

6. **test_rotation_initial_state_idle** — Create KeyRotationScheduler. Verify status is Idle. Verify no chunks need rotation.

7. **test_rotation_schedule_from_idle** — Create KeyRotationScheduler. Register some chunks. Call schedule_rotation(target_version=2). Verify status changes to Scheduled.

8. **test_rotation_double_schedule_fails** — Create KeyRotationScheduler. Schedule rotation. Try to schedule again. Verify error (already scheduled).

9. **test_rotation_mark_needs_rotation** — Create KeyRotationScheduler. Register 5 chunks with version 1. Call mark_needs_rotation(version=1). Verify all 5 are marked as needing rotation.

10. **test_rotation_register_chunk** — Create KeyRotationScheduler. Register chunk_id 1 with a WrappedKey. Verify registration succeeds. Register same chunk_id again. Document whether duplicate overwrites or is rejected.

### Category 3: GC Extended Security (5 tests)

11. **test_gc_config_defaults** — Create GcConfig::default(). Verify sweep_threshold, batch_size, and other defaults have safe values (not zero, not u64::MAX).

12. **test_gc_stats_initial** — Create GcEngine. Get stats. Verify all counters are 0 initially (chunks_scanned, chunks_reclaimed, bytes_reclaimed).

13. **test_gc_mark_before_sweep** — Create GcEngine. DO NOT mark any chunks. Add chunks to CAS index. Sweep. Verify all chunks are collected (nothing marked = nothing retained). (FINDING: empty mark phase deletes everything).

14. **test_gc_mark_and_retain** — Create GcEngine. Add 3 chunks to CAS index. Mark 2 as reachable. Sweep. Verify only 1 is collected and 2 are retained.

15. **test_gc_multiple_cycles** — Create GcEngine. Run 3 mark-sweep cycles. Verify stats accumulate across cycles (chunks_scanned increases).

### Category 4: Write Path & Pipeline Stats (5 tests)

16. **test_pipeline_config_defaults** — Create PipelineConfig::default(). Verify compression and checksum have safe defaults. Document encryption defaults.

17. **test_reduction_stats_ratio** — Create ReductionStats with input_bytes=1000 and stored_bytes=500. Verify reduction_ratio() returns approximately 2.0 (or 0.5 depending on definition).

18. **test_reduction_stats_zero_stored** — Create ReductionStats with stored_bytes=0. Call reduction_ratio(). Verify it doesn't panic (handles division by zero). (FINDING: zero stored bytes could cause panic).

19. **test_chunker_config_validation** — Create ChunkerConfig with min_chunk_size > max_chunk_size. Document whether this is caught during construction or causes runtime issues.

20. **test_cas_index_insert_duplicate** — Create CasIndex. Insert same hash twice. Verify refcount is 2 (not 1).

### Category 5: Snapshot & Segment Extended (5 tests)

21. **test_snapshot_create_and_list** — Create SnapshotManager. Create 3 snapshots. List all. Verify count is 3 and IDs are unique.

22. **test_snapshot_delete_nonexistent** — Create SnapshotManager. Try to delete a snapshot ID that doesn't exist. Verify error or false returned.

23. **test_segment_packer_seal_empty** — Create SegmentPacker. Seal without adding any entries. Document whether empty segment is created or error returned.

24. **test_segment_entry_integrity** — Create SegmentPacker. Add 5 entries with known data. Seal. Verify segment.entries.len() == 5 and verify_integrity() passes.

25. **test_segment_packer_config_defaults** — Create SegmentPackerConfig::default(). Verify segment_size and other defaults are reasonable (not 0, not unreasonably large).

## Implementation notes
- Use `fn make_xxx()` helper functions
- Mark findings with `// FINDING-REDUCE-EXT-XX: description`
- If a type is not public, skip and replace with alternative
- DO NOT use async code — all tests synchronous
- Use `assert!`, `assert_eq!`, `matches!`

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
