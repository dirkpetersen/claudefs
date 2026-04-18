# A5 FUSE Phase 38 Block 1: Configuration Management — Implementation for OpenCode

**Priority:** HIGH — Foundation for all Phase 38 blocks
**OpenCode Model:** minimax-m2p5
**Deliverables:** 6 modules (~2650 lines), 25 unit tests

---

## Executive Summary

Implement the configuration management framework for FUSE client. This enables zero-downtime policy updates (QoS, WORM, Quota) across the cluster while maintaining consistency and providing audit trails.

**Key Objectives:**
1. ✅ Config versioning & TTL-based caching
2. ✅ HTTP REST API for policy updates
3. ✅ Persistence to A2 Metadata KV store
4. ✅ Cluster-wide replication of policy changes
5. ✅ Config snapshots for debugging/recovery
6. ✅ Comprehensive validation & merging strategies

---

## Crate Context

**Crate:** `claudefs-fuse`
**Location:** `crates/claudefs-fuse/src/`
**Dependencies:**
- `tokio` (async runtime)
- `thiserror` (error handling)
- `serde` + `serde_json` (serialization)
- `dashmap` (concurrent hashmap)
- `tracing` (logging)
- Internal: Uses existing `error.rs`, `metrics.rs` from FUSE crate

---

## Architecture

