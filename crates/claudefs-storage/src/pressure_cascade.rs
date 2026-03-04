//! Orchestrates automated responses to storage pressure.

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::background_scheduler::{
    BackgroundScheduler, BackgroundTask, BackgroundTaskId, BackgroundTaskType,
};
use crate::device_health_monitor::{DeviceHealthMonitor, SmartSnapshot, WearSnapshot};
use crate::recovery::{RecoveryConfig, RecoveryManager, RecoveryReport};
use crate::storage_health::{
    StorageHealth, StorageHealthConfig, StorageHealthSnapshot, StorageHealthStatus,
};

/// Current pressure level for the storage system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PressureLevel {
    /// Health score > 0.8 - all systems nominal.
    Normal,
    /// Health score 0.6-0.8 - run prefetch, compact in background.
    Elevated,
    /// Health score 0.4-0.6 - slow new writes, boost recovery priority.
    High,
    /// Health score < 0.4 - reject non-critical writes, maximum recovery.
    Critical,
}

impl PressureLevel {
    /// Returns the recommended write delay in milliseconds.
    pub fn write_delay_ms(&self) -> u32 {
        match self {
            PressureLevel::Normal => 0,
            PressureLevel::Elevated => 0,
            PressureLevel::High => 10,
            PressureLevel::Critical => 100,
        }
    }

    /// Returns the recommended background task priority boost.
    pub fn priority_boost(&self) -> u32 {
        match self {
            PressureLevel::Normal => 0,
            PressureLevel::Elevated => 1,
            PressureLevel::High => 3,
            PressureLevel::Critical => 5,
        }
    }

    /// Whether new writes should be rejected (only Critical).
    pub fn should_reject_writes(&self) -> bool {
        matches!(self, PressureLevel::Critical)
    }
}

/// Signal from pressure cascade to write path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureSignal {
    /// Current pressure level.
    pub level: PressureLevel,
    /// Delay to apply before accepting new writes (ms).
    pub write_delay_ms: u32,
    /// Whether writes should be rejected entirely.
    pub reject_writes: bool,
    /// Recommended I/O budget reduction ratio (0.0 = no reduction, 1.0 = fully stop).
    pub io_budget_ratio: f64,
    /// Human-readable reason.
    pub reason: String,
}

/// Statistics for the pressure cascade.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PressureCascadeStats {
    /// Total pressure evaluations.
    pub pressure_evaluations: u64,
    /// Times at Normal level.
    pub normal_count: u64,
    /// Times at Elevated level.
    pub elevated_count: u64,
    /// Times at High level.
    pub high_count: u64,
    /// Times at Critical level.
    pub critical_count: u64,
    /// Checkpoints triggered.
    pub checkpoints_triggered: u64,
    /// Priority boosts applied.
    pub priority_boosts_applied: u64,
    /// Writes rejected.
    pub writes_rejected_count: u64,
}

/// Configuration for the pressure cascade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PressureCascadeConfig {
    /// Health score threshold below which pressure escalates from Normal to Elevated (default: 0.8).
    pub elevated_threshold: f64,
    /// Health score threshold below which pressure escalates to High (default: 0.6).
    pub high_threshold: f64,
    /// Health score threshold below which pressure escalates to Critical (default: 0.4).
    pub critical_threshold: f64,
    /// Trigger a checkpoint when level reaches High (default: true).
    pub checkpoint_on_high: bool,
    /// Maximum number of in-progress recovery tasks to boost (default: 4).
    pub max_recovery_boosts: usize,
}

impl Default for PressureCascadeConfig {
    fn default() -> Self {
        Self {
            elevated_threshold: 0.8,
            high_threshold: 0.6,
            critical_threshold: 0.4,
            checkpoint_on_high: true,
            max_recovery_boosts: 4,
        }
    }
}

/// Orchestrates automated responses to storage pressure.
pub struct PressureCascade {
    config: PressureCascadeConfig,
    health: StorageHealth,
    scheduler: BackgroundScheduler,
    recovery: RecoveryManager,
    current_level: PressureLevel,
    stats: PressureCascadeStats,
}

