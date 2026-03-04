# Implement 3 new modules for claudefs-transport crate

## Context

You are implementing 3 new modules for the `claudefs-transport` crate in the ClaudeFS distributed filesystem (Rust, Cargo workspace). The crate follows strict coding conventions.

## Coding Conventions (MANDATORY — follow exactly)

1. **No external async dependencies** — pure sync Rust. No `tokio`, no async/await. Tests must use std::thread or simple sync code only.
2. **Serde derive** on all public types: `#[derive(Debug, Clone, Serialize, Deserialize)]`
3. **Atomic counters** for stats: `AtomicU64`, `AtomicU32` with `Ordering::Relaxed`
4. **Stats snapshot pattern**: `XxxStats` (atomic) + `XxxStatsSnapshot` (plain struct with snapshot())
5. **Error types** with `thiserror`: `#[derive(Debug, thiserror::Error)]`
6. **No unwrap/expect** in production code
7. **Tests**: minimum 15 tests per module in `#[cfg(test)] mod tests` at bottom
8. **Module-level doc comment** `//!` at top of each file

## Standard imports available

```rust
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;
use thiserror::Error;
```

---

## Module 1: `repl_state.rs` — Journal Replication State Machine

### Purpose
Tracks the state of per-connection journal replication channels. Each storage node maintains one replication connection per peer (for D3: 2x journal replication). This module tracks which journal entries have been sent, acknowledged, and can have their space reclaimed.

### Types to implement

