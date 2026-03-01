//! Conflict resolution for cross-site replication.
//!
//! Implements last-write-wins (LWW) conflict resolution with administrator alerting
//! when manual resolution is required or split-brain conditions are detected.

use serde::{Deserialize, Serialize};

/// Site identifier wrapper type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SiteId(pub u64);

/// Types of conflicts that can occur during cross-site replication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictType {
    /// Conflict resolved using LWW (timestamp-based winner).
    LwwResolved,
    /// Conflict requires manual administrator resolution.
    ManualResolutionRequired,
    /// Split-brain condition detected (identical data at multiple sites).
    SplitBrain,
}

/// Record of a resolved conflict between two sites.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    /// Unique identifier for this conflict.
    pub conflict_id: u64,
    /// The inode that had the conflict.
    pub inode: u64,
    /// First site involved in the conflict.
    pub site_a: SiteId,
    /// Second site involved in the conflict.
    pub site_b: SiteId,
    /// Sequence number from site A.
    pub seq_a: u64,
    /// Sequence number from site B.
    pub seq_b: u64,
    /// Timestamp from site A (in nanoseconds).
    pub ts_a: u64,
    /// Timestamp from site B (in nanoseconds).
    pub ts_b: u64,
    /// The winning site after resolution.
    pub winner: SiteId,
    /// Type of conflict that occurred.
    pub conflict_type: ConflictType,
    /// Timestamp when the conflict was resolved (in nanoseconds).
    pub resolved_at: u64,
}

/// Conflict resolver implementing last-write-wins (LWW) semantics.
#[derive(Debug, Default)]
pub struct ConflictResolver {
    conflicts: Vec<ConflictRecord>,
    conflict_id_counter: u64,
}

impl ConflictResolver {
    /// Creates a new ConflictResolver.
    pub fn new() -> Self {
        Self {
            conflicts: Vec::new(),
            conflict_id_counter: 0,
        }
    }

    /// Resolves a conflict between two sites using LWW semantics.
    ///
    /// Winner selection:
    /// 1. Higher timestamp wins
    /// 2. If timestamps equal, higher sequence wins
    /// 3. If both equal, site_a wins (deterministic tiebreak)
    ///
    /// Returns a ConflictRecord with the resolution result.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        &mut self,
        inode: u64,
        site_a: SiteId,
        seq_a: u64,
        ts_a: u64,
        site_b: SiteId,
        seq_b: u64,
        ts_b: u64,
    ) -> ConflictRecord {
        let conflict_id = self.conflict_id_counter;
        self.conflict_id_counter += 1;

        let resolved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let (winner, conflict_type) = if ts_a > ts_b {
            (site_a, ConflictType::LwwResolved)
        } else if ts_b > ts_a {
            (site_b, ConflictType::LwwResolved)
        } else if seq_a > seq_b {
            (site_a, ConflictType::LwwResolved)
        } else if seq_b > seq_a {
            (site_b, ConflictType::LwwResolved)
        } else {
            (site_a, ConflictType::ManualResolutionRequired)
        };

        let record = ConflictRecord {
            conflict_id,
            inode,
            site_a,
            site_b,
            seq_a,
            seq_b,
            ts_a,
            ts_b,
            winner,
            conflict_type,
            resolved_at,
        };

        match conflict_type {
            ConflictType::ManualResolutionRequired => {
                tracing::warn!(
                    conflict_id = record.conflict_id,
                    inode = record.inode,
                    "Conflict requires manual resolution: ts_a={}, ts_b={}, seq_a={}, seq_b={}",
                    ts_a,
                    ts_b,
                    seq_a,
                    seq_b
                );
            }
            ConflictType::SplitBrain => {
                tracing::error!(
                    conflict_id = record.conflict_id,
                    inode = record.inode,
                    "Split-brain condition detected"
                );
            }
            ConflictType::LwwResolved => {}
        }

        self.conflicts.push(record.clone());
        record
    }

    /// Determines if administrator alerting is needed for the conflict.
    ///
    /// Returns true if the conflict type requires manual resolution or indicates split-brain.
    pub fn alert_needed(record: &ConflictRecord) -> bool {
        matches!(
            record.conflict_type,
            ConflictType::ManualResolutionRequired | ConflictType::SplitBrain
        )
    }

    /// Returns all conflict records for a given inode.
    pub fn conflicts_for_inode(&self, inode: u64) -> Vec<&ConflictRecord> {
        self.conflicts.iter().filter(|c| c.inode == inode).collect()
    }

    /// Returns the total number of conflicts resolved.
    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }

    /// Returns the number of split-brain conflicts.
    pub fn split_brain_count(&self) -> usize {
        self.conflicts
            .iter()
            .filter(|c| c.conflict_type == ConflictType::SplitBrain)
            .count()
    }
}

