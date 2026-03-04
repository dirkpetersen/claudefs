//! Parallel request fanout for distributing operations across multiple cluster nodes.
//!
//! Tracks parallel in-flight requests to multiple cluster nodes (fanout). Used by the write path
//! to dispatch to 2 journal replicas simultaneously, and by the EC stripe distribution to dispatch
//! to 6 data nodes. Also used for parallel read fan-out across EC stripe nodes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Unique identifier for a fanout operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FanoutId(pub u64);

/// A single target in a fanout operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutTarget {
    /// Target node identifier (opaque 16-byte UUID).
    pub node_id: [u8; 16],
    /// Human-readable label for debugging.
    pub label: String,
}

/// Result from a single fanout target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FanoutTargetResult {
    /// Target responded successfully.
    Success,
    /// Target failed with an error message.
    Failed(String),
    /// Target timed out.
    TimedOut,
}

/// Configuration for fanout operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutConfig {
    /// Minimum number of successes required for the fanout to succeed (quorum).
    /// For journal replication: 2. For EC reads: 4 (4+2, need 4 data shards).
    pub required_successes: usize,
    /// Total number of targets.
    pub total_targets: usize,
    /// Timeout for the entire fanout operation in milliseconds.
    pub timeout_ms: u64,
}

impl Default for FanoutConfig {
    fn default() -> Self {
        FanoutConfig {
            required_successes: 2,
            total_targets: 2,
            timeout_ms: 5000,
        }
    }
}

/// State of a fanout operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanoutState {
    /// Waiting for responses.
    InFlight,
    /// Enough successes received — quorum met.
    Succeeded,
    /// Too many failures — cannot meet quorum.
    Failed,
    /// Timed out before quorum.
    TimedOut,
}

/// A tracked fanout operation.
pub struct FanoutOp {
    /// Unique identifier for this operation.
    pub id: FanoutId,
    /// Configuration for this operation.
    pub config: FanoutConfig,
    /// Target nodes for this operation.
    pub targets: Vec<FanoutTarget>,
    results: HashMap<[u8; 16], FanoutTargetResult>,
    created_at_ms: u64,
    state: FanoutState,
}

impl FanoutOp {
    /// Create a new fanout operation.
    pub fn new(
        id: FanoutId,
        config: FanoutConfig,
        targets: Vec<FanoutTarget>,
        now_ms: u64,
    ) -> Self {
        FanoutOp {
            id,
            config,
            targets,
            results: HashMap::new(),
            created_at_ms: now_ms,
            state: FanoutState::InFlight,
        }
    }

    /// Record a result from a target node. Returns the updated FanoutState.
    pub fn record_result(&mut self, node_id: [u8; 16], result: FanoutTargetResult) -> FanoutState {
        self.results.insert(node_id, result);

        if self.state != FanoutState::InFlight {
            return self.state;
        }

        if self.quorum_met() {
            self.state = FanoutState::Succeeded;
        } else if !self.quorum_possible() {
            self.state = FanoutState::Failed;
        }

        self.state
    }

    /// Check if the operation has timed out. Updates state if so. Returns new state.
    pub fn check_timeout(&mut self, now_ms: u64) -> FanoutState {
        if self.state != FanoutState::InFlight {
            return self.state;
        }

        if now_ms >= self.created_at_ms + self.config.timeout_ms {
            self.state = FanoutState::TimedOut;
        }

        self.state
    }

    /// Current state.
    pub fn state(&self) -> FanoutState {
        self.state
    }

    /// Number of successes so far.
    pub fn success_count(&self) -> usize {
        self.results
            .values()
            .filter(|r| matches!(r, FanoutTargetResult::Success))
            .count()
    }

    /// Number of failures so far.
    pub fn failure_count(&self) -> usize {
        self.results
            .values()
            .filter(|r| {
                matches!(
                    r,
                    FanoutTargetResult::Failed(_) | FanoutTargetResult::TimedOut
                )
            })
            .count()
    }

