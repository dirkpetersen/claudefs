# Task: Write meta_consensus_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-meta` crate focusing on Raft consensus safety, membership transitions, lease management, ReadIndex protocol, and follower read routing.

## File location
`crates/claudefs-security/src/meta_consensus_security_tests.rs`

## Module structure
```rust
//! Consensus security tests for claudefs-meta: Raft, membership, leases, ReadIndex, follower reads.
//!
//! Part of A10 Phase 9: Meta consensus security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs and module exploration)

```rust
use claudefs_meta::consensus::{RaftConfig, RaftNode, RaftMessage, RaftState};
use claudefs_meta::membership::{MemberInfo, MembershipEvent, MembershipManager, NodeState};
use claudefs_meta::locking::{LockManager, LockType};
use claudefs_meta::lease::{LeaseManager, LeaseType};
use claudefs_meta::readindex::{ReadIndexManager, PendingRead, ReadStatus};
use claudefs_meta::follower_read::{FollowerReadConfig, FollowerReadRouter, ReadConsistency, ReadTarget};
use claudefs_meta::pathres::{PathResolver, PathCacheEntry, NegativeCacheEntry};
use claudefs_meta::fsck::{FsckConfig, FsckFinding, FsckIssue, FsckReport, FsckSeverity, FsckRepairAction, suggest_repair};
use claudefs_meta::{InodeId, NodeId, ShardId, MetaOp, LogIndex, Term};
use claudefs_meta::types::{FileType, Timestamp};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating

- `meta_security_tests.rs` (25 tests): input validation, distributed locking basics, metadata service ops, path cache
- `meta_deep_security_tests.rs` (25 tests): transactions, locking, tenants, quotas, shard routing, journal

DO NOT duplicate these. Focus on consensus, membership, leases, ReadIndex, and follower read routing.

## Test categories (25 tests total, 5 per category)

### Category 1: Raft Consensus Safety (5 tests)

1. **test_raft_initial_state_follower** — Create RaftNode with 3-node config. Verify initial state is Follower. Verify current_term is 0 and voted_for is None.

2. **test_raft_election_increments_term** — Create RaftNode. Call start_election(). Verify current_term increased by 1 and state is Candidate. Verify voted_for is self (node_id).

3. **test_raft_propose_as_follower_fails** — Create RaftNode (in Follower state). Try to propose a MetaOp. Verify it returns error (only leaders can propose).

4. **test_raft_term_monotonic** — Create RaftNode. Start election (term becomes 1). Start another election (term becomes 2). Verify term never decreases.

5. **test_raft_leadership_transfer** — Create RaftNode. Make it a leader (start election, win). Call transfer_leadership to another node. Verify is_transferring() returns true. Call cancel_transfer(). Verify is_transferring() returns false.

### Category 2: Membership Management (5 tests)

6. **test_membership_join_leave** — Create MembershipManager. Join node 1 and node 2. Verify member_count() == 2 and alive_count() == 2. Leave node 1. Verify member_count() == 1.

7. **test_membership_state_transitions** — Create MembershipManager. Join node. Suspect node. Verify state is Suspect. Confirm alive. Verify state is Alive. Mark dead. Verify state is Dead.

8. **test_membership_events_emitted** — Create MembershipManager. Join node. Suspect node. Mark dead. Call drain_events(). Verify events include NodeJoined, NodeSuspected, NodeDead in order.

9. **test_membership_duplicate_join** — Create MembershipManager. Join node 1. Try join node 1 again. Document whether duplicate join is rejected or idempotent.

10. **test_membership_suspect_unknown_node** — Create MembershipManager. Try to suspect a node that never joined. Verify error returned.

### Category 3: Lease Management (5 tests)

11. **test_lease_write_exclusivity** — Create LeaseManager. Grant write lease on inode 1 to client A. Try grant another write lease on inode 1 to client B. Verify error (write lease is exclusive).

12. **test_lease_read_coexistence** — Create LeaseManager. Grant read lease on inode 1 to client A. Grant read lease on inode 1 to client B. Verify both succeed. Verify leases_on(inode 1) returns 2 leases.

13. **test_lease_revoke_client_cleanup** — Create LeaseManager. Grant 3 leases to client A across 3 inodes. Call revoke_client(A). Verify returns 3. Verify active_lease_count() == 0.

14. **test_lease_renew_expired_fails** — Create LeaseManager with short duration. Grant lease. Wait or manually expire it. Try renew. Verify error (lease expired).

15. **test_lease_id_uniqueness** — Create LeaseManager. Grant 100 leases. Verify all lease IDs are unique (no collisions).

### Category 4: ReadIndex Protocol (5 tests)

16. **test_readindex_quorum_calculation** — Create ReadIndexManager. Register read with cluster_size=5. Confirm heartbeat from 2 nodes. Verify has_quorum returns false (need 3). Confirm 3rd. Verify has_quorum returns true.

17. **test_readindex_duplicate_confirmation** — Create ReadIndexManager. Register read with cluster_size=3. Confirm heartbeat from same node twice. Verify only counted once (still needs 1 more for quorum).

18. **test_readindex_timeout_cleanup** — Create ReadIndexManager with timeout_secs=1. Register read. Call cleanup_timed_out after sufficient delay. Verify timed-out read ID is returned.

19. **test_readindex_status_waiting_for_apply** — Create ReadIndexManager. Register read with read_index=10 and cluster_size=1. Confirm heartbeat. Check status with last_applied=5. Verify WaitingForApply. Check with last_applied=10. Verify Ready.

20. **test_readindex_pending_count** — Create ReadIndexManager. Register 5 reads. Verify pending_count() == 5. Complete 2. Verify pending_count() == 3.

### Category 5: Follower Read & Path Resolution (5 tests)

21. **test_follower_read_linearizable_goes_to_leader** — Create FollowerReadRouter. Set leader to node 1. Call route_read(Linearizable). Verify ReadTarget::Leader(node 1).

22. **test_follower_read_no_leader** — Create FollowerReadRouter. Don't set any leader. Call route_read(Linearizable). Verify ReadTarget::Unavailable.

23. **test_follower_read_staleness_bound** — Create FollowerReadRouter with max_stale_entries=10. Set leader commit_index=100. Update follower with last_applied=95. Verify is_within_bounds returns true. Update follower to last_applied=80. Verify is_within_bounds returns false (20 entries behind > max 10).

24. **test_path_resolver_parse** — Call PathResolver::parse_path("/a/b/c"). Verify returns ["a", "b", "c"]. Parse "//". Parse empty string. Document edge case behavior.

25. **test_path_resolver_negative_cache** — Create PathResolver. Cache negative entry for parent=1, name="missing". Verify check_negative returns true. Call invalidate_negative. Verify check_negative returns false.

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark security findings with `// FINDING-META-CONS-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code — all tests are synchronous
- For RaftConfig, use RaftConfig { node_id: NodeId::new(1), peers: vec![NodeId::new(2), NodeId::new(3)], election_timeout_min_ms: 150, election_timeout_max_ms: 300, heartbeat_interval_ms: 50 }
- For MetaOp, use MetaOp::CreateInode { attr: claudefs_meta::InodeAttr::new_file(InodeId::new(0), 0, 0, 0o644, 1) }

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
