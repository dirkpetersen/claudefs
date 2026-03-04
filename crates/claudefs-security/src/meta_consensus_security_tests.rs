//! Consensus security tests for claudefs-meta: Raft, membership, leases, ReadIndex, follower reads.
//!
//! Part of A10 Phase 9: Meta consensus security audit

#[cfg(test)]
mod tests {
    use claudefs_meta::{
        consensus::{RaftConfig, RaftNode},
        follower_read::{FollowerReadConfig, FollowerReadRouter, ReadConsistency, ReadTarget},
        lease::{LeaseManager, LeaseType},
        membership::{MemberInfo, MembershipEvent, MembershipManager, NodeState},
        pathres::{NegativeCacheEntry, PathResolver},
        readindex::{PendingRead, ReadIndexManager, ReadStatus},
        types::{FileType, InodeAttr, LogIndex, MetaOp, NodeId, RaftState, Term, Timestamp},
        InodeId, ShardId,
    };

    fn make_three_node_config(node_id: NodeId) -> RaftConfig {
        RaftConfig {
            node_id,
            peers: vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]
                .into_iter()
                .filter(|&id| id != node_id)
                .collect(),
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
        }
    }

    fn make_membership_manager() -> MembershipManager {
        MembershipManager::new(NodeId::new(1))
    }

    fn make_lease_manager() -> LeaseManager {
        LeaseManager::new(30)
    }

    fn make_readindex_manager() -> ReadIndexManager {
        ReadIndexManager::new(5)
    }

    fn make_follower_read_router() -> FollowerReadRouter {
        FollowerReadRouter::new(FollowerReadConfig::default())
    }

    fn make_path_resolver() -> PathResolver {
        PathResolver::new(256, 1000, 30, 10000)
    }

    // ============================================================================
    // Category 1: Raft Consensus Safety (5 tests)
    // ============================================================================

    #[test]
    fn test_raft_initial_state_follower() {
        let config = make_three_node_config(NodeId::new(1));
        let node = RaftNode::new(config);

        assert_eq!(node.state(), RaftState::Follower);
        assert_eq!(node.current_term(), Term::new(0));
        assert_eq!(node.voted_for(), None);
    }

    #[test]
    fn test_raft_election_increments_term() {
        let config = make_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        let msg = node.start_election();

        assert_eq!(node.current_term(), Term::new(1));
        assert_eq!(node.state(), RaftState::Candidate);
        assert_eq!(node.voted_for(), Some(NodeId::new(1)));

        match msg {
            claudefs_meta::types::RaftMessage::RequestVote {
                term, candidate_id, ..
            } => {
                assert_eq!(term, Term::new(1));
                assert_eq!(candidate_id, NodeId::new(1));
            }
            _ => panic!("expected RequestVote message"),
        }
    }

    #[test]
    fn test_raft_propose_as_follower_fails() {
        let config = make_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };

        let result = node.propose(op);

        assert!(result.is_err());
        match result.unwrap_err() {
            claudefs_meta::types::MetaError::NotLeader { .. } => {}
            e => panic!("expected NotLeader error, got: {:?}", e),
        }
    }

    #[test]
    fn test_raft_term_monotonic() {
        let config = make_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        node.start_election();
        let term_after_first = node.current_term();
        assert_eq!(term_after_first, Term::new(1));

        node.start_election();
        let term_after_second = node.current_term();
        assert_eq!(term_after_second, Term::new(2));

        assert!(term_after_second >= term_after_first);
    }

    #[test]
    fn test_raft_leadership_transfer() {
        let config = make_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        node.start_election();
        let _ = node.handle_vote_response(
            NodeId::new(2),
            &claudefs_meta::types::RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );
        let _ = node.handle_vote_response(
            NodeId::new(3),
            &claudefs_meta::types::RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );

        assert_eq!(node.state(), RaftState::Leader);

        let transfer_result = node.transfer_leadership(NodeId::new(2));
        assert!(transfer_result.is_ok());
        assert!(node.is_transferring());

        node.cancel_transfer();
        assert!(!node.is_transferring());
    }

    // ============================================================================
    // Category 2: Membership Management (5 tests)
    // ============================================================================

    #[test]
    fn test_membership_join_leave() {
        let mgr = make_membership_manager();

        mgr.join(NodeId::new(2), "192.168.1.2:8080".to_string())
            .unwrap();
        mgr.join(NodeId::new(3), "192.168.1.3:8080".to_string())
            .unwrap();

        assert_eq!(mgr.member_count(), 2);
        assert_eq!(mgr.alive_count(), 2);

        mgr.leave(NodeId::new(2)).unwrap();

        assert_eq!(mgr.member_count(), 1);
    }

    #[test]
    fn test_membership_state_transitions() {
        let mgr = make_membership_manager();

        mgr.join(NodeId::new(2), "192.168.1.2:8080".to_string())
            .unwrap();

        let member = mgr.get_member(NodeId::new(2)).unwrap();
        assert_eq!(member.state, NodeState::Alive);

        mgr.suspect(NodeId::new(2)).unwrap();
        let member = mgr.get_member(NodeId::new(2)).unwrap();
        assert_eq!(member.state, NodeState::Suspect);

        mgr.confirm_alive(NodeId::new(2)).unwrap();
        let member = mgr.get_member(NodeId::new(2)).unwrap();
        assert_eq!(member.state, NodeState::Alive);

        mgr.mark_dead(NodeId::new(2)).unwrap();
        let member = mgr.get_member(NodeId::new(2)).unwrap();
        assert_eq!(member.state, NodeState::Dead);
    }

    #[test]
    fn test_membership_events_emitted() {
        let mgr = make_membership_manager();

        mgr.join(NodeId::new(2), "192.168.1.2:8080".to_string())
            .unwrap();
        mgr.suspect(NodeId::new(2)).unwrap();
        mgr.mark_dead(NodeId::new(2)).unwrap();

        let events = mgr.drain_events();

        assert!(events.len() >= 3);
        assert!(matches!(
            events[0],
            MembershipEvent::NodeJoined { node_id } if node_id == NodeId::new(2)
        ));
        assert!(matches!(
            events[1],
            MembershipEvent::NodeSuspected { node_id } if node_id == NodeId::new(2)
        ));
        assert!(matches!(
            events[2],
            MembershipEvent::NodeDead { node_id } if node_id == NodeId::new(2)
        ));
    }

    #[test]
    fn test_membership_duplicate_join() {
        let mgr = make_membership_manager();

        let result1 = mgr.join(NodeId::new(2), "192.168.1.2:8080".to_string());
        assert!(result1.is_ok());

        let result2 = mgr.join(NodeId::new(2), "192.168.1.2:8080".to_string());

        if result2.is_err() {
            // Duplicate rejected
        } else {
            // Duplicate accepted (idempotent)
            // FINDING-META-CONS-01: Duplicate join is idempotent, not rejected
        }

        assert_eq!(mgr.member_count(), 1);
    }

    #[test]
    fn test_membership_suspect_unknown_node() {
        let mgr = make_membership_manager();

        let result = mgr.suspect(NodeId::new(99));

        assert!(result.is_err());
    }

    // ============================================================================
    // Category 3: Lease Management (5 tests)
    // ============================================================================

    #[test]
    fn test_lease_write_exclusivity() {
        let mgr = make_lease_manager();
        let ino = InodeId::new(1);

        let result1 = mgr.grant(ino, NodeId::new(100), LeaseType::Write);
        assert!(result1.is_ok());

        let result2 = mgr.grant(ino, NodeId::new(101), LeaseType::Write);
        assert!(result2.is_err());
    }

    #[test]
    fn test_lease_read_coexistence() {
        let mgr = make_lease_manager();
        let ino = InodeId::new(1);

        let result1 = mgr.grant(ino, NodeId::new(100), LeaseType::Read);
        assert!(result1.is_ok());

        let result2 = mgr.grant(ino, NodeId::new(101), LeaseType::Read);
        assert!(result2.is_ok());

        let leases = mgr.leases_on(ino);
        assert_eq!(leases.len(), 2);
    }

    #[test]
    fn test_lease_revoke_client_cleanup() {
        let mgr = make_lease_manager();
        let client = NodeId::new(100);

        mgr.grant(InodeId::new(1), client, LeaseType::Read).unwrap();
        mgr.grant(InodeId::new(2), client, LeaseType::Read).unwrap();
        mgr.grant(InodeId::new(3), client, LeaseType::Read).unwrap();

        let count = mgr.revoke_client(client);
        assert_eq!(count, 3);
        assert_eq!(mgr.active_lease_count(), 0);
    }

    #[test]
    fn test_lease_renew_expired_fails() {
        let mgr = LeaseManager::new(0);
        let ino = InodeId::new(1);
        let client = NodeId::new(100);

        let lease_id = mgr.grant(ino, client, LeaseType::Read).unwrap();

        mgr.cleanup_expired();

        let result = mgr.renew(lease_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_lease_id_uniqueness() {
        let mgr = make_lease_manager();
        let mut lease_ids = std::collections::HashSet::new();

        for i in 1..=100 {
            let lease_id = mgr
                .grant(InodeId::new(i), NodeId::new(i as u64), LeaseType::Read)
                .unwrap();
            assert!(lease_ids.insert(lease_id));
        }

        assert_eq!(lease_ids.len(), 100);
    }

    // ============================================================================
    // Category 4: ReadIndex Protocol (5 tests)
    // ============================================================================

    #[test]
    fn test_readindex_quorum_calculation() {
        let mgr = make_readindex_manager();
        let read_id = mgr.register_read(LogIndex::new(10), InodeId::new(1), 5);

        assert!(!mgr.has_quorum(read_id));

        mgr.confirm_heartbeat(read_id, NodeId::new(2)).unwrap();
        mgr.confirm_heartbeat(read_id, NodeId::new(3)).unwrap();
        assert!(!mgr.has_quorum(read_id));

        mgr.confirm_heartbeat(read_id, NodeId::new(4)).unwrap();
        assert!(mgr.has_quorum(read_id));
    }

    #[test]
    fn test_readindex_duplicate_confirmation() {
        let mgr = make_readindex_manager();
        let read_id = mgr.register_read(LogIndex::new(10), InodeId::new(1), 3);

        mgr.confirm_heartbeat(read_id, NodeId::new(2)).unwrap();
        mgr.confirm_heartbeat(read_id, NodeId::new(2)).unwrap();

        assert!(!mgr.has_quorum(read_id));
    }

    #[test]
    fn test_readindex_timeout_cleanup() {
        let mgr = ReadIndexManager::new(1);
        let read_id = mgr.register_read(LogIndex::new(10), InodeId::new(1), 3);

        std::thread::sleep(std::time::Duration::from_millis(1100));

        let timed_out = mgr.cleanup_timed_out();
        assert!(timed_out.contains(&read_id));
    }

    #[test]
    fn test_readindex_status_waiting_for_apply() {
        let mgr = make_readindex_manager();
        let read_id = mgr.register_read(LogIndex::new(10), InodeId::new(1), 1);

        mgr.confirm_heartbeat(read_id, NodeId::new(1)).unwrap();

        let status = mgr.check_status(read_id, LogIndex::new(5));
        assert_eq!(status, ReadStatus::WaitingForApply);

        let status = mgr.check_status(read_id, LogIndex::new(10));
        assert_eq!(status, ReadStatus::Ready);
    }

    #[test]
    fn test_readindex_pending_count() {
        let mgr = make_readindex_manager();

        mgr.register_read(LogIndex::new(10), InodeId::new(1), 3);
        mgr.register_read(LogIndex::new(20), InodeId::new(2), 3);
        mgr.register_read(LogIndex::new(30), InodeId::new(3), 3);
        mgr.register_read(LogIndex::new(40), InodeId::new(4), 3);
        mgr.register_read(LogIndex::new(50), InodeId::new(5), 3);

        assert_eq!(mgr.pending_count(), 5);

        mgr.complete_read(1).unwrap();
        mgr.complete_read(2).unwrap();

        assert_eq!(mgr.pending_count(), 3);
    }

    // ============================================================================
    // Category 5: Follower Read & Path Resolution (5 tests)
    // ============================================================================

    #[test]
    fn test_follower_read_linearizable_goes_to_leader() {
        let mut router = make_follower_read_router();
        router.set_leader(NodeId::new(1), LogIndex::new(100));

        let target = router.route_read(ReadConsistency::Linearizable);

        match target {
            ReadTarget::Leader(id) => assert_eq!(id, NodeId::new(1)),
            _ => panic!("expected Leader"),
        }
    }

    #[test]
    fn test_follower_read_no_leader() {
        let router = make_follower_read_router();

        let target = router.route_read(ReadConsistency::Linearizable);

        assert_eq!(target, ReadTarget::Unavailable);
    }

    #[test]
    fn test_follower_read_staleness_bound() {
        let mut router = FollowerReadRouter::new(FollowerReadConfig {
            max_stale_entries: 10,
            ..Default::default()
        });
        router.set_leader(NodeId::new(1), LogIndex::new(100));
        router.update_follower(NodeId::new(2), LogIndex::new(95), 500);

        assert!(router.is_within_bounds(&NodeId::new(2)));

        router.update_follower(NodeId::new(2), LogIndex::new(80), 500);

        assert!(!router.is_within_bounds(&NodeId::new(2)));
    }

    #[test]
    fn test_path_resolver_parse() {
        let components = PathResolver::parse_path("/a/b/c");
        assert_eq!(components, vec!["a", "b", "c"]);

        let components = PathResolver::parse_path("//");
        assert!(components.is_empty());

        let components = PathResolver::parse_path("");
        assert!(components.is_empty());
    }

    #[test]
    fn test_path_resolver_negative_cache() {
        let resolver = make_path_resolver();

        resolver.cache_negative(InodeId::new(1), "missing");

        assert!(resolver.check_negative(InodeId::new(1), "missing"));

        resolver.invalidate_negative(InodeId::new(1), "missing");

        assert!(!resolver.check_negative(InodeId::new(1), "missing"));
    }
}
