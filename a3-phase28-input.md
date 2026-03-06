# A3: Data Reduction — Phase 28: WORM + Key Rotation + Compliance

## Overview

Implement enterprise-grade compliance features for ClaudeFS data reduction: Write-Once-Read-Many (WORM) enforcement, key rotation without data re-encryption, and comprehensive audit trails.

**Target:** 80-100 new tests (+90 tests) across 4-5 modules
**Baseline:** 2020 tests passing (Phase 27 complete)

---

## Module 1: worm_policy.rs (~22 tests)

**Purpose:** Enforce Write-Once-Read-Many semantics on chunks and snapshots.

### Types

```rust
/// WORM policy enforcement mode for a snapshot or namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WormMode {
    /// Disabled: normal read-write semantics
    Disabled,
    /// Compliance mode: once finalized, cannot modify or delete for retention period
    ComplianceMode {
        retention_days: u16,  // 1-36500 (1 day to 100 years)
    },
    /// Governance mode: can be overridden by admins with proper authorization
    GovernanceMode {
        retention_days: u16,
    },
}

/// Per-chunk WORM state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkWormState {
    pub chunk_id: ChunkId,
    pub worm_mode: WormMode,
    /// Unix timestamp when finalization occurred
    pub finalized_at_ns: u64,
    /// Unix timestamp when chunk becomes eligible for deletion (after retention)
    pub retention_until_ns: u64,
    /// Administrator who initiated finalization
    pub finalized_by: String,
    /// Reason for WORM enforcement (e.g., "Compliance-20240315")
    pub policy_reason: String,
}

/// Per-snapshot WORM state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotWormState {
    pub snapshot_id: SnapshotId,
    pub worm_mode: WormMode,
    pub finalized_at_ns: u64,
    pub retention_until_ns: u64,
    pub finalized_by: String,
    pub policy_reason: String,
    /// In governance mode, track admin overrides
    pub governance_overrides: Vec<GovernanceOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceOverride {
    pub timestamp_ns: u64,
    pub admin_id: String,
    pub action: String,  // "delete", "modify_retention", etc.
    pub justification: String,
    pub audit_ticket: String,  // Link to ticket system
}

pub type ChunkId = u64;
pub type SnapshotId = u64;
```

### Methods

```rust
impl WormPolicy {
    pub fn new() -> Self { ... }

    /// Finalize (lock) a chunk with WORM semantics
    pub fn finalize_chunk(
        &mut self,
        chunk_id: ChunkId,
        worm_mode: WormMode,
        admin: String,
        reason: String,
    ) -> Result<(), WormError> { ... }

    /// Check if a chunk can be deleted (retention period must be expired)
    pub fn can_delete_chunk(&self, chunk_id: ChunkId, now_ns: u64) -> bool { ... }

    /// Check if a chunk can be modified (only in governance mode with override)
    pub fn can_modify_chunk(&self, chunk_id: ChunkId) -> Result<bool, WormError> { ... }

    /// Attempt to override WORM on a chunk in governance mode
    pub fn governance_override(
        &mut self,
        chunk_id: ChunkId,
        admin_id: String,
        action: String,
        justification: String,
        ticket: String,
    ) -> Result<(), WormError> { ... }

    /// Finalize an entire snapshot
    pub fn finalize_snapshot(
        &mut self,
        snapshot_id: SnapshotId,
        worm_mode: WormMode,
        admin: String,
        reason: String,
    ) -> Result<(), WormError> { ... }

    /// Get WORM state for a chunk
    pub fn get_chunk_state(&self, chunk_id: ChunkId) -> Option<&ChunkWormState> { ... }

    /// Get WORM state for a snapshot
    pub fn get_snapshot_state(&self, snapshot_id: SnapshotId) -> Option<&SnapshotWormState> { ... }

    /// List all chunks in compliance mode
    pub fn compliance_chunks(&self) -> Vec<&ChunkWormState> { ... }

    /// List all snapshots approaching retention expiry
    pub fn expiring_snapshots(&self, within_days: u16) -> Vec<&SnapshotWormState> { ... }

    /// Compute retention compliance stats
    pub fn stats(&self) -> WormStats { ... }
}

pub struct WormStats {
    pub total_finalized_chunks: u64,
    pub compliance_mode_chunks: u64,
    pub governance_mode_chunks: u64,
    pub chunks_eligible_for_deletion: u64,
    pub total_snapshots: u64,
    pub governance_overrides_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WormError {
    ChunkLocked,
    RetentionNotExpired,
    InvalidRetentionDays,
    SnapshotNotFound,
    ChunkNotFound,
}
```

