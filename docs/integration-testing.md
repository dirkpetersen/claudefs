# ClaudeFS Integration Testing Guide

**Phase 3 Focus:** Multi-node cluster testing, operational procedures validation, disaster recovery verification.

## Integration Test Scope

Integration tests verify behavior across multiple ClaudeFS components and cluster nodes. They complement unit tests (which test single components in isolation) by validating end-to-end functionality.

### Test Categories

1. **Cluster Formation & Health** — Verify SWIM membership, leader election, quorum
2. **Metadata Consistency** — Cross-node metadata replication, shard routing
3. **Data Operations** — Write/read/delete across nodes, data placement
4. **Raft Consensus** — Leader election, log replication, pre-vote protocol
5. **Failure Recovery** — Node failure, leader loss, network partition
6. **Scaling Operations** — Node join, node drain, rebalancing
7. **Performance Benchmarks** — Throughput, latency, scalability

## Test Infrastructure

### Test Cluster Architecture

```
┌─────────────────────────────────────────────────┐
│  Integration Test Controller (cargo test)      │
│  ┌────────────────────────────────────────────┐ │
│  │ Test Harness: spawn_cluster(3, config)    │ │
│  │  ├─ Node 1: storage + metadata leader    │ │
│  │  ├─ Node 2: storage + metadata follower  │ │
│  │  ├─ Node 3: storage + metadata follower  │ │
│  │  └─ Virtual FUSE client (loopback)       │ │
│  └────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────┘
```

### Key Components

**LocalClusterHarness** (new module: `crates/claudefs-meta/tests/integration.rs`)
- Spawns in-process cluster nodes for fast testing
- Manages RPC transport (loopback via std channels)
- Provides test utilities: read_file(), write_file(), check_quorum()

**Transport Shim** (`tests/mock_transport.rs`)
- Simulates network behavior: latency, packet loss, partition
- Enables failure injection tests

**Checkpoint System** (`tests/cluster_snapshot.rs`)
- Save/restore cluster state for DR testing
- Verify recovery from checkpoints

## Test Suite by Category

### 1. Cluster Formation Tests

```rust
#[tokio::test]
async fn test_cluster_bootstrap() {
    // Three nodes start up, discover each other, form quorum
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;
    assert_eq!(cluster.leader().await.unwrap().node_id(), 0); // Arbitrary leader elected
    assert_eq!(cluster.healthy_nodes().await, 3);
}

#[tokio::test]
async fn test_leader_election() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;
    let original_leader = cluster.leader().await.unwrap().node_id();

    // Partition leader from followers
    cluster.partition(vec![original_leader]).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // New leader elected among followers
    let new_leader = cluster.leader().await.unwrap().node_id();
    assert_ne!(new_leader, original_leader);
}
```

### 2. Metadata Consistency Tests

```rust
#[tokio::test]
async fn test_metadata_replication() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;

    // Write file via leader
    let inode = cluster.leader_create_file("/test.txt").await.unwrap();

    // Read from all nodes (replication has propagated)
    for node_id in 0..3 {
        let result = cluster.node(node_id).lookup_inode(inode).await;
        assert!(result.is_ok(), "Inode should be visible on all nodes");
    }
}

#[tokio::test]
async fn test_shard_routing() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;

    // Create 100 files, verify shard routing
    for i in 0..100 {
        let inode = cluster.leader_create_file(&format!("/file_{}", i)).await?;
        let shard = inode.shard_id();
        assert!(shard < 256); // Default 256 shards
    }
}
```

### 3. Raft Consensus Tests

```rust
#[tokio::test]
async fn test_raft_pre_vote() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;

    // Isolated node starts pre-election (doesn't disrupt cluster)
    let node = cluster.node(1);
    node.start_pre_election().await;

    // Cluster leader unchanged
    let leader = cluster.leader().await.unwrap();
    assert_eq!(leader.node_id(), 0);
}

#[tokio::test]
async fn test_log_replication() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;

    // Propose 10 mutations via leader
    for i in 0..10 {
        cluster.leader_apply_mutation(MockMutation::CreateFile(format!("file_{}", i))).await?;
    }

    // Verify all followers have same log entries
    for node_id in 1..3 {
        let log_len = cluster.node(node_id).log_length().await;
        assert_eq!(log_len, 10);
    }
}
```

### 4. Failure Recovery Tests

