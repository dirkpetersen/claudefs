//! ZNS (Zoned Namespace) support for NVMe Zoned Namespace Command Set.
//!
//! ZNS drives require sequential, append-only writes within zones.
//! This module provides zone management types and a zone manager for tracking
//! zone states and write pointers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{StorageError, StorageResult};

/// State of a ZNS zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ZoneState {
    /// Zone is empty and can accept writes
    #[default]
    Empty,
    /// Zone is partially written, write pointer is active
    Open,
    /// Zone has been explicitly closed but not full
    Closed,
    /// Zone is completely full
    Full,
    /// Zone is in read-only state
    ReadOnly,
    /// Zone is offline/unavailable
    Offline,
}

/// Describes a single zone on a ZNS device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneDescriptor {
    /// Zone index (0-based)
    pub zone_idx: u64,
    /// Zone start offset in 4KB blocks
    pub start_offset_4k: u64,
    /// Zone capacity in 4KB blocks
    pub capacity_4k: u64,
    /// Current write pointer offset (relative to zone start) in 4KB blocks
    pub write_pointer_4k: u64,
    /// Current zone state
    pub state: ZoneState,
}

impl ZoneDescriptor {
    /// Creates a new zone descriptor with the given parameters.
    pub fn new(zone_idx: u64, start_offset_4k: u64, capacity_4k: u64) -> Self {
        Self {
            zone_idx,
            start_offset_4k,
            capacity_4k,
            write_pointer_4k: 0,
            state: ZoneState::Empty,
        }
    }

    /// Returns the absolute write offset in 4KB blocks.
    pub fn write_offset_4k(&self) -> u64 {
        self.start_offset_4k + self.write_pointer_4k
    }

    /// Returns the number of free 4KB blocks in this zone.
    pub fn free_blocks_4k(&self) -> u64 {
        self.capacity_4k.saturating_sub(self.write_pointer_4k)
    }

    /// Returns true if this zone can accept more writes.
    pub fn is_writable(&self) -> bool {
        matches!(self.state, ZoneState::Empty | ZoneState::Open)
            && self.write_pointer_4k < self.capacity_4k
    }
}

/// Configuration for ZNS mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZnsConfig {
    /// Device index for the ZNS device
    pub device_idx: u16,
    /// Number of zones on the device
    pub num_zones: u64,
    /// Zone size in 4KB blocks
    pub zone_size_4k: u64,
    /// Maximum number of open zones
    pub max_open_zones: u32,
    /// Maximum number of active zones
    pub max_active_zones: u32,
}

impl ZnsConfig {
    /// Creates a new ZNS configuration.
    pub fn new(
        device_idx: u16,
        num_zones: u64,
        zone_size_4k: u64,
        max_open_zones: u32,
        max_active_zones: u32,
    ) -> Self {
        Self {
            device_idx,
            num_zones,
            zone_size_4k,
            max_open_zones,
            max_active_zones,
        }
    }

    /// Returns the total device capacity in 4KB blocks.
    pub fn total_capacity_4k(&self) -> u64 {
        self.num_zones * self.zone_size_4k
    }
}

/// Manages zones on a ZNS device.
/// Tracks zone states and write pointers.
pub struct ZoneManager {
    config: ZnsConfig,
    zones: Vec<ZoneDescriptor>,
}

impl ZoneManager {
    /// Create a new zone manager with the given configuration.
    /// Initializes all zones as Empty.
    pub fn new(config: ZnsConfig) -> Self {
        let mut zones = Vec::with_capacity(config.num_zones as usize);
        for i in 0..config.num_zones {
            let start_offset = i * config.zone_size_4k;
            zones.push(ZoneDescriptor::new(i, start_offset, config.zone_size_4k));
        }
        tracing::debug!(
            "ZoneManager created with {} zones, each {} 4KB blocks",
            config.num_zones,
            config.zone_size_4k
        );
        Self { config, zones }
    }

    /// Get a zone by index.
    pub fn zone(&self, zone_idx: u64) -> Option<&ZoneDescriptor> {
        self.zones.get(zone_idx as usize)
    }

    /// Get a mutable zone by index.
    fn zone_mut(&mut self, zone_idx: u64) -> Option<&mut ZoneDescriptor> {
        self.zones.get_mut(zone_idx as usize)
    }

