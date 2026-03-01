//! NVMe Wear Leveling Awareness
//!
//! Tracks and optimizes NVMe flash cell wear to maximize drive lifetime.
//! Works with SMART monitoring and FDP hints to provide intelligent write placement.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::error::{StorageError, StorageResult};

/// Percentage of NAND wear used (0.0 - 100.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WearLevel {
    /// Percentage of drive life used (0.0 - 100.0)
    pub percentage_used: f64,
    /// Estimated P/E cycles remaining
    pub estimated_pe_cycles_remaining: u64,
    /// Total bytes written to the drive
    pub total_bytes_written: u64,
    /// Total bytes read from the drive
    pub total_bytes_read: u64,
    /// Write Amplification Factor (ratio of actual writes to host writes)
    pub write_amplification_factor: f64,
}

impl Default for WearLevel {
    fn default() -> Self {
        Self {
            percentage_used: 0.0,
            estimated_pe_cycles_remaining: 1000,
            total_bytes_written: 0,
            total_bytes_read: 0,
            write_amplification_factor: 1.0,
        }
    }
}

/// Wear state for a single zone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneWear {
    /// Zone identifier
    pub zone_id: u32,
    /// Wear level as percentage (0.0 - 100.0)
    pub wear_level: f64,
    /// Number of writes to this zone
    pub write_count: u64,
    /// Number of erases of this zone
    pub erase_count: u64,
    /// Unix timestamp of last write
    pub last_written_at: u64,
    /// Whether this zone is considered "hot" (high write frequency)
    pub is_hot: bool,
}

impl ZoneWear {
    /// Creates a new zone with zero wear
    pub fn new(zone_id: u32) -> Self {
        Self {
            zone_id,
            wear_level: 0.0,
            write_count: 0,
            erase_count: 0,
            last_written_at: 0,
            is_hot: false,
        }
    }
}

/// Configuration for wear leveling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WearConfig {
    /// Zone is "hot" if wear exceeds this percentage of max (default 80.0)
    pub hot_zone_threshold: f64,
    /// Redirect writes to zones below this percentage (default 20.0)
    pub cold_zone_target_pct: f64,
    /// Seconds between rebalance checks (default 3600)
    pub rebalance_interval_secs: u64,
    /// Alert if WAF exceeds this value (default 3.0)
    pub max_write_amplification: f64,
}

impl Default for WearConfig {
    fn default() -> Self {
        Self {
            hot_zone_threshold: 80.0,
            cold_zone_target_pct: 20.0,
            rebalance_interval_secs: 3600,
            max_write_amplification: 3.0,
        }
    }
}

/// Write pattern classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WritePattern {
    /// Sequential writes (append-only, predictable)
    Sequential,
    /// Random writes (scattered access patterns)
    Random,
    /// Mixed workload (combination of sequential and random)
    Mixed,
    /// Append-only (log-structured)
    Append,
}

/// Advice for write placement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementAdvice {
    /// Preferred zone for writes (None = no preference)
    pub preferred_zone: Option<u32>,
    /// Zones to avoid writing to
    pub avoid_zones: Vec<u32>,
    /// Detected write pattern
    pub pattern: WritePattern,
    /// Human-readable reason for the advice
    pub reason: String,
}

/// Type of wear-related alert
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WearAlertType {
    /// Zone has become too hot
    ZoneHot,
    /// Write amplification factor is too high
    HighWriteAmplification,
    /// Drive is approaching end of life
    EndOfLifeApproaching,
    /// Wear is unevenly distributed across zones
    WearImbalance,
}

/// Wear-related alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WearAlert {
    /// Zone that triggered the alert (None for global alerts)
    pub zone_id: u32,
    /// Type of alert
    pub alert_type: WearAlertType,
    /// Wear level at time of alert
    pub wear_level: f64,
    /// Alert message
    pub message: String,
    /// Unix timestamp when alert was generated
    pub timestamp_secs: u64,
}

/// Wear leveling engine - manages zone wear tracking and optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WearLevelingEngine {
    /// Zone wear information indexed by zone ID
    zones: HashMap<u32, ZoneWear>,
    /// Configuration
    config: WearConfig,
    /// Active alerts
    alerts: Vec<WearAlert>,
    /// Total bytes written across all zones
    total_writes: u64,
    /// Total erase operations across all zones
    total_erases: u64,
    /// Global wear level
    global_wear: WearLevel,
}

