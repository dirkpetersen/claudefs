# A4 Phase 7: Wire Diagnostics, Credit Window, Multicast Group

You are implementing new modules for the `claudefs-transport` crate in the ClaudeFS distributed
filesystem (Rust, Cargo workspace). The crate already has 63 modules and 1013 passing tests.

## Context

ClaudeFS transport layer provides the network substrate for a distributed filesystem:
- Custom binary RPC over TCP/RDMA
- mTLS with cluster-issued certificates (D7)
- Flow control, QoS, backpressure, circuit breaking, rate limiting all exist
- Gossip/SWIM membership (D2), consistent hash routing (D4), EC stripe distribution (D1)

## Coding Conventions (MANDATORY — follow exactly)

1. **No external async dependencies** — modules must NOT use `tokio`, `async-trait`, `futures`, or
   any async constructs. These are pure sync data-structure/logic modules.
2. **Serde derive** on all public types: `#[derive(Debug, Clone, Serialize, Deserialize)]`
3. **Atomic counters** for stats: `AtomicU64`, `AtomicU32`, `AtomicU8` with `Ordering::Relaxed`
   for reads, `Ordering::Release/Acquire` only when needed for ordering.
4. **Stats snapshot pattern**: every module has `XxxStats` (atomic fields) + `XxxStatsSnapshot`
   (plain struct with snapshot() method). E.g.:
   ```rust
   pub struct WireDiagStats {
       pub pings_sent: AtomicU64,
       pub pings_received: AtomicU64,
   }
   pub struct WireDiagStatsSnapshot {
       pub pings_sent: u64,
       pub pings_received: u64,
   }
   impl WireDiagStats {
       pub fn snapshot(&self) -> WireDiagStatsSnapshot { ... }
   }
   ```
5. **Error types** with `thiserror`: `#[derive(Debug, thiserror::Error)]`
6. **No unwrap/expect** in production code — use `?` or return `Result`/`Option`
7. **Tests**: minimum 15 tests per module, all in `#[cfg(test)] mod tests` at bottom of file
8. **`Arc<Self>`** for shared state that needs to be cloned across threads
9. **`std::sync::Mutex`** or `RwLock` for interior mutability, NOT `tokio::sync`
10. **Module-level doc comment** `//!` at top of each file
11. Do NOT add `pub use` re-exports at the top of the file — those go in lib.rs (shown separately)

## Existing imports/types you can reference (already in scope from other modules)

```rust
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
```

---

## Module 1: `wire_diag.rs` — Wire Diagnostics

### Purpose
Provides wire-level diagnostics for ClaudeFS transport connections: ping/pong RTT measurement,
rolling latency statistics, and path tracing. Used by A9 (Test & Validation) for health checks
and by A8 (Management) for cluster observability dashboards.

### Types to implement

