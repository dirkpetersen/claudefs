//! FDP (Flexible Data Placement) hint manager.
//!
//! Per hardware.md: FDP allows tagging writes with placement hints that tell
//! the NVMe SSD where to place data internally, reducing write amplification
//! by grouping writes with similar lifetimes.

use crate::block::PlacementHint;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::debug;

/// Maps a ClaudeFS PlacementHint to an NVMe FDP Reclaim Unit Handle (RUH).
/// Each RUH groups writes with similar expected lifetimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FdpHandle {
    /// The Reclaim Unit Handle index (0-based)
    pub ruh_index: u16,
    /// The placement hint this handle maps to
    pub hint: PlacementHint,
}

/// FDP Configuration for a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FdpConfig {
    /// Whether FDP is enabled on this device
    pub enabled: bool,
    /// Number of available Reclaim Unit Handles on the device
    pub num_ruh: u16,
    /// Mapping from PlacementHint to RUH index
    pub hint_mapping: Vec<(PlacementHint, u16)>,
}

impl Default for FdpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            num_ruh: 6,
            hint_mapping: vec![
                (PlacementHint::Metadata, 0),
                (PlacementHint::HotData, 1),
                (PlacementHint::WarmData, 2),
                (PlacementHint::ColdData, 3),
                (PlacementHint::Snapshot, 4),
                (PlacementHint::Journal, 5),
            ],
        }
    }
}

/// FDP statistics for monitoring write distribution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FdpStats {
    /// Writes per RUH index
    pub writes_per_ruh: Vec<(u16, u64)>,
    /// Bytes written per RUH index
    pub bytes_per_ruh: Vec<(u16, u64)>,
    /// Total FDP-tagged writes
    pub total_fdp_writes: u64,
    /// Total non-FDP writes (fallback)
    pub total_fallback_writes: u64,
}

struct FdpInner {
    hint_to_ruh: HashMap<PlacementHint, u16>,
    writes_per_ruh: HashMap<u16, u64>,
    bytes_per_ruh: HashMap<u16, u64>,
    total_fdp_writes: u64,
    total_fallback_writes: u64,
}

impl FdpInner {
    fn new(config: &FdpConfig) -> Self {
        let hint_to_ruh: HashMap<PlacementHint, u16> =
            config.hint_mapping.iter().cloned().collect();

        Self {
            hint_to_ruh,
            writes_per_ruh: HashMap::new(),
            bytes_per_ruh: HashMap::new(),
            total_fdp_writes: 0,
            total_fallback_writes: 0,
        }
    }
}

/// FDP Hint Manager.
///
/// Manages the mapping from ClaudeFS PlacementHints to NVMe FDP Reclaim Unit Handles,
/// and tracks statistics for monitoring write distribution across RUHs.
pub struct FdpHintManager {
    config: FdpConfig,
    inner: Mutex<FdpInner>,
}

impl FdpHintManager {
    /// Creates a new FDP Hint Manager with the given configuration.
    pub fn new(config: FdpConfig) -> Self {
        let inner = FdpInner::new(&config);
        debug!(
            enabled = config.enabled,
            num_ruh = config.num_ruh,
            "FDP Hint Manager initialized"
        );
        Self {
            config,
            inner: Mutex::new(inner),
        }
    }

    /// Maps a placement hint to an FDP handle.
    /// Returns None if FDP is disabled or the hint is not mapped.
    pub fn resolve_hint(&self, hint: PlacementHint) -> Option<FdpHandle> {
        if !self.config.enabled {
            return None;
        }

        let inner = self.inner.lock().unwrap();
        inner
            .hint_to_ruh
            .get(&hint)
            .map(|&ruh_index| FdpHandle { ruh_index, hint })
    }