```rust
#[tokio::test]
async fn test_single_node_failure() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;

    // Kill node 1
    cluster.kill_node(1).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Cluster remains operational (2-node quorum)
    let health = cluster.cluster_status().await;
    assert_eq!(health.alive_count, 2);
    assert!(cluster.leader().await.is_ok());

    // Operations still succeed
    cluster.leader_create_file("/test.txt").await.ok();
}

#[tokio::test]
async fn test_leader_failure_and_recovery() {
    let mut cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;
    let original_leader = cluster.leader().await?.node_id();

    // Kill leader
    cluster.kill_node(original_leader).await;

    // New leader elected
    let new_leader = cluster.leader().await?.node_id();
    assert_ne!(new_leader, original_leader);

    // Resurrect killed node
    cluster.resurrect_node(original_leader).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Reintegrated as follower
    assert_eq!(cluster.cluster_status().await.alive_count, 3);
}

#[tokio::test]
async fn test_network_partition() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;

    // Partition: [0] vs [1, 2]
    cluster.partition(vec![0]).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Minority partition (node 0) loses leadership
    let status = cluster.node(0).status().await;
    assert_eq!(status.state, RaftState::Follower);

    // Majority partition (nodes 1-2) elects leader
    let leader = cluster.leader_in_partition(vec![1, 2]).await;
    assert!(leader.is_ok());
}
```

### 5. Scaling Tests

```rust
#[tokio::test]
async fn test_node_join() {
    let mut cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;

    // Add node 4
    cluster.add_node(4, DEFAULT_CONFIG).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify rebalancing
    let health = cluster.cluster_status().await;
    assert_eq!(health.alive_count, 4);

    // New node catches up on metadata
    let log_len = cluster.node(4).log_length().await;
    assert!(log_len > 0);
}

#[tokio::test]
async fn test_node_drain() {
    let cluster = LocalClusterHarness::new(5, DEFAULT_CONFIG).await;

    // Drain node 4 (graceful shutdown)
    cluster.drain_node(4, Duration::from_secs(5)).await?;

    // Node transfers leadership if leader, transitions to follower
    // Open transactions continue, new transactions blocked
}
```

### 6. Performance Benchmarks

```rust
#[tokio::test]
async fn bench_metadata_throughput() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;
    let start = Instant::now();

    // Create 10,000 files
    for i in 0..10000 {
        cluster.leader_create_file(&format!("/file_{:06}", i)).await?;
    }

    let elapsed = start.elapsed();
    let throughput = 10000.0 / elapsed.as_secs_f64();

    println!("Metadata creation throughput: {:.0} ops/sec", throughput);
    assert!(throughput > 1000.0, "Should exceed 1000 ops/sec");
}
```

## Test Execution

### Run All Integration Tests

```bash
# Unit tests only (fast)
cargo test --lib

# Integration tests only
cargo test --test '*' -- --test-threads=1 --nocapture

# All tests
cargo test
```

### Run Specific Test Category

```bash
# Cluster formation
cargo test test_cluster_ -- --nocapture

# Failure recovery
cargo test test_.*_failure -- --nocapture

# Performance benchmarks
cargo test bench_ -- --nocapture
```

### Generate Test Report

```bash
# Run with detailed output
cargo test --lib -- --nocapture 2>&1 | tee test_report.txt

# Extract metrics
grep "throughput\|latency\|ops/sec" test_report.txt
```

## CI/CD Integration

### GitHub Actions Workflow

```yaml
# .github/workflows/integration.yml
name: Integration Tests
on: [push, pull_request]

jobs:
  integration:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - name: Run integration tests
        run: cargo test --test '*' -- --nocapture
      - name: Upload test report
        if: failure()
        uses: actions/upload-artifact@v3
        with:
          name: integration-test-report
          path: test_report.txt
```

## Debugging Failed Tests

### Enable Tracing

```rust
#[tokio::test]
async fn test_with_tracing() {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .try_init()
        .ok();

    // Test code with full debug output
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;
}
```

Run with tracing:
```bash
RUST_LOG=debug cargo test test_name -- --nocapture
```

### Capture Cluster State

```rust
#[tokio::test]
async fn test_with_checkpoint() {
    let cluster = LocalClusterHarness::new(3, DEFAULT_CONFIG).await;

    // Save state before operation
    let checkpoint = cluster.checkpoint().await;

    // Perform test operation
    cluster.leader_create_file("/test.txt").await?;

    // If test fails, can restore and debug
    // cluster.restore(checkpoint).await;
}
```

## Future Enhancements

1. **Jepsen-style Testing** — Full adversarial model discovery via A9
2. **Continuous Fuzzing** — libfuzzer for RPC protocol, state machine
3. **Chaos Engineering** — Random failure injection, property-based testing
4. **Multi-site Testing** — Cross-datacenter replication with A6
5. **Performance Regression Detection** — Automated benchmarking in CI

## References

- **Raft Consensus:** [raft.github.io](https://raft.github.io)
- **Jepsen:** [jepsen.io](https://jepsen.io) — distributed systems testing framework
- **CrashMonkey:** [github.com/utsaslab/crashmonkey](https://github.com/utsaslab/crashmonkey) — file system crash consistency testing
- **FIO:** [fio.readthedocs.io](https://fio.readthedocs.io) — flexible I/O tester for performance benchmarks
