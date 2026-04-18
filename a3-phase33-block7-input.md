# Phase 33 Block 7: Stress Testing & Limits
## OpenCode Implementation Prompt

**Target:** Production readiness under extreme conditions
**Output:** ~500 LOC (source) + 12 tests

## Context
Test behavior under memory pressure, CPU throttling, 10K concurrent clients, and feature index overflow.

## Key Components

### 1. Memory Limiter (150 LOC)
```rust
pub struct MemoryLimiter {
    limit_bytes: u64,
    current_usage: Arc<AtomicU64>,
}
```

### 2. LatencyInjector (150 LOC)
- tc netem wrapper for cluster tests
- Packet loss injection
- Variable latency simulation

### 3. FeatureIndexOverflowHandler (200 LOC)
- Graceful degradation at capacity
- Fallback to faster heuristic
- Recovery mechanism

## Test Categories (12 tests)

1. **Memory limits** (3 tests)
   - test_memory_limit_gc_triggers
   - test_memory_limit_tiering_triggers
   - test_memory_limit_no_crash

2. **CPU throttling** (3 tests)
   - test_cpu_throttle_graceful_slowdown
   - test_cpu_throttle_feature_extraction
   - test_cpu_throttle_no_latency_cliff

3. **Concurrent clients** (3 tests)
   - test_10k_concurrent_clients
   - test_rapid_client_churn
   - test_uneven_client_load

4. **Feature index at capacity** (3 tests)
   - test_feature_index_overflow_graceful
   - test_feature_index_overflow_similarity_fallback
   - test_feature_index_overflow_recovery

## Generate
- `crates/claudefs-reduce/src/memory_limiter.rs`
- `crates/claudefs-reduce/src/latency_injector.rs`
- `crates/claudefs-reduce/src/index_overflow_handler.rs`
- `crates/claudefs-reduce/tests/cluster_stress_limits.rs` — 12 tests

All tests marked #[ignore].
