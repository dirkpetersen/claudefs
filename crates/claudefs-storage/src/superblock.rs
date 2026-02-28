//! Superblock module for device identification, crash recovery, and version compatibility.

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::{StorageError, StorageResult};

/// Superblock magic: "CFS!" = 0x43465321
pub const SUPERBLOCK_MAGIC: u32 = 0x43465321;
/// Current superblock version
pub const SUPERBLOCK_VERSION: u8 = 1;
/// Superblock is always at offset 0, taking one 4KB block
pub const SUPERBLOCK_OFFSET: u64 = 0;
/// Standard block size for base granularity
pub const BLOCK_SIZE: u32 = 4096;

/// Device role code (serializable version of DeviceRole)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum DeviceRoleCode {
    /// Journal device - fast write absorption, metadata WAL
    Journal = 0,
    /// Data device - bulk data storage, read-heavy
    Data = 1,
    /// Combined journal + data (for single-drive test setups)
    #[default]
    Combined = 2,
}

impl From<crate::device::DeviceRole> for DeviceRoleCode {
    fn from(role: crate::device::DeviceRole) -> Self {
        match role {
            crate::device::DeviceRole::Journal => Self::Journal,
            crate::device::DeviceRole::Data => Self::Data,
            crate::device::DeviceRole::Combined => Self::Combined,
        }
    }
}

/// Superblock structure stored at offset 0 of each device.
/// Provides device identification, crash recovery info, and version compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Superblock {
    /// Magic number for identification (0x43465321 = "CFS!")
    pub magic: u32,
    /// Version of the superblock format
    pub version: u8,
    /// Unique identifier for this device (UUID)
    pub device_uuid: [u8; 16],
    /// Cluster identifier this device belongs to
    pub cluster_uuid: [u8; 16],
    /// Device index in the storage pool
    pub device_idx: u16,
    /// Role of this device in the storage tier
    pub device_role: DeviceRoleCode,
    /// Total device capacity in bytes
    pub capacity_bytes: u64,
    /// Block size in bytes (typically 4096)
    pub block_size: u32,
    /// Total number of 4KB blocks on device
    pub total_blocks_4k: u64,
    /// Offset to allocator bitmap (in 4KB blocks)
    pub alloc_bitmap_offset_4k: u64,
    /// Offset to data area (in 4KB blocks)
    pub data_start_offset_4k: u64,
    /// Creation timestamp (seconds since epoch)
    pub created_at_secs: u64,
    /// Last update timestamp (seconds since epoch)
    pub updated_at_secs: u64,
    /// Number of times this device has been mounted
    pub mount_count: u64,
    /// CRC32C checksum of superblock (excluding this field)
    pub checksum: u32,
    /// Feature flags for future compatibility
    pub feature_flags: u64,
    /// Device label (human-readable name)
    pub label: String,
}

