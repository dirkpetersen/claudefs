//! Raft log snapshot and compaction for long-running Raft groups.
//!
//! This module provides snapshot management so the Raft log can be truncated
//! after a snapshot is taken, preventing unbounded log growth.

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

use crate::types::*;

/// A snapshot of the metadata state at a specific log index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RaftSnapshot {
    /// The log index this snapshot covers (all entries up to and including this index).
    pub last_included_index: LogIndex,
    /// The term of the last included log entry.
    pub last_included_term: Term,
    /// Serialized KV store state (all inode/dirent/xattr data).
    pub data: Vec<u8>,
    /// Timestamp when snapshot was created.
    pub created_at: Timestamp,
}

impl RaftSnapshot {
    /// Creates a new Raft snapshot.
    ///
    /// # Arguments
    /// * `last_included_index` - The log index this snapshot covers
    /// * `last_included_term` - The term of the last included log entry
    /// * `data` - Serialized state data
    pub fn new(last_included_index: LogIndex, last_included_term: Term, data: Vec<u8>) -> Self {
        Self {
            last_included_index,
            last_included_term,
            data,
            created_at: Timestamp::now(),
        }
    }
}

/// Manages Raft log snapshots and compaction.
pub struct SnapshotManager {
    /// The latest snapshot (if any).
    latest_snapshot: RwLock<Option<RaftSnapshot>>,
    /// Minimum log entries to keep before compacting.
    min_entries_before_compact: usize,
    /// Maximum log entries before forcing a snapshot.
    max_entries_before_snapshot: usize,
}

impl SnapshotManager {
    /// Creates a new snapshot manager.
    ///
    /// # Arguments
    /// * `min_entries_before_compact` - Minimum entries to keep before compaction
    /// * `max_entries_before_snapshot` - Maximum entries before triggering a snapshot
    pub fn new(min_entries_before_compact: usize, max_entries_before_snapshot: usize) -> Self {
        Self {
            latest_snapshot: RwLock::new(None),
            min_entries_before_compact,
            max_entries_before_snapshot,
        }
    }

    /// Creates and stores a new snapshot.
    ///
    /// # Arguments
    /// * `last_included_index` - The log index this snapshot covers
    /// * `last_included_term` - The term of the last included log entry
    /// * `data` - Serialized state data
    ///
    /// # Returns
    /// The created snapshot
    pub fn create_snapshot(
        &self,
        last_included_index: LogIndex,
        last_included_term: Term,
        data: Vec<u8>,
    ) -> RaftSnapshot {
        let snapshot = RaftSnapshot::new(last_included_index, last_included_term, data);
        let mut latest = self.latest_snapshot.write().unwrap();
        *latest = Some(snapshot.clone());
        tracing::info!(
            "Created snapshot at index {}, term {}",
            last_included_index.as_u64(),
            last_included_term.as_u64()
        );
        snapshot
    }

    /// Returns the latest snapshot, if one exists.
    pub fn latest_snapshot(&self) -> Option<RaftSnapshot> {
        let latest = self.latest_snapshot.read().unwrap();
        latest.clone()
    }

    /// Returns true if the log should be snapshotted based on current length.
    ///
    /// # Arguments
    /// * `current_log_len` - Current number of log entries
    pub fn should_snapshot(&self, current_log_len: usize) -> bool {
        current_log_len >= self.max_entries_before_snapshot
    }

    /// Returns the log index up to which entries can be discarded.
    ///
    /// Returns None if no snapshot exists.
    ///
    /// # Arguments
    /// * `current_log_len` - Current number of log entries
    pub fn compaction_point(&self, current_log_len: usize) -> Option<LogIndex> {
        let latest = self.latest_snapshot.read().unwrap();
        if let Some(snapshot) = latest.as_ref() {
            let min_index =
                snapshot.last_included_index.as_u64() as usize + self.min_entries_before_compact;
            if current_log_len > min_index {
                let index = std::cmp::min(
                    snapshot.last_included_index.as_u64(),
                    (current_log_len - self.min_entries_before_compact) as u64,
                );
                return Some(LogIndex::new(index));
            }
        }
        None
    }

    /// Restores state from a snapshot.
    ///
    /// # Arguments
    /// * `snapshot` - The snapshot to restore from
    ///
    /// # Returns
    /// The snapshot data for restoring state
    pub fn restore_snapshot(&self, snapshot: &RaftSnapshot) -> Vec<u8> {
        let mut latest = self.latest_snapshot.write().unwrap();
        *latest = Some(snapshot.clone());
        tracing::info!(
            "Restored snapshot from index {}, term {}",
            snapshot.last_included_index.as_u64(),
            snapshot.last_included_term.as_u64()
        );
        snapshot.data.clone()
    }

    /// Returns the number of snapshots stored (0 or 1).
    pub fn snapshot_count(&self) -> usize {
        let latest = self.latest_snapshot.read().unwrap();
        if latest.is_some() {
            1
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_snapshot() {
        let mgr = SnapshotManager::new(10, 100);
        let data = vec![1u8; 100];

        let snapshot = mgr.create_snapshot(LogIndex::new(50), Term::new(5), data.clone());

        assert_eq!(snapshot.last_included_index.as_u64(), 50);
        assert_eq!(snapshot.last_included_term.as_u64(), 5);
        assert_eq!(snapshot.data, data);
    }

    #[test]
    fn test_latest_snapshot_initially_none() {
        let mgr = SnapshotManager::new(10, 100);
        assert!(mgr.latest_snapshot().is_none());
    }

    #[test]
    fn test_should_snapshot_threshold() {
        let mgr = SnapshotManager::new(10, 100);

        assert!(!mgr.should_snapshot(99));
        assert!(mgr.should_snapshot(100));
        assert!(mgr.should_snapshot(150));
    }

    #[test]
    fn test_compaction_point() {
        let mgr = SnapshotManager::new(10, 100);
        mgr.create_snapshot(LogIndex::new(50), Term::new(5), vec![]);

        assert!(mgr.compaction_point(60).is_none());
        assert_eq!(mgr.compaction_point(70).unwrap().as_u64(), 50);
    }

    #[test]
    fn test_restore_snapshot() {
        let mgr = SnapshotManager::new(10, 100);
        let data = vec![1u8, 2u8, 3u8];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(10), data.clone());

        let restored = mgr.restore_snapshot(&snapshot);

        assert_eq!(restored, data);
        assert_eq!(
            mgr.latest_snapshot().unwrap().last_included_index.as_u64(),
            100
        );
    }

    #[test]
    fn test_snapshot_replaces_previous() {
        let mgr = SnapshotManager::new(10, 100);

        mgr.create_snapshot(LogIndex::new(50), Term::new(3), vec![1, 2, 3]);
        mgr.create_snapshot(LogIndex::new(100), Term::new(7), vec![4, 5, 6]);

        let latest = mgr.latest_snapshot().unwrap();
        assert_eq!(latest.last_included_index.as_u64(), 100);
    }

    #[test]
    fn test_should_snapshot_below_threshold() {
        let mgr = SnapshotManager::new(10, 100);

        assert!(!mgr.should_snapshot(0));
        assert!(!mgr.should_snapshot(50));
        assert!(!mgr.should_snapshot(99));
    }

    #[test]
    fn test_snapshot_count() {
        let mgr = SnapshotManager::new(10, 100);

        assert_eq!(mgr.snapshot_count(), 0);

        mgr.create_snapshot(LogIndex::new(50), Term::new(3), vec![]);

        assert_eq!(mgr.snapshot_count(), 1);
    }
}
