//! Directory mtime/ctime propagation tracking.
//!
//! POSIX requires that when a file in a directory is created, deleted, or renamed,
//! the parent directory's `mtime` and `ctime` must be updated. This module tracks
//! which directories need mtime updates and applies them efficiently.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::kvstore::{BatchOp, KvStore};
use crate::types::{InodeId, MetaError, Timestamp};

const MTIME_KEY_PREFIX: &[u8] = b"mt:";

/// A pending mtime/ctime update for a directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MtimeUpdate {
    /// The directory inode ID that needs updating.
    pub dir_ino: InodeId,
    /// The new mtime value.
    pub mtime: Timestamp,
    /// The new ctime value (usually same as mtime for directory updates).
    pub ctime: Timestamp,
    /// The reason for this update (for debugging).
    pub reason: MtimeReason,
}

/// The reason for an mtime update.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MtimeReason {
    /// A file was created in this directory.
    FileCreated,
    /// A file was deleted from this directory.
    FileDeleted,
    /// A file was renamed into or out of this directory.
    FileRenamed,
    /// A file's attributes were changed (touches ctime only).
    AttrChanged,
    /// A file's data was written (touches file mtime, not dir).
    DataWritten,
}

/// Batch collector for mtime updates.
///
/// Deduplicates multiple updates to the same directory within one operation batch,
/// keeping only the most recent mtime.
pub struct MtimeBatch {
    updates: Vec<MtimeUpdate>,
}

impl MtimeBatch {
    /// Creates a new empty batch.
    pub fn new() -> Self {
        Self {
            updates: Vec::new(),
        }
    }

    /// Adds an update to the batch.
    ///
    /// If an update for the same `dir_ino` already exists in the batch,
    /// replaces it with the new update (deduplication).
    pub fn add(&mut self, update: MtimeUpdate) {
        if let Some(existing) = self
            .updates
            .iter_mut()
            .find(|u| u.dir_ino == update.dir_ino)
        {
            *existing = update;
        } else {
            self.updates.push(update);
        }
    }

    /// Returns the number of unique directories in this batch.
    pub fn len(&self) -> usize {
        self.updates.len()
    }

    /// Returns true if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    /// Iterates over the batch updates in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &MtimeUpdate> {
        self.updates.iter()
    }

    /// Consumes the batch and returns all updates.
    #[allow(dead_code)]
    pub fn into_vec(self) -> Vec<MtimeUpdate> {
        self.updates
    }
}

impl Default for MtimeBatch {
    fn default() -> Self {
        Self::new()
    }
}

fn make_mtime_key(dir_ino: InodeId) -> Vec<u8> {
    let mut key = Vec::with_capacity(3 + 8);
    key.extend_from_slice(MTIME_KEY_PREFIX);
    key.extend_from_slice(&dir_ino.as_u64().to_be_bytes());
    key
}

/// Persistent store for directory mtime/ctime timestamps.
pub struct MtimeStore {
    kv: Arc<dyn KvStore>,
}

