# A2: claudefs-meta Phase 2 Extensions

## Context

You are implementing extensions to the `claudefs-meta` crate in the ClaudeFS distributed filesystem project. This is a Cargo workspace at `/home/cfs/claudefs/`.

The crate already has 758 passing tests and comprehensive implementation including:
- Raft consensus (`consensus.rs`) with pre-vote, leader transfer, log replication
- Persistent Raft log (`raft_log.rs`) backed by KvStore
- Snapshot management (`snapshot.rs`) for Raft log compaction
- Multi-Raft manager (`multiraft.rs`) with 256 virtual shards
- Full POSIX metadata ops (`node.rs`, `service.rs`)
- Inode, directory, xattr, lock, lease, quota, WORM, tenant modules
- Journal tailer (`journal_tailer.rs`) for A6 replication
- Conflict detection, CDC events, path resolution, GC, fsck

## Task 1: Add `snapshot_transfer.rs`

Create `/home/cfs/claudefs/crates/claudefs-meta/src/snapshot_transfer.rs`.

This module handles the Raft `InstallSnapshot` RPC — transferring a snapshot from leader to a follower that is too far behind (its log has been compacted away).

### Requirements

```rust
use crate::types::{LogIndex, MetaError, NodeId, Term, Timestamp};
use crate::snapshot::RaftSnapshot;
use serde::{Deserialize, Serialize};
```

#### Core structs:

```rust
/// A chunk of snapshot data (snapshots may be large, so they are chunked).
pub struct SnapshotChunk {
    pub chunk_index: u32,    // 0-based chunk index
    pub total_chunks: u32,   // total number of chunks
    pub data: Vec<u8>,       // raw bytes for this chunk
    pub is_last: bool,       // true for the final chunk
}

/// An InstallSnapshot RPC request (leader → follower).
pub struct InstallSnapshotRequest {
    pub term: Term,
    pub leader_id: NodeId,
    pub last_included_index: LogIndex,
    pub last_included_term: Term,
    pub chunk: SnapshotChunk,
}

/// An InstallSnapshot RPC response (follower → leader).
pub struct InstallSnapshotResponse {
    pub term: Term,           // follower's current term
    pub success: bool,
    pub bytes_received: u64,  // total bytes received so far
}

/// State machine for receiving a snapshot in chunks.
pub struct SnapshotReceiver {
    expected_term: Term,
    expected_leader: NodeId,
    last_included_index: LogIndex,
    last_included_term: Term,
    chunks_received: u32,
    total_chunks: u32,
    buffer: Vec<u8>,
    created_at: Timestamp,
}

impl SnapshotReceiver {
    /// Create a new receiver for the given snapshot metadata.
    pub fn new(request: &InstallSnapshotRequest) -> Self;

    /// Apply a chunk. Returns Ok(None) if more chunks needed, Ok(Some(RaftSnapshot)) when complete.
    pub fn apply_chunk(&mut self, request: &InstallSnapshotRequest) -> Result<Option<RaftSnapshot>, MetaError>;

    /// Returns true if the receiver has timed out (> 30 seconds since creation).
    pub fn is_stale(&self) -> bool;

    /// Returns the number of bytes received so far.
    pub fn bytes_received(&self) -> u64;
}

/// Splits a snapshot into chunks for network transfer.
pub struct SnapshotSender {
    snapshot: RaftSnapshot,
    chunk_size: usize,     // default 1MB per chunk
    bytes_sent: u64,
}

impl SnapshotSender {
    /// Create a new sender for the given snapshot with 1MB chunk size.
    pub fn new(snapshot: RaftSnapshot) -> Self;

    /// Create with a custom chunk size (for testing).
    pub fn with_chunk_size(snapshot: RaftSnapshot, chunk_size: usize) -> Self;

    /// Returns the total number of chunks.
    pub fn total_chunks(&self) -> u32;

    /// Get the chunk at the given index as an InstallSnapshotRequest.
    pub fn chunk_request(&self, term: Term, leader_id: NodeId, chunk_index: u32) -> Result<InstallSnapshotRequest, MetaError>;

    /// Returns the total snapshot size in bytes.
    pub fn snapshot_size(&self) -> usize;
}
```

