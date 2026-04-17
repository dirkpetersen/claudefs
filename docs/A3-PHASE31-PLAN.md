# A3: Data Reduction — Phase 31 Plan
## Operational Hardening & Cluster Testing

**Status:** 🟡 **PHASE 31 PLANNING** | **Date:** 2026-04-17
**Agent:** A3 (Data Reduction)
**Tests:** +100-150 operational hardening tests (target: 2250-2280 total)

---

## Overview

Phase 31 implements comprehensive cluster-wide operational hardening, chaos engineering, and production simulation tests for the data reduction subsystem. While Phase 30 focused on single-node integration testing with real modules, Phase 31 verifies behavior under realistic failure modes, concurrent cluster operations, and sustained production workloads.

**Previous Phase (30):** 61 integration tests → 2132 total tests ✅
**Phase 31 Target:** +100-150 operational tests
**Phase 31 Expected Total:** 2250-2280 tests

---

## Architecture Context

The data reduction pipeline operates at the storage node level with these critical cluster characteristics:

- **Distributed dedup coordination** — Dedup shards distributed across cluster nodes, fingerprint routing with consistency guarantees
- **Tier migration** — Hot (flash) ↔ Cold (S3) transitions triggered by space pressure, hits, or policy
- **Cross-node consistency** — S3 as authoritative store; local caches must maintain invalidation
- **Multi-tenant isolation** — Separate quota tracking, backpressure, and isolation per tenant
- **Failure recovery** — Journal replay on crash, GC coordination, refcount consistency under node failures

Phase 31 tests verify these properties under cluster conditions:
- Multi-node write conflicts (same block via different nodes)
- Network partition scenarios (dedup coordination timeout)
- Storage node failures (journal recovery, GC coordination)
- S3 backend failures (tiering retry, fallback, corruption detection)
- Resource exhaustion (quota enforcement, backpressure under sustained load)

---

## Phase 31 Test Blocks

### Block 1: Cluster-Wide Dedup Consistency (25 tests)
**File:** `crates/claudefs-reduce/tests/cluster_dedup_consistency.rs`

Tests dedup coordination across multiple storage nodes with various failure modes:

1. `test_dedup_coordination_two_nodes_same_block` - Identical block written to 2 nodes simultaneously
2. `test_dedup_shard_distribution_uniform` - Verify fingerprints hash-distributed across 8 shards
3. `test_dedup_routing_consistency_on_shard_failure` - Coordinator node failure handling
4. `test_dedup_fingerprint_cache_staleness` - Cache invalidation on S3 updates
5. `test_dedup_coordination_retry_on_network_delay` - Network partition tolerance
6. `test_dedup_lock_timeout_and_fallback` - Deadlock detection and fallback to local dedup
7. `test_dedup_collision_probability_blake3` - Verify Blake3 collision likelihood < 1e-12
8. `test_dedup_multi_tenant_isolation_fingerprints` - Tenant A blocks don't affect tenant B dedup
9. `test_dedup_cross_tenant_fingerprint_collision_handled` - Same fingerprint, different tenants
10. `test_dedup_coordinator_overload_backpressure` - Coordinator rate limiting behavior
11. `test_dedup_cache_eviction_under_memory_pressure` - Cache LRU behavior with 1000+ unique blocks
12. `test_dedup_batch_coordination_efficient_routing` - Batch multiple fingerprints in one RPC
13. `test_dedup_consistency_check_on_read_path` - Verify stored block matches fingerprint
14. `test_dedup_tombstone_handling_after_delete` - Dedup entry cleanup
15. `test_dedup_refcount_coordination_update_race` - Concurrent refcount updates across nodes
16. `test_dedup_coordinator_election_on_failure` - Leader election in dedup shard group
17. `test_dedup_log_replay_consistency` - Journal replay restores dedup state correctly
18. `test_dedup_similarity_detection_cross_node` - Similarity tier coordination
19. `test_dedup_bandwidth_throttle_per_tenant` - QoS dedup throughput limits
20. `test_dedup_cascade_failure_three_node_outage` - Graceful degradation with 2 of 3 nodes down
21. `test_dedup_snapshot_consistency_with_active_writes` - Snapshots during dedup coordination
22. `test_dedup_worm_enforcement_prevents_block_reuse` - WORM blocks not deduplicated
23. `test_dedup_key_rotation_updates_fingerprints` - Key rotation doesn't invalidate dedup
24. `test_dedup_concurrent_write_and_tiering` - Write dedup while tiering active
25. `test_dedup_recovery_after_coordinator_split_brain` - Majority quorum prevents divergence

