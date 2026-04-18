# A3: Phase 32 Blocks 3-8 — Multi-Node Cluster Testing Plan
## Specification & Sequencing

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Status:** Planning (Blocks 1-2 in OpenCode generation)
**Estimated Timeline:** Sequential generation via OpenCode after Blocks 1-2 complete

---

## Phase 32 Overview

**Phase 31:** 2,284 tests validating single-machine dedup behavior (deterministic, mocked)
**Phase 32:** 88-120 real cluster tests lifting Phase 31 to AWS multi-node infrastructure

**Critical Path:**
1. **Block 1** (12-15 tests): Cluster setup & health ✅ In OpenCode
2. **Block 2** (14-18 tests): Single-node dedup validation ✅ In OpenCode
3. **Block 3** (16-20 tests): Multi-node coordination → Open after Block 2
4. **Block 4** (12-16 tests): Tiering with real S3 → Open after Block 2
5. **Block 5** (14-18 tests): Multi-client workloads → Open after Blocks 2-3
6. **Block 6** (16-20 tests): Chaos & resilience → Open after Block 3
7. **Block 7** (10-14 tests): Performance benchmarking → Open after Blocks 2-4
8. **Block 8** (10-14 tests): Disaster recovery → Open after Block 6

---

## Block 3: Multi-Node Dedup Coordination (16-20 tests)

**File:** `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs`
**LOC:** 800-950
**Duration:** 3 days

### Key Tests
1. `test_cluster_two_nodes_same_fingerprint_coordination` — Nodes A & B write same block, coordinate
2. `test_cluster_dedup_shards_distributed_uniformly` — Fingerprints across 8 shards
3. `test_cluster_dedup_shard_leader_routing` — Write routes to shard leader
4. `test_cluster_dedup_shard_replica_consistency` — Followers replicate leader state
5. `test_cluster_dedup_three_node_write_conflict` — 3 nodes, LWW conflict resolution
6. `test_cluster_dedup_refcount_coordination_race` — Concurrent refcount updates race-free
7. `test_cluster_dedup_cache_coherency_multi_node` — Cross-node cache invalidation
8. `test_cluster_dedup_gc_coordination_multi_node` — GC coordination (no duplicate GC)
9. `test_cluster_dedup_tiering_multi_node_consistency` — Atomic multi-node tiering
10. `test_cluster_dedup_node_failure_shard_failover` — Node fail → shard leader election
11. `test_cluster_dedup_network_partition_shard_split` — Partition → quorum detection
12. `test_cluster_dedup_cascade_node_failures` — 2/5 nodes fail, graceful degradation
13. `test_cluster_dedup_throughput_5_nodes_linear` — 5 nodes → 5x throughput (±10%)
14. `test_cluster_dedup_latency_multinode_p99` — P99 <150ms with coordination
15. `test_cluster_dedup_cross_node_snapshot_consistency` — Snapshot during multi-node writes
16. `test_cluster_dedup_journal_replay_after_cascade_failure` — Kill 2 nodes, recover both
17. `test_cluster_dedup_worm_enforcement_multi_node` — WORM blocks not deduplicated
18. `test_cluster_dedup_tenant_isolation_multi_node` — Tenant quotas per node
19. `test_cluster_dedup_metrics_aggregation` — Metrics aggregated from all nodes
20. `test_cluster_multinode_dedup_ready_for_next_blocks` — All tests passed

### Readiness
- Depends on: Block 1 ✅ (cluster health), Block 2 ✅ (single-node dedup)
- Can begin: After Block 2 complete

---

## Block 4: Tiering with Real S3 Consistency (12-16 tests)

**File:** `crates/claudefs-reduce/tests/cluster_tiering_s3_consistency.rs`
**LOC:** 650-800
**Duration:** 2-3 days

