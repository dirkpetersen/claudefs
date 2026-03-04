# A4 Phase 8: Fanout, Quorum, Segment Router

You are implementing 3 new modules for the `claudefs-transport` crate in the ClaudeFS distributed
filesystem (Rust, Cargo workspace). The crate has 66 modules and 1070 passing tests.

## Context

ClaudeFS write path (from decisions.md D1, D3, D8):
1. Client writes to the metadata-owning node (consistent hash)
2. That node fans out the write to **2 journal replicas** synchronously (before ack)
3. Background: segment packer collects journal entries → EC 4+2 stripe distribution to 6 nodes
4. Background: async push to S3 (cache mode)

Read path:
- Small reads hit primary cache
- Large sequential reads fetch EC stripes from 6 nodes in **parallel**

These modules are the transport-layer primitives for those operations.

## Coding Conventions (MANDATORY — follow exactly)

1. **No external async dependencies** — pure sync Rust. No `tokio`, no async/await.
2. **Serde derive** on all public types: `#[derive(Debug, Clone, Serialize, Deserialize)]`
3. **Atomic counters** for stats: `AtomicU64`, `AtomicU32` with `Ordering::Relaxed`
4. **Stats snapshot pattern**: `XxxStats` (atomic) + `XxxStatsSnapshot` (plain struct with snapshot())
5. **Error types** with `thiserror`: `#[derive(Debug, thiserror::Error)]`
6. **No unwrap/expect** in production code
7. **Tests**: minimum 15 tests per module in `#[cfg(test)] mod tests` at bottom
8. **Module-level doc comment** `//!` at top of each file
9. Do NOT add `pub use` re-exports at top — those go in lib.rs (shown separately)

## Standard imports available

```rust
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use thiserror::Error;
```

---

## Module 1: `fanout.rs` — Parallel Request Fanout

### Purpose
Tracks parallel in-flight requests to multiple cluster nodes (fanout). Used by the write path
to dispatch to 2 journal replicas simultaneously, and by the EC stripe distribution to dispatch
to 6 data nodes. Also used for parallel read fan-out across EC stripe nodes.

This is a pure state-tracking module — callers do the actual network I/O and report results back.

### Architecture decision reference
- D3: "2x synchronous replication to two different nodes before ack to client"
- D1: "4+2 EC stripes distributed across 6 different nodes"
- D8: "Background segment packer... distributes stripes across 6 different nodes via consistent hash"

### Types to implement