**Status:** Planned (to be implemented)

---

### Block 2: Tier Migration & S3 Consistency (24 tests)
**File:** `crates/claudefs-reduce/tests/cluster_tiering_consistency.rs`

Tests tiering logic under cluster and S3 backend failures:

1. `test_tier_migration_hot_to_cold_complete_flow` - Full pipeline from flash to S3
2. `test_tier_migration_partial_failure_incomplete_upload` - Incomplete S3 PUT handling
3. `test_tier_migration_network_timeout_retry_backoff` - Exponential backoff retry strategy
4. `test_tier_migration_s3_slow_write_backpressure` - Backpressure when S3 slow (>100ms)
5. `test_tier_migration_concurrent_eviction_and_read` - Read while evicting same block
6. `test_tier_migration_space_pressure_triggers_rapid_tiering` - Aggressive tiering when <10% free
7. `test_tier_migration_refetch_on_missing_s3_block` - Block missing from S3, refetch from replicas
8. `test_tier_migration_cache_invalidation_on_s3_update` - Cache hit rate drops on S3-side update
9. `test_tier_migration_multi_tenant_isolation_tiering_rate` - Separate tiering budgets per tenant
10. `test_tier_migration_cold_region_latency_simulation` - Simulate 500ms S3 latency
11. `test_tier_migration_snapshot_cold_tier_consistency` - Snapshot includes cold blocks
12. `test_tier_migration_worm_blocks_not_tiered` - WORM blocks stay in flash
13. `test_tier_migration_expiry_policy_removes_old_blocks` - TTL-based eviction
14. `test_tier_migration_concurrent_tiering_multiple_nodes` - N nodes tiering to same S3 bucket
15. `test_tier_migration_s3_corruption_detection_via_checksum` - Corrupted block detected on read
16. `test_tier_migration_s3_object_tagging_metadata` - Tags preserve block metadata
17. `test_tier_migration_ec_parity_blocks_tiered_together` - EC stripe consistency during tiering
18. `test_tier_migration_journal_log_for_tiering_decisions` - Replay produces same tiering order
19. `test_tier_migration_cross_site_replication_tiering` - Tiering decision sync between sites
20. `test_tier_migration_s3_delete_on_local_deletion` - Cleanup S3 on block delete
21. `test_tier_migration_multi_region_s3_failover` - Primary S3 region down, use secondary
22. `test_tier_migration_bandwidth_throttle_tiering_rate` - Throttle tiering to <100MB/s
23. `test_tier_migration_concurrent_write_and_tiering_same_block` - Write arrives while tiering
24. `test_tier_migration_disaster_recovery_s3_rebuild` - Rebuild from S3 after flash failure

**Status:** Planned (to be implemented)

---

### Block 3: Chaos Engineering & Failure Modes (30 tests)
**File:** `crates/claudefs-reduce/tests/chaos_failure_modes.rs`

Tests recovery and correctness under injected failures:

