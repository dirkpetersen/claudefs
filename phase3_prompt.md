# A7 Phase 3: Advanced Gateway Features Implementation

## Context

The gateway crate is at `/home/cfs/claudefs/crates/claudefs-gateway/` with ~54 source files, ~29.9k LOC, and 1128 tests passing. We're adding 4 new modules in Phase 3.

## Available Integration Points

### From claudefs-meta (A2):
- `ClientSession` - Per-client session state machine (line 224 in lib.rs)
- `LeaseManager`, `LeaseType` - POSIX lease caching (line 177)
- `MetadataNode` - Core metadata operations (line 188)
- `InodeId` - from `types` module

### From claudefs-transport (A4):
- `TraceId`, `SpanRecord`, `TraceData`, `TraceAggregator` from `trace_aggregator.rs`
- `Protocol` enum is defined in gateway's `protocol.rs` (use existing)

## Implementation Requirements

Create 4 new modules in `/home/cfs/claudefs/crates/claudefs-gateway/src/`:

### 1. nfs_delegation_manager.rs (~30-35 tests)
NFSv4 delegation state machine with callback handling.

**Types to implement:**
- `DelegationId(u64)` - Unique delegation identifier
- `DelegationCookie([u8; 8])` - For NFS stateid_other field
- `DelegationType` enum: Open, ReadWrite, Read
- `DelegationState` enum: Granted, Recalled, Revoked (with timestamps)
- `ActiveDelegation` struct with id, client_id, inode_id, delegation_type, state, lease_expiry, conflicting_op
- `DelegationManager` with Arc<DashMap> for delegations, client_delegations, inode_delegations
- `DelegationMetrics` for monitoring

**Methods:**
- `new() -> Self`
- `grant_delegation(client_id, inode_id, delegation_type, lease_duration_secs) -> Result<ActiveDelegation>`
- `is_delegation_valid(delegation_id) -> bool`
- `get_delegation(delegation_id) -> Option<ActiveDelegation>`
- `recall_by_inode(inode_id) -> Result<Vec<DelegationId>>`
- `recall_by_client(client_id) -> Result<Vec<DelegationId>>`
- `process_delegation_return(delegation_id) -> Result<()>`
- `cleanup_expired() -> Result<usize>`
- `metrics() -> DelegationMetrics`

**Error enum:** `DelegationError` with variants: Expired, LeaseConflict, NotFound, InvalidState

### 2. cross_protocol_consistency.rs (~30-35 tests)
Detect and resolve conflicts when NFS/S3/SMB access same inode.

**Types to implement:**
- `ProtocolAccessRecord` - protocol, client_id, inode_id, access_type, timestamp, request_id
- `AccessType` enum: Read, Write(WriteOp), Delete, Metadata
- `WriteOp` enum: SetSize, SetTimes, SetMode, Write, Rename, Delete
- `ConflictType` enum: ReadWrite, ConcurrentWrites, RenameUnderAccess, DeleteUnderAccess
- `ConflictRecord` with conflict_id, conflict_type, accesses, detected_at, resolution
- `ConflictResolution` enum: LastWriteWins, AbortRequest, RevokeDelegation, ClientNotified
- `CrossProtocolCache` with recent_accesses, conflicts, metrics

**Methods:**
- `new(window_size: usize) -> Self`
- `record_access(protocol, client_id, inode_id, access_type, request_id) -> Result<Option<ConflictRecord>>`
- `has_concurrent_writes(inode_id) -> bool`
- `get_access_history(inode_id, lookback_ms) -> Vec<ProtocolAccessRecord>`
- `detect_conflict(rec1, rec2) -> Option<ConflictType>`
- `resolve_conflict(conflict, metadata) -> Result<ConflictResolution>`
- `metrics() -> CrossProtocolMetrics`
- `cleanup_old(older_than_ms) -> Result<usize>`

**Error enum:** `ConsistencyError` with variants: InvalidAccess, ResolutionFailed, CacheError

### 3. tiered_storage_router.rs (~25-30 tests)
Route reads based on tier (hot NVMe ↔ cold S3), manage prefetch.