    /// Records a write for statistics tracking.
    /// If FDP is disabled or hint is not mapped, records as fallback.
    pub fn record_write(&self, hint: PlacementHint, bytes: u64) {
        let mut inner = self.inner.lock().unwrap();

        if self.config.enabled {
            if let Some(&ruh_index) = inner.hint_to_ruh.get(&hint) {
                *inner.writes_per_ruh.entry(ruh_index).or_insert(0) += 1;
                *inner.bytes_per_ruh.entry(ruh_index).or_insert(0) += bytes;
                inner.total_fdp_writes += 1;
                debug!(hint = ?hint, ruh_index = ruh_index, bytes = bytes, "FDP write recorded");
            } else {
                inner.total_fallback_writes += 1;
                debug!(hint = ?hint, "Fallback write (unmapped hint)");
            }
        } else {
            inner.total_fallback_writes += 1;
            debug!(hint = ?hint, bytes = bytes, "Fallback write (FDP disabled)");
        }
    }

    /// Gets the RUH index for a given placement hint.
    /// Returns None if FDP is disabled or hint is not mapped.
    pub fn ruh_for_hint(&self, hint: PlacementHint) -> Option<u16> {
        if !self.config.enabled {
            return None;
        }

        let inner = self.inner.lock().unwrap();
        inner.hint_to_ruh.get(&hint).copied()
    }

    /// Returns current FDP statistics.
    pub fn stats(&self) -> FdpStats {
        let inner = self.inner.lock().unwrap();

        let mut writes_per_ruh: Vec<(u16, u64)> =
            inner.writes_per_ruh.iter().map(|(&k, &v)| (k, v)).collect();
        writes_per_ruh.sort_by_key(|(idx, _)| *idx);

        let mut bytes_per_ruh: Vec<(u16, u64)> =
            inner.bytes_per_ruh.iter().map(|(&k, &v)| (k, v)).collect();
        bytes_per_ruh.sort_by_key(|(idx, _)| *idx);

        FdpStats {
            writes_per_ruh,
            bytes_per_ruh,
            total_fdp_writes: inner.total_fdp_writes,
            total_fallback_writes: inner.total_fallback_writes,
        }
    }