1. `test_crash_during_write_dedup_recovery` - Crash during dedup fingerprint resolution
2. `test_crash_during_compression_recovery` - Crash mid-compression, verify journal replay
3. `test_crash_during_encryption_recovery` - Crash mid-encryption, verify key derivation
4. `test_crash_during_ec_encoding_recovery` - Crash mid-EC parity block computation
5. `test_crash_during_s3_upload_recovery` - Crash mid-S3 upload, verify orphan detection
6. `test_storage_node_failure_dedup_coordinator_election` - Node fails, coordinator elected
7. `test_storage_node_failure_journal_recovery_other_node` - Another node recovers journal
8. `test_network_partition_dedup_coordination_timeout` - Partition for 5s, retry succeeds
9. `test_network_partition_s3_upload_retry_after_partition_heals` - Upload resumes after healing
10. `test_disk_corruption_checksum_detects_write_path` - Corruption on write detected
11. `test_disk_corruption_checksum_detects_read_path` - Corruption on read detected
12. `test_memory_exhaustion_quota_enforcement_prevents_oom` - Quota stops writes before OOM
13. `test_memory_exhaustion_gc_runs_to_recover_space` - GC triggered, space recovered
14. `test_file_descriptor_exhaustion_backpressure` - FD limit reached, backpressure activates
15. `test_concurrent_write_read_same_block_consistency` - Write and read same block race
16. `test_concurrent_dedup_same_fingerprint_coordination` - Two nodes deduplicate same block
17. `test_concurrent_gc_and_write_refcount_consistency` - GC and write race on refcount
18. `test_concurrent_tiering_and_read_cache_coherency` - Read cache invalidated during tiering
19. `test_gc_with_pending_journal_entries_ordering` - GC respects journal ordering
20. `test_encryption_key_rotation_mid_write_session` - Key rotation during active writes
21. `test_encryption_key_rotation_orphan_blocks_reencrypted` - Orphans re-encrypted with new key
22. `test_quota_update_mid_write_session` - Quota decreased, backpressure activates
23. `test_tenant_deletion_cascading_block_cleanup` - Delete tenant, all blocks GC'd
24. `test_snapshot_freezes_state_during_writes` - Snapshot consistent despite concurrent writes
25. `test_worm_enforcement_cant_overwrite_after_retention` - WORM prevents overwrite
26. `test_erasure_coding_block_loss_recovery` - Lose parity block, recover via other nodes
27. `test_replication_lag_on_journal_recovery` - Site B lags, can still recover A's blocks
28. `test_cross_site_write_conflict_resolution` - Same inode written at both sites, LWW wins
29. `test_cascading_node_failures_three_node_outage` - Progressive failures, system degrades
30. `test_recovery_from_cascading_failures` - Cascade complete, recovery brings nodes back

**Status:** Planned (to be implemented)

---

### Block 4: Performance & Scalability (25 tests)
**File:** `crates/claudefs-reduce/tests/performance_scalability.rs`

Tests performance characteristics under realistic cluster load:

1. `test_throughput_single_large_write_100gb` - Single 100GB write throughput
2. `test_throughput_concurrent_writes_16_nodes_10gb_each` - 16 nodes × 10GB = 160GB total
3. `test_throughput_with_dedup_enabled_90percent_similarity` - Dedup with high similarity
4. `test_throughput_with_compression_enabled_8x_ratio` - 8:1 compression ratio
5. `test_throughput_with_ec_enabled_stripe_distribution` - EC overhead impact
6. `test_latency_small_write_p50_p99_p99p9` - Latency percentiles: p50, p99, p99.9
7. `test_latency_write_path_stages_breakdown` - Latency per stage: chunk, dedup, compress, encrypt, ec
8. `test_amplification_write_amplification_with_tiering_active` - Write amplification metric
9. `test_amplification_read_amplification_ec_reconstruction` - Read amplification from EC
10. `test_cache_hit_rate_vs_cache_size_curve` - Cache performance curve (16MB to 1GB)
11. `test_dedup_coordination_latency_p99_under_load` - Dedup RPC latency at 100k ops/s
12. `test_quota_enforcement_latency_impact` - Quota checks per write
13. `test_backpressure_response_time_degradation` - Response time as backpressure activates
14. `test_scaling_nodes_linear_throughput_4_to_16_nodes` - Verify linear scaling
15. `test_scaling_dedup_shards_throughput_vs_shard_count` - Shard count vs throughput
16. `test_scaling_gc_threads_throughput_impact` - GC thread count vs throughput
17. `test_memory_usage_per_node_under_1tb_data` - Memory overhead calculation
18. `test_memory_usage_cache_overhead_per_gb_cached` - Cache memory per GB
19. `test_cpu_usage_dedup_coordination_per_100k_fps_s` - CPU per dedup ops
20. `test_cpu_usage_compression_per_gb_s` - CPU per GB/s compression
21. `test_cpu_usage_encryption_per_gb_s` - CPU per GB/s encryption
22. `test_disk_io_queue_depth_distribution_under_load` - I/O queue depth stats
23. `test_network_bandwidth_utilized_vs_link_capacity` - Network utilization
24. `test_recovery_time_rto_after_single_node_failure` - RTO from crash to ready
25. `test_recovery_time_rpo_data_loss_on_node_failure` - RPO (journal governs)