### Configuration Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    A2 Metadata Service                        │
│              (PolicyEpoch KV store at /config/*) │
└────────────────┬──────────────────────────────────┘
                 │ A4 RPC: ConfigUpdate notifications
                 │
        ┌────────▼───────────────────────────────┐
        │   ConfigManager (in-memory cache)      │
        │ - TTL-based expiration (300s default)  │
        │ - Epoch tracking for consistency       │
        │ - Async update listener                │
        └────────┬───────────────────────────────┘
                 │
        ┌────────▼───────────────────────────────┐
        │  Policy Readers (lock-free read path)  │
        │ - QoS policies per tenant              │
        │ - WORM policies per path               │
        │ - Quota policies per user/group        │
        │ - Tracing config (sampling rate, etc)  │
        │ - Session config (timeouts, etc)       │
        └────────┬───────────────────────────────┘
                 │
        ┌────────▼───────────────────────────────┐
        │     HTTP REST API (Axum handler)       │
        │ - POST /config/qos/{tenant_id}         │
        │ - POST /config/worm/{path_prefix}      │
        │ - POST /config/quota/{user_or_group}   │
        │ - GET /config/current                  │
        │ - DELETE /config/override/{policy_id}  │
        └────────────────────────────────────────┘
```

### Module Responsibilities

| Module | Size | Purpose |
|--------|------|---------|
| `config_manager.rs` | 600 | PolicyEpoch versioning, TTL cache, merging, update broadcast |
| `config_validator.rs` | 400 | Validate limits, check conflicts, version policies |
| `config_http_api.rs` | 350 | REST endpoints (POST/GET/DELETE), auth, request/response |
| `config_storage.rs` | 300 | Persist to A2 KV, audit trail, versioning |
| `config_replication.rs` | 250 | Broadcast changes to cluster nodes via A4 |
| `config_snapshot.rs` | 250 | Point-in-time config snapshots for debugging |
| **Total** | 2150 | + shared test utilities |

---

## Module Specifications

### 1. config_manager.rs (600 lines)

**Purpose:** Core configuration management with versioning, caching, and merging.

**Key Types:**

```rust
/// Versioned configuration with epoch number
#[derive(Clone, Debug)]
pub struct PolicyEpoch {
    pub epoch: u64,                    // Version number (incremented on update)
    pub updated_at_ns: i64,           // Wall-clock time (ns since epoch)
    pub updated_by: String,           // User ID of updater
    pub qos_policies: HashMap<String, QoSPolicy>,        // tenant_id -> policy
    pub worm_policies: HashMap<String, WORMPolicy>,      // path_prefix -> policy
    pub quota_policies: HashMap<String, QuotaPolicy>,    // user_or_group_id -> policy
    pub tracing_config: TracingConfig,
    pub session_config: SessionConfig,
}

/// In-memory config cache with TTL and epoch tracking
pub struct ConfigCache {
    current: Arc<RwLock<PolicyEpoch>>,
    local_overrides: Arc<DashMap<String, serde_json::Value>>, // key = policy_id
    update_listeners: Arc<tokio::sync::broadcast::Sender<ConfigUpdateEvent>>,
    last_refresh_ns: Arc<AtomicU64>,
    cache_ttl_ms: u64,
    metadata_client: Arc<MetadataClient>, // A2 access
}

pub enum ConfigUpdateEvent {
    QoSUpdated { tenant_id: String, epoch: u64 },
    WORMUpdated { path: String, epoch: u64 },
    QuotaUpdated { user_id: String, epoch: u64 },
    FullRefresh { epoch: u64 },
}

pub enum MergeStrategy {
    Replace,       // Old policy fully replaced by new
    Union,         // Union of old + new (e.g., legal holds)
    Priority(u32), // Newer takes priority if within N seconds
}
```

**Public API:**

```rust
impl ConfigCache {
    /// Create cache with A2 metadata client
    pub fn new(metadata_client: Arc<MetadataClient>, cache_ttl_ms: u64) -> Self;

    /// Get current config (may trigger refresh if TTL expired)
    pub async fn get_current(&self) -> Arc<PolicyEpoch>;

    /// Get single QoS policy
    pub async fn get_qos_policy(&self, tenant_id: &str) -> Option<QoSPolicy>;

    /// Update QoS policy, broadcast to cluster
    pub async fn set_qos_policy(
        &self,
        tenant_id: &str,
        policy: QoSPolicy,
        updated_by: &str,
        merge_strategy: MergeStrategy,
    ) -> Result<u64, ConfigError>; // Returns new epoch

    /// Update WORM policy
    pub async fn set_worm_policy(
        &self,
        path: &str,
        policy: WORMPolicy,
        updated_by: &str,
        merge_strategy: MergeStrategy,
    ) -> Result<u64, ConfigError>;

    /// Update quota policy
    pub async fn set_quota_policy(
        &self,
        user_id: &str,
        policy: QuotaPolicy,
        updated_by: &str,
        merge_strategy: MergeStrategy,
    ) -> Result<u64, ConfigError>;

    /// Subscribe to config updates
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ConfigUpdateEvent>;

    /// Manual refresh (useful after network partition heals)
    pub async fn refresh_from_metadata(&self) -> Result<u64, ConfigError>;

    /// Apply local override (temporary, not persisted)
    pub fn set_local_override(&self, policy_id: String, value: serde_json::Value);

    /// Clear local override
    pub fn clear_local_override(&self, policy_id: &str);
}
```

**Implementation Details:**

- **Cache TTL:** 300 seconds (configurable)
- **Epoch:** Incremented on each successful update
- **Atomic updates:** Use Arc<RwLock> for atomic policy swaps
- **Merge logic:** Union for legal holds, replace for rate limits
- **Fallback:** Return last-known-good config if metadata unreachable
- **Broadcast:** Use Tokio broadcast channel for in-process listeners

**Tests (6):**
1. `test_config_loading_from_metadata` — Load PolicyEpoch from A2
2. `test_config_cache_expiration` — TTL-based refresh triggers
3. `test_config_merge_strategies` — Union/Replace/Priority merging
4. `test_atomic_config_swap` — Concurrent reads during update
5. `test_policy_update_listener` — Broadcast receiver pattern
6. `test_fallback_on_metadata_unavailable` — Last-known-good fallback

---

### 2. config_validator.rs (400 lines)

**Purpose:** Validate policies before persistence to avoid invalid state.

**Key Functions:**

```rust
pub struct PolicyValidator {
    max_qos_rate_bps: u64,           // Cluster-wide limit
    max_iops: u64,
    max_retention_days: u64,
    max_quota_bytes: u64,            // Per user
}

pub enum ValidationError {
    QoSRateTooHigh { requested: u64, max: u64 },
    IopsTooHigh { requested: u64, max: u64 },
    RetentionInvalid { reason: String },
    QuotaExceedsCapacity { quota: u64, available: u64 },
    WORMConflict { reason: String },
    PolicyVersionMismatch { expected: u64, actual: u64 },
}

impl PolicyValidator {
    pub fn new(
        max_qos_rate_bps: u64,
        max_iops: u64,
        max_retention_days: u64,
        max_quota_bytes: u64,
    ) -> Self;

    /// Validate QoS policy
    pub fn validate_qos_policy(&self, policy: &QoSPolicy) -> Result<(), ValidationError>;

    /// Validate WORM policy
    pub fn validate_worm_policy(&self, policy: &WORMPolicy) -> Result<(), ValidationError>;

    /// Validate quota policy
    pub fn validate_quota_policy(&self, policy: &QuotaPolicy) -> Result<(), ValidationError>;

    /// Check for conflicting holds (e.g., retention < legal hold)
    pub fn check_worm_consistency(
        &self,
        current: &WORMPolicy,
        update: &WORMPolicy,
    ) -> Result<(), ValidationError>;

    /// Check total quota doesn't exceed cluster storage
    pub async fn check_quota_capacity(
        &self,
        user_id: &str,
        new_limit: u64,
        metadata_client: &MetadataClient,
    ) -> Result<(), ValidationError>;
}
```

**Validation Rules:**
- QoS rates: positive, <= max_qos_rate_bps
- WORM retention: valid duration, no reduce on existing policy
- Quota: positive, sum of all quotas <= cluster capacity
- Conflicts: retain > hold for legal holds

**Tests (4):**
1. `test_qos_rate_limits` — Validate rate bounds
2. `test_worm_retention_validity` — Retention policy checks
3. `test_quota_consistency` — Quota sum validation
4. `test_policy_conflict_detection` — Detect conflicting policies

---

### 3. config_http_api.rs (350 lines)

**Purpose:** REST endpoints for policy updates and inspection.

**Endpoints:**

```rust
// POST /config/qos/{tenant_id}
// Body: { read_bps_limit: u64, write_bps_limit: u64, iops_limit: u64, burst_duration_ms: u32 }
// Response: { epoch: u64, updated_at_ns: i64 }
pub async fn update_qos_policy(
    config_cache: Arc<ConfigCache>,
    tenant_id: String,
    policy: Json<QoSPolicyRequest>,
    user_id: UserId, // From mTLS cert
) -> Result<Json<serde_json::Value>, StatusCode>;

// GET /config/current
// Response: { epoch, qos_policies, worm_policies, quota_policies, ... }
pub async fn get_current_config(
    config_cache: Arc<ConfigCache>,
) -> Result<Json<PolicyEpoch>, StatusCode>;

// POST /config/worm/{path_prefix}
// Body: { retention_days: u64, legal_holds: [...] }
// Response: { epoch: u64, updated_at_ns: i64 }
pub async fn update_worm_policy(
    config_cache: Arc<ConfigCache>,
    path: String,
    policy: Json<WORMPolicyRequest>,
    user_id: UserId,
) -> Result<Json<serde_json::Value>, StatusCode>;

// POST /config/quota/{user_or_group_id}
// Body: { bytes_hard_limit: u64, bytes_soft_limit: u64, grace_period_days: u64 }
// Response: { epoch: u64, updated_at_ns: i64 }
pub async fn update_quota_policy(
    config_cache: Arc<ConfigCache>,
    user_id: String,
    policy: Json<QuotaPolicyRequest>,
    auth_user: UserId, // Must be admin
) -> Result<Json<serde_json::Value>, StatusCode>;

// DELETE /config/override/{policy_id}
// Response: { cleared: bool }
pub async fn clear_local_override(
    config_cache: Arc<ConfigCache>,
    policy_id: String,
    auth_user: UserId,
) -> Result<Json<serde_json::Value>, StatusCode>;
```

**Axum Router Setup:**

```rust
pub fn router(config_cache: Arc<ConfigCache>) -> Router {
    Router::new()
        .route("/config/qos/:tenant_id", post(update_qos_policy))
        .route("/config/worm/:path", post(update_worm_policy))
        .route("/config/quota/:user_id", post(update_quota_policy))
        .route("/config/current", get(get_current_config))
        .route("/config/override/:policy_id", delete(clear_local_override))
        .layer(middleware::from_fn(auth_middleware))
        .with_state(config_cache)
}

async fn auth_middleware(...) -> ... {
    // Verify mTLS cert, extract user ID
}
```

**Error Handling:**
- 400 Bad Request: Invalid policy
- 401 Unauthorized: Missing/invalid cert
- 403 Forbidden: Admin-only endpoint
- 409 Conflict: Policy version mismatch
- 503 Service Unavailable: Metadata unreachable

**Tests (6):**
1. `test_qos_policy_update_api` — POST QoS endpoint
2. `test_worm_retention_api` — POST WORM endpoint
3. `test_quota_update_api` — POST quota endpoint
4. `test_config_dump_api` — GET /config/current
5. `test_invalid_policy_rejection` — 400 Bad Request validation
6. `test_auth_required_on_config_endpoints` — 401/403 auth checks

---

### 4. config_storage.rs (300 lines)

**Purpose:** Persist configuration to A2 Metadata KV store with versioning and audit trail.

**KV Schema:**

```
/config/fuse/current         → PolicyEpoch JSON (active config)
/config/fuse/epoch:{N}       → PolicyEpoch JSON (history snapshot)
/config/fuse/audit           → AuditLog JSON (append-only)
/config/fuse/rollback-points → RollbackPoints JSON (recent snapshots)
```

**Implementation:**

```rust
pub struct ConfigStorage {
    metadata_client: Arc<MetadataClient>,
}

pub struct AuditEntry {
    pub timestamp_ns: i64,
    pub user_id: String,
    pub action: String,  // "update_qos", "rollback", etc
    pub policy_type: String, // "qos", "worm", "quota"
    pub policy_id: String,
    pub old_epoch: u64,
    pub new_epoch: u64,
    pub details: serde_json::Value,
}

impl ConfigStorage {
    pub fn new(metadata_client: Arc<MetadataClient>) -> Self;

    /// Store new PolicyEpoch to A2, increment epoch
    pub async fn save_policy_epoch(
        &self,
        epoch: PolicyEpoch,
    ) -> Result<u64, StorageError>;

    /// Load current PolicyEpoch from A2
    pub async fn load_current_epoch(&self) -> Result<PolicyEpoch, StorageError>;

    /// Retrieve historical PolicyEpoch by number
    pub async fn load_epoch(&self, epoch_num: u64) -> Result<PolicyEpoch, StorageError>;

    /// Append audit entry
    pub async fn audit_log(
        &self,
        entry: AuditEntry,
    ) -> Result<(), StorageError>;

    /// Get audit trail (last N entries)
    pub async fn get_audit_trail(&self, limit: usize) -> Result<Vec<AuditEntry>, StorageError>;

    /// Rollback to previous epoch
    pub async fn rollback_to_epoch(
        &self,
        epoch_num: u64,
        user_id: &str,
    ) -> Result<u64, StorageError>;

    /// List all stored epochs
    pub async fn list_epochs(&self) -> Result<Vec<u64>, StorageError>;
}
```

**Audit Trail Format:**

```json
{
  "entries": [
    {
      "timestamp_ns": 1713446400000000000,
      "user_id": "admin@example.com",
      "action": "update_qos",
      "policy_type": "qos",
      "policy_id": "tenant-1",
      "old_epoch": 42,
      "new_epoch": 43,
      "details": {
        "read_bps_limit_old": 1000000000,
        "read_bps_limit_new": 2000000000
      }
    }
  ]
}
```

**Tests (3):**
1. `test_config_persistence_to_metadata` — Save/load PolicyEpoch
2. `test_config_audit_trail` — Audit log append and retrieval
3. `test_config_rollback` — Rollback to historical epoch

---

### 5. config_replication.rs (250 lines)

**Purpose:** Replicate config changes across cluster nodes.

**Implementation:**

```rust
pub struct ConfigReplicator {
    config_cache: Arc<ConfigCache>,
    transport_client: Arc<TransportClient>, // A4
    node_id: String,
}

pub enum ReplicationMessage {
    ConfigUpdate {
        epoch: u64,
        policy_type: String,  // "qos", "worm", "quota"
        policy_id: String,
        payload: serde_json::Value,
    },
    FullSync {
        epoch: u64,
        policies: PolicyEpoch,
    },
}

impl ConfigReplicator {
    pub fn new(
        config_cache: Arc<ConfigCache>,
        transport_client: Arc<TransportClient>,
        node_id: String,
    ) -> Self;

    /// Broadcast config update to all nodes
    pub async fn broadcast_config_update(
        &self,
        policy_type: &str,
        policy_id: &str,
        new_epoch: u64,
        payload: serde_json::Value,
    ) -> Result<(), ReplicationError>;

    /// Handle incoming config update from peer
    pub async fn handle_config_update(&self, msg: ReplicationMessage) -> Result<(), ReplicationError>;

    /// Full sync: send current config to late-joining node
    pub async fn full_sync_to_node(
        &self,
        target_node_id: &str,
    ) -> Result<(), ReplicationError>;

    /// Receive full sync from peer
    pub async fn handle_full_sync(&self, policies: PolicyEpoch) -> Result<(), ReplicationError>;
}
```

**Protocol:**
- Use A4 transport's RPC mechanism
- Message format: Protobuf (prost) or JSON
- Retries with exponential backoff
- Acknowledge receipt, track failed nodes

**Tests (4):**
1. `test_config_broadcast_to_nodes` — Broadcast update to 3 nodes
2. `test_cross_site_config_replication` — Multi-site sync via A6
3. `test_latecomers_receive_full_config` — Full sync to rejoining node
4. `test_config_consistency_after_failover` — LWW conflict resolution

---

### 6. config_snapshot.rs (250 lines)

**Purpose:** Point-in-time configuration snapshots for auditing/debugging.

**Implementation:**

```rust
pub struct ConfigSnapshot {
    pub snapshot_id: String,     // UUID or "current"
    pub timestamp_ns: i64,
    pub epoch: u64,
    pub policies: PolicyEpoch,
    pub reason: String,          // "manual", "pre-update", "failover", etc
}

pub struct ConfigSnapshotManager {
    metadata_client: Arc<MetadataClient>,
    max_snapshots: usize,        // Keep last N snapshots
}

impl ConfigSnapshotManager {
    pub fn new(metadata_client: Arc<MetadataClient>, max_snapshots: usize) -> Self;

    /// Create snapshot before update
    pub async fn create_snapshot(
        &self,
        epoch: u64,
        policies: PolicyEpoch,
        reason: &str,
    ) -> Result<String, SnapshotError>; // Returns snapshot_id

    /// Retrieve snapshot
    pub async fn get_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<ConfigSnapshot, SnapshotError>;

    /// List recent snapshots
    pub async fn list_snapshots(&self, limit: usize) -> Result<Vec<ConfigSnapshot>, SnapshotError>;

    /// Restore from snapshot (used for recovery)
    pub async fn restore_from_snapshot(
        &self,
        snapshot_id: &str,
        config_cache: &ConfigCache,
    ) -> Result<u64, SnapshotError>; // Returns new epoch

    /// Cleanup old snapshots (auto-eviction)
    pub async fn cleanup_old_snapshots(&self) -> Result<usize, SnapshotError>;
}
```

**Storage:**
- KV at `/config/fuse/snapshots/{snapshot_id}`
- JSON serialization
- Auto-cleanup of old snapshots

**Tests (2):**
1. `test_config_snapshot_creation` — Create snapshot, retrieve
2. `test_config_snapshot_restore` — Restore from old snapshot

---

## Test Utilities (Shared)

**New file:** `config_test_utils.rs` (~200 lines)

```rust
pub fn create_mock_metadata_client() -> Arc<MockMetadataClient>;
pub fn create_test_config_cache() -> Arc<ConfigCache>;
pub async fn wait_for_config_update(
    listener: &mut Receiver<ConfigUpdateEvent>,
    timeout_ms: u64,
) -> Option<ConfigUpdateEvent>;
pub fn assert_policy_equality(p1: &QoSPolicy, p2: &QoSPolicy);
```

---

## Integration with Phase 37 Modules

### QoS Client Bridge
- **Uses:** ConfigCache to fetch tenant QoS policies
- **Change:** Initialize with ConfigCache::subscribe() listener

### WORM Enforcement
- **Uses:** ConfigCache to fetch WORM retention policies
- **Change:** Update hold validity checks to use cached policies

### Quota Client Tracker
- **Uses:** ConfigCache to fetch per-user quota limits
- **Change:** Subscribe to quota policy updates

### Distributed Session Manager
- **Uses:** ConfigCache for session timeout policies
- **Change:** Hot-reload session timeout without reconnect

### OpenTelemetry Integration
- **Uses:** ConfigCache for tracing sampling rate
- **Change:** Hot-reload sampling rate

---

## Error Handling

**New Error Type:**

```rust
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("Config validation failed: {0}")]
    ValidationError(String),

    #[error("Config version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u64, actual: u64 },

    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    #[error("Metadata service unavailable: {0}")]
    MetadataUnavailable(String),

    #[error("Replication error: {0}")]
    ReplicationError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialize error: {0}")]
    SerializeError(String),
}

pub type ConfigResult<T> = Result<T, ConfigError>;
```

---

## Logging & Tracing

- Use `tracing` spans for all operations
- Log config updates with PolicyEpoch, user, timestamp
- Trace replication delays (multi-node)
- Debug: Dump full PolicyEpoch on trace level

---

## Performance Targets

- Config cache lookup: < 1μs (in-memory DashMap)
- TTL refresh check: < 10μs
- Policy update end-to-end: < 100ms (p99)
- Broadcast to 5 nodes: < 50ms
- Snapshot creation: < 10ms

---

## Testing Strategy

**Unit Tests (25 total):**
- config_manager: 6 tests
- config_validator: 4 tests
- config_http_api: 6 tests
- config_storage: 3 tests
- config_replication: 4 tests
- config_snapshot: 2 tests

**Integration Tests (in config_test_utils):**
- Config + QoS interaction
- Config + WORM interaction
- Config + Quota interaction
- Multi-node consistency

---

## Acceptance Criteria

✅ All 25 unit tests passing
✅ Config update latency < 100ms
✅ No regressions in Phase 37 tests
✅ Audit trail recorded for all updates
✅ Rollback functionality verified
✅ Multi-node replication working
✅ HTTP endpoints documented & tested

---

## Deliverables Checklist

- [ ] config_manager.rs (600 lines + 6 tests)
- [ ] config_validator.rs (400 lines + 4 tests)
- [ ] config_http_api.rs (350 lines + 6 tests)
- [ ] config_storage.rs (300 lines + 3 tests)
- [ ] config_replication.rs (250 lines + 4 tests)
- [ ] config_snapshot.rs (250 lines + 2 tests)
- [ ] config_test_utils.rs (200 lines shared)
- [ ] Update lib.rs exports
- [ ] Update Cargo.toml if needed
- [ ] All 25 tests passing
- [ ] cargo clippy clean
- [ ] Integration with QoS, WORM, Quota modules

---

**Implementation Ready for OpenCode** ✅

