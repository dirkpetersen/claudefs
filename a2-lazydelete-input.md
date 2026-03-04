# A2: Implement lazy_delete.rs — POSIX Unlink-While-Open Semantics

## Context

Implement deferred inode deletion for the `claudefs-meta` crate.

POSIX requires: when `unlink()` is called on a file that still has open file descriptors,
the directory entry is removed immediately but the data must persist until the last fd
is closed. This module tracks "orphaned" inodes (unlinked but still open) and triggers
GC when the last fd closes.

The crate uses:
- `thiserror` for errors
- `serde` + `bincode` for serialization
- `tracing` for logging

## Existing types

From `types.rs`:
```rust
pub struct InodeId(u64); // InodeId::new(u64), as_u64()
pub struct Timestamp(u64); // Timestamp::now()
pub enum MetaError { NotFound(String), KvError(String), /* ... */ }
```

From `kvstore.rs`:
```rust
pub trait KvStore: Send + Sync {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, MetaError>;
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), MetaError>;
    fn delete(&self, key: &[u8]) -> Result<(), MetaError>;
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, MetaError>;
    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), MetaError>;
}
pub struct MemoryKvStore;
pub enum BatchOp { Put { key: Vec<u8>, value: Vec<u8> }, Delete { key: Vec<u8> } }
```

## Task: Implement `crates/claudefs-meta/src/lazy_delete.rs`

```rust
/// Entry for a lazily-deleted inode.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LazyDeleteEntry {
    /// The inode that was unlinked.
    pub ino: InodeId,
    /// When the inode was unlinked.
    pub unlinked_at: Timestamp,
    /// Original path (for debugging and logging).
    pub original_path: String,
    /// Number of open file descriptors still holding this inode alive.
    /// When this reaches 0, the inode can be garbage collected.
    pub open_fd_count: u32,
}

/// Store for tracking lazily-deleted inodes.
///
/// An inode enters lazy-delete state when:
/// 1. Its nlink count drops to 0 (all directory entries removed)
/// 2. It still has open file descriptors
///
/// It exits lazy-delete state when:
/// - All file descriptors are closed (open_fd_count == 0) → ready for GC
/// - Or when server restart purges all entries with open_fd_count == 0
pub struct LazyDeleteStore {
    kv: Arc<dyn KvStore>,
}

impl LazyDeleteStore {
    /// Creates a new LazyDeleteStore.
    pub fn new(kv: Arc<dyn KvStore>) -> Self;

    /// Marks an inode as lazily deleted (unlinked but still open).
    ///
    /// Returns `MetaError::AlreadyExists` if already in lazy-delete state.
    pub fn mark_deleted(&self, ino: InodeId, original_path: String, open_fd_count: u32) -> Result<(), MetaError>;

    /// Increments the open fd count for a lazy-deleted inode.
    ///
    /// Returns `MetaError::NotFound` if inode is not in lazy-delete state.
    pub fn inc_fd_count(&self, ino: InodeId) -> Result<(), MetaError>;

    /// Decrements the open fd count for a lazy-deleted inode.
    ///
    /// Returns `Ok(true)` if the fd count reached 0 (inode ready for GC).
    /// Returns `Ok(false)` if there are still open fds.
    /// Returns `MetaError::NotFound` if inode is not in lazy-delete state.
    pub fn dec_fd_count(&self, ino: InodeId) -> Result<bool, MetaError>;

    /// Gets the lazy-delete entry for an inode.
    ///
    /// Returns `None` if the inode is not in lazy-delete state.
    pub fn get_entry(&self, ino: InodeId) -> Result<Option<LazyDeleteEntry>, MetaError>;

    /// Removes an inode from lazy-delete state.
    ///
    /// Called after the inode has been garbage collected.
    pub fn remove_entry(&self, ino: InodeId) -> Result<(), MetaError>;

    /// Returns all inodes ready for GC (open_fd_count == 0).
    pub fn ready_for_gc(&self) -> Result<Vec<LazyDeleteEntry>, MetaError>;

    /// Returns all lazy-deleted inodes (for monitoring).
    pub fn list_all(&self) -> Result<Vec<LazyDeleteEntry>, MetaError>;

    /// Returns the total number of inodes in lazy-delete state.
    pub fn count(&self) -> Result<usize, MetaError>;

    /// Purges all entries with open_fd_count == 0.
    ///
    /// Called on server restart to clean up any entries that were not GC'd before shutdown.
    /// Returns the number of entries purged.
    pub fn purge_ready_for_gc(&self) -> Result<usize, MetaError>;
}
```

## KV Key Format

- `b"ld:" + ino.as_u64().to_be_bytes()`
- Value: `bincode::serialize(&LazyDeleteEntry)`

## Required Tests (15 tests)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn make_store() -> LazyDeleteStore {
        LazyDeleteStore::new(Arc::new(MemoryKvStore::new()))
    }
    fn ino(n: u64) -> InodeId { InodeId::new(n) }
}
```

1. `test_mark_deleted_and_get` — mark inode as deleted, verify get_entry returns it
2. `test_mark_deleted_duplicate` — mark same inode twice, second returns AlreadyExists
3. `test_inc_fd_count` — inc_fd_count increments correctly
4. `test_inc_fd_count_not_found` — inc on non-existent ino returns NotFound
5. `test_dec_fd_count_still_open` — dec from 2, returns Ok(false), count=1
6. `test_dec_fd_count_reaches_zero` — dec from 1, returns Ok(true), fd_count=0
7. `test_dec_fd_count_not_found` — dec on non-existent ino returns NotFound
8. `test_remove_entry` — remove_entry, get_entry returns None
9. `test_ready_for_gc_empty` — ready_for_gc when all have open fds returns empty
10. `test_ready_for_gc_some` — mixed: some with fd_count=0, some with fd_count>0; only fd_count=0 returned
11. `test_list_all` — list_all returns all entries (both open and ready for GC)
12. `test_count` — count reflects number of entries
13. `test_purge_ready_for_gc` — purge removes fd_count=0 entries, returns count purged
14. `test_purge_keeps_open_entries` — purge does NOT remove entries with open fds
15. `test_entry_serde` — LazyDeleteEntry round-trips through bincode

## Important

- Write the file directly to `crates/claudefs-meta/src/lazy_delete.rs`
- Do NOT modify lib.rs
- No unused imports, no clippy warnings
- All tests must pass with `cargo test -p claudefs-meta lazy_delete`
