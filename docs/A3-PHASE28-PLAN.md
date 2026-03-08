# A3: Phase 28 Planning Document

**Status:** 🔴 BLOCKED (Fireworks API key invalid)  
**Date:** 2026-03-08  
**Agent:** A3 (Data Reduction)  
**Current Test Count:** 2020 (Phase 27)  
**Target Test Count:** 2110+ (Phase 28)  

---

## Phase 28 Goal

Add **4 production-ready modules** for multi-tenancy, intelligent tiering, QoS, and delta compression analytics. This phase addresses **Priority 1 feature gaps** identified in docs/agents.md:
- Multi-tenancy & quotas
- QoS / traffic shaping  
- Intelligent tiering
- Key rotation / compliance (Phase 26-27 already covered)

---

## Modules Planned

### 1. multi_tenant_quotas.rs (~22 tests)

**Purpose:** Per-tenant storage quotas for enforcing tenant isolation and cost attribution.

**Key Structures:**
- `TenantId(u64)` — unique tenant identifier
- `QuotaLimit` — soft/hard limits, enforcement flag
- `QuotaUsage` — concurrent tracking via Arc<AtomicU64>
- `MultiTenantQuotas` — main API

**Key Methods:**
- `set_quota(tenant_id, limit) -> Result<()>`
- `check_quota(tenant_id, num_bytes) -> Result<QuotaAction>` — enforce before write
- `record_write(tenant_id, raw_bytes, compressed_bytes, dedup_saved) -> Result<()>`
- `get_utilization_percent(tenant_id) -> f64`
- `get_dedup_ratio(tenant_id) -> f64` — (compressed + saved) / used

**Integration:**
- A2: Metadata service queries tenant IDs
- A8: Exports usage metrics to Prometheus

**Test Coverage:**
- Single tenant creation and update
- Multiple tenant isolation
- Soft/hard limit enforcement
- Dedup ratio calculation (3 scenarios)
- Concurrent write recording
- Prometheus metric export

---

### 2. tiering_advisor.rs (~26 tests)

**Purpose:** ML-inspired tiering recommendations based on access patterns and cost-benefit analysis.

**Key Structures:**
- `AccessMetrics` — segment age, size, access count, compression, dedup ratios
- `TieringRecommendation` — Flash, WarmS3, ColdS3, ArchiveS3 enum
- `TieringScore` — recommendation + score (0.0–1.0) + rationale
- `TieringAdvisor` — scoring engine with cost model
- `TieringAdvisorConfig` — configurable thresholds (days)

**Key Methods:**
- `new(config) -> Self`
- `recommend(metrics: &AccessMetrics) -> TieringScore`
  - Score based on: age, size, compression ratio, access frequency
  - Hot segments (high access) → Flash
  - Aged segments (>30d low access) → Warm S3
  - Very old (>90d) → Cold S3
  - Ancient (>365d) → Archive S3
- `batch_recommendations(Vec<AccessMetrics>) -> Vec<(u64, TieringScore)>`
- `update_cost_model(flash_cost, s3_cost, retrieval_cost) -> Result<()>`
- `get_estimated_savings(metrics: &AccessMetrics) -> (u64, f64)` — (bytes saved, cost benefit)

**Integration:**
- A5: FUSE client requests tiering recommendations
- A8: Visualizes scores on dashboards

**Test Coverage:**
- Hot segment → Flash recommendation
- Aged segment → Warm S3
- Very old → Cold S3
- Ancient → Archive S3
- Large segment prioritized over small
- Highly compressed data stays on flash (retrieval cost)
- Batch recommendations
- Cost model updates affect scores
- Edge cases (threshold boundaries)

---

### 3. dedup_qos.rs (~24 tests)

**Purpose:** QoS for deduplication operations with priority queues, bandwidth shaping, tenant isolation.

**Key Structures:**
- `DedupPriority` enum — Low, Normal, High (with Ord)
- `DedupOpType` enum — FingerprintLookup, SimilaritySearch, DeltaCompression
- `DedupQosRequest` — tenant_id, op_type, priority, size_bytes, optional deadline_ms
- `DedupQosStats` — requests_processed, priority_distribution, latencies, isolation_violations
- `DedupQos` — priority queues (3x Arc<Mutex<VecDeque>>), tenant_bandwidth_limit, stats

**Key Methods:**
- `enqueue_request(req: DedupQosRequest) -> Result<()>` — add to priority queue
- `dequeue_next() -> Option<DedupQosRequest>` — highest-priority ready request
- `set_tenant_bandwidth_limit(tenant_id, bytes_per_sec) -> Result<()>`
- `check_bandwidth_available(tenant_id, bytes) -> bool` — pre-flight check
- `record_bandwidth_used(tenant_id, bytes, latency_ms)` — update stats
- `get_tenant_isolation_score() -> f64` — measure fair scheduling
- `get_stats() -> DedupQosStats` — Prometheus export

**Integration:**
- A4: Transport layer respects bandwidth limits
- A2: Coordinates tenant isolation
- A8: Exports QoS metrics

**Test Coverage:**
- Priority ordering (high > normal > low)
- Bandwidth limit enforcement
- Pre-flight bandwidth checks
- Concurrent enqueue/dequeue
- Per-tenant isolation tracking
- SLA deadline enforcement (optional)
- Multiple tenants with different limits
- Starvation prevention for low-priority
- Statistics export (latency percentiles)
- Request type differentiation

---

### 4. delta_compression_stats.rs (~18 tests)

**Purpose:** Track effectiveness of delta compression: reference block reuse, hotness, cost-benefit.