**Status:** Planned (to be implemented)

---

### Block 5: Multi-Tenant & Multi-Site Operations (26 tests)
**File:** `crates/claudefs-reduce/tests/multitenancy_multisite.rs`

Tests multi-tenant isolation, quotas, and cross-site replication:

1. `test_tenant_isolation_write_from_tenant_a_not_visible_b` - Quota and visibility
2. `test_tenant_isolation_quota_enforcement_separate_budgets` - Separate quotas per tenant
3. `test_tenant_isolation_dedup_across_tenants_not_shared` - Dedup is per-tenant or shared?
4. `test_tenant_isolation_cache_not_shared_between_tenants` - Cache isolation
5. `test_tenant_isolation_gc_doesn't_affect_other_tenants` - GC isolation
6. `test_tenant_quota_increase_allows_more_writes` - Dynamic quota increase
7. `test_tenant_quota_decrease_triggers_enforcement` - Dynamic quota decrease
8. `test_tenant_quota_overage_backpressure_soft_limit` - Soft limit (warn) then hard (block)
9. `test_tenant_quota_hard_limit_rejects_new_writes` - Hard limit blocks all writes
10. `test_tenant_quota_soft_limit_recovery_after_gc` - Recovery after quota exceeded temporarily
11. `test_tenant_deletion_cascading_cleanup` - Delete tenant → all data cleaned
12. `test_tenant_account_multi_write_path_quota` - Count chunks, compressed, stored
13. `test_multisite_write_consistency_site_a_primary` - Primary site consistency
14. `test_multisite_write_consistency_site_b_async_replica` - Replica lag handling
15. `test_multisite_write_conflict_same_block_both_sites` - LWW conflict resolution
16. `test_multisite_dedup_coordination_across_sites` - Dedup shard routing across sites
17. `test_multisite_tiering_decision_consistency` - Same block tiered at both sites
18. `test_multisite_cache_coherency_read_after_write_consistency` - Cache coherency
19. `test_multisite_site_failure_recovery_from_replica` - Recover site A from site B
20. `test_multisite_network_partition_site_latency_spike` - Partition, latency spikes, heals
21. `test_multisite_split_brain_majority_quorum_prevails` - Quorum wins over minority
22. `test_multisite_gc_coordination_both_sites_same_decision` - GC synchronized
23. `test_multisite_quota_enforcement_replicated` - Quota limits replicated
24. `test_multisite_tenant_isolation_across_sites` - Isolation maintained across sites
25. `test_multisite_disaster_recovery_switchover_time_rto` - Switchover RTO < 5 min
26. `test_multisite_snapshot_consistency_across_sites` - Snapshot same at both sites

**Status:** Planned (to be implemented)

---

### Block 6: Long-Running Soak & Production Simulation (25 tests)
**File:** `crates/claudefs-reduce/tests/soak_production_simulation.rs`

Tests sustained operation over hours/days and production-like workloads:

