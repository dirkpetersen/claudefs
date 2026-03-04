#![warn(missing_docs)]

//! ClaudeFS reduction subsystem: Inline dedupe (BLAKE3), compression (LZ4/Zstd), encryption (AES-GCM)
//!
//! Write path: Data → Chunk (FastCDC) → Fingerprint (BLAKE3) → Dedupe → Compress → Encrypt → Store
//! Read path:  Store → Decrypt → Decompress → Data

pub mod async_meta_bridge;
pub mod audit_log;
pub mod background;
pub mod block_map;
pub mod cache_coherency;
pub mod checksum;
pub mod chunk_verifier;
pub mod compaction;
pub mod erasure_codec;
pub mod quota_tracker;
pub mod compression;
pub mod data_classifier;
pub mod dedupe;
pub mod encryption;
pub mod error;
pub mod eviction_scorer;
pub mod fingerprint;
pub mod gc;
pub mod journal_segment;
pub mod key_manager;
pub mod key_rotation_scheduler;
pub mod meta_bridge;
pub mod metrics;
pub mod pipeline;
pub mod pipeline_monitor;
pub mod prefetch;
pub mod read_cache;
pub mod read_planner;
pub mod recompressor;
pub mod segment;
pub mod segment_catalog;
pub mod segment_reader;
pub mod segment_splitter;
pub mod similarity;
pub mod snapshot;
pub mod stripe_coordinator;
pub mod stream_chunker;
pub mod tenant_isolator;
pub mod tiering;
pub mod write_amplification;
pub mod write_path;
pub mod write_buffer;
pub mod dedup_pipeline;
pub mod compaction_scheduler;
/// WORM compliance and retention policy enforcement.
pub mod worm_reducer;
/// Snapshot catalog for efficient snapshot management.
pub mod snapshot_catalog;
/// Chunk I/O scheduling with priority-based queue.
pub mod chunk_scheduler;
/// Tier migration policies for flash-to-S3 data movement.
pub mod tier_migration;
/// Encryption key store for managing versioned data encryption keys.
pub mod key_store;
/// Bandwidth throttling for background data reduction operations.
pub mod bandwidth_throttle;
/// Dedup analytics for capacity planning and reporting.
pub mod dedup_analytics;
/// Chunk rebalancing for cluster load distribution.
pub mod chunk_rebalancer;
/// Write coalescing for merging adjacent writes.
pub mod write_coalescer;
/// EC repair planning for degraded segments.
pub mod ec_repair;