**Key Structures:**
- `ReferenceBlockStats` — reference_block_id, reuse_count, size, total_delta_bytes, avg_delta_ratio, hotness_score, cpu_cycles
- `DeltaCompressionStats` — blocks_processed, references (Arc<RwLock<HashMap>>), total_cpu_cycles, total_storage_saved
- `GlobalDeltaStats` — aggregated stats (delta_coverage_percent, avg_delta_ratio, storage_saved, cpu_efficiency_ratio, hottest_reference)

**Key Methods:**
- `new() -> Self`
- `record_delta_compression(ref_block_id, delta_size, cpu_cycles) -> Result<()>`
- `get_reference_stats(ref_block_id) -> Option<ReferenceBlockStats>`
- `get_top_n_hot_references(n: usize) -> Vec<ReferenceBlockStats>` — sorted by hotness
- `compute_cost_benefit(ref_block_id) -> (u64, f64)` — (bytes saved, efficiency ratio)
- `get_global_stats() -> GlobalDeltaStats`
- `prune_cold_references(min_reuse_count: u64)` — cleanup unused
- `estimate_compression_effectiveness() -> f64`

**Integration:**
- A8: Exports effectiveness metrics to Prometheus
- A3: Uses hotness scores for prefetch optimization

**Test Coverage:**
- Record single delta compression
- Update reuse count on repeated reference
- Hotness score computation
- Reference stats retrieval
- Top N hot references ranking
- Cost-benefit calculation
- Global stats aggregation
- Pruning cold references
- Compression effectiveness estimation
- Multiple reference patterns
- CPU cycles tracking
- Handle not-found gracefully

---

## Test Summary

| Module | Tests | Category |
|--------|-------|----------|
| multi_tenant_quotas.rs | 22 | Multi-tenancy |
| tiering_advisor.rs | 26 | Intelligent Tiering |
| dedup_qos.rs | 24 | QoS & Traffic Shaping |
| delta_compression_stats.rs | 18 | Analytics & Metrics |
| **Total** | **~90** | |

**Expected Phase 28 result:** 2110+ tests (from 2020 baseline)

---

## Implementation Path

1. **OpenCode Prompt:** `/home/cfs/claudefs/a3-phase28-input.md` — ready, 500+ lines
2. **OpenCode Execution:** Generate all 4 modules in single run
3. **Integration:** Add pub mod declarations to lib.rs
4. **Testing:** `cargo test --lib -p claudefs-reduce` → target 2110+ tests
5. **Code Review:** Verify Rust safety, test coverage, integration points
6. **Commit:** `[A3] Phase 28: Advanced data reduction features`
7. **CHANGELOG:** Update with Phase 28 milestone

---

## Blocker Status

**Status:** 🔴 **Fireworks API Key Invalid** (2026-03-08)

- OpenCode cannot authenticate with Fireworks
- Error: UNAUTHORIZED when attempting to call minimax-m2p5 model
- API key: fw_J246CQF6HnGPVcHzLDhnRy (from cfs/fireworks-api-key secret)
- **Action Required:** A11 to refresh API key in Secrets Manager

**Workaround:** None (CLAUDE.md forbids direct Rust file writing). Must wait for infrastructure fix.

---

## Dependencies Met

✅ **A1 (Storage):** Block allocator, io_uring — Phase 10 complete  
✅ **A2 (Metadata):** Raft, KV store, tenant tracking — Phase 9+ complete  
✅ **A4 (Transport):** RDMA/TCP, bandwidth shaping — Phase 12 in progress  
✅ **A5 (FUSE):** Client caching, metadata resolution — Phase 3+ complete  
⏳ **A8 (Management):** Prometheus export — Phase 3 ongoing  

---

## Integration Checklist

- [ ] A2: Tenant ID coordination with metadata service
- [ ] A8: Prometheus metric export (quotas, tiering scores, QoS stats, delta effectiveness)
- [ ] A5: Request tiering recommendations from tiering_advisor
- [ ] A4: Enforce bandwidth limits from dedup_qos
- [ ] Test cluster: Multi-tenant quota validation tests
- [ ] Documentation: Updated docs/reduction.md with new modules

---

## Success Criteria

✅ All 4 modules compile cleanly (no warnings except documentation)  
✅ ~90 new tests pass (target 80-100)  
✅ Total tests reach 2110+ (from 2020 baseline)  
✅ Integration tests validate cross-module interaction  
✅ Prometheus metrics can be scraped successfully  
✅ Code follows A3 patterns (Arc<RwLock>, Arc<AtomicU64>, error handling)  
✅ Commit and push to main branch  

---

## References

- **VAST Data:** Similarity-based dedup + delta compression, tiering recommendations
- **TiKV:** Multi-tenant quota enforcement in distributed KV
- **Kubernetes:** QoS classes and priority-based scheduling
- **Prometheus:** Standard metrics export patterns

---

## Resume Instructions

When Fireworks API is restored:

```bash
# From /home/cfs/claudefs
export FIREWORKS_API_KEY=$(aws secretsmanager get-secret-value \
  --secret-id cfs/fireworks-api-key --region us-west-2 \
  --query SecretString --output text | jq -r '.FIREWORKS_API_KEY')

# Run OpenCode
~/.opencode/bin/opencode run "$(cat a3-phase28-input.md)" \
  --model fireworks-ai/accounts/fireworks/models/minimax-m2p5 > a3-phase28-output.md

# Extract modules and integrate into crate
# Then: cargo test --lib -p claudefs-reduce
# Expected: 2110+ tests passing
```

---

**Prepared by:** A3 Agent  
**Next action:** Resume when Fireworks API is fixed by A11/Supervisor  