    /// Find a zone in Empty or Open state for writing.
    /// Prefers Open zones (already started), then Empty zones.
    pub fn find_writable_zone(&self) -> Option<u64> {
        // First, try to find an already open zone with space
        for (idx, zone) in self.zones.iter().enumerate() {
            if zone.state == ZoneState::Open && zone.is_writable() {
                tracing::debug!("Found writable open zone {}", idx);
                return Some(idx as u64);
            }
        }
        // Then, try to find an empty zone
        for (idx, zone) in self.zones.iter().enumerate() {
            if zone.state == ZoneState::Empty {
                tracing::debug!("Found writable empty zone {}", idx);
                return Some(idx as u64);
            }
        }
        None
    }

    /// Record an append to a zone (advance write pointer).
    /// Returns the absolute offset where data was appended.
    pub fn append(&mut self, zone_idx: u64, blocks_4k: u64) -> StorageResult<u64> {
        let zone = self
            .zone_mut(zone_idx)
            .ok_or(StorageError::AllocatorError(format!(
                "Zone {} does not exist",
                zone_idx
            )))?;

        if !zone.is_writable() {
            return Err(StorageError::AllocatorError(format!(
                "Zone {} is not writable (state: {:?})",
                zone_idx, zone.state
            )));
        }

        let available = zone.capacity_4k - zone.write_pointer_4k;
        if blocks_4k > available {
            return Err(StorageError::AllocatorError(format!(
                "Zone {}: requested {} blocks but only {} available",
                zone_idx, blocks_4k, available
            )));
        }

        let write_offset = zone.write_offset_4k();
        zone.write_pointer_4k += blocks_4k;

        // Transition to Full if zone is now full
        // Transition to Open if zone was Empty
        if zone.state == ZoneState::Empty {
            zone.state = ZoneState::Open;
        }

        // Transition to Full if zone is now full
        if zone.write_pointer_4k >= zone.capacity_4k {
            zone.state = ZoneState::Full;
            tracing::debug!("Zone {} transitioned to Full", zone_idx);
        }

        tracing::debug!(
            "Appended {} blocks to zone {} at offset {}",
            blocks_4k,
            zone_idx,
            write_offset
        );
        Ok(write_offset)
    }

    /// Reset a zone to Empty state (zone reset command).
    pub fn reset_zone(&mut self, zone_idx: u64) -> StorageResult<()> {
        let zone = self
            .zone_mut(zone_idx)
            .ok_or(StorageError::AllocatorError(format!(
                "Zone {} does not exist",
                zone_idx
            )))?;

        // Can only reset Closed, Full, or ReadOnly zones
        if !matches!(
            zone.state,
            ZoneState::Closed | ZoneState::Full | ZoneState::ReadOnly
        ) {
            return Err(StorageError::AllocatorError(format!(
                "Cannot reset zone {} in state {:?}",
                zone_idx, zone.state
            )));
        }

        zone.state = ZoneState::Empty;
        zone.write_pointer_4k = 0;
        tracing::debug!("Zone {} reset to Empty", zone_idx);
        Ok(())
    }

    /// Finish a zone (transition Open -> Full).
    pub fn finish_zone(&mut self, zone_idx: u64) -> StorageResult<()> {
        let zone = self
            .zone_mut(zone_idx)
            .ok_or(StorageError::AllocatorError(format!(
                "Zone {} does not exist",
                zone_idx
            )))?;

        if zone.state != ZoneState::Open {
            return Err(StorageError::AllocatorError(format!(
                "Cannot finish zone {} in state {:?}",
                zone_idx, zone.state
            )));
        }

        zone.state = ZoneState::Full;
        tracing::debug!("Zone {} finished (Full)", zone_idx);
        Ok(())
    }

    /// Close a zone (transition Open -> Closed).
    pub fn close_zone(&mut self, zone_idx: u64) -> StorageResult<()> {
        let zone = self
            .zone_mut(zone_idx)
            .ok_or(StorageError::AllocatorError(format!(
                "Zone {} does not exist",
                zone_idx
            )))?;

        if zone.state != ZoneState::Open {
            return Err(StorageError::AllocatorError(format!(
                "Cannot close zone {} in state {:?}",
                zone_idx, zone.state
            )));
        }

        zone.state = ZoneState::Closed;
        tracing::debug!("Zone {} closed", zone_idx);
        Ok(())
    }

