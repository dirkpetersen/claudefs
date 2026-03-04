//! Snapshot management for the FUSE filesystem.
//!
//! Provides copy-on-write snapshot creation, cloning, and lifecycle management.

use crate::error::{FuseError, Result};
use std::collections::HashMap;

/// Lifecycle state of a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotState {
    /// Snapshot is being created.
    Creating,
    /// Snapshot is fully created and usable.
    Active,
    /// Snapshot is being deleted.
    Deleting,
    /// Snapshot encountered an error; contains error message.
    Error(String),
}

/// Metadata for a single snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    /// Unique snapshot identifier.
    pub id: u64,
    /// User-assigned snapshot name.
    pub name: String,
    /// Creation timestamp in seconds since Unix epoch.
    pub created_at_secs: u64,
    /// Total size in bytes.
    pub size_bytes: u64,
    /// Current lifecycle state.
    pub state: SnapshotState,
    /// True if this is a writable clone of another snapshot.
    pub is_clone: bool,
}

impl SnapshotInfo {
    /// Creates a new snapshot info with initial state `Creating`.
    pub fn new(id: u64, name: impl Into<String>) -> Self {
        SnapshotInfo {
            id,
            name: name.into(),
            created_at_secs: 0,
            size_bytes: 0,
            state: SnapshotState::Creating,
            is_clone: false,
        }
    }

    /// Returns `true` if the snapshot is in the `Active` state.
    pub fn is_active(&self) -> bool {
        matches!(self.state, SnapshotState::Active)
    }

    /// Returns `true` if the snapshot is active and not a writable clone.
    pub fn is_read_only(&self) -> bool {
        !self.is_clone && matches!(self.state, SnapshotState::Active)
    }

    /// Returns the age of the snapshot in seconds.
    ///
    /// Uses saturating subtraction, so returns 0 if `now_secs` is less than creation time.
    pub fn age_secs(&self, now_secs: u64) -> u64 {
        now_secs.saturating_sub(self.created_at_secs)
    }
}

/// Registry managing all snapshots for a filesystem.
pub struct SnapshotRegistry {
    snapshots: HashMap<u64, SnapshotInfo>,
    next_id: u64,
    max_snapshots: usize,
}

impl SnapshotRegistry {
    /// Creates a new registry with the given maximum snapshot capacity.
    pub fn new(max_snapshots: usize) -> Self {
        SnapshotRegistry {
            snapshots: HashMap::new(),
            next_id: 1,
            max_snapshots,
        }
    }

    /// Creates a new read-only snapshot.
    ///
    /// Returns the assigned snapshot ID on success.
    /// Fails if capacity is exceeded or a snapshot with the same name exists.
    pub fn create(&mut self, name: impl Into<String>, now_secs: u64) -> Result<u64> {
        if self.snapshots.len() >= self.max_snapshots {
            return Err(FuseError::InvalidArgument {
                msg: "snapshot capacity exceeded".to_string(),
            });
        }

        let name = name.into();
        if self.find_by_name(&name).is_some() {
            return Err(FuseError::AlreadyExists { name });
        }

        let id = self.next_id;
        self.next_id += 1;

        let mut info = SnapshotInfo::new(id, name);
        info.created_at_secs = now_secs;
        info.state = SnapshotState::Active;

        self.snapshots.insert(id, info);
        Ok(id)
    }

    /// Creates a writable clone of an existing snapshot.
    ///
    /// Returns the assigned clone ID on success.
    /// Fails if the source snapshot doesn't exist, capacity is exceeded,
    /// or a snapshot with the clone name already exists.
    pub fn create_clone(
        &mut self,
        snapshot_id: u64,
        clone_name: impl Into<String>,
        now_secs: u64,
    ) -> Result<u64> {
        let snapshot = self
            .snapshots
            .get(&snapshot_id)
            .ok_or(FuseError::NotFound { ino: snapshot_id })?;

        if self.snapshots.len() >= self.max_snapshots {
            return Err(FuseError::InvalidArgument {
                msg: "snapshot capacity exceeded".to_string(),
            });
        }

        let clone_name = clone_name.into();
        if self.find_by_name(&clone_name).is_some() {
            return Err(FuseError::AlreadyExists { name: clone_name });
        }

        let id = self.next_id;
        self.next_id += 1;

        let mut info = SnapshotInfo::new(id, clone_name);
        info.created_at_secs = now_secs;
        info.size_bytes = snapshot.size_bytes;
        info.state = SnapshotState::Active;
        info.is_clone = true;

        self.snapshots.insert(id, info);
        Ok(id)
    }

    /// Deletes a snapshot by ID.
    ///
    /// Fails if the snapshot does not exist.
    pub fn delete(&mut self, id: u64) -> Result<()> {
        if self.snapshots.remove(&id).is_none() {
            return Err(FuseError::NotFound { ino: id });
        }
        Ok(())
    }

    /// Returns a reference to the snapshot with the given ID, if it exists.
    pub fn get(&self, id: u64) -> Option<&SnapshotInfo> {
        self.snapshots.get(&id)
    }

