# Task: Write storage_erasure_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-storage` crate focusing on erasure coding integrity, superblock validation, device pool management, compaction state machine, and snapshot CoW correctness.

## File location
`crates/claudefs-security/src/storage_erasure_security_tests.rs`

## Module structure
```rust
//! Storage erasure/superblock/device/compaction/snapshot security tests.
//!
//! Part of A10 Phase 11: Storage erasure & infrastructure security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from source)

```rust
use claudefs_storage::erasure::{
    EcProfile, EcShard, StripeState, EcStripe, EcConfig, EcStats, EcError, ErasureCodingEngine,
};
use claudefs_storage::superblock::{
    Superblock, DeviceRoleCode, SUPERBLOCK_MAGIC, SUPERBLOCK_VERSION, BLOCK_SIZE,
};
use claudefs_storage::device::{
    NvmeDeviceInfo, DeviceConfig, DeviceRole, DeviceHealth, ManagedDevice, DevicePool,
};
use claudefs_storage::compaction::{
    SegmentId, SegmentInfo, CompactionConfig, CompactionState, CompactionTask, GcCandidate,
    CompactionStats, CompactionEngine,
};
use claudefs_storage::snapshot::{
    SnapshotId, SnapshotState, SnapshotInfo, CowMapping, SnapshotStats, SnapshotManager,
};
use claudefs_storage::{BlockId, BlockSize, BlockRef};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating
- `storage_security_tests.rs`: basic allocator, journal, checksum
- `storage_deep_security_tests.rs`: integrity chain, atomic write, recovery, journal, scrub, hot swap
- `storage_deep_security_tests_v2.rs`: allocator v2, block cache, quota, wear leveling, hot swap v2

DO NOT duplicate these. Focus on erasure coding, superblock, device pool, compaction, snapshot.

## Test categories (25 tests total, 5 per category)

### Category 1: Erasure Coding Security (5 tests)

1. **test_ec_profile_overhead** — Create EcProfile::ec_4_2(). Verify total_shards() == 6. Verify storage_overhead() is approximately 1.5. Verify can_tolerate_failures() == 2. Create EcProfile::ec_2_1(). Verify total_shards() == 3 and can_tolerate_failures() == 1.

2. **test_ec_encode_decode_roundtrip** — Create ErasureCodingEngine with default config. Encode a segment with known data (e.g. b"hello world" repeated to fill). Decode the stripe. Verify decoded data matches original input. (FINDING: verify data integrity preserved through encode/decode cycle).

3. **test_ec_reconstruct_missing_shard** — Create engine. Encode segment. Mark shard 0 as missing. Reconstruct shard 0. Verify the stripe is no longer degraded. Decode and verify data integrity.

4. **test_ec_too_many_missing_shards** — Create engine with ec_4_2 profile. Encode segment. Mark 3 shards as missing (exceeds parity count of 2). Try to reconstruct. Verify EcError::TooManyMissing returned.

5. **test_ec_shard_index_bounds** — Create engine. Encode segment. Try mark_shard_missing with index 10 (exceeds total_shards). Verify EcError::ShardIndexOutOfRange returned. (FINDING: out-of-bounds shard index handling).

### Category 2: Superblock Validation (5 tests)

6. **test_superblock_new_and_validate** — Create Superblock::new() with valid params. Call validate(). Verify Ok. Verify magic == SUPERBLOCK_MAGIC, version == SUPERBLOCK_VERSION, block_size == BLOCK_SIZE.

7. **test_superblock_checksum_integrity** — Create superblock. Call update_checksum(). Verify compute_checksum() matches superblock.checksum. Modify a field (e.g. mount_count += 1) WITHOUT updating checksum. Call validate(). Verify error (checksum mismatch). (FINDING: checksum detects tampering).

8. **test_superblock_serialize_roundtrip** — Create superblock. Call update_checksum(). Serialize to bytes with to_bytes(). Deserialize with from_bytes(). Verify all fields match original. Verify validate() passes on deserialized copy.

9. **test_superblock_corrupt_magic** — Create superblock. update_checksum(). to_bytes(). Corrupt first 4 bytes (magic). Call from_bytes() or validate(). Verify error (invalid magic). (FINDING: magic validation catches corruption).

10. **test_superblock_cluster_identity** — Create superblock with cluster_uuid A. Verify is_same_cluster(A) returns true. Verify is_same_cluster(B) returns false. Call increment_mount_count(). Verify mount_count increased.

