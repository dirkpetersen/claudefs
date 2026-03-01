//! CoW snapshot management for the CAS store.

use crate::error::ReduceError;
use crate::fingerprint::ChunkHash;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

/// Metadata for a snapshot (lightweight, stored in manifest).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// Unique snapshot identifier
    pub id: u64,
    /// User-friendly snapshot name
    pub name: String,
    /// Unix timestamp when created
    pub created_at_secs: u64,
    /// Number of blocks in this snapshot
    pub block_count: usize,
    /// Total bytes in snapshot
    pub total_bytes: u64,
}

/// A CoW snapshot containing the block hashes at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Snapshot metadata
    pub info: SnapshotInfo,
    /// CAS keys (chunk hashes) at snapshot time
    pub block_hashes: Vec<ChunkHash>,
}

/// Configuration for snapshot management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Maximum number of snapshots to retain
    pub max_snapshots: usize,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self { max_snapshots: 64 }
    }
}

/// Manager for CoW snapshots of the CAS store.
pub struct SnapshotManager {
    config: SnapshotConfig,
    next_id: u64,
    snapshots: HashMap<u64, Snapshot>,
    name_index: HashMap<String, u64>,
}

impl SnapshotManager {
    /// Create a new snapshot manager with the given configuration.
    pub fn new(config: SnapshotConfig) -> Self {
        Self {
            config,
            next_id: 1,
            snapshots: HashMap::new(),
            name_index: HashMap::new(),
        }
    }

    /// Create a new snapshot with the given name and block hashes.
    /// Returns Err if at max_snapshots limit.
    pub fn create_snapshot(
        &mut self,
        name: String,
        block_hashes: Vec<ChunkHash>,
        total_bytes: u64,
    ) -> Result<SnapshotInfo, ReduceError> {
        if self.snapshots.len() >= self.config.max_snapshots {
            return Err(ReduceError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Maximum snapshot limit ({}) reached", self.config.max_snapshots),
            )));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| ReduceError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )))?
            .as_secs();

        let id = self.next_id;
        self.next_id += 1;

        let info = SnapshotInfo {
            id,
            name: name.clone(),
            created_at_secs: now,
            block_count: block_hashes.len(),
            total_bytes,
        };

        let snapshot = Snapshot {
            info: info.clone(),
            block_hashes,
        };

        debug!(snapshot_id = id, name = %name, blocks = info.block_count, "Created snapshot");
        self.name_index.insert(name, id);
        self.snapshots.insert(id, snapshot);

        Ok(info)
    }

    /// Delete a snapshot by ID.
    /// Returns the deleted snapshot if it existed.
    pub fn delete_snapshot(&mut self, id: u64) -> Option<Snapshot> {
        if let Some(snapshot) = self.snapshots.remove(&id) {
            self.name_index.remove(&snapshot.info.name);
            debug!(snapshot_id = id, "Deleted snapshot");
            Some(snapshot)
        } else {
            None
        }
    }

    /// Get a snapshot by ID.
    pub fn get_snapshot(&self, id: u64) -> Option<&Snapshot> {
        self.snapshots.get(&id)
    }

    /// List all snapshots, sorted by creation time (oldest first).
    pub fn list_snapshots(&self) -> Vec<&SnapshotInfo> {
        let mut snapshots: Vec<&SnapshotInfo> = self.snapshots.values().map(|s| &s.info).collect();
        snapshots.sort_by_key(|info| info.created_at_secs);
        snapshots
    }

    /// Number of snapshots currently stored.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Clone an existing snapshot with a new name.
    /// Errors if source_id doesn't exist.
    pub fn clone_snapshot(
        &mut self,
        source_id: u64,
        new_name: String,
    ) -> Result<SnapshotInfo, ReduceError> {
        let source = self.snapshots.get(&source_id).ok_or_else(|| {
            ReduceError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Snapshot {} not found", source_id),
            ))
        })?;

        // Clone the block hashes (CoW - just reference, not copy)
        let block_hashes = source.block_hashes.clone();
        let total_bytes = source.info.total_bytes;

        self.create_snapshot(new_name, block_hashes, total_bytes)
    }

    /// Find a snapshot by name.
    pub fn find_by_name(&self, name: &str) -> Option<&Snapshot> {
        self.name_index.get(name).and_then(|id| self.snapshots.get(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::blake3_hash;

    fn make_hash(i: u8) -> ChunkHash {
        blake3_hash(&[i])
    }

    #[test]
    fn test_create_snapshot() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());
        let hashes = vec![make_hash(1), make_hash(2), make_hash(3)];
        
        let info = mgr.create_snapshot("test".to_string(), hashes.clone(), 12345).unwrap();
        
        assert_eq!(info.name, "test");
        assert_eq!(info.block_count, 3);
        assert_eq!(info.total_bytes, 12345);
        assert!(info.id > 0);
    }

    #[test]
    fn test_max_snapshots_limit() {
        let mut mgr = SnapshotManager::new(SnapshotConfig { max_snapshots: 2 });
        
        mgr.create_snapshot("s1".to_string(), vec![], 0).unwrap();
        mgr.create_snapshot("s2".to_string(), vec![], 0).unwrap();
        
        let result = mgr.create_snapshot("s3".to_string(), vec![], 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_snapshot() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());
        
        let info = mgr.create_snapshot("test".to_string(), vec![make_hash(1)], 100).unwrap();
        let deleted = mgr.delete_snapshot(info.id);
        
        assert!(deleted.is_some());
        assert!(mgr.get_snapshot(info.id).is_none());
    }

    #[test]
    fn test_get_snapshot() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());
        
        let hashes = vec![make_hash(1), make_hash(2)];
        let info = mgr.create_snapshot("test".to_string(), hashes.clone(), 200).unwrap();
        
        let snapshot = mgr.get_snapshot(info.id).unwrap();
        assert_eq!(snapshot.info.name, "test");
        assert_eq!(snapshot.block_hashes, hashes);
    }

    #[test]
    fn test_list_snapshots_sorted() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());
        
        mgr.create_snapshot("a".to_string(), vec![], 0).unwrap();
        mgr.create_snapshot("b".to_string(), vec![], 0).unwrap();
        mgr.create_snapshot("c".to_string(), vec![], 0).unwrap();
        
        let list = mgr.list_snapshots();
        assert_eq!(list.len(), 3);
        assert!(list[0].created_at_secs <= list[1].created_at_secs);
        assert!(list[1].created_at_secs <= list[2].created_at_secs);
    }

    #[test]
    fn test_clone_snapshot() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());
        
        let hashes = vec![make_hash(1), make_hash(2), make_hash(3)];
        let info = mgr.create_snapshot("original".to_string(), hashes.clone(), 300).unwrap();
        
        let cloned = mgr.clone_snapshot(info.id, "clone".to_string()).unwrap();
        
        assert_eq!(cloned.name, "clone");
        assert_eq!(cloned.block_count, 3);
        assert_eq!(cloned.total_bytes, 300);
    }

    #[test]
    fn test_clone_nonexistent_snapshot() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());
        
        let result = mgr.clone_snapshot(999, "test".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_find_by_name() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());
        
        let info = mgr.create_snapshot("myname".to_string(), vec![], 0).unwrap();
        
        let found = mgr.find_by_name("myname").unwrap();
        assert_eq!(found.info.id, info.id);
        
        assert!(mgr.find_by_name("nonexistent").is_none());
    }
}
