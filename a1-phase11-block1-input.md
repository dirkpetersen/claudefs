# A1 Phase 11 Block 1: Online Node Scaling & Rebalancing — OpenCode Implementation

**Context:** ClaudeFS Storage Engine (A1), Phase 11 Block 1 implementation. All code must pass `cargo build` and `cargo test --release` with zero clippy warnings.

**Target:** Implement 3 modules (dynamic_join.rs, dynamic_leave.rs, rebalance_orchestrator.rs) with 23 comprehensive tests totaling ~650 LOC.

---

## Architecture Overview

### System Context
- **Crate:** `crates/claudefs-storage`
- **Related Crates:** `claudefs-meta` (for shard management), `claudefs-transport` (for RPC)
- **Key Design Decisions:**
  - D2 (SWIM protocol): New nodes announce via gossip
  - D4 (Multi-Raft): 256 virtual shards per cluster, 3 replicas per shard
  - Consistent hashing: `hash(inode) % num_shards` → shard ID

### Existing Modules in Scope
- **background_scheduler.rs** — Task scheduling (exists, Phase 10)
- **write_journal.rs** — Write durability (exists, Phase 10)
- **block_allocator.rs** — Block allocation (exists, Phase 10)
- **tier_orchestrator.rs** — S3 tiering (exists, Phase 10)
- **resilience_coordinator.rs** — Failure handling (exists, Phase 10)

### Dependencies
- `tokio` — async runtime
- `parking_lot` — efficient locks
- `dashmap` — concurrent HashMap
- `tracing` — distributed tracing
- `thiserror` — error types
- `serde` — serialization
- Standard: `std::sync::{Arc, Mutex, atomic::*}`, `std::collections::{VecDeque, HashMap}`

---

## Module 1: dynamic_join.rs (~200 LOC)

**Purpose:** Handle node join protocol with graceful shard assignment and data migration.

### Public API
```rust
pub struct NodeJoinCoordinator {
    shard_count: usize,
    existing_nodes: Arc<DashMap<NodeId, NodeInfo>>,
    rebalance_progress: Arc<RwLock<RebalanceProgress>>,
}

impl NodeJoinCoordinator {
    pub fn new(shard_count: usize) -> Self { ... }

    /// Announce new node joining cluster
    pub async fn announce_node_join(
        &self,
        node_id: NodeId,
        node_info: NodeInfo,
    ) -> Result<Vec<ShardAssignment>, JoinError> { ... }

    /// Get shards to migrate to new node
    pub fn get_migration_plan(
        &self,
        new_node_id: NodeId,
    ) -> Result<Vec<ShardMigration>, JoinError> { ... }

    /// Confirm shard migration complete
    pub async fn confirm_shard_migration(
        &self,
        new_node_id: NodeId,
        shard_id: ShardId,
    ) -> Result<(), JoinError> { ... }

    /// Get current join progress
    pub fn get_join_progress(&self) -> JoinProgress { ... }
}

// Error type
#[derive(thiserror::Error, Debug)]
pub enum JoinError {
    #[error("node already exists")]
    NodeExists,
    #[error("invalid node info: {0}")]
    InvalidNodeInfo(String),
    #[error("shard assignment failed: {0}")]
    AssignmentFailed(String),
    #[error("migration in progress")]
    MigrationInProgress,
}

// Data structures
pub struct NodeInfo {
    pub node_id: NodeId,
    pub capacity_bytes: u64,
    pub available_bytes: u64,
    pub cpu_cores: u32,
}

pub struct ShardAssignment {
    pub shard_id: ShardId,
    pub primary_node: NodeId,
    pub replica_nodes: Vec<NodeId>,
}

pub struct ShardMigration {
    pub shard_id: ShardId,
    pub source_node: NodeId,
    pub target_node: NodeId,
    pub estimated_bytes: u64,
}

pub struct JoinProgress {
    pub node_id: NodeId,
    pub total_shards_to_migrate: usize,
    pub completed_migrations: usize,
    pub bytes_migrated: u64,
    pub estimated_completion_ms: u64,
}
```

### Implementation Details

**Algorithm: Fair Shard Distribution**
1. List all existing shards + their current replicas
2. For each shard: if new node not a replica, add it as a new replica
3. For heavily loaded nodes: relocate some shards to new node
4. Calculate fair distribution: each node gets ~shard_count/num_nodes shards
5. Generate migration plan (which shards to move where)

