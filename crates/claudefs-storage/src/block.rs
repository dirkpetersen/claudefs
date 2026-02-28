//! Core block types for the storage subsystem.

use core::fmt;
use serde::{Deserialize, Serialize};

/// Unique identifier for a block on a specific device.
/// Contains device index and block offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId {
    /// Index of the NVMe device (0-based)
    pub device_idx: u16,
    /// Block offset within the device (in units of the block's size class)
    pub offset: u64,
}

impl BlockId {
    /// Creates a new BlockId with the given device index and offset.
    pub fn new(device_idx: u16, offset: u64) -> Self {
        Self { device_idx, offset }
    }

    /// Returns the byte offset for this block given its size class.
    pub fn byte_offset(&self, size: BlockSize) -> u64 {
        self.offset * size.as_bytes()
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlockId({}, {})", self.device_idx, self.offset)
    }
}

/// Supported block size classes for the buddy allocator.
/// Per decisions.md D1: EC unit is 2MB packed segments.
/// The allocator manages these power-of-two block sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum BlockSize {
    /// 4KB — metadata, small inodes, journal entries
    B4K = 4096,
    /// 64KB — small file data, directory entries
    B64K = 65536,
    /// 1MB — medium file chunks, post-reduction data
    B1M = 1_048_576,
    /// 64MB — S3 blob chunks (per architecture: 64MB blob chunks to S3)
    B64M = 67_108_864,
}

impl BlockSize {
    /// Returns the size in bytes.
    pub fn as_bytes(&self) -> u64 {
        match self {
            BlockSize::B4K => 4096,
            BlockSize::B64K => 65536,
            BlockSize::B1M => 1_048_576,
            BlockSize::B64M => 67_108_864,
        }
    }

    /// Returns the matching BlockSize variant for the given byte count, or None if unsupported.
    pub fn from_bytes(bytes: u64) -> Option<BlockSize> {
        match bytes {
            4096 => Some(BlockSize::B4K),
            65536 => Some(BlockSize::B64K),
            1_048_576 => Some(BlockSize::B1M),
            67_108_864 => Some(BlockSize::B64M),
            _ => None,
        }
    }

    /// Returns all supported block sizes in ascending order.
    pub fn all() -> &'static [BlockSize] {
        &[
            BlockSize::B4K,
            BlockSize::B64K,
            BlockSize::B1M,
            BlockSize::B64M,
        ]
    }
}

impl fmt::Display for BlockSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockSize::B4K => write!(f, "4KB"),
            BlockSize::B64K => write!(f, "64KB"),
            BlockSize::B1M => write!(f, "1MB"),
            BlockSize::B64M => write!(f, "64MB"),
        }
    }
}

/// Reference to a block with its size class, for the allocator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockRef {
    /// The block identifier.
    pub id: BlockId,
    /// The size class of this block.
    pub size: BlockSize,
}

/// FDP (Flexible Data Placement) hint for NVMe write tagging.
/// Per hardware.md: FDP allows tagging writes with placement hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PlacementHint {
    /// Metadata: journal entries, inode blocks, Raft log
    Metadata = 0,
    /// Hot data: recently written, frequently accessed
    HotData = 1,
    /// Warm data: moderate access frequency
    WarmData = 2,
    /// Cold data: aged-out, candidate for S3 tiering
    ColdData = 3,
    /// Snapshot: CoW snapshot blocks
    Snapshot = 4,
    /// Journal: write-ahead log / write journal entries
    Journal = 5,
}

impl fmt::Display for PlacementHint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlacementHint::Metadata => write!(f, "Metadata"),
            PlacementHint::HotData => write!(f, "HotData"),
            PlacementHint::WarmData => write!(f, "WarmData"),
            PlacementHint::ColdData => write!(f, "ColdData"),
            PlacementHint::Snapshot => write!(f, "Snapshot"),
            PlacementHint::Journal => write!(f, "Journal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_id_creation() {
        let id = BlockId::new(0, 42);
        assert_eq!(id.device_idx, 0);
        assert_eq!(id.offset, 42);
    }

    #[test]
    fn test_block_id_byte_offset() {
        let id = BlockId::new(1, 100);
        assert_eq!(id.byte_offset(BlockSize::B4K), 100 * 4096);
        assert_eq!(id.byte_offset(BlockSize::B64K), 100 * 65536);
        assert_eq!(id.byte_offset(BlockSize::B1M), 100 * 1_048_576);
        assert_eq!(id.byte_offset(BlockSize::B64M), 100 * 67_108_864);
    }

    #[test]
    fn test_block_size_as_bytes() {
        assert_eq!(BlockSize::B4K.as_bytes(), 4096);
        assert_eq!(BlockSize::B64K.as_bytes(), 65536);
        assert_eq!(BlockSize::B1M.as_bytes(), 1_048_576);
        assert_eq!(BlockSize::B64M.as_bytes(), 67_108_864);
    }

    #[test]
    fn test_block_size_from_bytes() {
        assert_eq!(BlockSize::from_bytes(4096), Some(BlockSize::B4K));
        assert_eq!(BlockSize::from_bytes(65536), Some(BlockSize::B64K));
        assert_eq!(BlockSize::from_bytes(1_048_576), Some(BlockSize::B1M));
        assert_eq!(BlockSize::from_bytes(67_108_864), Some(BlockSize::B64M));
        assert_eq!(BlockSize::from_bytes(1234), None);
        assert_eq!(BlockSize::from_bytes(0), None);
    }

    #[test]
    fn test_block_size_all() {
        let all = BlockSize::all();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0], BlockSize::B4K);
        assert_eq!(all[1], BlockSize::B64K);
        assert_eq!(all[2], BlockSize::B1M);
        assert_eq!(all[3], BlockSize::B64M);
    }

    #[test]
    fn test_block_ref() {
        let id = BlockId::new(2, 50);
        let block_ref = BlockRef {
            id,
            size: BlockSize::B1M,
        };
        assert_eq!(block_ref.id.device_idx, 2);
        assert_eq!(block_ref.id.offset, 50);
        assert_eq!(block_ref.size, BlockSize::B1M);
    }

    #[test]
    fn test_display_impls() {
        let id = BlockId::new(1, 100);
        assert_eq!(format!("{}", id), "BlockId(1, 100)");

        assert_eq!(format!("{}", BlockSize::B4K), "4KB");
        assert_eq!(format!("{}", BlockSize::B64K), "64KB");
        assert_eq!(format!("{}", BlockSize::B1M), "1MB");
        assert_eq!(format!("{}", BlockSize::B64M), "64MB");

        assert_eq!(format!("{}", PlacementHint::Metadata), "Metadata");
        assert_eq!(format!("{}", PlacementHint::HotData), "HotData");
        assert_eq!(format!("{}", PlacementHint::WarmData), "WarmData");
        assert_eq!(format!("{}", PlacementHint::ColdData), "ColdData");
        assert_eq!(format!("{}", PlacementHint::Snapshot), "Snapshot");
        assert_eq!(format!("{}", PlacementHint::Journal), "Journal");
    }
}
