//! Speculative request hedging module for reducing tail latency.
//!
//! When a request takes longer than expected, a duplicate "hedge" request is sent
//! to an alternate node. The first response wins.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use thiserror::Error;

/// Errors that can occur in the hedge module.
#[derive(Error, Debug)]
pub enum HedgeError {
    /// Request was not found in the tracker.
    #[error("Request {request_id} not found in tracker")]
    RequestNotFound {
        /// The request ID that was not found.
        request_id: u64,
    },
}

/// Result type alias for hedge operations.
pub type Result<T> = std::result::Result<T, HedgeError>;

/// Configuration for speculative request hedging.
#[derive(Debug, Clone)]
pub struct HedgeConfig {
    /// Delay in milliseconds before a hedge request may be sent.
    pub hedge_delay_ms: u64,
    /// Maximum extra load percentage allowed for hedging.
    pub max_extra_load_pct: u8,
    /// Whether hedging is enabled.
    pub enabled: bool,
    /// Whether to exclude write requests from hedging.
    pub exclude_writes: bool,
}

impl HedgeConfig {
    /// Creates a new HedgeConfig with the specified values.
    pub fn new(
        hedge_delay_ms: u64,
        max_extra_load_pct: u8,
        enabled: bool,
        exclude_writes: bool,
    ) -> Self {
        Self {
            hedge_delay_ms,
            max_extra_load_pct,
            enabled,
            exclude_writes,
        }
    }
}

impl Default for HedgeConfig {
    fn default() -> Self {
        Self {
            hedge_delay_ms: 50,
            max_extra_load_pct: 5,
            enabled: true,
            exclude_writes: true,
        }
    }
}

/// Statistics about hedging behavior.
#[derive(Debug, Clone, Default)]
pub struct HedgeStats {
    /// Total number of requests processed.
    pub total_requests: u64,
    /// Total number of hedge requests sent.
    pub total_hedges: u64,
    /// Total number of hedge requests that won.
    pub total_hedge_wins: u64,
    /// Ratio of hedges to total requests.
    pub hedge_rate: f64,
    /// Ratio of hedge wins to total hedges.
    pub hedge_win_rate: f64,
    /// Whether hedging is currently enabled.
    pub enabled: bool,
}

/// Policy for determining when to send hedge requests.
pub struct HedgePolicy {
    config: HedgeConfig,
    total_requests: AtomicU64,
    total_hedges: AtomicU64,
    total_hedge_wins: AtomicU64,
}

impl HedgePolicy {
    /// Creates a new HedgePolicy with the given configuration.
    pub fn new(config: HedgeConfig) -> Self {
        Self {
            config,
            total_requests: AtomicU64::new(0),
            total_hedges: AtomicU64::new(0),
            total_hedge_wins: AtomicU64::new(0),
        }
    }

    /// Returns true if a hedge request should be sent.
    pub fn should_hedge(&self, elapsed_ms: u64, is_write: bool) -> bool {
        if !self.config.enabled {
            return false;
        }

        if self.config.exclude_writes && is_write {
            return false;
        }

        if elapsed_ms <= self.config.hedge_delay_ms {
            return false;
        }

        let current_rate = self.hedge_rate();
        let max_rate = self.config.max_extra_load_pct as f64 / 100.0;
        if current_rate >= max_rate {
            return false;
        }

        true
    }

    /// Records that a request was made.
    pub fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Records that a hedge request was sent.
    pub fn record_hedge(&self) {
        self.total_hedges.fetch_add(1, Ordering::Relaxed);
    }

    /// Records that a hedge request won (returned first).
    pub fn record_hedge_won(&self) {
        self.total_hedge_wins.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the hedge rate (hedges / requests).
    pub fn hedge_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        let hedges = self.total_hedges.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            hedges as f64 / total as f64
        }
    }

    /// Returns the hedge win rate (wins / hedges).
    pub fn hedge_win_rate(&self) -> f64 {
        let hedges = self.total_hedges.load(Ordering::Relaxed);
        let wins = self.total_hedge_wins.load(Ordering::Relaxed);
        if hedges == 0 {
            0.0
        } else {
            wins as f64 / hedges as f64
        }
    }

    /// Returns a snapshot of hedge statistics.
    pub fn stats(&self) -> HedgeStats {
        HedgeStats {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_hedges: self.total_hedges.load(Ordering::Relaxed),
            total_hedge_wins: self.total_hedge_wins.load(Ordering::Relaxed),
            hedge_rate: self.hedge_rate(),
            hedge_win_rate: self.hedge_win_rate(),
            enabled: self.config.enabled,
        }
    }

    /// Resets all statistics to zero.
    pub fn reset(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.total_hedges.store(0, Ordering::Relaxed);
        self.total_hedge_wins.store(0, Ordering::Relaxed);
    }
}

