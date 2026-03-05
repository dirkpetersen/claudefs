# A2 Phase 10: Multi-Tenancy, Quotas, and QoS Coordination

## Context

ClaudeFS A2 (Metadata Service) completed Phase 9 with 1035 tests (client_session, distributed_transaction, snapshot_transfer). Phase 10 focuses on **Priority 1 production features**: multi-tenancy support, per-tenant storage/IOPS quotas, and QoS coordination with A4 Transport.

These modules enable:
- **Tenant isolation:** Logical namespace separation, per-tenant metadata trees, strong quota boundaries
- **Resource enforcement:** Storage limits, IOPS caps, bandwidth throttling (A2↔A4 coordination)
- **Fair scheduling:** Prevent one tenant from starving others; deadline-based prioritization

## Existing Architecture

Read these existing A2 modules first:
- `session_manager.rs` — client_session.rs Phase 9, session state tracking
- `distributed_transaction.rs` — Phase 9, atomic operations across shards
- `membership.rs` — node membership, failure detection
- `consensus.rs` — Raft consensus, log replication
- `journal_tailer.rs` — follow Raft log, build replication batches
- `inode.rs` — POSIX inode operations
- `kvstore.rs` — key-value store interface (RocksDB)
- `cross_shard.rs` — multi-shard coordination
- `types.rs` — shared types (InodeId, NodeId, Timestamp, MetaError)

Also review A4 Transport modules:
- `bandwidth_shaper.rs` (Phase 12 candidate) — QoS enforcement at network layer
- Understand QosRequest, Priority levels (Critical/Interactive/Bulk)
- A2 will emit QoS hints; A4 will enforce at transport layer

## Phase 10 Module Specifications

### 1. quota_tracker.rs (~25 tests)

**Purpose:** Track and enforce per-tenant storage and IOPS quotas.

**Key Types:**
```
TenantId(String)  // Unique tenant identifier
QuotaType::Storage(bytes)  // 1TB = 1099511627776 bytes
QuotaType::Iops(u64)  // operations per second

TenantQuota {
    tenant_id: TenantId,
    storage_limit_bytes: u64,
    iops_limit: u64,
    soft_limit_warning_pct: f64,  // e.g., 80% → emit warning
    created_at: Timestamp,
    updated_at: Timestamp,
}

QuotaUsage {
    tenant_id: TenantId,
    used_storage_bytes: u64,  // cumulative file content
    used_iops_this_second: u64,  // ops in current 1-sec window
    storage_pct: f64,  // used / limit * 100
    iops_pct: f64,
    last_updated: Timestamp,
}

QuotaViolation {
    tenant_id: TenantId,
    violation_type: ViolationType,  // Storage / Iops / BothExceeded
    current_usage: u64,
    quota_limit: u64,
    exceeded_by_pct: f64,
    timestamp: Timestamp,
    severity: Severity,  // Warning / Critical
}
```

**Methods:**
- `QuotaTracker::new(config) → Self` — initialize with default soft limits (80%)
- `add_quota(tenant_id, storage_bytes, iops) → Result<(), MetaError>` — register new tenant quota
- `update_quota(tenant_id, new_storage, new_iops) → Result<(), MetaError>` — modify existing
- `get_quota(tenant_id) → Option<TenantQuota>` — lookup
- `get_usage(tenant_id) → Option<QuotaUsage>` — current usage snapshot
- `check_storage_available(tenant_id, bytes_needed) → Result<bool, QuotaViolation>` — before allocating
- `check_iops_available(tenant_id) → Result<bool, QuotaViolation>` — before issuing operation
- `record_storage_write(tenant_id, bytes_written) → Result<(), QuotaViolation>` — after write
- `record_storage_delete(tenant_id, bytes_freed)` — after delete
- `get_violations(tenant_id) → Vec<QuotaViolation>` — all active violations
- `reset_iops_window()` — called each second, resets used_iops_this_second counter

**State:**
- `DashMap<TenantId, TenantQuota>` — quotas
- `DashMap<TenantId, Arc<Mutex<QuotaUsage>>>` — usage counters (Arc for shared updates)
- `Vec<QuotaViolation>` — violation history (last 1000 or time-windowed)

**Behavior:**
- **Soft limit (80%):** Log warning; operations continue
- **Hard limit (100%):** Reject new writes; operations fail with ENOSPC-like error
- **IOPS window:** Sliding 1-second window; reset automatically
- **Thread-safe:** DashMap for lock-free reads; Arc<Mutex> for usage updates (few per second)

**Tests (~25):**
- Create/update/get quota
- Check storage available (below/at/above limit)
- Check IOPS available (below/above limit)
- Record writes/deletes
- Soft limit warnings at 80%
- Hard limit rejection at 100%
- Multiple tenants concurrent quota checks
- IOPS window reset behavior
- Violation history tracking
- Recover from over-quota state (delete to free space)

---

### 2. tenant_isolator.rs (~20 tests)

**Purpose:** Enforce strong tenant isolation at metadata level.

