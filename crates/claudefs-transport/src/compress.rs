//! Wire compression for TCP transport paths.
//!
//! This module provides on-the-wire payload compression for non-RDMA transport paths.
//! RDMA paths skip compression since they use zero-copy transfer.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::TransportError;

/// Compression algorithm to use for payload compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompressionAlgorithm {
    /// No compression (pass-through).
    None,
    /// Simple run-length encoding (built-in).
    #[default]
    Rle,
    /// LZ4 compression (future, requires lz4_flex crate).
    Lz4,
    /// Zstandard compression (future, requires zstd crate).
    Zstd,
}

/// Configuration for compression.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Compression algorithm to use.
    pub algorithm: CompressionAlgorithm,
    /// Minimum payload size to compress (in bytes). Payloads below this threshold
    /// are passed through without compression.
    pub min_payload_size: usize,
    /// Whether compression is enabled. When disabled, all payloads pass through
    /// uncompressed regardless of algorithm or size.
    pub enabled: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Rle,
            min_payload_size: 256,
            enabled: true,
        }
    }
}

/// Wire format for compressed data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedPayload {
    /// Algorithm used to compress the data.
    pub algorithm: CompressionAlgorithm,
    /// Original size of the uncompressed data in bytes.
    pub original_size: u32,
    /// Compressed data bytes.
    pub data: Vec<u8>,
}

/// Statistics for compression operations.
#[derive(Debug, Default)]
pub struct CompressionStats {
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    compressions: AtomicU64,
    decompressions: AtomicU64,
    skipped: AtomicU64,
}

/// Snapshot of compression statistics.
#[derive(Debug, Clone, Default)]
pub struct CompressionStatsSnapshot {
    /// Total bytes input to compression.
    pub bytes_in: u64,
    /// Total bytes output from compression.
    pub bytes_out: u64,
    /// Number of compressions performed.
    pub compressions: u64,
    /// Number of decompressions performed.
    pub decompressions: u64,
    /// Number of compressions skipped (below min size or disabled).
    pub skipped: u64,
    /// Compression ratio (bytes_out / bytes_in). Lower is better.
    pub ratio: f64,
}

/// Compressor for on-the-wire payload compression.
#[derive(Debug)]
pub struct Compressor {
    config: CompressionConfig,
    stats: CompressionStats,
}

impl Compressor {
    /// Create a new compressor with the given configuration.
    pub fn new(config: CompressionConfig) -> Self {
        Self {
            config,
            stats: CompressionStats::default(),
        }
    }

    /// Compress data using the configured algorithm.
    ///
    /// If compression is disabled or the payload is below min_payload_size,
    /// returns an uncompressed payload with algorithm=None.
    pub fn compress(&self, data: &[u8]) -> CompressedPayload {
        let original_size = data.len() as u32;

        // Track input bytes
        self.stats
            .bytes_in
            .fetch_add(original_size as u64, Ordering::Relaxed);

        // Check if we should compress
        if !self.config.enabled {
            self.stats.skipped.fetch_add(1, Ordering::Relaxed);
            return CompressedPayload {
                algorithm: CompressionAlgorithm::None,
                original_size,
                data: data.to_vec(),
            };
        }

        if data.len() < self.config.min_payload_size {
            self.stats.skipped.fetch_add(1, Ordering::Relaxed);
            return CompressedPayload {
                algorithm: CompressionAlgorithm::None,
                original_size,
                data: data.to_vec(),
            };
        }

        // Compress based on algorithm
        let (algorithm, compressed_data) = match self.config.algorithm {
            CompressionAlgorithm::None => (CompressionAlgorithm::None, data.to_vec()),
            CompressionAlgorithm::Rle => (CompressionAlgorithm::Rle, rle_compress(data)),
            CompressionAlgorithm::Lz4 => {
                // Future: implement with lz4_flex
                (CompressionAlgorithm::Lz4, data.to_vec())
            }
            CompressionAlgorithm::Zstd => {
                // Future: implement with zstd
                (CompressionAlgorithm::Zstd, data.to_vec())
            }
        };

        let output_size = compressed_data.len() as u64;
        self.stats
            .bytes_out
            .fetch_add(output_size, Ordering::Relaxed);
        self.stats.compressions.fetch_add(1, Ordering::Relaxed);

        CompressedPayload {
            algorithm,
            original_size,
            data: compressed_data,
        }
    }

    /// Decompress a compressed payload.
    pub fn decompress(&self, payload: &CompressedPayload) -> Result<Vec<u8>, TransportError> {
        self.stats.decompressions.fetch_add(1, Ordering::Relaxed);

        match payload.algorithm {
            CompressionAlgorithm::None => Ok(payload.data.clone()),
            CompressionAlgorithm::Rle => rle_decompress(&payload.data),
            CompressionAlgorithm::Lz4 => {
                // Future: implement with lz4_flex
                Err(TransportError::InvalidFrame {
                    reason: "LZ4 compression not yet implemented".to_string(),
                })
            }
            CompressionAlgorithm::Zstd => {
                // Future: implement with zstd
                Err(TransportError::InvalidFrame {
                    reason: "Zstd compression not yet implemented".to_string(),
                })
            }
        }
    }

