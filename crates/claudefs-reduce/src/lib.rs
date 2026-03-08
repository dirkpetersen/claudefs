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
/// Bloom filter for efficient cross-site dedup detection.
pub mod dedup_bloom;
pub mod dedupe;
pub mod encryption;
pub mod error;
pub mod eviction_scorer;
pub mod fingerprint;
pub mod gc;
pub mod journal_segment;
/// Journal replay for crash recovery and inode reconstruction.
pub mod journal_replay;
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
/// Namespace tree structure for inode hierarchy (crash recovery support).
pub mod namespace_tree;
pub mod tiering;
pub mod write_amplification;
/// Distributed dedup coordinator with consistent hash shard routing.
pub mod dedup_coordinator;
/// Reference counting table for CAS block lifecycle tracking.
pub mod refcount_table;
/// Full data reduction pipeline orchestrator (ingest through tiering).
pub mod pipeline_orchestrator;
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
/// Segment-level garbage collection integration.
pub mod segment_gc;
/// Checksum store for end-to-end data integrity.
pub mod checksum_store;
/// Chunk reference tracking for GC coordination.
pub mod chunk_tracker;
/// Consistent hash ring for shard/node assignment.
pub mod hash_ring;
/// Pipeline backpressure for memory management.
pub mod pipeline_backpressure;
/// Append-only write journal for ordered write tracking.
pub mod write_journal;
pub mod ingest_pipeline;
pub mod prefetch_manager;
pub mod dedup_index;
pub mod object_store_bridge;
pub mod chunk_pool;
pub mod recovery_scanner;
/// GC wave coordinator for multi-phase garbage collection.
pub mod gc_coordinator;
/// Block-level snapshot diff for incremental replication.
pub mod snapshot_diff;
/// Write fence for crash-consistent write ordering.
pub mod write_fence;
/// Inline dedup decision engine for hot-path write path.
pub mod inline_dedup;
/// Compression algorithm advisor based on observed ratios.
pub mod compression_advisor;
/// LRU cache for dedup hash lookups.
pub mod dedup_cache;
/// Flash layer write pressure tracking (D6 high/critical watermarks).
pub mod segment_pressure;
/// Per-file encryption key derivation via HKDF.
pub mod key_derivation;
/// Per-segment statistics collection and aggregation.
pub mod segment_stats;
/// Single-chunk data reduction pipeline (dedup→compress→encrypt).
pub mod chunk_pipeline;
/// Eviction policy engine for flash layer management.
pub mod eviction_policy;
/// Replication filter using Bloom filter for cross-site dedup.
pub mod replication_filter;
/// Compression statistics collection and aggregation.
pub mod compression_stats;
/// Super-feature delta index for similarity-based compression.
pub mod delta_index;
/// 64MB S3 blob assembler for tiered storage object packing.
pub mod object_assembler;
/// Flash defragmentation planner for slot consolidation (D6 priority).
pub mod defrag_planner;
/// Read amplification tracker for performance monitoring.
pub mod read_amplification;
/// Key rotation orchestrator for envelope encryption and key lifecycle management.
pub mod key_rotation_orchestrator;
/// WORM retention enforcement with compliance audit logging.
pub mod worm_retention_enforcer;
/// Persistence and recovery for key rotation checkpoints.
pub mod rotation_checkpoint;
/// Tier 2 similarity detection and delta compression coordinator.
pub mod similarity_coordinator;
/// AI-assisted data classification for workload-specific tiering.
pub mod adaptive_classifier;
/// Crash recovery and cross-shard consistency verification.
pub mod recovery_enhancer;
/// Detailed monitoring and metrics for Tier 2 pipeline effectiveness.
pub mod similarity_tier_stats;
/// Per-tenant storage quotas for multi-tenancy and cost attribution.
pub mod multi_tenant_quotas;
/// Machine learning-inspired intelligent tiering advisor.
pub mod tiering_advisor;

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
pub use segment_gc::{
    SegmentGc, SegmentGcAction, SegmentGcConfig, SegmentGcReport, SegmentInfo,
};
pub use checksum_store::{
    ChecksumEntry, ChecksumStore, ChecksumStoreConfig, ChecksumVerifyResult,
};
pub use pipeline_backpressure::{
    BackpressureConfig, BackpressureState, BackpressureStats, PipelineBackpressure,
};
pub use ingest_pipeline::{
    IngestChunk, IngestConfig, IngestMetrics, IngestPipeline, IngestStage,
};
pub use prefetch_manager::{
    PrefetchEntry, PrefetchError, PrefetchManager, PrefetchManagerConfig, PrefetchRequest,
    PrefetchStatus,
};
pub use dedup_index::{
    DedupIndex, DedupIndexConfig, DedupIndexEntry, DedupIndexStats,
};
pub use object_store_bridge::{
    MemoryObjectStore, ObjectKey, ObjectMetadata, ObjectStoreStats, StoreResult,
};
pub use chunk_pool::{ChunkPool, PoolConfig, PoolStats, PooledBuffer};
pub use chunk_tracker::{ChunkRecord, ChunkState, ChunkTracker, TrackerStats};
pub use hash_ring::{HashRing, HashRingConfig, RingMember, RingStats};
pub use write_journal::{
    JournalEntryData, WriteJournal, WriteJournalConfig, WriteJournalStats,
};
pub use recovery_scanner::{
    RecoveryEntry, RecoveryError, RecoveryReport, RecoveryScanner, RecoveryScannerConfig,
    SegmentHeader,
};
pub use dedup_bloom::{BloomConfig, BloomStats, DedupBloom};
pub use journal_replay::{
    InodeReplayState, JournalReplayer, ReplayAction, ReplayConfig, ReplayState, ReplayStats,
};
pub use namespace_tree::{DirEntry, DirId, NamespaceTree};
pub use gc_coordinator::{
    GcCandidate, GcCoordinator, GcCoordinatorConfig, GcPhase, GcWaveStats,
};
pub use snapshot_diff::{
    BlockHash, SnapshotBlock, SnapshotDiff, SnapshotDiffConfig, SnapshotDiffResult,
};
pub use write_fence::{FenceState, WriteFence, WriteFenceConfig, WriteFenceStats};
pub use inline_dedup::{
    DedupDecision, InlineDedup, InlineDedupConfig, InlineDedupIndex, InlineDedupStats,
    SkipReason,
};
pub use compression_advisor::{
    AlgoMetrics, CompressionAdvice, CompressionAdvisor,
};
pub use dedup_cache::{DedupCache, DedupCacheConfig, DedupCacheStats};
pub use segment_pressure::{
    PressureLevel, PressureStats, SegmentPressure, SegmentPressureConfig,
};
pub use key_derivation::{
    DerivedKey, KeyDerivation, KeyDerivationConfig, KeyDerivationStats, MasterKey,
};
pub use segment_stats::{
    AggregatedSegmentStats, SegmentLifecycle, SegmentStat, SegmentStatsCollector,
};
pub use chunk_pipeline::{
    ChunkPipeline, ChunkPipelineConfig, ChunkPipelineResult, ChunkPipelineStats,
};
pub use eviction_policy::{
    EvictableSegment, EvictionPassStats, EvictionPolicy, EvictionPolicyConfig, EvictionStrategy,
};
pub use replication_filter::{
    ReplicationFilter, ReplicationFilterConfig, ReplicationFilterStats,
};

