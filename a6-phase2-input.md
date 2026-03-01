# A6: ClaudeFS Replication — Phase 2: Conduit and Sync

You are implementing Phase 2 of the `claudefs-repl` crate for ClaudeFS, a distributed scale-out POSIX filesystem in Rust.

## Phase 1 Already Implemented

The following modules are already complete and passing tests:
- `src/error.rs` — ReplError enum (Journal, WalCorrupted, SiteUnknown, ConflictDetected, NetworkError, Serialization, Io, VersionMismatch, Shutdown)
- `src/journal.rs` — JournalEntry (seq, shard_id, site_id, timestamp_us, inode, op, payload, crc32), OpKind enum, JournalTailer
- `src/wal.rs` — ReplicationCursor, WalRecord, ReplicationWal
- `src/topology.rs` — SiteId, NodeId, ReplicationRole, SiteInfo, ReplicationTopology

## Architecture Context

- ClaudeFS cross-site replication: asynchronous journal entries flow from primary site to replica site(s)
- Transport: gRPC over mTLS (tonic crate) — the "cloud conduit"
- Conflict resolution: Last-Write-Wins (LWW) based on timestamp_us field
- Admin alerting on conflicts (log the conflict, record it for reporting)
- Batch compaction: coalesce multiple journal entries per inode into fewer before sending

## Shared Conventions

- **Error handling**: `thiserror` for library errors
- **Serialization**: `serde` + `bincode` for internal, `prost` for gRPC
- **Async**: Tokio async/await. Use `tokio::sync::{mpsc, Mutex, RwLock}` as needed.
- **Logging**: `tracing` crate with structured spans
- **Testing**: inline `#[cfg(test)]` modules, `#[tokio::test]` for async tests
- **No unsafe code** in this crate
- All public items must have `///` doc comments

## Current Cargo.toml (already correct, DO NOT change it)

```toml
[package]
name = "claudefs-repl"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "ClaudeFS subsystem: Cross-site journal replication, cloud conduit (gRPC/mTLS)"

[[bin]]
name = "cfs-repl"
path = "src/main.rs"

[dependencies]
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
prost.workspace = true
tonic.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
bincode.workspace = true
rand.workspace = true
bytes.workspace = true

[lib]
name = "claudefs_repl"
path = "src/lib.rs"
```

## Task: Implement Phase 2

### Files to populate

#### `src/conduit.rs`

The conduit is the gRPC/mTLS relay that streams journal entries between sites.

Since we cannot define `.proto` files in this context (they require a build.rs), implement a **pure-Rust mock conduit** that simulates the gRPC channel with in-process channels (tokio mpsc). This represents the interface that will be wired to real gRPC in Phase 3 when build.rs is set up.