    /// Get a snapshot of current compression statistics.
    pub fn stats(&self) -> CompressionStatsSnapshot {
        let bytes_in = self.stats.bytes_in.load(Ordering::Relaxed);
        let bytes_out = self.stats.bytes_out.load(Ordering::Relaxed);
        let ratio = if bytes_in > 0 {
            bytes_out as f64 / bytes_in as f64
        } else {
            1.0
        };

        CompressionStatsSnapshot {
            bytes_in,
            bytes_out,
            compressions: self.stats.compressions.load(Ordering::Relaxed),
            decompressions: self.stats.decompressions.load(Ordering::Relaxed),
            skipped: self.stats.skipped.load(Ordering::Relaxed),
            ratio,
        }
    }
}

/// Compress data using simple run-length encoding.
///
/// Format: For runs >= 3 identical bytes, emit [0xFF, byte, count_high, count_low].
/// For non-runs, emit bytes directly.
/// Literal 0xFF bytes (or runs < 3 of 0xFF) are encoded as [0xFF, 0xFF, 0x00, 0x01].
fn rle_compress(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        let current = data[i];
        let mut run_length = 1;

        // Count run of identical bytes
        while i + run_length < data.len() && data[i + run_length] == current {
            run_length += 1;
        }

        // If run >= 3, encode as run
        if run_length >= 3 {
            result.push(0xFF);
            result.push(current);
            result.push((run_length >> 8) as u8);
            result.push((run_length & 0xFF) as u8);
            i += run_length;
            continue;
        }

        // Run < 3: output literal bytes, escaping 0xFF
        let mut j = 0;
        while j < run_length {
            if current == 0xFF {
                // Escape literal 0xFF
                result.push(0xFF);
                result.push(0xFF);
                result.push(0x00);
                result.push(0x01);
            } else {
                result.push(current);
            }
            j += 1;
        }
        i += run_length;
    }

    result
}

