//! BSD-style file locking (flock) implementation.
//!
//! This module provides whole-file advisory locking semantics compatible with
//! the POSIX `flock()` system call. Locks are associated with file descriptors
//! and process IDs, allowing proper cleanup on file close or process exit.
//!
//! # Lock Types
//!
//! - **Shared** (`LOCK_SH`): Multiple readers can hold a shared lock simultaneously.
//! - **Exclusive** (`LOCK_EX`): Only one exclusive lock per file; blocks all others.
//! - **Unlock** (`LOCK_UN`): Releases the lock held by this fd/pid pair.
//!
//! # Lock Upgrades/Downgrades
//!
//! A process holding a shared lock may upgrade to exclusive (fails if other
//! shared holders exist). A process holding exclusive may downgrade to shared
//! (always succeeds). These are atomic operations.

use crate::inode::InodeId;
use std::collections::HashMap;

/// Type of flock lock operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlockType {
    /// Shared (read) lock - multiple holders allowed on same inode.
    Shared,
    /// Exclusive (write) lock - single holder allowed per inode.
    Exclusive,
    /// Unlock operation - releases the lock for this fd.
    Unlock,
}

/// Handle representing a flock lock request.
#[derive(Debug, Clone)]
pub struct FlockHandle {
    /// File descriptor requesting the lock.
    pub fd: u64,
    /// Inode number of the file being locked.
    pub ino: InodeId,
    /// Process ID of the lock requester.
    pub pid: u32,
    /// Type of lock requested (shared, exclusive, or unlock).
    pub lock_type: FlockType,
    /// If true, return EWOULDBLOCK instead of blocking on conflict.
    pub nonblocking: bool,
}

impl FlockHandle {
    /// Creates a new flock handle with the given parameters.
    pub fn new(fd: u64, ino: InodeId, pid: u32, lock_type: FlockType, nonblocking: bool) -> Self {
        Self {
            fd,
            ino,
            pid,
            lock_type,
            nonblocking,
        }
    }

    /// Returns true if this lock request should block on conflict.
    pub fn is_blocking(&self) -> bool {
        !self.nonblocking
    }

    /// Returns true if this is a shared lock request.
    pub fn is_shared(&self) -> bool {
        matches!(self.lock_type, FlockType::Shared)
    }

    /// Returns true if this is an exclusive lock request.
    pub fn is_exclusive(&self) -> bool {
        matches!(self.lock_type, FlockType::Exclusive)
    }
}

/// Result of a lock acquisition attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlockConflict {
    /// No conflict; lock was acquired successfully.
    None,
    /// Lock would block; contains PID of the conflicting lock holder.
    WouldBlock { holder_pid: u32 },
    /// Deadlock detected (not yet implemented).
    Deadlock,
}

/// An active flock lock entry in the registry.
#[derive(Debug, Clone)]
pub struct FlockEntry {
    /// File descriptor holding this lock.
    pub fd: u64,
    /// Inode number of the locked file.
    pub ino: InodeId,
    /// Process ID owning this lock.
    pub pid: u32,
    /// Type of lock (shared or exclusive).
    pub lock_type: FlockType,
}

/// Registry tracking all active flock locks.
///
/// Maintains locks keyed by (fd, inode) for O(1) lookup and a secondary
/// index by inode for efficient conflict checking.
pub struct FlockRegistry {
    locks: HashMap<(u64, InodeId), FlockEntry>,
    by_inode: HashMap<InodeId, Vec<(u64, InodeId)>>,
}

impl Default for FlockRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FlockRegistry {
    /// Creates an empty flock registry.
    pub fn new() -> Self {
        Self {
            locks: HashMap::new(),
            by_inode: HashMap::new(),
        }
    }