**Key Types:**
```
TenantNamespace {
    tenant_id: TenantId,
    root_inode: InodeId,  // /tenants/{tenant_id}/
    metadata_shard_range: (u32, u32),  // shard IDs assigned to this tenant [start, end)
}

TenantContext {
    tenant_id: TenantId,
    user_id: uid_t,  // authenticated user
    session_id: SessionId,  // from client_session.rs
    namespace_root: InodeId,
    capabilities: TenantCapabilities,  // read, write, delete, admin
}

TenantCapabilities {
    can_read: bool,
    can_write: bool,
    can_delete: bool,
    can_modify_quotas: bool,
    can_view_other_tenants: bool,  // false for regular tenants
}

IsolationViolation {
    violation_type: ViolationType,  // CrossTenantRead / NamespaceEscape / PermissionDenied
    tenant_id: TenantId,
    attempted_inode: InodeId,
    owner_tenant: TenantId,
    timestamp: Timestamp,
}
```

**Methods:**
- `TenantIsolator::new(config) → Self`
- `register_tenant(tenant_id, initial_capacity_bytes) → Result<TenantNamespace, MetaError>` — allocate namespace root and shard range
- `get_tenant_namespace(tenant_id) → Option<TenantNamespace>`
- `get_tenant_context(session_id) → Option<TenantContext>` — from active session
- `enforce_isolation(context, inode_id) → Result<(), IsolationViolation>` — before any inode operation
  - Verify inode belongs to context's tenant
  - Verify shard range alignment
  - Reject cross-tenant access attempts
- `list_inodes_in_tenant(tenant_id, dir_inode) → Result<Vec<InodeId>, MetaError>` — tenant-scoped directory listing
- `get_violations(tenant_id) → Vec<IsolationViolation>` — access attempt logs

**State:**
- `DashMap<TenantId, TenantNamespace>` — tenant metadata
- `DashMap<SessionId, TenantContext>` — session↔tenant mapping (populated from client_session.rs)
- `Vec<IsolationViolation>` — audit log (last 10k or time-windowed)

**Behavior:**
- **Namespace root:** Each tenant has a dedicated root inode under `/tenants/{tenant_id}/`
- **Shard isolation:** Tenant's metadata shards don't overlap with other tenants
- **Session binding:** TenantContext derives from authenticated session; operations inherit tenant_id
- **Cross-tenant access:** Any attempt to read/write/delete inode outside tenant's namespace → IsolationViolation + EACCES
- **Audit logging:** All violations logged for compliance

**Tests (~20):**
- Register/list/get tenant
- Session→tenant context mapping
- Enforce isolation (allow within namespace, reject outside)
- Cross-tenant read attempt blocked
- Namespace escape attempt blocked (directory traversal)
- Permission denied for unauth'd operations
- Multiple tenants concurrent isolation checks
- Shard range alignment
- Audit log of violations
- List inodes within tenant's directory tree

---

### 3. qos_coordinator.rs (~20-25 tests)

**Purpose:** Coordinate QoS enforcement between A2 (Metadata) and A4 (Transport).

**Key Types:**
```
Priority = enum { Critical, Interactive, Bulk }  // shared with A4

QosRequest {
    request_id: RequestId,
    operation_type: OpType,  // Read / Write / Metadata / Delete
    tenant_id: TenantId,
    priority: Priority,
    estimated_duration_ms: u64,
    estimated_bytes: u64,  // for Write operations
    deadline_ms: u64,  // absolute deadline; 0 = no deadline
}

QosContext {
    request_id: RequestId,
    priority: Priority,
    tenant_id: TenantId,
    started_at: Timestamp,
    deadline: Option<Timestamp>,
    sla_target_p99_ms: u64,  // e.g., Critical → 10ms, Interactive → 50ms, Bulk → 500ms
}

QosMetrics {
    request_id: RequestId,
    operation_type: OpType,
    priority: Priority,
    latency_ms: u64,  // actual duration
    sla_target_ms: u64,
    sla_met: bool,  // latency <= sla_target
    tenant_id: TenantId,
    bytes_processed: u64,
}

QosViolation {
    request_id: RequestId,
    priority: Priority,
    tenant_id: TenantId,
    sla_target_ms: u64,
    actual_latency_ms: u64,
    violation_severity: f64,  // (actual - target) / target * 100 %
    timestamp: Timestamp,
}
```

**Methods:**
- `QosCoordinator::new(config) → Self` — configure SLA targets per priority level
- `create_context(request: QosRequest) → QosContext` — allocate unique request_id, set deadline
- `estimate_priority(tenant_id, op_type) → Priority` — ML-assisted: based on tenant tier, operation history
  - High-tier tenants → Critical for reads
  - Bulk operations → Bulk (even for high-tier, if operation > 10MB)
  - Metadata-only → Interactive
- `should_reject_operation(context: &QosContext) → bool` — if queue too full, reject to maintain SLA
  - Queue depth ≥ max_queue_depth → reject Bulk operations
  - Reject if estimated wait time > (deadline - now)
- `emit_qos_hint(context) → QosHint` — serialize for A4 Transport
  ```
  struct QosHint {
      priority: Priority,
      deadline_us: u64,
      max_latency_us: u64,
  }
  ```
