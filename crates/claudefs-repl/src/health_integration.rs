//! Health integration for replication metrics and health checks.
//!
//! Provides health checking and HTTP endpoint integration for replication status.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::dual_site_orchestrator::HealthStatus;
use crate::repl_metrics_exporter::ReplMetricsExporter;
use crate::topology::SiteId;

/// Health status response for replication.
#[derive(Debug, Clone, Serialize)]
pub struct ReplHealthStatus {
    /// Overall health status.
    pub status: HealthStatus,
    /// Per-site replication lag in seconds.
    pub lag_secs: HashMap<SiteId, f64>,
    /// Whether split-brain is currently detected.
    pub split_brain_detected: bool,
    /// Number of connected sites.
    pub connected_sites: usize,
    /// Human-readable status message.
    pub message: String,
}

/// Health checker using metrics exporter.
/// Determines health based on replication lag and split-brain status.
pub struct ReplHealthChecker {
    exporter: Arc<ReplMetricsExporter>,
    lag_warn_threshold_secs: f64,
    lag_critical_threshold_secs: f64,
    split_brain_active: Mutex<bool>,
}

impl ReplHealthChecker {
    /// Create a new health checker with default thresholds.
    pub fn new(exporter: Arc<ReplMetricsExporter>) -> Self {
        Self {
            exporter,
            lag_warn_threshold_secs: 60.0,
            lag_critical_threshold_secs: 300.0,
            split_brain_active: Mutex::new(false),
        }
    }

    /// Check the current health status.
    pub fn check_health(&self) -> ReplHealthStatus {
        let connected_sites = self.exporter.connected_sites_count() as usize;
        let split_brain = self.exporter.get_current_split_brain_status();
        *self.split_brain_active.lock().unwrap() = split_brain;

        let lag_secs = self.exporter.get_all_lags();
        let mut max_lag = 0.0f64;
        let mut any_lag_critical = false;
        let mut any_lag_warn = false;
        let mut any_lag_above_warn = false;

        for lag in lag_secs.values() {
            if *lag >= self.lag_critical_threshold_secs {
                any_lag_critical = true;
            }
            if *lag >= self.lag_warn_threshold_secs {
                any_lag_warn = true;
            }
            if *lag > self.lag_warn_threshold_secs {
                any_lag_above_warn = true;
            }
            if *lag > max_lag {
                max_lag = *lag;
            }
        }

        let (status, message) = if split_brain {
            (HealthStatus::Unhealthy, "Split-brain detected".to_string())
        } else if connected_sites == 0 {
            (HealthStatus::Unhealthy, "No sites connected".to_string())
        } else if any_lag_critical {
            (
                HealthStatus::Unhealthy,
                format!("Critical lag: {:.1}s", max_lag),
            )
        } else if connected_sites < 2 || any_lag_above_warn {
            (
                HealthStatus::Degraded,
                format!("Degraded: lag {:.1}s, {} sites", max_lag, connected_sites),
            )
        } else if any_lag_warn {
            (
                HealthStatus::Degraded,
                format!("Warning lag: {:.1}s", max_lag),
            )
        } else {
            (HealthStatus::Healthy, "All sites healthy".to_string())
        };

        ReplHealthStatus {
            status,
            lag_secs,
            split_brain_detected: split_brain,
            connected_sites,
            message,
        }
    }

    /// Convert health status to HTTP response.
    /// Returns (status_code, json_body).
    pub fn to_http_response(&self) -> (u16, String) {
        let health = self.check_health();
        let status_code = match health.status {
            HealthStatus::Healthy => 200,
            HealthStatus::Degraded => 200,
            HealthStatus::Unhealthy => 503,
        };
        (status_code, health.to_json())
    }

    /// Set the lag thresholds.
    pub fn set_lag_thresholds(&mut self, warn: f64, critical: f64) {
        self.lag_warn_threshold_secs = warn;
        self.lag_critical_threshold_secs = critical;
    }

    /// Get the status as JSON.
    pub fn get_status_json(&self) -> String {
        self.check_health().to_json()
    }

    /// Mark split-brain status.
    pub fn mark_split_brain(&self, active: bool) {
        *self.split_brain_active.lock().unwrap() = active;
    }

    /// Get the current status.
    pub fn get_status(&self) -> HealthStatus {
        self.check_health().status
    }
}

impl ReplHealthStatus {
    /// Convert to JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_checker_healthy_status() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        exporter.set_connected_sites(2);
        exporter.update_replication_lag(1, 10.0);
        exporter.update_replication_lag(2, 10.0);

        let checker = ReplHealthChecker::new(exporter);
        let status = checker.check_health();

        assert_eq!(status.status, HealthStatus::Healthy);
        assert!(status.message.contains("healthy"));
    }

    #[test]
    fn test_health_checker_degraded_on_lag() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        exporter.set_connected_sites(2);
        exporter.update_replication_lag(1, 100.0);

        let mut checker = ReplHealthChecker::new(exporter);
        checker.set_lag_thresholds(60.0, 300.0);

        let status = checker.check_health();
        assert_eq!(status.status, HealthStatus::Degraded);
    }

    #[test]
    fn test_health_checker_unhealthy_on_split_brain() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        exporter.set_connected_sites(2);
        exporter.record_split_brain_event();

        let checker = ReplHealthChecker::new(exporter);
        let status = checker.check_health();

        assert_eq!(status.status, HealthStatus::Unhealthy);
        assert!(status.split_brain_detected);
    }

    #[test]
    fn test_health_check_http_response_codes() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        exporter.set_connected_sites(2);

        let checker = ReplHealthChecker::new(exporter);
        let (code, _body) = checker.to_http_response();

        assert_eq!(code, 200);
    }

    #[test]
    fn test_health_status_json_serialization() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        exporter.set_connected_sites(2);
        exporter.update_replication_lag(1, 50.0);

        let checker = ReplHealthChecker::new(exporter);
        let json = checker.get_status_json();

        assert!(json.contains("\"status\""));
        assert!(json.contains("\"connected_sites\""));
    }

    #[test]
    fn test_health_checker_unhealthy_on_no_sites() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        exporter.set_connected_sites(0);

        let checker = ReplHealthChecker::new(exporter);
        let status = checker.check_health();

        assert_eq!(status.status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_health_checker_critical_lag() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        exporter.set_connected_sites(2);
        exporter.update_replication_lag(1, 500.0);

        let mut checker = ReplHealthChecker::new(exporter);
        checker.set_lag_thresholds(60.0, 300.0);

        let status = checker.check_health();
        assert_eq!(status.status, HealthStatus::Unhealthy);
    }
}
