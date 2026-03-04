# A4 Phase 10: Write Pipeline, Splice Queue, Connection Drain Aware

You are implementing 3 new modules for the `claudefs-transport` crate in the ClaudeFS distributed
filesystem (Rust, Cargo workspace). The crate has 72 modules and 1176 passing tests.

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
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;
```

---

## Module 1: `write_pipeline.rs` — Write Pipeline Stage Tracker

### Purpose
Tracks a write request as it moves through the ClaudeFS write pipeline stages (per D3/D8):
1. Client receive → journal write (local NVMe)
2. Journal replicate (2x sync to peers)
3. Segment pack (accumulate to 2MB)
4. EC distribute (4+2 stripes to 6 nodes)
5. S3 async upload (cache mode, D5)

Used by A1 (storage) to track write state, by A5 (FUSE) to know when to ack
to the client, and by A8 (monitoring) for write latency breakdown.

### Types to implement

```rust
/// Unique ID for a write pipeline operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WriteId(pub u64);

/// Stage in the write pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WriteStage {
    /// Write request received, pending journal write.
    Received,
    /// Written to local NVMe journal.
    JournalWritten,
    /// Replicated to 2 peers (D3 — client ack point).
    JournalReplicated,
    /// Packed into a 2MB segment.
    SegmentPacked,
    /// EC stripes distributed to 6 nodes.
    EcDistributed,
    /// Uploaded to S3 (cache mode, D5).
    S3Uploaded,
    /// Write fully complete.
    Complete,
}

impl WriteStage {
    /// The stage at which the client receives the ack (after journal replicated, per D3).
    pub fn client_ack_stage() -> Self { WriteStage::JournalReplicated }
    /// Whether the client has been acked at this stage.
    pub fn is_client_acked(&self) -> bool;
}

/// A write pipeline operation tracking one client write.
#[derive(Debug)]
pub struct WritePipelineOp {
    pub id: WriteId,
    pub stage: WriteStage,
    pub size_bytes: u32,
    pub created_at_ms: u64,
    /// Timestamp when each stage was reached (None if not yet reached).
    pub stage_timestamps: [Option<u64>; 7],  // one per WriteStage variant
}

impl WritePipelineOp {
    pub fn new(id: WriteId, size_bytes: u32, now_ms: u64) -> Self;

    /// Advance to the next stage. Returns error if already at Complete or if stage order violated.
    pub fn advance(&mut self, stage: WriteStage, now_ms: u64) -> Result<(), WriteError>;

    /// Latency to reach a specific stage from creation (None if stage not reached).
    pub fn latency_to_stage_ms(&self, stage: WriteStage) -> Option<u64>;

    /// Whether the client has been acked (stage >= JournalReplicated).
    pub fn is_client_acked(&self) -> bool;
}

/// Error for write pipeline operations.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("write {0:?} not found")]
    NotFound(WriteId),
    #[error("invalid stage transition from {from:?} to {to:?}")]
    InvalidTransition { from: WriteStage, to: WriteStage },
    #[error("write {0:?} already complete")]
    AlreadyComplete(WriteId),
}

/// Manager for concurrent write pipeline operations.
pub struct WritePipelineManager {
    next_id: AtomicU64,
    ops: Mutex<HashMap<WriteId, WritePipelineOp>>,
    stats: Arc<WritePipelineStats>,
}

impl WritePipelineManager {
    pub fn new() -> Self;

    /// Start tracking a new write.
    pub fn start(&self, size_bytes: u32, now_ms: u64) -> WriteId;

    /// Advance a write to a new stage. Returns new stage, or error if not found / invalid.
    pub fn advance(&self, id: WriteId, stage: WriteStage, now_ms: u64) -> Result<WriteStage, WriteError>;

    /// Complete a write (moves to Complete stage, removes from active tracking).
    pub fn complete(&self, id: WriteId, now_ms: u64) -> Result<(), WriteError>;

