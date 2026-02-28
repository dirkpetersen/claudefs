//! Device hot-swap coordination for storage devices.
//!
//! This module provides hot-swap capabilities for NVMe storage devices,
//! including device registration, state management, and graceful drain
//! for safe device removal.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::block::{BlockRef, BlockSize};
use crate::device::DeviceRole;
use crate::error::{StorageError, StorageResult};

/// Errors that can occur during hot-swap operations.
#[derive(Debug, Error)]
pub enum HotSwapError {
    #[error("Device {0} not found")]
    DeviceNotFound(u16),

    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition { from: DeviceState, to: DeviceState },

    #[error("Device {0} is not in drainable state: {1:?}")]
    NotDrainable(u16, DeviceState),

    #[error("Device {0} is not removable: {1:?}")]
    NotRemovable(u16, DeviceState),

    #[error("Device {0} already registered")]
    AlreadyRegistered(u16),

    #[error("Device {0} failed: {1}")]
    DeviceFailed(u16, String),
}

/// Lifecycle state of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceState {
    /// Device is being initialized (superblock being written, allocator setup)
    Initializing,
    /// Device is active and accepting I/O
    Active,
    /// Device is being drained (no new allocations, existing data being migrated)
    Draining,
    /// Device has been fully drained and can be safely removed
    Drained,
    /// Device has been removed from the pool
    Removed,
    /// Device has failed and needs recovery
    Failed,
}

/// Tracks migration progress during device drain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrainProgress {
    /// The device being drained.
    pub device_idx: u16,
    /// Total number of blocks that need migration.
    pub total_blocks_to_migrate: u64,
    /// Number of blocks successfully migrated.
    pub blocks_migrated: u64,
    /// Number of blocks that failed migration.
    pub blocks_failed: u64,
    /// When the drain started (Unix timestamp in seconds).
    pub started_at_secs: u64,
    /// Estimated completion time (Unix timestamp in seconds).
    pub estimated_completion_secs: Option<u64>,
}

impl DrainProgress {
    /// Create a new DrainProgress for a device drain operation.
    pub fn new(device_idx: u16, total_blocks: u64) -> Self {
        let started_at_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            device_idx,
            total_blocks_to_migrate: total_blocks,
            blocks_migrated: 0,
            blocks_failed: 0,
            started_at_secs,
            estimated_completion_secs: None,
        }
    }

    /// Returns the progress as a percentage (0.0 to 100.0).
    pub fn progress_pct(&self) -> f64 {
        if self.total_blocks_to_migrate == 0 {
            return 100.0;
        }
        (self.blocks_migrated as f64 / self.total_blocks_to_migrate as f64) * 100.0
    }

    /// Returns true if drain is complete (all blocks migrated or failed).
    pub fn is_complete(&self) -> bool {
        self.blocks_migrated + self.blocks_failed >= self.total_blocks_to_migrate
    }

    /// Record successful migration of blocks.
    pub fn record_migrated(&mut self, count: u64) {
        self.blocks_migrated += count;
    }

    /// Record failed migration of blocks.
    pub fn record_failed(&mut self, count: u64) {
        self.blocks_failed += count;
    }
}

/// State of a block migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationState {
    /// Migration has not started yet.
    Pending,
    /// Migration is currently in progress.
    InProgress,
    /// Migration completed successfully.
    Completed,
    /// Migration failed.
    Failed,
}

/// Represents a block that needs to be migrated from one device to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMigration {
    /// The source block to migrate from.
    pub source: BlockRef,
    /// The destination block (None until target is allocated).
    pub destination: Option<BlockRef>,
    /// Current state of the migration.
    pub state: MigrationState,
}