```rust
/// Configuration for wire diagnostics.
pub struct WireDiagConfig {
    /// Window size for rolling RTT statistics (default: 128).
    pub window_size: usize,
    /// Timeout after which a ping is considered lost (milliseconds, default: 5000).
    pub ping_timeout_ms: u64,
    /// Maximum number of concurrent in-flight pings (default: 8).
    pub max_inflight: usize,
}
impl Default for WireDiagConfig { ... }

/// A single RTT sample.
pub struct RttSample {
    /// Sequence number of the ping.
    pub seq: u64,
    /// Round-trip time in microseconds.
    pub rtt_us: u64,
    /// Timestamp of the ping send (ms since epoch).
    pub sent_at_ms: u64,
}

/// Rolling RTT statistics over the last `window_size` samples.
pub struct RttSeries {
    samples: VecDeque<RttSample>,
    window_size: usize,
}

impl RttSeries {
    pub fn new(window_size: usize) -> Self;
    /// Push a new sample (drops oldest if window is full).
    pub fn push(&mut self, sample: RttSample);
    /// Minimum RTT in the window (None if empty).
    pub fn min_us(&self) -> Option<u64>;
    /// Maximum RTT in the window (None if empty).
    pub fn max_us(&self) -> Option<u64>;
    /// Mean RTT in the window (None if empty).
    pub fn mean_us(&self) -> Option<u64>;
    /// p99 RTT in the window (None if < 100 samples; sort-based ok).
    pub fn p99_us(&self) -> Option<u64>;
    /// Number of samples in the window.
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

/// A hop entry in a trace path (one intermediate node in a multi-hop RPC path).
pub struct TraceHop {
    /// Node identifier (opaque bytes, typically 16-byte UUID).
    pub node_id: [u8; 16],
    /// Cumulative RTT to this hop in microseconds.
    pub cumulative_rtt_us: u64,
    /// Incremental latency added by this hop in microseconds.
    pub hop_latency_us: u64,
}

/// Result of a trace-path operation (like traceroute but for ClaudeFS RPC).
pub struct TracePath {
    pub hops: Vec<TraceHop>,
    /// Total RTT for the full path in microseconds.
    pub total_rtt_us: u64,
    /// Whether the path completed successfully (destination responded).
    pub complete: bool,
}

/// An in-flight ping record.
pub struct InFlightPing {
    pub seq: u64,
    pub sent_at_ms: u64,
    pub timeout_ms: u64,
}

/// Wire diagnostics manager.
pub struct WireDiag {
    config: WireDiagConfig,
    next_seq: AtomicU64,
    inflight: Mutex<HashMap<u64, InFlightPing>>,
    rtt_series: Mutex<RttSeries>,
    stats: Arc<WireDiagStats>,
}

impl WireDiag {
    pub fn new(config: WireDiagConfig) -> Self;

    /// Create a new ping request — returns the ping sequence number to include in the wire message.
    /// Returns None if max_inflight is already reached.
    pub fn send_ping(&self, now_ms: u64) -> Option<u64>;

    /// Record a received pong response. Updates RTT series and stats.
    /// Returns the RTT in microseconds, or None if the seq was not in-flight.
    pub fn receive_pong(&self, seq: u64, now_ms: u64) -> Option<u64>;

    /// Expire timed-out in-flight pings. Returns count of timed-out pings.
    pub fn expire_timeouts(&self, now_ms: u64) -> u64;

    /// Current count of in-flight pings.
    pub fn inflight_count(&self) -> usize;

    /// Take a snapshot of the current RTT series statistics.
    pub fn rtt_snapshot(&self) -> RttSeriesSnapshot;

    /// Get the stats arc.
    pub fn stats(&self) -> Arc<WireDiagStats>;
}

/// Immutable snapshot of RTT series statistics.
pub struct RttSeriesSnapshot {
    pub sample_count: usize,
    pub min_us: Option<u64>,
    pub max_us: Option<u64>,
    pub mean_us: Option<u64>,
    pub p99_us: Option<u64>,
}

/// Atomic stats for wire diagnostics.
pub struct WireDiagStats {
    pub pings_sent: AtomicU64,
    pub pongs_received: AtomicU64,
    pub pings_timed_out: AtomicU64,
    pub pings_rejected: AtomicU64,  // rejected due to max_inflight
}

pub struct WireDiagStatsSnapshot {
    pub pings_sent: u64,
    pub pongs_received: u64,
    pub pings_timed_out: u64,
    pub pings_rejected: u64,
}

impl WireDiagStats {
    pub fn snapshot(&self) -> WireDiagStatsSnapshot;
}
```

### Tests (minimum 15)
- `test_new_default_config` — create with Default config, assert window_size=128
- `test_send_ping_basic` — send_ping returns Some(0), inflight_count becomes 1
- `test_send_ping_max_inflight` — after max_inflight pings, returns None
- `test_receive_pong_records_rtt` — send then receive, rtt > 0
- `test_receive_pong_unknown_seq` — returns None for unknown seq
- `test_expire_timeouts_removes_stale` — send ping at t=0, expire at t>timeout, count=1
- `test_expire_timeouts_keeps_fresh` — send ping at t=0, expire at t<timeout, count=0
- `test_rtt_series_push_and_stats` — push 10 samples, check min/max/mean
- `test_rtt_series_window_eviction` — push window_size+1 samples, len() == window_size
- `test_rtt_series_p99_needs_100` — push 99 samples, p99 returns None; push 1 more, returns Some
- `test_rtt_series_empty` — empty series returns None for all stats
- `test_stats_snapshot` — send 3 pings, receive 2, expire 1 → verify snapshot counts
- `test_trace_path_complete` — construct TracePath with 3 hops, complete=true
- `test_trace_path_incomplete` — construct TracePath with 2 hops, complete=false
- `test_rtt_sample_ordering` — push samples in non-monotonic order, min/max correct