```rust
/// A journal entry sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct JournalSeq(pub u64);

impl JournalSeq {
    /// Returns the next sequence number.
    pub fn next(self) -> Self {
        JournalSeq(self.0 + 1)
    }

    /// Returns true if self is before other (self < other).
    pub fn is_before(self, other: Self) -> bool {
        self.0 < other.0
    }
}

/// State of journal replication for a single peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplState {
    /// Initial state — not yet connected.
    Idle,
    /// Connected and syncing historical entries.
    Syncing,
    /// Up to date — sending live journal entries.
    Live,
    /// Connection lost — will retry.
    Disconnected,
    /// Peer is too far behind — needs full resync.
    NeedsResync,
}

/// A journal entry record tracked by the replication state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub seq: JournalSeq,
    /// Byte size of this entry.
    pub size_bytes: u32,
    /// Timestamp when this entry was written (ms since epoch).
    pub written_at_ms: u64,
}

/// Configuration for replication state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplStateConfig {
    /// Maximum in-flight (sent but not acked) journal entries (default: 256).
    pub max_inflight: usize,
    /// Maximum gap between local head and peer ack before NeedsResync (default: 10000).
    pub max_lag_entries: u64,
    /// Timeout in ms before declaring a connection lost (default: 10000).
    pub connection_timeout_ms: u64,
}

impl Default for ReplStateConfig {
    fn default() -> Self {
        Self {
            max_inflight: 256,
            max_lag_entries: 10000,
            connection_timeout_ms: 10000,
        }
    }
}

/// Replication state for one peer connection.
pub struct ReplChannel {
    config: ReplStateConfig,
    peer_id: [u8; 16],
    state: ReplState,
    /// Sequence of the latest journal entry written locally.
    local_head: JournalSeq,
    /// Sequence of the latest entry acknowledged by the peer.
    peer_acked: JournalSeq,
    /// In-flight entries: sent to peer, waiting for ack.
    inflight: VecDeque<JournalEntry>,
    /// Last activity timestamp (ms since epoch).
    last_activity_ms: u64,
    stats: Arc<ReplChannelStats>,
}

impl ReplChannel {
    /// Creates a new replication channel for a peer.
    pub fn new(peer_id: [u8; 16], config: ReplStateConfig, now_ms: u64) -> Self {
        Self {
            config,
            peer_id,
            state: ReplState::Idle,
            local_head: JournalSeq(0),
            peer_acked: JournalSeq(0),
            inflight: VecDeque::new(),
            last_activity_ms: now_ms,
            stats: Arc::new(ReplChannelStats::new()),
        }
    }

    /// Record a new locally-written journal entry. Returns whether the entry was accepted.
    /// Returns false (and sets state to NeedsResync) if lag exceeds max_lag_entries.
    pub fn advance_local(&mut self, entry: JournalEntry, now_ms: u64) -> bool {
        self.last_activity_ms = now_ms;

        let lag = self.lag();
        if lag >= self.config.max_lag_entries as u64 {
            self.state = ReplState::NeedsResync;
            self.stats.resync_events.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        if self.inflight.len() >= self.config.max_inflight {
            return false;
        }

        if entry.seq.0 > self.local_head.0 {
            self.local_head = entry.seq;
        }
        self.inflight.push_back(entry);
        self.stats.entries_sent.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Record an ack from the peer for entries up to and including `seq`.
    /// Removes acked entries from inflight queue.
    pub fn ack(&mut self, seq: JournalSeq, now_ms: u64) {
        self.last_activity_ms = now_ms;

        while let Some(front) = self.inflight.front() {
            if front.seq.is_before(seq) || front.seq == seq {
                self.inflight.pop_front();
            } else {
                break;
            }
        }

        if seq.0 > self.peer_acked.0 {
            self.peer_acked = seq;
        }
        self.stats.entries_acked.fetch_add(1, Ordering::Relaxed);
    }

    /// Check for timeout. Updates state to Disconnected if no activity in connection_timeout_ms.
    pub fn check_timeout(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.last_activity_ms);
        if elapsed >= self.config.connection_timeout_ms && self.state != ReplState::Disconnected {
            self.state = ReplState::Disconnected;
            self.stats.disconnections.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Mark connection as established (transitions Idle/Disconnected → Syncing).
    pub fn connect(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
        if self.state == ReplState::Idle || self.state == ReplState::Disconnected {
            self.state = ReplState::Syncing;
        }
    }

    /// Mark sync as complete (transitions Syncing → Live).
    pub fn mark_live(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
        if self.state == ReplState::Syncing {
            self.state = ReplState::Live;
        }
    }

    /// Mark connection as lost (transitions * → Disconnected).
    pub fn disconnect(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
        self.state = ReplState::Disconnected;
        self.stats.disconnections.fetch_add(1, Ordering::Relaxed);
    }

    /// Current state.
    pub fn state(&self) -> ReplState {
        self.state
    }

    /// Current lag in entries (local_head - peer_acked).
    pub fn lag(&self) -> u64 {
        self.local_head.0.saturating_sub(self.peer_acked.0)
    }

    /// Number of in-flight (sent, not acked) entries.
    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    /// Whether the peer is caught up (lag == 0).
    pub fn is_caught_up(&self) -> bool {
        self.lag() == 0
    }

    /// Peer node ID.
    pub fn peer_id(&self) -> [u8; 16] {
        self.peer_id
    }

    /// Returns the stats for this channel.
    pub fn stats(&self) -> Arc<ReplChannelStats> {
        Arc::clone(&self.stats)
    }
}

/// Statistics for a replication channel.
pub struct ReplChannelStats {
    pub entries_sent: AtomicU64,
    pub entries_acked: AtomicU64,
    pub ack_timeouts: AtomicU64,
    pub disconnections: AtomicU64,
    pub resync_events: AtomicU64,
}

impl ReplChannelStats {
    /// Creates a new ReplChannelStats with zero counters.
    pub fn new() -> Self {
        Self {
            entries_sent: AtomicU64::new(0),
            entries_acked: AtomicU64::new(0),
            ack_timeouts: AtomicU64::new(0),
            disconnections: AtomicU64::new(0),
            resync_events: AtomicU64::new(0),
        }
    }

    /// Creates a snapshot of the current stats.
    pub fn snapshot(&self, lag: u64, inflight: usize, state: ReplState) -> ReplChannelStatsSnapshot {
        ReplChannelStatsSnapshot {
            entries_sent: self.entries_sent.load(Ordering::Relaxed),
            entries_acked: self.entries_acked.load(Ordering::Relaxed),
            ack_timeouts: self.ack_timeouts.load(Ordering::Relaxed),
            disconnections: self.disconnections.load(Ordering::Relaxed),
            resync_events: self.resync_events.load(Ordering::Relaxed),
            current_lag: lag,
            inflight_count: inflight,
            state,
        }
    }
}

/// Snapshot of ReplChannelStats.
pub struct ReplChannelStatsSnapshot {
    pub entries_sent: u64,
    pub entries_acked: u64,
    pub ack_timeouts: u64,
    pub disconnections: u64,
    pub resync_events: u64,
    pub current_lag: u64,
    pub inflight_count: usize,
    pub state: ReplState,
}
```

