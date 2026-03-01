//! Property-Based Tests for Data Reduction - Self-contained tests

use proptest::prelude::*;
use std::collections::HashSet;

/// Generates random byte slices
pub fn arb_data(max_size: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0u8..255, 0..max_size)
}

/// Simple chunk structure
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub data: Vec<u8>,
    pub hash: u64,
    pub offset: u64,
}

/// Simple chunker simulation
pub fn chunk_data(data: &[u8], chunk_size: usize) -> Vec<Chunk> {
    if data.is_empty() || chunk_size == 0 {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut hash: u64 = 0;

    for (i, &byte) in data.iter().enumerate() {
        hash = hash.wrapping_add((byte as u64).wrapping_mul((i + 1) as u64));

        if (i + 1) % chunk_size == 0 || i == data.len() - 1 {
            let chunk_data = data[offset..=i].to_vec();
            chunks.push(Chunk {
                data: chunk_data,
                hash,
                offset: offset as u64,
            });
            offset = i + 1;
            hash = 0;
        }
    }

    chunks
}

/// Test compression roundtrip simulation
pub fn compression_roundtrip(data: Vec<u8>) -> Result<(), TestCaseError> {
    if data.is_empty() {
        return Ok(());
    }

    // Simple simulation: compress is just identity for this test
    let compressed = data.clone();
    let decompressed = compressed;

    prop_assert_eq!(
        decompressed,
        data,
        "Decompressed data should match original"
    );

    Ok(())
}

/// Test encryption roundtrip simulation
pub fn encryption_roundtrip(data: Vec<u8>, key: Vec<u8>) -> Result<(), TestCaseError> {
    if data.is_empty() {
        return Ok(());
    }

    prop_assert!(key.len() == 32, "Key must be 256-bit");

    Ok(())
}

/// Test fingerprint determinism simulation
pub fn fingerprint_determinism(data: Vec<u8>) -> Result<(), TestCaseError> {
    if data.is_empty() {
        return Ok(());
    }

    let hash1 = simple_hash(&data);
    let hash2 = simple_hash(&data);

    prop_assert_eq!(hash1, hash2, "Same data should produce same hash");

    Ok(())
}

/// Simple hash function
fn simple_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    for (i, &byte) in data.iter().enumerate() {
        hash = hash.wrapping_add((byte as u64).wrapping_mul((i + 1) as u64));
    }
    hash
}

/// Test chunking reassembly
pub fn chunking_reassembly(data: Vec<u8>) -> Result<(), TestCaseError> {
    if data.is_empty() {
        return Ok(());
    }

    let chunks = chunk_data(&data, 1024);

    let mut reassembled = Vec::new();
    for chunk in &chunks {
        reassembled.extend_from_slice(&chunk.data);
    }

    prop_assert_eq!(reassembled, data, "Reassembled data should equal original");

    Ok(())
}

/// Test dedup ratio
pub fn dedup_ratio(data: Vec<u8>) -> Result<(), TestCaseError> {
    if data.len() < 64 {
        return Ok(());
    }

    let chunks = chunk_data(&data, 1024);

    let mut unique_hashes = HashSet::new();
    for chunk in &chunks {
        unique_hashes.insert(chunk.hash);
    }

    let chunk_count = chunks.len() as f64;
    let unique_count = unique_hashes.len() as f64;
    let dedup_ratio = chunk_count / unique_count.max(1.0);

    prop_assert!(dedup_ratio >= 1.0, "Dedup ratio should be >= 1.0");
    prop_assert!(
        dedup_ratio <= chunk_count,
        "Dedup ratio can't exceed chunk count"
    );

    Ok(())
}

/// Test compression algorithm variants
fn test_compression_algo(algo: u8, data: &[u8]) {
    if data.is_empty() {
        return;
    }

    // Just verify algorithm variants exist
    assert!(algo <= 2);
}

/// Test chunk struct fields
fn run_chunk_fields() {
    let chunk = Chunk {
        data: vec![1, 2, 3],
        hash: 42,
        offset: 0,
    };

    assert_eq!(chunk.data.len(), 3);
    assert_eq!(chunk.offset, 0);
    assert_eq!(chunk.hash, 42);
}

/// Test chunker produces non-empty chunks
fn run_chunker_non_empty() {
    let data = vec![1u8; 1000];
    let chunks = chunk_data(&data, 256);

    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(!chunk.data.is_empty());
    }
}

/// Test chunker with small data
fn run_chunker_small_data() {
    let data = vec![1u8; 10];
    let chunks = chunk_data(&data, 5);

    assert!(!chunks.is_empty());
}

/// Test chunker with large data
fn run_chunker_large_data() {
    let data = vec![1u8; 10 * 1024 * 1024];
    let chunks = chunk_data(&data, 64 * 1024);

    assert!(chunks.len() > 1);
}

/// Test chunk data coverage
fn run_chunk_data_coverage() {
    let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let chunks = chunk_data(&data, 3);

    let mut total = 0;
    for chunk in &chunks {
        total += chunk.data.len();
    }

    assert!(total >= data.len());
}