1. `test_soak_24hr_sustained_1gb_s_write_throughput` - 24 hours, 1GB/s write
2. `test_soak_24hr_varying_workload_peak_valleys` - Variable load (peak 2GB/s, valley 100MB/s)
3. `test_soak_24hr_memory_leak_detection` - Memory usage stable over 24hr
4. `test_soak_24hr_cpu_efficiency_no_runaway_threads` - CPU stable (not trending up)
5. `test_soak_24hr_no_deadlocks_detected` - Watchdog detects hangs
6. `test_soak_24hr_cache_working_set_stable` - Cache misses stable after warmup
7. `test_soak_gc_cycles_proper_cleanup` - Multiple GC cycles, refcount correct
8. `test_soak_tiering_sustained_s3_uploads` - S3 tiering works over 24hr
9. `test_soak_dedup_fingerprint_cache_stable` - Fingerprint cache doesn't degrade
10. `test_soak_journal_log_rotation_no_buildup` - Journal rotates properly
11. `test_production_sim_oltp_workload_mixed_reads_writes` - 90% read, 10% write
12. `test_production_sim_oltp_metadata_heavy_lookups` - Many small lookups
13. `test_production_sim_olap_scan_large_sequential` - Large sequential scans
14. `test_production_sim_batch_nightly_large_archive` - Nightly batch job
15. `test_production_sim_backup_incremental_daily` - Daily incremental backup
16. `test_production_sim_media_ingest_burst_load` - Sudden burst (media ingest)
17. `test_production_sim_vm_clone_dedup_heavy` - VM cloning, high dedup ratio
18. `test_production_sim_database_snapshot_consistency` - Snapshot during DB writes
19. `test_production_sim_ransomware_encrypted_files` - Random encrypted payload (low compression)
20. `test_production_sim_compliance_retention_worm_enforcement` - WORM retention policy
21. `test_production_sim_key_rotation_no_data_loss` - Key rotation during writes
22. `test_production_sim_node_failure_recovery_background` - Node fails, recovery in background
23. `test_production_sim_snapshot_backup_incremental` - Snapshot and incremental backup flow
24. `test_production_sim_tenant_quota_violation_corrective_action` - Quota violation handling
25. `test_production_sim_disaster_recovery_failover_scenario` - DR scenario execution

**Status:** Planned (to be implemented)

---

## Implementation Strategy

### Test Organization
- Each block in its own file: `cluster_*.rs`, `chaos_*.rs`, `performance_*.rs`, `multitenancy_*.rs`, `soak_*.rs`
- Block files live in `crates/claudefs-reduce/tests/`
- Each test uses real module instances (no mocks)
- No wall-clock timing assertions (use logical event ordering)
- Resource cleanup: no cross-test contamination (each test is stateless)

### Multi-Node Simulation
- Phase 31 tests run on **single local machine** (no cluster required yet)
- Simulate multiple nodes via multiple `MetricsCollector` instances
- Simulate network delays via `std::thread::sleep(Duration)`
- Simulate S3 backend via in-memory mock store (fast, deterministic)
- Chaos injection via controlled errors in mock components

### Performance Testing
- Use `std::time::Instant` for performance measurements (no wall-clock)
- Measure throughput: operations / elapsed time
- Measure latency: record timestamps at each pipeline stage
- Performance assertions use ranges (not exact values): `assert!(throughput > 900MB/s && throughput < 1100MB/s)` (10% tolerance)
- Baseline metrics from Phase 30 used as reference

### Chaos Injection Framework
```rust
// Pseudo-code: chaos injection pattern
pub struct ChaoticS3Backend {
    storage: HashMap<String, Vec<u8>>,
    failure_mode: FailureMode,
}

pub enum FailureMode {
    None,
    SlowWrite(Duration),           // Add latency to PUT
    CorruptedRead,                 // Flip bits on GET
    PartialUpload(usize),          // Upload truncated at N bytes
    NetworkTimeout(Duration),       // Timeout after N ms
    KeyNotFound,                   // Return 404
}
```

### Cross-Site Replication Simulation
- Simulate Site A and Site B as separate `ReductionEngine` instances
- Simulate async journal replication via delayed `apply_remote_journal()`
- Simulate network partition via controlled failure injection
- Verify: state divergence bounded, majority quorum prevents split-brain

### Multi-Tenant Simulation
- Create multiple `TenantContext` instances with separate quota budgets
- Submit writes from each tenant concurrently
- Verify: quota enforcement, dedup isolation (if design requires)

### Metrics Collection
- Use existing `ReductionMetrics` from Phase 29+
- Collect per-test baseline: throughput, latency, memory, CPU
- Compare with Phase 30 baselines to detect regressions
- Export summary stats at end of test

---

## Test Infrastructure

### Async Runtime
- All soak tests use `#[tokio::test]` (Tokio runtime)
- Timeouts via `tokio::time::timeout()` (prevents hanging tests)
- Concurrent operations via `tokio::task::spawn()`

### Determinism
- No `rand::thread_rng()` (use seeded `rand::StdRng` for reproducibility)
- No wall-clock timing (use logical event counters)
- No system `sleep()` (use controlled delays for network simulation)
- Assertions based on outcome, not timing