impl WearLevelingEngine {
    /// Creates a new wear leveling engine with the given configuration
    pub fn new(config: WearConfig) -> Self {
        Self {
            zones: HashMap::new(),
            config,
            alerts: Vec::new(),
            total_writes: 0,
            total_erases: 0,
            global_wear: WearLevel::default(),
        }
    }

    /// Registers a new zone with zero wear
    pub fn register_zone(&mut self, zone_id: u32) {
        let zone = ZoneWear::new(zone_id);
        debug!("Registering zone {} with zero wear", zone_id);
        self.zones.insert(zone_id, zone);
    }

    /// Records a write operation to a zone
    pub fn record_write(
        &mut self,
        zone_id: u32,
        bytes: u64,
        timestamp_secs: u64,
    ) -> StorageResult<()> {
        let zone = self
            .zones
            .get_mut(&zone_id)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("zone {}", zone_id),
                reason: "Zone not found".to_string(),
            })?;

        zone.write_count += 1;
        zone.last_written_at = timestamp_secs;

        let wear_increment = (bytes as f64) / (1024.0 * 1024.0 * 1024.0); // GB written
        let wear_per_write = wear_increment / 100.0; // Assume 100GB max per zone
        zone.wear_level = (zone.wear_level + wear_per_write).min(100.0);

        if zone.wear_level > self.config.hot_zone_threshold && !zone.is_hot {
            zone.is_hot = true;
            let alert = WearAlert {
                zone_id,
                alert_type: WearAlertType::ZoneHot,
                wear_level: zone.wear_level,
                message: format!(
                    "Zone {} wear level {:.1}% exceeds hot threshold {:.1}%",
                    zone_id, zone.wear_level, self.config.hot_zone_threshold
                ),
                timestamp_secs,
            };
            warn!("{}", alert.message);
            self.alerts.push(alert);
        }

        self.total_writes += bytes;
        self.update_global_wear();

        Ok(())
    }

    /// Records an erase operation on a zone
    pub fn record_erase(&mut self, zone_id: u32, timestamp_secs: u64) -> StorageResult<()> {
        let zone = self
            .zones
            .get_mut(&zone_id)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("zone {}", zone_id),
                reason: "Zone not found".to_string(),
            })?;

        zone.erase_count += 1;
        let wear_increment = 0.1; // Each erase adds some wear
        zone.wear_level = (zone.wear_level + wear_increment).min(100.0);

        self.total_erases += 1;

        debug!(
            "Zone {} erased, total erases: {}, wear: {:.1}%",
            zone_id, zone.erase_count, zone.wear_level
        );

        Ok(())
    }

    /// Gets placement advice for a write operation
    pub fn get_placement_advice(&self, bytes: u64, pattern: WritePattern) -> PlacementAdvice {
        let cold_zones: Vec<&ZoneWear> = self
            .zones
            .values()
            .filter(|z| z.wear_level < self.config.cold_zone_target_pct)
            .collect();

        let hot_zones: Vec<&ZoneWear> = self
            .zones
            .values()
            .filter(|z| z.is_hot || z.wear_level > self.config.hot_zone_threshold)
            .collect();

        let preferred = cold_zones
            .iter()
            .min_by(|a, b| a.wear_level.partial_cmp(&b.wear_level).unwrap())
            .map(|z| z.zone_id);

        let avoid: Vec<u32> = hot_zones.iter().map(|z| z.zone_id).collect();

        let reason = match (preferred.is_some(), avoid.is_empty()) {
            (true, true) => format!(
                "Found {} cold zones (< {:.1}% wear), {} hot zones to avoid",
                cold_zones.len(),
                self.config.cold_zone_target_pct,
                hot_zones.len()
            ),
            (true, false) => format!(
                "Found {} cold zones, avoiding {} hot zones",
                cold_zones.len(),
                avoid.len()
            ),
            (false, true) => "No cold zones available, using any available zone".to_string(),
            (false, false) => "All zones are hot, minimal choice available".to_string(),
        };

        PlacementAdvice {
            preferred_zone: preferred,
            avoid_zones: avoid,
            pattern,
            reason,
        }
    }

    /// Checks if wear is balanced across zones
    /// Returns Some(imbalance_pct) if max - min wear exceeds threshold, None otherwise
    pub fn check_wear_balance(&self) -> Option<f64> {
        if self.zones.is_empty() {
            return None;
        }

        let wear_levels: Vec<f64> = self.zones.values().map(|z| z.wear_level).collect();
        let max_wear = wear_levels.iter().cloned().fold(0.0_f64, f64::max);
        let min_wear = wear_levels.iter().cloned().fold(100.0_f64, f64::min);

        let imbalance = max_wear - min_wear;
        let threshold = 30.0; // Imbalance threshold percentage

        if imbalance > threshold {
            Some(imbalance)
        } else {
            None
        }
    }

    /// Gets zone wear information
    pub fn get_zone(&self, zone_id: u32) -> Option<&ZoneWear> {
        self.zones.get(&zone_id)
    }

    /// Returns zones above the hot threshold
    pub fn hot_zones(&self) -> Vec<&ZoneWear> {
        self.zones
            .values()
            .filter(|z| z.is_hot || z.wear_level > self.config.hot_zone_threshold)
            .collect()
    }

    /// Returns zones below the cold target
    pub fn cold_zones(&self) -> Vec<&ZoneWear> {
        self.zones
            .values()
            .filter(|z| z.wear_level < self.config.cold_zone_target_pct)
            .collect()
    }

    /// Recalculates global wear level from all zones
    pub fn update_global_wear(&mut self) {
        if self.zones.is_empty() {
            self.global_wear = WearLevel::default();
            return;
        }

        let total_wear: f64 = self.zones.values().map(|z| z.wear_level).sum();
        let zone_count = self.zones.len() as f64;
        let avg_wear = total_wear / zone_count;

        let max_pe_cycles = 1000u64;
        let pe_remaining = ((100.0 - avg_wear) / 100.0 * max_pe_cycles as f64) as u64;

        let waf = if self.total_writes > 0 {
            let host_writes = self.total_writes as f64;
            let nand_writes = self.total_erases as f64 * (1024.0 * 1024.0 * 1024.0); // Estimate
            if nand_writes > 0.0 {
                (nand_writes / host_writes).max(1.0)
            } else {
                1.0
            }
        } else {
            1.0
        };

        if waf > self.config.max_write_amplification {
            let alert = WearAlert {
                zone_id: 0,
                alert_type: WearAlertType::HighWriteAmplification,
                wear_level: avg_wear,
                message: format!(
                    "Write amplification factor {:.2} exceeds max {:.2}",
                    waf, self.config.max_write_amplification
                ),
                timestamp_secs: 0,
            };
            warn!("{}", alert.message);
            self.alerts.push(alert);
        }

        if avg_wear > 90.0 {
            let alert = WearAlert {
                zone_id: 0,
                alert_type: WearAlertType::EndOfLifeApproaching,
                wear_level: avg_wear,
                message: format!(
                    "Average wear {:.1}% approaching end of life (90%+)",
                    avg_wear
                ),
                timestamp_secs: 0,
            };
            warn!("{}", alert.message);
            self.alerts.push(alert);
        }

        self.global_wear = WearLevel {
            percentage_used: avg_wear,
            estimated_pe_cycles_remaining: pe_remaining,
            total_bytes_written: self.total_writes,
            total_bytes_read: 0,
            write_amplification_factor: waf,
        };
    }

    /// Returns wear statistics
    pub fn stats(&self) -> WearStats {
        let zone_list: Vec<&ZoneWear> = self.zones.values().collect();
        let total_zones = self.zones.len();

        let avg_wear_pct = if total_zones > 0 {
            zone_list.iter().map(|z| z.wear_level).sum::<f64>() / total_zones as f64
        } else {
            0.0
        };

        let max_wear_pct = zone_list
            .iter()
            .map(|z| z.wear_level)
            .fold(0.0_f64, f64::max);

        let min_wear_pct = if total_zones > 0 {
            zone_list
                .iter()
                .map(|z| z.wear_level)
                .fold(100.0_f64, f64::min)
        } else {
            0.0
        };

        let hot_count = zone_list
            .iter()
            .filter(|z| z.is_hot || z.wear_level > self.config.hot_zone_threshold)
            .count();

        let cold_count = zone_list
            .iter()
            .filter(|z| z.wear_level < self.config.cold_zone_target_pct)
            .count();

        WearStats {
            total_zones,
            hot_zones: hot_count,
            cold_zones: cold_count,
            avg_wear_pct,
            max_wear_pct,
            min_wear_pct,
            write_amplification: self.global_wear.write_amplification_factor,
            alerts_count: self.alerts.len(),
        }
    }

    /// Returns current alerts
    pub fn alerts(&self) -> &[WearAlert] {
        &self.alerts
    }

    /// Clears all alerts
    pub fn clear_alerts(&mut self) {
        info!("Clearing {} wear alerts", self.alerts.len());
        self.alerts.clear();
    }
}

