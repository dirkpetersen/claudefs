# Phase 33 Plan: Production Enhancements & Feature Gaps
## A3: Data Reduction

**Agent:** A3 | **Target:** 122 integration tests, 5,000+ LOC | **Status:** 📋 PLANNING
**Date Started:** 2026-04-18
**Previous Phase:** Phase 32 ✅ (8 blocks, 135 tests, 2,419 total)

---

## Executive Summary

Phase 33 extends ClaudeFS data reduction with production-grade features identified as critical gaps vs VAST Data / Weka (docs/agents.md Priority 1–2). The phase focuses on:

1. **Dynamic GC & Memory Management** — workload-aware garbage collection
2. **Quota & Multi-Tenancy** — per-tenant storage fairness
3. **Observability Integration** — distributed tracing + enhanced metrics
4. **Performance Optimization** — tile caching, batch processing
5. **Online Scaling** — hot tier management, graceful degradation
6. **Chaos & Stress Testing** — production readiness validation
7. **Feature Interaction** — end-to-end regression suite

### Why Phase 33?

Phase 32 validated the data reduction pipeline on a real cluster (135 integration tests). Phase 33 addresses:

| Gap | Impact | Priority |
|-----|--------|----------|
| **No dynamic GC tuning** | Memory pressure on small nodes; GC stalls on large writes | P1 |
| **No quota enforcement** | Multi-tenant fairness impossible; one tenant can fill cluster | P1 |
| **No distributed tracing** | Black-box performance debugging; latency attribution impossible | P1 |
| **Feature extraction bottleneck** | Similarity pipeline saturates CPU; feature lookup stalls at scale | P1 |
| **No online tier management** | Manual tiering, no graceful degradation under flash pressure | P1 |
| **No stress/chaos validation** | Unknown behavior under extreme load, node failures, network partitions | P2 |

### Dependencies (All Available ✅)

- **A2 Phase 11:** Quotas, multi-tenancy metadata available
- **A4 Phase 13:** Distributed tracing hooks (1,363+ tests) ✅
- **A8 Phase 11:** Prometheus/OpenTelemetry integration available
- **A11 Phase 5 Blocks 1-3:** CI/CD infrastructure hardened ✅
- **Phase 32 Blocks 1-8:** All cluster test harness ready ✅

---

## Phase 33 Structure: 8 Blocks

### Block 1: Dynamic GC Tuning (20 tests)

**Objective:** Garbage collection that adapts to memory pressure and workload.

