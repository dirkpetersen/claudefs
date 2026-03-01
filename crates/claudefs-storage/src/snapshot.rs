//! Copy-on-Write block-level snapshot management.
//!
//! This module provides CoW snapshots for point-in-time consistent views of data.
//! Blocks are not copied until modified, enabling efficient snapshot creation.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::block::{BlockId, BlockSize};
use crate::error::{StorageError, StorageResult};

/// Unique identifier for a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub u64);

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SnapshotId({})", self.0)
    }
}

/// State of a snapshot in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotState {
    /// Snapshot is active and usable.
    Active,
    /// Snapshot is being garbage collected.
    Deleting,
    /// Snapshot has been fully cleaned up.
    Deleted,
}

/// Information about a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// Unique snapshot identifier.
    pub id: SnapshotId,
    /// Human-readable name.
    pub name: String,
    /// Parent snapshot for incremental snapshots.
    pub parent_id: Option<SnapshotId>,
    /// Creation timestamp (seconds since epoch).
    pub created_at_secs: u64,
    /// Current state of the snapshot.
    pub state: SnapshotState,
    /// Number of blocks referenced by this snapshot.
    pub blocks_referenced: u64,
    /// Number of blocks that have been copied on write.
    pub cow_blocks: u64,
}

/// CoW mapping: original block to snapshot copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CowMapping {
    /// The original block (before CoW).
    pub original: BlockId,
    /// The CoW copy for this snapshot.
    pub snapshot_copy: BlockId,
    /// Which snapshot owns this mapping.
    pub snapshot_id: SnapshotId,
    /// Size of the block.
    pub block_size: BlockSize,
}

/// Statistics for snapshot operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotStats {
    /// Total snapshots created.
    pub snapshots_created: u64,
    /// Total snapshots deleted.
    pub snapshots_deleted: u64,
    /// Total CoW operations performed.
    pub cow_operations: u64,
    /// Blocks with refcount > 1 (shared).
    pub blocks_shared: u64,
    /// Estimated bytes saved by CoW.
    pub space_saved_bytes: u64,
}

/// Manager for CoW snapshots.
pub struct SnapshotManager {
    /// All snapshots indexed by ID.
    snapshots: HashMap<SnapshotId, SnapshotInfo>,
    /// CoW mappings per snapshot: (snapshot_id, original_block) -> CowMapping.
    cow_mappings: HashMap<(SnapshotId, BlockId), CowMapping>,
    /// Reference counts for shared blocks.
    block_refcounts: HashMap<BlockId, u32>,
    /// Next available snapshot ID.
    next_id: u64,
    /// Statistics.
    stats: SnapshotStats,
}

