//! Quorum-based write coordination for multi-site writes.
//!
//! This module provides types and utilities for managing quorum-based write operations
//! in a distributed, multi-site replication setting. It handles vote collection,
//! quorum formation, split-brain detection, and timeout handling.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Quorum type defining how many votes are required for a write to succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuorumType {
    /// Majority quorum: more than 50% of sites must accept.
    Majority,
    /// All sites must accept for the write to succeed.
    All,
    /// Custom quorum: exactly `n` sites must accept.
    Custom(usize),
}

/// Configuration for write quorum operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteQuorumConfig {
    /// The type of quorum required.
    pub quorum_type: QuorumType,
    /// Timeout in milliseconds for waiting for votes.
    pub timeout_ms: u64,
    /// Total number of sites in the cluster.
    pub site_count: usize,
}

/// Write request from a client targeting a specific shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    /// ID of the originating site.
    pub site_id: u32,
    /// ID of the target shard.
    pub shard_id: u32,
    /// Sequence number for ordering.
    pub seq: u64,
    /// Write payload data.
    pub data: Vec<u8>,
    /// Client identifier.
    pub client_id: String,
    /// Timestamp of the write request.
    pub timestamp: u64,
}

/// Response from a write operation after quorum is reached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResponse {
    /// List of site IDs that acknowledged the write.
    pub quorum_acks: Vec<u32>,
    /// Timestamp assigned to the committed write.
    pub write_ts: u64,
    /// Site that committed the write.
    pub committing_site: u32,
}

/// Result of a vote from a remote site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteVoteResult {
    /// Site accepted the write.
    Accepted,
    /// Site rejected the write.
    Rejected,
    /// Site timed out before voting.
    Timeout,
}

/// Matcher for tracking votes and determining quorum satisfaction.
#[derive(Debug, Clone)]
pub struct QuorumMatcher {
    votes: HashMap<u32, WriteVoteResult>,
    config: WriteQuorumConfig,
}

impl WriteQuorumConfig {
    /// Creates a new write quorum configuration.
    pub fn new(quorum_type: QuorumType, timeout_ms: u64, site_count: usize) -> Self {
        WriteQuorumConfig {
            quorum_type,
            timeout_ms,
            site_count,
        }
    }

    /// Validates the configuration.
    pub fn is_valid(&self) -> bool {
        match self.quorum_type {
            QuorumType::Majority | QuorumType::All => self.site_count >= 1,
            QuorumType::Custom(n) => n > 0 && n <= self.site_count,
        }
    }
}

impl QuorumMatcher {
    /// Creates a new quorum matcher with the given configuration.
    pub fn new(config: WriteQuorumConfig) -> Self {
        QuorumMatcher {
            votes: HashMap::new(),
            config,
        }
    }

    /// Adds a vote from a site. Returns true.
    pub fn add_vote(&mut self, site_id: u32, result: WriteVoteResult) -> bool {
        self.votes.insert(site_id, result);
        true
    }

    /// Checks if quorum is satisfied based on current votes.
    pub fn is_satisfied(&self) -> bool {
        let accepted = self
            .votes
            .values()
            .filter(|v| **v == WriteVoteResult::Accepted)
            .count();

        match self.config.quorum_type {
            QuorumType::Majority => accepted > self.config.site_count / 2,
            QuorumType::All => accepted == self.config.site_count,
            QuorumType::Custom(n) => accepted >= n,
        }
    }

    /// Returns list of sites that haven't voted yet.
    pub fn pending_sites(&self) -> Vec<u32> {
        (1..=self.config.site_count as u32)
            .filter(|site_id| !self.votes.contains_key(site_id))
            .collect()
    }

    /// Detects split-brain scenarios where conflicting votes prevent consensus.
    pub fn detect_split_brain(&self) -> Option<String> {
        let accepted = self
            .votes
            .values()
            .filter(|v| **v == WriteVoteResult::Accepted)
            .count();
        let rejected = self
            .votes
            .values()
            .filter(|v| **v == WriteVoteResult::Rejected)
            .count();

        if accepted == 0 || rejected == 0 {
            return None;
        }

        let accepted_satisfies = match self.config.quorum_type {
            QuorumType::Majority => accepted > self.config.site_count / 2,
            QuorumType::All => accepted == self.config.site_count,
            QuorumType::Custom(n) => accepted >= n,
        };

        let rejected_satisfies = match self.config.quorum_type {
            QuorumType::Majority => rejected > self.config.site_count / 2,
            QuorumType::All => rejected == self.config.site_count,
            QuorumType::Custom(n) => rejected >= n,
        };

        if !accepted_satisfies && !rejected_satisfies && accepted > 0 && rejected > 0 {
            Some(format!(
                "Split-brain: {} accepted, {} rejected",
                accepted, rejected
            ))
        } else {
            None
        }
    }

    /// Checks if the given elapsed time exceeds the configured timeout.
    pub fn timed_out(&self, elapsed_ms: u64) -> bool {
        elapsed_ms >= self.config.timeout_ms
    }