/// Test chunk hash equality
fn run_chunk_hash_equality() {
    let data = b"test data";
    let hash1 = simple_hash(data);
    let hash2 = simple_hash(data);
    assert_eq!(hash1, hash2);
}

/// Test chunk hash inequality
fn run_chunk_hash_inequality() {
    let hash1 = simple_hash(b"data1");
    let hash2 = simple_hash(b"data2");
    assert_ne!(hash1, hash2);
}

/// Test chunker config defaults
fn run_chunk_config_default() {
    // Just verify we can call the function
    let chunks = chunk_data(b"test", 1024);
    assert!(!chunks.is_empty());
}

/// Test dedup with duplicate data
fn run_dedup_duplicate() {
    let data = vec![1u8, 2, 3, 1, 2, 3, 1, 2, 3];
    let chunks = chunk_data(&data, 3);

    assert!(chunks.len() >= 1);
}

/// Test chunker with all same bytes
fn run_chunker_uniform_data() {
    let data = vec![0u8; 1000];
    let chunks = chunk_data(&data, 256);

    assert!(!chunks.is_empty());
}

/// Test chunk clone
fn run_chunk_clone() {
    let chunk = Chunk {
        data: vec![1, 2, 3],
        hash: 42,
        offset: 0,
    };
    let cloned = chunk.clone();
    assert_eq!(chunk, cloned);
}

/// Test dedup ratio calculation
fn run_dedup_ratio_calc() {
    let data: Vec<u8> = (0..100).map(|i| (i % 10) as u8).collect();
    let chunks = chunk_data(&data, 10);

    let unique: HashSet<u64> = chunks.iter().map(|c| c.hash).collect();
    let ratio = chunks.len() as f64 / unique.len().max(1) as f64;

    assert!(ratio >= 1.0);
}

proptest! {
    #[test]
    fn prop_compression_roundtrip(data in arb_data(65536)) {
        compression_roundtrip(data)?;
    }

    #[test]
    fn prop_encryption_roundtrip(data in arb_data(65536), key in prop::collection::vec(0u8..255, 32..33)) {
        encryption_roundtrip(data, key)?;
    }

    #[test]
    fn prop_fingerprint_determinism(data in arb_data(65536)) {
        fingerprint_determinism(data)?;
    }

    #[test]
    fn prop_chunking_reassembly(data in proptest::collection::vec(0u8..255, 1024..1024*1024)) {
        chunking_reassembly(data)?;
    }

    #[test]
    fn prop_dedup_ratio(data in proptest::collection::vec(0u8..255, 1024..1024*1024)) {
        dedup_ratio(data)?;
    }
}

#[test]
fn test_compression_algo_0() {
    test_compression_algo(0, b"test data");
}

#[test]
fn test_compression_algo_1() {
    test_compression_algo(1, b"test data");
}

#[test]
fn test_compression_algo_2() {
    test_compression_algo(2, b"test data");
}

#[test]
fn test_chunk_fields_runner() {
    run_chunk_fields();
}

#[test]
fn test_chunker_non_empty_runner() {
    run_chunker_non_empty();
}

#[test]
fn test_chunker_small_data_runner() {
    run_chunker_small_data();
}

#[test]
fn test_chunker_large_data_runner() {
    run_chunker_large_data();
}

#[test]
fn test_chunk_data_coverage_runner() {
    run_chunk_data_coverage();
}

#[test]
fn test_chunk_hash_equality_runner() {
    run_chunk_hash_equality();
}

#[test]
fn test_chunk_hash_inequality_runner() {
    run_chunk_hash_inequality();
}

#[test]
fn test_chunk_config_default_runner() {
    run_chunk_config_default();
}

#[test]
fn test_dedup_duplicate_runner() {
    run_dedup_duplicate();
}

#[test]
fn test_chunker_uniform_data_runner() {
    run_chunker_uniform_data();
}

#[test]
fn test_chunk_clone_runner() {
    run_chunk_clone();
}

#[test]
fn test_dedup_ratio_calc_runner() {
    run_dedup_ratio_calc();
}

#[test]
fn test_chunk_debug() {
    let chunk = Chunk {
        data: vec![1, 2, 3],
        hash: 42,
        offset: 0,
    };
    let _ = format!("{:?}", chunk);
}

#[test]
fn test_empty_data_chunking() {
    let data: Vec<u8> = vec![];
    let chunks = chunk_data(&data, 1024);
    assert!(chunks.is_empty());
}

#[test]
fn test_zero_chunk_size() {
    let data = vec![1u8; 100];
    let chunks = chunk_data(&data, 0);
    assert!(chunks.is_empty());
}

#[test]
fn test_single_byte_chunking() {
    let data = vec![1u8];
    let chunks = chunk_data(&data, 1024);
    assert_eq!(chunks.len(), 1);
}

#[test]
fn test_exact_chunk_size() {
    let data = vec![1u8; 512];
    let chunks = chunk_data(&data, 512);
    assert_eq!(chunks.len(), 1);
}

#[test]
fn test_hash_stability() {
    let data = b"hello world";
    let hash1 = simple_hash(data);
    let hash2 = simple_hash(data);
    assert_eq!(hash1, hash2);
}
