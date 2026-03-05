//! Byte-range POSIX lock table (fcntl/lockf byte-range locks).
//!
//! This module tracks in-memory state of byte-range locks held by client processes,
//! detects conflicts between lock requests, and provides the data structures needed
//! to drive lock RPCs to the metadata server.
//!
//! Distinct from:
//! - `flock.rs` — BSD whole-file flock(2) advisory locks
//! - `locking.rs` — low-level synchronization primitives

use std::collections::HashMap;
use std::time::Instant;

/// Lock type for byte-range locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockType {
    Read,
    Write,
}

impl LockType {
    /// Returns true if this is a read (shared) lock.
    pub fn is_read(&self) -> bool {
        matches!(self, LockType::Read)
    }

    /// Returns true if this is a write (exclusive) lock.
    pub fn is_write(&self) -> bool {
        matches!(self, LockType::Write)
    }
}

/// A byte-range lock record.
#[derive(Debug, Clone, PartialEq)]
pub struct ByteRangeLock {
    pub owner_pid: u32,
    pub lock_type: LockType,
    pub start: u64,
    pub end: u64,
    pub acquired_at: Instant,
}

impl ByteRangeLock {
    /// Creates a new byte range lock.
    pub fn new(owner_pid: u32, lock_type: LockType, start: u64, end: u64) -> Self {
        Self {
            owner_pid,
            lock_type,
            start,
            end,
            acquired_at: Instant::now(),
        }
    }

    /// Returns true if this lock's range overlaps [start, end).
    pub fn overlaps(&self, start: u64, end: u64) -> bool {
        self.start < end && self.end > start
    }

    /// Returns true if this lock's range contains the point `offset`.
    pub fn contains(&self, offset: u64) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Returns true if this lock conflicts with a request on [start, end).
    /// Write requests conflict with any lock. Read requests only conflict with Write locks.
    /// Same PID does not conflict with itself.
    pub fn conflicts_with(&self, lock_type: LockType, start: u64, end: u64) -> bool {
        if !self.overlaps(start, end) {
            return false;
        }
        if self.owner_pid == 0 {
            return true;
        }
        match (self.lock_type, lock_type) {
            (_, LockType::Write) => true,
            (LockType::Write, LockType::Read) => true,
            (LockType::Read, LockType::Read) => false,
        }
    }
}

/// Error indicating a conflicting lock was found.
#[derive(Debug, Clone)]
pub struct LockConflict {
    pub conflicting_lock: ByteRangeLock,
}

impl std::fmt::Display for LockConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "conflicting lock: pid={}, type={:?}, range=[{}, {})",
            self.conflicting_lock.owner_pid,
            self.conflicting_lock.lock_type,
            self.conflicting_lock.start,
            self.conflicting_lock.end
        )
    }
}

impl std::error::Error for LockConflict {}

/// The range lock table for a single file.
#[derive(Debug, Default)]
pub struct RangeLockTable {
    locks: Vec<ByteRangeLock>,
}

impl RangeLockTable {
    /// Creates a new empty range lock table.
    pub fn new() -> Self {
        Self { locks: Vec::new() }
    }

    /// Attempts to acquire a lock. Returns Err if a conflicting lock exists.
    pub fn try_lock(&mut self, lock: ByteRangeLock) -> Result<(), LockConflict> {
        if let Some(conflict) =
            self.find_conflict_internal(lock.lock_type, lock.start, lock.end, lock.owner_pid)
        {
            return Err(LockConflict {
                conflicting_lock: conflict.clone(),
            });
        }
        self.locks.push(lock);
        Ok(())
    }

    /// Releases all locks held by the given PID in the given range.
    /// If range is (0, u64::MAX), releases all locks for the PID.
    pub fn unlock(&mut self, owner_pid: u32, start: u64, end: u64) {
        self.locks.retain(|lock| {
            if lock.owner_pid != owner_pid {
                return true;
            }
            if start == 0 && end == u64::MAX {
                return false;
            }
            !lock.overlaps(start, end)
        });
    }

    /// Returns the first conflicting lock for the given request, if any.
    pub fn find_conflict(
        &self,
        lock_type: LockType,
        start: u64,
        end: u64,
        requester_pid: u32,
    ) -> Option<&ByteRangeLock> {
        self.find_conflict_internal(lock_type, start, end, requester_pid)
    }

    fn find_conflict_internal(
        &self,
        lock_type: LockType,
        start: u64,
        end: u64,
        requester_pid: u32,
    ) -> Option<&ByteRangeLock> {
        for lock in &self.locks {
            if lock.owner_pid == requester_pid {
                continue;
            }
            if lock.conflicts_with(lock_type, start, end) {
                return Some(lock);
            }
        }
        None
    }

    /// Returns all locks held by the given PID.
    pub fn locks_for_pid(&self, pid: u32) -> Vec<&ByteRangeLock> {
        self.locks.iter().filter(|l| l.owner_pid == pid).collect()
    }

