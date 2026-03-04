//! Unified health aggregator for the storage system.
//!
//! Combines DeviceHealthMonitor, scrub statistics, and background scheduler state
//! into a single health picture for the operator.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

use crate::background_scheduler::SchedulerStats;
use crate::device_health_monitor::{
    AlertSeverity, DeviceHealthMonitor, DeviceHealthSummary, HealthAlert, HealthAlertType,
    SmartSnapshot, WearSnapshot,
};
use crate::scrub::ScrubStats;

/// Overall health status of the storage system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageHealthStatus {
    /// All systems healthy.
    Healthy,
    /// Minor issues detected, operator should be aware.
    Degraded,
    /// Major issues requiring immediate attention.
    Critical,
    /// System offline or uninitialized.
    Offline,
}

/// A single storage health event (alert or notice).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthEvent {
    /// Timestamp in seconds since epoch.
    pub timestamp_secs: u64,
    /// Source of the event (e.g., "device:0", "scrub", "scheduler").
    pub source: String,
    /// Human-readable message.
    pub message: String,
    /// Severity of the event.
    pub severity: HealthEventSeverity,
}

/// Severity of a health event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthEventSeverity {
    /// Informational.
    Info,
    /// Warning.
    Warning,
    /// Critical.
    Critical,
}

/// Snapshot of the full storage health at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageHealthSnapshot {
    /// Overall health status.
    pub status: StorageHealthStatus,
    /// Overall health score (0.0 = failed, 1.0 = perfect).
    pub overall_score: f64,
    /// Number of devices registered.
    pub device_count: usize,
    /// Number of healthy devices.
    pub healthy_device_count: usize,
    /// Number of degraded devices.
    pub degraded_device_count: usize,
    /// Number of critical devices.
    pub critical_device_count: usize,
    /// Active alerts.
    pub active_alerts: Vec<HealthEvent>,
    /// Scrub errors from last run.
    pub scrub_errors_recent: u64,
    /// Pending background tasks.
    pub scheduler_pending_tasks: usize,
    /// Timestamp of the snapshot.
    pub snapshot_timestamp_secs: u64,
}

impl StorageHealthSnapshot {
    /// Returns true if status is Healthy.
    pub fn is_healthy(&self) -> bool {
        self.status == StorageHealthStatus::Healthy
    }
}

/// Configuration for the storage health aggregator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageHealthConfig {
    /// Health score threshold below which a device is "degraded".
    pub degraded_threshold: f64,
    /// Health score threshold below which a device is "critical".
    pub critical_threshold: f64,
    /// Number of scrub errors that triggers a Warning.
    pub scrub_error_warning_threshold: u64,
    /// Number of scrub errors that triggers Critical.
    pub scrub_error_critical_threshold: u64,
}

impl Default for StorageHealthConfig {
    fn default() -> Self {
        Self {
            degraded_threshold: 0.6,
            critical_threshold: 0.3,
            scrub_error_warning_threshold: 1,
            scrub_error_critical_threshold: 10,
        }
    }
}

/// Unified health aggregator for the storage system.
pub struct StorageHealth {
    config: StorageHealthConfig,
    device_monitor: DeviceHealthMonitor,
    scrub_stats: ScrubStats,
    scheduler_stats: SchedulerStats,
}

impl StorageHealth {
    /// Creates a new StorageHealth with the given configuration.
    pub fn new(config: StorageHealthConfig) -> Self {
        info!(
            degraded_threshold = config.degraded_threshold,
            critical_threshold = config.critical_threshold,
            scrub_error_warning_threshold = config.scrub_error_warning_threshold,
            scrub_error_critical_threshold = config.scrub_error_critical_threshold,
            "creating storage health aggregator"
        );

        Self {
            config,
            device_monitor: DeviceHealthMonitor::new(),
            scrub_stats: ScrubStats::default(),
            scheduler_stats: SchedulerStats::default(),
        }
    }

    /// Register a device for monitoring.
    pub fn register_device(&mut self, device_idx: u16, device_path: String) {
        self.device_monitor.register_device(device_idx, device_path);
    }

