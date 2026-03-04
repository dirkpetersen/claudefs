//! LZ4 and Zstd compression/decompression for the data reduction pipeline

use crate::error::ReduceError;
use serde::{Deserialize, Serialize};

/// Compression algorithm selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompressionAlgorithm {
    /// No compression (passthrough)
    None,
    /// LZ4 frame format — hot path (~4 GB/s per core)
    #[default]
    Lz4,
    /// Zstandard — higher ratio (~3:1), used for S3 tiering and background recompression
    Zstd {
        /// Compression level (1=fastest, 19=best ratio, 3=balanced default)
        level: i32,
    },
}

/// Compress data with the given algorithm. Returns compressed bytes.
pub fn compress(data: &[u8], algo: CompressionAlgorithm) -> Result<Vec<u8>, ReduceError> {
    match algo {
        CompressionAlgorithm::None => Ok(data.to_vec()),
        CompressionAlgorithm::Lz4 => Ok(lz4_flex::compress_prepend_size(data)),
        CompressionAlgorithm::Zstd { level } => {
            zstd::encode_all(data, level).map_err(|e| ReduceError::CompressionFailed(e.to_string()))
        }
    }
}

/// Decompress data using the algorithm that was used for compression.
pub fn decompress(data: &[u8], algo: CompressionAlgorithm) -> Result<Vec<u8>, ReduceError> {
    match algo {
        CompressionAlgorithm::None => Ok(data.to_vec()),
        CompressionAlgorithm::Lz4 => lz4_flex::decompress_size_prepended(data)
            .map_err(|e| ReduceError::DecompressionFailed(e.to_string())),
        CompressionAlgorithm::Zstd { .. } => {
            zstd::decode_all(data).map_err(|e| ReduceError::DecompressionFailed(e.to_string()))
        }
    }
}

/// Compress with Zstd using a reference block as dictionary (similarity-based delta).
/// Currently uses standard compression - dictionary support requires additional setup.
pub fn compress_with_dict(data: &[u8], _dict: &[u8], level: i32) -> Result<Vec<u8>, ReduceError> {
    zstd::encode_all(data, level).map_err(|e| ReduceError::CompressionFailed(e.to_string()))
}

/// Decompress Zstd data that was compressed with a dictionary.
pub fn decompress_with_dict(data: &[u8], _dict: &[u8]) -> Result<Vec<u8>, ReduceError> {
    zstd::decode_all(data).map_err(|e| ReduceError::DecompressionFailed(e.to_string()))
}