pub use compression_stats::{
    AggregatedCompressionStats, CompressionBucket, CompressionStats, CompressionStatsConfig,
};
pub use delta_index::{
    DeltaIndex, DeltaIndexConfig, DeltaIndexEntry, DeltaIndexStats, SuperFeature,
};
pub use object_assembler::{
    BlobKey, ChunkLocation as BlobChunkLocation, CompletedBlob, ObjectAssembler,
    ObjectAssemblerConfig, ObjectAssemblerStats,
};

pub use dedup_coordinator::{
    DedupCoordinator, DedupCoordinatorConfig, DedupCoordinatorStats, DedupLookupResult,
    NodeFingerprintStore, ShardId,
};
pub use refcount_table::{
    RefcountTable, RefcountTableConfig, RefcountTableStats, RefEntry,
};
pub use pipeline_orchestrator::{
    OrchestratorState, PipelineOrchestrator, PipelineOrchestratorConfig, PipelineStage,
    StageMetricsData,
};
pub use defrag_planner::{
    DefragAction, DefragPlanner, DefragPlannerConfig, DefragPlannerStats, SegmentSlotInfo,
    SlotState,
};
pub use read_amplification::{
    ReadAmplificationConfig, ReadAmplificationStats, ReadAmplificationTracker, ReadEvent,
};
pub use key_rotation_orchestrator::{
    KeyRotationOrchestrator, RotationMetrics, RotationPhase, RotationSchedule,
};
pub use worm_retention_enforcer::{
    AuditLogEntry, ComplianceHold, RetentionPolicy, RetentionType, WormRetentionEnforcer,
};
pub use rotation_checkpoint::{
    RecoveryInfo, RotationCheckpoint, RotationCheckpointStore, RotationRecovery,
};
// pub use similarity_coordinator::{  // TODO: A3 to regenerate
//     SimilarityCoordinator, SimilarityCoordinatorConfig, SimilarityJob, SimilarityMetrics,
//     SimilarityPhase,
// };
// pub use adaptive_classifier::{  // TODO: A3 to regenerate
//     AdaptiveClassifier, AdaptiveClassifierConfig, ClassificationHint, ClassificationMetrics,
//     DataWorkload, TieringAdvice, WorkloadDetector,
// };
// pub use recovery_enhancer::{  // TODO: A3 to regenerate
//     ConsistencyReport, IncompleteReduction, RecoveryCheckpoint, RecoveryEnhancer,
//     RecoveryEnhancerConfig, RecoveryMetrics,
// };
// pub use similarity_tier_stats::{  // TODO: A3 to regenerate
//     SimilarityTierStats, SimilarityTierStatsConfig, TierStats, WorkloadTierStats,
// };