```rust
/// Unique identifier for a fanout operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FanoutId(pub u64);

/// A single target in a fanout operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutTarget {
    /// Target node identifier (opaque 16-byte UUID).
    pub node_id: [u8; 16],
    /// Human-readable label for debugging.
    pub label: String,
}

/// Result from a single fanout target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FanoutTargetResult {
    /// Target responded successfully.
    Success,
    /// Target failed with an error message.
    Failed(String),
    /// Target timed out.
    TimedOut,
}

/// Configuration for fanout operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutConfig {
    /// Minimum number of successes required for the fanout to succeed (quorum).
    /// For journal replication: 2. For EC reads: 4 (4+2, need 4 data shards).
    pub required_successes: usize,
    /// Total number of targets.
    pub total_targets: usize,
    /// Timeout for the entire fanout operation in milliseconds.
    pub timeout_ms: u64,
}

impl Default for FanoutConfig {
    // required_successes=2, total_targets=2, timeout_ms=5000
}

/// State of a fanout operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanoutState {
    /// Waiting for responses.
    InFlight,
    /// Enough successes received — quorum met.
    Succeeded,
    /// Too many failures — cannot meet quorum.
    Failed,
    /// Timed out before quorum.
    TimedOut,
}

/// A tracked fanout operation.
pub struct FanoutOp {
    pub id: FanoutId,
    pub config: FanoutConfig,
    pub targets: Vec<FanoutTarget>,
    results: HashMap<[u8; 16], FanoutTargetResult>,
    created_at_ms: u64,
    state: FanoutState,
}

impl FanoutOp {
    pub fn new(id: FanoutId, config: FanoutConfig, targets: Vec<FanoutTarget>, now_ms: u64) -> Self;

    /// Record a result from a target node. Returns the updated FanoutState.
    pub fn record_result(&mut self, node_id: [u8; 16], result: FanoutTargetResult) -> FanoutState;

    /// Check if the operation has timed out. Updates state if so. Returns new state.
    pub fn check_timeout(&mut self, now_ms: u64) -> FanoutState;

    /// Current state.
    pub fn state(&self) -> FanoutState;

    /// Number of successes so far.
    pub fn success_count(&self) -> usize;

    /// Number of failures so far.
    pub fn failure_count(&self) -> usize;

    /// Number of pending (no response yet) targets.
    pub fn pending_count(&self) -> usize;

    /// Whether quorum has been met (success_count >= required_successes).
    pub fn quorum_met(&self) -> bool;

    /// Whether quorum is still achievable (pending + success >= required_successes).
    pub fn quorum_possible(&self) -> bool;
}

/// Manager for multiple concurrent fanout operations.
pub struct FanoutManager {
    next_id: AtomicU64,
    ops: Mutex<HashMap<FanoutId, FanoutOp>>,
    stats: Arc<FanoutStats>,
}

impl FanoutManager {
    pub fn new() -> Self;

    /// Start a new fanout operation. Returns the FanoutId.
    pub fn start(&self, config: FanoutConfig, targets: Vec<FanoutTarget>, now_ms: u64) -> FanoutId;

    /// Record a result for a specific fanout. Returns the new state, or None if fanout not found.
    pub fn record_result(&self, id: FanoutId, node_id: [u8; 16], result: FanoutTargetResult, now_ms: u64) -> Option<FanoutState>;

    /// Check timeouts for all in-flight ops. Returns ids that transitioned to TimedOut.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<FanoutId>;

    /// Complete (remove) a fanout operation. Returns the final FanoutState, or None if not found.
    pub fn complete(&self, id: FanoutId) -> Option<FanoutState>;

    /// Get state of a specific fanout.
    pub fn state(&self, id: FanoutId) -> Option<FanoutState>;

    /// Number of in-flight fanout operations.
    pub fn in_flight_count(&self) -> usize;

    /// Stats reference.
    pub fn stats(&self) -> Arc<FanoutStats>;
}

pub struct FanoutStats {
    pub ops_started: AtomicU64,
    pub ops_succeeded: AtomicU64,
    pub ops_failed: AtomicU64,
    pub ops_timed_out: AtomicU64,
    pub total_targets_sent: AtomicU64,
    pub total_target_successes: AtomicU64,
    pub total_target_failures: AtomicU64,
}

pub struct FanoutStatsSnapshot {
    pub ops_started: u64,
    pub ops_succeeded: u64,
    pub ops_failed: u64,
    pub ops_timed_out: u64,
    pub total_targets_sent: u64,
    pub total_target_successes: u64,
    pub total_target_failures: u64,
    pub in_flight: usize,
}

impl FanoutStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, in_flight: usize) -> FanoutStatsSnapshot;
}
```

### Tests (minimum 15)
- `test_fanout_basic_success` — 2 targets, 2 successes → state Succeeded
- `test_fanout_quorum_met_early` — 3 targets, require 2, first 2 succeed → Succeeded
- `test_fanout_failure_blocks_quorum` — 2 targets, 1 failure 1 pending → state still InFlight
- `test_fanout_quorum_impossible` — 2 of 2 required, first target fails → quorum_possible() false → Failed
- `test_fanout_timeout` — check_timeout after timeout_ms → TimedOut
- `test_fanout_timeout_not_expired` — check_timeout before timeout_ms → InFlight
- `test_fanout_success_count` — track success_count/failure_count/pending_count correctly
- `test_fanout_all_fail` — all targets fail → Failed
- `test_fanout_single_target` — 1 target, requires 1 success → works
- `test_fanout_ec_quorum` — 6 targets, require 4 (EC 4+2), 4 succeed → Succeeded
- `test_manager_start_and_complete` — start fanout, record results, complete → returns final state
- `test_manager_record_result` — record_result returns correct state transitions
- `test_manager_check_timeouts` — expired op appears in timeout list
- `test_manager_in_flight_count` — count increases on start, decreases on complete
- `test_stats_counts` — start/succeed/fail/timeout operations, verify stats snapshot

