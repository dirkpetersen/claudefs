//! POSIX file locking support for ClaudeFS FUSE client.
//!
//! Implements advisory file locking (fcntl-style) for POSIX compliance.
//! Lock state is tracked in memory; distributed locking via the metadata
//! service (A2) will be integrated in Phase 6.

use crate::error::Result;
use crate::inode::InodeId;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockType {
    Shared,
    Exclusive,
    Unlock,
}

#[derive(Debug, Clone)]
pub struct LockRecord {
    pub lock_type: LockType,
    pub owner: u64,
    pub pid: u32,
    pub start: u64,
    pub end: u64,
}

pub struct LockManager {
    locks: HashMap<InodeId, Vec<LockRecord>>,
}

fn ranges_overlap(s1: u64, e1: u64, s2: u64, e2: u64) -> bool {
    s1 < e2 && s2 < e1
}

impl LockManager {
    pub fn new() -> Self {
        Self {
            locks: HashMap::new(),
        }
    }

    pub fn try_lock(&mut self, ino: InodeId, req: LockRecord) -> Result<bool> {
        if req.lock_type == LockType::Unlock {
            self.unlock(ino, req.owner);
            return Ok(true);
        }

        if self.has_conflicting_lock_with_owner(
            ino,
            req.lock_type,
            req.start,
            req.end,
            Some(req.owner),
        ) {
            return Ok(false);
        }

        let locks = self.locks.entry(ino).or_default();
        locks.push(req);
        Ok(true)
    }

    pub fn unlock(&mut self, ino: InodeId, owner: u64) {
        if let Some(locks) = self.locks.get_mut(&ino) {
            locks.retain(|l| l.owner != owner);
        }
    }

    pub fn has_conflicting_lock(
        &self,
        ino: InodeId,
        lock_type: LockType,
        start: u64,
        end: u64,
    ) -> bool {
        self.has_conflicting_lock_with_owner(ino, lock_type, start, end, None)
    }

    pub fn has_conflicting_lock_with_owner(
        &self,
        ino: InodeId,
        lock_type: LockType,
        start: u64,
        end: u64,
        exclude_owner: Option<u64>,
    ) -> bool {
        let Some(locks) = self.locks.get(&ino) else {
            return false;
        };

        for lock in locks {
            if let Some(exclude) = exclude_owner {
                if lock.owner == exclude {
                    continue;
                }
            }
            if ranges_overlap(start, end, lock.start, lock.end) {
                if lock_type == LockType::Exclusive {
                    return true;
                }
                if lock.lock_type == LockType::Exclusive {
                    return true;
                }
            }
        }
        false
    }

    pub fn lock_count(&self, ino: InodeId) -> usize {
        self.locks.get(&ino).map(|l| l.len()).unwrap_or(0)
    }

    pub fn total_locks(&self) -> usize {
        self.locks.values().map(|l| l.len()).sum()
    }

