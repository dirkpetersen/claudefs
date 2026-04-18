# A2 Phase 11 Block B: tenant_audit.rs Implementation

**Crate:** claudefs-meta
**File:** crates/claudefs-meta/src/tenant_audit.rs
**Target:** 420 LOC, 18 tests, 0 warnings
**Model:** minimax-m2p5

---

## Context

Building on Phase 11 Block A (quota_replication), Block B implements comprehensive audit logging for compliance.

**Compliance Standards:**
- **SOC2:** 90-day minimum retention, quarterly reviews
- **HIPAA:** 6-year retention, immutable, no automatic deletion
- **GDPR:** 30-day retention maximum, data subject access rights (DSAR)
- **PCI-DSS:** 1-year retention, monthly access reviews

---

## Requirements

### 1. Core Data Structures

```rust
/// Audit event with immutable logged data
#[derive(Clone, Debug)]
pub struct AuditEvent {
    /// Unique event ID (UUID string)
    pub event_id: String,
    /// Unix epoch ms (UTC) when event occurred
    pub timestamp: u64,
    /// Type of event (quota, acl, access, admin, etc.)
    pub event_type: AuditEventType,
    /// Tenant ID involved in event
    pub tenant_id: String,
    /// Optional: user ID performing action
    pub user_id: Option<String>,
    /// Optional: client IP address
    pub client_ip: Option<String>,
    /// Action performed (Create/Read/Update/Delete/Admin)
    pub action: AuditAction,
    /// Resource being acted on (inode ID, quota ID, etc.)
    pub resource_id: String,
    /// Type of resource (file, directory, quota, acl, tenant)
    pub resource_type: String,
    /// Whether the operation succeeded
    pub success: bool,
    /// Optional error code if failed
    pub error_code: Option<String>,
    /// Duration in microseconds
    pub duration_micros: u64,
    /// Additional details (tenant-specific, context)
    pub details: std::collections::HashMap<String, String>,
}

/// Type of audit event
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditEventType {
    /// Quota limit modified
    QuotaModified,
    /// Quota limit exceeded
    QuotaExceeded,
    /// Attempt to access another tenant's resource
    IsolationViolationAttempt,
    /// Access control list modified
    AclModified,
    /// New tenant created
    TenantCreated,
    /// Tenant deleted
    TenantDeleted,
    /// Capability granted to principal
    CapabilityGranted,
    /// Capability revoked from principal
    CapabilityRevoked,
    /// File/directory read
    DataAccess,
    /// File/directory write
    DataModified,
    /// Metadata changed (chmod, chown, stat, etc.)
    Metadata,
    /// Client session created
    SessionCreated,
    /// Client session terminated
    SessionTerminated,
    /// Administrative action (user add/remove, node rebalance, etc.)
    AdminAction,
    /// Security event (failed auth, suspicious activity)
    SecurityEvent,
}

/// Action performed
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditAction {
    /// Resource created
    Create,
    /// Resource read
    Read,
    /// Resource updated
    Update,
    /// Resource deleted
    Delete,
    /// Administrative action
    Admin,
}

/// Compliance mode (retention policy)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComplianceMode {
    /// SOC2: 90 days minimum
    SOC2,
    /// HIPAA: 6 years, never auto-delete
    HIPAA,
    /// GDPR: 30 days maximum, data subject rights
    GDPR,
    /// PCI-DSS: 1 year, quarterly reviews
    PCI_DSS,
}

/// Audit logger (holds all audit state)
pub struct AuditLogger {
    /// All events: event_id → event
    pub events: Arc<DashMap<String, AuditEvent>>,
    /// Index: tenant_id → [event_ids]
    pub by_tenant: Arc<DashMap<String, Vec<String>>>,
    /// Index: resource_id → [event_ids]
    pub by_resource: Arc<DashMap<String, Vec<String>>>,
    /// Retention days for this compliance mode
    pub retention_days: u32,
    /// Current compliance mode
    pub compliance_mode: ComplianceMode,
    /// High-volume data access sample rate (e.g., 0.1 for 10%)
    pub data_access_sample_rate: f64,
}

/// Compliance report for auditors
#[derive(Clone, Debug)]
pub struct AuditReport {
    /// Report start time (Unix epoch ms)
    pub start_time: u64,
    /// Report end time (Unix epoch ms)
    pub end_time: u64,
    /// Total events in report period
    pub total_events: usize,
    /// Event counts by type
    pub events_by_type: std::collections::HashMap<String, usize>,
    /// Failed operations (type, error)
    pub failed_operations: Vec<(String, String)>,
    /// Total data access events
    pub data_accesses: usize,
    /// Total privilege escalations/grants
    pub privilege_escalations: usize,
    /// Anomalies detected
    pub anomalies: Vec<String>,
}

/// Import QuotaType from Phase 10
pub use crate::quota_tracker::QuotaType;
```