### Tests (minimum 15)
- `test_new_channel_idle` — new channel is in Idle state
- `test_connect_transitions_to_syncing` — connect() moves to Syncing
- `test_mark_live_transitions_to_live` — mark_live() moves Syncing → Live
- `test_advance_local_increments_lag` — advance_local adds entry, lag increases
- `test_ack_reduces_lag` — ack() removes entries from inflight
- `test_ack_cumulative` — ack for seq 5 removes all entries seq <= 5
- `test_disconnect_transitions_state` — disconnect() → Disconnected
- `test_timeout_triggers_disconnect` — check_timeout after timeout_ms → Disconnected
- `test_timeout_not_expired` — check_timeout before timeout_ms → no change
- `test_max_lag_triggers_resync` — exceed max_lag_entries → NeedsResync
- `test_inflight_count` — advance_local + ack, inflight_count correct
- `test_is_caught_up_true` — lag == 0 → caught up
- `test_is_caught_up_false` — lag > 0 → not caught up
- `test_stats_snapshot` — verify stat counts after operations
- `test_journal_seq_ordering` — JournalSeq comparison, next(), is_before()

---

## Module 2: `read_repair.rs` — EC Read Repair Tracker

### Purpose
Tracks in-progress read repair operations for EC-encoded segments. When a read detects a missing or corrupt shard, a repair operation is initiated: fetch surviving shards, reconstruct missing data, and write repaired shards back to their nodes.

### Types to implement