**Key Properties:**
- ✅ No client I/O interruption during assignment
- ✅ Incremental migration (shard by shard)
- ✅ Atomic shard assignment via metadata service
- ✅ Recovery: if join fails mid-way, resume cleanly

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: Single node join
    #[tokio::test]
    async fn test_node_join_basic() {
        // Given: cluster with 3 nodes, 256 shards, each node has ~85 shards
        // When: new 4th node joins
        // Then: new node gets ~64 shards assigned, old nodes each lose ~16 shards
        //       distribution is balanced (no node >85 shards after)
    }

    // Test 2: Concurrent node join
    #[tokio::test]
    async fn test_node_join_concurrent() {
        // Given: 3 nodes, 256 shards
        // When: 3 new nodes join simultaneously
        // Then: all assignments succeed, total ~64 shards per node
        //       no conflicts, no data loss
    }

    // Test 3: Incomplete recovery
    #[tokio::test]
    async fn test_node_join_incomplete_recovery() {
        // Given: node join in progress, 100/256 shards migrated
        // When: new node crashes, then rejoins
        // Then: join resumes from shard 100 (not from 0)
        //       no duplicate migrations
    }

    // Test 4: Fair distribution
    #[tokio::test]
    async fn test_shard_assignment_fairness() {
        // Given: 256 shards, 5 nodes before, 1 joins
        // Then: each node has 256/6 = 42.67 ≈ 43 or 42 shards
        //       imbalance < 2 shards per node
    }

    // Test 5: Data integrity
    #[tokio::test]
    async fn test_data_migration_integrity() {
        // Given: shard with data blocks A, B, C
        // When: migrate to new node
        // Then: checksum(A+B+C) same before/after migration
        //       no blocks lost or corrupted
    }

    // Test 6: Client I/O during join
    #[tokio::test]
    async fn test_client_io_during_rebalance() {
        // Given: client I/O active, node join starts
        // When: ongoing write to shard being migrated
        // Then: write succeeds on source (old node)
        //       after migration completes, read goes to target (new node)
        //       no write loss
    }

    // Test 7: Rebalance cancellation
    #[tokio::test]
    async fn test_rebalance_cancellation() {
        // Given: node join in progress, 50% shards migrated
        // When: admin calls cancel_join()
        // Then: remaining migrations paused
        //       cluster can continue operating
        //       admin can resume or rollback
    }

    // Test 8: Progress tracking
    #[tokio::test]
    async fn test_rebalance_progress_tracking() {
        // Given: node join with 256 shards
        // When: 64 shards migrated
        // Then: progress shows (64/256, 25%, estimated_time_remaining)
        //       metrics are accurate to within 1%
    }
}
```

---

## Module 2: dynamic_leave.rs (~200 LOC)

**Purpose:** Handle node leave protocol with graceful draining and data relocation.

### Public API
```rust
pub struct NodeLeaveCoordinator {
    drain_timeout_ms: u64,
    existing_shards: Arc<DashMap<ShardId, ShardInfo>>,
}

impl NodeLeaveCoordinator {
    pub fn new(drain_timeout_ms: u64) -> Self { ... }

    /// Start graceful node leave (drain writes for drain_timeout_ms)
    pub async fn initiate_graceful_leave(
        &self,
        node_id: NodeId,
        drain_timeout_ms: Option<u64>,
    ) -> Result<LeaveStatus, LeaveError> { ... }

    /// Get shards that must be relocated from leaving node
    pub fn get_relocation_plan(
        &self,
        node_id: NodeId,
    ) -> Result<Vec<ShardRelocation>, LeaveError> { ... }

    /// Confirm all data relocated, safe to remove node
    pub async fn confirm_node_removal(
        &self,
        node_id: NodeId,
    ) -> Result<(), LeaveError> { ... }

    /// Abort leave (cancel relocation, keep node in cluster)
    pub async fn abort_leave(
        &self,
        node_id: NodeId,
    ) -> Result<(), LeaveError> { ... }

