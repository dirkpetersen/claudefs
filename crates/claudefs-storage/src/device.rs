//! Device management for NVMe storage devices.
//!
//! This module provides device discovery, configuration, and management
//! for NVMe devices in the storage engine. It handles device pooling,
//! block allocation per device, and health monitoring.

use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::allocator::{AllocatorConfig, AllocatorStats, BuddyAllocator};
use crate::block::{BlockRef, BlockSize};
use crate::error::StorageResult;

/// Information about a discovered NVMe device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeDeviceInfo {
    /// Device path (e.g., "/dev/nvme0n1")
    pub path: String,
    /// Device serial number
    pub serial: String,
    /// Device model name
    pub model: String,
    /// Firmware revision
    pub firmware_rev: String,
    /// Total capacity in bytes
    pub capacity_bytes: u64,
    /// Namespace ID
    pub namespace_id: u32,
    /// Whether FDP (Flexible Data Placement) is supported
    pub fdp_supported: bool,
    /// Whether ZNS (Zoned Namespace) is supported
    pub zns_supported: bool,
    /// Sector size in bytes (typically 512 or 4096)
    pub sector_size: u32,
    /// Maximum data transfer size in bytes
    pub max_transfer_size: u32,
    /// Number of I/O queues available
    pub num_io_queues: u16,
}

impl NvmeDeviceInfo {
    /// Create a new NvmeDeviceInfo with the given parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: String,
        serial: String,
        model: String,
        firmware_rev: String,
        capacity_bytes: u64,
        namespace_id: u32,
        fdp_supported: bool,
        zns_supported: bool,
        sector_size: u32,
        max_transfer_size: u32,
        num_io_queues: u16,
    ) -> Self {
        Self {
            path,
            serial,
            model,
            firmware_rev,
            capacity_bytes,
            namespace_id,
            fdp_supported,
            zns_supported,
            sector_size,
            max_transfer_size,
            num_io_queues,
        }
    }
}

/// Configuration for an NVMe device in the storage engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Device path
    pub path: String,
    /// Device index in the storage pool (0-based)
    pub device_idx: u16,
    /// Role of this device in the storage tier
    pub role: DeviceRole,
    /// Whether to enable FDP hints for this device
    pub fdp_enabled: bool,
    /// Queue depth for io_uring submissions
    pub queue_depth: u32,
    /// Whether to use direct I/O (bypass page cache)
    pub direct_io: bool,
}

impl DeviceConfig {
    /// Create a new DeviceConfig with the given parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: String,
        device_idx: u16,
        role: DeviceRole,
        fdp_enabled: bool,
        queue_depth: u32,
        direct_io: bool,
    ) -> Self {
        Self {
            path,
            device_idx,
            role,
            fdp_enabled,
            queue_depth,
            direct_io,
        }
    }
}

/// Role of an NVMe device in the storage tier.
/// Per hardware.md drive tier strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceRole {
    /// SLC/TLC write journal — fast write absorption, metadata WAL
    Journal,
    /// QLC data tier — bulk data storage, read-heavy
    Data,
    /// Combined journal + data (for single-drive test setups)
    Combined,
}

/// Health status of an NVMe device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceHealth {
    /// Temperature in Celsius
    pub temperature_celsius: u16,
    /// Percentage of rated lifetime used (0-100+)
    pub percentage_used: u8,
    /// Available spare capacity percentage
    pub available_spare: u8,
    /// Data units written (in 512KB units)
    pub data_units_written: u64,
    /// Data units read (in 512KB units)
    pub data_units_read: u64,
    /// Power-on hours
    pub power_on_hours: u64,
    /// Number of unsafe shutdowns
    pub unsafe_shutdowns: u32,
    /// Whether the drive has critical warnings
    pub critical_warning: bool,
}

impl Default for DeviceHealth {
    fn default() -> Self {
        Self {
            temperature_celsius: 0,
            percentage_used: 0,
            available_spare: 100,
            data_units_written: 0,
            data_units_read: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            critical_warning: false,
        }
    }
}

/// Represents a managed NVMe device with its allocator.
pub struct ManagedDevice {
    /// Device configuration
    pub config: DeviceConfig,
    /// Device information (discovered at init)
    pub info: NvmeDeviceInfo,
    /// Block allocator for this device
    pub allocator: BuddyAllocator,
    /// File descriptor for the device (for I/O operations)
    fd: Option<std::fs::File>,
}

