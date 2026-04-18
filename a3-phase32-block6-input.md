# A3: Phase 32 Block 6 — Chaos Engineering & Resilience
## OpenCode Implementation Prompt

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Task:** Implement 16-20 chaos engineering tests for cluster resilience
**Target File:** `crates/claudefs-reduce/tests/cluster_chaos_resilience.rs`
**Target LOC:** 900-1100
**Target Tests:** 16-20

---

## High-Level Specs

### Block 6 Purpose
Validate system resilience through **deliberate failure injection**: node crashes, network partitions, disk full, memory pressure, concurrent failures, and recovery verification.

### Key Tests
1. `test_cluster_chaos_random_node_failures` — Kill random storage node, verify recovery
2. `test_cluster_chaos_cascade_failures_2_of_5` — Kill 2/5 storage nodes, system survives
3. `test_cluster_chaos_storage_node_restart` — Kill node, restart, data intact
4. `test_cluster_chaos_fuse_client_disconnect` — Client network down, recovery
5. `test_cluster_chaos_metadata_shard_partition` — Metadata shard split, quorum detects
6. `test_cluster_chaos_network_latency_injection` — Add 100ms latency, measure impact
7. `test_cluster_chaos_packet_loss_5_percent` — 5% packet loss, adaptive retry
8. `test_cluster_chaos_disk_full_on_storage_node` — Disk full, graceful backpressure
9. `test_cluster_chaos_memory_pressure_on_node` — Memory constrained, no OOM kills
10. `test_cluster_chaos_concurrent_client_and_node_failures` — Client + node fail simultaneously
11. `test_cluster_chaos_s3_availability_zones_down` — S3 region down, fallback
12. `test_cluster_chaos_power_cycle_node` — Abrupt power loss (simulated), recovery
13. `test_cluster_chaos_disk_corruption_detection` — Corrupt S3 chunk, detected
14. `test_cluster_chaos_shard_replica_corruption` — Corrupt replica, consistency check catches
15. `test_cluster_chaos_replication_lag_spike` — Cross-site lag spikes, recovery
16. `test_cluster_chaos_metadata_split_brain` — Quorum split, recovery to leader
17. `test_cluster_chaos_sustained_failures_24hr` — Multiple failures over time
18. `test_cluster_chaos_concurrent_tiering_failures` — Failures during active tiering
19. `test_cluster_chaos_recovery_ordering` — Nodes recover in different orders, consistent state
20. `test_cluster_chaos_all_resilience_ready` — All chaos patterns validated

### Prerequisites
- Block 1 ✅ (cluster health)
- Block 2 ✅ (single-node dedup)
- Block 3 ✅ (multi-node coordination)
- Block 5 ✅ (multi-client, now tested against failures)

### Helper Functions to Create
- `kill_node(node_id: &str) -> Result<(), String>` — SSH kill, return immediately
- `restart_node(node_id: &str, wait_healthy: bool) -> Result<(), String>` — SSH start, wait boot
- `get_node_status(node_id: &str) -> Result<NodeStatus, String>` — Running? Disk full? Memory?
- `inject_network_latency(node_id: &str, latency_ms: u32) -> Result<(), String>` — Via tc/iptables
- `remove_network_latency(node_id: &str) -> Result<(), String>`
- `inject_packet_loss(node_id: &str, loss_percent: f32) -> Result<(), String>`
- `remove_packet_loss(node_id: &str) -> Result<(), String>`
- `fill_disk(node_id: &str, percentage: f32) -> Result<(), String>` — Fill to N%
- `clear_disk_fill(node_id: &str) -> Result<(), String>` — Release space
- `check_data_integrity_after_chaos(files: &[&str]) -> Result<(), String>` — Verify checksums
- `get_recovery_time_and_consistency() -> Result<(Duration, bool), String>` — Time + ok?

### Error Handling
- Use `Result<(), String>` for tests
- Chaos should not crash tests; test validates graceful degradation
- Recovery timeouts: 5 minutes max per node
- Data integrity: 100% (assert no data loss)

### Assertions
- Verify system remains operational (reduced capacity)
- Verify no data loss or corruption
- Verify recovery time acceptable (<5 min per node)
- Verify metrics updated correctly during chaos
- Verify consistent state after recovery

### Test Execution
- Depends on Block 1-5 passing
- Sequential execution (cleanup between chaos scenarios)
- Total runtime target: 20-30 minutes (chaos + recovery time)

---

## Full Implementation Details

See `a3-phase32-blocks-3-8-plan.md` section "Block 6: Chaos Engineering & Resilience (16-20 tests)" for complete specifications including:
- Detailed test implementations for all 20 tests
- Failure modes (node crash, partition, resource exhaustion)
- Expected recovery behavior (quorum, leader election, backoff)
- Consistency validation (checksum, refcount, metadata)
- Performance degradation acceptance levels
- Concurrent failure scenarios

---

## Success Criteria

✅ All 16-20 tests compile without errors
✅ All tests marked `#[ignore]` (require real cluster)
✅ Zero clippy warnings in new code
✅ Tests pass with graceful degradation observed
✅ No data loss during any chaos scenario
✅ Recovery time <5 min per node
✅ Consistency verified after recovery
✅ Ready for Block 7 (Performance) and Block 8 (DR)

---

## Output Specification

Generate a complete, production-ready Rust test file:
- Use existing helper functions from cluster_helpers.rs where applicable
- Create new helpers as specified above
- All tests marked `#[ignore]`
- Follow conventions from cluster_multinode_dedup.rs and cluster_multi_client_workloads.rs
- Use `std::process::Command` for SSH and network manipulation
- Result<(), String> for error handling
- Comprehensive assertions with descriptive messages
- Include cleanup on test end (remove latency, reconnect nodes, restore capacity)

Expected line count: 900-1100 LOC
Expected test count: 16-20