---

### 2. Public Functions

#### 2.1 `new` (Constructor)

```rust
pub fn new(compliance_mode: ComplianceMode, data_access_sample_rate: f64) -> Self
```

**Behavior:**
- Create new AuditLogger with empty event maps
- Set retention_days based on compliance_mode:
  - SOC2 → 90
  - HIPAA → 2190 (6 years)
  - GDPR → 30
  - PCI_DSS → 365

- Set sample rate (0.0-1.0)
- Return initialized AuditLogger

---

#### 2.2 `log_event`

```rust
pub fn log_event(&self, event: AuditEvent) -> Result<String, String>
```

**Behavior:**
- Accept AuditEvent (event_id already set by caller)
- Validate: tenant_id non-empty, timestamp reasonable (not in future)
- Insert into events map
- Add event_id to by_tenant[tenant_id] index
- Add event_id to by_resource[resource_id] index
- Return event_id (or error if validation failed)
- **Events are immutable:** cannot be modified after logging

---

#### 2.3 `log_quota_modification`

```rust
pub fn log_quota_modification(
    &self,
    tenant_id: &str,
    quota_type: QuotaType,
    old_limit: u64,
    new_limit: u64,
    reason: &str,
) -> Result<String, String>
```

**Behavior:**
- Create AuditEvent with:
  - event_type = QuotaModified
  - action = Update
  - resource_id = format!("{tenant_id}:{quota_type:?}")
  - resource_type = "quota"
  - details: { "old_limit": old_limit, "new_limit": new_limit, "reason": reason }
  - success = true
  - timestamp = current Unix epoch ms

- Call log_event with the created event
- Return event_id

---

#### 2.4 `log_isolation_violation_attempt`

```rust
pub fn log_isolation_violation_attempt(
    &self,
    accessing_tenant_id: &str,
    target_tenant_id: &str,
    resource_id: &str,
    user_id: Option<&str>,
) -> Result<String, String>
```

**Behavior:**
- Create AuditEvent with:
  - event_type = IsolationViolationAttempt
  - action = Read
  - resource_id (provided)
  - resource_type = "tenant_namespace"
  - success = false
  - error_code = Some("ISOLATION_VIOLATION")
  - details: { "accessing_tenant": accessing_tenant_id, "target_tenant": target_tenant_id }
  - user_id (if provided)
  - timestamp = current Unix epoch ms

- Call log_event
- Check if this tenant has >3 violations in last 5 minutes
  - If yes: log Alert with details
  - Update metrics.security_alerts

- Return event_id

---

#### 2.5 `log_data_access`

```rust
pub fn log_data_access(
    &self,
    tenant_id: &str,
    inode_id: &str,
    file_path: &str,
    user_id: Option<&str>,
    client_ip: Option<&str>,
) -> Result<String, String>
```

**Behavior:**
- Check if should sample: generate random 0.0-1.0, sample if <= data_access_sample_rate
  - If not sampling: return Ok("sampled".to_string()) without logging

- Create AuditEvent with:
  - event_type = DataAccess
  - action = Read
  - resource_id = inode_id
  - resource_type = "file"
  - success = true
  - details: { "file_path": file_path }
  - user_id, client_ip (if provided)
  - timestamp = current Unix epoch ms

- Call log_event
- Return event_id

---

#### 2.6 `query_events`

```rust
pub fn query_events(
    &self,
    tenant_id: Option<&str>,
    resource_id: Option<&str>,
    start_time_ms: u64,
    end_time_ms: u64,
) -> Vec<AuditEvent>
```

**Behavior:**
- If tenant_id provided: fetch all event_ids from by_tenant[tenant_id]
  - Filter by time range (start_time_ms ≤ timestamp ≤ end_time_ms)
  - Fetch events from events map
  - Return (max 10,000 results)

- If resource_id provided: fetch all event_ids from by_resource[resource_id]
  - Same time filtering and fetch

