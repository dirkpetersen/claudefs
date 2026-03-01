#![warn(missing_docs)]

//! ClaudeFS reduction subsystem: Inline dedupe (BLAKE3), compression (LZ4/Zstd), encryption (AES-GCM)
//!
//! Write path: Data → Chunk (FastCDC) → Fingerprint (BLAKE3) → Dedupe → Compress → Encrypt → Store
//! Read path:  Store → Decrypt → Decompress → Data

pub mod background;
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
pub mod recompressor;
pub mod segment;
pub mod similarity;
pub mod snapshot;
pub mod write_path;
pub mod worm_reducer;

pub use compression::CompressionAlgorithm;
pub use dedupe::{CasIndex, Chunk, Chunker, ChunkerConfig};
pub use encryption::{EncryptedChunk, EncryptionAlgorithm, EncryptionKey};
pub use error::ReduceError;
pub use fingerprint::{ChunkHash, SuperFeatures};
pub use gc::{GcConfig, GcEngine, GcStats};
pub use key_manager::{DataKey, KeyManager, KeyManagerConfig, KeyVersion, WrappedKey};
pub use metrics::{MetricKind, MetricValue, MetricsHandle, MetricsSnapshot, ReduceMetric, ReductionMetrics};
pub use pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats};
pub use background::{BackgroundConfig, BackgroundHandle, BackgroundProcessor, BackgroundStats, BackgroundTask};
pub use segment::{Segment, SegmentEntry, SegmentPacker, SegmentPackerConfig};
pub use similarity::{DeltaCompressor, SimilarityIndex};
pub use meta_bridge::{BlockLocation, FingerprintStore, LocalFingerprintStore, NullFingerprintStore};
pub use recompressor::{RecompressedChunk, RecompressionStats, RecompressorConfig, Recompressor};
pub use snapshot::{Snapshot, SnapshotConfig, SnapshotInfo, SnapshotManager};
pub use write_path::{IntegratedWritePath, WritePathConfig, WritePathResult, WritePathStats};
