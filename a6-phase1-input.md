# A6: ClaudeFS Replication — Phase 1 Foundation

You are implementing the `claudefs-repl` crate for ClaudeFS, a distributed scale-out POSIX filesystem in Rust.

## Project Context

ClaudeFS is a multi-site distributed filesystem with cross-site journal replication. The replication subsystem (A6) implements:
- Cross-site asynchronous journal replication
- Cloud conduit (gRPC/mTLS) for WAN transport
- LWW (last-write-wins) conflict detection with admin alerting
- UID mapping between sites
- Batch compaction of journal entries

## Architecture Decisions (from docs/decisions.md)

- **D3**: Write journal: 2x synchronous replication within a site before ack. Then asynchronously packed into EC 4+2 segments.
- **Write path**: client write → 2x journal replication (sync, fast ack) → segment packing + EC 4+2 (async) → journal space reclaimed → async cross-site replication via conduit.
- **Cross-site**: Two metadata servers syncing asynchronously. Eventually-consistent with last-write-wins conflict resolution. Admin alerting on write conflicts.
- **D2**: SWIM-based membership. Sites identified by site IDs.
- **D4**: Multi-Raft, 256 virtual shards per site. Each shard has its own Raft journal.

## Shared Conventions

- **Error handling**: `thiserror` for library errors, `anyhow` at binary entry points
- **Serialization**: `serde` + `bincode` for internal wire format, `prost` for gRPC Protobuf
- **Async**: Tokio async/await throughout
- **Logging**: `tracing` crate with structured spans
- **Testing**: inline `#[cfg(test)]` modules, property-based tests with `proptest` where useful
- **No unsafe code** in this crate (unsafe is only in A1/A4/A5/A7)
- All public items must have `///` doc comments

## Current Crate Structure

The crate already has these files (currently near-empty stubs):
- `src/lib.rs` — re-exports modules
- `src/journal.rs` — stub
- `src/wal.rs` — stub
- `src/topology.rs` — stub
- `src/conduit.rs` — stub
- `src/sync.rs` — stub
- `src/main.rs` — stub server entry point

