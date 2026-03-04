//! Lazy deletion tracking for POSIX unlink-while-open semantics.
//!
//! POSIX requires that when `unlink()` is called on a file that still has open file
//! descriptors, the directory entry is removed immediately but the data must persist
//! until the last fd is closed. This module tracks "orphaned" inodes (unlinked but
//! still open) and triggers GC when the last fd closes.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::kvstore::KvStore;
use crate::types::{InodeId, MetaError, Timestamp};

const LAZY_DELETE_PREFIX: &[u8] = b"ld:";

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
/// - All file descriptors are closed (open_fd_count == 0) -> ready for GC
/// - Or when server restart purges all entries with open_fd_count == 0
pub struct LazyDeleteStore {
    kv: Arc<dyn KvStore>,
}

impl LazyDeleteStore {
    /// Creates a new LazyDeleteStore.
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    fn make_key(ino: InodeId) -> Vec<u8> {
        let mut key = LAZY_DELETE_PREFIX.to_vec();
        key.extend_from_slice(&ino.as_u64().to_be_bytes());
        key
    }

    /// Marks an inode as lazily deleted (unlinked but still open).
    ///
    /// Returns `MetaError::AlreadyExists` if already in lazy-delete state.
    pub fn mark_deleted(
        &self,
        ino: InodeId,
        original_path: String,
        open_fd_count: u32,
    ) -> Result<(), MetaError> {
        let key = Self::make_key(ino);

        if self.kv.contains_key(&key)? {
            return Err(MetaError::AlreadyExists(format!(
                "inode {} already in lazy-delete state",
                ino
            )));
        }

        let entry = LazyDeleteEntry {
            ino,
            unlinked_at: Timestamp::now(),
            original_path,
            open_fd_count,
        };

        let value = bincode::serialize(&entry).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key, value)?;

