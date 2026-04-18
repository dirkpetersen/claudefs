# A3: Data Reduction — Phase 32 Plan
## Real Cluster Testing & Production Validation

**Status:** 🟡 **PHASE 32 PLANNING** | **Date:** 2026-04-18
**Agent:** A3 (Data Reduction)
**Baseline Tests:** 2,284 (Phase 31)
**Phase 32 Target:** +80-120 cluster integration tests (target: 2,380-2,400 total)
**Session:** 12 (estimated)

---

## Overview

Phase 31 validated all data reduction behavior on a **single-machine simulation** with deterministic fault injection. Phase 32 lifts these tests to **real AWS multi-node cluster** environment where:

- **Real FUSE clients** mount the file system on actual client nodes
- **Real storage nodes** run with genuine io_uring I/O, network communication
- **Real S3 backend** (AWS S3 API, not mock) for tiering
- **Real network partitions** (security group rules) and latency (cross-region)
- **Real node failures** (instance termination, reboot)
- **Real monitoring** (Prometheus, CloudWatch, actual metrics)

**Key difference from Phase 31:**
- Phase 31: Single-machine, mocked infrastructure, deterministic timing
- Phase 32: Multi-node cluster, real AWS infrastructure, real latencies
- Phase 32 validates end-to-end behavior from FUSE client through reduction pipeline to S3

---

## Dependencies

### A11 Infrastructure (CRITICAL)
- **Required:** Phase 5 Block 1 complete (Terraform provisioning)
- **Status:** 🟡 Block 1 planning/testing spec complete, OpenCode ready
- **What A3 needs:**
  - 1 orchestrator node (for test coordination)
  - 5 storage nodes (i4i.2xlarge) across 2 availability zones
  - 2 client nodes (FUSE mounts) for multi-client testing
  - 1 conduit node (cross-site replication)
  - 1 Jepsen node (optional, for future chaos)
  - Prometheus monitoring infrastructure
  - S3 bucket (for tiering tests)
  - Security groups allowing controlled network faults

### A2 Metadata Service
- **Required:** Phase 11 Block A complete (quota_replication)
- **Status:** 🟡 Block A OpenCode running
- **What A3 needs:**
  - Stable quota APIs (tracking, enforcement)
  - Cross-site replication journal (for A6 integration)
  - Multi-tenant isolation (for tenant testing)

### A1 Storage Engine
- **Required:** Phase 11 complete (online scaling, flash defrag, intelligent tiering)
- **Status:** 🟡 Block 1 OpenCode running
- **What A3 needs:**
  - Stable block allocator
  - NVMe I/O performance baseline
  - Space management APIs

### A4 Transport
- **Required:** Stable RPC protocol
- **Status:** ✅ Phase 13 complete (1,363+ tests)
- **What A3 needs:**
  - Bandwidth shaping (for backpressure testing)
  - Network latency injection (for cross-region testing)

---

## Phase 32 Test Blocks

### Block 1: Multi-Node Cluster Setup & Health (12-15 tests)
**Duration:** 2-3 days
**File:** `crates/claudefs-reduce/tests/cluster_multinode_setup.rs`

Validate that the cluster is properly provisioned and all infrastructure is operational before running workloads.

**Key Tests:**
1. `test_cluster_all_nodes_online` - All 5 storage + 2 clients operational
2. `test_storage_nodes_ntp_synchronized` - NTP within 100ms across nodes
3. `test_s3_bucket_accessible_from_all_nodes` - S3 connectivity verified
4. `test_prometheus_metrics_collection` - Metrics flowing to Prometheus
5. `test_fuse_mounts_online_both_clients` - Both client nodes mounted
6. `test_network_connectivity_matrix` - Ping all node pairs, latency <5ms intra-AZ, <20ms cross-AZ
7. `test_security_groups_rules_correct` - Network rules match expected (no surprise access)
8. `test_disk_io_baseline_performance` - Storage nodes can achieve 500K+ IOPS on NVMe
9. `test_memory_available_on_all_nodes` - No memory pressure detected
10. `test_cross_az_latency_acceptable` - Cross-AZ latency 15-30ms (typical AWS)
11. `test_s3_throughput_baseline` - S3 PUT 50-100MB/s, GET 100-200MB/s
12. `test_cluster_clock_skew_within_limits` - Clock skew <10ms across nodes
13. `test_metadata_service_responding` - Metadata RPC endpoints accessible
14. `test_replication_conduit_healthy` - Cross-site conduit node operational
15. `test_cluster_initial_state_ready_for_workload` - All health checks passed, ready to proceed