    /// Attempts to acquire a lock, returning conflict status.
    ///
    /// Handles upgrade (shared -> exclusive) and downgrade (exclusive -> shared)
    /// atomically. An unlock request removes the lock if present.
    pub fn try_acquire(&mut self, handle: FlockHandle) -> FlockConflict {
        let key = (handle.fd, handle.ino);
        let new_type = handle.lock_type;

        if new_type == FlockType::Unlock {
            self.locks.remove(&key);
            self.remove_from_inode_index(&key);
            return FlockConflict::None;
        }

        if let Some(existing) = self.locks.get(&key) {
            if existing.pid == handle.pid {
                match (existing.lock_type, new_type) {
                    (FlockType::Shared, FlockType::Exclusive) => {
                        let others: Vec<_> = self
                            .by_inode
                            .get(&handle.ino)
                            .map(|v| {
                                v.iter()
                                    .filter(|(fd, ino)| {
                                        if let Some(e) = self.locks.get(&(*fd, *ino)) {
                                            e.pid != handle.pid
                                                && matches!(e.lock_type, FlockType::Shared)
                                        } else {
                                            false
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !others.is_empty() {
                            return FlockConflict::WouldBlock { holder_pid: 0 };
                        }
                        let entry = FlockEntry {
                            fd: handle.fd,
                            ino: handle.ino,
                            pid: handle.pid,
                            lock_type: new_type,
                        };
                        self.locks.insert(key, entry);
                        return FlockConflict::None;
                    }
                    (FlockType::Exclusive, FlockType::Shared) => {
                        let entry = FlockEntry {
                            fd: handle.fd,
                            ino: handle.ino,
                            pid: handle.pid,
                            lock_type: new_type,
                        };
                        self.locks.insert(key, entry);
                        return FlockConflict::None;
                    }
                    _ => {
                        let entry = FlockEntry {
                            fd: handle.fd,
                            ino: handle.ino,
                            pid: handle.pid,
                            lock_type: new_type,
                        };
                        self.locks.insert(key, entry);
                        return FlockConflict::None;
                    }
                }
            }
        }

        for (k, entry) in &self.locks {
            let (_, ino) = *k;
            if ino == handle.ino && entry.ino == handle.ino {
                match (&entry.lock_type, new_type) {
                    (FlockType::Shared, FlockType::Shared) => continue,
                    (FlockType::Shared, FlockType::Exclusive) => {
                        return FlockConflict::WouldBlock {
                            holder_pid: entry.pid,
                        };
                    }
                    (FlockType::Exclusive, _) => {
                        return FlockConflict::WouldBlock {
                            holder_pid: entry.pid,
                        };
                    }
                    _ => {}
                }
            }
        }

        let entry = FlockEntry {
            fd: handle.fd,
            ino: handle.ino,
            pid: handle.pid,
            lock_type: new_type,
        };
        self.locks.insert(key, entry);
        self.by_inode.entry(handle.ino).or_default().push(key);

        FlockConflict::None
    }

    fn remove_from_inode_index(&mut self, key: &(u64, InodeId)) {
        if let Some(entries) = self.by_inode.get_mut(&key.1) {
            entries.retain(|k| k != key);
            if entries.is_empty() {
                self.by_inode.remove(&key.1);
            }
        }
    }

    /// Releases the lock for the given fd and inode.
    pub fn release(&mut self, fd: u64, ino: InodeId) {
        let key = (fd, ino);
        self.locks.remove(&key);
        self.remove_from_inode_index(&key);
    }

    /// Releases all locks held by the given process ID.
    ///
    /// Called when a process exits or closes all its file descriptors.
    pub fn release_all_for_pid(&mut self, pid: u32) {
        let to_remove: Vec<_> = self
            .locks
            .iter()
            .filter(|(_, e)| e.pid == pid)
            .map(|(k, _)| *k)
            .collect();

        for key in to_remove {
            self.locks.remove(&key);
            self.remove_from_inode_index(&key);
        }
    }

    /// Returns true if a lock exists for the given fd and inode.
    pub fn has_lock(&self, fd: u64, ino: InodeId) -> bool {
        self.locks.contains_key(&(fd, ino))
    }

    /// Returns the lock type for the given fd and inode, if any.
    pub fn lock_type_for(&self, fd: u64, ino: InodeId) -> Option<FlockType> {
        self.locks.get(&(fd, ino)).map(|e| e.lock_type)
    }

    /// Returns the number of lock holders for the given inode.
    pub fn holder_count(&self, ino: InodeId) -> usize {
        self.by_inode.get(&ino).map(|v| v.len()).unwrap_or(0)
    }
}

/// Statistics for flock operations.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlockStats {
    /// Total successful lock acquisitions.
    pub acquires: u64,
    /// Total lock releases.
    pub releases: u64,
    /// Total conflicts encountered.
    pub conflicts: u64,
    /// Total lock upgrades (shared -> exclusive).
    pub upgrades: u64,
    /// Total lock downgrades (exclusive -> shared).
    pub downgrades: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_plus_shared_succeeds() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Shared, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Shared, false);

        assert_eq!(registry.try_acquire(h1), FlockConflict::None);
        assert_eq!(registry.try_acquire(h2), FlockConflict::None);
    }

    #[test]
    fn test_exclusive_blocks_shared() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Shared, false);

        assert_eq!(registry.try_acquire(h1), FlockConflict::None);
        assert!(matches!(
            registry.try_acquire(h2),
            FlockConflict::WouldBlock { .. }
        ));
    }

