# A4 Phase 11: Lease Manager, Shard Map, RPC Timeout Budget

You are implementing 3 new modules for the `claudefs-transport` crate in the ClaudeFS distributed
filesystem (Rust, Cargo workspace). The crate has 75 modules and 1233 passing tests.

## Coding Conventions (MANDATORY)

1. **No external async dependencies** — pure sync Rust only
2. **Serde derive** on all public types: `#[derive(Debug, Clone, Serialize, Deserialize)]`
3. **Atomic counters**: `AtomicU64`, `AtomicU32` with `Ordering::Relaxed`
4. **Stats snapshot pattern**: `XxxStats` (atomic) + `XxxStatsSnapshot` (plain struct with snapshot())
5. **Error types** with `thiserror`
6. **No unwrap/expect** in production code
7. **Tests**: minimum 15 per module in `#[cfg(test)] mod tests` at bottom
8. **Module-level doc comment** `//!` at top

## Standard imports

```rust
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::collections::HashMap;
use thiserror::Error;
```

---

## Module 1: `lease.rs` — Distributed Lease Manager

### Purpose
Manages time-limited leases for distributed locks and coordination. Used by A2 (Metadata)
to grant client-side metadata caching leases (clients can cache inode data until the lease
expires), by A5 (FUSE) for open-file delegation, and by A7 (pNFS) for layout leases.

A lease grants a holder exclusive or shared access to a resource for a time window.
This module tracks lease state without network I/O — callers handle the actual messaging.

### Types to implement

```rust
/// Unique lease identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LeaseId(pub u64);

/// Type of lease access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseType {
    /// Shared read lease — multiple holders allowed.
    Shared,
    /// Exclusive write lease — only one holder allowed.
    Exclusive,
}

/// State of a lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseState {
    /// Active and valid.
    Active,
    /// Recall requested — holder should release soon.
    Recalled,
    /// Expired (past expiry time).
    Expired,
    /// Revoked by administrator or conflict.
    Revoked,
}

/// A granted lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub id: LeaseId,
    pub resource_id: String,
    pub holder_id: [u8; 16],
    pub lease_type: LeaseType,
    pub state: LeaseState,
    pub granted_at_ms: u64,
    pub expires_at_ms: u64,
    pub recalled_at_ms: Option<u64>,
}

impl Lease {
    pub fn is_expired(&self, now_ms: u64) -> bool;
    pub fn remaining_ms(&self, now_ms: u64) -> i64;  // negative if expired
    pub fn is_active(&self, now_ms: u64) -> bool;
}

/// Configuration for lease management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseConfig {
    /// Default lease duration in ms (default: 30000 = 30 seconds).
    pub default_duration_ms: u64,
    /// Maximum lease duration in ms (default: 300000 = 5 minutes).
    pub max_duration_ms: u64,
    /// Grace period after expiry before forced cleanup in ms (default: 5000).
    pub grace_period_ms: u64,
    /// Maximum leases per resource (shared) (default: 64).
    pub max_shared_leases: usize,
}

impl Default for LeaseConfig {
    // default_duration_ms=30000, max_duration_ms=300000, grace_period_ms=5000, max_shared_leases=64
}

/// Error type for lease operations.
#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    #[error("lease {0:?} not found")]
    NotFound(LeaseId),
    #[error("resource {0:?} is exclusively locked")]
    ExclusiveConflict(String),
    #[error("cannot grant exclusive lease while shared leases exist")]
    SharedConflict,
    #[error("too many shared leases for resource {0:?} (max {1})")]
    TooManyShared(String, usize),
    #[error("lease {0:?} has already expired or been revoked")]
    NotActive(LeaseId),
}

/// Manages distributed leases for resources.
pub struct LeaseManager {
    config: LeaseConfig,
    next_id: AtomicU64,
    /// resource_id → Vec of active leases
    leases: RwLock<HashMap<String, Vec<Lease>>>,
    stats: Arc<LeaseStats>,
}

impl LeaseManager {
    pub fn new(config: LeaseConfig) -> Self;

    /// Grant a lease on a resource. Returns LeaseId.
    pub fn grant(
        &self,
        resource_id: String,
        holder_id: [u8; 16],
        lease_type: LeaseType,
        duration_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<LeaseId, LeaseError>;

    /// Renew a lease — extend its expiry time.
    pub fn renew(&self, id: LeaseId, duration_ms: u64, now_ms: u64) -> Result<(), LeaseError>;

    /// Release a lease voluntarily.
    pub fn release(&self, id: LeaseId) -> Result<(), LeaseError>;

    /// Recall a lease (request holder to release).
    pub fn recall(&self, id: LeaseId, now_ms: u64) -> Result<(), LeaseError>;

    /// Revoke a lease (force-remove).
    pub fn revoke(&self, id: LeaseId) -> Result<(), LeaseError>;

    /// Check if a holder still has a valid lease on a resource.
    pub fn check_lease(&self, resource_id: &str, holder_id: &[u8; 16], now_ms: u64) -> bool;

    /// Expire all leases past their expiry time. Returns count removed.
    pub fn expire_leases(&self, now_ms: u64) -> usize;

    /// Get lease info.
    pub fn get(&self, id: LeaseId, now_ms: u64) -> Option<Lease>;

    /// List all active leases for a resource.
    pub fn resource_leases(&self, resource_id: &str, now_ms: u64) -> Vec<Lease>;

    pub fn stats(&self) -> Arc<LeaseStats>;
}

pub struct LeaseStats {
    pub leases_granted: AtomicU64,
    pub leases_released: AtomicU64,
    pub leases_expired: AtomicU64,
    pub leases_revoked: AtomicU64,
    pub leases_recalled: AtomicU64,
    pub lease_conflicts: AtomicU64,
    pub renewals: AtomicU64,
}

pub struct LeaseStatsSnapshot {
    pub leases_granted: u64,
    pub leases_released: u64,
    pub leases_expired: u64,
    pub leases_revoked: u64,
    pub leases_recalled: u64,
    pub lease_conflicts: u64,
    pub renewals: u64,
    pub active_leases: usize,
}

impl LeaseStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, active_leases: usize) -> LeaseStatsSnapshot;
}
```