```rust
/// Unique ID for a repair operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepairId(pub u64);

/// State of a single shard in a repair operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardRepairState {
    /// Fetching data from the shard node.
    Fetching,
    /// Data fetched successfully.
    Fetched,
    /// Fetch failed (node unreachable or checksum error).
    Failed,
    /// This shard is missing — will be reconstructed.
    Missing,
    /// Reconstruction completed — writing repaired shard back.
    Reconstructing,
    /// Write-back completed — shard is repaired.
    Repaired,
}

/// One shard in a repair operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairShard {
    pub node_id: [u8; 16],
    pub shard_index: usize,
    pub state: ShardRepairState,
}

/// Priority of a repair operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RepairPriority {
    /// Background repair — triggered by node failure, non-urgent.
    Background,
    /// Foreground repair — blocking a client read, urgent.
    Foreground,
}

/// Configuration for read repair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRepairConfig {
    /// Timeout for the entire repair in milliseconds (default: 30000).
    pub timeout_ms: u64,
    /// Maximum concurrent repairs (default: 16).
    pub max_concurrent: usize,
}

impl Default for ReadRepairConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30000,
            max_concurrent: 16,
        }
    }
}

/// Error for repair operations.
#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    #[error("repair {0:?} not found")]
    NotFound(RepairId),
    #[error("too many concurrent repairs (max {0})")]
    TooManyConcurrent(usize),
    #[error("cannot reconstruct: only {available} shards available, need {needed}")]
    InsufficientShards { available: usize, needed: usize },
    #[error("repair {0:?} already completed")]
    AlreadyCompleted(RepairId),
}

/// State of the overall repair operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepairOpState {
    /// Fetching available shards.
    Fetching,
    /// Reconstructing missing shards from available ones.
    Reconstructing,
    /// Writing repaired shards back to nodes.
    WritingBack,
    /// Repair complete.
    Complete,
    /// Repair failed (not enough shards available).
    Failed,
    /// Repair timed out.
    TimedOut,
}

/// A tracked repair operation.
pub struct RepairOp {
    pub id: RepairId,
    pub segment_id: u64,
    pub priority: RepairPriority,
    pub shards: Vec<RepairShard>,
    pub state: RepairOpState,
    pub created_at_ms: u64,
    pub ec_data_shards: usize,
    pub ec_parity_shards: usize,
}

impl RepairOp {
    /// Creates a new repair operation.
    pub fn new(
        id: RepairId,
        segment_id: u64,
        priority: RepairPriority,
        shards: Vec<RepairShard>,
        ec_data_shards: usize,
        ec_parity_shards: usize,
        now_ms: u64,
    ) -> Self {
        Self {
            id,
            segment_id,
            priority,
            shards,
            state: RepairOpState::Fetching,
            created_at_ms: now_ms,
            ec_data_shards,
            ec_parity_shards,
        }
    }

    /// Record a fetch result for a shard node.
    pub fn record_fetch(&mut self, node_id: &[u8; 16], success: bool) {
        for shard in &mut self.shards {
            if &shard.node_id == node_id {
                if success {
                    shard.state = ShardRepairState::Fetched;
                } else {
                    shard.state = ShardRepairState::Failed;
                }
                break;
            }
        }
    }

    /// Begin reconstruction phase (called when enough shards are fetched).
    /// Returns error if not enough shards are available.
    pub fn begin_reconstruct(&mut self) -> Result<(), RepairError> {
        if !self.can_reconstruct() {
            return Err(RepairError::InsufficientShards {
                available: self.fetched_count(),
                needed: self.ec_data_shards,
            });
        }
        self.state = RepairOpState::Reconstructing;
        for shard in &mut self.shards {
            if shard.state == ShardRepairState::Missing || shard.state == ShardRepairState::Failed {
                shard.state = ShardRepairState::Reconstructing;
            }
        }
        Ok(())
    }

    /// Begin write-back phase.
    pub fn begin_writeback(&mut self) {
        self.state = RepairOpState::WritingBack;
    }

    /// Mark repair as complete.
    pub fn complete(&mut self) {
        self.state = RepairOpState::Complete;
        for shard in &mut self.shards {
            if shard.state == ShardRepairState::Reconstructing {
                shard.state = ShardRepairState::Repaired;
            }
        }
    }

    /// Mark repair as failed.
    pub fn fail(&mut self) {
        self.state = RepairOpState::Failed;
    }

    /// Check timeout. Returns true if timed out.
    pub fn check_timeout(&mut self, now_ms: u64) -> bool {
        let elapsed = now_ms.saturating_sub(self.created_at_ms);
        if elapsed >= 30000 {
            self.state = RepairOpState::TimedOut;
            true
        } else {
            false
        }
    }

    /// Number of fetched (healthy) shards.
    pub fn fetched_count(&self) -> usize {
        self.shards.iter().filter(|s| s.state == ShardRepairState::Fetched).count()
    }

    /// Number of missing shards (need reconstruction).
    pub fn missing_count(&self) -> usize {
        self.shards.iter().filter(|s| s.state == ShardRepairState::Missing).count()
    }

    /// Whether enough shards are available to reconstruct.
    /// Can reconstruct if fetched_count >= ec_data_shards
    pub fn can_reconstruct(&self) -> bool {
        self.fetched_count() >= self.ec_data_shards
    }
}

/// Manager for concurrent repair operations.
pub struct ReadRepairManager {
    config: ReadRepairConfig,
    next_id: AtomicU64,
    ops: Mutex<HashMap<RepairId, RepairOp>>,
    stats: Arc<ReadRepairStats>,
}

impl ReadRepairManager {
    /// Creates a new ReadRepairManager with the given configuration.
    pub fn new(config: ReadRepairConfig) -> Self {
        Self {
            config,
            next_id: AtomicU64::new(1),
            ops: Mutex::new(HashMap::new()),
            stats: Arc::new(ReadRepairStats::new()),
        }
    }

    /// Start a new repair. Returns error if too many concurrent repairs.
    pub fn start_repair(
        &self,
        segment_id: u64,
        priority: RepairPriority,
        shards: Vec<RepairShard>,
        ec_data_shards: usize,
        ec_parity_shards: usize,
        now_ms: u64,
    ) -> Result<RepairId, RepairError> {
        let active = {
            let ops = self.ops.lock().unwrap();
            ops.len()
        };
        if active >= self.config.max_concurrent {
            return Err(RepairError::TooManyConcurrent(self.config.max_concurrent));
        }

        let id = RepairId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let op = RepairOp::new(id, segment_id, priority, shards, ec_data_shards, ec_parity_shards, now_ms);

        self.stats.repairs_started.fetch_add(1, Ordering::Relaxed);
        match priority {
            RepairPriority::Foreground => self.stats.foreground_repairs.fetch_add(1, Ordering::Relaxed),
            RepairPriority::Background => self.stats.background_repairs.fetch_add(1, Ordering::Relaxed),
        }

        let mut ops = self.ops.lock().unwrap();
        ops.insert(id, op);

        Ok(id)
    }

    /// Record a fetch result for a shard. Returns new op state, or None if not found.
    pub fn record_fetch(&self, id: RepairId, node_id: &[u8; 16], success: bool) -> Option<RepairOpState> {
        let mut ops = self.ops.lock().unwrap();
        let op = ops.get_mut(&id)?;
        op.record_fetch(node_id, success);
        Some(op.state)
    }

    /// Transition repair to reconstruction phase.
    pub fn begin_reconstruct(&self, id: RepairId) -> Result<RepairOpState, RepairError> {
        let mut ops = self.ops.lock().unwrap();
        let op = ops.get_mut(&id).ok_or(RepairError::NotFound(id))?;
        
        if op.state == RepairOpState::Complete || op.state == RepairOpState::Failed || op.state == RepairOpState::TimedOut {
            return Err(RepairError::AlreadyCompleted(id));
        }
        
        op.begin_reconstruct()?;
        Ok(op.state)
    }

    /// Complete a repair.
    pub fn complete_repair(&self, id: RepairId) -> Result<(), RepairError> {
        let mut ops = self.ops.lock().unwrap();
        let op = ops.get_mut(&id).ok_or(RepairError::NotFound(id))?;
        
        let shard_count = op.shards.len();
        op.complete();
        
        self.stats.repairs_completed.fetch_add(1, Ordering::Relaxed);
        self.stats.shards_repaired.fetch_add(shard_count as u64, Ordering::Relaxed);
        
        Ok(())
    }

    /// Check timeouts. Returns IDs of timed-out repairs.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<RepairId> {
        let mut ops = self.ops.lock().unwrap();
        let mut timed_out = Vec::new();
        
        for (id, op) in ops.iter_mut() {
            if op.check_timeout(now_ms) {
                self.stats.repairs_timed_out.fetch_add(1, Ordering::Relaxed);
                timed_out.push(*id);
            }
        }
        
        timed_out
    }

    /// Remove a completed/failed repair.
    pub fn remove(&self, id: RepairId) {
        let mut ops = self.ops.lock().unwrap();
        if let Some(op) = ops.remove(&id) {
            if op.state == RepairOpState::Failed {
                self.stats.repairs_failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Number of active repairs.
    pub fn active_count(&self) -> usize {
        let ops = self.ops.lock().unwrap();
        ops.len()
    }

    /// Returns the stats for this manager.
    pub fn stats(&self) -> Arc<ReadRepairStats> {
        Arc::clone(&self.stats)
    }
}

/// Statistics for read repair operations.
pub struct ReadRepairStats {
    pub repairs_started: AtomicU64,
    pub repairs_completed: AtomicU64,
    pub repairs_failed: AtomicU64,
    pub repairs_timed_out: AtomicU64,
    pub shards_repaired: AtomicU64,
    pub foreground_repairs: AtomicU64,
    pub background_repairs: AtomicU64,
}

impl ReadRepairStats {
    /// Creates a new ReadRepairStats with zero counters.
    pub fn new() -> Self {
        Self {
            repairs_started: AtomicU64::new(0),
            repairs_completed: AtomicU64::new(0),
            repairs_failed: AtomicU64::new(0),
            repairs_timed_out: AtomicU64::new(0),
            shards_repaired: AtomicU64::new(0),
            foreground_repairs: AtomicU64::new(0),
            background_repairs: AtomicU64::new(0),
        }
    }

    /// Creates a snapshot of the current stats.
    pub fn snapshot(&self, active_repairs: usize) -> ReadRepairStatsSnapshot {
        ReadRepairStatsSnapshot {
            repairs_started: self.repairs_started.load(Ordering::Relaxed),
            repairs_completed: self.repairs_completed.load(Ordering::Relaxed),
            repairs_failed: self.repairs_failed.load(Ordering::Relaxed),
            repairs_timed_out: self.repairs_timed_out.load(Ordering::Relaxed),
            shards_repaired: self.shards_repaired.load(Ordering::Relaxed),
            foreground_repairs: self.foreground_repairs.load(Ordering::Relaxed),
            background_repairs: self.background_repairs.load(Ordering::Relaxed),
            active_repairs,
        }
    }
}

/// Snapshot of ReadRepairStats.
pub struct ReadRepairStatsSnapshot {
    pub repairs_started: u64,
    pub repairs_completed: u64,
    pub repairs_failed: u64,
    pub repairs_timed_out: u64,
    pub shards_repaired: u64,
    pub foreground_repairs: u64,
    pub background_repairs: u64,
    pub active_repairs: usize,
}
```