    /// Returns all snapshots sorted by creation time (oldest first).
    pub fn list(&self) -> Vec<&SnapshotInfo> {
        let mut list: Vec<_> = self.snapshots.values().collect();
        list.sort_by_key(|s| s.created_at_secs);
        list
    }

    /// Returns the total number of snapshots (including clones).
    pub fn count(&self) -> usize {
        self.snapshots.len()
    }

    /// Finds a snapshot by name.
    pub fn find_by_name(&self, name: &str) -> Option<&SnapshotInfo> {
        self.snapshots.values().find(|s| s.name == name)
    }

    /// Returns the count of snapshots in the `Active` state.
    pub fn active_count(&self) -> usize {
        self.snapshots.values().filter(|s| s.is_active()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_registry() -> SnapshotRegistry {
        SnapshotRegistry::new(10)
    }

    #[test]
    fn test_new_snapshot_info() {
        let info = SnapshotInfo::new(1, "test");
        assert_eq!(info.id, 1);
        assert_eq!(info.name, "test");
        assert_eq!(info.state, SnapshotState::Creating);
        assert!(!info.is_clone);
    }

    #[test]
    fn test_create_snapshot() {
        let mut registry = create_test_registry();
        let id = registry.create("snap1", 1000).unwrap();
        assert_eq!(id, 1);

        let info = registry.get(id).unwrap();
        assert_eq!(info.name, "snap1");
        assert_eq!(info.created_at_secs, 1000);
        assert!(info.is_active());
    }

    #[test]
    fn test_delete_snapshot() {
        let mut registry = create_test_registry();
        let id = registry.create("snap1", 1000).unwrap();

        registry.delete(id).unwrap();
        assert!(registry.get(id).is_none());
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut registry = create_test_registry();
        let result = registry.delete(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_snapshots() {
        let mut registry = create_test_registry();
        registry.create("snap1", 1000).unwrap();
        registry.create("snap2", 500).unwrap();
        registry.create("snap3", 1500).unwrap();

        let list = registry.list();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "snap2");
        assert_eq!(list[1].name, "snap1");
        assert_eq!(list[2].name, "snap3");
    }

    #[test]
    fn test_find_by_name() {
        let mut registry = create_test_registry();
        registry.create("snap1", 1000).unwrap();

        let found = registry.find_by_name("snap1").unwrap();
        assert_eq!(found.id, 1);

        assert!(registry.find_by_name("nonexistent").is_none());
    }

    #[test]
    fn test_capacity_limit() {
        let mut registry = SnapshotRegistry::new(2);
        registry.create("snap1", 1000).unwrap();
        registry.create("snap2", 1000).unwrap();

        let result = registry.create("snap3", 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_name_error() {
        let mut registry = create_test_registry();
        registry.create("snap1", 1000).unwrap();

        let result = registry.create("snap1", 1000);
        assert!(matches!(result, Err(FuseError::AlreadyExists { .. })));
    }

    #[test]
    fn test_age_secs() {
        let mut info = SnapshotInfo::new(1, "test");
        info.created_at_secs = 1000;

        assert_eq!(info.age_secs(1500), 500);
        assert_eq!(info.age_secs(500), 0);
    }

    #[test]
    fn test_is_read_only() {
        let mut info = SnapshotInfo::new(1, "test");
        info.state = SnapshotState::Active;

        assert!(info.is_read_only());

        info.is_clone = true;
        assert!(!info.is_read_only());

        info.is_clone = false;
        info.state = SnapshotState::Creating;
        assert!(!info.is_read_only());
    }

    #[test]
    fn test_create_clone() {
        let mut registry = create_test_registry();
        registry.create("snap1", 1000).unwrap();

        let clone_id = registry.create_clone(1, "clone1", 1500).unwrap();
        let clone = registry.get(clone_id).unwrap();

        assert!(clone.is_clone);
        assert_eq!(clone.created_at_secs, 1500);
    }

    #[test]
    fn test_create_clone_nonexistent() {
        let mut registry = create_test_registry();
        let result = registry.create_clone(999, "clone1", 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_active_count() {
        let mut registry = create_test_registry();
        registry.create("snap1", 1000).unwrap();

        let id2 = registry.create("snap2", 1000).unwrap();
        assert_eq!(registry.active_count(), 2);

        registry.delete(id2).unwrap();
        assert_eq!(registry.active_count(), 1);
    }

    #[test]
    fn test_count() {
        let mut registry = create_test_registry();
        assert_eq!(registry.count(), 0);

        registry.create("snap1", 1000).unwrap();
        assert_eq!(registry.count(), 1);

        registry.create("snap2", 1000).unwrap();
        assert_eq!(registry.count(), 2);
    }

    #[test]
    fn test_is_active() {
        let mut info = SnapshotInfo::new(1, "test");
        assert!(!info.is_active());

        info.state = SnapshotState::Active;
        assert!(info.is_active());

        info.state = SnapshotState::Deleting;
        assert!(!info.is_active());
    }
}
