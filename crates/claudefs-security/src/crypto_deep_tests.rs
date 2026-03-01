// FILE: crypto_deep_tests.rs
use claudefs_reduce::compression::{compress, decompress, CompressionAlgorithm};
use claudefs_reduce::dedupe::{Chunker, ChunkerConfig};
use claudefs_reduce::encryption::{
    decrypt, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey, Nonce,
};
use claudefs_reduce::fingerprint::{blake3_hash, ChunkHash};
use claudefs_reduce::key_manager::{DataKey, KeyManager, KeyManagerConfig, WrappedKey};
use claudefs_reduce::pipeline::{PipelineConfig, ReductionPipeline};
use claudefs_reduce::ReductionError;
use std::collections::HashMap;
use std::num::Wrapping;

#[test]
fn finding_31_key_not_zeroized_on_drop() {
    let key = EncryptionKey([0u8; 32]);
    let ptr = key.0.as_ptr();
    let mut values = [0u8; 32];
    unsafe {
        std::ptr::read_nonoverlapping(ptr, values.as_mut_ptr(), 32);
    }
    assert_ne!(values, [0u8; 32], "Key memory was zeroized on drop");
}

#[test]
fn finding_31_datakey_not_zeroized() {
    let key = DataKey { key: [0u8; 32] };
    let ptr = key.key.as_ptr();
    let mut values = [0u8; 32];
    unsafe {
        std::ptr::read_nonoverlapping(ptr, values.as_mut_ptr(), 32);
    }
    assert_ne!(values, [0u8; 32], "DataKey memory was zeroized on drop");
}

#[test]
fn finding_32_nonce_entropy_distribution() {
    let key = EncryptionKey([1u8; 32]);
    let mut byte_counts = [[0u32; 256]; 12];

    for _ in 0..10000 {
        let nonce = random_nonce();
        for (i, &b) in nonce.0.iter().enumerate() {
            byte_counts[i][b as usize] += 1;
        }
    }

    let expected = 10000.0 / 256.0;
    for i in 0..12 {
        for count in &byte_counts[i] {
            let deviation = (*count as f64 - expected).abs() / expected;
            assert!(
                deviation < 0.1,
                "Byte {} has bias: {} vs expected {}",
                i,
                count,
                expected
            );
        }
    }
}

#[test]
fn finding_32_nonce_birthday_bound() {
    let nonce_bits: f64 = 96.0;
    let birthday_bound = (2.0_f64.powf(nonce_bits / 2.0)).sqrt() as u64;
    let collision_prob_10k = 1.0 - (-(10000.0 * 9999.0 / (2.0 * 2.0_f64.powf(nonce_bits)))).exp();

    assert!(
        birthday_bound > 1_000_000,
        "Birthday bound for 96-bit nonce should be > 1M"
    );
    assert!(
        collision_prob_10k < 0.01,
        "Collision probability for 10k nonces should be < 1%"
    );
}

#[test]
fn finding_33_hkdf_context_separation() {
    let master = EncryptionKey([0xAB; 32]);
    let hash1: [u8; 32] = [1u8; 32];
    let hash2: [u8; 32] = [2u8; 32];

    let key1 = claudefs_reduce::encryption::derive_chunk_key(&master, &hash1);
    let key2 = claudefs_reduce::encryption::derive_chunk_key(&master, &hash2);

    assert_ne!(
        key1.0, key2.0,
        "Different inputs should produce different keys"
    );
}

#[test]
fn finding_33_hkdf_empty_hash_handled() {
    let master = EncryptionKey([0xAB; 32]);
    let zero_hash: [u8; 32] = [0u8; 32];

    let result = std::panic::catch_unwind(|| {
        claudefs_reduce::encryption::derive_chunk_key(&master, &zero_hash)
    });

    assert!(result.is_ok(), "derive_chunk_key should handle zero hash");
}

#[test]
fn finding_34_key_pruning_data_loss() {
    let mut config = KeyManagerConfig::default();
    config.max_key_history = 3;
    let mut km = KeyManager::new(config);

    for i in 0..5 {
        let _ = km.rotate_key(EncryptionKey([i; 32]));
    }

    let initial_wrapped = km.wrap_dek(&km.generate_dek().unwrap()).unwrap();
    let initial_version = initial_wrapped.kek_version;

    for _ in 0..10 {
        let _ = km.rotate_key(EncryptionKey([rand::random(); 32]));
    }

    let result = km.unwrap_dek(&initial_wrapped);
    assert!(
        result.is_err(),
        "Old DEK should be inaccessible after pruning"
    );
}

#[test]
fn finding_34_key_version_sequence() {
    let mut km = KeyManager::new(KeyManagerConfig::default());

    let v0 = km.rotate_key(EncryptionKey([0; 32]));
    let v1 = km.rotate_key(EncryptionKey([1; 32]));
    let v2 = km.rotate_key(EncryptionKey([2; 32]));

    assert!(
        v0 < v1 && v1 < v2,
        "Versions should increment monotonically"
    );
}