- If neither: iterate all events (expensive), filter by time
  - Return max 10,000

---

#### 2.7 `generate_compliance_report`

```rust
pub fn generate_compliance_report(
    &self,
    start_time_ms: u64,
    end_time_ms: u64,
) -> AuditReport
```

**Behavior:**
- Query all events in time range
- Count by event_type (map)
- Count data accesses (DataAccess type)
- Count privilege escalations (CapabilityGranted type)
- Count failed ops (success == false)
- Detect anomalies:
  - >10 IsolationViolationAttempt in 1 hour → add "Isolation attacks detected"
  - >5 QuotaExceeded in 1 hour → add "Quota spike detected"

- Return AuditReport with all counts

---

#### 2.8 `export_for_dsar`

```rust
pub fn export_for_dsar(
    &self,
    user_id: &str,
    to_file: &str,
) -> Result<usize, String>
```

**Behavior:**
- Query all events where user_id matches (or mentioned in details)
- Serialize to JSON (Pretty format)
- Write to file
- Log export event itself to audit trail
- Return count of events exported

**GDPR Compliance:**
- Set auto-delete reminder for 30 days
- User has right to export their data
- Include all personal data associated with user_id

---

#### 2.9 `purge_expired_events`

```rust
pub fn purge_expired_events(&self) -> usize
```

**Behavior:**
- Compute expiry_time_ms = now - (retention_days * 86400 * 1000)
- Iterate all events
- If event.timestamp < expiry_time_ms:
  - **HIPAA mode:** Never auto-purge; only manual admin purge
    - Log "Attempted auto-purge in HIPAA mode (rejected)"
    - Skip
  - Other modes: Remove from events, by_tenant, by_resource
  - Log deletion itself as an AuditEvent (event_type = AdminAction, action = Delete, resource_type = "audit_event")

- Return count of purged events

---

#### 2.10 `get_mode`

```rust
pub fn get_mode(&self) -> ComplianceMode
```

**Behavior:**
- Return current compliance_mode

---

### 3. Thread Safety

- **DashMap** for events, by_tenant, by_resource: lock-free reads, safe iteration
- **No async required** for logging (synchronous)
- **Sampling:** Use thread-local or atomic random number generator

---

### 4. Error Handling

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuditError {
    #[error("event validation failed: {0}")]
    ValidationFailed(String),

    #[error("file I/O error: {0}")]
    FileIoError(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}
```

---

### 5. Tests (18 total)

1. **test_audit_log_quota_modification** — Log quota change
2. **test_audit_log_isolation_violation** — Log failed isolation check
3. **test_audit_log_data_access** — Log file read
4. **test_audit_log_data_modification** — Log file write
5. **test_audit_query_by_tenant** — Query events for tenant
6. **test_audit_query_by_resource** — Query events for resource
7. **test_audit_query_by_time_range** — Query events in date range
8. **test_audit_compliance_report_soc2** — Generate SOC2 report
9. **test_audit_compliance_report_hipaa** — Generate HIPAA report
10. **test_audit_compliance_report_gdpr** — Generate GDPR report
11. **test_audit_compliance_report_pci_dss** — Generate PCI-DSS report
12. **test_audit_export_for_dsar** — GDPR DSAR export
13. **test_audit_immutability** — Can't modify logged events
14. **test_audit_retention_policy_gdpr** — Auto-purge after 30 days
15. **test_audit_retention_policy_hipaa** — Never auto-purge in HIPAA mode
16. **test_audit_anomaly_detection** — Detect quota/isolation spikes
17. **test_audit_repeated_violations_alert** — Alert on >3 violations in 5 min
18. **test_audit_data_access_sampling** — 10% of data accesses logged

---

## Deliverable

**Output file:** `crates/claudefs-meta/src/tenant_audit.rs`

**Specifications:**
- 400-450 LOC (implementation + tests)
- 18 unit tests
- Zero warnings
- Imports: std::*, dashmap::*, serde_json, thiserror, uuid

---

## Integration Notes

- Works with Phase 11 Block A: log conflicts to audit
- Works with Phase 10 quota_tracker: log quota modifications
- Works with Phase 10 tenant_isolator: log isolation violations
- Output feeds into A8 Prometheus for metrics
- GDPR compliance: 30-day retention, DSAR export capability

**Expected integration time:** 1-2 hours total (planning + coding + testing)