    /// Returns the total number of active locks.
    pub fn len(&self) -> usize {
        self.locks.len()
    }

    /// Returns true if there are no locks.
    pub fn is_empty(&self) -> bool {
        self.locks.is_empty()
    }

    /// Releases all locks for a PID (called on process exit).
    pub fn release_pid(&mut self, pid: u32) {
        self.locks.retain(|lock| lock.owner_pid != pid);
    }
}

/// Multi-inode range lock manager (one table per open file).
pub struct RangeLockManager {
    tables: HashMap<u64, RangeLockTable>,
}

impl RangeLockManager {
    /// Creates a new range lock manager.
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
        }
    }

    /// Attempts to acquire a lock on the given inode.
    pub fn try_lock(&mut self, inode: u64, lock: ByteRangeLock) -> Result<(), LockConflict> {
        let table = self.tables.entry(inode).or_insert_with(RangeLockTable::new);
        table.try_lock(lock)
    }

    /// Unlocks a range on the given inode.
    pub fn unlock(&mut self, inode: u64, owner_pid: u32, start: u64, end: u64) {
        if let Some(table) = self.tables.get_mut(&inode) {
            table.unlock(owner_pid, start, end);
        }
    }

    /// Finds a conflicting lock on the given inode.
    pub fn find_conflict(
        &self,
        inode: u64,
        lock_type: LockType,
        start: u64,
        end: u64,
        requester_pid: u32,
    ) -> Option<ByteRangeLock> {
        self.tables
            .get(&inode)
            .and_then(|t| t.find_conflict(lock_type, start, end, requester_pid))
            .cloned()
    }

    /// Releases all locks for a PID across all inodes.
    pub fn release_pid(&mut self, pid: u32) {
        for table in self.tables.values_mut() {
            table.release_pid(pid);
        }
    }

    /// Returns the total number of locks across all inodes.
    pub fn total_locks(&self) -> usize {
        self.tables.values().map(|t| t.len()).sum()
    }

    /// Returns the number of locks for a specific inode.
    pub fn inode_lock_count(&self, inode: u64) -> usize {
        self.tables.get(&inode).map(|t| t.len()).unwrap_or(0)
    }
}