impl Default for ConflictRecord {
    fn default() -> Self {
        Self {
            conflict_id: 0,
            inode: 0,
            site_a: SiteId(0),
            site_b: SiteId(0),
            seq_a: 0,
            seq_b: 0,
            ts_a: 0,
            ts_b: 0,
            winner: SiteId(0),
            conflict_type: ConflictType::LwwResolved,
            resolved_at: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site_id(n: u64) -> SiteId {
        SiteId(n)
    }

    #[test]
    fn test_lww_higher_timestamp_wins() {
        let mut resolver = ConflictResolver::new();
        let record = resolver.resolve(1, site_id(1), 10, 100, site_id(2), 20, 200);
        assert_eq!(record.winner, site_id(2));
        assert_eq!(record.conflict_type, ConflictType::LwwResolved);
    }

    #[test]
    fn test_lww_equal_timestamp_higher_seq_wins() {
        let mut resolver = ConflictResolver::new();
        let record = resolver.resolve(1, site_id(1), 100, 100, site_id(2), 200, 100);
        assert_eq!(record.winner, site_id(2));
        assert_eq!(record.conflict_type, ConflictType::LwwResolved);
    }

    #[test]
    fn test_lww_deterministic_tiebreak_site_a_wins() {
        let mut resolver = ConflictResolver::new();
        let record = resolver.resolve(1, site_id(1), 100, 100, site_id(2), 100, 100);
        assert_eq!(record.winner, site_id(1));
        assert_eq!(record.conflict_type, ConflictType::ManualResolutionRequired);
    }

    #[test]
    fn test_manual_resolution_required_when_all_equal() {
        let mut resolver = ConflictResolver::new();
        let record = resolver.resolve(1, site_id(1), 100, 100, site_id(2), 100, 100);
        assert_eq!(record.conflict_type, ConflictType::ManualResolutionRequired);
    }

    #[test]
    fn test_alert_needed_returns_true_for_manual_resolution() {
        let record = ConflictRecord {
            conflict_id: 1,
            inode: 1,
            site_a: site_id(1),
            site_b: site_id(2),
            seq_a: 100,
            seq_b: 100,
            ts_a: 100,
            ts_b: 100,
            winner: site_id(1),
            conflict_type: ConflictType::ManualResolutionRequired,
            resolved_at: 1000,
        };
        assert!(ConflictResolver::alert_needed(&record));
    }

    #[test]
    fn test_alert_needed_returns_false_for_lww_resolved() {
        let record = ConflictRecord {
            conflict_id: 1,
            inode: 1,
            site_a: site_id(1),
            site_b: site_id(2),
            seq_a: 100,
            seq_b: 200,
            ts_a: 100,
            ts_b: 200,
            winner: site_id(2),
            conflict_type: ConflictType::LwwResolved,
            resolved_at: 1000,
        };
        assert!(!ConflictResolver::alert_needed(&record));
    }

    #[test]
    fn test_conflicts_for_inode_filters_correctly() {
        let mut resolver = ConflictResolver::new();
        resolver.resolve(1, site_id(1), 10, 100, site_id(2), 20, 200);
        resolver.resolve(2, site_id(1), 10, 100, site_id(2), 20, 200);
        resolver.resolve(1, site_id(1), 10, 100, site_id(2), 20, 200);

        let conflicts = resolver.conflicts_for_inode(1);
        assert_eq!(conflicts.len(), 2);
    }

    #[test]
    fn test_conflict_count_increases() {
        let mut resolver = ConflictResolver::new();
        assert_eq!(resolver.conflict_count(), 0);
        resolver.resolve(1, site_id(1), 10, 100, site_id(2), 20, 200);
        assert_eq!(resolver.conflict_count(), 1);
        resolver.resolve(2, site_id(1), 10, 100, site_id(2), 20, 200);
        assert_eq!(resolver.conflict_count(), 2);
    }

    #[test]
    fn test_split_brain_count() {
        let mut resolver = ConflictResolver::new();
        resolver.resolve(1, site_id(1), 10, 100, site_id(2), 20, 200);
        resolver.resolve(2, site_id(1), 10, 100, site_id(2), 10, 100);
        assert_eq!(resolver.split_brain_count(), 0);
    }

    #[test]
    fn test_multiple_conflicts_for_same_inode() {
        let mut resolver = ConflictResolver::new();
        for i in 0..5 {
            resolver.resolve(1, site_id(1), i, i * 100, site_id(2), i + 1, i * 100 + 50);
        }
        assert_eq!(resolver.conflict_count(), 5);
        assert_eq!(resolver.conflicts_for_inode(1).len(), 5);
    }

    #[test]
    fn test_multiple_conflicts_for_different_inodes() {
        let mut resolver = ConflictResolver::new();
        resolver.resolve(1, site_id(1), 10, 100, site_id(2), 20, 200);
        resolver.resolve(2, site_id(1), 10, 100, site_id(2), 20, 200);
        resolver.resolve(3, site_id(1), 10, 100, site_id(2), 20, 200);
        assert_eq!(resolver.conflict_count(), 3);
        assert_eq!(resolver.conflicts_for_inode(1).len(), 1);
        assert_eq!(resolver.conflicts_for_inode(2).len(), 1);
        assert_eq!(resolver.conflicts_for_inode(3).len(), 1);
    }

    #[test]
    fn test_conflict_record_fields_correctly_populated() {
        let mut resolver = ConflictResolver::new();
        let record = resolver.resolve(42, site_id(1), 10, 100, site_id(2), 20, 200);

        assert_eq!(record.inode, 42);
        assert_eq!(record.site_a, site_id(1));
        assert_eq!(record.site_b, site_id(2));
        assert_eq!(record.seq_a, 10);
        assert_eq!(record.seq_b, 20);
        assert_eq!(record.ts_a, 100);
        assert_eq!(record.ts_b, 200);
        assert!(record.resolved_at > 0);
    }

    #[test]
    fn test_round_trip_serialize_deserialize() {
        let record = ConflictRecord {
            conflict_id: 1,
            inode: 42,
            site_a: site_id(1),
            site_b: site_id(2),
            seq_a: 10,
            seq_b: 20,
            ts_a: 100,
            ts_b: 200,
            winner: site_id(2),
            conflict_type: ConflictType::LwwResolved,
            resolved_at: 1000,
        };

        let serialized = bincode::serialize(&record).unwrap();
        let deserialized: ConflictRecord = bincode::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.conflict_id, record.conflict_id);
        assert_eq!(deserialized.inode, record.inode);
        assert_eq!(deserialized.site_a, record.site_a);
        assert_eq!(deserialized.site_b, record.site_b);
        assert_eq!(deserialized.seq_a, record.seq_a);
        assert_eq!(deserialized.seq_b, record.seq_b);
        assert_eq!(deserialized.ts_a, record.ts_a);
        assert_eq!(deserialized.ts_b, record.ts_b);
        assert_eq!(deserialized.winner, record.winner);
        assert_eq!(deserialized.conflict_type, record.conflict_type);
        assert_eq!(deserialized.resolved_at, record.resolved_at);
    }

    #[test]
    fn test_resolve_stores_record_in_internal_vec() {
        let mut resolver = ConflictResolver::new();
        assert!(resolver.conflicts.is_empty());

        resolver.resolve(1, site_id(1), 10, 100, site_id(2), 20, 200);
        assert_eq!(resolver.conflicts.len(), 1);

        resolver.resolve(2, site_id(1), 10, 100, site_id(2), 20, 200);
        assert_eq!(resolver.conflicts.len(), 2);
    }

    #[test]
    fn test_consecutive_conflict_ids_are_unique() {
        let mut resolver = ConflictResolver::new();
        let id1 = resolver
            .resolve(1, site_id(1), 10, 100, site_id(2), 20, 200)
            .conflict_id;
        let id2 = resolver
            .resolve(2, site_id(1), 10, 100, site_id(2), 20, 200)
            .conflict_id;
        let id3 = resolver
            .resolve(3, site_id(1), 10, 100, site_id(2), 20, 200)
            .conflict_id;

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_winner_is_site_b_when_site_b_has_higher_timestamp() {
        let mut resolver = ConflictResolver::new();
        let record = resolver.resolve(1, site_id(1), 10, 100, site_id(2), 5, 500);
        assert_eq!(record.winner, site_id(2));
    }

    #[test]
    fn test_winner_is_site_a_when_timestamps_equal_and_seq_a_greater() {
        let mut resolver = ConflictResolver::new();
        let record = resolver.resolve(1, site_id(1), 200, 100, site_id(2), 100, 100);
        assert_eq!(record.winner, site_id(1));
    }

    #[test]
    fn test_winner_is_site_a_when_all_equal_tiebreak() {
        let mut resolver = ConflictResolver::new();
        let record = resolver.resolve(1, site_id(1), 100, 100, site_id(2), 100, 100);
        assert_eq!(record.winner, site_id(1));
    }

    #[test]
    fn test_conflict_type_lww_resolved_for_normal_cases() {
        let mut resolver = ConflictResolver::new();
        let record1 = resolver.resolve(1, site_id(1), 10, 200, site_id(2), 20, 100);
        let record2 = resolver.resolve(2, site_id(1), 20, 100, site_id(2), 10, 100);
        let record3 = resolver.resolve(3, site_id(1), 10, 100, site_id(2), 20, 100);

        assert_eq!(record1.conflict_type, ConflictType::LwwResolved);
        assert_eq!(record2.conflict_type, ConflictType::LwwResolved);
        assert_eq!(record3.conflict_type, ConflictType::LwwResolved);
    }

    #[test]
    fn test_new_resolver_is_empty() {
        let resolver = ConflictResolver::new();
        assert_eq!(resolver.conflict_count(), 0);
        assert_eq!(resolver.split_brain_count(), 0);
    }

    #[test]
    fn test_conflicts_for_inode_returns_empty_for_unknown() {
        let resolver = ConflictResolver::new();
        let conflicts = resolver.conflicts_for_inode(999);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_alert_needed_returns_true_for_split_brain() {
        let record = ConflictRecord {
            conflict_id: 1,
            inode: 1,
            site_a: site_id(1),
            site_b: site_id(2),
            seq_a: 100,
            seq_b: 100,
            ts_a: 100,
            ts_b: 100,
            winner: site_id(1),
            conflict_type: ConflictType::SplitBrain,
            resolved_at: 1000,
        };
        assert!(ConflictResolver::alert_needed(&record));
    }

    #[test]
    fn test_conflict_type_bincode_serialize() {
        let ct = ConflictType::LwwResolved;
        let serialized = bincode::serialize(&ct).unwrap();
        let deserialized: ConflictType = bincode::deserialize(&serialized).unwrap();
        assert_eq!(deserialized, ConflictType::LwwResolved);

        let ct2 = ConflictType::ManualResolutionRequired;
        let serialized2 = bincode::serialize(&ct2).unwrap();
        let deserialized2: ConflictType = bincode::deserialize(&serialized2).unwrap();
        assert_eq!(deserialized2, ConflictType::ManualResolutionRequired);

        let ct3 = ConflictType::SplitBrain;
        let serialized3 = bincode::serialize(&ct3).unwrap();
        let deserialized3: ConflictType = bincode::deserialize(&serialized3).unwrap();
        assert_eq!(deserialized3, ConflictType::SplitBrain);
    }
}
