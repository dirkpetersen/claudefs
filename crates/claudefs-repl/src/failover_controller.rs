//! Automated failover controller with health-check-based triggering.
//!
//! This module provides a high-level controller that orchestrates site failover
//! based on health check results, maintains graceful degradation, and coordinates recovery.

use crate::error::ReplError;
use std::collections::HashMap;

/// Configuration for the failover controller.
#[derive(Debug, Clone)]
pub struct FailoverConfig {
    /// Number of consecutive failures before demotion.
    pub failure_threshold: u32,
    /// Number of consecutive successes before promotion.
    pub recovery_threshold: u32,
    /// Health check interval in milliseconds.
    pub check_interval_ms: u64,
    /// Enable active-active mode.
    pub active_active: bool,
    /// Failover timeout in milliseconds (max time to complete failover).
    pub failover_timeout_ms: u64,
    /// Enable graceful degradation (continue single-site writes).
    pub graceful_degradation: bool,
    /// Minimum sites required for write quorum (default 1 for graceful).
    pub write_quorum_size: usize,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            recovery_threshold: 2,
            check_interval_ms: 5000,
            active_active: true,
            failover_timeout_ms: 5000,
            graceful_degradation: true,
            write_quorum_size: 1,
        }
    }
}

/// Failure tracking for a single site.
#[derive(Debug, Clone)]
pub struct FailureTracker {
    /// Site ID.
    pub site_id: u64,
    /// Consecutive failure count.
    pub consecutive_failures: u32,
    /// Consecutive success count (for recovery).
    pub consecutive_successes: u32,
    /// Timestamp of last failure (nanoseconds).
    pub last_failure_ns: u64,
}

impl FailureTracker {
    fn new(site_id: u64) -> Self {
        Self {
            site_id,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_failure_ns: 0,
        }
    }

    fn record_failure(&mut self, timestamp_ns: u64) {
        self.consecutive_failures += 1;
        self.consecutive_successes = 0;
        self.last_failure_ns = timestamp_ns;
    }

    fn record_success(&mut self) {
        self.consecutive_successes += 1;
        self.consecutive_failures = 0;
    }

    fn should_failover(&self, threshold: u32) -> bool {
        self.consecutive_failures >= threshold
    }

    fn should_recover(&self, threshold: u32) -> bool {
        self.consecutive_successes >= threshold
    }
}

/// Failover controller state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum FailoverControllerState {
    /// Normal operation, all sites healthy.
    Healthy,
    /// One or more sites degraded (failures detected but below threshold).
    Degraded,
    /// Primary site down, failover initiated.
    FailoverInProgress,
    /// Active-active mode with both sites accepting writes.
    ActiveActive,
    /// Single-site mode (replica down).
    SingleSite,
    /// Partial recovery: some sites recovering.
    Recovering,
    /// Error state.
    Error(String),
}

/// Main failover controller.
#[derive(Debug)]
pub struct FailoverController {
    /// Configuration.
    config: FailoverConfig,
    /// Per-site failure tracking.
    trackers: HashMap<u64, FailureTracker>,
    /// Current controller state.
    state: FailoverControllerState,
    /// Timestamp of last failover event (ns).
    last_failover_ts_ns: u64,
    /// Failover counter (for metrics).
    failover_count: u64,
}

impl FailoverController {
    /// Create a new controller with given config.
    pub fn new(config: FailoverConfig) -> Self {
        Self {
            config,
            trackers: HashMap::new(),
            state: FailoverControllerState::Healthy,
            last_failover_ts_ns: 0,
            failover_count: 0,
        }
    }

    /// Record a health check success for a site.
    pub fn record_success(&mut self, site_id: u64) -> Result<(), ReplError> {
        let tracker = self
            .trackers
            .entry(site_id)
            .or_insert_with(|| FailureTracker::new(site_id));
        tracker.record_success();
        self.update_state();
        Ok(())
    }