    /// Get the current stage of a write.
    pub fn stage(&self, id: WriteId) -> Option<WriteStage>;

    /// Number of active (not-yet-complete) writes.
    pub fn active_count(&self) -> usize;

    /// Number of writes that have been client-acked but not yet EC-distributed (pending background work).
    pub fn pending_background_count(&self) -> usize;

    pub fn stats(&self) -> Arc<WritePipelineStats>;
}

pub struct WritePipelineStats {
    pub writes_started: AtomicU64,
    pub writes_completed: AtomicU64,
    pub client_acks_issued: AtomicU64,
    pub ec_distributions_completed: AtomicU64,
    pub s3_uploads_completed: AtomicU64,
    pub total_bytes_written: AtomicU64,
}

pub struct WritePipelineStatsSnapshot {
    pub writes_started: u64,
    pub writes_completed: u64,
    pub client_acks_issued: u64,
    pub ec_distributions_completed: u64,
    pub s3_uploads_completed: u64,
    pub total_bytes_written: u64,
    pub active_writes: usize,
    pub pending_background: usize,
}

impl WritePipelineStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, active: usize, pending_bg: usize) -> WritePipelineStatsSnapshot;
}
```

### Tests (minimum 15)
- `test_new_op_starts_at_received`
- `test_advance_single_stage`
- `test_advance_multiple_stages_in_order`
- `test_client_ack_stage_is_journal_replicated`
- `test_is_client_acked_false_before_replication`
- `test_is_client_acked_true_after_replication`
- `test_latency_to_stage` — advance to stage, latency_to_stage_ms returns > 0
- `test_latency_to_stage_unreached` — stage not reached returns None
- `test_advance_invalid_transition` — backwards stage returns error
- `test_advance_already_complete` — advance after Complete returns error
- `test_manager_start_and_advance`
- `test_manager_complete`
- `test_manager_active_count`
- `test_manager_pending_background_count` — count of acked but not ec-distributed
- `test_stats_counts`

---

## Module 2: `splice_queue.rs` — Zero-Copy Splice Queue

### Purpose
A FIFO queue tracking zero-copy splice operations for disk-to-network data transfer
(D3 write path: read from NVMe → splice to network socket). Each entry represents
a pending or in-progress splice operation with source (NVMe block) and destination
(network connection) metadata.

This is a pure state-tracking module — actual splice(2) calls happen in A1/A5
which use io_uring. This module tracks what's queued and provides backpressure.

### Types to implement

```rust
/// Identifies the source of splice data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceSource {
    /// Device/file descriptor identifier (opaque u64, maps to NVMe block device fd).
    pub fd_id: u64,
    /// Byte offset in the source.
    pub offset: u64,
    /// Byte length to splice.
    pub length: u32,
}

/// Identifies the destination of splice data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceDestination {
    /// Connection identifier (opaque u64).
    pub conn_id: u64,
    /// Optional offset (for RDMA writes; 0 for TCP streaming).
    pub offset: u64,
}

/// State of a splice operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpliceState {
    /// Queued, not yet submitted to io_uring.
    Pending,
    /// Submitted to io_uring, waiting for completion.
    InFlight,
    /// Completed successfully.
    Done,
    /// Failed.
    Failed,
}

/// A single splice queue entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceEntry {
    pub id: u64,
    pub source: SpliceSource,
    pub dest: SpliceDestination,
    pub state: SpliceState,
    pub queued_at_ms: u64,
    pub submitted_at_ms: Option<u64>,
    pub completed_at_ms: Option<u64>,
}

/// Configuration for the splice queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceQueueConfig {
    /// Maximum entries in the queue (pending + in-flight, default: 1024).
    pub max_entries: usize,
    /// Maximum concurrent in-flight splice operations (default: 64).
    pub max_inflight: usize,
    /// Timeout for an in-flight splice in milliseconds (default: 5000).
    pub inflight_timeout_ms: u64,
}

