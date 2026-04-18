# Phase 33 Block 8: Integration & Validation
## OpenCode Implementation Prompt

**Target:** End-to-end regression suite and performance validation
**Output:** ~400 LOC (source) + 8 tests

## Context
Validate Phase 33 features working together and regression testing against Phase 32.

## Test Categories (8 tests)

1. **Feature interaction** (3 tests)
   - test_gc_quota_tiering_interaction
   - test_tracing_quota_accounting_reconciliation
   - test_cache_optimization_with_tier_transitions

2. **Regression suite** (3 tests)
   - test_phase32_exact_dedup_still_works
   - test_phase32_compression_still_works
   - test_phase32_s3_tiering_still_works

3. **Performance baselines** (2 tests)
   - test_phase33_throughput_sla
   - test_phase33_latency_p99_sla

## Generate
- `crates/claudefs-reduce/tests/cluster_integration_phase33.rs` — 8 tests

All tests marked #[ignore].