### Tests (minimum 16)
- `test_new_repair_op` — create repair, state is Fetching
- `test_record_fetch_success` — record successful fetch, fetched_count increases
- `test_record_fetch_failure` — record failed fetch, shard state is Failed
- `test_can_reconstruct_true` — enough shards fetched → can_reconstruct true
- `test_can_reconstruct_false` — not enough → can_reconstruct false
- `test_begin_reconstruct_success` — enough shards → Reconstructing state
- `test_begin_reconstruct_insufficient` — not enough → InsufficientShards error
- `test_complete_repair` — full lifecycle Fetching → Reconstructing → WritingBack → Complete
- `test_repair_timeout` — check_timeout after timeout_ms → TimedOut
- `test_repair_timeout_not_expired` — before timeout → no change
- `test_manager_start_too_many` — exceed max_concurrent → TooManyConcurrent error
- `test_manager_check_timeouts` — expired repair in timeout list
- `test_manager_active_count` — count reflects active repairs
- `test_priority_ordering` — Foreground > Background
- `test_stats_counts` — start/complete/fail/timeout, verify stats
- `test_missing_vs_failed_shards` — missing_count vs fetch failure

---

## Module 3: `node_blacklist.rs` — Transient Node Blacklist

### Purpose
Manages a transient blacklist of nodes that have recently failed or been marked as unreachable. Used by the routing layer to avoid sending requests to known-bad nodes for a configurable backoff period. Entries expire automatically.

