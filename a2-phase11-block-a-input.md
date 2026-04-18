# A2 Phase 11 Block A: quota_replication.rs Implementation

**Crate:** claudefs-meta
**File:** crates/claudefs-meta/src/quota_replication.rs
**Target:** 450 LOC, 20 tests, 0 warnings
**Model:** minimax-m2p5

---

## Context

**ClaudeFS** is a distributed, scale-out POSIX file system implemented in Rust. This is agent A2 (Metadata Service).

**Phase 10 (Complete):**
- ✅ quota_tracker.rs — Per-tenant storage/IOPS quotas with soft/hard limits
- ✅ tenant_isolator.rs — Namespace isolation for multi-tenancy
- ✅ qos_coordinator.rs — QoS coordination between A2 and A4

**Phase 11 Block A (This Task):**
Implement cross-site quota configuration replication with eventual consistency. When quota limits are updated on site A, sync to site B. Detect and resolve conflicts (e.g., both sites independently update the same quota).

---

## Requirements

### 1. Core Data Structures

Implement the following public types in `quota_replication.rs`:

```rust
/// Quota replication request to send to remote site
pub struct QuotaReplicationRequest {
    /// Unique request ID (UUID string)
    pub request_id: String,
    /// Tenant ID being updated
    pub tenant_id: String,
    /// Type of quota (Storage or Iops)
    pub quota_type: QuotaType,
    /// Soft limit (80% watermark) for quotas
    pub soft_limit: u64,
    /// Hard limit (100% watermark) for quotas
    pub hard_limit: u64,
    /// Unix epoch ms when request was created
    pub timestamp: u64,
    /// Lamport clock generation for ordering
    pub generation: u64,
    /// Source site (e.g., "site-a" or "site-b")
    pub source_site: String,
}

/// Acknowledgment of quota replication
pub struct QuotaReplicationAck {
    /// Request ID being acknowledged
    pub request_id: String,
    /// Status of replication (Pending/Applied/Conflict/Failed)
    pub status: ReplicationStatus,
    /// Destination site that processed the request
    pub destination_site: String,
    /// Unix epoch ms when applied locally
    pub applied_at: u64,
}

/// Status of a replication request
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplicationStatus {
    /// Received but not yet applied
    Pending,
    /// Successfully applied
    Applied,
    /// Conflicted with local state, resolution applied
    Conflict,
    /// Failed (with error message)
    Failed(String),
}

/// Detected conflict during quota replication
pub struct QuotaReplicationConflict {
    /// Tenant ID with conflicting quotas
    pub tenant_id: String,
    /// Type of quota
    pub quota_type: QuotaType,
    /// Limit at site A (local)
    pub site_a_limit: u64,
    /// Limit at site B (remote)
    pub site_b_limit: u64,
    /// Strategy used to resolve conflict
    pub resolution_strategy: ResolutionStrategy,
    /// When conflict was detected (Unix epoch ms)
    pub detected_at: u64,
}

/// Strategy for resolving quota conflicts
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionStrategy {
    /// Use higher of the two limits
    MaxWins,
    /// Use most recent (by timestamp)
    TimestampWins,
    /// Requires manual admin review (alert raised)
    AdminReview,
}

/// Metrics for quota replication
#[derive(Clone, Default, Debug)]
pub struct QuotaReplicationMetrics {
    /// Number of replication requests sent to remote site
    pub requests_sent: u64,
    /// Number of replication requests received from remote site
    pub requests_received: u64,
    /// Number of ACKs received from remote site
    pub acks_received: u64,
    /// Number of conflicts detected
    pub conflicts_detected: u64,
    /// Current replication lag in milliseconds
    pub replication_lag_ms: u64,
    /// Number of pending (unanswered) requests
    pub pending_requests: usize,
}

/// Import existing QuotaType from Phase 10
pub use crate::quota_tracker::QuotaType;
```

Use existing types from the crate:
- **QuotaType** — from `crate::quota_tracker` (Storage / Iops)
- **DashMap** — from `dashmap` crate (thread-safe, lock-free reads)
- **Arc, Mutex** — from `std::sync` (for hot counters)
- **UUID** — use `uuid::Uuid::new_v4().to_string()` for request_id

