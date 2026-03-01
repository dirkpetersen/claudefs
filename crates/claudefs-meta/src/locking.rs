//! Distributed locking for POSIX mandatory locking.
//!
//! Per-inode read/write locks for fcntl(), per-directory locks for
//! atomic rename/link operations (per docs/metadata.md).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::types::{InodeId, MetaError, NodeId};

/// Type of POSIX file lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockType {
    /// Shared lock, multiple readers allowed.
    Read,
    /// Exclusive lock, only one writer allowed.
    Write,
}

/// Represents an active lock on an inode.
#[derive(Clone, Debug)]
pub struct LockEntry {
    /// Inode this lock is held on.
    pub ino: InodeId,
    /// Type of lock (read or write).
    pub lock_type: LockType,
    /// Node that holds this lock.
    pub holder: NodeId,
    /// Unique identifier for this lock instance.
    pub lock_id: u64,
}

/// Manages per-inode POSIX read/write locks.
pub struct LockManager {
    locks: Arc<RwLock<HashMap<InodeId, Vec<LockEntry>>>>,
    next_lock_id: Arc<RwLock<u64>>,
}

impl LockManager {
    /// Creates a new LockManager.
    pub fn new() -> Self {
        Self {
            locks: Arc::new(RwLock::new(HashMap::new())),
            next_lock_id: Arc::new(RwLock::new(1)),
        }
    }

    /// Acquires a lock on an inode.
    pub fn acquire(
        &self,
        ino: InodeId,
        lock_type: LockType,
        holder: NodeId,
    ) -> Result<u64, MetaError> {
        let mut locks = self
            .locks
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let inode_locks = locks.entry(ino).or_default();

        match lock_type {
            LockType::Read => {
                if inode_locks.iter().any(|l| l.lock_type == LockType::Write) {
                    return Err(MetaError::PermissionDenied);
                }
            }
            LockType::Write => {
                if !inode_locks.is_empty() {
                    return Err(MetaError::PermissionDenied);
                }
            }
        }

        let mut next_id = self
            .next_lock_id
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let lock_id = *next_id;
        *next_id += 1;

        inode_locks.push(LockEntry {
            ino,
            lock_type,
            holder,
            lock_id,
        });

        Ok(lock_id)
    }

    /// Releases a lock by its ID.
    pub fn release(&self, lock_id: u64) -> Result<(), MetaError> {
        let mut locks = self
            .locks
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;

        for inode_locks in locks.values_mut() {
            if let Some(pos) = inode_locks.iter().position(|l| l.lock_id == lock_id) {
                inode_locks.remove(pos);
                return Ok(());
            }
        }

        Ok(())
    }

    /// Releases all locks held by a node.
    pub fn release_all_for_node(&self, node: NodeId) -> Result<usize, MetaError> {
        let mut locks = self
            .locks
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let mut released = 0;

        for inode_locks in locks.values_mut() {
            let before = inode_locks.len();
            inode_locks.retain(|l| l.holder != node);
            released += before - inode_locks.len();
        }

        locks.retain(|_, v| !v.is_empty());

        Ok(released)
    }

    /// Returns true if the inode has any active locks.
    pub fn is_locked(&self, ino: InodeId) -> Result<bool, MetaError> {
        let locks = self
            .locks
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(locks.get(&ino).is_some_and(|v| !v.is_empty()))
    }

