# Phase 33 Block 2: Quota Enforcement
## OpenCode Implementation Prompt

**Target:** Generate Rust source + integration tests for per-tenant storage quotas
**Model:** minimax-m2p5
**Output:** ~550 LOC (source) + test code (18 tests)

---

## Context

ClaudeFS data reduction (A3) Phase 33 Block 2 focuses on multi-tenant fairness through:

1. Per-tenant storage quotas (soft warnings at 90%, hard rejections at 100%)
2. Fairness queuing to prevent one tenant from starving others
3. Accurate quota accounting with dedup credits
4. Crash-safe consistency

**Key constraint:** All unsafe code isolated to io_uring/FUSE/transport FFI boundaries. Quota enforcement must be pure safe Rust.

---

## Architecture Context

### Integration Points

**A2 Metadata Service (Phase 11):** Quotas, multi-tenancy metadata available
- TenantId available from metadata layer
- Per-tenant tracking hooks in write path

**Phase 32 Pipeline:** Dedup/compression savings quantifiable
- `DedupResult` includes `savings_bytes`
- `CompressionRatio` from pipeline
- `DeltaCompression` credits available

### Existing QuotaTracker (from Phase 31)

```rust
pub struct QuotaTracker {
    pub namespace_id: NamespaceId,
    pub used_bytes: u64,
    pub limit_bytes: u64,
}

pub enum QuotaViolation {
    SoftQuota,
    HardQuota,
}
```

### Write Path Integration Point

```rust
// Before write_pipeline.process():
let quota_ok = quota_manager.check_quota(tenant_id, write_size)?;

// After dedup/compress completion:
quota_manager.update_usage(tenant_id, savings_bytes)?;
```

---

## Implementation Specification

### 1. QuotaManager (250 LOC)

Core quota enforcement with per-tenant tracking.

```rust
pub struct QuotaConfig {
    pub soft_quota_percent: f64,        // Default: 90%
    pub hard_quota_percent: f64,        // Default: 100%
    pub grace_period_secs: u64,         // Default: 300
    pub admin_override_enabled: bool,   // Default: true
}

pub struct QuotaManager {
    quotas: Arc<RwLock<HashMap<TenantId, TenantQuota>>>,
    accounting: Arc<RwLock<QuotaAccounting>>,
    config: QuotaConfig,
    metrics: Arc<QuotaMetrics>,
}

pub struct TenantQuota {
    tenant_id: TenantId,
    limit_bytes: u64,
    used_bytes: u64,
    soft_quota_triggered: bool,
    hard_quota_timestamp: Option<Instant>,
    dedup_credits: HashMap<BlockId, u64>,
}

impl QuotaManager {
    pub async fn check_quota(
        &self,
        tenant_id: TenantId,
        write_bytes: u64,
        is_admin: bool,
    ) -> Result<QuotaDecision, ReduceError> {
        // Returns: AllowedFull, AllowedRestricted, SoftQuotaWarning, Rejected
    }

    pub async fn update_usage(
        &mut self,
        tenant_id: TenantId,
        delta_bytes: i64,  // negative for dedup savings
        reason: UsageReason,  // Dedup, Compression, Tiering, etc.
    ) -> Result<(), ReduceError> {
        // Crash-safe: log to journal before applying
    }

    pub async fn apply_dedup_credit(
        &mut self,
        tenant_id: TenantId,
        block_id: BlockId,
        credit_bytes: u64,
    ) -> Result<(), ReduceError> {
        // Handle cross-tenant dedup (shared blocks counted once)
    }

    pub async fn get_tenant_usage(&self, tenant_id: TenantId) -> Result<TenantUsage, ReduceError> {
        // Returns: used_bytes, limit_bytes, percent_used, soft_quota_warning
    }
}
```

### 2. FairnessQueue (200 LOC)

Priority queuing to prevent starvation.

