# A3: Phase 32 Block 4 — Tiering with Real S3 Consistency
## OpenCode Implementation Prompt (Summary)

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Task:** Implement 12-16 integration tests for real S3 tiering consistency
**Target File:** `crates/claudefs-reduce/tests/cluster_tiering_s3_consistency.rs`
**Target LOC:** 650-800
**Target Tests:** 12-16

---

## High-Level Specs

### Block 4 Purpose
Validate tiering behavior with **real AWS S3 backend** (not mocked), including hot-to-cold transitions, cold reads, bandwidth limits, failure resilience, and data consistency.

### Key Tests (Condensed)
1. `test_cluster_tiering_hot_to_cold_transition` — Hot data aged, moved to S3
2. `test_cluster_tiering_s3_fetch_on_cold_read` — Cold data fetched from S3 on demand
3. `test_cluster_tiering_policy_based_movement` — Tiering policy enforced
4. `test_cluster_tiering_s3_failure_resilience` — S3 down → backpressure, don't fail
5. `test_cluster_tiering_bandwidth_limit_enforcement` — Tiering respects bandwidth cap
6. `test_cluster_tiering_concurrent_hot_cold_access` — Concurrent hot/cold access
7. `test_cluster_tiering_cache_populated_from_s3` — Cold data cached on first fetch
8. `test_cluster_tiering_metadata_consistency_s3` — S3 metadata matches
9. `test_cluster_tiering_partial_s3_restore` — Kill S3, graceful recovery
10. `test_cluster_tiering_s3_cleanup_old_chunks` — Old chunks GC'd
11. `test_cluster_tiering_burst_capacity_handling` — Burst handled (no data loss)
12. `test_cluster_tiering_performance_s3_tier` — S3 latency acceptable (<1s)
13. `test_cluster_tiering_cross_region_s3` — S3 in different region
14. `test_cluster_tiering_s3_encryption_at_rest` — Objects encrypted
15. `test_cluster_tiering_refcount_with_s3_chunks` — Refcount accurate
16. `test_cluster_tiering_quota_accounting_with_s3` — Quotas include S3 data

### Prerequisites
- Block 1 ✅ (cluster health)
- Block 2 ✅ (single-node dedup)

### Helper Functions to Create
- `trigger_tiering_manually() -> Result<(), String>`
- `check_s3_objects(prefix: &str) -> Result<Vec<S3Object>, String>`
- `fetch_cold_data_and_measure_latency(file_path: &str) -> Result<Duration, String>`
- `simulate_s3_unavailability() -> Result<(), String>`
- `restore_s3_access() -> Result<(), String>`
- `get_tiering_metrics() -> Result<TieringMetrics, String>`
- `query_cache_statistics() -> Result<CacheStats, String>`

### Error Handling
- Use `Result<(), String>` for tests
- S3 failures: test recovery, not failure
- Bandwidth limits: verify enforcement
- Cold miss latency: track and assert acceptable

### Assertions
- Verify hot data on storage, cold on S3
- Verify cold reads fetch from S3, populate cache
- Verify metadata consistency (size, checksum, timestamp)
- Verify quota accounting includes S3 data
- Verify no data loss on S3 failures

### Test Execution
- Depends on Block 1 & 2 passing
- Some tests sequential (tiering policy changes)
- Some tests can be parallel (independent cold reads)
- Total runtime target: 10-15 minutes
- May vary based on AWS S3 latency

---

## Full Implementation Details

See `a3-phase32-blocks-3-8-plan.md` section "Block 4: Tiering with Real S3 Consistency (12-16 tests)" for complete specifications including:
- Detailed test implementations for all 16 tests
- Tiering policy (temperature, age, size thresholds)
- S3 consistency requirements (checksums, metadata)
- Failure scenarios (S3 down, network partition)
- Performance baselines (50-100MB/s S3 throughput, <1s cold miss)
- Quota accounting (storage + S3 data)

---

## Success Criteria

✅ All 12-16 tests compile without errors
✅ All tests pass with real AWS S3 backend
✅ Hot-to-cold transition validated
✅ Cold data reads from S3 correctly
✅ S3 failures handled gracefully (backpressure, no data loss)
✅ Bandwidth limits enforced
✅ Data consistency verified (checksums, metadata)
✅ Quota accounting includes S3 data
✅ Performance acceptable (<1s cold miss latency)
✅ Zero clippy warnings

