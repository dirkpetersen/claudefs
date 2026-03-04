//! Device health monitoring for storage devices.
//!
//! Aggregates SMART data, wear level, and capacity into unified health scores and alerts.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Errors from the device health monitor.
#[derive(Debug, Error)]
pub enum HealthMonitorError {
    #[error("Device not registered: {0}")]
    DeviceNotFound(u16),
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// SMART data snapshot from a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartSnapshot {
    /// Number of reallocated sectors.
    pub reallocated_sectors: u32,
    /// Total media errors.
    pub media_errors: u64,
    /// Number of unsafe shutdowns.
    pub unsafe_shutdowns: u32,
    /// Temperature in Celsius.
    pub temperature_celsius: u8,
    /// Percentage of write endurance used (0-100).
    pub percentage_used: u8,
}

/// Wear level snapshot from a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WearSnapshot {
    /// Percentage of write endurance used (0-100).
    pub wear_percentage_used: u8,
    /// Power-on hours.
    pub power_on_hours: u64,
}

/// Health alert type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthAlertType {
    /// Free capacity below 20%.
    LowCapacity,
    /// Wear above 80%.
    HighWear,
    /// Media errors detected.
    MediaErrors,
    /// Temperature above 70°C.
    HighTemperature,
    /// Overall health score below 0.3.
    CriticalHealth,
}

impl HealthAlertType {
    /// Returns display name for this alert type.
    pub fn display_name(&self) -> &'static str {
        match self {
            HealthAlertType::LowCapacity => "LowCapacity",
            HealthAlertType::HighWear => "HighWear",
            HealthAlertType::MediaErrors => "MediaErrors",
            HealthAlertType::HighTemperature => "HighTemperature",
            HealthAlertType::CriticalHealth => "CriticalHealth",
        }
    }
}

/// Alert severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Warning-level alert.
    Warning,
    /// Critical-level alert.
    Critical,
}

/// A health alert for a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthAlert {
    /// Device index.
    pub device_idx: u16,
    /// Type of alert.
    pub alert_type: HealthAlertType,
    /// Severity level.
    pub severity: AlertSeverity,
    /// Human-readable message.
    pub message: String,
}

/// Summary of device health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceHealthSummary {
    /// Device index.
    pub device_idx: u16,
    /// Device path.
    pub device_path: String,
    /// Overall health score (0.0-1.0).
    pub health_score: f64,
    /// Percentage of capacity free.
    pub capacity_pct_free: f64,
    /// Percentage of wear used.
    pub wear_pct_used: u8,
    /// Temperature in Celsius.
    pub temperature_celsius: u8,
    /// Last update timestamp (unix secs).
    pub last_updated: u64,
}

/// Device data storage.
#[derive(Debug, Clone)]
struct DeviceData {
    path: String,
    smart: Option<SmartSnapshot>,
    wear: Option<WearSnapshot>,
    total_bytes: u64,
    free_bytes: u64,
    last_updated: u64,
}