impl Superblock {
    /// Creates a new superblock with computed layout.
    ///
    /// Layout:
    /// - Block 0: Superblock (1 x 4KB)
    /// - Blocks 1..N: Allocator bitmap (ceil(total_blocks / (4096 * 8)) blocks)
    /// - Blocks N+1..: Data area
    pub fn new(
        device_uuid: [u8; 16],
        cluster_uuid: [u8; 16],
        device_idx: u16,
        role: DeviceRoleCode,
        capacity_bytes: u64,
        label: String,
    ) -> Self {
        let total_blocks_4k = capacity_bytes / BLOCK_SIZE as u64;
        let bitmap_bits = total_blocks_4k;
        let bitmap_blocks = bitmap_bits.div_ceil(BLOCK_SIZE as u64 * 8);
        let alloc_bitmap_offset_4k = 1;
        let data_start_offset_4k = 1 + bitmap_blocks;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            magic: SUPERBLOCK_MAGIC,
            version: SUPERBLOCK_VERSION,
            device_uuid,
            cluster_uuid,
            device_idx,
            device_role: role,
            capacity_bytes,
            block_size: BLOCK_SIZE,
            total_blocks_4k,
            alloc_bitmap_offset_4k,
            data_start_offset_4k,
            created_at_secs: now,
            updated_at_secs: now,
            mount_count: 0,
            checksum: 0,
            feature_flags: 0,
            label,
        }
    }

    /// Validates the superblock magic, version, and checksum.
    pub fn validate(&self) -> StorageResult<()> {
        if self.magic != SUPERBLOCK_MAGIC {
            debug!(
                expected = SUPERBLOCK_MAGIC,
                actual = self.magic,
                "invalid superblock magic"
            );
            return Err(StorageError::CorruptedSuperblock {
                reason: format!(
                    "invalid magic: expected {:#x}, got {:#x}",
                    SUPERBLOCK_MAGIC, self.magic
                ),
            });
        }

        if self.version != SUPERBLOCK_VERSION {
            debug!(
                expected = SUPERBLOCK_VERSION,
                actual = self.version,
                "unsupported superblock version"
            );
            return Err(StorageError::CorruptedSuperblock {
                reason: format!(
                    "unsupported version: expected {}, got {}",
                    SUPERBLOCK_VERSION, self.version
                ),
            });
        }

        let computed_checksum = self.compute_checksum();
        if computed_checksum != self.checksum {
            debug!(
                expected = self.checksum,
                actual = computed_checksum,
                "checksum mismatch"
            );
            return Err(StorageError::CorruptedSuperblock {
                reason: format!(
                    "checksum mismatch: expected {:#x}, got {:#x}",
                    self.checksum, computed_checksum
                ),
            });
        }

        Ok(())
    }

    /// Computes the CRC32C checksum of the superblock bytes (excluding checksum field).
    pub fn compute_checksum(&self) -> u32 {
        let mut self_clone = self.clone();
        self_clone.checksum = 0;
        let bytes = bincode::serialize(&self_clone).unwrap_or_default();
        crc32c(&bytes)
    }

    /// Recomputes and stores the checksum.
    pub fn update_checksum(&mut self) {
        self.checksum = self.compute_checksum();
    }

    /// Serializes the superblock to bytes, padded to 4096 bytes.
    pub fn to_bytes(&self) -> StorageResult<Vec<u8>> {
        let mut bytes = bincode::serialize(self).map_err(|e| StorageError::SerializationError {
            reason: e.to_string(),
        })?;

        if bytes.len() > BLOCK_SIZE as usize {
            return Err(StorageError::SerializationError {
                reason: format!(
                    "superblock too large: {} bytes (max {})",
                    bytes.len(),
                    BLOCK_SIZE
                ),
            });
        }

        bytes.resize(BLOCK_SIZE as usize, 0);
        Ok(bytes)
    }

    /// Deserializes a superblock from bytes.
    pub fn from_bytes(data: &[u8]) -> StorageResult<Self> {
        if data.len() < BLOCK_SIZE as usize {
            return Err(StorageError::CorruptedSuperblock {
                reason: format!("data too small: {} bytes (min {})", data.len(), BLOCK_SIZE),
            });
        }

        let superblock: Superblock =
            bincode::deserialize(data).map_err(|e| StorageError::CorruptedSuperblock {
                reason: format!("deserialization failed: {}", e),
            })?;

        Ok(superblock)
    }

    /// Increments the mount count and updates the timestamp.
    pub fn increment_mount_count(&mut self) {
        self.mount_count += 1;
        self.updated_at_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Checks if this device belongs to the specified cluster.
    pub fn is_same_cluster(&self, cluster_uuid: &[u8; 16]) -> bool {
        &self.cluster_uuid == cluster_uuid
    }

    /// Returns the data area size in bytes.
    pub fn data_area_size_bytes(&self) -> u64 {
        let data_blocks = self
            .total_blocks_4k
            .saturating_sub(self.data_start_offset_4k);
        data_blocks * BLOCK_SIZE as u64
    }

    /// Returns the allocator bitmap size in blocks.
    pub fn bitmap_size_blocks(&self) -> u64 {
        self.data_start_offset_4k.saturating_sub(1)
    }
}

/// Generates the CRC32C lookup table at compile time.
const fn make_crc32c_table() -> [u32; 256] {
    const POLY: u32 = 0x82F63B78;
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
}