impl MtimeStore {
    /// Creates a new MtimeStore backed by the given KV store.
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Gets the current mtime for a directory.
    ///
    /// Returns `None` if no mtime has been recorded (use InodeAttr.mtime instead).
    pub fn get_mtime(&self, dir_ino: InodeId) -> Result<Option<Timestamp>, MetaError> {
        let key = make_mtime_key(dir_ino);
        match self.kv.get(&key)? {
            Some(value) => {
                if value.len() >= 12 {
                    let secs = u64::from_be_bytes(value[0..8].try_into().unwrap());
                    let nanos = u32::from_be_bytes(value[8..12].try_into().unwrap());
                    Ok(Some(Timestamp { secs, nanos }))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Sets the mtime for a directory.
    pub fn set_mtime(&self, dir_ino: InodeId, mtime: Timestamp) -> Result<(), MetaError> {
        let key = make_mtime_key(dir_ino);
        let mut value = Vec::with_capacity(12);
        value.extend_from_slice(&mtime.secs.to_be_bytes());
        value.extend_from_slice(&mtime.nanos.to_be_bytes());
        self.kv.put(key, value)
    }

    /// Applies a batch of mtime updates atomically using write_batch.
    ///
    /// Only updates directories where the new mtime is more recent than the stored mtime.
    pub fn apply_batch(&self, batch: MtimeBatch) -> Result<usize, MetaError> {
        if batch.is_empty() {
            return Ok(0);
        }

        let mut ops = Vec::new();
        let mut updated_count = 0;

        for update in batch.iter() {
            let key = make_mtime_key(update.dir_ino);
            let current = self.kv.get(&key)?;

            let should_update = match current {
                Some(value) if value.len() >= 12 => {
                    let stored_secs = u64::from_be_bytes(value[0..8].try_into().unwrap());
                    let stored_nanos = u32::from_be_bytes(value[8..12].try_into().unwrap());
                    let stored_ts = Timestamp {
                        secs: stored_secs,
                        nanos: stored_nanos,
                    };
                    update.mtime > stored_ts
                }
                _ => true,
            };

            if should_update {
                let mut value = Vec::with_capacity(12);
                value.extend_from_slice(&update.mtime.secs.to_be_bytes());
                value.extend_from_slice(&update.mtime.nanos.to_be_bytes());
                ops.push(BatchOp::Put { key, value });
                updated_count += 1;
            }
        }

        if !ops.is_empty() {
            self.kv.write_batch(ops)?;
        }

        Ok(updated_count)
    }

    /// Removes the stored mtime for a directory (on rmdir).
    pub fn remove_mtime(&self, dir_ino: InodeId) -> Result<(), MetaError> {
        let key = make_mtime_key(dir_ino);
        self.kv.delete(&key)
    }

    /// Returns all stored mtime entries.
    pub fn list_all(&self) -> Result<Vec<(InodeId, Timestamp)>, MetaError> {
        let pairs = self.kv.scan_prefix(MTIME_KEY_PREFIX)?;
        let mut result = Vec::new();
        for (key, value) in pairs {
            if key.len() == 11 && key.starts_with(MTIME_KEY_PREFIX) {
                let ino_bytes = &key[3..];
                if let Ok(ino) = ino_bytes.try_into() {
                    let ino = u64::from_be_bytes(ino);
                    if value.len() >= 12 {
                        let secs = u64::from_be_bytes(value[0..8].try_into().unwrap());
                        let nanos = u32::from_be_bytes(value[8..12].try_into().unwrap());
                        result.push((InodeId::new(ino), Timestamp { secs, nanos }));
                    }
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn make_store() -> MtimeStore {
        MtimeStore::new(Arc::new(MemoryKvStore::new()))
    }

    fn ts(secs: u64, nanos: u32) -> Timestamp {
        Timestamp { secs, nanos }
    }

    #[test]
    fn test_set_and_get_mtime() {
        let store = make_store();
        let dir_ino = InodeId::new(42);
        let mtime = ts(1000, 500);

        store.set_mtime(dir_ino, mtime).unwrap();
        let result = store.get_mtime(dir_ino).unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap(), mtime);
    }

    #[test]
    fn test_get_mtime_missing() {
        let store = make_store();
        let dir_ino = InodeId::new(999);

        let result = store.get_mtime(dir_ino).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_mtime_batch_dedup() {
        let mut batch = MtimeBatch::new();
        let dir_ino = InodeId::new(1);

        batch.add(MtimeUpdate {
            dir_ino,
            mtime: ts(100, 0),
            ctime: ts(100, 0),
            reason: MtimeReason::FileCreated,
        });

        batch.add(MtimeUpdate {
            dir_ino,
            mtime: ts(200, 0),
            ctime: ts(200, 0),
            reason: MtimeReason::FileDeleted,
        });

        assert_eq!(batch.len(), 1);
        assert_eq!(batch.iter().next().unwrap().mtime, ts(200, 0));
    }

    #[test]
    fn test_mtime_batch_empty() {
        let batch = MtimeBatch::new();
        assert_eq!(batch.len(), 0);
        assert!(batch.is_empty());
    }

    #[test]
    fn test_mtime_batch_add_different_dirs() {
        let mut batch = MtimeBatch::new();

        batch.add(MtimeUpdate {
            dir_ino: InodeId::new(1),
            mtime: ts(100, 0),
            ctime: ts(100, 0),
            reason: MtimeReason::FileCreated,
        });
        batch.add(MtimeUpdate {
            dir_ino: InodeId::new(2),
            mtime: ts(200, 0),
            ctime: ts(200, 0),
            reason: MtimeReason::FileDeleted,
        });
        batch.add(MtimeUpdate {
            dir_ino: InodeId::new(3),
            mtime: ts(300, 0),
            ctime: ts(300, 0),
            reason: MtimeReason::FileRenamed,
        });

        assert_eq!(batch.len(), 3);
    }

    #[test]
    fn test_apply_batch_updates_newer() {
        let store = make_store();
        let dir_ino = InodeId::new(1);

        store.set_mtime(dir_ino, ts(100, 0)).unwrap();

        let mut batch = MtimeBatch::new();
        batch.add(MtimeUpdate {
            dir_ino,
            mtime: ts(200, 0),
            ctime: ts(200, 0),
            reason: MtimeReason::FileCreated,
        });

        let count = store.apply_batch(batch).unwrap();
        assert_eq!(count, 1);

        let result = store.get_mtime(dir_ino).unwrap().unwrap();
        assert_eq!(result, ts(200, 0));
    }

    #[test]
    fn test_apply_batch_skips_older() {
        let store = make_store();
        let dir_ino = InodeId::new(1);

        store.set_mtime(dir_ino, ts(200, 0)).unwrap();

        let mut batch = MtimeBatch::new();
        batch.add(MtimeUpdate {
            dir_ino,
            mtime: ts(100, 0),
            ctime: ts(100, 0),
            reason: MtimeReason::FileCreated,
        });

        let count = store.apply_batch(batch).unwrap();
        assert_eq!(count, 0);

        let result = store.get_mtime(dir_ino).unwrap().unwrap();
        assert_eq!(result, ts(200, 0));
    }

    #[test]
    fn test_apply_batch_empty() {
        let store = make_store();
        let batch = MtimeBatch::new();

        let count = store.apply_batch(batch).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_apply_batch_count() {
        let store = make_store();

        let mut batch = MtimeBatch::new();
        batch.add(MtimeUpdate {
            dir_ino: InodeId::new(1),
            mtime: ts(100, 0),
            ctime: ts(100, 0),
            reason: MtimeReason::FileCreated,
        });
        batch.add(MtimeUpdate {
            dir_ino: InodeId::new(2),
            mtime: ts(200, 0),
            ctime: ts(200, 0),
            reason: MtimeReason::FileDeleted,
        });
        batch.add(MtimeUpdate {
            dir_ino: InodeId::new(3),
            mtime: ts(300, 0),
            ctime: ts(300, 0),
            reason: MtimeReason::FileRenamed,
        });

        let count = store.apply_batch(batch).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_remove_mtime() {
        let store = make_store();
        let dir_ino = InodeId::new(1);

        store.set_mtime(dir_ino, ts(100, 0)).unwrap();
        store.remove_mtime(dir_ino).unwrap();

        let result = store.get_mtime(dir_ino).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_remove_nonexistent() {
        let store = make_store();
        let dir_ino = InodeId::new(999);

        let result = store.remove_mtime(dir_ino);
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_all() {
        let store = make_store();

        store.set_mtime(InodeId::new(1), ts(100, 0)).unwrap();
        store.set_mtime(InodeId::new(2), ts(200, 0)).unwrap();
        store.set_mtime(InodeId::new(3), ts(300, 0)).unwrap();

        let entries = store.list_all().unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_mtime_reason_serde() {
        let reasons = [
            MtimeReason::FileCreated,
            MtimeReason::FileDeleted,
            MtimeReason::FileRenamed,
            MtimeReason::AttrChanged,
            MtimeReason::DataWritten,
        ];

        for reason in reasons {
            let encoded = bincode::serialize(&reason).unwrap();
            let decoded: MtimeReason = bincode::deserialize(&encoded).unwrap();
            assert_eq!(reason, decoded);
        }
    }

    #[test]
    fn test_mtime_update_serde() {
        let update = MtimeUpdate {
            dir_ino: InodeId::new(42),
            mtime: ts(1000, 500),
            ctime: ts(1000, 500),
            reason: MtimeReason::FileCreated,
        };

        let encoded = bincode::serialize(&update).unwrap();
        let decoded: MtimeUpdate = bincode::deserialize(&encoded).unwrap();
        assert_eq!(update.dir_ino, decoded.dir_ino);
        assert_eq!(update.mtime, decoded.mtime);
        assert_eq!(update.ctime, decoded.ctime);
        assert_eq!(update.reason, decoded.reason);
    }
}