The conduit must handle:
1. Sending batches of journal entries to a remote site
2. Receiving batches of journal entries from a remote site
3. Reconnection with exponential backoff
4. Flow control (don't overwhelm the remote)
5. mTLS configuration (represented as a config struct even if not enforced in tests)

```rust
/// mTLS configuration for the conduit channel.
pub struct ConduitTlsConfig {
    /// PEM-encoded certificate chain for this node.
    pub cert_pem: Vec<u8>,
    /// PEM-encoded private key.
    pub key_pem: Vec<u8>,
    /// PEM-encoded CA certificate chain for verifying peers.
    pub ca_pem: Vec<u8>,
}

/// Configuration for a conduit connection to one remote site.
pub struct ConduitConfig {
    /// Local site ID.
    pub local_site_id: u64,
    /// Remote site ID.
    pub remote_site_id: u64,
    /// Remote conduit endpoints (try in order, round-robin on failure).
    pub remote_addrs: Vec<String>,
    /// mTLS config (None for plaintext, used in tests).
    pub tls: Option<ConduitTlsConfig>,
    /// Maximum batch size (number of entries per send).
    pub max_batch_size: usize,
    /// Initial reconnect delay (ms).
    pub reconnect_delay_ms: u64,
    /// Max reconnect delay (ms) after backoff.
    pub max_reconnect_delay_ms: u64,
}

impl Default for ConduitConfig { ... }  // sensible defaults

/// A batch of journal entries sent over the conduit.
pub struct EntryBatch {
    /// Sending site's ID.
    pub source_site_id: u64,
    /// Sequence of entries in this batch (must be from a single shard, ordered by seq).
    pub entries: Vec<JournalEntry>,
    /// Batch sequence number (monotonically increasing per conduit connection).
    pub batch_seq: u64,
}

/// Statistics for one conduit connection.
pub struct ConduitStats {
    pub batches_sent: u64,
    pub batches_received: u64,
    pub entries_sent: u64,
    pub entries_received: u64,
    pub send_errors: u64,
    pub reconnects: u64,
}

/// State of the conduit connection.
pub enum ConduitState {
    Connected,
    Reconnecting { attempt: u32, delay_ms: u64 },
    Shutdown,
}

/// A conduit connection to one remote site.
/// In production, this wraps a tonic gRPC channel.
/// In tests, it uses tokio mpsc channels for in-process simulation.
pub struct Conduit {
    config: ConduitConfig,
    state: Arc<tokio::sync::Mutex<ConduitState>>,
    stats: Arc<ConduitStats>,  // use atomic fields inside ConduitStats
    sender: tokio::sync::mpsc::Sender<EntryBatch>,
    receiver: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<EntryBatch>>>,
}

impl Conduit {
    /// Create a paired (sender, receiver) conduit for in-process testing.
    /// Returns (conduit_a_to_b, conduit_b_to_a).
    pub fn new_pair(config_a: ConduitConfig, config_b: ConduitConfig) -> (Self, Self)

    /// Send a batch of entries. Returns error if the conduit is shut down.
    pub async fn send_batch(&self, batch: EntryBatch) -> Result<(), ReplError>

    /// Receive the next batch. Returns None if the conduit is shut down.
    pub async fn recv_batch(&self) -> Option<EntryBatch>

    /// Get current connection state.
    pub async fn state(&self) -> ConduitState

    /// Mark conduit as shutting down (drains in-flight sends).
    pub async fn shutdown(&self)

    /// Get a snapshot of current statistics.
    pub fn stats(&self) -> ConduitStats
}
```

Note: `ConduitStats` fields should use `std::sync::atomic::AtomicU64` (not wrapped in Mutex) for lock-free reads.

Include at least **20 tests** for:
- Creating a pair and sending/receiving a batch
- Stats increment on send/receive
- Batch sequence numbers
- Multiple batches
- Shutdown behavior (send after shutdown fails, recv returns None)
- Empty batch handling
- Large batch handling
- EntryBatch struct creation and fields
- ConduitConfig defaults
- ConduitTlsConfig creation
- Concurrent sends from multiple tasks
- Receive returns None after shutdown

#### `src/sync.rs`

The sync module coordinates journal replication, including:
1. **ConflictDetector** — LWW conflict resolution with admin alerting
2. **BatchCompactor** — coalesces journal entries per inode before sending
3. **ReplicationSync** — high-level coordinator that drives the replication loop

```rust
/// A detected write conflict (same inode, different timestamps on two sites).
pub struct Conflict {
    pub inode: u64,
    pub local_site_id: u64,
    pub remote_site_id: u64,
    pub local_ts: u64,
    pub remote_ts: u64,
    /// The winner (site_id of the entry with the higher timestamp_us).
    pub winner_site_id: u64,
    /// When the conflict was detected (system time microseconds).
    pub detected_at_us: u64,
}

/// Detects and records LWW conflicts between local and remote journal entries.
pub struct ConflictDetector {
    local_site_id: u64,
    conflicts: Arc<tokio::sync::Mutex<Vec<Conflict>>>,
}

impl ConflictDetector {
    /// Create a new conflict detector for the given local site.
    pub fn new(local_site_id: u64) -> Self

    /// Compare a local entry with an incoming remote entry for the same inode.
    /// If they have different timestamps, this is a conflict (resolved by LWW).
    /// Returns Some(Conflict) if a conflict was detected, None if no conflict.
    /// A conflict is: same inode, same shard, remote_site_id != local_site_id,
    /// AND both operations modify the same resource AND the remote entry does NOT
    /// extend the local sequence (i.e., they're concurrent updates, not sequential).
    pub async fn check(&self, local: &JournalEntry, remote: &JournalEntry) -> Option<Conflict>

    /// Return all recorded conflicts.
    pub async fn conflicts(&self) -> Vec<Conflict>

    /// Clear the conflict log (e.g., after admin has acknowledged them).
    pub async fn clear_conflicts(&self)

    /// Return the count of recorded conflicts.
    pub async fn conflict_count(&self) -> usize

    /// Check if two entries conflict (same inode, same shard, different sites).
    pub fn entries_conflict(local: &JournalEntry, remote: &JournalEntry) -> bool
}

/// Compaction result for a group of entries.
pub struct CompactionResult {
    /// Entries that should be sent (after deduplication/compaction).
    pub entries: Vec<JournalEntry>,
    /// Number of entries removed by compaction.
    pub removed_count: usize,
}

/// Compacts (deduplicates) journal entries before sending over the conduit.
///
/// Rules:
/// - For the same inode + op (e.g., multiple Writes), keep only the latest by timestamp_us.
/// - For SetAttr ops on the same inode, keep only the latest by timestamp_us.
/// - Create/Unlink/MkDir/Symlink/Link are always kept (structural ops).
/// - Rename is always kept.
/// - Within a shard, output entries are sorted by seq.
pub struct BatchCompactor;

impl BatchCompactor {
    /// Compact a batch of journal entries.
    /// Removes redundant entries (e.g., superseded Writes to the same inode).
    pub fn compact(entries: Vec<JournalEntry>) -> CompactionResult

    /// Compact for a specific inode only (used in targeted tests).
    pub fn compact_inode(entries: Vec<JournalEntry>, inode: u64) -> CompactionResult
}

/// High-level replication synchronization state machine.
/// Drives the replication loop for one remote site.
pub struct ReplicationSync {
    local_site_id: u64,
    remote_site_id: u64,
    wal: Arc<tokio::sync::Mutex<ReplicationWal>>,
    detector: Arc<ConflictDetector>,
}

/// The outcome of applying a batch of remote entries.
pub enum ApplyResult {
    /// All entries applied cleanly.
    Applied { count: usize },
    /// Some entries were applied, some had conflicts (resolved by LWW).
    AppliedWithConflicts { applied: usize, conflicts: usize },
    /// Batch was rejected (e.g., wrong site, bad sequence number).
    Rejected { reason: String },
}

impl ReplicationSync {
    /// Create a new replication sync for one remote site.
    pub fn new(local_site_id: u64, remote_site_id: u64) -> Self

    /// Apply a received batch from the remote site.
    /// Compares remote entries against what we expect, detects conflicts,
    /// and advances the WAL cursor.
    /// Returns an ApplyResult describing what happened.
    pub async fn apply_batch(&self, batch: &EntryBatch, local_entries: &[JournalEntry]) -> ApplyResult

    /// Get the current replication lag (in entries) for a shard.
    /// This is the difference between the local journal tip and the remote cursor.
    pub async fn lag(&self, shard_id: u32, local_tip: u64) -> u64

    /// Get the WAL (for inspection/persistence).
    pub async fn wal_snapshot(&self) -> Vec<ReplicationCursor>

    /// Get the conflict detector (for admin reporting).
    pub fn detector(&self) -> Arc<ConflictDetector>
}
```

Include at least **25 tests** for:
- ConflictDetector: detect conflict on same inode, no conflict when different inodes, LWW winner selection (higher timestamp wins), clear conflicts, conflict count, entries_conflict predicate
- BatchCompactor: remove duplicate writes to same inode, keep latest SetAttr, keep all structural ops, keep all Renames, output sorted by seq, empty input, single entry, no compaction needed (all unique)
- ReplicationSync: apply clean batch, apply batch with conflicts, reject batch from wrong site, lag calculation, WAL snapshot
- ApplyResult variants

### Implementation Requirements

1. **No external crate additions** — use only what's already in Cargo.toml.

2. **ConduitStats** must use `Arc<ConduitStats>` where stats fields are `AtomicU64`. The `stats()` method returns a snapshot `ConduitStats` struct (not Arc). To avoid confusion, you can use two types: `ConduitStatsInner` (with AtomicU64 fields) stored in Arc, and `ConduitStats` (plain u64 snapshot) returned by the `stats()` method.

3. **Import journal types correctly**: `use crate::journal::{JournalEntry, OpKind};`
   Import WAL types: `use crate::wal::{ReplicationCursor, ReplicationWal};`
   Import conduit types in sync.rs: `use crate::conduit::{Conduit, EntryBatch};`
   Import error: `use crate::error::ReplError;`

4. **Async tests**: Use `#[tokio::test]` for async tests.

5. **Tests must be in `#[cfg(test)]` modules** inside each file.

6. **All tests must pass** `cargo test -p claudefs-repl`.

7. **No clippy warnings**: Code should pass `cargo clippy -p claudefs-repl -- -D warnings`.

8. **Update `src/lib.rs`**: The current lib.rs already has `pub mod conduit;` and `pub mod sync;`. These stubs just contain a comment. You need to REPLACE their content entirely. The lib.rs itself does not need changing.

### Output Format

For each file, output the COMPLETE file content in a code block labeled with the filename:

```rust
// File: crates/claudefs-repl/src/conduit.rs
<full file content>
```

```rust
// File: crates/claudefs-repl/src/sync.rs
<full file content>
```

Output ONLY `conduit.rs` and `sync.rs`. Do NOT change any other files.