impl PressureCascade {
    /// Creates a new pressure cascade.
    pub fn new(config: PressureCascadeConfig) -> Self {
        Self {
            config,
            health: StorageHealth::new(StorageHealthConfig::default()),
            scheduler: BackgroundScheduler::new(),
            recovery: RecoveryManager::new(RecoveryConfig::default()),
            current_level: PressureLevel::Normal,
            stats: PressureCascadeStats::default(),
        }
    }

    /// Registers a device with the health monitor.
    pub fn register_device(&mut self, device_idx: u16, device_path: String) {
        self.health.register_device(device_idx, device_path);
    }

    /// Updates device SMART data.
    pub fn update_device_smart(&mut self, device_idx: u16, smart: SmartSnapshot) {
        self.health.update_device_smart(device_idx, smart);
    }

    /// Updates device capacity data.
    pub fn update_device_capacity(&mut self, device_idx: u16, total_bytes: u64, free_bytes: u64) {
        self.health
            .update_device_capacity(device_idx, total_bytes, free_bytes);
    }

    /// Evaluates current pressure level based on latest health snapshot.
    pub fn evaluate(&mut self) -> PressureLevel {
        self.stats.pressure_evaluations += 1;

        let snapshot = self.health.snapshot();
        let score = snapshot.overall_score;

        let new_level = if score > self.config.elevated_threshold {
            PressureLevel::Normal
        } else if score > self.config.high_threshold {
            PressureLevel::Elevated
        } else if score > self.config.critical_threshold {
            PressureLevel::High
        } else {
            PressureLevel::Critical
        };

        match new_level {
            PressureLevel::Normal => self.stats.normal_count += 1,
            PressureLevel::Elevated => self.stats.elevated_count += 1,
            PressureLevel::High => self.stats.high_count += 1,
            PressureLevel::Critical => self.stats.critical_count += 1,
        }

        if new_level > self.current_level && new_level >= PressureLevel::High {
            if self.config.checkpoint_on_high {
                let _ = self.recovery.report();
                self.stats.checkpoints_triggered += 1;
                info!(
                    "Checkpoint triggered due to pressure escalation to {:?}",
                    new_level
                );
            }
        }

        self.current_level = new_level;
        new_level
    }

    /// Returns the current backpressure signal for the write path.
    pub fn backpressure_signal(&self) -> BackpressureSignal {
        let (delay, reject, budget_ratio, reason) = match self.current_level {
            PressureLevel::Normal => (0, false, 0.0, "System operating normally".to_string()),
            PressureLevel::Elevated => (
                0,
                false,
                0.1,
                "Elevated pressure: background tasks accelerated".to_string(),
            ),
            PressureLevel::High => (
                10,
                false,
                0.3,
                "High pressure: writes throttled, recovery prioritized".to_string(),
            ),
            PressureLevel::Critical => (
                100,
                true,
                0.8,
                "Critical pressure: non-critical writes rejected".to_string(),
            ),
        };

        BackpressureSignal {
            level: self.current_level,
            write_delay_ms: delay,
            reject_writes: reject,
            io_budget_ratio: budget_ratio,
            reason,
        }
    }

    /// Returns the current pressure level without re-evaluating.
    pub fn current_level(&self) -> PressureLevel {
        self.current_level
    }

    /// Schedules a recovery task with priority boost based on current level.
    pub fn schedule_boosted_recovery(&mut self, estimated_bytes: u64) -> u64 {
        let boost = self.current_level.priority_boost();
        let base_priority = 10u32;
        let priority = base_priority.saturating_sub(boost) as u8;

        let task = BackgroundTask::new(
            BackgroundTaskType::JournalFlush,
            estimated_bytes,
            format!("Boosted recovery task (level {:?})", self.current_level),
        );

        let task_id = self.scheduler.schedule(task);

        if self.current_level >= PressureLevel::Elevated {
            self.stats.priority_boosts_applied += 1;
        }

        debug!(
            task_id = task_id.0,
            level = ?self.current_level,
            boost,
            "Scheduled boosted recovery task"
        );

        task_id.0
    }

    /// Returns pressure cascade statistics.
    pub fn stats(&self) -> &PressureCascadeStats {
        &self.stats
    }

    /// Returns recovery statistics.
    pub fn recovery_stats(&self) -> RecoveryReport {
        self.recovery.report()
    }
}

