# A2: Implement hardlink.rs — Hard Link Tracking Module

## Context

You are implementing a new module for the `claudefs-meta` crate (`crates/claudefs-meta/`).

The crate uses:
- `thiserror` for errors
- `serde` + `bincode` for serialization
- `tracing` for logging
- All public types must have `///` doc comments
- Tests: standard `#[test]` in `#[cfg(test)] mod tests`

## Existing types

From `types.rs`:
```rust
pub struct InodeId(u64); // newtype with InodeId::new(u64), as_u64()
pub enum MetaError { NotFound(String), AlreadyExists(String), KvError(String), InvalidArgument(String), PermissionDenied, /* ... */ }
```

From `kvstore.rs`:
```rust
pub trait KvStore: Send + Sync {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, MetaError>;
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), MetaError>;
    fn delete(&self, key: &[u8]) -> Result<(), MetaError>;
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, MetaError>;
    fn scan_range(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, MetaError>;
    fn contains_key(&self, key: &[u8]) -> Result<bool, MetaError>;
    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), MetaError>;
}
pub struct MemoryKvStore; // implements KvStore + Default + Clone
```

## Task: Implement `crates/claudefs-meta/src/hardlink.rs`

Hard links in POSIX filesystems allow multiple directory entries to point to the same
inode. This module tracks hard link references to detect when an inode's nlink count
is zero (allowing garbage collection).

```rust
/// Hard link tracking store.
///
/// Maintains an explicit map of (parent_ino, name) → target_ino for all hard links.
/// A hard link is a directory entry that is NOT the original file creation entry.
///
/// When an inode's nlink drops to 0, it can be garbage collected by calling
/// the GcManager. This store tracks all non-primary links for a given target inode.
pub struct HardLinkStore {
    kv: Arc<dyn KvStore>,
}

impl HardLinkStore {
    /// Creates a new HardLinkStore backed by the given KV store.
    pub fn new(kv: Arc<dyn KvStore>) -> Self;

    /// Records a new hard link: (parent_ino, name) → target_ino.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::AlreadyExists` if a link from (parent, name) already exists.
    pub fn add_link(&self, parent_ino: InodeId, name: &str, target_ino: InodeId) -> Result<(), MetaError>;

    /// Removes a hard link by (parent_ino, name).
    ///
    /// Returns `Ok(())` if the link was removed, or if it didn't exist (idempotent).
    pub fn remove_link(&self, parent_ino: InodeId, name: &str) -> Result<(), MetaError>;

    /// Returns the target inode ID for a given (parent_ino, name) link.
    ///
    /// Returns `None` if the link doesn't exist.
    pub fn get_target(&self, parent_ino: InodeId, name: &str) -> Result<Option<InodeId>, MetaError>;

    /// Lists all hard links pointing to a given target inode.
    ///
    /// Returns a list of (parent_ino, name) tuples.
    /// This is used to implement `stat` (nlink count) and for fsck verification.
    pub fn list_links_to(&self, target_ino: InodeId) -> Result<Vec<(InodeId, String)>, MetaError>;

    /// Lists all hard links that originate from a given parent directory.
    ///
    /// Returns a list of (name, target_ino) tuples.
    /// This is used to enumerate directory contents efficiently.
    pub fn list_links_from(&self, parent_ino: InodeId) -> Result<Vec<(String, InodeId)>, MetaError>;

    /// Returns the number of hard links to a given target inode.
    ///
    /// This is the nlink count for the inode (excluding the implicit count from InodeAttr.nlink).
    pub fn link_count(&self, target_ino: InodeId) -> Result<u32, MetaError>;

    /// Checks if an inode has any hard links remaining.
    ///
    /// Returns `true` if there are one or more hard links from any (parent, name) to this inode.
    pub fn has_links(&self, target_ino: InodeId) -> Result<bool, MetaError>;

    /// Atomically moves a link: removes (old_parent, old_name) and adds (new_parent, new_name).
    ///
    /// This is used for the rename() syscall when moving hard links.
    /// Returns `MetaError::NotFound` if the source link doesn't exist.
    pub fn rename_link(&self, old_parent: InodeId, old_name: &str, new_parent: InodeId, new_name: &str) -> Result<(), MetaError>;

    /// Returns total number of hard links tracked in this store.
    pub fn total_link_count(&self) -> Result<usize, MetaError>;
}
```

## KV Key Format

Use these key formats:
- Forward index `(parent_ino, name) → target_ino`:
  `b"hl:fwd:" + parent_ino.as_u64().to_be_bytes() + b":" + name.as_bytes()`
  Value: `target_ino.as_u64().to_be_bytes()`

- Reverse index `target_ino → [(parent_ino, name)]`:
  `b"hl:rev:" + target_ino.as_u64().to_be_bytes() + b":" + parent_ino.as_u64().to_be_bytes() + b":" + name.as_bytes()`
  Value: `[]` (empty — presence of key indicates the link exists)

## Required Tests (15 tests)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn make_store() -> HardLinkStore {
        HardLinkStore::new(Arc::new(MemoryKvStore::new()))
    }

    // Tests:
}
```

1. `test_add_and_get_link` — add link, verify get_target returns correct inode
2. `test_add_link_already_exists` — add same link twice, second returns AlreadyExists
3. `test_remove_link` — add then remove, get_target returns None
4. `test_remove_nonexistent_link` — remove non-existent link is Ok (idempotent)
5. `test_list_links_to_single` — add 3 links to same target, list_links_to returns all 3
6. `test_list_links_to_empty` — no links to inode, list_links_to returns empty
7. `test_list_links_from_single` — add 3 links from same parent, list_links_from returns all 3
8. `test_link_count` — link_count returns correct count (0, then 1, then 2 as links added)
9. `test_has_links` — has_links false before adding, true after adding
10. `test_rename_link_success` — rename from old to new location, get_target on old returns None, new returns inode
11. `test_rename_link_not_found` — rename on non-existent source returns NotFound
12. `test_total_link_count` — total_link_count reflects all links across all inodes
13. `test_multiple_targets_from_same_parent` — multiple different names in same parent to different targets
14. `test_links_independent_across_parents` — same name in different parents, each independent
15. `test_remove_one_of_many_links` — add 3 links to same target, remove 1, link_count == 2

## Important Rules

- Write the file directly to `crates/claudefs-meta/src/hardlink.rs`
- Do NOT modify lib.rs (I will update it separately)
- No unused imports
- No dead code warnings
- Every public item must have `///` doc comment
- All tests must pass with `cargo test -p claudefs-meta hardlink`
