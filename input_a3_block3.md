# A3 Phase 32 Block 3: Multi-Node Dedup Coordination Tests

## Task
Implement 16-20 integration tests for multi-node dedup coordination in `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs`.

## Context
- Block 1 (cluster health) and Block 2 (single-node dedup) already exist in this crate
- Tests should validate dedup coordination across multiple storage nodes with real fingerprint routing, shard leaders, replicas, and failure scenarios
- Target: 800-950 LOC, 16-20 tests
- Uses existing helpers from `cluster_helpers.rs` and `cluster_single_node_dedup.rs`

## Architecture Context
- **Hash ring for shard routing:** Fingerprint hash determines shard (8 shards typical), shard determines primary and replicas
- **D2 SWIM protocol:** Failure detection across nodes
- **D4 Multi-Raft:** Each shard has Raft group (leader + 2 replicas typical)
- **LWW conflict resolution:** Last-write-wins for concurrent writes to same fingerprint

## Required Helper Functions

```rust
// Identify shard leader for a fingerprint hash
fn identify_shard_leader(fingerprint_hash: u64) -> Result<String, String>

// Get replica nodes for a fingerprint hash  
fn get_shard_replicas(fingerprint_hash: u64) -> Result<Vec<String>, String>

// Write data from specific node via SSH
fn write_from_node(node_ip: &str, path: &str, size_mb: usize) -> Result<(), String>

// Query fingerprint routing info (shard, leader, replicas)
#[derive(Debug)]
struct RoutingInfo {
    shard_id: u32,
    leader: String,
    replicas: Vec<String>,
}
fn query_fingerprint_routing(fingerprint: &str) -> Result<RoutingInfo, String>

// Wait for replica consistency across nodes
fn wait_for_replica_consistency(fingerprint: &str, timeout_secs: u64) -> Result<(), String>

// Simulate node failure (kill process or stop service)
fn simulate_node_failure(node_ip: &str) -> Result<(), String>

// Restore node after failure
fn restore_node(node_ip: &str) -> Result<(), String>

// Simulate network partition (iptables drop)
fn simulate_network_partition(node_ips: &[&str]) -> Result<(), String>

// Remove network partition
fn remove_network_partition(node_ips: &[&str]) -> Result<(), String>

// Get all storage node IPs from environment
fn get_storage_nodes() -> Vec<String>
```

## Required Tests (20 total)

1. **test_cluster_two_nodes_same_fingerprint_coordination** — 2 nodes write identical data, verify coordination via metrics
2. **test_cluster_dedup_shards_distributed_uniformly** — Write many files, verify fingerprints across 8 shards
3. **test_cluster_dedup_shard_leader_routing** — Writes route to correct shard leader
4. **test_cluster_dedup_shard_replica_consistency** — Followers replicate leader state
5. **test_cluster_dedup_three_node_write_conflict** — 3 nodes write same fingerprint, verify LWW wins
6. **test_cluster_dedup_refcount_coordination_race** — Concurrent refcount increments across nodes
7. **test_cluster_dedup_cache_coherency_multi_node** — Cross-node cache invalidation works
8. **test_cluster_dedup_gc_coordination_multi_node** — GC coordinated (no duplicate GC)
9. **test_cluster_dedup_tiering_multi_node_consistency** — Atomic multi-node tiering
10. **test_cluster_dedup_node_failure_shard_failover** — Node fail triggers leader election
11. **test_cluster_dedup_network_partition_shard_split** — Partition detection and quorum
12. **test_cluster_dedup_cascade_node_failures** — 2 of 5 nodes fail, graceful degradation
13. **test_cluster_dedup_throughput_5_nodes_linear** — 5 nodes achieves ~5x throughput
14. **test_cluster_dedup_latency_multinode_p99** — P99 latency <150ms with coordination
15. **test_cluster_dedup_cross_node_snapshot_consistency** — Snapshot during writes works
16. **test_cluster_dedup_journal_replay_after_cascade_failure** — Kill 2 nodes, recover both
17. **test_cluster_dedup_worm_enforcement_multi_node** — WORM files not deduplicated
18. **test_cluster_dedup_tenant_isolation_multi_node** — Tenant quotas enforced per node
19. **test_cluster_dedup_metrics_aggregation** — Prometheus metrics aggregated across nodes
20. **test_cluster_multinode_dedup_ready_for_next_blocks** — Summary pass/fail

## Test Patterns

Each test should:
- Use `#[test]` and `#[ignore]` attributes
- Check cluster availability at start, skip if not available
- Use environment variables: `CLAUDEFS_STORAGE_NODE_IPS` (comma-separated IPs)
- Use SSH commands via existing helpers
- Query Prometheus metrics for validation
- Clean up test files after completion
- Use `Result<(), String>` return type with clear error messages

## Environment Variables
- `CLAUDEFS_STORAGE_NODE_IPS` — Comma-separated list of storage node IPs
- `CLAUDEFS_CLIENT_NODE_IPS` — Comma-separated list of client IPs
- `PROMETHEUS_URL` — Prometheus endpoint (default: http://localhost:9090)
- `CLAUDEFS_S3_BUCKET` — S3 bucket for tiering tests
- `SSH_PRIVATE_KEY` — SSH key path (default: ~/.ssh/id_rsa)
- `SSH_USER` — SSH user (default: ubuntu)

## Metrics to Query
- `claudefs_dedup_fingerprints_stored_total`
- `claudefs_dedup_references_total`
- `claudefs_dedup_shard_queries_total`
- `claudefs_dedup_coordination_conflicts_total`
- `claudefs_dedup_write_latency_seconds_bucket`
- `claudefs_dedup_cache_hits_total`
- `claudefs_dedup_cache_evictions_total`
- `claudefs_tiering_bytes_to_s3_total`
- `claudefs_replication_lag_seconds`

## Error Handling
- Use `Result<(), String>` for all test functions
- Clear error messages for failures (shard routing, leader election, partition detection)
- Skip with message if multi-node coordination not available

## Performance Targets
- 5 nodes linear scaling: 5x throughput vs single node
- P99 latency: <150ms with multi-node coordination
- Throughput: >= 50K fingerprints/sec at cluster level

## Compilation Requirements
- Must compile without errors
- Zero clippy warnings
- Must pass on properly functioning 5-node cluster

## Output
Write the complete test file to: `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs`