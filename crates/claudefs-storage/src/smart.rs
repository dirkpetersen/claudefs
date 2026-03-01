//! NVMe SMART health monitoring for predictive failure detection.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single SMART attribute from NVMe health log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartAttribute {
    /// SMART attribute ID.
    pub id: u8,
    /// Human-readable attribute name.
    pub name: String,
    /// Current attribute value.
    pub value: u64,
    /// Warning threshold.
    pub threshold: u64,
    /// Worst observed value.
    pub worst: u64,
}

/// NVMe SMART/Health Information Log.
/// Represents the data structure returned by the Get Log Page command (SMART/Health Information).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeSmartLog {
    /// Critical warning bitfield.
    /// Bit 0: Temperature threshold
    /// Bit 1: Available spare below threshold
    /// Bit 2: NVM subsystem reliability compromised
    /// Bit 3: Media in read-only mode
    /// Bit 4: Volatile backup failed
    pub critical_warning: u8,
    /// Composite temperature in Kelvin.
    pub temperature_kelvin: u16,
    /// Available spare capacity percentage (0-100).
    pub available_spare_pct: u8,
    /// Available spare threshold percentage.
    pub available_spare_threshold: u8,
    /// Percentage of endurance used (0-255, can exceed 100).
    pub percent_used: u8,
    /// Data units read (in 1000 * 512-byte units).
    pub data_units_read: u128,
    /// Data units written (in 1000 * 512-byte units).
    pub data_units_written: u128,
    /// Total number of host read commands.
    pub host_read_commands: u128,
    /// Total number of host write commands.
    pub host_write_commands: u128,
    /// Power-on hours.
    pub power_on_hours: u64,
    /// Number of unsafe shutdowns.
    pub unsafe_shutdowns: u64,
    /// Number of media errors.
    pub media_errors: u64,
    /// Number of error log entries.
    pub error_log_entries: u64,
}

impl NvmeSmartLog {
    /// Convert temperature from Kelvin to Celsius.
    pub fn temperature_celsius(&self) -> f64 {
        (self.temperature_kelvin as f64) - 273.15
    }

    /// Check if any critical warning bit is set.
    pub fn is_critical(&self) -> bool {
        self.critical_warning != 0
    }

    /// Check if available spare is above threshold.
    pub fn spare_ok(&self) -> bool {
        self.available_spare_pct >= self.available_spare_threshold
    }

    /// Check if percent_used is below 100 (endurance not exhausted).
    pub fn endurance_ok(&self) -> bool {
        self.percent_used < 100
    }

    /// Convert data_units_read to terabytes.
    pub fn data_read_tb(&self) -> f64 {
        (self.data_units_read as f64) * 512.0 / 1_000_000_000.0
    }

    /// Convert data_units_written to terabytes.
    pub fn data_written_tb(&self) -> f64 {
        (self.data_units_written as f64) * 512.0 / 1_000_000_000.0
    }
}

/// Health status of an NVMe device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    /// All parameters nominal.
    Healthy,
    /// Approaching thresholds, warning level.
    Warning {
        /// List of warning reasons.
        reasons: Vec<String>,
    },
    /// Immediate attention needed.
    Critical {
        /// List of critical failure reasons.
        reasons: Vec<String>,
    },
    /// Device has failed.
    Failed {
        /// List of failure reasons.
        reasons: Vec<String>,
    },
}

/// Configuration for SMART monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartMonitorConfig {
    /// How often to poll SMART data (seconds).
    pub poll_interval_secs: u64,
    /// Temperature warning threshold (Celsius).
    pub temp_warning_celsius: f64,
    /// Temperature critical threshold (Celsius).
    pub temp_critical_celsius: f64,
    /// Spare capacity warning threshold (percentage).
    pub spare_warning_pct: u8,
    /// Endurance warning threshold (percentage).
    pub endurance_warning_pct: u8,
}

impl Default for SmartMonitorConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 60,
            temp_warning_celsius: 70.0,
            temp_critical_celsius: 80.0,
            spare_warning_pct: 20,
            endurance_warning_pct: 80,
        }
    }
}