---

## Module 2: `quorum.rs` — Quorum Voting

### Purpose
Tracks voting outcomes for distributed consensus operations. Used by A2 (Metadata) for
Raft-style quorum operations and by A4's fanout infrastructure to determine when enough
nodes have agreed. Supports N-of-M voting with configurable thresholds.

### Types to implement

```rust
/// Quorum policy determines how many votes are needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuorumPolicy {
    /// Strict majority: floor(N/2) + 1 votes needed.
    Majority,
    /// All N votes needed (unanimity).
    All,
    /// At least `n` votes needed out of total.
    AtLeast(usize),
}

/// A single vote in a quorum round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    /// Voter identifier (opaque 16-byte UUID).
    pub voter_id: [u8; 16],
    /// Whether this vote is in favor (true) or against (false).
    pub approve: bool,
    /// Optional rejection reason (if approve = false).
    pub reason: Option<String>,
}

/// Result of a quorum evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuorumResult {
    /// Quorum not yet reached — still collecting votes.
    Pending,
    /// Quorum achieved — enough approvals.
    Achieved,
    /// Quorum failed — too many rejections, not achievable.
    Failed,
    /// Quorum expired (timeout).
    Expired,
}

/// Configuration for a quorum round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumConfig {
    /// Total number of voters.
    pub total_voters: usize,
    /// Quorum policy.
    pub policy: QuorumPolicy,
    /// Round timeout in milliseconds (default: 5000).
    pub timeout_ms: u64,
}

impl Default for QuorumConfig {
    // total_voters=3, policy=Majority, timeout_ms=5000
}

/// A quorum round — collects votes and determines outcome.
pub struct QuorumRound {
    pub config: QuorumConfig,
    votes: HashMap<[u8; 16], Vote>,
    created_at_ms: u64,
    result: QuorumResult,
}

impl QuorumRound {
    pub fn new(config: QuorumConfig, now_ms: u64) -> Self;

    /// Record a vote. Returns error if voter already voted or round is concluded.
    pub fn vote(&mut self, v: Vote) -> Result<QuorumResult, QuorumError>;

    /// Force-check timeout. Updates result to Expired if timed out. Returns new result.
    pub fn check_timeout(&mut self, now_ms: u64) -> QuorumResult;

    /// Current result.
    pub fn result(&self) -> QuorumResult;

    /// Number of votes received so far.
    pub fn vote_count(&self) -> usize;

    /// Number of approvals so far.
    pub fn approval_count(&self) -> usize;

    /// Number of rejections so far.
    pub fn rejection_count(&self) -> usize;

    /// Required approvals based on policy.
    pub fn required(&self) -> usize;

    /// Whether quorum is still achievable (remaining votes + approvals >= required).
    pub fn achievable(&self) -> bool;

    /// List of voter IDs who have voted.
    pub fn voted_ids(&self) -> Vec<[u8; 16]>;
}

/// Error type for quorum operations.
#[derive(Debug, thiserror::Error)]
pub enum QuorumError {
    #[error("voter {0:?} already voted")]
    AlreadyVoted([u8; 16]),
    #[error("quorum round already concluded with result {0:?}")]
    AlreadyConcluded(QuorumResult),
}

/// Manager for multiple concurrent quorum rounds.
pub struct QuorumManager {
    next_id: AtomicU64,
    rounds: Mutex<HashMap<u64, QuorumRound>>,
    stats: Arc<QuorumStats>,
}

impl QuorumManager {
    pub fn new() -> Self;

    /// Start a new quorum round. Returns the round ID.
    pub fn start_round(&self, config: QuorumConfig, now_ms: u64) -> u64;

    /// Submit a vote for a round. Returns new QuorumResult, or None if round not found.
    pub fn vote(&self, round_id: u64, v: Vote) -> Option<Result<QuorumResult, QuorumError>>;

    /// Check timeouts. Returns IDs of rounds that expired.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<u64>;

    /// Remove a concluded round. Returns final result or None if not found.
    pub fn complete(&self, round_id: u64) -> Option<QuorumResult>;

    /// Active (pending) round count.
    pub fn active_count(&self) -> usize;

    pub fn stats(&self) -> Arc<QuorumStats>;
}

pub struct QuorumStats {
    pub rounds_started: AtomicU64,
    pub rounds_achieved: AtomicU64,
    pub rounds_failed: AtomicU64,
    pub rounds_expired: AtomicU64,
    pub total_votes: AtomicU64,
    pub total_approvals: AtomicU64,
    pub total_rejections: AtomicU64,
}

pub struct QuorumStatsSnapshot {
    pub rounds_started: u64,
    pub rounds_achieved: u64,
    pub rounds_failed: u64,
    pub rounds_expired: u64,
    pub total_votes: u64,
    pub total_approvals: u64,
    pub total_rejections: u64,
    pub active_rounds: usize,
}

impl QuorumStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, active_rounds: usize) -> QuorumStatsSnapshot;
}
```

