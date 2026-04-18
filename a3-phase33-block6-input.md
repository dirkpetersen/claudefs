# Phase 33 Block 6: Online Tier Management
## OpenCode Implementation Prompt

**Target:** Dynamic hot/warm/cold tier transitions with graceful degradation
**Output:** ~800 LOC (source) + 18 tests

## Context
Implement predictive tiering with graceful S3 fallback when flash is full.

## Key Components

### 1. TierPredictor (350 LOC)
- ML-style access pattern forecasting
- Detect hot vs cold phases
- Predict transition points

### 2. TierMigrator (300 LOC)
- Coordinate live tier transitions
- Zero downtime
- Crash-safe state machine

### 3. TransparentS3Fetch (150 LOC)
- Background S3 fetch on miss
- <500ms p99 latency
- Concurrent fetch handling

## Test Categories (18 tests)

1. **Workload prediction** (4 tests)
   - test_predictor_hot_phase
   - test_predictor_cold_phase
   - test_predictor_transition_point
   - test_predictor_accuracy_vs_latency

2. **Predictive tiering** (4 tests)
   - test_tier_evict_before_pressure
   - test_tier_evict_prevents_stall
   - test_tier_evict_preserves_locality
   - test_tier_keep_hot_on_flash

3. **Graceful S3 fallback** (4 tests)
   - test_transparent_s3_fetch_on_miss
   - test_transparent_s3_fetch_latency
   - test_transparent_s3_fetch_concurrent
   - test_transparent_s3_fetch_consistency

4. **Live tier transitions** (3 tests)
   - test_live_tier_transition_no_downtime
   - test_live_tier_transition_crash_safe
   - test_live_tier_transition_data_integrity

5. **Degradation handling** (3 tests)
   - test_degrade_flash_full_s3_writable
   - test_degrade_s3_latency_high
   - test_degrade_recovery_auto

## Generate
- `crates/claudefs-reduce/src/tier_predictor.rs`
- `crates/claudefs-reduce/src/tier_migrator.rs`
- `crates/claudefs-reduce/src/transparent_s3_fetch.rs`
- `crates/claudefs-reduce/tests/cluster_tier_management.rs` — 18 tests

All tests marked #[ignore].