    /// Update device SMART data.
    pub fn update_device_smart(&mut self, device_idx: u16, smart: SmartSnapshot) {
        let _ = self.device_monitor.update_smart(device_idx, smart);
    }

    /// Update device wear data.
    pub fn update_device_wear(&mut self, device_idx: u16, wear: WearSnapshot) {
        let _ = self.device_monitor.update_wear(device_idx, wear);
    }

    /// Update device capacity data.
    pub fn update_device_capacity(&mut self, device_idx: u16, total_bytes: u64, free_bytes: u64) {
        let _ = self
            .device_monitor
            .update_capacity(device_idx, total_bytes, free_bytes);
    }

    /// Update scrub statistics from the latest scrub run.
    pub fn update_scrub_stats(&mut self, stats: ScrubStats) {
        self.scrub_stats = stats;
    }

    /// Update scheduler statistics.
    pub fn update_scheduler_stats(&mut self, stats: SchedulerStats) {
        self.scheduler_stats = stats;
    }

    /// Compute and return a full health snapshot.
    pub fn snapshot(&self) -> StorageHealthSnapshot {
        let summaries = self.device_monitor.health_summary();
        let device_alerts = self.device_monitor.check_alerts();

        let device_count = summaries.len();
        let mut healthy_device_count = 0usize;
        let mut degraded_device_count = 0usize;
        let mut critical_device_count = 0usize;

        for summary in &summaries {
            if summary.health_score >= self.config.degraded_threshold {
                healthy_device_count += 1;
            } else if summary.health_score >= self.config.critical_threshold {
                degraded_device_count += 1;
            } else {
                critical_device_count += 1;
            }
        }

        let mut active_alerts: Vec<HealthEvent> = Vec::new();
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for alert in &device_alerts {
            let severity = match alert.severity {
                AlertSeverity::Warning => HealthEventSeverity::Warning,
                AlertSeverity::Critical => HealthEventSeverity::Critical,
            };
            active_alerts.push(HealthEvent {
                timestamp_secs: now_secs,
                source: format!("device:{}", alert.device_idx),
                message: alert.message.clone(),
                severity,
            });
        }

        if self.scrub_stats.errors_detected >= self.config.scrub_error_critical_threshold {
            active_alerts.push(HealthEvent {
                timestamp_secs: now_secs,
                source: "scrub".to_string(),
                message: format!(
                    "{} scrub errors detected (critical threshold {})",
                    self.scrub_stats.errors_detected, self.config.scrub_error_critical_threshold
                ),
                severity: HealthEventSeverity::Critical,
            });
        } else if self.scrub_stats.errors_detected >= self.config.scrub_error_warning_threshold {
            active_alerts.push(HealthEvent {
                timestamp_secs: now_secs,
                source: "scrub".to_string(),
                message: format!(
                    "{} scrub errors detected (warning threshold {})",
                    self.scrub_stats.errors_detected, self.config.scrub_error_warning_threshold
                ),
                severity: HealthEventSeverity::Warning,
            });
        }

        let overall_score = if device_count == 0 {
            1.0
        } else {
            summaries.iter().map(|s| s.health_score).sum::<f64>() / device_count as f64
        };

        let has_critical_device = critical_device_count > 0;
        let has_degraded_device = degraded_device_count > 0;
        let has_critical_alert = active_alerts
            .iter()
            .any(|a| a.severity == HealthEventSeverity::Critical);
        let has_warning_alert = active_alerts
            .iter()
            .any(|a| a.severity == HealthEventSeverity::Warning);

        let status = if device_count == 0 {
            StorageHealthStatus::Offline
        } else if has_critical_device || has_critical_alert {
            StorageHealthStatus::Critical
        } else if has_degraded_device || has_warning_alert {
            StorageHealthStatus::Degraded
        } else {
            StorageHealthStatus::Healthy
        };

        StorageHealthSnapshot {
            status,
            overall_score,
            device_count,
            healthy_device_count,
            degraded_device_count,
            critical_device_count,
            active_alerts,
            scrub_errors_recent: self.scrub_stats.errors_detected,
            scheduler_pending_tasks: self.scheduler_stats.pending_count,
            snapshot_timestamp_secs: now_secs,
        }
    }