    /// Returns reference to the votes map.
    pub fn get_votes(&self) -> &HashMap<u32, WriteVoteResult> {
        &self.votes
    }

    /// Resets all votes.
    pub fn reset(&mut self) {
        self.votes.clear();
    }

    /// Returns total number of votes received.
    pub fn vote_count(&self) -> usize {
        self.votes.len()
    }

    /// Returns number of accepted votes.
    pub fn accepted_count(&self) -> usize {
        self.votes
            .values()
            .filter(|v| **v == WriteVoteResult::Accepted)
            .count()
    }

    /// Returns number of rejected votes.
    pub fn rejected_count(&self) -> usize {
        self.votes
            .values()
            .filter(|v| **v == WriteVoteResult::Rejected)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quorum_formation_majority() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 3);
        let mut matcher = QuorumMatcher::new(config);

        assert!(!matcher.is_satisfied());
        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Accepted);
        assert!(matcher.is_satisfied());
    }

    #[test]
    fn test_quorum_formation_all() {
        let config = WriteQuorumConfig::new(QuorumType::All, 5000, 3);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Accepted);
        assert!(!matcher.is_satisfied());

        matcher.add_vote(3, WriteVoteResult::Accepted);
        assert!(matcher.is_satisfied());
    }

    #[test]
    fn test_quorum_formation_custom() {
        let config = WriteQuorumConfig::new(QuorumType::Custom(2), 5000, 4);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        assert!(!matcher.is_satisfied());

        matcher.add_vote(2, WriteVoteResult::Accepted);
        assert!(matcher.is_satisfied());
    }

    #[test]
    fn test_satisfaction_checks() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 3);
        let mut matcher = QuorumMatcher::new(config);

        assert!(!matcher.is_satisfied());
        matcher.add_vote(1, WriteVoteResult::Accepted);
        assert!(!matcher.is_satisfied());
        matcher.add_vote(2, WriteVoteResult::Accepted);
        assert!(matcher.is_satisfied());
    }

    #[test]
    fn test_timeout_handling() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 3);
        let matcher = QuorumMatcher::new(config);

        assert!(!matcher.timed_out(1000));
        assert!(!matcher.timed_out(4999));
        assert!(matcher.timed_out(5000));
        assert!(matcher.timed_out(10000));
    }

    #[test]
    fn test_split_brain_detection_2way() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 4);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Accepted);
        matcher.add_vote(3, WriteVoteResult::Rejected);
        matcher.add_vote(4, WriteVoteResult::Rejected);

        let split = matcher.detect_split_brain();
        assert!(split.is_some());
        assert!(split.unwrap().contains("Split-brain"));
    }

    #[test]
    fn test_split_brain_no_split_all_accepted() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 3);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Accepted);
        matcher.add_vote(3, WriteVoteResult::Accepted);

        assert!(matcher.detect_split_brain().is_none());
    }

    #[test]
    fn test_vote_idempotency() {
        let config = WriteQuorumConfig::new(QuorumType::All, 5000, 2);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(1, WriteVoteResult::Rejected);

        assert_eq!(matcher.vote_count(), 1);
        assert!(!matcher.is_satisfied());
    }

    #[test]
    fn test_dynamic_site_removal() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 5);
        let mut matcher = QuorumMatcher::new(config);

        let pending = matcher.pending_sites();
        assert_eq!(pending, vec![1, 2, 3, 4, 5]);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(3, WriteVoteResult::Accepted);

        let pending = matcher.pending_sites();
        assert_eq!(pending, vec![2, 4, 5]);
    }

    #[test]
    fn test_config_validation() {
        let config1 = WriteQuorumConfig::new(QuorumType::Majority, 5000, 0);
        assert!(!config1.is_valid());

        let config2 = WriteQuorumConfig::new(QuorumType::Majority, 5000, 1);
        assert!(config2.is_valid());

        let config3 = WriteQuorumConfig::new(QuorumType::Custom(0), 5000, 5);
        assert!(!config3.is_valid());

        let config4 = WriteQuorumConfig::new(QuorumType::Custom(6), 5000, 5);
        assert!(!config4.is_valid());

        let config5 = WriteQuorumConfig::new(QuorumType::Custom(3), 5000, 5);
        assert!(config5.is_valid());
    }

    #[test]
    fn test_empty_votes() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 3);
        let matcher = QuorumMatcher::new(config);

        assert_eq!(matcher.vote_count(), 0);
        assert_eq!(matcher.accepted_count(), 0);
        assert_eq!(matcher.rejected_count(), 0);
        assert!(!matcher.is_satisfied());
    }

    #[test]
    fn test_all_rejected() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 3);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Rejected);
        matcher.add_vote(2, WriteVoteResult::Rejected);
        matcher.add_vote(3, WriteVoteResult::Rejected);

        assert!(!matcher.is_satisfied());
    }

    #[test]
    fn test_single_site() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 1);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        assert!(matcher.is_satisfied());
    }

    #[test]
    fn test_large_site_count() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 100);
        let mut matcher = QuorumMatcher::new(config);

        for i in 1..=51 {
            matcher.add_vote(i, WriteVoteResult::Accepted);
        }

        assert!(matcher.is_satisfied());
        assert_eq!(matcher.accepted_count(), 51);
    }

    #[test]
    fn test_concurrent_vote_updates() {
        let config = WriteQuorumConfig::new(QuorumType::Custom(3), 5000, 5);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Accepted);
        assert!(!matcher.is_satisfied());

        matcher.add_vote(3, WriteVoteResult::Accepted);
        assert!(matcher.is_satisfied());

        matcher.add_vote(4, WriteVoteResult::Rejected);
        matcher.add_vote(5, WriteVoteResult::Accepted);
        assert!(matcher.is_satisfied());
    }

    #[test]
    fn test_partial_satisfaction() {
        let config = WriteQuorumConfig::new(QuorumType::All, 5000, 4);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Accepted);
        matcher.add_vote(3, WriteVoteResult::Timeout);

        assert!(!matcher.is_satisfied());
    }

    #[test]
    fn test_vote_counting_accuracy() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 5);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Accepted);
        matcher.add_vote(3, WriteVoteResult::Rejected);
        matcher.add_vote(4, WriteVoteResult::Rejected);
        matcher.add_vote(5, WriteVoteResult::Timeout);

        assert_eq!(matcher.vote_count(), 5);
        assert_eq!(matcher.accepted_count(), 2);
        assert_eq!(matcher.rejected_count(), 2);
    }

    #[test]
    fn test_accepted_count() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 3);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Rejected);
        matcher.add_vote(3, WriteVoteResult::Accepted);

        assert_eq!(matcher.accepted_count(), 2);
    }

    #[test]
    fn test_rejected_count() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 3);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Rejected);
        matcher.add_vote(2, WriteVoteResult::Rejected);
        matcher.add_vote(3, WriteVoteResult::Accepted);

        assert_eq!(matcher.rejected_count(), 2);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let config = WriteQuorumConfig::new(QuorumType::Custom(2), 5000, 4);
        let encoded = serde_json::to_string(&config).unwrap();
        let decoded: WriteQuorumConfig = serde_json::from_str(&encoded).unwrap();
        assert_eq!(config.quorum_type, decoded.quorum_type);
        assert_eq!(config.timeout_ms, decoded.timeout_ms);
        assert_eq!(config.site_count, decoded.site_count);

        let vote = WriteVoteResult::Accepted;
        let encoded = serde_json::to_string(&vote).unwrap();
        let decoded: WriteVoteResult = serde_json::from_str(&encoded).unwrap();
        assert_eq!(vote, decoded);
    }

    #[test]
    fn test_reset_clears_votes() {
        let config = WriteQuorumConfig::new(QuorumType::All, 5000, 3);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(1, WriteVoteResult::Accepted);
        matcher.add_vote(2, WriteVoteResult::Accepted);
        matcher.add_vote(3, WriteVoteResult::Accepted);

        assert_eq!(matcher.vote_count(), 3);

        matcher.reset();

        assert_eq!(matcher.vote_count(), 0);
        assert!(!matcher.is_satisfied());
    }

    #[test]
    fn test_pending_sites_after_votes() {
        let config = WriteQuorumConfig::new(QuorumType::Majority, 5000, 4);
        let mut matcher = QuorumMatcher::new(config);

        matcher.add_vote(2, WriteVoteResult::Accepted);
        matcher.add_vote(4, WriteVoteResult::Accepted);

        let pending = matcher.pending_sites();
        assert!(pending.contains(&1));
        assert!(!pending.contains(&2));
        assert!(pending.contains(&3));
        assert!(!pending.contains(&4));
    }

    #[test]
    fn test_config_clone() {
        let config = WriteQuorumConfig::new(QuorumType::Custom(2), 5000, 4);
        let cloned = config.clone();
        assert_eq!(config.quorum_type, cloned.quorum_type);
        assert_eq!(config.timeout_ms, cloned.timeout_ms);
        assert_eq!(config.site_count, cloned.site_count);
    }

    #[test]
    fn test_custom_quorum_edge_cases() {
        let config1 = WriteQuorumConfig::new(QuorumType::Custom(1), 5000, 1);
        let mut matcher1 = QuorumMatcher::new(config1);
        matcher1.add_vote(1, WriteVoteResult::Accepted);
        assert!(matcher1.is_satisfied());

        let config2 = WriteQuorumConfig::new(QuorumType::Custom(5), 5000, 5);
        let mut matcher2 = QuorumMatcher::new(config2);
        for i in 1..=5 {
            matcher2.add_vote(i, WriteVoteResult::Accepted);
        }
        assert!(matcher2.is_satisfied());

        let config3 = WriteQuorumConfig::new(QuorumType::Custom(3), 5000, 3);
        let mut matcher3 = QuorumMatcher::new(config3);
        matcher3.add_vote(1, WriteVoteResult::Accepted);
        matcher3.add_vote(2, WriteVoteResult::Accepted);
        assert!(!matcher3.is_satisfied());
    }
}