### Types to implement

```rust
/// Reason a node was blacklisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlacklistReason {
    /// Connection refused or timeout.
    ConnectionFailed,
    /// Node returned an error response.
    ErrorResponse(String),
    /// Node was slow — exceeded latency threshold.
    LatencyThreshold,
    /// Explicit administrative action.
    Manual,
}

/// A blacklist entry for one node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
    pub node_id: [u8; 16],
    pub reason: BlacklistReason,
    /// When this entry was created (ms since epoch).
    pub added_at_ms: u64,
    /// When this entry expires (ms since epoch).
    pub expires_at_ms: u64,
    /// Number of times this node has been blacklisted.
    pub failure_count: u32,
}

impl BlacklistEntry {
    /// Returns true if this entry has expired.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

/// Configuration for the blacklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistConfig {
    /// Base backoff duration in milliseconds (default: 5000 = 5 seconds).
    pub base_backoff_ms: u64,
    /// Maximum backoff duration in milliseconds (default: 300000 = 5 minutes).
    pub max_backoff_ms: u64,
    /// Whether to use exponential backoff (double each failure, default: true).
    pub exponential: bool,
    /// Maximum number of blacklisted nodes (default: 128).
    pub max_entries: usize,
}

impl Default for BlacklistConfig {
    fn default() -> Self {
        Self {
            base_backoff_ms: 5000,
            max_backoff_ms: 300000,
            exponential: true,
            max_entries: 128,
        }
    }
}

/// Manages the transient node blacklist.
pub struct NodeBlacklist {
    config: BlacklistConfig,
    entries: RwLock<HashMap<[u8; 16], BlacklistEntry>>,
    stats: Arc<BlacklistStats>,
}

impl NodeBlacklist {
    /// Creates a new NodeBlacklist with the given configuration.
    pub fn new(config: BlacklistConfig) -> Self {
        Self {
            config,
            entries: RwLock::new(HashMap::new()),
            stats: Arc::new(BlacklistStats::new()),
        }
    }

    /// Add or update a node in the blacklist.
    /// If already blacklisted: increments failure_count and extends backoff.
    pub fn blacklist(&self, node_id: [u8; 16], reason: BlacklistReason, now_ms: u64) {
        let mut entries = self.entries.write().unwrap();
        
        let (failure_count, added_at_ms) = if let Some(existing) = entries.get(&node_id) {
            (existing.failure_count + 1, existing.added_at_ms)
        } else {
            (1, now_ms)
        };

        let backoff_ms = if self.config.exponential {
            let exponential = self.config.base_backoff_ms * (2u64.saturating_pow(failure_count - 1));
            exponential.min(self.config.max_backoff_ms)
        } else {
            self.config.base_backoff_ms
        };

        let expires_at_ms = now_ms.saturating_add(backoff_ms);

        let entry = BlacklistEntry {
            node_id,
            reason,
            added_at_ms,
            expires_at_ms,
            failure_count,
        };

        if entries.len() >= self.config.max_entries && !entries.contains_key(&node_id) {
            return;
        }

        entries.insert(node_id, entry);
        self.stats.nodes_blacklisted.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove a node from the blacklist explicitly.
    pub fn remove(&self, node_id: &[u8; 16]) {
        let mut entries = self.entries.write().unwrap();
        if entries.remove(node_id).is_some() {
            self.stats.nodes_removed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Check if a node is currently blacklisted (and not expired).
    pub fn is_blacklisted(&self, node_id: &[u8; 16], now_ms: u64) -> bool {
        self.stats.blacklist_checks.fetch_add(1, Ordering::Relaxed);
        
        let entries = self.entries.read().unwrap();
        if let Some(entry) = entries.get(node_id) {
            if !entry.is_expired(now_ms) {
                self.stats.checks_hit.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Expire all entries that have passed their expiry time. Returns count removed.
    pub fn expire(&self, now_ms: u64) -> usize {
        let mut entries = self.entries.write().unwrap();
        let before = entries.len();
        entries.retain(|_, entry| !entry.is_expired(now_ms));
        let removed = before - entries.len();
        if removed > 0 {
            self.stats.nodes_expired.fetch_add(removed as u64, Ordering::Relaxed);
        }
        removed
    }

    /// Get the blacklist entry for a node (None if not blacklisted or expired).
    pub fn entry(&self, node_id: &[u8; 16], now_ms: u64) -> Option<BlacklistEntry> {
        let entries = self.entries.read().unwrap();
        entries.get(node_id).filter(|e| !e.is_expired(now_ms)).cloned()
    }

    /// List all currently active (non-expired) blacklisted nodes.
    pub fn active_entries(&self, now_ms: u64) -> Vec<BlacklistEntry> {
        let entries = self.entries.read().unwrap();
        entries.values()
            .filter(|e| !e.is_expired(now_ms))
            .cloned()
            .collect()
    }

    /// Filter a list of node_ids, returning only those NOT blacklisted.
    pub fn filter_available<'a>(&self, nodes: &'a [[u8; 16]], now_ms: u64) -> Vec<&'a [u8; 16]> {
        nodes.iter()
            .filter(|id| !self.is_blacklisted(id, now_ms))
            .collect()
    }

    /// Number of active (non-expired) entries.
    pub fn active_count(&self, now_ms: u64) -> usize {
        let entries = self.entries.read().unwrap();
        entries.values().filter(|e| !e.is_expired(now_ms)).count()
    }

    /// Returns the stats for this blacklist.
    pub fn stats(&self) -> Arc<BlacklistStats> {
        Arc::clone(&self.stats)
    }
}

/// Statistics for the blacklist.
pub struct BlacklistStats {
    pub nodes_blacklisted: AtomicU64,
    pub nodes_removed: AtomicU64,
    pub nodes_expired: AtomicU64,
    pub blacklist_checks: AtomicU64,
    pub checks_hit: AtomicU64,
}

impl BlacklistStats {
    /// Creates a new BlacklistStats with zero counters.
    pub fn new() -> Self {
        Self {
            nodes_blacklisted: AtomicU64::new(0),
            nodes_removed: AtomicU64::new(0),
            nodes_expired: AtomicU64::new(0),
            blacklist_checks: AtomicU64::new(0),
            checks_hit: AtomicU64::new(0),
        }
    }

    /// Creates a snapshot of the current stats.
    pub fn snapshot(&self, active_count: usize) -> BlacklistStatsSnapshot {
        BlacklistStatsSnapshot {
            nodes_blacklisted: self.nodes_blacklisted.load(Ordering::Relaxed),
            nodes_removed: self.nodes_removed.load(Ordering::Relaxed),
            nodes_expired: self.nodes_expired.load(Ordering::Relaxed),
            blacklist_checks: self.blacklist_checks.load(Ordering::Relaxed),
            checks_hit: self.checks_hit.load(Ordering::Relaxed),
            active_count,
        }
    }
}

/// Snapshot of BlacklistStats.
pub struct BlacklistStatsSnapshot {
    pub nodes_blacklisted: u64,
    pub nodes_removed: u64,
    pub nodes_expired: u64,
    pub blacklist_checks: u64,
    pub checks_hit: u64,
    pub active_count: usize,
}
```