### Tests (minimum 16)
- `test_grant_shared_lease`
- `test_grant_exclusive_lease`
- `test_grant_exclusive_blocks_shared` — exclusive held → grant shared returns ExclusiveConflict
- `test_grant_shared_blocks_exclusive` — shared held → grant exclusive returns SharedConflict
- `test_multiple_shared_leases` — grant 3 shared leases on same resource → all succeed
- `test_release_lease`
- `test_release_unknown_lease` — returns NotFound
- `test_renew_extends_expiry`
- `test_renew_expired_lease` — returns NotActive
- `test_recall_lease` — state changes to Recalled
- `test_revoke_removes_lease` — revoke → get returns None
- `test_expire_leases` — expired leases removed by expire_leases()
- `test_check_lease_valid` — active lease → true
- `test_check_lease_expired` — expired → false
- `test_resource_leases_list`
- `test_stats_counts`

---

## Module 2: `shard_map.rs` — Virtual Shard → Node Mapping

### Purpose
Implements the virtual shard → node routing table from D4 (Multi-Raft topology).
ClaudeFS uses 256 virtual shards (configurable). Each shard has a Raft group with
a current leader and 2 followers. This module tracks the current shard-to-node mapping
and handles shard rebalancing events when nodes join/leave.

Used by A2 (Metadata) for routing inode operations to the correct shard leader,
and by A4 routing for shard-aware request dispatch.

### Architecture reference
- D4: "Virtual shards: 256 default... hash(inode) % num_shards determines the shard"
- D4: "Each shard: Independent Raft group with 3 replicas on 3 different nodes"

### Types to implement

