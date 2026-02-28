//! Data reduction pipeline: chunk → deduplicate → compress → encrypt

use crate::{
    compression::{compress, decompress, is_compressible, CompressionAlgorithm},
    dedupe::{CasIndex, Chunker, ChunkerConfig},
    encryption::{
        decrypt, derive_chunk_key, encrypt, EncryptedChunk, EncryptionAlgorithm, EncryptionKey,
        Nonce,
    },
    error::ReduceError,
    fingerprint::ChunkHash,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

/// Configuration for the data reduction pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// FastCDC chunker parameters
    pub chunker: ChunkerConfig,
    /// Compression for the inline hot write path
    pub inline_compression: CompressionAlgorithm,
    /// Compression for background S3 tiering
    pub background_compression: CompressionAlgorithm,
    /// Encryption algorithm
    pub encryption: EncryptionAlgorithm,
    /// Enable deduplication
    pub dedup_enabled: bool,
    /// Enable compression
    pub compression_enabled: bool,
    /// Enable encryption (requires master_key to be set)
    pub encryption_enabled: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunker: ChunkerConfig::default(),
            inline_compression: CompressionAlgorithm::Lz4,
            background_compression: CompressionAlgorithm::Zstd { level: 3 },
            encryption: EncryptionAlgorithm::AesGcm256,
            dedup_enabled: true,
            compression_enabled: true,
            encryption_enabled: false,
        }
    }
}

/// A fully processed chunk ready for storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReducedChunk {
    /// BLAKE3 hash of original chunk (CAS key)
    pub hash: ChunkHash,
    /// Byte offset in the original data stream
    pub offset: u64,
    /// Original uncompressed, unencrypted size in bytes
    pub original_size: usize,
    /// True if this chunk duplicates an existing CAS entry
    pub is_duplicate: bool,
    /// Payload: None if is_duplicate, Some if new chunk
    pub payload: Option<EncryptedChunk>,
    /// Compression algorithm used (needed for decompression)
    pub compression: CompressionAlgorithm,
}

/// Statistics from a pipeline run
#[derive(Debug, Default, Clone)]
pub struct ReductionStats {
    /// Total input bytes
    pub input_bytes: u64,
    /// Total chunks produced
    pub chunks_total: usize,
    /// Chunks eliminated by dedup
    pub chunks_deduplicated: usize,
    /// Bytes after deduplication
    pub bytes_after_dedup: u64,
    /// Bytes after compression
    pub bytes_after_compression: u64,
    /// Bytes after encryption
    pub bytes_after_encryption: u64,
    /// Compression ratio (bytes_after_dedup / bytes_after_compression)
    pub compression_ratio: f64,
    /// Dedup ratio (input_bytes / bytes_after_dedup)
    pub dedup_ratio: f64,
}

/// The data reduction pipeline
pub struct ReductionPipeline {
    config: PipelineConfig,
    cas: CasIndex,
    master_key: Option<EncryptionKey>,
    chunker: Chunker,
}

impl ReductionPipeline {
    /// Create with config and no encryption key
    pub fn new(config: PipelineConfig) -> Self {
        let chunker = Chunker::with_config(config.chunker.clone());
        Self {
            config,
            cas: CasIndex::new(),
            master_key: None,
            chunker,
        }
    }

    /// Create with config and encryption master key
    pub fn with_master_key(config: PipelineConfig, master_key: EncryptionKey) -> Self {
        let chunker = Chunker::with_config(config.chunker.clone());
        Self {
            config,
            cas: CasIndex::new(),
            master_key: Some(master_key),
            chunker,
        }
    }

