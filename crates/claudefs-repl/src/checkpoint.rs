//! Replication checkpoints for persistent replication state.
//!
//! Checkpoints provide a way to capture the full WAL state at a point in time,
//! enabling quick recovery and resynchronization after failures.

use crate::wal::ReplicationCursor;
use serde::{Deserialize, Serialize};

/// A replication checkpoint: full snapshot of WAL state at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplicationCheckpoint {
    /// Timestamp when checkpoint was created (microseconds since epoch).
    pub created_at_us: u64,
    /// Site ID this checkpoint belongs to.
    pub site_id: u64,
    /// The replication cursors at checkpoint time.
    pub cursors: Vec<ReplicationCursor>,
    /// A hash fingerprint of the cursors for quick comparison.
    pub fingerprint: u64,
    /// Number of cursors in this checkpoint.
    pub cursor_count: usize,
    /// Unique identifier for this checkpoint.
    pub checkpoint_id: u64,
}

impl ReplicationCheckpoint {
    /// Create a new checkpoint from a set of cursors.
    pub fn new(
        site_id: u64,
        checkpoint_id: u64,
        created_at_us: u64,
        cursors: Vec<ReplicationCursor>,
    ) -> Self {
        let fingerprint = Self::compute_fingerprint(&cursors);
        let cursor_count = cursors.len();
        Self {
            created_at_us,
            site_id,
            cursors,
            fingerprint,
            cursor_count,
            checkpoint_id,
        }
    }

    /// Compute the fingerprint from the cursors.
    pub fn compute_fingerprint(cursors: &[ReplicationCursor]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        // Sort cursors for deterministic fingerprint
        let mut sorted: Vec<_> = cursors.to_vec();
        sorted.sort_by_key(|c| (c.site_id, c.shard_id));
        sorted.hash(&mut hasher);
        hasher.finish()
    }

    /// Serialize to bincode bytes (for persistence).
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bincode bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }

    /// Compare fingerprints with another checkpoint.
    pub fn fingerprint_matches(&self, other: &ReplicationCheckpoint) -> bool {
        self.fingerprint == other.fingerprint
    }

    /// Returns the replication lag relative to another checkpoint.
    pub fn lag_vs(&self, other: &ReplicationCheckpoint) -> u64 {
        let self_max: u64 = self.cursors.iter().map(|c| c.last_seq).max().unwrap_or(0);
        let other_max: u64 = other.cursors.iter().map(|c| c.last_seq).max().unwrap_or(0);
        self_max.saturating_sub(other_max)
    }
}

/// Manages a rolling window of checkpoints.
#[derive(Debug)]
pub struct CheckpointManager {
    /// Site ID this manager is for.
    site_id: u64,
    /// The checkpoints.
    checkpoints: Vec<ReplicationCheckpoint>,
    /// Next checkpoint ID to assign.
    next_id: u64,
    /// Maximum number of checkpoints to keep.
    max_checkpoints: usize,
}

impl CheckpointManager {
    /// Create a new checkpoint manager.
    pub fn new(site_id: u64, max_checkpoints: usize) -> Self {
        Self {
            site_id,
            checkpoints: Vec::new(),
            next_id: 1,
            max_checkpoints,
        }
    }

    /// Create a new checkpoint from the current WAL state.
    /// Returns None if all checkpoints were pruned (e.g., max_checkpoints = 0).
    pub fn create(
        &mut self,
        cursors: Vec<ReplicationCursor>,
        created_at_us: u64,
    ) -> Option<&ReplicationCheckpoint> {
        let checkpoint =
            ReplicationCheckpoint::new(self.site_id, self.next_id, created_at_us, cursors);
        self.next_id += 1;
        self.checkpoints.push(checkpoint);
        self.prune();
        self.checkpoints.last()
    }

    /// Get the latest checkpoint.
    pub fn latest(&self) -> Option<&ReplicationCheckpoint> {
        self.checkpoints.last()
    }

    /// Get all checkpoints (oldest first).
    pub fn all(&self) -> &[ReplicationCheckpoint] {
        &self.checkpoints
    }

    /// Prune old checkpoints, keeping at most max_checkpoints.
    pub fn prune(&mut self) {
        while self.checkpoints.len() > self.max_checkpoints {
            self.checkpoints.remove(0);
        }
    }

    /// Find a checkpoint by ID.
    pub fn find_by_id(&self, checkpoint_id: u64) -> Option<&ReplicationCheckpoint> {
        self.checkpoints
            .iter()
            .find(|c| c.checkpoint_id == checkpoint_id)
    }