### Tests (22 tests)
- `finalize_chunk_compliance` — locks chunk in compliance mode
- `finalize_chunk_governance` — locks chunk in governance mode
- `cannot_modify_compliance_chunk` — compliance mode prevents modifications
- `governance_override_allowed` — admin can override in governance mode
- `governance_override_tracked` — overrides are logged with audit details
- `retention_expiry_unlocks` — after retention period, chunk can be deleted
- `finalize_snapshot_locks_all_chunks` — finalizing snapshot affects all constituent chunks
- `snapshot_expiry_scheduled` — snapshots approaching expiry are tracked
- `worm_mode_disabled_allows_modification` — disabled mode permits changes
- `compliance_chunks_query` — retrieve all compliance-mode chunks
- `governance_overrides_query` — list all overrides for audit
- `invalid_retention_days_rejected` — validation of retention period (1-36500)
- `multiple_governance_overrides` — chain multiple overrides with audit trail
- `stats_finalized_chunks` — stat tracking for finalized chunk count
- `stats_compliance_vs_governance` — stat breakdown by mode
- `stats_chunks_eligible_for_deletion` — count chunks past retention
- `worm_state_persistence` — serialize/deserialize WormState
- `chunk_worm_state_serialization` — individual chunk state round-trip
- `snapshot_worm_state_serialization` — individual snapshot state round-trip
- `compliance_expiring_snapshots` — find snapshots within N days of expiry
- `empty_worm_policy_stats` — stats on new empty policy
- `governance_override_empty_list` — no overrides initially

---

## Module 2: key_rotation.rs (~24 tests)

**Purpose:** Rotate encryption keys without re-encrypting all data (envelope encryption pattern).

### Types

```rust
/// Master encryption key (stored in KMS or HSM, never on disk)
pub type MasterKey = [u8; 32];

/// Data encryption key ID (KEK = Key Encryption Key)
pub type KeyId = u64;

/// Envelope-encrypted data encryption key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedDataKey {
    pub key_id: KeyId,
    /// Encrypted DEK = AES-GCM(masterkey, dek_plaintext, iv, aad)
    pub ciphertext: Vec<u8>,
    pub iv: [u8; 12],
    pub aad: Vec<u8>,  // Associated Authenticated Data (key_id, timestamp, etc)
}

/// Key version with rotation metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyVersion {
    pub key_id: KeyId,
    /// Unix timestamp when this key became active
    pub created_at_ns: u64,
    /// Unix timestamp when this key is rotated out (None = still active)
    pub retired_at_ns: Option<u64>,
    /// Is this the current key for new encryptions?
    pub is_active: bool,
    /// Estimated number of chunks encrypted with this key
    pub chunk_count: u64,
    /// Number of chunks successfully re-encrypted with new key
    pub reencrypted_chunk_count: u64,
}

/// Key rotation state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RotationPhase {
    /// Key is active for new encryptions
    Active,
    /// Key is retired; background re-encryption in progress
    Rotating,
    /// All chunks re-encrypted; old key ready for archival
    Complete,
    /// Key archived and no longer usable
    Archived,
}

pub struct KeyCatalog {
    keys: HashMap<KeyId, KeyVersion>,
    current_active_key: Option<KeyId>,
    master_key: MasterKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationStats {
    pub total_keys: u64,
    pub active_keys: u64,
    pub retiring_keys: u64,
    pub archived_keys: u64,
    pub chunks_reencrypted_total: u64,
    pub chunks_pending_reencryption: u64,
    pub estimated_completion_ns: Option<u64>,
}
```

### Methods