impl SnapshotManager {
    /// Creates a new snapshot manager.
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
            cow_mappings: HashMap::new(),
            block_refcounts: HashMap::new(),
            next_id: 1,
            stats: SnapshotStats::default(),
        }
    }

    /// Creates a new snapshot.
    pub fn create_snapshot(
        &mut self,
        name: &str,
        parent: Option<SnapshotId>,
    ) -> StorageResult<SnapshotId> {
        let id = SnapshotId(self.next_id);
        self.next_id += 1;

        let created_at_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let info = SnapshotInfo {
            id,
            name: name.to_string(),
            parent_id: parent,
            created_at_secs,
            state: SnapshotState::Active,
            blocks_referenced: 0,
            cow_blocks: 0,
        };

        debug!(snapshot_id = %id, name = %name, parent = ?parent, "Creating snapshot");
        info!(snapshot_id = %id, name = %name, "Created snapshot");

        self.snapshots.insert(id, info);
        self.stats.snapshots_created += 1;

        Ok(id)
    }

    /// Marks a snapshot for deletion.
    pub fn delete_snapshot(&mut self, id: SnapshotId) -> StorageResult<()> {
        let info = self
            .snapshots
            .get_mut(&id)
            .ok_or(StorageError::SnapshotNotFound { snapshot_id: id.0 })?;

        if info.state == SnapshotState::Deleted {
            return Err(StorageError::InvalidSnapshotState {
                snapshot_id: id.0,
                state: "Deleted",
            });
        }

        debug!(snapshot_id = %id, "Marking snapshot for deletion");
        info.state = SnapshotState::Deleting;

        Ok(())
    }

    /// Gets snapshot info by ID.
    pub fn get_snapshot(&self, id: SnapshotId) -> Option<&SnapshotInfo> {
        self.snapshots.get(&id)
    }

    /// Lists all snapshots.
    pub fn list_snapshots(&self) -> Vec<&SnapshotInfo> {
        self.snapshots.values().collect()
    }

    /// Records a CoW mapping for a block.
    pub fn cow_block(
        &mut self,
        snapshot_id: SnapshotId,
        original: BlockId,
        copy: BlockId,
        size: BlockSize,
    ) -> StorageResult<()> {
        let info = self
            .snapshots
            .get_mut(&snapshot_id)
            .ok_or(StorageError::SnapshotNotFound {
                snapshot_id: snapshot_id.0,
            })?;

        if info.state != SnapshotState::Active {
            return Err(StorageError::InvalidSnapshotState {
                snapshot_id: snapshot_id.0,
                state: "Active",
            });
        }

        let mapping = CowMapping {
            original,
            snapshot_copy: copy,
            snapshot_id,
            block_size: size,
        };

        debug!(snapshot_id = %snapshot_id, original = %original, copy = %copy, "Recording CoW mapping");
        self.cow_mappings.insert((snapshot_id, original), mapping);
        info.cow_blocks += 1;
        self.stats.cow_operations += 1;

        Ok(())
    }

    /// Resolves a block ID for a snapshot.
    /// Returns the CoW copy if it exists, otherwise the original block.
    pub fn resolve_block(&self, snapshot_id: SnapshotId, block_id: BlockId) -> BlockId {
        self.cow_mappings
            .get(&(snapshot_id, block_id))
            .map(|m| m.snapshot_copy)
            .unwrap_or(block_id)
    }

    /// Increments the reference count for a block.
    pub fn increment_ref(&mut self, block_id: BlockId) {
        let count = self.block_refcounts.entry(block_id).or_insert(0);
        *count += 1;
        debug!(block_id = %block_id, refcount = %count, "Incremented block refcount");
    }

    /// Decrements the reference count for a block.
    /// Returns the new reference count (0 means block can be freed).
    pub fn decrement_ref(&mut self, block_id: BlockId) -> u32 {
        let count = self.block_refcounts.entry(block_id).or_insert(0);
        if *count > 0 {
            *count -= 1;
        }
        debug!(block_id = %block_id, refcount = %count, "Decremented block refcount");
        *count
    }

    /// Gets the reference count for a block.
    pub fn refcount(&self, block_id: BlockId) -> u32 {
        self.block_refcounts.get(&block_id).copied().unwrap_or(0)
    }

    /// Returns the total number of snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Returns the number of CoW blocks for a snapshot.
    pub fn cow_count(&self, snapshot_id: SnapshotId) -> usize {
        self.cow_mappings
            .keys()
            .filter(|(id, _)| *id == snapshot_id)
            .count()
    }

    /// Returns snapshot statistics.
    pub fn stats(&self) -> &SnapshotStats {
        &self.stats
    }

    /// Returns snapshots that are candidates for garbage collection.
    /// These are snapshots in Deleting state with zero refcounts.
    pub fn gc_candidates(&self) -> Vec<SnapshotId> {
        self.snapshots
            .iter()
            .filter(|(_, info)| {
                info.state == SnapshotState::Deleting
                    && self.block_refcounts.values().all(|&c| c == 0)
            })
            .map(|(&id, _)| id)
            .collect()
    }
}

