#![warn(missing_docs)]

//! ClaudeFS reduction subsystem: Inline dedupe (BLAKE3), compression (LZ4/Zstd), encryption (AES-GCM)
//!
//! Write path: Data → Chunk (FastCDC) → Fingerprint (BLAKE3) → Dedupe → Compress → Encrypt → Store
//! Read path:  Store → Decrypt → Decompress → Data

pub mod compression;
pub mod dedupe;
pub mod encryption;
pub mod error;
pub mod fingerprint;
pub mod pipeline;

pub use compression::CompressionAlgorithm;
pub use dedupe::{CasIndex, Chunk, Chunker, ChunkerConfig};
pub use encryption::{EncryptedChunk, EncryptionAlgorithm, EncryptionKey};
pub use error::ReduceError;
pub use fingerprint::{ChunkHash, SuperFeatures};
pub use pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats};
