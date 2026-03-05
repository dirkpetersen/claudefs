# A7 Phase 3: Advanced Gateway Features (NFSv4 Delegation, Cross-Protocol Consistency, Tiered Storage)

**Target:** 1200+ tests (72+ new tests), 4-5 new modules

**Status:** Phase 2 COMPLETE (1128 tests) → Phase 3 PLANNING

---

## Context & Integration Points

### A7 Current State (Phase 2 Complete)
- **54 source files, ~29.9k LOC, 1128 tests passing**
- **Protocols:** NFSv3 (fully implemented), NFSv4 (session management), pNFS (round-robin), S3 (full), SMB3 (Samba VFS stubs)
- **Infrastructure:** Connection pooling, circuit breaker, health checks, quota enforcement, XDR marshaling
- **Key Crate Location:** `/home/cfs/claudefs/crates/claudefs-gateway/src/`

### A2 (claudefs-meta) API Available
From `/home/cfs/claudefs/crates/claudefs-meta/src/`:
- **ClientSession:** Per-client session state machine (Active/Idle/Expired/Revoked) + lease tracking
  - `pub struct ClientSession { session_id, client_id, created_on_node, lease_expiry, pending_ops }`
  - Type exported at `crates/claudefs-meta/src/lib.rs:135`
- **LeaseManager:** POSIX lease caching for concurrent access detection
  - `pub struct LeaseManager, pub struct LeaseType` (lib.rs:177)
  - File: `lease.rs` — **key for detecting cross-protocol conflicts**
- **DistributedTransactionEngine:** 2-phase commit for atomic multi-inode operations
  - `pub struct DistributedTransactionEngine` (lib.rs:218)
  - File: `distributed_transaction.rs` — **key for rename/link atomicity**
- **QosManager:** Per-tenant rate limiting and QoS enforcement
  - `pub struct QosManager, pub enum QosClass` (lib.rs:192)
  - File: `qos.rs`
- **MetadataNode:** Core entry point with 20+ POSIX operations
  - `pub struct MetadataNode` (node.rs:99)
  - Methods: create_file, mkdir, lookup, getattr, setattr, readdir, readdir_plus, unlink, rmdir, rename, etc. (node.rs:125+)
- **RPC Types:** `pub enum MetadataRequest, pub enum MetadataResponse` (rpc.rs:16/251)
  - Helper: `is_read_only(request) -> bool` (rpc.rs:223)

### A4 (claudefs-transport) API Available
From `/home/cfs/claudefs/crates/claudefs-transport/src/`:
- **TraceAggregator (Phase 12):** Distributed tracing with OTEL
  - `pub struct TraceId([u8; 16])` — unique per request
  - `pub struct SpanRecord` — individual operation span
  - `pub struct TraceData` — collected trace with latency stats (min/max/mean/p50/p99)
  - `pub struct TraceAggregator` — central collection point
  - File: `trace_aggregator.rs`
  - **Key Methods:** `record_span(trace_id, span)`, `complete_trace(trace_id) -> Option<TraceData>`, `export_batch() -> Vec<TraceData>`, `stats()`
  - Config: `max_traces_in_flight: 10_000`, `trace_timeout_ms: 30_000`
- **BandwidthShaper (Phase 12):** Token bucket QoS enforcement
  - `pub struct BandwidthAllocation { tenant_id, bytes_per_sec, burst_bytes, enforcement_mode }`
  - `pub struct TokenBucket { try_consume(tokens) -> bool, available() -> u64 }`
  - `pub struct BandwidthShaper` — enforcement engine
  - File: `bandwidth_shaper.rs`
  - **Key Methods:** Register allocations, enforce limits
- **AdaptiveRouter (Phase 12):** Endpoint selection based on health/latency
  - `pub struct EndpointMetrics { rtt_p50_us, rtt_p99_us, availability, queue_depth, healthy }`
  - `pub struct RoutingDecision { primary_endpoint, failover_endpoints, score }`
  - `pub struct AdaptiveRouter` — routing engine
  - File: `adaptive_router.rs`
  - **Key Methods:** Compute routing decisions based on endpoint health
