#![warn(missing_docs)]

//! ClaudeFS storage subsystem: Local NVMe I/O via io_uring, FDP/ZNS placement, block allocator
//!
//! This crate provides the foundational block storage layer for ClaudeFS.
//! It manages NVMe devices via io_uring passthrough, handles block allocation
//! using a buddy allocator, and supports FDP/ZNS data placement hints.

pub mod allocator;
pub mod block;
pub mod capacity;
pub mod checksum;
pub mod defrag;
pub mod device;
pub mod engine;
pub mod error;
pub mod fdp;
pub mod flush;
pub mod io_uring_bridge;
pub mod segment;
pub mod superblock;
pub mod zns;

#[cfg(feature = "uring")]
pub mod uring_engine;

pub use block::{BlockId, BlockRef, BlockSize, PlacementHint};
pub use capacity::{CapacityTracker, CapacityLevel, CapacityTrackerStats, SegmentTracker, TierOverride, WatermarkConfig};
pub use checksum::{Checksum, ChecksumAlgorithm, BlockHeader};
pub use defrag::{DefragConfig, DefragEngine, DefragPlan, DefragStats, FragmentationReport, SizeClassFragmentation, BlockRelocation};
pub use device::{DeviceConfig, DevicePool, DeviceRole, ManagedDevice, NvmeDeviceInfo, DeviceHealth};
pub use fdp::{FdpConfig, FdpHandle, FdpHintManager, FdpStats};
pub use allocator::{BuddyAllocator, AllocatorConfig, AllocatorStats};
pub use engine::{StorageEngine, StorageEngineConfig, StorageEngineStats};
pub use error::{StorageError, StorageResult};
pub use io_uring_bridge::{IoEngine, MockIoEngine, IoStats, IoRequestId, IoOpType};
pub use segment::{SegmentPacker, SegmentPackerConfig, PackedSegment, SegmentHeader, SegmentEntry, SegmentPackerStats, SEGMENT_SIZE};
pub use superblock::{Superblock, DeviceRoleCode, SUPERBLOCK_MAGIC, SUPERBLOCK_VERSION};

#[cfg(feature = "uring")]
pub use uring_engine::{UringConfig, UringIoEngine, UringError, UringStats};