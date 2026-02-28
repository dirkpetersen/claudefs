//! Content fingerprinting: BLAKE3 hashing and MinHash Super-Features

use serde::{Deserialize, Serialize};

/// A 32-byte BLAKE3 hash identifying a chunk's content. Used as the CAS key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkHash(pub [u8; 32]);

impl ChunkHash {
    /// Return the hash as a lowercase hex string
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
    /// Return the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Display for ChunkHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Four 64-bit MinHash values representing a chunk's similarity signature.
/// Chunks sharing 3+ Super-Features are candidates for delta compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SuperFeatures(pub [u64; 4]);

impl SuperFeatures {
    /// Count shared features with another SuperFeatures
    pub fn similarity(&self, other: &SuperFeatures) -> usize {
        self.0
            .iter()
            .zip(other.0.iter())
            .filter(|(a, b)| a == b)
            .count()
    }
    /// True if 3 or more features match (chunks are "similar")
    pub fn is_similar(&self, other: &SuperFeatures) -> bool {
        self.similarity(other) >= 3
    }
}

/// Compute BLAKE3 hash of data
pub fn blake3_hash(data: &[u8]) -> ChunkHash {
    let hash = blake3::hash(data);
    ChunkHash(*hash.as_bytes())
}

/// FNV-1a 64-bit hash of a byte slice
fn fnv1a_hash(data: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    data.iter()
        .fold(OFFSET, |h, &b| h.wrapping_mul(PRIME) ^ (b as u64))
}

/// Compute MinHash Super-Features from chunk data.
/// Divides the chunk into 4 equal sub-regions and computes FNV-1a hash per region.
/// If data is shorter than 4 bytes, returns SuperFeatures([0; 4]).
pub fn super_features(data: &[u8]) -> SuperFeatures {
    if data.len() < 4 {
        return SuperFeatures([0u64; 4]);
    }
    let region_size = data.len().div_ceil(4);
    let mut features = [0u64; 4];
    for (i, feature) in features.iter_mut().enumerate() {
        let start = i * region_size;
        let end = ((i + 1) * region_size).min(data.len());
        *feature = fnv1a_hash(&data[start..end]);
    }
    SuperFeatures(features)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn blake3_hash_is_deterministic() {
        let h1 = blake3_hash(b"hello world");
        let h2 = blake3_hash(b"hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_data_produces_different_hashes() {
        let h1 = blake3_hash(b"hello");
        let h2 = blake3_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn super_features_identical_data() {
        let sf1 = super_features(b"hello world this is test data for super features computation");
        let sf2 = super_features(b"hello world this is test data for super features computation");
        assert_eq!(sf1, sf2);
        assert_eq!(sf1.similarity(&sf2), 4);
        assert!(sf1.is_similar(&sf2));
    }

    #[test]
    fn super_features_short_data() {
        let sf = super_features(b"hi");
        assert_eq!(sf, SuperFeatures([0u64; 4]));
    }

    proptest! {
        #[test]
        fn prop_blake3_deterministic(data in prop::collection::vec(0u8..=255, 0..10_000)) {
            prop_assert_eq!(blake3_hash(&data), blake3_hash(&data));
        }
    }
}
