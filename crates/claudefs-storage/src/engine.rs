//! Storage engine combining device pool and I/O engine.
//!
//! This module provides the main entry point for the storage subsystem,
//! managing a pool of devices and routing block operations through the I/O engine.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::block::{BlockRef, BlockSize, PlacementHint};
use crate::device::{DeviceConfig, DevicePool, DeviceRole, ManagedDevice};
use crate::error::{StorageError, StorageResult};
use crate::io_uring_bridge::{IoEngine, IoStats};

/// Configuration for the storage engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageEngineConfig {
    /// Name/label for this storage engine instance
    pub name: String,
    /// Default placement hint for writes without explicit hints
    pub default_placement: PlacementHint,
    /// Whether to verify checksums on reads
    pub verify_checksums: bool,
    /// Whether to use direct I/O by default
    pub direct_io: bool,
}

impl Default for StorageEngineConfig {
    fn default() -> Self {
        Self {
            name: "claudefs-storage".to_string(),
            default_placement: PlacementHint::HotData,
            verify_checksums: true,
            direct_io: true,
        }
    }
}

/// Aggregate statistics for the storage engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageEngineStats {
    /// Number of devices in the pool
    pub device_count: usize,
    /// Total capacity in bytes across all devices
    pub total_capacity_bytes: u64,
    /// Free capacity in bytes across all devices
    pub free_capacity_bytes: u64,
    /// I/O statistics from the I/O engine
    pub io_stats: IoStats,
}

/// The main storage engine combining device pool and I/O engine.
/// 
/// This is the primary public API for the storage subsystem. It manages
/// a pool of devices, routes block operations through the I/O engine,
/// and handles block allocation and deallocation.
pub struct StorageEngine<E: IoEngine> {
    config: StorageEngineConfig,
    device_pool: DevicePool,
    io_engine: Arc<E>,
}

impl<E: IoEngine> StorageEngine<E> {
    /// Create a new storage engine with the given config and I/O engine.
    pub fn new(config: StorageEngineConfig, io_engine: E) -> Self {
        info!(
            "Creating storage engine '{}' with verify_checksums={}, direct_io={}",
            config.name, config.verify_checksums, config.direct_io
        );
        Self {
            config,
            device_pool: DevicePool::new(),
            io_engine: Arc::new(io_engine),
        }
    }

    /// Add a device to the storage engine's pool.
    pub fn add_device(&mut self, device: ManagedDevice) {
        let idx = device.config.device_idx;
        let capacity = device.allocator.free_capacity_bytes();
        debug!("Adding device {} with {} bytes free", idx, capacity);
        self.device_pool.add_device(device);
    }

    /// Add a mock device with the given capacity (in 4KB blocks).
    pub fn add_mock_device(
        &mut self,
        device_idx: u16,
        role: DeviceRole,
        capacity_4k: u64,
    ) -> StorageResult<()> {
        let config = DeviceConfig::new(
            format!("/dev/mock{}", device_idx),
            device_idx,
            role,
            false,
            32,
            self.config.direct_io,
        );
        let device = ManagedDevice::new_mock(config, capacity_4k)?;
        self.add_device(device);
        Ok(())
    }

    /// Allocate a block of the given size from a device with the specified role.
    /// If role is None, tries all devices.
    pub fn allocate(
        &self,
        size: BlockSize,
        preferred_role: Option<DeviceRole>,
    ) -> StorageResult<BlockRef> {
        // Try devices of the preferred role first
        if let Some(role) = preferred_role {
            let devices = self.device_pool.devices_by_role(role);
            for device in devices {
                match device.allocate_block(size) {
                    Ok(block_ref) => {
                        debug!(
                            "Allocated {} block at offset {} on device {} (role {:?})",
                            size, block_ref.id.offset, block_ref.id.device_idx, role
                        );
                        return Ok(block_ref);
                    }
                    Err(StorageError::OutOfSpace) => continue,
                    Err(e) => return Err(e),
                }
            }
        }

        // Fall back to any device
        for device in self.device_pool.iter() {
            match device.allocate_block(size) {
                Ok(block_ref) => {
                    debug!(
                        "Allocated {} block at offset {} on device {} (fallback)",
                        size, block_ref.id.offset, block_ref.id.device_idx
                    );
                    return Ok(block_ref);
                }
                Err(StorageError::OutOfSpace) => continue,
                Err(e) => return Err(e),
            }
        }

        Err(StorageError::OutOfSpace)
    }

