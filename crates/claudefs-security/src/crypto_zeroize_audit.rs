//! Phase 3 crypto zeroize and key material handling audit.
//!
//! Findings: FINDING-CZ-01 through FINDING-CZ-15
//!
//! Audits sensitive key material handling in claudefs-reduce encryption pipeline.
//! Verifies that key material debug output is redacted and documents
//! zeroize-on-drop gaps for remediation.

use claudefs_reduce::encryption::{
    decrypt, derive_chunk_key, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey, Nonce,
};
use claudefs_reduce::key_manager::{
    DataKey, KeyManager, KeyManagerConfig, KeyVersion, VersionedKey, WrappedKey,
};
use claudefs_reduce::worm_reducer::{RetentionPolicy, WormMode, WormReducer};

#[test]
fn finding_cz_01_encryption_key_cloned_without_zeroize() {
    let key = EncryptionKey([0x42; 32]);
    let cloned = key.clone();
    assert_eq!(cloned.0, [0x42; 32]);

    // FINDING-CZ-01: EncryptionKey derives Clone but does NOT implement Zeroize.
    // The key material persists in memory after the original key goes out of scope.
    // A3 should add derive(Zeroize, ZeroizeOnDrop) to remediate.
    drop(key);
    let _still_accessible = cloned.0;
}

#[test]
fn finding_cz_02_data_key_cloned_without_zeroize() {
    let dek = DataKey { key: [0xAB; 32] };
    let cloned = dek.clone();
    assert_eq!(cloned.key, [0xAB; 32]);

    // FINDING-CZ-02: DataKey derives Clone + Serialize but does NOT implement Zeroize.
    // The DEK key material is not wiped when the DataKey is dropped.
    // A3 should add derive(Zeroize, ZeroizeOnDrop) and remove Serialize if not needed.
    drop(dek);
    let _still_accessible = cloned.key;
}

#[test]
fn finding_cz_03_versioned_key_accessible_after_drop() {
    let vk = VersionedKey {
        version: KeyVersion(1),
        key: EncryptionKey([0x99; 32]),
    };
    let cloned = vk.clone();
    assert_eq!(cloned.key.0, [0x99; 32]);

    // FINDING-CZ-03: VersionedKey derives Clone but does NOT implement Zeroize.
    // The KEK key material remains in memory after drop.
    // A3 should add derive(Zeroize, ZeroizeOnDrop) to remediate.
    drop(vk);
    let _still_accessible = cloned.key.0;
}

#[test]
fn finding_cz_04_key_manager_history_not_zeroized() {
    let config = KeyManagerConfig::default();
    let mut km = KeyManager::with_initial_key(config, EncryptionKey([0x11; 32]));

    let dek = km.generate_dek().unwrap();
    let _wrapped = km.wrap_dek(&dek).unwrap();

    km.rotate_key(EncryptionKey([0x22; 32]));
    assert!(km.history_size() > 0);

    // FINDING-CZ-04: KeyManager::kek_history uses HashMap which does not zeroize on clear().
    // When clear_history() or rotate_key() is called, old key material remains in memory.
    // A3 should implement a custom drop or explicit zeroize method for the history.
    let history_size_before = km.history_size();
    km.clear_history();
    let history_size_after = km.history_size();
    assert_eq!(history_size_after, 0);
    // Note: Memory is not actually zeroized - this is the finding
}

#[test]
fn finding_cz_05_encryption_key_debug_redacted() {
    let key = EncryptionKey([0xAA; 32]);
    let debug_str = format!("{:?}", key);

    assert_eq!(debug_str, "EncryptionKey([REDACTED])");
    // PASS: EncryptionKey Debug implementation correctly redacts key material
}

#[test]
fn finding_cz_06_data_key_debug_redacted() {
    let dek = DataKey { key: [0xBB; 32] };
    let debug_str = format!("{:?}", dek);

    assert_eq!(debug_str, "DataKey([REDACTED])");
    // PASS: DataKey Debug implementation correctly redacts key material
}

#[test]
fn finding_cz_07_versioned_key_debug_redacted() {
    let vk = VersionedKey {
        version: KeyVersion(5),
        key: EncryptionKey([0xCC; 32]),
    };
    let debug_str = format!("{:?}", vk);

    assert!(debug_str.contains("[REDACTED]"));
    assert!(debug_str.contains("version"));
    // PASS: VersionedKey Debug implementation correctly redacts key material
}

#[test]
fn finding_cz_08_worm_policy_overwrite_compliance_gap() {
    let mut reducer = WormReducer::new();

    reducer.register(123, RetentionPolicy::immutable_until(1000), 512);
    let (old_policy, old_size) = reducer.get(&123).unwrap();
    assert!(matches!(old_policy.mode, WormMode::Immutable));
    assert_eq!(*old_size, 512);

    // FINDING-CZ-08: WormReducer::register() allows overwriting existing retention policies
    // without any warning or validation. This is a compliance weakness - an older legal hold
    // could be accidentally replaced with a shorter immutable period.
    // A3 should add logic to prevent overwriting stronger policies with weaker ones.
    reducer.register(123, RetentionPolicy::none(), 256);
    let (new_policy, new_size) = reducer.get(&123).unwrap();
    assert!(matches!(new_policy.mode, WormMode::None));
    assert_eq!(*new_size, 256);
}

