use serde::{Deserialize, Serialize};
use std::sync::{atomic::AtomicU64, RwLock};

use crate::types::*;

/// Which side won a conflict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictWinner {
    /// Local version was newer.
    Local,
    /// Remote version was newer.
    Remote,
}

/// A conflict event detected during cross-site replication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConflictEvent {
    /// The inode that has a conflict.
    pub ino: InodeId,
    /// The local version (our change).
    pub local_clock: VectorClock,
    /// The remote version (their change).
    pub remote_clock: VectorClock,
    /// Which version won (Last-Write-Wins by sequence, then site_id).
    pub winner: ConflictWinner,
    /// The operation that caused the conflict.
    pub operation: MetaOp,
    /// When the conflict was detected.
    pub detected_at: Timestamp,
}

/// Manages conflict detection and resolution for cross-site replication.
///
/// Uses vector clocks to detect concurrent modifications and Last-Write-Wins
/// (LWW) strategy to resolve conflicts based on sequence number and site ID.
pub struct ConflictDetector {
    /// Log of detected conflicts.
    conflict_log: RwLock<Vec<ConflictEvent>>,
    /// Maximum conflicts to keep in the log.
    max_log_entries: usize,
    /// The local site ID.
    site_id: u64,
    /// Sequence number counter for generating new vector clocks.
    sequence_counter: AtomicU64,
}

impl ConflictDetector {
    /// Create a new conflict detector.
    ///
    /// # Arguments
    /// * `site_id` - The local site ID for this node
    /// * `max_log_entries` - Maximum number of conflict events to keep in the log
    pub fn new(site_id: u64, max_log_entries: usize) -> Self {
        Self {
            conflict_log: RwLock::new(Vec::new()),
            max_log_entries,
            site_id,
            sequence_counter: AtomicU64::new(0),
        }
    }

    /// Increment the local clock and return a new VectorClock.
    ///
    /// Returns a new vector clock with the local site ID and an incremented
    /// internal sequence number.
    pub fn increment_clock(&self) -> VectorClock {
        let seq = self
            .sequence_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        VectorClock::new(self.site_id, seq)
    }

    /// Determine the winner using Last-Write-Wins (LWW).
    ///
    /// Compares two vector clocks: higher sequence wins; on tie, higher site_id wins.
    ///
    /// # Arguments
    /// * `local` - The local vector clock
    /// * `remote` - The remote vector clock
    ///
    /// # Returns
    /// The winner (Local or Remote)
    pub fn resolve_lww(local: &VectorClock, remote: &VectorClock) -> ConflictWinner {
        if local.sequence > remote.sequence {
            ConflictWinner::Local
        } else if remote.sequence > local.sequence {
            ConflictWinner::Remote
        } else {
            // Tie-breaker: higher site_id wins
            if local.site_id > remote.site_id {
                ConflictWinner::Local
            } else {
                ConflictWinner::Remote
            }
        }
    }

    /// Check if two vector clocks represent concurrent modifications.
    ///
    /// Returns true if neither clock dominates the other:
    /// - Both have seen changes the other hasn't
    /// - Or both have identical sequences from different sites
    ///
    /// # Arguments
    /// * `local` - The local vector clock
    /// * `remote` - The remote vector clock
    ///
    /// # Returns
    /// true if the modifications are concurrent
    pub fn is_concurrent(local: &VectorClock, remote: &VectorClock) -> bool {
        // If they are equal in sequence but from different sites, they're concurrent
        if local.sequence == remote.sequence && local.site_id != remote.site_id {
            return true;
        }
        // If sequence differs, check if one dominates the other
        // A dominates B if A has higher sequence (or same sequence but higher site_id)
        let local_dominates = local.sequence > remote.sequence
            || (local.sequence == remote.sequence && local.site_id > remote.site_id);
        let remote_dominates = remote.sequence > local.sequence
            || (remote.sequence == local.sequence && remote.site_id > local.site_id);

        // Concurrent if neither dominates
        !local_dominates && !remote_dominates
    }

    /// Detect a conflict between local and remote vector clocks.
    ///
    /// Returns None if there is no conflict (remote is strictly newer).
    /// Returns Some(ConflictEvent) if concurrent modification detected.
    ///
    /// # Arguments
    /// * `ino` - The inode that has the conflict
    /// * `local_clock` - The local vector clock
    /// * `remote_clock` - The remote vector clock
    /// * `operation` - The operation that caused the conflict
    ///
    /// # Returns
    /// None if no conflict, or Some(event) if concurrent modification detected
    pub fn detect_conflict(
        &self,
        ino: InodeId,
        local_clock: &VectorClock,
        remote_clock: &VectorClock,
        operation: MetaOp,
    ) -> Option<ConflictEvent> {
        // If remote is strictly newer, no conflict (we lost, but no concurrent modification)
        let winner = Self::resolve_lww(local_clock, remote_clock);
        let remote_is_newer = matches!(winner, ConflictWinner::Remote);

        // Check for concurrent modification
        if Self::is_concurrent(local_clock, remote_clock) {
            let event = ConflictEvent {
                ino,
                local_clock: *local_clock,
                remote_clock: *remote_clock,
                winner,
                operation,
                detected_at: Timestamp::now(),
            };

            // Add to conflict log
            self.log_conflict(event.clone());
            Some(event)
        } else if remote_is_newer {
            // Remote is newer but not concurrent - log non-conflicting newer version
            // This is informational, not a conflict
            None
        } else {
            // Local is newer - no conflict
            None
        }
    }

    /// Add a conflict event to the log.
    fn log_conflict(&self, event: ConflictEvent) {
        let mut log = self.conflict_log.write().unwrap();

        // Evict old entries if at capacity
        if log.len() >= self.max_log_entries {
            let evict_count = self.max_log_entries / 4;
            log.drain(0..evict_count);
        }

        log.push(event);
    }

