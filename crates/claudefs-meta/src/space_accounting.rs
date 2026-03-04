//! Per-directory disk usage tracking for quota enforcement and du-like reporting.
//!
//! This module tracks disk usage (bytes and inode count) for each directory subtree.
//! When a file is created/deleted/resized, the accounting is updated for the containing
//! directory and all ancestor directories up to root.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::kvstore::{BatchOp, KvStore};
use crate::types::InodeId;
use crate::types::MetaError;

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
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if both bytes and inodes are zero.
    pub fn is_empty(&self) -> bool {
        self.bytes == 0 && self.inodes == 0
    }
}

/// Space accounting store for per-directory usage tracking.
pub struct SpaceAccountingStore {
    kv: Arc<dyn KvStore>,
}

impl SpaceAccountingStore {
    /// Creates a new SpaceAccountingStore backed by the given KV store.
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    fn key_for(dir_ino: InodeId) -> Vec<u8> {
        let mut key = b"sa:".to_vec();
        key.extend(dir_ino.as_u64().to_be_bytes());
        key
    }

    /// Gets the current disk usage for a directory (direct subtree, not ancestors).
    ///
    /// Returns empty DirUsage if no accounting entry exists.
    pub fn get_usage(&self, dir_ino: InodeId) -> Result<DirUsage, MetaError> {
        let key = Self::key_for(dir_ino);
        match self.kv.get(&key)? {
            Some(data) => {
                Ok(bincode::deserialize(&data).map_err(|e| MetaError::KvError(e.to_string()))?)
            }
            None => Ok(DirUsage::new()),
        }
    }