- **ConnectionPool:** Multi-peer connection pooling
  - `pub struct ConnectionPool` (connection.rs:34)
  - Methods: `get_connection(addr) -> Arc<TcpConnection>`, `return_connection(addr, conn)`, `remove_peer(addr)`, `stats()`
- **RpcClient:** Low-level request/response
  - `pub struct RpcClient` (rpc.rs:32)
  - Methods: `call(opcode, payload) -> Result<Frame>`, `call_one_way(opcode, payload)`
  - Config: `response_timeout_ms: 5000`
- **TransportClient:** High-level availability gating
  - `pub struct TransportClient` (client.rs:37)
  - Methods: `is_available() -> bool`, `pre_request() -> Result<()>`, `post_request_success(latency)`, `post_request_failure()`
  - Accessors: `health()`, `circuit_breaker()`, `metrics()`, `flow_controller()`

---

## Phase 3 Module Specifications

### 1. **nfs_delegation_manager.rs** (~30-35 tests)

**Purpose:** NFSv4 delegation state machine with callback handling

**Key Types:**
```rust
pub struct DelegationId(u64);  // Unique delegation identifier
pub struct DelegationCookie([u8; 8]);  // For NFS stateid_other field

pub enum DelegationType {
    Open,      // OPEN delegation (write for exclusive, read for shared)
    ReadWrite,
    Read,
}

pub enum DelegationState {
    Granted(DelegationCookie, Timestamp),  // cookie + grant time
    Recalled(Timestamp),                    // Recall issued at time
    Revoked(Timestamp),                     // Client didn't respond or conflict
}

pub struct ActiveDelegation {
    id: DelegationId,
    client_id: ClientId,
    inode_id: InodeId,
    delegation_type: DelegationType,
    state: DelegationState,
    lease_expiry: Timestamp,
    conflicting_op: Option<String>,  // e.g., "setattr from other client"
}

pub struct DelegationManager {
    delegations: Arc<DashMap<DelegationId, ActiveDelegation>>,
    client_delegations: Arc<DashMap<ClientId, Vec<DelegationId>>>,
    inode_delegations: Arc<DashMap<InodeId, Vec<DelegationId>>>,
    metrics: DelegationMetrics,
}

pub struct DelegationMetrics {
    total_granted: u64,
    total_recalled: u64,
    total_revoked: u64,
    active_delegations: u64,
    recall_latency_ms: Vec<u64>,  // histogram for callback time
}
```

**Methods:**
```rust
impl DelegationManager {
    pub fn new() -> Self;

    // Grant a new delegation
    pub async fn grant_delegation(
        &self,
        client_id: ClientId,
        inode_id: InodeId,
        delegation_type: DelegationType,
        lease_duration_secs: u64,
    ) -> Result<ActiveDelegation>;

    // Check if delegation still valid
    pub fn is_delegation_valid(&self, delegation_id: DelegationId) -> bool;

    // Get delegation state
    pub fn get_delegation(&self, delegation_id: DelegationId) -> Option<ActiveDelegation>;

    // Recall all delegations for an inode (conflict detected)
    pub async fn recall_by_inode(&self, inode_id: InodeId) -> Result<Vec<DelegationId>>;

    // Recall specific client's delegations (client disconnect)
    pub async fn recall_by_client(&self, client_id: ClientId) -> Result<Vec<DelegationId>>;

    // Process client's DELEGRETURN to remove expired delegation
    pub fn process_delegation_return(&self, delegation_id: DelegationId) -> Result<()>;

    // Cleanup expired delegations (grace period enforcer)
    pub async fn cleanup_expired(&self) -> Result<usize>;  // Returns count cleaned

    // Get metrics for monitoring
    pub fn metrics(&self) -> DelegationMetrics;
}
```

