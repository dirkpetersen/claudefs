// File: crates/claudefs-repl/src/health.rs

//! Replication Health Monitoring.
//!
//! Monitors the health of each replication link and overall replication status.

use std::collections::HashMap;

/// Health status of a single replication link.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkHealth {
    /// Replication is current (lag within acceptable bounds).
    Healthy,
    /// Lag is growing but not critical.
    Degraded {
        /// Number of entries behind.
        lag_entries: u64,
        /// Lag in milliseconds (optional).
        lag_ms: Option<u64>,
    },
    /// Conduit is disconnected or not sending.
    Disconnected,
    /// Lag exceeds critical threshold.
    Critical {
        /// Number of entries behind.
        lag_entries: u64,
    },
}

/// Health report for one remote site's replication link.
#[derive(Debug, Clone)]
pub struct LinkHealthReport {
    /// Remote site ID.
    pub site_id: u64,
    /// Remote site name.
    pub site_name: String,
    /// Health status.
    pub health: LinkHealth,
    /// Last successful batch timestamp (microseconds).
    pub last_successful_batch_us: Option<u64>,
    /// Entries behind remote.
    pub entries_behind: u64,
    /// Consecutive error count.
    pub consecutive_errors: u32,
}

/// Overall replication cluster health.
#[derive(Debug, Clone, PartialEq)]
pub enum ClusterHealth {
    /// All links are healthy.
    Healthy,
    /// Some links are degraded but majority are healthy.
    Degraded,
    /// Majority of links are down or critical.
    Critical,
    /// No remote sites configured.
    NotConfigured,
}

/// Thresholds for health determination.
#[derive(Debug, Clone)]
pub struct HealthThresholds {
    /// Entry lag before a link is considered Degraded.
    pub degraded_lag_entries: u64,
    /// Entry lag before a link is considered Critical.
    pub critical_lag_entries: u64,
    /// Consecutive errors before marking Disconnected.
    pub disconnected_errors: u32,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            degraded_lag_entries: 1000,
            critical_lag_entries: 100_000,
            disconnected_errors: 5,
        }
    }
}

/// Internal state per site.
struct SiteHealthState {
    /// Consecutive error count.
    consecutive_errors: u32,
    /// Last successful batch timestamp (microseconds).
    last_successful_batch_us: Option<u64>,
    /// Entries behind.
    entries_behind: u64,
    /// Site name.
    site_name: String,
}

/// Computes and tracks replication health across all sites.
pub struct ReplicationHealthMonitor {
    /// Health thresholds.
    thresholds: HealthThresholds,
    /// Per-site state.
    site_state: HashMap<u64, SiteHealthState>,
}

impl ReplicationHealthMonitor {
    /// Create a new health monitor with default thresholds.
    pub fn new(thresholds: HealthThresholds) -> Self {
        Self {
            thresholds,
            site_state: HashMap::new(),
        }
    }

    /// Register a site for monitoring.
    pub fn register_site(&mut self, site_id: u64, site_name: String) {
        self.site_state.insert(
            site_id,
            SiteHealthState {
                consecutive_errors: 0,
                last_successful_batch_us: None,
                entries_behind: 0,
                site_name,
            },
        );
    }

    /// Record a successful batch sent/received for a site.
    pub fn record_success(&mut self, site_id: u64, entries_behind: u64, timestamp_us: u64) {
        if let Some(state) = self.site_state.get_mut(&site_id) {
            state.consecutive_errors = 0;
            state.last_successful_batch_us = Some(timestamp_us);
            state.entries_behind = entries_behind;
        }
    }

    /// Record a send/receive error for a site.
    pub fn record_error(&mut self, site_id: u64) {
        if let Some(state) = self.site_state.get_mut(&site_id) {
            state.consecutive_errors += 1;
        }
    }

    /// Get the health report for a specific site.
    pub fn site_health(&self, site_id: u64) -> Option<LinkHealthReport> {
        let state = self.site_state.get(&site_id)?;

        let health = self.compute_link_health(state);

        Some(LinkHealthReport {
            site_id,
            site_name: state.site_name.clone(),
            health,
            last_successful_batch_us: state.last_successful_batch_us,
            entries_behind: state.entries_behind,
            consecutive_errors: state.consecutive_errors,
        })
    }

    /// Get health reports for all registered sites.
    pub fn all_site_health(&self) -> Vec<LinkHealthReport> {
        let mut reports: Vec<_> = self
            .site_state
            .iter()
            .map(|(&site_id, state)| {
                let health = self.compute_link_health(state);
                LinkHealthReport {
                    site_id,
                    site_name: state.site_name.clone(),
                    health,
                    last_successful_batch_us: state.last_successful_batch_us,
                    entries_behind: state.entries_behind,
                    consecutive_errors: state.consecutive_errors,
                }
            })
            .collect();

        reports.sort_by_key(|r| r.site_id);
        reports
    }

