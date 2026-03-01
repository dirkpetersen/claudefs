#![warn(missing_docs)]

//! ClaudeFS storage subsystem: Local NVMe I/O via io_uring, FDP/ZNS placement, block allocator
//!
//! This crate provides the foundational block storage layer for ClaudeFS.
//! It manages NVMe devices via io_uring passthrough, handles block allocation
//! using a buddy allocator, and supports FDP/ZNS data placement hints.

pub mod allocator;
pub mod atomic_write;
pub mod block;
pub mod block_cache;
pub mod capacity;
pub mod checksum;
pub mod compaction;
pub mod defrag;
pub mod device;
pub mod encryption;
pub mod engine;
pub mod error;
pub mod fdp;
pub mod flush;
pub mod io_uring_bridge;
pub mod io_scheduler;
pub mod metrics;
pub mod quota;
pub mod qos_storage;
pub mod recovery;
pub mod hot_swap;
pub mod segment;
pub mod s3_tier;
pub mod smart;
pub mod snapshot;
pub mod superblock;
pub mod scrub;
pub mod write_journal;
pub mod wear_leveling;
pub mod zns;

#[cfg(feature = "uring")]
pub mod uring_engine;

pub use atomic_write::{
    AtomicWriteBatch, AtomicWriteCapability, AtomicWriteEngine, AtomicWriteRequest, AtomicWriteStats,
};
pub use block::{BlockId, BlockRef, BlockSize, PlacementHint};
pub use block_cache::{BlockCache, BlockCacheConfig, CacheEntry, CacheStats};
pub use capacity::{CapacityTracker, CapacityLevel, CapacityTrackerStats, SegmentTracker, TierOverride, WatermarkConfig};
pub use checksum::{Checksum, ChecksumAlgorithm, BlockHeader};
pub use compaction::{CompactionConfig, CompactionEngine, CompactionState, CompactionStats, CompactionTask, GcCandidate, SegmentId, SegmentInfo};
pub use defrag::{DefragConfig, DefragEngine, DefragPlan, DefragStats, FragmentationReport, SizeClassFragmentation, BlockRelocation};
pub use device::{DeviceConfig, DevicePool, DeviceRole, ManagedDevice, NvmeDeviceInfo, DeviceHealth};
pub use encryption::{
    EncryptedBlock, EncryptionAlgorithm, EncryptionConfig, EncryptionEngine, EncryptionKey,
    EncryptionStats,
};
pub use fdp::{FdpConfig, FdpHandle, FdpHintManager, FdpStats};
pub use allocator::{BuddyAllocator, AllocatorConfig, AllocatorStats};
pub use engine::{StorageEngine, StorageEngineConfig, StorageEngineStats};
pub use error::{StorageError, StorageResult};
pub use io_uring_bridge::{IoEngine, MockIoEngine, IoStats, IoRequestId, IoOpType};
pub use io_scheduler::{IoPriority, ScheduledIo, IoScheduler, IoSchedulerConfig, IoSchedulerStats};
pub use metrics::{Metric, MetricType, MetricValue, StorageMetrics};
pub use recovery::{
    RecoveryConfig, RecoveryManager, RecoveryPhase, RecoveryReport, RecoveryState,
    JOURNAL_CHECKPOINT_MAGIC, AllocatorBitmap, JournalCheckpoint,
};
pub use scrub::{ScrubConfig, ScrubEngine, ScrubError, ScrubState, ScrubStats};
pub use segment::{SegmentPacker, SegmentPackerConfig, PackedSegment, SegmentHeader, SegmentEntry, SegmentPackerStats, SEGMENT_SIZE};
pub use hot_swap::{
    HotSwapManager, HotSwapStats, HotSwapEvent, HotSwapError,
    DeviceState, DrainProgress, BlockMigration, MigrationState,
};
pub use s3_tier::{
    ObjectStoreBackend, MockObjectStore, MockObjectStoreStats,
    TieringEngine, TieringConfig, TieringMode, TieringStats, S3KeyBuilder,
};
pub use smart::{
    AlertSeverity, HealthStatus, NvmeSmartLog, SmartAlert, SmartAttribute,
    SmartMonitor, SmartMonitorConfig,
};
pub use snapshot::{
    CowMapping, SnapshotId, SnapshotInfo, SnapshotManager, SnapshotState, SnapshotStats,
};
pub use superblock::{Superblock, DeviceRoleCode, SUPERBLOCK_MAGIC, SUPERBLOCK_VERSION};
pub use write_journal::{
    JournalConfig, JournalEntry, JournalOp, JournalStats, SyncMode, WriteJournal,
};
pub use wear_leveling::{
    PlacementAdvice, WearAlert, WearAlertType, WearConfig, WearLevel, WearLevelingEngine,
    WearStats, WritePattern, ZoneWear,
};
pub use quota::{
    QuotaLimit, QuotaUsage, QuotaStatus, TenantQuota, QuotaManager, QuotaStats,
};
pub use qos_storage::{
    BandwidthTracker, IoRequest, IoType, QosDecision, QosEnforcer, QosEnforcerStats, QosPolicy,
    TokenBucket, WorkloadClass,
};

#[cfg(feature = "uring")]
pub use uring_engine::{UringConfig, UringIoEngine, UringStats};