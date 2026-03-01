//! Distributed System Tests
//!
//! Tests that simulate multi-node distributed scenarios using in-memory test infrastructure.

use crate::chaos::{FaultInjector, FaultType, NetworkTopology};
use crate::jepsen::{JepsenChecker, JepsenHistory, Nemesis, RegisterModel};
use crate::linearizability::{History, Operation};

pub struct TwoPhaseCommitSim {
    pub nodes: Vec<u32>,
    prepared: std::collections::HashSet<u32>,
    committed: std::collections::HashSet<u32>,
    aborted: bool,
}

impl TwoPhaseCommitSim {
    pub fn new(node_count: u32) -> Self {
        Self {
            nodes: (0..node_count).collect(),
            prepared: std::collections::HashSet::new(),
            committed: std::collections::HashSet::new(),
            aborted: false,
        }
    }

    pub fn prepare_all(&mut self) -> bool {
        if self.aborted {
            return false;
        }
        for node in &self.nodes {
            self.prepared.insert(*node);
        }
        true
    }

    pub fn commit_all(&mut self) -> bool {
        if self.prepared.len() == self.nodes.len() as usize {
            for node in &self.nodes {
                self.committed.insert(*node);
            }
            true
        } else {
            false
        }
    }

    pub fn abort_all(&mut self) -> bool {
        self.aborted = true;
        self.prepared.clear();
        true
    }

    pub fn prepare_with_failures(&self, failing_nodes: &[u32]) -> bool {
        let failing: std::collections::HashSet<_> = failing_nodes.iter().cloned().collect();
        !self.aborted && self.nodes.iter().all(|n| !failing.contains(n))
    }
}

pub struct QuorumVote {
    pub total_nodes: u32,
    pub votes_cast: u32,
    pub votes_yes: u32,
}

impl QuorumVote {
    pub fn new(total: u32) -> Self {
        Self {
            total_nodes: total,
            votes_cast: 0,
            votes_yes: 0,
        }
    }

    pub fn cast_yes(&mut self) {
        self.votes_cast += 1;
        self.votes_yes += 1;
    }

    pub fn cast_no(&mut self) {
        self.votes_cast += 1;
    }

    pub fn has_quorum(&self) -> bool {
        let majority = (self.total_nodes / 2) + 1;
        self.votes_yes >= majority
    }

    pub fn has_strong_quorum(&self) -> bool {
        let two_thirds = (2 * self.total_nodes + 2) / 3;
        self.votes_yes >= two_thirds
    }
}

pub struct RaftElectionSim {
    pub node_count: u32,
    pub term: u64,
    votes: std::collections::HashMap<u32, u32>,
}

impl RaftElectionSim {
    pub fn new(node_count: u32) -> Self {
        Self {
            node_count,
            term: 1,
            votes: std::collections::HashMap::new(),
        }
    }

    pub fn start_election(&mut self, candidate: u32) {
        self.votes.clear();
        self.votes.insert(candidate, 1);
    }

    pub fn vote_for(&mut self, _voter: u32, candidate: u32) {
        let current = self.votes.entry(candidate).or_insert(0);
        *current += 1;
    }

    pub fn has_winner(&self) -> Option<u32> {
        let majority = (self.node_count / 2) + 1;
        for (candidate, votes) in &self.votes {
            if *votes >= majority {
                return Some(*candidate);
            }
        }
        None
    }

    pub fn advance_term(&mut self) {
        self.term += 1;
        self.votes.clear();
    }
}

pub struct PartitionScenario {
    pub topology: NetworkTopology,
    pub fault_injector: FaultInjector,
}

impl PartitionScenario {
    pub fn new(node_count: u32) -> Self {
        let mut topology = NetworkTopology::new();
        for i in 0..node_count {
            topology.add_node(i);
        }
        Self {
            topology,
            fault_injector: FaultInjector::new(),
        }
    }

    pub fn partition_network(&mut self, group_a: Vec<u32>, group_b: Vec<u32>) {
        for a in &group_a {
            for b in &group_b {
                self.topology.add_partition(*a, *b);
                self.fault_injector
                    .inject(FaultType::NetworkPartition { from: *a, to: *b });
            }
        }
    }

    pub fn heal_partition(&mut self) {
        self.fault_injector = FaultInjector::new();
        self.topology = NetworkTopology::new();
    }

    pub fn can_reach(&self, from: u32, to: u32) -> bool {
        self.topology.can_reach(from, to)
    }