impl Default for SpliceQueueConfig {
    // max_entries=1024, max_inflight=64, inflight_timeout_ms=5000
}

/// Error type for splice queue operations.
#[derive(Debug, thiserror::Error)]
pub enum SpliceQueueError {
    #[error("splice queue full (max {0})")]
    QueueFull(usize),
    #[error("max inflight reached (max {0})")]
    MaxInflight(usize),
    #[error("splice entry {0} not found")]
    NotFound(u64),
}

/// Zero-copy splice operation queue.
pub struct SpliceQueue {
    config: SpliceQueueConfig,
    next_id: AtomicU64,
    pending: Mutex<VecDeque<SpliceEntry>>,
    inflight: Mutex<HashMap<u64, SpliceEntry>>,
    stats: Arc<SpliceQueueStats>,
}

impl SpliceQueue {
    pub fn new(config: SpliceQueueConfig) -> Self;

    /// Enqueue a new splice operation. Returns id, or error if queue full.
    pub fn enqueue(&self, source: SpliceSource, dest: SpliceDestination, now_ms: u64) -> Result<u64, SpliceQueueError>;

    /// Dequeue the next pending entry for submission to io_uring.
    /// Returns None if no pending entries, or if max_inflight is reached.
    pub fn dequeue_for_submit(&self, now_ms: u64) -> Result<Option<SpliceEntry>, SpliceQueueError>;

    /// Record completion of an in-flight splice.
    /// Returns the completed entry, or error if not found.
    pub fn complete(&self, id: u64, success: bool, now_ms: u64) -> Result<SpliceEntry, SpliceQueueError>;

    /// Check for timed-out in-flight splices. Returns their IDs.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<u64>;

    /// Number of pending entries.
    pub fn pending_count(&self) -> usize;

    /// Number of in-flight entries.
    pub fn inflight_count(&self) -> usize;

    /// Total entries (pending + in-flight).
    pub fn total_count(&self) -> usize;

    pub fn stats(&self) -> Arc<SpliceQueueStats>;
}

pub struct SpliceQueueStats {
    pub enqueued: AtomicU64,
    pub submitted: AtomicU64,
    pub completed: AtomicU64,
    pub failed: AtomicU64,
    pub timed_out: AtomicU64,
    pub total_bytes_spliced: AtomicU64,
}

pub struct SpliceQueueStatsSnapshot {
    pub enqueued: u64,
    pub submitted: u64,
    pub completed: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub total_bytes_spliced: u64,
    pub pending_count: usize,
    pub inflight_count: usize,
}

impl SpliceQueueStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, pending: usize, inflight: usize) -> SpliceQueueStatsSnapshot;
}
```

### Tests (minimum 15)
- `test_enqueue_success`
- `test_enqueue_queue_full` — exceed max_entries → QueueFull
- `test_dequeue_for_submit_basic` — enqueue then dequeue, state InFlight
- `test_dequeue_for_submit_max_inflight` — at max_inflight, dequeue returns None (not error)
- `test_dequeue_empty_queue` — empty queue returns Ok(None)
- `test_complete_success` — complete in-flight splice, state Done
- `test_complete_not_found` — complete unknown id → NotFound
- `test_complete_failure` — complete with success=false, state Failed
- `test_check_timeouts` — timed-out in-flight appears in result
- `test_check_timeouts_fresh` — fresh in-flight NOT in result
- `test_pending_count` — count reflects pending entries
- `test_inflight_count` — count reflects in-flight entries
- `test_total_count` — pending + inflight
- `test_stats_bytes_spliced` — complete splice, total_bytes_spliced incremented
- `test_multiple_enqueue_dequeue`

---

## Module 3: `conn_drain_aware.rs` — Drain-Aware Connection Wrapper

### Purpose
Integrates with the existing `drain.rs` module (DrainController/DrainGuard) to
make connection-level operations drain-aware. When a drain is signaled (e.g., for
graceful node shutdown), new requests are rejected and in-flight requests are tracked
until completion.

This module provides a wrapper around connection-level state that coordinates with
the drain protocol used by A11 (Infrastructure) for graceful cluster node shutdown.

Note: `drain.rs` already exists with `DrainController`, `DrainGuard`, `DrainConfig`,
`DrainState`, `DrainStats`. This module builds ON TOP of that.

### Types to implement

```rust
/// State of a drain-aware connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnDrainState {
    /// Normal operation — accepting new requests.
    Active,
    /// Drain signaled — no new requests, waiting for in-flight to complete.
    Draining,
    /// All in-flight requests completed — ready for connection close.
    Drained,
}