/// Decompress RLE-encoded data.
fn rle_decompress(data: &[u8]) -> Result<Vec<u8>, TransportError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();
    let mut i = 0;

    while i < data.len() {
        if data[i] == 0xFF {
            if i + 3 >= data.len() {
                return Err(TransportError::InvalidFrame {
                    reason: "RLE: incomplete escape sequence at end of data".to_string(),
                });
            }

            let byte = data[i + 1];
            let count = ((data[i + 2] as u16) << 8) | (data[i + 3] as u16);

            if count == 0 {
                return Err(TransportError::InvalidFrame {
                    reason: "RLE: zero run length".to_string(),
                });
            }

            // Check for literal 0xFF escape: [0xFF, 0xFF, 0x00, 0x01]
            if byte == 0xFF && count == 1 {
                result.push(0xFF);
            } else {
                // Extend result if needed
                if result.len() + count as usize > result.capacity() {
                    result.reserve(count as usize);
                }
                for _ in 0..count {
                    result.push(byte);
                }
            }

            i += 4;
        } else {
            result.push(data[i]);
            i += 1;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_config_default() {
        let config = CompressionConfig::default();
        assert_eq!(config.algorithm, CompressionAlgorithm::Rle);
        assert_eq!(config.min_payload_size, 256);
        assert!(config.enabled);
    }

    #[test]
    fn test_compression_algorithm_values() {
        assert_eq!(CompressionAlgorithm::None as u8, 0);
        assert_eq!(CompressionAlgorithm::Rle as u8, 1);
        assert_eq!(CompressionAlgorithm::Lz4 as u8, 2);
        assert_eq!(CompressionAlgorithm::Zstd as u8, 3);
    }

    #[test]
    fn test_compress_none() {
        let config = CompressionConfig {
            algorithm: CompressionAlgorithm::None,
            min_payload_size: 256,
            enabled: true,
        };
        let compressor = Compressor::new(config);
        let data = vec![1u8, 2, 3, 4, 5];

        let payload = compressor.compress(&data);
        assert_eq!(payload.algorithm, CompressionAlgorithm::None);
        assert_eq!(payload.data, data);
    }

    #[test]
    fn test_compress_disabled() {
        let config = CompressionConfig {
            algorithm: CompressionAlgorithm::Rle,
            min_payload_size: 256,
            enabled: false,
        };
        let compressor = Compressor::new(config);
        let data = vec![1u8; 512];

        let payload = compressor.compress(&data);
        assert_eq!(payload.algorithm, CompressionAlgorithm::None);
        assert_eq!(payload.data, data);
    }

    #[test]
    fn test_compress_below_min_size() {
        let config = CompressionConfig {
            algorithm: CompressionAlgorithm::Rle,
            min_payload_size: 256,
            enabled: true,
        };
        let compressor = Compressor::new(config);
        let data = vec![1u8; 100];

        let payload = compressor.compress(&data);
        assert_eq!(payload.algorithm, CompressionAlgorithm::None);
        assert_eq!(payload.data, data);
    }

    #[test]
    fn test_rle_compress_uniform() {
        let data = vec![0xAA; 1000];
        let compressed = rle_compress(&data);
        // Should be much smaller: [0xFF, 0xAA, 0x03, 0xE8]
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_rle_compress_random() {
        let data: Vec<u8> = (0..100).map(|_| rand_byte()).collect();
        let compressed = rle_compress(&data);
        let decompressed = rle_decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_rle_compress_empty() {
        let data: Vec<u8> = vec![];
        let compressed = rle_compress(&data);
        assert!(compressed.is_empty());
        let decompressed = rle_decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_rle_compress_escape_byte() {
        // Test encoding of 0xFF runs - 4 consecutive 0xFF bytes is a run, not literal
        let data = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let compressed = rle_compress(&data);
        // Run of 4 0xFF bytes encoded as [0xFF, 0xFF, 0x00, 0x04]
        assert_eq!(compressed.len(), 4);
        let decompressed = rle_decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);

        // Test encoding of exactly 2 0xFF bytes (not a run, so escape each)
        let data2 = vec![0xFF, 0xFF];
        let compressed2 = rle_compress(&data2);
        // Each 0xFF escaped: [0xFF, 0xFF, 0x00, 0x01][0xFF, 0xFF, 0x00, 0x01]
        assert_eq!(compressed2.len(), 8);
        let decompressed2 = rle_decompress(&compressed2).unwrap();
        assert_eq!(decompressed2, data2);
    }

    #[test]
    fn test_rle_roundtrip() {
        let test_cases = vec![
            vec![1u8; 10],
            vec![1u8; 100],
            vec![0xFF; 50],
            (0..100).map(|_| rand_byte()).collect(),
            vec![1, 2, 3, 4, 5],
            vec![1, 1, 1, 2, 2, 2, 3, 3, 3],
            vec![],
        ];

        for data in test_cases {
            let compressed = rle_compress(&data);
            let decompressed = rle_decompress(&compressed).unwrap();
            assert_eq!(
                decompressed,
                data,
                "Roundtrip failed for data: {:?}",
                &data[..data.len().min(20)]
            );
        }
    }

    #[test]
    fn test_compressed_payload_encode() {
        let payload = CompressedPayload {
            algorithm: CompressionAlgorithm::Rle,
            original_size: 100,
            data: vec![1, 2, 3],
        };

        let encoded = bincode::serialize(&payload).unwrap();
        let decoded: CompressedPayload = bincode::deserialize(&encoded).unwrap();

        assert_eq!(decoded.algorithm, CompressionAlgorithm::Rle);
        assert_eq!(decoded.original_size, 100);
        assert_eq!(decoded.data, vec![1, 2, 3]);
    }

    #[test]
    fn test_compressor_stats() {
        let config = CompressionConfig::default();
        let compressor = Compressor::new(config);

        let data = vec![0xAA; 300];
        let payload = compressor.compress(&data);

        // Decompress to increment decompressions
        let _ = compressor.decompress(&payload);

        let stats = compressor.stats();
        assert_eq!(stats.compressions, 1);
        assert_eq!(stats.decompressions, 1);
        assert!(stats.bytes_in > 0);
    }

    #[test]
    fn test_compressor_stats_ratio() {
        let config = CompressionConfig::default();
        let compressor = Compressor::new(config);

        let data = vec![0xBB; 300];
        let payload = compressor.compress(&data);

        let stats = compressor.stats();
        assert!(stats.ratio > 0.0);
        assert!(stats.ratio <= 1.0);
    }

    #[test]
    fn test_decompress_invalid() {
        let config = CompressionConfig::default();
        let compressor = Compressor::new(config);

        // Invalid RLE: incomplete escape sequence
        let invalid_data = vec![0xFF, 1, 2];
        let payload = CompressedPayload {
            algorithm: CompressionAlgorithm::Rle,
            original_size: 10,
            data: invalid_data,
        };

        let result = compressor.decompress(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn test_rle_compress_alternating() {
        // Alternating bytes don't compress well with RLE
        let data: Vec<u8> = (0..100).map(|i| if i % 2 == 0 { 1 } else { 2 }).collect();
        let compressed = rle_compress(&data);
        // Alternating pattern may not compress at all, or may even expand
        let decompressed = rle_decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    fn rand_byte() -> u8 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        (nanos % 256) as u8
    }
}