**Test Coverage (~30 tests):**
- Grant and validate delegations (6 tests)
- Recall by inode (conflict detection) (4 tests)
- Recall by client (4 tests)
- DELEGRETURN processing (3 tests)
- Lease expiry and cleanup (4 tests)
- Metrics tracking (3 tests)
- Concurrent delegation ops (3 tests)
- Grace period enforcement (2 tests)

**Integration Points:**
- **ClientSession (A2):** Query active leases for grace period
- **LeaseManager (A2):** Coordinate with existing lease tracking
- **gateway_metrics.rs:** Export delegation metrics to Prometheus
- **Dependency:** `Arc`, `DashMap`, `tokio::time::Instant`, `thiserror`

---

### 2. **cross_protocol_consistency.rs** (~30-35 tests)

**Purpose:** Detect and resolve conflicts when NFS/S3/SMB access same inode

**Key Types:**
```rust
pub struct ProtocolAccessRecord {
    protocol: Protocol,      // NFS, S3, SMB
    client_id: u64,          // Client identifier
    inode_id: InodeId,
    access_type: AccessType,  // Read, Write, Delete
    timestamp: Timestamp,
    request_id: u64,         // For tracing
}

pub enum AccessType {
    Read,
    Write(WriteOp),  // SetAttr, Write, Rename
    Delete,
    Metadata,  // XAttr, ACL changes
}

pub enum WriteOp {
    SetSize,
    SetTimes,
    SetMode,
    Write,
    Rename,
    Delete,
}

pub enum ConflictType {
    ReadWrite,               // NFS reader, S3 writer same inode
    ConcurrentWrites,        // Multiple writers (NFS + S3)
    RenameUnderAccess,       // Rename while being read
    DeleteUnderAccess,       // Delete while being read
}

pub struct ConflictRecord {
    conflict_id: u64,
    conflict_type: ConflictType,
    accesses: [ProtocolAccessRecord; 2],  // The conflicting ops
    detected_at: Timestamp,
    resolution: ConflictResolution,
}

pub enum ConflictResolution {
    LastWriteWins,           // Newer timestamp wins
    AbortRequest(Protocol),  // Older request aborted
    RevokeDelegation,        // NFSv4 delegation revoked
    ClientNotified,          // Client callback sent
}

pub struct CrossProtocolCache {
    recent_accesses: Arc<DashMap<InodeId, VecDeque<ProtocolAccessRecord>>>,
    conflicts: Arc<DashMap<u64, ConflictRecord>>,
    metrics: CrossProtocolMetrics,
}

pub struct CrossProtocolMetrics {
    total_accesses: u64,
    conflicts_detected: u64,
    conflicts_resolved: u64,
    resolution_latency_us: Vec<u64>,
}
```

**Methods:**
```rust
impl CrossProtocolCache {
    pub fn new(window_size: usize) -> Self;

    // Record access from any protocol
    pub async fn record_access(
        &self,
        protocol: Protocol,
        client_id: u64,
        inode_id: InodeId,
        access_type: AccessType,
        request_id: u64,
    ) -> Result<Option<ConflictRecord>>;

    // Check if inode has concurrent writes
    pub fn has_concurrent_writes(&self, inode_id: InodeId) -> bool;

    // Get all accesses to an inode in time window
    pub fn get_access_history(
        &self,
        inode_id: InodeId,
        lookback_ms: u64,
    ) -> Vec<ProtocolAccessRecord>;

    // Detect conflict between two accesses
    pub fn detect_conflict(
        rec1: &ProtocolAccessRecord,
        rec2: &ProtocolAccessRecord,
    ) -> Option<ConflictType>;

    // Resolve conflict (update metadata timestamp, invalidate caches)
    pub async fn resolve_conflict(
        &self,
        conflict: ConflictRecord,
        metadata: &MetadataNode,
    ) -> Result<ConflictResolution>;

    // Get metrics
    pub fn metrics(&self) -> CrossProtocolMetrics;

    // Cleanup old records
    pub async fn cleanup_old(&self, older_than_ms: u64) -> Result<usize>;
}

// Helper for cache invalidation
pub async fn invalidate_caches_for_inode(
    inode_id: InodeId,
    gateways: &[&GatewayContext],  // NFSv3, pNFS, S3, SMB
) -> Result<()>;
```