/// Configuration for drain-aware connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnDrainConfig {
    /// Maximum time to wait for drain completion in ms (default: 30000).
    pub drain_timeout_ms: u64,
    /// How long to wait before force-closing (after drain_timeout_ms, default: 5000).
    pub force_close_delay_ms: u64,
}

impl Default for ConnDrainConfig {
    // drain_timeout_ms=30000, force_close_delay_ms=5000
}

/// Error type for drain-aware operations.
#[derive(Debug, thiserror::Error)]
pub enum ConnDrainError {
    #[error("connection is draining — no new requests accepted")]
    Draining,
    #[error("connection is already drained")]
    AlreadyDrained,
    #[error("drain timed out after {0}ms")]
    DrainTimedOut(u64),
}

/// A drain-aware connection tracker.
/// Wraps per-connection inflight request tracking with drain awareness.
pub struct ConnDrainTracker {
    config: ConnDrainConfig,
    conn_id: u64,
    state: Mutex<ConnDrainState>,
    inflight_count: AtomicU64,
    drain_started_at_ms: Mutex<Option<u64>>,
    stats: Arc<ConnDrainStats>,
}

impl ConnDrainTracker {
    pub fn new(conn_id: u64, config: ConnDrainConfig) -> Self;

    /// Attempt to register a new in-flight request.
    /// Returns error if state is Draining or Drained.
    pub fn begin_request(&self) -> Result<(), ConnDrainError>;

    /// Complete an in-flight request. Decrements inflight count.
    /// If state is Draining and inflight reaches 0, transitions to Drained.
    pub fn end_request(&self);

    /// Signal that this connection should drain (no new requests).
    /// Returns error if already draining/drained.
    pub fn begin_drain(&self, now_ms: u64) -> Result<(), ConnDrainError>;

    /// Check drain timeout. Returns DrainTimedOut if drain_timeout_ms has elapsed.
    pub fn check_drain_timeout(&self, now_ms: u64) -> Result<(), ConnDrainError>;

    /// Current state.
    pub fn state(&self) -> ConnDrainState;

    /// Current in-flight request count.
    pub fn inflight_count(&self) -> u64;

    /// Whether drain is complete (state == Drained).
    pub fn is_drained(&self) -> bool;

    pub fn stats(&self) -> Arc<ConnDrainStats>;
}

/// Manager for multiple drain-aware connections.
pub struct ConnDrainManager {
    config: ConnDrainConfig,
    connections: RwLock<HashMap<u64, ConnDrainTracker>>,
    stats: Arc<ConnDrainStats>,
}

impl ConnDrainManager {
    pub fn new(config: ConnDrainConfig) -> Self;

    /// Register a new connection.
    pub fn register(&self, conn_id: u64);

    /// Remove a connection.
    pub fn remove(&self, conn_id: u64);

    /// Begin request on a connection. Returns error if draining/drained/not found.
    pub fn begin_request(&self, conn_id: u64) -> Result<(), ConnDrainError>;

    /// End request on a connection.
    pub fn end_request(&self, conn_id: u64);