    /// Returns the current overall status (shortcut).
    pub fn status(&self) -> StorageHealthStatus {
        self.snapshot().status
    }

    /// Returns all active alerts as HealthEvents.
    pub fn active_alerts(&self) -> Vec<HealthEvent> {
        self.snapshot().active_alerts
    }
}

impl Default for StorageHealth {
    fn default() -> Self {
        Self::new(StorageHealthConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_health() -> StorageHealth {
        StorageHealth::new(StorageHealthConfig::default())
    }

    #[test]
    fn test_new_health_with_no_devices_status_offline() {
        let health = create_test_health();
        assert_eq!(health.status(), StorageHealthStatus::Offline);
    }

    #[test]
    fn test_register_one_device_device_count_1() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        let snapshot = health.snapshot();
        assert_eq!(snapshot.device_count, 1);
    }

    #[test]
    fn test_healthy_device_status_healthy() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 900);

        assert_eq!(health.status(), StorageHealthStatus::Healthy);
    }

    #[test]
    fn test_device_with_wear_100_status_degraded_or_critical() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_wear(
            0,
            WearSnapshot {
                wear_percentage_used: 100,
                power_on_hours: 10000,
            },
        );

        let status = health.status();
        assert!(status == StorageHealthStatus::Degraded || status == StorageHealthStatus::Critical);
    }

    #[test]
    fn test_device_score_below_critical_threshold_status_critical() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 0);
        health.update_device_wear(
            0,
            WearSnapshot {
                wear_percentage_used: 100,
                power_on_hours: 10000,
            },
        );

        assert_eq!(health.status(), StorageHealthStatus::Critical);
    }

    #[test]
    fn test_multiple_healthy_devices_status_healthy() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.register_device(1, "/dev/nvme1".to_string());
        health.update_device_capacity(0, 1000, 900);
        health.update_device_capacity(1, 1000, 800);

        assert_eq!(health.status(), StorageHealthStatus::Healthy);
    }

    #[test]
    fn test_one_critical_device_among_healthy_status_critical() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.register_device(1, "/dev/nvme1".to_string());
        health.update_device_capacity(0, 1000, 900);
        health.update_device_capacity(1, 1000, 0);
        health.update_device_wear(
            1,
            WearSnapshot {
                wear_percentage_used: 100,
                power_on_hours: 10000,
            },
        );

        assert_eq!(health.status(), StorageHealthStatus::Critical);
    }

    #[test]
    fn test_is_healthy_returns_true_only_for_healthy() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 900);

        let snapshot = health.snapshot();
        assert!(snapshot.is_healthy());

        health.update_device_capacity(0, 1000, 0);
        let snapshot = health.snapshot();
        assert!(!snapshot.is_healthy());
    }

    #[test]
    fn test_overall_score_1_0_with_one_perfect_device() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 1000);

        let snapshot = health.snapshot();
        assert!((snapshot.overall_score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_overall_score_0_0_with_one_failed_device() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 0);
        health.update_device_wear(
            0,
            WearSnapshot {
                wear_percentage_used: 100,
                power_on_hours: 10000,
            },
        );

        let snapshot = health.snapshot();
        assert!(snapshot.overall_score < 0.3);
    }

    #[test]
    fn test_overall_score_average_of_two_device_scores() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.register_device(1, "/dev/nvme1".to_string());
        health.update_device_capacity(0, 1000, 1000);
        health.update_device_capacity(1, 1000, 500);

        let snapshot = health.snapshot();
        assert!((snapshot.overall_score - 0.75).abs() < 0.15);
    }

    #[test]
    fn test_scrub_0_errors_no_scrub_health_event() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 900);
        health.update_scrub_stats(ScrubStats::default());

        let alerts = health.active_alerts();
        assert!(!alerts.iter().any(|a| a.source == "scrub"));
    }

    #[test]
    fn test_scrub_1_error_warning_health_event() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 900);

        let scrub_stats = ScrubStats {
            errors_detected: 1,
            ..Default::default()
        };
        health.update_scrub_stats(scrub_stats);

        let alerts = health.active_alerts();
        let scrub_alert = alerts.iter().find(|a| a.source == "scrub");
        assert!(scrub_alert.is_some());
        assert_eq!(scrub_alert.unwrap().severity, HealthEventSeverity::Warning);
    }

    #[test]
    fn test_scrub_10_errors_critical_health_event() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 900);

        let scrub_stats = ScrubStats {
            errors_detected: 10,
            ..Default::default()
        };
        health.update_scrub_stats(scrub_stats);

        let alerts = health.active_alerts();
        let scrub_alert = alerts.iter().find(|a| a.source == "scrub");
        assert!(scrub_alert.is_some());
        assert_eq!(scrub_alert.unwrap().severity, HealthEventSeverity::Critical);
    }

    #[test]
    fn test_scrub_critical_plus_healthy_devices_overall_status_critical() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 900);

        let scrub_stats = ScrubStats {
            errors_detected: 10,
            ..Default::default()
        };
        health.update_scrub_stats(scrub_stats);

        assert_eq!(health.status(), StorageHealthStatus::Critical);
    }

    #[test]
    fn test_active_alerts_returns_device_alerts() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 100);

        let alerts = health.active_alerts();
        assert!(!alerts.is_empty());
        assert!(alerts.iter().any(|a| a.source.starts_with("device:")));
    }

    #[test]
    fn test_device_media_errors_alert_critical_health_event() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 900);
        health.update_device_smart(
            0,
            SmartSnapshot {
                reallocated_sectors: 0,
                media_errors: 5,
                unsafe_shutdowns: 0,
                temperature_celsius: 40,
                percentage_used: 10,
            },
        );

        let alerts = health.active_alerts();
        assert!(alerts.iter().any(|a| {
            a.severity == HealthEventSeverity::Critical
                && a.source == "device:0"
                && a.message.contains("media errors")
        }));
    }

    #[test]
    fn test_healthy_device_count_correct_for_mixed_devices() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.register_device(1, "/dev/nvme1".to_string());
        health.register_device(2, "/dev/nvme2".to_string());
        health.update_device_capacity(0, 1000, 900);
        health.update_device_capacity(1, 1000, 500);
        health.update_device_capacity(2, 1000, 100);

        let snapshot = health.snapshot();
        assert_eq!(snapshot.healthy_device_count, 1);
    }

    #[test]
    fn test_degraded_device_count_correct_for_mixed_devices() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.register_device(1, "/dev/nvme1".to_string());
        health.update_device_capacity(0, 1000, 900);
        health.update_device_wear(
            1,
            WearSnapshot {
                wear_percentage_used: 50,
                power_on_hours: 1000,
            },
        );
        health.update_device_capacity(1, 1000, 400);

        let snapshot = health.snapshot();
        assert!(snapshot.degraded_device_count >= 1);
    }

    #[test]
    fn test_critical_device_count_correct_for_mixed_devices() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.register_device(1, "/dev/nvme1".to_string());
        health.update_device_capacity(0, 1000, 900);
        health.update_device_capacity(1, 1000, 0);
        health.update_device_wear(
            1,
            WearSnapshot {
                wear_percentage_used: 100,
                power_on_hours: 10000,
            },
        );

        let snapshot = health.snapshot();
        assert_eq!(snapshot.critical_device_count, 1);
    }

    #[test]
    fn test_snapshot_timestamp_secs_is_nonzero() {
        let mut health = create_test_health();
        health.register_device(0, "/dev/nvme0".to_string());
        health.update_device_capacity(0, 1000, 900);

        let snapshot = health.snapshot();
        assert!(snapshot.snapshot_timestamp_secs > 0);
    }

    #[test]
    fn test_default_config_has_degraded_threshold_0_6() {
        let config = StorageHealthConfig::default();
        assert!((config.degraded_threshold - 0.6).abs() < 0.001);
    }
}
