//! Core block types for the storage subsystem.

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
