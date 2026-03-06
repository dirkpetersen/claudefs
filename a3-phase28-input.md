# A3 Phase 28: Advanced Data Reduction Features — Production Readiness

**Phase Goal:** Add 4 new modules supporting multi-tenancy, intelligent tiering, QoS, and delta compression analytics. Target: 80-100 new tests, reaching 2100+ total for Phase 28 completion.

**Current State:** 97 modules, 2020 tests passing. Phase 27 disabled. Now adding Priority 1 production-ready features.

---

## Integration Context

- **A2 Metadata Service:** A3 depends on A2 for tenant tracking, quota enforcement coordination
- **A5 FUSE Client:** A3 provides reduction stats to A5 for client-side tiering hints
- **A8 Management:** A3 exports Prometheus metrics via metrics module; A8 consumes them for dashboards
- **A4 Transport:** A3 QoS module coordinates bandwidth shaping with A4's network layer

---

## Module 1: multi_tenant_quotas.rs (~22 tests)

**Purpose:** Per-tenant storage quotas for enforcing tenant isolation and cost attribution.

### Structures

```rust
pub struct TenantId(pub u64);

pub struct QuotaLimit {
    pub soft_limit_bytes: u64,
    pub hard_limit_bytes: u64,
    pub enforce_on_write: bool,  // fail writes at hard limit
}

pub struct QuotaUsage {
    pub tenant_id: TenantId,
    pub used_bytes: Arc<AtomicU64>,
    pub compressed_bytes: Arc<AtomicU64>,
    pub dedup_saved_bytes: Arc<AtomicU64>,
    pub last_update_ms: u64,
}

pub struct MultiTenantQuotas {
    quotas: Arc<RwLock<HashMap<TenantId, QuotaLimit>>>,
    usage: Arc<RwLock<HashMap<TenantId, QuotaUsage>>>,
}
```

### Key Methods

- `set_quota(tenant_id, limit) -> Result<()>` — configure tenant quota
- `check_quota(tenant_id, num_bytes) -> Result<QuotaAction>` — enforce before write
- `record_write(tenant_id, raw_bytes, compressed_bytes, dedup_saved) -> Result<()>` — update usage post-write
- `get_usage(tenant_id) -> QuotaUsage` — read-only access to current usage
- `get_utilization_percent(tenant_id) -> f64` — percentage of soft limit used
- `prune_inactive_tenants(cutoff_ms)` — cleanup old tenants
- `get_dedup_ratio(tenant_id) -> f64` — (compressed + saved) / used

### Test Requirements (~22 tests)

- Create quota for single tenant
- Update quota limits
- Record writes and track usage
- Check quota enforcement (soft/hard limits)
- Multiple tenant isolation
- Quota enforcement returns error at hard limit
- Dedup ratio calculation (3 scenarios)
- Inactive tenant pruning
- Concurrent write recording via Arc<AtomicU64>
- Export to QuotaMetrics struct for Prometheus

---

## Module 2: tiering_advisor.rs (~26 tests)

**Purpose:** Machine learning-inspired tiering recommendations based on access patterns and cost-benefit analysis.

### Structures

```rust
#[derive(Debug, Clone)]
pub struct AccessMetrics {
    pub segment_id: u64,
    pub size_bytes: u64,
    pub last_access_age_sec: u64,
    pub access_count: u64,
    pub compression_ratio: f64,
    pub dedup_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TieringRecommendation {
    Flash,       // keep in NVMe
    WarmS3,      // S3 Standard
    ColdS3,      // S3 IA
    ArchiveS3,   // S3 Glacier
}

pub struct TieringScore {
    pub recommendation: TieringRecommendation,
    pub score: f64,  // 0.0 to 1.0
    pub rationale: String,
}

pub struct TieringAdvisor {
    flash_to_s3_cost_ratio: f64,  // S3 storage / NVMe storage annual
    access_cost_per_mb: f64,      // S3 retrieval cost
    threshold_access_age_days: u64,
    config: TieringAdvisorConfig,
}

pub struct TieringAdvisorConfig {
    pub flash_threshold_days: u64,    // if last_access > this, consider S3
    pub cold_threshold_days: u64,     // if last_access > this, consider Cold tier
    pub archive_threshold_days: u64,  // if last_access > this, consider Archive
}
```

### Key Methods