        debug!(
            "marked inode {} for lazy deletion with {} open fds",
            ino, open_fd_count
        );
        Ok(())
    }

    /// Increments the open fd count for a lazy-deleted inode.
    ///
    /// Returns `MetaError::NotFound` if inode is not in lazy-delete state.
    pub fn inc_fd_count(&self, ino: InodeId) -> Result<(), MetaError> {
        let key = Self::make_key(ino);

        let value = self.kv.get(&key)?.ok_or_else(|| {
            MetaError::NotFound(format!("inode {} not in lazy-delete state", ino))
        })?;

        let mut entry: LazyDeleteEntry =
            bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))?;

        entry.open_fd_count += 1;

        let new_value =
            bincode::serialize(&entry).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key, new_value)?;

        debug!(
            "incremented fd count for inode {} to {}",
            ino, entry.open_fd_count
        );
        Ok(())
    }

    /// Decrements the open fd count for a lazy-deleted inode.
    ///
    /// Returns `Ok(true)` if the fd count reached 0 (inode ready for GC).
    /// Returns `Ok(false)` if there are still open fds.
    /// Returns `MetaError::NotFound` if inode is not in lazy-delete state.
    pub fn dec_fd_count(&self, ino: InodeId) -> Result<bool, MetaError> {
        let key = Self::make_key(ino);

        let value = self.kv.get(&key)?.ok_or_else(|| {
            MetaError::NotFound(format!("inode {} not in lazy-delete state", ino))
        })?;

        let mut entry: LazyDeleteEntry =
            bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))?;

        if entry.open_fd_count == 0 {
            return Err(MetaError::InvalidArgument(format!(
                "inode {} fd count already at 0",
                ino
            )));
        }

        entry.open_fd_count -= 1;

        let new_value =
            bincode::serialize(&entry).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key, new_value)?;

        let ready = entry.open_fd_count == 0;
        debug!(
            "decremented fd count for inode {} to {} (ready for GC: {})",
            ino, entry.open_fd_count, ready
        );
        Ok(ready)
    }

    /// Gets the lazy-delete entry for an inode.
    ///
    /// Returns `None` if the inode is not in lazy-delete state.
    pub fn get_entry(&self, ino: InodeId) -> Result<Option<LazyDeleteEntry>, MetaError> {
        let key = Self::make_key(ino);

        let value = self.kv.get(&key)?;

        match value {
            Some(v) => {
                let entry: LazyDeleteEntry =
                    bincode::deserialize(&v).map_err(|e| MetaError::KvError(e.to_string()))?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    /// Removes an inode from lazy-delete state.
    ///
    /// Called after the inode has been garbage collected.
    pub fn remove_entry(&self, ino: InodeId) -> Result<(), MetaError> {
        let key = Self::make_key(ino);
        self.kv.delete(&key)?;
        debug!("removed inode {} from lazy-delete state", ino);
        Ok(())
    }

    /// Returns all inodes ready for GC (open_fd_count == 0).
    pub fn ready_for_gc(&self) -> Result<Vec<LazyDeleteEntry>, MetaError> {
        let entries = self.kv.scan_prefix(LAZY_DELETE_PREFIX)?;

        let mut ready = Vec::new();
        for (_, value) in entries {
            let entry: LazyDeleteEntry =
                bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))?;
            if entry.open_fd_count == 0 {
                ready.push(entry);
            }
        }

        Ok(ready)
    }

    /// Returns all lazy-deleted inodes (for monitoring).
    pub fn list_all(&self) -> Result<Vec<LazyDeleteEntry>, MetaError> {
        let entries = self.kv.scan_prefix(LAZY_DELETE_PREFIX)?;

        let mut result = Vec::new();
        for (_, value) in entries {
            let entry: LazyDeleteEntry =
                bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))?;
            result.push(entry);
        }

        Ok(result)
    }

    /// Returns the total number of inodes in lazy-delete state.
    pub fn count(&self) -> Result<usize, MetaError> {
        let entries = self.kv.scan_prefix(LAZY_DELETE_PREFIX)?;
        Ok(entries.len())
    }

    /// Purges all entries with open_fd_count == 0.
    ///
    /// Called on server restart to clean up any entries that were not GC'd before shutdown.
    /// Returns the number of entries purged.
    pub fn purge_ready_for_gc(&self) -> Result<usize, MetaError> {
        let entries = self.kv.scan_prefix(LAZY_DELETE_PREFIX)?;

        let mut purged = 0;
        let mut to_delete = Vec::new();

        for (key, value) in entries {
            let entry: LazyDeleteEntry =
                bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))?;
            if entry.open_fd_count == 0 {
                to_delete.push(key);
            }
        }

        for key in to_delete {
            self.kv.delete(&key)?;
            purged += 1;
        }

        debug!("purged {} lazy-delete entries ready for GC", purged);
        Ok(purged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn make_store() -> LazyDeleteStore {
        LazyDeleteStore::new(Arc::new(MemoryKvStore::new()))
    }

    fn ino(n: u64) -> InodeId {
        InodeId::new(n)
    }

    #[test]
    fn test_mark_deleted_and_get() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file".to_string(), 1)
            .unwrap();

        let entry = store.get_entry(ino(42)).unwrap();
        assert!(entry.is_some());
        let e = entry.unwrap();
        assert_eq!(e.ino, ino(42));
        assert_eq!(e.original_path, "/test/file");
        assert_eq!(e.open_fd_count, 1);
    }

    #[test]
    fn test_mark_deleted_duplicate() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file".to_string(), 1)
            .unwrap();

        let result = store.mark_deleted(ino(42), "/test/file".to_string(), 1);
        assert!(result.is_err());
        match result {
            Err(MetaError::AlreadyExists(_)) => {}
            _ => panic!("expected AlreadyExists error"),
        }
    }

    #[test]
    fn test_inc_fd_count() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file".to_string(), 1)
            .unwrap();

        store.inc_fd_count(ino(42)).unwrap();

        let entry = store.get_entry(ino(42)).unwrap().unwrap();
        assert_eq!(entry.open_fd_count, 2);
    }

    #[test]
    fn test_inc_fd_count_not_found() {
        let store = make_store();

        let result = store.inc_fd_count(ino(42));
        assert!(result.is_err());
        match result {
            Err(MetaError::NotFound(_)) => {}
            _ => panic!("expected NotFound error"),
        }
    }

    #[test]
    fn test_dec_fd_count_still_open() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file".to_string(), 2)
            .unwrap();

        let still_open = store.dec_fd_count(ino(42)).unwrap();
        assert!(!still_open);

        let entry = store.get_entry(ino(42)).unwrap().unwrap();
        assert_eq!(entry.open_fd_count, 1);
    }

    #[test]
    fn test_dec_fd_count_reaches_zero() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file".to_string(), 1)
            .unwrap();

        let ready = store.dec_fd_count(ino(42)).unwrap();
        assert!(ready);

        let entry = store.get_entry(ino(42)).unwrap().unwrap();
        assert_eq!(entry.open_fd_count, 0);
    }

    #[test]
    fn test_dec_fd_count_not_found() {
        let store = make_store();

        let result = store.dec_fd_count(ino(42));
        assert!(result.is_err());
        match result {
            Err(MetaError::NotFound(_)) => {}
            _ => panic!("expected NotFound error"),
        }
    }

    #[test]
    fn test_remove_entry() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file".to_string(), 1)
            .unwrap();

        store.remove_entry(ino(42)).unwrap();

        let entry = store.get_entry(ino(42)).unwrap();
        assert!(entry.is_none());
    }

    #[test]
    fn test_ready_for_gc_empty() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file".to_string(), 1)
            .unwrap();
        store
            .mark_deleted(ino(43), "/test/file2".to_string(), 1)
            .unwrap();

        let ready = store.ready_for_gc().unwrap();
        assert!(ready.is_empty());
    }

    #[test]
    fn test_ready_for_gc_some() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file1".to_string(), 0)
            .unwrap();
        store
            .mark_deleted(ino(43), "/test/file2".to_string(), 1)
            .unwrap();
        store
            .mark_deleted(ino(44), "/test/file3".to_string(), 0)
            .unwrap();

        let ready = store.ready_for_gc().unwrap();
        assert_eq!(ready.len(), 2);
        assert!(ready.iter().any(|e| e.ino == ino(42)));
        assert!(ready.iter().any(|e| e.ino == ino(44)));
        assert!(!ready.iter().any(|e| e.ino == ino(43)));
    }

    #[test]
    fn test_list_all() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file1".to_string(), 0)
            .unwrap();
        store
            .mark_deleted(ino(43), "/test/file2".to_string(), 1)
            .unwrap();

        let all = store.list_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_count() {
        let store = make_store();
        assert_eq!(store.count().unwrap(), 0);

        store
            .mark_deleted(ino(42), "/test/file1".to_string(), 1)
            .unwrap();
        assert_eq!(store.count().unwrap(), 1);

        store
            .mark_deleted(ino(43), "/test/file2".to_string(), 1)
            .unwrap();
        assert_eq!(store.count().unwrap(), 2);
    }

    #[test]
    fn test_purge_ready_for_gc() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file1".to_string(), 0)
            .unwrap();
        store
            .mark_deleted(ino(43), "/test/file2".to_string(), 1)
            .unwrap();
        store
            .mark_deleted(ino(44), "/test/file3".to_string(), 0)
            .unwrap();

        let purged = store.purge_ready_for_gc().unwrap();
        assert_eq!(purged, 2);

        let remaining = store.list_all().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].ino, ino(43));
    }

    #[test]
    fn test_purge_keeps_open_entries() {
        let store = make_store();
        store
            .mark_deleted(ino(42), "/test/file1".to_string(), 1)
            .unwrap();
        store
            .mark_deleted(ino(43), "/test/file2".to_string(), 2)
            .unwrap();

        let purged = store.purge_ready_for_gc().unwrap();
        assert_eq!(purged, 0);

        let remaining = store.list_all().unwrap();
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    fn test_entry_serde() {
        let entry = LazyDeleteEntry {
            ino: ino(42),
            unlinked_at: Timestamp::now(),
            original_path: "/test/file".to_string(),
            open_fd_count: 3,
        };

        let encoded = bincode::serialize(&entry).unwrap();
        let decoded: LazyDeleteEntry = bincode::deserialize(&encoded).unwrap();

        assert_eq!(decoded.ino, entry.ino);
        assert_eq!(decoded.original_path, entry.original_path);
        assert_eq!(decoded.open_fd_count, entry.open_fd_count);
    }
}
