# A3 Phase 31: Blocks 1-3 Implementation (79 Tests)

## Context

This task implements three critical test blocks for ClaudeFS's Data Reduction subsystem (A3), Phase 31. These tests verify operational hardening, chaos engineering, and failure recovery under cluster conditions.

**Previous Phase (30):** 2,132 total tests ✅
**This Implementation:** +79 tests (Blocks 1-3)
**Expected Total After:** 2,211 tests

**File:** `crates/claudefs-reduce/tests/` (3 new test files)
- `cluster_dedup_consistency.rs` — Block 1: 25 tests
- `cluster_tiering_consistency.rs` — Block 2: 24 tests
- `chaos_failure_modes.rs` — Block 3: 30 tests

---

## Architecture Overview

The data reduction pipeline processes writes through:
1. **Dedupe** — BLAKE3 fingerprinting, CAS exact-match lookup
2. **Compression** — LZ4 (inline) / Zstd (async)
3. **Encryption** — AES-GCM authenticated encryption
4. **Erasure Coding** — 4+2 segment packing
5. **Tiering** — Flash → S3 migration

**Cluster characteristics:**
- Dedup shards distributed across nodes via consistent hash
- S3 is the authoritative store; local flash is a cache
- Cross-node consistency via metadata coordination
- Multi-tenant quotas and backpressure
- Crash recovery via journal replay

---

## Block 1: Cluster-Wide Dedup Consistency (25 Tests)

**File:** `crates/claudefs-reduce/tests/cluster_dedup_consistency.rs`

### Overview
Tests dedup coordination across multiple storage nodes with various failure modes. Verifies:
- Fingerprint distribution across shards
- Cache invalidation on S3 updates
- Coordinator failure handling
- Multi-tenant isolation
- Concurrent refcount updates

### Key Simulated Components

**Mock Dedup Coordinator:**
- Consistent hash ring for shard assignment (8 default shards)
- Fingerprint lookups return: `(block_id, refcount, location)`
- Shard failure modes: timeout, network partition, split-brain

**Mock Metadata Service:**
- In-memory HashMap: fingerprint → (location, refcount)
- Atomic refcount updates
- Shard election (3-way Raft quorum simulation)

**Multi-node Simulation:**
- Each test creates N `MockStorageNode` instances
- Each node has its own dedup cache + metadata view
- Writes coordinated via mock RPC with configurable latency

### Test Patterns

Each test follows:
```rust
#[tokio::test]
async fn test_name() {
    // Setup: Create N nodes, configure coordinator
    let mut nodes = vec![...];
    let coordinator = MockDedupCoordinator::new(shards=8);

    // Action: Send writes, simulate failures, etc.
    let write_result = node1.write_deduped(block_data).await;

    // Verify: Check consistency invariant
    assert!(verify_invariant(&nodes, &coordinator));
}
```

### Test List (25 tests)

1. **test_dedup_coordination_two_nodes_same_block**
   - Identical block written to 2 nodes simultaneously
   - Verify: Coordinator routes both to same dedup shard, refcount = 2
   - Assert: `block.refcount == 2`

2. **test_dedup_shard_distribution_uniform**
   - Write 1000 distinct blocks, verify shards 0-7 receive ~125 each
   - Assert: Shard distribution within 10% of mean

3. **test_dedup_routing_consistency_on_shard_failure**
   - Coordinator node (shard leader) fails
   - Remaining 2 followers should elect new leader
   - Subsequent writes should route correctly
   - Assert: No writes lost, routing continues

4. **test_dedup_fingerprint_cache_staleness**
   - Cache fingerprint locally
   - Simulate S3 update from another site
   - Verify cache invalidation
   - Assert: Next lookup fetches fresh from metadata

5. **test_dedup_coordination_retry_on_network_delay**
   - Introduce 100ms RPC latency
   - Write 100 blocks
   - Assert: All writes complete (no timeout), latency absorbed

6. **test_dedup_lock_timeout_and_fallback**
   - Deadlock scenario: node A holds lock, requests from node B
   - Timeout after 1s
   - Fallback to local dedup (no coordination)
   - Assert: Writes proceed (lower efficiency but no hang)

7. **test_dedup_collision_probability_blake3**
   - Generate 1M random fingerprints
   - Assert: 0 collisions (probability < 1e-12)

8. **test_dedup_multi_tenant_isolation_fingerprints**
   - Tenant A writes block X with fingerprint FP1
   - Tenant B writes block Y with fingerprint FP1 (same data, different tenant)
   - Verify: No cross-tenant dedup (blocks separate in metadata)
   - Assert: 2 distinct physical blocks stored