/// Events emitted during hot-swap operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HotSwapEvent {
    /// A device is being added to the pool.
    DeviceAdding {
        /// Device index.
        device_idx: u16,
        /// Device role.
        role: DeviceRole,
        /// Device capacity in bytes.
        capacity_bytes: u64,
    },
    /// Device has been successfully added and is active.
    DeviceAdded {
        /// Device index.
        device_idx: u16,
    },
    /// Device drain has started.
    DrainStarted {
        /// Device index.
        device_idx: u16,
        /// Number of blocks to migrate.
        blocks_to_migrate: u64,
    },
    /// Device drain progress update.
    DrainProgress {
        /// Device index.
        device_idx: u16,
        /// Progress percentage.
        progress_pct: f64,
    },
    /// Device drain has completed.
    DrainCompleted {
        /// Device index.
        device_idx: u16,
    },
    /// Device has been removed from the pool.
    DeviceRemoved {
        /// Device index.
        device_idx: u16,
    },
    /// Device has failed.
    DeviceFailed {
        /// Device index.
        device_idx: u16,
        /// Failure reason.
        reason: String,
    },
    /// A batch of block migrations completed.
    MigrationBatchCompleted {
        /// Device index.
        device_idx: u16,
        /// Number of blocks migrated in this batch.
        blocks_migrated: u64,
    },
}

/// Statistics for hot-swap operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HotSwapStats {
    /// Total number of devices added.
    pub devices_added: u64,
    /// Total number of devices removed.
    pub devices_removed: u64,
    /// Number of successful drains.
    pub drains_completed: u64,
    /// Number of failed drains.
    pub drains_failed: u64,
    /// Total number of blocks migrated.
    pub total_blocks_migrated: u64,
}

/// Coordinates hot-swap operations for storage devices.
pub struct HotSwapManager {
    device_states: Mutex<HashMap<u16, DeviceState>>,
    drain_progress: Mutex<HashMap<u16, DrainProgress>>,
    pending_migrations: Mutex<Vec<BlockMigration>>,
    events: Mutex<Vec<HotSwapEvent>>,
    stats: Mutex<HotSwapStats>,
}

impl HotSwapManager {
    /// Create a new HotSwapManager.
    pub fn new() -> Self {
        Self {
            device_states: Mutex::new(HashMap::new()),
            drain_progress: Mutex::new(HashMap::new()),
            pending_migrations: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            stats: Mutex::new(HotSwapStats::default()),
        }
    }

    /// Register a new device being added. Sets state to Initializing.
    pub fn register_device(
        &self,
        device_idx: u16,
        role: DeviceRole,
        capacity_bytes: u64,
    ) -> StorageResult<()> {
        let mut states = self.device_states.lock().unwrap();

        if states.contains_key(&device_idx) {
            return Err(StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: format!("device already registered: {}", device_idx),
            });
        }

        states.insert(device_idx, DeviceState::Initializing);

        let mut events = self.events.lock().unwrap();
        events.push(HotSwapEvent::DeviceAdding {
            device_idx,
            role,
            capacity_bytes,
        });