    pub fn clear_inode(&mut self, ino: InodeId) {
        self.locks.remove(&ino);
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exclusive_lock_granted_on_empty_inode() {
        let mut mgr = LockManager::new();
        let lock = LockRecord {
            lock_type: LockType::Exclusive,
            owner: 1,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        let result = mgr.try_lock(100, lock).unwrap();
        assert!(result);
    }

    #[test]
    fn test_shared_locks_can_coexist() {
        let mut mgr = LockManager::new();
        let lock1 = LockRecord {
            lock_type: LockType::Shared,
            owner: 1,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        let lock2 = LockRecord {
            lock_type: LockType::Shared,
            owner: 2,
            pid: 101,
            start: 0,
            end: u64::MAX,
        };
        assert!(mgr.try_lock(100, lock1).unwrap());
        assert!(mgr.try_lock(100, lock2).unwrap());
    }

    #[test]
    fn test_exclusive_lock_blocked_by_existing_shared_lock() {
        let mut mgr = LockManager::new();
        let shared = LockRecord {
            lock_type: LockType::Shared,
            owner: 1,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        let exclusive = LockRecord {
            lock_type: LockType::Exclusive,
            owner: 2,
            pid: 101,
            start: 0,
            end: u64::MAX,
        };
        assert!(mgr.try_lock(100, shared).unwrap());
        assert!(!mgr.try_lock(100, exclusive).unwrap());
    }

    #[test]
    fn test_exclusive_lock_blocked_by_existing_exclusive_lock() {
        let mut mgr = LockManager::new();
        let lock1 = LockRecord {
            lock_type: LockType::Exclusive,
            owner: 1,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        let lock2 = LockRecord {
            lock_type: LockType::Exclusive,
            owner: 2,
            pid: 101,
            start: 0,
            end: u64::MAX,
        };
        assert!(mgr.try_lock(100, lock1).unwrap());
        assert!(!mgr.try_lock(100, lock2).unwrap());
    }

    #[test]
    fn test_unlock_removes_lock() {
        let mut mgr = LockManager::new();
        let lock = LockRecord {
            lock_type: LockType::Exclusive,
            owner: 1,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        assert!(mgr.try_lock(100, lock).unwrap());
        assert_eq!(mgr.lock_count(100), 1);

        let unlock = LockRecord {
            lock_type: LockType::Unlock,
            owner: 1,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        mgr.try_lock(100, unlock).unwrap();
        assert_eq!(mgr.lock_count(100), 0);
    }

    #[test]
    fn test_lock_count_returns_correct_count() {
        let mut mgr = LockManager::new();
        assert_eq!(mgr.lock_count(100), 0);

        let lock1 = LockRecord {
            lock_type: LockType::Shared,
            owner: 1,
            pid: 100,
            start: 0,
            end: 100,
        };
        mgr.try_lock(100, lock1).unwrap();
        assert_eq!(mgr.lock_count(100), 1);

        let lock2 = LockRecord {
            lock_type: LockType::Shared,
            owner: 2,
            pid: 101,
            start: 200,
            end: 300,
        };
        mgr.try_lock(100, lock2).unwrap();
        assert_eq!(mgr.lock_count(100), 2);
    }

    #[test]
    fn test_total_locks_sums_across_inodes() {
        let mut mgr = LockManager::new();
        assert_eq!(mgr.total_locks(), 0);

        let lock1 = LockRecord {
            lock_type: LockType::Shared,
            owner: 1,
            pid: 100,
            start: 0,
            end: 100,
        };
        let lock2 = LockRecord {
            lock_type: LockType::Shared,
            owner: 2,
            pid: 101,
            start: 0,
            end: 100,
        };
        mgr.try_lock(100, lock1).unwrap();
        mgr.try_lock(200, lock2).unwrap();
        assert_eq!(mgr.total_locks(), 2);
    }

    #[test]
    fn test_ranges_overlap_detects_overlap() {
        assert!(ranges_overlap(0, 100, 50, 150));
        assert!(ranges_overlap(50, 150, 0, 100));
        assert!(ranges_overlap(0, 100, 0, 100));
    }

    #[test]
    fn test_ranges_overlap_non_overlapping() {
        assert!(!ranges_overlap(0, 100, 200, 300));
        assert!(!ranges_overlap(200, 300, 0, 100));
        assert!(!ranges_overlap(100, 200, 0, 100));
        assert!(!ranges_overlap(0, 100, 100, 200));
    }

    #[test]
    fn test_clear_inode_removes_all_locks() {
        let mut mgr = LockManager::new();
        let lock = LockRecord {
            lock_type: LockType::Shared,
            owner: 1,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        mgr.try_lock(100, lock).unwrap();
        assert_eq!(mgr.lock_count(100), 1);

        mgr.clear_inode(100);
        assert_eq!(mgr.lock_count(100), 0);
    }

    #[test]
    fn test_has_conflicting_lock_returns_false_for_same_owner() {
        let mut mgr = LockManager::new();
        let lock = LockRecord {
            lock_type: LockType::Exclusive,
            owner: 1,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        mgr.try_lock(100, lock).unwrap();

        assert!(!mgr.has_conflicting_lock_with_owner(100, LockType::Shared, 0, u64::MAX, Some(1)));
    }

    #[test]
    fn test_try_lock_unlock_always_returns_true() {
        let mut mgr = LockManager::new();
        let unlock = LockRecord {
            lock_type: LockType::Unlock,
            owner: 999,
            pid: 100,
            start: 0,
            end: u64::MAX,
        };
        let result = mgr.try_lock(100, unlock).unwrap();
        assert!(result);
    }
}