    /// Number of pending (no response yet) targets.
    pub fn pending_count(&self) -> usize {
        self.config.total_targets.saturating_sub(self.results.len())
    }

    /// Whether quorum has been met (success_count >= required_successes).
    pub fn quorum_met(&self) -> bool {
        self.success_count() >= self.config.required_successes
    }

    /// Whether quorum is still achievable (pending + success >= required_successes).
    pub fn quorum_possible(&self) -> bool {
        let pending = self.pending_count();
        let successes = self.success_count();
        pending + successes >= self.config.required_successes
    }
}

/// Statistics for fanout operations.
pub struct FanoutStats {
    pub ops_started: AtomicU64,
    pub ops_succeeded: AtomicU64,
    pub ops_failed: AtomicU64,
    pub ops_timed_out: AtomicU64,
    pub total_targets_sent: AtomicU64,
    pub total_target_successes: AtomicU64,
    pub total_target_failures: AtomicU64,
}

impl FanoutStats {
    /// Create new fanout statistics.
    pub fn new() -> Self {
        FanoutStats {
            ops_started: AtomicU64::new(0),
            ops_succeeded: AtomicU64::new(0),
            ops_failed: AtomicU64::new(0),
            ops_timed_out: AtomicU64::new(0),
            total_targets_sent: AtomicU64::new(0),
            total_target_successes: AtomicU64::new(0),
            total_target_failures: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of current statistics.
    pub fn snapshot(&self, in_flight: usize) -> FanoutStatsSnapshot {
        FanoutStatsSnapshot {
            ops_started: self.ops_started.load(Ordering::Relaxed),
            ops_succeeded: self.ops_succeeded.load(Ordering::Relaxed),
            ops_failed: self.ops_failed.load(Ordering::Relaxed),
            ops_timed_out: self.ops_timed_out.load(Ordering::Relaxed),
            total_targets_sent: self.total_targets_sent.load(Ordering::Relaxed),
            total_target_successes: self.total_target_successes.load(Ordering::Relaxed),
            total_target_failures: self.total_target_failures.load(Ordering::Relaxed),
            in_flight,
        }
    }
}

impl Default for FanoutStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of fanout statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct FanoutStatsSnapshot {
    pub ops_started: u64,
    pub ops_succeeded: u64,
    pub ops_failed: u64,
    pub ops_timed_out: u64,
    pub total_targets_sent: u64,
    pub total_target_successes: u64,
    pub total_target_failures: u64,
    pub in_flight: usize,
}

/// Manager for multiple concurrent fanout operations.
pub struct FanoutManager {
    next_id: AtomicU64,
    ops: Mutex<HashMap<FanoutId, FanoutOp>>,
    stats: Arc<FanoutStats>,
}

impl FanoutManager {
    /// Create a new fanout manager.
    pub fn new() -> Self {
        FanoutManager {
            next_id: AtomicU64::new(1),
            ops: Mutex::new(HashMap::new()),
            stats: Arc::new(FanoutStats::new()),
        }
    }

    /// Start a new fanout operation. Returns the FanoutId.
    pub fn start(&self, config: FanoutConfig, targets: Vec<FanoutTarget>, now_ms: u64) -> FanoutId {
        let id = FanoutId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let op = FanoutOp::new(id, config.clone(), targets.clone(), now_ms);

        self.stats.ops_started.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_targets_sent
            .fetch_add(targets.len() as u64, Ordering::Relaxed);

        if let Ok(mut ops) = self.ops.lock() {
            ops.insert(id, op);
        }

        id
    }

    /// Record a result for a specific fanout. Returns the new state, or None if fanout not found.
    pub fn record_result(
        &self,
        id: FanoutId,
        node_id: [u8; 16],
        result: FanoutTargetResult,
        _now_ms: u64,
    ) -> Option<FanoutState> {
        let mut ops = self.ops.lock().ok()?;
        let op = ops.get_mut(&id)?;

        let state = op.record_result(node_id, result.clone());

        match result {
            FanoutTargetResult::Success => {
                self.stats
                    .total_target_successes
                    .fetch_add(1, Ordering::Relaxed);
            }
            FanoutTargetResult::Failed(_) | FanoutTargetResult::TimedOut => {
                self.stats
                    .total_target_failures
                    .fetch_add(1, Ordering::Relaxed);
            }
        }

        Some(state)
    }

    /// Check timeouts for all in-flight ops. Returns ids that transitioned to TimedOut.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<FanoutId> {
        let mut timed_out = Vec::new();

        if let Ok(mut ops) = self.ops.lock() {
            for (id, op) in ops.iter_mut() {
                let prev_state = op.state();
                let new_state = op.check_timeout(now_ms);
                if prev_state != FanoutState::TimedOut && new_state == FanoutState::TimedOut {
                    timed_out.push(*id);
                    self.stats.ops_timed_out.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        timed_out
    }

    /// Complete (remove) a fanout operation. Returns the final FanoutState, or None if not found.
    pub fn complete(&self, id: FanoutId) -> Option<FanoutState> {
        let mut ops = self.ops.lock().ok()?;
        let op = ops.remove(&id)?;

        match op.state() {
            FanoutState::Succeeded => {
                self.stats.ops_succeeded.fetch_add(1, Ordering::Relaxed);
            }
            FanoutState::Failed => {
                self.stats.ops_failed.fetch_add(1, Ordering::Relaxed);
            }
            FanoutState::TimedOut => {
                self.stats.ops_timed_out.fetch_add(1, Ordering::Relaxed);
            }
            FanoutState::InFlight => {}
        }

        Some(op.state())
    }

    /// Get state of a specific fanout.
    pub fn state(&self, id: FanoutId) -> Option<FanoutState> {
        let ops = self.ops.lock().ok()?;
        ops.get(&id).map(|op| op.state())
    }

    /// Number of in-flight fanout operations.
    pub fn in_flight_count(&self) -> usize {
        self.ops.lock().map(|ops| ops.len()).unwrap_or(0)
    }

    /// Stats reference.
    pub fn stats(&self) -> Arc<FanoutStats> {
        self.stats.clone()
    }
}

impl Default for FanoutManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_id(seed: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[0] = seed;
        id
    }

    fn make_target(seed: u8) -> FanoutTarget {
        FanoutTarget {
            node_id: make_node_id(seed),
            label: format!("node-{}", seed),
        }
    }

    #[test]
    fn test_fanout_basic_success() {
        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 2,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        let state1 = op.record_result(make_node_id(1), FanoutTargetResult::Success);
        assert_eq!(state1, FanoutState::InFlight);

        let state2 = op.record_result(make_node_id(2), FanoutTargetResult::Success);
        assert_eq!(state2, FanoutState::Succeeded);
        assert!(op.quorum_met());
    }

    #[test]
    fn test_fanout_quorum_met_early() {
        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 3,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2), make_target(3)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        let state1 = op.record_result(make_node_id(1), FanoutTargetResult::Success);
        assert_eq!(state1, FanoutState::InFlight);

        let state2 = op.record_result(make_node_id(2), FanoutTargetResult::Success);
        assert_eq!(state2, FanoutState::Succeeded);
    }

    #[test]
    fn test_fanout_failure_blocks_quorum() {
        let config = FanoutConfig {
            required_successes: 1,
            total_targets: 2,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        let state = op.record_result(
            make_node_id(1),
            FanoutTargetResult::Failed("error".to_string()),
        );
        assert_eq!(state, FanoutState::InFlight);
        assert!(op.quorum_possible());
        assert_eq!(op.pending_count(), 1);
    }

    #[test]
    fn test_fanout_quorum_impossible() {
        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 2,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        let state = op.record_result(
            make_node_id(1),
            FanoutTargetResult::Failed("error".to_string()),
        );
        assert_eq!(state, FanoutState::Failed);
        assert!(!op.quorum_possible());
    }

    #[test]
    fn test_fanout_timeout() {
        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 2,
            timeout_ms: 1000,
        };
        let targets = vec![make_target(1), make_target(2)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        let state = op.check_timeout(1500);
        assert_eq!(state, FanoutState::TimedOut);
    }

    #[test]
    fn test_fanout_timeout_not_expired() {
        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 2,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        let state = op.check_timeout(3000);
        assert_eq!(state, FanoutState::InFlight);
    }

    #[test]
    fn test_fanout_success_count() {
        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 3,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2), make_target(3)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        assert_eq!(op.success_count(), 0);
        assert_eq!(op.failure_count(), 0);
        assert_eq!(op.pending_count(), 3);

        op.record_result(make_node_id(1), FanoutTargetResult::Success);
        assert_eq!(op.success_count(), 1);
        assert_eq!(op.failure_count(), 0);
        assert_eq!(op.pending_count(), 2);

        op.record_result(
            make_node_id(2),
            FanoutTargetResult::Failed("err".to_string()),
        );
        assert_eq!(op.success_count(), 1);
        assert_eq!(op.failure_count(), 1);
        assert_eq!(op.pending_count(), 1);

        op.record_result(make_node_id(3), FanoutTargetResult::TimedOut);
        assert_eq!(op.success_count(), 1);
        assert_eq!(op.failure_count(), 2);
        assert_eq!(op.pending_count(), 0);
    }

    #[test]
    fn test_fanout_all_fail() {
        let config = FanoutConfig {
            required_successes: 1,
            total_targets: 2,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        op.record_result(
            make_node_id(1),
            FanoutTargetResult::Failed("err1".to_string()),
        );
        let state = op.record_result(
            make_node_id(2),
            FanoutTargetResult::Failed("err2".to_string()),
        );
        assert_eq!(state, FanoutState::Failed);
    }

    #[test]
    fn test_fanout_single_target() {
        let config = FanoutConfig {
            required_successes: 1,
            total_targets: 1,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        let state = op.record_result(make_node_id(1), FanoutTargetResult::Success);
        assert_eq!(state, FanoutState::Succeeded);
    }

    #[test]
    fn test_fanout_ec_quorum() {
        let config = FanoutConfig {
            required_successes: 4,
            total_targets: 6,
            timeout_ms: 5000,
        };
        let targets = vec![
            make_target(1),
            make_target(2),
            make_target(3),
            make_target(4),
            make_target(5),
            make_target(6),
        ];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        op.record_result(make_node_id(1), FanoutTargetResult::Success);
        op.record_result(make_node_id(2), FanoutTargetResult::Success);
        op.record_result(make_node_id(3), FanoutTargetResult::Success);
        let state = op.record_result(make_node_id(4), FanoutTargetResult::Success);

        assert_eq!(state, FanoutState::Succeeded);
    }

    #[test]
    fn test_manager_start_and_complete() {
        let manager = FanoutManager::new();
        let config = FanoutConfig::default();
        let targets = vec![make_target(1), make_target(2)];

        let id = manager.start(config, targets, 0);
        assert_eq!(manager.in_flight_count(), 1);

        manager.record_result(id, make_node_id(1), FanoutTargetResult::Success, 0);
        manager.record_result(id, make_node_id(2), FanoutTargetResult::Success, 0);

        let final_state = manager.complete(id);
        assert_eq!(final_state, Some(FanoutState::Succeeded));
        assert_eq!(manager.in_flight_count(), 0);
    }

    #[test]
    fn test_manager_record_result() {
        let manager = FanoutManager::new();
        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 3,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2), make_target(3)];

        let id = manager.start(config, targets, 0);

        let state1 = manager.record_result(id, make_node_id(1), FanoutTargetResult::Success, 0);
        assert_eq!(state1, Some(FanoutState::InFlight));

        let state2 = manager.record_result(id, make_node_id(2), FanoutTargetResult::Success, 0);
        assert_eq!(state2, Some(FanoutState::Succeeded));
    }

    #[test]
    fn test_manager_check_timeouts() {
        let manager = FanoutManager::new();
        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 2,
            timeout_ms: 100,
        };
        let targets = vec![make_target(1), make_target(2)];

        let id = manager.start(config, targets, 0);

        let timed_out = manager.check_timeouts(200);
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], id);
        assert_eq!(manager.state(id), Some(FanoutState::TimedOut));
    }

    #[test]
    fn test_manager_in_flight_count() {
        let manager = FanoutManager::new();
        assert_eq!(manager.in_flight_count(), 0);

        let id1 = manager.start(
            FanoutConfig::default(),
            vec![make_target(1), make_target(2)],
            0,
        );
        assert_eq!(manager.in_flight_count(), 1);

        let id2 = manager.start(
            FanoutConfig::default(),
            vec![make_target(3), make_target(4)],
            0,
        );
        assert_eq!(manager.in_flight_count(), 2);

        manager.complete(id1);
        assert_eq!(manager.in_flight_count(), 1);

        manager.complete(id2);
        assert_eq!(manager.in_flight_count(), 0);
    }

    #[test]
    fn test_stats_counts() {
        let manager = FanoutManager::new();
        let stats = manager.stats();

        let config = FanoutConfig {
            required_successes: 2,
            total_targets: 2,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2)];

        let id1 = manager.start(config.clone(), targets.clone(), 0);
        let id2 = manager.start(config.clone(), targets.clone(), 0);
        let id3 = manager.start(config.clone(), targets.clone(), 0);

        manager.record_result(id1, make_node_id(1), FanoutTargetResult::Success, 0);
        manager.record_result(id1, make_node_id(2), FanoutTargetResult::Success, 0);
        manager.complete(id1);

        manager.record_result(
            id2,
            make_node_id(1),
            FanoutTargetResult::Failed("err".to_string()),
            0,
        );
        manager.record_result(
            id2,
            make_node_id(2),
            FanoutTargetResult::Failed("err".to_string()),
            0,
        );
        manager.complete(id2);

        let snapshot = stats.snapshot(manager.in_flight_count());
        assert_eq!(snapshot.ops_started, 3);
        assert_eq!(snapshot.ops_succeeded, 1);
        assert_eq!(snapshot.ops_failed, 1);
        assert_eq!(snapshot.total_targets_sent, 6);
        assert_eq!(snapshot.total_target_successes, 2);
        assert_eq!(snapshot.total_target_failures, 2);
    }

    #[test]
    fn test_fanout_config_default() {
        let config = FanoutConfig::default();
        assert_eq!(config.required_successes, 2);
        assert_eq!(config.total_targets, 2);
        assert_eq!(config.timeout_ms, 5000);
    }

    #[test]
    fn test_fanout_duplicate_result() {
        let config = FanoutConfig {
            required_successes: 1,
            total_targets: 2,
            timeout_ms: 5000,
        };
        let targets = vec![make_target(1), make_target(2)];
        let mut op = FanoutOp::new(FanoutId(1), config, targets, 0);

        let state1 = op.record_result(make_node_id(1), FanoutTargetResult::Success);
        assert_eq!(state1, FanoutState::Succeeded);

        let state2 = op.record_result(
            make_node_id(1),
            FanoutTargetResult::Failed("err".to_string()),
        );
        assert_eq!(state2, FanoutState::Succeeded);
        assert_eq!(op.success_count(), 0);
    }

    #[test]
    fn test_manager_complete_nonexistent() {
        let manager = FanoutManager::new();
        let result = manager.complete(FanoutId(999));
        assert!(result.is_none());
    }

    #[test]
    fn test_manager_state_nonexistent() {
        let manager = FanoutManager::new();
        let result = manager.state(FanoutId(999));
        assert!(result.is_none());
    }
}