        debug!(
            "registered device {} with role {:?}, capacity {} bytes",
            device_idx, role, capacity_bytes
        );
        Ok(())
    }

    /// Mark a device as active (ready for I/O).
    pub fn activate_device(&self, device_idx: u16) -> StorageResult<()> {
        let mut states = self.device_states.lock().unwrap();

        let current_state = states
            .get(&device_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: "device not found".to_string(),
            })?;

        if *current_state != DeviceState::Initializing {
            return Err(StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: format!("cannot activate from state {:?}", current_state),
            });
        }

        states.insert(device_idx, DeviceState::Active);

        let mut events = self.events.lock().unwrap();
        events.push(HotSwapEvent::DeviceAdded { device_idx });

        let mut stats = self.stats.lock().unwrap();
        stats.devices_added += 1;

        info!("activated device {}", device_idx);
        Ok(())
    }

    /// Start draining a device. Call this before removing a device.
    /// Returns the DrainProgress for tracking migration.
    pub fn start_drain(
        &self,
        device_idx: u16,
        allocated_blocks: Vec<BlockRef>,
    ) -> StorageResult<DrainProgress> {
        let mut states = self.device_states.lock().unwrap();

        let current_state = states
            .get(&device_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: "device not found".to_string(),
            })?;

        if *current_state != DeviceState::Active {
            return Err(StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: format!("cannot start drain from state {:?}", current_state),
            });
        }

        states.insert(device_idx, DeviceState::Draining);

        let total_blocks = allocated_blocks.len() as u64;
        let progress = DrainProgress::new(device_idx, total_blocks);

        let mut migrations = self.pending_migrations.lock().unwrap();
        for block_ref in allocated_blocks {
            migrations.push(BlockMigration {
                source: block_ref,
                destination: None,
                state: MigrationState::Pending,
            });
        }

        let mut drain_progress = self.drain_progress.lock().unwrap();
        drain_progress.insert(device_idx, progress.clone());

        let mut events = self.events.lock().unwrap();
        events.push(HotSwapEvent::DrainStarted {
            device_idx,
            blocks_to_migrate: total_blocks,
        });

        info!(
            "started drain for device {}, {} blocks to migrate",
            device_idx, total_blocks
        );
        Ok(progress)
    }

    /// Record that a batch of blocks has been migrated.
    pub fn record_migration_batch(
        &self,
        device_idx: u16,
        migrated: &[BlockMigration],
    ) -> StorageResult<()> {
        let count = migrated.len() as u64;

        let mut drain_progress = self.drain_progress.lock().unwrap();
        let progress =
            drain_progress
                .get_mut(&device_idx)
                .ok_or_else(|| StorageError::DeviceError {
                    device: format!("device_{}", device_idx),
                    reason: "no drain in progress".to_string(),
                })?;

        progress.record_migrated(count);

        let mut events = self.events.lock().unwrap();
        events.push(HotSwapEvent::MigrationBatchCompleted {
            device_idx,
            blocks_migrated: count,
        });

        let progress_pct = progress.progress_pct();
        if progress_pct < 100.0 {
            events.push(HotSwapEvent::DrainProgress {
                device_idx,
                progress_pct,
            });
        }

        debug!(
            "recorded {} migrated blocks for device {}, progress: {:.1}%",
            count, device_idx, progress_pct
        );
        Ok(())
    }

    /// Check if drain is complete for a device.
    pub fn is_drain_complete(&self, device_idx: u16) -> bool {
        let drain_progress = self.drain_progress.lock().unwrap();
        drain_progress
            .get(&device_idx)
            .map(|p| p.is_complete())
            .unwrap_or(false)
    }

    /// Mark a device as fully drained and ready for removal.
    pub fn complete_drain(&self, device_idx: u16) -> StorageResult<()> {
        let mut states = self.device_states.lock().unwrap();

        let current_state = states
            .get(&device_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: "device not found".to_string(),
            })?;

        if *current_state != DeviceState::Draining {
            return Err(StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: format!("cannot complete drain from state {:?}", current_state),
            });
        }

        if !self.is_drain_complete(device_idx) {
            return Err(StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: "drain not complete".to_string(),
            });
        }

        states.insert(device_idx, DeviceState::Drained);

        let mut events = self.events.lock().unwrap();
        events.push(HotSwapEvent::DrainCompleted { device_idx });

        let mut stats = self.stats.lock().unwrap();
        stats.drains_completed += 1;

        info!("drain completed for device {}", device_idx);
        Ok(())
    }

    /// Remove a drained device.
    pub fn remove_device(&self, device_idx: u16) -> StorageResult<()> {
        let mut states = self.device_states.lock().unwrap();

        let current_state = states
            .get(&device_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: "device not found".to_string(),
            })?;

        if *current_state != DeviceState::Drained {
            return Err(StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: format!("device is not in Drained state: {:?}", current_state),
            });
        }

        states.insert(device_idx, DeviceState::Removed);

        let mut events = self.events.lock().unwrap();
        events.push(HotSwapEvent::DeviceRemoved { device_idx });

        let mut stats = self.stats.lock().unwrap();
        stats.devices_removed += 1;

        info!("removed device {}", device_idx);
        Ok(())
    }

    /// Mark a device as failed.
    pub fn fail_device(&self, device_idx: u16, reason: String) -> StorageResult<()> {
        let mut states = self.device_states.lock().unwrap();

        let current_state = states
            .get(&device_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: "device not found".to_string(),
            })?;

        if *current_state == DeviceState::Removed {
            return Err(StorageError::DeviceError {
                device: format!("device_{}", device_idx),
                reason: "cannot fail removed device".to_string(),
            });
        }

        states.insert(device_idx, DeviceState::Failed);

        let mut events = self.events.lock().unwrap();
        events.push(HotSwapEvent::DeviceFailed {
            device_idx,
            reason: reason.clone(),
        });

        let mut stats = self.stats.lock().unwrap();
        if *current_state == DeviceState::Draining {
            stats.drains_failed += 1;
        }

        warn!("device {} failed: {}", device_idx, reason);
        Ok(())
    }

    /// Get the current state of a device.
    pub fn device_state(&self, device_idx: u16) -> Option<DeviceState> {
        let states = self.device_states.lock().unwrap();
        states.get(&device_idx).copied()
    }

    /// Get drain progress for a device.
    pub fn drain_progress(&self, device_idx: u16) -> Option<DrainProgress> {
        let drain_progress = self.drain_progress.lock().unwrap();
        drain_progress.get(&device_idx).cloned()
    }

    /// Get all pending events since last drain.
    pub fn drain_events(&self) -> Vec<HotSwapEvent> {
        let events = self.events.lock().unwrap();
        events.clone()
    }

    /// Can this device accept new allocations?
    pub fn can_allocate(&self, device_idx: u16) -> bool {
        let states = self.device_states.lock().unwrap();
        states
            .get(&device_idx)
            .map(|&s| s == DeviceState::Active)
            .unwrap_or(false)
    }

    /// Get statistics.
    pub fn stats(&self) -> HotSwapStats {
        let stats = self.stats.lock().unwrap();
        stats.clone()
    }
}