impl ManagedDevice {
    /// Create a new managed device from config and info.
    /// Opens the device file if path exists.
    pub fn new(config: DeviceConfig, info: NvmeDeviceInfo) -> StorageResult<Self> {
        let total_blocks_4k = info.capacity_bytes / 4096;
        let allocator_config = AllocatorConfig {
            device_idx: config.device_idx,
            total_blocks_4k,
        };
        let allocator = BuddyAllocator::new(allocator_config)?;

        let fd = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(if config.direct_io { libc::O_DIRECT } else { 0 })
            .open(&config.path)
        {
            Ok(file) => {
                info!("Opened device {} for I/O", config.path);
                Some(file)
            }
            Err(e) => {
                warn!(
                    "Failed to open device {}: {} - running in mock mode",
                    config.path, e
                );
                None
            }
        };

        Ok(Self {
            config,
            info,
            allocator,
            fd,
        })
    }

    /// Create a mock device for testing (no real file).
    pub fn new_mock(config: DeviceConfig, capacity_4k_blocks: u64) -> StorageResult<Self> {
        let allocator_config = AllocatorConfig {
            device_idx: config.device_idx,
            total_blocks_4k: capacity_4k_blocks,
        };
        let allocator = BuddyAllocator::new(allocator_config)?;

        let info = NvmeDeviceInfo::new(
            config.path.clone(),
            "MOCK_SERIAL".to_string(),
            "Mock Device".to_string(),
            "1.0.0".to_string(),
            capacity_4k_blocks * 4096,
            1,
            config.fdp_enabled,
            false,
            4096,
            1024 * 1024,
            32,
        );

        debug!(
            "Created mock device {} with {} 4KB blocks",
            config.path, capacity_4k_blocks
        );

        Ok(Self {
            config,
            info,
            allocator,
            fd: None,
        })
    }

    /// Returns whether this device supports FDP and it is enabled.
    pub fn fdp_active(&self) -> bool {
        self.info.fdp_supported && self.config.fdp_enabled
    }

    /// Returns whether this device supports ZNS.
    pub fn zns_supported(&self) -> bool {
        self.info.zns_supported
    }

    /// Allocate a block from this device.
    pub fn allocate_block(&self, size: BlockSize) -> StorageResult<BlockRef> {
        self.allocator.allocate(size)
    }

    /// Free a block on this device.
    pub fn free_block(&self, block_ref: BlockRef) -> StorageResult<()> {
        self.allocator.free(block_ref)
    }

    /// Returns the raw file descriptor for io_uring operations, if the device is open.
    pub fn raw_fd(&self) -> Option<i32> {
        self.fd.as_ref().map(|f| f.as_raw_fd())
    }

    /// Returns the device path.
    pub fn path(&self) -> &str {
        &self.config.path
    }

    /// Returns device stats from the allocator.
    pub fn allocator_stats(&self) -> AllocatorStats {
        self.allocator.stats()
    }
}

impl Drop for ManagedDevice {
    fn drop(&mut self) {
        if self.fd.is_some() {
            debug!("Closing device {}", self.config.path);
        }
    }
}

/// Pool of managed NVMe devices.
pub struct DevicePool {
    devices: Vec<ManagedDevice>,
}

impl DevicePool {
    /// Create an empty device pool.
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    /// Add a device to the pool.
    pub fn add_device(&mut self, device: ManagedDevice) {
        let idx = device.config.device_idx;
        debug!("Adding device {} at index {}", device.path(), idx);
        self.devices.push(device);
    }

    /// Get a device by index.
    pub fn device(&self, idx: u16) -> Option<&ManagedDevice> {
        self.devices.iter().find(|d| d.config.device_idx == idx)
    }

    /// Get a mutable reference to a device by index.
    pub fn device_mut(&mut self, idx: u16) -> Option<&mut ManagedDevice> {
        self.devices.iter_mut().find(|d| d.config.device_idx == idx)
    }

    /// Returns the number of devices in the pool.
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// Returns whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    /// Returns total capacity across all devices in bytes.
    pub fn total_capacity_bytes(&self) -> u64 {
        self.devices
            .iter()
            .map(|d| d.allocator.total_capacity_bytes())
            .sum()
    }

    /// Returns free capacity across all devices in bytes.
    pub fn free_capacity_bytes(&self) -> u64 {
        self.devices
            .iter()
            .map(|d| d.allocator.free_capacity_bytes())
            .sum()
    }

