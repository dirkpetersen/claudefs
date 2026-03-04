//! Byte-range lock manager for POSIX file locking (fcntl F_GETLK/F_SETLK).
//!
//! Per-inode range locks with conflict detection for read/write locks.

use std::collections::HashMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::types::{InodeId, MetaError};

/// Type of range lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockType {
    /// Shared lock, multiple readers allowed on overlapping ranges.
    Read,
    /// Exclusive lock, no other locks allowed on overlapping range.
    Write,
}

/// Represents a byte-range lock on a file region.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RangeLock {
    /// Process ID that owns this lock.
    pub owner_pid: u64,
    /// Type of lock (read or write).
    pub lock_type: LockType,
    /// Start of locked range (inclusive).
    pub start: u64,
    /// End of locked range (inclusive).
    pub end: u64,
}

impl RangeLock {
    /// Creates a new RangeLock.
    pub fn new(owner_pid: u64, lock_type: LockType, start: u64, end: u64) -> Self {
        Self {
            owner_pid,
            lock_type,
            start,
            end,
        }
    }

    /// Checks if this lock's range overlaps with another range.
    ///
    /// end=u64::MAX means "end of file" and overlaps with any range.
    pub fn overlaps(&self, other_start: u64, other_end: u64) -> bool {
        if self.end == u64::MAX {
            return self.start <= other_end;
        }
        if other_end == u64::MAX {
            return other_start <= self.end;
        }
        self.start <= other_end && other_start <= self.end
    }

    /// Checks if this lock conflicts with another lock.
    ///
    /// Conflicts:
    /// - Read+Read: no conflict (multiple readers allowed)
    /// - Read+Write: conflict
    /// - Write+Read: conflict
    /// - Write+Write: conflict
    pub fn conflicts_with(&self, other: &RangeLock) -> bool {
        if !self.overlaps(other.start, other.end) {
            return false;
        }
        !matches!(
            (self.lock_type, other.lock_type),
            (LockType::Read, LockType::Read)
        )
    }
}

/// Manages byte-range locks per inode.
pub struct RangeLockManager {
    locks: RwLock<HashMap<InodeId, Vec<RangeLock>>>,
}

impl Default for RangeLockManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RangeLockManager {
    /// Creates a new RangeLockManager.
    pub fn new() -> Self {
        Self {
            locks: RwLock::new(HashMap::new()),
        }
    }

    /// Attempts to acquire a lock on an inode.
    ///
    /// Returns Ok(()) on success, Err(MetaError::PermissionDenied) if conflict.
    pub fn lock(
        &self,
        inode: InodeId,
        owner_pid: u64,
        lock_type: LockType,
        start: u64,
        end: u64,
    ) -> Result<(), MetaError> {
        let new_lock = RangeLock::new(owner_pid, lock_type, start, end);

        let mut locks = self.locks.write().unwrap();

        let inode_locks = locks.entry(inode).or_default();

        for existing_lock in inode_locks.iter() {
            if existing_lock.owner_pid == owner_pid {
                continue;
            }
            if existing_lock.conflicts_with(&new_lock) {
                return Err(MetaError::PermissionDenied);
            }
        }

        for existing_lock in inode_locks.iter_mut() {
            if existing_lock.owner_pid == owner_pid
                && existing_lock.start == start
                && existing_lock.end == end
            {
                *existing_lock = new_lock;
                return Ok(());
            }
        }

        inode_locks.push(new_lock);
        Ok(())
    }

    /// Releases a lock on an inode for a specific owner.
    ///
    /// Returns Ok(()) even if no lock was held.
    pub fn unlock(
        &self,
        inode: InodeId,
        owner_pid: u64,
        start: u64,
        end: u64,
    ) -> Result<(), MetaError> {
        let mut locks = self.locks.write().unwrap();

        if let Some(inode_locks) = locks.get_mut(&inode) {
            inode_locks.retain(|lock| {
                !(lock.owner_pid == owner_pid && lock.start == start && lock.end == end)
            });

            if inode_locks.is_empty() {
                locks.remove(&inode);
            }
        }

        Ok(())
    }

    /// Tests if a lock can be acquired without actually acquiring it.
    ///
    /// Returns None if no conflict (lock would succeed), Some(RangeLock) if conflict.
    pub fn test_lock(
        &self,
        inode: InodeId,
        owner_pid: u64,
        lock_type: LockType,
        start: u64,
        end: u64,
    ) -> Option<RangeLock> {
        let new_lock = RangeLock::new(owner_pid, lock_type, start, end);

        let locks = self.locks.read().unwrap();

        let inode_locks = locks.get(&inode)?;

        for existing_lock in inode_locks {
            if existing_lock.owner_pid == owner_pid {
                continue;
            }
            if existing_lock.conflicts_with(&new_lock) {
                return Some(existing_lock.clone());
            }
        }

        None
    }

    /// Returns all locks for an inode.
    pub fn get_locks(&self, inode: InodeId) -> Vec<RangeLock> {
        let locks = self.locks.read().unwrap();
        locks.get(&inode).cloned().unwrap_or_default()
    }

    /// Releases all locks owned by a specific process across all inodes.
    pub fn release_all_by_owner(&self, owner_pid: u64) {
        let mut locks = self.locks.write().unwrap();

        for inode_locks in locks.values_mut() {
            inode_locks.retain(|lock| lock.owner_pid != owner_pid);
        }

        locks.retain(|_, v| !v.is_empty());
    }