    #[test]
    fn test_exclusive_blocks_exclusive() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Exclusive, false);

        assert_eq!(registry.try_acquire(h1), FlockConflict::None);
        assert!(matches!(
            registry.try_acquire(h2),
            FlockConflict::WouldBlock { .. }
        ));
    }

    #[test]
    fn test_shared_blocks_exclusive() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Shared, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Exclusive, false);

        assert_eq!(registry.try_acquire(h1), FlockConflict::None);
        assert!(matches!(
            registry.try_acquire(h2),
            FlockConflict::WouldBlock { .. }
        ));
    }

    #[test]
    fn test_acquire_returns_wouldblock_for_nonblocking() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Exclusive, true);

        assert_eq!(registry.try_acquire(h1), FlockConflict::None);
        assert!(matches!(
            registry.try_acquire(h2),
            FlockConflict::WouldBlock { .. }
        ));
    }

    #[test]
    fn test_release_removes_lock() {
        let mut registry = FlockRegistry::new();
        let h = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);

        assert_eq!(registry.try_acquire(h), FlockConflict::None);
        assert!(registry.has_lock(1, 100));

        registry.release(1, 100);
        assert!(!registry.has_lock(1, 100));
    }

    #[test]
    fn test_has_lock_after_acquire_release() {
        let mut registry = FlockRegistry::new();
        let h = FlockHandle::new(1, 100, 1, FlockType::Shared, false);

        assert!(!registry.has_lock(1, 100));
        registry.try_acquire(h);
        assert!(registry.has_lock(1, 100));
        registry.release(1, 100);
        assert!(!registry.has_lock(1, 100));
    }

    #[test]
    fn test_upgrade_shared_to_exclusive_when_alone() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Shared, false);

        assert_eq!(registry.try_acquire(h1), FlockConflict::None);

        let h1_upgrade = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);
        assert_eq!(registry.try_acquire(h1_upgrade), FlockConflict::None);
    }

    #[test]
    fn test_upgrade_blocked_when_another_shared_holder() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Shared, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Shared, false);

        assert_eq!(registry.try_acquire(h1), FlockConflict::None);
        assert_eq!(registry.try_acquire(h2), FlockConflict::None);

        let h1_upgrade = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);
        assert!(matches!(
            registry.try_acquire(h1_upgrade),
            FlockConflict::WouldBlock { .. }
        ));
    }

    #[test]
    fn test_downgrade_exclusive_to_shared() {
        let mut registry = FlockRegistry::new();
        let h = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);

        assert_eq!(registry.try_acquire(h), FlockConflict::None);

        let h_downgrade = FlockHandle::new(1, 100, 1, FlockType::Shared, false);
        assert_eq!(registry.try_acquire(h_downgrade), FlockConflict::None);
    }

    #[test]
    fn test_release_all_for_pid() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Shared, false);
        let h2 = FlockHandle::new(2, 200, 1, FlockType::Shared, false);

        registry.try_acquire(h1);
        registry.try_acquire(h2);

        assert!(registry.has_lock(1, 100));
        assert!(registry.has_lock(2, 200));

        registry.release_all_for_pid(1);

        assert!(!registry.has_lock(1, 100));
        assert!(!registry.has_lock(2, 200));
    }

    #[test]
    fn test_holder_count() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Shared, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Shared, false);

        assert_eq!(registry.holder_count(100), 0);

        registry.try_acquire(h1);
        assert_eq!(registry.holder_count(100), 1);

        registry.try_acquire(h2);
        assert_eq!(registry.holder_count(100), 2);
    }

    #[test]
    fn test_lock_type_for() {
        let mut registry = FlockRegistry::new();
        let h = FlockHandle::new(1, 100, 1, FlockType::Shared, false);

        assert_eq!(registry.lock_type_for(1, 100), None);

        registry.try_acquire(h);
        assert_eq!(registry.lock_type_for(1, 100), Some(FlockType::Shared));

        registry.release(1, 100);
        assert_eq!(registry.lock_type_for(1, 100), None);
    }

    #[test]
    fn test_unlock() {
        let mut registry = FlockRegistry::new();
        let h = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);

        registry.try_acquire(h);
        assert!(registry.has_lock(1, 100));

        let unlock = FlockHandle::new(1, 100, 1, FlockType::Unlock, false);
        registry.try_acquire(unlock);
        assert!(!registry.has_lock(1, 100));
    }

    #[test]
    fn test_different_inodes_independent() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);
        let h2 = FlockHandle::new(2, 200, 2, FlockType::Exclusive, false);

        assert_eq!(registry.try_acquire(h1), FlockConflict::None);
        assert_eq!(registry.try_acquire(h2), FlockConflict::None);
    }
}