---

## Module 2: `credit_window.rs` — Credit-Window Flow Control

### Purpose
Implements credit-window flow control for per-connection in-flight byte budgets. The sender
acquires credits (bytes) before sending; the receiver returns credits as it processes messages.
This is distinct from `flowcontrol.rs` (which uses token buckets) — this is an explicit
credit-grant/consume protocol used by A6 (Replication) to prevent journal backlog buildup
and by A5 (FUSE client) to manage prefetch budgets.

### Types to implement

```rust
/// State of a credit window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreditWindowState {
    /// Normal operation — credits available.
    Normal,
    /// Warning — credits running low (below 25%).
    Warning,
    /// Throttled — credits below 10%, slow down.
    Throttled,
    /// Exhausted — no credits available.
    Exhausted,
}

/// Configuration for a credit window.
pub struct CreditWindowConfig {
    /// Total credit budget in bytes (default: 64MB = 67_108_864).
    pub total_credits: u64,
    /// Warning threshold as fraction 0.0..1.0 (default: 0.25).
    pub warning_threshold: f64,
    /// Throttle threshold as fraction 0.0..1.0 (default: 0.10).
    pub throttle_threshold: f64,
    /// Maximum single allocation in bytes (default: 8MB = 8_388_608).
    pub max_single_alloc: u64,
}
impl Default for CreditWindowConfig { ... }

/// A granted credit allocation — release on drop (but also has explicit `release()`).
pub struct CreditGrant {
    credits: u64,
    window: Arc<CreditWindowInner>,
}
impl CreditGrant {
    /// Credits held by this grant.
    pub fn credits(&self) -> u64;
    /// Explicitly release credits back to the window.
    pub fn release(self);
}
impl Drop for CreditGrant { fn drop(&mut self) { ... } }

// Internal shared state
struct CreditWindowInner {
    available: AtomicU64,
    total: u64,
    config: CreditWindowConfig,
    stats: Arc<CreditWindowStats>,
}

/// Credit window manager.
pub struct CreditWindow {
    inner: Arc<CreditWindowInner>,
}

impl CreditWindow {
    pub fn new(config: CreditWindowConfig) -> Self;

    /// Attempt to acquire `bytes` credits.
    /// Returns Some(CreditGrant) if credits available, None if exhausted.
    pub fn try_acquire(&self, bytes: u64) -> Option<CreditGrant>;

    /// Force-return `bytes` credits (used when receiver sends credit grants back to sender).
    pub fn return_credits(&self, bytes: u64);

    /// Current available credits.
    pub fn available(&self) -> u64;

    /// Total configured credits.
    pub fn total(&self) -> u64;

    /// Current state based on available/total ratio.
    pub fn state(&self) -> CreditWindowState;

    /// Utilization as a value 0.0..=1.0 (consumed / total).
    pub fn utilization(&self) -> f64;

    /// Stats reference.
    pub fn stats(&self) -> Arc<CreditWindowStats>;
}

pub struct CreditWindowStats {
    pub grants_issued: AtomicU64,
    pub grants_denied: AtomicU64,
    pub credits_granted: AtomicU64,
    pub credits_returned: AtomicU64,
    pub throttle_events: AtomicU64,
    pub exhaustion_events: AtomicU64,
}

pub struct CreditWindowStatsSnapshot {
    pub grants_issued: u64,
    pub grants_denied: u64,
    pub credits_granted: u64,
    pub credits_returned: u64,
    pub throttle_events: u64,
    pub exhaustion_events: u64,
    pub available_credits: u64,
    pub total_credits: u64,
    pub state: CreditWindowState,
}

impl CreditWindowStats {
    pub fn snapshot(&self, available: u64, total: u64, state: CreditWindowState) -> CreditWindowStatsSnapshot;
}
```

