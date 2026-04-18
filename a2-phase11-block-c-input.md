# A2 Phase 11 Block C: qos_cross_site.rs Implementation

**Crate:** claudefs-meta
**File:** crates/claudefs-meta/src/qos_cross_site.rs
**Target:** 380 LOC, 16 tests, 0 warnings
**Model:** minimax-m2p5

---

## Context

Building on Phase 10's qos_coordinator and Phase 11 Block B's audit logging, Block C coordinates QoS globally across sites.

**Goal:** Ensure deadline propagation, SLA attainment, and graceful failover when primary site is unavailable.

---

## Requirements

### 1. Core Data Structures

```rust
/// Global QoS policy for a tenant across sites
#[derive(Clone, Debug)]
pub struct GlobalQosPolicy {
    /// Unique policy ID
    pub policy_id: String,
    /// Tenant ID this policy applies to
    pub tenant_id: String,
    /// SLA targets for site A (primary)
    pub site_a_sla_targets: SlaTargets,
    /// SLA targets for site B (secondary)
    pub site_b_sla_targets: SlaTargets,
    /// Behavior when primary site fails
    pub failover_behavior: FailoverBehavior,
    /// Priority remapping: primary priority → secondary priority
    pub priority_remap: std::collections::HashMap<Priority, Priority>,
}

/// SLA targets for latency (p99 in milliseconds)
#[derive(Clone, Debug, Copy)]
pub struct SlaTargets {
    /// Critical operations p99 latency (ms)
    pub critical_p99_ms: u64,
    /// Interactive operations p99 latency (ms)
    pub interactive_p99_ms: u64,
    /// Bulk operations p99 latency (ms)
    pub bulk_p99_ms: u64,
}

/// Failover behavior when primary site unavailable
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailoverBehavior {
    /// Maintain exact SLAs (may reject ops if can't meet deadline)
    Strict,
    /// Relax SLAs by 50% during failover (e.g., 50ms → 75ms)
    Relaxed,
}

/// Import Priority from Phase 10
pub use crate::qos_coordinator::Priority;

/// QoS context for cross-site operation
#[derive(Clone, Debug)]
pub struct GlobalQosContext {
    /// Unique request ID (UUID)
    pub request_id: String,
    /// Tenant ID for this operation
    pub tenant_id: String,
    /// Priority level
    pub priority: Priority,
    /// Absolute deadline (Unix epoch ms)
    pub deadline_ms: u64,
    /// Local site SLA target in ms
    pub local_sla_ms: u64,
    /// Remote site SLA target (if cross-site), in ms
    pub remote_sla_ms: Option<u64>,
    /// True if operation spans sites
    pub site_aware: bool,
    /// Replication level: 1 (single site) or 2 (replicated)
    pub replication_level: u8,
}

/// Metrics for cross-site QoS
#[derive(Clone, Debug, Default)]
pub struct CrossSiteQosMetrics {
    /// Requests sent to remote site for coordination
    pub requests_sent_remote: u64,
    /// Requests denied by remote site (no capacity)
    pub requests_denied_remote: u64,
    /// Operations that missed deadline
    pub deadline_violations: u64,
    /// Number of times failover activated
    pub failover_activations: u64,
    /// SLA attainment ratio (0.0-1.0)
    pub sla_attainment: f64,
}

/// Failover event record
#[derive(Clone, Debug)]
pub struct FailoverEvent {
    /// When failover was triggered (Unix epoch ms)
    pub timestamp: u64,
    /// From site (primary)
    pub from_site: String,
    /// To site (secondary)
    pub to_site: String,
    /// Reason for failover
    pub reason: FailoverReason,
    /// SLA adjustments applied: priority → (old_ms, new_ms)
    pub sla_adjustments: std::collections::HashMap<String, (u64, u64)>,
}

/// Reason for failover
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailoverReason {
    /// Primary site unreachable
    SiteUnreachable,
    /// Network latency too high
    HighLatency,
    /// Resource exhausted on primary
    ResourceExhausted,
    /// Manually triggered by admin
    AdminTriggered,
}
```

---

### 2. Public Functions

#### 2.1 `get_global_qos_policy`

```rust
pub fn get_global_qos_policy(
    tenant_id: &str,
    policies: &Arc<DashMap<String, GlobalQosPolicy>>,
) -> Result<GlobalQosPolicy, String>
```

**Behavior:**
- Fetch policy for tenant from policies map
- Return policy or error if not found

---

#### 2.2 `compute_global_deadline`

```rust
pub fn compute_global_deadline(
    local_sla_target_ms: u64,
    network_latency_ms: u64,
    replication_level: u8,
) -> u64
```

**Behavior:**
- Compute absolute deadline
- If replication_level == 1 (single site):
  - deadline = now_ms + local_sla_target_ms

- If replication_level == 2 (cross-site):
  - Subtract network latency from SLA
  - deadline = now_ms + (local_sla_target_ms - network_latency_ms)
  - Ensure deadline > now_ms

- Return deadline (Unix epoch ms)

---

#### 2.3 `admit_cross_site_operation`

