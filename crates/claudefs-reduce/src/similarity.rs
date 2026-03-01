//! Similarity-based background deduplication using MinHash Super-Features.
//! Tier 2 of the data reduction pipeline finds similar (but not identical) chunks
//! for delta compression using Zstd dictionary mode.

use crate::error::ReduceError;
use crate::fingerprint::{ChunkHash, SuperFeatures};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::debug;

/// Inverted index mapping individual feature values to chunks that have them.
/// Used for fast similarity lookup: chunks sharing 3+ features are "similar".
#[allow(clippy::type_complexity)]
pub struct SimilarityIndex {
    index: Arc<RwLock<HashMap<u64, Vec<(ChunkHash, SuperFeatures)>>>>,
}

impl Default for SimilarityIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SimilarityIndex {
    /// Create a new empty similarity index.
    pub fn new() -> Self {
        Self {
            index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert a chunk's Super-Features into the index.
    /// For each of the 4 feature values, inserts (hash, features) into that feature's bucket.
    pub fn insert(&self, hash: ChunkHash, features: SuperFeatures) {
        let mut index = self.index.write().unwrap();
        for feature in features.0 {
            index.entry(feature).or_default().push((hash, features));
        }
    }

    /// Find a similar chunk that shares at least 3 features with the query.
    /// Returns the first chunk hash that meets the similarity threshold.
    pub fn find_similar(&self, features: &SuperFeatures) -> Option<ChunkHash> {
        let index = self.index.read().unwrap();

        // Count how many features each candidate chunk shares
        let mut candidate_scores: HashMap<ChunkHash, usize> = HashMap::new();

        for feature in features.0 {
            if let Some(chunks) = index.get(&feature) {
                for (hash, _) in chunks {
                    *candidate_scores.entry(*hash).or_insert(0) += 1;
                }
            }
        }

        // Find first chunk with >= 3 shared features
        for (hash, score) in candidate_scores {
            if score >= 3 {
                debug!(?hash, score, "Found similar chunk");
                return Some(hash);
            }
        }

        None
    }

    /// Remove a chunk from the index (for GC).
    pub fn remove(&self, hash: &ChunkHash) {
        let mut index = self.index.write().unwrap();
        // We need to iterate through all feature buckets and remove this hash
        let hashes_to_remove: Vec<(u64, ChunkHash)> = index
            .iter()
            .flat_map(|(&feature, chunks)| {
                chunks
                    .iter()
                    .find(|(h, _)| h == hash)
                    .map(|_| (feature, *hash))
            })
            .collect();

        for (feature, h) in hashes_to_remove {
            if let Some(chunks) = index.get_mut(&feature) {
                chunks.retain(|(hash, _)| hash != &h);
            }
        }
    }

    /// Number of unique chunks indexed.
    pub fn entry_count(&self) -> usize {
        let index = self.index.read().unwrap();
        let mut unique_hashes = std::collections::HashSet::new();
        for chunks in index.values() {
            for (hash, _) in chunks {
                unique_hashes.insert(*hash);
            }
        }
        unique_hashes.len()
    }
}

/// Delta compressor using Zstd dictionary mode for similar chunk compression.
pub struct DeltaCompressor;

impl DeltaCompressor {
    /// Compress data using a reference as the Zstd dictionary.
    /// This achieves better compression for similar data by using the reference
    /// as training data for the compression dictionary.
    pub fn compress_delta(
        data: &[u8],
        reference: &[u8],
        level: i32,
    ) -> Result<Vec<u8>, ReduceError> {
        use std::io::Write;
        use zstd::stream::write::Encoder;

        if reference.is_empty() {
            return Err(ReduceError::CompressionFailed(
                "reference data cannot be empty for delta compression".to_string(),
            ));
        }

        let mut encoder = Encoder::with_dictionary(Vec::new(), level, reference)
            .map_err(|e| ReduceError::CompressionFailed(e.to_string()))?;
        encoder
            .write_all(data)
            .map_err(|e| ReduceError::CompressionFailed(e.to_string()))?;
        let compressed = encoder
            .finish()
            .map_err(|e| ReduceError::CompressionFailed(e.to_string()))?;
        Ok(compressed)
    }

    /// Decompress delta using the reference as the dictionary.
    pub fn decompress_delta(delta: &[u8], reference: &[u8]) -> Result<Vec<u8>, ReduceError> {
        use std::io::Read;
        use zstd::stream::read::Decoder;

        if reference.is_empty() {
            return Err(ReduceError::DecompressionFailed(
                "reference data cannot be empty for delta decompression".to_string(),
            ));
        }

        let mut decoder = Decoder::with_dictionary(std::io::Cursor::new(delta), reference)
            .map_err(|e| ReduceError::DecompressionFailed(e.to_string()))?;
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| ReduceError::DecompressionFailed(e.to_string()))?;
        Ok(decompressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::super_features;

    #[test]
    fn test_insert_and_find_identical() {
        let index = SimilarityIndex::new();
        let data = b"hello world this is test data for similarity index";
        let hash = ChunkHash(*blake3::hash(data).as_bytes());
        let features = super_features(data);

        index.insert(hash, features);

        let found = index.find_similar(&features);
        assert!(found.is_some());
    }

    #[test]
    fn test_find_similar_three_of_four() {
        let index = SimilarityIndex::new();

        // Create two chunks that share 3+ of 4 features
        // Use longer data to get 4 regions that can share features
        let data1: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
        let mut data2 = data1.clone();
        data2[90] = 255; // Change one byte in region 3
        data2[91] = 254; // Change another byte in region 3

        let hash1 = ChunkHash(*blake3::hash(&data1).as_bytes());
        let _hash2 = ChunkHash(*blake3::hash(&data2).as_bytes());
        let features1 = super_features(&data1);
        let features2 = super_features(&data2);

        index.insert(hash1, features1);

        // Check they share at least 3 features
        let similarity = features1.similarity(&features2);
        assert!(similarity >= 3, "features share {} of 4", similarity);

        let found = index.find_similar(&features2);
        assert!(found.is_some());
    }

    #[test]
    fn test_find_similar_two_of_four() {
        let index = SimilarityIndex::new();

        // Create data with different features
        let data1 = b"aaaaaaaaaa";
        let data2 = b"bbbbbbbbbb"; // Completely different
        let hash1 = ChunkHash(*blake3::hash(data1).as_bytes());
        let features1 = super_features(data1);
        let features2 = super_features(data2);

        index.insert(hash1, features1);

        // They should share 0 features
        let similarity = features1.similarity(&features2);
        assert!(
            similarity < 3,
            "features share {} of 4, should be < 3",
            similarity
        );

        let found = index.find_similar(&features2);
        assert!(
            found.is_none(),
            "should not find similar chunk for dissimilar features"
        );
    }

    #[test]
    fn test_find_dissimilar_returns_none() {
        let index = SimilarityIndex::new();

        let data1 = b"very unique data 12345";
        let data2 = b"completely different data";
        let hash1 = ChunkHash(*blake3::hash(data1).as_bytes());
        let features1 = super_features(data1);
        let features2 = super_features(data2);

        index.insert(hash1, features1);

        let found = index.find_similar(&features2);
        assert!(found.is_none());
    }

    #[test]
    fn test_remove() {
        let index = SimilarityIndex::new();

        let data = b"test data for removal";
        let hash = ChunkHash(*blake3::hash(data).as_bytes());
        let features = super_features(data);

        index.insert(hash, features);
        assert!(index.find_similar(&features).is_some());

        index.remove(&hash);
        assert!(index.find_similar(&features).is_none());
    }

    #[test]
    fn test_entry_count() {
        let index = SimilarityIndex::new();

        assert_eq!(index.entry_count(), 0);

        let data1 = b"test data one";
        let data2 = b"test data two";
        let hash1 = ChunkHash(*blake3::hash(data1).as_bytes());
        let hash2 = ChunkHash(*blake3::hash(data2).as_bytes());

        index.insert(hash1, super_features(data1));
        assert_eq!(index.entry_count(), 1);

        index.insert(hash2, super_features(data2));
        assert_eq!(index.entry_count(), 2);
    }

    #[test]
    fn test_delta_compress_roundtrip() {
        let original = b"This is the original data that we want to compress with delta compression using zstd dictionary mode. It has some repeating patterns.";
        let reference = b"This is the original data that we want to compress with delta compression using zstd dictionary mode. It has some repeating patterns and more content here.";

        let compressed = DeltaCompressor::compress_delta(original, reference, 3).unwrap();
        let decompressed = DeltaCompressor::decompress_delta(&compressed, reference).unwrap();

        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_delta_reduces_size() {
        // Create data with clear repetitive patterns
        let data: Vec<u8> = std::iter::repeat_n(
            b"The quick brown fox jumps over the lazy dog. ".as_slice(),
            10,
        )
        .flatten()
        .copied()
        .collect();

        let mut reference = data.clone();
        reference.push(0); // Make it slightly different to avoid exact match

        let compressed = DeltaCompressor::compress_delta(&data, &reference, 3).unwrap();

        // Verify roundtrip works
        let decompressed = DeltaCompressor::decompress_delta(&compressed, &reference).unwrap();
        assert_eq!(decompressed, data);
    }
}