pub use async_meta_bridge::{
    AsyncFingerprintStore, AsyncIntegratedWritePath, AsyncLocalFingerprintStore,
    AsyncNullFingerprintStore,
};
pub use audit_log::{AuditEvent, AuditEventKind, AuditLog, AuditLogConfig};
pub use checksum::{ChecksumAlgorithm, ChecksummedBlock, DataChecksum};
pub use compression::CompressionAlgorithm;
pub use dedupe::{CasIndex, Chunk, Chunker, ChunkerConfig};
pub use encryption::{EncryptedChunk, EncryptionAlgorithm, EncryptionKey};
pub use error::ReduceError;
pub use fingerprint::{ChunkHash, SuperFeatures};
pub use gc::{GcConfig, GcEngine, GcStats};
pub use key_manager::{DataKey, KeyManager, KeyManagerConfig, KeyVersion, VersionedKey, WrappedKey};
pub use metrics::{MetricKind, MetricValue, MetricsHandle, MetricsSnapshot, ReduceMetric, ReductionMetrics};
pub use pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats};
pub use background::{BackgroundConfig, BackgroundHandle, BackgroundProcessor, BackgroundStats, BackgroundTask};
pub use segment::{Segment, SegmentEntry, SegmentPacker, SegmentPackerConfig};
pub use segment_catalog::{CatalogConfig, ChunkLocation, SegmentCatalog};
pub use segment_reader::SegmentReader;
pub use similarity::{DeltaCompressor, SimilarityIndex};
pub use meta_bridge::{BlockLocation, FingerprintStore, LocalFingerprintStore, NullFingerprintStore};
pub use recompressor::{RecompressedChunk, RecompressionStats, RecompressorConfig, Recompressor};
pub use snapshot::{Snapshot, SnapshotConfig, SnapshotInfo, SnapshotManager};
pub use tiering::{AccessRecord, TierClass, TierConfig, TierTracker};
pub use write_path::{IntegratedWritePath, WritePathConfig, WritePathResult, WritePathStats};
pub use compaction::{CompactionConfig, CompactionEngine, CompactionResult};
pub use erasure_codec::{EcStripe, EncodedSegment, ErasureCodec};
pub use quota_tracker::{NamespaceId, QuotaConfig, QuotaTracker, QuotaUsage, QuotaViolation};
pub use prefetch::{AccessHistory, AccessPattern, PrefetchConfig, PrefetchHint, PrefetchTracker};
pub use read_cache::{CacheStats, ReadCache, ReadCacheConfig};
pub use stream_chunker::{StreamChunkResult, StreamChunker, StreamChunkerConfig, StreamingStats};
pub use chunk_verifier::{
    ChunkVerifier, ChunkVerifierConfig, VerificationPriority, VerificationResult,
    VerificationSchedule, VerificationStats,
};
pub use pipeline_monitor::{
    AlertThreshold, PipelineAlert, PipelineMetrics, PipelineMonitor, StageMetrics,
};
pub use write_amplification::{
    WriteAmplificationConfig, WriteAmplificationStats, WriteAmplificationTracker, WriteEvent,
};
pub use data_classifier::{
    ClassificationResult, CompressionHint, DataClass, DataClassifier,
};
pub use eviction_scorer::{
    EvictionCandidate, EvictionConfig, EvictionScorer, EvictionStats, SegmentEvictionInfo,
};
pub use segment_splitter::{
    ChunkRef, SegmentPlan, SegmentSplitter, SplitStats, SplitterConfig,
};
pub use block_map::{BlockEntry, BlockMap, BlockMapStore, LogicalRange};
pub use journal_segment::{
    JournalConfig, JournalEntry, JournalError, JournalSegment, JournalState,
};
pub use tenant_isolator::{
    TenantError, TenantId, TenantIsolator, TenantPolicy, TenantPriority, TenantUsage,
};
pub use cache_coherency::{
    CacheEntry, CacheKey, CacheVersion, CoherencyTracker, InvalidationEvent,
};
pub use stripe_coordinator::{
    EcConfig, NodeId, ShardPlacement, StripeCoordinator, StripePlan, StripeStats,
};
pub use read_planner::{
    CachedChunkInfo, ChunkFetchPlan, ReadPlan, ReadPlanner, ReadRequest,
};
pub use write_buffer::{
    FlushReason, FlushResult, PendingWrite, WriteBuffer, WriteBufferConfig,
};
pub use dedup_pipeline::{
    DedupPipeline, DedupPipelineConfig, DedupResult, DedupStats,
};
pub use compaction_scheduler::{
    CompactionJob, CompactionPriority, CompactionScheduler, CompactionSchedulerConfig,
    SchedulerStats,
};
pub use snapshot_catalog::{SnapshotCatalog, SnapshotId, SnapshotRecord};
pub use chunk_scheduler::{
    ChunkOp, ChunkScheduler, OpPriority, ScheduledOp, SchedulerConfig, SchedulerError,
};
pub use tier_migration::{
    MigrationCandidate, MigrationConfig, MigrationDirection, MigrationStats, TierMigrator,
};
pub use key_store::{KeyStore, KeyStoreConfig, KeyStoreStats, StoredKey};
pub use bandwidth_throttle::{
    BandwidthThrottle, ThrottleConfig, ThrottleDecision, ThrottleStats, TokenBucket,
};
pub use dedup_analytics::{DedupAnalytics, DedupSample, DedupTrend};
pub use chunk_rebalancer::{
    ChunkRebalancer, NodeLoad, RebalanceAction, RebalancePlan, RebalancerConfig,
};
pub use write_coalescer::{CoalesceConfig, CoalescedWrite, WriteCoalescer, WriteOp};
pub use ec_repair::{EcRepair, RepairAssessment, RepairPlan, ShardState};