**Scope:**
- Adaptive thresholds based on system memory pressure (RSS monitoring)
- Workload-aware collection frequency (batch writes vs streaming vs idle)
- Backpressure handling (slow GC doesn't block I/O path)
- Reference counting consistency checks
- Mark-and-sweep verification (periodic, audits reference counts)

**Key Components:**
- `GcController` — monitors memory, adjusts thresholds dynamically
- `ReferenceCountValidator` — background mark-and-sweep audit
- Integration with existing `BlockRefCount` trait

**Test Categories (20 tests):**
1. **Memory pressure adaptation** (4 tests)
   - test_gc_threshold_low_memory — trigger GC at 60% usage
   - test_gc_threshold_high_memory — defer GC at 20% usage
   - test_gc_backpressure_under_load — GC doesn't block writes
   - test_gc_recovery_after_pressure — resume normal collection

2. **Workload-aware frequency** (4 tests)
   - test_gc_batch_writes_high_frequency — large batch = frequent GC
   - test_gc_streaming_low_frequency — small streams = deferred GC
   - test_gc_idle_background_sweep — idle periods = full mark-and-sweep
   - test_gc_mixed_workload_adaptation — dynamic switching

3. **Reference count consistency** (6 tests)
   - test_refcount_increment_decrement_balance — refcounts stay consistent
   - test_refcount_snapshot_safe — GC during snapshots doesn't corrupt
   - test_refcount_dedup_block_sharing — exact dedup increments correctly
   - test_refcount_similarity_delta_update — similarity delta refcounts
   - test_refcount_orphaned_block_detection — orphaned blocks marked for GC
   - test_refcount_multi_snapshot_complex — overlapping snapshots don't leak

4. **Mark-and-sweep audit** (6 tests)
   - test_mark_sweep_finds_all_reachable — all live blocks marked
   - test_mark_sweep_detects_orphans — orphaned blocks not marked
   - test_mark_sweep_corrects_overcounts — audit fixes overcounts
   - test_mark_sweep_concurrent_safe — doesn't interfere with writers
   - test_mark_sweep_large_index_performance — 100M+ blocks processed in <1s
   - test_mark_sweep_recovery_after_crash — crash-safe state reconstruction

**OpenCode Input:** ~350 lines (detailed specs, APIs, test scenarios)

---

### Block 2: Quota Enforcement (18 tests)

**Objective:** Per-tenant storage limits with fairness queuing.

**Scope:**
- Soft quotas (warning at 90%, recommend cleanup)
- Hard quotas (write rejection at 100%)
- Fairness queuing (prevent one tenant from starving others)
- Quota accounting (accurate, crash-safe)
- Multi-tenant dedup accounting (shared deduplicated blocks)

**Key Components:**
- `QuotaManager` — tracks per-tenant usage
- `FairnessQueue` — prioritizes writes when shared resources constrained
- `QuotaAccountant` — credits/debits for dedup, compression, delta savings
- Integration with metadata write path

**Test Categories (18 tests):**
1. **Quota enforcement** (4 tests)
   - test_soft_quota_warning — warn at 90%
   - test_hard_quota_rejection — reject at 100%
   - test_quota_grace_period — allow cleanup period after hard quota
   - test_quota_override_admin — admin can force write above quota

2. **Fairness queuing** (4 tests)
   - test_fairness_no_starvation — all tenants make progress
   - test_fairness_weighted_priority — prioritize by quota % consumed
   - test_fairness_queue_timeout — prevent indefinite waits
   - test_fairness_batch_clustering — group similar-sized writes

3. **Dedup accounting** (4 tests)
   - test_quota_exact_dedup_credit — exact match gives full credit back
   - test_quota_similarity_dedup_credit — similarity match gives delta credit
   - test_quota_cross_tenant_dedup — shared blocks don't double-count
   - test_quota_snapshot_accounting — snapshots don't charge for shared blocks

4. **Quota consistency** (6 tests)
   - test_quota_crash_recovery — quotas consistent after crash
   - test_quota_concurrent_updates — no race conditions in accounting
   - test_quota_compression_savings — quota adjusted for compression
   - test_quota_tiering_to_s3 — eviction reduces local quota pressure
   - test_quota_complex_topology — multiple dedup layers accounted correctly
   - test_quota_audit_trail — historical usage queryable

**OpenCode Input:** ~320 lines (quota data structures, fairness algorithms, test harness)

---

### Block 3: Distributed Tracing Integration (16 tests)

**Objective:** OpenTelemetry/Jaeger integration for latency attribution.

**Scope:**
- Span propagation across async boundaries (io_uring, tokio::spawn)
- Latency attribution per stage (dedupe → compress → encrypt → write)
- Histogram metrics for each stage
- Trace export to Jaeger / Prometheus
- Integration with A4 transport layer traces

**Key Components:**
- `TracingContext` — wrapper around otel::Context
- `SpanGuard` — RAII span management
- Per-stage histogram metrics
- Integration with existing `tracing` macros

**Test Categories (16 tests):**
1. **Span propagation** (4 tests)
   - test_span_propagation_inline_dedup — span follows dedupe pipeline
   - test_span_propagation_async_gc — background GC spans tracked
   - test_span_propagation_cross_node — spans cross SSH/RPC boundaries
   - test_span_propagation_nested_spans — parent-child relationships correct

2. **Latency attribution** (4 tests)
   - test_latency_dedupe_stage — dedupe latency isolated
   - test_latency_compress_stage — compression latency isolated
   - test_latency_encrypt_stage — encryption latency isolated
   - test_latency_write_stage — write latency isolated

3. **Metrics export** (4 tests)
   - test_histogram_metrics_dedupe — dedupe histogram exported to Prometheus
   - test_histogram_metrics_all_stages — all stages exported
   - test_trace_export_jaeger — traces sent to Jaeger endpoint
   - test_otel_context_sampling — sampling rate respected

4. **Integration testing** (4 tests)
   - test_trace_cluster_write_path — full cluster trace with multiple nodes
   - test_trace_similarity_lookup — feature extraction + delta compression traced
   - test_trace_s3_tiering — async tiering spans exported
   - test_trace_performance_correlation — high latency correlates with known bottleneck

**OpenCode Input:** ~280 lines (span creation, metrics definitions, Jaeger export config)

---

### Block 4: Feature Extraction Optimization (16 tests)

**Objective:** Tile caching and batch processing for similarity detection.

**Scope:**
- Tile cache for frequently accessed chunks (L3-resident features)
- Batch feature extraction (vectorized, SIMD-friendly)
- Bloom filter pre-filtering (reduce false positives before index lookup)
- Feature extraction latency <100μs per chunk at scale

**Key Components:**
- `FeatureTileCache` — LRU cache of Super-Features for hot chunks
- `BatchFeatureExtractor` — process 128-chunk batches
- `BloomFilterIndex` — pre-filter for feature lookups
- Vectorized Rabin fingerprint computation

**Test Categories (16 tests):**
1. **Tile caching** (4 tests)
   - test_tile_cache_hit_rate — >90% hit rate on repeated access
   - test_tile_cache_eviction_policy — LRU eviction correct
   - test_tile_cache_memory_bounded — cache respects size limit
   - test_tile_cache_invalidation — cache invalidated on block change

2. **Batch processing** (4 tests)
   - test_batch_feature_extraction_correctness — same output as serial
   - test_batch_feature_extraction_speedup — 2-4x speedup with batching
   - test_batch_feature_extraction_simd — SIMD instructions used (perf counter verification)
   - test_batch_feature_extraction_heterogeneous — handles variable-length chunks

3. **Bloom filter optimization** (4 tests)
   - test_bloom_filter_false_positive_rate — <1% false positive rate
   - test_bloom_filter_reduces_lookups — 50%+ lookup reduction
   - test_bloom_filter_update_correctness — dynamic updates correct
   - test_bloom_filter_concurrent_reads — lock-free reads

4. **Performance validation** (4 tests)
   - test_feature_extraction_latency_p50 — <50μs p50 latency
   - test_feature_extraction_latency_p99 — <200μs p99 latency
   - test_similarity_index_lookup_l3_cache — lookups hit L3 cache
   - test_feature_extraction_throughput_scale — >10M features/sec at scale

**OpenCode Input:** ~300 lines (tile cache impl, batch vectorization, bloom filter)

---

### Block 5: Similarity Search Scaling (14 tests)

**Objective:** Approximate nearest-neighbor matching for ultra-scale clusters.

**Scope:**
- LSH (Locality-Sensitive Hashing) for approximate matching
- Index sharding across metadata servers
- Hierarchical approximate search (coarse → fine)
- Candidate list merging from multiple shards
- Recall tuning (95%+ recall vs latency tradeoff)

**Key Components:**
- `LSHIndex` — distributed LSH hash tables
- `ApproximateMatcher` — coarse+fine search
- `CandidateAggregator` — merge candidates from shards
- `RecallTuner` — adjust LSH parameters for target recall/latency

**Test Categories (14 tests):**
1. **LSH correctness** (3 tests)
   - test_lsh_exact_matches_always_found — all exact duplicates found
   - test_lsh_similar_within_threshold — similar blocks found consistently
   - test_lsh_dissimilar_rejected — dissimilar blocks not matched

2. **Distributed search** (3 tests)
   - test_lsh_query_single_shard — single shard search correct
   - test_lsh_query_multi_shard — results aggregated correctly
   - test_lsh_query_shard_failure — query completes with partial results

3. **Hierarchical search** (3 tests)
   - test_hierarchical_coarse_filtering — coarse filter reduces candidates 10x
   - test_hierarchical_fine_ranking — fine ranking recalls target matches
   - test_hierarchical_recall_tuning — recall parameter adjustable

4. **Scalability** (2 tests)
   - test_search_latency_1pb_index — search <10ms in 1PB index
   - test_search_latency_scale_horizontal — latency doesn't increase with nodes

5. **Recall validation** (3 tests)
   - test_recall_95_percent — achieves 95%+ recall at <5ms latency
   - test_recall_similarity_threshold — recall vs threshold tradeoff documented
   - test_recall_index_rebuild — recall consistent after index rebuild

**OpenCode Input:** ~280 lines (LSH implementation, hierarchical search, candidate aggregation)

---

### Block 6: Online Tier Management (18 tests)

**Objective:** Dynamic hot/warm/cold tier transitions with graceful degradation.

**Scope:**
- Workload prediction (access pattern forecasting)
- Predictive tiering (move to cold before cache pressure)
- Graceful degradation (return data transparently from S3 if flash full)
- Zero data loss during tier transitions
- Live tier transitions (no downtime)

**Key Components:**
- `TierPredictor` — ML-style access pattern forecasting
- `TierMigrator` — coordinates tier transitions
- `TransparentS3Fetch` — background S3 fetch on miss
- Integration with D5 tiering policy

**Test Categories (18 tests):**
1. **Workload prediction** (4 tests)
   - test_predictor_hot_phase — detects active access patterns
   - test_predictor_cold_phase — detects aging patterns
   - test_predictor_transition_point — predicts phase change timing
   - test_predictor_accuracy_vs_latency — prediction accuracy >90% at <10ms

2. **Predictive tiering** (4 tests)
   - test_tier_evict_before_pressure — evict 1 min before cache full
   - test_tier_evict_prevents_stall — no write stalls from tier transitions
   - test_tier_evict_preserves_locality — L1/L2 cache optimizations maintained
   - test_tier_keep_hot_on_flash — detected-hot blocks retained

3. **Graceful S3 fallback** (4 tests)
   - test_transparent_s3_fetch_on_miss — miss triggers background fetch
   - test_transparent_s3_fetch_latency — fetch <500ms p99
   - test_transparent_s3_fetch_concurrent — parallel fetches don't block writers
   - test_transparent_s3_fetch_consistency — reads always consistent

4. **Live tier transitions** (3 tests)
   - test_live_tier_transition_no_downtime — transition doesn't stop I/O
   - test_live_tier_transition_crash_safe — crash during transition recovers cleanly
   - test_live_tier_transition_data_integrity — no data loss

5. **Degradation handling** (3 tests)
   - test_degrade_flash_full_s3_writable — writes go to S3, reads from both
   - test_degrade_s3_latency_high — use local flash for reads despite S3 slowness
   - test_degrade_recovery_auto — auto-recovery when pressure drops

**OpenCode Input:** ~350 lines (predictor model, tier migration state machine, fallback paths)

---

### Block 7: Stress Testing & Limits (12 tests)

**Objective:** Production readiness under extreme conditions.

**Scope:**
- Memory limit enforcement (no OOM panics)
- CPU throttling behavior (graceful degradation, no latency cliff)
- Concurrent client limits (10K+ clients, one connection per client)
- Feature index at 100% capacity (edge case handling)
- Network packet loss / latency injection

**Key Components:**
- Memory limiter (cgroup integration for local testing)
- Latency injector (tc netem wrapper in cluster tests)
- Feature index overflow handler
- Load generator (synthetic workload)

**Test Categories (12 tests):**
1. **Memory limits** (3 tests)
   - test_memory_limit_gc_triggers — GC forced at hard limit
   - test_memory_limit_tiering_triggers — S3 tiering on OOM pressure
   - test_memory_limit_no_crash — no panics at 100% memory pressure

2. **CPU throttling** (3 tests)
   - test_cpu_throttle_graceful_slowdown — throughput decreases linearly
   - test_cpu_throttle_feature_extraction — feature extraction rate drops proportionally
   - test_cpu_throttle_no_latency_cliff — no sudden drops

3. **Concurrent clients** (3 tests)
   - test_10k_concurrent_clients — sustain 10K connections
   - test_rapid_client_churn — client connect/disconnect at high rate
   - test_uneven_client_load — fair distribution even with skewed workload

4. **Feature index at capacity** (3 tests)
   - test_feature_index_overflow_graceful — overflow doesn't crash
   - test_feature_index_overflow_similarity_fallback — similarity matching reverts to faster heuristic
   - test_feature_index_overflow_recovery — index can rebuild to capacity

**OpenCode Input:** ~250 lines (stress harness, load generators, limit enforcement)

---

### Block 8: Integration & Validation (8 tests)

**Objective:** End-to-end regression suite validating feature interactions.

**Scope:**
- Phase 33 features working together (GC + quotas + tiering + tracing)
- Regression suite (Phase 32 scenarios still pass with Phase 33 changes)
- Performance baselines (throughput/latency SLAs maintained)
- Scale validation (100M+ blocks, 1PB+ data)

**Key Components:**
- Composite test suite (Blocks 1-7 scenarios combined)
- Benchmark comparison (Phase 33 vs Phase 32)
- Cluster-scale validation (multi-node, multi-client)

**Test Categories (8 tests):**
1. **Feature interaction** (3 tests)
   - test_gc_quota_tiering_interaction — GC respects quotas, tiering respects GC
   - test_tracing_quota_accounting_reconciliation — traces match quota changes
   - test_cache_optimization_with_tier_transitions — tile cache coherent after tiering

2. **Regression suite** (3 tests)
   - test_phase32_exact_dedup_still_works — all exact-match dedupe scenarios pass
   - test_phase32_compression_still_works — all compression scenarios pass
   - test_phase32_s3_tiering_still_works — all S3 tiering scenarios pass

3. **Performance baselines** (2 tests)
   - test_write_throughput_phase33_vs_phase32 — throughput >90% of Phase 32
   - test_read_latency_phase33_vs_phase32 — latency <110% of Phase 32

**OpenCode Input:** ~200 lines (composite scenarios, benchmark harness, comparison metrics)

---

## Timeline & Dependencies

### Sequential Execution (Blocks can run in sequence)

```
Block 1 (GC)          → Block 2 (Quota)
                      ↓
                    Block 3 (Tracing)
                    ↓
                    Block 4 (Features)
                    ↓
                    Block 5 (LSH)
                    ↓
                    Block 6 (Tier Mgmt)
                    ↓
                    Block 7 (Stress)
                    ↓
                    Block 8 (Validation)
```

**Rationale:**
- Blocks 1-2 are foundational (GC, quotas)
- Blocks 3-5 are independent (tracing, feature optimization, search)
- Block 6 builds on Blocks 1-2 (tiering respects GC/quotas)
- Block 7 validates all together
- Block 8 final regression suite

### Estimated Timeline

| Block | LOC | OpenCode Time | Test Time | Total |
|-------|-----|---------------|-----------|-------|
| 1 (GC) | 600 | 45 min | 30 min | ~1.25h |
| 2 (Quota) | 550 | 40 min | 25 min | ~1h |
| 3 (Tracing) | 500 | 35 min | 20 min | ~55 min |
| 4 (Features) | 700 | 50 min | 35 min | ~1.5h |
| 5 (Search) | 650 | 45 min | 30 min | ~1.25h |
| 6 (Tier Mgmt) | 800 | 60 min | 40 min | ~1.75h |
| 7 (Stress) | 500 | 40 min | 35 min | ~1.25h |
| 8 (Validation) | 400 | 30 min | 25 min | ~55 min |
| **Total** | **5,200** | **~345 min (5.75h)** | **~240 min (4h)** | **~10h** |

**Wall-clock estimate:** ~12-14 hours with OpenCode + testing + validation cycles (allows for iteration/debugging).

---

## Success Criteria

### Phase 33 Complete When:

✅ **All 8 blocks deployed and passing:**
- Block 1: 20 tests, GC adaptive to memory/workload
- Block 2: 18 tests, quotas enforced, fairness queuing
- Block 3: 16 tests, distributed tracing integrated
- Block 4: 16 tests, feature extraction <100μs/chunk
- Block 5: 14 tests, LSH search <10ms at 1PB scale
- Block 6: 18 tests, online tiering with graceful degradation
- Block 7: 12 tests, stress limits enforced safely
- Block 8: 8 tests, all features working together + regression suite

✅ **Test totals:**
- Phase 32 (baseline): 2,419 tests
- Phase 33 (new): 122 tests
- **Phase 33 Total: 2,541 tests** ✅

✅ **Performance criteria:**
- GC latency <50ms p99 (no write blocking)
- Quota accounting accuracy >99.9%
- Trace export latency <5ms p99
- Feature extraction >10M features/sec
- LSH search <10ms p99 at 1PB scale
- Tier transition downtime 0 seconds
- Stress test: no OOM panics at 100% memory pressure

✅ **Code quality:**
- Zero unsafe code in A3 (all in io_uring/FUSE/transport FFI)
- All tests marked `#[ignore]` (cluster-only)
- Full integration test coverage of new features
- Comprehensive error scenarios tested

### Blockers Resolved:

- ✅ A2 Phase 11: Multi-tenancy metadata (quotas)
- ✅ A4 Phase 13: Tracing infrastructure (1,363+ tests)
- ✅ A8 Phase 11: Prometheus/OTEL integration
- ✅ A11 Phase 5 Blocks 1-3: CI/CD hardening

---

## Known Unknowns & Risks

### Risks

1. **LSH index at 100%+ capacity** — adaptive fallback needed
   - *Mitigation:* Test index overflow scenarios (Block 7 + 8)

2. **Graceful degradation to S3** — latency cliff if network saturated
   - *Mitigation:* Introduce artificial latency in Block 6 tests

3. **Multi-tenant dedup accounting** — ensuring no double-counting at scale
   - *Mitigation:* Exhaustive Block 2 tests with cross-tenant dedup

4. **Trace performance** — OpenTelemetry overhead on hot path
   - *Mitigation:* Validate <2% overhead in Block 3 benchmarks

### Learning Opportunities

- Feature extraction vectorization (SIMD) will reveal CPU bottlenecks
- Quota fairness queuing under adversarial load (one tenant aggressive, others quiet)
- LSH recall accuracy vs latency tradeoff (empirical vs theoretical)
- Memory predictor accuracy under real cluster workloads

---

## Phase 33 Artifacts

### Committed to Git

```
crates/claudefs-reduce/src/
├── gc_controller.rs          # Dynamic GC tuning
├── quota_manager.rs          # Per-tenant quotas
├── otel_tracing.rs           # OpenTelemetry integration
├── feature_cache.rs          # Tile caching + batch extraction
├── lsh_index.rs              # LSH approximate search
├── tier_predictor.rs         # Workload forecasting
├── tier_migrator.rs          # Live tier transitions
└── stress_testing.rs         # Limit enforcement

crates/claudefs-reduce/tests/
├── cluster_gc_dynamic.rs              # Block 1 (20 tests)
├── cluster_quota_fairness.rs          # Block 2 (18 tests)
├── cluster_tracing_latency.rs         # Block 3 (16 tests)
├── cluster_feature_optimization.rs    # Block 4 (16 tests)
├── cluster_similarity_scaling.rs      # Block 5 (14 tests)
├── cluster_tier_management.rs         # Block 6 (18 tests)
├── cluster_stress_limits.rs           # Block 7 (12 tests)
└── cluster_integration_validation.rs  # Block 8 (8 tests)

Documentation:
├── a3-phase33-plan.md                 # This file
├── a3-phase33-block1-output.md        # Block 1 OpenCode output
├── a3-phase33-block2-output.md        # Block 2 OpenCode output
├── ... (6 more per block)
└── a3-phase33-completion-summary.md   # Phase 33 summary
```

### CHANGELOG Entry

```markdown
## 2026-04-18 → 2026-04-19 (Phase 33 execution)

### A3: Data Reduction — Production Enhancements

**Phase 33 Completion: 122 new integration tests, 5,200+ LOC**
- Block 1: Dynamic GC tuning (20 tests) — memory pressure adaptation, workload-aware frequencies
- Block 2: Quota enforcement (18 tests) — per-tenant limits, fairness queuing, dedup accounting
- Block 3: Distributed tracing (16 tests) — OTEL/Jaeger integration, latency attribution
- Block 4: Feature optimization (16 tests) — tile caching, batch processing, <100μs/chunk
- Block 5: LSH scaling (14 tests) — approximate NN search, <10ms at 1PB scale
- Block 6: Online tier management (18 tests) — predictive tiering, graceful S3 fallback
- Block 7: Stress testing (12 tests) — memory/CPU limits, 10K clients, edge cases
- Block 8: Integration & validation (8 tests) — regression suite, performance baselines

**Total Tests:** 2,419 (Phase 32) + 122 (Phase 33) = **2,541** ✅
**Production Readiness:** GC adaptive, quotas enforced, zero OOM panics, latency SLAs met
```

---

## Next Phase (Phase 34): Post-Production Monitoring

Future work beyond Phase 33:
- Online defragmentation (active wear-leveling)
- Advanced ML-based predictor tuning
- Multi-site dedup coordination
- Adaptive compression algorithm selection
- Cost optimizer (dynamic pricing model for cloud tiering)

---

## References

- **docs/reduction.md** — Data reduction architecture
- **docs/decisions.md** — D5 (S3 tiering), D10 (embedded KV)
- **docs/agents.md** — A3 scope, dependencies, phase timeline
- **Phase 32 blocks** — Cluster test harness (cluster_helpers.rs, cluster_*.rs)
- **A2 Phase 11** — Quota metadata structures
- **A4 Phase 13** — Distributed tracing hooks
- **A8 Phase 11** — Prometheus/OTEL exporters

---

**Plan Status:** ✅ Ready for approval & implementation
**Next Step:** OpenCode generation of Blocks 1-8 input prompts