#### Tests (at least 10):
- `test_snapshot_sender_single_chunk` — small snapshot fits in one chunk
- `test_snapshot_sender_multiple_chunks` — large snapshot splits correctly
- `test_snapshot_sender_chunk_count` — verify total_chunks calculation
- `test_snapshot_receiver_single_chunk` — receive and reassemble single chunk
- `test_snapshot_receiver_multiple_chunks` — receive and reassemble multi-chunk
- `test_snapshot_receiver_wrong_term_rejected` — mismatched term returns error
- `test_snapshot_receiver_wrong_chunk_index` — out-of-order chunk returns error
- `test_snapshot_receiver_stale_detection` — stale receiver is detected
- `test_roundtrip_small_snapshot` — send then receive produces identical data
- `test_roundtrip_large_snapshot` — 5MB snapshot roundtrip with small chunks
- `test_chunk_request_last_chunk_flag` — last chunk has is_last=true

## Task 2: Add `batch_ingest.rs`

Create `/home/cfs/claudefs/crates/claudefs-meta/src/batch_ingest.rs`.

This module provides high-throughput batch file creation for ML training workflows (thousands of file creates per second). Batch ingest groups many CreateInode + CreateEntry operations into a single Raft proposal.

### Requirements

```rust
use crate::types::{DirEntry, FileType, InodeAttr, InodeId, MetaError, MetaOp, Timestamp};
use crate::kvstore::KvStore;
use crate::inode::InodeStore;
use crate::directory::DirectoryStore;
use crate::journal::MetadataJournal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
```

#### Core structs:

```rust
/// A single file to create in a batch.
pub struct BatchFileSpec {
    pub name: String,    // filename (not path)
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub size: u64,       // pre-allocated size (0 for empty)
}

/// Result of creating one file in a batch.
pub struct BatchFileResult {
    pub name: String,
    pub ino: InodeId,
}

/// Configuration for batch ingest behavior.
pub struct BatchIngestConfig {
    pub max_batch_size: usize,     // max files per batch (default 1000)
    pub auto_flush_at: usize,      // flush when this many files queued (default 500)
}

impl Default for BatchIngestConfig { ... }

/// High-throughput batch file creator.
///
/// Queues file creation requests and flushes them as a single Raft proposal
/// via the MetadataJournal for durability.
pub struct BatchIngestor {
    parent_ino: InodeId,
    config: BatchIngestConfig,
    pending: Vec<BatchFileSpec>,
    inode_store: Arc<InodeStore>,
    dir_store: Arc<DirectoryStore>,
    journal: Arc<MetadataJournal>,
    inode_counter_base: u64,  // starting inode for this batch
}

impl BatchIngestor {
    /// Create a new batch ingestor for files under `parent_ino`.
    pub fn new(
        parent_ino: InodeId,
        inode_store: Arc<InodeStore>,
        dir_store: Arc<DirectoryStore>,
        journal: Arc<MetadataJournal>,
        config: BatchIngestConfig,
    ) -> Self;

    /// Queue a file for creation. Automatically flushes if auto_flush_at is reached.
    /// Returns Ok(None) if buffered, Ok(Some(results)) if auto-flushed.
    pub fn add(&mut self, spec: BatchFileSpec) -> Result<Option<Vec<BatchFileResult>>, MetaError>;

    /// Flush all pending files. Creates them and appends to journal.
    /// Returns the created file results.
    pub fn flush(&mut self) -> Result<Vec<BatchFileResult>, MetaError>;

    /// Number of files currently queued.
    pub fn pending_count(&self) -> usize;

    /// Whether the buffer needs flushing.
    pub fn needs_flush(&self) -> bool;
}
```

#### Tests (at least 8):
- `test_batch_ingest_single_file` — create one file via batch
- `test_batch_ingest_many_files` — create 100 files, verify all present
- `test_batch_auto_flush` — auto-flush triggers at threshold
- `test_batch_ingest_names_unique` — duplicate name within batch is an error
- `test_batch_ingest_inode_ids_unique` — all created inodes have different IDs
- `test_batch_ingest_journal_entries` — journal has entries after flush
- `test_batch_empty_flush` — flushing empty batch returns empty vec
- `test_batch_ingest_large_batch` — 1000-file batch succeeds

## Task 3: Expand `proptests.rs` with 10 more proptest cases

Add to the existing `/home/cfs/claudefs/crates/claudefs-meta/src/proptests.rs` (currently 314 lines with 16 proptest functions).

Add the following new proptest functions to the existing `proptest! { }` block at the end of the file:

