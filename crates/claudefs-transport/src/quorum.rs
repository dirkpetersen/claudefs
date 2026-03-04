//! Quorum voting for distributed consensus operations.
//!
//! Tracks voting outcomes for distributed consensus operations. Used by A2 (Metadata) for
//! Raft-style quorum operations and by A4's fanout infrastructure to determine when enough
//! nodes have agreed. Supports N-of-M voting with configurable thresholds.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Quorum policy determines how many votes are needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuorumPolicy {
    /// Strict majority: floor(N/2) + 1 votes needed.
    Majority,
    /// All N votes needed (unanimity).
    All,
    /// At least `n` votes needed out of total.
    AtLeast(usize),
}

/// A single vote in a quorum round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    /// Voter identifier (opaque 16-byte UUID).
    pub voter_id: [u8; 16],
    /// Whether this vote is in favor (true) or against (false).
    pub approve: bool,
    /// Optional rejection reason (if approve = false).
    pub reason: Option<String>,
}

/// Result of a quorum evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuorumResult {
    /// Quorum not yet reached — still collecting votes.
    Pending,
    /// Quorum achieved — enough approvals.
    Achieved,
    /// Quorum failed — too many rejections, not achievable.
    Failed,
    /// Quorum expired (timeout).
    Expired,
}

/// Configuration for a quorum round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumConfig {
    /// Total number of voters.
    pub total_voters: usize,
    /// Quorum policy.
    pub policy: QuorumPolicy,
    /// Round timeout in milliseconds (default: 5000).
    pub timeout_ms: u64,
}

impl Default for QuorumConfig {
    fn default() -> Self {
        QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        }
    }
}

/// Error type for quorum operations.
#[derive(Debug, Error)]
pub enum QuorumError {
    /// Voter already voted in this round.
    #[error("voter {0:?} already voted")]
    AlreadyVoted([u8; 16]),
    /// Quorum round already concluded.
    #[error("quorum round already concluded with result {0:?}")]
    AlreadyConcluded(QuorumResult),
}

/// A quorum round — collects votes and determines outcome.
pub struct QuorumRound {
    /// Configuration for this round.
    pub config: QuorumConfig,
    votes: HashMap<[u8; 16], Vote>,
    created_at_ms: u64,
    result: QuorumResult,
}

impl QuorumRound {
    /// Create a new quorum round.
    pub fn new(config: QuorumConfig, now_ms: u64) -> Self {
        QuorumRound {
            config,
            votes: HashMap::new(),
            created_at_ms: now_ms,
            result: QuorumResult::Pending,
        }
    }

    /// Record a vote. Returns error if voter already voted or round is concluded.
    pub fn vote(&mut self, v: Vote) -> Result<QuorumResult, QuorumError> {
        if self.result != QuorumResult::Pending {
            return Err(QuorumError::AlreadyConcluded(self.result));
        }

        if self.votes.contains_key(&v.voter_id) {
            return Err(QuorumError::AlreadyVoted(v.voter_id));
        }

        self.votes.insert(v.voter_id, v.clone());

        if v.approve {
            if self.approval_count() >= self.required() {
                self.result = QuorumResult::Achieved;
            }
        } else {
            if !self.achievable() {
                self.result = QuorumResult::Failed;
            }
        }

        Ok(self.result)
    }

    /// Force-check timeout. Updates result to Expired if timed out. Returns new result.
    pub fn check_timeout(&mut self, now_ms: u64) -> QuorumResult {
        if self.result != QuorumResult::Pending {
            return self.result;
        }

        if now_ms >= self.created_at_ms + self.config.timeout_ms {
            self.result = QuorumResult::Expired;
        }

        self.result
    }

    /// Current result.
    pub fn result(&self) -> QuorumResult {
        self.result
    }

    /// Number of votes received so far.
    pub fn vote_count(&self) -> usize {
        self.votes.len()
    }

    /// Number of approvals so far.
    pub fn approval_count(&self) -> usize {
        self.votes.values().filter(|v| v.approve).count()
    }

    /// Number of rejections so far.
    pub fn rejection_count(&self) -> usize {
        self.votes.values().filter(|v| !v.approve).count()
    }