/// Severity level for SMART alerts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Informational message.
    Info,
    /// Warning condition.
    Warning,
    /// Critical condition requiring immediate attention.
    Critical,
}

/// A SMART health alert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartAlert {
    /// Device name.
    pub device: String,
    /// Alert severity.
    pub severity: AlertSeverity,
    /// Alert message.
    pub message: String,
    /// Unix timestamp in seconds.
    pub timestamp_secs: u64,
}

/// Monitor for NVMe SMART health data.
pub struct SmartMonitor {
    config: SmartMonitorConfig,
    devices: HashMap<String, NvmeSmartLog>,
    alerts: Vec<SmartAlert>,
}

impl SmartMonitor {
    /// Create a new SMART monitor with the given configuration.
    pub fn new(config: SmartMonitorConfig) -> Self {
        tracing::info!(
            "SMART monitor initialized with poll interval {}s",
            config.poll_interval_secs
        );
        Self {
            config,
            devices: HashMap::new(),
            alerts: Vec::new(),
        }
    }

    /// Update SMART data for a device.
    pub fn update_device(&mut self, device_name: &str, log: NvmeSmartLog) {
        let is_new = !self.devices.contains_key(device_name);
        self.devices.insert(device_name.to_string(), log);

        if is_new {
            tracing::info!("Added device {} to SMART monitoring", device_name);
        }
    }

    /// Evaluate health of a specific device.
    pub fn evaluate_health(&self, device_name: &str) -> Option<HealthStatus> {
        let log = self.devices.get(device_name)?;
        Some(self.evaluate_log(log))
    }