    /// Get the overall cluster health.
    pub fn cluster_health(&self) -> ClusterHealth {
        if self.site_state.is_empty() {
            return ClusterHealth::NotConfigured;
        }

        let mut degraded_count = 0;
        let mut critical_count = 0;
        let mut disconnected_count = 0;

        for state in self.site_state.values() {
            let health = self.compute_link_health(state);
            match health {
                LinkHealth::Healthy => {}
                LinkHealth::Degraded { .. } => degraded_count += 1,
                LinkHealth::Disconnected => disconnected_count += 1,
                LinkHealth::Critical { .. } => critical_count += 1,
            }
        }

        let total = self.site_state.len();

        if critical_count > total / 2 || disconnected_count > total / 2 {
            ClusterHealth::Critical
        } else if degraded_count > 0 || critical_count > 0 {
            ClusterHealth::Degraded
        } else {
            ClusterHealth::Healthy
        }
    }

    /// Reset error count and state for a site (after reconnect).
    pub fn reset_site(&mut self, site_id: u64) {
        if let Some(state) = self.site_state.get_mut(&site_id) {
            state.consecutive_errors = 0;
            state.entries_behind = 0;
        }
    }

    /// Remove a site from monitoring.
    pub fn remove_site(&mut self, site_id: u64) {
        self.site_state.remove(&site_id);
    }

    fn compute_link_health(&self, state: &SiteHealthState) -> LinkHealth {
        if state.consecutive_errors >= self.thresholds.disconnected_errors {
            LinkHealth::Disconnected
        } else if state.entries_behind >= self.thresholds.critical_lag_entries {
            LinkHealth::Critical {
                lag_entries: state.entries_behind,
            }
        } else if state.entries_behind >= self.thresholds.degraded_lag_entries {
            LinkHealth::Degraded {
                lag_entries: state.entries_behind,
                lag_ms: None,
            }
        } else {
            LinkHealth::Healthy
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_monitor_not_configured() {
        let monitor = ReplicationHealthMonitor::new(HealthThresholds::default());
        assert_eq!(monitor.cluster_health(), ClusterHealth::NotConfigured);
    }

    #[test]
    fn test_register_site_record_success_healthy() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());
        monitor.register_site(2, "site2".to_string());

        monitor.record_success(2, 100, 1000000);

        let health = monitor.site_health(2).unwrap();
        assert_eq!(health.health, LinkHealth::Healthy);
    }

    #[test]
    fn test_record_errors_degraded() {
        let mut thresholds = HealthThresholds::default();
        thresholds.disconnected_errors = 3;
        let mut monitor = ReplicationHealthMonitor::new(thresholds);

        monitor.register_site(2, "site2".to_string());

        monitor.record_error(2);
        monitor.record_error(2);

        let health = monitor.site_health(2).unwrap();
        assert_eq!(health.consecutive_errors, 2);
        assert!(matches!(health.health, LinkHealth::Healthy));
    }

    #[test]
    fn test_record_errors_disconnected() {
        let mut thresholds = HealthThresholds::default();
        thresholds.disconnected_errors = 3;
        let mut monitor = ReplicationHealthMonitor::new(thresholds);

        monitor.register_site(2, "site2".to_string());

        monitor.record_error(2);
        monitor.record_error(2);
        monitor.record_error(2);

        let health = monitor.site_health(2).unwrap();
        assert_eq!(health.consecutive_errors, 3);
        assert_eq!(health.health, LinkHealth::Disconnected);
    }

    #[test]
    fn test_large_lag_critical() {
        let mut thresholds = HealthThresholds::default();
        thresholds.critical_lag_entries = 100_000;
        let mut monitor = ReplicationHealthMonitor::new(thresholds);

        monitor.register_site(2, "site2".to_string());
        monitor.record_success(2, 150_000, 1000000);

        let health = monitor.site_health(2).unwrap();
        assert!(matches!(
            health.health,
            LinkHealth::Critical {
                lag_entries: 150_000
            }
        ));
    }