- `new(config: TieringAdvisorConfig) -> Self`
- `recommend(metrics: &AccessMetrics) -> TieringScore` — ML-inspired scoring based on:
  - Age: older segments score lower (S3 candidates)
  - Size: larger segments have bigger savings
  - Compression: poorly compressed data costs more to retrieve from S3
  - Access frequency: high-access segments stay on flash
  - Hot/cold classification from access count
- `batch_recommendations(metrics_batch: Vec<AccessMetrics>) -> Vec<(u64, TieringScore)>` — score multiple segments
- `update_cost_model(flash_cost, s3_cost, retrieval_cost) -> Result<()>` — adjust parameters
- `get_estimated_savings(metrics: &AccessMetrics) -> (u64, f64)` — (bytes saved, cost benefit)

### Test Requirements (~26 tests)

- Recommend Flash for hot segment (high access count, young)
- Recommend WarmS3 for aged segment (>30 days, low access)
- Recommend ColdS3 for very old segment (>90 days)
- Recommend ArchiveS3 for ancient segment (>365 days)
- Large segment prioritized over small (even if similar age)
- Highly compressed data stays on flash (retrieval cost high)
- Batch recommendations
- Cost model updates affect recommendations
- Estimated savings calculation (multiple scenarios)
- Tier promotion/demotion logic consistency
- Config parameter edge cases (thresholds)

---

## Module 3: dedup_qos.rs (~24 tests)

**Purpose:** QoS for deduplication operations: priority queues, bandwidth shaping, tenant isolation.

### Structures

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DedupPriority {
    Low = 0,
    Normal = 1,
    High = 2,
}

pub struct DedupQosRequest {
    pub tenant_id: u64,
    pub op_type: DedupOpType,      // FingerprintLookup, SimilaritySearch, etc.
    pub priority: DedupPriority,
    pub size_bytes: u64,
    pub deadline_ms: Option<u64>,  // SLA deadline
}

pub enum DedupOpType {
    FingerprintLookup,      // exact match dedup
    SimilaritySearch,       // find similar blocks
    DeltaCompression,       // compute delta
}

pub struct DedupQosStats {
    pub requests_processed: u64,
    pub priority_distribution: [u64; 3],  // [Low, Normal, High]
    pub avg_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub tenant_isolation_violations: u64,
}

pub struct DedupQos {
    queues: [Arc<Mutex<VecDeque<DedupQosRequest>>>; 3],  // per priority
    tenant_bandwidth_limit: Arc<RwLock<HashMap<u64, u64>>>,  // tenant_id -> bytes/sec
    stats: Arc<Mutex<DedupQosStats>>,
}
```

### Key Methods

- `enqueue_request(req: DedupQosRequest) -> Result<()>` — add request to priority queue
- `dequeue_next() -> Option<DedupQosRequest>` — get highest-priority ready request
- `set_tenant_bandwidth_limit(tenant_id, bytes_per_sec) -> Result<()>` — enforce per-tenant bandwidth
- `check_bandwidth_available(tenant_id, bytes) -> bool` — pre-flight check
- `record_bandwidth_used(tenant_id, bytes, latency_ms)` — update stats
- `get_tenant_isolation_score() -> f64` — measure fair scheduling across tenants
- `get_stats() -> DedupQosStats` — return statistics for Prometheus

### Test Requirements (~24 tests)

- Enqueue low/normal/high priority requests
- Dequeue respects priority order
- Tenant bandwidth limits enforced
- Bandwidth check prevents exceeding limit
- Bandwidth recording updates stats
- Concurrent enqueue/dequeue via Arc<Mutex>
- Per-tenant isolation tracking
- SLA deadline enforcement (optional latency target)
- Multiple tenants with different limits
- Priority starvation prevention (low-priority eventually served)
- Stats export (avg/p99 latency, tenant metrics)
- Request type differentiation (lookup vs search vs delta)

---

## Module 4: delta_compression_stats.rs (~18 tests)

**Purpose:** Track effectiveness of delta compression strategy: reference block reuse, hotness, cost-benefit.

### Structures

```rust
#[derive(Debug, Clone)]
pub struct ReferenceBlockStats {
    pub reference_block_id: u64,
    pub reuse_count: u64,  // how many deltas reference this block
    pub reference_size: u64,
    pub total_delta_bytes: u64,  // sum of all deltas
    pub avg_delta_ratio: f64,    // avg delta_size / reference_size
    pub hotness_score: f64,      // based on reuse rate
    pub cpu_cycles_for_deltas: u64,  // profiling data
}