/// Statistics summary for wear leveling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WearStats {
    /// Total number of zones
    pub total_zones: usize,
    /// Number of hot zones
    pub hot_zones: usize,
    /// Number of cold zones
    pub cold_zones: usize,
    /// Average wear percentage across all zones
    pub avg_wear_pct: f64,
    /// Maximum wear percentage across all zones
    pub max_wear_pct: f64,
    /// Minimum wear percentage across all zones
    pub min_wear_pct: f64,
    /// Current write amplification factor
    pub write_amplification: f64,
    /// Number of active alerts
    pub alerts_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wear_level_new() {
        let wear = WearLevel::default();
        assert_eq!(wear.percentage_used, 0.0);
        assert_eq!(wear.estimated_pe_cycles_remaining, 1000);
        assert_eq!(wear.total_bytes_written, 0);
        assert_eq!(wear.total_bytes_read, 0);
        assert_eq!(wear.write_amplification_factor, 1.0);
    }

    #[test]
    fn test_zone_wear_new() {
        let zone = ZoneWear::new(42);
        assert_eq!(zone.zone_id, 42);
        assert_eq!(zone.wear_level, 0.0);
        assert_eq!(zone.write_count, 0);
        assert_eq!(zone.erase_count, 0);
        assert_eq!(zone.last_written_at, 0);
        assert!(!zone.is_hot);
    }

    #[test]
    fn test_wear_config_defaults() {
        let config = WearConfig::default();
        assert_eq!(config.hot_zone_threshold, 80.0);
        assert_eq!(config.cold_zone_target_pct, 20.0);
        assert_eq!(config.rebalance_interval_secs, 3600);
        assert_eq!(config.max_write_amplification, 3.0);
    }

    #[test]
    fn test_register_zone() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        let zone = engine.get_zone(1).expect("Zone should exist");
        assert_eq!(zone.zone_id, 1);
        assert_eq!(zone.wear_level, 0.0);
    }

    #[test]
    fn test_register_multiple_zones() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        engine.register_zone(2);
        engine.register_zone(3);
        assert!(engine.get_zone(1).is_some());
        assert!(engine.get_zone(2).is_some());
        assert!(engine.get_zone(3).is_some());
        assert!(engine.get_zone(4).is_none());
    }

    #[test]
    fn test_record_write() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        engine.record_write(1, 1024 * 1024, 1000).unwrap();
        let zone = engine.get_zone(1).expect("Zone should exist");
        assert_eq!(zone.write_count, 1);
        assert!(zone.wear_level > 0.0);
    }

    #[test]
    fn test_record_write_unknown_zone() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        let result = engine.record_write(999, 1024, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_record_erase() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        engine.record_erase(1, 1000).unwrap();
        let zone = engine.get_zone(1).expect("Zone should exist");
        assert_eq!(zone.erase_count, 1);
    }

    #[test]
    fn test_hot_zone_detection() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 10.0;
        let mut engine = WearLevelingEngine::new(config);
        engine.register_zone(1);
        for _ in 0..20 {
            engine.record_write(1, 100 * 1024 * 1024, 1000).unwrap();
        }
        let zone = engine.get_zone(1).expect("Zone should exist");
        assert!(zone.is_hot);
    }

    #[test]
    fn test_cold_zones() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        engine.register_zone(2);
        engine.register_zone(3);
        let cold = engine.cold_zones();
        assert_eq!(cold.len(), 3);
    }

    #[test]
    fn test_placement_advice_prefers_cold() {
        let mut config = WearConfig::default();
        config.cold_zone_target_pct = 30.0;
        let mut engine = WearLevelingEngine::new(config);
        engine.register_zone(1);
        engine.register_zone(2);
        engine.register_zone(3);
        for _ in 0..100 {
            engine.record_write(1, 1024, 1000).unwrap();
        }
        let advice = engine.get_placement_advice(1024, WritePattern::Random);
        assert!(advice.preferred_zone.is_some());
        if let Some(zone) = advice.preferred_zone {
            assert_ne!(zone, 1);
        }
    }

    #[test]
    fn test_placement_advice_avoids_hot() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 10.0;
        let mut engine = WearLevelingEngine::new(config);
        engine.register_zone(1);
        engine.register_zone(2);
        for _ in 0..50 {
            engine.record_write(1, 1024 * 1024, 1000).unwrap();
        }
        let advice = engine.get_placement_advice(1024, WritePattern::Random);
        assert!(advice.avoid_zones.contains(&1));
    }

    #[test]
    fn test_placement_advice_sequential() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        let advice = engine.get_placement_advice(4096, WritePattern::Sequential);
        assert_eq!(advice.pattern, WritePattern::Sequential);
    }

    #[test]
    fn test_wear_balance_check_balanced() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        engine.register_zone(2);
        engine.record_write(1, 1024, 1000).unwrap();
        engine.record_write(2, 1024, 1000).unwrap();
        let imbalance = engine.check_wear_balance();
        assert!(imbalance.is_none());
    }

    #[test]
    fn test_wear_balance_check_imbalanced() {
        let config = WearConfig::default();
        let mut engine = WearLevelingEngine::new(config);
        engine.register_zone(1);
        engine.register_zone(2);
        for _ in 0..500 {
            engine.record_write(1, 1024 * 1024, 1000).unwrap();
        }
        let imbalance = engine.check_wear_balance();
        assert!(imbalance.is_some());
    }

    #[test]
    fn test_update_global_wear() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        engine.register_zone(2);
        engine.record_write(1, 1024, 1000).unwrap();
        engine.record_write(2, 2048, 1000).unwrap();
        engine.update_global_wear();
        assert!(engine.global_wear.percentage_used > 0.0);
    }

    #[test]
    fn test_write_amplification_tracking() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        engine.record_write(1, 1024, 1000).unwrap();
        engine.record_erase(1, 1001).unwrap();
        engine.update_global_wear();
        assert!(engine.global_wear.write_amplification_factor >= 1.0);
    }

    #[test]
    fn test_wear_alert_high_waf() {
        let mut config = WearConfig::default();
        config.max_write_amplification = 1.5;
        let mut engine = WearLevelingEngine::new(config);
        engine.register_zone(1);
        for _ in 0..10 {
            engine.record_write(1, 1024, 1000).unwrap();
            engine.record_erase(1, 1000).unwrap();
        }
        let alerts: Vec<_> = engine
            .alerts()
            .iter()
            .filter(|a| a.alert_type == WearAlertType::HighWriteAmplification)
            .collect();
        assert!(!alerts.is_empty());
    }

    #[test]
    fn test_wear_alert_hot_zone() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 5.0;
        let mut engine = WearLevelingEngine::new(config);
        engine.register_zone(1);
        for _ in 0..30 {
            engine.record_write(1, 1024 * 1024, 1000).unwrap();
        }
        let alerts: Vec<_> = engine
            .alerts()
            .iter()
            .filter(|a| a.alert_type == WearAlertType::ZoneHot)
            .collect();
        assert!(!alerts.is_empty());
    }

    #[test]
    fn test_clear_alerts() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 1.0;
        let mut engine = WearLevelingEngine::new(config);
        engine.register_zone(1);
        for _ in 0..10 {
            engine.record_write(1, 1024 * 1024 * 100, 1000).unwrap();
        }
        assert!(!engine.alerts().is_empty());
        engine.clear_alerts();
        assert!(engine.alerts().is_empty());
    }

    #[test]
    fn test_stats() {
        let mut engine = WearLevelingEngine::new(WearConfig::default());
        engine.register_zone(1);
        engine.register_zone(2);
        engine.record_write(1, 1024, 1000).unwrap();
        let stats = engine.stats();
        assert_eq!(stats.total_zones, 2);
        assert!(stats.avg_wear_pct >= 0.0);
    }

    #[test]
    fn test_end_of_life_approaching() {
        let mut config = WearConfig::default();
        let mut engine = WearLevelingEngine::new(config);
        engine.register_zone(1);
        for _ in 0..1000 {
            engine.record_write(1, 1024 * 1024 * 10, 1000).unwrap();
        }
        let alerts: Vec<_> = engine
            .alerts()
            .iter()
            .filter(|a| a.alert_type == WearAlertType::EndOfLifeApproaching)
            .collect();
        assert!(!alerts.is_empty());
    }
}
