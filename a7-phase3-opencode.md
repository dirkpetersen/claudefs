# A7 Phase 3: Advanced Gateway Features — OpenCode Implementation Prompt

## Context & Existing Architecture

ClaudeFS A7 gateway crate is at `/home/cfs/claudefs/crates/claudefs-gateway/src/` with:
- **54 source modules, ~29.9k LOC, 1128 tests passing** (Phase 2 complete)
- **Protocols:** NFSv3, NFSv4, pNFS, S3, SMB3 (via Samba VFS plugin)
- **Infrastructure:** Connection pooling, circuit breaker, health checks, quota enforcement, XDR marshaling

### Available Dependencies (Cargo.toml)
All standard workspace deps: `tokio`, `thiserror`, `anyhow`, `serde`, `prost`, `tonic`, `tracing`, `rand`, `sha2`, `base64`, `dashmap`, `uuid`, `bytes`

### Key Existing Modules to Integrate With
- **protocol.rs:** `Protocol` enum (NFS, S3, SMB)
- **error.rs:** Gateway error types (extend as needed)
- **gateway_metrics.rs:** Prometheus metric export
- **nfs_cache.rs:** NFS attribute/data cache for invalidation
- **s3.rs:** S3 object metadata cache for invalidation
- **session.rs:** Client session state
- **gateway_conn_pool.rs:** Connection pooling primitives

### Available A2 (Metadata) Integration
From `claudefs-meta` crate (if imported):
- `ClientSession` — Per-client session state machine
- `LeaseManager`, `LeaseType` — POSIX lease tracking
- `DistributedTransactionEngine` — 2-phase commit
- `QosManager` — QoS enforcement
- `MetadataNode` — Core metadata ops

### Available A4 (Transport) Integration
From `claudefs-transport` crate (if imported):
- `TraceId([u8; 16])` — Unique trace identifier
- `SpanRecord` — Individual span data
- `TraceData` — Collected trace with latency stats
- `TraceAggregator` — Central collection point
- `BandwidthShaper` — Token bucket QoS
- `AdaptiveRouter` — Endpoint selection

---

## Implementation Requirements: 4 New Modules

### 1. nfs_delegation_manager.rs (~35 tests)

**Purpose:** NFSv4 delegation state machine (grant, recall, revoke) with lease-based callback handling.

**File:** `/home/cfs/claudefs/crates/claudefs-gateway/src/nfs_delegation_manager.rs`

