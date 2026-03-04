//! Metadata checkpoint manager for fast restart.
//!
//! This module implements periodic full-state checkpoints that can replace
//! log replay on startup. A checkpoint is a serialized snapshot of the entire
//! in-memory metadata state (inodes, directory entries, xattrs, ACLs).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::{LogIndex, MetaError, NodeId, ShardId, Timestamp};

/// A key-value pair as raw bytes.
type KvPair = (Vec<u8>, Vec<u8>);

/// Metadata describing a checkpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointMeta {
    /// Unique identifier for this checkpoint.
    pub checkpoint_id: u64,
    /// When this checkpoint was created.
    pub created_at: Timestamp,
    /// The journal index at the time of checkpoint.
    pub log_index: LogIndex,
    /// The node that created this checkpoint.
    pub node_id: NodeId,
    /// The shard this checkpoint belongs to.
    pub shard_id: ShardId,
    /// Number of KV entries captured in this checkpoint.
    pub entry_count: u64,
    /// Size of the checkpoint data in bytes.
    pub size_bytes: u64,
}

/// A complete checkpoint with metadata and serialized data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Metadata about this checkpoint.
    pub meta: CheckpointMeta,
    /// Bincode-serialized KV pairs (key→value).
    pub data: Vec<u8>,
}

/// Manager for creating, storing, and restoring checkpoints.
pub struct CheckpointManager {
    /// In-memory store of checkpoints keyed by checkpoint_id.
    checkpoints: HashMap<u64, Checkpoint>,
    /// Next checkpoint ID to allocate.
    next_id: u64,
    /// Maximum number of checkpoints to retain.
    max_checkpoints: usize,
    /// Node ID for checkpoint metadata.
    node_id: NodeId,
    /// Shard ID for checkpoint metadata.
    shard_id: ShardId,
}

impl CheckpointManager {
    /// Creates a new checkpoint manager.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The ID of this metadata node.
    /// * `shard_id` - The shard this manager is responsible for.
    /// * `max_checkpoints` - Maximum number of checkpoints to retain (oldest evicted).
    pub fn new(node_id: NodeId, shard_id: ShardId, max_checkpoints: usize) -> Self {
        Self {
            checkpoints: HashMap::new(),
            next_id: 1,
            max_checkpoints,
            node_id,
            shard_id,
        }
    }

    /// Creates a checkpoint from a KV store snapshot.
    ///
    /// # Arguments
    ///
    /// * `log_index` - The current journal index.
    /// * `entries` - KV pairs to capture in the checkpoint.
    ///
    /// # Returns
    ///
    /// The metadata for the created checkpoint.
    pub fn create_checkpoint(
        &mut self,
        log_index: LogIndex,
        entries: Vec<KvPair>,
    ) -> Result<CheckpointMeta, MetaError> {
        let checkpoint_id = self.next_id;
        self.next_id += 1;

        let entry_count = entries.len() as u64;
        let data = bincode::serialize(&entries)
            .map_err(|e| MetaError::KvError(format!("serialize: {}", e)))?;
        let size_bytes = data.len() as u64;

        let meta = CheckpointMeta {
            checkpoint_id,
            created_at: Timestamp::now(),
            log_index,
            node_id: self.node_id,
            shard_id: self.shard_id,
            entry_count,
            size_bytes,
        };

        let checkpoint = Checkpoint {
            meta: meta.clone(),
            data,
        };
        self.checkpoints.insert(checkpoint_id, checkpoint);

        self.evict_old();

        Ok(meta)
    }

    /// Loads a checkpoint by ID.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::InodeNotFound` if the checkpoint doesn't exist.
    pub fn load_checkpoint(&self, checkpoint_id: u64) -> Result<&Checkpoint, MetaError> {
        self.checkpoints
            .get(&checkpoint_id)
            .ok_or_else(|| MetaError::InodeNotFound(crate::types::InodeId::new(checkpoint_id)))
    }

    /// Returns the latest checkpoint (highest log_index).
    pub fn latest(&self) -> Option<&CheckpointMeta> {
        self.checkpoints
            .values()
            .map(|c| &c.meta)
            .max_by_key(|m| m.log_index)
    }

    /// Lists all checkpoints sorted by log_index ascending.
    pub fn list(&self) -> Vec<&CheckpointMeta> {
        let mut metas: Vec<_> = self.checkpoints.values().map(|c| &c.meta).collect();
        metas.sort_by_key(|m| m.log_index);
        metas
    }

    /// Deletes a checkpoint by ID.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::InodeNotFound` if the checkpoint doesn't exist.
    pub fn delete_checkpoint(&mut self, checkpoint_id: u64) -> Result<(), MetaError> {
        if self.checkpoints.remove(&checkpoint_id).is_some() {
            Ok(())
        } else {
            Err(MetaError::InodeNotFound(crate::types::InodeId::new(
                checkpoint_id,
            )))
        }
    }

