//! Meta conflict detection and resolution security tests.
//!
//! Part of A10 Phase 24: Meta conflict detection security audit

use claudefs_meta::conflict::{ConflictDetector, ConflictEvent, ConflictWinner};
use claudefs_meta::types::{DirEntry, FileType, InodeAttr, InodeId, MetaOp, VectorClock};

fn make_inode_op(ino: u64) -> MetaOp {
    MetaOp::CreateInode {
        attr: InodeAttr::new_file(InodeId::new(ino), 0, 0, 0o644, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_lww_higher_sequence_wins() {
        let local = VectorClock::new(1, 100);
        let remote = VectorClock::new(2, 50);

        let winner = ConflictDetector::resolve_lww(&local, &remote);

        assert_eq!(winner, ConflictWinner::Local);
        // FINDING-META-CONF-01: higher sequence always wins — correct LWW semantics
    }

    #[test]
    fn test_resolve_lww_higher_sequence_wins_swapped() {
        let local = VectorClock::new(1, 50);
        let remote = VectorClock::new(2, 100);

        let winner = ConflictDetector::resolve_lww(&local, &remote);

        assert_eq!(winner, ConflictWinner::Remote);
        // FINDING-META-CONF-01: higher sequence always wins — correct LWW semantics
    }

    #[test]
    fn test_resolve_lww_tie_breaks_by_site_id() {
        let local = VectorClock::new(5, 100);
        let remote = VectorClock::new(3, 100);

        let winner = ConflictDetector::resolve_lww(&local, &remote);

        assert_eq!(winner, ConflictWinner::Local);
        // FINDING-META-CONF-02: site_id tie-breaker is deterministic — both sites agree on winner
    }

    #[test]
    fn test_resolve_lww_tie_breaks_by_site_id_swapped() {
        let local = VectorClock::new(3, 100);
        let remote = VectorClock::new(5, 100);

        let winner = ConflictDetector::resolve_lww(&local, &remote);

        assert_eq!(winner, ConflictWinner::Remote);
        // FINDING-META-CONF-02: site_id tie-breaker is deterministic — both sites agree on winner
    }

    #[test]
    fn test_resolve_lww_equal_clocks() {
        let local = VectorClock::new(1, 100);
        let remote = VectorClock::new(1, 100);

        let winner = ConflictDetector::resolve_lww(&local, &remote);

        assert_eq!(winner, ConflictWinner::Remote);
        // FINDING-META-CONF-03: same-site equal clocks resolve to Remote — potential concern if both are actually local
    }

    #[test]
    fn test_is_concurrent_same_seq_different_sites() {
        let local = VectorClock::new(1, 100);
        let remote = VectorClock::new(2, 100);

        let result = ConflictDetector::is_concurrent(&local, &remote);

        assert!(result);
        // FINDING-META-CONF-04: same sequence from different sites is correctly identified as concurrent
    }

    #[test]
    fn test_is_concurrent_strictly_ordered_local_dominates() {
        let local = VectorClock::new(1, 100);
        let remote = VectorClock::new(2, 50);

        let result = ConflictDetector::is_concurrent(&local, &remote);

        assert!(!result);
        // FINDING-META-CONF-05: strictly ordered operations correctly identified as non-concurrent
    }

    #[test]
    fn test_is_concurrent_strictly_ordered_remote_dominates() {
        let local = VectorClock::new(1, 50);
        let remote = VectorClock::new(2, 100);

        let result = ConflictDetector::is_concurrent(&local, &remote);

        assert!(!result);
        // FINDING-META-CONF-05: strictly ordered operations correctly identified as non-concurrent
    }

    #[test]
    fn test_detect_concurrent_conflict() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);

        let result = detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );

        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.ino, InodeId::new(100));
        assert!(matches!(
            event.winner,
            ConflictWinner::Local | ConflictWinner::Remote
        ));
    }

    #[test]
    fn test_detect_no_conflict_remote_newer() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 10);
        let remote_clock = VectorClock::new(2, 20);

        let result = detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );

        assert!(result.is_none());
        // FINDING-META-CONF-06: strictly newer remote operations are not flagged as conflicts
    }

    #[test]
    fn test_detect_no_conflict_local_newer() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 20);
        let remote_clock = VectorClock::new(2, 10);

        let result = detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );

        assert!(result.is_none());
        // FINDING-META-CONF-07: strictly newer local operations are not flagged as conflicts
    }

    #[test]
    fn test_detect_conflict_logs_event() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);

        detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );

        assert_eq!(detector.conflict_count(), 1);

        detector.detect_conflict(
            InodeId::new(200),
            &local_clock,
            &remote_clock,
            make_inode_op(200),
        );

        assert_eq!(detector.conflict_count(), 2);
        assert_eq!(detector.conflicts().len(), 2);
    }

    #[test]
    fn test_detect_conflict_for_specific_inode() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);

        detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );
        detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );
        detector.detect_conflict(
            InodeId::new(200),
            &local_clock,
            &remote_clock,
            make_inode_op(200),
        );

        assert_eq!(detector.conflicts_for_inode(InodeId::new(100)).len(), 2);
        assert_eq!(detector.conflicts_for_inode(InodeId::new(200)).len(), 1);
        assert_eq!(detector.conflicts_for_inode(InodeId::new(999)).len(), 0);
    }

    #[test]
    fn test_conflict_log_eviction() {
        let detector = ConflictDetector::new(1, 4);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);

        for i in 0..5 {
            detector.detect_conflict(
                InodeId::new(i + 1),
                &local_clock,
                &remote_clock,
                make_inode_op(i + 1),
            );
        }

        assert!(detector.conflict_count() <= 4);
        // FINDING-META-CONF-08: conflict log bounded — prevents memory exhaustion from repeated conflicts
    }

    #[test]
    fn test_clear_conflicts() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);

        detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );
        detector.detect_conflict(
            InodeId::new(200),
            &local_clock,
            &remote_clock,
            make_inode_op(200),
        );
        detector.detect_conflict(
            InodeId::new(300),
            &local_clock,
            &remote_clock,
            make_inode_op(300),
        );

        assert_eq!(detector.conflict_count(), 3);

        let cleared = detector.clear_conflicts();

        assert_eq!(cleared, 3);
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
        // FINDING-META-CONF-09: clock increments monotonically — prevents clock rollback
    }

    #[test]
    fn test_conflict_event_fields() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);

        let result = detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );

        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.ino, InodeId::new(100));
        assert_eq!(event.local_clock, local_clock);
        assert_eq!(event.remote_clock, remote_clock);
        assert!(event.detected_at.secs > 0);
        assert!(matches!(
            event.winner,
            ConflictWinner::Local | ConflictWinner::Remote
        ));
    }

    #[test]
    fn test_conflict_winner_variants() {
        assert_eq!(ConflictWinner::Local, ConflictWinner::Local);
        assert_eq!(ConflictWinner::Remote, ConflictWinner::Remote);
        assert_ne!(ConflictWinner::Local, ConflictWinner::Remote);

        let local = ConflictWinner::Local;
        let serialized = format!("{:?}", local);
        assert!(serialized.contains("Local"));

        let remote = ConflictWinner::Remote;
        let serialized = format!("{:?}", remote);
        assert!(serialized.contains("Remote"));
    }

    #[test]
    fn test_zero_sequence_clocks() {
        let local = VectorClock::new(1, 0);
        let remote = VectorClock::new(2, 0);

        let concurrent = ConflictDetector::is_concurrent(&local, &remote);
        assert!(concurrent);

        let winner = ConflictDetector::resolve_lww(&local, &remote);
        assert_eq!(winner, ConflictWinner::Remote);
    }

    #[test]
    fn test_max_sequence_clocks() {
        let local = VectorClock::new(1, u64::MAX);
        let remote = VectorClock::new(2, u64::MAX);

        let concurrent = ConflictDetector::is_concurrent(&local, &remote);
        assert!(concurrent);

        let winner = ConflictDetector::resolve_lww(&local, &remote);
        assert_eq!(winner, ConflictWinner::Remote);
    }

    #[test]
    fn test_same_site_different_sequence() {
        let local = VectorClock::new(1, 10);
        let remote = VectorClock::new(1, 20);

        let concurrent = ConflictDetector::is_concurrent(&local, &remote);
        assert!(!concurrent);

        let winner = ConflictDetector::resolve_lww(&local, &remote);
        assert_eq!(winner, ConflictWinner::Remote);
    }

    #[test]
    fn test_empty_conflict_log() {
        let detector = ConflictDetector::new(1, 100);

        assert!(detector.conflicts().is_empty());
        assert!(detector.conflicts_for_inode(InodeId::new(1)).is_empty());
        assert_eq!(detector.conflict_count(), 0);
    }

    #[test]
    fn test_multiple_detectors_independent() {
        let detector1 = ConflictDetector::new(1, 100);
        let detector2 = ConflictDetector::new(2, 100);

        let local_clock1 = VectorClock::new(1, 100);
        let remote_clock1 = VectorClock::new(2, 100);
        let local_clock2 = VectorClock::new(2, 100);
        let remote_clock2 = VectorClock::new(1, 100);

        detector1.detect_conflict(
            InodeId::new(100),
            &local_clock1,
            &remote_clock1,
            make_inode_op(100),
        );
        detector2.detect_conflict(
            InodeId::new(200),
            &local_clock2,
            &remote_clock2,
            make_inode_op(200),
        );

        assert_eq!(detector1.conflict_count(), 1);
        assert_eq!(detector2.conflict_count(), 1);

        detector1.clear_conflicts();

        assert_eq!(detector1.conflict_count(), 0);
        assert_eq!(detector2.conflict_count(), 1);
    }

    #[test]
    fn test_lww_deterministic() {
        let local = VectorClock::new(1, 100);
        let remote = VectorClock::new(2, 50);

        for _ in 0..100 {
            let winner = ConflictDetector::resolve_lww(&local, &remote);
            assert_eq!(winner, ConflictWinner::Local);
        }
        // FINDING-META-CONF-10: LWW resolution is deterministic — both sites reach same conclusion
    }

    #[test]
    fn test_concurrent_detection_symmetric() {
        let pairs = [
            (VectorClock::new(1, 100), VectorClock::new(2, 100)),
            (VectorClock::new(1, 50), VectorClock::new(2, 100)),
            (VectorClock::new(1, 100), VectorClock::new(2, 50)),
            (VectorClock::new(3, 100), VectorClock::new(5, 100)),
        ];

        for (a, b) in pairs {
            assert_eq!(
                ConflictDetector::is_concurrent(&a, &b),
                ConflictDetector::is_concurrent(&b, &a),
                "is_concurrent should be symmetric"
            );
        }
        // FINDING-META-CONF-11: concurrent detection is symmetric — order of comparison doesn't matter
    }

    #[test]
    fn test_conflict_preserves_operation() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);
        let op = MetaOp::CreateEntry {
            parent: InodeId::new(1),
            name: "test".to_string(),
            entry: DirEntry {
                ino: InodeId::new(100),
                name: "test".to_string(),
                file_type: FileType::RegularFile,
            },
        };

        let result = detector.detect_conflict(InodeId::new(100), &local_clock, &remote_clock, op);

        assert!(result.is_some());
        let event = result.unwrap();
        assert!(matches!(event.operation, MetaOp::CreateEntry { .. }));
    }

    #[test]
    fn test_log_eviction_removes_oldest() {
        let detector = ConflictDetector::new(1, 4);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);

        for i in 1..=5 {
            detector.detect_conflict(
                InodeId::new(i),
                &local_clock,
                &remote_clock,
                make_inode_op(i),
            );
        }

        let conflicts = detector.conflicts();
        assert!(conflicts.len() <= 4);

        let inos: Vec<u64> = conflicts.iter().map(|e| e.ino.as_u64()).collect();
        assert!(!inos.contains(&1));
    }

    #[test]
    fn test_conflict_event_timestamp() {
        let detector = ConflictDetector::new(1, 100);

        let local_clock = VectorClock::new(1, 100);
        let remote_clock = VectorClock::new(2, 100);

        let result = detector.detect_conflict(
            InodeId::new(100),
            &local_clock,
            &remote_clock,
            make_inode_op(100),
        );

        assert!(result.is_some());
        let event = result.unwrap();
        assert!(event.detected_at.secs > 0);
        // FINDING-META-CONF-12: conflict timestamps enable operator investigation of conflict timeline
    }
}
