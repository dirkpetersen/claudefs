#![warn(missing_docs)]

//! ClaudeFS reduction subsystem: Inline dedupe (BLAKE3), compression (LZ4/Zstd), encryption (AES-GCM)
//!
//! Write path: Data → Chunk (FastCDC) → Fingerprint (BLAKE3) → Dedupe → Compress → Encrypt → Store
//! Read path:  Store → Decrypt → Decompress → Data

pub mod async_meta_bridge;
pub mod audit_log;
pub mod background;
pub mod checksum;
pub mod compaction;
pub mod erasure_codec;
pub mod quota_tracker;
pub mod compression;
pub mod dedupe;
pub mod encryption;
pub mod error;
pub mod fingerprint;
pub mod gc;
pub mod key_manager;
pub mod key_rotation_scheduler;
pub mod meta_bridge;
pub mod metrics;
pub mod pipeline;
pub mod prefetch;
pub mod read_cache;
pub mod recompressor;
pub mod segment;
pub mod segment_catalog;
pub mod segment_reader;
pub mod similarity;
pub mod snapshot;
pub mod stream_chunker;
pub mod tiering;
pub mod write_path;
/// WORM compliance and retention policy enforcement.
pub mod worm_reducer;

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