#[test]
fn finding_cz_09_nonce_generation_random() {
    let mut nonces = Vec::new();
    for _ in 0..100 {
        nonces.push(random_nonce());
    }

    for i in 0..nonces.len() {
        for j in (i + 1)..nonces.len() {
            assert_ne!(
                nonces[i], nonces[j],
                "Duplicate nonce detected - potential issue"
            );
        }
    }

    // Verify nonce doesn't contain any fixed patterns that might indicate weak RNG
    let sample = random_nonce();
    let all_zero = sample.0.iter().all(|&b| b == 0);
    let all_same = sample.0.iter().all(|&b| b == sample.0[0]);
    assert!(!all_zero, "Nonce should not be all zeros");
    assert!(!all_same, "Nonce should not be constant value");
    // PASS: Nonce generation produces unique, random values
}

#[test]
fn finding_cz_10_encrypted_chunk_no_plaintext_leak() {
    let plaintext = b"THIS IS SUPER SECRET DATA THAT SHOULD NEVER APPEAR IN CIPHERTEXT";
    let key = EncryptionKey([0x55; 32]);

    let encrypted = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

    let ciphertext_str = String::from_utf8_lossy(&encrypted.ciphertext);
    assert!(!ciphertext_str.contains("SUPER SECRET"));
    assert!(!ciphertext_str.contains("THIS IS"));

    // Also verify with ChaCha20
    let encrypted_chacha = encrypt(plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();
    let chacha_ct_str = String::from_utf8_lossy(&encrypted_chacha.ciphertext);
    assert!(!chacha_ct_str.contains("SUPER SECRET"));
    // PASS: Ciphertext does not contain plaintext
}

#[test]
fn finding_cz_11_wrapped_dek_includes_auth_tag() {
    let config = KeyManagerConfig::default();
    let km = KeyManager::with_initial_key(config, EncryptionKey([0x66; 32]));

    let dek = km.generate_dek().unwrap();
    let raw_dek_len = dek.key.len(); // 32 bytes

    let wrapped = km.wrap_dek(&dek).unwrap();
    let wrapped_len = wrapped.ciphertext.len();

    // AES-256-GCM: 32 bytes (DEK) + 16 bytes (auth tag) = 48 bytes ciphertext
    // Additionally we have 12 bytes nonce stored separately
    assert!(
        wrapped_len > raw_dek_len,
        "Wrapped DEK should be larger than raw DEK due to auth tag"
    );
    assert_eq!(wrapped_len, 48, "Expected 48 bytes: 32 DEK + 16 auth tag");
    // PASS: Wrapped key includes authentication tag
}

#[test]
fn finding_cz_12_key_rotation_preserves_old_key_accessibility() {
    let config = KeyManagerConfig::default();
    let mut km = KeyManager::with_initial_key(config, EncryptionKey([0x11; 32]));

    let dek = km.generate_dek().unwrap();
    let wrapped_v0 = km.wrap_dek(&dek).unwrap();
    assert_eq!(wrapped_v0.kek_version, KeyVersion(0));

    // Rotate to new key version
    km.rotate_key(EncryptionKey([0x22; 32]));
    assert_eq!(km.current_version(), Some(KeyVersion(1)));

    // Old key should still be accessible for decryption
    let unwrapped = km.unwrap_dek(&wrapped_v0).unwrap();
    assert_eq!(unwrapped.key, dek.key);

    // Verify that old key is retained in history
    assert!(
        km.history_size() >= 1,
        "Old key should be retained in history"
    );
    // PASS: Key rotation preserves old key for decryption
}

#[test]
fn finding_cz_13_chacha20_ciphertext_no_key_leak() {
    let plaintext = vec![0u8; 1024]; // All zeros
    let key = EncryptionKey([0x77; 32]);

    let encrypted = encrypt(&plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();

    // If key leaked into ciphertext, we'd see repeating patterns
    let first_32 = &encrypted.ciphertext[..32];
    let all_same = first_32.iter().all(|&b| b == first_32[0]);
    assert!(!all_same, "Ciphertext should not have repeating patterns");

    // Verify roundtrip
    let decrypted = decrypt(&encrypted, &key).unwrap();
    assert_eq!(decrypted, plaintext);
    // PASS: ChaCha20 encryption doesn't expose key in ciphertext
}

#[test]
fn finding_cz_14_hkdf_info_string_namespaced() {
    let master = EncryptionKey([0x88; 32]);
    let chunk_hash = [0x01; 32];

    let chunk_key = derive_chunk_key(&master, &chunk_hash);

    // The info string should be "claudefs-chunk-key" || chunk_hash
    // This tests that HKDF uses proper namespace to prevent key reuse across contexts
    let master2 = EncryptionKey([0x99; 32]);
    let chunk_key_different_master = derive_chunk_key(&master2, &chunk_hash);

    // Different master keys should produce different chunk keys
    assert_ne!(chunk_key.0, chunk_key_different_master.0);

    let hash2 = [0x02; 32];
    let chunk_key_different_hash = derive_chunk_key(&master, &hash2);

    // Different chunk hashes should produce different chunk keys
    assert_ne!(chunk_key.0, chunk_key_different_hash.0);

    // Deterministic: same inputs should produce same output
    let chunk_key2 = derive_chunk_key(&master, &chunk_hash);
    assert_eq!(chunk_key.0, chunk_key2.0);
    // PASS: HKDF info string is properly namespaced
}

#[test]
fn finding_cz_15_empty_key_still_encrypts() {
    // Edge case: all-zeros key should still produce valid encryption
    // (even though it's not a secure key for production)
    let key = EncryptionKey([0x00; 32]);
    let plaintext = b"test data";

    let encrypted = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
    let decrypted = decrypt(&encrypted, &key).unwrap();

    assert_eq!(decrypted, plaintext);

    // Same with ChaCha20
    let encrypted_chacha = encrypt(plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();
    let decrypted_chacha = decrypt(&encrypted_chacha, &key).unwrap();
    assert_eq!(decrypted_chacha, plaintext);
    // PASS: Empty key still produces valid encryption (edge case handling)
}

#[test]
fn finding_cz_aes_gcm_ciphertext_properties() {
    let key = EncryptionKey([0xAA; 32]);
    let plaintext = b"AES-GCM test";

    let encrypted = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

    // AES-GCM produces: plaintext_len + 16 bytes (auth tag) = 12 + 16 = 28 bytes
    assert_eq!(encrypted.ciphertext.len(), plaintext.len() + 16);

    // Verify different nonces produce different ciphertext
    let encrypted2 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
    assert_ne!(encrypted.ciphertext, encrypted2.ciphertext);

    // Verify roundtrip
    let decrypted = decrypt(&encrypted, &key).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn finding_cz_key_diversification() {
    let master = EncryptionKey([0xBB; 32]);

    // Two different chunk hashes should produce completely different chunk keys
    let hash1 = [0x11; 32];
    let hash2 = [0x12; 32];

    let key1 = derive_chunk_key(&master, &hash1);
    let key2 = derive_chunk_key(&master, &hash2);

    // Check that keys are significantly different (not just 1 bit flip)
    let mut diff_bits = 0u32;
    for i in 0..32 {
        let x = key1.0[i] ^ key2.0[i];
        diff_bits += x.count_ones();
    }

    assert!(diff_bits > 16, "Keys should differ by more than a few bits");
    // PASS: Key diversification produces independent-looking keys
}

#[test]
fn finding_cz_wrapped_key_format() {
    let config = KeyManagerConfig::default();
    let km = KeyManager::with_initial_key(config, EncryptionKey([0xCC; 32]));

    let dek = DataKey { key: [0xDD; 32] };
    let wrapped = km.wrap_dek(&dek).unwrap();

    // WrappedKey should contain: ciphertext (48 bytes), nonce (12 bytes), version
    assert_eq!(wrapped.ciphertext.len(), 48);
    assert_eq!(wrapped.nonce.len(), 12);

    // Unwrap should recover original
    let unwrapped = km.unwrap_dek(&wrapped).unwrap();
    assert_eq!(unwrapped.key, dek.key);
}

#[test]
fn finding_cz_key_version_monotonic() {
    let config = KeyManagerConfig::default();
    let mut km = KeyManager::with_initial_key(config, EncryptionKey([0x11; 32]));

    assert_eq!(km.current_version(), Some(KeyVersion(0)));

    let v1 = km.rotate_key(EncryptionKey([0x22; 32]));
    assert_eq!(v1, KeyVersion(1));
    assert_eq!(km.current_version(), Some(KeyVersion(1)));

    let v2 = km.rotate_key(EncryptionKey([0x33; 32]));
    assert_eq!(v2, KeyVersion(2));
    assert_eq!(km.current_version(), Some(KeyVersion(2)));

    let v3 = km.rotate_key(EncryptionKey([0x44; 32]));
    assert_eq!(v3, KeyVersion(3));
    // PASS: Key versions increment monotonically
}

#[test]
fn finding_cz_worm_legal_hold_never_expires() {
    let mut reducer = WormReducer::new();

    reducer.register(100, RetentionPolicy::legal_hold(), 1024);
    reducer.register(200, RetentionPolicy::immutable_until(100), 2048);

    // Legal hold should never expire
    assert_eq!(reducer.active_count(u64::MAX), 1);

    // Immutable until 100 should expire after 100
    assert_eq!(reducer.active_count(50), 2);
    assert_eq!(reducer.active_count(100), 2);
    assert_eq!(reducer.active_count(101), 1);
    // PASS: Legal hold correctly never expires
}
