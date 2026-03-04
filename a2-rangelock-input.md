# A2: Implement range_lock.rs — POSIX Byte-Range Locks

## Context

Implement POSIX byte-range locks for the `claudefs-meta` crate (`crates/claudefs-meta/`).
This is needed for `fcntl()` F_SETLK/F_SETLKW/F_GETLK support in the FUSE client.

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
pub enum MetaError { NotFound(String), AlreadyExists(String), KvError(String),
    InvalidArgument(String), PermissionDenied, /* ... */ }
```

## Task: Implement `crates/claudefs-meta/src/range_lock.rs`

```rust
/// The type of a byte-range lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockType {
    /// Shared (read) lock — multiple readers allowed.
    Read,
    /// Exclusive (write) lock — only one writer, no readers.
    Write,
}

/// A POSIX byte-range lock.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RangeLock {
    /// The owner of this lock (process ID or client ID).
    pub owner_pid: u64,
    /// The type of lock.
    pub lock_type: LockType,
    /// Start offset of the locked range (inclusive).
    pub start: u64,
    /// End offset of the locked range (inclusive). u64::MAX means "end of file".
    pub end: u64,
}

impl RangeLock {
    /// Creates a new RangeLock.
    pub fn new(owner_pid: u64, lock_type: LockType, start: u64, end: u64) -> Self;

    /// Returns true if this lock overlaps with the given range [other_start, other_end].
    pub fn overlaps(&self, other_start: u64, other_end: u64) -> bool;

    /// Returns true if this lock conflicts with another lock.
    ///
    /// Two locks conflict if they overlap AND at least one is a Write lock.
    /// Two Read locks on the same range do NOT conflict.
    pub fn conflicts_with(&self, other: &RangeLock) -> bool;
}

/// In-memory manager for per-inode byte-range locks.
///
/// This implements POSIX advisory byte-range locking (fcntl F_SETLK).
/// Locks are not persisted — they are cleared on server restart (standard POSIX behavior).
pub struct RangeLockManager {
    // Per-inode lock lists.
    // InodeId.as_u64() → Vec<RangeLock>
    locks: std::sync::RwLock<std::collections::HashMap<u64, Vec<RangeLock>>>,
}

impl RangeLockManager {
    /// Creates a new empty RangeLockManager.
    pub fn new() -> Self;

    /// Attempts to acquire a byte-range lock on the given inode.
    ///
    /// Returns `Ok(())` if the lock was acquired.
    /// Returns `Err(MetaError::PermissionDenied)` if a conflicting lock exists.
    ///
    /// If a lock already exists for the same (owner_pid, start, end), it is upgraded/replaced.
    pub fn lock(&self, ino: InodeId, new_lock: RangeLock) -> Result<(), MetaError>;

    /// Releases a byte-range lock on the given inode.
    ///
    /// Removes all locks owned by `owner_pid` that overlap the range [start, end].
    /// If a lock partially overlaps, the non-overlapping portion is retained.
    /// Returns `Ok(())` even if no lock was held (idempotent).
    pub fn unlock(&self, ino: InodeId, owner_pid: u64, start: u64, end: u64) -> Result<(), MetaError>;

    /// Checks if a lock can be acquired without actually acquiring it (fcntl F_GETLK).
    ///
    /// Returns `None` if the lock can be acquired (no conflicts).
    /// Returns `Some(conflicting_lock)` with the first conflicting lock if it cannot.
    pub fn test_lock(&self, ino: InodeId, requested: &RangeLock) -> Option<RangeLock>;

    /// Returns all current locks held on the given inode.
    pub fn get_locks(&self, ino: InodeId) -> Vec<RangeLock>;

    /// Releases ALL locks held by a given owner_pid across ALL inodes.
    ///
    /// Called when a client disconnects to clean up all held locks.
    pub fn release_all_by_owner(&self, owner_pid: u64);

    /// Returns the total number of locks held across all inodes.
    pub fn total_lock_count(&self) -> usize;

    /// Returns the number of inodes that have at least one lock.
    pub fn locked_inode_count(&self) -> usize;
}

impl Default for RangeLockManager { ... }
```

## Required Tests (16 tests)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_mgr() -> RangeLockManager { RangeLockManager::new() }
    fn ino(n: u64) -> InodeId { InodeId::new(n) }
    fn read_lock(pid: u64, start: u64, end: u64) -> RangeLock {
        RangeLock::new(pid, LockType::Read, start, end)
    }
    fn write_lock(pid: u64, start: u64, end: u64) -> RangeLock {
        RangeLock::new(pid, LockType::Write, start, end)
    }
}
```

Tests:
1. `test_lock_read_no_conflict` — two processes read-lock same range, both succeed
2. `test_lock_write_conflicts_with_read` — read lock held, write lock on same range returns PermissionDenied
3. `test_lock_write_conflicts_with_write` — write lock held, second write lock returns PermissionDenied
4. `test_lock_non_overlapping_ranges` — write locks on [0,100] and [101,200], no conflict
5. `test_lock_adjacent_ranges_no_conflict` — write locks on [0,99] and [100,199] (adjacent), no conflict
6. `test_overlaps_logic` — test RangeLock::overlaps directly with various ranges
7. `test_conflicts_with_logic` — test RangeLock::conflicts_with: R+R=no, R+W=yes, W+R=yes, W+W=yes
8. `test_unlock_removes_lock` — lock then unlock, get_locks returns empty
9. `test_unlock_nonexistent` — unlock with no lock held is Ok
10. `test_test_lock_no_conflict` — test_lock returns None when no conflicting lock
11. `test_test_lock_conflict` — test_lock returns Some with conflicting lock info
12. `test_release_all_by_owner` — owner holds 2 locks on different inodes, release_all_by_owner removes both
13. `test_total_lock_count` — total_lock_count tracks correctly
14. `test_locked_inode_count` — locked_inode_count tracks correctly
15. `test_lock_upgrade_same_owner` — same owner re-locks same range with different type, replaces
16. `test_lock_eof_range` — lock with end=u64::MAX (whole file) conflicts with partial ranges

## Important

- Write the file directly to `crates/claudefs-meta/src/range_lock.rs`
- Do NOT modify lib.rs
- No unused imports, no clippy warnings
- All 16 tests must pass with `cargo test -p claudefs-meta range_lock`
- Use `std::sync::RwLock<HashMap<u64, Vec<RangeLock>>>` for the lock table (no external deps needed)
