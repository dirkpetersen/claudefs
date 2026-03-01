//! Background LZ4 → Zstd recompression before S3 tiering.

use crate::compression::{compress, decompress, CompressionAlgorithm};
use crate::error::ReduceError;
use crate::fingerprint::ChunkHash;
use serde::{Deserialize, Serialize};

/// Configuration for the recompressor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecompressorConfig {
    /// Zstd compression level (1=fastest, 19=best, 3=balanced)
    pub zstd_level: i32,
    /// Minimum improvement percentage to accept (default 5%)
    pub min_improvement_pct: u8,
}

impl Default for RecompressorConfig {
    fn default() -> Self {
        Self {
            zstd_level: 3,
            min_improvement_pct: 5,
        }
    }
}

/// Statistics from recompression operations.
#[derive(Debug, Clone, Default)]
pub struct RecompressionStats {
    /// Total chunks processed
    pub chunks_processed: u64,
    /// Chunks where Zstd was smaller than LZ4 by threshold
    pub chunks_improved: u64,
    /// Chunks where recompression didn't help
    pub chunks_skipped: u64,
    /// Total bytes before recompression (LZ4 size)
    pub bytes_before: u64,
    /// Total bytes after recompression (Zstd size)
    pub bytes_after: u64,
}

impl RecompressionStats {
    /// Compression ratio (before / after)
    pub fn compression_ratio(&self) -> f64 {
        if self.bytes_after > 0 {
            self.bytes_before as f64 / self.bytes_after as f64
        } else {
            1.0
        }
    }

    /// Bytes saved (can be negative if Zstd is larger)
    pub fn bytes_saved(&self) -> i64 {
        self.bytes_before as i64 - self.bytes_after as i64
    }
}

/// Result of a successful recompression.
#[derive(Debug, Clone)]
pub struct RecompressedChunk {
    /// Chunk hash
    pub hash: ChunkHash,
    /// New Zstd-compressed data
    pub data: Vec<u8>,
    /// Original LZ4 size
    pub original_lz4_size: usize,
    /// New Zstd size
    pub new_zstd_size: usize,
}

/// LZ4 → Zstd recompressor for S3 tiering optimization.
pub struct Recompressor {
    config: RecompressorConfig,
}

impl Recompressor {
    /// Create a new recompressor with the given configuration.
    pub fn new(config: RecompressorConfig) -> Self {
        Self { config }
    }

    /// Recompress a single LZ4 chunk to Zstd if it improves.
    /// Returns Some(RecompressedChunk) if improvement exceeds threshold, None otherwise.
    pub fn recompress_chunk(
        &self,
        hash: ChunkHash,
        lz4_data: &[u8],
    ) -> Result<Option<RecompressedChunk>, ReduceError> {
        // Step (a): Decompress LZ4
        let plaintext = decompress(lz4_data, CompressionAlgorithm::Lz4)?;

        // Step (b): Recompress with Zstd
        let zstd_data = compress(
            &plaintext,
            CompressionAlgorithm::Zstd {
                level: self.config.zstd_level,
            },
        )?;

        let lz4_size = lz4_data.len();
        let zstd_size = zstd_data.len();

        // Step (c): Check if improvement exceeds threshold
        let threshold = (lz4_size * (100 - self.config.min_improvement_pct) as usize) / 100;

        if zstd_size < threshold {
            Ok(Some(RecompressedChunk {
                hash,
                data: zstd_data,
                original_lz4_size: lz4_size,
                new_zstd_size: zstd_size,
            }))
        } else {
            // Step (d): No improvement
            Ok(None)
        }
    }