    /// Open a zone (transition Empty/Closed -> Open).
    pub fn open_zone(&mut self, zone_idx: u64) -> StorageResult<()> {
        let zone = self
            .zone_mut(zone_idx)
            .ok_or(StorageError::AllocatorError(format!(
                "Zone {} does not exist",
                zone_idx
            )))?;

        if !matches!(zone.state, ZoneState::Empty | ZoneState::Closed) {
            return Err(StorageError::AllocatorError(format!(
                "Cannot open zone {} in state {:?}",
                zone_idx, zone.state
            )));
        }

        zone.state = ZoneState::Open;
        tracing::debug!("Zone {} opened", zone_idx);
        Ok(())
    }

    /// Number of zones in each state.
    pub fn zone_state_counts(&self) -> Vec<(ZoneState, usize)> {
        let mut counts: HashMap<ZoneState, usize> = HashMap::new();
        for zone in &self.zones {
            *counts.entry(zone.state).or_insert(0) += 1;
        }
        let mut result: Vec<_> = counts.into_iter().collect();
        result.sort_by_key(|(state, _)| *state as u8);
        result
    }

    /// Total number of zones.
    pub fn num_zones(&self) -> u64 {
        self.zones.len() as u64
    }

    /// Returns zones that are Full and could be candidates for garbage collection.
    pub fn gc_candidates(&self) -> Vec<u64> {
        self.zones
            .iter()
            .filter(|z| z.state == ZoneState::Full)
            .map(|z| z.zone_idx)
            .collect()
    }

    /// Returns the total used capacity in 4KB blocks.
    pub fn used_blocks_4k(&self) -> u64 {
        self.zones.iter().map(|z| z.write_pointer_4k).sum()
    }

    /// Returns the configuration.
    pub fn config(&self) -> &ZnsConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ZnsConfig {
        ZnsConfig::new(0, 10, 1024, 32, 64)
    }