### Category 3: Device Pool Management (5 tests)

11. **test_device_pool_add_and_query** — Create DevicePool. Create 3 ManagedDevice::new_mock() with different device_idx. Add all to pool. Verify len() == 3. Query by idx. Verify device(idx) returns correct device.

12. **test_device_pool_role_filtering** — Create pool with 2 Data devices and 1 Journal device. Call devices_by_role(DeviceRole::Data). Verify 2 returned. Call devices_by_role(DeviceRole::Journal). Verify 1 returned.

13. **test_device_health_defaults** — Create DeviceHealth::default(). Verify temperature_celsius == 0, percentage_used == 0, available_spare == 100, critical_warning == false, unsafe_shutdowns == 0.

14. **test_device_pool_capacity** — Create pool with 2 mock devices, each 1GB capacity. Verify total_capacity_bytes() == 2GB. Allocate some blocks from first device. Verify free_capacity_bytes() < total_capacity_bytes().

15. **test_device_fdp_zns_flags** — Create ManagedDevice::new_mock with DeviceConfig where fdp_enabled=true. Verify fdp_active() returns true. Create NvmeDeviceInfo with zns_supported=true. Create device with it. Verify zns_supported() returns true.

### Category 4: Compaction State Machine (5 tests)

16. **test_compaction_config_defaults** — Create CompactionConfig::default(). Verify min_dead_pct > 0.0 (reasonable threshold). Verify max_concurrent >= 1. Verify gc_interval_secs > 0.

17. **test_compaction_register_and_candidates** — Create CompactionEngine. Register 3 segments: one with 50% dead bytes, one with 10% dead bytes, one with 80% dead bytes. Call find_candidates(). Verify only segments above min_dead_pct are returned. Verify sorted by priority.

18. **test_compaction_task_state_machine** — Create engine. Register segment. Create compaction task. Advance task through states: Pending → Selecting → Reading → Writing → Verifying → Completed. Verify each advance returns correct state. Complete task. Verify stats updated.

19. **test_compaction_max_concurrent_limit** — Create engine with max_concurrent=1. Register 2 segments. Create first compaction task. Verify can_start_compaction() returns false. Complete first task. Verify can_start_compaction() returns true.

20. **test_compaction_fail_task** — Create engine. Register segment. Create task. Advance to Reading state. Call fail_task(). Verify state is Failed. Verify stats count failure. (FINDING: verify failed tasks free up concurrent slots).

### Category 5: Snapshot CoW Correctness (5 tests)

21. **test_snapshot_create_and_list** — Create SnapshotManager. Create 3 snapshots with different names. Verify snapshot_count() == 3. List snapshots. Verify all 3 present with unique IDs.

22. **test_snapshot_cow_mapping** — Create manager. Create snapshot. Call cow_block() to create CoW mapping for a block. Verify cow_count() for snapshot == 1. Call resolve_block(). Verify it returns the CoW copy block, not the original.

23. **test_snapshot_refcount** — Create manager. Call increment_ref for block_id 100 twice. Verify refcount(100) == 2. Call decrement_ref. Verify refcount(100) == 1. Decrement again. Verify refcount(100) == 0.

24. **test_snapshot_parent_child** — Create manager. Create parent snapshot. Create child snapshot with parent_id set to parent. Verify get_snapshot(child).parent_id == Some(parent_id). Delete parent. Verify child still accessible.

25. **test_snapshot_gc_candidates** — Create manager. Create 3 snapshots. Delete one (state should become Deleting/Deleted). Call gc_candidates(). Verify deleted snapshot is returned as a candidate. Verify active snapshots are NOT candidates.

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark findings with `// FINDING-STOR-EC-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code — all tests are synchronous
- For mock devices use `ManagedDevice::new_mock(config, capacity_4k_blocks)` where capacity_4k_blocks is number of 4KB blocks
- For DeviceConfig: `DeviceConfig::new(path, device_idx, role, fdp_enabled, queue_depth, direct_io)`
- For NvmeDeviceInfo: `NvmeDeviceInfo::new(path, serial, model, firmware, capacity, ns_id, fdp, zns, sector_size, max_transfer, io_queues)`
- For SegmentInfo: `SegmentInfo::new(id, total_bytes, live_bytes, block_count, live_block_count, created_at_secs)`
- For EcConfig::default() the segment_size may be large (2MB); use data that matches segment_size or smaller

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