- `record_completion(request_id, actual_latency_ms, bytes) → QosMetrics` — after operation
  - Calculate if SLA met
  - Log metrics and violations
- `get_violations(tenant_id) → Vec<QosViolation>` — SLA misses by tenant
- `get_metrics_summary() → QosMetricsSummary` — agg stats: per-priority p50/p99 latency, SLA attainment %

**State:**
- `DashMap<RequestId, QosContext>` — in-flight requests
- `VecDeque<RequestId>` per priority — queue for backpressure (max 1000 per priority)
- `Vec<QosMetrics>` — completed request history (ring buffer, 100k slots)
- `Vec<QosViolation>` — SLA violations (ring buffer, 10k slots)
- `SLA config:` [Critical:10ms p99, Interactive:50ms p99, Bulk:500ms p99]

**Behavior:**
- **Priority-aware scheduling:** A2 prioritizes metadata operations for Critical > Interactive > Bulk
- **Deadline awareness:** Reject operations that can't make deadline (avoid queue thrashing)
- **Backpressure:** If queue for a priority is full (1000+), reject new Bulk operations to protect Interactive/Critical
- **SLA tracking:** Measure p99 latency per priority; alert if < 95% attainment
- **A2↔A4 coordination:** A2 emits QosHint via distributed_transaction context; A4 uses hint for network prioritization
- **Admission control:** Before Raft consensus, check if operation can meet its deadline

**Tests (~20-25):**
- Create QosContext with/without deadline
- Estimate priority for various operation types
- Reject operation if deadline already missed
- Reject operation if queue full (backpressure)
- Emit QosHint with correct priority and deadline
- Record completion and check SLA met/missed
- Track SLA violations
- Multiple priorities concurrent (no starvation)
- Metrics summary: p50/p99 latency
- Deadline propagation through distributed_transaction
- A2↔A4 hint serialization (mock A4 consumer)

---

## Integration Points

### quota_tracker ↔ tenant_isolator
- tenant_isolator calls quota_tracker.check_storage_available() before inode allocation
- quota_tracker aggregates across all inodes in tenant's namespace

### tenant_isolator ↔ distributed_transaction
- distributed_transaction creates TenantContext for cross-shard operations
- enforcement: operations span only shards belonging to context's tenant

### qos_coordinator ↔ quota_tracker
- QosRequest includes tenant_id; qos_coordinator checks quotas before admitting
- If tenant over-quota, prioritize as Bulk even if logically Critical

### qos_coordinator ↔ A4 Transport (bandwidth_shaper.rs)
- A2 creates QosContext, emits QosHint
- A4 bandwidth_shaper consumes QosHint to prioritize network traffic
- Backpressure: if A4 queue full, A2 qos_coordinator rejects new operations

### all three ↔ client_session (Phase 9)
- Session carries TenantContext
- All quota/isolation/QoS decisions inherit tenant_id from session
- Session lifecycle manages TenantContext lifecycle

---

## Estimated Tests

| Module | Description | Test Count |
|--------|-------------|-----------|
| quota_tracker.rs | Storage/IOPS enforcement | ~25 |
| tenant_isolator.rs | Namespace isolation + audit | ~20 |
| qos_coordinator.rs | QoS scheduling + SLA tracking | ~22 |
| **Total** | | **~67** |

**Phase 10 Target:** 1035 + 67 = **1102+ tests** (+67 from Phase 9)

---

## Implementation Notes

### Thread Safety & Async
- Use `DashMap<K, V>` for lock-free reads of quotas/tenants/contexts
- Use `Arc<Mutex<...>>` only for hot counters (used_iops_this_second, current_usage metrics)
- All methods must be `async`-ready for Tokio; use `tokio::spawn_blocking()` only for slow I/O to KV store
- Metrics/violations are append-only ring buffers (lock-free with atomic indexing)

### Error Handling
- Define new error variants in `types.rs`:
  - `MetaError::QuotaExceeded { tenant_id, used, limit }`
  - `MetaError::IsolationViolation { tenant_id, inode_id }`
  - `MetaError::SlaViolation { request_id, sla_ms, actual_ms }`
  - `MetaError::QosRejected { reason: String }`

### Serialization
- All types must be `Serialize + Deserialize` for Protobuf/wire format
- `TenantId`, `Priority`, `QosHint` will be replicated across sites via A6 replication

### Testing Strategy
- **Unit tests:** Each module independently (quotas, isolation, QoS)
- **Integration tests:** quota_tracker + tenant_isolator together (quota checks within tenant namespace)
- **Coordination tests:** qos_coordinator emits QosHint; mock A4 consumer validates
- **Property tests:** invariants (quota never exceeds limit, tenant never crosses namespace boundary, no priority starvation)

---

## Deliverables

1. ✅ Three new modules: quota_tracker.rs, tenant_isolator.rs, qos_coordinator.rs
2. ✅ Update lib.rs to export new types
3. ✅ ~67 unit + integration tests passing
4. ✅ Build clean (cargo check, cargo clippy)
5. ✅ CHANGELOG entry with Phase 10 summary
6. ✅ Code ready for A2 Phase 11 planning (cross-site quota replication, SLA enforcement at global scale)
