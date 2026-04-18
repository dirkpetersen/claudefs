# A3: Phase 32 Block 3 — Multi-Node Dedup Coordination
## OpenCode Implementation Prompt (Summary)

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Task:** Implement 16-20 integration tests for multi-node dedup coordination on real cluster
**Target File:** `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs`
**Target LOC:** 800-950
**Target Tests:** 16-20

---

## High-Level Specs

### Block 3 Purpose
Validate dedup coordination **across multiple storage nodes** with real fingerprint routing, shard leaders, replicas, and failure scenarios.

### Key Tests (Condensed)
1. `test_cluster_two_nodes_same_fingerprint_coordination` — 2 nodes write same block, coordinate
2. `test_cluster_dedup_shards_distributed_uniformly` — Fingerprints across 8 shards
3. `test_cluster_dedup_shard_leader_routing` — Writes route to shard leader
4. `test_cluster_dedup_shard_replica_consistency` — Followers replicate leader state
5. `test_cluster_dedup_three_node_write_conflict` — 3 nodes, LWW conflict resolution
6. `test_cluster_dedup_refcount_coordination_race` — Concurrent refcount updates
7. `test_cluster_dedup_cache_coherency_multi_node` — Cross-node cache invalidation
8. `test_cluster_dedup_gc_coordination_multi_node` — GC coordination (no duplicate GC)
9. `test_cluster_dedup_tiering_multi_node_consistency` — Atomic multi-node tiering
10. `test_cluster_dedup_node_failure_shard_failover` — Node fail → leader election
11. `test_cluster_dedup_network_partition_shard_split` — Partition → quorum detection
12. `test_cluster_dedup_cascade_node_failures` — 2 of 5 nodes fail, graceful degradation
13. `test_cluster_dedup_throughput_5_nodes_linear` — 5 nodes → 5x throughput
14. `test_cluster_dedup_latency_multinode_p99` — P99 <150ms with coordination
15. `test_cluster_dedup_cross_node_snapshot_consistency` — Snapshot during writes
16. `test_cluster_dedup_journal_replay_after_cascade_failure` — Kill 2, recover both
17. `test_cluster_dedup_worm_enforcement_multi_node` — WORM not deduplicated
18. `test_cluster_dedup_tenant_isolation_multi_node` — Tenant quotas per node
19. `test_cluster_dedup_metrics_aggregation` — Prometheus metrics aggregated
20. `test_cluster_multinode_dedup_ready_for_next_blocks` — Summary pass/fail

### Prerequisites
- Block 1 ✅ (cluster health)
- Block 2 ✅ (single-node dedup)

### Helper Functions to Create
- `identify_shard_leader(fingerprint_hash: u64) -> Result<StorageNodeId, String>`
- `get_shard_replicas(fingerprint_hash: u64) -> Result<Vec<StorageNodeId>, String>`
- `write_from_node(node_id: &str, data: &[u8]) -> Result<(), String>`
- `query_fingerprint_routing(fingerprint: &str) -> Result<RoutingInfo, String>`
- `wait_for_replica_consistency(fingerprint: &str, timeout: Duration) -> Result<(), String>`
- `simulate_node_failure(node_id: &str) -> Result<(), String>`
- `simulate_network_partition(node_ids: &[&str]) -> Result<(), String>`

### Error Handling
- Use `Result<(), String>` for tests
- Clear messages for shard failures, leader election timeouts, partition scenarios
- If multi-node coordination not available: skip with explicit message

### Assertions
- Standard `assert!()`, `assert_eq!()` with descriptive messages
- Verify shard leader, replica consistency, failover success

### Test Execution
- Depends on Block 1 & 2 passing
- Sequential execution (coordination tests need deterministic state)
- Total runtime target: 10-15 minutes

---

## Full Implementation Details

See `a3-phase32-blocks-3-8-plan.md` section "Block 3: Multi-Node Dedup Coordination (16-20 tests)" for complete specifications including:
- Detailed test implementations for all 20 tests
- Architecture context (D2 SWIM, D4 Multi-Raft, shard routing)
- Failure scenarios (node failure, partition, cascade failures)
- Performance expectations (5x linear, P99 <150ms)
- Consistency requirements (LWW, snapshot, journal replay)

---

## Success Criteria

✅ All 16-20 tests compile without errors
✅ All tests pass on properly functioning 5-node cluster
✅ Shard coordination verified (leader election, replica consistency)
✅ Failure scenarios handled correctly (failover, partition detection)
✅ Performance scaling validated (near-linear 5x with 5 nodes)
✅ Zero clippy warnings

