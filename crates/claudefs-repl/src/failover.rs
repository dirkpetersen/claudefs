//! Active-active site failover management.
//!
//! Implements automatic site failover with read-write capability on both sites.
//! This is Priority 3 in the ClaudeFS feature roadmap.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};

/// Site role in active-active mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SiteMode {
    /// Site is fully active: accepts reads and writes.
    #[default]
    ActiveReadWrite,
    /// Site is in standby: accepts reads only.
    StandbyReadOnly,
    /// Site is degraded but still accepts writes.
    DegradedAcceptWrites,
    /// Site is offline.
    Offline,
}

/// Failover configuration.
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
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            recovery_threshold: 2,
            check_interval_ms: 5000,
            active_active: true,
        }
    }
}

/// Failover event.
#[derive(Debug, Clone, PartialEq)]
pub enum FailoverEvent {
    /// Site promoted to a new mode.
    SitePromoted {
        /// Site identifier.
        site_id: u64,
        /// New site mode.
        new_mode: SiteMode,
    },
    /// Site demoted to a new mode.
    SiteDemoted {
        /// Site identifier.
        site_id: u64,
        /// New site mode.
        new_mode: SiteMode,
        /// Reason for demotion.
        reason: String,
    },
    /// Site recovered and is now fully active.
    SiteRecovered {
        /// Site identifier.
        site_id: u64,
    },
    /// Conflict detected that requires resolution.
    ConflictRequiresResolution {
        /// Site identifier.
        site_id: u64,
        /// Inode with conflict.
        inode: u64,
    },
}

/// Per-site failover state.
#[derive(Debug, Clone, Default)]
pub struct SiteFailoverState {
    /// Site identifier.
    pub site_id: u64,
    /// Current site mode.
    pub mode: SiteMode,
    /// Consecutive failure count.
    pub consecutive_failures: u32,
    /// Consecutive success count.
    pub consecutive_successes: u32,
    /// Last health check timestamp in microseconds.
    pub last_check_us: u64,
    /// Total number of failovers for this site.
    pub failover_count: u64,
}

impl SiteFailoverState {
    /// Create a new site failover state.
    pub fn new(site_id: u64) -> Self {
        Self {
            site_id,
            mode: SiteMode::ActiveReadWrite,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_check_us: 0,
            failover_count: 0,
        }
    }

    /// Check if the site is writable.
    pub fn is_writable(&self) -> bool {
        matches!(
            self.mode,
            SiteMode::ActiveReadWrite | SiteMode::DegradedAcceptWrites
        )
    }

    /// Check if the site is readable.
    pub fn is_readable(&self) -> bool {
        !matches!(self.mode, SiteMode::Offline)
    }

    fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.consecutive_successes = 0;
    }

    fn record_success(&mut self) {
        self.consecutive_successes += 1;
        self.consecutive_failures = 0;
    }

    #[allow(dead_code)]
    fn reset_counters(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_successes = 0;
    }
}

/// The failover manager.
pub struct FailoverManager {
    config: FailoverConfig,
    #[allow(dead_code)]
    local_site_id: u64,
    sites: Arc<Mutex<HashMap<u64, SiteFailoverState>>>,
    events: Arc<Mutex<Vec<FailoverEvent>>>,
}