    /// Process a write: chunk → deduplicate → compress → encrypt.
    /// Returns processed chunks and statistics.
    #[instrument(skip(self, data), fields(input_bytes = data.len()))]
    pub fn process_write(
        &mut self,
        data: &[u8],
    ) -> Result<(Vec<ReducedChunk>, ReductionStats), ReduceError> {
        let mut stats = ReductionStats {
            input_bytes: data.len() as u64,
            ..Default::default()
        };
        let chunks = self.chunker.chunk(data);
        stats.chunks_total = chunks.len();
        let mut results = Vec::with_capacity(chunks.len());

        for chunk in chunks {
            let original_size = chunk.data.len();
            let is_duplicate = self.config.dedup_enabled && self.cas.lookup(&chunk.hash);

            if is_duplicate {
                stats.chunks_deduplicated += 1;
                self.cas.insert(chunk.hash);
                results.push(ReducedChunk {
                    hash: chunk.hash,
                    offset: chunk.offset,
                    original_size,
                    is_duplicate: true,
                    payload: None,
                    compression: CompressionAlgorithm::None,
                });
                continue;
            }

            if self.config.dedup_enabled {
                self.cas.insert(chunk.hash);
            }
            stats.bytes_after_dedup += original_size as u64;

            // Compress
            let (compressed, compression_algo) =
                if self.config.compression_enabled && is_compressible(&chunk.data) {
                    let algo = self.config.inline_compression;
                    let c = compress(&chunk.data, algo)?;
                    debug!(
                        original = original_size,
                        compressed = c.len(),
                        "chunk compressed"
                    );
                    (c, algo)
                } else {
                    (chunk.data.to_vec(), CompressionAlgorithm::None)
                };
            stats.bytes_after_compression += compressed.len() as u64;

            // Encrypt
            let payload = if self.config.encryption_enabled {
                let master = self.master_key.as_ref().ok_or(ReduceError::MissingKey)?;
                let chunk_key = derive_chunk_key(master, chunk.hash.as_bytes());
                let enc = encrypt(&compressed, &chunk_key, self.config.encryption)?;
                stats.bytes_after_encryption += enc.ciphertext.len() as u64;
                enc
            } else {
                // Store compressed bytes in ciphertext field; zero nonce as sentinel
                stats.bytes_after_encryption += compressed.len() as u64;
                EncryptedChunk {
                    ciphertext: compressed,
                    nonce: Nonce([0u8; 12]),
                    algo: EncryptionAlgorithm::AesGcm256,
                }
            };

            results.push(ReducedChunk {
                hash: chunk.hash,
                offset: chunk.offset,
                original_size,
                is_duplicate: false,
                payload: Some(payload),
                compression: compression_algo,
            });
        }

        stats.compression_ratio = if stats.bytes_after_compression > 0 {
            stats.bytes_after_dedup as f64 / stats.bytes_after_compression as f64
        } else {
            1.0
        };
        stats.dedup_ratio = if stats.bytes_after_dedup > 0 {
            stats.input_bytes as f64 / stats.bytes_after_dedup as f64
        } else if stats.input_bytes > 0 {
            f64::INFINITY
        } else {
            1.0
        };

        Ok((results, stats))
    }

    /// Recover data from processed chunks.
    /// Duplicate chunks cannot be recovered in Phase 1 (returns MissingChunkData).
    pub fn process_read(&self, chunks: &[ReducedChunk]) -> Result<Vec<u8>, ReduceError> {
        let mut output = Vec::new();
        for chunk in chunks {
            if chunk.is_duplicate {
                return Err(ReduceError::MissingChunkData);
            }
            let payload = chunk
                .payload
                .as_ref()
                .ok_or(ReduceError::MissingChunkData)?;
            let compressed = if self.config.encryption_enabled {
                let master = self.master_key.as_ref().ok_or(ReduceError::MissingKey)?;
                let chunk_key = derive_chunk_key(master, chunk.hash.as_bytes());
                decrypt(payload, &chunk_key)?
            } else {
                payload.ciphertext.clone()
            };
            let original = decompress(&compressed, chunk.compression)?;
            output.extend_from_slice(&original);
        }
        Ok(output)
    }

    /// Access the CAS index
    pub fn cas(&self) -> &CasIndex {
        &self.cas
    }

    /// Access pipeline configuration
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_no_encryption() {
        let config = PipelineConfig {
            encryption_enabled: false,
            ..Default::default()
        };
        let mut p = ReductionPipeline::new(config);
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let (chunks, stats) = p.process_write(&data).unwrap();
        assert_eq!(stats.input_bytes, data.len() as u64);
        assert!(chunks.iter().all(|c| !c.is_duplicate));
        assert_eq!(p.process_read(&chunks).unwrap(), data);
    }

    #[test]
    fn roundtrip_with_encryption() {
        let config = PipelineConfig {
            encryption_enabled: true,
            ..Default::default()
        };
        let mut p = ReductionPipeline::with_master_key(config, EncryptionKey([0x42u8; 32]));
        let data = b"Hello ClaudeFS!".repeat(10_000);
        let (chunks, _) = p.process_write(&data).unwrap();
        assert_eq!(p.process_read(&chunks).unwrap(), data);
    }

    #[test]
    fn dedup_detects_duplicates() {
        let config = PipelineConfig {
            encryption_enabled: false,
            ..Default::default()
        };
        let mut p = ReductionPipeline::new(config);
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let (chunks1, stats1) = p.process_write(&data).unwrap();
        let (_, stats2) = p.process_write(&data).unwrap();
        assert_eq!(stats1.chunks_deduplicated, 0);
        assert_eq!(stats2.chunks_deduplicated, chunks1.len());
    }

    #[test]
    fn missing_key_error() {
        let config = PipelineConfig {
            encryption_enabled: true,
            ..Default::default()
        };
        let mut p = ReductionPipeline::new(config);
        assert!(matches!(
            p.process_write(b"test"),
            Err(ReduceError::MissingKey)
        ));
    }

    #[test]
    fn empty_input() {
        let config = PipelineConfig {
            encryption_enabled: false,
            ..Default::default()
        };
        let mut p = ReductionPipeline::new(config);
        let (chunks, stats) = p.process_write(&[]).unwrap();
        assert!(chunks.is_empty());
        assert_eq!(stats.input_bytes, 0);
    }
}