```rust
pub struct FairnessQueueConfig {
    pub max_queue_depth: usize,        // Default: 10000
    pub batch_timeout_ms: u64,         // Default: 100
    pub priority_boost_percent: f64,   // Default: 10%
}

pub struct FairnessQueue {
    queue: Arc<Mutex<PriorityQueue<QueuedWrite>>>,
    config: FairnessQueueConfig,
    metrics: Arc<QueueMetrics>,
}

pub struct QueuedWrite {
    tenant_id: TenantId,
    write_size: u64,
    priority: f64,  // % quota consumed (higher = lower priority)
    enqueued_at: Instant,
}

impl FairnessQueue {
    pub async fn enqueue(&mut self, write: QueuedWrite) -> Result<(), ReduceError> {
        // FIFO within priority level to prevent starvation
    }

    pub async fn dequeue(&mut self) -> Result<Option<QueuedWrite>, ReduceError> {
        // Return next write, respecting priority + fairness
    }

    pub async fn get_queue_depth(&self, tenant_id: TenantId) -> Result<usize, ReduceError> {
        // How many writes queued for this tenant?
    }
}
```

### 3. QuotaAccountant (200 LOC)

Crash-safe accounting with historical audit trail.

```rust
pub struct QuotaAccountant {
    journal: Arc<QuotaJournal>,  // Crash-safe append-only log
    snapshots: Arc<RwLock<Vec<QuotaSnapshot>>>,
}

pub struct UsageReason {
    pub kind: UsageKind,  // Dedup, Compression, Tiering, Repair, etc.
    pub metadata: HashMap<String, String>,
}

pub struct QuotaJournalEntry {
    timestamp: Instant,
    tenant_id: TenantId,
    delta_bytes: i64,
    reason: UsageReason,
}

impl QuotaAccountant {
    pub async fn record(
        &mut self,
        tenant_id: TenantId,
        delta_bytes: i64,
        reason: UsageReason,
    ) -> Result<(), ReduceError> {
        // Write to journal, then apply to in-memory state
    }

    pub async fn reconcile(&mut self) -> Result<ReconciliationStats, ReduceError> {
        // Verify accounting consistency, detect leaks
    }

    pub async fn audit_trail(
        &self,
        tenant_id: TenantId,
        since: Instant,
    ) -> Result<Vec<QuotaJournalEntry>, ReduceError> {
        // Return historical usage
    }
}
```

### 4. Cross-Tenant Dedup Handling (100 LOC)

When an exact dedup match spans multiple tenants:
- Record the match in both tenants' dedup credit tables
- Use reference counting to track which tenants own which blocks
- On eviction, apportion the credit fairly

```rust
pub struct CrossTenantDedupEntry {
    block_id: BlockId,
    owner_tenant_id: TenantId,
    referring_tenants: HashSet<TenantId>,
    refcount: u64,
}

pub async fn handle_cross_tenant_dedup(
    accountant: &mut QuotaAccountant,
    block_id: BlockId,
    initial_owner: TenantId,
    new_referrer: TenantId,
) -> Result<(), ReduceError> {
    // Update refcount, record in both tenants' dedup tables
}
```

---

## Test Categories (18 tests)

### 1. Quota Enforcement (4 tests)

```rust
#[tokio::test]
#[ignore]
async fn test_soft_quota_warning() {
    // Write at 90% of quota -> should return SoftQuotaWarning
    // Client gets data back but with warning
}

#[tokio::test]
#[ignore]
async fn test_hard_quota_rejection() {
    // Write at 100% of quota -> should return Rejected
    // No data written
}

#[tokio::test]
#[ignore]
async fn test_quota_grace_period() {
    // Hit hard quota, admin initiates cleanup (e.g., tiering to S3)
    // 5-minute grace period allows writes to tier-eligible data
}

#[tokio::test]
#[ignore]
async fn test_quota_override_admin() {
    // Admin force-write above quota with override flag
    // Exceeds hard limit but logged for audit
}
```

### 2. Fairness Queuing (4 tests)

