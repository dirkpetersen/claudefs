//! Journal batch compression for WAN bandwidth optimization.

use crate::conduit::EntryBatch;
use crate::error::ReplError;
use serde::{Deserialize, Serialize};

/// Compression algorithm for journal batch wire encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompressionAlgo {
    /// No compression.
    None,
    /// LZ4 frame format (low latency, ~2x ratio).
    #[default]
    Lz4,
    /// Zstd (higher ratio, slightly more CPU — good for WAN).
    Zstd,
}

impl CompressionAlgo {
    /// Returns true if this algo actually compresses data.
    pub fn is_compressed(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Compression configuration.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Algorithm to use.
    pub algo: CompressionAlgo,
    /// Zstd compression level (1–22; default 3). Ignored for LZ4/None.
    pub zstd_level: i32,
    /// Minimum uncompressed bytes before attempting compression.
    /// Batches smaller than this are sent uncompressed even if algo != None.
    pub min_compress_bytes: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algo: CompressionAlgo::Lz4,
            zstd_level: 3,
            min_compress_bytes: 256,
        }
    }
}

/// A compressed batch ready for wire transmission.
#[derive(Debug, Clone)]
pub struct CompressedBatch {
    /// Batch sequence number (same as EntryBatch.batch_seq).
    pub batch_seq: u64,
    /// Source site ID.
    pub source_site_id: u64,
    /// Original (uncompressed) byte count.
    pub original_bytes: usize,
    /// Compressed byte count (== original_bytes if algo == None).
    pub compressed_bytes: usize,
    /// Algorithm used.
    pub algo: CompressionAlgo,
    /// Compressed payload (bincode-serialized EntryBatch, then compressed).
    pub data: Vec<u8>,
}

impl CompressedBatch {
    /// Returns the compression ratio (original / compressed). >= 1.0 means compression helped.
    pub fn compression_ratio(&self) -> f64 {
        if self.compressed_bytes == 0 {
            return 1.0;
        }
        self.original_bytes as f64 / self.compressed_bytes as f64
    }

    /// Returns true if compression reduced the size.
    pub fn is_beneficial(&self) -> bool {
        self.compressed_bytes < self.original_bytes
    }
}

/// Compresses and decompresses EntryBatch objects.
pub struct BatchCompressor {
    config: CompressionConfig,
}

impl BatchCompressor {
    /// Create a new compressor with the given config.
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Get the current config.
    pub fn config(&self) -> &CompressionConfig {
        &self.config
    }

    /// Compress an EntryBatch into a CompressedBatch.
    /// Uses bincode for serialization, then applies the configured compression.
    /// Falls back to CompressionAlgo::None if the batch is below min_compress_bytes.
    pub fn compress(&self, batch: &EntryBatch) -> Result<CompressedBatch, ReplError> {
        let serialized =
            bincode::serialize(batch).map_err(|e| ReplError::Compression(e.to_string()))?;
        let original_bytes = serialized.len();

        let effective_algo = if original_bytes < self.config.min_compress_bytes {
            CompressionAlgo::None
        } else {
            self.config.algo
        };

        let compressed_data = match effective_algo {
            CompressionAlgo::None => serialized.clone(),
            CompressionAlgo::Lz4 => lz4_flex::compress_prepend_size(&serialized),
            CompressionAlgo::Zstd => {
                let level = self.config.zstd_level.clamp(1, 22);
                zstd::encode_all(serialized.as_slice(), level)
                    .map_err(|e| ReplError::Compression(e.to_string()))?
            }
        };
        let compressed_bytes = compressed_data.len();

        Ok(CompressedBatch {
            batch_seq: batch.batch_seq,
            source_site_id: batch.source_site_id,
            original_bytes,
            compressed_bytes,
            algo: effective_algo,
            data: compressed_data,
        })
    }

    /// Decompress a CompressedBatch back into an EntryBatch.
    pub fn decompress(&self, compressed: &CompressedBatch) -> Result<EntryBatch, ReplError> {
        let decompressed = self.decompress_bytes(&compressed.data, compressed.algo)?;
        let batch: EntryBatch = bincode::deserialize(&decompressed)
            .map_err(|e| ReplError::Compression(e.to_string()))?;
        Ok(batch)
    }