Current `Cargo.toml` dependencies:
```toml
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
prost.workspace = true
tonic.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

Workspace also has: `bincode = "1.3"`, `rand = "0.8"`, `bytes = "1"`.

## Task

Implement **Phase 1: Foundation** — the core data types and journal plumbing.

### Files to Create/Populate

#### `src/error.rs`

A new file with `ReplError` using `thiserror`:

```rust
pub enum ReplError {
    Journal { msg: String },         // Journal read/write error
    WalCorrupted { msg: String },     // WAL data is corrupt
    SiteUnknown { site_id: u64 },    // Unknown site ID
    ConflictDetected { inode: u64, local_ts: u64, remote_ts: u64 }, // LWW conflict
    NetworkError { msg: String },    // Conduit transport error
    Serialization(#[from] bincode::Error), // serde/bincode error
    Io(#[from] std::io::Error),      // I/O error
    VersionMismatch { expected: u32, got: u32 }, // Protocol version mismatch
    Shutdown,                        // Replication engine was shut down
}
```

#### `src/journal.rs`

Journal entry types and tailer:

```rust
/// Filesystem operation kind recorded in the journal
pub enum OpKind {
    Create,    // create file/dir/symlink
    Unlink,    // unlink/rmdir
    Rename,    // rename (src, dst)
    Write,     // write data range
    Truncate,  // truncate to length
    SetAttr,   // chmod/chown/utimes
    Link,      // hard link
    Symlink,   // symlink target
    MkDir,     // mkdir (distinct for POSIX semantics)
    SetXattr,  // extended attribute set
    RemoveXattr, // extended attribute remove
}

/// A single journal entry written by the metadata service
pub struct JournalEntry {
    pub seq: u64,            // monotonically increasing sequence number, per-shard
    pub shard_id: u32,       // which virtual shard (0..255)
    pub site_id: u64,        // originating site
    pub timestamp_us: u64,   // microseconds since Unix epoch (for LWW)
    pub inode: u64,          // affected inode
    pub op: OpKind,          // operation type
    pub payload: Vec<u8>,    // serialized operation details (bincode)
    pub crc32: u32,          // CRC32 checksum of (seq, shard_id, site_id, timestamp_us, inode, op_discriminant, payload)
}

/// Position within a journal: shard + sequence number
pub struct JournalPosition {
    pub shard_id: u32,
    pub seq: u64,
}

/// JournalTailer streams entries starting from a given position.
/// In production, this reads from the Raft journal (A2). For now,
/// it uses an in-memory buffer for testing.
pub struct JournalTailer {
    // internal state
}

impl JournalTailer {
    /// Create a new tailer backed by an in-memory entry buffer.
    pub fn new_in_memory(entries: Vec<JournalEntry>) -> Self

    /// Create a new tailer starting from the given position.
    pub fn new_from_position(entries: Vec<JournalEntry>, pos: JournalPosition) -> Self

    /// Return the next entry, or None if the journal is at the tip.
    pub async fn next(&mut self) -> Option<JournalEntry>

    /// Return the current read position.
    pub fn position(&self) -> Option<JournalPosition>

    /// Add entries (used in tests to simulate journal appends).
    pub fn append(&mut self, entry: JournalEntry)
}
```

Include at least **15 tests** for `JournalEntry` serialization/deserialization (bincode round-trip), crc32 validation, tailer ordering, filtering by shard, position seeking, and async iteration.

#### `src/wal.rs`

Replication WAL tracks which journal entries have been successfully replicated to each remote site:

```rust
/// A site+sequence position representing how far replication has advanced.
pub struct ReplicationCursor {
    pub site_id: u64,      // remote site we are replicating TO
    pub shard_id: u32,
    pub last_seq: u64,     // last sequence number successfully replicated to remote
}

/// A single WAL record written when we advance the cursor.
pub struct WalRecord {
    pub cursor: ReplicationCursor,
    pub replicated_at_us: u64,  // timestamp when replication was confirmed
    pub entry_count: u32,       // how many entries this advance covers
}

/// The replication WAL is an in-memory (later: persisted) log of replication
/// progress. After restart, we resume from the last confirmed cursor.
pub struct ReplicationWal {
    // internal state: per (site_id, shard_id) -> last_seq
}

impl ReplicationWal {
    pub fn new() -> Self

    /// Record that entries up to `seq` have been replicated to `site_id/shard`.
    pub fn advance(&mut self, site_id: u64, shard_id: u32, seq: u64, replicated_at_us: u64, entry_count: u32)

    /// Get the current cursor for a (site_id, shard_id) pair. Returns seq=0 if unknown.
    pub fn cursor(&self, site_id: u64, shard_id: u32) -> ReplicationCursor

    /// Get all cursors (snapshot of current state).
    pub fn all_cursors(&self) -> Vec<ReplicationCursor>

    /// Reset the cursor for a site (used when a site is removed or reset).
    pub fn reset(&mut self, site_id: u64, shard_id: u32)

    /// Returns the WAL history as a slice of records (most recent last).
    pub fn history(&self) -> &[WalRecord]

    /// Compact history older than `before_us` (keep at least the latest per cursor).
    pub fn compact(&mut self, before_us: u64)
}
```

Include at least **18 tests** for: advance and read-back, multiple sites, multiple shards, cursor initialization, history ordering, reset behavior, compaction.

#### `src/topology.rs`

Site and peer topology management:

```rust
/// Unique identifier for a replication site (e.g., "us-west-2").
pub type SiteId = u64;

/// Unique identifier for a storage node within a site.
pub type NodeId = u64;

/// Replication role of this node.
pub enum ReplicationRole {
    /// Primary site — originates writes, pushes journal to replicas.
    Primary,
    /// Replica site — receives journal from primary, applies locally.
    Replica { primary_site_id: SiteId },
    /// Bidirectional — both sites can write; uses LWW conflict resolution.
    Bidirectional,
}

/// Information about a remote replication site.
pub struct SiteInfo {
    pub site_id: SiteId,
    pub name: String,          // human-readable (e.g., "us-west-2")
    pub conduit_addrs: Vec<String>,  // gRPC endpoints for the conduit server
    pub role: ReplicationRole,
    pub active: bool,          // true = replication is enabled
    pub lag_us: Option<u64>,   // latest measured replication lag in microseconds
}

/// Manages the topology of known replication sites and their state.
pub struct ReplicationTopology {
    pub local_site_id: SiteId,
    // internal map: site_id -> SiteInfo
}

impl ReplicationTopology {
    pub fn new(local_site_id: SiteId) -> Self

    /// Add or update a remote site.
    pub fn upsert_site(&mut self, info: SiteInfo)

    /// Remove a remote site.
    pub fn remove_site(&mut self, site_id: SiteId) -> Option<SiteInfo>

    /// Get info for a specific site.
    pub fn get_site(&self, site_id: SiteId) -> Option<&SiteInfo>

    /// List all active remote sites (not the local site).
    pub fn active_sites(&self) -> Vec<&SiteInfo>

    /// Update the measured replication lag for a site.
    pub fn update_lag(&mut self, site_id: SiteId, lag_us: u64)

    /// Mark a site as inactive (e.g., conduit is down).
    pub fn deactivate(&mut self, site_id: SiteId)

    /// Mark a site as active.
    pub fn activate(&mut self, site_id: SiteId)

    /// Return the number of known remote sites.
    pub fn site_count(&self) -> usize
}
```

Include at least **16 tests**: add/remove sites, active filtering, lag update, deactivate/activate, duplicate upsert, bidirectional role.

#### Update `src/lib.rs`

Expose all modules including the new `error` module:

```rust
pub mod conduit;
pub mod error;
pub mod journal;
pub mod sync;
pub mod topology;
pub mod wal;
```

### Implementation Requirements

1. **All public types must derive** `Debug, Clone` at minimum. Use `serde::{Serialize, Deserialize}` on wire types (`JournalEntry`, `WalRecord`, `ReplicationCursor`, `SiteInfo`).

2. **CRC32 implementation**: Use a simple CRC32 computed over the serialized bytes of the entry (excluding the crc32 field itself). A helper function `compute_crc32(data: &[u8]) -> u32` using a basic CRC32 implementation without external deps is fine — the polynomial is 0xEDB88320 (IEEE 802.3).

3. **No external crate additions** beyond what's already in Cargo.toml. The `rand` and `bytes` and `bincode` crates are available from workspace deps but not yet in claudefs-repl's Cargo.toml — you may add `bincode.workspace = true`, `rand.workspace = true`, `bytes.workspace = true` to the `[dependencies]` section of `crates/claudefs-repl/Cargo.toml`.

4. **Async tests**: Use `#[tokio::test]` for async tests.

5. **Tests must be in `#[cfg(test)]` modules** inside each file.

6. **All tests must pass** `cargo test -p claudefs-repl`.

7. **No clippy warnings**: Code should be clean enough to pass `cargo clippy -p claudefs-repl`.

### Output Format

For each file, output the COMPLETE file content in a code block labeled with the filename, like:

```rust
// File: crates/claudefs-repl/src/error.rs
<full file content>
```

Output ALL files: `error.rs`, `journal.rs`, `wal.rs`, `topology.rs`, and the updated `lib.rs` and `Cargo.toml`.

Do NOT output `conduit.rs` or `sync.rs` — those will be implemented in Phase 2.
