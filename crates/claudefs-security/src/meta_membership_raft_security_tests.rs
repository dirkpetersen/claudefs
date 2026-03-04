//! Metadata membership and Raft log security tests.
//!
//! Part of A10 Phase 27: Meta membership + Raft log security audit

#[cfg(test)]
mod tests {
    use claudefs_meta::kvstore::KvStore;
    use claudefs_meta::kvstore::MemoryKvStore;
    use claudefs_meta::membership::{MembershipEvent, MembershipManager, NodeState};
    use claudefs_meta::raft_log::RaftLogStore;
    use claudefs_meta::types::{InodeAttr, InodeId, LogEntry, LogIndex, MetaOp, NodeId, Term};
    use std::sync::Arc;

    fn make_node_id(id: u64) -> NodeId {
        NodeId::new(id)
    }

    fn make_kv() -> Arc<dyn KvStore> {
        Arc::new(MemoryKvStore::new())
    }

    fn make_entry(index: u64, term: u64) -> LogEntry {
        LogEntry {
            index: LogIndex::new(index),
            term: Term::new(term),
            op: MetaOp::CreateInode {
                attr: InodeAttr::new_file(InodeId::new(index), 0, 0, 0o644, 0),
            },
        }
    }

    // ============================================================================
    // Category 1: Membership Sybil / Rapid Join-Leave (5 tests)
    // ============================================================================

    #[test]
    fn test_mem_raft_sec_rapid_join_stress() {
        let mgr = MembershipManager::new(make_node_id(1));

        for i in 0..150u64 {
            mgr.join(make_node_id(i), format!("192.168.1.{}:8080", i % 256))
                .unwrap();
        }

        assert_eq!(mgr.member_count(), 150);
        assert_eq!(mgr.alive_count(), 150);
    }

    #[test]
    fn test_mem_raft_sec_join_same_node_id_twice() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        let events1 = mgr.drain_events();

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        let events2 = mgr.drain_events();