    /// Returns whether FDP is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

/// Creates a disabled FDP manager (for non-FDP devices).
pub fn disabled() -> FdpHintManager {
    FdpHintManager::new(FdpConfig {
        enabled: false,
        num_ruh: 0,
        hint_mapping: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fdp_default_mapping() {
        let manager = FdpHintManager::new(FdpConfig::default());
        let config = FdpConfig::default();

        assert!(manager.is_enabled());
        assert_eq!(config.num_ruh, 6);
        assert_eq!(config.hint_mapping.len(), 6);

        assert_eq!(config.hint_mapping[0], (PlacementHint::Metadata, 0));
        assert_eq!(config.hint_mapping[1], (PlacementHint::HotData, 1));
        assert_eq!(config.hint_mapping[2], (PlacementHint::WarmData, 2));
        assert_eq!(config.hint_mapping[3], (PlacementHint::ColdData, 3));
        assert_eq!(config.hint_mapping[4], (PlacementHint::Snapshot, 4));
        assert_eq!(config.hint_mapping[5], (PlacementHint::Journal, 5));
    }

    #[test]
    fn test_resolve_hint() {
        let manager = FdpHintManager::new(FdpConfig::default());

        let handle = manager.resolve_hint(PlacementHint::Metadata).unwrap();
        assert_eq!(handle.ruh_index, 0);
        assert_eq!(handle.hint, PlacementHint::Metadata);

        let handle = manager.resolve_hint(PlacementHint::HotData).unwrap();
        assert_eq!(handle.ruh_index, 1);

        let handle = manager.resolve_hint(PlacementHint::WarmData).unwrap();
        assert_eq!(handle.ruh_index, 2);

        let handle = manager.resolve_hint(PlacementHint::ColdData).unwrap();
        assert_eq!(handle.ruh_index, 3);

        let handle = manager.resolve_hint(PlacementHint::Snapshot).unwrap();
        assert_eq!(handle.ruh_index, 4);

        let handle = manager.resolve_hint(PlacementHint::Journal).unwrap();
        assert_eq!(handle.ruh_index, 5);
    }

    #[test]
    fn test_disabled_fdp() {
        let manager = disabled();

        assert!(!manager.is_enabled());
        assert!(manager.resolve_hint(PlacementHint::Metadata).is_none());
        assert!(manager.resolve_hint(PlacementHint::HotData).is_none());
        assert!(manager.ruh_for_hint(PlacementHint::ColdData).is_none());
    }

    #[test]
    fn test_record_write_stats() {
        let manager = FdpHintManager::new(FdpConfig::default());

        manager.record_write(PlacementHint::Metadata, 4096);
        manager.record_write(PlacementHint::Metadata, 4096);
        manager.record_write(PlacementHint::HotData, 65536);
        manager.record_write(PlacementHint::ColdData, 1048576);

        let stats = manager.stats();

        assert_eq!(stats.total_fdp_writes, 4);
        assert_eq!(stats.total_fallback_writes, 0);

        let metadata_writes: u64 = stats
            .writes_per_ruh
            .iter()
            .find(|(idx, _)| *idx == 0)
            .map(|(_, c)| *c)
            .unwrap();
        assert_eq!(metadata_writes, 2);

        let hot_writes: u64 = stats
            .writes_per_ruh
            .iter()
            .find(|(idx, _)| *idx == 1)
            .map(|(_, c)| *c)
            .unwrap();
        assert_eq!(hot_writes, 1);
    }

    #[test]
    fn test_custom_config() {
        let config = FdpConfig {
            enabled: true,
            num_ruh: 3,
            hint_mapping: vec![
                (PlacementHint::Metadata, 0),
                (PlacementHint::HotData, 1),
                (PlacementHint::ColdData, 2),
            ],
        };
        let manager = FdpHintManager::new(config);

        let handle = manager.resolve_hint(PlacementHint::Metadata).unwrap();
        assert_eq!(handle.ruh_index, 0);

        let handle = manager.resolve_hint(PlacementHint::HotData).unwrap();
        assert_eq!(handle.ruh_index, 1);

        let handle = manager.resolve_hint(PlacementHint::ColdData).unwrap();
        assert_eq!(handle.ruh_index, 2);
    }

    #[test]
    fn test_unmapped_hint() {
        let config = FdpConfig {
            enabled: true,
            num_ruh: 2,
            hint_mapping: vec![(PlacementHint::Metadata, 0), (PlacementHint::HotData, 1)],
        };
        let manager = FdpHintManager::new(config);

        assert!(manager.resolve_hint(PlacementHint::WarmData).is_none());
        assert!(manager.resolve_hint(PlacementHint::ColdData).is_none());
        assert!(manager.resolve_hint(PlacementHint::Snapshot).is_none());
        assert!(manager.resolve_hint(PlacementHint::Journal).is_none());

        assert!(manager.ruh_for_hint(PlacementHint::WarmData).is_none());
    }

    #[test]
    fn test_fdp_stats_accumulation() {
        let manager = FdpHintManager::new(FdpConfig::default());

        for _ in 0..100 {
            manager.record_write(PlacementHint::Metadata, 4096);
        }
        for _ in 0..50 {
            manager.record_write(PlacementHint::HotData, 65536);
        }

        let stats = manager.stats();

        let metadata_writes: u64 = stats
            .writes_per_ruh
            .iter()
            .find(|(idx, _)| *idx == 0)
            .map(|(_, c)| *c)
            .unwrap();
        assert_eq!(metadata_writes, 100);

        let metadata_bytes: u64 = stats
            .bytes_per_ruh
            .iter()
            .find(|(idx, _)| *idx == 0)
            .map(|(_, c)| *c)
            .unwrap();
        assert_eq!(metadata_bytes, 100 * 4096);

        let hot_writes: u64 = stats
            .writes_per_ruh
            .iter()
            .find(|(idx, _)| *idx == 1)
            .map(|(_, c)| *c)
            .unwrap();
        assert_eq!(hot_writes, 50);
    }

    #[test]
    fn test_disabled_records_fallback() {
        let manager = disabled();

        manager.record_write(PlacementHint::Metadata, 4096);
        manager.record_write(PlacementHint::HotData, 65536);

        let stats = manager.stats();
        assert_eq!(stats.total_fallback_writes, 2);
        assert_eq!(stats.total_fdp_writes, 0);
    }
}
