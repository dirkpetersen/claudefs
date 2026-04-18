# A3: Phase 32 Block 7 — Performance Benchmarking
## OpenCode Implementation Prompt

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Task:** Implement 10-14 performance benchmark tests on real cluster
**Target File:** `crates/claudefs-reduce/tests/cluster_performance_benchmarks.rs`
**Target LOC:** 600-750
**Target Tests:** 10-14

---

## High-Level Specs

### Block 7 Purpose
Establish **performance baselines** for production: throughput, latency distribution, cache effectiveness, resource utilization, and scaling characteristics with 1-5 storage nodes and 1-2 clients.

### Key Tests
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

### Prerequisites
- Block 1 ✅ (cluster health)
- Block 2 ✅ (single-node dedup)
- Block 3 ✅ (multi-node coordination)
- Block 5 ✅ (multi-client, now benchmarked)

### Helper Functions to Create
- `run_throughput_test(client_id: usize, duration_secs: u64, block_size: usize) -> Result<ThroughputResult, String>`
  - Returns: bytes/sec, p50/p99/p999 latency, ops/sec
- `measure_cache_statistics() -> Result<CacheStats, String>`
  - Returns: hits, misses, hit_ratio%
- `get_node_resource_utilization(node_id: &str) -> Result<ResourceStats, String>`
  - Returns: cpu%, memory_gb, network_mbps
- `run_tiering_benchmark(file_size_mb: usize) -> Result<TieringBenchmark, String>`
  - Returns: hot-to-cold throughput, completion time
- `measure_coordination_overhead() -> Result<f32, String>`
  - Returns: % latency overhead vs single-node
- `run_latency_percentile_test(samples: usize) -> Result<LatencyDistribution, String>`
  - Returns: p50, p90, p99, p999, p99.9 (ms)

### Error Handling
- Use `Result<(), String>` for tests
- Benchmark timeouts: 10 minutes per test
- Sample sizes: 1000+ ops per test for statistical significance
- Store results in structured format for later comparison

### Assertions
- Throughput: single-node ≥500 MB/s (baseline from Phase 31)
- Throughput: 5 nodes ≥ 2x single-node (allowing 40% coordination overhead)
- Latency: p99 <200ms for single-node, <300ms for 5-node
- Cache hit ratio: >80% for hot data
- CPU utilization: <80% per node (headroom for burst)
- No OOM during sustained benchmark

### Test Execution
- Depends on Block 1-5 passing
- Sequential execution (benchmarks can interfere)
- Total runtime target: 15-20 minutes (includes warmup, cooldown)

---

## Full Implementation Details

See `a3-phase32-blocks-3-8-plan.md` section "Block 7: Performance Benchmarking (10-14 tests)" for complete specifications including:
- Detailed benchmark implementations for all 14 tests
- Workload patterns (sequential, random, mixed)
- Sampling strategy (1000+ ops per metric)
- Baseline thresholds (from Phase 31 simulation)
- Metric collection (CSV export for trending)
- Comparison methodology (vs Phase 31, vs previous phases)

---

## Success Criteria

✅ All 10-14 tests compile without errors
✅ All tests marked `#[ignore]` (require real cluster)
✅ Zero clippy warnings in new code
✅ All benchmarks complete within time budget
✅ Throughput: 5 nodes ≥ 2x single-node (with 40% coordination overhead acceptable)
✅ Latency: p99 <300ms for 5-node
✅ Cache hit ratio: >80% observed
✅ Results exported to CSV for trending
✅ Ready for Block 8 (DR)

---

## Output Specification

Generate a complete, production-ready Rust test file:
- Use existing helper functions from cluster_helpers.rs where applicable
- Create new helpers as specified above
- All tests marked `#[ignore]`
- Follow conventions from other cluster tests
- Use `std::time::Instant` for timing
- Export results to CSV or JSON
- Result<(), String> for error handling
- Comprehensive assertions with thresholds

Expected line count: 600-750 LOC
Expected test count: 10-14

---

## Baseline Comparison (Phase 31)

From Phase 31 single-machine simulation:
- Single-node throughput: ~600 MB/s (simulated)
- P99 latency: ~120ms (simulated)
- Cache hit ratio: ~85% (hot workload)

Phase 32 expectations (real cluster):
- Single-node: 400-800 MB/s (real I/O variance)
- Multi-node 5x: ≥ 2x single-node (accounting for coordination)
- P99 latency: <300ms for 5-node (vs 120ms for single)