    fn evaluate_log(&self, log: &NvmeSmartLog) -> HealthStatus {
        if log.is_critical() {
            let mut reasons = Vec::new();
            if log.critical_warning & 0x01 != 0 {
                reasons.push("Temperature critical".to_string());
            }
            if log.critical_warning & 0x02 != 0 {
                reasons.push("Available spare below threshold".to_string());
            }
            if log.critical_warning & 0x04 != 0 {
                reasons.push("NVM subsystem reliability compromised".to_string());
            }
            if log.critical_warning & 0x08 != 0 {
                reasons.push("Media in read-only mode".to_string());
            }
            if log.critical_warning & 0x10 != 0 {
                reasons.push("Volatile backup failed".to_string());
            }
            return HealthStatus::Critical { reasons };
        }

        if !log.endurance_ok() {
            return HealthStatus::Failed {
                reasons: vec![format!("Endurance exhausted: {}% used", log.percent_used)],
            };
        }

        let mut warnings = Vec::new();

        let temp_celsius = log.temperature_celsius();
        if temp_celsius >= self.config.temp_critical_celsius {
            warnings.push(format!("Temperature critical: {:.1}°C", temp_celsius));
        } else if temp_celsius >= self.config.temp_warning_celsius {
            warnings.push(format!("Temperature high: {:.1}°C", temp_celsius));
        }

        if log.available_spare_pct < self.config.spare_warning_pct {
            warnings.push(format!("Low spare: {}%", log.available_spare_pct));
        }

        if log.percent_used >= self.config.endurance_warning_pct {
            warnings.push(format!("High endurance: {}% used", log.percent_used));
        }

        if log.media_errors > 0 {
            warnings.push(format!("Media errors: {}", log.media_errors));
        }

        if log.unsafe_shutdowns > 10 {
            warnings.push(format!("Unsafe shutdowns: {}", log.unsafe_shutdowns));
        }

        if warnings.is_empty() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Warning { reasons: warnings }
        }
    }

    /// Evaluate health of all monitored devices.
    pub fn evaluate_all(&self) -> HashMap<String, HealthStatus> {
        self.devices
            .keys()
            .filter_map(|name| {
                self.evaluate_health(name)
                    .map(|health| (name.clone(), health))
            })
            .collect()
    }

    /// Get latest SMART log for a device.
    pub fn get_log(&self, device_name: &str) -> Option<&NvmeSmartLog> {
        self.devices.get(device_name)
    }

    /// Number of monitored devices.
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Get all collected alerts.
    pub fn alerts(&self) -> &[SmartAlert] {
        &self.alerts
    }

    /// Clear all alerts.
    pub fn clear_alerts(&mut self) {
        self.alerts.clear();
        tracing::debug!("SMART alerts cleared");
    }

    /// Check device health and generate alerts if needed.
    pub fn check_and_alert(&mut self, device_name: &str) {
        if let Some(health) = self.evaluate_health(device_name) {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            match &health {
                HealthStatus::Healthy => {}
                HealthStatus::Warning { reasons } => {
                    for reason in reasons {
                        tracing::warn!("Device {} warning: {}", device_name, reason);
                        self.alerts.push(SmartAlert {
                            device: device_name.to_string(),
                            severity: AlertSeverity::Warning,
                            message: reason.clone(),
                            timestamp_secs: timestamp,
                        });
                    }
                }
                HealthStatus::Critical { reasons } => {
                    for reason in reasons {
                        tracing::warn!("Device {} critical: {}", device_name, reason);
                        self.alerts.push(SmartAlert {
                            device: device_name.to_string(),
                            severity: AlertSeverity::Critical,
                            message: reason.clone(),
                            timestamp_secs: timestamp,
                        });
                    }
                }
                HealthStatus::Failed { reasons } => {
                    for reason in reasons {
                        tracing::error!("Device {} failed: {}", device_name, reason);
                        self.alerts.push(SmartAlert {
                            device: device_name.to_string(),
                            severity: AlertSeverity::Critical,
                            message: reason.clone(),
                            timestamp_secs: timestamp,
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temperature_conversion() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 313, // 40°C
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!((log.temperature_celsius() - 39.85).abs() < 0.1);
    }

    #[test]
    fn test_temperature_conversion_zero_kelvin() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 273, // 0°C
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!((log.temperature_celsius() - (-0.15)).abs() < 0.1);
    }

    #[test]
    fn test_critical_warning_no_bits() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 313,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(!log.is_critical());
    }

    #[test]
    fn test_critical_warning_bit_0() {
        let log = NvmeSmartLog {
            critical_warning: 0x01,
            temperature_kelvin: 353, // 80°C
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(log.is_critical());
    }

    #[test]
    fn test_critical_warning_multiple_bits() {
        let log = NvmeSmartLog {
            critical_warning: 0x1F, // all bits set
            temperature_kelvin: 353,
            available_spare_pct: 5,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(log.is_critical());
    }

    #[test]
    fn test_spare_ok_above_threshold() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 313,
            available_spare_pct: 50,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(log.spare_ok());
    }

    #[test]
    fn test_spare_ok_below_threshold() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 313,
            available_spare_pct: 5,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(!log.spare_ok());
    }

    #[test]
    fn test_endurance_ok_under_100() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 313,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 50,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(log.endurance_ok());
    }

    #[test]
    fn test_endurance_ok_at_100() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 313,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 100,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(!log.endurance_ok());
    }

    #[test]
    fn test_data_read_tb_conversion() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 313,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 1_000_000, // 1M units * 512 bytes = ~512 GB
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        let tb = log.data_read_tb();
        assert!((tb - 0.512).abs() < 0.001);
    }

    #[test]
    fn test_data_written_tb_conversion() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 313,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 5,
            data_units_read: 0,
            data_units_written: 10_000_000, // 10M units * 512 bytes = ~5.12 TB
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        let tb = log.data_written_tb();
        assert!((tb - 5.12).abs() < 0.01);
    }

    #[test]
    fn test_healthy_device() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 303, // 30°C
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 10,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 100,
            unsafe_shutdowns: 2,
            media_errors: 0,
            error_log_entries: 0,
        };
        let monitor = SmartMonitor::new(SmartMonitorConfig::default());
        let health = monitor.evaluate_log(&log);
        matches!(health, HealthStatus::Healthy);
    }

    #[test]
    fn test_warning_high_temp() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 344, // ~71°C - above warning threshold of 70°C
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 10,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 100,
            unsafe_shutdowns: 2,
            media_errors: 0,
            error_log_entries: 0,
        };
        let config = SmartMonitorConfig::default();
        let monitor = SmartMonitor::new(config);
        let health = monitor.evaluate_log(&log);
        match health {
            HealthStatus::Warning { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("Temperature")));
            }
            _ => panic!("Expected Warning status"),
        }
    }

    #[test]
    fn test_critical_low_spare() {
        let log = NvmeSmartLog {
            critical_warning: 0x02, // spare bit
            temperature_kelvin: 313,
            available_spare_pct: 5,
            available_spare_threshold: 10,
            percent_used: 10,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 100,
            unsafe_shutdowns: 2,
            media_errors: 0,
            error_log_entries: 0,
        };
        let config = SmartMonitorConfig::default();
        let monitor = SmartMonitor::new(config);
        let health = monitor.evaluate_log(&log);
        match health {
            HealthStatus::Critical { reasons } => {
                assert!(reasons
                    .iter()
                    .any(|r| r.contains("spare") || r.contains("spare")));
            }
            _ => panic!("Expected Critical status"),
        }
    }

    #[test]
    fn test_multiple_devices() {
        let mut monitor = SmartMonitor::new(SmartMonitorConfig::default());

        monitor.update_device(
            "nvme0n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 303,
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 10,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );

        monitor.update_device(
            "nvme1n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 343, // hot
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 90, // high endurance
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );

        assert_eq!(monitor.device_count(), 2);

        let health = monitor.evaluate_health("nvme0n1").unwrap();
        matches!(health, HealthStatus::Healthy);

        let health = monitor.evaluate_health("nvme1n1").unwrap();
        match health {
            HealthStatus::Warning { reasons } => assert!(!reasons.is_empty()),
            _ => panic!("Expected Warning for nvme1n1"),
        }
    }

    #[test]
    fn test_alert_generation() {
        let mut monitor = SmartMonitor::new(SmartMonitorConfig::default());

        monitor.update_device(
            "nvme0n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 343,
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 90,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );

        monitor.check_and_alert("nvme0n1");

        assert!(!monitor.alerts().is_empty());
    }

    #[test]
    fn test_config_defaults() {
        let config = SmartMonitorConfig::default();
        assert_eq!(config.poll_interval_secs, 60);
        assert!((config.temp_warning_celsius - 70.0).abs() < 0.001);
        assert!((config.temp_critical_celsius - 80.0).abs() < 0.001);
        assert_eq!(config.spare_warning_pct, 20);
        assert_eq!(config.endurance_warning_pct, 80);
    }

    #[test]
    fn test_update_device_replaces_old() {
        let mut monitor = SmartMonitor::new(SmartMonitorConfig::default());

        monitor.update_device(
            "nvme0n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 303,
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 10,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );

        // Update with new data
        monitor.update_device(
            "nvme0n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 343, // hotter
                available_spare_pct: 50,
                available_spare_threshold: 10,
                percent_used: 20,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 200,
                unsafe_shutdowns: 3,
                media_errors: 0,
                error_log_entries: 0,
            },
        );

        assert_eq!(monitor.device_count(), 1);

        let log = monitor.get_log("nvme0n1").unwrap();
        assert_eq!(log.power_on_hours, 200);
    }

    #[test]
    fn test_evaluate_nonexistent() {
        let monitor = SmartMonitor::new(SmartMonitorConfig::default());
        let result = monitor.evaluate_health("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_device_count_tracking() {
        let mut monitor = SmartMonitor::new(SmartMonitorConfig::default());
        assert_eq!(monitor.device_count(), 0);

        monitor.update_device(
            "nvme0n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 303,
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 10,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );
        assert_eq!(monitor.device_count(), 1);

        monitor.update_device(
            "nvme1n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 303,
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 10,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );
        assert_eq!(monitor.device_count(), 2);
    }

    #[test]
    fn test_clear_alerts() {
        let mut monitor = SmartMonitor::new(SmartMonitorConfig::default());

        monitor.update_device(
            "nvme0n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 343,
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 90,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );

        monitor.check_and_alert("nvme0n1");
        assert!(!monitor.alerts().is_empty());

        monitor.clear_alerts();
        assert!(monitor.alerts().is_empty());
    }

    #[test]
    fn test_smart_log_with_all_critical_bits() {
        let log = NvmeSmartLog {
            critical_warning: 0x1F,
            temperature_kelvin: 400,
            available_spare_pct: 0,
            available_spare_threshold: 10,
            percent_used: 150,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(log.is_critical());
        assert!(!log.spare_ok());
        assert!(!log.endurance_ok());
    }

    #[test]
    fn test_endurance_exceeded() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 303,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 150, // > 100%
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 100,
            unsafe_shutdowns: 2,
            media_errors: 0,
            error_log_entries: 0,
        };

        let config = SmartMonitorConfig::default();
        let monitor = SmartMonitor::new(config);
        let health = monitor.evaluate_log(&log);

        match health {
            HealthStatus::Failed { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("150")));
            }
            _ => panic!("Expected Failed status"),
        }
    }

    #[test]
    fn test_combined_warning_reasons() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 353, // critical temp
            available_spare_pct: 5,  // low spare
            available_spare_threshold: 10,
            percent_used: 90, // high endurance
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 100,
            unsafe_shutdowns: 2,
            media_errors: 5, // some media errors
            error_log_entries: 0,
        };

        let config = SmartMonitorConfig::default();
        let monitor = SmartMonitor::new(config);
        let health = monitor.evaluate_log(&log);

        match health {
            HealthStatus::Warning { reasons } => {
                assert!(reasons.len() >= 3);
            }
            _ => panic!("Expected Warning status with multiple reasons"),
        }
    }

    #[test]
    fn test_media_errors_warning() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 303,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 10,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 100,
            unsafe_shutdowns: 2,
            media_errors: 1,
            error_log_entries: 0,
        };

        let config = SmartMonitorConfig::default();
        let monitor = SmartMonitor::new(config);
        let health = monitor.evaluate_log(&log);

        match health {
            HealthStatus::Warning { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("Media error")));
            }
            _ => panic!("Expected Warning for media errors"),
        }
    }

    #[test]
    fn test_unsafe_shutdowns_warning() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 303,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 10,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 100,
            unsafe_shutdowns: 15, // > 10
            media_errors: 0,
            error_log_entries: 0,
        };

        let config = SmartMonitorConfig::default();
        let monitor = SmartMonitor::new(config);
        let health = monitor.evaluate_log(&log);

        match health {
            HealthStatus::Warning { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("Unsafe shutdown")));
            }
            _ => panic!("Expected Warning for unsafe shutdowns"),
        }
    }

    #[test]
    fn test_evaluate_all_devices() {
        let mut monitor = SmartMonitor::new(SmartMonitorConfig::default());

        monitor.update_device(
            "nvme0n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 303,
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 10,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );

        monitor.update_device(
            "nvme1n1",
            NvmeSmartLog {
                critical_warning: 0,
                temperature_kelvin: 343,
                available_spare_pct: 100,
                available_spare_threshold: 10,
                percent_used: 90,
                data_units_read: 0,
                data_units_written: 0,
                host_read_commands: 0,
                host_write_commands: 0,
                power_on_hours: 100,
                unsafe_shutdowns: 2,
                media_errors: 0,
                error_log_entries: 0,
            },
        );

        let all = monitor.evaluate_all();

        assert_eq!(all.len(), 2);
        assert!(matches!(all.get("nvme0n1").unwrap(), HealthStatus::Healthy));
        assert!(matches!(
            all.get("nvme1n1").unwrap(),
            HealthStatus::Warning { .. }
        ));
    }
}