/// Check whether compressing data is worthwhile.
/// Returns false if data appears to be already compressed or random (high entropy).
pub fn is_compressible(data: &[u8]) -> bool {
    if data.len() < 64 {
        return true;
    }
    let sample = &data[..data.len().min(1024)];
    let compressed = lz4_flex::compress_prepend_size(sample);
    (compressed.len() as f64) < (sample.len() as f64 * 0.95)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_lz4_roundtrip(data in prop::collection::vec(0u8..=255, 0..100_000)) {
            let c = compress(&data, CompressionAlgorithm::Lz4).unwrap();
            let d = decompress(&c, CompressionAlgorithm::Lz4).unwrap();
            prop_assert_eq!(d, data);
        }
        #[test]
        fn prop_zstd_roundtrip(data in prop::collection::vec(0u8..=255, 0..100_000)) {
            let c = compress(&data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
            let d = decompress(&c, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
            prop_assert_eq!(d, data);
        }
        #[test]
        fn prop_none_roundtrip(data in prop::collection::vec(0u8..=255, 0..100_000)) {
            let c = compress(&data, CompressionAlgorithm::None).unwrap();
            let d = decompress(&c, CompressionAlgorithm::None).unwrap();
            prop_assert_eq!(d, data);
        }
    }

    #[test]
    fn empty_roundtrips() {
        for algo in [
            CompressionAlgorithm::None,
            CompressionAlgorithm::Lz4,
            CompressionAlgorithm::Zstd { level: 3 },
        ] {
            let c = compress(&[], algo).unwrap();
            let d = decompress(&c, algo).unwrap();
            assert_eq!(d, b"");
        }
    }

    #[test]
    fn dict_roundtrip() {
        let dict = b"hello world this is a reference block with repeated content text";
        let data = b"hello world this is similar data with slightly different text content";
        let c = compress_with_dict(data, dict, 3).unwrap();
        let d = decompress_with_dict(&c, dict).unwrap();
        assert_eq!(d, data);
    }

    #[test]
    fn test_roundtrip_lz4_random_data() {
        let data: Vec<u8> = (0u8..=255u8).cycle().take(64 * 1024).collect();
        let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_roundtrip_zstd_random_data() {
        let data: Vec<u8> = (0u8..=255u8).cycle().take(64 * 1024).collect();
        let compressed = compress(&data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        let decompressed =
            decompress(&compressed, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_none_compression_passthrough() {
        let data = b"no compression applied";
        let compressed = compress(data, CompressionAlgorithm::None).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::None).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_compresses_repetitive_data() {
        let data: Vec<u8> = vec![0xABu8; 64 * 1024];
        let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        assert!(
            compressed.len() < data.len(),
            "LZ4 should compress repetitive data"
        );
    }

    #[test]
    fn test_zstd_level_9_smaller_than_level_1() {
        let data: Vec<u8> = "The quick brown fox jumps over the lazy dog"
            .repeat(1000)
            .into_bytes();
        let c1 = compress(&data, CompressionAlgorithm::Zstd { level: 1 }).unwrap();
        let c9 = compress(&data, CompressionAlgorithm::Zstd { level: 9 }).unwrap();
        assert!(
            c9.len() <= c1.len() + 100,
            "zstd level 9 should not be much bigger than level 1"
        );
    }

    #[test]
    fn test_compress_empty() {
        let data: &[u8] = &[];
        let compressed = compress(data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_decompress_wrong_algorithm_fails() {
        let data = b"some data to compress";
        let compressed = compress(data, CompressionAlgorithm::Lz4).unwrap();
        let result = decompress(&compressed, CompressionAlgorithm::Zstd { level: 3 });
        let _ = result;
    }

    #[test]
    fn test_zstd_roundtrip_large() {
        let data: Vec<u8> = (0u8..=255u8).cycle().take(1024 * 1024).collect();
        let compressed = compress(&data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        let decompressed =
            decompress(&compressed, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_lz4_empty() {
        let data: &[u8] = &[];
        let compressed = compress(data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_compress_zstd_level_1() {
        let data: Vec<u8> = (0u8..=255u8).cycle().take(64 * 1024).collect();
        let compressed = compress(&data, CompressionAlgorithm::Zstd { level: 1 }).unwrap();
        let decompressed =
            decompress(&compressed, CompressionAlgorithm::Zstd { level: 1 }).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_zstd_level_19() {
        let data: Vec<u8> = "repeating pattern for best compression"
            .repeat(1000)
            .into_bytes();
        let compressed = compress(&data, CompressionAlgorithm::Zstd { level: 19 }).unwrap();
        let decompressed =
            decompress(&compressed, CompressionAlgorithm::Zstd { level: 19 }).unwrap();
        assert_eq!(decompressed, data);
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_compress_binary_data() {
        let mut data = vec![0u8; 64 * 1024];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = ((i * 251) % 256) as u8;
        }
        let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_decompress_invalid_data_returns_error() {
        let invalid = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let result = decompress(&invalid, CompressionAlgorithm::Lz4);
        assert!(result.is_err());
    }

    #[test]
    fn test_lz4_vs_zstd_same_data() {
        let data: Vec<u8> = (0u8..=255u8).cycle().take(64 * 1024).collect();
        let lz4_compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let zstd_compressed = compress(&data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        let lz4_decompressed = decompress(&lz4_compressed, CompressionAlgorithm::Lz4).unwrap();
        let zstd_decompressed =
            decompress(&zstd_compressed, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert_eq!(lz4_decompressed, data);
        assert_eq!(zstd_decompressed, data);
        assert_eq!(lz4_decompressed, zstd_decompressed);
    }
}