9. **test_dedup_cross_tenant_fingerprint_collision_handled**
   - Same fingerprint, different tenants
   - Verify: Blocks separated by tenant_id key in metadata
   - Assert: No corruption when reading across tenants

10. **test_dedup_coordinator_overload_backpressure**
    - Coordinator receives 10k ops/s (overloaded)
    - Verify: Backpressure activates, response times increase gracefully
    - Assert: No panics, no hangs (degradation acceptable)

11. **test_dedup_cache_eviction_under_memory_pressure**
    - Add 1000 unique fingerprints to cache (simulated 10MB limit)
    - Verify LRU eviction
    - Assert: Cache size bounded at 10MB, oldest entries evicted first

12. **test_dedup_batch_coordination_efficient_routing**
    - Batch 100 fingerprint lookups in one RPC (vs 100 individual RPCs)
    - Assert: Batch throughput > individual throughput by 5x

13. **test_dedup_consistency_check_on_read_path**
    - Write block, modify metadata to mismatch fingerprint
    - Read block, verify mismatch detected
    - Assert: Checksum mismatch error raised

14. **test_dedup_tombstone_handling_after_delete**
    - Write block (refcount=1), delete, refcount should become 0
    - Verify: Metadata tombstone created
    - Assert: Subsequent writes reuse same location

15. **test_dedup_refcount_coordination_update_race**
    - Node A and B both reference same block (race on refcount increment)
    - Use atomic operations
    - Assert: Refcount = 2 (no lost update)

16. **test_dedup_coordinator_election_on_failure**
    - Kill shard leader (node with Raft term highest)
    - Followers should elect new leader
    - Assert: Election time < 1s, writes resume

17. **test_dedup_log_replay_consistency**
    - Crash during dedup coordination RPC
    - Replay journal on recovery
    - Assert: Dedup state consistent with journal

18. **test_dedup_similarity_detection_cross_node**
    - Node A and B write similar blocks (90% same content)
    - Similarity tier coordinator finds match
    - Assert: Delta stored instead of full block

19. **test_dedup_bandwidth_throttle_per_tenant**
    - Set dedup throughput limit per tenant (e.g., 100MB/s)
    - Tenant writes 200MB/s
    - Assert: Backpressure activates, sustained rate = 100MB/s

20. **test_dedup_cascade_failure_three_node_outage**
    - 3-node shard group, lose 2 nodes
    - System should degrade gracefully (minority shard unavailable)
    - Assert: Writes for other shards proceed

21. **test_dedup_snapshot_consistency_with_active_writes**
    - Create snapshot while dedup coordination ongoing
    - Assert: Snapshot includes correct block pointers

22. **test_dedup_worm_enforcement_prevents_block_reuse**
    - Write WORM block, attempt dedup of same data
    - Assert: WORM block not reused (creates new block)

23. **test_dedup_key_rotation_updates_fingerprints**
    - Rotate encryption key
    - Verify: Fingerprint metadata updated (plaintext FP unchanged)
    - Assert: Dedup still works post-rotation

24. **test_dedup_concurrent_write_and_tiering**
    - Write block to flash, dedup coordinates lookup
    - Meanwhile, tiering background task runs
    - Assert: Write and tiering don't corrupt state

25. **test_dedup_recovery_after_coordinator_split_brain**
    - Network partition: 2 nodes think they're leaders
    - Partition heals
    - Majority quorum should win
    - Assert: Split-brain resolved via quorum voting

---

## Block 2: Tier Migration & S3 Consistency (24 Tests)

**File:** `crates/claudefs-reduce/tests/cluster_tiering_consistency.rs`

### Overview
Tests tiering logic (flash → S3) under cluster and S3 backend failures. Verifies:
- Incomplete uploads handled correctly
- Backpressure when S3 slow
- Cache invalidation on S3 updates
- Multi-tenant tiering budgets
- Disaster recovery from S3

### Mock S3 Backend

```rust
struct MockS3Backend {
    objects: HashMap<String, Vec<u8>>,
    failure_mode: FailureMode,
    latency: Duration,
}

enum FailureMode {
    None,
    SlowWrite(Duration),    // Simulate 500ms latency
    CorruptedRead,          // Flip bits on GET
    PartialUpload(usize),   // Upload truncated at N bytes
    NetworkTimeout,         // Timeout after 5s
    KeyNotFound,            // Return 404
}
```

### Test Patterns