    /// Required approvals based on policy.
    pub fn required(&self) -> usize {
        match self.config.policy {
            QuorumPolicy::Majority => (self.config.total_voters / 2) + 1,
            QuorumPolicy::All => self.config.total_voters,
            QuorumPolicy::AtLeast(n) => n,
        }
    }

    /// Whether quorum is still achievable (remaining votes + approvals >= required).
    pub fn achievable(&self) -> bool {
        let remaining = self.config.total_voters.saturating_sub(self.votes.len());
        let approvals = self.approval_count();
        approvals + remaining >= self.required()
    }

    /// List of voter IDs who have voted.
    pub fn voted_ids(&self) -> Vec<[u8; 16]> {
        self.votes.keys().copied().collect()
    }
}

/// Statistics for quorum operations.
pub struct QuorumStats {
    pub rounds_started: AtomicU64,
    pub rounds_achieved: AtomicU64,
    pub rounds_failed: AtomicU64,
    pub rounds_expired: AtomicU64,
    pub total_votes: AtomicU64,
    pub total_approvals: AtomicU64,
    pub total_rejections: AtomicU64,
}

impl QuorumStats {
    /// Create new quorum statistics.
    pub fn new() -> Self {
        QuorumStats {
            rounds_started: AtomicU64::new(0),
            rounds_achieved: AtomicU64::new(0),
            rounds_failed: AtomicU64::new(0),
            rounds_expired: AtomicU64::new(0),
            total_votes: AtomicU64::new(0),
            total_approvals: AtomicU64::new(0),
            total_rejections: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of current statistics.
    pub fn snapshot(&self, active_rounds: usize) -> QuorumStatsSnapshot {
        QuorumStatsSnapshot {
            rounds_started: self.rounds_started.load(Ordering::Relaxed),
            rounds_achieved: self.rounds_achieved.load(Ordering::Relaxed),
            rounds_failed: self.rounds_failed.load(Ordering::Relaxed),
            rounds_expired: self.rounds_expired.load(Ordering::Relaxed),
            total_votes: self.total_votes.load(Ordering::Relaxed),
            total_approvals: self.total_approvals.load(Ordering::Relaxed),
            total_rejections: self.total_rejections.load(Ordering::Relaxed),
            active_rounds,
        }
    }
}

impl Default for QuorumStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of quorum statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct QuorumStatsSnapshot {
    pub rounds_started: u64,
    pub rounds_achieved: u64,
    pub rounds_failed: u64,
    pub rounds_expired: u64,
    pub total_votes: u64,
    pub total_approvals: u64,
    pub total_rejections: u64,
    pub active_rounds: usize,
}

/// Manager for multiple concurrent quorum rounds.
pub struct QuorumManager {
    next_id: AtomicU64,
    rounds: Mutex<HashMap<u64, QuorumRound>>,
    stats: Arc<QuorumStats>,
}

impl QuorumManager {
    /// Create a new quorum manager.
    pub fn new() -> Self {
        QuorumManager {
            next_id: AtomicU64::new(1),
            rounds: Mutex::new(HashMap::new()),
            stats: Arc::new(QuorumStats::new()),
        }
    }

    /// Start a new quorum round. Returns the round ID.
    pub fn start_round(&self, config: QuorumConfig, now_ms: u64) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let round = QuorumRound::new(config, now_ms);

        self.stats.rounds_started.fetch_add(1, Ordering::Relaxed);

        if let Ok(mut rounds) = self.rounds.lock() {
            rounds.insert(id, round);
        }

        id
    }

    /// Submit a vote for a round. Returns new QuorumResult, or None if round not found.
    pub fn vote(&self, round_id: u64, v: Vote) -> Option<Result<QuorumResult, QuorumError>> {
        let mut rounds = self.rounds.lock().ok()?;
        let round = rounds.get_mut(&round_id)?;

        let prev_result = round.result();
        let result = round.vote(v.clone());

        if let Ok(QuorumResult::Achieved) = result {
            if prev_result != QuorumResult::Achieved {
                self.stats.rounds_achieved.fetch_add(1, Ordering::Relaxed);
            }
        } else if let Ok(QuorumResult::Failed) = result {
            if prev_result != QuorumResult::Failed {
                self.stats.rounds_failed.fetch_add(1, Ordering::Relaxed);
            }
        }

        self.stats.total_votes.fetch_add(1, Ordering::Relaxed);
        if v.approve {
            self.stats.total_approvals.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats.total_rejections.fetch_add(1, Ordering::Relaxed);
        }

        Some(result)
    }

    /// Check timeouts. Returns IDs of rounds that expired.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<u64> {
        let mut expired = Vec::new();

        if let Ok(mut rounds) = self.rounds.lock() {
            for (id, round) in rounds.iter_mut() {
                let prev_result = round.result();
                let new_result = round.check_timeout(now_ms);
                if prev_result != QuorumResult::Expired && new_result == QuorumResult::Expired {
                    expired.push(*id);
                    self.stats.rounds_expired.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        expired
    }

    /// Remove a concluded round. Returns final result or None if not found.
    pub fn complete(&self, round_id: u64) -> Option<QuorumResult> {
        let mut rounds = self.rounds.lock().ok()?;
        let round = rounds.remove(&round_id)?;
        Some(round.result())
    }

    /// Active (pending) round count.
    pub fn active_count(&self) -> usize {
        self.rounds.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// Stats reference.
    pub fn stats(&self) -> Arc<QuorumStats> {
        self.stats.clone()
    }
}

impl Default for QuorumManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_voter_id(seed: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[0] = seed;
        id
    }

    fn make_vote(seed: u8, approve: bool) -> Vote {
        Vote {
            voter_id: make_voter_id(seed),
            approve,
            reason: if approve {
                None
            } else {
                Some("rejected".to_string())
            },
        }
    }

    #[test]
    fn test_quorum_majority_3_of_3() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        let r1 = round.vote(make_vote(1, true)).unwrap();
        assert_eq!(r1, QuorumResult::Pending);

        let r2 = round.vote(make_vote(2, true)).unwrap();
        assert_eq!(r2, QuorumResult::Achieved);
    }

    #[test]
    fn test_quorum_majority_calculation() {
        let config3 = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };
        let round3 = QuorumRound::new(config3, 0);
        assert_eq!(round3.required(), 2);

        let config5 = QuorumConfig {
            total_voters: 5,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };
        let round5 = QuorumRound::new(config5, 0);
        assert_eq!(round5.required(), 3);
    }

    #[test]
    fn test_quorum_all_policy() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::All,
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, true)).unwrap();
        let r2 = round.vote(make_vote(2, true)).unwrap();
        assert_eq!(r2, QuorumResult::Pending);

        let r3 = round.vote(make_vote(3, true)).unwrap();
        assert_eq!(r3, QuorumResult::Achieved);
    }

    #[test]
    fn test_quorum_at_least_policy() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::AtLeast(2),
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, true)).unwrap();
        let r2 = round.vote(make_vote(2, true)).unwrap();
        assert_eq!(r2, QuorumResult::Achieved);
    }

    #[test]
    fn test_quorum_rejection_fails() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, false)).unwrap();
        let r2 = round.vote(make_vote(2, false)).unwrap();
        assert_eq!(r2, QuorumResult::Failed);
    }

    #[test]
    fn test_quorum_already_voted() {
        let config = QuorumConfig::default();
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, true)).unwrap();
        let result = round.vote(make_vote(1, true));
        assert!(matches!(result, Err(QuorumError::AlreadyVoted(_))));
    }

    #[test]
    fn test_quorum_concluded_vote() {
        let config = QuorumConfig {
            total_voters: 2,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, true)).unwrap();
        round.vote(make_vote(2, true)).unwrap();

        let result = round.vote(make_vote(3, true));
        assert!(matches!(
            result,
            Err(QuorumError::AlreadyConcluded(QuorumResult::Achieved))
        ));
    }

    #[test]
    fn test_quorum_timeout() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 100,
        };
        let mut round = QuorumRound::new(config, 0);

        let result = round.check_timeout(200);
        assert_eq!(result, QuorumResult::Expired);
    }

    #[test]
    fn test_quorum_not_expired_yet() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        let result = round.check_timeout(3000);
        assert_eq!(result, QuorumResult::Pending);
    }

    #[test]
    fn test_quorum_achievable_false() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, false)).unwrap();
        round.vote(make_vote(2, false)).unwrap();

        assert!(!round.achievable());
    }

    #[test]
    fn test_quorum_achievable_true() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, true)).unwrap();

        assert!(round.achievable());
    }

    #[test]
    fn test_quorum_all_approve() {
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::All,
            timeout_ms: 5000,
        };
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, true)).unwrap();
        round.vote(make_vote(2, true)).unwrap();
        let r3 = round.vote(make_vote(3, true)).unwrap();

        assert_eq!(r3, QuorumResult::Achieved);
        assert_eq!(round.approval_count(), 3);
        assert_eq!(round.rejection_count(), 0);
    }

    #[test]
    fn test_manager_round_lifecycle() {
        let manager = QuorumManager::new();
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };

        let id = manager.start_round(config, 0);
        assert_eq!(manager.active_count(), 1);

        manager.vote(id, make_vote(1, true)).unwrap().unwrap();
        manager.vote(id, make_vote(2, true)).unwrap().unwrap();

        let result = manager.complete(id);
        assert_eq!(result, Some(QuorumResult::Achieved));
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_manager_check_timeouts() {
        let manager = QuorumManager::new();
        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 100,
        };

        let id = manager.start_round(config, 0);

        let expired = manager.check_timeouts(200);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], id);
    }

    #[test]
    fn test_manager_active_count() {
        let manager = QuorumManager::new();
        assert_eq!(manager.active_count(), 0);

        let id1 = manager.start_round(QuorumConfig::default(), 0);
        assert_eq!(manager.active_count(), 1);

        let id2 = manager.start_round(QuorumConfig::default(), 0);
        assert_eq!(manager.active_count(), 2);

        manager.complete(id1);
        assert_eq!(manager.active_count(), 1);

        manager.complete(id2);
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_stats_snapshot() {
        let manager = QuorumManager::new();
        let stats = manager.stats();

        let config = QuorumConfig {
            total_voters: 3,
            policy: QuorumPolicy::Majority,
            timeout_ms: 5000,
        };

        let id1 = manager.start_round(config.clone(), 0);
        let id2 = manager.start_round(config.clone(), 0);
        let id3 = manager.start_round(config.clone(), 0);

        manager.vote(id1, make_vote(1, true)).unwrap().unwrap();
        manager.vote(id1, make_vote(2, true)).unwrap().unwrap();
        manager.complete(id1);

        manager.vote(id2, make_vote(1, false)).unwrap().unwrap();
        manager.vote(id2, make_vote(2, false)).unwrap().unwrap();
        manager.complete(id2);

        let snapshot = stats.snapshot(manager.active_count());
        assert_eq!(snapshot.rounds_started, 3);
        assert_eq!(snapshot.rounds_achieved, 1);
        assert_eq!(snapshot.rounds_failed, 1);
        assert_eq!(snapshot.total_votes, 4);
        assert_eq!(snapshot.total_approvals, 2);
        assert_eq!(snapshot.total_rejections, 2);
    }

    #[test]
    fn test_quorum_config_default() {
        let config = QuorumConfig::default();
        assert_eq!(config.total_voters, 3);
        assert_eq!(config.policy, QuorumPolicy::Majority);
        assert_eq!(config.timeout_ms, 5000);
    }

    #[test]
    fn test_manager_vote_nonexistent() {
        let manager = QuorumManager::new();
        let result = manager.vote(999, make_vote(1, true));
        assert!(result.is_none());
    }

    #[test]
    fn test_manager_complete_nonexistent() {
        let manager = QuorumManager::new();
        let result = manager.complete(999);
        assert!(result.is_none());
    }

    #[test]
    fn test_quorum_voted_ids() {
        let config = QuorumConfig::default();
        let mut round = QuorumRound::new(config, 0);

        round.vote(make_vote(1, true)).unwrap();
        round.vote(make_vote(2, true)).unwrap();

        let ids = round.voted_ids();
        assert_eq!(ids.len(), 2);
    }
}