    /// Compress raw bytes with the configured algorithm.
    /// Returns (compressed_bytes, algo_actually_used).
    pub fn compress_bytes(&self, data: &[u8]) -> Result<(Vec<u8>, CompressionAlgo), ReplError> {
        if self.config.algo == CompressionAlgo::None {
            return Ok((data.to_vec(), CompressionAlgo::None));
        }

        let effective_level = self.config.zstd_level.clamp(1, 22);

        match self.config.algo {
            CompressionAlgo::None => Ok((data.to_vec(), CompressionAlgo::None)),
            CompressionAlgo::Lz4 => {
                let compressed = lz4_flex::compress_prepend_size(data);
                Ok((compressed, CompressionAlgo::Lz4))
            }
            CompressionAlgo::Zstd => {
                let compressed = zstd::encode_all(data, effective_level)
                    .map_err(|e| ReplError::Compression(e.to_string()))?;
                Ok((compressed, CompressionAlgo::Zstd))
            }
        }
    }

    /// Decompress raw bytes with the specified algorithm.
    pub fn decompress_bytes(
        &self,
        data: &[u8],
        algo: CompressionAlgo,
    ) -> Result<Vec<u8>, ReplError> {
        match algo {
            CompressionAlgo::None => Ok(data.to_vec()),
            CompressionAlgo::Lz4 => lz4_flex::decompress_size_prepended(data)
                .map_err(|e| ReplError::Compression(e.to_string())),
            CompressionAlgo::Zstd => {
                zstd::decode_all(data).map_err(|e| ReplError::Compression(e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{JournalEntry, OpKind};

    fn make_test_entry(seq: u64) -> JournalEntry {
        JournalEntry::new(
            seq,
            0,
            1,
            1000 + seq,
            100 + seq,
            OpKind::Write,
            vec![1, 2, 3, 4, 5],
        )
    }

    fn make_test_batch(seq: u64, entry_count: usize) -> EntryBatch {
        let entries: Vec<_> = (0..entry_count)
            .map(|i| make_test_entry(seq + i as u64))
            .collect();
        EntryBatch::new(1, entries, seq)
    }

    #[test]
    fn compression_algo_default_is_lz4() {
        let algo = CompressionAlgo::default();
        assert_eq!(algo, CompressionAlgo::Lz4);
    }

    #[test]
    fn compression_algo_is_compressed_none_false() {
        assert!(!CompressionAlgo::None.is_compressed());
    }

    #[test]
    fn compression_algo_is_compressed_lz4_true() {
        assert!(CompressionAlgo::Lz4.is_compressed());
    }

    #[test]
    fn compression_config_default() {
        let config = CompressionConfig::default();
        assert_eq!(config.algo, CompressionAlgo::Lz4);
        assert_eq!(config.zstd_level, 3);
        assert_eq!(config.min_compress_bytes, 256);
    }

    #[test]
    fn compress_decompress_roundtrip_none() {
        let config = CompressionConfig {
            algo: CompressionAlgo::None,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 10);
        let compressed = compressor.compress(&batch).unwrap();
        assert_eq!(compressed.algo, CompressionAlgo::None);

        let decompressed = compressor.decompress(&compressed).unwrap();
        assert_eq!(decompressed.batch_seq, batch.batch_seq);
        assert_eq!(decompressed.source_site_id, batch.source_site_id);
        assert_eq!(decompressed.entries.len(), batch.entries.len());
    }

    #[test]
    fn compress_decompress_roundtrip_lz4() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Lz4,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 10);
        let compressed = compressor.compress(&batch).unwrap();
        assert_eq!(compressed.algo, CompressionAlgo::Lz4);

        let decompressed = compressor.decompress(&compressed).unwrap();
        assert_eq!(decompressed.batch_seq, batch.batch_seq);
        assert_eq!(decompressed.source_site_id, batch.source_site_id);
        assert_eq!(decompressed.entries.len(), batch.entries.len());
    }

    #[test]
    fn compress_decompress_roundtrip_zstd() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Zstd,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 10);
        let compressed = compressor.compress(&batch).unwrap();
        assert_eq!(compressed.algo, CompressionAlgo::Zstd);

        let decompressed = compressor.decompress(&compressed).unwrap();
        assert_eq!(decompressed.batch_seq, batch.batch_seq);
        assert_eq!(decompressed.source_site_id, batch.source_site_id);
        assert_eq!(decompressed.entries.len(), batch.entries.len());
    }

    #[test]
    fn compress_small_batch_uses_none_algo() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Lz4,
            zstd_level: 3,
            min_compress_bytes: 1000,
        };
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 2);
        let compressed = compressor.compress(&batch).unwrap();
        assert_eq!(compressed.algo, CompressionAlgo::None);
    }

    #[test]
    fn compressed_batch_compression_ratio() {
        let config = CompressionConfig::default();
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 50);
        let compressed = compressor.compress(&batch).unwrap();
        let ratio = compressed.compression_ratio();
        assert!(ratio >= 1.0);
    }

    #[test]
    fn compressed_batch_is_beneficial_when_compressed() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Lz4,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 50);
        let compressed = compressor.compress(&batch).unwrap();
        assert!(compressed.is_beneficial());
    }

    #[test]
    fn compressed_batch_is_beneficial_false_for_none() {
        let config = CompressionConfig {
            algo: CompressionAlgo::None,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 10);
        let compressed = compressor.compress(&batch).unwrap();
        assert!(!compressed.is_beneficial());
    }

    #[test]
    fn compress_bytes_lz4_roundtrip() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Lz4,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let data = vec![0u8; 1000];
        let (compressed, algo) = compressor.compress_bytes(&data).unwrap();
        assert_eq!(algo, CompressionAlgo::Lz4);

        let decompressed = compressor.decompress_bytes(&compressed, algo).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_bytes_zstd_roundtrip() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Zstd,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let data = vec![0u8; 1000];
        let (compressed, algo) = compressor.compress_bytes(&data).unwrap();
        assert_eq!(algo, CompressionAlgo::Zstd);

        let decompressed = compressor.decompress_bytes(&compressed, algo).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_bytes_none_passthrough() {
        let config = CompressionConfig {
            algo: CompressionAlgo::None,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let data = vec![1, 2, 3, 4, 5];
        let (compressed, algo) = compressor.compress_bytes(&data).unwrap();
        assert_eq!(algo, CompressionAlgo::None);
        assert_eq!(compressed, data);
    }

    #[test]
    fn decompress_wrong_algo_returns_error() {
        let config = CompressionConfig::default();
        let compressor = BatchCompressor::new(config);

        let data = vec![0u8; 100];
        let compressed_lz4 = lz4_flex::compress_prepend_size(&data);

        let result = compressor.decompress_bytes(&compressed_lz4, CompressionAlgo::Zstd);
        assert!(result.is_err());
    }

    #[test]
    fn compression_config_custom_zstd_level() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Zstd,
            zstd_level: 10,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);
        assert_eq!(compressor.config().zstd_level, 10);
    }

    #[test]
    fn compress_large_batch_lz4() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Lz4,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 100);
        let compressed = compressor.compress(&batch).unwrap();

        let decompressed = compressor.decompress(&compressed).unwrap();
        assert_eq!(decompressed.entries.len(), 100);
    }

    #[test]
    fn compress_large_batch_zstd() {
        let config = CompressionConfig {
            algo: CompressionAlgo::Zstd,
            zstd_level: 3,
            min_compress_bytes: 0,
        };
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(1, 100);
        let compressed = compressor.compress(&batch).unwrap();

        let decompressed = compressor.decompress(&compressed).unwrap();
        assert_eq!(decompressed.entries.len(), 100);
    }

    #[test]
    fn batch_compressor_config_accessor() {
        let config = CompressionConfig::default();
        let compressor = BatchCompressor::new(config.clone());
        assert_eq!(compressor.config().algo, config.algo);
        assert_eq!(compressor.config().zstd_level, config.zstd_level);
        assert_eq!(
            compressor.config().min_compress_bytes,
            config.min_compress_bytes
        );
    }

    #[test]
    fn compressed_batch_seq_preserved() {
        let config = CompressionConfig::default();
        let compressor = BatchCompressor::new(config);

        let batch = make_test_batch(42, 5);
        let compressed = compressor.compress(&batch).unwrap();
        assert_eq!(compressed.batch_seq, 42);
    }

    #[test]
    fn compressed_batch_site_id_preserved() {
        let config = CompressionConfig::default();
        let compressor = BatchCompressor::new(config);

        let entries = vec![make_test_entry(1)];
        let batch = EntryBatch::new(99, entries, 1);
        let compressed = compressor.compress(&batch).unwrap();
        assert_eq!(compressed.source_site_id, 99);
    }

    #[test]
    fn empty_entries_batch_compress_decompress() {
        let config = CompressionConfig::default();
        let compressor = BatchCompressor::new(config);

        let batch = EntryBatch::new(1, vec![], 1);
        let compressed = compressor.compress(&batch).unwrap();

        let decompressed = compressor.decompress(&compressed).unwrap();
        assert!(decompressed.entries.is_empty());
    }
}