### Tests (minimum 16)
- `test_quorum_majority_3_of_3` — 3 voters, majority=2, 2 approve → Achieved
- `test_quorum_majority_calculation` — for N=3, required=2; for N=5, required=3
- `test_quorum_all_policy` — 3 voters, All policy, needs 3 → 2 not enough
- `test_quorum_at_least_policy` — AtLeast(2), 3 voters, 2 approve → Achieved
- `test_quorum_rejection_fails` — 3 voters, majority=2, 2 reject → Failed
- `test_quorum_already_voted` — same voter votes twice → AlreadyVoted error
- `test_quorum_concluded_vote` — vote after Achieved → AlreadyConcluded error
- `test_quorum_timeout` — check_timeout after expiry → Expired
- `test_quorum_not_expired_yet` — check_timeout before expiry → Pending
- `test_quorum_achievable_false` — 2 required, 1 rejected, 1 pending, achievable=false
- `test_quorum_achievable_true` — 2 required, 1 approved, 1 pending, achievable=true
- `test_quorum_all_approve` — 3 of 3 approve → Achieved
- `test_manager_round_lifecycle` — start, vote, complete
- `test_manager_check_timeouts` — expired round in timeout list
- `test_manager_active_count` — count reflects active rounds
- `test_stats_snapshot` — verify stats after multiple rounds

---

## Module 3: `segment_router.rs` — EC Stripe-Aware Segment Routing

### Purpose
Computes which cluster nodes hold each EC stripe for a given segment. Implements the
data placement strategy from D1 (4+2 Reed-Solomon EC) and D8 (metadata-local primary,
distributed EC stripes). Used by A5 (FUSE client) for parallel reads and by A1 (storage)
for write distribution.

This is a pure computation module — no network I/O. Given a segment ID and a list of
available nodes, it deterministically returns the 6 nodes that should hold the stripes.

### Architecture reference
- D1: "4+2 (4 data + 2 parity) for clusters with 6+ nodes; 2+1 for 3-5 nodes"
- D8: "Background segment packer... distributes stripes across 6 different nodes via consistent hash"
- D3: "EC unit: 2MB packed segments (post-dedup, post-compression)"

### Types to implement