```rust
pub async fn admit_cross_site_operation(
    context: &GlobalQosContext,
    remote_rpc_call: impl std::future::Future<Output = Result<bool, String>>,
) -> Result<bool, String>
```

**Behavior:**
- Call remote RPC (via A4): "can_serve_within_deadline(deadline_ms)?"
- If remote says yes: return Ok(true)
- If remote says no:
  - Check if can escalate to Bulk priority
  - If yes: return Ok(true) with updated priority
  - If no: return Ok(false) — reject operation

- Timeout: 100ms for RPC call
  - If timeout: assume remote overloaded, return Ok(false)

---

#### 2.4 `send_qos_hint_to_remote`

```rust
pub async fn send_qos_hint_to_remote(
    context: &GlobalQosContext,
    remote_site: &str,
) -> Result<(), String>
```

**Behavior:**
- Encapsulate GlobalQosContext as QoSHint
- Send via A6 conduit (gRPC)
- Remote qos_coordinator receives hint and adjusts bandwidth_shaper
- Return Ok or error on send failure

---

#### 2.5 `handle_failover_to_remote_site`

```rust
pub fn handle_failover_to_remote_site(
    policy: &GlobalQosPolicy,
    from_site: &str,
    to_site: &str,
    reason: FailoverReason,
    audit_logger: &Arc<DashMap<String, String>>,
) -> FailoverEvent
```

**Behavior:**
- Activate failover_behavior:
  - If Strict: maintain exact SLAs (may reject ops)
  - If Relaxed: multiply all SLAs by 1.5x

- Create SLA adjustments map: for each priority level
  - old_ms = policy.site_a_sla_targets.get(priority)
  - new_ms = old_ms * 1.5 (if Relaxed)
  - Store in map

- Create FailoverEvent with:
  - timestamp = current Unix epoch ms
  - from_site, to_site (from args)
  - reason (from args)
  - sla_adjustments (computed above)

- Log to audit: "Failover: {reason:?}, SLAs adjusted"
- Increment metrics.failover_activations

- Return FailoverEvent

---

#### 2.6 `compute_sla_attainment`

```rust
pub fn compute_sla_attainment(
    completed_ops: u64,
    ops_meeting_deadline: u64,
) -> f64
```

**Behavior:**
- If completed_ops == 0: return 1.0 (perfect, no ops)
- attainment = ops_meeting_deadline as f64 / completed_ops as f64
- Return clamped to 0.0-1.0

---

#### 2.7 `query_cross_site_latency`

```rust
pub async fn query_cross_site_latency(
    remote_site: &str,
    latency_cache: &Arc<std::sync::Mutex<std::collections::HashMap<String, (u64, u64)>>>,
) -> Result<u64, String>
```

**Behavior:**
- Check latency_cache[remote_site]
- If exists and <10s old: return cached value
- Otherwise: Ping remote site (gRPC heartbeat)
- Measure round-trip time
- Store in cache with timestamp
- Return measured latency (ms)
- Cache TTL: 10 seconds

---

### 3. Integration

- Uses **A4 bandwidth_shaper** RPC for admission control
- Uses **A6 conduit** (gRPC) for deadline propagation
- Uses **Phase 10 qos_coordinator** for SLA target lookup
- Uses **tenant_audit** (Block B) for failover event logging

---

### 4. Thread Safety

- **DashMap** for policies, latency_cache: lock-free reads
- **Arc<Mutex>** for latency_cache with 10s TTL

---

### 5. Tests (16 total)

1. **test_global_qos_policy_fetch** — Load policy
2. **test_compute_global_deadline_single_site** — Single site SLA
3. **test_compute_global_deadline_replicated** — Cross-site, account for latency
4. **test_admit_cross_site_success** — Remote has capacity
5. **test_admit_cross_site_denied** — Remote overloaded
6. **test_admit_cross_site_escalate_to_bulk** — Escalate from Interactive
7. **test_admit_cross_site_timeout** — Remote RPC timeout, assume denied
8. **test_send_qos_hint_to_remote** — Propagate deadline
9. **test_handle_failover_strict** — Primary down, Strict behavior
10. **test_handle_failover_relaxed** — Primary down, Relaxed (1.5x SLAs)
11. **test_failover_event_logging** — Event details correct
12. **test_compute_sla_attainment_perfect** — 100% on deadline
13. **test_compute_sla_attainment_degraded** — 70% on deadline
14. **test_query_cross_site_latency** — Measure latency
15. **test_latency_caching** — Cache with 10s TTL
16. **test_priority_remap_across_sites** — Remap Critical → Interactive

---

## Deliverable

**Output file:** `crates/claudefs-meta/src/qos_cross_site.rs`

**Specifications:**
- 350-400 LOC
- 16 tests
- Zero warnings
- Imports: std::*, dashmap::*, tokio, thiserror

---

## Integration Expected

- A4 Transport: RPC for admission control
- A6 Replication: gRPC conduit for deadline propagation
- Phase 10 qos_coordinator: SLA lookup
- tenant_audit (Block B): failover logging
- A8 Management: Prometheus metrics export

**Expected integration time:** 1-2 hours