impl Default for HotSwapManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_block_ref(device_idx: u16, offset: u64) -> BlockRef {
        BlockRef {
            id: BlockId::new(device_idx, offset),
            size: BlockSize::B4K,
        }
    }

    #[test]
    fn test_device_state_transitions() {
        let manager = HotSwapManager::new();

        // Register device
        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        assert_eq!(manager.device_state(0), Some(DeviceState::Initializing));

        // Activate device
        manager.activate_device(0).unwrap();
        assert_eq!(manager.device_state(0), Some(DeviceState::Active));

        // Start drain
        manager.start_drain(0, vec![]).unwrap();
        assert_eq!(manager.device_state(0), Some(DeviceState::Draining));

        // Complete drain
        manager.complete_drain(0).unwrap();
        assert_eq!(manager.device_state(0), Some(DeviceState::Drained));

        // Remove device
        manager.remove_device(0).unwrap();
        assert_eq!(manager.device_state(0), Some(DeviceState::Removed));
    }

    #[test]
    fn test_register_and_activate() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Combined, 500_000_000)
            .unwrap();
        assert_eq!(manager.device_state(0), Some(DeviceState::Initializing));

        manager.activate_device(0).unwrap();
        assert_eq!(manager.device_state(0), Some(DeviceState::Active));

        let stats = manager.stats();
        assert_eq!(stats.devices_added, 1);
    }

    #[test]
    fn test_start_drain() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let blocks = vec![
            create_test_block_ref(0, 0),
            create_test_block_ref(0, 1),
            create_test_block_ref(0, 2),
        ];

        let progress = manager.start_drain(0, blocks).unwrap();

        assert_eq!(progress.total_blocks_to_migrate, 3);
        assert_eq!(progress.blocks_migrated, 0);
    }

    #[test]
    fn test_drain_progress() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let blocks: Vec<BlockRef> = (0..10).map(|i| create_test_block_ref(0, i)).collect();
        manager.start_drain(0, blocks).unwrap();

        let migrations = vec![
            BlockMigration {
                source: create_test_block_ref(0, 0),
                destination: Some(create_test_block_ref(1, 0)),
                state: MigrationState::Completed,
            },
            BlockMigration {
                source: create_test_block_ref(0, 1),
                destination: Some(create_test_block_ref(1, 1)),
                state: MigrationState::Completed,
            },
        ];

        manager.record_migration_batch(0, &migrations).unwrap();

        let progress = manager.drain_progress(0).unwrap();
        assert_eq!(progress.blocks_migrated, 2);
        assert!((progress.progress_pct() - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_drain_complete() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let blocks = vec![create_test_block_ref(0, 0)];
        manager.start_drain(0, blocks).unwrap();

        let migrations = vec![BlockMigration {
            source: create_test_block_ref(0, 0),
            destination: Some(create_test_block_ref(1, 0)),
            state: MigrationState::Completed,
        }];

        manager.record_migration_batch(0, &migrations).unwrap();

        assert!(manager.is_drain_complete(0));

        manager.complete_drain(0).unwrap();
        assert_eq!(manager.device_state(0), Some(DeviceState::Drained));
    }

    #[test]
    fn test_remove_drained_device() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let blocks = vec![create_test_block_ref(0, 0)];
        manager.start_drain(0, blocks).unwrap();

        let migrations = vec![BlockMigration {
            source: create_test_block_ref(0, 0),
            destination: Some(create_test_block_ref(1, 0)),
            state: MigrationState::Completed,
        }];

        manager.record_migration_batch(0, &migrations).unwrap();
        manager.complete_drain(0).unwrap();
        manager.remove_device(0).unwrap();

        assert_eq!(manager.device_state(0), Some(DeviceState::Removed));

        let stats = manager.stats();
        assert_eq!(stats.devices_removed, 1);
    }

    #[test]
    fn test_cannot_remove_active_device() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let result = manager.remove_device(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_cannot_allocate_draining() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        assert!(manager.can_allocate(0));

        manager.start_drain(0, vec![]).unwrap();

        assert!(!manager.can_allocate(0));
    }

    #[test]
    fn test_fail_device() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        manager.fail_device(0, "media error".to_string()).unwrap();

        assert_eq!(manager.device_state(0), Some(DeviceState::Failed));

        let events = manager.drain_events();
        assert!(events.iter().any(
            |e| matches!(e, HotSwapEvent::DeviceFailed { reason, .. } if reason == "media error")
        ));
    }

    #[test]
    fn test_events_emitted() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let events = manager.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, HotSwapEvent::DeviceAdding { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, HotSwapEvent::DeviceAdded { .. })));
    }

    #[test]
    fn test_migration_states() {
        let migration = BlockMigration {
            source: create_test_block_ref(0, 0),
            destination: None,
            state: MigrationState::Pending,
        };

        assert_eq!(migration.state, MigrationState::Pending);

        let migration = BlockMigration {
            source: create_test_block_ref(0, 0),
            destination: Some(create_test_block_ref(1, 0)),
            state: MigrationState::InProgress,
        };

        assert_eq!(migration.state, MigrationState::InProgress);

        let migration = BlockMigration {
            source: create_test_block_ref(0, 0),
            destination: Some(create_test_block_ref(1, 0)),
            state: MigrationState::Completed,
        };

        assert_eq!(migration.state, MigrationState::Completed);
    }

    #[test]
    fn test_drain_progress_percentage() {
        let mut progress = DrainProgress::new(0, 100);

        assert!((progress.progress_pct() - 0.0).abs() < 0.01);

        progress.record_migrated(25);
        assert!((progress.progress_pct() - 25.0).abs() < 0.01);

        progress.record_migrated(25);
        assert!((progress.progress_pct() - 50.0).abs() < 0.01);

        progress.record_migrated(50);
        assert!((progress.progress_pct() - 100.0).abs() < 0.01);

        // Zero case
        let progress = DrainProgress::new(0, 0);
        assert!((progress.progress_pct() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_stats_tracking() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let blocks = vec![create_test_block_ref(0, 0)];
        manager.start_drain(0, blocks).unwrap();

        let migrations = vec![BlockMigration {
            source: create_test_block_ref(0, 0),
            destination: Some(create_test_block_ref(1, 0)),
            state: MigrationState::Completed,
        }];

        manager.record_migration_batch(0, &migrations).unwrap();
        manager.complete_drain(0).unwrap();
        manager.remove_device(0).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.devices_added, 1);
        assert_eq!(stats.devices_removed, 1);
        assert_eq!(stats.drains_completed, 1);
    }

    #[test]
    fn test_multiple_devices() {
        let manager = HotSwapManager::new();

        // Add multiple devices
        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager
            .register_device(1, DeviceRole::Journal, 500_000_000)
            .unwrap();
        manager
            .register_device(2, DeviceRole::Combined, 2_000_000_000)
            .unwrap();

        manager.activate_device(0).unwrap();
        manager.activate_device(1).unwrap();
        manager.activate_device(2).unwrap();

        assert!(manager.can_allocate(0));
        assert!(manager.can_allocate(1));
        assert!(manager.can_allocate(2));

        // Drain one device
        manager.start_drain(1, vec![]).unwrap();

        assert!(manager.can_allocate(0));
        assert!(!manager.can_allocate(1));
        assert!(manager.can_allocate(2));
    }

    #[test]
    fn test_double_register_fails() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();

        let result = manager.register_device(0, DeviceRole::Data, 1_000_000_000);
        assert!(result.is_err());
    }
}