Tiering tests verify:
- High watermark (80% flash full) triggers eviction
- Low watermark (60%) stops eviction
- Failed uploads retried with exponential backoff
- Checksum validated on tiered block read

### Test List (24 tests)

1. **test_tier_migration_hot_to_cold_complete_flow**
   - Write block to flash, trigger tiering (80% full)
   - Verify: Block uploaded to S3, deleted from flash
   - Assert: Block retrievable from S3

2. **test_tier_migration_partial_failure_incomplete_upload**
   - S3 upload reaches 50%, then fails
   - Verify: Retry succeeds on retry
   - Assert: Block eventually in S3

3. **test_tier_migration_network_timeout_retry_backoff**
   - S3 timeout on first attempt (5s)
   - Retry with exponential backoff (1s, 2s, 4s, 8s)
   - Assert: Max 3 retries, backoff respected

4. **test_tier_migration_s3_slow_write_backpressure**
   - S3 latency 500ms (slow)
   - Write rate 1GB/s to flash
   - Assert: Backpressure activates, write rate throttles

5. **test_tier_migration_concurrent_eviction_and_read**
   - Evict block B to S3 while reading same block B
   - Assert: Read gets block from flash (not yet evicted)

6. **test_tier_migration_space_pressure_triggers_rapid_tiering**
   - Flash at 95% capacity
   - Verify: Aggressive tiering starts (LRU score relaxed)
   - Assert: Eviction rate increases

7. **test_tier_migration_refetch_on_missing_s3_block**
   - Block missing from S3 (corruption/deletion)
   - Attempt to read from S3, get 404
   - Refetch from EC replicas
   - Assert: Block recovered correctly

8. **test_tier_migration_cache_invalidation_on_s3_update**
   - Cache block in flash
   - Simulate external S3 update (different site)
   - Read cache gets stale data
   - Assert: Cache invalidation on next metadata sync

9. **test_tier_migration_multi_tenant_isolation_tiering_rate**
   - Tenant A: 100MB/s tiering budget
   - Tenant B: 50MB/s tiering budget
   - Assert: Each tenant's tiering rate limited independently

10. **test_tier_migration_cold_region_latency_simulation**
    - Simulate 500ms S3 latency (cold region)
    - Tiering should adjust backoff accordingly
    - Assert: Tiering throughput reduced but stable

11. **test_tier_migration_snapshot_cold_tier_consistency**
    - Create snapshot, some blocks in flash, some in S3
    - Read snapshot
    - Assert: Snapshot includes both cold and hot blocks correctly

12. **test_tier_migration_worm_blocks_not_tiered**
    - Write WORM block to flash
    - Trigger tiering (80% full)
    - Assert: WORM block NOT evicted (stays in flash)

13. **test_tier_migration_expiry_policy_removes_old_blocks**
    - Write block, set TTL=1 day
    - Advance time 2 days
    - Trigger GC
    - Assert: Block removed (and from S3)

14. **test_tier_migration_concurrent_tiering_multiple_nodes**
    - 5 nodes tier to same S3 bucket
    - Verify: Object key collision avoidance
    - Assert: All blocks unique in S3

15. **test_tier_migration_s3_corruption_detection_via_checksum**
    - Tier block to S3, corrupt 1 bit
    - Read block, verify checksum fails
    - Assert: Corruption detected, error returned

16. **test_tier_migration_s3_object_tagging_metadata**
    - Tier block with metadata (tenant_id, age, compression)
    - Verify: S3 object tagged correctly
    - Assert: Tags readable on GET

17. **test_tier_migration_ec_parity_blocks_tiered_together**
    - EC stripe (4 data + 2 parity)
    - Tier data blocks, parity must follow
    - Assert: All 6 blocks in S3 (or all in flash)

18. **test_tier_migration_journal_log_for_tiering_decisions**
    - Log all tiering decisions to journal
    - Crash, replay journal
    - Assert: Tiering decisions same order as before

19. **test_tier_migration_cross_site_replication_tiering**
    - Site A tiers block to S3
    - Site B should be notified (via replication conduit)
    - Assert: Both sites know block is cold

20. **test_tier_migration_s3_delete_on_local_deletion**
    - Delete block from flash, should be deleted from S3
    - Verify: S3 GET returns 404
    - Assert: Object truly deleted

21. **test_tier_migration_multi_region_s3_failover**
    - Primary S3 region down
    - Failover to secondary region
    - Assert: Tiering continues (new region available)