**Key Types:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DelegationId(pub u64);

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct DelegationCookie([u8; 8]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DelegationType {
    Open,
    ReadWrite,
    Read,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum DelegationState {
    Granted,
    Recalled,
    Revoked,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveDelegation {
    pub id: DelegationId,
    pub client_id: u64,
    pub inode_id: u64,
    pub delegation_type: DelegationType,
    pub state: DelegationState,
    pub lease_expiry_ms: u64,
    pub conflicting_op: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DelegationMetrics {
    pub total_granted: u64,
    pub total_recalled: u64,
    pub total_revoked: u64,
    pub active_delegations: u64,
}

#[derive(Debug)]
pub enum DelegationError {
    Expired,
    LeaseConflict,
    NotFound,
    InvalidState,
}

impl std::fmt::Display for DelegationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Expired => write!(f, "Delegation expired"),
            Self::LeaseConflict => write!(f, "Lease conflict detected"),
            Self::NotFound => write!(f, "Delegation not found"),
            Self::InvalidState => write!(f, "Invalid delegation state"),
        }
    }
}

impl std::error::Error for DelegationError {}
```

**DelegationManager Implementation:**

```rust
use std::sync::Arc;
use dashmap::DashMap;
use tokio::time::Instant;

pub struct DelegationManager {
    delegations: Arc<DashMap<DelegationId, ActiveDelegation>>,
    client_delegations: Arc<DashMap<u64, Vec<DelegationId>>>,
    inode_delegations: Arc<DashMap<u64, Vec<DelegationId>>>,
    metrics: std::sync::Arc<parking_lot::Mutex<DelegationMetrics>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl DelegationManager {
    pub fn new() -> Self {
        Self {
            delegations: Arc::new(DashMap::new()),
            client_delegations: Arc::new(DashMap::new()),
            inode_delegations: Arc::new(DashMap::new()),
            metrics: Arc::new(parking_lot::Mutex::new(DelegationMetrics {
                total_granted: 0,
                total_recalled: 0,
                total_revoked: 0,
                active_delegations: 0,
            })),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    pub async fn grant_delegation(
        &self,
        client_id: u64,
        inode_id: u64,
        delegation_type: DelegationType,
        lease_duration_secs: u64,
    ) -> Result<ActiveDelegation, DelegationError> {
        // Generate unique ID and cookie
        let id = DelegationId(self.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
        let mut cookie = [0u8; 8];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut cookie);

        let lease_expiry_ms = Instant::now()
            .elapsed()
            .as_millis() as u64
            + (lease_duration_secs * 1000);

        let delegation = ActiveDelegation {
            id,
            client_id,
            inode_id,
            delegation_type,
            state: DelegationState::Granted,
            lease_expiry_ms,
            conflicting_op: None,
        };

        self.delegations.insert(id, delegation.clone());

        // Track by client
        self.client_delegations
            .entry(client_id)
            .or_insert_with(Vec::new)
            .push(id);

        // Track by inode
        self.inode_delegations
            .entry(inode_id)
            .or_insert_with(Vec::new)
            .push(id);

        // Update metrics
        {
            let mut metrics = self.metrics.lock();
            metrics.total_granted += 1;
            metrics.active_delegations += 1;
        }

        Ok(delegation)
    }

    pub fn is_delegation_valid(&self, delegation_id: DelegationId) -> bool {
        if let Some(delegation) = self.delegations.get(&delegation_id) {
            delegation.state == DelegationState::Granted
                && delegation.lease_expiry_ms > Instant::now().elapsed().as_millis() as u64
        } else {
            false
        }
    }

    pub fn get_delegation(&self, delegation_id: DelegationId) -> Option<ActiveDelegation> {
        self.delegations.get(&delegation_id).map(|d| d.clone())
    }

    pub async fn recall_by_inode(&self, inode_id: u64) -> Result<Vec<DelegationId>, DelegationError> {
        let mut recalled = Vec::new();

        if let Some(deleg_ids) = self.inode_delegations.get(&inode_id) {
            for &id in deleg_ids.iter() {
                if let Some(mut delegation) = self.delegations.get_mut(&id) {
                    if delegation.state == DelegationState::Granted {
                        delegation.state = DelegationState::Recalled;
                        recalled.push(id);

                        let mut metrics = self.metrics.lock();
                        metrics.total_recalled += 1;
                    }
                }
            }
        }

        Ok(recalled)
    }

    pub async fn recall_by_client(&self, client_id: u64) -> Result<Vec<DelegationId>, DelegationError> {
        let mut recalled = Vec::new();

        if let Some(deleg_ids) = self.client_delegations.get(&client_id) {
            for &id in deleg_ids.iter() {
                if let Some(mut delegation) = self.delegations.get_mut(&id) {
                    if delegation.state == DelegationState::Granted {
                        delegation.state = DelegationState::Recalled;
                        recalled.push(id);

                        let mut metrics = self.metrics.lock();
                        metrics.total_recalled += 1;
                    }
                }
            }
        }

        Ok(recalled)
    }

    pub fn process_delegation_return(&self, delegation_id: DelegationId) -> Result<(), DelegationError> {
        if let Some(mut delegation) = self.delegations.get_mut(&delegation_id) {
            if delegation.state == DelegationState::Recalled {
                delegation.state = DelegationState::Revoked;

                let mut metrics = self.metrics.lock();
                metrics.total_revoked += 1;
                metrics.active_delegations = metrics.active_delegations.saturating_sub(1);

                Ok(())
            } else {
                Err(DelegationError::InvalidState)
            }
        } else {
            Err(DelegationError::NotFound)
        }
    }

    pub async fn cleanup_expired(&self) -> Result<usize, DelegationError> {
        let mut cleaned = 0;
        let now = Instant::now().elapsed().as_millis() as u64;

        self.delegations.retain(|_id, delegation| {
            if delegation.lease_expiry_ms < now {
                cleaned += 1;
                false
            } else {
                true
            }
        });

        if cleaned > 0 {
            let mut metrics = self.metrics.lock();
            metrics.active_delegations = metrics.active_delegations.saturating_sub(cleaned);
        }

        Ok(cleaned)
    }

    pub fn metrics(&self) -> DelegationMetrics {
        self.metrics.lock().clone()
    }
}

impl Default for DelegationManager {
    fn default() -> Self {
        Self::new()
    }
}
```

**Test Module:** Add 35+ unit tests in `#[cfg(test)]` module covering:
- Grant and validate delegations (6)
- Recall by inode (4)
- Recall by client (4)
- DELEGRETURN processing (3)
- Lease expiry and cleanup (4)
- Metrics tracking (3)
- Concurrent operations (3)
- Grace period enforcement (2)
- Cookie uniqueness (2)

---

### 2. cross_protocol_consistency.rs (~35 tests)

**Purpose:** Detect and resolve conflicts when NFS/S3/SMB access same inode.

**File:** `/home/cfs/claudefs/crates/claudefs-gateway/src/cross_protocol_consistency.rs`

**Key Types:**

```rust
use crate::protocol::Protocol;
use std::collections::VecDeque;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolAccessRecord {
    pub protocol: Protocol,
    pub client_id: u64,
    pub inode_id: u64,
    pub access_type: AccessType,
    pub timestamp_ms: u64,
    pub request_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AccessType {
    Read,
    Write,
    Delete,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConflictType {
    ReadWrite,
    ConcurrentWrites,
    RenameUnderAccess,
    DeleteUnderAccess,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConflictRecord {
    pub conflict_id: u64,
    pub conflict_type: ConflictType,
    pub accesses: [ProtocolAccessRecord; 2],
    pub detected_at_ms: u64,
    pub resolution: ConflictResolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConflictResolution {
    LastWriteWins,
    AbortRequest,
    RevokeDelegation,
    ClientNotified,
}

#[derive(Debug, Clone)]
pub struct CrossProtocolMetrics {
    pub total_accesses: u64,
    pub conflicts_detected: u64,
    pub conflicts_resolved: u64,
}

#[derive(Debug)]
pub enum ConsistencyError {
    InvalidAccess,
    ResolutionFailed,
    CacheError,
    NotFound,
}

impl std::fmt::Display for ConsistencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::InvalidAccess => write!(f, "Invalid access type"),
            Self::ResolutionFailed => write!(f, "Conflict resolution failed"),
            Self::CacheError => write!(f, "Cache operation failed"),
            Self::NotFound => write!(f, "Access record not found"),
        }
    }
}

impl std::error::Error for ConsistencyError {}
```

**CrossProtocolCache Implementation:**

```rust
pub struct CrossProtocolCache {
    recent_accesses: Arc<DashMap<u64, VecDeque<ProtocolAccessRecord>>>,
    conflicts: Arc<DashMap<u64, ConflictRecord>>,
    metrics: std::sync::Arc<parking_lot::Mutex<CrossProtocolMetrics>>,
    next_conflict_id: std::sync::atomic::AtomicU64,
    window_size: usize,
}

impl CrossProtocolCache {
    pub fn new(window_size: usize) -> Self {
        Self {
            recent_accesses: Arc::new(DashMap::new()),
            conflicts: Arc::new(DashMap::new()),
            metrics: Arc::new(parking_lot::Mutex::new(CrossProtocolMetrics {
                total_accesses: 0,
                conflicts_detected: 0,
                conflicts_resolved: 0,
            })),
            next_conflict_id: std::sync::atomic::AtomicU64::new(1),
            window_size,
        }
    }

    pub async fn record_access(
        &self,
        protocol: Protocol,
        client_id: u64,
        inode_id: u64,
        access_type: AccessType,
        request_id: u64,
    ) -> Result<Option<ConflictRecord>, ConsistencyError> {
        use tokio::time::Instant;

        let record = ProtocolAccessRecord {
            protocol,
            client_id,
            inode_id,
            access_type,
            timestamp_ms: Instant::now().elapsed().as_millis() as u64,
            request_id,
        };

        // Get or create access history for this inode
        let mut accesses = self.recent_accesses.entry(inode_id).or_insert_with(VecDeque::new);

        // Check for conflicts with recent accesses
        let mut detected_conflict = None;
        if accesses.len() > 0 {
            if let Some(prev_record) = accesses.back() {
                if let Some(conflict_type) = Self::detect_conflict(prev_record, &record) {
                    let conflict_id = self.next_conflict_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let conflict = ConflictRecord {
                        conflict_id,
                        conflict_type,
                        accesses: [prev_record.clone(), record.clone()],
                        detected_at_ms: Instant::now().elapsed().as_millis() as u64,
                        resolution: ConflictResolution::LastWriteWins,
                    };

                    self.conflicts.insert(conflict_id, conflict.clone());

                    let mut metrics = self.metrics.lock();
                    metrics.conflicts_detected += 1;

                    detected_conflict = Some(conflict);
                }
            }
        }

        // Add to history
        accesses.push_back(record);

        // Maintain window size
        while accesses.len() > self.window_size {
            accesses.pop_front();
        }

        let mut metrics = self.metrics.lock();
        metrics.total_accesses += 1;

        Ok(detected_conflict)
    }

    pub fn has_concurrent_writes(&self, inode_id: u64) -> bool {
        if let Some(accesses) = self.recent_accesses.get(&inode_id) {
            let mut write_count = 0;
            let mut protocols = std::collections::HashSet::new();

            for access in accesses.iter() {
                if access.access_type == AccessType::Write {
                    write_count += 1;
                    protocols.insert(access.protocol);
                }
            }

            write_count > 1 || (write_count == 1 && protocols.len() > 1)
        } else {
            false
        }
    }

    pub fn get_access_history(&self, inode_id: u64, lookback_ms: u64) -> Vec<ProtocolAccessRecord> {
        if let Some(accesses) = self.recent_accesses.get(&inode_id) {
            let now = tokio::time::Instant::now().elapsed().as_millis() as u64;
            accesses
                .iter()
                .filter(|a| now - a.timestamp_ms < lookback_ms)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn detect_conflict(
        rec1: &ProtocolAccessRecord,
        rec2: &ProtocolAccessRecord,
    ) -> Option<ConflictType> {
        if rec1.protocol == rec2.protocol {
            return None; // Same protocol, no cross-protocol conflict
        }

        match (rec1.access_type, rec2.access_type) {
            (AccessType::Read, AccessType::Write) | (AccessType::Write, AccessType::Read) => {
                Some(ConflictType::ReadWrite)
            }
            (AccessType::Write, AccessType::Write) => Some(ConflictType::ConcurrentWrites),
            (AccessType::Delete, AccessType::Read) => Some(ConflictType::DeleteUnderAccess),
            (AccessType::Read, AccessType::Delete) => Some(ConflictType::DeleteUnderAccess),
            (AccessType::Write, AccessType::Delete) => Some(ConflictType::DeleteUnderAccess),
            _ => None,
        }
    }

    pub async fn resolve_conflict(
        &self,
        mut conflict: ConflictRecord,
    ) -> Result<ConflictResolution, ConsistencyError> {
        // Simple last-write-wins for now
        let resolution = if conflict.accesses[1].timestamp_ms > conflict.accesses[0].timestamp_ms {
            ConflictResolution::LastWriteWins
        } else {
            ConflictResolution::LastWriteWins
        };

        conflict.resolution = resolution;
        self.conflicts.insert(conflict.conflict_id, conflict);

        let mut metrics = self.metrics.lock();
        metrics.conflicts_resolved += 1;

        Ok(resolution)
    }

    pub fn metrics(&self) -> CrossProtocolMetrics {
        self.metrics.lock().clone()
    }

    pub async fn cleanup_old(&self, older_than_ms: u64) -> Result<usize, ConsistencyError> {
        let mut cleaned = 0;
        let now = tokio::time::Instant::now().elapsed().as_millis() as u64;

        self.recent_accesses.retain(|_inode_id, accesses| {
            let before = accesses.len();
            while let Some(access) = accesses.front() {
                if now - access.timestamp_ms > older_than_ms {
                    accesses.pop_front();
                    cleaned += 1;
                } else {
                    break;
                }
            }
            !accesses.is_empty() || before == 0
        });

        Ok(cleaned)
    }
}

impl Default for CrossProtocolCache {
    fn default() -> Self {
        Self::new(1000)
    }
}
```

**Test Module:** Add 35+ unit tests covering:
- Record single-protocol accesses (4)
- Detect read-write conflicts (4)
- Detect concurrent writes (4)
- Detect delete conflicts (3)
- Resolve conflicts (4)
- Cross-protocol conflict handling (3)
- Cache cleanup (3)
- Metrics tracking (3)
- Window size enforcement (2)

---

### 3. tiered_storage_router.rs (~30 tests)

**Purpose:** Route reads based on tier (hot NVMe ↔ warm memory ↔ cold S3).

**File:** `/home/cfs/claudefs/crates/claudefs-gateway/src/tiered_storage_router.rs`

**Key Types:**

```rust
use crate::protocol::Protocol;
use std::collections::VecDeque;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StorageTier {
    Hot,
    Warm,
    Cold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AccessPattern {
    Sequential,
    Random,
    Streaming,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TierHint {
    pub tier: StorageTier,
    pub reason: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ObjectTierMetadata {
    pub inode_id: u64,
    pub object_key: String,
    pub current_tier: StorageTier,
    pub access_pattern: AccessPattern,
    pub last_access_ms: u64,
    pub access_count: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct TieringPolicy {
    pub promotion_threshold: u64,
    pub demotion_threshold_ms: u64,
    pub prefetch_distance_kb: u64,
    pub cold_tier_cost_us: u64,
}

#[derive(Debug, Clone)]
pub struct AccessRecord {
    pub inode_id: u64,
    pub offset: u64,
    pub size: u64,
    pub timestamp_ms: u64,
    pub source: Protocol,
}

#[derive(Debug, Clone)]
pub struct TieringMetrics {
    pub hot_tier_reads: u64,
    pub cold_tier_reads: u64,
    pub prefetch_hits: u64,
    pub prefetch_misses: u64,
    pub promotions: u64,
    pub demotions: u64,
}

#[derive(Debug)]
pub enum TieringError {
    InvalidTier,
    PromotionFailed,
    DemotionFailed,
    ObjectNotFound,
}

impl std::fmt::Display for TieringError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::InvalidTier => write!(f, "Invalid storage tier"),
            Self::PromotionFailed => write!(f, "Tier promotion failed"),
            Self::DemotionFailed => write!(f, "Tier demotion failed"),
            Self::ObjectNotFound => write!(f, "Object not found"),
        }
    }
}

impl std::error::Error for TieringError {}
```

**TieringRouter Implementation:**

```rust
pub struct TieringRouter {
    object_metadata: Arc<DashMap<u64, ObjectTierMetadata>>,
    policy: Arc<TieringPolicy>,
    access_trace: Arc<parking_lot::Mutex<VecDeque<AccessRecord>>>,
    metrics: std::sync::Arc<parking_lot::Mutex<TieringMetrics>>,
}

impl TieringRouter {
    pub fn new(policy: TieringPolicy) -> Self {
        Self {
            object_metadata: Arc::new(DashMap::new()),
            policy: Arc::new(policy),
            access_trace: Arc::new(parking_lot::Mutex::new(VecDeque::with_capacity(10000))),
            metrics: Arc::new(parking_lot::Mutex::new(TieringMetrics {
                hot_tier_reads: 0,
                cold_tier_reads: 0,
                prefetch_hits: 0,
                prefetch_misses: 0,
                promotions: 0,
                demotions: 0,
            })),
        }
    }

    pub async fn record_access(
        &self,
        inode_id: u64,
        offset: u64,
        size: u64,
        protocol: Protocol,
    ) -> Result<AccessRecord, TieringError> {
        use tokio::time::Instant;

        let record = AccessRecord {
            inode_id,
            offset,
            size,
            timestamp_ms: Instant::now().elapsed().as_millis() as u64,
            source: protocol,
        };

        // Update access count
        self.object_metadata
            .entry(inode_id)
            .and_modify(|m| m.access_count += 1);

        // Record in trace
        {
            let mut trace = self.access_trace.lock();
            trace.push_back(record.clone());
            if trace.len() > 10000 {
                trace.pop_front();
            }
        }

        Ok(record)
    }

    pub fn detect_access_pattern(&self, inode_id: u64) -> AccessPattern {
        let trace = self.access_trace.lock();
        let mut last_offset: Option<u64> = None;
        let mut is_sequential = true;
        let mut count = 0;

        for record in trace.iter().rev().take(20) {
            if record.inode_id != inode_id {
                continue;
            }

            count += 1;
            if let Some(last) = last_offset {
                let diff = if record.offset > last {
                    record.offset - last
                } else if last > record.offset {
                    last - record.offset
                } else {
                    0
                };

                if diff > 1024 * 1024 {
                    // Large jumps suggest random or streaming
                    is_sequential = false;
                }
            }
            last_offset = Some(record.offset);
        }

        match count {
            0 => AccessPattern::Unknown,
            1..=2 => AccessPattern::Unknown,
            _ if is_sequential => AccessPattern::Sequential,
            _ => AccessPattern::Random,
        }
    }

    pub fn get_tier_hint(&self, inode_id: u64) -> TierHint {
        if let Some(metadata) = self.object_metadata.get(&inode_id) {
            let tier = match metadata.access_count {
                0..=10 => StorageTier::Cold,
                11..=100 => StorageTier::Warm,
                _ => StorageTier::Hot,
            };

            let reason = match tier {
                StorageTier::Hot => "frequent_access".to_string(),
                StorageTier::Warm => "moderate_access".to_string(),
                StorageTier::Cold => "infrequent_access".to_string(),
            };

            TierHint {
                tier,
                reason,
                confidence: 0.8,
            }
        } else {
            TierHint {
                tier: StorageTier::Cold,
                reason: "new_object".to_string(),
                confidence: 0.5,
            }
        }
    }

    pub async fn promote_to_hot(
        &self,
        inode_id: u64,
    ) -> Result<(), TieringError> {
        self.object_metadata
            .alter(&inode_id, |_k, mut v| {
                v.current_tier = StorageTier::Hot;
                v
            });

        let mut metrics = self.metrics.lock();
        metrics.promotions += 1;

        Ok(())
    }

    pub async fn demote_to_cold(
        &self,
        inode_id: u64,
    ) -> Result<(), TieringError> {
        self.object_metadata
            .alter(&inode_id, |_k, mut v| {
                v.current_tier = StorageTier::Cold;
                v
            });

        let mut metrics = self.metrics.lock();
        metrics.demotions += 1;

        Ok(())
    }

    pub fn compute_prefetch_list(&self, inode_id: u64, current_offset: u64) -> Vec<(u64, u64)> {
        let mut prefetch_list = Vec::new();
        let prefetch_distance = self.policy.prefetch_distance_kb * 1024;

        // Simple linear prefetch for sequential patterns
        let offset_base = (current_offset / prefetch_distance) * prefetch_distance;
        for i in 0..4 {
            let offset = offset_base + (i * prefetch_distance);
            prefetch_list.push((offset, 64 * 1024)); // 64KB chunks
        }

        if let Some(metadata) = self.object_metadata.get(&inode_id) {
            if metadata.size_bytes > 0 && prefetch_list[3].0 > metadata.size_bytes {
                prefetch_list.truncate(3);
            }
        }

        prefetch_list
    }

    pub fn current_tier(&self, inode_id: u64) -> Option<StorageTier> {
        self.object_metadata.get(&inode_id).map(|m| m.current_tier)
    }

    pub fn metrics(&self) -> TieringMetrics {
        self.metrics.lock().clone()
    }
}

impl Default for TieringRouter {
    fn default() -> Self {
        Self::new(TieringPolicy {
            promotion_threshold: 50,
            demotion_threshold_ms: 86400000, // 1 day
            prefetch_distance_kb: 256,
            cold_tier_cost_us: 5000,
        })
    }
}
```

**Test Module:** Add 30+ unit tests covering:
- Record sequential accesses (4)
- Record random accesses (4)
- Detect sequential patterns (4)
- Detect random patterns (3)
- Compute prefetch lists (3)
- Promote to hot tier (3)
- Demote to cold tier (3)
- Get tier hints (2)
- Metrics tracking (2)

---

### 4. gateway_observability.rs (~25 tests)

**Purpose:** OpenTelemetry span instrumentation, per-protocol latency tracking.

**File:** `/home/cfs/claudefs/crates/claudefs-gateway/src/gateway_observability.rs`

**Key Types:**

```rust
use crate::protocol::Protocol;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct TraceId([pub u8; 16]);

impl TraceId {
    pub fn new() -> Self {
        use rand::RngCore;
        let mut id = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut id);
        TraceId(id)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SpanId([u8; 8]);

impl SpanId {
    pub fn new() -> Self {
        use rand::RngCore;
        let mut id = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut id);
        SpanId(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SpanStatus {
    Ok,
    Error,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone)]
pub struct ProtocolSpan {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
    pub protocol: Protocol,
    pub operation: String,
    pub client_id: u64,
    pub inode_id: u64,
    pub start_time_ns: u64,
    pub end_time_ns: u64,
    pub status: SpanStatus,
    pub events: Vec<SpanEvent>,
}

#[derive(Debug, Clone)]
pub struct OpMetrics {
    pub op_name: String,
    pub count: u64,
    pub total_latency_ns: u64,
    pub min_latency_ns: u64,
    pub max_latency_ns: u64,
    pub errors: u64,
}

#[derive(Debug, Clone)]
pub struct ProtocolMetrics {
    pub protocol: Protocol,
    pub total_ops: u64,
    pub total_latency_ns: u64,
    pub total_errors: u64,
}

#[derive(Debug, Clone)]
pub struct GlobalMetrics {
    pub total_requests: u64,
    pub total_errors: u64,
    pub total_latency_ns: u64,
}

#[derive(Debug)]
pub enum ObservabilityError {
    SpanNotFound,
    AggregationFailed,
    InvalidTrace,
}

impl std::fmt::Display for ObservabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::SpanNotFound => write!(f, "Span not found"),
            Self::AggregationFailed => write!(f, "Aggregation failed"),
            Self::InvalidTrace => write!(f, "Invalid trace"),
        }
    }
}

impl std::error::Error for ObservabilityError {}
```

**GatewayObserver Implementation:**

```rust
pub struct GatewayObserver {
    span_buffer: Arc<DashMap<TraceId, Vec<ProtocolSpan>>>,
    per_protocol_metrics: Arc<DashMap<Protocol, ProtocolMetrics>>,
    global_metrics: std::sync::Arc<parking_lot::Mutex<GlobalMetrics>>,
}

impl GatewayObserver {
    pub fn new() -> Self {
        Self {
            span_buffer: Arc::new(DashMap::new()),
            per_protocol_metrics: Arc::new(DashMap::new()),
            global_metrics: Arc::new(parking_lot::Mutex::new(GlobalMetrics {
                total_requests: 0,
                total_errors: 0,
                total_latency_ns: 0,
            })),
        }
    }

    pub fn start_operation_span(
        &self,
        trace_id: TraceId,
        protocol: Protocol,
        operation: &str,
        client_id: u64,
        inode_id: u64,
    ) -> ProtocolSpan {
        let start_time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        ProtocolSpan {
            trace_id,
            span_id: SpanId::new(),
            parent_span_id: None,
            protocol,
            operation: operation.to_string(),
            client_id,
            inode_id,
            start_time_ns,
            end_time_ns: 0,
            status: SpanStatus::Ok,
            events: Vec::new(),
        }
    }

    pub fn record_event(
        &self,
        trace_id: TraceId,
        event_name: &str,
    ) -> Result<(), ObservabilityError> {
        let timestamp_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        if let Some(mut spans) = self.span_buffer.get_mut(&trace_id) {
            if let Some(span) = spans.last_mut() {
                span.events.push(SpanEvent {
                    name: event_name.to_string(),
                    timestamp_ns,
                });
                return Ok(());
            }
        }

        Err(ObservabilityError::SpanNotFound)
    }

    pub fn end_operation_span(
        &self,
        trace_id: TraceId,
        mut span: ProtocolSpan,
        status: SpanStatus,
    ) -> Result<(), ObservabilityError> {
        let end_time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let latency_ns = end_time_ns - span.start_time_ns;
        span.end_time_ns = end_time_ns;
        span.status = status.clone();

        self.span_buffer
            .entry(trace_id)
            .or_insert_with(Vec::new)
            .push(span.clone());

        // Update per-protocol metrics
        self.per_protocol_metrics
            .entry(span.protocol)
            .or_insert_with(|| ProtocolMetrics {
                protocol: span.protocol,
                total_ops: 0,
                total_latency_ns: 0,
                total_errors: 0,
            })
            .and_modify(|m| {
                m.total_ops += 1;
                m.total_latency_ns += latency_ns;
                if status != SpanStatus::Ok {
                    m.total_errors += 1;
                }
            });

        // Update global metrics
        {
            let mut global = self.global_metrics.lock();
            global.total_requests += 1;
            global.total_latency_ns += latency_ns;
            if status != SpanStatus::Ok {
                global.total_errors += 1;
            }
        }

        Ok(())
    }

    pub fn get_protocol_metrics(&self, protocol: Protocol) -> Option<ProtocolMetrics> {
        self.per_protocol_metrics.get(&protocol).map(|m| m.clone())
    }

    pub fn global_metrics(&self) -> GlobalMetrics {
        self.global_metrics.lock().clone()
    }

    pub async fn flush_to_aggregator(&self) -> Result<usize, ObservabilityError> {
        let count = self.span_buffer.len();
        self.span_buffer.clear();
        Ok(count)
    }
}

impl Default for GatewayObserver {
    fn default() -> Self {
        Self::new()
    }
}

// RAII guard for automatic span completion
pub struct OperationSpanGuard {
    observer: Arc<GatewayObserver>,
    span: ProtocolSpan,
    trace_id: TraceId,
    completed: Arc<parking_lot::Mutex<bool>>,
}

impl OperationSpanGuard {
    pub fn new(observer: Arc<GatewayObserver>, trace_id: TraceId, span: ProtocolSpan) -> Self {
        Self {
            observer,
            span,
            trace_id,
            completed: Arc::new(parking_lot::Mutex::new(false)),
        }
    }

    pub fn set_status(&mut self, status: SpanStatus) {
        self.span.status = status;
    }
}

impl Drop for OperationSpanGuard {
    fn drop(&mut self) {
        let mut completed = self.completed.lock();
        if !*completed {
            let _ = self.observer.end_operation_span(
                self.trace_id,
                self.span.clone(),
                self.span.status.clone(),
            );
            *completed = true;
        }
    }
}
```

**Test Module:** Add 25+ unit tests covering:
- Create and complete spans (4)
- Record events within spans (3)
- Per-protocol metrics (4)
- Per-operation latency (3)
- Global metrics aggregation (3)
- Span cleanup (2)
- Concurrent span operations (2)
- Error handling (2)

---

## Module Registration

Add these 4 lines to `/home/cfs/claudefs/crates/claudefs-gateway/src/lib.rs` after the last existing `pub mod` declaration:

```rust
/// NFSv4 delegation state machine and callback handling.
pub mod nfs_delegation_manager;
/// Cross-protocol consistency detection and conflict resolution.
pub mod cross_protocol_consistency;
/// Tiered storage routing (hot/warm/cold) and access pattern detection.
pub mod tiered_storage_router;
/// OpenTelemetry span instrumentation and per-protocol latency tracking.
pub mod gateway_observability;
```

---

## Build & Test Validation

After implementation:

```bash
cd /home/cfs/claudefs
cargo build -p claudefs-gateway 2>&1 | tail -30
cargo test -p claudefs-gateway --lib 2>&1 | tail -50
```

**Expected Results:**
- All 4 modules compile cleanly (only warnings for unused code acceptable)
- Total test count: **1200+** (1128 Phase 2 + 72+ Phase 3)
- 0 test failures
- No compilation errors

---

## Code Style Checklist

- [x] Use `thiserror::Error` for all error enums
- [x] Use `Arc<DashMap<...>>` for concurrent collections
- [x] Use `parking_lot::Mutex` for simple locking
- [x] Derive `Debug, Clone` for public types
- [x] Add comprehensive `#[cfg(test)]` modules
- [x] Document public APIs with doc comments
- [x] No unsafe code (only in FFI boundaries)
- [x] Follow existing gateway code conventions