```rust
// 1. RaftLogStore roundtrip for term persistence
fn prop_raft_log_store_term_roundtrip(term_val in 0u64..=u64::MAX) {
    // Uses MemoryKvStore + RaftLogStore
    // Saves and loads term, asserts equality
}

// 2. RaftLogStore roundtrip for voted_for persistence
fn prop_raft_log_store_voted_for_roundtrip(node_val in 1u64..=100u64) {
    // Saves voted_for and loads it back
}

// 3. Conflict vector clock: same sequence, higher site wins
fn prop_conflict_higher_site_wins_on_tie(site1 in 0u64..100u64, site2 in 0u64..100u64, seq in 0u64..=u64::MAX) {
    // Build two VectorClocks with same seq, different sites
    // Verify ordering matches site ID comparison
}

// 4. NegativeCache: size never exceeds capacity
fn prop_neg_cache_size_bounded(capacity in 1usize..=100usize, num_inserts in 0usize..=200usize) {
    // Insert up to num_inserts entries, verify size <= capacity
}

// 5. LeaseManager: expired leases not returned
fn prop_lease_expired_not_returned(lease_ttl_ms in 1u64..=50u64) {
    // Create lease with short TTL, sleep enough, verify lookup returns None
    // Use std::thread::sleep(Duration::from_millis(lease_ttl_ms + 10))
}

// 6. QuotaManager: usage never goes negative on remove
fn prop_quota_usage_non_negative(add_bytes in 1u64..=1_000_000u64, remove_bytes in 1u64..=1_000_000u64) {
    // Create quota entry, add usage, then remove usage
    // Usage should be max(0, add_bytes - remove_bytes) or similar
}

// 7. Snapshot serialization roundtrip
fn prop_snapshot_data_roundtrip(data_len in 0usize..=4096usize) {
    // Create RaftSnapshot with random data, serialize+deserialize, verify equality
}

// 8. Transaction ID is unique per counter increment
fn prop_transaction_id_unique(n in 1u64..=1000u64) {
    // Create TransactionManager, allocate n transaction IDs, verify all unique
}

// 9. ShardRouter shard_for routes same inode to same shard
fn prop_shard_router_deterministic(ino in 1u64..=u64::MAX, num_shards in 1u16..=256u16) {
    // ShardRouter with num_shards, route ino twice, verify same shard
}

// 10. VectorClock increment is always monotonically increasing
fn prop_vectorclock_increment_monotonic(site in 0u64..=100u64, n in 1usize..=50usize) {
    // Start at seq=0, increment n times, verify monotonically increases
}
```

## Important: imports needed for new proptests

You'll need to add these imports at the top of proptests.rs:
```rust
use crate::raft_log::RaftLogStore;
use crate::conflict::ConflictDetector;
use crate::neg_cache::{NegCacheConfig, NegativeCache};
use crate::lease::{LeaseManager, LeaseType};
use crate::quota::QuotaManager;
use crate::snapshot::RaftSnapshot;
use crate::transaction::TransactionManager;
use crate::shard::ShardRouter;
use std::time::Duration;
```

## Task 4: Wire new modules into `lib.rs`

Add to `/home/cfs/claudefs/crates/claudefs-meta/src/lib.rs`:

```rust
/// High-throughput batch file ingestion
pub mod batch_ingest;

/// Raft snapshot transfer (InstallSnapshot RPC)
pub mod snapshot_transfer;

pub use batch_ingest::{BatchFileResult, BatchFileSpec, BatchIngestConfig, BatchIngestor};
pub use snapshot_transfer::{InstallSnapshotRequest, InstallSnapshotResponse, SnapshotChunk, SnapshotReceiver, SnapshotSender};
```

## Constraints

- Use only dependencies already in Cargo.toml: tokio, thiserror, anyhow, serde, bincode, prost, tonic, tracing, tracing-subscriber, tempfile (dev), proptest (dev)
- No `unsafe` code — this crate is safe Rust only
- All public types must have doc comments
- All tests must pass with `cargo test -p claudefs-meta`
- Zero clippy warnings (run `cargo clippy -p claudefs-meta -- -D warnings`)
- Follow the conventions in the existing code:
  - `thiserror` for errors
  - `serde::{Serialize, Deserialize}` on all persisted types
  - `std::sync::{Arc, RwLock, Mutex}` for thread safety
  - Error type is `MetaError` from `crate::types`
- Do NOT modify existing `.rs` files except `proptests.rs` and `lib.rs`
- Place files at the exact paths specified

## File paths to create/modify:
1. CREATE: `/home/cfs/claudefs/crates/claudefs-meta/src/snapshot_transfer.rs`
2. CREATE: `/home/cfs/claudefs/crates/claudefs-meta/src/batch_ingest.rs`
3. MODIFY: `/home/cfs/claudefs/crates/claudefs-meta/src/proptests.rs` (append 10 new proptest functions)
4. MODIFY: `/home/cfs/claudefs/crates/claudefs-meta/src/lib.rs` (add 2 new module declarations + pub use)

Output all files in full. For modifications, output the complete new file content.
