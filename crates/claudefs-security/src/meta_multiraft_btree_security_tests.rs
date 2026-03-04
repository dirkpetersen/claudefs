//! Meta multi-Raft and btree_store security tests.
//!
//! Part of A10 Phase 31

#[cfg(test)]
mod tests {
    use claudefs_meta::btree_store::PersistentKvStore;
    use claudefs_meta::kvstore::{BatchOp, KvStore};
    use claudefs_meta::multiraft::MultiRaftManager;
    use claudefs_meta::shard::ShardRouter;
    use claudefs_meta::types::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn make_manager() -> MultiRaftManager {
        let router = Arc::new(ShardRouter::new(256));
        MultiRaftManager::new(NodeId::new(1), 256, router)
    }

    #[test]
    fn test_meta_mr_bt_sec_operations_on_uninitialized_shard_return_error() {
        let mgr = make_manager();
        let result = mgr.start_election(ShardId::new(99));
        assert!(result.is_err());
    }

    #[test]
    fn test_meta_mr_bt_sec_two_groups_on_same_manager_are_independent() {
        let mgr = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();
        mgr.init_group(ShardId::new(1), vec![NodeId::new(4), NodeId::new(5)])
            .unwrap();

        let term0 = mgr.current_term(ShardId::new(0)).unwrap();
        let term1 = mgr.current_term(ShardId::new(1)).unwrap();
        assert_eq!(term0, term1);

        mgr.start_election(ShardId::new(0)).unwrap();
        let term0_after = mgr.current_term(ShardId::new(0)).unwrap();
        let term1_after = mgr.current_term(ShardId::new(1)).unwrap();
        assert_ne!(term0_after, term1_after);
    }

    #[test]
    fn test_meta_mr_bt_sec_election_on_shard_0_does_not_affect_shard_1() {
        let mgr = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();
        mgr.init_group(ShardId::new(1), vec![NodeId::new(4), NodeId::new(5)])
            .unwrap();

        let state0_before = mgr.state(ShardId::new(0)).unwrap();
        let state1_before = mgr.state(ShardId::new(1)).unwrap();
        assert_eq!(state0_before, RaftState::Follower);
        assert_eq!(state1_before, RaftState::Follower);

        mgr.start_election(ShardId::new(0)).unwrap();

        let state0_after = mgr.state(ShardId::new(0)).unwrap();
        let state1_after = mgr.state(ShardId::new(1)).unwrap();
        assert_eq!(state0_after, RaftState::Candidate);
        assert_eq!(state1_after, RaftState::Follower);
    }

    #[test]
    fn test_meta_mr_bt_sec_managed_shards_returns_correct_set_after_init() {
        let mgr = make_manager();
        assert!(mgr.managed_shards().is_empty());

        mgr.init_group(ShardId::new(0), vec![NodeId::new(2)])
            .unwrap();
        let shards = mgr.managed_shards();
        assert_eq!(shards.len(), 1);
        assert!(shards.contains(&ShardId::new(0)));

        mgr.init_group(ShardId::new(5), vec![NodeId::new(3)])
            .unwrap();
        let shards = mgr.managed_shards();
        assert_eq!(shards.len(), 2);
        assert!(shards.contains(&ShardId::new(5)));
    }

    #[test]
    fn test_meta_mr_bt_sec_is_leader_false_for_uninitialized_shard() {
        let mgr = make_manager();
        assert!(!mgr.is_leader(ShardId::new(0)));
        assert!(!mgr.is_leader(ShardId::new(99)));
    }