    /// Restores KV entries from a checkpoint.
    ///
    /// Returns the KV pairs that should be loaded into the KV store.
    pub fn restore(&self, checkpoint_id: u64) -> Result<Vec<KvPair>, MetaError> {
        let checkpoint = self.load_checkpoint(checkpoint_id)?;
        let entries: Vec<KvPair> = bincode::deserialize(&checkpoint.data)
            .map_err(|e| MetaError::KvError(format!("deserialize: {}", e)))?;
        Ok(entries)
    }

    /// Evicts oldest checkpoints to maintain the configured limit.
    fn evict_old(&mut self) {
        while self.checkpoints.len() > self.max_checkpoints {
            if let Some((&oldest_id, _)) = self
                .checkpoints
                .iter()
                .min_by_key(|(_, c)| c.meta.log_index)
            {
                let _ = self.checkpoints.remove(&oldest_id);
            } else {
                break;
            }
        }
    }

    /// Returns the total size of all stored checkpoints in bytes.
    pub fn total_size_bytes(&self) -> u64 {
        self.checkpoints.values().map(|c| c.meta.size_bytes).sum()
    }

    /// Returns the number of stored checkpoints.
    pub fn count(&self) -> usize {
        self.checkpoints.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager(max_checkpoints: usize) -> CheckpointManager {
        CheckpointManager::new(NodeId::new(1), ShardId::new(0), max_checkpoints)
    }

    #[test]
    fn test_create_checkpoint() {
        let mut manager = make_manager(10);

        let entries = vec![
            (b"key1".to_vec(), b"value1".to_vec()),
            (b"key2".to_vec(), b"value2".to_vec()),
        ];

        let meta = manager
            .create_checkpoint(LogIndex::new(100), entries)
            .unwrap();

        assert_eq!(meta.checkpoint_id, 1);
        assert_eq!(meta.log_index, LogIndex::new(100));
        assert_eq!(meta.entry_count, 2);
        assert!(meta.size_bytes > 0);
    }

    #[test]
    fn test_load_checkpoint() {
        let mut manager = make_manager(10);

        let entries = vec![(b"key".to_vec(), b"value".to_vec())];
        let meta = manager
            .create_checkpoint(LogIndex::new(50), entries)
            .unwrap();

        let loaded = manager.load_checkpoint(meta.checkpoint_id).unwrap();
        assert_eq!(loaded.meta.checkpoint_id, meta.checkpoint_id);
        assert_eq!(loaded.meta.log_index, LogIndex::new(50));
    }

    #[test]
    fn test_load_checkpoint_not_found() {
        let manager = make_manager(10);
        let result = manager.load_checkpoint(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_latest_checkpoint() {
        let mut manager = make_manager(10);

        let meta1 = manager
            .create_checkpoint(LogIndex::new(100), vec![])
            .unwrap();
        let meta2 = manager
            .create_checkpoint(LogIndex::new(200), vec![])
            .unwrap();
        let _meta3 = manager
            .create_checkpoint(LogIndex::new(150), vec![])
            .unwrap();

        let latest = manager.latest().unwrap();
        assert_eq!(latest.checkpoint_id, meta2.checkpoint_id);
        assert_eq!(latest.log_index, LogIndex::new(200));
    }

    #[test]
    fn test_latest_checkpoint_empty() {
        let manager = make_manager(10);
        assert!(manager.latest().is_none());
    }

    #[test]
    fn test_list_checkpoints_sorted() {
        let mut manager = make_manager(10);

        let _meta1 = manager
            .create_checkpoint(LogIndex::new(100), vec![])
            .unwrap();
        let _meta2 = manager
            .create_checkpoint(LogIndex::new(50), vec![])
            .unwrap();
        let _meta3 = manager
            .create_checkpoint(LogIndex::new(150), vec![])
            .unwrap();

        let list = manager.list();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].log_index, LogIndex::new(50));
        assert_eq!(list[1].log_index, LogIndex::new(100));
        assert_eq!(list[2].log_index, LogIndex::new(150));
    }

    #[test]
    fn test_delete_checkpoint() {
        let mut manager = make_manager(10);

        let meta = manager
            .create_checkpoint(LogIndex::new(100), vec![])
            .unwrap();

        assert!(manager.load_checkpoint(meta.checkpoint_id).is_ok());
        manager.delete_checkpoint(meta.checkpoint_id).unwrap();
        assert!(manager.load_checkpoint(meta.checkpoint_id).is_err());
    }

    #[test]
    fn test_delete_checkpoint_not_found() {
        let mut manager = make_manager(10);
        let result = manager.delete_checkpoint(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_restore_checkpoint() {
        let mut manager = make_manager(10);

        let entries = vec![
            (b"key1".to_vec(), b"value1".to_vec()),
            (b"key2".to_vec(), b"value2".to_vec()),
            (b"key3".to_vec(), b"value3".to_vec()),
        ];

        let meta = manager
            .create_checkpoint(LogIndex::new(100), entries.clone())
            .unwrap();

        let restored = manager.restore(meta.checkpoint_id).unwrap();
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[0].0, b"key1");
        assert_eq!(restored[1].0, b"key2");
        assert_eq!(restored[2].0, b"key3");
    }

    #[test]
    fn test_max_checkpoints_eviction() {
        let mut manager = make_manager(3);

        let meta1 = manager
            .create_checkpoint(LogIndex::new(10), vec![])
            .unwrap();
        let meta2 = manager
            .create_checkpoint(LogIndex::new(20), vec![])
            .unwrap();
        let meta3 = manager
            .create_checkpoint(LogIndex::new(30), vec![])
            .unwrap();
        let _meta4 = manager
            .create_checkpoint(LogIndex::new(40), vec![])
            .unwrap();

        assert_eq!(manager.count(), 3);

        assert!(manager.load_checkpoint(meta1.checkpoint_id).is_err());

        assert!(manager.load_checkpoint(meta2.checkpoint_id).is_ok());
        assert!(manager.load_checkpoint(meta3.checkpoint_id).is_ok());
    }

    #[test]
    fn test_total_size_bytes() {
        let mut manager = make_manager(10);

        let entries1 = vec![(b"key".to_vec(), b"value".to_vec())];
        let entries2 = vec![
            (b"key1".to_vec(), b"value1".to_vec()),
            (b"key2".to_vec(), b"value2".to_vec()),
        ];

        let meta1 = manager
            .create_checkpoint(LogIndex::new(10), entries1)
            .unwrap();
        let meta2 = manager
            .create_checkpoint(LogIndex::new(20), entries2)
            .unwrap();

        let total = manager.total_size_bytes();
        assert_eq!(total, meta1.size_bytes + meta2.size_bytes);
    }

    #[test]
    fn test_total_size_bytes_empty() {
        let manager = make_manager(10);
        assert_eq!(manager.total_size_bytes(), 0);
    }

    #[test]
    fn test_checkpoint_meta_serde() {
        let meta = CheckpointMeta {
            checkpoint_id: 42,
            created_at: Timestamp::now(),
            log_index: LogIndex::new(100),
            node_id: NodeId::new(1),
            shard_id: ShardId::new(0),
            entry_count: 10,
            size_bytes: 1024,
        };

        let encoded = bincode::serialize(&meta).unwrap();
        let decoded: CheckpointMeta = bincode::deserialize(&encoded).unwrap();
        assert_eq!(meta.checkpoint_id, decoded.checkpoint_id);
        assert_eq!(meta.log_index, decoded.log_index);
    }

    #[test]
    fn test_checkpoint_serde() {
        let meta = CheckpointMeta {
            checkpoint_id: 42,
            created_at: Timestamp::now(),
            log_index: LogIndex::new(100),
            node_id: NodeId::new(1),
            shard_id: ShardId::new(0),
            entry_count: 2,
            size_bytes: 100,
        };
        let checkpoint = Checkpoint {
            meta: meta.clone(),
            data: vec![1, 2, 3, 4],
        };

        let encoded = bincode::serialize(&checkpoint).unwrap();
        let decoded: Checkpoint = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded.meta.checkpoint_id, meta.checkpoint_id);
        assert_eq!(decoded.data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_count() {
        let mut manager = make_manager(10);

        assert_eq!(manager.count(), 0);

        manager
            .create_checkpoint(LogIndex::new(10), vec![])
            .unwrap();
        assert_eq!(manager.count(), 1);

        manager
            .create_checkpoint(LogIndex::new(20), vec![])
            .unwrap();
        assert_eq!(manager.count(), 2);
    }

    #[test]
    fn test_eviction_by_log_index_order() {
        let mut manager = make_manager(2);

        let meta_old = manager
            .create_checkpoint(LogIndex::new(10), vec![])
            .unwrap();
        let meta_mid = manager
            .create_checkpoint(LogIndex::new(30), vec![])
            .unwrap();
        let meta_new = manager
            .create_checkpoint(LogIndex::new(20), vec![])
            .unwrap();

        assert_eq!(manager.count(), 2);

        assert!(manager.load_checkpoint(meta_old.checkpoint_id).is_err());
        assert!(manager.load_checkpoint(meta_mid.checkpoint_id).is_ok());
        assert!(manager.load_checkpoint(meta_new.checkpoint_id).is_ok());
    }
}
