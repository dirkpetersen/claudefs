# A2: Implement space_accounting.rs — Directory Disk Usage Tracking

## Context

Implement a new module for the `claudefs-meta` crate (`crates/claudefs-meta/`).

The crate uses:
- `thiserror` for errors
- `serde` + `bincode` for serialization
- `tracing` for logging
- All public types must have `///` doc comments
- Tests: standard `#[test]` in `#[cfg(test)] mod tests`

## Existing types

From `types.rs`:
```rust
pub struct InodeId(u64); // InodeId::new(u64), as_u64()
pub enum MetaError { NotFound(String), AlreadyExists(String), KvError(String), InvalidArgument(String), /* ... */ }
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
pub struct MemoryKvStore; // implements KvStore
pub enum BatchOp { Put { key: Vec<u8>, value: Vec<u8> }, Delete { key: Vec<u8> } }
```

## Task: Implement `crates/claudefs-meta/src/space_accounting.rs`

Track per-directory disk usage (bytes and inode count) for quota enforcement
and du-like reporting. When a file is created/deleted/resized, the accounting
is updated for the containing directory and all ancestor directories up to root.

```rust
/// Disk usage for a directory subtree.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirUsage {
    /// Total bytes used by all files in this directory subtree.
    pub bytes: u64,
    /// Total number of inodes (files + subdirectories) in this subtree.
    pub inodes: u64,
}

impl DirUsage {
    /// Creates a new empty DirUsage.
    pub fn new() -> Self;

    /// Returns true if both bytes and inodes are zero.
    pub fn is_empty(&self) -> bool;
}

/// Space accounting store for per-directory usage tracking.
pub struct SpaceAccountingStore {
    kv: Arc<dyn KvStore>,
}

impl SpaceAccountingStore {
    /// Creates a new SpaceAccountingStore backed by the given KV store.
    pub fn new(kv: Arc<dyn KvStore>) -> Self;

    /// Gets the current disk usage for a directory (direct subtree, not ancestors).
    ///
    /// Returns empty DirUsage if no accounting entry exists.
    pub fn get_usage(&self, dir_ino: InodeId) -> Result<DirUsage, MetaError>;

    /// Sets the disk usage for a directory directly.
    ///
    /// Use this for initial setup or bulk import.
    pub fn set_usage(&self, dir_ino: InodeId, usage: DirUsage) -> Result<(), MetaError>;

    /// Atomically adds delta_bytes bytes and delta_inodes inodes to a directory's usage.
    ///
    /// delta_bytes and delta_inodes can be negative (for deletions/shrinks).
    /// Uses saturating arithmetic to avoid underflow.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::KvError` if the KV store operation fails.
    pub fn add_delta(&self, dir_ino: InodeId, delta_bytes: i64, delta_inodes: i64) -> Result<(), MetaError>;

    /// Propagates usage deltas up the directory tree to all ancestor directories.
    ///
    /// Takes a slice of ancestor InodeIds from child to root (inclusive).
    /// For example: if file /a/b/c.txt is created, ancestors = [ino_b, ino_a, root_ino]
    ///
    /// Updates all ancestors atomically using write_batch.
    pub fn propagate_up(&self, ancestors: &[InodeId], delta_bytes: i64, delta_inodes: i64) -> Result<(), MetaError>;

    /// Removes the accounting entry for a directory (on rmdir).
    pub fn remove_usage(&self, dir_ino: InodeId) -> Result<(), MetaError>;

    /// Returns the total bytes and inodes tracked across all directories.
    ///
    /// This is a sum over all stored entries — NOT propagation-adjusted.
    /// Use this for monitoring and consistency checks.
    pub fn total_tracked(&self) -> Result<DirUsage, MetaError>;

    /// Lists all directory accounting entries.
    ///
    /// Returns (dir_ino, DirUsage) pairs sorted by InodeId.
    pub fn list_all(&self) -> Result<Vec<(InodeId, DirUsage)>, MetaError>;
}
```

## KV Key Format

Use this key format:
- `b"sa:" + dir_ino.as_u64().to_be_bytes()`
- Value: `bincode::serialize(&DirUsage)` → `(bytes: u64, inodes: u64)`

## Required Tests (14 tests)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn make_store() -> SpaceAccountingStore {
        SpaceAccountingStore::new(Arc::new(MemoryKvStore::new()))
    }
}
```

1. `test_get_usage_empty` — get_usage on unknown inode returns empty DirUsage
2. `test_set_and_get_usage` — set_usage then get_usage returns same value
3. `test_add_delta_positive` — add_delta +100 bytes +1 inode, verify get_usage
4. `test_add_delta_multiple` — add_delta called 3 times, verify cumulative result
5. `test_add_delta_negative` — add_delta -50 bytes (shrink), verify correct result
6. `test_add_delta_saturating` — add_delta negative that would underflow, saturates to 0
7. `test_propagate_up_single_level` — propagate to [parent, root], both updated
8. `test_propagate_up_multiple_levels` — 3-level hierarchy, all ancestors updated
9. `test_propagate_up_empty_ancestors` — empty slice is no-op
10. `test_remove_usage` — remove_usage, get_usage returns empty
11. `test_remove_nonexistent` — remove_usage on unknown inode is Ok
12. `test_total_tracked` — total_tracked sums multiple dirs correctly
13. `test_list_all` — list_all returns all entries sorted
14. `test_dir_usage_serde` — DirUsage round-trips through bincode

## Important

- Write the file directly to `crates/claudefs-meta/src/space_accounting.rs`
- Do NOT modify lib.rs — I will update it separately
- No unused imports, no clippy warnings
- All tests must pass with `cargo test -p claudefs-meta space_accounting`