```rust
/// EC stripe configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EcConfig {
    /// 4 data + 2 parity (requires 6+ nodes). Default for large clusters.
    FourPlusTwo,
    /// 2 data + 1 parity (requires 3+ nodes). For small clusters.
    TwoPlusOne,
}

impl EcConfig {
    /// Number of data shards.
    pub fn data_shards(&self) -> usize;
    /// Number of parity shards.
    pub fn parity_shards(&self) -> usize;
    /// Total shards (data + parity).
    pub fn total_shards(&self) -> usize;
    /// Minimum nodes required.
    pub fn min_nodes(&self) -> usize;
}

/// A segment identifier (64-bit, derived from segment's BLAKE3 hash prefix or offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentId(pub u64);

/// A stripe assignment: which node holds which shard index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StripeAssignment {
    /// Node identifier.
    pub node_id: [u8; 16],
    /// Shard index (0..data_shards for data, data_shards..total_shards for parity).
    pub shard_index: usize,
    /// Whether this is a parity shard.
    pub is_parity: bool,
}

/// Result of routing a segment to EC stripe nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentPlacement {
    pub segment_id: SegmentId,
    pub ec_config: EcConfig,
    /// The stripe assignments, one per shard.
    pub stripes: Vec<StripeAssignment>,
    /// The primary node (metadata-owning node, from consistent hash of segment_id).
    pub primary_node: [u8; 16],
}

impl SegmentPlacement {
    /// Get the node for a specific shard index.
    pub fn node_for_shard(&self, shard_index: usize) -> Option<&StripeAssignment>;

    /// Get all data shard assignments.
    pub fn data_stripes(&self) -> Vec<&StripeAssignment>;

    /// Get all parity shard assignments.
    pub fn parity_stripes(&self) -> Vec<&StripeAssignment>;

    /// Whether this segment can be reconstructed if `failed_nodes` are unavailable.
    /// For 4+2: can reconstruct with up to 2 failures. For 2+1: up to 1 failure.
    pub fn can_reconstruct(&self, failed_nodes: &[[u8; 16]]) -> bool;
}

/// Error type for segment routing.
#[derive(Debug, thiserror::Error)]
pub enum SegmentRouterError {
    #[error("not enough nodes: need {needed} for {config:?}, have {available}")]
    InsufficientNodes { needed: usize, available: usize, config: EcConfig },
    #[error("shard index {0} out of range")]
    ShardIndexOutOfRange(usize),
}

/// Configuration for the segment router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentRouterConfig {
    /// EC configuration to use.
    pub ec_config: EcConfig,
    /// Seed for deterministic placement (set to 0 in production, non-zero for testing).
    pub placement_seed: u64,
}

impl Default for SegmentRouterConfig {
    // ec_config=FourPlusTwo, placement_seed=0
}

/// Routes segments to EC stripe nodes using consistent hashing.
pub struct SegmentRouter {
    config: SegmentRouterConfig,
    stats: Arc<SegmentRouterStats>,
}

impl SegmentRouter {
    pub fn new(config: SegmentRouterConfig) -> Self;

    /// Compute the EC stripe placement for a segment across the given nodes.
    ///
    /// Algorithm: use a deterministic hash of (segment_id XOR shard_index) to pick
    /// from available nodes, without replacement (each shard on a different node).
    /// If nodes.len() < ec_config.total_shards(), returns InsufficientNodes.
    pub fn place_segment(
        &self,
        segment_id: SegmentId,
        nodes: &[[u8; 16]],
    ) -> Result<SegmentPlacement, SegmentRouterError>;

    /// Determine which shard a specific node holds for a given segment.
    /// Returns None if the node is not involved in this segment's placement.
    pub fn shard_for_node(
        &self,
        segment_id: SegmentId,
        node_id: &[u8; 16],
        nodes: &[[u8; 16]],
    ) -> Option<usize>;

    /// List all segments that involve a given node, filtering from a segment list.
    pub fn segments_on_node(
        &self,
        node_id: &[u8; 16],
        segments: &[SegmentId],
        nodes: &[[u8; 16]],
    ) -> Vec<SegmentId>;

    /// Stats reference.
    pub fn stats(&self) -> Arc<SegmentRouterStats>;
}

pub struct SegmentRouterStats {
    pub placements_computed: AtomicU64,
    pub placement_errors: AtomicU64,
    pub shard_lookups: AtomicU64,
}

pub struct SegmentRouterStatsSnapshot {
    pub placements_computed: u64,
    pub placement_errors: u64,
    pub shard_lookups: u64,
}

impl SegmentRouterStats {
    pub fn new() -> Self;
    pub fn snapshot(&self) -> SegmentRouterStatsSnapshot;
}
```