    pub fn nodes_in_majority_partition(&self, all_nodes: &[u32]) -> Vec<u32> {
        let mut partition_sizes: std::collections::HashMap<Vec<u32>, usize> =
            std::collections::HashMap::new();

        for node in all_nodes {
            let mut group = vec![*node];
            for other in all_nodes {
                if other != node && self.topology.can_reach(*node, *other) {
                    group.push(*other);
                }
            }
            group.sort();
            *partition_sizes.entry(group).or_insert(0) += 1;
        }

        partition_sizes
            .into_iter()
            .max_by_key(|(_, size)| *size)
            .map(|(group, _)| group)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod two_phase_commit_tests {
    use super::*;

    #[test]
    fn test_two_phase_commit_new() {
        let sim = TwoPhaseCommitSim::new(3);
        assert_eq!(sim.nodes.len(), 3);
    }

    #[test]
    fn test_prepare_all_succeeds() {
        let mut sim = TwoPhaseCommitSim::new(3);
        assert!(sim.prepare_all());
    }

    #[test]
    fn test_commit_all_with_all_prepared() {
        let mut sim = TwoPhaseCommitSim::new(3);
        sim.prepare_all();
        assert!(sim.commit_all());
    }

    #[test]
    fn test_abort_all() {
        let mut sim = TwoPhaseCommitSim::new(3);
        assert!(sim.abort_all());
        // After abort, prepare should return false
        let result = sim.prepare_all();
        assert!(!result);
    }

    #[test]
    fn test_prepare_with_failures_returns_true_when_no_failures() {
        let sim = TwoPhaseCommitSim::new(3);
        assert!(sim.prepare_with_failures(&[]));
    }

    #[test]
    fn test_prepare_with_failures_returns_false_with_failures() {
        let sim = TwoPhaseCommitSim::new(3);
        assert!(!sim.prepare_with_failures(&[1, 2]));
    }
}

#[cfg(test)]
mod quorum_vote_tests {
    use super::*;

    #[test]
    fn test_quorum_vote_new() {
        let vote = QuorumVote::new(5);
        assert_eq!(vote.total_nodes, 5);
        assert_eq!(vote.votes_cast, 0);
    }

    #[test]
    fn test_majority_quorum_requires_over_half() {
        let mut vote = QuorumVote::new(5);

        vote.cast_yes();
        vote.cast_yes();
        vote.cast_no();

        assert!(!vote.has_quorum());

        vote.cast_yes();
        vote.cast_yes();

        assert!(vote.has_quorum());
    }

    #[test]
    fn test_quorum_with_3_of_5() {
        let mut vote = QuorumVote::new(5);

        vote.cast_yes();
        vote.cast_yes();
        vote.cast_yes();

        assert!(vote.has_quorum());
    }

    #[test]
    fn test_not_quorum_with_2_of_5() {
        let mut vote = QuorumVote::new(5);

        vote.cast_yes();
        vote.cast_yes();

        assert!(!vote.has_quorum());
    }

    #[test]
    fn test_strong_quorum_requires_2_3() {
        // For 5 nodes, 2/3 strong quorum = 4
        // So 3/5 should NOT be strong quorum
        let mut vote = QuorumVote::new(5);

        vote.cast_yes();
        vote.cast_yes();
        vote.cast_yes();

        assert!(!vote.has_strong_quorum());

        // 4/5 IS strong quorum
        let mut vote2 = QuorumVote::new(5);
        vote2.cast_yes();
        vote2.cast_yes();
        vote2.cast_yes();
        vote2.cast_yes();

        assert!(vote2.has_strong_quorum());
    }

    #[test]
    fn test_strong_quorum_3_of_5() {
        let mut vote = QuorumVote::new(5);
        vote.cast_yes();
        vote.cast_yes();
        vote.cast_yes();

        // 3/5 is not strong quorum (needs 4)
        assert!(!vote.has_strong_quorum());
    }

    #[test]
    fn test_strong_quorum_2_of_3() {
        let mut vote = QuorumVote::new(3);
        vote.cast_yes();
        vote.cast_yes();

        // 2/3 is strong quorum for n=3 (needs 2)
        assert!(vote.has_strong_quorum());
    }

    #[test]
    fn test_cast_yes_increments_both() {
        let mut vote = QuorumVote::new(5);
        vote.cast_yes();

        assert_eq!(vote.votes_cast, 1);
        assert_eq!(vote.votes_yes, 1);
    }

    #[test]
    fn test_cast_no_only_increments_votes_cast() {
        let mut vote = QuorumVote::new(5);
        vote.cast_no();

        assert_eq!(vote.votes_cast, 1);
        assert_eq!(vote.votes_yes, 0);
    }
}

#[cfg(test)]
mod raft_election_tests {
    use super::*;

    #[test]
    fn test_raft_election_new() {
        let sim = RaftElectionSim::new(5);
        assert_eq!(sim.node_count, 5);
        assert_eq!(sim.term, 1);
    }

    #[test]
    fn test_start_election() {
        let mut sim = RaftElectionSim::new(5);
        sim.start_election(1);

        assert_eq!(sim.votes.get(&1), Some(&1));
    }

    #[test]
    fn test_vote_for() {
        let mut sim = RaftElectionSim::new(5);
        sim.start_election(1);
        sim.vote_for(2, 1);
        sim.vote_for(3, 1);

        assert_eq!(sim.votes.get(&1), Some(&3));
    }

    #[test]
    fn test_has_winner_with_majority() {
        let mut sim = RaftElectionSim::new(5);
        sim.start_election(1);
        sim.vote_for(2, 1);
        sim.vote_for(3, 1);

        assert_eq!(sim.has_winner(), Some(1));
    }

    #[test]
    fn test_has_winner_split_vote() {
        let mut sim = RaftElectionSim::new(5);
        // Candidate 1 starts with 1 vote (from start_election), then gets 1 more = 2 total
        sim.start_election(1);
        sim.vote_for(2, 1);

        // Candidate 2 starts fresh with 2 votes
        sim.vote_for(3, 2);
        sim.vote_for(4, 2);

        // Neither has majority (3), so no winner
        assert!(sim.has_winner().is_none());
    }

    #[test]
    fn test_advance_term_increments() {
        let mut sim = RaftElectionSim::new(5);
        assert_eq!(sim.term, 1);

        sim.advance_term();
        assert_eq!(sim.term, 2);

        sim.advance_term();
        assert_eq!(sim.term, 3);
    }

    #[test]
    fn test_advance_term_clears_votes() {
        let mut sim = RaftElectionSim::new(5);
        sim.start_election(1);
        sim.vote_for(2, 1);

        sim.advance_term();
        assert!(sim.votes.is_empty());
    }
}

#[cfg(test)]
mod partition_scenario_tests {
    use super::*;

    #[test]
    fn test_partition_scenario_new() {
        let scenario = PartitionScenario::new(5);
        // Initially all nodes can reach each other (no partitions yet)
        assert!(scenario.can_reach(0, 1));
    }

    #[test]
    fn test_partition_network() {
        let mut scenario = PartitionScenario::new(5);
        scenario.partition_network(vec![0, 1], vec![2, 3, 4]);

        assert!(!scenario.can_reach(0, 2));
    }

    #[test]
    fn test_heal_partition_restores_connectivity() {
        let mut scenario = PartitionScenario::new(5);
        scenario.partition_network(vec![0, 1], vec![2, 3, 4]);
        scenario.heal_partition();

        assert!(scenario.can_reach(0, 2));
    }

    #[test]
    fn test_nodes_in_majority_partition() {
        let mut scenario = PartitionScenario::new(5);
        scenario.partition_network(vec![0, 1, 2], vec![3, 4]);

        let all_nodes = vec![0, 1, 2, 3, 4];
        let majority = scenario.nodes_in_majority_partition(&all_nodes);

        assert!(majority.len() >= 3);
    }

    #[test]
    fn test_nodes_in_same_partition_can_reach() {
        let mut scenario = PartitionScenario::new(4);
        scenario.partition_network(vec![0, 1], vec![2, 3]);

        assert!(scenario.can_reach(0, 1));
        assert!(scenario.can_reach(2, 3));
    }

    #[test]
    fn test_nodes_in_different_partitions_cannot_reach() {
        let mut scenario = PartitionScenario::new(4);
        scenario.partition_network(vec![0, 1], vec![2, 3]);

        assert!(!scenario.can_reach(0, 2));
        assert!(!scenario.can_reach(1, 3));
    }

    #[test]
    fn test_heal_restores_all_connectivity() {
        let mut scenario = PartitionScenario::new(3);
        scenario.partition_network(vec![0], vec![1, 2]);
        scenario.heal_partition();

        assert!(scenario.can_reach(0, 1));
        assert!(scenario.can_reach(0, 2));
        assert!(scenario.can_reach(1, 2));
    }

    #[test]
    fn test_single_node_always_in_majority() {
        let scenario = PartitionScenario::new(1);
        let all_nodes = vec![0];
        let majority = scenario.nodes_in_majority_partition(&all_nodes);

        assert!(majority.contains(&0));
    }
}
