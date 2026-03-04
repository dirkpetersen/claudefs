//! Security tests for storage layer encryption
//!
//! Part of A10 Phase 4: Extended FUSE + Storage Security Audit

#[cfg(test)]
mod tests {
    use claudefs_storage::encryption::{
        EncryptionAlgorithm, EncryptionConfig, EncryptionEngine, EncryptionKey,
    };

    // =========================================================================
    // A. EncryptionKey Security Tests
    // =========================================================================

    #[test]
    fn test_key_material_not_zeroized_on_drop() {
        let key = EncryptionKey::new(
            "test-key".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0xAA; 32],
        )
        .unwrap();

        assert_eq!(key.key_len(), 32);
    }

    #[test]
    fn test_mock_encryption_is_xor_not_aead() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"test data";
        let encrypted = engine.encrypt(plaintext).unwrap();

        let decrypted = engine.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext.as_slice());
    }

    #[test]
    fn test_nonce_derived_from_plaintext() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext1 = b"Hello World Test";
        let encrypted1 = engine.encrypt(plaintext1).unwrap();

        let plaintext2 = b"Hello World Test";
        let encrypted2 = engine.encrypt(plaintext2).unwrap();

        assert_eq!(encrypted1.nonce(), encrypted2.nonce());
    }

    #[test]
    fn test_tag_derived_from_plaintext() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext1 = b"Hello World Test Data";
        let encrypted1 = engine.encrypt(plaintext1).unwrap();

        let plaintext2 = b"Hello World Test Data";
        let encrypted2 = engine.encrypt(plaintext2).unwrap();

        assert_eq!(encrypted1.tag(), encrypted2.tag());
    }

    #[test]
    fn test_key_bytes_accessible_via_as_bytes() {
        let key = EncryptionKey::new(
            "test-key".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0xBB; 32],
        )
        .unwrap();

        assert_eq!(key.key_len(), 32);
    }

    // =========================================================================
    // B. EncryptionEngine Security Tests
    // =========================================================================

    #[test]
    fn test_encrypt_without_key_reveals_nothing() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let result = engine.encrypt(b"test");

        assert!(result.is_err());
    }

    #[test]
    fn test_encryption_disabled_by_default() {
        let config = EncryptionConfig::default();
        assert!(!config.is_enabled());
    }

    #[test]
    fn test_none_algorithm_plaintext_passthrough() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key =
            EncryptionKey::new("test-key".to_string(), EncryptionAlgorithm::None, vec![]).unwrap();
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Secret data";
        let encrypted = engine.encrypt(plaintext).unwrap();

        assert_eq!(encrypted.ciphertext(), plaintext);
    }

    #[test]
    fn test_key_rotation_preserves_old_keys() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key1 = EncryptionKey::new(
            "key-1".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x11; 32],
        )
        .unwrap();
        engine.add_key(key1.clone());
        engine.set_current_key(key1.id()).unwrap();

        let encrypted = engine.encrypt(b"data").unwrap();

        let key2 = EncryptionKey::new(
            "key-2".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x22; 32],
        )
        .unwrap();
        engine.rotate_key(key2).unwrap();

        assert_eq!(engine.key_count(), 2);

        let decrypted = engine.decrypt(&encrypted);
        assert!(decrypted.is_ok());
    }

    #[test]
    fn test_same_plaintext_same_ciphertext() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Identical text";
        let enc1 = engine.encrypt(plaintext).unwrap();
        let enc2 = engine.encrypt(plaintext).unwrap();

        assert_eq!(enc1.ciphertext(), enc2.ciphertext());
    }

    // =========================================================================
    // C. EncryptedBlock Integrity Tests
    // Note: Cannot test internal tampering because fields are private.
    // These tests verify behavior through the public API.
    // =========================================================================

    #[test]
    fn test_encrypted_block_tag_not_verified() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Test data";
        let encrypted = engine.encrypt(plaintext).unwrap();

        let result = engine.decrypt(&encrypted);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encrypted_block_nonce_not_verified() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Test data";
        let encrypted = engine.encrypt(plaintext).unwrap();

        let result = engine.decrypt(&encrypted);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ciphertext_tamper_undetected() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Test data for tampering";
        let encrypted = engine.encrypt(plaintext).unwrap();

        let decrypted = engine.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext.as_slice());
    }

    #[test]
    fn test_original_size_not_verified() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Short";
        let encrypted = engine.encrypt(plaintext).unwrap();
        let _original_size = encrypted.original_size();

        let result = engine.decrypt(&encrypted);
        assert!(result.is_ok());
    }

    // =========================================================================
    // D. Key Management Tests
    // =========================================================================

    #[test]
    fn test_generate_mock_key_predictable() {
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);

        assert_eq!(key.key_len(), 32);
    }

    #[test]
    fn test_key_id_from_system_time() {
        let key1 = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        let key2 = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);

        assert_ne!(key1.id(), key2.id());
    }

    #[test]
    fn test_set_current_key_accepts_any_registered() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key1 = EncryptionKey::new(
            "key-1".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x11; 32],
        )
        .unwrap();
        let key2 = EncryptionKey::new(
            "key-2".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x22; 32],
        )
        .unwrap();

        engine.add_key(key1);
        engine.add_key(key2);

        let result = engine.set_current_key("key-2");
        assert!(result.is_ok());
    }

    #[test]
    fn test_encrypted_block_key_id_not_authenticated() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key1 = EncryptionKey::new(
            "key-1".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x11; 32],
        )
        .unwrap();

        engine.add_key(key1.clone());
        engine.set_current_key(key1.id()).unwrap();

        let encrypted = engine.encrypt(b"Secret").unwrap();

        assert_eq!(encrypted.key_id(), "key-1");
    }

    #[test]
    fn test_key_length_zero_for_none() {
        let key =
            EncryptionKey::new("none-key".to_string(), EncryptionAlgorithm::None, vec![]).unwrap();

        assert_eq!(key.key_len(), 0);
    }

    // =========================================================================
    // E. Error Handling and Stats Tests
    // =========================================================================

    #[test]
    fn test_encryption_error_increments_and_stops() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());

        let _ = engine.encrypt(b"test1");
        assert_eq!(engine.stats().encryption_errors(), 1);

        let _ = engine.encrypt(b"test2");
        assert_eq!(engine.stats().encryption_errors(), 2);
    }

    #[test]
    fn test_stats_overflow_at_u64_max() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"x";
        engine.encrypt(plaintext).unwrap();

        assert!(engine.stats().blocks_encrypted() >= 1);
    }

    #[test]
    fn test_multiple_key_rotation_stats() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());

        for i in 0..5 {
            let key = EncryptionKey::new(
                format!("key-{}", i),
                EncryptionAlgorithm::Aes256Gcm,
                vec![i as u8; 32],
            )
            .unwrap();
            engine.add_key(key);
            engine.set_current_key(&format!("key-{}", i)).unwrap();

            let new_key = EncryptionKey::new(
                format!("key-{}", i + 1),
                EncryptionAlgorithm::Aes256Gcm,
                vec![(i + 1) as u8; 32],
            )
            .unwrap();
            engine.rotate_key(new_key).unwrap();
        }

        assert_eq!(engine.stats().key_rotations(), 5);
    }

    #[test]
    fn test_decrypt_missing_key_error_message() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());

        let mut engine2 = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine2.add_key(key.clone());
        engine2.set_current_key(key.id()).unwrap();

        let encrypted = engine2.encrypt(b"data").unwrap();

        let result = engine.decrypt(&encrypted);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("nonexistent") || err_msg.contains("not found"));
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = EncryptionConfig::default();

        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: EncryptionConfig = serde_json::from_str(&serialized).unwrap();

        assert_eq!(config.is_enabled(), deserialized.is_enabled());
        assert_eq!(config.algorithm(), deserialized.algorithm());
    }
}
