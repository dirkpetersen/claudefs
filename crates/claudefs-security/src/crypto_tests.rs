//! Cryptographic security property tests.
//!
//! Verifies security-critical properties of the encryption subsystem:
//! - Nonce uniqueness across many encryptions
//! - Key isolation between different chunks
//! - Ciphertext indistinguishability
//! - Auth tag integrity verification
//! - Deterministic key derivation consistency

use claudefs_reduce::encryption::{
    decrypt, derive_chunk_key, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey,
};
use claudefs_reduce::key_manager::{KeyManager, KeyManagerConfig, KeyVersion};
use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_nonce_uniqueness_1000() {
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let nonce = random_nonce();
            let bytes = nonce.0;
            assert!(
                seen.insert(bytes),
                "Nonce collision detected in 1000 samples"
            );
        }
    }

    #[test]
    fn test_nonce_not_all_zeros() {
        for _ in 0..100 {
            let nonce = random_nonce();
            assert_ne!(nonce.0, [0u8; 12], "Nonce should never be all zeros");
        }
    }

    #[test]
    fn test_different_master_keys_different_derived() {
        let key1 = EncryptionKey([1u8; 32]);
        let key2 = EncryptionKey([2u8; 32]);
        let hash = [0u8; 32];

        let derived1 = derive_chunk_key(&key1, &hash);
        let derived2 = derive_chunk_key(&key2, &hash);
        assert_ne!(
            derived1.0, derived2.0,
            "Different master keys must derive different chunk keys"
        );
    }

    #[test]
    fn test_different_hashes_different_derived() {
        let key = EncryptionKey([42u8; 32]);
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];

        let derived1 = derive_chunk_key(&key, &hash1);
        let derived2 = derive_chunk_key(&key, &hash2);
        assert_ne!(
            derived1.0, derived2.0,
            "Different chunk hashes must derive different keys"
        );
    }

    #[test]
    fn test_key_derivation_deterministic() {
        let key = EncryptionKey([42u8; 32]);
        let hash = [99u8; 32];

        let derived1 = derive_chunk_key(&key, &hash);
        let derived2 = derive_chunk_key(&key, &hash);
        assert_eq!(derived1.0, derived2.0, "Same inputs must derive same key");
    }

    #[test]
    fn test_derived_key_not_equal_to_master() {
        let key = EncryptionKey([42u8; 32]);
        let hash = [0u8; 32];
        let derived = derive_chunk_key(&key, &hash);
        assert_ne!(derived.0, key.0, "Derived key must differ from master key");
    }

    #[test]
    fn test_same_plaintext_different_ciphertext_aesgcm() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"same data same data";

        let ct1 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        let ct2 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

        assert_ne!(
            ct1.ciphertext, ct2.ciphertext,
            "Same plaintext encrypted twice must produce different ciphertext"
        );
        assert_ne!(
            ct1.nonce.0, ct2.nonce.0,
            "Each encryption must use a different nonce"
        );
    }

    #[test]
    fn test_same_plaintext_different_ciphertext_chacha() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"same data same data";

        let ct1 = encrypt(plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();
        let ct2 = encrypt(plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();

        assert_ne!(ct1.ciphertext, ct2.ciphertext);
        assert_ne!(ct1.nonce.0, ct2.nonce.0);
    }

    #[test]
    fn test_ciphertext_longer_than_plaintext() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"test data";

        let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        assert_eq!(
            ct.ciphertext.len(),
            plaintext.len() + 16,
            "Ciphertext should be plaintext + 16-byte auth tag"
        );
    }

    #[test]
    fn test_single_bit_flip_detected_aesgcm() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"sensitive data here";
        let mut ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

        for byte_idx in 0..ct.ciphertext.len() {
            for bit_idx in 0..8 {
                ct.ciphertext[byte_idx] ^= 1 << bit_idx;
                let result = decrypt(&ct, &key);
                assert!(
                    result.is_err(),
                    "Bit flip at byte {byte_idx} bit {bit_idx} must be detected"
                );
                ct.ciphertext[byte_idx] ^= 1 << bit_idx;
            }
        }
    }

    #[test]
    fn test_single_bit_flip_detected_chacha() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"sensitive data here";
        let mut ct = encrypt(plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();

        ct.ciphertext[0] ^= 0x01;
        assert!(
            decrypt(&ct, &key).is_err(),
            "Tampering at start must be detected"
        );
        ct.ciphertext[0] ^= 0x01;

        let last = ct.ciphertext.len() - 1;
        ct.ciphertext[last] ^= 0x01;
        assert!(
            decrypt(&ct, &key).is_err(),
            "Tampering at end must be detected"
        );
    }

    #[test]
    fn test_nonce_tamper_detected() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"sensitive data";
        let mut ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

        ct.nonce.0[0] ^= 0xFF;
        assert!(
            decrypt(&ct, &key).is_err(),
            "Modified nonce must cause decryption failure"
        );
    }

    #[test]
    fn test_wrong_key_decryption_fails() {
        let key1 = EncryptionKey([1u8; 32]);
        let key2 = EncryptionKey([2u8; 32]);
        let plaintext = b"secret message";

        let ct = encrypt(plaintext, &key1, EncryptionAlgorithm::AesGcm256).unwrap();
        assert!(
            decrypt(&ct, &key2).is_err(),
            "Decryption with wrong key must fail"
        );
    }

    #[test]
    fn test_cross_algorithm_decryption_fails() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"test data";

        let mut ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        ct.algo = EncryptionAlgorithm::ChaCha20Poly1305;
        assert!(
            decrypt(&ct, &key).is_err(),
            "Cross-algorithm decryption must fail"
        );
    }

    #[test]
    fn test_empty_plaintext_roundtrip() {
        let key = EncryptionKey([42u8; 32]);
        for algo in [
            EncryptionAlgorithm::AesGcm256,
            EncryptionAlgorithm::ChaCha20Poly1305,
        ] {
            let ct = encrypt(b"", &key, algo).unwrap();
            let pt = decrypt(&ct, &key).unwrap();
            assert!(
                pt.is_empty(),
                "Empty plaintext roundtrip failed for {:?}",
                algo
            );
        }
    }

    #[test]
    fn test_large_plaintext_roundtrip() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = vec![0xABu8; 1024 * 1024];
        for algo in [
            EncryptionAlgorithm::AesGcm256,
            EncryptionAlgorithm::ChaCha20Poly1305,
        ] {
            let ct = encrypt(&plaintext, &key, algo).unwrap();
            let pt = decrypt(&ct, &key).unwrap();
            assert_eq!(
                pt, plaintext,
                "Large plaintext roundtrip failed for {:?}",
                algo
            );
        }
    }

    #[test]
    fn test_dek_randomness() {
        let config = KeyManagerConfig {
            max_key_history: 10,
        };
        let km = KeyManager::new(config);
        let dek1 = km.generate_dek().unwrap();
        let dek2 = km.generate_dek().unwrap();
        assert_ne!(dek1.key, dek2.key, "Generated DEKs must be random");
    }

    #[test]
    fn test_wrapped_dek_contains_auth_tag() {
        let config = KeyManagerConfig {
            max_key_history: 10,
        };
        let mut km = KeyManager::new(config);
        km.rotate_key(EncryptionKey([42u8; 32]));
        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        assert_eq!(
            wrapped.ciphertext.len(),
            48,
            "Wrapped DEK must be 32 (DEK) + 16 (auth tag) = 48 bytes"
        );
    }

    #[test]
    fn test_wrapped_dek_tamper_detected() {
        let config = KeyManagerConfig {
            max_key_history: 10,
        };
        let mut km = KeyManager::new(config);
        km.rotate_key(EncryptionKey([42u8; 32]));
        let dek = km.generate_dek().unwrap();
        let mut wrapped = km.wrap_dek(&dek).unwrap();

        wrapped.ciphertext[0] ^= 0xFF;
        assert!(
            km.unwrap_dek(&wrapped).is_err(),
            "Tampered wrapped DEK must fail to unwrap"
        );
    }

    #[test]
    fn test_key_rotation_preserves_decryption() {
        let config = KeyManagerConfig {
            max_key_history: 10,
        };
        let mut km = KeyManager::new(config);

        km.rotate_key(EncryptionKey([1u8; 32]));
        let dek = km.generate_dek().unwrap();
        let wrapped_v0 = km.wrap_dek(&dek).unwrap();

        km.rotate_key(EncryptionKey([2u8; 32]));

        let unwrapped = km.unwrap_dek(&wrapped_v0).unwrap();
        assert_eq!(
            unwrapped.key, dek.key,
            "DEK from old key version must still be accessible"
        );
    }

    proptest! {
        #[test]
        fn prop_encrypt_decrypt_roundtrip_aesgcm(
            plaintext in proptest::collection::vec(any::<u8>(), 0..4096),
            key_bytes in proptest::collection::vec(any::<u8>(), 32..=32),
        ) {
            let mut key_arr = [0u8; 32];
            key_arr.copy_from_slice(&key_bytes);
            let key = EncryptionKey(key_arr);
            let ct = encrypt(&plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
            let pt = decrypt(&ct, &key).unwrap();
            prop_assert_eq!(pt, plaintext);
        }

        #[test]
        fn prop_encrypt_decrypt_roundtrip_chacha(
            plaintext in proptest::collection::vec(any::<u8>(), 0..4096),
            key_bytes in proptest::collection::vec(any::<u8>(), 32..=32),
        ) {
            let mut key_arr = [0u8; 32];
            key_arr.copy_from_slice(&key_bytes);
            let key = EncryptionKey(key_arr);
            let ct = encrypt(&plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();
            let pt = decrypt(&ct, &key).unwrap();
            prop_assert_eq!(pt, plaintext);
        }

        #[test]
        fn prop_different_keys_different_ciphertext(
            plaintext in proptest::collection::vec(any::<u8>(), 16..256),
        ) {
            let key1 = EncryptionKey([1u8; 32]);
            let key2 = EncryptionKey([2u8; 32]);
            let ct1 = encrypt(&plaintext, &key1, EncryptionAlgorithm::AesGcm256).unwrap();
            let ct2 = encrypt(&plaintext, &key2, EncryptionAlgorithm::AesGcm256).unwrap();
            prop_assert_ne!(ct1.ciphertext, ct2.ciphertext);
        }

        #[test]
        fn prop_derived_keys_are_256_bit(
            master_bytes in proptest::collection::vec(any::<u8>(), 32..=32),
            hash_bytes in proptest::collection::vec(any::<u8>(), 32..=32),
        ) {
            let mut master_arr = [0u8; 32];
            master_arr.copy_from_slice(&master_bytes);
            let mut hash_arr = [0u8; 32];
            hash_arr.copy_from_slice(&hash_bytes);
            let key = EncryptionKey(master_arr);
            let derived = derive_chunk_key(&key, &hash_arr);
            prop_assert_eq!(derived.0.len(), 32, "Derived key must be 256 bits");
        }
    }
}