/// Tracker for managing in-flight requests and hedge timing.
pub struct HedgeTracker {
    config: HedgeConfig,
    requests: Arc<Mutex<HashMap<u64, Instant>>>,
    total_requests: AtomicU64,
    total_hedges: AtomicU64,
    total_hedge_wins: AtomicU64,
}

impl HedgeTracker {
    /// Creates a new HedgeTracker with the given configuration.
    pub fn new(config: HedgeConfig) -> Self {
        Self {
            config,
            requests: Arc::new(Mutex::new(HashMap::new())),
            total_requests: AtomicU64::new(0),
            total_hedges: AtomicU64::new(0),
            total_hedge_wins: AtomicU64::new(0),
        }
    }

    /// Records the start of a request.
    pub fn start_request(&self, request_id: u64) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        let mut requests = self.requests.lock().unwrap();
        requests.insert(request_id, Instant::now());
    }

    /// Checks if a hedge request should be sent for this request.
    pub fn check_hedge(&self, request_id: u64) -> bool {
        if !self.config.enabled {
            return false;
        }

        let start_time = {
            let requests = self.requests.lock().unwrap();
            requests.get(&request_id).copied()
        };

        let Some(start) = start_time else {
            return false;
        };

        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed <= self.config.hedge_delay_ms {
            return false;
        }

        let current_rate = {
            let total = self.total_requests.load(Ordering::Relaxed);
            let hedges = self.total_hedges.load(Ordering::Relaxed);
            if total == 0 {
                0.0
            } else {
                hedges as f64 / total as f64
            }
        };

        let max_rate = self.config.max_extra_load_pct as f64 / 100.0;
        if current_rate >= max_rate {
            return false;
        }

        self.total_hedges.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Completes a request, removing it from tracking.
    pub fn complete_request(&self, request_id: u64, was_hedge_winner: bool) {
        let mut requests = self.requests.lock().unwrap();
        requests.remove(&request_id);

        if was_hedge_winner {
            self.total_hedge_wins.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Returns the number of currently active tracked requests.
    pub fn active_count(&self) -> usize {
        let requests = self.requests.lock().unwrap();
        requests.len()
    }

    /// Returns a snapshot of hedge statistics.
    pub fn stats(&self) -> HedgeStats {
        HedgeStats {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_hedges: self.total_hedges.load(Ordering::Relaxed),
            total_hedge_wins: self.total_hedge_wins.load(Ordering::Relaxed),
            hedge_rate: {
                let total = self.total_requests.load(Ordering::Relaxed);
                let hedges = self.total_hedges.load(Ordering::Relaxed);
                if total == 0 {
                    0.0
                } else {
                    hedges as f64 / total as f64
                }
            },
            hedge_win_rate: {
                let hedges = self.total_hedges.load(Ordering::Relaxed);
                let wins = self.total_hedge_wins.load(Ordering::Relaxed);
                if hedges == 0 {
                    0.0
                } else {
                    wins as f64 / hedges as f64
                }
            },
            enabled: self.config.enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hedge_config_default() {
        let config = HedgeConfig::default();
        assert_eq!(config.hedge_delay_ms, 50);
        assert_eq!(config.max_extra_load_pct, 5);
        assert!(config.enabled);
        assert!(config.exclude_writes);
    }

    #[test]
    fn test_hedge_policy_new() {
        let config = HedgeConfig::default();
        let policy = HedgePolicy::new(config);
        let stats = policy.stats();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_hedges, 0);
        assert_eq!(stats.total_hedge_wins, 0);
    }

    #[test]
    fn test_should_not_hedge_when_disabled() {
        let mut config = HedgeConfig::default();
        config.enabled = false;
        let policy = HedgePolicy::new(config);

        let result = policy.should_hedge(100, false);
        assert!(!result);
    }

    #[test]
    fn test_should_not_hedge_when_under_delay() {
        let config = HedgeConfig::default();
        let policy = HedgePolicy::new(config);

        let result = policy.should_hedge(30, false);
        assert!(!result);

        let result2 = policy.should_hedge(50, false);
        assert!(!result2);
    }

    #[test]
    fn test_should_hedge_when_delay_exceeded() {
        let config = HedgeConfig::default();
        let policy = HedgePolicy::new(config);
        policy.record_request();

        let result = policy.should_hedge(51, false);
        assert!(result);
    }

    #[test]
    fn test_should_not_hedge_writes() {
        let config = HedgeConfig::default();
        let policy = HedgePolicy::new(config);

        let result = policy.should_hedge(100, true);
        assert!(!result);
    }

    #[test]
    fn test_should_hedge_writes_when_allowed() {
        let mut config = HedgeConfig::default();
        config.exclude_writes = false;
        let policy = HedgePolicy::new(config);
        policy.record_request();

        let result = policy.should_hedge(100, true);
        assert!(result);
    }

    #[test]
    fn test_hedge_rate_tracking() {
        let config = HedgeConfig::default();
        let policy = HedgePolicy::new(config);

        for _ in 0..100 {
            policy.record_request();
        }
        for _ in 0..5 {
            policy.record_hedge();
        }

        let rate = policy.hedge_rate();
        assert!((rate - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_hedge_win_rate_tracking() {
        let config = HedgeConfig::default();
        let policy = HedgePolicy::new(config);

        for _ in 0..20 {
            policy.record_hedge();
        }
        for _ in 0..4 {
            policy.record_hedge_won();
        }

        let rate = policy.hedge_win_rate();
        assert!((rate - 0.2).abs() < 0.001);
    }

    #[test]
    fn test_stats_snapshot() {
        let config = HedgeConfig::default();
        let policy = HedgePolicy::new(config);

        for _ in 0..50 {
            policy.record_request();
        }
        for _ in 0..10 {
            policy.record_hedge();
        }
        for _ in 0..2 {
            policy.record_hedge_won();
        }

        let stats = policy.stats();
        assert_eq!(stats.total_requests, 50);
        assert_eq!(stats.total_hedges, 10);
        assert_eq!(stats.total_hedge_wins, 2);
        assert!((stats.hedge_rate - 0.2).abs() < 0.001);
        assert!((stats.hedge_win_rate - 0.2).abs() < 0.001);
        assert!(stats.enabled);
    }

    #[test]
    fn test_reset_clears_stats() {
        let config = HedgeConfig::default();
        let policy = HedgePolicy::new(config);

        for _ in 0..100 {
            policy.record_request();
        }
        for _ in 0..50 {
            policy.record_hedge();
        }

        policy.reset();

        let stats = policy.stats();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_hedges, 0);
        assert_eq!(stats.total_hedge_wins, 0);
    }

    #[test]
    fn test_extra_load_budget() {
        let mut config = HedgeConfig::default();
        config.max_extra_load_pct = 5;
        let policy = HedgePolicy::new(config);

        for _ in 0..100 {
            policy.record_request();
        }
        for _ in 0..5 {
            policy.record_hedge();
        }

        let result = policy.should_hedge(100, false);
        assert!(!result);

        let mut config2 = HedgeConfig::default();
        config2.max_extra_load_pct = 10;
        let policy2 = HedgePolicy::new(config2);

        for _ in 0..100 {
            policy2.record_request();
        }
        for _ in 0..5 {
            policy2.record_hedge();
        }

        let result2 = policy2.should_hedge(100, false);
        assert!(result2);
    }

    #[test]
    fn test_tracker_new() {
        let config = HedgeConfig::default();
        let tracker = HedgeTracker::new(config);
        assert_eq!(tracker.active_count(), 0);
    }

    #[test]
    fn test_tracker_start_request() {
        let config = HedgeConfig::default();
        let tracker = HedgeTracker::new(config);

        tracker.start_request(1);
        assert_eq!(tracker.active_count(), 1);

        tracker.start_request(2);
        tracker.start_request(3);
        assert_eq!(tracker.active_count(), 3);
    }

    #[test]
    fn test_tracker_complete_request() {
        let config = HedgeConfig::default();
        let tracker = HedgeTracker::new(config);

        tracker.start_request(1);
        tracker.start_request(2);
        assert_eq!(tracker.active_count(), 2);

        tracker.complete_request(1, false);
        assert_eq!(tracker.active_count(), 1);

        tracker.complete_request(2, true);
        assert_eq!(tracker.active_count(), 0);
    }

    #[test]
    fn test_tracker_check_hedge_before_delay() {
        let config = HedgeConfig::default();
        let tracker = HedgeTracker::new(config);

        tracker.start_request(1);

        let result = tracker.check_hedge(1);
        assert!(!result);
    }

    #[test]
    fn test_tracker_check_hedge_after_delay() {
        let config = HedgeConfig::default();
        let tracker = HedgeTracker::new(config);

        tracker.start_request(1);

        std::thread::sleep(std::time::Duration::from_millis(60));

        let result = tracker.check_hedge(1);
        assert!(result);
    }

    #[test]
    fn test_tracker_stats() {
        let mut config = HedgeConfig::default();
        config.max_extra_load_pct = 50;
        let tracker = HedgeTracker::new(config);

        tracker.start_request(1);
        std::thread::sleep(std::time::Duration::from_millis(60));
        tracker.start_request(2);
        std::thread::sleep(std::time::Duration::from_millis(60));
        tracker.start_request(3);

        let _ = tracker.check_hedge(1);
        let _ = tracker.check_hedge(2);

        tracker.complete_request(1, true);
        tracker.complete_request(2, false);

        let stats = tracker.stats();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.total_hedges, 2);
        assert_eq!(stats.total_hedge_wins, 1);
    }
}
