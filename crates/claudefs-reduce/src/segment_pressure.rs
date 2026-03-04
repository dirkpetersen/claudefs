/// Pressure level enumeration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PressureLevel {
    /// Under 60% - normal operation
    Normal,
    /// 60-80% - monitor closely
    Elevated,
    /// 80-95% - start evicting (D6 high watermark)
    High,
    /// 95-100% - write-through mode, alert admin (D6 critical)
    Critical,
}

impl PressureLevel {
    /// Returns the pressure level for a given fill percentage (0-100).
    pub fn from_pct(pct: u8) -> Self {
        match pct {
            0..=59 => PressureLevel::Normal,
            60..=79 => PressureLevel::Elevated,
            80..=94 => PressureLevel::High,
            _ => PressureLevel::Critical,
        }
    }

    /// Returns true if eviction should be triggered.
    pub fn should_evict(&self) -> bool {
        matches!(self, PressureLevel::High | PressureLevel::Critical)
    }

    /// Returns true if write-through mode should be enabled.
    pub fn write_through_required(&self) -> bool {
        matches!(self, PressureLevel::Critical)
    }
}

/// Configuration for segment pressure tracking.
#[derive(Debug, Clone)]
pub struct SegmentPressureConfig {
    pub total_capacity_bytes: u64,
    /// Alert if any single segment exceeds this fill percent
    pub per_segment_alert_pct: u8,
}

impl Default for SegmentPressureConfig {
    fn default() -> Self {
        Self {
            total_capacity_bytes: 100 * 1024 * 1024 * 1024, // 100 GB
            per_segment_alert_pct: 90,
        }
    }
}

/// Stats for pressure tracking.
#[derive(Debug, Clone, Default)]
pub struct PressureStats {
    pub total_updates: u64,
    pub high_pressure_events: u64,
    pub critical_pressure_events: u64,
    pub current_used_bytes: u64,
}

/// Tracks flash layer fill level and emits pressure signals per D6.
pub struct SegmentPressure {
    config: SegmentPressureConfig,
    used_bytes: u64,
    stats: PressureStats,
}

impl SegmentPressure {
    pub fn new(config: SegmentPressureConfig) -> Self {
        Self {
            config,
            used_bytes: 0,
            stats: PressureStats::default(),
        }
    }

    /// Record that `bytes` have been written to the flash layer.
    pub fn record_write(&mut self, bytes: u64) {
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        self.stats.total_updates += 1;
        self.stats.current_used_bytes = self.used_bytes;
        let level = self.current_level();
        if level == PressureLevel::High {
            self.stats.high_pressure_events += 1;
        } else if level == PressureLevel::Critical {
            self.stats.critical_pressure_events += 1;
        }
    }

    /// Record that `bytes` have been freed (eviction/deletion).
    pub fn record_free(&mut self, bytes: u64) {
        self.used_bytes = self.used_bytes.saturating_sub(bytes);
        self.stats.current_used_bytes = self.used_bytes;
        self.stats.total_updates += 1;
    }

    /// Returns fill percentage (0-100), clamped.
    pub fn fill_pct(&self) -> u8 {
        if self.config.total_capacity_bytes == 0 {
            return 100;
        }
        ((self.used_bytes * 100) / self.config.total_capacity_bytes).min(100) as u8
    }

    /// Returns the current pressure level.
    pub fn current_level(&self) -> PressureLevel {
        PressureLevel::from_pct(self.fill_pct())
    }

    /// Returns true if writes should be throttled.
    pub fn should_throttle(&self) -> bool {
        self.current_level().should_evict()
    }

    /// Returns used bytes.
    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    /// Returns stats.
    pub fn stats(&self) -> &PressureStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_level_normal() {
        assert_eq!(PressureLevel::from_pct(50), PressureLevel::Normal);
    }

    #[test]
    fn pressure_level_elevated() {
        assert_eq!(PressureLevel::from_pct(70), PressureLevel::Elevated);
    }

    #[test]
    fn pressure_level_high() {
        assert_eq!(PressureLevel::from_pct(85), PressureLevel::High);
    }

    #[test]
    fn pressure_level_critical() {
        assert_eq!(PressureLevel::from_pct(99), PressureLevel::Critical);
    }

    #[test]
    fn pressure_level_boundary_60() {
        assert_eq!(PressureLevel::from_pct(60), PressureLevel::Elevated);
    }

    #[test]
    fn pressure_level_boundary_80() {
        assert_eq!(PressureLevel::from_pct(80), PressureLevel::High);
    }

    #[test]
    fn pressure_level_boundary_95() {
        assert_eq!(PressureLevel::from_pct(95), PressureLevel::Critical);
    }

    #[test]
    fn should_evict_normal() {
        assert!(!PressureLevel::Normal.should_evict());
    }

    #[test]
    fn should_evict_elevated() {
        assert!(!PressureLevel::Elevated.should_evict());
    }

    #[test]
    fn should_evict_high() {
        assert!(PressureLevel::High.should_evict());
    }

    #[test]
    fn should_evict_critical() {
        assert!(PressureLevel::Critical.should_evict());
    }

    #[test]
    fn write_through_required_normal() {
        assert!(!PressureLevel::Normal.write_through_required());
    }

    #[test]
    fn write_through_required_critical() {
        assert!(PressureLevel::Critical.write_through_required());
    }

    #[test]
    fn segment_pressure_config_default() {
        let config = SegmentPressureConfig::default();
        assert_eq!(config.total_capacity_bytes, 100 * 1024 * 1024 * 1024);
        assert_eq!(config.per_segment_alert_pct, 90);
    }

    #[test]
    fn new_pressure_empty() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let pressure = SegmentPressure::new(config);
        assert_eq!(pressure.used_bytes(), 0);
        assert_eq!(pressure.fill_pct(), 0);
    }

    #[test]
    fn record_write_increases_used() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(100);
        assert_eq!(pressure.used_bytes(), 100);
    }

    #[test]
    fn record_free_decreases_used() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(100);
        pressure.record_free(50);
        assert_eq!(pressure.used_bytes(), 50);
    }

    #[test]
    fn record_free_saturates_at_zero() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(100);
        pressure.record_free(200);
        assert_eq!(pressure.used_bytes(), 0);
    }

    #[test]
    fn fill_pct_50_percent() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(500);
        assert_eq!(pressure.fill_pct(), 50);
    }

    #[test]
    fn fill_pct_100_percent() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(1000);
        assert_eq!(pressure.fill_pct(), 100);
    }

    #[test]
    fn current_level_transitions() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        assert_eq!(pressure.current_level(), PressureLevel::Normal);
        pressure.record_write(850);
        assert_eq!(pressure.current_level(), PressureLevel::High);
    }

    #[test]
    fn should_throttle_false_when_normal() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let pressure = SegmentPressure::new(config);
        assert!(!pressure.should_throttle());
    }

    #[test]
    fn should_throttle_true_at_high() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(850);
        assert!(pressure.should_throttle());
    }

    #[test]
    fn stats_high_pressure_events_count() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(850);
        assert_eq!(pressure.stats().high_pressure_events, 1);
    }

    #[test]
    fn stats_critical_pressure_events_count() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(960);
        assert_eq!(pressure.stats().critical_pressure_events, 1);
    }

    #[test]
    fn stats_total_updates() {
        let config = SegmentPressureConfig {
            total_capacity_bytes: 1000,
            per_segment_alert_pct: 90,
        };
        let mut pressure = SegmentPressure::new(config);
        pressure.record_write(100);
        pressure.record_free(50);
        assert_eq!(pressure.stats().total_updates, 2);
    }
}