```rust
impl KeyCatalog {
    pub fn new(master_key: MasterKey) -> Self { ... }

    /// Generate a new DEK and envelope-encrypt it with the current master key
    pub fn generate_and_encrypt_dek(
        &mut self,
        aad: Vec<u8>,
    ) -> Result<EncryptedDataKey, KeyRotationError> { ... }

    /// Decrypt a DEK using the master key
    pub fn decrypt_dek(
        &self,
        encrypted_dek: &EncryptedDataKey,
    ) -> Result<Vec<u8>, KeyRotationError> { ... }

    /// Rotate to a new master key: re-encrypt all DEKs
    pub fn rotate_master_key(
        &mut self,
        new_master_key: MasterKey,
    ) -> Result<(), KeyRotationError> { ... }

    /// Create a new data encryption key version for future encryptions
    pub fn initiate_key_rotation(
        &mut self,
        new_key: MasterKey,  // Or generate internally
    ) -> Result<KeyId, KeyRotationError> { ... }

    /// Mark chunks as re-encrypted with new key (background task)
    pub fn record_chunk_reencryption(
        &mut self,
        key_id: KeyId,
        chunk_count: u64,
    ) -> Result<(), KeyRotationError> { ... }

    /// Retire old key when all chunks are re-encrypted
    pub fn complete_key_rotation(&mut self, key_id: KeyId) -> Result<(), KeyRotationError> { ... }

    /// Archive a key (removes from active set, keeps for audit)
    pub fn archive_key(&mut self, key_id: KeyId) -> Result<(), KeyRotationError> { ... }

    /// Get the active key for new encryptions
    pub fn active_key(&self) -> Option<&KeyVersion> { ... }

    /// Get all keys in rotation phase
    pub fn rotating_keys(&self) -> Vec<&KeyVersion> { ... }

    /// Get rotation progress stats
    pub fn rotation_stats(&self) -> RotationStats { ... }

    /// Get key version by ID
    pub fn get_key(&self, key_id: KeyId) -> Option<&KeyVersion> { ... }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyRotationError {
    MasterKeyNotSet,
    KeyNotFound,
    DecryptionFailed,
    InvalidAad,
    RotationInProgress,
    NoChunksToRotate,
}
```

### Tests (24 tests)
- `generate_and_encrypt_dek` — create new DEK and envelope it
- `decrypt_dek_succeeds` — decrypt with correct master key
- `decrypt_dek_wrong_key_fails` — decryption with wrong key fails
- `initiate_key_rotation` — create new key version
- `active_key_returns_current` — get current encryption key
- `new_key_becomes_active` — after rotation, new key is active for writes
- `old_key_marked_retiring` — previous key enters rotating phase
- `record_chunk_reencryption` — track re-encrypted chunk count
- `rotation_progress_tracks` — rotation stats show progress
- `complete_key_rotation_when_done` — transition to Complete phase when all chunks reencrypted
- `archived_key_not_for_encryption` — archived keys can't be used for new encryption
- `rotate_master_key_reencrypts_deks` — rotating master key encrypts all existing DEKs with new key
- `multiple_keys_in_rotation` — manage multiple key versions simultaneously
- `rotating_keys_query` — retrieve keys in rotation phase
- `key_catalog_empty_initially` — empty catalog state
- `dek_aad_protects_integrity` — AAD prevents tampering
- `encrypted_dek_serialization` — persist encrypted DEK
- `key_version_serialization` — serialize key metadata
- `rotation_stats_accuracy` — stats match actual state
- `key_expiry_scheduled` — keys can have optional expiry timestamps
- `prevent_rotation_in_progress` — can't start new rotation during active rotation
- `reencryption_batch_processing` — process reencryption in batches
- `key_audit_trail` — track all key creation/rotation events

---

## Module 3: compliance_audit.rs (~24 tests)

**Purpose:** Comprehensive audit trail for data reduction operations (dedup, compression, encryption, WORM).

### Types

```rust
/// Audit event kinds for data reduction pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventKind {
    /// Chunk deduplicated
    DedupeHit,
    /// Chunk processed through similarity pipeline
    SimilarityMatch,
    /// Data compressed
    CompressionApplied,
    /// Data encrypted
    EncryptionApplied,
    /// Chunk finalized under WORM
    WormFinalize,
    /// Key rotation initiated
    KeyRotationInitiated,
    /// Chunk re-encrypted with new key
    ChunkReencrypted,
    /// Governance override applied
    GovernanceOverride,
    /// Snapshot deleted
    SnapshotDeleted,
    /// Compliance quota enforced
    QuotaEnforced,
}

/// Audit event record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp_ns: u64,
    pub event_id: u64,  // Monotonic counter
    pub kind: AuditEventKind,
    /// Which component performed the action
    pub actor: String,  // "system", "admin", "api", etc.
    /// Subject of audit (chunk_id, snapshot_id, tenant_id, etc)
    pub subject: String,
    /// Optional result data (compression_ratio, dedup_savings, etc)
    pub details: HashMap<String, String>,
    /// Tenant/namespace for multi-tenant auditing
    pub tenant_id: Option<u64>,
}

pub struct ComplianceAudit {
    events: Vec<AuditEvent>,
    next_event_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditStats {
    pub total_events: u64,
    pub events_by_kind: HashMap<AuditEventKind, u64>,
    pub events_by_actor: HashMap<String, u64>,
    pub governance_overrides_count: u64,
    pub worm_violations_detected: u64,
    pub quota_violations_detected: u64,
}
```