```rust
/// Virtual shard identifier (0..num_shards).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VirtualShard(pub u32);

/// Role of a node in a shard's Raft group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardRole {
    Leader,
    Follower,
    Learner,
}

/// A node's assignment within a shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardReplica {
    pub node_id: [u8; 16],
    pub role: ShardRole,
    /// When this replica was assigned (ms since epoch).
    pub assigned_at_ms: u64,
}

/// Current state of a virtual shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardInfo {
    pub shard: VirtualShard,
    pub replicas: Vec<ShardReplica>,  // len == replication_factor (usually 3)
}

impl ShardInfo {
    /// Get the current leader node, or None if no leader.
    pub fn leader(&self) -> Option<&ShardReplica>;

    /// Get follower nodes.
    pub fn followers(&self) -> Vec<&ShardReplica>;

    /// Whether this shard has quorum (majority of replicas assigned).
    pub fn has_quorum(&self, replication_factor: usize) -> bool;
}

/// Configuration for the shard map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardMapConfig {
    /// Total number of virtual shards (default: 256, immutable after cluster creation).
    pub num_shards: u32,
    /// Replication factor per shard (default: 3 for Raft).
    pub replication_factor: usize,
}

impl Default for ShardMapConfig {
    // num_shards=256, replication_factor=3
}

/// Error for shard map operations.
#[derive(Debug, thiserror::Error)]
pub enum ShardMapError {
    #[error("shard {0:?} not found")]
    ShardNotFound(VirtualShard),
    #[error("node {0:?} not assigned to shard {1:?}")]
    NodeNotInShard([u8; 16], VirtualShard),
    #[error("shard {0:?} out of range (max {1})")]
    ShardOutOfRange(u32, u32),
}

/// Maps virtual shards to their Raft replicas.
pub struct ShardMap {
    config: ShardMapConfig,
    shards: RwLock<HashMap<VirtualShard, ShardInfo>>,
    stats: Arc<ShardMapStats>,
}

impl ShardMap {
    pub fn new(config: ShardMapConfig) -> Self;

    /// Compute the shard for an inode or key (hash(key) % num_shards).
    pub fn shard_for_key(&self, key: u64) -> VirtualShard;

    /// Get shard info (replicas + leader).
    pub fn get_shard(&self, shard: VirtualShard) -> Result<ShardInfo, ShardMapError>;

    /// Get the leader node for a shard. Returns None if no leader elected.
    pub fn leader_for_shard(&self, shard: VirtualShard) -> Result<Option<[u8; 16]>, ShardMapError>;

    /// Get the leader for the shard responsible for a key.
    pub fn leader_for_key(&self, key: u64) -> Option<[u8; 16]>;

    /// Update the leader for a shard (Raft leader election result).
    pub fn update_leader(&self, shard: VirtualShard, new_leader: [u8; 16], now_ms: u64) -> Result<(), ShardMapError>;

    /// Assign a replica set to a shard (initial cluster setup or rebalancing).
    pub fn assign_replicas(&self, shard: VirtualShard, replicas: Vec<ShardReplica>) -> Result<(), ShardMapError>;

    /// Remove a node from all shards (node leaving). Returns list of affected shards.
    pub fn remove_node(&self, node_id: &[u8; 16]) -> Vec<VirtualShard>;

    /// Get all shards where a specific node is a replica.
    pub fn shards_for_node(&self, node_id: &[u8; 16]) -> Vec<VirtualShard>;

    /// Number of shards with an elected leader.
    pub fn shards_with_leader(&self) -> usize;

    /// Number of shards without quorum.
    pub fn shards_without_quorum(&self) -> usize;

    pub fn stats(&self) -> Arc<ShardMapStats>;
}

pub struct ShardMapStats {
    pub leader_updates: AtomicU64,
    pub replica_assignments: AtomicU64,
    pub node_removals: AtomicU64,
    pub key_lookups: AtomicU64,
}

pub struct ShardMapStatsSnapshot {
    pub leader_updates: u64,
    pub replica_assignments: u64,
    pub node_removals: u64,
    pub key_lookups: u64,
    pub total_shards: u32,
    pub shards_with_leader: usize,
    pub shards_without_quorum: usize,
}

impl ShardMapStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, total_shards: u32, with_leader: usize, without_quorum: usize) -> ShardMapStatsSnapshot;
}
```