---

### Block 2: Single-Node Dedup on Cluster (14-18 tests)
**Duration:** 2 days
**File:** `crates/claudefs-reduce/tests/cluster_single_node_dedup.rs`

Run the Phase 31 dedup tests, but with **real clients writing to real storage node**.

**Key Tests:**
1. `test_cluster_dedup_basic_write_from_fuse_client` - Write 100MB via FUSE, verify fingerprints stored
2. `test_cluster_dedup_cache_hit_on_second_write` - Write same 100MB again, verify cache hit (reduced I/O)
3. `test_cluster_dedup_fingerprint_persisted_to_s3` - Write, tiering, verify fingerprint in S3
4. `test_cluster_dedup_refcount_accurate_after_deletes` - Write 10 refs, delete 8, verify refcount=2
5. `test_cluster_dedup_coordination_real_rpc` - Two clients write same block, coordination works
6. `test_cluster_dedup_throughput_baseline` - Single node 50K-100K dedup fingerprint ops/sec
7. `test_cluster_dedup_latency_p99_write_path` - Write latency P99 <100ms
8. `test_cluster_dedup_cache_eviction_under_memory_pressure` - Cache LRU works with real memory
9. `test_cluster_dedup_cross_tenant_isolation_real` - Tenant A and B fingerprints isolated
10. `test_cluster_dedup_crash_recovery_real` - Kill storage node, restart, verify journal recovery
11. `test_cluster_dedup_coordinator_failover_real` - Kill coordinator shard leader, new leader elected
12. `test_cluster_dedup_network_partition_recovery_real` - Partition for 5s, verify recovery
13. `test_cluster_dedup_metrics_accurate` - Prometheus metrics match internal counters
14. `test_cluster_dedup_no_data_corruption` - Verify checksums on all reads
15. `test_cluster_dedup_quota_enforcement_active` - Quota limits prevent writes above limit
16. `test_cluster_dedup_multi_region_replication` - Replicate fingerprints to Site B
17. `test_cluster_tiering_real_s3_backend` - Tiering to real S3, not mock
18. `test_cluster_dedup_performance_vs_phase31` - Cluster performance within 10% of single-machine simulation

---

### Block 3: Multi-Node Dedup Coordination (16-20 tests)
**Duration:** 3 days
**File:** `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs`

Validate dedup coordination across **multiple storage nodes** with **real fingerprint routing**.

**Key Tests:**
1. `test_cluster_two_nodes_same_fingerprint_coordination` - Nodes A & B both write same block, coordinate
2. `test_cluster_dedup_shards_distributed_uniformly` - Fingerprints distributed across 8 shards
3. `test_cluster_dedup_shard_leader_routing` - Write to shard leader, verify routing correct
4. `test_cluster_dedup_shard_replica_consistency` - Shard followers replicate leader state
5. `test_cluster_dedup_three_node_write_conflict` - 3 nodes write same block, LWW conflict resolution
6. `test_cluster_dedup_refcount_coordination_race` - Concurrent refcount updates race-free
7. `test_cluster_dedup_cache_coherency_multi_node` - Update fingerprint on Node A, invalidate on B cache
8. `test_cluster_dedup_gc_coordination_multi_node` - GC on Node A, Node B doesn't GC same block
9. `test_cluster_dedup_tiering_multi_node_consistency` - Tiering blocks to S3 from multiple nodes atomically
10. `test_cluster_dedup_node_failure_shard_failover` - Node fails, shard leader election on replicas
11. `test_cluster_dedup_network_partition_shard_split` - Partition splits cluster, quorum detection works
12. `test_cluster_dedup_cascade_node_failures` - 2 of 5 nodes fail, system degrades gracefully
13. `test_cluster_dedup_throughput_5_nodes_linear` - 5 nodes → 5x single-node throughput (within 10%)
14. `test_cluster_dedup_latency_multinode_p99` - Multi-node coordination latency P99 <150ms
15. `test_cluster_dedup_cross_node_snapshot_consistency` - Snapshot during multi-node writes
16. `test_cluster_dedup_journal_replay_after_cascade_failure` - Kill 2 nodes, recover both, verify consistency
17. `test_cluster_dedup_worm_enforcement_multi_node` - WORM blocks not deduplicated across nodes
18. `test_cluster_dedup_tenant_isolation_multi_node` - Tenant quotas isolated per node
19. `test_cluster_dedup_metrics_aggregation` - Prometheus metrics aggregated from all nodes
20. `test_cluster_multinode_vs_phase31_parity` - Behavior matches Phase 31 simulation (within 5% tolerance)