**Types to implement:**
- `StorageTier` enum: Hot, Warm, Cold
- `AccessPattern` enum: Sequential, Random, Streaming, Unknown
- `TierHint` struct: tier, reason, confidence
- `ObjectTierMetadata` with inode_id, object_key, current_tier, access_pattern, last_access, access_count, size_bytes, promoted_at, demoted_at
- `TieringPolicy` struct: promotion_threshold, demotion_threshold, prefetch_distance_kb, cold_tier_cost_us
- `TieringRouter` with object_metadata, policy, access_trace, metrics
- `AccessRecord`: inode_id, offset, size, timestamp, source (Protocol)
- `TieringMetrics`: hot_tier_reads, cold_tier_reads, prefetch_hits, prefetch_misses, promotions, demotions, tier_change_latency_us

**Methods:**
- `new(policy: TieringPolicy) -> Self`
- `record_access(inode_id, offset, size, protocol) -> Result<AccessRecord>`
- `detect_access_pattern(inode_id) -> AccessPattern`
- `get_tier_hint(inode_id) -> TierHint`
- `promote_to_hot(inode_id, transport) -> Result<()>` - stub, don't actually call transport
- `demote_to_cold(inode_id, storage_client) -> Result<()>` - stub
- `compute_prefetch_list(inode_id, current_offset) -> Vec<(u64, u64)>`
- `current_tier(inode_id) -> Option<StorageTier>`
- `metrics() -> TieringMetrics`

**Error enum:** `TieringError` with variants: InvalidTier, PromotionFailed, DemotionFailed, ObjectNotFound

### 4. gateway_observability.rs (~20-25 tests)
OpenTelemetry span instrumentation, per-protocol latency tracking.

**Types to implement:**
- `ProtocolSpan` - trace_id, span_id, parent_span_id, protocol, operation, client_id, inode_id, start_time_ns, end_time_ns, status, attributes, events
- `SpanStatus` enum: Ok, Error(String), Cancelled
- `SpanEvent` - name, timestamp_ns, attributes
- `GatewayObserver` with trace_aggregator (Arc<TraceAggregator> from A4), span_buffer, per_protocol_metrics, global_metrics
- `ProtocolMetrics` - protocol, operations (DashMap<String, OpMetrics>)
- `OpMetrics` - op_name, count, latency_ns (LatencyHistogram), errors
- `LatencyHistogram` - min_ns, max_ns, mean_ns, p50_ns, p99_ns
- `GlobalMetrics` - total_requests, total_errors, total_latency_ns, critical_path_latency
- `OperationSpanGuard` - RAII guard for auto-completing spans on drop

**Methods:**
- `new(trace_aggregator: Arc<TraceAggregator>) -> Self`
- `start_operation_span(protocol, operation, client_id, inode_id) -> OperationSpanGuard`
- `record_event(trace_id, event_name, attributes) -> Result<()>`
- `end_operation_span(trace_id, status) -> Result<()>`
- `flush_to_aggregator() -> Result<usize>`
- `get_protocol_metrics(protocol) -> ProtocolMetrics`
- `get_operation_latency(protocol, operation) -> Option<OpMetrics>`
- `global_metrics() -> GlobalMetrics`

**Error enum:** `ObservabilityError` with variants: SpanNotFound, AggregationFailed, InvalidTrace

## Code Style Requirements

- Use `thiserror::Error` for error enums
- Use `Arc<DashMap<...>>` for concurrent data structures
- Use `tokio::time::Instant` for timestamps where appropriate
- Derive `Debug, Clone` for all public types
- Add `#[cfg(test)]` module with comprehensive tests for each module
- Follow existing gateway code conventions (check existing files in the crate)
- NO external dependencies beyond what's already in Cargo.toml

## Module Registration

Add to lib.rs:
```rust
pub mod nfs_delegation_manager;
pub mod cross_protocol_consistency;
pub mod tiered_storage_router;
pub mod gateway_observability;
```

## Build & Test

Run after implementation:
```bash
cd /home/cfs/claudefs && cargo build -p claudefs-gateway
cargo test -p claudefs-gateway --lib 2>&1 | tail -20
```

Expected: All 4 modules compile, ~72+ new tests, total test count 1200+.