impl Default for RangeLockManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_lock(pid: u32, start: u64, end: u64) -> ByteRangeLock {
        ByteRangeLock::new(pid, LockType::Read, start, end)
    }

    fn write_lock(pid: u32, start: u64, end: u64) -> ByteRangeLock {
        ByteRangeLock::new(pid, LockType::Write, start, end)
    }

    #[test]
    fn new_table_is_empty() {
        let table = RangeLockTable::new();
        assert!(table.is_empty());
    }

    #[test]
    fn acquire_read_lock_succeeds_when_empty() {
        let mut table = RangeLockTable::new();
        let result = table.try_lock(read_lock(100, 0, 100));
        assert!(result.is_ok());
    }

    #[test]
    fn acquire_write_lock_succeeds_when_empty() {
        let mut table = RangeLockTable::new();
        let result = table.try_lock(write_lock(100, 0, 100));
        assert!(result.is_ok());
    }

    #[test]
    fn two_read_locks_different_pids_succeed() {
        let mut table = RangeLockTable::new();
        assert!(table.try_lock(read_lock(100, 0, 100)).is_ok());
        assert!(table.try_lock(read_lock(200, 0, 100)).is_ok());
    }

    #[test]
    fn read_write_conflict_different_pids() {
        let mut table = RangeLockTable::new();
        assert!(table.try_lock(read_lock(100, 0, 100)).is_ok());
        let result = table.try_lock(write_lock(200, 0, 100));
        assert!(result.is_err());
    }

    #[test]
    fn write_read_conflict_different_pids() {
        let mut table = RangeLockTable::new();
        assert!(table.try_lock(write_lock(100, 0, 100)).is_ok());
        let result = table.try_lock(read_lock(200, 0, 100));
        assert!(result.is_err());
    }

    #[test]
    fn write_write_conflict_different_pids() {
        let mut table = RangeLockTable::new();
        assert!(table.try_lock(write_lock(100, 0, 100)).is_ok());
        let result = table.try_lock(write_lock(200, 0, 100));
        assert!(result.is_err());
    }

    #[test]
    fn same_pid_can_relock_own_range() {
        let mut table = RangeLockTable::new();
        assert!(table.try_lock(read_lock(100, 0, 100)).is_ok());
        let result = table.try_lock(write_lock(100, 0, 100));
        assert!(result.is_ok());
    }

    #[test]
    fn non_overlapping_ranges_write_write_succeed() {
        let mut table = RangeLockTable::new();
        assert!(table.try_lock(write_lock(100, 0, 50)).is_ok());
        assert!(table.try_lock(write_lock(200, 50, 100)).is_ok());
    }

    #[test]
    fn overlaps_adjacent_returns_false() {
        let lock = ByteRangeLock::new(1, LockType::Write, 0, 50);
        assert!(!lock.overlaps(50, 100));
    }

    #[test]
    fn overlaps_returns_true() {
        let lock = ByteRangeLock::new(1, LockType::Write, 0, 100);
        assert!(lock.overlaps(50, 150));
    }

    #[test]
    fn contains_offset_in_range() {
        let lock = ByteRangeLock::new(1, LockType::Write, 10, 20);
        assert!(lock.contains(15));
    }

    #[test]
    fn contains_offset_outside_range() {
        let lock = ByteRangeLock::new(1, LockType::Write, 10, 20);
        assert!(!lock.contains(5));
        assert!(!lock.contains(20));
    }

    #[test]
    fn unlock_removes_lock() {
        let mut table = RangeLockTable::new();
        table.try_lock(write_lock(100, 0, 100)).unwrap();
        table.unlock(100, 0, 100);
        assert!(table.is_empty());
    }

    #[test]
    fn unlock_noop_when_no_lock() {
        let mut table = RangeLockTable::new();
        table.unlock(100, 0, 100);
        assert!(table.is_empty());
    }

    #[test]
    fn find_conflict_returns_conflicting_lock() {
        let mut table = RangeLockTable::new();
        table.try_lock(write_lock(100, 0, 100)).unwrap();
        let conflict = table.find_conflict(LockType::Read, 0, 100, 200);
        assert!(conflict.is_some());
    }

    #[test]
    fn find_conflict_returns_none_when_no_conflict() {
        let mut table = RangeLockTable::new();
        table.try_lock(read_lock(100, 0, 100)).unwrap();
        let conflict = table.find_conflict(LockType::Read, 200, 300, 200);
        assert!(conflict.is_none());
    }

    #[test]
    fn release_pid_removes_all_locks() {
        let mut table = RangeLockTable::new();
        table.try_lock(read_lock(100, 0, 100)).unwrap();
        table.try_lock(write_lock(100, 200, 300)).unwrap();
        table.release_pid(100);
        assert!(table.is_empty());
    }

    #[test]
    fn manager_try_lock_routes_to_inode() {
        let mut mgr = RangeLockManager::new();
        assert!(mgr.try_lock(1, write_lock(100, 0, 100)).is_ok());
        assert!(mgr.try_lock(2, write_lock(100, 0, 100)).is_ok());
    }

    #[test]
    fn manager_release_pid_removes_across_inodes() {
        let mut mgr = RangeLockManager::new();
        mgr.try_lock(1, read_lock(100, 0, 100)).unwrap();
        mgr.try_lock(2, read_lock(100, 0, 100)).unwrap();
        mgr.release_pid(100);
        assert_eq!(mgr.total_locks(), 0);
    }

    #[test]
    fn locks_for_pid_returns_correct_locks() {
        let mut table = RangeLockTable::new();
        table.try_lock(read_lock(100, 0, 50)).unwrap();
        table.try_lock(read_lock(100, 100, 150)).unwrap();
        table.try_lock(read_lock(200, 50, 100)).unwrap();
        let locks = table.locks_for_pid(100);
        assert_eq!(locks.len(), 2);
    }

    #[test]
    fn end_max_covers_all_remaining_bytes() {
        let mut table = RangeLockTable::new();
        let lock = ByteRangeLock::new(1, LockType::Write, 0, u64::MAX);
        assert!(table.try_lock(lock).is_ok());
        let locks = table.locks_for_pid(1);
        assert!(!locks.is_empty());
        assert!(locks[0].contains(u64::MAX - 1));
    }

    #[test]
    fn len_returns_lock_count() {
        let mut table = RangeLockTable::new();
        table.try_lock(read_lock(100, 0, 50)).unwrap();
        table.try_lock(read_lock(200, 100, 150)).unwrap();
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn inode_lock_count_zero_when_empty() {
        let mgr = RangeLockManager::new();
        assert_eq!(mgr.inode_lock_count(999), 0);
    }

    #[test]
    fn unlock_partial_range() {
        let mut table = RangeLockTable::new();
        table.try_lock(read_lock(100, 0, 100)).unwrap();
        table.unlock(100, 25, 75);
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn find_conflict_same_pid_returns_none() {
        let mut table = RangeLockTable::new();
        table.try_lock(write_lock(100, 0, 100)).unwrap();
        let conflict = table.find_conflict(LockType::Write, 0, 100, 100);
        assert!(conflict.is_none());
    }

    #[test]
    fn manager_find_conflict() {
        let mut mgr = RangeLockManager::new();
        mgr.try_lock(1, write_lock(100, 0, 100)).unwrap();
        let conflict = mgr.find_conflict(1, LockType::Read, 0, 100, 200);
        assert!(conflict.is_some());
    }

    #[test]
    fn manager_total_locks() {
        let mut mgr = RangeLockManager::new();
        mgr.try_lock(1, read_lock(100, 0, 50)).unwrap();
        mgr.try_lock(2, read_lock(100, 0, 50)).unwrap();
        assert_eq!(mgr.total_locks(), 2);
    }
}