### Tests (minimum 18)
- `test_new_default_config` — create, available == total == 64MB
- `test_try_acquire_success` — acquire 1MB, available decreases
- `test_try_acquire_returns_on_drop` — acquire 1MB, drop grant, available restored
- `test_try_acquire_explicit_release` — acquire, call release(), available restored
- `test_try_acquire_exact_total` — acquire all credits at once → Some
- `test_try_acquire_over_total` — acquire total+1 → None
- `test_try_acquire_max_single_alloc` — acquire max_single_alloc+1 → None
- `test_try_acquire_exhausts_window` — fill window to 0, next acquire returns None
- `test_state_normal` — 50% available → Normal
- `test_state_warning` — 24% available → Warning
- `test_state_throttled` — 9% available → Throttled
- `test_state_exhausted` — 0 available → Exhausted
- `test_return_credits` — consume all, return_credits restores
- `test_return_credits_no_overflow` — return_credits can't exceed total
- `test_utilization` — consume half, utilization == 0.5
- `test_stats_counts` — issue 3 grants, deny 1 → counts correct
- `test_multiple_acquisitions_sum` — 3 × 1MB + 61MB = 64MB → no credits left
- `test_state_transitions` — progressively consume window and verify state changes

---

## Module 3: `multicast_group.rs` — Multicast Group Management

### Purpose
Manages named multicast groups for broadcasting control-plane messages (config updates,
membership events, shard rebalancing notifications) to sets of cluster nodes. Used by A2
(Metadata Service) for cluster-wide config propagation and by A6 (Replication) for site
membership announcements. This is a pure protocol-layer abstraction — does NOT do actual
network I/O; callers handle sending to returned member lists.

### Types to implement

```rust
/// Unique group identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub String);

impl GroupId {
    pub fn new(name: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
}

/// A node member of a multicast group.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupMember {
    /// Opaque node identifier (16 bytes).
    pub node_id: [u8; 16],
    /// Human-readable label (hostname or address).
    pub label: String,
    /// Timestamp when this member joined (ms since epoch).
    pub joined_at_ms: u64,
}

/// Membership event for group change notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupEvent {
    /// A node joined the group.
    Join { group: GroupId, member: GroupMember },
    /// A node left the group.
    Leave { group: GroupId, node_id: [u8; 16] },
    /// Group was dissolved (all members removed).
    Dissolved { group: GroupId },
}

/// Result of a broadcast operation.
pub struct BroadcastResult {
    /// Number of members in the group at the time of broadcast.
    pub group_size: usize,
    /// Member node_ids that were targeted.
    pub targeted: Vec<[u8; 16]>,
}

/// Configuration for multicast groups.
pub struct MulticastGroupConfig {
    /// Maximum number of groups (default: 256).
    pub max_groups: usize,
    /// Maximum members per group (default: 64).
    pub max_members_per_group: usize,
}
impl Default for MulticastGroupConfig { ... }

/// Error type for multicast group operations.
#[derive(Debug, thiserror::Error)]
pub enum MulticastError {
    #[error("group {0:?} not found")]
    GroupNotFound(GroupId),
    #[error("group {0:?} already exists")]
    GroupAlreadyExists(GroupId),
    #[error("member already in group {0:?}")]
    AlreadyMember(GroupId),
    #[error("member not in group {0:?}")]
    NotMember(GroupId),
    #[error("group limit reached ({0})")]
    GroupLimitReached(usize),
    #[error("member limit reached for group {0:?} ({1})")]
    MemberLimitReached(GroupId, usize),
}

/// Manager for named multicast groups.
pub struct MulticastGroupManager {
    config: MulticastGroupConfig,
    // groups: group_id → Vec<GroupMember>
    groups: RwLock<HashMap<GroupId, Vec<GroupMember>>>,
    stats: Arc<MulticastGroupStats>,
}

impl MulticastGroupManager {
    pub fn new(config: MulticastGroupConfig) -> Self;

    /// Create a new empty group. Error if already exists or group limit reached.
    pub fn create_group(&self, group: GroupId) -> Result<(), MulticastError>;

    /// Add a member to a group. Error if group not found, member already present, or member limit.
    pub fn join(&self, group: &GroupId, member: GroupMember) -> Result<GroupEvent, MulticastError>;

    /// Remove a member from a group. Returns Leave event.
    pub fn leave(&self, group: &GroupId, node_id: &[u8; 16]) -> Result<GroupEvent, MulticastError>;

    /// Dissolve a group — removes all members. Returns Dissolved event.
    pub fn dissolve(&self, group: &GroupId) -> Result<GroupEvent, MulticastError>;

    /// Get all members of a group.
    pub fn members(&self, group: &GroupId) -> Result<Vec<GroupMember>, MulticastError>;

    /// Check if a node is a member of a group.
    pub fn is_member(&self, group: &GroupId, node_id: &[u8; 16]) -> bool;

    /// Prepare a broadcast to a group — returns BroadcastResult with targeted member ids.
    /// Caller is responsible for actually sending the message to each targeted node.
    pub fn prepare_broadcast(&self, group: &GroupId) -> Result<BroadcastResult, MulticastError>;

    /// List all group IDs.
    pub fn list_groups(&self) -> Vec<GroupId>;

    /// Number of groups currently registered.
    pub fn group_count(&self) -> usize;

    /// Stats reference.
    pub fn stats(&self) -> Arc<MulticastGroupStats>;
}

pub struct MulticastGroupStats {
    pub groups_created: AtomicU64,
    pub groups_dissolved: AtomicU64,
    pub joins: AtomicU64,
    pub leaves: AtomicU64,
    pub broadcasts_prepared: AtomicU64,
    pub total_broadcast_targets: AtomicU64,  // sum of group_size at each broadcast
}

pub struct MulticastGroupStatsSnapshot {
    pub groups_created: u64,
    pub groups_dissolved: u64,
    pub joins: u64,
    pub leaves: u64,
    pub broadcasts_prepared: u64,
    pub total_broadcast_targets: u64,
    pub active_groups: usize,
}

impl MulticastGroupStats {
    pub fn snapshot(&self, active_groups: usize) -> MulticastGroupStatsSnapshot;
}
```