```rust
#[tokio::test]
#[ignore]
async fn test_fairness_no_starvation() {
    // 2 tenants: T1 at 95% quota, T2 at 10% quota
    // Send writes from both -> both make progress, no starvation
}

#[tokio::test]
#[ignore]
async fn test_fairness_weighted_priority() {
    // T1 at 80% priority, T2 at 40% priority
    // Verify dequeue order respects weighted fairness
}

#[tokio::test]
#[ignore]
async fn test_fairness_queue_timeout() {
    // Enqueue write, wait 10 minutes without dequeue
    // Should auto-expire to prevent indefinite blocking
}

#[tokio::test]
#[ignore]
async fn test_fairness_batch_clustering() {
    // 100 tiny writes from 2 tenants
    // Queue should cluster similar-sized writes for efficiency
}
```

### 3. Dedup Accounting (4 tests)

```rust
#[tokio::test]
#[ignore]
async fn test_quota_exact_dedup_credit() {
    // T1 writes 1GB, exact dedup match with T2's data
    // T1 should get 1GB credit back (not charged for duplicate)
}

#[tokio::test]
#[ignore]
async fn test_quota_similarity_dedup_credit() {
    // T1 writes data similar to T2's data, delta compression saves 800MB
    // T1 should get 800MB credit (charged for delta only)
}

#[tokio::test]
#[ignore]
async fn test_quota_cross_tenant_dedup() {
    // T1 and T2 write identical data
    // Shared block should be counted once, not twice
}

#[tokio::test]
#[ignore]
async fn test_quota_snapshot_accounting() {
    // Create snapshot from shared blocks
    // Snapshot shouldn't double-charge for blocks already allocated to snapshot owner
}
```

### 4. Quota Consistency (6 tests)

```rust
#[tokio::test]
#[ignore]
async fn test_quota_crash_recovery() {
    // Simulate crash after quota update
    // Recover from journal, verify no leaks or double-counts
}

#[tokio::test]
#[ignore]
async fn test_quota_concurrent_updates() {
    // 10 concurrent writes from same tenant
    // No race conditions, final usage correct
}

#[tokio::test]
#[ignore]
async fn test_quota_compression_savings() {
    // Write 1GB, compression achieves 4:1 ratio
    // Quota should reflect 250MB (post-compression)
}

#[tokio::test]
#[ignore]
async fn test_quota_tiering_to_s3() {
    // At hard quota, tier 500MB to S3
    // Available quota should increase by 500MB
}

#[tokio::test]
#[ignore]
async fn test_quota_complex_topology() {
    // Dedup layers: exact match + similarity + compression
    // All credits applied correctly
}

#[tokio::test]
#[ignore]
async fn test_quota_audit_trail() {
    // Query historical usage for last 24 hours
    // Return chronological record of all quota changes
}
```

---

## OpenCode Directives

1. **Create modules:**
   - `crates/claudefs-reduce/src/quota_manager.rs` — Main enforcement
   - `crates/claudefs-reduce/src/fairness_queue.rs` — Priority queuing
   - `crates/claudefs-reduce/src/quota_accountant.rs` — Accounting + journal

2. **Update:**
   - `crates/claudefs-reduce/src/lib.rs` — Export public types
   - `crates/claudefs-reduce/src/error.rs` — Add QuotaError variants if needed

3. **Generate test file:**
   - `crates/claudefs-reduce/tests/cluster_quota_enforcement.rs` — 18 tests, 550 LOC

4. **Ensure:**
   - All tests compile cleanly (no warnings in generated code)
   - Tests marked `#[ignore]` (cluster-only)
   - Documentation comments for all public items
   - Error handling via Result<T, ReduceError>

---

## Success Criteria

- [ ] All 18 tests compile and run under `cargo test --ignored`
- [ ] Zero compilation errors in generated code
- [ ] All public types properly documented
- [ ] Quota enforcement prevents over-subscription
- [ ] Fairness queuing prevents starvation
- [ ] Cross-tenant dedup accounting correct

**Timeline:** ~45 minutes (30 min generation + 15 min testing/fixes)