### Tests (minimum 16)
- `test_shard_for_key_range` — result is in 0..num_shards
- `test_shard_for_key_deterministic` — same key → same shard
- `test_assign_replicas`
- `test_get_shard` — after assign, get_shard returns correct info
- `test_get_shard_not_found` — ShardNotFound for unassigned shard
- `test_shard_out_of_range` — shard >= num_shards → ShardOutOfRange
- `test_leader_for_shard` — after assign with leader, returns leader node_id
- `test_leader_for_shard_no_leader` — all followers → returns None
- `test_update_leader` — update_leader changes the leader
- `test_remove_node_from_shards` — node removed from all assigned shards
- `test_shards_for_node` — lists all shards where node is a replica
- `test_shards_with_leader_count`
- `test_shards_without_quorum_count`
- `test_leader_for_key` — key → shard → leader
- `test_has_quorum_true` — 3 of 3 replicas assigned
- `test_stats_counts`

---

## Module 3: `timeout_budget.rs` — RPC Timeout Budget

### Purpose
Manages cascading timeout budgets for nested RPC calls. When a client sends a request
with a 100ms deadline, internal sub-requests (storage reads, metadata lookups) must share
that budget and not exceed it. This module tracks the remaining time budget and propagates
it through nested RPC call chains.

Used by A4 transport to attach deadline propagation to all outgoing RPCs, ensuring
that sub-requests don't outlive the original client deadline (per the Deadline module
which already handles encoding/decoding of deadlines on the wire).

### Types to implement

```rust
/// A time budget for a chain of nested RPC calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutBudget {
    /// Total budget allocated at the start (ms).
    pub total_ms: u64,
    /// When the budget was created (ms since epoch).
    pub created_at_ms: u64,
    /// Overhead to subtract for each hop (serialization, network, processing, default: 5ms).
    pub per_hop_overhead_ms: u64,
    /// Number of hops this budget has been through.
    pub hops: u32,
}

impl TimeoutBudget {
    /// Create a new budget.
    pub fn new(total_ms: u64, now_ms: u64) -> Self;
    pub fn with_overhead(total_ms: u64, per_hop_overhead_ms: u64, now_ms: u64) -> Self;

    /// Remaining budget in ms. Returns 0 if expired.
    pub fn remaining_ms(&self, now_ms: u64) -> u64;

    /// Whether the budget has been exhausted (remaining == 0).
    pub fn is_exhausted(&self, now_ms: u64) -> bool;

    /// Create a child budget for a sub-request:
    /// - subtracts per_hop_overhead_ms once
    /// - remaining budget = min(remaining_ms, max_sub_ms)
    /// - increments hops counter
    /// Returns None if budget is already exhausted.
    pub fn child(&self, max_sub_ms: Option<u64>, now_ms: u64) -> Option<Self>;

    /// Fraction of budget remaining (0.0 = exhausted, 1.0 = full).
    pub fn fraction_remaining(&self, now_ms: u64) -> f64;
}

/// Configuration for timeout budget tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutBudgetConfig {
    /// Default timeout budget for new client requests in ms (default: 1000).
    pub default_budget_ms: u64,
    /// Per-hop overhead in ms (default: 5).
    pub per_hop_overhead_ms: u64,
    /// Maximum number of hops before forcibly exhausting budget (default: 16).
    pub max_hops: u32,
    /// Warn when remaining budget is below this fraction (default: 0.1 = 10%).
    pub warn_threshold: f64,
}

impl Default for TimeoutBudgetConfig {
    // default_budget_ms=1000, per_hop_overhead_ms=5, max_hops=16, warn_threshold=0.1
}

/// Manages RPC timeout budgets.
pub struct TimeoutBudgetManager {
    config: TimeoutBudgetConfig,
    /// Active budgets, keyed by an opaque request ID.
    active: Mutex<HashMap<u64, TimeoutBudget>>,
    stats: Arc<TimeoutBudgetStats>,
}

impl TimeoutBudgetManager {
    pub fn new(config: TimeoutBudgetConfig) -> Self;

    /// Allocate a new budget for a top-level request.
    pub fn allocate(&self, request_id: u64, budget_ms: Option<u64>, now_ms: u64) -> TimeoutBudget;

    /// Create a child budget from a tracked parent budget (for sub-request).
    pub fn child_budget(&self, parent_id: u64, max_sub_ms: Option<u64>, now_ms: u64) -> Option<TimeoutBudget>;

    /// Remove a budget (request completed).
    pub fn release(&self, request_id: u64);

    /// Expire all exhausted budgets. Returns count removed.
    pub fn expire(&self, now_ms: u64) -> usize;

    /// Number of active budgets.
    pub fn active_count(&self) -> usize;

    /// Number of budgets in the warning zone (< warn_threshold remaining).
    pub fn warning_count(&self, now_ms: u64) -> usize;

    pub fn stats(&self) -> Arc<TimeoutBudgetStats>;
}

pub struct TimeoutBudgetStats {
    pub budgets_allocated: AtomicU64,
    pub budgets_released: AtomicU64,
    pub budgets_expired: AtomicU64,
    pub child_budgets_created: AtomicU64,
    pub budgets_exhausted: AtomicU64,  // child returned None
    pub hops_total: AtomicU64,
}

pub struct TimeoutBudgetStatsSnapshot {
    pub budgets_allocated: u64,
    pub budgets_released: u64,
    pub budgets_expired: u64,
    pub child_budgets_created: u64,
    pub budgets_exhausted: u64,
    pub hops_total: u64,
    pub active_count: usize,
}

impl TimeoutBudgetStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, active: usize) -> TimeoutBudgetStatsSnapshot;
}
```

