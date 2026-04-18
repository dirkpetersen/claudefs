# A3 Phase 32 Block 7: Performance Benchmark Tests Implementation

## Task
Create a new test file at `crates/claudefs-reduce/tests/cluster_performance_benchmarks.rs` with 10-14 performance benchmark tests.

## Target Specifications

### Test Count: 10-14 tests
### Target LOC: 600-750 lines

### Required Tests:

1. `test_cluster_benchmark_single_node_throughput` — Single storage node max throughput (MB/s)
2. `test_cluster_benchmark_multi_node_throughput_5x` — 5 nodes → 5x throughput (±10% tolerance)
3. `test_cluster_benchmark_latency_p50_p99_p999` — Latency distribution (ms): p50, p99, p999
4. `test_cluster_benchmark_cache_hit_ratio` — Cache effectiveness (% hits vs total)
5. `test_cluster_benchmark_memory_utilization` — Memory footprint under load (GB)
6. `test_cluster_benchmark_cpu_utilization_per_node` — CPU% per node under max throughput
7. `test_cluster_benchmark_network_bandwidth_utilization` — Network saturation (% of 100Gbps or 10Gbps)
8. `test_cluster_benchmark_s3_tiering_throughput` — S3 tiering rate (MB/s hot-to-cold)
9. `test_cluster_benchmark_dedup_compression_ratio` — Actual compression achieved (vs Phase 31)
10. `test_cluster_benchmark_coordination_overhead` — Coordination latency impact (% overhead)
11. `test_cluster_benchmark_concurrent_clients_scaling` — Throughput vs client count (1 client baseline, 2x client)
12. `test_cluster_benchmark_large_file_performance` — 10GB+ file write/read (time to complete)
13. `test_cluster_benchmark_small_file_performance` — 1KB file operations (ops/sec)
14. `test_cluster_benchmark_mixed_workload_iops_mbs` — Mixed workload: IOPs vs MB/s

### Helper Functions to Implement:

1. `run_throughput_test(client_id: usize, duration_secs: u64, block_size: usize) -> Result<ThroughputResult, String>`
   - Returns: bytes/sec, p50/p99/p999 latency, ops/sec

2. `measure_cache_statistics() -> Result<CacheStats, String>`
   - Returns: hits, misses, hit_ratio%

3. `get_node_resource_utilization(node_id: &str) -> Result<ResourceStats, String>`
   - Returns: cpu%, memory_gb, network_mbps

4. `run_tiering_benchmark(file_size_mb: usize) -> Result<TieringBenchmark, String>`
   - Returns: hot-to-cold throughput, completion time

5. `measure_coordination_overhead() -> Result<f32, String>`
   - Returns: % latency overhead vs single-node

6. `run_latency_percentile_test(samples: usize) -> Result<LatencyDistribution, String>`
   - Returns: p50, p90, p99, p999, p99.9 (ms)

### Data Structures to Create:

```rust
#[derive(Debug, Clone)]
pub struct ThroughputResult {
    pub bytes_per_sec: u64,
    pub mb_per_sec: f64,
    pub ops_per_sec: f64,
    pub p50_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub p999_latency_ms: f64,
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub hit_ratio: f64,
}

#[derive(Debug, Clone)]
pub struct ResourceStats {
    pub cpu_percent: f64,
    pub memory_gb: f64,
    pub network_mbps: f64,
}

#[derive(Debug, Clone)]
pub struct TieringBenchmark {
    pub throughput_mb_per_sec: f64,
    pub completion_time_secs: f64,
}

#[derive(Debug, Clone)]
pub struct LatencyDistribution {
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub p9999_ms: f64,
}

#[derive(Debug, Clone)]
pub struct CompressionStats {
    pub original_bytes: u64,
    pub compressed_bytes: u64,
    pub ratio: f64,
}
```

### Error Handling:
- Use `Result<(), String>` for test functions
- Benchmark timeouts: 10 minutes per test
- Sample sizes: 1000+ ops per test for statistical significance

### Assertions:
- Throughput: single-node ≥500 MB/s (baseline from Phase 31)
- Throughput: 5 nodes ≥ 2x single-node (allowing 40% coordination overhead)
- Latency: p99 <200ms for single-node, <300ms for 5-node
- Cache hit ratio: >80% for hot data
- CPU utilization: <80% per node
- No OOM during sustained benchmark

### Conventions:
- All tests must be marked `#[ignore]`
- Use helper functions from `cluster_helpers.rs` (import them)
- Follow the pattern from `cluster_multinode_dedup.rs`
- Use `std::time::Instant` for timing
- Export results to CSV or JSON for trending
- Result<(), String> for error handling

### Dependencies:
The test file can use:
- `std::time::{Duration, Instant}`
- `std::process::Command`
- Helper functions from `cluster_helpers.rs` (already available)
- `serde` and `serde_json` for CSV/JSON export (already in dev-dependencies)
- `urlencoding` for Prometheus queries (already in dev-dependencies)

### Phase 31 Baselines (for comparison):
- Single-node throughput: ~600 MB/s (simulated)
- P99 latency: ~120ms (simulated)
- Cache hit ratio: ~85% (hot workload)

### Phase 32 Expectations:
- Single-node: 400-800 MB/s (real I/O variance)
- Multi-node 5x: ≥ 2x single-node
- P99 latency: <300ms for 5-node

## Output
Generate the complete Rust test file with all 14 benchmark tests, helper functions, and data structures.

Use model: fireworks-ai/accounts/fireworks/models/minimax-m2p5