### Tests (minimum 17)
- `test_create_group` — create group, list_groups returns it
- `test_create_group_duplicate` — returns GroupAlreadyExists error
- `test_create_group_limit` — fill to max_groups, next returns GroupLimitReached
- `test_join_success` — join returns Join event with correct member
- `test_join_unknown_group` — returns GroupNotFound
- `test_join_duplicate_member` — returns AlreadyMember
- `test_join_member_limit` — fill group, returns MemberLimitReached
- `test_leave_success` — join then leave, members() empty
- `test_leave_not_member` — returns NotMember
- `test_dissolve_removes_all` — join 3 members, dissolve, members() error GroupNotFound
- `test_dissolve_unknown_group` — returns GroupNotFound
- `test_is_member_true` — join member, is_member returns true
- `test_is_member_false` — is_member for non-member returns false
- `test_prepare_broadcast_returns_all_members` — 3 members, targeted.len() == 3
- `test_prepare_broadcast_empty_group` — targeted.len() == 0
- `test_stats_counts` — create/join/leave/dissolve/broadcast, verify counts
- `test_multiple_groups_independent` — two groups, members don't cross

---

## Output Format

For each module, output:

```
=== FILE: crates/claudefs-transport/src/wire_diag.rs ===
<complete file content>

=== FILE: crates/claudefs-transport/src/credit_window.rs ===
<complete file content>

=== FILE: crates/claudefs-transport/src/multicast_group.rs ===
<complete file content>
```

Then output the `lib.rs` additions needed:

```
=== LIB.RS ADDITIONS ===
pub mod wire_diag;
pub mod credit_window;
pub mod multicast_group;

pub use wire_diag::{
    InFlightPing, RttSample, RttSeries, RttSeriesSnapshot,
    TraceHop, TracePath, WireDiag, WireDiagConfig, WireDiagStats, WireDiagStatsSnapshot,
};
pub use credit_window::{
    CreditGrant, CreditWindow, CreditWindowConfig, CreditWindowState,
    CreditWindowStats, CreditWindowStatsSnapshot,
};
pub use multicast_group::{
    BroadcastResult, GroupEvent, GroupId, GroupMember, MulticastError,
    MulticastGroupConfig, MulticastGroupManager, MulticastGroupStats, MulticastGroupStatsSnapshot,
};
```

## Important
- Produce complete, compilable Rust files — no TODOs, no unimplemented!() in test paths
- Every test must have a descriptive name and assert something meaningful
- Stats snapshot() methods must load atomics with Ordering::Relaxed
- p99 computation in RttSeries: sort the window values, take index at 99th percentile
- CreditGrant Drop impl: must not panic even if window is in bad state
- MulticastGroupManager: use Arc<Self> pattern is NOT needed — it uses RwLock internally