### Tests (minimum 15)
- `test_blacklist_node` — blacklist a node, is_blacklisted returns true
- `test_not_blacklisted` — unknown node → is_blacklisted false
- `test_blacklist_expired` — add entry, check after expiry → false
- `test_blacklist_not_expired` — check before expiry → true
- `test_blacklist_increments_failure_count` — blacklist same node twice → failure_count=2
- `test_exponential_backoff` — failure_count 1 vs 2: second has longer backoff
- `test_max_backoff` — many failures don't exceed max_backoff_ms
- `test_remove_explicit` — blacklist then remove, is_blacklisted false
- `test_expire_removes_old` — add expired entry, expire() removes it
- `test_expire_keeps_fresh` — add fresh entry, expire() keeps it
- `test_filter_available` — mixed blacklisted/available nodes, filter returns only available
- `test_filter_all_blacklisted` — all nodes blacklisted → empty vec
- `test_active_entries` — active_entries() returns only non-expired entries
- `test_active_count` — count reflects non-expired entries
- `test_stats_counts` — blacklist/remove/expire/check, verify stats

---

## Important Notes

1. Write complete, compilable Rust code — no TODOs
2. JournalSeq starts at 0, is_before(a, b) = a < b
3. ReadRepairOp: can_reconstruct = fetched_count >= ec_data_shards
4. NodeBlacklist: exponential backoff = base_backoff_ms * 2^(failure_count-1), capped at max_backoff_ms
5. All tests must be synchronous (no tokio, no async/await)
6. Use `#[cfg(test)] mod tests` at the bottom of each file
7. Output the three Rust files ready to be placed in `/home/cfs/claudefs/crates/claudefs-transport/src/`