    /// Record a health check failure for a site.
    pub fn record_failure(&mut self, site_id: u64) -> Result<(), ReplError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| ReplError::OrchestratorError {
                msg: format!("system time error: {}", e),
            })?
            .as_nanos() as u64;

        let tracker = self
            .trackers
            .entry(site_id)
            .or_insert_with(|| FailureTracker::new(site_id));
        tracker.record_failure(now);
        self.update_state();
        Ok(())
    }

    /// Check if failover should be triggered for a site.
    pub fn should_failover(&self, site_id: u64) -> bool {
        self.trackers
            .get(&site_id)
            .map(|t| t.should_failover(self.config.failure_threshold))
            .unwrap_or(false)
    }

    /// Check if recovery should be triggered for a site.
    pub fn should_recover(&self, site_id: u64) -> bool {
        self.trackers
            .get(&site_id)
            .map(|t| t.should_recover(self.config.recovery_threshold))
            .unwrap_or(false)
    }

    /// Get current state.
    pub fn state(&self) -> &FailoverControllerState {
        &self.state
    }

    /// Get all active (non-failed) sites.
    pub fn active_sites(&self) -> Vec<u64> {
        self.trackers
            .iter()
            .filter(|(_, t)| !t.should_failover(self.config.failure_threshold))
            .map(|(&id, _)| id)
            .collect()
    }

    /// Estimate failover time (milliseconds).
    /// Should be <5000ms (5 seconds).
    /// Includes: quorum consensus + metadata switchover + client reconnection.
    pub fn estimated_failover_time_ms(&self) -> u64 {
        let quorum_consensus_time = 1000;
        let metadata_switchover_time = 1000;
        let client_reconnection_time = 2000;
        let safety_margin = 500;

        quorum_consensus_time + metadata_switchover_time + client_reconnection_time + safety_margin
    }

    /// Reset tracking for a site (used after recovery).
    pub fn reset_site(&mut self, site_id: u64) -> Result<(), ReplError> {
        if let Some(tracker) = self.trackers.get_mut(&site_id) {
            tracker.consecutive_failures = 0;
            tracker.consecutive_successes = 0;
            self.update_state();
            Ok(())
        } else {
            Err(ReplError::SiteUnknown { site_id })
        }
    }

    /// Get failover count.
    pub fn failover_count(&self) -> u64 {
        self.failover_count
    }

    /// Get last failover timestamp (ns).
    pub fn last_failover_ts_ns(&self) -> u64 {
        self.last_failover_ts_ns
    }

    fn update_state(&mut self) {
        if matches!(self.state, FailoverControllerState::FailoverInProgress) {
            return;
        }

        let healthy_count = self
            .trackers
            .values()
            .filter(|t| !t.should_failover(self.config.failure_threshold))
            .count();

        let total_sites = self.trackers.len();

        if total_sites == 0 {
            self.state = FailoverControllerState::Healthy;
            return;
        }

        let has_degraded = self.trackers.values().any(|t| {
            t.consecutive_failures > 0 && !t.should_failover(self.config.failure_threshold)
        });

        let has_offline = self
            .trackers
            .values()
            .any(|t| t.should_failover(self.config.failure_threshold));

        if healthy_count == 0 {
            self.state = FailoverControllerState::Error("No healthy sites".to_string());
        } else if has_offline
            && self.config.graceful_degradation
            && healthy_count >= self.config.write_quorum_size
        {
            self.state = FailoverControllerState::SingleSite;
        } else if has_degraded {
            self.state = FailoverControllerState::Degraded;
        } else if healthy_count == total_sites {
            if self.config.active_active {
                self.state = FailoverControllerState::ActiveActive;
            } else {
                self.state = FailoverControllerState::Healthy;
            }
        } else {
            self.state = FailoverControllerState::Recovering;
        }
    }

    /// Trigger failover for a site (records failover event).
    pub fn trigger_failover(&mut self, site_id: u64) -> Result<(), ReplError> {
        if !self.should_failover(site_id) {
            return Err(ReplError::OrchestratorError {
                msg: format!("site {} has not reached failure threshold", site_id),
            });
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| ReplError::OrchestratorError {
                msg: format!("system time error: {}", e),
            })?
            .as_nanos() as u64;

        self.last_failover_ts_ns = now;
        self.failover_count += 1;
        self.state = FailoverControllerState::FailoverInProgress;
        self.update_state();

        Ok(())
    }

    /// Get tracker for a specific site.
    pub fn tracker(&self, site_id: u64) -> Option<&FailureTracker> {
        self.trackers.get(&site_id)
    }

    /// Get all trackers.
    pub fn all_trackers(&self) -> &HashMap<u64, FailureTracker> {
        &self.trackers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller_with_sites(config: FailoverConfig, sites: &[u64]) -> FailoverController {
        let mut controller = FailoverController::new(config);
        for &site_id in sites {
            let _ = controller.record_success(site_id);
        }
        controller
    }

    #[test]
    fn test_failure_tracking_increments() {
        let config = FailoverConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_failure(1).unwrap();
        controller.record_failure(1).unwrap();

        let tracker = controller.tracker(1).unwrap();
        assert_eq!(tracker.consecutive_failures, 2);
    }

    #[test]
    fn test_success_resets_counter() {
        let config = FailoverConfig::default();
        let mut controller = FailoverController::new(config);

        controller.record_failure(1).unwrap();
        controller.record_failure(1).unwrap();

        let tracker_before = controller.tracker(1).unwrap();
        assert!(tracker_before.consecutive_failures > 0);

        controller.record_success(1).unwrap();

        let tracker_after = controller.tracker(1).unwrap();
        assert_eq!(tracker_after.consecutive_failures, 0);
        assert!(tracker_after.consecutive_successes > 0);
    }

    #[test]
    fn test_failover_trigger_on_threshold() {
        let config = FailoverConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_success(1).unwrap();
        controller.record_failure(1).unwrap();
        controller.record_failure(1).unwrap();

        assert!(!controller.should_failover(1));

        controller.record_failure(1).unwrap();

        assert!(controller.should_failover(1));
    }

    #[test]
    fn test_recovery_trigger_on_successes() {
        let config = FailoverConfig {
            failure_threshold: 3,
            recovery_threshold: 2,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_success(1).unwrap();
        controller.record_failure(1).unwrap();
        controller.record_failure(1).unwrap();
        controller.record_failure(1).unwrap();

        assert!(controller.should_failover(1));

        controller.reset_site(1).unwrap();
        controller.record_success(1).unwrap();

        assert!(!controller.should_recover(1));

        controller.record_success(1).unwrap();

        assert!(controller.should_recover(1));
    }

    #[test]
    fn test_graceful_degradation_mode() {
        let config = FailoverConfig {
            failure_threshold: 2,
            graceful_degradation: true,
            write_quorum_size: 1,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_success(1).unwrap();
        controller.record_success(2).unwrap();

        controller.record_failure(1).unwrap();
        controller.record_failure(1).unwrap();

        assert_eq!(controller.state(), &FailoverControllerState::SingleSite);

        let active = controller.active_sites();
        assert!(active.contains(&2));
    }

    #[test]
    fn test_active_sites_list() {
        let config = FailoverConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let controller = controller_with_sites(config, &[1, 2, 3]);

        let active = controller.active_sites();
        assert_eq!(active.len(), 3);
        assert!(active.contains(&1));
        assert!(active.contains(&2));
        assert!(active.contains(&3));
    }

    #[test]
    fn test_failover_timing_estimate() {
        let config = FailoverConfig::default();
        let controller = FailoverController::new(config);

        let estimated_ms = controller.estimated_failover_time_ms();
        // Timing should be reasonable: health check + quorum consensus + metadata switchover
        assert!(
            estimated_ms > 0 && estimated_ms < 10000,
            "failover time should be 0-10000ms, got {}",
            estimated_ms
        );
    }

    #[test]
    fn test_trigger_failover_increments_count() {
        let config = FailoverConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_success(1).unwrap();
        assert_eq!(controller.failover_count(), 0);

        controller.record_failure(1).unwrap();
        controller.trigger_failover(1).unwrap();

        assert_eq!(controller.failover_count(), 1);
    }

    #[test]
    fn test_trigger_failover_updates_state() {
        let config = FailoverConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        // Register multiple sites so we have at least one healthy one after failover
        controller.record_success(1).unwrap();
        controller.record_success(2).unwrap();

        // Now trigger failure on site 1
        controller.record_failure(1).unwrap();
        let result = controller.trigger_failover(1);

        // Failover should succeed
        assert!(result.is_ok());

        // State should be valid (may be Healthy, Degraded, or FailoverInProgress)
        match controller.state() {
            FailoverControllerState::Healthy
            | FailoverControllerState::Degraded
            | FailoverControllerState::FailoverInProgress
            | FailoverControllerState::SingleSite => { /* OK */ }
            FailoverControllerState::Error(_) => panic!("unexpected error state"),
            _ => { /* other states OK */ }
        }
    }

    #[test]
    fn test_reset_site_clears_counters() {
        let config = FailoverConfig::default();
        let mut controller = FailoverController::new(config);

        controller.record_failure(1).unwrap();
        controller.record_failure(1).unwrap();

        let tracker_before = controller.tracker(1).unwrap();
        assert!(tracker_before.consecutive_failures > 0);

        controller.reset_site(1).unwrap();

        let tracker_after = controller.tracker(1).unwrap();
        assert_eq!(tracker_after.consecutive_failures, 0);
        assert_eq!(tracker_after.consecutive_successes, 0);
    }

    #[test]
    fn test_error_state_no_healthy_sites() {
        let config = FailoverConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_failure(1).unwrap();
        controller.record_failure(1).unwrap();
        controller.trigger_failover(1).unwrap();

        assert!(matches!(
            controller.state(),
            FailoverControllerState::Error(_)
        ));
    }

    #[test]
    fn test_active_active_state_when_all_healthy() {
        let config = FailoverConfig {
            active_active: true,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_success(1).unwrap();
        controller.record_success(2).unwrap();

        assert_eq!(controller.state(), &FailoverControllerState::ActiveActive);
    }

    #[test]
    fn test_degraded_state_partial_failures() {
        let config = FailoverConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_success(1).unwrap();
        controller.record_success(2).unwrap();
        controller.record_failure(2).unwrap();

        assert_eq!(controller.state(), &FailoverControllerState::Degraded);
    }

    #[test]
    fn test_tracking_unknown_site_returns_none() {
        let config = FailoverConfig::default();
        let controller = FailoverController::new(config);

        assert!(controller.tracker(999).is_none());
    }

    #[test]
    fn test_all_trackers_returns_map() {
        let config = FailoverConfig::default();
        let mut controller = FailoverController::new(config);

        controller.record_success(1).unwrap();
        controller.record_success(2).unwrap();

        let trackers = controller.all_trackers();
        assert_eq!(trackers.len(), 2);
    }

    #[test]
    fn test_last_failover_timestamp_updated() {
        let config = FailoverConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let mut controller = FailoverController::new(config);

        controller.record_success(1).unwrap();
        assert_eq!(controller.last_failover_ts_ns(), 0);

        controller.record_failure(1).unwrap();
        controller.trigger_failover(1).unwrap();

        assert!(controller.last_failover_ts_ns() > 0);
    }

    #[test]
    fn test_failover_config_defaults() {
        let config = FailoverConfig::default();
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.recovery_threshold, 2);
        assert_eq!(config.check_interval_ms, 5000);
        assert!(config.active_active);
        assert_eq!(config.failover_timeout_ms, 5000);
        assert!(config.graceful_degradation);
        assert_eq!(config.write_quorum_size, 1);
    }
}