**Test Coverage (~30 tests):**
- Record single-protocol accesses (4 tests)
- Detect read-write conflicts (4 tests)
- Detect concurrent writes (4 tests)
- Detect rename/delete conflicts (3 tests)
- Resolve conflicts with last-write-wins (4 tests)
- Revoke NFSv4 delegations on conflict (3 tests)
- Cache invalidation across protocols (3 tests)
- Metrics tracking (3 tests)

**Integration Points:**
- **DelegationManager:** Revoke delegations on conflicts
- **nfs_cache.rs:** Invalidate NFS attribute cache
- **s3.rs:** Invalidate S3 object metadata cache
- **LeaseManager (A2):** Coordinate lease-based consistency
- **Dependency:** `tokio::time`, `VecDeque`, `DashMap`, `arc`

---

### 3. **tiered_storage_router.rs** (~25-30 tests)

**Purpose:** Route reads based on tier (hot NVMe ↔ cold S3), manage prefetch

**Key Types:**
```rust
pub enum StorageTier {
    Hot,      // NVMe — direct fast path
    Warm,     // Cached in memory
    Cold,     // S3 — fetch on demand
}

pub enum AccessPattern {
    Sequential,    // Detect from recent ops: offset increasing
    Random,        // Random offsets
    Streaming,     // Large sequential reads
    Unknown,
}

pub struct TierHint {
    tier: StorageTier,
    reason: String,  // "frequent_access", "large_size", "old_data"
    confidence: f64, // 0.0-1.0
}

pub struct ObjectTierMetadata {
    inode_id: InodeId,
    object_key: String,
    current_tier: StorageTier,
    access_pattern: AccessPattern,
    last_access: Timestamp,
    access_count: u64,
    size_bytes: u64,
    promoted_at: Option<Timestamp>,
    demoted_at: Option<Timestamp>,
}

pub struct TieringPolicy {
    promotion_threshold: u64,      // accesses to promote from S3 → memory
    demotion_threshold: u64,       // time_ms to demote from memory → S3
    prefetch_distance_kb: u64,     // Look-ahead for sequential reads
    cold_tier_cost_us: u64,        // Latency to S3 (for routing decisions)
}

pub struct TieringRouter {
    object_metadata: Arc<DashMap<InodeId, ObjectTierMetadata>>,
    policy: Arc<TieringPolicy>,
    access_trace: Arc<VecDeque<AccessRecord>>,
    metrics: TieringMetrics,
}

pub struct AccessRecord {
    inode_id: InodeId,
    offset: u64,
    size: u64,
    timestamp: Timestamp,
    source: Protocol,  // NFS, S3, etc.
}

pub struct TieringMetrics {
    hot_tier_reads: u64,
    cold_tier_reads: u64,
    prefetch_hits: u64,
    prefetch_misses: u64,
    promotions: u64,
    demotions: u64,
    tier_change_latency_us: Vec<u64>,
}
```