    #[test]
    fn test_cluster_health_mixed_states() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());

        monitor.register_site(2, "site2".to_string());
        monitor.register_site(3, "site3".to_string());
        monitor.register_site(4, "site4".to_string());

        monitor.record_success(2, 100, 1000000);
        monitor.record_success(3, 2000, 1000000);
        monitor.record_error(4);
        monitor.record_error(4);
        monitor.record_error(4);
        monitor.record_error(4);
        monitor.record_error(4);

        assert_eq!(monitor.cluster_health(), ClusterHealth::Degraded);
    }

    #[test]
    fn test_cluster_health_all_healthy() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());

        monitor.register_site(2, "site2".to_string());
        monitor.register_site(3, "site3".to_string());

        monitor.record_success(2, 100, 1000000);
        monitor.record_success(3, 100, 1000000);

        assert_eq!(monitor.cluster_health(), ClusterHealth::Healthy);
    }

    #[test]
    fn test_cluster_health_critical() {
        let mut thresholds = HealthThresholds::default();
        thresholds.disconnected_errors = 2;
        let mut monitor = ReplicationHealthMonitor::new(thresholds);

        monitor.register_site(2, "site2".to_string());
        monitor.register_site(3, "site3".to_string());

        monitor.record_error(2);
        monitor.record_error(2);
        monitor.record_error(3);
        monitor.record_error(3);

        assert_eq!(monitor.cluster_health(), ClusterHealth::Critical);
    }

    #[test]
    fn test_reset_site_clears_errors() {
        let mut thresholds = HealthThresholds::default();
        thresholds.disconnected_errors = 3;
        let mut monitor = ReplicationHealthMonitor::new(thresholds);

        monitor.register_site(2, "site2".to_string());

        monitor.record_error(2);
        monitor.record_error(2);
        monitor.record_error(2);

        assert!(matches!(
            monitor.site_health(2).unwrap().health,
            LinkHealth::Disconnected
        ));

        monitor.reset_site(2);

        let health = monitor.site_health(2).unwrap();
        assert_eq!(health.consecutive_errors, 0);
    }

    #[test]
    fn test_remove_site() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());

        monitor.register_site(2, "site2".to_string());
        monitor.register_site(3, "site3".to_string());

        monitor.remove_site(2);

        assert!(monitor.site_health(2).is_none());
        assert!(monitor.site_health(3).is_some());
    }

    #[test]
    fn test_all_site_health_returns_all() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());

        monitor.register_site(2, "site2".to_string());
        monitor.register_site(3, "site3".to_string());
        monitor.register_site(4, "site4".to_string());

        let reports = monitor.all_site_health();
        assert_eq!(reports.len(), 3);
    }

    #[test]
    fn test_default_thresholds_values() {
        let thresholds = HealthThresholds::default();

        assert_eq!(thresholds.degraded_lag_entries, 1000);
        assert_eq!(thresholds.critical_lag_entries, 100_000);
        assert_eq!(thresholds.disconnected_errors, 5);
    }

    #[test]
    fn test_link_health_report_fields() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());
        monitor.register_site(2, "site2".to_string());
        monitor.record_success(2, 100, 1000000);

        let report = monitor.site_health(2).unwrap();

        assert_eq!(report.site_id, 2);
        assert_eq!(report.site_name, "site2");
        assert!(matches!(report.health, LinkHealth::Healthy));
        assert_eq!(report.last_successful_batch_us, Some(1000000));
        assert_eq!(report.entries_behind, 100);
        assert_eq!(report.consecutive_errors, 0);
    }

    #[test]
    fn test_degraded_lag_threshold() {
        let mut thresholds = HealthThresholds::default();
        thresholds.degraded_lag_entries = 500;
        let mut monitor = ReplicationHealthMonitor::new(thresholds);

        monitor.register_site(2, "site2".to_string());
        monitor.record_success(2, 800, 1000000);

        let health = monitor.site_health(2).unwrap();
        assert!(matches!(
            health.health,
            LinkHealth::Degraded {
                lag_entries: 800,
                ..
            }
        ));
    }

    #[test]
    fn test_site_health_nonexistent() {
        let monitor = ReplicationHealthMonitor::new(HealthThresholds::default());
        assert!(monitor.site_health(999).is_none());
    }

    #[test]
    fn test_multiple_sites_mixed_health() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());

        monitor.register_site(2, "site2".to_string());
        monitor.register_site(3, "site3".to_string());

        monitor.record_success(2, 100, 1000000);
        monitor.record_success(3, 2000, 1000000);

        let reports = monitor.all_site_health();
        assert_eq!(reports.len(), 2);

        let site2 = reports.iter().find(|r| r.site_id == 2).unwrap();
        let site3 = reports.iter().find(|r| r.site_id == 3).unwrap();

        assert!(matches!(site2.health, LinkHealth::Healthy));
        assert!(matches!(site3.health, LinkHealth::Degraded { .. }));
    }

    #[test]
    fn test_register_duplicate_site_overwrites() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());

        monitor.register_site(2, "site2_old".to_string());
        monitor.register_site(2, "site2_new".to_string());

        let report = monitor.site_health(2).unwrap();
        assert_eq!(report.site_name, "site2_new");
    }

    #[test]
    fn test_cluster_health_empty_after_removal() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());

        monitor.register_site(2, "site2".to_string());
        monitor.remove_site(2);

        assert_eq!(monitor.cluster_health(), ClusterHealth::NotConfigured);
    }

    #[test]
    fn test_record_success_updates_entries_behind() {
        let mut monitor = ReplicationHealthMonitor::new(HealthThresholds::default());

        monitor.register_site(2, "site2".to_string());
        monitor.record_success(2, 500, 1000000);
        monitor.record_success(2, 100, 1000001);

        let report = monitor.site_health(2).unwrap();
        assert_eq!(report.entries_behind, 100);
    }

    #[test]
    fn test_link_health_partial_eq() {
        let h1 = LinkHealth::Healthy;
        let h2 = LinkHealth::Healthy;
        let h3 = LinkHealth::Degraded {
            lag_entries: 100,
            lag_ms: None,
        };

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_cluster_health_partial_eq() {
        let c1 = ClusterHealth::Healthy;
        let c2 = ClusterHealth::Healthy;
        let c3 = ClusterHealth::Degraded;

        assert_eq!(c1, c2);
        assert_ne!(c1, c3);
    }
}