### Key Tests
1. `test_cluster_tiering_hot_to_cold_transition` — Hot data aged, moved to S3
2. `test_cluster_tiering_s3_fetch_on_cold_read` — Cold data fetched from S3 on demand
3. `test_cluster_tiering_policy_based_movement` — Tiering policy enforced (temperature, age)
4. `test_cluster_tiering_s3_failure_resilience` — S3 down → writes backpressured, don't fail
5. `test_cluster_tiering_bandwidth_limit_enforcement` — Tiering respects bandwidth cap
6. `test_cluster_tiering_concurrent_hot_cold_access` — Concurrent hot/cold access
7. `test_cluster_tiering_cache_populated_from_s3` — Cold data cached on first fetch
8. `test_cluster_tiering_metadata_consistency_s3` — S3 metadata (size, checksum) matches
9. `test_cluster_tiering_partial_s3_restore` — Kill S3 during restore → graceful recovery
10. `test_cluster_tiering_s3_cleanup_old_chunks` — Old S3 chunks GC'd correctly
11. `test_cluster_tiering_burst_capacity_handling` — Temporary burst handled (no data loss)
12. `test_cluster_tiering_performance_s3_tier` — S3 tier latency acceptable (<1s cold miss)
13. `test_cluster_tiering_cross_region_s3` — S3 in different region, latency acceptable
14. `test_cluster_tiering_s3_encryption_at_rest` — S3 objects encrypted
15. `test_cluster_tiering_refcount_with_s3_chunks` — Refcount accurate with tiered data
16. `test_cluster_tiering_quota_accounting_with_s3` — Quotas include S3-tiered data

### Readiness
- Depends on: Block 1 ✅, Block 2 ✅
- Can begin: After Block 2 complete (parallel with Block 3 if resources available)

---

## Block 5: Multi-Client Workloads (14-18 tests)

**File:** `crates/claudefs-reduce/tests/cluster_multi_client_workloads.rs`
**LOC:** 750-900
**Duration:** 3 days

### Key Tests
1. `test_cluster_two_clients_concurrent_writes` — Both clients write concurrently
2. `test_cluster_two_clients_same_file_coordination` — Both write same file, LWW wins
3. `test_cluster_two_clients_dedup_shared_data` — Writes share fingerprints
4. `test_cluster_two_clients_quota_per_client` — Each client has separate quota
5. `test_cluster_two_clients_cache_coherency_across_clients` — Cache invalidation on other client writes
6. `test_cluster_two_clients_refcount_coordination_concurrent` — Concurrent refcount updates
7. `test_cluster_two_clients_one_fails` — Client 1 crashes, Client 2 continues
8. `test_cluster_two_clients_snapshot_consistency` — Snapshot with both clients writing
9. `test_cluster_two_clients_read_after_write_different_client` — Write on C1, read on C2
10. `test_cluster_two_clients_metadata_consistency_reads` — Both clients see same metadata
11. `test_cluster_two_clients_performance_parallel_writes` — Throughput with 2 clients
12. `test_cluster_two_clients_network_partition_between_clients` — Client partition, recovery
13. `test_cluster_two_clients_delete_coordination` — Delete on C1, C2 sees update
14. `test_cluster_two_clients_replication_consistency_cross_site` — Multi-site + multi-client
15. `test_cluster_two_clients_latency_p99_concurrent` — Latency with concurrent load
16. `test_cluster_two_clients_mixed_workload_production_like` — Real production-like workload
17. `test_cluster_two_clients_10x_throughput` — 2 clients → ~2x throughput
18. `test_cluster_multi_client_ready_for_chaos` — All tests passed

### Readiness
- Depends on: Block 1 ✅, Block 2 ✅, Block 3 (for coordination patterns)
- Can begin: After Block 2-3 complete

---

## Block 6: Chaos Engineering & Resilience (16-20 tests)

**File:** `crates/claudefs-reduce/tests/cluster_chaos_resilience.rs`
**LOC:** 900-1100
**Duration:** 3-4 days

