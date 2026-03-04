//! Snapshot catalog for efficient snapshot management and space accounting.
//!
//! Tracks snapshot metadata including unique vs shared chunks for
//! understanding storage efficiency and space reclamation opportunities.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub u64);

/// Metadata record for a single snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    /// Unique identifier for this snapshot.
    pub id: SnapshotId,
    /// Human-readable name.
    pub name: String,
    /// Creation timestamp in milliseconds since Unix epoch.
    pub created_at_ms: u64,
    /// Total number of inodes captured in this snapshot.
    pub inode_count: u64,
    /// Number of chunks unique to this snapshot (not shared with others).
    pub unique_chunk_count: u64,
    /// Number of chunks shared with other snapshots.
    pub shared_chunk_count: u64,
    /// Total bytes across all chunks (unique + shared).
    pub total_bytes: u64,
    /// Bytes in unique chunks only.
    pub unique_bytes: u64,
}

impl SnapshotRecord {
    /// Returns bytes in shared chunks.
    pub fn shared_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.unique_bytes)
    }

    /// Returns space amplification factor (total_bytes / unique_bytes).
    /// Returns 1.0 if unique_bytes is 0.
    pub fn space_amplification(&self) -> f64 {
        if self.unique_bytes == 0 {
            return 1.0;
        }
        self.total_bytes as f64 / self.unique_bytes as f64
    }
}

/// Catalog tracking all snapshots with efficient lookup.
pub struct SnapshotCatalog {
    snapshots: HashMap<SnapshotId, SnapshotRecord>,
    name_index: HashMap<String, SnapshotId>,
    next_id: u64,
}