impl Default for SnapshotManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_manager() -> SnapshotManager {
        SnapshotManager::new()
    }

    #[test]
    fn test_create_snapshot() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test-snap", None).unwrap();
        assert_eq!(id.0, 1);
        let info = mgr.get_snapshot(id).unwrap();
        assert_eq!(info.name, "test-snap");
        assert_eq!(info.state, SnapshotState::Active);
        assert!(info.parent_id.is_none());
    }

    #[test]
    fn test_delete_snapshot() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test-snap", None).unwrap();
        mgr.delete_snapshot(id).unwrap();
        let info = mgr.get_snapshot(id).unwrap();
        assert_eq!(info.state, SnapshotState::Deleting);
    }

    #[test]
    fn test_get_snapshot() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test-snap", None).unwrap();
        let info = mgr.get_snapshot(id);
        assert!(info.is_some());
        assert_eq!(info.unwrap().name, "test-snap");

        let nonexistent = mgr.get_snapshot(SnapshotId(999));
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_list_snapshots() {
        let mut mgr = create_manager();
        mgr.create_snapshot("snap1", None).unwrap();
        mgr.create_snapshot("snap2", None).unwrap();
        let list = mgr.list_snapshots();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_cow_block_mapping() {
        let mut mgr = create_manager();
        let snap_id = mgr.create_snapshot("test-snap", None).unwrap();
        let original = BlockId::new(0, 100);
        let copy = BlockId::new(1, 200);

        mgr.cow_block(snap_id, original, copy, BlockSize::B4K)
            .unwrap();

        let info = mgr.get_snapshot(snap_id).unwrap();
        assert_eq!(info.cow_blocks, 1);
    }

    #[test]
    fn test_resolve_block_original() {
        let mgr = create_manager();
        let block = BlockId::new(0, 100);
        let resolved = mgr.resolve_block(SnapshotId(1), block);
        assert_eq!(resolved, block);
    }

    #[test]
    fn test_resolve_block_with_cow() {
        let mut mgr = create_manager();
        let snap_id = mgr.create_snapshot("test-snap", None).unwrap();
        let original = BlockId::new(0, 100);
        let copy = BlockId::new(1, 200);

        mgr.cow_block(snap_id, original, copy, BlockSize::B4K)
            .unwrap();

        let resolved = mgr.resolve_block(snap_id, original);
        assert_eq!(resolved, copy);
    }

    #[test]
    fn test_reference_counting_increment() {
        let mut mgr = create_manager();
        let block = BlockId::new(0, 100);
        mgr.increment_ref(block);
        assert_eq!(mgr.refcount(block), 1);
        mgr.increment_ref(block);
        assert_eq!(mgr.refcount(block), 2);
    }

    #[test]
    fn test_reference_counting_decrement() {
        let mut mgr = create_manager();
        let block = BlockId::new(0, 100);
        mgr.increment_ref(block);
        mgr.increment_ref(block);
        assert_eq!(mgr.decrement_ref(block), 1);
        assert_eq!(mgr.decrement_ref(block), 0);
    }

    #[test]
    fn test_block_freed_when_refcount_zero() {
        let mut mgr = create_manager();
        let block = BlockId::new(0, 100);
        mgr.increment_ref(block);
        assert_eq!(mgr.decrement_ref(block), 0);
        assert_eq!(mgr.refcount(block), 0);
    }

    #[test]
    fn test_stats_tracking() {
        let mut mgr = create_manager();
        mgr.create_snapshot("snap1", None).unwrap();
        mgr.create_snapshot("snap2", None).unwrap();

        let stats = mgr.stats();
        assert_eq!(stats.snapshots_created, 2);
    }

    #[test]
    fn test_create_with_parent() {
        let mut mgr = create_manager();
        let parent = mgr.create_snapshot("parent", None).unwrap();
        let child = mgr.create_snapshot("child", Some(parent)).unwrap();

        let info = mgr.get_snapshot(child).unwrap();
        assert_eq!(info.parent_id, Some(parent));
    }

    #[test]
    fn test_multiple_snapshots_of_same_data() {
        let mut mgr = create_manager();
        let id1 = mgr.create_snapshot("snap1", None).unwrap();
        let id2 = mgr.create_snapshot("snap2", None).unwrap();
        let block = BlockId::new(0, 100);
        let copy1 = BlockId::new(1, 50);
        let copy2 = BlockId::new(1, 60);

        mgr.cow_block(id1, block, copy1, BlockSize::B4K).unwrap();
        mgr.cow_block(id2, block, copy2, BlockSize::B4K).unwrap();

        assert_eq!(mgr.resolve_block(id1, block), copy1);
        assert_eq!(mgr.resolve_block(id2, block), copy2);
    }

    #[test]
    fn test_gc_candidates() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test-snap", None).unwrap();
        mgr.delete_snapshot(id).unwrap();

        let block = BlockId::new(0, 100);
        mgr.increment_ref(block);
        mgr.decrement_ref(block);

        let candidates = mgr.gc_candidates();
        assert!(candidates.contains(&id));
    }

    #[test]
    fn test_snapshot_name_stored() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("my-snapshot", None).unwrap();
        let info = mgr.get_snapshot(id).unwrap();
        assert_eq!(info.name, "my-snapshot");
    }

    #[test]
    fn test_delete_nonexistent_returns_error() {
        let mut mgr = create_manager();
        let result = mgr.delete_snapshot(SnapshotId(999));
        assert!(result.is_err());
    }

    #[test]
    fn test_cow_count_per_snapshot() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test-snap", None).unwrap();
        let block1 = BlockId::new(0, 100);
        let block2 = BlockId::new(0, 200);

        mgr.cow_block(id, block1, BlockId::new(1, 50), BlockSize::B4K)
            .unwrap();
        mgr.cow_block(id, block2, BlockId::new(1, 60), BlockSize::B4K)
            .unwrap();

        assert_eq!(mgr.cow_count(id), 2);
    }

    #[test]
    fn test_snapshot_id_display() {
        let id = SnapshotId(42);
        assert_eq!(format!("{}", id), "SnapshotId(42)");
    }

    #[test]
    fn test_snapshot_state_transitions() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test", None).unwrap();
        let info = mgr.get_snapshot(id).unwrap();
        assert_eq!(info.state, SnapshotState::Active);

        mgr.delete_snapshot(id).unwrap();
        let info = mgr.get_snapshot(id).unwrap();
        assert_eq!(info.state, SnapshotState::Deleting);
    }

    #[test]
    fn test_multiple_cow_blocks() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test", None).unwrap();

        for i in 0..10 {
            let original = BlockId::new(0, i);
            let copy = BlockId::new(1, i + 100);
            mgr.cow_block(id, original, copy, BlockSize::B4K).unwrap();
        }

        assert_eq!(mgr.cow_count(id), 10);
    }

    #[test]
    fn test_incremental_snapshot_chain() {
        let mut mgr = create_manager();
        let root = mgr.create_snapshot("root", None).unwrap();
        let child1 = mgr.create_snapshot("child1", Some(root)).unwrap();
        let child2 = mgr.create_snapshot("child2", Some(child1)).unwrap();
        let child3 = mgr.create_snapshot("child3", Some(child2)).unwrap();

        assert_eq!(mgr.get_snapshot(root).unwrap().parent_id, None);
        assert_eq!(mgr.get_snapshot(child1).unwrap().parent_id, Some(root));
        assert_eq!(mgr.get_snapshot(child2).unwrap().parent_id, Some(child1));
        assert_eq!(mgr.get_snapshot(child3).unwrap().parent_id, Some(child2));
    }

    #[test]
    fn test_stats_after_cow_operations() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test", None).unwrap();

        mgr.cow_block(id, BlockId::new(0, 1), BlockId::new(1, 1), BlockSize::B4K)
            .unwrap();
        mgr.cow_block(id, BlockId::new(0, 2), BlockId::new(1, 2), BlockSize::B4K)
            .unwrap();

        let stats = mgr.stats();
        assert_eq!(stats.cow_operations, 2);
    }

    #[test]
    fn test_snapshot_count() {
        let mut mgr = create_manager();
        mgr.create_snapshot("snap1", None).unwrap();
        mgr.create_snapshot("snap2", None).unwrap();
        mgr.create_snapshot("snap3", None).unwrap();

        assert_eq!(mgr.snapshot_count(), 3);
    }

    #[test]
    fn test_snapshot_info_fields() {
        let mut mgr = create_manager();
        let id = mgr
            .create_snapshot("test-snap", Some(SnapshotId(5)))
            .unwrap();
        let info = mgr.get_snapshot(id).unwrap();

        assert_eq!(info.id, id);
        assert!(!info.name.is_empty());
        assert!(info.created_at_secs > 0);
        assert_eq!(info.state, SnapshotState::Active);
    }

    #[test]
    fn test_cow_mapping_fields() {
        let mut mgr = create_manager();
        let id = mgr.create_snapshot("test", None).unwrap();
        let original = BlockId::new(0, 100);
        let copy = BlockId::new(1, 200);

        mgr.cow_block(id, original, copy, BlockSize::B1M).unwrap();

        let key = (id, original);
        let mapping = mgr.cow_mappings.get(&key).unwrap();
        assert_eq!(mapping.original, original);
        assert_eq!(mapping.snapshot_copy, copy);
        assert_eq!(mapping.snapshot_id, id);
        assert_eq!(mapping.block_size, BlockSize::B1M);
    }
}