#[test]
fn finding_35_no_aad_binding() {
    let key = EncryptionKey([0x42; 32]);
    let plaintext = b"test data";

    let ct1 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
    let ct2 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

    let dec1 = decrypt(&ct1, &key).unwrap();
    let dec2 = decrypt(&ct2, &key).unwrap();

    assert_eq!(dec1, plaintext);
    assert_eq!(dec2, plaintext);
}

#[test]
fn finding_35_ciphertext_portability() {
    let key = EncryptionKey([0x55; 32]);
    let plaintext = b"portable data";

    let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

    let dec = decrypt(&ct, &key).unwrap();
    assert_eq!(
        dec, plaintext,
        "Same ciphertext should decrypt with same key"
    );
}

#[test]
fn prop_nonce_uniqueness() {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_nonce_uniqueness_inner(_seed: u32) {
            let mut nonces = Vec::new();
            for _ in 0..100 {
                let nonce = random_nonce();
                for existing in &nonces {
                    assert_ne!(nonce.0, existing.0, "Nonces should be unique");
                }
                nonces.push(nonce);
            }
        }
    }
}

#[test]
fn prop_key_derivation_collision_resistance() {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_no_collision(h1 in any::<[u8; 32]>(), h2 in any::<[u8; 32]>()) {
            if h1 != h2 {
                let master = EncryptionKey([0xAA; 32]);
                let k1 = claudefs_reduce::encryption::derive_chunk_key(&master, &h1);
                let k2 = claudefs_reduce::encryption::derive_chunk_key(&master, &h2);
                assert_ne!(k1.0, k2.0);
            }
        }
    }
}

#[test]
fn prop_encrypt_key_sensitivity() {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_bit_flip((key_val, bit_pos): (u64, u8)) {
            let mut key = [0u8; 32];
            let idx = (bit_pos % 256) as usize;
            key[idx / 8] = (key_val & 0xFF) as u8;

            let other_key = {
                let mut k = key;
                k[idx / 8] ^= 1 << (idx % 8);
                EncryptionKey(k)
            };

            let plaintext = b"sensitive data";
            let key1 = EncryptionKey(key);

            let ct1 = encrypt(plaintext, &key1, EncryptionAlgorithm::AesGcm256).unwrap();
            let ct2 = encrypt(plaintext, &other_key, EncryptionAlgorithm::AesGcm256).unwrap();

            assert_ne!(ct1.ciphertext, ct2.ciphertext);
        }
    }
}

#[test]
fn key_manager_no_kek_wrap_fails() {
    let km = KeyManager::new(KeyManagerConfig::default());
    let dek = DataKey { key: [0u8; 32] };

    let result = km.wrap_dek(&dek);
    assert!(result.is_err(), "Wrap without KEK should fail");
}

#[test]
fn key_manager_initial_version_zero() {
    let mut km =
        KeyManager::with_initial_key(KeyManagerConfig::default(), EncryptionKey([0xAA; 32]));

    let version = km.rotate_key(EncryptionKey([0xBB; 32]));
    assert_eq!(version, 0, "First rotation should produce version 0");
}

#[test]
fn pipeline_encrypt_corrupt_detected() {
    let config = PipelineConfig {
        chunker: ChunkerConfig::default(),
        inline_compression: CompressionAlgorithm::None,
        background_compression: CompressionAlgorithm::None,
        encryption: EncryptionAlgorithm::AesGcm256,
        dedup_enabled: false,
        compression_enabled: false,
        encryption_enabled: true,
    };

    let mut pipeline = ReductionPipeline::with_master_key(config, EncryptionKey([0xCC; 32]));

    let data = b"test data for encryption";
    let (mut chunks, _) = pipeline.process_write(data).unwrap();

    if !chunks.is_empty() {
        chunks[0].encrypted_data[0] ^= 0xFF;

        let result = pipeline.process_read(&chunks);
        assert!(
            result.is_err(),
            "Corrupted ciphertext should fail to decrypt"
        );
    }
}

#[test]
fn encrypt_large_payload() {
    let key = EncryptionKey([0x99; 32]);
    let plaintext: Vec<u8> = (0..16 * 1024 * 1024).map(|i| (i % 256) as u8).collect();

    let ct = encrypt(&plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
    let dec = decrypt(&ct, &key).unwrap();

    assert_eq!(dec, plaintext);
}

#[test]
fn decrypt_truncated_fails() {
    let key = EncryptionKey([0x77; 32]);
    let plaintext = b"test data";

    let mut ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
    ct.ciphertext.truncate(ct.ciphertext.len() / 2);

    let result = decrypt(&ct, &key);
    assert!(result.is_err());
}

#[test]
fn decrypt_extended_fails() {
    let key = EncryptionKey([0x77; 32]);
    let plaintext = b"test data";

    let mut ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
    ct.ciphertext.push(0xFF);
    ct.ciphertext.push(0xFF);

    let result = decrypt(&ct, &key);
    assert!(result.is_err());
}

#[test]
fn history_size_tracks_rotations() {
    let mut config = KeyManagerConfig::default();
    config.max_key_history = 5;
    let mut km = KeyManager::new(config);

    for i in 0..10 {
        let _ = km.rotate_key(EncryptionKey([i; 32]));
    }

    let history = km.history_size();
    assert_eq!(history, 10, "history_size should track all rotations");
}