pub struct DeltaCompressionStats {
    pub total_blocks_processed: u64,
    pub blocks_with_reference: u64,
    pub references: Arc<RwLock<HashMap<u64, ReferenceBlockStats>>>,
    pub total_cpu_cycles: Arc<AtomicU64>,
    pub total_storage_saved: Arc<AtomicU64>,
}
```

### Key Methods

- `new() -> Self`
- `record_delta_compression(ref_block_id, delta_size, cpu_cycles) -> Result<()>` — update stats post-delta
- `get_reference_stats(ref_block_id) -> Option<ReferenceBlockStats>` — read stats for a reference
- `get_top_n_hot_references(n: usize) -> Vec<ReferenceBlockStats>` — by reuse count / hotness
- `compute_cost_benefit(ref_block_id) -> (u64, f64)` — (storage saved bytes, cpu efficiency ratio)
- `get_global_stats() -> GlobalDeltaStats` — aggregated stats
- `prune_cold_references(min_reuse_count: u64)` — cleanup unused references
- `estimate_compression_effectiveness() -> f64` — (total_saved / total_cpu_cost_equivalent)

### Global Stats Struct

```rust
pub struct GlobalDeltaStats {
    pub total_blocks: u64,
    pub blocks_with_delta: u64,
    pub delta_coverage_percent: f64,
    pub avg_delta_ratio: f64,
    pub total_storage_saved_bytes: u64,
    pub cpu_efficiency_ratio: f64,  // storage_saved / cpu_cost
    pub hottest_reference: Option<u64>,
}
```

### Test Requirements (~18 tests)

- Create stats tracker
- Record single delta compression
- Update reuse count on repeated reference
- Compute hotness score (based on reuse rate)
- Get reference stats
- Get top N hot references (sorted by reuse/hotness)
- Cost-benefit calculation
- Global stats aggregation
- Prune cold references (reuse_count < threshold)
- Compression effectiveness estimation
- Multiple references with different reuse patterns
- CPU cycles tracking and efficiency ratio
- Handle reference not found gracefully

---

## Integration Points

1. **multi_tenant_quotas** → A2 metadata queries tenant IDs; A8 exports usage via Prometheus
2. **tiering_advisor** → A5 requests recommendations for tiering policy; A8 visualizes scores
3. **dedup_qos** → A4 transport layer respects bandwidth limits; A2 coordinates tenant isolation
4. **delta_compression_stats** → A8 exports effectiveness metrics; A3 pipeline uses hotness for prefetch optimization

---

## Testing Strategy

- **Property-based tests:** Score distributions, cost-benefit consistency
- **Integration tests:** Multi-module interaction (quotas + QoS + tiering)
- **Concurrency tests:** Arc<Atomic*> and Arc<Mutex> correctness under concurrent access
- **Edge cases:** Overflow prevention, zero-division guards, precision in cost calculations

---

## Shared Conventions (Follow Existing A3 Patterns)

- Error handling: Use `crate::error::ReduceError` for all errors
- Logging: `use tracing::{info, warn, debug, span, Level}`
- Metrics: Export stats as structs with `pub` fields for A8 consumption
- Testing: One `#[cfg(test)] mod tests` block per module with minimum 18-26 tests
- Async: Use `Arc<RwLock<_>>` for shared state, `Arc<AtomicU64>` for counters
- Documentation: Crate-level doc comments for public types and methods

---

## Deliverables

1. **multi_tenant_quotas.rs** — ~22 tests
2. **tiering_advisor.rs** — ~26 tests
3. **dedup_qos.rs** — ~24 tests
4. **delta_compression_stats.rs** — ~18 tests

Total: ~90 tests expected. Bring total to **2110+ tests**.

All code should follow the Rust safety rules: no `unsafe` blocks except where necessary for FFI (none here). All modules are safe Rust.

---

## Build and Test

After generation, the code will be integrated into the A3 crate. Test with:

```bash
cargo test --lib -p claudefs-reduce 2>&1 | tail -5
```

Expected result: `test result: ok. 2110+ passed; 0 failed`

---

## References

- **VAST Data whitepaper:** Similarity-based dedup + delta compression for scale-out storage
- **TiKV sharding:** Multi-tenant quota enforcement in distributed KV
- **Kubernetes QoS:** Priority-based resource scheduling (adapted for dedup operations)
- **Prometheus:** Standard metrics export patterns used in A3

Generate fully functional, production-ready Rust code with comprehensive tests. All four modules must compile cleanly with no warnings.