22. **test_tier_migration_bandwidth_throttle_tiering_rate**
    - Set tiering bandwidth limit (100MB/s)
    - Attempt 200MB/s tiering
    - Assert: Sustained rate = 100MB/s

23. **test_tier_migration_concurrent_write_and_tiering_same_block**
    - Write arrives while block is mid-tiering to S3
    - Assert: Write completes, tiering doesn't corrupt state

24. **test_tier_migration_disaster_recovery_s3_rebuild**
    - Flash layer completely lost (crash)
    - Rebuild from S3
    - Assert: All blocks recovered correctly

---

## Block 3: Chaos Engineering & Failure Modes (30 Tests)

**File:** `crates/claudefs-reduce/tests/chaos_failure_modes.rs`

### Overview
Tests recovery and correctness under injected failures. Verifies:
- Crash recovery during pipeline stages
- Storage node failures
- Network partitions
- Disk corruption detection
- Concurrent failure scenarios

### Chaos Injection Framework

```rust
pub struct ChaosInjector {
    failure_point: FailurePoint,
    probability: f32,  // 0.0-1.0: likelihood of injection
}

pub enum FailurePoint {
    DuringDedup,
    DuringCompression,
    DuringEncryption,
    DuringEC,
    DuringS3Upload,
    NetworkPartition(Duration),
    DiskCorruption,
    OOM,
}

impl ChaosInjector {
    fn inject_failure(&self, rng: &mut SeededRng) -> Option<ChaoticError> {
        if rng.gen::<f32>() < self.probability {
            Some(match self.failure_point { ... })
        } else {
            None
        }
    }
}
```

### Test Patterns

Each chaos test:
1. Inject failure at specific point
2. Crash and journal replay (or retry)
3. Verify consistency: blocks not lost, metadata consistent

### Test List (30 tests)

1. **test_crash_during_write_dedup_recovery**
   - Crash during dedup fingerprint resolution
   - Journal replay on restart
   - Assert: Dedup state consistent (no double-count or lost refcount)

2. **test_crash_during_compression_recovery**
   - Crash mid-LZ4 compression
   - Restart, replay
   - Assert: Compressed block either complete or rolled back

3. **test_crash_during_encryption_recovery**
   - Crash mid-AES-GCM encryption
   - Restart, replay
   - Assert: Encrypted block either complete or rolled back

4. **test_crash_during_ec_encoding_recovery**
   - Crash during EC 4+2 parity computation
   - Restart, replay
   - Assert: Parity blocks all present or all absent (no partial)

5. **test_crash_during_s3_upload_recovery**
   - Crash during S3 PUT (bytes 0-50% uploaded)
   - Restart, verify orphan detection
   - Assert: Orphan cleaned up, block re-tiered on retry

6. **test_storage_node_failure_dedup_coordinator_election**
   - Dedup coordinator node fails
   - Remaining nodes elect new coordinator
   - Assert: Election < 1s, dedup coordination resumes

7. **test_storage_node_failure_journal_recovery_other_node**
   - Node A fails, Node B picks up journal
   - Replay Node A's pending writes
   - Assert: No data loss, writes eventually complete

8. **test_network_partition_dedup_coordination_timeout**
   - Network partition 5s, dedup coordination stalled
   - Partition heals after 5s
   - Assert: Writes resume (with backoff)

9. **test_network_partition_s3_upload_retry_after_partition_heals**
   - S3 upload mid-partition (connection dies)
   - Partition heals
   - Retry succeeds
   - Assert: Block eventually in S3

10. **test_disk_corruption_checksum_detects_write_path**
    - Corrupt block on flash during write (flip 1 bit)
    - Verify: Write checksum catches it
    - Assert: Corruption detected before commit

11. **test_disk_corruption_checksum_detects_read_path**
    - Block in flash, corrupt 1 bit after write
    - Read block
    - Assert: Checksum mismatch on read

12. **test_memory_exhaustion_quota_enforcement_prevents_oom**
    - Memory quota 500MB, writes attempt 1GB
    - Assert: Backpressure stops writes before OOM

13. **test_memory_exhaustion_gc_runs_to_recover_space**
    - Memory low (>80% quota)
    - GC triggered
    - Assert: GC frees space, quota recovers

14. **test_file_descriptor_exhaustion_backpressure**
    - FD limit 256, open 250 files
    - Write more blocks (would exceed FD limit)
    - Assert: Backpressure activates, no EMFILE

15. **test_concurrent_write_read_same_block_consistency**
    - Thread A writing block X
    - Thread B reading block X (race)
    - Assert: B reads either old version or new version (not corrupt)