    /// Get devices by role.
    pub fn devices_by_role(&self, role: DeviceRole) -> Vec<&ManagedDevice> {
        self.devices
            .iter()
            .filter(|d| d.config.role == role)
            .collect()
    }

    /// Returns an iterator over all devices.
    pub fn iter(&self) -> impl Iterator<Item = &ManagedDevice> {
        self.devices.iter()
    }
}

impl Default for DevicePool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_config_creation() {
        let config = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Journal,
            true,
            32,
            true,
        );

        assert_eq!(config.path, "/dev/nvme0n1");
        assert_eq!(config.device_idx, 0);
        assert_eq!(config.role, DeviceRole::Journal);
        assert!(config.fdp_enabled);
        assert_eq!(config.queue_depth, 32);
        assert!(config.direct_io);

        let data_config = DeviceConfig::new(
            "/dev/nvme1n1".to_string(),
            1,
            DeviceRole::Data,
            false,
            64,
            true,
        );
        assert_eq!(data_config.role, DeviceRole::Data);

        let combined_config = DeviceConfig::new(
            "/dev/nvme2n1".to_string(),
            2,
            DeviceRole::Combined,
            false,
            32,
            false,
        );
        assert_eq!(combined_config.role, DeviceRole::Combined);
    }

    #[test]
    fn test_mock_device_creation() {
        let config =
            DeviceConfig::new("/dev/null".to_string(), 0, DeviceRole::Data, true, 32, true);

        let device = ManagedDevice::new_mock(config.clone(), 16384).unwrap();

        assert_eq!(device.path(), "/dev/null");
        assert_eq!(device.config.device_idx, 0);
        assert!(device.fdp_active());

        let stats = device.allocator_stats();
        assert_eq!(stats.total_blocks_4k, 16384);
        assert_eq!(stats.free_blocks_4k, 16384);
    }

    #[test]
    fn test_device_pool_operations() {
        let mut pool = DevicePool::new();

        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);

        let config1 = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Journal,
            true,
            32,
            true,
        );
        let device1 = ManagedDevice::new_mock(config1, 16384).unwrap();
        pool.add_device(device1);

        let config2 = DeviceConfig::new(
            "/dev/nvme1n1".to_string(),
            1,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let device2 = ManagedDevice::new_mock(config2, 32768).unwrap();
        pool.add_device(device2);

        let config3 = DeviceConfig::new(
            "/dev/nvme2n1".to_string(),
            2,
            DeviceRole::Combined,
            false,
            32,
            true,
        );
        let device3 = ManagedDevice::new_mock(config3, 8192).unwrap();
        pool.add_device(device3);

        assert_eq!(pool.len(), 3);
        assert!(!pool.is_empty());

        assert_eq!(pool.total_capacity_bytes(), (16384 + 32768 + 8192) * 4096);
        assert_eq!(pool.free_capacity_bytes(), (16384 + 32768 + 8192) * 4096);

        let journal_devices = pool.devices_by_role(DeviceRole::Journal);
        assert_eq!(journal_devices.len(), 1);
        assert_eq!(journal_devices[0].config.device_idx, 0);

        let data_devices = pool.devices_by_role(DeviceRole::Data);
        assert_eq!(data_devices.len(), 1);
        assert_eq!(data_devices[0].config.device_idx, 1);

        let combined_devices = pool.devices_by_role(DeviceRole::Combined);
        assert_eq!(combined_devices.len(), 1);
        assert_eq!(combined_devices[0].config.device_idx, 2);

        let device_by_idx = pool.device(1);
        assert!(device_by_idx.is_some());
        assert_eq!(device_by_idx.unwrap().path(), "/dev/nvme1n1");

        let count = pool.iter().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_fdp_and_zns_flags() {
        let journal_config = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Journal,
            true,
            32,
            true,
        );
        let journal_info = NvmeDeviceInfo::new(
            "/dev/nvme0n1".to_string(),
            "SN123".to_string(),
            "Samsung 990 Pro".to_string(),
            "1.0".to_string(),
            2_000_000_000_000,
            1,
            true,
            false,
            4096,
            1024 * 1024,
            64,
        );
        let journal_device = ManagedDevice::new(journal_config, journal_info).ok();

        if let Some(device) = journal_device {
            assert!(device.fdp_active());
            assert!(!device.zns_supported());
        }

        let data_config = DeviceConfig::new(
            "/dev/nvme1n1".to_string(),
            1,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let data_info = NvmeDeviceInfo::new(
            "/dev/nvme1n1".to_string(),
            "SN456".to_string(),
            "WD SN770".to_string(),
            "2.0".to_string(),
            2_000_000_000_000,
            1,
            false,
            false,
            4096,
            512 * 1024,
            32,
        );
        let data_device = ManagedDevice::new(data_config, data_info).ok();

        if let Some(device) = data_device {
            assert!(!device.fdp_active());
            assert!(!device.zns_supported());
        }

        let mock_config = DeviceConfig::new(
            "/dev/mock".to_string(),
            2,
            DeviceRole::Combined,
            true,
            32,
            true,
        );
        let mock_device = ManagedDevice::new_mock(mock_config, 1000).unwrap();
        assert!(mock_device.fdp_active());
        assert!(!mock_device.zns_supported());

        let mock_no_fdp = DeviceConfig::new(
            "/dev/mock".to_string(),
            3,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let mock_no_fdp_device = ManagedDevice::new_mock(mock_no_fdp, 1000).unwrap();
        assert!(!mock_no_fdp_device.fdp_active());
    }

    #[test]
    fn test_managed_device_allocate_free() {
        let config = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let device = ManagedDevice::new_mock(config, 1000).unwrap();

        let initial_capacity = device.allocator.total_capacity_bytes();
        assert_eq!(initial_capacity, 1000 * 4096);

        let block = device.allocate_block(BlockSize::B4K).unwrap();
        assert_eq!(block.id.device_idx, 0);
        assert_eq!(block.size, BlockSize::B4K);

        let after_alloc = device.allocate_block(BlockSize::B64K).unwrap();
        assert_eq!(after_alloc.size, BlockSize::B64K);

        device.free_block(block).unwrap();
        device.free_block(after_alloc).unwrap();

        let final_free = device.allocator.free_capacity_bytes();
        assert!(final_free > 0);
    }

    #[test]
    fn test_device_health_default() {
        let health = DeviceHealth::default();
        assert_eq!(health.temperature_celsius, 0);
        assert_eq!(health.percentage_used, 0);
        assert_eq!(health.available_spare, 100);
        assert_eq!(health.data_units_written, 0);
        assert_eq!(health.data_units_read, 0);
        assert_eq!(health.power_on_hours, 0);
        assert_eq!(health.unsafe_shutdowns, 0);
        assert!(!health.critical_warning);
    }

    #[test]
    fn test_nvme_device_info() {
        let info = NvmeDeviceInfo::new(
            "/dev/nvme0n1".to_string(),
            "SN123456789".to_string(),
            "Samsung 990 Pro 2TB".to_string(),
            "5B2QFXG7".to_string(),
            2_000_000_000_000,
            1,
            true,
            false,
            4096,
            1024 * 1024,
            64,
        );

        assert_eq!(info.path, "/dev/nvme0n1");
        assert_eq!(info.serial, "SN123456789");
        assert_eq!(info.model, "Samsung 990 Pro 2TB");
        assert_eq!(info.firmware_rev, "5B2QFXG7");
        assert_eq!(info.capacity_bytes, 2_000_000_000_000);
        assert!(info.fdp_supported);
        assert!(!info.zns_supported);
        assert_eq!(info.sector_size, 4096);
        assert_eq!(info.max_transfer_size, 1024 * 1024);
        assert_eq!(info.num_io_queues, 64);
    }

    #[test]
    fn test_managed_device_no_double_close() {
        let config = DeviceConfig::new(
            "/dev/mock".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            false,
        );
        let device = ManagedDevice::new_mock(config, 1000).unwrap();
        assert!(device.raw_fd().is_none());
    }

    #[test]
    fn test_managed_device_real_open_close() {
        let config = DeviceConfig::new(
            "/dev/null".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            false,
        );
        let info = NvmeDeviceInfo::new(
            "/dev/null".to_string(),
            "NULL".to_string(),
            "Null Device".to_string(),
            "1.0".to_string(),
            0,
            1,
            false,
            false,
            512,
            1024 * 1024,
            1,
        );
        let device = ManagedDevice::new(config, info).unwrap();
        assert!(device.raw_fd().is_some());
        let fd = device.raw_fd().unwrap();
        assert!(fd >= 0);
    }
}