    /// Returns all locks held on an inode.
    pub fn locks_on(&self, ino: InodeId) -> Result<Vec<LockEntry>, MetaError> {
        let locks = self
            .locks
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(locks.get(&ino).cloned().unwrap_or_default())
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

    fn make_manager() -> LockManager {
        LockManager::new()
    }

    #[test]
    fn test_acquire_read_lock() {
        let mgr = make_manager();
        let ino = InodeId::new(42);
        let lock_id = mgr.acquire(ino, LockType::Read, NodeId::new(1)).unwrap();
        assert!(lock_id > 0);
        assert!(mgr.is_locked(ino).unwrap());
    }

    #[test]
    fn test_multiple_read_locks() {
        let mgr = make_manager();
        let ino = InodeId::new(42);
        mgr.acquire(ino, LockType::Read, NodeId::new(1)).unwrap();
        mgr.acquire(ino, LockType::Read, NodeId::new(2)).unwrap();

        let locks = mgr.locks_on(ino).unwrap();
        assert_eq!(locks.len(), 2);
    }

    #[test]
    fn test_write_lock_exclusive() {
        let mgr = make_manager();
        let ino = InodeId::new(42);
        mgr.acquire(ino, LockType::Write, NodeId::new(1)).unwrap();

        match mgr.acquire(ino, LockType::Write, NodeId::new(2)) {
            Err(MetaError::PermissionDenied) => {}
            other => panic!("expected PermissionDenied, got {:?}", other),
        }

        match mgr.acquire(ino, LockType::Read, NodeId::new(2)) {
            Err(MetaError::PermissionDenied) => {}
            other => panic!("expected PermissionDenied, got {:?}", other),
        }
    }

    #[test]
    fn test_write_lock_blocked_by_read() {
        let mgr = make_manager();
        let ino = InodeId::new(42);
        mgr.acquire(ino, LockType::Read, NodeId::new(1)).unwrap();

        match mgr.acquire(ino, LockType::Write, NodeId::new(2)) {
            Err(MetaError::PermissionDenied) => {}
            other => panic!("expected PermissionDenied, got {:?}", other),
        }
    }

    #[test]
    fn test_release_lock() {
        let mgr = make_manager();
        let ino = InodeId::new(42);
        let lock_id = mgr.acquire(ino, LockType::Write, NodeId::new(1)).unwrap();
        mgr.release(lock_id).unwrap();

        assert!(!mgr.is_locked(ino).unwrap());

        mgr.acquire(ino, LockType::Write, NodeId::new(2)).unwrap();
    }

    #[test]
    fn test_release_all_for_node() {
        let mgr = make_manager();
        let node1 = NodeId::new(1);
        mgr.acquire(InodeId::new(1), LockType::Read, node1).unwrap();
        mgr.acquire(InodeId::new(2), LockType::Read, node1).unwrap();
        mgr.acquire(InodeId::new(3), LockType::Write, node1)
            .unwrap();
        mgr.acquire(InodeId::new(1), LockType::Read, NodeId::new(2))
            .unwrap();

        let released = mgr.release_all_for_node(node1).unwrap();
        assert_eq!(released, 3);

        assert!(mgr.is_locked(InodeId::new(1)).unwrap());
        assert!(!mgr.is_locked(InodeId::new(2)).unwrap());
        assert!(!mgr.is_locked(InodeId::new(3)).unwrap());
    }

    #[test]
    fn test_independent_inodes() {
        let mgr = make_manager();
        mgr.acquire(InodeId::new(1), LockType::Write, NodeId::new(1))
            .unwrap();
        mgr.acquire(InodeId::new(2), LockType::Write, NodeId::new(1))
            .unwrap();
    }

    #[test]
    fn test_release_enables_new_lock() {
        let mgr = make_manager();
        let ino = InodeId::new(42);
        let lock_id = mgr.acquire(ino, LockType::Write, NodeId::new(1)).unwrap();

        // Write lock blocks others
        assert!(mgr
            .acquire(ino, LockType::Write, NodeId::new(2))
            .is_err());

        // Release the lock
        mgr.release(lock_id).unwrap();

        // Now another node can acquire
        mgr.acquire(ino, LockType::Write, NodeId::new(2)).unwrap();
    }

    #[test]
    fn test_release_nonexistent_lock() {
        let mgr = make_manager();
        // Releasing a non-existent lock_id should succeed silently
        mgr.release(99999).unwrap();
    }

    #[test]
    fn test_locks_on_empty() {
        let mgr = make_manager();
        let locks = mgr.locks_on(InodeId::new(42)).unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn test_is_locked_after_all_released() {
        let mgr = make_manager();
        let ino = InodeId::new(42);
        let id1 = mgr.acquire(ino, LockType::Read, NodeId::new(1)).unwrap();
        let id2 = mgr.acquire(ino, LockType::Read, NodeId::new(2)).unwrap();

        assert!(mgr.is_locked(ino).unwrap());

        mgr.release(id1).unwrap();
        assert!(mgr.is_locked(ino).unwrap()); // Still locked by node 2

        mgr.release(id2).unwrap();
        assert!(!mgr.is_locked(ino).unwrap()); // Now fully unlocked
    }

    #[test]
    fn test_lock_ids_unique() {
        let mgr = make_manager();
        let id1 = mgr
            .acquire(InodeId::new(1), LockType::Read, NodeId::new(1))
            .unwrap();
        let id2 = mgr
            .acquire(InodeId::new(1), LockType::Read, NodeId::new(2))
            .unwrap();
        let id3 = mgr
            .acquire(InodeId::new(2), LockType::Write, NodeId::new(3))
            .unwrap();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_release_all_for_node_returns_zero() {
        let mgr = make_manager();
        let released = mgr.release_all_for_node(NodeId::new(999)).unwrap();
        assert_eq!(released, 0);
    }
}