impl Default for SnapshotCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotCatalog {
    /// Creates an empty catalog.
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
            name_index: HashMap::new(),
            next_id: 1,
        }
    }

    /// Adds a snapshot record and returns its assigned ID.
    pub fn add(&mut self, mut record: SnapshotRecord) -> SnapshotId {
        let id = SnapshotId(self.next_id);
        self.next_id += 1;
        record.id = id;
        self.name_index.insert(record.name.clone(), id);
        self.snapshots.insert(id, record);
        id
    }

    /// Retrieves a snapshot by ID.
    pub fn get(&self, id: SnapshotId) -> Option<&SnapshotRecord> {
        self.snapshots.get(&id)
    }

    /// Retrieves a snapshot by name.
    pub fn get_by_name(&self, name: &str) -> Option<&SnapshotRecord> {
        self.name_index
            .get(name)
            .and_then(|id| self.snapshots.get(id))
    }

    /// Lists all snapshots sorted by creation time ascending.
    pub fn list(&self) -> Vec<&SnapshotRecord> {
        let mut records: Vec<_> = self.snapshots.values().collect();
        records.sort_by_key(|r| r.created_at_ms);
        records
    }

    /// Deletes a snapshot by ID. Returns true if found and removed.
    pub fn delete(&mut self, id: SnapshotId) -> bool {
        if let Some(record) = self.snapshots.remove(&id) {
            self.name_index.remove(&record.name);
            true
        } else {
            false
        }
    }

    /// Returns the number of snapshots.
    pub fn count(&self) -> usize {
        self.snapshots.len()
    }

    /// Returns sum of unique bytes across all snapshots.
    pub fn total_unique_bytes(&self) -> u64 {
        self.snapshots.values().map(|r| r.unique_bytes).sum()
    }

    /// Returns sum of shared bytes across all snapshots.
    pub fn total_shared_bytes(&self) -> u64 {
        self.snapshots.values().map(|r| r.shared_bytes()).sum()
    }

    /// Returns the oldest snapshot by creation time.
    pub fn oldest(&self) -> Option<&SnapshotRecord> {
        self.snapshots.values().min_by_key(|r| r.created_at_ms)
    }

    /// Returns the newest snapshot by creation time.
    pub fn newest(&self) -> Option<&SnapshotRecord> {
        self.snapshots.values().max_by_key(|r| r.created_at_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn snapshot_id_equality() {
        let id1 = SnapshotId(1);
        let id2 = SnapshotId(1);
        let id3 = SnapshotId(2);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn new_catalog_is_empty() {
        let catalog = SnapshotCatalog::new();
        assert!(catalog.list().is_empty());
        assert_eq!(catalog.count(), 0);
    }

    #[test]
    fn add_snapshot_returns_id() {
        let mut catalog = SnapshotCatalog::new();
        let record = SnapshotRecord {
            id: SnapshotId(0),
            name: "snap1".to_string(),
            created_at_ms: 1000,
            inode_count: 10,
            unique_chunk_count: 5,
            shared_chunk_count: 3,
            total_bytes: 1000,
            unique_bytes: 400,
        };

        let id = catalog.add(record);
        assert_eq!(id, SnapshotId(1));
        assert_eq!(catalog.count(), 1);
    }

    #[test]
    fn get_snapshot_found() {
        let mut catalog = SnapshotCatalog::new();
        let record = SnapshotRecord {
            id: SnapshotId(0),
            name: "snap1".to_string(),
            created_at_ms: 1000,
            inode_count: 10,
            unique_chunk_count: 5,
            shared_chunk_count: 3,
            total_bytes: 1000,
            unique_bytes: 400,
        };

        let id = catalog.add(record);
        let found = catalog.get(id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "snap1");
    }

    #[test]
    fn get_snapshot_not_found() {
        let catalog = SnapshotCatalog::new();
        assert!(catalog.get(SnapshotId(999)).is_none());
    }

    #[test]
    fn get_by_name_found() {
        let mut catalog = SnapshotCatalog::new();
        let record = SnapshotRecord {
            id: SnapshotId(0),
            name: "daily-backup".to_string(),
            created_at_ms: 1000,
            inode_count: 100,
            unique_chunk_count: 20,
            shared_chunk_count: 80,
            total_bytes: 10000,
            unique_bytes: 2000,
        };

        catalog.add(record);
        let found = catalog.get_by_name("daily-backup");
        assert!(found.is_some());
        assert_eq!(found.unwrap().inode_count, 100);
    }

    #[test]
    fn get_by_name_not_found() {
        let catalog = SnapshotCatalog::new();
        assert!(catalog.get_by_name("nonexistent").is_none());
    }

    #[test]
    fn list_sorted_by_time() {
        let mut catalog = SnapshotCatalog::new();

        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "snap3".to_string(),
            created_at_ms: 3000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });
        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "snap1".to_string(),
            created_at_ms: 1000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });
        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "snap2".to_string(),
            created_at_ms: 2000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });

        let list = catalog.list();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "snap1");
        assert_eq!(list[1].name, "snap2");
        assert_eq!(list[2].name, "snap3");
    }

    #[test]
    fn delete_snapshot() {
        let mut catalog = SnapshotCatalog::new();
        let record = SnapshotRecord {
            id: SnapshotId(0),
            name: "snap1".to_string(),
            created_at_ms: 1000,
            inode_count: 10,
            unique_chunk_count: 5,
            shared_chunk_count: 3,
            total_bytes: 1000,
            unique_bytes: 400,
        };

        let id = catalog.add(record);
        assert!(catalog.delete(id));
        assert_eq!(catalog.count(), 0);
        assert!(catalog.get(id).is_none());
        assert!(catalog.get_by_name("snap1").is_none());
    }

    #[test]
    fn delete_unknown_returns_false() {
        let mut catalog = SnapshotCatalog::new();
        assert!(!catalog.delete(SnapshotId(999)));
    }

    #[test]
    fn count_increments() {
        let mut catalog = SnapshotCatalog::new();
        assert_eq!(catalog.count(), 0);

        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "s1".to_string(),
            created_at_ms: 1000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });
        assert_eq!(catalog.count(), 1);

        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "s2".to_string(),
            created_at_ms: 2000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });
        assert_eq!(catalog.count(), 2);
    }

    #[test]
    fn total_unique_bytes_sum() {
        let mut catalog = SnapshotCatalog::new();

        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "s1".to_string(),
            created_at_ms: 1000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 1000,
            unique_bytes: 300,
        });
        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "s2".to_string(),
            created_at_ms: 2000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 1000,
            unique_bytes: 200,
        });

        assert_eq!(catalog.total_unique_bytes(), 500);
    }

    #[test]
    fn total_shared_bytes_sum() {
        let mut catalog = SnapshotCatalog::new();

        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "s1".to_string(),
            created_at_ms: 1000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 1000,
            unique_bytes: 300,
        });
        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "s2".to_string(),
            created_at_ms: 2000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 1000,
            unique_bytes: 200,
        });

        assert_eq!(catalog.total_shared_bytes(), 1500);
    }

    #[test]
    fn oldest_empty_catalog() {
        let catalog = SnapshotCatalog::new();
        assert!(catalog.oldest().is_none());
    }

    #[test]
    fn oldest_with_snapshots() {
        let mut catalog = SnapshotCatalog::new();

        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "newer".to_string(),
            created_at_ms: 2000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });
        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "older".to_string(),
            created_at_ms: 1000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });

        let oldest = catalog.oldest().unwrap();
        assert_eq!(oldest.name, "older");
    }

    #[test]
    fn newest_with_snapshots() {
        let mut catalog = SnapshotCatalog::new();

        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "older".to_string(),
            created_at_ms: 1000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });
        catalog.add(SnapshotRecord {
            id: SnapshotId(0),
            name: "newer".to_string(),
            created_at_ms: 2000,
            inode_count: 1,
            unique_chunk_count: 1,
            shared_chunk_count: 0,
            total_bytes: 100,
            unique_bytes: 100,
        });

        let newest = catalog.newest().unwrap();
        assert_eq!(newest.name, "newer");
    }

    #[test]
    fn shared_bytes_calculation() {
        let record = SnapshotRecord {
            id: SnapshotId(1),
            name: "test".to_string(),
            created_at_ms: 1000,
            inode_count: 10,
            unique_chunk_count: 5,
            shared_chunk_count: 10,
            total_bytes: 10000,
            unique_bytes: 3000,
        };

        assert_eq!(record.shared_bytes(), 7000);
    }

    #[test]
    fn space_amplification() {
        let record = SnapshotRecord {
            id: SnapshotId(1),
            name: "test".to_string(),
            created_at_ms: 1000,
            inode_count: 10,
            unique_chunk_count: 5,
            shared_chunk_count: 10,
            total_bytes: 10000,
            unique_bytes: 2000,
        };

        let amp = record.space_amplification();
        assert!((amp - 5.0).abs() < 1e-10);
    }

    #[test]
    fn space_amplification_zero_unique() {
        let record = SnapshotRecord {
            id: SnapshotId(1),
            name: "test".to_string(),
            created_at_ms: 1000,
            inode_count: 10,
            unique_chunk_count: 0,
            shared_chunk_count: 10,
            total_bytes: 10000,
            unique_bytes: 0,
        };

        assert!((record.space_amplification() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn snapshot_id_hash_set() {
        let mut set = HashSet::new();
        set.insert(SnapshotId(1));
        set.insert(SnapshotId(2));
        set.insert(SnapshotId(1));

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn snapshot_record_clone() {
        let record = SnapshotRecord {
            id: SnapshotId(1),
            name: "original".to_string(),
            created_at_ms: 1000,
            inode_count: 10,
            unique_chunk_count: 5,
            shared_chunk_count: 3,
            total_bytes: 1000,
            unique_bytes: 400,
        };

        let cloned = record.clone();
        assert_eq!(cloned.id, record.id);
        assert_eq!(cloned.name, record.name);
    }

    #[test]
    fn snapshot_catalog_default() {
        let catalog = SnapshotCatalog::default();
        assert_eq!(catalog.count(), 0);
    }

    #[test]
    fn snapshot_id_debug_format() {
        let id = SnapshotId(42);
        let debug_str = format!("{:?}", id);
        assert!(debug_str.contains("42"));
    }
}