---

### Block 4: Tiering with Real S3 (12-16 tests)
**Duration:** 2-3 days
**File:** `crates/claudefs-reduce/tests/cluster_tiering_real_s3.rs`

Validate tiering pipeline with **real AWS S3**, not mocked backend.

**Key Tests:**
1. `test_cluster_tiering_hot_to_cold_real_s3` - Hot (flash) → Cold (S3) with real S3 API
2. `test_cluster_tiering_s3_put_timeout_recovery` - S3 PUT timeout, retry with backoff
3. `test_cluster_tiering_s3_corruption_checksum_detect` - Corrupted S3 object detected on read
4. `test_cluster_tiering_s3_multipart_large_blocks` - Tiering 500MB+ blocks with multipart upload
5. `test_cluster_tiering_s3_glacier_lifecycle` - Transition to Glacier after 30 days, verify cost
6. `test_cluster_tiering_concurrent_tiering_multiple_nodes` - 5 nodes tiering simultaneously to same S3 bucket
7. `test_cluster_tiering_s3_delete_on_local_deletion` - Delete locally → delete in S3
8. `test_cluster_tiering_cache_invalidation_on_s3_update` - External S3 update invalidates local cache
9. `test_cluster_tiering_refetch_on_missing_s3_block` - Block missing from S3, refetch from replicas
10. `test_cluster_tiering_s3_region_failover` - Primary S3 region down, use secondary
11. `test_cluster_tiering_backpressure_slow_s3` - S3 slow (500ms), backpressure activates <100ms
12. `test_cluster_tiering_throughput_real_s3_latency` - Accounting for real 50-100ms S3 latency
13. `test_cluster_tiering_metadata_tagging_s3` - S3 objects tagged with tenant/compression/encryption metadata
14. `test_cluster_tiering_erasure_coding_parity_tiering` - EC parity blocks tiered together atomically
15. `test_cluster_tiering_cost_calculation_real_s3` - Verify cost metrics match S3 bill
16. `test_cluster_tiering_vs_phase31_behavior` - Behavior matches Phase 31 tiering tests

---

### Block 5: Multi-Client Workloads (14-18 tests)
**Duration:** 2-3 days
**File:** `crates/claudefs-reduce/tests/cluster_multi_client_workloads.rs`

Validate reduction pipeline under **real multi-client workloads** from 2 FUSE clients.

**Key Tests:**
1. `test_cluster_multi_client_concurrent_writes_same_file` - Both clients write to same file, consistency
2. `test_cluster_multi_client_dedup_same_block` - Both clients write same block (dedup), verify coordination
3. `test_cluster_multi_client_file_locking` - One client locks file, other waits
4. `test_cluster_multi_client_quota_enforcement_fair` - Quota divided fairly between clients
5. `test_cluster_multi_client_throughput_aggregated` - Two clients → 1.8-2.0x single-client throughput
6. `test_cluster_multi_client_compression_ratio_maintained` - Compression ratio same with 2 clients
7. `test_cluster_multi_client_cache_coherency` - Client A writes, Client B reads, sees fresh data
8. `test_cluster_multi_client_snapshot_consistency` - Snapshot during multi-client writes
9. `test_cluster_multi_client_one_client_fails` - Client 1 dies, Client 2 continues, no data loss
10. `test_cluster_multi_client_network_partition_between_clients` - Clients in separate subnets, still consistent
11. `test_cluster_multi_client_read_latency_p50_p99` - Real FUSE read latency, cache hits vs misses
12. `test_cluster_multi_client_write_latency_p50_p99` - Real FUSE write latency, across reduction pipeline
13. `test_cluster_multi_client_metadata_heavy_workload` - 1000s of small ops, dedup coordination stable
14. `test_cluster_multi_client_io_amplification_measured` - Real I/O amplification (write/read) with network
15. `test_cluster_multi_client_cross_az_write_performance` - Writing across AZs (high latency), behavior degradation <20%
16. `test_cluster_multi_client_compression_cpu_impact` - Monitor CPU usage on storage nodes
17. `test_cluster_multi_client_memory_stability_24hr` - Run 24hr multi-client workload, no memory leak
18. `test_cluster_multi_client_vs_phase31_parity` - Real behavior matches Phase 31 simulation

---

### Block 6: Chaos & Resilience on Cluster (16-20 tests)
**Duration:** 3-4 days
**File:** `crates/claudefs-reduce/tests/cluster_chaos_resilience.rs`

Inject real-world failures into the cluster and verify recovery.