### Tests (minimum 15)
- `test_new_budget_full`
- `test_remaining_decreases_with_time` — remaining_ms(t+500) < remaining_ms(t)
- `test_exhausted_when_past_deadline`
- `test_not_exhausted_before_deadline`
- `test_child_budget_subtracts_overhead` — child.total_ms < parent.remaining_ms
- `test_child_budget_caps_at_max_sub_ms`
- `test_child_returns_none_when_exhausted`
- `test_child_increments_hops`
- `test_fraction_remaining_full` — fresh budget → ~1.0
- `test_fraction_remaining_half` — half elapsed → ~0.5
- `test_fraction_remaining_zero` — exhausted → 0.0
- `test_manager_allocate`
- `test_manager_child_budget`
- `test_manager_expire`
- `test_stats_counts`

---

## Output Format

```
=== FILE: crates/claudefs-transport/src/lease.rs ===
<complete file>

=== FILE: crates/claudefs-transport/src/shard_map.rs ===
<complete file>

=== FILE: crates/claudefs-transport/src/timeout_budget.rs ===
<complete file>
```

Then lib.rs additions:
```
=== LIB.RS ADDITIONS ===
pub mod lease;
pub mod shard_map;
pub mod timeout_budget;

pub use lease::{
    Lease, LeaseConfig, LeaseError, LeaseId, LeaseManager, LeaseState, LeaseStats,
    LeaseStatsSnapshot, LeaseType,
};
pub use shard_map::{
    ShardInfo, ShardMap, ShardMapConfig, ShardMapError, ShardMapStats, ShardMapStatsSnapshot,
    ShardReplica, ShardRole, VirtualShard,
};
pub use timeout_budget::{
    TimeoutBudget, TimeoutBudgetConfig, TimeoutBudgetManager, TimeoutBudgetStats,
    TimeoutBudgetStatsSnapshot,
};
```

## Important
- Complete compilable Rust
- LeaseManager: exclusive lease blocks all others; shared leases block exclusive; multiple shared OK
- ShardMap: shard_for_key = key % num_shards as u32
- TimeoutBudget.child(): new total_ms = remaining_ms - per_hop_overhead_ms (capped at max_sub_ms if given)
