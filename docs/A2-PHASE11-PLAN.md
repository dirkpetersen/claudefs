# A2: Metadata Service — Phase 11 Planning

**Agent:** A2 (Metadata Service) | **Status:** 🟡 **PHASE 11 PLANNING** | **Date:** 2026-04-18

**Phase Target:** 1100-1150 tests (+50-75 from Phase 10's 1035 tests)

**Baseline:** Phase 10 complete with 73 modules, 1035+ tests, 0 failures, clean build

---

## Phase 11 Overview: Cross-Site Coordination & Compliance

Building on Phase 10's multi-tenancy foundation (quota_tracker, tenant_isolator, qos_coordinator), Phase 11 focuses on:

1. **Cross-site tenant state sync** — quota configs, tenant metadata, policies
2. **Comprehensive audit logging** — compliance trail for SOC2, HIPAA, GDPR, PCI-DSS
3. **Global QoS coordination** — coordinated scheduling across sites with deadline propagation
4. **Advanced tenant features** — billing integration, resource reservation, capacity planning

**Implementation blocks:**
- **Block A:** quota_replication.rs (20 tests)
- **Block B:** tenant_audit.rs (18 tests)
- **Block C:** qos_cross_site.rs (16 tests)
- **Block D:** billing_aggregator.rs (15 tests)
- **Optional:** capacity_planner.rs (8 tests)

---

## Block A: quota_replication.rs (20 tests, 450 LOC)

**Goal:** Cross-site quota configuration replication with eventual consistency.

### Data Structures

```rust
pub struct QuotaReplicationRequest {
    pub request_id: String,           // UUID
    pub tenant_id: String,
    pub quota_type: QuotaType,        // Storage/Iops
    pub soft_limit: u64,              // 80% watermark
    pub hard_limit: u64,              // 100% watermark
    pub timestamp: u64,               // Unix epoch ms
    pub generation: u64,              // Lamport clock for ordering
    pub source_site: String,          // Origin site (site-a / site-b)
}

pub struct QuotaReplicationAck {
    pub request_id: String,
    pub status: ReplicationStatus,    // Pending/Applied/Conflict
    pub destination_site: String,
    pub applied_at: u64,              // Timestamp when applied
}

pub enum ReplicationStatus {
    Pending,
    Applied,
    Conflict,
    Failed(String),
}

pub struct QuotaReplicationConflict {
    pub tenant_id: String,
    pub quota_type: QuotaType,
    pub site_a_limit: u64,
    pub site_b_limit: u64,
    pub resolution_strategy: ResolutionStrategy, // MaxWins/TimestampWins/AdminReview
    pub detected_at: u64,
}

pub enum ResolutionStrategy {
    MaxWins,                          // Use higher limit
    TimestampWins,                    // Use most recent
    AdminReview,                      // Requires manual intervention
}

pub struct QuotaReplicationMetrics {
    pub requests_sent: u64,
    pub requests_received: u64,
    pub acks_received: u64,
    pub conflicts_detected: u64,
    pub replication_lag_ms: u64,
    pub pending_requests: usize,
}
```

### Core Functions

1. **`replicate_quota_config`** — Send quota changes to remote site
   - Generates request_id with Lamport clock
   - Serializes and sends via A6 journal_tailer
   - Tracks pending requests in memory
   - Returns Future<ReplicationAck>

2. **`apply_remote_quota_update`** — Receive and apply quota update from peer site
   - Validates tenant_id belongs to local site or is global
   - Checks Lamport clock ordering
   - Compares with local quota_tracker
   - Detects conflicts (hard_limit differs)
   - Calls resolution_strategy function
   - Updates quota_tracker with new values
   - Sends back ReplicationAck

3. **`handle_quota_conflict`** — Resolve concurrent quota updates
   - Logs conflict event to tenant_audit
   - Executes resolution_strategy (MaxWins / TimestampWins / AdminReview)
   - Updates metrics.conflicts_detected
   - If AdminReview, raises alert

4. **`sync_quota_state`** — Full quota sync after site recovery
   - Iterates all tenants in quota_tracker
   - Sends full QuotaReplicationRequest for each (generation=0)
   - Waits for all acks (timeout 30s)
   - Returns count of successfully synced tenants

5. **`get_replication_metrics`** — Return current replication status
   - Pending request count
   - Replication lag in ms
   - Conflict count
   - Acks/requests ratio

### Test Coverage (20 tests)

1. **test_quota_replicate_storage_limit** — Send storage quota to peer, verify ack
2. **test_quota_replicate_iops_limit** — Send IOPS quota to peer
3. **test_quota_replication_batching** — Send 10 updates in batch
4. **test_quota_replication_ordering** — Ensure Lamport clock maintains order
5. **test_quota_apply_remote_update** — Receive and apply update
6. **test_quota_conflict_max_wins** — Conflicting limits, MaxWins strategy
7. **test_quota_conflict_timestamp_wins** — Conflicting limits, TimestampWins strategy
8. **test_quota_conflict_admin_review** — Conflicting limits, AdminReview alert
9. **test_quota_sync_after_recovery** — Full state sync on partition heal
10. **test_quota_replication_idempotency** — Re-applying same request is idempotent
11. **test_quota_replication_lag** — Measure replication lag in ms
12. **test_quota_replication_partial_failure** — 1/2 tenants sync, 1/2 fail
13. **test_quota_replication_timeout** — Pending requests timeout after 60s
14. **test_quota_replication_concurrent_updates** — 5 concurrent updates to same tenant
15. **test_quota_replication_across_generations** — Updates span multiple generations
16. **test_quota_replication_metrics** — Verify metrics accuracy
17. **test_quota_replication_with_client_session** — Update quota mid-session
18. **test_quota_replication_persistence** — Replicated quotas survive node restart
19. **test_quota_replication_large_batch** — 100 tenants, single batch
20. **test_quota_replication_conflict_resolution_audit** — Conflicts logged to audit trail

---

## Block B: tenant_audit.rs (18 tests, 420 LOC)

**Goal:** Comprehensive audit logging for compliance (SOC2, HIPAA, GDPR, PCI-DSS).

### Data Structures

```rust
pub struct AuditEvent {
    pub event_id: String,             // UUID
    pub timestamp: u64,               // Unix epoch ms (UTC)
    pub event_type: AuditEventType,
    pub tenant_id: String,
    pub user_id: Option<String>,
    pub client_ip: Option<String>,
    pub action: AuditAction,          // Create/Read/Update/Delete/Admin
    pub resource_id: String,          // Inode / quota / ACL / config
    pub resource_type: String,        // file / directory / quota / tenant
    pub success: bool,
    pub error_code: Option<String>,
    pub duration_micros: u64,
    pub details: std::collections::HashMap<String, String>,
}

pub enum AuditEventType {
    QuotaModified,
    QuotaExceeded,
    IsolationViolationAttempt,
    ACLModified,
    TenantCreated,
    TenantDeleted,
    CapabilityGranted,
    CapabilityRevoked,
    DataAccess,           // Read file
    DataModified,         // Write file
    Metadata,             // stat, chmod, chown
    SessionCreated,
    SessionTerminated,
    AdminAction,
    SecurityEvent,        // Failed auth, etc
}

pub enum AuditAction {
    Create,
    Read,
    Update,
    Delete,
    Admin,
}

pub struct AuditLogger {
    events: Arc<DashMap<String, AuditEvent>>,  // event_id → event
    by_tenant: Arc<DashMap<String, Vec<String>>>, // tenant_id → [event_ids]
    by_resource: Arc<DashMap<String, Vec<String>>>, // resource_id → [event_ids]
    retention_days: u32,
    compliance_mode: ComplianceMode,
}

pub enum ComplianceMode {
    SOC2,                 // 90-day retention minimum
    HIPAA,                // 6-year retention, immutable
    GDPR,                 // 30-day retention, data subject rights
    PCI_DSS,              // 1-year retention, quarterly reviews
}

pub struct AuditReport {
    pub start_time: u64,
    pub end_time: u64,
    pub total_events: usize,
    pub events_by_type: std::collections::HashMap<String, usize>,
    pub failed_operations: Vec<(AuditEventType, String)>,
    pub data_accesses: usize,
    pub privilege_escalations: usize,
}
```

### Core Functions

1. **`log_event`** — Log an audit event
   - Generates event_id (UUID)
   - Timestamps (UTC)
   - Stores in events map + indexes
   - Enforces immutability (can't modify logged events)

2. **`log_quota_modification`** — Specialized: quota change
   - Captures old_limit, new_limit, reason
   - Logs as QuotaModified event
   - Links to tenant_id and quota_tracker

3. **`log_isolation_violation_attempt`** — Specialized: security event
   - Tenant A tried to access Tenant B's inode
   - Records both tenant_ids
   - Logs to security_events stream
   - Triggers alert if repeated (>3 in 5 min)

4. **`log_data_access`** — Specialized: file access (read)
   - Client accessed file (inode_id)
   - Captures file_path, user_id, client_ip
   - Records timestamp
   - May be high-volume (sample 10% by default)

5. **`query_events`** — Search audit log
   - Query by tenant_id, resource_id, time_range
   - Return matching events (up to 10K results)
   - Used for incident investigation

6. **`generate_compliance_report`** — Generate audit report
   - Count events by type
   - Detect anomalies (quota exceeds)
   - Check for unauthorized access attempts
   - Export as JSON/CSV for compliance officer
   - Supports SOC2, HIPAA, GDPR, PCI-DSS formats

7. **`export_for_dsar`** — Data Subject Access Request (GDPR)
   - Collect all events related to user_id
   - Export in portable format (JSON)
   - Include timestamps, actions, outcomes
   - Delete after 30-day request deadline

8. **`purge_expired_events`** — Cleanup per retention policy
   - Remove events older than retention_days
   - HIPAA: 6 years (never auto-purge, admin only)
   - GDPR: 30 days (auto-purge)
   - SOC2: 90 days (auto-purge)
   - Log deletion events themselves (immutable record)

### Test Coverage (18 tests)

1. **test_audit_log_quota_modification** — Capture quota change event
2. **test_audit_log_isolation_violation** — Log failed isolation check
3. **test_audit_log_data_access** — Log file read
4. **test_audit_log_data_modification** — Log file write
5. **test_audit_query_by_tenant** — Search all events for tenant
6. **test_audit_query_by_resource** — Search all events for file/quota
7. **test_audit_query_by_time_range** — Query events in date range
8. **test_audit_compliance_report_soc2** — Generate SOC2 report
9. **test_audit_compliance_report_hipaa** — Generate HIPAA report
10. **test_audit_compliance_report_gdpr** — Generate GDPR report
11. **test_audit_compliance_report_pci_dss** — Generate PCI-DSS report
12. **test_audit_export_for_dsar** — GDPR data subject access request
13. **test_audit_immutability** — Can't modify logged events
14. **test_audit_retention_policy_gdpr** — Events auto-purged after 30 days
15. **test_audit_retention_policy_hipaa** — Events never auto-purged (manual only)
16. **test_audit_anomaly_detection** — Detect quota exceed spike
17. **test_audit_repeated_violations** — Alert on >3 violations in 5 min
18. **test_audit_event_sampling** — High-volume data access sampled (10%)

---

## Block C: qos_cross_site.rs (16 tests, 380 LOC)

**Goal:** Global QoS coordination across sites with deadline propagation.

### Data Structures

```rust
pub struct GlobalQosPolicy {
    pub policy_id: String,
    pub tenant_id: String,
    pub site_a_sla_targets: SlaTargets,
    pub site_b_sla_targets: SlaTargets,
    pub failover_behavior: FailoverBehavior,  // Strict / Relaxed
    pub priority_remap: std::collections::HashMap<Priority, Priority>, // site_a → site_b mapping
}

pub struct SlaTargets {
    pub critical_p99_ms: u64,
    pub interactive_p99_ms: u64,
    pub bulk_p99_ms: u64,
}

pub enum FailoverBehavior {
    Strict,                           // Maintain exact SLAs (may reject ops)
    Relaxed,                          // Relax SLAs by 50% during failover
}

pub struct GlobalQosContext {
    pub request_id: String,
    pub tenant_id: String,
    pub priority: Priority,
    pub deadline_ms: u64,             // Absolute deadline (epoch ms)
    pub local_sla_ms: u64,            // Local site SLA
    pub remote_sla_ms: Option<u64>,   // Remote site SLA if coordinating
    pub site_aware: bool,             // True if operation crosses sites
    pub replication_level: u8,        // 1-2 (single site or replicated)
}

pub struct CrossSiteQosMetrics {
    pub requests_sent_remote: u64,
    pub requests_denied_remote: u64,
    pub deadline_violations: u64,
    pub failover_activations: u64,
    pub sla_attainment: f64,          // 0.0-1.0
}

pub struct FailoverEvent {
    pub timestamp: u64,
    pub from_site: String,
    pub to_site: String,
    pub reason: FailoverReason,
    pub sla_adjustments: std::collections::HashMap<Priority, (u64, u64)>, // (old_ms, new_ms)
}

pub enum FailoverReason {
    SiteUnreachable,
    HighLatency,
    ResourceExhausted,
    AdminTriggered,
}
```

### Core Functions

1. **`get_global_qos_policy`** — Fetch policy for tenant
   - Includes SLA targets for both sites
   - Failover behavior
   - Priority remapping

2. **`compute_global_deadline`** — Compute end-to-end deadline
   - Input: local SLA target (e.g., 50ms for Interactive)
   - Output: absolute deadline timestamp
   - If operation is replicated, add network latency (subtract 5-10ms for network)
   - Return GlobalQosContext

3. **`admit_cross_site_operation`** — Admission control for cross-site operations
   - Check if remote site can meet deadline
   - Call A4 RPC: can_serve_within_deadline(deadline_ms)?
   - If no: reject or escalate to Bulk priority
   - If yes: return request_id for tracking

4. **`send_qos_hint_to_remote`** — Propagate deadline to remote site
   - Encapsulate deadline_ms + priority
   - Send via A6 conduit (gRPC)
   - Remote qos_coordinator adjusts bandwidth_shaper priority

5. **`handle_failover_to_remote_site`** — Adjust SLAs during failover
   - Detect loss of primary site
   - Activate failover_behavior (Strict / Relaxed)
   - If Relaxed: multiply SLAs by 1.5x
   - Log FailoverEvent with adjustments
   - Update metrics.failover_activations

6. **`compute_sla_attainment`** — Periodic SLA compliance metric
   - Count ops that met deadline vs total ops
   - Return percentage 0.0-1.0
   - Report to Prometheus (A8)

7. **`query_cross_site_delays`** — Measure network latency
   - Ping remote site (gRPC heartbeat)
   - Track p99 latency
   - Feed into deadline calculation
   - Cache for 10s

### Test Coverage (16 tests)

1. **test_global_qos_policy_fetch** — Load policy for tenant
2. **test_compute_global_deadline_single_site** — Single-site SLA + no latency
3. **test_compute_global_deadline_replicated** — Cross-site operation, account for latency
4. **test_admit_cross_site_operation_success** — Remote site has capacity
5. **test_admit_cross_site_operation_denied** — Remote site overloaded
6. **test_admit_cross_site_operation_escalate_to_bulk** — Escalate from Interactive to Bulk
7. **test_send_qos_hint_to_remote** — Propagate deadline to remote A4
8. **test_handle_failover_strict** — Primary down, activate Strict (maintain SLAs)
9. **test_handle_failover_relaxed** — Primary down, activate Relaxed (1.5x SLAs)
10. **test_failover_sla_adjustment_metrics** — Track SLA adjustments in metrics
11. **test_compute_sla_attainment_perfect** — 100% of ops meet deadline
12. **test_compute_sla_attainment_degraded** — 70% of ops meet deadline
13. **test_query_cross_site_latency** — Measure network latency
14. **test_qos_priority_remap_across_sites** — Critical at site A → Interactive at site B
15. **test_qos_context_serialization** — GlobalQosContext serializes via bincode
16. **test_cross_site_qos_metrics_accuracy** — Verify metric calculations

---

## Block D: billing_aggregator.rs (15 tests, 350 LOC)

**Goal:** Cross-tenant billing aggregation with metering and chargeback.

### Data Structures

```rust
pub struct BillingRecord {
    pub record_id: String,            // UUID
    pub tenant_id: String,
    pub time_window: (u64, u64),      // (start_ms, end_ms) — 1-hour window
    pub storage_gb_hours: f64,
    pub iops_count: u64,
    pub network_gb: f64,
    pub snapshot_gb: f64,
    pub cost_usd: f64,
    pub cost_breakdown: CostBreakdown,
    pub status: BillingStatus,        // Pending / Finalized / Disputed
}

pub struct CostBreakdown {
    pub storage_cost: f64,            // $/GB-hour
    pub iops_cost: f64,               // $/1M IOPS
    pub network_cost: f64,            // $/GB
    pub snapshot_cost: f64,           // $/GB-hour
    pub taxes: f64,
    pub discounts: f64,
}

pub enum BillingStatus {
    Pending,
    Finalized,
    Disputed(String),                 // reason
    Paid,
}

pub struct BillingAggregator {
    pub hourly_records: Arc<DashMap<String, BillingRecord>>, // "tenant-time" → record
    pub pricing: BillingPricing,
    pub exchange_rates: Arc<DashMap<String, f64>>, // Currency → USD
}

pub struct BillingPricing {
    pub storage_gb_hour_usd: f64,     // e.g., $0.0001
    pub iops_million_usd: f64,        // e.g., $5.00
    pub network_gb_usd: f64,          // e.g., $0.10
    pub snapshot_gb_hour_usd: f64,    // e.g., $0.00002
    pub volume_discount_threshold: u32, // Apply discount after N GB
    pub volume_discount_pct: f64,     // e.g., 10% discount
}

pub struct BillingReport {
    pub start_time: u64,
    pub end_time: u64,
    pub total_tenants_billed: usize,
    pub total_revenue_usd: f64,
    pub revenue_by_service: std::collections::HashMap<String, f64>,
    pub top_10_tenants: Vec<(String, f64)>,
    pub disputed_records: usize,
}
```

### Core Functions

1. **`record_usage`** — Ingest usage metrics (called by metrics module)
   - Tenant ID, storage bytes, IOPS count, network bytes, snapshots
   - Accumulate into 1-hour window buckets
   - Store as pending

2. **`finalize_billing_window`** — Close hourly window and calculate charges
   - For all pending records in [T, T+1h):
     - Fetch quota_tracker to check tenant quotas
     - Fetch qos_metrics to count IOPS/latency
     - Fetch replication metrics to measure network
     - Calculate costs with breakdown
     - Apply volume discounts if applicable
     - Apply tenant-specific pricing (negotiated rates)
   - Mark as Finalized
   - Send to ledger

3. **`apply_volume_discount`** — Compute discount for high-volume tenant
   - If storage > threshold, apply discount_pct
   - Return adjusted cost

4. **`apply_negotiated_pricing`** — Override standard pricing for tenant
   - Tenant-specific pricing agreement
   - Applied before discount
   - Logged as pricing override

5. **`query_tenant_bill`** — Fetch bill for tenant in time range
   - Sum all BillingRecords for tenant in range
   - Return total cost with breakdown
   - Include disputable items

6. **`dispute_billing_record`** — Tenant contests a charge
   - Mark record as Disputed(reason)
   - Create ticket in audit trail
   - Admin can review and adjust

7. **`generate_monthly_invoice`** — Create invoice for customer
   - Aggregate all 1-hour records
   - Apply taxes (varies by region)
   - Add payment terms (net-30)
   - Export as PDF/JSON

8. **`compute_mrr`** — Monthly Recurring Revenue forecast
   - Extrapolate current usage to full month
   - Return forecast in USD

### Test Coverage (15 tests)

1. **test_billing_record_storage** — Charge per GB-hour
2. **test_billing_record_iops** — Charge per 1M IOPS
3. **test_billing_record_network** — Charge per GB egress
4. **test_billing_record_snapshot** — Charge per snapshot GB-hour
5. **test_billing_finalize_window** — Close hourly window, calculate costs
6. **test_billing_volume_discount** — Apply 10% discount for >100GB
7. **test_billing_negotiated_pricing** — Custom pricing agreement
8. **test_billing_cost_breakdown** — Verify cost breakdown accuracy
9. **test_billing_dispute_record** — Tenant disputes charge
10. **test_billing_query_tenant_bill** — Fetch total bill for tenant/period
11. **test_billing_monthly_invoice** — Generate monthly invoice
12. **test_billing_tax_calculation** — Calculate regional taxes
13. **test_billing_mrr_forecast** — Compute MRR from current usage
14. **test_billing_multiple_tenants** — Bill 10 tenants in same window
15. **test_billing_currency_conversion** — Convert pricing to EUR, GBP, JPY

---

## Optional: capacity_planner.rs (8 tests, 200 LOC)

**Goal:** Predict storage/IOPS needs and recommend scaling.

### Data Structures

```rust
pub struct CapacityPlan {
    pub tenant_id: String,
    pub current_storage_gb: u64,
    pub projected_storage_gb_30d: u64,
    pub current_iops: u64,
    pub projected_iops_30d: u64,
    pub growth_rate_pct: f64,
    pub recommendation: CapacityRecommendation,
    pub confidence: f64,              // 0.0-1.0
}

pub enum CapacityRecommendation {
    NoAction,
    WarningThresholdReached,          // >80% utilized
    ScaleUp,
    ScaleDown,
    NegotiateHigherQuota,
}

pub struct CapacityTrend {
    pub datapoints: Vec<(u64, u64)>,  // (timestamp, usage_bytes)
    pub regression_slope: f64,        // Linear regression
    pub seasonal_factor: f64,         // Cyclic pattern
}
```

### Core Functions

1. **`analyze_usage_trend`** — Compute growth rate
2. **`generate_capacity_plan`** — Recommend scaling action
3. **`query_capacity_forecast`** — Get 30/90-day projection
4. **`alert_quota_approaching`** — Warn tenant at 80%

### Test Coverage (8 tests)

1. **test_capacity_analyze_linear_growth**
2. **test_capacity_analyze_seasonal_pattern**
3. **test_capacity_forecast_30d**
4. **test_capacity_recommendation_scale_up**
5. **test_capacity_recommendation_scale_down**
6. **test_capacity_confidence_high**
7. **test_capacity_confidence_low**
8. **test_capacity_alert_threshold_reached**

---

## Implementation Schedule

### Block A: quota_replication.rs (Day 1)
- **Input:** a2-phase11-block-a-input.md (quota replication spec)
- **Output:** quota_replication.rs (450 LOC, 20 tests)
- **OpenCode:** minimax-m2p5
- **Timeline:** 2-3 hours

### Block B: tenant_audit.rs (Day 1-2)
- **Input:** a2-phase11-block-b-input.md (audit logging spec)
- **Output:** tenant_audit.rs (420 LOC, 18 tests)
- **OpenCode:** minimax-m2p5
- **Timeline:** 2-3 hours

### Block C: qos_cross_site.rs (Day 2)
- **Input:** a2-phase11-block-c-input.md (cross-site QoS spec)
- **Output:** qos_cross_site.rs (380 LOC, 16 tests)
- **OpenCode:** minimax-m2p5
- **Timeline:** 2-3 hours

### Block D: billing_aggregator.rs (Day 2-3)
- **Input:** a2-phase11-block-d-input.md (billing spec)
- **Output:** billing_aggregator.rs (350 LOC, 15 tests)
- **OpenCode:** minimax-m2p5
- **Timeline:** 2-3 hours

### Optional Block E: capacity_planner.rs (Day 3)
- **Input:** a2-phase11-block-e-input.md (capacity planning spec)
- **Output:** capacity_planner.rs (200 LOC, 8 tests)
- **OpenCode:** minimax-m2p5
- **Timeline:** 1-2 hours

**Total:** 1,800 LOC, 69-77 tests, 3-4 days

---

## Success Criteria

✅ All 4 modules (A-D) compile without warnings
✅ 69+ new tests, all passing (1,100+ total)
✅ `cargo test --lib -p claudefs-meta` succeeds
✅ `cargo clippy -p claudefs-meta` zero warnings
✅ All modules exported in lib.rs pub use section
✅ lib.rs compiles (no orphaned modules)
✅ CHANGELOG updated with Phase 11 summary
✅ All commits pushed to main with [A2] prefix
✅ Zero regressions in Phase 10 tests

---

## Dependencies

**A2 ← A6:**
- quota_replication.rs uses journal_tailer for cross-site sync
- tenant_audit.rs may log replication events

**A2 ← A4:**
- qos_cross_site.rs calls A4 bandwidth_shaper RPC
- Deadline propagation via transport layer

**A2 ← A8:**
- billing_aggregator.rs reads metrics from A8
- Integrates with Prometheus exporter

**A2 → A5:**
- Client sessions may trigger audit events

**A2 → A11:**
- Capacity planner output feeds into infrastructure scaling

---

## Architectural Patterns

**Thread Safety:**
- DashMap for lock-free reads (events, records)
- Arc<Mutex> only for hot counters (conflicts, requests_sent)

**Error Handling:**
- New MetaError variants: ReplicationFailed, AuditFailed, BillingCalculationFailed
- Propagate via thiserror

**Async:**
- All network operations return Future
- tokio::spawn for background finalization
- No blocking I/O in critical path

**Testing:**
- Unit tests in each module
- proptest for pricing edge cases
- Integration tests for cross-module interactions

---

## Deliverables

- **4 new source files:** quota_replication.rs, tenant_audit.rs, qos_cross_site.rs, billing_aggregator.rs
- **Optional:** capacity_planner.rs (8 tests)
- **lib.rs:** Updated with pub mod and pub use statements
- **Updated CHANGELOG:** Phase 11 summary with test count
- **GitHub commits:** 4-5 commits, all pushed

---

## Notes

1. **Conflict detection:** quota_replication uses Lamport clocks for ordering, not vector clocks (simpler for quotas)
2. **Audit immutability:** Once logged, events cannot be modified or deleted (except per compliance policy purge)
3. **Billing windows:** 1-hour windows align with Prometheus scrape intervals (A8)
4. **Cross-site RPC:** All remote calls use existing A6 journal_tailer infrastructure (no new transport channels)
5. **Compliance modes:** SOC2/HIPAA/GDPR/PCI-DSS all supported; toggled at cluster creation
6. **Capacity planner:** Optional; can be implemented in Phase 12 if needed

---

## Open Questions

- **Billing currency:** USD primary, support EUR/GBP/JPY? (Answer: Yes, in Block D)
- **Audit sampling:** Always log data access or sample 10%? (Answer: Sample 10% by default, configurable)
- **Admin review:** Who approves disputed charges? (Answer: Admin user (via future RBAC), defaults to max-wins)
- **Retention enforcement:** Automatic purge or only on explicit admin command? (Answer: Auto-purge per compliance mode)

---

**Next Steps:** Prepare Block A OpenCode prompt (a2-phase11-block-a-input.md) and kick off implementation.