    /// Check if quorum safe after node removal
    pub fn verify_quorum_safe(
        &self,
        leaving_nodes: &[NodeId],
    ) -> Result<bool, LeaveError> { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum LeaveError {
    #[error("node not found")]
    NodeNotFound,
    #[error("quorum would be lost")]
    QuorumLossPrevented,
    #[error("drain timeout exceeded")]
    DrainTimeoutExceeded,
    #[error("relocation failed: {0}")]
    RelocationFailed(String),
}

pub struct LeaveStatus {
    pub node_id: NodeId,
    pub drain_window_ms: u64,
    pub shards_to_relocate: usize,
    pub bytes_to_relocate: u64,
    pub estimated_completion_ms: u64,
}

pub struct ShardRelocation {
    pub shard_id: ShardId,
    pub source_node: NodeId,
    pub target_node: NodeId,
    pub estimated_bytes: u64,
}
```

### Implementation Details

**Algorithm: Graceful Drain + Relocation**
1. Mark node as "leaving" in cluster membership
2. For drain_timeout_ms: accept new writes to node but don't accept new shard assignments
3. After timeout: no more writes accepted, start relocation
4. For each shard on leaving node: pick replacement node(s)
5. Verify quorum won't be lost after removal
6. Migrate all data, then remove node from cluster

**Key Properties:**
- ✅ 120s default drain window (configurable)
- ✅ Ongoing I/O completes before relocation
- ✅ Quorum safety verified before removal
- ✅ Can abort leave if cluster needs capacity back

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: Graceful drain
    #[tokio::test]
    async fn test_node_leave_graceful_drain() {
        // Given: node with active I/O
        // When: initiate_graceful_leave() called
        // Then: writes continue for 120s
        //       no new shard assignments to leaving node
        //       after 120s, accept no more writes
    }

    // Test 2: Drain timeout
    #[tokio::test]
    async fn test_node_leave_drain_timeout() {
        // Given: drain_timeout = 120s, many pending writes
        // When: timeout reached
        // Then: remaining unflushed data relocated atomically
        //       no data loss
    }

    // Test 3: Client I/O during drain
    #[tokio::test]
    async fn test_node_leave_with_client_io() {
        // Given: node leaving, client I/O ongoing
        // When: drain window active
        // Then: all in-flight I/O completes
        //       after drain ends, no new I/O accepted
    }

    // Test 4: Quorum check
    #[tokio::test]
    async fn test_node_leave_quorum_check() {
        // Given: 3-node Raft group, one node leaving
        // When: verify_quorum_safe() called
        // Then: returns true (2 nodes > quorum of 1)
        // Given: 3-node group, two nodes leaving
        // Then: returns false (1 node = quorum of 1, unsafe)
    }

    // Test 5: Concurrent leave
    #[tokio::test]
    async fn test_node_leave_concurrent() {
        // Given: 5 nodes, 256 shards evenly distributed
        // When: 2 nodes initiated leave simultaneously
        // Then: both relocations proceed without conflict
        //       remaining 3 nodes get balanced load
    }

    // Test 6: Leave with failures
    #[tokio::test]
    async fn test_node_remove_with_failures() {
        // Given: relocation in progress, target node becomes unreachable
        // When: failover to alternate target
        // Then: relocation retries with new target
        //       eventually completes
    }

    // Test 7: Verification
    #[tokio::test]
    async fn test_node_leave_verification() {
        // Given: node leave complete
        // When: verify all shards relocated
        // Then: leaving node has 0 shards assigned
        //       data found on new locations
    }
}
```

---

## Module 3: rebalance_orchestrator.rs (~250 LOC)

**Purpose:** Orchestrate shard rebalancing across cluster (used by join/leave).

### Public API
```rust
pub struct RebalanceOrchestrator {
    shard_count: usize,
    active_rebalances: Arc<DashMap<RebalanceId, RebalanceContext>>,
    bandwidth_limiter: Arc<BandwidthLimiter>,
}

impl RebalanceOrchestrator {
    pub fn new(shard_count: usize, max_bandwidth_mbps: u64) -> Self { ... }

    /// Start rebalancing migration
    pub async fn start_rebalancing(
        &self,
        migrations: Vec<ShardMigration>,
    ) -> Result<RebalanceId, RebalanceError> { ... }

    /// Get rebalance state machine state
    pub fn get_rebalance_state(
        &self,
        rebalance_id: RebalanceId,
    ) -> Result<RebalanceState, RebalanceError> { ... }

    /// Pause rebalancing (can resume later)
    pub async fn pause_rebalancing(
        &self,
        rebalance_id: RebalanceId,
    ) -> Result<(), RebalanceError> { ... }

    /// Resume paused rebalancing
    pub async fn resume_rebalancing(
        &self,
        rebalance_id: RebalanceId,
    ) -> Result<(), RebalanceError> { ... }

    /// Abort rebalancing (rollback all migrations)
    pub async fn abort_rebalancing(
        &self,
        rebalance_id: RebalanceId,
    ) -> Result<(), RebalanceError> { ... }

    /// Update throughput limiter
    pub fn set_bandwidth_limit(&self, mbps: u64) { ... }

    /// Get progress metrics
    pub fn get_progress(
        &self,
        rebalance_id: RebalanceId,
    ) -> Result<RebalanceProgress, RebalanceError> { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum RebalanceError {
    #[error("rebalance not found")]
    NotFound,
    #[error("invalid state transition")]
    InvalidStateTransition,
    #[error("migration failed: {0}")]
    MigrationFailed(String),
    #[error("bandwidth exceeded")]
    BandwidthExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebalanceState {
    Running,
    Paused,
    Completed,
    Failed,
    RolledBack,
}

pub struct RebalanceProgress {
    pub total_shards: usize,
    pub completed_shards: usize,
    pub bytes_migrated: u64,
    pub estimated_bytes_total: u64,
    pub elapsed_ms: u64,
    pub estimated_remaining_ms: u64,
    pub throughput_mbps: f64,
    pub current_state: RebalanceState,
}

pub struct BandwidthLimiter {
    max_mbps: Arc<AtomicU64>,
    current_mbps: Arc<AtomicU64>,
}
```

### Implementation Details

**State Machine: Running → Paused → Running → Completed (or Failed/RolledBack)**
1. Start: initialize all migrations
2. Process migrations concurrently, respecting bandwidth limit
3. Pause: save state, pause in-flight migrations
4. Resume: restore state, continue from last checkpoint
5. Complete: all migrations done, verify data consistency
6. Abort: rollback all changes, restore old replica assignments

**Key Properties:**
- ✅ State transitions validated
- ✅ Pause/resume safe (no data loss)
- ✅ Abort/rollback safe (restore from backups)
- ✅ Bandwidth limited (no network saturation)
- ✅ Progress metrics accurate

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: State machine
    #[tokio::test]
    async fn test_rebalance_orchestration_state_machine() {
        // Given: rebalance created
        // Then: state = Running
        // When: pause() called
        // Then: state = Paused
        // When: resume() called
        // Then: state = Running again
        // When: complete()
        // Then: state = Completed
    }

    // Test 2: Throughput limiting
    #[tokio::test]
    async fn test_rebalance_throughput_limiting() {
        // Given: max_bandwidth = 100 MB/s, 1GB migration
        // When: rebalancing runs
        // Then: measured throughput ≤ 100 MB/s ± 5%
    }

    // Test 3: Pause/resume
    #[tokio::test]
    async fn test_rebalance_pause_and_resume() {
        // Given: rebalance in progress
        // When: pause() at 50%
        // Then: in-flight migration pauses
        // When: resume()
        // Then: resumes from 50% (not from 0%)
    }

    // Test 4: Abort/rollback
    #[tokio::test]
    async fn test_rebalance_abort_and_rollback() {
        // Given: 256 shard migrations, 100 completed
        // When: abort()
        // Then: completed shards rolled back to original replicas
        //       pending migrations cancelled
        //       cluster back to pre-rebalance state
    }

    // Test 5: Priority
    #[tokio::test]
    async fn test_rebalance_priority_queuing() {
        // Given: 2 rebalances queued
        // When: mark rebalance 1 as high-priority
        // Then: rebalance 1 gets 80% bandwidth
        //       rebalance 2 gets 20% bandwidth
    }

    // Test 6: Metrics
    #[tokio::test]
    async fn test_rebalance_metrics_tracking() {
        // Given: rebalance running
        // When: query progress at various points
        // Then: elapsed_ms, throughput_mbps, estimated_remaining_ms accurate
        //       completed_shards matches reality
    }

    // Test 7: Cross-site
    #[tokio::test]
    async fn test_rebalance_cross_site() {
        // Given: 5 nodes site A, 2 nodes site B
        // When: migrate shard from A to B
        // Then: cross-site migration respects WAN link (lower bandwidth)
        //       slower than local migration
    }

    // Test 8: Under load
    #[tokio::test]
    async fn test_rebalance_under_load() {
        // Given: cluster at 80% capacity, rebalancing starts
        // When: new I/O arrives
        // Then: rebalancing adapts bandwidth down (no cluster saturation)
        //       client I/O latency doesn't increase >20%
    }
}
```

---

## Integration Testing

These 3 modules work together:

```
NodeJoinCoordinator
    → assigns_shards_to → RebalanceOrchestrator
        → tracks_migrations → progress_metrics

NodeLeaveCoordinator
    → initiates_drain → (120s window)
    → relocates_shards → RebalanceOrchestrator
        → migrates_data → progress_metrics

RebalanceOrchestrator
    ← used_by_join_and_leave
    → can_pause_resume_abort
    → respects_bandwidth_limits
    → provides_progress_visibility
```

---

## Code Style & Conventions

**Async Runtime:**
- All I/O via Tokio
- `#[tokio::test]` for async tests
- `futures::join_all()` for parallel operations

**Error Handling:**
- Define error type with `#[derive(thiserror::Error)]`
- Return `Result<T, ModuleError>` from fallible functions
- Use `?` operator for propagation

**Concurrency:**
- Use `Arc<DashMap<K, V>>` for concurrent dicts
- Use `Arc<AtomicU64>` + `Ordering::SeqCst` for counters
- Use `Arc<RwLock<T>>` for occasional writes, frequent reads

**Testing:**
- Property-based tests with `proptest`
- Concurrency tests: spawn threads, verify atomicity
- Memory bounds: track `Arc` ref counts, ensure no leaks
- Naming: `test_<subsystem>_<property>_<scenario>`

**Logging:**
- Use `tracing::info!`, `tracing::warn!`, `tracing::error!`
- Include operation IDs in logs for tracing

---

## Expected Output

**Files to create:**
1. `crates/claudefs-storage/src/dynamic_join.rs` (~200 LOC, 8 tests)
2. `crates/claudefs-storage/src/dynamic_leave.rs` (~200 LOC, 7 tests)
3. `crates/claudefs-storage/src/rebalance_orchestrator.rs` (~250 LOC, 8 tests)

**Total:** ~650 LOC, 23 tests

**Validation:**
- `cargo build -p claudefs-storage` succeeds
- `cargo test -p claudefs-storage --lib` passes 23/23 new tests
- `cargo clippy -p claudefs-storage -- -D warnings` has zero errors
- No regressions in Phase 10 tests (1301+ existing tests still pass)

---

## Notes for Implementation

1. **SWIM Gossip Protocol:** Node discovery handled by `claudefs-meta` metadata service. These modules assume nodes exist and are reachable.

2. **Shard Assignment:** Use consistent hash ring for even distribution. Prefer even distribution (each node gets ⌊shards/nodes⌋ or ⌈shards/nodes⌉ shards).

3. **Atomic Metadata Updates:** Shard replica assignment changes must be durable in metadata service before migration starts.

4. **Bandwidth Limiter:** Global shared limit for all concurrent rebalances. Use token bucket algorithm.

5. **Rollback Safety:** Keep old replica assignment in metadata until migration verified. Only then update metadata.

6. **Progress Tracking:** Store in-memory + sync to metadata service periodically (every 10s) for admin visibility.

7. **Testing:** All tests should be deterministic (no flaky timing). Use controlled time progression or explicit synchronization barriers.

---

## Reference Implementation Patterns

**Lock-free counter with progress:**
```rust
let counter = Arc::new(AtomicUsize::new(0));
let total = 100;
for _ in 0..total {
    counter.fetch_add(1, Ordering::SeqCst);
}
assert_eq!(counter.load(Ordering::SeqCst), total);
```

**Pause/resume state machine:**
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State { Running, Paused, Completed }

let state = Arc::new(Mutex::new(State::Running));
// pause
{ let mut s = state.lock(); *s = State::Paused; }
// resume
{ let mut s = state.lock(); *s = State::Running; }
```

**Bandwidth limiting:**
```rust
let token_bucket = Arc::new(TokenBucket::new(max_mbps));
let available = token_bucket.try_acquire(mbps_needed);
if available {
    // proceed with migration
} else {
    // backpressure, wait or fail
}
```

---

## Deliverable Checklist

- [ ] All 3 modules compile
- [ ] All 23 tests pass
- [ ] Zero clippy warnings
- [ ] No regressions in Phase 10 tests
- [ ] Code follows ClaudeFS conventions
- [ ] Error types properly defined
- [ ] Async/await correctly used
- [ ] Thread safety verified
- [ ] Test coverage >90% per module
- [ ] Documentation complete
- [ ] Ready for `cargo build && cargo test --release`