---

### 2. Public Functions

Implement the following 7 functions in `pub impl`:

#### 2.1 `replicate_quota_config`

```rust
pub fn replicate_quota_config(
    request_id: &str,
    tenant_id: &str,
    quota_type: QuotaType,
    soft_limit: u64,
    hard_limit: u64,
    source_site: &str,
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    generation: &Arc<std::sync::atomic::AtomicU64>,
) -> QuotaReplicationRequest
```

**Behavior:**
- Accept quota update parameters
- Increment generation counter (atomic, SeqCst ordering)
- Create QuotaReplicationRequest with:
  - request_id (provided as input)
  - tenant_id, quota_type, soft_limit, hard_limit (from args)
  - timestamp = current Unix epoch ms
  - generation = atomic counter value
  - source_site (from args)
- Insert into pending_requests map
- Return the created request
- **No async, pure function**

---

#### 2.2 `apply_remote_quota_update`

```rust
pub fn apply_remote_quota_update(
    request: &QuotaReplicationRequest,
    quota_tracker: &crate::quota_tracker::QuotaTracker,
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    acks: &Arc<DashMap<String, QuotaReplicationAck>>,
    local_generation: &Arc<std::sync::atomic::AtomicU64>,
    conflict_log: &Arc<DashMap<String, QuotaReplicationConflict>>,
    local_site: &str,
) -> QuotaReplicationAck
```

**Behavior:**
- Check if local generation is less than remote generation
  - If remote is newer: accept and update local quota
  - If local is newer: conflict detected
  - If equal: use TimestampWins (by request timestamp)

- If no conflict: apply to quota_tracker (call its update API)
  - Create ACK with status = Applied
  - Remove from pending_requests

- If conflict detected:
  - Fetch existing local quota for this (tenant_id, quota_type) from quota_tracker
  - Create QuotaReplicationConflict entry
  - Apply resolution (MaxWins by default)
  - Update quota_tracker with winning limit
  - Create ACK with status = Conflict
  - Increment conflicts_detected metric

- Store ACK in acks map
- Set applied_at = current Unix epoch ms
- Set destination_site = local_site
- Return ACK

**Validation:**
- tenant_id and quota_type must be non-empty
- soft_limit ≤ hard_limit (reject if not)
- Return Failed ACK if validation fails

---

#### 2.3 `handle_quota_conflict`

```rust
pub fn handle_quota_conflict(
    conflict: &QuotaReplicationConflict,
    strategy: ResolutionStrategy,
    quota_tracker: &crate::quota_tracker::QuotaTracker,
    audit_log: &Arc<DashMap<String, String>>,
) -> u64
```

**Behavior:**
- Take conflict record and resolution strategy
- Match on strategy:
  - **MaxWins:** Return max(site_a_limit, site_b_limit)
  - **TimestampWins:** Compare timestamps (from calling context), return older request's limit
  - **AdminReview:** Return 0 (sentinel; caller must alert admin)

- Log conflict to audit_log: format as "{tenant_id}:{quota_type}:{detected_at}:resolved"
- Update quota_tracker with winning limit (if not AdminReview)
- Return the winning limit value

**Error Handling:**
- If quota_tracker update fails, catch error and log to audit
- Return 0 on error (caller should handle gracefully)

---

#### 2.4 `sync_quota_state`

```rust
pub async fn sync_quota_state(
    quota_tracker: &crate::quota_tracker::QuotaTracker,
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    acks: &Arc<DashMap<String, QuotaReplicationAck>>,
    source_site: &str,
    destination_site: &str,
    local_generation: &Arc<std::sync::atomic::AtomicU64>,
) -> Result<(usize, usize), String>
```

**Behavior:**
- Full state sync (used after partition recovery)
- Iterate all tenants in quota_tracker
- For each tenant + quota_type:
  - Create QuotaReplicationRequest with generation=0 (full sync marker)
  - Insert into pending_requests
  - Increment generation