impl DeviceData {
    fn new(path: String) -> Self {
        Self {
            path,
            smart: None,
            wear: None,
            total_bytes: 0,
            free_bytes: 0,
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    fn compute_health_score(&self) -> f64 {
        let mut total_weight = 0.0f64;
        let mut weighted_score = 0.0f64;

        if self.wear.is_some() {
            let w = self.compute_wear_score();
            weighted_score += w * 0.35;
            total_weight += 0.35;
        }

        if self.total_bytes > 0 {
            let c = self.compute_capacity_score();
            weighted_score += c * 0.35;
            total_weight += 0.35;
        }

        if self.smart.is_some() {
            let s = self.compute_smart_score();
            weighted_score += s * 0.30;
            total_weight += 0.30;
        }

        if total_weight == 0.0 {
            1.0
        } else {
            weighted_score / total_weight
        }
    }

    fn compute_wear_score(&self) -> f64 {
        match &self.wear {
            Some(w) => (100.0 - w.wear_percentage_used as f64) / 100.0,
            None => 1.0,
        }
    }

    fn compute_capacity_score(&self) -> f64 {
        if self.total_bytes == 0 {
            return 1.0;
        }
        (self.free_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
    }

    fn compute_smart_score(&self) -> f64 {
        let Some(smart) = &self.smart else {
            return 1.0;
        };

        let mut score: f64 = 1.0;

        if smart.media_errors > 0 {
            score -= 0.3;
        }
        if smart.reallocated_sectors > 0 {
            score -= 0.2;
        }
        if smart.percentage_used > 80 {
            score -= 0.2;
        }

        score.max(0.0)
    }

    fn to_summary(&self, health_score: f64) -> DeviceHealthSummary {
        let capacity_pct = if self.total_bytes > 0 {
            (self.free_bytes as f64 / self.total_bytes as f64) * 100.0
        } else {
            100.0
        };

        DeviceHealthSummary {
            device_idx: 0,
            device_path: self.path.clone(),
            health_score,
            capacity_pct_free: capacity_pct,
            wear_pct_used: self
                .wear
                .as_ref()
                .map(|w| w.wear_percentage_used)
                .unwrap_or(0),
            temperature_celsius: self
                .smart
                .as_ref()
                .map(|s| s.temperature_celsius)
                .unwrap_or(0),
            last_updated: self.last_updated,
        }
    }
}

/// Device health monitor.
///
/// Aggregates SMART data, wear level, and capacity into unified health scores and alerts.
#[derive(Debug)]
pub struct DeviceHealthMonitor {
    devices: BTreeMap<u16, DeviceData>,
}

impl DeviceHealthMonitor {
    /// Creates a new device health monitor.
    pub fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
        }
    }

    /// Registers a device for monitoring.
    pub fn register_device(&mut self, device_idx: u16, device_path: String) {
        self.devices
            .insert(device_idx, DeviceData::new(device_path));
    }

    /// Updates SMART data for a device.
    pub fn update_smart(
        &mut self,
        device_idx: u16,
        smart: SmartSnapshot,
    ) -> Result<(), HealthMonitorError> {
        let device = self
            .devices
            .get_mut(&device_idx)
            .ok_or(HealthMonitorError::DeviceNotFound(device_idx))?;
        device.smart = Some(smart);
        device.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(())
    }

    /// Updates wear level data for a device.
    pub fn update_wear(
        &mut self,
        device_idx: u16,
        wear: WearSnapshot,
    ) -> Result<(), HealthMonitorError> {
        let device = self
            .devices
            .get_mut(&device_idx)
            .ok_or(HealthMonitorError::DeviceNotFound(device_idx))?;
        device.wear = Some(wear);
        device.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(())
    }

    /// Updates capacity information for a device.
    pub fn update_capacity(
        &mut self,
        device_idx: u16,
        total_bytes: u64,
        free_bytes: u64,
    ) -> Result<(), HealthMonitorError> {
        let device = self
            .devices
            .get_mut(&device_idx)
            .ok_or(HealthMonitorError::DeviceNotFound(device_idx))?;
        device.total_bytes = total_bytes;
        device.free_bytes = free_bytes;
        device.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(())
    }

    /// Computes health score for a device (0.0 to 1.0).
    pub fn compute_health_score(&self, device_idx: u16) -> Result<f64, HealthMonitorError> {
        let device = self
            .devices
            .get(&device_idx)
            .ok_or(HealthMonitorError::DeviceNotFound(device_idx))?;
        Ok(device.compute_health_score())
    }

    /// Returns health summary for all devices, sorted by score ascending.
    pub fn health_summary(&self) -> Vec<DeviceHealthSummary> {
        let mut summaries: Vec<DeviceHealthSummary> = self
            .devices
            .iter()
            .map(|(idx, data)| {
                let score = data.compute_health_score();
                let mut summary = data.to_summary(score);
                summary.device_idx = *idx;
                summary
            })
            .collect();

        summaries.sort_by(|a, b| {
            a.health_score
                .partial_cmp(&b.health_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        summaries
    }

    /// Returns alerts for devices below thresholds.
    pub fn check_alerts(&self) -> Vec<HealthAlert> {
        let mut alerts = Vec::new();

        for (idx, data) in &self.devices {
            let health_score = data.compute_health_score();
            let capacity_pct = if data.total_bytes > 0 {
                (data.free_bytes as f64 / data.total_bytes as f64) * 100.0
            } else {
                100.0
            };
            let wear_pct = data
                .wear
                .as_ref()
                .map(|w| w.wear_percentage_used)
                .unwrap_or(0);
            let temp = data
                .smart
                .as_ref()
                .map(|s| s.temperature_celsius)
                .unwrap_or(0);
            let media_errors = data.smart.as_ref().map(|s| s.media_errors).unwrap_or(0);

            if capacity_pct < 20.0 {
                alerts.push(HealthAlert {
                    device_idx: *idx,
                    alert_type: HealthAlertType::LowCapacity,
                    severity: if capacity_pct < 5.0 {
                        AlertSeverity::Critical
                    } else {
                        AlertSeverity::Warning
                    },
                    message: format!("Capacity at {:.1}% free (below 20%)", capacity_pct),
                });
            }

            if wear_pct > 80 {
                alerts.push(HealthAlert {
                    device_idx: *idx,
                    alert_type: HealthAlertType::HighWear,
                    severity: if wear_pct > 95 {
                        AlertSeverity::Critical
                    } else {
                        AlertSeverity::Warning
                    },
                    message: format!("Wear at {}% (above 80%)", wear_pct),
                });
            }

            if media_errors > 0 {
                alerts.push(HealthAlert {
                    device_idx: *idx,
                    alert_type: HealthAlertType::MediaErrors,
                    severity: AlertSeverity::Critical,
                    message: format!("{} media errors detected", media_errors),
                });
            }

            if temp > 70 {
                alerts.push(HealthAlert {
                    device_idx: *idx,
                    alert_type: HealthAlertType::HighTemperature,
                    severity: if temp > 80 {
                        AlertSeverity::Critical
                    } else {
                        AlertSeverity::Warning
                    },
                    message: format!("Temperature at {}°C (above 70°C)", temp),
                });
            }

            if health_score < 0.3 {
                alerts.push(HealthAlert {
                    device_idx: *idx,
                    alert_type: HealthAlertType::CriticalHealth,
                    severity: AlertSeverity::Critical,
                    message: format!("Health score {:.2} (below 0.3)", health_score),
                });
            }
        }

        alerts
    }
}

impl Default for DeviceHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_monitor_with_device() -> DeviceHealthMonitor {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0n1".to_string());
        monitor
    }

    #[test]
    fn test_register_device() {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0n1".to_string());

        let score = monitor.compute_health_score(0).unwrap();
        assert!((score - 1.0).abs() < 0.001, "Default should be healthy");
    }

    #[test]
    fn test_update_smart_no_errors() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_smart(
                0,
                SmartSnapshot {
                    reallocated_sectors: 0,
                    media_errors: 0,
                    unsafe_shutdowns: 0,
                    temperature_celsius: 40,
                    percentage_used: 10,
                },
            )
            .unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        assert!(score > 0.8, "No errors should give high score");
    }

    #[test]
    fn test_update_smart_with_media_errors() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_smart(
                0,
                SmartSnapshot {
                    reallocated_sectors: 0,
                    media_errors: 5,
                    unsafe_shutdowns: 0,
                    temperature_celsius: 40,
                    percentage_used: 10,
                },
            )
            .unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        assert!(score < 0.8, "Media errors should penalize score");
    }

    #[test]
    fn test_update_wear_0_percent() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 0,
                    power_on_hours: 100,
                },
            )
            .unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        assert!((score - 1.0).abs() < 0.01, "0% wear should give score 1.0");
    }

    #[test]
    fn test_update_wear_100_percent() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 100,
                    power_on_hours: 10000,
                },
            )
            .unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        assert!(
            (score - 0.0).abs() < 0.01,
            "100% wear should give score 0.0"
        );
    }

    #[test]
    fn test_update_wear_50_percent() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 50,
                    power_on_hours: 5000,
                },
            )
            .unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        assert!((score - 0.5).abs() < 0.1, "50% wear should give score ~0.5");
    }

    #[test]
    fn test_capacity_100_percent_free() {
        let mut monitor = create_monitor_with_device();
        monitor.update_capacity(0, 1000, 1000).unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        assert!(
            (score - 1.0).abs() < 0.01,
            "100% free should give score 1.0"
        );
    }

    #[test]
    fn test_capacity_0_percent_free() {
        let mut monitor = create_monitor_with_device();
        monitor.update_capacity(0, 1000, 0).unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        assert!((score - 0.0).abs() < 0.01, "0% free should give score 0.0");
    }

    #[test]
    fn test_capacity_50_percent_free() {
        let mut monitor = create_monitor_with_device();
        monitor.update_capacity(0, 1000, 500).unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        assert!((score - 0.5).abs() < 0.1, "50% free should give score ~0.5");
    }

    #[test]
    fn test_overall_health_weighted_average() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 50,
                    power_on_hours: 100,
                },
            )
            .unwrap();
        monitor.update_capacity(0, 1000, 800).unwrap();
        monitor
            .update_smart(
                0,
                SmartSnapshot {
                    reallocated_sectors: 0,
                    media_errors: 0,
                    unsafe_shutdowns: 0,
                    temperature_celsius: 40,
                    percentage_used: 10,
                },
            )
            .unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        let expected = (0.5 * 0.35) + (0.8 * 0.35) + (1.0 * 0.30);
        assert!((score - expected).abs() < 0.01);
    }

    #[test]
    fn test_health_summary_sorted() {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0".to_string());
        monitor.register_device(1, "/dev/nvme1".to_string());

        monitor.update_capacity(0, 1000, 100).unwrap();
        monitor.update_capacity(1, 1000, 800).unwrap();

        let summary = monitor.health_summary();
        assert_eq!(summary[0].device_idx, 0);
        assert_eq!(summary[1].device_idx, 1);
    }

    #[test]
    fn test_check_alerts_no_alerts_when_healthy() {
        let mut monitor = create_monitor_with_device();
        monitor.update_capacity(0, 1000, 900).unwrap();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 10,
                    power_on_hours: 100,
                },
            )
            .unwrap();
        monitor
            .update_smart(
                0,
                SmartSnapshot {
                    reallocated_sectors: 0,
                    media_errors: 0,
                    unsafe_shutdowns: 0,
                    temperature_celsius: 40,
                    percentage_used: 10,
                },
            )
            .unwrap();

        let alerts = monitor.check_alerts();
        assert!(alerts.is_empty(), "Healthy device should have no alerts");
    }

    #[test]
    fn test_check_alerts_low_capacity() {
        let mut monitor = create_monitor_with_device();
        monitor.update_capacity(0, 1000, 150).unwrap();

        let alerts = monitor.check_alerts();
        assert!(alerts
            .iter()
            .any(|a| a.alert_type == HealthAlertType::LowCapacity));
    }

    #[test]
    fn test_check_alerts_high_wear() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 85,
                    power_on_hours: 100,
                },
            )
            .unwrap();

        let alerts = monitor.check_alerts();
        assert!(alerts
            .iter()
            .any(|a| a.alert_type == HealthAlertType::HighWear));
    }

    #[test]
    fn test_check_alerts_media_errors() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_smart(
                0,
                SmartSnapshot {
                    reallocated_sectors: 0,
                    media_errors: 1,
                    unsafe_shutdowns: 0,
                    temperature_celsius: 40,
                    percentage_used: 10,
                },
            )
            .unwrap();

        let alerts = monitor.check_alerts();
        assert!(alerts
            .iter()
            .any(|a| a.alert_type == HealthAlertType::MediaErrors));
    }

    #[test]
    fn test_check_alerts_high_temperature() {
        let mut monitor = create_monitor_with_device();
        monitor
            .update_smart(
                0,
                SmartSnapshot {
                    reallocated_sectors: 0,
                    media_errors: 0,
                    unsafe_shutdowns: 0,
                    temperature_celsius: 75,
                    percentage_used: 10,
                },
            )
            .unwrap();

        let alerts = monitor.check_alerts();
        assert!(alerts
            .iter()
            .any(|a| a.alert_type == HealthAlertType::HighTemperature));
    }

    #[test]
    fn test_check_alerts_critical_health() {
        let mut monitor = create_monitor_with_device();
        monitor.update_capacity(0, 1000, 50).unwrap();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 90,
                    power_on_hours: 100,
                },
            )
            .unwrap();

        let alerts = monitor.check_alerts();
        assert!(alerts
            .iter()
            .any(|a| a.alert_type == HealthAlertType::CriticalHealth));
    }

    #[test]
    fn test_check_alerts_warning_vs_critical() {
        let mut monitor = create_monitor_with_device();

        monitor.update_capacity(0, 1000, 50).unwrap();
        let alerts = monitor.check_alerts();
        let cap_alert = alerts
            .iter()
            .find(|a| a.alert_type == HealthAlertType::LowCapacity)
            .unwrap();
        assert_eq!(cap_alert.severity, AlertSeverity::Warning);

        monitor.update_capacity(0, 1000, 50).unwrap();
        let mut monitor2 = DeviceHealthMonitor::new();
        monitor2.register_device(0, "/dev/nvme0".to_string());
        monitor2.update_capacity(0, 1000, 5).unwrap();
        let alerts2 = monitor2.check_alerts();
        let cap_alert2 = alerts2
            .iter()
            .find(|a| a.alert_type == HealthAlertType::LowCapacity)
            .unwrap();
        assert_eq!(cap_alert2.severity, AlertSeverity::Critical);
    }

    #[test]
    fn test_unregistered_device_error() {
        let monitor = DeviceHealthMonitor::new();

        let result = monitor.compute_health_score(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_devices_tracked_independently() {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0".to_string());
        monitor.register_device(1, "/dev/nvme1".to_string());

        monitor.update_capacity(0, 1000, 100).unwrap();
        monitor.update_capacity(1, 1000, 900).unwrap();

        let score0 = monitor.compute_health_score(0).unwrap();
        let score1 = monitor.compute_health_score(1).unwrap();

        assert!(score0 < score1);
    }

    #[test]
    fn test_default_health_score_without_data() {
        let mut monitor = create_monitor_with_device();

        let score = monitor.compute_health_score(0).unwrap();
        assert!(
            (score - 1.0).abs() < 0.001,
            "Default should be healthy without data"
        );
    }

    #[test]
    fn test_alert_display_names() {
        assert_eq!(HealthAlertType::LowCapacity.display_name(), "LowCapacity");
        assert_eq!(HealthAlertType::HighWear.display_name(), "HighWear");
        assert_eq!(HealthAlertType::MediaErrors.display_name(), "MediaErrors");
        assert_eq!(
            HealthAlertType::HighTemperature.display_name(),
            "HighTemperature"
        );
        assert_eq!(
            HealthAlertType::CriticalHealth.display_name(),
            "CriticalHealth"
        );
    }

    #[test]
    fn test_multiple_alerts_per_device() {
        let mut monitor = create_monitor_with_device();
        monitor.update_capacity(0, 1000, 50).unwrap();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 90,
                    power_on_hours: 100,
                },
            )
            .unwrap();
        monitor
            .update_smart(
                0,
                SmartSnapshot {
                    reallocated_sectors: 0,
                    media_errors: 1,
                    unsafe_shutdowns: 0,
                    temperature_celsius: 75,
                    percentage_used: 10,
                },
            )
            .unwrap();

        let alerts = monitor.check_alerts();
        assert!(alerts.len() >= 4, "Should have multiple alerts");
    }
}