### Methods

```rust
impl ComplianceAudit {
    pub fn new() -> Self { ... }

    /// Log an audit event
    pub fn log_event(
        &mut self,
        kind: AuditEventKind,
        actor: String,
        subject: String,
        details: HashMap<String, String>,
        tenant_id: Option<u64>,
    ) -> u64 { ... }  // Returns event_id

    /// Query events by kind
    pub fn events_by_kind(&self, kind: AuditEventKind) -> Vec<&AuditEvent> { ... }

    /// Query events by actor
    pub fn events_by_actor(&self, actor: &str) -> Vec<&AuditEvent> { ... }

    /// Query events by tenant
    pub fn events_by_tenant(&self, tenant_id: u64) -> Vec<&AuditEvent> { ... }

    /// Query events in time range
    pub fn events_in_range(
        &self,
        start_ns: u64,
        end_ns: u64,
    ) -> Vec<&AuditEvent> { ... }

    /// Find governance overrides
    pub fn governance_overrides(&self) -> Vec<&AuditEvent> { ... }

    /// Detect WORM policy violations (attempt to modify finalized chunk)
    pub fn worm_violations(&self) -> Vec<&AuditEvent> { ... }

    /// Detect quota violations
    pub fn quota_violations(&self) -> Vec<&AuditEvent> { ... }

    /// Export audit log as JSON/CSV for compliance reports
    pub fn export_json(&self) -> String { ... }
    pub fn export_csv(&self) -> String { ... }

    /// Compute audit statistics
    pub fn stats(&self) -> AuditStats { ... }

    /// Retention policy: delete events older than N days
    pub fn purge_old_events(&mut self, retention_days: u16) { ... }

    /// Seal audit log (cryptographically sign and make immutable)
    pub fn seal_log(&mut self, key: &[u8; 32]) -> Result<[u8; 32], ComplianceError> { ... }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComplianceError {
    AuditLogFull,
    LogAlreadySealed,
    InvalidTimeRange,
}
```

### Tests (24 tests)
- `log_event_dedup_hit` — log a deduplication hit
- `log_event_compression` — log compression operation
- `log_event_encryption` — log encryption operation
- `log_event_worm_finalize` — log WORM finalization
- `query_events_by_kind` — retrieve events by kind
- `query_events_by_actor` — retrieve events by actor
- `query_events_by_tenant` — multi-tenant isolation in audit log
- `query_events_time_range` — time-based queries
- `governance_overrides_query` — find all governance overrides
- `worm_violations_query` — detect policy violations
- `quota_violations_query` — detect quota enforcement
- `event_monotonic_id` — event IDs increment monotonically
- `stats_total_events` — compute total event count
- `stats_events_by_kind` — aggregate by event kind
- `stats_events_by_actor` — aggregate by actor
- `stats_governance_overrides_count` — count governance actions
- `export_json_format` — JSON export for external systems
- `export_csv_format` — CSV export for spreadsheets
- `purge_old_events_retention` — delete old audit entries
- `seal_log_immutable` — cryptographically seal audit log
- `audit_event_serialization` — persist events
- `empty_audit_stats` — stats on empty log
- `multi_tenant_isolation` — tenants can't see each other's events
- `large_event_log_performance` — handle 100K+ events efficiently

---

## Module 4: tiering_advisor_ml.rs (~20 tests)

**Purpose:** Machine-learning-assisted intelligent tiering decisions (S3 vs flash).

### Types

```rust
/// Workload classification for ML-assisted tiering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkloadPattern {
    /// Hot: accessed frequently (within last 24 hours)
    Hot,
    /// Warm: accessed occasionally (within last 7 days)
    Warm,
    /// Cold: accessed rarely (older than 7 days)
    Cold,
    /// Archive: never accessed, kept for compliance
    Archive,
}

/// Per-chunk access metrics for ML features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkAccessMetrics {
    pub chunk_id: u64,
    pub access_count: u64,
    pub last_access_ns: u64,
    pub size_bytes: u64,
    /// Age since creation (ns)
    pub age_ns: u64,
    /// Compression ratio achieved
    pub compression_ratio: f64,
    /// Number of dedupe references
    pub reference_count: u64,
}

/// ML model prediction for a chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieringPrediction {
    pub chunk_id: u64,
    pub recommended_tier: WorkloadPattern,
    /// Confidence 0.0-1.0
    pub confidence: f64,
    /// S3 cost vs flash cost trade-off
    pub cost_delta: f64,  // Negative = save money by tiering to S3
}

pub struct TieringAdvisor {
    /// Recent access patterns
    metrics: HashMap<u64, ChunkAccessMetrics>,
    /// ML model weights (trained offline, frozen here)
    model_weights: ModelWeights,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelWeights {
    /// Decision thresholds
    hot_threshold_days: f64,     // Age < N days → hot
    warm_threshold_days: f64,    // Age < N days → warm
    access_count_weight: f64,
    compression_ratio_weight: f64,
    reference_count_weight: f64,
}
```