16. **test_concurrent_dedup_same_fingerprint_coordination**
    - 2 nodes deduplicate same fingerprint simultaneously
    - Assert: Refcount = 2 (no lost increment)

17. **test_concurrent_gc_and_write_refcount_consistency**
    - GC walks refcount table
    - Write increments same block's refcount
    - Assert: Refcount correct after both complete

18. **test_concurrent_tiering_and_read_cache_coherency**
    - Block in read cache
    - Tiering evicts block to S3
    - Next read fetches from S3 (not stale cache)
    - Assert: Cache invalidated

19. **test_gc_with_pending_journal_entries_ordering**
    - Multiple journal entries pending (write A, write B)
    - GC runs concurrently
    - Assert: GC respects journal ordering (doesn't GC B before A)

20. **test_encryption_key_rotation_mid_write_session**
    - Rotate encryption key during active writes
    - Assert: Writes complete (old key still usable)

21. **test_encryption_key_rotation_orphan_blocks_reencrypted**
    - Key rotation, orphan blocks identified
    - Background task re-encrypts with new key
    - Assert: All blocks encrypted with new key

22. **test_quota_update_mid_write_session**
    - Quota 1GB, writes 500MB
    - Quota decreased to 250MB (backpressure should activate)
    - Assert: Further writes backpressed

23. **test_tenant_deletion_cascading_block_cleanup**
    - Tenant A owns 100 blocks
    - Delete tenant
    - GC should clean all 100 blocks
    - Assert: 0 blocks remain for tenant A

24. **test_snapshot_freezes_state_during_writes**
    - Create snapshot S1
    - Concurrent writes to filesystem
    - Assert: S1 consistent (doesn't see new writes)

25. **test_worm_enforcement_cant_overwrite_after_retention**
    - Write WORM block, set retention 1 day
    - Advance time 2 days
    - Attempt overwrite
    - Assert: Overwrite rejected (retention enforced)

26. **test_erasure_coding_block_loss_recovery**
    - EC stripe (4+2), lose 2 blocks
    - Reconstruct via remaining 4
    - Assert: Reconstructed blocks match original

27. **test_replication_lag_on_journal_recovery**
    - Site A fails
    - Site B (replica, lagging 10s)
    - Recover Site A from Site B
    - Assert: Last 10s of writes may be lost (acceptable)

28. **test_cross_site_write_conflict_resolution**
    - Same inode written at Site A (timestamp T1) and Site B (timestamp T2)
    - T1 > T2 (A's write is newer)
    - Assert: LWW (last-write-wins) resolves to A's version

29. **test_cascading_node_failures_three_node_outage**
    - Node 1 fails, system continues (quorum active)
    - Node 2 fails, system continues (minority shard unavailable)
    - Node 3 fails, entire system down
    - Assert: Graceful degradation at each step

30. **test_recovery_from_cascading_failures**
    - Cascade complete (all down)
    - Nodes restart one-by-one
    - Assert: System comes back online, data intact

---

## Implementation Notes

### Async Runtime
- All tests use `#[tokio::test]`
- Timeouts via `tokio::time::timeout()` (prevent hanging)
- Concurrent ops via `tokio::task::spawn()`

### Determinism
- Use seeded `rand::StdRng` (reproducible)
- No wall-clock timing assertions
- Logical event counters instead

### Cleanup
- Each test allocates `/tmp/claudefs_test_<uuid>/`
- Cleanup on completion (even on failure)

### Performance Baselines
- Use reference metrics from Phase 30
- Performance assertions use ranges (±10% tolerance)

---

## Success Criteria

✅ All 79 tests compile (no warnings)
✅ All 79 tests pass (100% pass rate)
✅ No memory leaks (valgrind clean)
✅ No deadlocks
✅ Crash recovery RTO < 30s
✅ Multi-node consistency maintained

---

## Files to Create

1. `crates/claudefs-reduce/tests/cluster_dedup_consistency.rs` — Block 1
2. `crates/claudefs-reduce/tests/cluster_tiering_consistency.rs` — Block 2
3. `crates/claudefs-reduce/tests/chaos_failure_modes.rs` — Block 3

---

## Build & Test

```bash
cargo test -p claudefs-reduce --test cluster_dedup_consistency
cargo test -p claudefs-reduce --test cluster_tiering_consistency
cargo test -p claudefs-reduce --test chaos_failure_modes
cargo test -p claudefs-reduce  # All tests
cargo clippy -p claudefs-reduce
```

Expected: 79 new tests passing, 0 warnings.

---
