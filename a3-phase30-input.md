# A3 Phase 30: Integration Testing & Production Hardening — OpenCode Implementation

## Overview

Implement 4 integration test blocks for the `claudefs-reduce` crate to verify end-to-end data reduction pipeline integration with A2 (Metadata), A4 (Transport), A5 (FUSE), and A8 (Management).

**Current state:** 99 modules, 2071 tests passing.

**Target:** 64 new integration tests (18+16+16+14), reaching 2135 total tests.

**Architecture:** Each block tests critical pipeline stages with realistic workloads, measuring correctness, performance, and consistency guarantees.

---

## Integration Test Blocks Summary

### Block 1: Write Path Integration Tests (~18 tests)
**Module:** `crates/claudefs-reduce/tests/integration_write_path.rs`

Tests for full pipeline: chunking → dedup → compression → encryption → EC storage

1. `test_write_path_all_stages_enabled` - Full pipeline with all stages
2. `test_write_path_no_compression` - Dedup+encrypt only (skip compress)
3. `test_write_path_no_dedup` - Chunking+compress+encrypt (skip dedup)
4. `test_distributed_dedup_coordination` - Multi-node fingerprint routing
5. `test_stripe_coordinator_ec_placement` - EC stripe creation & placement
6. `test_quota_enforcement_single_tenant` - Enforce quota for single tenant
7. `test_quota_enforcement_multi_tenant` - Tenant isolation
8. `test_bandwidth_throttle_under_load` - QoS shaping verification
9. `test_write_amplification_tracking` - Amplification ratio consistency
10. `test_segment_packing_completeness` - Segments pack correctly
11. `test_chunk_pipeline_backpressure` - Backpressure when queue full
12. `test_inline_dedup_cache_hits` - Inline dedup with caching
13. `test_compression_advisor_recommends_compression` - Compression heuristics
14. `test_encryption_key_derivation` - Key derivation per chunk
15. `test_write_fence_consistency` - Consistency barrier
16. `test_write_coalescer_merges_small_writes` - Coalescing for small writes
17. `test_chunk_scheduler_priority_fairness` - Scheduling fairness
18. `test_write_buffer_overflow_spill` - Buffer overflow handling

### Block 2: Read Path & Recovery Integration Tests (~16 tests)
**Module:** `crates/claudefs-reduce/tests/integration_read_path.rs`

Tests for read consistency, crash recovery, and durability verification.

1. `test_read_path_full_pipeline` - Full read with decrypt + decompress + reassemble
2. `test_read_with_missing_blocks_ec_reconstruction` - EC reconstruction from stripes
3. `test_read_with_2_missing_blocks_ec_fails_gracefully` - EC limits
4. `test_crash_recovery_incomplete_dedup` - Recovery from partial dedup
5. `test_crash_recovery_incomplete_encryption` - Recovery from partial encryption
6. `test_journal_replay_consistency` - Journal replay produces same state as live
7. `test_refcount_consistency_concurrent_ops` - Refcount under concurrent writes
8. `test_refcount_decrement_on_delete` - Refcount decrement
9. `test_gc_respects_refcount_live_blocks` - GC doesn't collect live blocks
10. `test_gc_coordination_with_refcount` - GC coordinator + refcount table
11. `test_checksum_verification_detects_corruption` - Checksum validation
12. `test_segment_reader_handles_holes` - Segment reader with gaps
13. `test_decompression_format_mismatch` - Decompression with wrong codec
14. `test_read_amplification_tracking` - Read amplification measurement
15. `test_read_cache_hit_rate` - Read cache effectiveness
16. `test_read_planner_prefetch_strategy` - Prefetch coordination

### Block 3: Tier Migration & Lifecycle Integration Tests (~16 tests)
**Module:** `crates/claudefs-reduce/tests/integration_tier_migration.rs`

Tests for S3 tiering, snapshot lifecycle, WORM compliance, and data retention.

1. `test_eviction_policy_high_watermark_triggers` - Watermark-based eviction
2. `test_eviction_policy_low_watermark_stops` - Eviction stops at low watermark
3. `test_s3_blob_assembly_64mb_chunks` - Object assembler creates 64MB blobs
4. `test_s3_blob_retrieval_reassembles` - Retrieve and reassemble S3 blobs
5. `test_snapshot_creation_and_lifecycle` - Snapshot → age → archive
6. `test_snapshot_incremental_diff` - Delta since last snapshot
7. `test_delta_compression_similarity_detection` - Tier 2 delta pipeline
8. `test_delta_index_lookup_efficiency` - Delta index queries
9. `test_similarity_tier_stats_accuracy` - Tier 2 statistics tracking
10. `test_worm_retention_policy_enforcement` - WORM compliance
11. `test_worm_legal_hold_override` - Legal hold supersedes retention
12. `test_key_rotation_without_full_re_encryption` - Envelope key rotation
13. `test_key_rotation_checkpoint_recovery` - Recovery from key rotation checkpoint
14. `test_tier_migration_consistency` - Data integrity through tiering
15. `test_tiering_advisor_recommendations` - Tiering policy decisions
16. `test_adaptive_tiering_respects_latency_slo` - Tiering respects latency

### Block 4: Performance & Consistency Integration Tests (~14 tests)
**Module:** `crates/claudefs-reduce/tests/integration_performance.rs`

Tests for performance metrics, amplification tracking, tenant isolation, and consistency.

1. `test_write_amplification_ratio_tracking` - Amplification consistency
2. `test_read_amplification_from_ec_reconstruction` - Read amplification
3. `test_metrics_export_prometheus_compatible` - Prometheus export format
4. `test_metrics_include_dedup_compression_ec_stats` - Metric completeness
5. `test_tenant_isolation_performance_noisy_neighbor` - QoS isolation
6. `test_similarity_detection_performance_latency_slo` - Tier 2 latency SLO
7. `test_similarity_detection_accuracy_recall` - Tier 2 accuracy
8. `test_pipeline_backpressure_under_memory_pressure` - Memory safety under load
9. `test_pipeline_monitor_alert_thresholds` - Alerting
10. `test_pipeline_monitor_latency_percentiles` - Latency tracking
11. `test_write_coalescer_efficiency` - Coalescing reduces overhead
12. `test_gc_efficiency_collection_rate` - GC efficiency
13. `test_cache_coherency_prevents_stale_reads` - Cache consistency
14. `test_multi_tenant_isolation_consistent_hashing` - Tenant routing

---

## Implementation Strategy

For each test block module, implement:
1. Integration tests that exercise real module interactions (not mocks where possible)
2. Use `#[tokio::test]` for async tests
3. Import modules directly from `claudefs_reduce`
4. Tests should be deterministic and reproduce reliably
5. Add ~10% tolerance to latency assertions (avoid flakiness)

All tests should follow these patterns:
- Setup fixtures/data
- Execute pipeline stages
- Assert correctness of results
- Verify metrics are tracked
- Clean up resources

Target completion: 64 total tests, 2135 tests in claudefs-reduce