### Resource Management
- Each test allocates scratch directory: `/tmp/claudefs_test_<uuid>/`
- Cleanup on test completion (even on failure)
- No persistent state between tests (all in-memory for Phase 31)

---

## Milestones & Timeline

### Session 1 (Next): Planning & Setup
- Write detailed specifications for all 5 blocks ✅ (this document)
- Create chaos injection framework (mock S3, controlled delays)
- Set up shared test utilities: `cluster_test_utils.rs`

### Session 2: Block Implementation (OpenCode)
- Blocks 1-2: Cluster dedup + tiering (49 tests) → OpenCode
- Blocks 3-4: Chaos + performance (55 tests) → OpenCode

### Session 3: Block Implementation & Integration
- Blocks 5-6: Multi-tenant + soak (51 tests) → OpenCode
- Integrate all blocks into test suite

### Session 4: Validation & Tuning
- Run full test suite (130 tests)
- Fix any failures, tune performance thresholds
- Baseline metrics collection

### Session 5: Phase 31 Completion
- Final validation, commit, CHANGELOG update
- Handoff to Phase 32 (if defined)

---

## Success Criteria

✅ **All 130 operational tests passing** (100%+ Phase 30 baseline)
✅ **No memory leaks** detected in 24hr soak tests
✅ **No deadlocks** detected in concurrent tests
✅ **Crash recovery** RTO < 30 seconds
✅ **Multi-node consistency** maintained under all failure modes
✅ **Quota enforcement** prevents resource exhaustion
✅ **Chaos injection** surfaces no new panics

---

## Known Constraints

1. **No actual cluster** — Phase 31 tests on single local machine (cluster testing deferred to Phase 32)
2. **No real S3** — Mock S3 backend in memory (AWS testing deferred to Phase 32)
3. **No real network** — Simulated delays via `std::thread::sleep()` (real network testing deferred to Phase 32)
4. **Single-threaded chaos** — Failure injection via controlled execution paths (true race condition detection requires Loom library)

---

## Deliverables Summary

| Item | Count | Files |
|------|-------|-------|
| Tests | 130 total | 6 test files (+61 Phase 30) |
| Blocks | 6 operational | cluster_dedup_consistency, cluster_tiering_consistency, chaos_failure_modes, performance_scalability, multitenancy_multisite, soak_production_simulation |
| Estimated LOC | 3000-4000 | Test implementation + chaos framework |
| Expected Duration | 2-3 weeks | Sessions 2-5 |

---

## Files to Create

1. `crates/claudefs-reduce/tests/cluster_dedup_consistency.rs` (25 tests)
2. `crates/claudefs-reduce/tests/cluster_tiering_consistency.rs` (24 tests)
3. `crates/claudefs-reduce/tests/chaos_failure_modes.rs` (30 tests)
4. `crates/claudefs-reduce/tests/performance_scalability.rs` (25 tests)
5. `crates/claudefs-reduce/tests/multitenancy_multisite.rs` (26 tests)
6. `crates/claudefs-reduce/tests/soak_production_simulation.rs` (25 tests)
7. `crates/claudefs-reduce/tests/chaos_utils.rs` (shared chaos injection framework)

---

## Next Steps

1. ✅ **This session:** Create Phase 31 plan (completed)
2. **Session 2:** Implement chaos framework & 2 test blocks (49 tests)
3. **Session 3:** Implement remaining 3 blocks (81 tests)
4. **Session 4:** Validation & tuning
5. **Session 5:** Phase 31 completion, commit, and handoff

---

## Handoff to Phase 32

Phase 31 validates local operational hardening. Phase 32 (future) would extend to:
- **Real cluster testing** — Multi-node on AWS test cluster
- **Real S3 backend** — AWS S3 instead of mock
- **Real network failures** — Partition simulation via `toxiproxy`
- **Production monitoring** — Prometheus scraping real cluster metrics
- **Disaster recovery drills** — Site failover scenarios
- **Performance tuning** — Optimize for production SLOs

---

**Author:** A3 (Data Reduction Agent)
**Created:** 2026-04-17
**Phase:** 31 Planning