    /// Free a block.
    pub fn free(&self, block_ref: BlockRef) -> StorageResult<()> {
        let device_idx = block_ref.id.device_idx;
        let device = self
            .device_pool
            .device(device_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("device{}", device_idx),
                reason: "Device not found".to_string(),
            })?;

        device.free_block(block_ref)?;
        debug!(
            "Freed {} block at offset {} on device {}",
            block_ref.size, block_ref.id.offset, device_idx
        );
        Ok(())
    }

    /// Read a block's data.
    pub async fn read(&self, block_ref: BlockRef) -> StorageResult<Vec<u8>> {
        self.io_engine.read_block(block_ref).await
    }

    /// Write data to a block.
    pub async fn write(
        &self,
        block_ref: BlockRef,
        data: Vec<u8>,
        hint: Option<PlacementHint>,
    ) -> StorageResult<()> {
        let hint = hint.or(Some(self.config.default_placement));
        self.io_engine.write_block(block_ref, data, hint).await
    }

    /// Allocate a block and immediately write data to it.
    pub async fn allocate_and_write(
        &self,
        size: BlockSize,
        data: Vec<u8>,
        hint: Option<PlacementHint>,
        role: Option<DeviceRole>,
    ) -> StorageResult<BlockRef> {
        let block_ref = self.allocate(size, role)?;
        let write_hint = hint.or(Some(self.config.default_placement));
        self.io_engine
            .write_block(block_ref, data, write_hint)
            .await?;
        Ok(block_ref)
    }

    /// Flush all pending writes to persistent storage.
    pub async fn flush(&self) -> StorageResult<()> {
        self.io_engine.flush().await
    }

    /// Discard a block (TRIM).
    pub async fn discard(&self, block_ref: BlockRef) -> StorageResult<()> {
        self.io_engine.discard_block(block_ref).await
    }

    /// Free and discard a block (free from allocator + TRIM on device).
    pub async fn free_and_discard(&self, block_ref: BlockRef) -> StorageResult<()> {
        self.free(block_ref)?;
        self.io_engine.discard_block(block_ref).await
    }

    /// Get aggregate storage engine statistics.
    pub fn stats(&self) -> StorageEngineStats {
        StorageEngineStats {
            device_count: self.device_pool.len(),
            total_capacity_bytes: self.device_pool.total_capacity_bytes(),
            free_capacity_bytes: self.device_pool.free_capacity_bytes(),
            io_stats: self.io_engine.stats(),
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &StorageEngineConfig {
        &self.config
    }

    /// Get a reference to the I/O engine.
    pub fn io_engine(&self) -> &E {
        &self.io_engine
    }

    /// Get the device pool.
    pub fn device_pool(&self) -> &DevicePool {
        &self.device_pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockSize;
    use crate::device::DeviceRole;
    use crate::io_uring_bridge::MockIoEngine;

    #[tokio::test]
    async fn test_engine_creation() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let engine: StorageEngine<MockIoEngine> = StorageEngine::new(config.clone(), mock_io);

        assert_eq!(engine.config().name, "claudefs-storage");
        assert_eq!(engine.config().default_placement, PlacementHint::HotData);
        assert!(engine.config().verify_checksums);
        assert!(engine.config().direct_io);
        assert_eq!(engine.device_pool().len(), 0);
    }

    #[tokio::test]
    async fn test_engine_add_mock_device() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine
            .add_mock_device(0, DeviceRole::Data, 16384)
            .unwrap();
        engine
            .add_mock_device(1, DeviceRole::Journal, 8192)
            .unwrap();
        engine
            .add_mock_device(2, DeviceRole::Combined, 32768)
            .unwrap();

        assert_eq!(engine.device_pool().len(), 3);

        let stats = engine.stats();
        assert_eq!(stats.device_count, 3);
    }

    #[tokio::test]
    async fn test_engine_allocate_and_free() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine
            .add_mock_device(0, DeviceRole::Data, 16384)
            .unwrap();

        let block_ref = engine.allocate(BlockSize::B4K, None).unwrap();
        assert_eq!(block_ref.id.device_idx, 0);
        assert_eq!(block_ref.size, BlockSize::B4K);

        engine.free(block_ref).unwrap();

        let block_ref2 = engine.allocate(BlockSize::B64K, None).unwrap();
        assert_eq!(block_ref2.size, BlockSize::B64K);

        engine.free(block_ref2).unwrap();
    }

    #[tokio::test]
    async fn test_engine_read_write() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine
            .add_mock_device(0, DeviceRole::Data, 16384)
            .unwrap();

        let block_ref = engine.allocate(BlockSize::B4K, None).unwrap();
        let data = vec![0xDE; 4096];

        engine
            .write(block_ref, data.clone(), None)
            .await
            .unwrap();

        let read_data = engine.read(block_ref).await.unwrap();
        assert_eq!(read_data, data);
    }

    #[tokio::test]
    async fn test_engine_allocate_and_write() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine
            .add_mock_device(0, DeviceRole::Data, 16384)
            .unwrap();

        let data = vec![0xCA; 4096];
        let block_ref = engine
            .allocate_and_write(BlockSize::B4K, data.clone(), None, None)
            .await
            .unwrap();

        let read_data = engine.read(block_ref).await.unwrap();
        assert_eq!(read_data, data);
    }

    #[tokio::test]
    async fn test_engine_stats() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine
            .add_mock_device(0, DeviceRole::Data, 16384)
            .unwrap();
        engine
            .add_mock_device(1, DeviceRole::Journal, 8192)
            .unwrap();

        let stats = engine.stats();
        assert_eq!(stats.device_count, 2);
        assert_eq!(stats.total_capacity_bytes, (16384 + 8192) * 4096);
    }

    #[tokio::test]
    async fn test_engine_preferred_role() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine
            .add_mock_device(0, DeviceRole::Journal, 8192)
            .unwrap();
        engine
            .add_mock_device(1, DeviceRole::Data, 16384)
            .unwrap();

        // Allocate from journal role
        let journal_block = engine
            .allocate(BlockSize::B4K, Some(DeviceRole::Journal))
            .unwrap();
        assert_eq!(journal_block.id.device_idx, 0);

        // Allocate from data role
        let data_block = engine
            .allocate(BlockSize::B4K, Some(DeviceRole::Data))
            .unwrap();
        assert_eq!(data_block.id.device_idx, 1);
    }

    #[tokio::test]
    async fn test_engine_flush() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine.flush().await.unwrap();

        let stats = engine.stats();
        assert_eq!(stats.io_stats.flushes_completed, 1);
    }

    #[tokio::test]
    async fn test_engine_discard() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine
            .add_mock_device(0, DeviceRole::Data, 16384)
            .unwrap();

        let block_ref = engine.allocate(BlockSize::B4K, None).unwrap();
        let data = vec![0xAB; 4096];
        engine
            .write(block_ref, data, None)
            .await
            .unwrap();

        engine.discard(block_ref).await.unwrap();

        let result = engine.read(block_ref).await;
        assert!(matches!(result, Err(StorageError::BlockNotFound { .. })));
    }

    #[tokio::test]
    async fn test_engine_free_and_discard() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        engine
            .add_mock_device(0, DeviceRole::Data, 16384)
            .unwrap();

        let block_ref = engine.allocate(BlockSize::B4K, None).unwrap();
        let data = vec![0xAB; 4096];
        engine
            .write(block_ref, data, None)
            .await
            .unwrap();

        engine.free_and_discard(block_ref).await.unwrap();

        // Block should be freed from allocator - we can allocate again
        let _block_ref2 = engine.allocate(BlockSize::B4K, None).unwrap();
    }

    #[tokio::test]
    async fn test_engine_out_of_space() {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        // Add a very small device
        engine.add_mock_device(0, DeviceRole::Data, 1).unwrap();

        // Try to allocate more than available
        let result = engine.allocate(BlockSize::B1M, None);
        assert!(matches!(result, Err(StorageError::OutOfSpace)));
    }
}