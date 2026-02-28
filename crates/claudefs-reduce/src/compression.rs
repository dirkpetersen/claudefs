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
}