**Methods:**
```rust
impl TieringRouter {
    pub fn new(policy: TieringPolicy) -> Self;

    // Record an access (read or write)
    pub async fn record_access(
        &self,
        inode_id: InodeId,
        offset: u64,
        size: u64,
        protocol: Protocol,
    ) -> Result<AccessRecord>;

    // Detect access pattern (sequential, random, etc.)
    pub fn detect_access_pattern(&self, inode_id: InodeId) -> AccessPattern;

    // Get tier hint for routing a read
    pub fn get_tier_hint(&self, inode_id: InodeId) -> TierHint;

    // Promote object from S3 to memory (triggers prefetch)
    pub async fn promote_to_hot(
        &self,
        inode_id: InodeId,
        transport: &TransportClient,
    ) -> Result<()>;

    // Demote object from memory to S3 (async write-back)
    pub async fn demote_to_cold(
        &self,
        inode_id: InodeId,
        storage_client: &StorageClient,
    ) -> Result<()>;

    // Compute prefetch list (for sequential reads)
    pub fn compute_prefetch_list(
        &self,
        inode_id: InodeId,
        current_offset: u64,
    ) -> Vec<(u64, u64)>;  // Vec<(offset, size)>

    // Get current tier for object
    pub fn current_tier(&self, inode_id: InodeId) -> Option<StorageTier>;

    // Get metrics
    pub fn metrics(&self) -> TieringMetrics;
}
```

**Test Coverage (~25 tests):**
- Record and detect sequential patterns (4 tests)
- Record and detect random patterns (4 tests)
- Compute prefetch lists for sequential reads (3 tests)
- Promote hot objects from S3 (3 tests)
- Demote cold objects to S3 (3 tests)
- Tier hint computation (3 tests)
- Metrics tracking (2 tests)

**Integration Points:**
- **s3.rs:** Route cold reads to S3
- **A1 Storage (claudefs-storage):** Prefetch warm data to NVMe
- **gateway_metrics.rs:** Export tiering metrics
- **Dependency:** `tokio::time`, `DashMap`, `VecDeque`, `Arc`

---

### 4. **gateway_observability.rs** (~20-25 tests)

**Purpose:** OpenTelemetry span instrumentation, per-protocol latency tracking

**Key Types:**
```rust
pub struct ProtocolSpan {
    trace_id: TraceId,         // From A4 trace_aggregator
    span_id: [u8; 8],
    parent_span_id: Option<[u8; 8]>,
    protocol: Protocol,
    operation: String,         // "READ", "WRITE", "MKDIR", etc.
    client_id: u64,
    inode_id: InodeId,
    start_time_ns: u64,
    end_time_ns: u64,
    status: SpanStatus,
    attributes: Vec<(String, String)>,
    events: Vec<SpanEvent>,
}

pub enum SpanStatus {
    Ok,
    Error(String),
    Cancelled,
}

pub struct SpanEvent {
    name: String,
    timestamp_ns: u64,
    attributes: Vec<(String, String)>,
}

pub struct GatewayObserver {
    trace_aggregator: Arc<TraceAggregator>,  // From A4
    span_buffer: Arc<DashMap<TraceId, Vec<ProtocolSpan>>>,
    per_protocol_metrics: Arc<DashMap<Protocol, ProtocolMetrics>>,
    global_metrics: GlobalMetrics,
}

pub struct ProtocolMetrics {
    protocol: Protocol,
    operations: DashMap<String, OpMetrics>,  // Op → latency stats
}

pub struct OpMetrics {
    op_name: String,
    count: u64,
    latency_ns: LatencyHistogram,  // min, max, mean, p50, p99
    errors: u64,
}

pub struct LatencyHistogram {
    min_ns: u64,
    max_ns: u64,
    mean_ns: f64,
    p50_ns: u64,
    p99_ns: u64,
}

pub struct GlobalMetrics {
    total_requests: u64,
    total_errors: u64,
    total_latency_ns: u64,
    critical_path_latency: Vec<u64>,  // Slowest operations
}
```