### Key Tests
1. `test_cluster_chaos_random_node_failures` — Kill random node, recovery
2. `test_cluster_chaos_cascade_failures_2_of_5` — Kill 2 storage nodes, system survives
3. `test_cluster_chaos_storage_node_restart` — Kill node, data available after restart
4. `test_cluster_chaos_fuse_client_disconnect` — Client network down, recovery
5. `test_cluster_chaos_metadata_shard_partition` — Split metadata shard, quorum detects
6. `test_cluster_chaos_network_latency_injection` — Add 100ms latency, observe impact
7. `test_cluster_chaos_packet_loss_5_percent` — 5% packet loss, system adapts
8. `test_cluster_chaos_disk_full_on_storage_node` — Storage disk full, graceful backpressure
9. `test_cluster_chaos_memory_pressure_on_node` — Memory constrained, no OOM
10. `test_cluster_chaos_concurrent_client_and_node_failures` — Client fail + node fail simultaneously
11. `test_cluster_chaos_s3_availability_zones_down` — S3 region down, fallback/retry
12. `test_cluster_chaos_power_cycle_node` — Abrupt power loss simulation, recovery
13. `test_cluster_chaos_disk_corruption_detection` — Corrupt S3 chunk, detected
14. `test_cluster_chaos_shard_replica_corruption` — Corrupt replica, consistency check catches it
15. `test_cluster_chaos_replication_lag_spike` — Cross-site replication lag spikes, recovery
16. `test_cluster_chaos_metadata_split_brain` — Quorum split, recovery to single leader
17. `test_cluster_chaos_sustained_failures_24hr` — Multiple failures over 24 hours
18. `test_cluster_chaos_concurrent_tiering_failures` — Failures during active tiering
19. `test_cluster_chaos_recovery_ordering` — Nodes recover in different orders
20. `test_cluster_chaos_all_resilience_ready` — All resilience patterns validated

### Readiness
- Depends on: Block 1-5 ✅ (understands normal behavior)
- Can begin: After Block 5 complete

---

## Block 7: Performance Benchmarking (10-14 tests)

**File:** `crates/claudefs-reduce/tests/cluster_performance_benchmarks.rs`
**LOC:** 600-750
**Duration:** 2-3 days

### Key Tests
1. `test_cluster_benchmark_single_node_throughput` — Single node max throughput
2. `test_cluster_benchmark_multi_node_throughput_5x` — 5 nodes → 5x throughput
3. `test_cluster_benchmark_latency_p50_p99_p999` — Latency distribution
4. `test_cluster_benchmark_cache_hit_ratio` — Cache effectiveness
5. `test_cluster_benchmark_memory_utilization` — Memory footprint under load
6. `test_cluster_benchmark_cpu_utilization_per_node` — CPU% per node
7. `test_cluster_benchmark_network_bandwidth_utilization` — Network saturation
8. `test_cluster_benchmark_s3_tiering_throughput` — S3 tiering rate
9. `test_cluster_benchmark_dedup_compression_ratio` — Actual compression achieved
10. `test_cluster_benchmark_coordination_overhead` — Coordination latency impact
11. `test_cluster_benchmark_concurrent_clients_scaling` — Throughput vs client count
12. `test_cluster_benchmark_large_file_performance` — 10GB+ file write/read
13. `test_cluster_benchmark_small_file_performance` — 1KB file operations
14. `test_cluster_benchmark_mixed_workload_iops_mbs` — Mixed workload metrics

### Readiness
- Depends on: Block 2-5 ✅ (normal operation metrics)
- Can begin: After Block 5 complete (parallel with Block 6 if resources available)

---

## Block 8: Disaster Recovery (10-14 tests)

**File:** `crates/claudefs-reduce/tests/cluster_disaster_recovery.rs`
**LOC:** 650-800
**Duration:** 2-3 days

