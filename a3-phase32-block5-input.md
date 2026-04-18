# A3: Phase 32 Block 5 — Multi-Client Workloads
## OpenCode Implementation Prompt

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Task:** Implement 14-18 integration tests for multi-client workloads on real cluster
**Target File:** `crates/claudefs-reduce/tests/cluster_multi_client_workloads.rs`
**Target LOC:** 750-900
**Target Tests:** 14-18

---

## High-Level Specs

### Block 5 Purpose
Validate dedup behavior with **two concurrent FUSE clients** writing to the same cluster, including cache coherency, quota enforcement, coordination, and failure scenarios.

### Key Tests
1. `test_cluster_two_clients_concurrent_writes` — Both clients write concurrently, no corruption
2. `test_cluster_two_clients_same_file_coordination` — Both write same file, LWW resolution
3. `test_cluster_two_clients_dedup_shared_data` — Same fingerprints shared across clients
4. `test_cluster_two_clients_quota_per_client` — Each client has independent quota
5. `test_cluster_two_clients_cache_coherency_across_clients` — Cache invalidation on peer writes
6. `test_cluster_two_clients_refcount_coordination_concurrent` — Concurrent refcount updates race-free
7. `test_cluster_two_clients_one_fails` — Client 1 crashes, Client 2 continues normally
8. `test_cluster_two_clients_snapshot_consistency` — Snapshot while both clients writing
9. `test_cluster_two_clients_read_after_write_different_client` — Write on C1, read on C2
10. `test_cluster_two_clients_metadata_consistency_reads` — Both see same metadata
11. `test_cluster_two_clients_performance_parallel_writes` — Throughput with 2 clients
12. `test_cluster_two_clients_network_partition_between_clients` — Partition, recovery
13. `test_cluster_two_clients_delete_coordination` — Delete on C1, C2 sees update
14. `test_cluster_two_clients_replication_consistency_cross_site` — Multi-site + multi-client
15. `test_cluster_two_clients_latency_p99_concurrent` — P99 latency under concurrent load
16. `test_cluster_two_clients_mixed_workload_production_like` — Production-like mixed workload
17. `test_cluster_two_clients_10x_throughput` — 2 clients approaching 2x throughput
18. `test_cluster_multi_client_ready_for_chaos` — All tests passed

### Prerequisites
- Block 1 ✅ (cluster health)
- Block 2 ✅ (single-node dedup)
- Block 3 ✅ (multi-node coordination)

### Helper Functions to Create
- `write_from_client(client_id: usize, path: &str, data: &[u8]) -> Result<(), String>`
- `read_from_client(client_id: usize, path: &str) -> Result<Vec<u8>, String>`
- `delete_from_client(client_id: usize, path: &str) -> Result<(), String>`
- `snapshot_from_client(client_id: usize) -> Result<SnapshotId, String>`
- `get_client_quota(client_id: usize) -> Result<QuotaInfo, String>`
- `simulate_client_failure(client_id: usize) -> Result<(), String>`
- `reconnect_client(client_id: usize) -> Result<(), String>`
- `measure_concurrent_throughput(client_ids: &[usize], duration: Duration) -> Result<u64, String>`

### Error Handling
- Use `Result<(), String>` for tests
- Clear messages for coordination failures, quota violations
- If multi-client not available: skip with explicit message

### Assertions
- Standard `assert!()`, `assert_eq!()` with descriptive messages
- Verify cache coherency, quota enforcement, throughput improvement

### Test Execution
- Depends on Block 1, 2, 3 passing
- Some tests sequential (quota/snapshot), some parallel (independent writes)
- Total runtime target: 12-15 minutes

---

## Full Implementation Details

See `a3-phase32-blocks-3-8-plan.md` section "Block 5: Multi-Client Workloads (14-18 tests)" for complete specifications including:
- Detailed test implementations for all 18 tests
- Architecture context (quota system, cache coherency)
- Multi-client scenarios (concurrent writes, coordination, failures)
- Performance expectations (2x throughput with 2 clients)
- Consistency requirements (metadata, refcounts)

---

## Success Criteria

✅ All 14-18 tests compile without errors
✅ All tests marked `#[ignore]` (require real cluster with 2 FUSE clients)
✅ Zero clippy warnings in new code
✅ Tests pass on cluster with both clients operational
✅ Multi-client coordination verified (no data loss, no corruption)
✅ Cache coherency working (peer writes invalidate caches)
✅ Quota enforcement per-client
✅ Ready for Block 6 (Chaos & Resilience)

---

## Output Specification

Generate a complete, production-ready Rust test file:
- Use existing helper functions from cluster_helpers.rs where applicable
- Create new helpers as specified above
- All tests marked `#[ignore]`
- Follow conventions from cluster_single_node_dedup.rs and cluster_multinode_dedup.rs
- Use `std::process::Command` for SSH to client nodes
- Result<(), String> for error handling
- Comprehensive assertions with descriptive messages

Expected line count: 750-900 LOC
Expected test count: 14-18