    #[test]
    fn test_zone_manager_creation() {
        let config = test_config();
        let manager = ZoneManager::new(config.clone());

        assert_eq!(manager.num_zones(), 10);
        let counts = manager.zone_state_counts();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0].0, ZoneState::Empty);
        assert_eq!(counts[0].1, 10);
    }

    #[test]
    fn test_zone_append() {
        let config = test_config();
        let mut manager = ZoneManager::new(config);

        // Initially find writable zone should succeed
        let zone_idx = manager.find_writable_zone().unwrap();
        assert_eq!(zone_idx, 0);

        // Append 100 blocks
        let offset = manager.append(zone_idx, 100).unwrap();
        assert_eq!(offset, 0); // First zone starts at 0

        let zone = manager.zone(zone_idx).unwrap();
        assert_eq!(zone.write_pointer_4k, 100);
        assert_eq!(zone.state, ZoneState::Open);

        // Append more to verify write pointer advances
        let offset = manager.append(zone_idx, 50).unwrap();
        assert_eq!(offset, 100);

        let zone = manager.zone(zone_idx).unwrap();
        assert_eq!(zone.write_pointer_4k, 150);
    }

    #[test]
    fn test_zone_state_transitions() {
        let config = test_config();
        let mut manager = ZoneManager::new(config);

        let zone_idx = 0u64;

        // Open zone
        manager.open_zone(zone_idx).unwrap();
        assert_eq!(manager.zone(zone_idx).unwrap().state, ZoneState::Open);

        // Close zone
        manager.close_zone(zone_idx).unwrap();
        assert_eq!(manager.zone(zone_idx).unwrap().state, ZoneState::Closed);

        // Reopen zone
        manager.open_zone(zone_idx).unwrap();
        assert_eq!(manager.zone(zone_idx).unwrap().state, ZoneState::Open);

        // Finish zone
        manager.finish_zone(zone_idx).unwrap();
        assert_eq!(manager.zone(zone_idx).unwrap().state, ZoneState::Full);

        // Reset zone
        manager.reset_zone(zone_idx).unwrap();
        assert_eq!(manager.zone(zone_idx).unwrap().state, ZoneState::Empty);
        assert_eq!(manager.zone(zone_idx).unwrap().write_pointer_4k, 0);
    }

    #[test]
    fn test_find_writable_zone() {
        let config = test_config();
        let mut manager = ZoneManager::new(config);

        // Initially should find empty zone 0
        assert_eq!(manager.find_writable_zone(), Some(0));

        // Open zone 0
        manager.open_zone(0).unwrap();

        // Open zone 1
        manager.open_zone(1).unwrap();

        // Should still find zone 0 (Open takes precedence)
        assert_eq!(manager.find_writable_zone(), Some(0));

        // Close zone 0
        manager.close_zone(0).unwrap();

        // Now should find zone 1 (still Open)
        assert_eq!(manager.find_writable_zone(), Some(1));

        // Close zone 1
        manager.close_zone(1).unwrap();

        // Now should find zone 2 (Empty)
        assert_eq!(manager.find_writable_zone(), Some(2));
    }

    #[test]
    fn test_zone_full() {
        let config = ZnsConfig::new(0, 2, 100, 32, 64);
        let mut manager = ZoneManager::new(config);

        let zone_idx = 0;

        // Fill the zone exactly to capacity
        manager.append(zone_idx, 100).unwrap();

        let zone = manager.zone(zone_idx).unwrap();
        assert_eq!(zone.state, ZoneState::Full);
        assert_eq!(zone.write_pointer_4k, 100);

        // Trying to append more should fail
        let result = manager.append(zone_idx, 1);
        assert!(result.is_err());

        // Find writable zone should skip this one
        assert_eq!(manager.find_writable_zone(), Some(1));
    }

    #[test]
    fn test_gc_candidates() {
        let config = test_config();
        let mut manager = ZoneManager::new(config);

        // Fill zones 0 and 2 to Full (append reaches capacity)
        manager.append(0, 1024).unwrap();
        assert_eq!(manager.zone(0).unwrap().state, ZoneState::Full);

        manager.append(2, 1024).unwrap();
        assert_eq!(manager.zone(2).unwrap().state, ZoneState::Full);

        // Zone 1 is still Empty
        // Zone 3-9 are Empty

        let gc_candidates = manager.gc_candidates();
        assert!(gc_candidates.contains(&0));
        assert!(gc_candidates.contains(&2));
        assert_eq!(gc_candidates.len(), 2);
    }

    #[test]
    fn test_used_blocks() {
        let config = test_config();
        let mut manager = ZoneManager::new(config);

        assert_eq!(manager.used_blocks_4k(), 0);

        manager.append(0, 100).unwrap();
        manager.append(1, 200).unwrap();

        assert_eq!(manager.used_blocks_4k(), 300);
    }

    #[test]
    fn test_zone_descriptor() {
        let zd = ZoneDescriptor::new(5, 5120, 1024);

        assert_eq!(zd.zone_idx, 5);
        assert_eq!(zd.start_offset_4k, 5120);
        assert_eq!(zd.capacity_4k, 1024);
        assert_eq!(zd.write_pointer_4k, 0);
        assert_eq!(zd.state, ZoneState::Empty);
        assert!(zd.is_writable());
        assert_eq!(zd.write_offset_4k(), 5120);
        assert_eq!(zd.free_blocks_4k(), 1024);
    }

    #[test]
    fn test_zns_config() {
        let config = ZnsConfig::new(1, 100, 2048, 16, 32);

        assert_eq!(config.device_idx, 1);
        assert_eq!(config.num_zones, 100);
        assert_eq!(config.zone_size_4k, 2048);
        assert_eq!(config.total_capacity_4k(), 100 * 2048);
    }

    #[test]
    fn test_invalid_zone_operations() {
        let config = test_config();
        let mut manager = ZoneManager::new(config);

        // Append to non-existent zone
        let result = manager.append(999, 10);
        assert!(result.is_err());

        // Open non-existent zone
        let result = manager.open_zone(999);
        assert!(result.is_err());

        // Close non-open zone
        let result = manager.close_zone(0);
        assert!(result.is_err());

        // Reset an empty zone (should fail)
        let result = manager.reset_zone(0);
        assert!(result.is_err());
    }
}