### Key Tests
1. `test_cluster_dr_metadata_backup_and_restore` — Metadata backup, restore from backup
2. `test_cluster_dr_s3_backup_integrity` — S3 backup complete, checksums match
3. `test_cluster_dr_point_in_time_recovery` — Recover to specific point in time
4. `test_cluster_dr_site_a_complete_failure` — Site A down, Site B takes over
5. `test_cluster_dr_cross_site_replication_lag_recovery` — Replication lag > RTO, data loss assessment
6. `test_cluster_dr_metadata_shard_loss_recovery` — Metadata shard lost, recovered from replica
7. `test_cluster_dr_s3_bucket_loss_recovery` — S3 bucket unavailable, fallback tier
8. `test_cluster_dr_client_snapshot_recovery` — Client data restored from snapshot
9. `test_cluster_dr_cascading_failures_recovery` — Multiple failures, full recovery order
10. `test_cluster_dr_rpo_rto_metrics_measured` — RPO <10min, RTO <30min verified
11. `test_cluster_dr_recovery_performance_degradation` — Recovery performance acceptable
12. `test_cluster_dr_data_integrity_after_recovery` — No data loss, no corruption
13. `test_cluster_dr_automated_failover_trigger` — Automatic failover on threshold
14. `test_cluster_dr_runbooks_documented` — Runbooks exist and are accurate

### Readiness
- Depends on: Block 1-6 ✅ (normal + failure scenarios)
- Can begin: After Block 6 complete

---

## Implementation Strategy

### Sequential Generation via OpenCode
1. **Now:** Block 1 + Block 2 running in parallel (background processes)
2. **Hour 1-2:** Block 1 & 2 complete, extract outputs, commit
3. **Hour 2-3:** Start Block 3 + Block 4 (parallel)
4. **Hour 3-4:** Block 3 & 4 complete, extract, commit
5. **Hour 4-5:** Start Block 5 (depends on Block 3)
6. **Hour 5-6:** Start Block 6 (depends on Block 5)
7. **Hour 6-7:** Start Block 7 (depends on Block 5)
8. **Hour 7-8:** Start Block 8 (depends on Block 6)
9. **Hour 8+:** All blocks complete, commit final batch

### Parallelization Opportunities
- Blocks 3 & 4 can run in parallel (independent)
- Blocks 5, 7 can run in parallel (both depend on Block 2 done)
- Block 6 can run in parallel with Block 7 once Block 5 done

### Total Timeline
- Sequential generation: ~8-10 hours
- Cluster testing: 5-20 minutes per test block (depending on infrastructure)
- Total: 1-2 weeks (including infrastructure stabilization)

---

## Success Criteria for Phase 32

- ✅ Block 1: Cluster setup validated (12-15 tests)
- ✅ Block 2: Single-node dedup on cluster (14-18 tests)
- ✅ Block 3: Multi-node coordination (16-20 tests)
- ✅ Block 4: Tiering with S3 (12-16 tests)
- ✅ Block 5: Multi-client workloads (14-18 tests)
- ✅ Block 6: Chaos & resilience (16-20 tests)
- ✅ Block 7: Performance benchmarks (10-14 tests)
- ✅ Block 8: Disaster recovery (10-14 tests)
- ✅ **Total:** 88-120 tests passing on real cluster
- ✅ **Performance:** Within 10% of Phase 31 simulation
- ✅ **Data integrity:** 100% (zero corruption, zero data loss)
- ✅ **Production readiness:** 95%+

---

## Next Actions

1. Monitor OpenCode processes for Block 1 & 2 completion
2. Extract Rust code from outputs, place in test files
3. Run `cargo test --lib cluster_multinode_setup --lib cluster_single_node_dedup`
4. Verify all tests pass
5. Commit results: `[A3] Phase 32 Blocks 1-2 Complete`
6. Begin Block 3 & 4 OpenCode generation (run simultaneously)
7. Continue through Block 8