    /// Clear all checkpoints.
    pub fn clear(&mut self) {
        self.checkpoints.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod checkpoint_creation {
        use super::*;

        #[test]
        fn test_create_checkpoint_with_cursors() {
            let cursors = vec![
                ReplicationCursor::new(2, 0, 100),
                ReplicationCursor::new(2, 1, 200),
            ];
            let checkpoint = ReplicationCheckpoint::new(1, 1, 1000000, cursors.clone());

            assert_eq!(checkpoint.site_id, 1);
            assert_eq!(checkpoint.checkpoint_id, 1);
            assert_eq!(checkpoint.created_at_us, 1000000);
            assert_eq!(checkpoint.cursors.len(), 2);
            assert_eq!(checkpoint.cursor_count, 2);
        }

        #[test]
        fn test_checkpoint_empty_cursors() {
            let checkpoint = ReplicationCheckpoint::new(1, 1, 1000000, vec![]);

            assert!(checkpoint.cursors.is_empty());
            assert_eq!(checkpoint.cursor_count, 0);
        }

        #[test]
        fn test_checkpoint_with_many_cursors() {
            let mut cursors = Vec::new();
            for i in 0..256u32 {
                cursors.push(ReplicationCursor::new(2, i, i as u64 * 100));
            }
            let checkpoint = ReplicationCheckpoint::new(1, 1, 1000000, cursors);

            assert_eq!(checkpoint.cursor_count, 256);
        }
    }

    mod fingerprint {
        use super::*;

        #[test]
        fn test_fingerprint_determinism() {
            let cursors1 = vec![
                ReplicationCursor::new(2, 0, 100),
                ReplicationCursor::new(2, 1, 200),
            ];
            let cursors2 = vec![
                ReplicationCursor::new(2, 0, 100),
                ReplicationCursor::new(2, 1, 200),
            ];

            let fp1 = ReplicationCheckpoint::compute_fingerprint(&cursors1);
            let fp2 = ReplicationCheckpoint::compute_fingerprint(&cursors2);

            assert_eq!(fp1, fp2);
        }

        #[test]
        fn test_fingerprint_changes_when_cursor_changes() {
            let cursors1 = vec![
                ReplicationCursor::new(2, 0, 100),
                ReplicationCursor::new(2, 1, 200),
            ];
            let cursors2 = vec![
                ReplicationCursor::new(2, 0, 100),
                ReplicationCursor::new(2, 1, 201), // Changed
            ];

            let fp1 = ReplicationCheckpoint::compute_fingerprint(&cursors1);
            let fp2 = ReplicationCheckpoint::compute_fingerprint(&cursors2);

            assert_ne!(fp1, fp2);
        }

        #[test]
        fn test_fingerprint_empty_cursors() {
            let fp = ReplicationCheckpoint::compute_fingerprint(&[]);
            let fp2 = ReplicationCheckpoint::compute_fingerprint(&[]);
            assert_eq!(fp, fp2);
        }

        #[test]
        fn test_checkpoint_fingerprint_field() {
            let cursors = vec![ReplicationCursor::new(2, 0, 100)];
            let checkpoint = ReplicationCheckpoint::new(1, 1, 1000000, cursors);

            let computed = ReplicationCheckpoint::compute_fingerprint(&checkpoint.cursors);
            assert_eq!(checkpoint.fingerprint, computed);
        }
    }

    mod serialize_deserialize {
        use super::*;

        #[test]
        fn test_serialize_deserialize_roundtrip() {
            let original = ReplicationCheckpoint::new(
                1,
                42,
                1700000000000000,
                vec![
                    ReplicationCursor::new(2, 0, 100),
                    ReplicationCursor::new(2, 1, 200),
                ],
            );

            let bytes = original.to_bytes().unwrap();
            let restored = ReplicationCheckpoint::from_bytes(&bytes).unwrap();

            assert_eq!(original, restored);
        }

        #[test]
        fn test_serialize_empty_cursors() {
            let original = ReplicationCheckpoint::new(1, 1, 1000000, vec![]);

            let bytes = original.to_bytes().unwrap();
            let restored = ReplicationCheckpoint::from_bytes(&bytes).unwrap();

            assert!(restored.cursors.is_empty());
        }

        #[test]
        fn test_serialize_many_cursors() {
            let mut cursors = Vec::new();
            for i in 0..256u32 {
                cursors.push(ReplicationCursor::new(2, i, i as u64 * 100));
            }
            let original = ReplicationCheckpoint::new(1, 1, 1000000, cursors);

            let bytes = original.to_bytes().unwrap();
            let restored = ReplicationCheckpoint::from_bytes(&bytes).unwrap();

            assert_eq!(restored.cursor_count, 256);
        }
    }

    mod fingerprint_matches {
        use super::*;

        #[test]
        fn test_fingerprint_matches_true() {
            let cursors = vec![ReplicationCursor::new(2, 0, 100)];
            let cp1 = ReplicationCheckpoint::new(1, 1, 1000000, cursors.clone());
            let cp2 = ReplicationCheckpoint::new(1, 2, 1000001, cursors);

            assert!(cp1.fingerprint_matches(&cp2));
        }

        #[test]
        fn test_fingerprint_matches_false() {
            let cp1 =
                ReplicationCheckpoint::new(1, 1, 1000000, vec![ReplicationCursor::new(2, 0, 100)]);
            let cp2 =
                ReplicationCheckpoint::new(1, 2, 1000001, vec![ReplicationCursor::new(2, 0, 200)]);

            assert!(!cp1.fingerprint_matches(&cp2));
        }

        #[test]
        fn test_fingerprint_matches_empty() {
            let cp1 = ReplicationCheckpoint::new(1, 1, 1000000, vec![]);
            let cp2 = ReplicationCheckpoint::new(1, 2, 1000001, vec![]);

            assert!(cp1.fingerprint_matches(&cp2));
        }
    }

    mod lag_vs {
        use super::*;

        #[test]
        fn test_lag_vs_calculation() {
            let cp1 = ReplicationCheckpoint::new(
                1,
                1,
                1000000,
                vec![
                    ReplicationCursor::new(2, 0, 100),
                    ReplicationCursor::new(2, 1, 200),
                ],
            );
            let cp2 = ReplicationCheckpoint::new(
                1,
                2,
                1000001,
                vec![
                    ReplicationCursor::new(2, 0, 50),
                    ReplicationCursor::new(2, 1, 150),
                ],
            );

            assert_eq!(cp1.lag_vs(&cp2), 50); // max(100,200) - max(50,150) = 200 - 150 = 50
        }

        #[test]
        fn test_lag_vs_zero() {
            let cp1 =
                ReplicationCheckpoint::new(1, 1, 1000000, vec![ReplicationCursor::new(2, 0, 100)]);
            let cp2 =
                ReplicationCheckpoint::new(1, 2, 1000001, vec![ReplicationCursor::new(2, 0, 100)]);

            assert_eq!(cp1.lag_vs(&cp2), 0);
        }

        #[test]
        fn test_lag_vs_empty_cursors() {
            let cp1 = ReplicationCheckpoint::new(1, 1, 1000000, vec![]);
            let cp2 = ReplicationCheckpoint::new(1, 2, 1000001, vec![]);

            assert_eq!(cp1.lag_vs(&cp2), 0);
        }

        #[test]
        fn test_lag_vs_saturating() {
            let cp1 =
                ReplicationCheckpoint::new(1, 1, 1000000, vec![ReplicationCursor::new(2, 0, 100)]);
            let cp2 = ReplicationCheckpoint::new(
                1,
                2,
                1000001,
                vec![ReplicationCursor::new(2, 0, 200)], // cp2 is ahead
            );

            assert_eq!(cp1.lag_vs(&cp2), 0); // saturating subtraction
        }
    }

    mod checkpoint_manager {
        use super::*;

        #[test]
        fn test_create_checkpoint() {
            let mut manager = CheckpointManager::new(1, 10);
            let cursors = vec![ReplicationCursor::new(2, 0, 100)];
            let checkpoint = manager.create(cursors, 1000000).unwrap();

            assert_eq!(checkpoint.checkpoint_id, 1);
            assert_eq!(manager.latest().unwrap().checkpoint_id, 1);
        }

        #[test]
        fn test_latest() {
            let mut manager = CheckpointManager::new(1, 10);
            manager.create(vec![ReplicationCursor::new(2, 0, 100)], 1000000);
            manager.create(vec![ReplicationCursor::new(2, 0, 200)], 1000001);
            manager.create(vec![ReplicationCursor::new(2, 0, 300)], 1000002);

            let latest = manager.latest().unwrap();
            assert_eq!(latest.checkpoint_id, 3);
        }

        #[test]
        fn test_all() {
            let mut manager = CheckpointManager::new(1, 10);
            manager.create(vec![ReplicationCursor::new(2, 0, 100)], 1000000);
            manager.create(vec![ReplicationCursor::new(2, 0, 200)], 1000001);

            let all = manager.all();
            assert_eq!(all.len(), 2);
            assert_eq!(all[0].checkpoint_id, 1);
            assert_eq!(all[1].checkpoint_id, 2);
        }

        #[test]
        fn test_prune() {
            let mut manager = CheckpointManager::new(1, 3);
            for i in 0..5u64 {
                manager.create(vec![ReplicationCursor::new(2, 0, i * 100)], 1000000 + i);
            }

            // Should only keep 3 most recent
            assert_eq!(manager.all().len(), 3);
            assert_eq!(manager.all()[0].checkpoint_id, 3); // Oldest remaining
            assert_eq!(manager.all()[2].checkpoint_id, 5); // Newest
        }

        #[test]
        fn test_find_by_id() {
            let mut manager = CheckpointManager::new(1, 10);
            manager.create(vec![ReplicationCursor::new(2, 0, 100)], 1000000);
            manager.create(vec![ReplicationCursor::new(2, 0, 200)], 1000001);
            manager.create(vec![ReplicationCursor::new(2, 0, 300)], 1000002);

            let found = manager.find_by_id(2).unwrap();
            assert_eq!(found.checkpoint_id, 2);
        }

        #[test]
        fn test_find_by_id_nonexistent() {
            let mut manager = CheckpointManager::new(1, 10);
            manager.create(vec![ReplicationCursor::new(2, 0, 100)], 1000000);

            let found = manager.find_by_id(999);
            assert!(found.is_none());
        }

        #[test]
        fn test_clear() {
            let mut manager = CheckpointManager::new(1, 10);
            manager.create(vec![ReplicationCursor::new(2, 0, 100)], 1000000);
            manager.create(vec![ReplicationCursor::new(2, 0, 200)], 1000001);

            manager.clear();

            assert!(manager.all().is_empty());
            assert!(manager.latest().is_none());
        }

        #[test]
        fn test_rolling_window() {
            let mut manager = CheckpointManager::new(1, 5);
            for i in 0..10u64 {
                manager.create(vec![ReplicationCursor::new(2, 0, i * 100)], 1000000 + i);
            }

            // Should keep only 5 most recent
            let all = manager.all();
            assert_eq!(all.len(), 5);
            assert_eq!(all[0].checkpoint_id, 6); // First of the 5 kept
            assert_eq!(all[4].checkpoint_id, 10); // Last created
        }

        #[test]
        fn test_empty_cursors_checkpoint() {
            let mut manager = CheckpointManager::new(1, 10);
            let checkpoint = manager.create(vec![], 1000000).unwrap();

            assert!(checkpoint.cursors.is_empty());
            assert_eq!(checkpoint.cursor_count, 0);
        }

        #[test]
        fn test_checkpoint_with_256_cursors() {
            let mut manager = CheckpointManager::new(1, 10);
            let mut cursors = Vec::new();
            for i in 0..256u32 {
                cursors.push(ReplicationCursor::new(2, i, i as u64 * 100));
            }
            let checkpoint = manager.create(cursors, 1000000).unwrap();

            assert_eq!(checkpoint.cursor_count, 256);
        }

        #[test]
        fn test_checkpoint_ids_increment() {
            let mut manager = CheckpointManager::new(1, 10);
            manager.create(vec![ReplicationCursor::new(2, 0, 100)], 1000000);
            manager.create(vec![ReplicationCursor::new(2, 0, 200)], 1000001);
            manager.create(vec![ReplicationCursor::new(2, 0, 300)], 1000002);

            let all = manager.all();
            assert_eq!(all[0].checkpoint_id, 1);
            assert_eq!(all[1].checkpoint_id, 2);
            assert_eq!(all[2].checkpoint_id, 3);
        }

        #[test]
        fn test_max_checkpoints_zero() {
            let mut manager = CheckpointManager::new(1, 0);
            manager.create(vec![ReplicationCursor::new(2, 0, 100)], 1000000);
            manager.create(vec![ReplicationCursor::new(2, 0, 200)], 1000001);

            // With max_checkpoints = 0, should keep none
            assert!(manager.all().is_empty());
        }
    }

    mod replication_checkpoint_equality {
        use super::*;

        #[test]
        fn test_checkpoint_equality() {
            let cp1 =
                ReplicationCheckpoint::new(1, 1, 1000000, vec![ReplicationCursor::new(2, 0, 100)]);
            let cp2 =
                ReplicationCheckpoint::new(1, 1, 1000000, vec![ReplicationCursor::new(2, 0, 100)]);
            assert_eq!(cp1, cp2);
        }

        #[test]
        fn test_checkpoint_inequality() {
            let cp1 =
                ReplicationCheckpoint::new(1, 1, 1000000, vec![ReplicationCursor::new(2, 0, 100)]);
            let cp2 =
                ReplicationCheckpoint::new(1, 2, 1000000, vec![ReplicationCursor::new(2, 0, 100)]);
            assert_ne!(cp1, cp2);
        }
    }
}