    /// Recompress a batch of LZ4 chunks.
    /// Returns improved chunks and aggregate statistics.
    pub fn recompress_batch(
        &self,
        chunks: &[(ChunkHash, Vec<u8>)],
    ) -> (Vec<RecompressedChunk>, RecompressionStats) {
        let mut improved = Vec::new();
        let mut stats = RecompressionStats::default();

        for (hash, lz4_data) in chunks {
            stats.chunks_processed += 1;
            stats.bytes_before += lz4_data.len() as u64;

            match self.recompress_chunk(*hash, lz4_data) {
                Ok(Some(recompressed)) => {
                    stats.chunks_improved += 1;
                    stats.bytes_after += recompressed.new_zstd_size as u64;
                    improved.push(recompressed);
                }
                Ok(None) => {
                    stats.chunks_skipped += 1;
                    stats.bytes_after += lz4_data.len() as u64;
                }
                Err(_) => {
                    // On error, keep original
                    stats.chunks_skipped += 1;
                    stats.bytes_after += lz4_data.len() as u64;
                }
            }
        }

        (improved, stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::compress;

    fn make_lz4_data(size: usize) -> Vec<u8> {
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        compress(&data, CompressionAlgorithm::Lz4).unwrap()
    }

    #[test]
    fn test_recompressor_compressible_data() {
        let recompressor = Recompressor::new(RecompressorConfig::default());
        let hash = ChunkHash([0u8; 32]);

        // Highly compressible data
        let data: Vec<u8> = vec![0u8; 10000];
        let lz4_data = compress(&data, CompressionAlgorithm::Lz4).unwrap();

        let result = recompressor.recompress_chunk(hash, &lz4_data).unwrap();

        assert!(result.is_some());
        let recompressed = result.unwrap();
        assert!(recompressed.new_zstd_size < recompressed.original_lz4_size);
    }

    #[test]
    fn test_recompressor_random_data() {
        let recompressor = Recompressor::new(RecompressorConfig::default());
        let hash = ChunkHash([0u8; 32]);

        // Random-looking data won't compress well
        let data: Vec<u8> = (0..1000).map(|_| rand::random::<u8>()).collect();
        let lz4_data = compress(&data, CompressionAlgorithm::Lz4).unwrap();

        let result = recompressor.recompress_chunk(hash, &lz4_data).unwrap();

        // Should either be None or not much improvement
        assert!(
            result.is_none()
                || result.as_ref().unwrap().new_zstd_size
                    >= result.unwrap().original_lz4_size * 95 / 100
        );
    }

    #[test]
    fn test_recompressor_batch() {
        let recompressor = Recompressor::new(RecompressorConfig::default());

        // Use large structured data where Zstd beats LZ4 by >= 5%
        let chunks: Vec<(ChunkHash, Vec<u8>)> = (0..5)
            .map(|i| {
                // Create text-like data that Zstd handles better than LZ4
                let base = format!("The quick brown fox {} jumps over the lazy dog. ", i);
                let data: Vec<u8> = base
                    .as_bytes()
                    .iter()
                    .cycle()
                    .take(50_000)
                    .copied()
                    .collect();
                let lz4 = compress(&data, CompressionAlgorithm::Lz4).unwrap();
                (ChunkHash([i; 32]), lz4)
            })
            .collect();

        let (improved, stats) = recompressor.recompress_batch(&chunks);

        assert_eq!(stats.chunks_processed, 5);
        assert!(stats.chunks_improved > 0);
    }

    #[test]
    fn test_recompressor_roundtrip() {
        let recompressor = Recompressor::new(RecompressorConfig::default());
        let hash = ChunkHash([42u8; 32]);

        let original = b"This is test data for roundtrip verification with some repeated content repeated content".to_vec();
        let lz4_data = compress(&original, CompressionAlgorithm::Lz4).unwrap();

        let recompressed = recompressor.recompress_chunk(hash, &lz4_data).unwrap();

        if let Some(rc) = recompressed {
            // Verify we can decompress the Zstd
            let decompressed =
                decompress(&rc.data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
            assert_eq!(decompressed, original);
        }
    }

    #[test]
    fn test_recompressor_empty() {
        let recompressor = Recompressor::new(RecompressorConfig::default());
        let hash = ChunkHash([0u8; 32]);

        let empty_lz4 = compress(&[], CompressionAlgorithm::Lz4).unwrap();
        let result = recompressor.recompress_chunk(hash, &empty_lz4).unwrap();

        // Empty might or might not improve
        assert!(result.is_none() || result.unwrap().new_zstd_size <= 100);
    }

    #[test]
    fn test_recompressor_threshold() {
        // High threshold = more likely to accept
        let recompressor = Recompressor::new(RecompressorConfig {
            zstd_level: 3,
            min_improvement_pct: 50, // 50% improvement required
        });

        let hash = ChunkHash([0u8; 32]);
        let data = vec![1u8; 1000];
        let lz4_data = compress(&data, CompressionAlgorithm::Lz4).unwrap();

        let result = recompressor.recompress_chunk(hash, &lz4_data).unwrap();

        // With 50% threshold, likely no improvement
        assert!(result.is_none());
    }

    #[test]
    fn test_recompressor_stats() {
        let recompressor = Recompressor::new(RecompressorConfig::default());

        let chunks: Vec<(ChunkHash, Vec<u8>)> = vec![
            (ChunkHash([1u8; 32]), make_lz4_data(1000)),
            (ChunkHash([2u8; 32]), make_lz4_data(2000)),
            (ChunkHash([3u8; 32]), make_lz4_data(3000)),
        ];

        let (_, stats) = recompressor.recompress_batch(&chunks);

        assert_eq!(stats.chunks_processed, 3);
        assert!(stats.bytes_before > 0);
        assert!(stats.compression_ratio() >= 1.0);
    }
}