**Key Tests:**
1. `test_cluster_chaos_storage_node_crash_mid_write` - Crash during write, verify journal recovery
2. `test_cluster_chaos_storage_node_slow_io` - Add 500ms latency to one node's disk I/O
3. `test_cluster_chaos_network_partition_two_nodes` - Partition node 1 from nodes 2-5 for 30s
4. `test_cluster_chaos_three_node_cascade_failure` - Kill nodes 1, 2, 3 sequentially, system degrades
5. `test_cluster_chaos_cascade_recovery_nodes_rejoin` - Nodes come back online one-by-one, rejoin
6. `test_cluster_chaos_client_network_drop_packets` - Random packet loss (5%) between client and storage
7. `test_cluster_chaos_client_latency_spike` - Add 1s latency spike client-to-storage for 10s
8. `test_cluster_chaos_s3_backend_temporarily_unavailable` - S3 unreachable for 2min, tiering queued
9. `test_cluster_chaos_security_group_rules_misconfigured` - Network rule typo, recovery after fix
10. `test_cluster_chaos_disk_fill_up_space_exhaustion` - Storage node disk 95% full, backpressure activates
11. `test_cluster_chaos_memory_pressure_oom_killer` - Memory pressure, verify dedup cache evicts
12. `test_cluster_chaos_cpu_maxed_out_all_cores` - CPU contention, verify graceful degradation
13. `test_cluster_chaos_concurrent_failures_three_events` - Simultaneous: node crash + network partition + S3 timeout
14. `test_cluster_chaos_leadership_election_under_load` - Dedup shard leader fails during high write load
15. `test_cluster_chaos_replication_lag_under_partition` - Cross-site replication lag measured during partition
16. `test_cluster_chaos_write_path_survives_10_failures_in_sequence` - Write survives 10 sequential failures
17. `test_cluster_chaos_read_path_survives_corruption` - Read verifies block checksum, detects corruption
18. `test_cluster_chaos_recovery_time_rto_all_failures` - All failures recover in <30s
19. `test_cluster_chaos_recovery_point_rpo_no_data_loss` - No data loss across all failure scenarios
20. `test_cluster_chaos_vs_phase31_coverage` - Same failure modes as Phase 31, validated on real cluster

---

### Block 7: Performance Benchmarking (10-14 tests)
**Duration:** 2-3 days
**File:** `crates/claudefs-reduce/tests/cluster_benchmarks_production.rs`

Establish **production performance baselines** on real cluster.

**Key Tests:**
1. `test_cluster_bench_throughput_single_client_100gb` - Single client sequential write 100GB, measure throughput
2. `test_cluster_bench_throughput_two_clients_concurrent` - Two clients concurrent writes, aggregate throughput
3. `test_cluster_bench_throughput_dedup_90percent_similarity` - Dedup-heavy workload (90% similar), measure dedup overhead
4. `test_cluster_bench_throughput_compression_zstd_ratio` - Compression ratio with real data, measure CPU
5. `test_cluster_bench_latency_write_path_stages` - Breakdown: FDeploy → Dedup → Compression → Encryption → EC → S3
6. `test_cluster_bench_latency_read_path_stages` - Breakdown: FUSE read → Cache/Fetch → Decompress → Decrypt → Copy
7. `test_cluster_bench_cache_hit_rate_vs_working_set` - Cache hit rate for 10GB, 50GB, 100GB working set
8. `test_cluster_bench_io_amplification_write_and_read` - Measure write amplification (EC parity), read amplification (rebuild)
9. `test_cluster_bench_network_bandwidth_utilized` - Network saturation for dedup coordination + tiering
10. `test_cluster_bench_cpu_utilization_per_operation_type` - CPU per dedup, compress, encrypt, tiering
11. `test_cluster_bench_memory_per_node_under_1tb_data` - Memory footprint as data size scales
12. `test_cluster_bench_ssd_wear_distribution` - Wear leveling uniformity across SSDs
13. `test_cluster_bench_prometheus_collection_overhead` - Metrics collection CPU impact (<1%)
14. `test_cluster_bench_comparison_to_phase31_simulation` - Real cluster vs Phase 31 simulation performance (±10%)

---

### Block 8: Disaster Recovery & Production Readiness (10-14 tests)
**Duration:** 2-3 days
**File:** `crates/claudefs-reduce/tests/cluster_disaster_recovery.rs`

Validate **disaster recovery** scenarios and production operational readiness.

