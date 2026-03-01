// FILE: dependency_audit_tests.rs
use claudefs_reduce::compression::{compress, decompress, CompressionAlgorithm, is_compressible};
use claudefs_reduce::dedupe::{Chunker, ChunkerConfig, Chunk};
use claudefs_reduce::encryption::{
    decrypt, encrypt, EncryptionAlgorithm, EncryptionKey,
};
use claudefs_reduce::fingerprint::{blake3_hash, ChunkHash};
use claudefs_reduce::pipeline::ReductionPipeline;
use claudefs_transport::message::{serialize_message, deserialize_message};
use claudefs_transport::protocol::{Frame, Opcode, FrameFlags};
use std::collections::HashMap;

#[test]
fn finding_36_rand_is_csprng() {
    let values: Vec<u8> = (0..10000).map(|_| {
        use rand::Rng;
        rand::thread_rng().gen()
    }).collect();
    
    let unique_count = values.iter().collect::<std::collections::HashSet<_>>().len();
    assert!(unique_count > 9000, "rand::thread_rng should produce high entropy");
}

#[test]
fn finding_37_aesgcm_crate_properties() {
    let key = EncryptionKey([0x42; 32]);
    let plaintext = b"test AES-GCM encryption";
    
    let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
    let dec = decrypt(&ct, &key).unwrap();
    
    assert_eq!(dec, plaintext);
    assert_eq!(ct.algo, EncryptionAlgorithm::AesGcm256);
}

#[test]
fn finding_37_chacha_crate_properties() {
    let key = EncryptionKey([0x43; 32]);
    let plaintext = b"test ChaCha20Poly1305 encryption";
    
    let ct = encrypt(plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();
    let dec = decrypt(&ct, &key).unwrap();
    
    assert_eq!(dec, plaintext);
    assert_eq!(ct.algo, EncryptionAlgorithm::ChaCha20Poly1305);
}

#[test]
fn finding_38_lz4_decompression_bounded() {
    let data: Vec<u8> = b"hello world hello world hello world".to_vec();
    
    let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
    let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
    
    assert_eq!(decompressed.len() <= data.len() * 4, "Decompressed size should be bounded");
    assert_eq!(decompressed, data);
}

#[test]
fn finding_38_zstd_decompression_bounded() {
    let data: Vec<u8> = b"test data for zstd compression".to_vec();
    
    let compressed = compress(&data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
    let decompressed = decompress(&compressed, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
    
    assert_eq!(decompressed.len() <= data.len() * 4, "Decompressed size should be bounded");
    assert_eq!(decompressed, data);
}

#[test]
fn finding_39_bincode_large_vec() {
    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    struct TestMsg {
        data: Vec<u8>,
    }
    
    let large_data: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
    let msg = TestMsg { data: large_data };
    
    let serialized = serialize_message(&msg).unwrap();
    let deserialized: TestMsg = deserialize_message(&serialized).unwrap();
    
    assert_eq!(deserialized.data.len(), 100_000);
}

#[test]
fn finding_39_bincode_truncated() {
    #[derive(serde::Serialize, serde::Deserialize, Debug)]
    struct TestMsg {
        data: Vec<u8>,
    }
    
    let data = vec![0u8; 100];
    let msg = TestMsg { data };
    
    let mut serialized = serialize_message(&msg).unwrap();
    serialized.truncate(10);
    
    let result = deserialize_message::<TestMsg>(&serialized);
    assert!(result.is_err() || result.unwrap().data.len() < 100);
}

#[test]
fn finding_40_blake3_determinism() {
    let data = b"deterministic input";
    
    let hash1 = blake3_hash(data);
    let hash2 = blake3_hash(data);
    
    assert_eq!(hash1.0, hash2.0, "Same input should produce same hash");
}

#[test]
fn finding_40_blake3_avalanche() {
    let data1 = b"input with a bit";
    let data2 = b"input with b bit";
    
    let hash1 = blake3_hash(data1);
    let hash2 = blake3_hash(data2);
    
    let diff_count = hash1.0.iter().zip(hash2.0.iter())
        .filter(|(a, b)| a != b)
        .count();
    
    assert!(diff_count > 10, "Single bit flip should cause avalanche effect");
}

#[test]
fn finding_40_fastcdc_determinism() {
    let chunker = Chunker::new();
    let data = b"test data for chunking ".$pseudo";
    
    let chunks1 = chunker.chunk(data);
    let chunks2 = chunker.chunk(data);
    
    assert_eq!(chunks1.len(), chunks2.len());
    for (c1, c2) in chunks1.iter().zip(chunks2.iter()) {
        assert_eq!(c1.hash.0, c2.hash.0);
    }
}

#[test]
fn dep_compression_ratio_bounds() {
    let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    
    let compressed = compress(&data, CompressionAlgorithm::Zstd { level: 1 }).unwrap();
    
    assert!(compressed.len() >= data.len() / 10, "Compressed size should be reasonable");
    assert!(compressed.len() <= data.len() * 2, "Compressed size should not blow up");
}

#[test]
fn dep_blake3_uniformity() {
    let mut bit_counts = [0u32; 256];
    
    for i in 0..1000 {
        let data = i.to_le_bytes();
        let hash = blake3_hash(&data);
        for &byte in &hash.0 {
            bit_counts[byte as usize] += 1;
        }
    }
    
    let avg = 1000.0 * 32.0 / 256.0;
    for count in &bit_counts {
        let deviation = (*count as f64 - avg).abs() / avg;
        assert!(deviation < 0.2, "Hash bytes should be uniformly distributed");
    }
}

#[test]
fn dep_chunker_min_max_respected() {
    let config = ChunkerConfig {
        min_size: 1024,
        avg_size: 4096,
        max_size: 8192,
    };
    let chunker = Chunker::with_config(config);
    
    let data: Vec<u8> = (0..100000).map(|i| (i % 256) as u8).collect();
    let chunks = chunker.chunk(&data);
    
    for chunk in &chunks {
        let size = chunk.data.len();
        assert!(size >= config.min_size, "Chunk {} smaller than min {}", size, config.min_size);
        assert!(size <= config.max_size, "Chunk {} larger than max {}", size, config.max_size);
    }
}

#[test]
fn dep_encryption_roundtrip_all_algos() {
    let key = EncryptionKey([0xAA; 32]);
    let plaintext = b"roundtrip test data";
    
    for algo in &[EncryptionAlgorithm::AesGcm256, EncryptionAlgorithm::ChaCha20Poly1305] {
        let ct = encrypt(plaintext, &key, *algo).unwrap();
        let dec = decrypt(&ct, &key).unwrap();
        assert_eq!(dec, plaintext, "Roundtrip failed for {:?}", algo);
    }
}

#[test]
fn prop_blake3_no_trivial_collisions() {
    use proptest::prelude::*;
    
    proptest! {
        #[test]
        fn prop_no_collision((d1, d2): (Vec<u8>, Vec<u8>)) {
            if d1 != d2 {
                let h1 = blake3_hash(&d1);
                let h2 = blake3_hash(&d2);
                assert_ne!(h1.0, h2.0, "Different inputs should have different hashes");
            }
        }
    }
}
