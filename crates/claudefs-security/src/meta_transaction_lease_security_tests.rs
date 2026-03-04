//! Meta distributed transaction and lease management security tests.
//!
//! Part of A10 Phase 22: Meta transaction/lease security audit

#[cfg(test)]
mod tests {
    use claudefs_meta::lease::{Lease, LeaseManager, LeaseType};
    use claudefs_meta::transaction::{
        Transaction, TransactionId, TransactionManager, TransactionParticipant, TransactionState,
    };
    use claudefs_meta::types::{
        DirEntry, FileType, InodeId, MetaError, MetaOp, NodeId, ShardId, Timestamp,
    };

    fn make_test_operation() -> MetaOp {
        MetaOp::CreateEntry {
            parent: InodeId::new(1),
            name: "test".to_string(),
            entry: DirEntry {
                name: "test".to_string(),
                ino: InodeId::new(2),
                file_type: FileType::RegularFile,
            },
        }
    }

    // ============================================================================
    // Category 1: Transaction State Machine (5 tests)
    // ============================================================================

    #[test]
    fn test_transaction_begin() {
        let mgr = TransactionManager::new(60);
        let coordinator = ShardId::new(1);
        let participants = vec![ShardId::new(2), ShardId::new(3)];
        let op = make_test_operation();

        let txn_id = mgr.begin_transaction(coordinator, participants.clone(), op);

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.state, TransactionState::Preparing);
        assert_eq!(txn.participants.len(), 2);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn test_transaction_all_commit() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, make_test_operation());

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_commit(txn_id, ShardId::new(2)).unwrap();

        let state = mgr.check_votes(txn_id).unwrap();
        assert_eq!(state, TransactionState::Committing);

        mgr.commit(txn_id).unwrap();

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.state, TransactionState::Committed);
    }

    #[test]
    fn test_transaction_any_abort() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2), ShardId::new(3)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, make_test_operation());

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_abort(txn_id, ShardId::new(2)).unwrap();
        mgr.vote_commit(txn_id, ShardId::new(3)).unwrap();

        let state = mgr.check_votes(txn_id).unwrap();
        assert_eq!(state, TransactionState::Aborting);

        mgr.abort(txn_id).unwrap();

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.state, TransactionState::Aborted);
    }

    #[test]
    fn test_transaction_partial_votes() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, make_test_operation());

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();

        let state = mgr.check_votes(txn_id).unwrap();
        assert_eq!(state, TransactionState::Preparing);
    }

    #[test]
    fn test_transaction_commit_wrong_state() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1)];

        let txn_id1 =
            mgr.begin_transaction(ShardId::new(0), participants.clone(), make_test_operation());
        let result = mgr.commit(txn_id1);
        assert!(result.is_err());

        let txn_id2 =
            mgr.begin_transaction(ShardId::new(0), participants.clone(), make_test_operation());
        mgr.abort(txn_id2).unwrap();
        let result = mgr.commit(txn_id2);
        assert!(result.is_err());
    }

    // ============================================================================
    // Category 2: Transaction Error Handling (5 tests)
    // ============================================================================

    #[test]
    fn test_transaction_vote_nonexistent() {
        let mgr = TransactionManager::new(60);
        let result = mgr.vote_commit(TransactionId::new(9999), ShardId::new(1));
        assert!(result.is_err());

        let result = mgr.vote_abort(TransactionId::new(9999), ShardId::new(1));
        assert!(result.is_err());
    }

    #[test]
    fn test_transaction_vote_wrong_shard() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, make_test_operation());

        let result = mgr.vote_commit(txn_id, ShardId::new(3));
        assert!(result.is_err());
    }

    #[test]
    fn test_transaction_double_vote() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, make_test_operation());

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.participants[0].voted, Some(true));

        mgr.vote_abort(txn_id, ShardId::new(1)).unwrap();

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.participants[0].voted, Some(false));
    }

    #[test]
    fn test_transaction_cleanup_completed() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1)];

        let txn_id1 =
            mgr.begin_transaction(ShardId::new(0), participants.clone(), make_test_operation());
        let txn_id2 =
            mgr.begin_transaction(ShardId::new(0), participants.clone(), make_test_operation());
        let _txn_id3 =
            mgr.begin_transaction(ShardId::new(0), participants.clone(), make_test_operation());

        mgr.vote_commit(txn_id1, ShardId::new(1)).unwrap();
        mgr.check_votes(txn_id1).unwrap();
        mgr.commit(txn_id1).unwrap();

        mgr.abort(txn_id2).unwrap();

        let cleaned = mgr.cleanup_completed();
        assert_eq!(cleaned, 2);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn test_transaction_timeout_cleanup() {
        let mgr = TransactionManager::new(0);
        let participants = vec![ShardId::new(1)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, make_test_operation());

        std::thread::sleep(std::time::Duration::from_millis(10));
        let timed_out = mgr.cleanup_timed_out();

        assert!(timed_out.contains(&txn_id));
        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.state, TransactionState::Aborted);
    }

    // ============================================================================
    // Category 3: Transaction ID & Participant (3 tests)
    // ============================================================================

    #[test]
    fn test_transaction_id_unique() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1)];

        let ids: Vec<TransactionId> = (0..5)
            .map(|_| {
                mgr.begin_transaction(ShardId::new(0), participants.clone(), make_test_operation())
            })
            .collect();

        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }

        let mut values: Vec<u64> = ids.iter().map(|id| id.as_u64()).collect();
        values.sort();
        for i in 0..values.len() - 1 {
            assert_eq!(values[i] + 1, values[i + 1]);
        }
    }

    #[test]
    fn test_transaction_participant_votes() {
        let mut participant = TransactionParticipant::new(ShardId::new(1));
        assert_eq!(participant.voted, None);

        participant.vote_commit();
        assert_eq!(participant.voted, Some(true));

        let mut participant2 = TransactionParticipant::new(ShardId::new(2));
        participant2.vote_abort();
        assert_eq!(participant2.voted, Some(false));
    }

    #[test]
    fn test_transaction_id_display() {
        let id = TransactionId::new(42);
        let s = id.to_string();
        assert!(s.contains("42"));
        assert_eq!(id.as_u64(), 42);
    }

    // ============================================================================
    // Category 4: Lease Grant & Revocation (5 tests)
    // ============================================================================

    #[test]
    fn test_lease_grant_read() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);
        let client = NodeId::new(100);

        let lease_id = mgr.grant(ino, client, LeaseType::Read).unwrap();
        assert!(lease_id > 0);
        assert!(mgr.has_valid_lease(ino, client));
        assert_eq!(mgr.active_lease_count(), 1);
    }

    #[test]
    fn test_lease_multiple_read_coexist() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let id1 = mgr.grant(ino, NodeId::new(100), LeaseType::Read).unwrap();
        let id2 = mgr.grant(ino, NodeId::new(101), LeaseType::Read).unwrap();
        let id3 = mgr.grant(ino, NodeId::new(102), LeaseType::Read).unwrap();

        assert!(id1 > 0);
        assert!(id2 > 0);
        assert!(id3 > 0);
        assert!(mgr.has_valid_lease(ino, NodeId::new(100)));
        assert!(mgr.has_valid_lease(ino, NodeId::new(101)));
        assert!(mgr.has_valid_lease(ino, NodeId::new(102)));

        let leases = mgr.leases_on(ino);
        assert_eq!(leases.len(), 3);
    }

    #[test]
    fn test_lease_write_exclusive() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Write).unwrap();

        let result = mgr.grant(ino, NodeId::new(101), LeaseType::Read);
        assert!(matches!(result, Err(MetaError::PermissionDenied)));

        let result = mgr.grant(ino, NodeId::new(101), LeaseType::Write);
        assert!(matches!(result, Err(MetaError::PermissionDenied)));
    }

    #[test]
    fn test_lease_write_blocked_by_read() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Read).unwrap();

        let result = mgr.grant(ino, NodeId::new(101), LeaseType::Write);
        assert!(matches!(result, Err(MetaError::PermissionDenied)));
    }

    #[test]
    fn test_lease_revoke_inode() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Read).unwrap();
        let _ = mgr.grant(ino, NodeId::new(101), LeaseType::Read).unwrap();

        let clients = mgr.revoke(ino);
        assert_eq!(clients.len(), 2);
        assert!(clients.contains(&NodeId::new(100)));
        assert!(clients.contains(&NodeId::new(101)));
        assert!(!mgr.has_valid_lease(ino, NodeId::new(100)));
        assert!(!mgr.has_valid_lease(ino, NodeId::new(101)));
    }

    // ============================================================================
    // Category 5: Lease Advanced Operations (7 tests)
    // ============================================================================

    #[test]
    fn test_lease_revoke_specific() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);
        let client = NodeId::new(100);

        let lease_id = mgr.grant(ino, client, LeaseType::Read).unwrap();

        mgr.revoke_lease(lease_id).unwrap();
        assert!(!mgr.has_valid_lease(ino, client));

        let result = mgr.revoke_lease(lease_id);
        assert!(matches!(result, Err(MetaError::PermissionDenied)));
    }

    #[test]
    fn test_lease_revoke_client() {
        let mgr = LeaseManager::new(30);
        let client = NodeId::new(100);

        let _ = mgr.grant(InodeId::new(1), client, LeaseType::Read).unwrap();
        let _ = mgr.grant(InodeId::new(2), client, LeaseType::Read).unwrap();
        let _ = mgr.grant(InodeId::new(3), client, LeaseType::Read).unwrap();

        let count = mgr.revoke_client(client);
        assert_eq!(count, 3);
        assert!(!mgr.has_valid_lease(InodeId::new(1), client));
        assert!(!mgr.has_valid_lease(InodeId::new(2), client));
        assert!(!mgr.has_valid_lease(InodeId::new(3), client));
    }

    #[test]
    fn test_lease_renew() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);
        let client = NodeId::new(100);

        let lease_id = mgr.grant(ino, client, LeaseType::Read).unwrap();

        let original_leases = mgr.leases_on(ino);
        let original_expires = original_leases[0].expires_at;

        mgr.renew(lease_id).unwrap();

        let updated_leases = mgr.leases_on(ino);
        assert!(updated_leases[0].expires_at > original_expires);

        let result = mgr.renew(9999);
        assert!(matches!(result, Err(MetaError::PermissionDenied)));
    }

    #[test]
    fn test_lease_count() {
        let mgr = LeaseManager::new(30);

        let _ = mgr
            .grant(InodeId::new(1), NodeId::new(100), LeaseType::Read)
            .unwrap();
        let _ = mgr
            .grant(InodeId::new(2), NodeId::new(101), LeaseType::Read)
            .unwrap();
        let _ = mgr
            .grant(InodeId::new(3), NodeId::new(102), LeaseType::Read)
            .unwrap();
        let _ = mgr
            .grant(InodeId::new(4), NodeId::new(103), LeaseType::Read)
            .unwrap();
        let _ = mgr
            .grant(InodeId::new(5), NodeId::new(104), LeaseType::Read)
            .unwrap();

        assert_eq!(mgr.active_lease_count(), 5);

        mgr.revoke_lease(1).unwrap();
        assert_eq!(mgr.active_lease_count(), 4);
    }

    #[test]
    fn test_lease_leases_on_empty() {
        let mgr = LeaseManager::new(30);

        let leases = mgr.leases_on(InodeId::new(999));
        assert!(leases.is_empty());
        assert_eq!(mgr.active_lease_count(), 0);
    }

    #[test]
    fn test_lease_type_variants() {
        assert_ne!(LeaseType::Read, LeaseType::Write);

        let read = LeaseType::Read;
        let write = LeaseType::Write;

        match read {
            LeaseType::Read => {}
            LeaseType::Write => panic!("expected Read"),
        }

        match write {
            LeaseType::Read => panic!("expected Write"),
            LeaseType::Write => {}
        }
    }

    #[test]
    fn test_lease_grant_after_revoke() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Write).unwrap();
        mgr.revoke(ino);

        let result = mgr.grant(ino, NodeId::new(100), LeaseType::Write);
        assert!(result.is_ok());

        let result = mgr.grant(ino, NodeId::new(101), LeaseType::Read);
        assert!(matches!(result, Err(MetaError::PermissionDenied)));
    }
}