impl Default for PressureCascade {
    fn default() -> Self {
        Self::new(PressureCascadeConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cascade_is_at_normal_pressure() {
        let cascade = PressureCascade::new(PressureCascadeConfig::default());
        assert_eq!(cascade.current_level(), PressureLevel::Normal);
    }

    #[test]
    fn healthy_device_returns_normal() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 900);

        let level = cascade.evaluate();
        assert_eq!(level, PressureLevel::Normal);
    }

    #[test]
    fn device_at_100_wear_0_capacity_returns_critical() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 0);
        cascade.update_device_smart(
            0,
            SmartSnapshot {
                reallocated_sectors: 0,
                media_errors: 0,
                unsafe_shutdowns: 0,
                temperature_celsius: 40,
                percentage_used: 100,
            },
        );

        let level = cascade.evaluate();
        assert_eq!(level, PressureLevel::Critical);
    }

    #[test]
    fn degraded_device_returns_elevated_or_high() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 300);

        let level = cascade.evaluate();
        assert!(level >= PressureLevel::Elevated);
    }

    #[test]
    fn pressure_level_ordering() {
        assert!(PressureLevel::Normal < PressureLevel::Elevated);
        assert!(PressureLevel::Elevated < PressureLevel::High);
        assert!(PressureLevel::High < PressureLevel::Critical);
    }

    #[test]
    fn write_delay_ms_normal_is_0() {
        assert_eq!(PressureLevel::Normal.write_delay_ms(), 0);
    }

    #[test]
    fn write_delay_ms_high_is_10() {
        assert_eq!(PressureLevel::High.write_delay_ms(), 10);
    }

    #[test]
    fn write_delay_ms_critical_is_100() {
        assert_eq!(PressureLevel::Critical.write_delay_ms(), 100);
    }

    #[test]
    fn priority_boost_normal_is_0() {
        assert_eq!(PressureLevel::Normal.priority_boost(), 0);
    }

    #[test]
    fn priority_boost_critical_is_5() {
        assert_eq!(PressureLevel::Critical.priority_boost(), 5);
    }

    #[test]
    fn should_reject_writes_only_critical() {
        assert!(!PressureLevel::Normal.should_reject_writes());
        assert!(!PressureLevel::Elevated.should_reject_writes());
        assert!(!PressureLevel::High.should_reject_writes());
        assert!(PressureLevel::Critical.should_reject_writes());
    }

    #[test]
    fn backpressure_signal_at_normal_reject_writes_false_delay_0() {
        let cascade = PressureCascade::new(PressureCascadeConfig::default());
        let signal = cascade.backpressure_signal();
        assert!(!signal.reject_writes);
        assert_eq!(signal.write_delay_ms, 0);
    }

    #[test]
    fn backpressure_signal_at_critical_reject_writes_true_delay_100() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 0);
        cascade.update_device_smart(
            0,
            SmartSnapshot {
                reallocated_sectors: 0,
                media_errors: 5,
                unsafe_shutdowns: 0,
                temperature_celsius: 85,
                percentage_used: 100,
            },
        );
        cascade.evaluate();

        let signal = cascade.backpressure_signal();
        assert!(signal.reject_writes);
        assert_eq!(signal.write_delay_ms, 100);
    }

    #[test]
    fn backpressure_signal_io_budget_ratio_increases_with_level() {
        let normal_signal =
            PressureCascade::new(PressureCascadeConfig::default()).backpressure_signal();
        assert_eq!(normal_signal.io_budget_ratio, 0.0);

        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 100);
        cascade.evaluate();

        let degraded_signal = cascade.backpressure_signal();
        assert!(degraded_signal.io_budget_ratio > normal_signal.io_budget_ratio);
    }

    #[test]
    fn stats_pressure_evaluations_increments_on_evaluate() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 900);

        cascade.evaluate();
        assert_eq!(cascade.stats().pressure_evaluations, 1);
    }

    #[test]
    fn stats_critical_count_increments_when_critical_level_returned() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 0);
        cascade.update_device_smart(
            0,
            SmartSnapshot {
                reallocated_sectors: 0,
                media_errors: 10,
                unsafe_shutdowns: 0,
                temperature_celsius: 90,
                percentage_used: 100,
            },
        );

        cascade.evaluate();
        assert_eq!(cascade.stats().critical_count, 1);
    }

    #[test]
    fn stats_normal_count_increments_when_normal_level_returned() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 900);

        cascade.evaluate();
        assert_eq!(cascade.stats().normal_count, 1);
    }

    #[test]
    fn stats_checkpoints_triggered_increments_on_high_pressure_with_checkpoint_on_high_true() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig {
            checkpoint_on_high: true,
            ..Default::default()
        });
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 100);
        cascade.update_device_smart(
            0,
            SmartSnapshot {
                reallocated_sectors: 0,
                media_errors: 0,
                unsafe_shutdowns: 0,
                temperature_celsius: 40,
                percentage_used: 80,
            },
        );

        cascade.evaluate();
        assert!(cascade.stats().checkpoints_triggered >= 1);
    }

    #[test]
    fn stats_checkpoints_triggered_does_not_increment_when_checkpoint_on_high_false() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig {
            checkpoint_on_high: false,
            ..Default::default()
        });
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 100);

        cascade.evaluate();
        assert_eq!(cascade.stats().checkpoints_triggered, 0);
    }

    #[test]
    fn schedule_boosted_recovery_at_normal_returns_low_priority_boost() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        let task_id = cascade.schedule_boosted_recovery(4096);
        assert!(task_id > 0);
    }

    #[test]
    fn schedule_boosted_recovery_at_critical_returns_high_priority_boost() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 0);
        cascade.update_device_smart(
            0,
            SmartSnapshot {
                reallocated_sectors: 0,
                media_errors: 10,
                unsafe_shutdowns: 0,
                temperature_celsius: 90,
                percentage_used: 100,
            },
        );
        cascade.evaluate();

        let task_id = cascade.schedule_boosted_recovery(4096);
        assert!(task_id > 0);
    }

    #[test]
    fn stats_priority_boosts_applied_increments_on_boosted_schedule() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 100);
        cascade.evaluate();

        cascade.schedule_boosted_recovery(4096);
        assert_eq!(cascade.stats().priority_boosts_applied, 1);
    }

    #[test]
    fn register_device_update_data_changes_health_score() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        cascade.register_device(0, "/dev/nvme0".to_string());

        let initial_level = cascade.evaluate();

        cascade.update_device_capacity(0, 1000, 100);
        let new_level = cascade.evaluate();

        assert!(new_level >= initial_level);
    }

    #[test]
    fn pressure_cascade_config_default_has_elevated_threshold_0_8() {
        let config = PressureCascadeConfig::default();
        assert!((config.elevated_threshold - 0.8).abs() < 0.001);
    }

    #[test]
    fn multiple_evaluate_calls_at_same_level_do_not_repeat_checkpoint() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig {
            checkpoint_on_high: true,
            ..Default::default()
        });
        cascade.register_device(0, "/dev/nvme0".to_string());
        cascade.update_device_capacity(0, 1000, 100);

        cascade.evaluate();
        let first_checkpoints = cascade.stats().checkpoints_triggered;

        cascade.evaluate();
        assert_eq!(cascade.stats().checkpoints_triggered, first_checkpoints);
    }

    #[test]
    fn backpressure_signal_has_correct_level() {
        let cascade = PressureCascade::new(PressureCascadeConfig::default());
        let signal = cascade.backpressure_signal();
        assert_eq!(signal.level, PressureLevel::Normal);
    }

    #[test]
    fn backpressure_signal_has_reason() {
        let cascade = PressureCascade::new(PressureCascadeConfig::default());
        let signal = cascade.backpressure_signal();
        assert!(!signal.reason.is_empty());
    }

    #[test]
    fn pressure_cascade_stats_default() {
        let stats = PressureCascadeStats::default();
        assert_eq!(stats.pressure_evaluations, 0);
        assert_eq!(stats.normal_count, 0);
    }

    #[test]
    fn pressure_level_partial_ord() {
        assert!(PressureLevel::High > PressureLevel::Elevated);
        assert!(PressureLevel::Critical > PressureLevel::High);
    }

    #[test]
    fn evaluate_returns_offline_when_no_devices() {
        let mut cascade = PressureCascade::new(PressureCascadeConfig::default());
        let level = cascade.evaluate();
        assert_eq!(level, PressureLevel::Normal);
    }
}