### Tests (minimum 16)
- `test_ec_config_four_plus_two` — FourPlusTwo: data=4, parity=2, total=6
- `test_ec_config_two_plus_one` — TwoPlusOne: data=2, parity=1, total=3
- `test_place_segment_success` — 6 nodes, FourPlusTwo → 6 stripe assignments
- `test_place_segment_all_different_nodes` — no node appears twice in the 6 stripes
- `test_place_segment_insufficient_nodes` — 5 nodes, FourPlusTwo → InsufficientNodes error
- `test_place_segment_two_plus_one` — 3 nodes, TwoPlusOne → 3 stripe assignments
- `test_place_segment_deterministic` — same segment_id + nodes → same result
- `test_place_segment_different_segments` — different segment_ids → different node ordering
- `test_data_vs_parity_stripes` — data_stripes() returns 4 items, parity_stripes() returns 2
- `test_node_for_shard` — node_for_shard(0) returns first stripe's node
- `test_can_reconstruct_zero_failures` — no failures → can reconstruct
- `test_can_reconstruct_two_failures` — FourPlusTwo, 2 failures → can reconstruct
- `test_can_reconstruct_three_failures` — FourPlusTwo, 3 failures → cannot reconstruct
- `test_shard_for_node` — node in placement → returns correct shard index
- `test_segments_on_node` — multiple segments, node appears in some
- `test_stats_counts` — place_segment increments stats

## Implementation note for place_segment
Use this deterministic algorithm:
```
fn pick_nodes(segment_id: u64, shard_index: usize, nodes: &[[u8; 16]]) -> ... {
    // Hash the segment_id XOR (shard_index as u64) XOR placement_seed
    // Use FNV-1a or a simple integer hash to spread across nodes
    // Pick without replacement: for shard 0 pick from all nodes,
    //   for shard 1 pick from remaining nodes, etc.
    // Simple approach: compute hash for each node position, sort by hash, take top N
}
```

The primary node is the node assigned to shard 0.

---

## Output Format

```
=== FILE: crates/claudefs-transport/src/fanout.rs ===
<complete file content>

=== FILE: crates/claudefs-transport/src/quorum.rs ===
<complete file content>

=== FILE: crates/claudefs-transport/src/segment_router.rs ===
<complete file content>
```

Then the lib.rs additions:

```
=== LIB.RS ADDITIONS ===
// Module declarations to add (alphabetical):
pub mod fanout;
pub mod quorum;
pub mod segment_router;

// Re-exports to add:
pub use fanout::{
    FanoutConfig, FanoutId, FanoutManager, FanoutOp, FanoutState, FanoutStats, FanoutStatsSnapshot,
    FanoutTarget, FanoutTargetResult,
};
pub use quorum::{
    QuorumConfig, QuorumError, QuorumManager, QuorumPolicy, QuorumResult, QuorumRound,
    QuorumStats, QuorumStatsSnapshot, Vote,
};
pub use segment_router::{
    EcConfig, SegmentId, SegmentPlacement, SegmentRouter, SegmentRouterConfig,
    SegmentRouterError, SegmentRouterStats, SegmentRouterStatsSnapshot, StripeAssignment,
};
```

## Important
- Produce complete, compilable Rust files — no TODOs, no unimplemented!()
- All test names must be descriptive
- For place_segment, use a simple deterministic shuffle:
  sort nodes by `fnv1a(segment_id XOR shard_idx XOR node_bytes_as_u64) mod N` without replacement
  (simplest: compute a hash score for each node, sort by score, take first N positions)
- FanoutOp.record_result: update state immediately — if quorum met → Succeeded; if quorum impossible → Failed
- QuorumRound.required(): for Majority → (total_voters / 2) + 1; for All → total_voters; for AtLeast(n) → n