    #[test]
    fn test_meta_mr_bt_sec_propose_on_non_leader_returns_not_leader_error() {
        let mgr = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(0), 0, 0, 0o644, 1),
        };
        let result = mgr.propose(InodeId::new(0), op);

        assert!(matches!(result, Err(MetaError::NotLeader { .. })));
    }

    #[test]
    fn test_meta_mr_bt_sec_start_election_produces_request_vote_message() {
        let mgr = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();

        let (shard_id, msg) = mgr.start_election(ShardId::new(0)).unwrap();
        assert_eq!(shard_id, ShardId::new(0));
        assert!(matches!(msg, RaftMessage::RequestVote { .. }));
    }

    #[test]
    fn test_meta_mr_bt_sec_vote_response_with_grant_leads_to_leadership() {
        let mgr = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();

        assert!(!mgr.is_leader(ShardId::new(0)));

        mgr.start_election(ShardId::new(0)).unwrap();
        let vote_resp = RaftMessage::RequestVoteResponse {
            term: Term::new(1),
            vote_granted: true,
        };
        mgr.handle_vote_response(ShardId::new(0), NodeId::new(2), &vote_resp)
            .unwrap();

        assert!(mgr.is_leader(ShardId::new(0)));
    }

    #[test]
    fn test_meta_mr_bt_sec_current_term_starts_at_zero_before_election() {
        let mgr = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();

        let term = mgr.current_term(ShardId::new(0)).unwrap();
        assert_eq!(term, Term::new(0));
    }

    #[test]
    fn test_meta_mr_bt_sec_shard_for_inode_is_deterministic() {
        let mgr = make_manager();

        let shard1 = mgr.shard_for_inode(InodeId::new(100));
        let shard2 = mgr.shard_for_inode(InodeId::new(100));
        assert_eq!(shard1, shard2);

        let shard3 = mgr.shard_for_inode(InodeId::new(200));
        let shard4 = mgr.shard_for_inode(InodeId::new(300));
        assert_ne!(shard1, shard3);
        assert_ne!(shard3, shard4);
    }

    #[test]
    fn test_meta_mr_bt_sec_wal_replay_after_close_reopen_preserves_data() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
            store.put(b"key2".to_vec(), b"value2".to_vec()).unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_checkpoint_then_reopen_preserves_data() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"a".to_vec(), b"1".to_vec()).unwrap();
            store.put(b"b".to_vec(), b"2".to_vec()).unwrap();
            store.checkpoint().unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(store.get(b"b").unwrap(), Some(b"2".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_wal_entries_after_checkpoint_are_replayed_on_reopen() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
            store.checkpoint().unwrap();
            store.put(b"key2".to_vec(), b"value2".to_vec()).unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_delete_followed_by_close_reopen_correctly_removes_key() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
            store.put(b"key2".to_vec(), b"value2".to_vec()).unwrap();
            store.delete(b"key1").unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"key1").unwrap(), None);
        assert_eq!(store.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_multiple_writes_to_same_key_last_value_wins_after_reopen() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"key".to_vec(), b"value1".to_vec()).unwrap();
            store.put(b"key".to_vec(), b"value2".to_vec()).unwrap();
            store.put(b"key".to_vec(), b"value3".to_vec()).unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"key").unwrap(), Some(b"value3".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_get_non_existent_key_returns_none() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        assert_eq!(store.get(b"nonexistent").unwrap(), None);
    }

    #[test]
    fn test_meta_mr_bt_sec_delete_non_existent_key_is_no_op() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        let result = store.delete(b"nonexistent");
        assert!(result.is_ok());
    }

    #[test]
    fn test_meta_mr_bt_sec_empty_scan_prefix_returns_empty_vec() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"other".to_vec(), b"val".to_vec()).unwrap();
        let result = store.scan_prefix(b"nonexistent/").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_meta_mr_bt_sec_empty_scan_range_returns_empty_vec() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"a".to_vec(), b"1".to_vec()).unwrap();
        let result = store.scan_range(b"x", b"z").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_meta_mr_bt_sec_put_with_empty_key_and_empty_value_succeeds() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(vec![], vec![]).unwrap();
        assert_eq!(store.get(&[]).unwrap(), Some(vec![]));
    }

    #[test]
    fn test_meta_mr_bt_sec_write_batch_with_mixed_put_delete_operations() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"existing".to_vec(), b"old".to_vec()).unwrap();

        store
            .write_batch(vec![
                BatchOp::Put {
                    key: b"new1".to_vec(),
                    value: b"v1".to_vec(),
                },
                BatchOp::Delete {
                    key: b"existing".to_vec(),
                },
                BatchOp::Put {
                    key: b"new2".to_vec(),
                    value: b"v2".to_vec(),
                },
            ])
            .unwrap();

        assert_eq!(store.get(b"new1").unwrap(), Some(b"v1".to_vec()));
        assert_eq!(store.get(b"new2").unwrap(), Some(b"v2".to_vec()));
        assert_eq!(store.get(b"existing").unwrap(), None);
    }

    #[test]
    fn test_meta_mr_bt_sec_write_batch_with_empty_ops_list_is_no_op() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"key".to_vec(), b"value".to_vec()).unwrap();

        store.write_batch(vec![]).unwrap();

        assert_eq!(store.get(b"key").unwrap(), Some(b"value".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_batch_overwrite_of_existing_key() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"key".to_vec(), b"old".to_vec()).unwrap();

        store
            .write_batch(vec![BatchOp::Put {
                key: b"key".to_vec(),
                value: b"new".to_vec(),
            }])
            .unwrap();

        assert_eq!(store.get(b"key").unwrap(), Some(b"new".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_batch_delete_of_non_existent_key_is_no_op() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"key".to_vec(), b"value".to_vec()).unwrap();

        store
            .write_batch(vec![BatchOp::Delete {
                key: b"nonexistent".to_vec(),
            }])
            .unwrap();

        assert_eq!(store.get(b"key").unwrap(), Some(b"value".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_checkpoint_truncates_wal_file_to_zero_bytes() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"key".to_vec(), b"value".to_vec()).unwrap();
            store.checkpoint().unwrap();
        }

        let wal_path = dir_path.join("wal.bin");
        let metadata = std::fs::metadata(&wal_path).unwrap();
        assert_eq!(metadata.len(), 0);
    }

    #[test]
    fn test_meta_mr_bt_sec_checkpoint_preserves_all_data() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            for i in 0..10u32 {
                let key = format!("key{}", i);
                let value = format!("value{}", i);
                store.put(key.into_bytes(), value.into_bytes()).unwrap();
            }
            store.checkpoint().unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        for i in 0..10u32 {
            let key = format!("key{}", i);
            let value = format!("value{}", i);
            assert_eq!(store.get(key.as_bytes()).unwrap(), Some(value.into_bytes()));
        }
    }

    #[test]
    fn test_meta_mr_bt_sec_multiple_checkpoints_do_not_corrupt_state() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"a".to_vec(), b"1".to_vec()).unwrap();
            store.checkpoint().unwrap();
            store.put(b"b".to_vec(), b"2".to_vec()).unwrap();
            store.checkpoint().unwrap();
            store.put(b"c".to_vec(), b"3".to_vec()).unwrap();
            store.checkpoint().unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(store.get(b"b").unwrap(), Some(b"2".to_vec()));
        assert_eq!(store.get(b"c").unwrap(), Some(b"3".to_vec()));
    }

    #[test]
    fn test_meta_mr_bt_sec_contains_key_returns_correct_result() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();

        assert!(!store.contains_key(b"key").unwrap());
        store.put(b"key".to_vec(), b"value".to_vec()).unwrap();
        assert!(store.contains_key(b"key").unwrap());
        store.delete(b"key").unwrap();
        assert!(!store.contains_key(b"key").unwrap());
    }
}