    /// Signal drain on a specific connection.
    pub fn drain_connection(&self, conn_id: u64, now_ms: u64) -> Result<(), ConnDrainError>;

    /// Signal drain on ALL connections.
    pub fn drain_all(&self, now_ms: u64);

    /// Check drain timeouts across all connections. Returns conn_ids that timed out.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<u64>;

    /// Count of connections in each state.
    pub fn state_counts(&self) -> (usize, usize, usize);  // (active, draining, drained)

    pub fn stats(&self) -> Arc<ConnDrainStats>;
}

pub struct ConnDrainStats {
    pub connections_registered: AtomicU64,
    pub connections_removed: AtomicU64,
    pub drains_initiated: AtomicU64,
    pub drains_completed: AtomicU64,
    pub drains_timed_out: AtomicU64,
    pub requests_rejected: AtomicU64,
    pub total_requests: AtomicU64,
}

pub struct ConnDrainStatsSnapshot {
    pub connections_registered: u64,
    pub connections_removed: u64,
    pub drains_initiated: u64,
    pub drains_completed: u64,
    pub drains_timed_out: u64,
    pub requests_rejected: u64,
    pub total_requests: u64,
    pub active_connections: usize,
    pub draining_connections: usize,
    pub drained_connections: usize,
}

impl ConnDrainStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, active: usize, draining: usize, drained: usize) -> ConnDrainStatsSnapshot;
}
```

### Tests (minimum 15)
- `test_new_tracker_is_active`
- `test_begin_request_success`
- `test_begin_request_while_draining` — returns Draining error
- `test_begin_request_while_drained` — returns AlreadyDrained error
- `test_end_request_decrements_count`
- `test_drain_transitions_to_draining`
- `test_drain_with_no_inflight_transitions_to_drained`
- `test_drain_completes_when_last_request_ends` — begin_drain with 1 inflight, end_request → Drained
- `test_drain_already_draining` — begin_drain twice → AlreadyDrained error
- `test_drain_timeout` — check_drain_timeout after timeout_ms → DrainTimedOut
- `test_drain_timeout_not_expired` — before timeout → Ok
- `test_manager_register_and_remove`
- `test_manager_drain_all`
- `test_manager_state_counts`
- `test_stats_counts`

---

## Output Format

For each module:
```
=== FILE: crates/claudefs-transport/src/write_pipeline.rs ===
<complete file content>

=== FILE: crates/claudefs-transport/src/splice_queue.rs ===
<complete file content>

=== FILE: crates/claudefs-transport/src/conn_drain_aware.rs ===
<complete file content>
```

Then lib.rs additions:
```
=== LIB.RS ADDITIONS ===
pub mod write_pipeline;
pub mod splice_queue;
pub mod conn_drain_aware;

pub use write_pipeline::{
    WriteError, WriteId, WritePipelineManager, WritePipelineOp, WritePipelineStats,
    WritePipelineStatsSnapshot, WriteStage,
};
pub use splice_queue::{
    SpliceDestination, SpliceEntry, SpliceQueue, SpliceQueueConfig, SpliceQueueError,
    SpliceQueueStats, SpliceQueueStatsSnapshot, SpliceSource, SpliceState,
};
pub use conn_drain_aware::{
    ConnDrainConfig, ConnDrainError, ConnDrainManager, ConnDrainState, ConnDrainStats,
    ConnDrainStatsSnapshot, ConnDrainTracker,
};
```

## Important
- Complete compilable Rust — no TODOs
- WriteStage ordering: Received < JournalWritten < JournalReplicated < SegmentPacked < EcDistributed < S3Uploaded < Complete
- advance() must enforce ordering (reject going backwards)
- stage_timestamps: array of 7 Option<u64>, index by WriteStage as usize
- SpliceQueue.dequeue_for_submit returns Ok(None) when empty or max_inflight reached, NOT an error
- ConnDrainTracker: all state transitions use Mutex for correctness