/// CRC32C implementation using the standard Castagnoli polynomial.
fn crc32c(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = make_crc32c_table();
    let mut crc: u32 = !0;
    for &byte in data {
        let idx = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ TABLE[idx];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_uuid() -> [u8; 16] {
        let mut uuid = [0u8; 16];
        uuid[0..4].copy_from_slice(&0x01020304_u32.to_le_bytes());
        uuid[4..8].copy_from_slice(&0x05060708_u32.to_le_bytes());
        uuid[8..12].copy_from_slice(&0x090A0B0C_u32.to_le_bytes());
        uuid[12..16].copy_from_slice(&0x0D0E0F10_u32.to_le_bytes());
        uuid
    }

    #[test]
    fn test_superblock_creation() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "test-device".to_string(),
        );

        assert_eq!(sb.magic, SUPERBLOCK_MAGIC);
        assert_eq!(sb.version, SUPERBLOCK_VERSION);
        assert_eq!(sb.device_uuid, device_uuid);
        assert_eq!(sb.cluster_uuid, cluster_uuid);
        assert_eq!(sb.device_idx, 0);
        assert_eq!(sb.device_role, DeviceRoleCode::Data);
        assert_eq!(sb.capacity_bytes, 1_000_000_000_000);
        assert_eq!(sb.block_size, BLOCK_SIZE);
        assert_eq!(sb.mount_count, 0);
        assert_eq!(sb.label, "test-device");
        assert!(sb.alloc_bitmap_offset_4k >= 1);
        assert!(sb.data_start_offset_4k > sb.alloc_bitmap_offset_4k);
    }

    #[test]
    fn test_superblock_checksum() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Journal,
            1_000_000_000_000,
            "test".to_string(),
        );

        sb.update_checksum();
        let checksum = sb.checksum;

        assert_ne!(checksum, 0);

        let recomputed = sb.compute_checksum();
        assert_eq!(checksum, recomputed);
    }

    #[test]
    fn test_superblock_serialize_roundtrip() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            1,
            DeviceRoleCode::Combined,
            500_000_000_000,
            "roundtrip-test".to_string(),
        );
        sb.update_checksum();

        let bytes = sb.to_bytes().unwrap();
        assert_eq!(bytes.len(), BLOCK_SIZE as usize);

        let sb2 = Superblock::from_bytes(&bytes).unwrap();

        assert_eq!(sb.magic, sb2.magic);
        assert_eq!(sb.version, sb2.version);
        assert_eq!(sb.device_uuid, sb2.device_uuid);
        assert_eq!(sb.cluster_uuid, sb2.cluster_uuid);
        assert_eq!(sb.device_idx, sb2.device_idx);
        assert_eq!(sb.device_role, sb2.device_role);
        assert_eq!(sb.capacity_bytes, sb2.capacity_bytes);
        assert_eq!(sb.checksum, sb2.checksum);
        assert_eq!(sb.label, sb2.label);
    }

    #[test]
    fn test_superblock_validate() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "valid".to_string(),
        );
        sb.update_checksum();

        assert!(sb.validate().is_ok());
    }

    #[test]
    fn test_superblock_invalid_magic() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "invalid-magic".to_string(),
        );
        sb.update_checksum();
        sb.magic = 0xDEADBEEF;

        let result = sb.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_superblock_invalid_checksum() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "invalid-checksum".to_string(),
        );
        sb.update_checksum();
        sb.checksum = 0xDEADBEEF;

        let result = sb.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_layout_computation() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let capacity: u64 = 1_000_000_000_000;
        let sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            capacity,
            "layout-test".to_string(),
        );

        let total_blocks = capacity / BLOCK_SIZE as u64;
        let bitmap_blocks = total_blocks.div_ceil(BLOCK_SIZE as u64 * 8);

        assert_eq!(sb.alloc_bitmap_offset_4k, 1);
        assert_eq!(sb.data_start_offset_4k, 1 + bitmap_blocks);
        assert_eq!(sb.total_blocks_4k, total_blocks);

        let data_blocks = sb.total_blocks_4k - sb.data_start_offset_4k;
        assert_eq!(sb.data_area_size_bytes(), data_blocks * BLOCK_SIZE as u64);
    }

    #[test]
    fn test_mount_count() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "mount-test".to_string(),
        );

        assert_eq!(sb.mount_count, 0);

        sb.increment_mount_count();
        assert_eq!(sb.mount_count, 1);

        sb.increment_mount_count();
        sb.increment_mount_count();
        assert_eq!(sb.mount_count, 3);
    }

    #[test]
    fn test_cluster_membership() {
        let device_uuid = create_test_uuid();

        let cluster_a = create_test_uuid();
        let mut cluster_b = create_test_uuid();
        cluster_b[0] = !cluster_b[0];

        let sb = Superblock::new(
            device_uuid,
            cluster_a,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "cluster-test".to_string(),
        );

        assert!(sb.is_same_cluster(&cluster_a));
        assert!(!sb.is_same_cluster(&cluster_b));
    }

    #[test]
    fn test_device_role_codes() {
        assert_eq!(DeviceRoleCode::Journal as u8, 0);
        assert_eq!(DeviceRoleCode::Data as u8, 1);
        assert_eq!(DeviceRoleCode::Combined as u8, 2);

        let role_journal = DeviceRoleCode::Journal;
        let role_data = DeviceRoleCode::Data;
        let _role_combined = DeviceRoleCode::Combined;

        assert_eq!(role_journal, role_journal);
        assert_ne!(role_journal, role_data);

        assert_eq!(DeviceRoleCode::default(), DeviceRoleCode::Combined);
    }

    #[test]
    fn test_device_role_conversion() {
        use crate::device::DeviceRole;

        assert_eq!(
            DeviceRoleCode::from(DeviceRole::Journal),
            DeviceRoleCode::Journal
        );
        assert_eq!(DeviceRoleCode::from(DeviceRole::Data), DeviceRoleCode::Data);
        assert_eq!(
            DeviceRoleCode::from(DeviceRole::Combined),
            DeviceRoleCode::Combined
        );
    }

    #[test]
    fn test_from_bytes_too_small() {
        let small_data = vec![0u8; 100];
        let result = Superblock::from_bytes(&small_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_too_large() {
        let device_uuid = create_test_uuid();
        let cluster_uuid = create_test_uuid();

        let sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "x".repeat(10000),
        );

        let result = sb.to_bytes();
        if result.is_err() {
            assert!(matches!(
                result,
                Err(StorageError::SerializationError { .. })
            ));
        }
    }
}