impl FailoverManager {
    /// Create a new failover manager.
    pub fn new(config: FailoverConfig, local_site_id: u64) -> Self {
        Self {
            config,
            local_site_id,
            sites: Arc::new(Mutex::new(HashMap::new())),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a new site for failover management.
    pub async fn register_site(&self, site_id: u64) {
        let mut sites = self.sites.lock().await;
        sites.entry(site_id).or_insert_with(|| SiteFailoverState::new(site_id));
    }

    /// Record health check result and process state transitions.
    pub async fn record_health(&self, site_id: u64, healthy: bool) -> Vec<FailoverEvent> {
        let mut sites = self.sites.lock().await;
        let mut events = Vec::new();

        let state = match sites.get_mut(&site_id) {
            Some(s) => s,
            None => {
                let mut new_state = SiteFailoverState::new(site_id);
                if !healthy {
                    new_state.record_failure();
                } else {
                    new_state.record_success();
                }
                sites.insert(site_id, new_state);
                sites.get_mut(&site_id).unwrap()
            }
        };

        let old_mode = state.mode.clone();

        if healthy {
            state.record_success();
        } else {
            state.record_failure();
        }

        state.last_check_us = u64::MAX;

        let new_mode = self.calculate_new_mode(state, healthy);

        if new_mode != old_mode {
            state.mode = new_mode.clone();
            state.failover_count += 1;

            if self.is_promotion(&old_mode, &new_mode) {
                match &new_mode {
                    SiteMode::StandbyReadOnly => {
                        events.push(FailoverEvent::SitePromoted {
                            site_id,
                            new_mode: new_mode.clone(),
                        });
                    }
                    SiteMode::ActiveReadWrite => {
                        events.push(FailoverEvent::SiteRecovered { site_id });
                    }
                    _ => {}
                }
            } else {
                let reason = match old_mode {
                    SiteMode::ActiveReadWrite => "consecutive failures".to_string(),
                    SiteMode::DegradedAcceptWrites => "continued failures".to_string(),
                    SiteMode::StandbyReadOnly => "health check failed".to_string(),
                    SiteMode::Offline => "already offline".to_string(),
                };
                events.push(FailoverEvent::SiteDemoted {
                    site_id,
                    new_mode,
                    reason,
                });
            }
        }

        let mut events_lock = self.events.lock().await;
        events_lock.extend(events.clone());
        events
    }

    fn calculate_new_mode(&self, state: &SiteFailoverState, healthy: bool) -> SiteMode {
        let failures = state.consecutive_failures;
        let successes = state.consecutive_successes;

        match state.mode {
            SiteMode::ActiveReadWrite => {
                if failures >= self.config.failure_threshold {
                    SiteMode::DegradedAcceptWrites
                } else {
                    SiteMode::ActiveReadWrite
                }
            }
            SiteMode::DegradedAcceptWrites => {
                if failures >= self.config.failure_threshold {
                    SiteMode::Offline
                } else {
                    SiteMode::DegradedAcceptWrites
                }
            }
            SiteMode::StandbyReadOnly => {
                if !healthy && failures >= self.config.failure_threshold {
                    SiteMode::Offline
                } else if successes >= self.config.recovery_threshold {
                    SiteMode::ActiveReadWrite
                } else {
                    SiteMode::StandbyReadOnly
                }
            }
            SiteMode::Offline => {
                if successes >= self.config.recovery_threshold {
                    SiteMode::StandbyReadOnly
                } else {
                    SiteMode::Offline
                }
            }
        }
    }

    fn is_promotion(&self, old_mode: &SiteMode, new_mode: &SiteMode) -> bool {
        matches!(
            (old_mode, new_mode),
            (SiteMode::Offline, SiteMode::StandbyReadOnly)
                | (SiteMode::StandbyReadOnly, SiteMode::ActiveReadWrite)
                | (SiteMode::Offline, SiteMode::ActiveReadWrite)
                | (SiteMode::Offline, SiteMode::DegradedAcceptWrites)
        )
    }

    /// Get the mode for a specific site.
    pub async fn site_mode(&self, site_id: u64) -> Option<SiteMode> {
        let sites = self.sites.lock().await;
        sites.get(&site_id).map(|s| s.mode.clone())
    }

    /// Get list of writable site IDs.
    pub async fn writable_sites(&self) -> Vec<u64> {
        let sites = self.sites.lock().await;
        sites
            .values()
            .filter(|s| s.is_writable())
            .map(|s| s.site_id)
            .collect()
    }

    /// Get list of readable site IDs.
    pub async fn readable_sites(&self) -> Vec<u64> {
        let sites = self.sites.lock().await;
        sites
            .values()
            .filter(|s| s.is_readable())
            .map(|s| s.site_id)
            .collect()
    }

    /// Force a site into a specific mode.
    pub async fn force_mode(
        &self,
        site_id: u64,
        mode: SiteMode,
    ) -> Result<(), crate::error::ReplError> {
        let mut sites = self.sites.lock().await;
        let state = sites.get_mut(&site_id).ok_or(crate::error::ReplError::SiteUnknown { site_id })?;

        let old_mode = state.mode.clone();
        state.mode = mode.clone();

        if old_mode != mode {
            state.failover_count += 1;
            let mut events_lock = self.events.lock().await;

            if self.is_promotion(&old_mode, &mode) {
                events_lock.push(FailoverEvent::SitePromoted {
                    site_id,
                    new_mode: mode,
                });
            } else {
                events_lock.push(FailoverEvent::SiteDemoted {
                    site_id,
                    new_mode: mode,
                    reason: "forced".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Drain and return all pending events.
    pub async fn drain_events(&self) -> Vec<FailoverEvent> {
        let mut events = self.events.lock().await;
        std::mem::take(&mut *events)
    }

    /// Get failover state for all sites.
    pub async fn all_states(&self) -> Vec<SiteFailoverState> {
        let sites = self.sites.lock().await;
        sites.values().cloned().collect()
    }

    /// Get failover counts per site.
    pub async fn failover_counts(&self) -> HashMap<u64, u64> {
        let sites = self.sites.lock().await;
        sites
            .values()
            .map(|s| (s.site_id, s.failover_count))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_failover_manager_new() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        let modes = manager.writable_sites().await;
        assert!(modes.is_empty());
    }

    #[tokio::test]
    async fn test_register_site() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::ActiveReadWrite));
    }

    #[tokio::test]
    async fn test_record_health_healthy() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        let events = manager.record_health(100, true).await;
        assert!(events.is_empty());

        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::ActiveReadWrite));
    }

    #[tokio::test]
    async fn test_record_health_single_failure() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        let events = manager.record_health(100, false).await;
        assert!(events.is_empty());

        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::ActiveReadWrite));
    }

    #[tokio::test]
    async fn test_record_health_failure_threshold() {
        let config = FailoverConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager.record_health(100, false).await;
        manager.record_health(100, false).await;
        let events = manager.record_health(100, false).await;

        assert!(!events.is_empty());
        if let FailoverEvent::SiteDemoted { new_mode, .. } = &events[0] {
            assert_eq!(new_mode, &SiteMode::DegradedAcceptWrites);
        } else {
            panic!("expected SiteDemoted");
        }
    }

    #[tokio::test]
    async fn test_record_health_offline_transition() {
        let config = FailoverConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager.record_health(100, false).await;
        manager.record_health(100, false).await;
        let _events = manager.record_health(100, false).await;

        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::Offline));
    }

    #[tokio::test]
    async fn test_record_health_recovery_to_standby() {
        let config = FailoverConfig {
            failure_threshold: 2,
            recovery_threshold: 2,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager.record_health(100, false).await;
        manager.record_health(100, false).await;
        assert_eq!(manager.site_mode(100).await, Some(SiteMode::DegradedAcceptWrites));

        manager.record_health(100, false).await;
        assert_eq!(manager.site_mode(100).await, Some(SiteMode::Offline));

        let _events = manager.record_health(100, true).await;

        let _events = manager.record_health(100, true).await;
        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::StandbyReadOnly));
    }

    #[tokio::test]
    async fn test_record_health_recovery_to_active() {
        let config = FailoverConfig {
            failure_threshold: 2,
            recovery_threshold: 2,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager.record_health(100, false).await;
        manager.record_health(100, false).await;
        assert_eq!(manager.site_mode(100).await, Some(SiteMode::DegradedAcceptWrites));

        manager.record_health(100, false).await;
        assert_eq!(manager.site_mode(100).await, Some(SiteMode::Offline));

        manager.record_health(100, true).await;
        manager.record_health(100, true).await;
        assert_eq!(
            manager.site_mode(100).await,
            Some(SiteMode::StandbyReadOnly)
        );

        manager.record_health(100, true).await;
        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::ActiveReadWrite));
    }

    #[tokio::test]
    async fn test_writable_sites() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        manager.register_site(200).await;

        let writable = manager.writable_sites().await;
        assert_eq!(writable.len(), 2);
    }

    #[tokio::test]
    async fn test_writable_sites_offline() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        manager.register_site(200).await;

        manager.force_mode(100, SiteMode::Offline).await.unwrap();

        let writable = manager.writable_sites().await;
        assert_eq!(writable, vec![200]);
    }

    #[tokio::test]
    async fn test_readable_sites() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        manager.register_site(200).await;

        manager.force_mode(100, SiteMode::StandbyReadOnly).await.unwrap();

        let readable = manager.readable_sites().await;
        assert_eq!(readable.len(), 2);
    }

    #[tokio::test]
    async fn test_readable_sites_offline_excluded() {
        let config = FailoverConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        manager.register_site(200).await;

        manager.force_mode(100, SiteMode::Offline).await.unwrap();
        manager.force_mode(200, SiteMode::Offline).await.unwrap();

        let readable = manager.readable_sites().await;
        assert!(readable.is_empty());
    }

    #[tokio::test]
    async fn test_force_mode() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager
            .force_mode(100, SiteMode::StandbyReadOnly)
            .await
            .unwrap();

        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::StandbyReadOnly));
    }

    #[tokio::test]
    async fn test_force_mode_unknown_site() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);

        let result = manager.force_mode(999, SiteMode::Offline).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_force_mode_events() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager
            .force_mode(100, SiteMode::Offline)
            .await
            .unwrap();

        let events = manager.drain_events().await;
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn test_drain_events() {
        let config = FailoverConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        manager.record_health(100, false).await;

        let events = manager.drain_events().await;
        assert!(!events.is_empty());

        let events = manager.drain_events().await;
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_all_states() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        manager.register_site(200).await;

        let states = manager.all_states().await;
        assert_eq!(states.len(), 2);
    }

    #[tokio::test]
    async fn test_failover_counts() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager
            .force_mode(100, SiteMode::Offline)
            .await
            .unwrap();

        let counts = manager.failover_counts().await;
        assert_eq!(counts[&100], 1);
    }

    #[tokio::test]
    async fn test_degraded_accept_writes() {
        let config = FailoverConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager.record_health(100, false).await;
        manager.record_health(100, false).await;
        let events = manager.record_health(100, false).await;

        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::DegradedAcceptWrites));

        let writable = manager.writable_sites().await;
        assert!(writable.contains(&100));
    }

    #[tokio::test]
    async fn test_standby_readonly_not_writable() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager
            .force_mode(100, SiteMode::StandbyReadOnly)
            .await
            .unwrap();

        let writable = manager.writable_sites().await;
        assert!(!writable.contains(&100));

        let readable = manager.readable_sites().await;
        assert!(readable.contains(&100));
    }

    #[tokio::test]
    async fn test_standby_recovery() {
        let config = FailoverConfig {
            failure_threshold: 1,
            recovery_threshold: 2,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager
            .force_mode(100, SiteMode::StandbyReadOnly)
            .await
            .unwrap();

        manager.record_health(100, true).await;
        let events = manager.record_health(100, true).await;

        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::ActiveReadWrite));
    }

    #[tokio::test]
    async fn test_standby_failure_to_offline() {
        let config = FailoverConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;

        manager
            .force_mode(100, SiteMode::StandbyReadOnly)
            .await
            .unwrap();

        let events = manager.record_health(100, false).await;
        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::Offline));
    }

    #[tokio::test]
    async fn test_multiple_sites() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);

        for i in 1..=5 {
            manager.register_site(i).await;
        }

        let writable = manager.writable_sites().await;
        assert_eq!(writable.len(), 5);
    }

    #[tokio::test]
    async fn test_failover_event_variants() {
        let event1 = FailoverEvent::SitePromoted {
            site_id: 1,
            new_mode: SiteMode::ActiveReadWrite,
        };
        let event2 = FailoverEvent::SiteDemoted {
            site_id: 1,
            new_mode: SiteMode::Offline,
            reason: "test".to_string(),
        };
        let event3 = FailoverEvent::SiteRecovered { site_id: 1 };
        let event4 = FailoverEvent::ConflictRequiresResolution {
            site_id: 1,
            inode: 100,
        };

        format!("{:?}", event1);
        format!("{:?}", event2);
        format!("{:?}", event3);
        format!("{:?}", event4);
    }

    #[tokio::test]
    async fn test_site_failover_state_new() {
        let state = SiteFailoverState::new(100);
        assert_eq!(state.site_id, 100);
        assert_eq!(state.mode, SiteMode::ActiveReadWrite);
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.consecutive_successes, 0);
    }

    #[tokio::test]
    async fn test_site_failover_state_is_writable() {
        let state = SiteFailoverState {
            site_id: 100,
            mode: SiteMode::ActiveReadWrite,
            ..Default::default()
        };
        assert!(state.is_writable());

        let state = SiteFailoverState {
            site_id: 100,
            mode: SiteMode::DegradedAcceptWrites,
            ..Default::default()
        };
        assert!(state.is_writable());

        let state = SiteFailoverState {
            site_id: 100,
            mode: SiteMode::StandbyReadOnly,
            ..Default::default()
        };
        assert!(!state.is_writable());

        let state = SiteFailoverState {
            site_id: 100,
            mode: SiteMode::Offline,
            ..Default::default()
        };
        assert!(!state.is_writable());
    }

    #[tokio::test]
    async fn test_site_failover_state_is_readable() {
        let state = SiteFailoverState {
            site_id: 100,
            mode: SiteMode::ActiveReadWrite,
            ..Default::default()
        };
        assert!(state.is_readable());

        let state = SiteFailoverState {
            site_id: 100,
            mode: SiteMode::Offline,
            ..Default::default()
        };
        assert!(!state.is_readable());
    }

    #[tokio::test]
    async fn test_site_mode_default() {
        let mode: SiteMode = Default::default();
        assert_eq!(mode, SiteMode::ActiveReadWrite);
    }

    #[tokio::test]
    async fn test_failover_config_default() {
        let config = FailoverConfig::default();
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.recovery_threshold, 2);
        assert_eq!(config.check_interval_ms, 5000);
        assert!(config.active_active);
    }
}