**Methods:**
```rust
impl GatewayObserver {
    pub fn new(trace_aggregator: Arc<TraceAggregator>) -> Self;

    // Create a new span for an incoming operation
    pub fn start_operation_span(
        &self,
        protocol: Protocol,
        operation: &str,
        client_id: u64,
        inode_id: InodeId,
    ) -> OperationSpanGuard;

    // Record a sub-operation (e.g., metadata lookup during NFS read)
    pub fn record_event(
        &self,
        trace_id: TraceId,
        event_name: &str,
        attributes: Vec<(String, String)>,
    ) -> Result<()>;

    // Complete a span and record latency
    pub fn end_operation_span(
        &self,
        trace_id: TraceId,
        status: SpanStatus,
    ) -> Result<()>;

    // Export all spans to A4 trace_aggregator
    pub async fn flush_to_aggregator(&self) -> Result<usize>;  // Returns count

    // Get per-protocol metrics
    pub fn get_protocol_metrics(&self, protocol: Protocol) -> ProtocolMetrics;

    // Get latency histogram for specific operation
    pub fn get_operation_latency(
        &self,
        protocol: Protocol,
        operation: &str,
    ) -> Option<OpMetrics>;

    // Get global metrics
    pub fn global_metrics(&self) -> GlobalMetrics;
}

// RAII guard to auto-complete spans
pub struct OperationSpanGuard {
    observer: Arc<GatewayObserver>,
    trace_id: TraceId,
    protocol: Protocol,
    operation: String,
    status: Arc<Mutex<SpanStatus>>,
}

impl Drop for OperationSpanGuard {
    fn drop(&mut self) {
        // Auto-complete span on drop
        let _ = self.observer.end_operation_span(self.trace_id, self.status.lock().unwrap().clone());
    }
}
```

**Test Coverage (~20 tests):**
- Create and complete operation spans (4 tests)
- Record events within spans (3 tests)
- Per-protocol latency tracking (4 tests)
- Per-operation latency stats (3 tests)
- Export spans to A4 aggregator (3 tests)
- Concurrent span operations (2 tests)

**Integration Points:**
- **A4 TraceAggregator:** Export completed traces
- **gateway_metrics.rs:** Publish Prometheus metrics
- **Dependency:** `tokio::sync::Mutex`, `Arc`, `DashMap`, `trace_aggregator` (from A4)

---

## Implementation Notes

### Build & Test Validation
```bash
# Build
cd /home/cfs/claudefs && cargo build -p claudefs-gateway

# Test (all 4 modules)
cargo test -p claudefs-gateway --lib 2>&1 | tail -20

# Expected: ~1200+ tests passing
# Test time: ~2-3 seconds
```

### File Placement
All files go into: `/home/cfs/claudefs/crates/claudefs-gateway/src/`

### Module Registration
Update `lib.rs` to export new modules:
```rust
pub mod nfs_delegation_manager;
pub mod cross_protocol_consistency;
pub mod tiered_storage_router;
pub mod gateway_observability;
```

### Dependencies
All modules use existing workspace dependencies:
- `tokio` — async runtime
- `arc`, `DashMap` — concurrent data structures
- `thiserror` — error handling
- `tracing` — observability (tracing spans)
- `claudefs-transport` — TraceAggregator, TraceId types
- `claudefs-meta` — ClientSession, LeaseManager, MetadataNode

### Error Handling
Define error enum for each module:
```rust
#[derive(thiserror::Error, Debug)]
pub enum DelegationError {
    #[error("delegation expired")]
    Expired,
    #[error("lease conflict")]
    LeaseConflict,
    // ... more variants
}
```

---

## Acceptance Criteria

✅ All 4 modules compile without warnings
✅ ~72+ new tests added (30 + 30 + 25 + 20 = 105 minimum)
✅ Total test count: 1200+ (1128 + 72+)
✅ 100% of new code paths covered by tests
✅ Integration with A2 (MetadataNode, ClientSession, LeaseManager)
✅ Integration with A4 (TraceAggregator)
✅ No breaking changes to existing gateway APIs
✅ `cargo clippy` passes with no warnings on new code

---

## Model Selection

Use: `fireworks-ai/accounts/fireworks/models/minimax-m2p5` (default)
Fallback: `fireworks-ai/accounts/fireworks/models/glm-5` if timeouts