    /// Get all recorded conflicts.
    ///
    /// # Returns
    /// A vector of all conflict events
    pub fn conflicts(&self) -> Vec<ConflictEvent> {
        self.conflict_log.read().unwrap().clone()
    }

    /// Get conflicts for a specific inode.
    ///
    /// # Arguments
    /// * `ino` - The inode to get conflicts for
    ///
    /// # Returns
    /// A vector of conflict events for the given inode
    pub fn conflicts_for_inode(&self, ino: InodeId) -> Vec<ConflictEvent> {
        self.conflict_log
            .read()
            .unwrap()
            .iter()
            .filter(|e| e.ino == ino)
            .cloned()
            .collect()
    }

    /// Get the number of recorded conflicts.
    ///
    /// # Returns
    /// The number of conflicts in the log
    pub fn conflict_count(&self) -> usize {
        self.conflict_log.read().unwrap().len()
    }

    /// Clear all recorded conflicts.
    ///
    /// # Returns
    /// The number of conflicts that were cleared
    pub fn clear_conflicts(&self) -> usize {
        let mut log = self.conflict_log.write().unwrap();
        let count = log.len();
        log.clear();
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_inode_op(ino: InodeId) -> MetaOp {
        MetaOp::CreateInode {
            attr: InodeAttr::new_file(ino, 0, 0, 0o644, 1),
        }
    }

    #[test]
    fn test_no_conflict_remote_strictly_newer() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 10);
        let remote_clock = VectorClock::new(2, 20);

        let result = detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            create_inode_op(InodeId::new(100)),
        );

        // Remote is strictly newer, no conflict
        assert!(result.is_none());
    }

    #[test]
    fn test_conflict_concurrent_modification() {
        let detector = ConflictDetector::new(1, 100);

        // Same sequence from different sites = concurrent
        let local_clock = VectorClock::new(1, 10);
        let remote_clock = VectorClock::new(2, 10);

        let result = detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            create_inode_op(InodeId::new(100)),
        );

        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.ino, InodeId::new(100));
    }

    #[test]
    fn test_resolve_lww_higher_sequence_wins() {
        let local = VectorClock::new(1, 100);
        let remote = VectorClock::new(2, 50);

        let winner = ConflictDetector::resolve_lww(&local, &remote);

        assert_eq!(winner, ConflictWinner::Local);
    }

    #[test]
    fn test_resolve_lww_higher_site_id_breaks_tie() {
        let local = VectorClock::new(5, 100); // site_id = 5
        let remote = VectorClock::new(3, 100); // site_id = 3, same sequence

        let winner = ConflictDetector::resolve_lww(&local, &remote);

        // Higher site_id wins the tie
        assert_eq!(winner, ConflictWinner::Local);

        // Reverse
        let local2 = VectorClock::new(3, 100);
        let remote2 = VectorClock::new(5, 100);

        let winner2 = ConflictDetector::resolve_lww(&local2, &remote2);
        assert_eq!(winner2, ConflictWinner::Remote);
    }

    #[test]
    fn test_is_concurrent_different_sites_same_sequence() {
        let local = VectorClock::new(1, 100);
        let remote = VectorClock::new(2, 100);

        let result = ConflictDetector::is_concurrent(&local, &remote);

        assert!(result);
    }

    #[test]
    fn test_is_concurrent_strictly_ordered() {
        // Local has higher sequence, so it dominates
        let local = VectorClock::new(1, 100);
        let remote = VectorClock::new(2, 50);

        let result = ConflictDetector::is_concurrent(&local, &remote);

        assert!(!result);

        // Remote has higher sequence
        let local2 = VectorClock::new(1, 50);
        let remote2 = VectorClock::new(2, 100);

        let result2 = ConflictDetector::is_concurrent(&local2, &remote2);

        assert!(!result2);
    }

    #[test]
    fn test_conflict_logging() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 10);
        let remote_clock = VectorClock::new(2, 10);

        detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            create_inode_op(InodeId::new(100)),
        );

        assert_eq!(detector.conflict_count(), 1);
    }

    #[test]
    fn test_conflicts_for_inode() {
        let detector = ConflictDetector::new(1, 100);

        // Add conflicts for different inodes
        let local_clock = VectorClock::new(1, 10);
        let remote_clock = VectorClock::new(2, 10);

        detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            create_inode_op(InodeId::new(100)),
        );
        detector.detect_conflict(
            InodeId::new(200),
            &local_clock,
            &remote_clock,
            create_inode_op(InodeId::new(200)),
        );
        detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            create_inode_op(InodeId::new(100)),
        );

        let ino100_conflicts = detector.conflicts_for_inode(InodeId::new(100));
        let ino200_conflicts = detector.conflicts_for_inode(InodeId::new(200));

        assert_eq!(ino100_conflicts.len(), 2);
        assert_eq!(ino200_conflicts.len(), 1);
    }

    #[test]
    fn test_clear_conflicts() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 10);
        let remote_clock = VectorClock::new(2, 10);

        detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            create_inode_op(InodeId::new(100)),
        );

        assert_eq!(detector.conflict_count(), 1);

        let cleared = detector.clear_conflicts();

        assert_eq!(cleared, 1);
        assert_eq!(detector.conflict_count(), 0);
    }

    #[test]
    fn test_increment_clock() {
        let detector = ConflictDetector::new(42, 100);

        let clock1 = detector.increment_clock();
        let clock2 = detector.increment_clock();
        let clock3 = detector.increment_clock();

        assert_eq!(clock1.site_id, 42);
        assert_eq!(clock1.sequence, 0);
        assert_eq!(clock2.sequence, 1);
        assert_eq!(clock3.sequence, 2);
    }
}