### Methods

```rust
impl TieringAdvisor {
    pub fn new(model_weights: ModelWeights) -> Self { ... }

    /// Record access to a chunk (updates last_access_ns)
    pub fn record_access(&mut self, chunk_id: u64) { ... }

    /// Predict workload tier for a chunk
    pub fn predict_tier(&self, chunk_id: u64) -> Result<TieringPrediction, TieringError> { ... }

    /// Batch predict for multiple chunks (for bulk tiering decisions)
    pub fn predict_batch(&self, chunk_ids: &[u64]) -> Vec<TieringPrediction> { ... }

    /// Ingest metrics from chunks
    pub fn update_metrics(
        &mut self,
        chunk_id: u64,
        metrics: ChunkAccessMetrics,
    ) { ... }

    /// Compute cost savings: flash cost vs S3 cost
    pub fn cost_analysis(&self, chunk_id: u64) -> Result<CostAnalysis, TieringError> { ... }

    /// Get all chunks recommended for tiering to S3
    pub fn recommendations_for_tiering(&self) -> Vec<TieringPrediction> { ... }

    /// Compute advisor accuracy against actual access patterns
    pub fn compute_accuracy(&self) -> f64 { ... }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostAnalysis {
    pub chunk_id: u64,
    pub flash_cost_per_month: f64,
    pub s3_cost_per_month: f64,
    pub savings_per_month: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TieringError {
    ChunkNotFound,
    ModelNotTrained,
    PredictionFailed,
}
```

### Tests (20 tests)
- `predict_hot_recent_access` — recently accessed chunks predicted hot
- `predict_cold_old_age` — old chunks predicted cold
- `predict_warm_intermediate_age` — intermediate age predicts warm
- `predict_batch_multiple` — predict for multiple chunks
- `confidence_score_accuracy` — confidence correlates with correctness
- `record_access_updates_metrics` — accessing chunk updates last_access
- `update_metrics_custom` — ingest custom metrics
- `cost_analysis_s3_cheaper` — recommends S3 when cost-effective
- `cost_analysis_flash_better` — recommends flash when higher access
- `recommendations_for_tiering` — query chunks recommended for S3
- `tiering_advisor_empty` — predict on empty advisor (should fail gracefully)
- `model_weights_serialization` — persist model weights
- `access_metrics_serialization` — persist chunk metrics
- `accuracy_high_on_hot_chunks` — high accuracy for frequently accessed
- `accuracy_low_variance_access` — lower accuracy for erratic patterns
- `batch_predict_consistency` — batch results consistent with individual predictions
- `workload_pattern_encoding` — WorkloadPattern enum serialization
- `cost_analysis_with_compression_ratio` — compression ratio affects cost
- `cost_analysis_with_reference_count` — shared chunks have different cost
- `ml_model_weighted_average` — model combines multiple features correctly

---

## Integration Points

1. **worm_policy** ↔ **compliance_audit**
   - When chunk is finalized, log AuditEvent
   - Query audit log for WORM violations

2. **key_rotation** ↔ **compliance_audit**
   - When key rotation completes, log AuditEvent
   - Track all re-encryption progress in audit

3. **tiering_advisor_ml** ↔ **quota_tracker** (existing A3 module)
   - Tiering decisions respect quota limits
   - Don't tier chunks that would violate tenant quota

4. **All modules** ↔ **async_meta_bridge** (existing A3 module)
   - Metadata bridge integrates WORM finalization with metadata service
   - Key catalog syncs with distributed metadata for cross-site consistency

---

## Test Strategy

- **Unit tests:** Individual module behavior in isolation
- **Property tests:** Use proptest for randomized multi-step sequences
- **Integration tests:** Simulate multi-tenant WORM + key rotation + audit logging flows
- **Performance tests:** Verify audit log query performance on 100K+ events
- **Serialization tests:** All types round-trip through bincode/serde

---

## Success Criteria

✅ **2020 + 90 = 2110 tests passing**
✅ **cargo build --lib clean**
✅ **cargo clippy clean on new modules**
✅ **All 4 modules registered in lib.rs and public**
✅ **Integration tests verify cross-module interactions**