**Key Tests:**
1. `test_cluster_dr_site_b_cold_standby_receive_replicated_blocks` - Block written at Site A, received at Site B
2. `test_cluster_dr_site_a_failure_site_b_takes_over` - Kill Site A, clients failover to Site B
3. `test_cluster_dr_split_brain_detection_quorum` - Partition splits cluster, quorum detection prevents split-brain
4. `test_cluster_dr_dedup_state_survives_site_failure` - Fingerprints reconstructed after Site A failure
5. `test_cluster_dr_s3_backend_continues_after_site_failure` - Tiering continues from Site B after Site A down
6. `test_cluster_dr_backup_consistency_after_failure` - Backups taken post-failure are consistent
7. `test_cluster_dr_rto_site_a_restart_rejoin` - Site A restarts, rejoins, catches up replication (<1 hour)
8. `test_cluster_dr_rpo_no_data_loss_between_sites` - Replication lag <5s, RPO acceptable
9. `test_cluster_dr_node_replacement_new_hardware` - Replace failed node with new, verify recovery
10. `test_cluster_dr_storage_pool_replacement` - Replace full storage pool, verify data integrity
11. `test_cluster_dr_monitoring_alerts_critical_failures` - Prometheus alerts fire on critical failures
12. `test_cluster_dr_operational_procedures_documented` - Runbooks exist for all failure scenarios
13. `test_cluster_dr_compliance_audit_trail_maintained` - Audit logs persisted through failures
14. `test_cluster_dr_production_readiness_checklist_passed` - All readiness criteria met

---

## Implementation Strategy

### Dependencies & Sequencing
1. **Wait for A11 Phase 5 Block 1** — Terraform provisioning complete, cluster online
2. **Blocks 1-2:** Sequential (setup, then single-node validation)
3. **Blocks 3-5:** Can run in parallel (multi-node coordination, tiering, multi-client)
4. **Block 6:** Sequential (chaos requires stable baseline from Blocks 3-5)
5. **Blocks 7-8:** Parallel (benchmarking and DR don't interfere)

### OpenCode Delegation
- All test blocks will be implemented as standalone Rust integration tests
- OpenCode will generate test suites (similar to Phase 31)
- Each block: 500-800 LOC, 12-20 tests
- Total Phase 32: ~5,000-6,000 LOC, 88-120 new tests

### Validation Approach
- Each test: real cluster setup, workload, verification, cleanup
- Metrics: collect from Prometheus + client-side measurement
- Performance: ±10% tolerance vs Phase 31 simulation (accounts for real network variance)
- Chaos: deterministic scenarios (not random), repeatable results

---

## Success Criteria

- [ ] **All 8 blocks implemented:** 88-120 tests passing (target: 2,380-2,400 total)
- [ ] **Cluster stability:** 24hr soak test with no crashes/corruptions
- [ ] **Performance parity:** Real cluster within ±10% of Phase 31 simulation
- [ ] **Failure recovery:** All chaos scenarios recover within <30s RTO
- [ ] **Multi-node consistency:** No data loss across all tested failures
- [ ] **Prometheus integration:** All metrics flowing, dashboards populated
- [ ] **Documentation:** DR runbooks, operational procedures documented
- [ ] **Production ready:** 95%+ confidence in multi-node deployment

---

## Timeline Estimate

- **Block 1 (setup):** 2-3 days
- **Block 2 (single-node):** 2 days
- **Blocks 3-5 (parallel):** 3-4 days each = 6-8 days total
- **Block 6 (chaos):** 3-4 days
- **Blocks 7-8 (parallel):** 2-3 days each = 4-6 days total
- **Testing & validation:** 2-3 days

**Total estimated:** 3-4 weeks (19-30 days)
**Parallel execution:** With proper dependency management, ~2 weeks

---

## Phase 32 Deliverables

1. **Integration tests:** 8 test modules, 88-120 tests, 5,000-6,000 LOC
2. **Performance baselines:** Throughput, latency, I/O amplification for real cluster
3. **Runbooks:** DR procedures, failure recovery steps
4. **Prometheus dashboards:** Production monitoring setup
5. **CHANGELOG updates:** Milestone commits after each block
6. **GitHub Issues:** Known limitations, future work

---

## Handoff to Phase 33+

Phase 33 (future) will focus on:
- **Long-term stability:** 30-60 day production simulation
- **Cost optimization:** S3 tiering cost analysis, Glacier lifecycle
- **Performance tuning:** Optimize dedup shard placement, compression ratio
- **Operational automation:** Auto-scaling, auto-failover scripting
- **Multi-region deployment:** 3+ geographic regions with replication

---

**Next Step:** Upon A11 Phase 5 Block 1 completion, begin Block 1 cluster setup validation.
