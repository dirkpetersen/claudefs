#![warn(missing_docs)]

//! ClaudeFS storage subsystem: Local NVMe I/O via io_uring, FDP/ZNS placement, block allocator
//!
//! This crate provides the foundational block storage layer for ClaudeFS.
//! It manages NVMe devices via io_uring passthrough, handles block allocation
//! using a buddy allocator, and supports FDP/ZNS data placement hints.

pub mod allocator;
pub mod block;
pub mod checksum;
pub mod device;
pub mod engine;
pub mod error;
pub mod flush;
pub mod io_uring_bridge;
pub mod segment;
pub mod zns;

pub use block::{BlockId, BlockRef, BlockSize, PlacementHint};
pub use checksum::{Checksum, ChecksumAlgorithm, BlockHeader};
pub use device::{DeviceConfig, DevicePool, DeviceRole, ManagedDevice, NvmeDeviceInfo, DeviceHealth};
pub use allocator::{BuddyAllocator, AllocatorConfig, AllocatorStats};
pub use engine::{StorageEngine, StorageEngineConfig, StorageEngineStats};
pub use error::{StorageError, StorageResult};
pub use io_uring_bridge::{IoEngine, MockIoEngine, IoStats, IoRequestId, IoOpType};
pub use segment::{SegmentPacker, SegmentPackerConfig, PackedSegment, SegmentHeader, SegmentEntry, SegmentPackerStats, SEGMENT_SIZE};