    /// Returns the total number of locks across all inodes.
    pub fn total_lock_count(&self) -> usize {
        let locks = self.locks.read().unwrap();
        locks.values().map(|v| v.len()).sum()
    }

    /// Returns the number of inodes that have at least one lock.
    pub fn locked_inode_count(&self) -> usize {
        let locks = self.locks.read().unwrap();
        locks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inode(n: u64) -> InodeId {
        InodeId::new(n)
    }

    #[test]
    fn test_lock_read_no_conflict() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Read, 0, 99).unwrap();
        mgr.lock(inode(1), 101, LockType::Read, 0, 99).unwrap();

        assert_eq!(mgr.get_locks(inode(1)).len(), 2);
    }

    #[test]
    fn test_lock_write_conflicts_with_read() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Read, 0, 99).unwrap();
        let result = mgr.lock(inode(1), 101, LockType::Write, 0, 99);

        assert!(result.is_err());
    }

    #[test]
    fn test_lock_write_conflicts_with_write() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();
        let result = mgr.lock(inode(1), 101, LockType::Write, 0, 99);

        assert!(result.is_err());
    }

    #[test]
    fn test_lock_non_overlapping_ranges() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Write, 0, 100).unwrap();
        mgr.lock(inode(1), 101, LockType::Write, 101, 200).unwrap();

        assert_eq!(mgr.get_locks(inode(1)).len(), 2);
    }

    #[test]
    fn test_lock_adjacent_ranges_no_conflict() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();
        mgr.lock(inode(1), 101, LockType::Write, 100, 199).unwrap();

        assert_eq!(mgr.get_locks(inode(1)).len(), 2);
    }

    #[test]
    fn test_overlaps_logic() {
        let lock = RangeLock::new(1, LockType::Write, 0, 100);

        assert!(lock.overlaps(0, 100));
        assert!(lock.overlaps(50, 150));
        assert!(lock.overlaps(0, 50));
        assert!(lock.overlaps(50, 200));
        assert!(!lock.overlaps(101, 200));
        assert!(!lock.overlaps(200, 300));
    }

    #[test]
    fn test_conflicts_with_logic() {
        let read_lock = RangeLock::new(1, LockType::Read, 0, 100);
        let write_lock = RangeLock::new(1, LockType::Write, 0, 100);
        let other_read = RangeLock::new(2, LockType::Read, 0, 100);
        let other_write = RangeLock::new(2, LockType::Write, 0, 100);

        assert!(!read_lock.conflicts_with(&other_read));
        assert!(read_lock.conflicts_with(&other_write));
        assert!(write_lock.conflicts_with(&other_read));
        assert!(write_lock.conflicts_with(&other_write));
    }

    #[test]
    fn test_unlock_removes_lock() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();
        mgr.unlock(inode(1), 100, 0, 99).unwrap();

        assert!(mgr.get_locks(inode(1)).is_empty());
    }

    #[test]
    fn test_unlock_nonexistent() {
        let mgr = RangeLockManager::new();

        let result = mgr.unlock(inode(1), 100, 0, 99);

        assert!(result.is_ok());
    }

    #[test]
    fn test_test_lock_no_conflict() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();
        let result = mgr.test_lock(inode(1), 101, LockType::Read, 100, 199);

        assert!(result.is_none());
    }

    #[test]
    fn test_test_lock_conflict() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();
        let result = mgr.test_lock(inode(1), 101, LockType::Write, 50, 150);

        assert!(result.is_some());
        assert_eq!(result.unwrap().owner_pid, 100);
    }

    #[test]
    fn test_release_all_by_owner() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();
        mgr.lock(inode(2), 100, LockType::Write, 0, 99).unwrap();

        mgr.release_all_by_owner(100);

        assert!(mgr.get_locks(inode(1)).is_empty());
        assert!(mgr.get_locks(inode(2)).is_empty());
    }

    #[test]
    fn test_total_lock_count() {
        let mgr = RangeLockManager::new();

        assert_eq!(mgr.total_lock_count(), 0);

        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();
        mgr.lock(inode(1), 101, LockType::Read, 100, 199).unwrap();

        assert_eq!(mgr.total_lock_count(), 2);
    }

    #[test]
    fn test_locked_inode_count() {
        let mgr = RangeLockManager::new();

        assert_eq!(mgr.locked_inode_count(), 0);

        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();
        mgr.lock(inode(2), 100, LockType::Write, 0, 99).unwrap();

        assert_eq!(mgr.locked_inode_count(), 2);
    }

    #[test]
    fn test_lock_upgrade_same_owner() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Read, 0, 99).unwrap();
        mgr.lock(inode(1), 100, LockType::Write, 0, 99).unwrap();

        let locks = mgr.get_locks(inode(1));
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].lock_type, LockType::Write);
    }

    #[test]
    fn test_lock_eof_range() {
        let mgr = RangeLockManager::new();

        mgr.lock(inode(1), 100, LockType::Write, 0, u64::MAX)
            .unwrap();
        let result = mgr.lock(inode(1), 101, LockType::Write, 50, 150);

        assert!(result.is_err());
    }
}