        assert_eq!(mgr.member_count(), 1);
        assert_eq!(events1.len(), 1);
        assert_eq!(events2.len(), 1);
    }

    #[test]
    fn test_mem_raft_sec_join_leave_rejoin_cycle() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        for _ in 0..5 {
            mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
            assert_eq!(mgr.member_count(), 1);

            let removed = mgr.leave(node_id).unwrap();
            assert!(removed);
            assert_eq!(mgr.member_count(), 0);
        }

        let member = mgr.get_member(node_id);
        assert!(member.is_none());
    }

    #[test]
    fn test_mem_raft_sec_leave_nonexistent_returns_false() {
        let mgr = MembershipManager::new(make_node_id(1));

        let removed = mgr.leave(make_node_id(999)).unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_mem_raft_sec_suspect_mark_dead_nonexistent_error() {
        let mgr = MembershipManager::new(make_node_id(1));

        let result = mgr.suspect(make_node_id(999));
        assert!(result.is_err());

        let result = mgr.mark_dead(make_node_id(999));
        assert!(result.is_err());
    }

    // ============================================================================
    // Category 2: Membership State Machine Violations (5 tests)
    // ============================================================================

    #[test]
    fn test_mem_raft_sec_suspect_dead_node_noop() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        mgr.mark_dead(node_id).unwrap();

        let _ = mgr.drain_events();

        mgr.suspect(node_id).unwrap();

        let member = mgr.get_member(node_id).unwrap();
        assert_eq!(member.state, NodeState::Dead);

        let events = mgr.drain_events();
        assert!(events.is_empty());
    }

    #[test]
    fn test_mem_raft_sec_mark_dead_already_dead_noop() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        mgr.mark_dead(node_id).unwrap();
        let _ = mgr.drain_events();

        mgr.mark_dead(node_id).unwrap();

        let member = mgr.get_member(node_id).unwrap();
        assert_eq!(member.state, NodeState::Dead);

        let events = mgr.drain_events();
        assert!(events.is_empty());
    }

    #[test]
    fn test_mem_raft_sec_confirm_alive_dead_node_no_recovery() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        mgr.mark_dead(node_id).unwrap();
        let _ = mgr.drain_events();

        mgr.confirm_alive(node_id).unwrap();

        let member = mgr.get_member(node_id).unwrap();
        assert_eq!(member.state, NodeState::Dead);

        let events = mgr.drain_events();
        assert!(events.is_empty());
    }

    #[test]
    fn test_mem_raft_sec_heartbeat_dead_node_updates_timestamp() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        mgr.mark_dead(node_id).unwrap();

        let member_before = mgr.get_member(node_id).unwrap();
        let ts_before = member_before.last_heartbeat;

        mgr.heartbeat(node_id).unwrap();

        let member_after = mgr.get_member(node_id).unwrap();
        assert_eq!(member_after.state, NodeState::Dead);
        assert!(member_after.last_heartbeat.secs >= ts_before.secs);
    }

    #[test]
    fn test_mem_raft_sec_generation_saturating_no_overflow() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();

        for _ in 0..10 {
            mgr.suspect(node_id).unwrap();
            mgr.confirm_alive(node_id).unwrap();
        }

        let member = mgr.get_member(node_id).unwrap();
        assert!(member.generation >= 20);
    }

    // ============================================================================
    // Category 3: Event Integrity (4 tests)
    // ============================================================================

    #[test]
    fn test_mem_raft_sec_events_order_join_suspect_recover() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        mgr.suspect(node_id).unwrap();
        mgr.confirm_alive(node_id).unwrap();

        let events = mgr.drain_events();

        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], MembershipEvent::NodeJoined { .. }));
        assert!(matches!(events[1], MembershipEvent::NodeSuspected { .. }));
        assert!(matches!(events[2], MembershipEvent::NodeRecovered { .. }));
    }

    #[test]
    fn test_mem_raft_sec_events_drain_twice_empty_second() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(42), "192.168.1.42:8080".to_string())
            .unwrap();

        let events1 = mgr.drain_events();
        assert_eq!(events1.len(), 1);

        let events2 = mgr.drain_events();
        assert!(events2.is_empty());
    }

    #[test]
    fn test_mem_raft_sec_multiple_transitions_event_sequence() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        mgr.suspect(node_id).unwrap();
        mgr.confirm_alive(node_id).unwrap();
        mgr.suspect(node_id).unwrap();
        mgr.mark_dead(node_id).unwrap();

        let events = mgr.drain_events();

        assert_eq!(events.len(), 5);
        assert!(matches!(events[0], MembershipEvent::NodeJoined { .. }));
        assert!(matches!(events[1], MembershipEvent::NodeSuspected { .. }));
        assert!(matches!(events[2], MembershipEvent::NodeRecovered { .. }));
        assert!(matches!(events[3], MembershipEvent::NodeSuspected { .. }));
        assert!(matches!(events[4], MembershipEvent::NodeDead { .. }));
    }

    #[test]
    fn test_mem_raft_sec_leave_generates_node_dead_event() {
        let mgr = MembershipManager::new(make_node_id(1));
        let node_id = make_node_id(42);

        mgr.join(node_id, "192.168.1.42:8080".to_string()).unwrap();
        let _ = mgr.drain_events();

        mgr.leave(node_id).unwrap();

        let events = mgr.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            MembershipEvent::NodeDead {
                node_id: id
            } if id == node_id
        ));
    }

    // ============================================================================
    // Category 4: Raft Log Injection / Corruption (5 tests)
    // ============================================================================

    #[test]
    fn test_mem_raft_sec_invalid_term_bytes_error() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv.clone());

        kv.put(b"raft/term".to_vec(), b"invalid".to_vec()).unwrap();

        let result = store.load_term();
        assert!(result.is_err());
    }

    #[test]
    fn test_mem_raft_sec_invalid_voted_for_bytes_error() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv.clone());

        kv.put(b"raft/voted_for".to_vec(), b"short".to_vec())
            .unwrap();

        let result = store.load_voted_for();
        assert!(result.is_err());
    }

    #[test]
    fn test_mem_raft_sec_invalid_commit_index_bytes_error() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv.clone());

        kv.put(b"raft/commit_index".to_vec(), vec![0x00, 0x01, 0x02])
            .unwrap();

        let result = store.load_commit_index();
        assert!(result.is_err());
    }

    #[test]
    fn test_mem_raft_sec_append_entry_index_zero() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        let entry = make_entry(0, 1);
        store.append_entry(&entry).unwrap();

        let retrieved = store.get_entry(LogIndex::new(0)).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().index.as_u64(), 0);
    }

    #[test]
    fn test_mem_raft_sec_append_entry_index_max() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        let entry = make_entry(u64::MAX, 1);
        store.append_entry(&entry).unwrap();

        let retrieved = store.get_entry(LogIndex::new(u64::MAX)).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().index.as_u64(), u64::MAX);
    }

    // ============================================================================
    // Category 5: Raft Log Truncation Safety (5 tests)
    // ============================================================================

    #[test]
    fn test_mem_raft_sec_truncate_from_index_one_removes_all() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        for i in 1..=5 {
            store.append_entry(&make_entry(i, 1)).unwrap();
        }

        store.truncate_from(LogIndex::new(1)).unwrap();

        assert_eq!(store.entry_count().unwrap(), 0);
    }

    #[test]
    fn test_mem_raft_sec_truncate_from_beyond_last_noop() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        store.append_entry(&make_entry(1, 1)).unwrap();
        store.append_entry(&make_entry(2, 1)).unwrap();

        store.truncate_from(LogIndex::new(100)).unwrap();

        assert_eq!(store.entry_count().unwrap(), 2);
    }

    #[test]
    fn test_mem_raft_sec_truncate_then_append_consistency() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        for i in 1..=5 {
            store.append_entry(&make_entry(i, 1)).unwrap();
        }

        store.truncate_from(LogIndex::new(3)).unwrap();
        assert_eq!(store.entry_count().unwrap(), 2);

        store.append_entry(&make_entry(3, 2)).unwrap();
        store.append_entry(&make_entry(4, 2)).unwrap();

        assert_eq!(store.entry_count().unwrap(), 4);

        let e3 = store.get_entry(LogIndex::new(3)).unwrap().unwrap();
        assert_eq!(e3.term.as_u64(), 2);
    }

    #[test]
    fn test_mem_raft_sec_multiple_rapid_truncations() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        for i in 1..=10 {
            store.append_entry(&make_entry(i, 1)).unwrap();
        }

        store.truncate_from(LogIndex::new(8)).unwrap();
        assert_eq!(store.entry_count().unwrap(), 7);

        store.truncate_from(LogIndex::new(5)).unwrap();
        assert_eq!(store.entry_count().unwrap(), 4);

        store.truncate_from(LogIndex::new(2)).unwrap();
        assert_eq!(store.entry_count().unwrap(), 1);
    }

    #[test]
    fn test_mem_raft_sec_truncate_from_zero_removes_everything() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        for i in 1..=5 {
            store.append_entry(&make_entry(i, 1)).unwrap();
        }

        store.truncate_from(LogIndex::new(0)).unwrap();

        assert_eq!(store.entry_count().unwrap(), 0);
        assert_eq!(store.last_index().unwrap().as_u64(), 0);
    }

    // ============================================================================
    // Category 6: Raft Hard State Atomicity (4 tests)
    // ============================================================================

    #[test]
    fn test_mem_raft_sec_hard_state_voted_for_none_deletes_key() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv.clone());

        store.save_voted_for(Some(make_node_id(42))).unwrap();
        assert!(kv.get(b"raft/voted_for").unwrap().is_some());

        store.save_voted_for(None).unwrap();
        assert!(kv.get(b"raft/voted_for").unwrap().is_none());
    }

    #[test]
    fn test_mem_raft_sec_hard_state_roundtrip_all_fields() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        store
            .save_hard_state(Term::new(5), Some(make_node_id(42)), LogIndex::new(100))
            .unwrap();

        assert_eq!(store.load_term().unwrap().as_u64(), 5);
        assert_eq!(store.load_voted_for().unwrap().unwrap().as_u64(), 42);
        assert_eq!(store.load_commit_index().unwrap().as_u64(), 100);
    }

    #[test]
    fn test_mem_raft_sec_hard_state_overwrite_multiple_times() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        store
            .save_hard_state(Term::new(1), Some(make_node_id(1)), LogIndex::new(10))
            .unwrap();

        store
            .save_hard_state(Term::new(2), Some(make_node_id(2)), LogIndex::new(20))
            .unwrap();

        store
            .save_hard_state(Term::new(3), Some(make_node_id(3)), LogIndex::new(30))
            .unwrap();

        assert_eq!(store.load_term().unwrap().as_u64(), 3);
        assert_eq!(store.load_voted_for().unwrap().unwrap().as_u64(), 3);
        assert_eq!(store.load_commit_index().unwrap().as_u64(), 30);
    }

    #[test]
    fn test_mem_raft_sec_hard_state_term_zero_initial() {
        let kv = make_kv();
        let store = RaftLogStore::new(kv);

        store
            .save_hard_state(Term::new(0), None, LogIndex::new(0))
            .unwrap();

        assert_eq!(store.load_term().unwrap().as_u64(), 0);
        assert!(store.load_voted_for().unwrap().is_none());
        assert_eq!(store.load_commit_index().unwrap().as_u64(), 0);
    }
}