- Wait for ACKs: collect all request_ids, poll acks map until all present or timeout (30s)
- Count successful ACKs (status == Applied or Conflict)
- Return (total_synced, total_failed)

**Timeout Behavior:**
- If 30s passed and not all acks received, return early with partial counts
- Log remaining pending requests

**Async Details:**
- Use tokio::time::sleep(Duration::from_millis(100)) in polling loop
- Use tokio::time::timeout for 30s deadline

---

#### 2.5 `get_replication_metrics`

```rust
pub fn get_replication_metrics(
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    acks: &Arc<DashMap<String, QuotaReplicationAck>>,
    metrics: &Arc<std::sync::Mutex<QuotaReplicationMetrics>>,
) -> QuotaReplicationMetrics
```

**Behavior:**
- Calculate current state:
  - pending_requests = pending_requests.len()
  - replication_lag_ms = now - (oldest pending request's timestamp, or 0)

- Lock metrics mutex and read current state
- Update with live counts:
  - pending_requests (just calculated)
  - replication_lag_ms (just calculated)

- Return updated metrics
- Do NOT consume/remove any entries

---

#### 2.6 `batch_replicate_quotas`

```rust
pub fn batch_replicate_quotas(
    requests: Vec<QuotaReplicationRequest>,
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    metrics: &Arc<std::sync::Mutex<QuotaReplicationMetrics>>,
) -> usize
```

**Behavior:**
- Accept vector of replication requests (pre-formed)
- For each request:
  - Insert into pending_requests map
  - Increment metrics.requests_sent

- Return count of requests inserted
- Used for batching multiple quota updates before sending to remote

---

#### 2.7 `clear_acked_requests`

```rust
pub fn clear_acked_requests(
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    acks: &Arc<DashMap<String, QuotaReplicationAck>>,
) -> usize
```

**Behavior:**
- Iterate pending_requests
- For each pending request, check if ack exists in acks map
- If ack status is Applied or Conflict: remove from pending
- Count removed entries
- Return count
- Used for cleanup after receiving ACKs

---

### 3. Integration Points

**Expected interactions with existing modules:**

1. **quota_tracker** (Phase 10):
   - Call `quota_tracker.get_quota(tenant_id, quota_type)` to fetch current
   - Call `quota_tracker.set_quota(tenant_id, quota_type, soft_limit, hard_limit)` to update
   - Return type: likely `Result<QuotaValue, QuotaError>`

2. **A6 replication (journal_tailer):**
   - `quota_replication.rs` will be called by A6 when it receives remote updates
   - Pass remote QuotaReplicationRequest → this module → returns ACK
   - Batching: A6 may call `batch_replicate_quotas` with multiple requests

3. **client_session** (Phase 9):
   - TenantContext may include quota_replication state
   - Sessions may inherit current quotas (read-only)

4. **tenant_audit** (Phase 11 Block B):
   - After Phase 11 Block B is implemented, log conflicts to audit trail
   - Audit log format: "{conflict detection, resolution, outcome}"

---

### 4. Thread Safety Requirements

- **DashMap** for pending_requests and acks: lock-free reads, concurrent iteration safe
- **Arc<AtomicU64>** for generation counter: use SeqCst ordering for consistency
- **Arc<Mutex<QuotaReplicationMetrics>>** for hot metrics: lock only for update

**No deadlocks:** Functions must not hold multiple locks simultaneously.

---

### 5. Error Handling

Define or reuse error type:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum QuotaReplicationError {
    #[error("invalid quota request: {0}")]
    InvalidRequest(String),

    #[error("quota not found: tenant={tenant_id}, type={quota_type}")]
    QuotaNotFound { tenant_id: String, quota_type: String },

    #[error("replication timeout after {0}ms")]
    ReplicationTimeout(u64),

    #[error("soft limit must be <= hard limit")]
    InvalidLimits,
}
```

Return `Result<T, QuotaReplicationError>` where appropriate.

---

### 6. Tests (20 total)

Write comprehensive tests covering:

1. **test_replicate_storage_limit** — Send storage quota update
2. **test_replicate_iops_limit** — Send IOPS quota update
3. **test_replicate_batch_updates** — Batch 10 quota updates
4. **test_replication_ordering_by_generation** — Lamport clock maintains order
5. **test_apply_remote_update_success** — Apply update, return Applied ACK
6. **test_apply_remote_update_validation** — Reject soft > hard
7. **test_conflict_max_wins_strategy** — 90GB vs 100GB → choose 100GB
8. **test_conflict_timestamp_wins_strategy** — Older update loses
9. **test_conflict_admin_review_strategy** — Alert raised, return 0
10. **test_sync_after_recovery** — Full state sync, all acks received
11. **test_sync_partial_failure** — 80% tenants sync, 20% timeout
12. **test_replication_idempotency** — Re-apply same request → no double update
13. **test_replication_lag_measurement** — Lag increases with pending requests
14. **test_pending_requests_cleanup** — Clear acked requests
15. **test_concurrent_updates_same_tenant** — 5 concurrent updates, last-write-wins
16. **test_generation_counter_increment** — Each request increments generation
17. **test_metrics_accuracy** — requests_sent, acks_received, conflicts match
18. **test_replication_with_quota_tracker** — Mock quota_tracker integration
19. **test_large_batch_100_tenants** — Batch 100 quotas, verify all pending
20. **test_replication_conflict_audit_trail** — Conflicts logged with details

**Test Framework:**
- Use `#[tokio::test]` for async tests (sync_quota_state)
- Use `#[test]` for synchronous tests
- Mock quota_tracker with simple HashMap or trait impl
- Use std::sync::Arc and DashMap directly in tests
- Check results with assert_eq!, assert!, etc.

---

### 7. Code Quality

**Requirements:**
- Zero compiler warnings (or mark explicitly with `#[allow]`)
- Follow existing A2 conventions:
  - Error handling: Use thiserror for library errors
  - Documentation: Add `///` doc comments to public types/functions
  - Logging: Use `tracing` for structured logging (optional but encouraged)
  - Thread safety: No `unsafe` code needed

**Clippy:**
- Fix all clippy warnings
- Use `cargo clippy -p claudefs-meta -- -D warnings` to verify

---

### 8. Deliverable

**Output file:** `crates/claudefs-meta/src/quota_replication.rs`

**Specifications:**
- 400-500 LOC (implementation + comments + tests)
- 20 unit tests (inline in same file or in separate test module)
- Zero compiler warnings
- Imports: std::*, dashmap::*, uuid::Uuid, thiserror, tokio (for async tests)
- Export pub use statements for all public types in final implementation

**Integration:**
After this file is generated:
1. Place in crates/claudefs-meta/src/quota_replication.rs
2. Add to lib.rs: `pub mod quota_replication;`
3. Add pub use statements in lib.rs for main types
4. Run: `cargo build -p claudefs-meta`
5. Run: `cargo test --lib -p claudefs-meta`

---

## Example Usage (for context)

```rust
// Create a replication request
let generation = Arc::new(AtomicU64::new(1));
let pending = Arc::new(DashMap::new());

let req = quota_replication::replicate_quota_config(
    "req-001",
    "tenant-a",
    QuotaType::Storage,
    80_000, // soft
    100_000, // hard
    "site-a",
    &pending,
    &generation,
);

// Later, apply on remote site
let ack = quota_replication::apply_remote_quota_update(
    &req,
    &quota_tracker,
    &pending,
    &acks,
    &local_generation,
    &conflict_log,
    "site-b",
);

// Check metrics
let metrics = quota_replication::get_replication_metrics(&pending, &acks, &metrics);
println!("Pending: {}, ACKs: {}", metrics.pending_requests, metrics.acks_received);
```

---

## Summary

Implement a robust cross-site quota replication module with:
- Lamport clock ordering
- Conflict detection & resolution
- Full state sync capability
- Comprehensive metrics
- 20 unit tests
- Production-ready error handling

**Estimated implementation time:** 2-3 hours
**Output size:** ~450 LOC + 20 tests
