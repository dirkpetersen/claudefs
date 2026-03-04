# A2: Implement mtime_tracker.rs — Directory Mtime/Ctime Propagation

## Context

Implement mtime/ctime update tracking for the `claudefs-meta` crate.

POSIX requires that when a file in a directory is created, deleted, or renamed,
the parent directory's `mtime` and `ctime` must be updated. This module tracks
which directories need mtime updates and applies them efficiently.

The crate uses:
- `thiserror` for errors
- `serde` + `bincode` for serialization
- `tracing` for logging

## Existing types

From `types.rs`:
```rust
pub struct InodeId(u64); // InodeId::new(u64), as_u64()
pub struct Timestamp(u64); // Timestamp::now() returns current time as nanos since epoch
impl Timestamp {
    pub fn now() -> Self;
    pub fn as_nanos(&self) -> u64;
    pub fn new(nanos: u64) -> Self; // if this constructor exists — check the source
}
pub enum MetaError { NotFound(String), KvError(String), /* ... */ }
```

From `kvstore.rs`:
```rust
pub trait KvStore: Send + Sync {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, MetaError>;
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), MetaError>;
    fn delete(&self, key: &[u8]) -> Result<(), MetaError>;
    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), MetaError>;
}
pub struct MemoryKvStore; // implements KvStore
pub enum BatchOp { Put { key: Vec<u8>, value: Vec<u8> }, Delete { key: Vec<u8> } }
```

## Task: Implement `crates/claudefs-meta/src/mtime_tracker.rs`

```rust
/// A pending mtime/ctime update for a directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MtimeUpdate {
    /// The directory inode ID that needs updating.
    pub dir_ino: InodeId,
    /// The new mtime value.
    pub mtime: Timestamp,
    /// The new ctime value (usually same as mtime for directory updates).
    pub ctime: Timestamp,
    /// The reason for this update (for debugging).
    pub reason: MtimeReason,
}

/// The reason for an mtime update.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MtimeReason {
    /// A file was created in this directory.
    FileCreated,
    /// A file was deleted from this directory.
    FileDeleted,
    /// A file was renamed into or out of this directory.
    FileRenamed,
    /// A file's attributes were changed (touches ctime only).
    AttrChanged,
    /// A file's data was written (touches file mtime, not dir).
    DataWritten,
}

/// Batch collector for mtime updates.
///
/// Deduplicates multiple updates to the same directory within one operation batch,
/// keeping only the most recent mtime.
pub struct MtimeBatch {
    updates: Vec<MtimeUpdate>,
}

impl MtimeBatch {
    /// Creates a new empty batch.
    pub fn new() -> Self;

    /// Adds an update to the batch.
    ///
    /// If an update for the same `dir_ino` already exists in the batch,
    /// replaces it with the new update (deduplication).
    pub fn add(&mut self, update: MtimeUpdate);

    /// Returns the number of unique directories in this batch.
    pub fn len(&self) -> usize;

    /// Returns true if the batch is empty.
    pub fn is_empty(&self) -> bool;

    /// Iterates over the batch updates in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &MtimeUpdate>;

    /// Consumes the batch and returns all updates.
    pub fn into_vec(self) -> Vec<MtimeUpdate>;
}

impl Default for MtimeBatch { ... }

/// Persistent store for directory mtime/ctime timestamps.
pub struct MtimeStore {
    kv: Arc<dyn KvStore>,
}

impl MtimeStore {
    /// Creates a new MtimeStore backed by the given KV store.
    pub fn new(kv: Arc<dyn KvStore>) -> Self;

    /// Gets the current mtime for a directory.
    ///
    /// Returns `None` if no mtime has been recorded (use InodeAttr.mtime instead).
    pub fn get_mtime(&self, dir_ino: InodeId) -> Result<Option<Timestamp>, MetaError>;

    /// Sets the mtime for a directory.
    pub fn set_mtime(&self, dir_ino: InodeId, mtime: Timestamp) -> Result<(), MetaError>;

    /// Applies a batch of mtime updates atomically using write_batch.
    ///
    /// Only updates directories where the new mtime is more recent than the stored mtime.
    pub fn apply_batch(&self, batch: MtimeBatch) -> Result<usize, MetaError>;

    /// Removes the stored mtime for a directory (on rmdir).
    pub fn remove_mtime(&self, dir_ino: InodeId) -> Result<(), MetaError>;

    /// Returns all stored mtime entries.
    pub fn list_all(&self) -> Result<Vec<(InodeId, Timestamp)>, MetaError>;
}
```

## KV Key Format

- `b"mt:" + dir_ino.as_u64().to_be_bytes()`
- Value: `mtime_timestamp.as_nanos().to_be_bytes()` (8 bytes)

## Required Tests (14 tests)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn make_store() -> MtimeStore {
        MtimeStore::new(Arc::new(MemoryKvStore::new()))
    }

    fn ts(nanos: u64) -> Timestamp {
        Timestamp::from_nanos(nanos) // or however Timestamp is constructed
    }
}
```

Before writing tests, read `crates/claudefs-meta/src/types.rs` to see how Timestamp is constructed.

Tests:
1. `test_set_and_get_mtime` — set_mtime then get_mtime returns correct value
2. `test_get_mtime_missing` — get_mtime on unknown inode returns None
3. `test_mtime_batch_dedup` — add two updates for same dir, only latest kept
4. `test_mtime_batch_empty` — empty batch, len=0, is_empty=true
5. `test_mtime_batch_add_different_dirs` — add updates for 3 different dirs, len=3
6. `test_apply_batch_updates_newer` — apply_batch only updates if new mtime > current
7. `test_apply_batch_skips_older` — apply_batch skips if stored mtime is newer
8. `test_apply_batch_empty` — apply_batch with empty batch returns 0 updates
9. `test_apply_batch_count` — apply_batch returns count of dirs actually updated
10. `test_remove_mtime` — remove_mtime then get_mtime returns None
11. `test_remove_nonexistent` — remove_mtime on unknown inode is Ok
12. `test_list_all` — list_all returns all stored entries
13. `test_mtime_reason_serde` — MtimeReason variants round-trip through bincode
14. `test_mtime_update_serde` — MtimeUpdate round-trips through bincode

## Important

- Read `crates/claudefs-meta/src/types.rs` FIRST to understand how Timestamp is constructed
- Write the file directly to `crates/claudefs-meta/src/mtime_tracker.rs`
- Do NOT modify lib.rs
- No unused imports, no clippy warnings
- All tests must pass