    /// Sets the disk usage for a directory directly.
    ///
    /// Use this for initial setup or bulk import.
    pub fn set_usage(&self, dir_ino: InodeId, usage: DirUsage) -> Result<(), MetaError> {
        let key = Self::key_for(dir_ino);
        let data = bincode::serialize(&usage).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key, data)
    }

    /// Atomically adds delta_bytes bytes and delta_inodes inodes to a directory's usage.
    ///
    /// delta_bytes and delta_inodes can be negative (for deletions/shrinks).
    /// Uses saturating arithmetic to avoid underflow.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::KvError` if the KV store operation fails.
    pub fn add_delta(
        &self,
        dir_ino: InodeId,
        delta_bytes: i64,
        delta_inodes: i64,
    ) -> Result<(), MetaError> {
        let key = Self::key_for(dir_ino);
        let current = self.get_usage(dir_ino)?;
        let new_bytes = if delta_bytes >= 0 {
            current.bytes.saturating_add(delta_bytes as u64)
        } else {
            current.bytes.saturating_sub((-delta_bytes) as u64)
        };
        let new_inodes = if delta_inodes >= 0 {
            current.inodes.saturating_add(delta_inodes as u64)
        } else {
            current.inodes.saturating_sub((-delta_inodes) as u64)
        };
        let new_usage = DirUsage {
            bytes: new_bytes,
            inodes: new_inodes,
        };
        let data = bincode::serialize(&new_usage).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key, data)
    }

    /// Propagates usage deltas up the directory tree to all ancestor directories.
    ///
    /// Takes a slice of ancestor InodeIds from child to root (inclusive).
    /// For example: if file /a/b/c.txt is created, ancestors = [ino_b, ino_a, root_ino]
    ///
    /// Updates all ancestors atomically using write_batch.
    pub fn propagate_up(
        &self,
        ancestors: &[InodeId],
        delta_bytes: i64,
        delta_inodes: i64,
    ) -> Result<(), MetaError> {
        if ancestors.is_empty() {
            return Ok(());
        }

        let mut ops = Vec::with_capacity(ancestors.len());

        for &dir_ino in ancestors {
            let key = Self::key_for(dir_ino);
            let current = self.get_usage(dir_ino)?;
            let new_bytes = if delta_bytes >= 0 {
                current.bytes.saturating_add(delta_bytes as u64)
            } else {
                current.bytes.saturating_sub((-delta_bytes) as u64)
            };
            let new_inodes = if delta_inodes >= 0 {
                current.inodes.saturating_add(delta_inodes as u64)
            } else {
                current.inodes.saturating_sub((-delta_inodes) as u64)
            };
            let new_usage = DirUsage {
                bytes: new_bytes,
                inodes: new_inodes,
            };
            let data =
                bincode::serialize(&new_usage).map_err(|e| MetaError::KvError(e.to_string()))?;
            ops.push(BatchOp::Put { key, value: data });
        }

        self.kv.write_batch(ops)
    }

    /// Removes the accounting entry for a directory (on rmdir).
    pub fn remove_usage(&self, dir_ino: InodeId) -> Result<(), MetaError> {
        let key = Self::key_for(dir_ino);
        self.kv.delete(&key)
    }

    /// Returns the total bytes and inodes tracked across all directories.
    ///
    /// This is a sum over all stored entries — NOT propagation-adjusted.
    /// Use this for monitoring and consistency checks.
    pub fn total_tracked(&self) -> Result<DirUsage, MetaError> {
        let entries = self.kv.scan_prefix(b"sa:")?;
        let mut total = DirUsage::new();
        for (_, value) in entries {
            let usage: DirUsage =
                bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))?;
            total.bytes = total.bytes.saturating_add(usage.bytes);
            total.inodes = total.inodes.saturating_add(usage.inodes);
        }
        Ok(total)
    }

    /// Lists all directory accounting entries.
    ///
    /// Returns (dir_ino, DirUsage) pairs sorted by InodeId.
    pub fn list_all(&self) -> Result<Vec<(InodeId, DirUsage)>, MetaError> {
        let entries = self.kv.scan_prefix(b"sa:")?;
        let mut result = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            if key.len() != 11 || &key[0..3] != b"sa:" {
                continue;
            }
            let ino_bytes: [u8; 8] = key[3..11].try_into().unwrap();
            let ino = InodeId::new(u64::from_be_bytes(ino_bytes));
            let usage: DirUsage =
                bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))?;
            result.push((ino, usage));
        }
        result.sort_by_key(|(ino, _)| *ino);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn make_store() -> SpaceAccountingStore {
        SpaceAccountingStore::new(Arc::new(MemoryKvStore::new()))
    }

    #[test]
    fn test_get_usage_empty() {
        let store = make_store();
        let usage = store.get_usage(InodeId::new(42)).unwrap();
        assert!(usage.is_empty());
    }

    #[test]
    fn test_set_and_get_usage() {
        let store = make_store();
        let usage = DirUsage {
            bytes: 1000,
            inodes: 5,
        };
        store.set_usage(InodeId::new(42), usage.clone()).unwrap();
        let retrieved = store.get_usage(InodeId::new(42)).unwrap();
        assert_eq!(retrieved, usage);
    }

    #[test]
    fn test_add_delta_positive() {
        let store = make_store();
        store.add_delta(InodeId::new(1), 100, 1).unwrap();
        let usage = store.get_usage(InodeId::new(1)).unwrap();
        assert_eq!(usage.bytes, 100);
        assert_eq!(usage.inodes, 1);
    }

    #[test]
    fn test_add_delta_multiple() {
        let store = make_store();
        store.add_delta(InodeId::new(1), 100, 1).unwrap();
        store.add_delta(InodeId::new(1), 200, 2).unwrap();
        store.add_delta(InodeId::new(1), 50, 1).unwrap();
        let usage = store.get_usage(InodeId::new(1)).unwrap();
        assert_eq!(usage.bytes, 350);
        assert_eq!(usage.inodes, 4);
    }

    #[test]
    fn test_add_delta_negative() {
        let store = make_store();
        store
            .set_usage(
                InodeId::new(1),
                DirUsage {
                    bytes: 200,
                    inodes: 5,
                },
            )
            .unwrap();
        store.add_delta(InodeId::new(1), -50, -2).unwrap();
        let usage = store.get_usage(InodeId::new(1)).unwrap();
        assert_eq!(usage.bytes, 150);
        assert_eq!(usage.inodes, 3);
    }

    #[test]
    fn test_add_delta_saturating() {
        let store = make_store();
        store
            .set_usage(
                InodeId::new(1),
                DirUsage {
                    bytes: 30,
                    inodes: 2,
                },
            )
            .unwrap();
        store.add_delta(InodeId::new(1), -100, -5).unwrap();
        let usage = store.get_usage(InodeId::new(1)).unwrap();
        assert_eq!(usage.bytes, 0);
        assert_eq!(usage.inodes, 0);
    }

    #[test]
    fn test_propagate_up_single_level() {
        let store = make_store();
        let parent = InodeId::new(2);
        let root = InodeId::ROOT_INODE;
        store.propagate_up(&[parent, root], 100, 1).unwrap();
        let parent_usage = store.get_usage(parent).unwrap();
        let root_usage = store.get_usage(root).unwrap();
        assert_eq!(parent_usage.bytes, 100);
        assert_eq!(parent_usage.inodes, 1);
        assert_eq!(root_usage.bytes, 100);
        assert_eq!(root_usage.inodes, 1);
    }

    #[test]
    fn test_propagate_up_multiple_levels() {
        let store = make_store();
        let level1 = InodeId::new(2);
        let level2 = InodeId::new(3);
        let root = InodeId::ROOT_INODE;
        store.propagate_up(&[level1, level2, root], 500, 3).unwrap();
        let l1 = store.get_usage(level1).unwrap();
        let l2 = store.get_usage(level2).unwrap();
        let r = store.get_usage(root).unwrap();
        assert_eq!(l1.bytes, 500);
        assert_eq!(l1.inodes, 3);
        assert_eq!(l2.bytes, 500);
        assert_eq!(l2.inodes, 3);
        assert_eq!(r.bytes, 500);
        assert_eq!(r.inodes, 3);
    }

    #[test]
    fn test_propagate_up_empty_ancestors() {
        let store = make_store();
        store.propagate_up(&[], 100, 1).unwrap();
        let usage = store.get_usage(InodeId::ROOT_INODE).unwrap();
        assert!(usage.is_empty());
    }

    #[test]
    fn test_remove_usage() {
        let store = make_store();
        store
            .set_usage(
                InodeId::new(42),
                DirUsage {
                    bytes: 1000,
                    inodes: 5,
                },
            )
            .unwrap();
        store.remove_usage(InodeId::new(42)).unwrap();
        let usage = store.get_usage(InodeId::new(42)).unwrap();
        assert!(usage.is_empty());
    }

    #[test]
    fn test_remove_nonexistent() {
        let store = make_store();
        store.remove_usage(InodeId::new(999)).unwrap();
    }

    #[test]
    fn test_total_tracked() {
        let store = make_store();
        store
            .set_usage(
                InodeId::new(1),
                DirUsage {
                    bytes: 100,
                    inodes: 2,
                },
            )
            .unwrap();
        store
            .set_usage(
                InodeId::new(2),
                DirUsage {
                    bytes: 200,
                    inodes: 3,
                },
            )
            .unwrap();
        store
            .set_usage(
                InodeId::new(3),
                DirUsage {
                    bytes: 50,
                    inodes: 1,
                },
            )
            .unwrap();
        let total = store.total_tracked().unwrap();
        assert_eq!(total.bytes, 350);
        assert_eq!(total.inodes, 6);
    }

    #[test]
    fn test_list_all() {
        let store = make_store();
        store
            .set_usage(
                InodeId::new(3),
                DirUsage {
                    bytes: 50,
                    inodes: 1,
                },
            )
            .unwrap();
        store
            .set_usage(
                InodeId::new(1),
                DirUsage {
                    bytes: 100,
                    inodes: 2,
                },
            )
            .unwrap();
        store
            .set_usage(
                InodeId::new(2),
                DirUsage {
                    bytes: 200,
                    inodes: 3,
                },
            )
            .unwrap();
        let all = store.list_all().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].0, InodeId::new(1));
        assert_eq!(all[1].0, InodeId::new(2));
        assert_eq!(all[2].0, InodeId::new(3));
    }

    #[test]
    fn test_dir_usage_serde() {
        let usage = DirUsage {
            bytes: 12345,
            inodes: 67,
        };
        let encoded = bincode::serialize(&usage).unwrap();
        let decoded: DirUsage = bincode::deserialize(&encoded).unwrap();
        assert_eq!(usage, decoded);
    }
}
