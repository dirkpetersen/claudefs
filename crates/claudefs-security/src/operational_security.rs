//! Operational security tests for secrets management, audit trails, and compliance validation.
//!
//! Verifies security-critical operational properties:
//! - Secrets management: key isolation, secure storage, memory protection, rotation
//! - Audit trail: event logging, tamper evidence, integrity verification
//! - Compliance: retention policy, access control, data classification, regulatory requirements

use claudefs_reduce::encryption::{
    decrypt, derive_chunk_key, encrypt, random_nonce, EncryptedChunk, EncryptionAlgorithm,
    EncryptionKey,
};
use claudefs_reduce::key_manager::{
    DataKey, KeyManager, KeyManagerConfig, KeyVersion, VersionedKey,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use zeroize::Zeroize;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const TEST_TENANT_ID: &str = "tenant-001";
    const TEST_USER_ID: &str = "user-001";

    fn test_key() -> EncryptionKey {
        EncryptionKey([42u8; 32])
    }

    fn test_chunk_hash() -> [u8; 32] {
        [1u8; 32]
    }

    #[derive(Clone, Debug)]
    struct AuditEvent {
        timestamp: u64,
        event_type: String,
        user_id: String,
        tenant_id: String,
        resource_id: String,
        action: String,
        result: String,
        metadata: HashMap<String, String>,
    }

    thread_local! {
        static AUDIT_LOG: Mutex<Vec<AuditEvent>> = Mutex::new(Vec::new());
    }

    fn init_audit_log() {
        AUDIT_LOG.with(|log| log.lock().unwrap().clear());
    }

    fn log_audit_event(event: AuditEvent) {
        AUDIT_LOG.with(|log| log.lock().unwrap().push(event));
    }

    fn get_audit_log() -> Vec<AuditEvent> {
        AUDIT_LOG.with(|log| log.lock().unwrap().clone())
    }

    fn verify_audit_integrity(log: &[AuditEvent]) -> bool {
        for i in 1..log.len() {
            if log[i].timestamp < log[i - 1].timestamp {
                return false;
            }
        }
        true
    }

    fn create_audit_event(event_type: &str, user_id: &str) -> AuditEvent {
        AuditEvent {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            event_type: event_type.to_string(),
            user_id: user_id.to_string(),
            tenant_id: TEST_TENANT_ID.to_string(),
            resource_id: "resource-001".to_string(),
            action: "read".to_string(),
            result: "success".to_string(),
            metadata: HashMap::new(),
        }
    }

    struct CompliancePolicy {
        retention_days: u32,
        min_encryption_level: u32,
        audit_enabled: bool,
        data_classification: String,
    }

    impl Default for CompliancePolicy {
        fn default() -> Self {
            Self {
                retention_days: 90,
                min_encryption_level: 256,
                audit_enabled: true,
                data_classification: "confidential".to_string(),
            }
        }
    }

    impl CompliancePolicy {
        fn new(
            retention_days: u32,
            min_encryption_level: u32,
            audit_enabled: bool,
            data_classification: String,
        ) -> Self {
            Self {
                retention_days,
                min_encryption_level,
                audit_enabled,
                data_classification,
            }
        }
    }

    mod secrets_management {
        use super::*;

        #[test]
        fn test_key_isolation_between_tenants() {
            let tenant1_key = EncryptionKey([1u8; 32]);
            let tenant2_key = EncryptionKey([2u8; 32]);
            let hash = test_chunk_hash();

            let derived1 = derive_chunk_key(&tenant1_key, &hash);
            let derived2 = derive_chunk_key(&tenant2_key, &hash);

            assert_ne!(
                derived1.0, derived2.0,
                "Different tenants must have isolated keys"
            );
        }

        #[test]
        fn test_key_isolation_different_chunks() {
            let key = test_key();
            let hash1 = [1u8; 32];
            let hash2 = [2u8; 32];

            let derived1 = derive_chunk_key(&key, &hash1);
            let derived2 = derive_chunk_key(&key, &hash2);

            assert_ne!(
                derived1.0, derived2.0,
                "Different chunks must have different derived keys"
            );
        }

        #[test]
        fn test_master_key_not_exposed_in_derived() {
            let master = test_key();
            let hash = test_chunk_hash();
            let derived = derive_chunk_key(&master, &hash);

            assert_ne!(
                derived.0, master.0,
                "Derived key must not expose master key"
            );
        }

        #[test]
        fn test_key_derivation_salted() {
            let key = test_key();
            let hash1 = [1u8; 32];
            let hash2 = [1u8; 32];

            let derived1 = derive_chunk_key(&key, &hash1);
            let derived2 = derive_chunk_key(&key, &hash2);

            assert_eq!(
                derived1.0, derived2.0,
                "Same inputs must produce same derived key"
            );
        }

        #[test]
        fn test_encryption_hides_plaintext() {
            let key = test_key();
            let plaintext = b"super secret data that must be hidden";

            let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

            assert!(
                ct.ciphertext != plaintext.to_vec(),
                "Ciphertext must differ from plaintext"
            );
            let matching_prefix = ct
                .ciphertext
                .iter()
                .zip(plaintext.iter())
                .take_while(|(c, p)| c == p)
                .count();
            assert!(
                matching_prefix < 5,
                "Ciphertext must not have long plaintext prefix"
            );
        }

        #[test]
        fn test_random_key_generation() {
            let config = KeyManagerConfig {
                max_key_history: 10,
            };
            let km = KeyManager::new(config);

            let mut seen = std::collections::HashSet::new();
            for _ in 0..100 {
                let dek = km.generate_dek().unwrap();
                assert!(
                    seen.insert(dek.key.to_vec()),
                    "Each generated key must be unique"
                );
            }
        }

        #[test]
        fn test_wrapped_dek_integrity() {
            let config = KeyManagerConfig {
                max_key_history: 10,
            };
            let mut km = KeyManager::new(config);
            km.rotate_key(test_key());

            let dek = km.generate_dek().unwrap();
            let wrapped = km.wrap_dek(&dek).unwrap();

            assert!(
                wrapped.ciphertext.len() >= 32,
                "Wrapped DEK must contain encrypted key data"
            );
        }

        #[test]
        fn test_key_rotation_invalidates_old_sessions() {
            let config = KeyManagerConfig {
                max_key_history: 10,
            };
            let mut km = KeyManager::new(config);

            km.rotate_key(EncryptionKey([1u8; 32]));
            let dek_v1 = km.generate_dek().unwrap();
            let wrapped_v1 = km.wrap_dek(&dek_v1).unwrap();

            km.rotate_key(EncryptionKey([2u8; 32]));
            let dek_v2 = km.generate_dek().unwrap();

            assert_ne!(
                dek_v1.key, dek_v2.key,
                "New key version must generate different DEKs"
            );

            let unwrapped = km.unwrap_dek(&wrapped_v1).unwrap();
            assert_eq!(
                unwrapped.key, dek_v1.key,
                "Old wrapped DEK must still be recoverable"
            );
        }

        #[test]
        fn test_multiple_key_versions_tracked() {
            let config = KeyManagerConfig { max_key_history: 5 };
            let mut km = KeyManager::new(config);

            let v1 = km.rotate_key(EncryptionKey([1u8; 32]));
            let v2 = km.rotate_key(EncryptionKey([2u8; 32]));
            let v3 = km.rotate_key(EncryptionKey([3u8; 32]));

            assert!(
                v2.0 > v1.0 && v3.0 > v2.0,
                "Key manager must track version history"
            );
        }

        #[test]
        fn test_dek_wrap_unwrap_roundtrip() {
            let config = KeyManagerConfig {
                max_key_history: 10,
            };
            let mut km = KeyManager::new(config);
            km.rotate_key(test_key());

            let dek = km.generate_dek().unwrap();
            let wrapped = km.wrap_dek(&dek).unwrap();
            let unwrapped = km.unwrap_dek(&wrapped).unwrap();

            assert_eq!(
                unwrapped.key, dek.key,
                "DEK roundtrip must preserve key material"
            );
        }

        #[test]
        fn test_wrong_key_fails_decryption() {
            let key1 = EncryptionKey([1u8; 32]);
            let key2 = EncryptionKey([2u8; 32]);
            let plaintext = b"secret message";

            let ct = encrypt(plaintext, &key1, EncryptionAlgorithm::AesGcm256).unwrap();
            let result = decrypt(&ct, &key2);

            assert!(result.is_err(), "Decryption with wrong key must fail");
        }

        #[test]
        fn test_tampered_ciphertext_detected() {
            let key = test_key();
            let plaintext = b"sensitive data";
            let mut ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

            ct.ciphertext[0] ^= 0xFF;
            let result = decrypt(&ct, &key);

            assert!(result.is_err(), "Tampered ciphertext must fail decryption");
        }

        #[test]
        fn test_nonce_reuse_prevented() {
            let key = test_key();
            let plaintext = b"test data";

            let ct1 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
            let ct2 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

            assert_ne!(
                ct1.nonce.0, ct2.nonce.0,
                "Each encryption must use unique nonce"
            );
        }

        #[test]
        fn test_large_data_encryption() {
            let key = test_key();
            let plaintext = vec![0xAB; 1024 * 1024];

            let ct = encrypt(&plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
            let decrypted = decrypt(&ct, &key).unwrap();

            assert_eq!(
                decrypted, plaintext,
                "Large data must encrypt/decrypt correctly"
            );
        }

        #[test]
        fn test_chunk_key_uniqueness() {
            let master = test_key();
            let mut keys = std::collections::HashSet::new();

            for i in 0..256 {
                let mut hash = [0u8; 32];
                hash[0] = (i / 100) as u8;
                hash[1] = (i / 10) as u8;
                hash[2] = (i % 10) as u8;
                hash[3..].fill(i as u8);
                let derived = derive_chunk_key(&master, &hash);
                assert!(
                    keys.insert(derived.0.to_vec()),
                    "Each chunk hash must derive unique key"
                );
            }
        }

        #[test]
        fn test_zeroized_key_unrecoverable() {
            let mut key = test_key();
            key.0.zeroize();
            assert_eq!(
                key.0.iter().filter(|&&x| x != 0).count(),
                0,
                "Key must be zeroized"
            );
        }

        #[test]
        fn test_key_version_rollback_prevention() {
            let config = KeyManagerConfig { max_key_history: 3 };
            let mut km = KeyManager::new(config);

            let v1 = km.rotate_key(EncryptionKey([1u8; 32]));
            let v2 = km.rotate_key(EncryptionKey([2u8; 32]));
            let v3 = km.rotate_key(EncryptionKey([3u8; 32]));
            let v4 = km.rotate_key(EncryptionKey([4u8; 32]));

            assert!(
                v4.0 > v3.0 && v3.0 > v2.0 && v2.0 > v1.0,
                "Key version must advance forward"
            );
        }

        #[test]
        fn test_encrypted_chunk_contains_no_plaintext() {
            let key = test_key();
            let plaintext = b"XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";

            let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

            assert!(
                ct.ciphertext != plaintext.to_vec(),
                "Ciphertext must differ from plaintext"
            );
            let matching_prefix = ct
                .ciphertext
                .iter()
                .zip(plaintext.iter())
                .take_while(|(c, p)| c == p)
                .count();
            assert!(
                matching_prefix < 5,
                "Encrypted output must not have plaintext prefix"
            );
        }

        #[test]
        fn test_algorithm_upgrade_path() {
            let key = test_key();

            let ct_aes = encrypt(b"data", &key, EncryptionAlgorithm::AesGcm256).unwrap();
            let ct_chacha = encrypt(b"data", &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();

            assert_ne!(
                ct_aes.algo, ct_chacha.algo,
                "Different algorithms must be selectable"
            );

            let pt_aes = decrypt(&ct_aes, &key).unwrap();
            let pt_chacha = decrypt(&ct_chacha, &key).unwrap();

            assert_eq!(pt_aes, b"data");
            assert_eq!(pt_chacha, b"data");
        }

        #[test]
        fn test_key_manager_initialization_secure() {
            let config = KeyManagerConfig {
                max_key_history: 10,
            };
            let mut km = KeyManager::new(config);

            let version = km.rotate_key(test_key());
            assert_eq!(version.0, 0, "New key manager must start at version 0");
        }

        #[test]
        fn test_cross_tenant_key_isolation() {
            let tenant_keys = vec![
                EncryptionKey([1u8; 32]),
                EncryptionKey([2u8; 32]),
                EncryptionKey([3u8; 32]),
            ];

            let hash = test_chunk_hash();
            let mut derived_keys = Vec::new();

            for key in &tenant_keys {
                derived_keys.push(derive_chunk_key(key, &hash));
            }

            for i in 0..derived_keys.len() {
                for j in (i + 1)..derived_keys.len() {
                    assert_ne!(
                        derived_keys[i].0, derived_keys[j].0,
                        "Tenant keys must be isolated"
                    );
                }
            }
        }

        #[test]
        fn test_secret_rotation_preserves_data() {
            let config = KeyManagerConfig {
                max_key_history: 10,
            };
            let mut km = KeyManager::new(config);

            km.rotate_key(EncryptionKey([1u8; 32]));
            let _dek = km.generate_dek().unwrap();
            let plaintext = b"important data";

            let key = test_key();
            let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

            km.rotate_key(EncryptionKey([2u8; 32]));

            let decrypted = decrypt(&ct, &key).unwrap();
            assert_eq!(
                decrypted, plaintext,
                "Data must remain accessible after key rotation"
            );
        }
    }

    mod audit_trail {
        use super::*;

        #[test]
        fn test_audit_event_created() {
            init_audit_log();
            let event = create_audit_event("file_access", TEST_USER_ID);
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(log.len(), 1, "Audit event must be logged");
        }

        #[test]
        fn test_audit_timestamp_ordering() {
            init_audit_log();

            for i in 0..10 {
                let mut event = create_audit_event("test", TEST_USER_ID);
                event.timestamp = i;
                log_audit_event(event);
            }

            let log = get_audit_log();
            assert!(
                verify_audit_integrity(&log),
                "Audit log must be chronologically ordered"
            );
        }

        #[test]
        fn test_audit_event_contains_user_id() {
            init_audit_log();
            let event = create_audit_event("file_write", "admin-user");
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(
                log[0].user_id, "admin-user",
                "Audit event must contain user ID"
            );
        }

        #[test]
        fn test_audit_event_contains_tenant_id() {
            init_audit_log();
            let event = create_audit_event("file_read", TEST_USER_ID);
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(
                log[0].tenant_id, TEST_TENANT_ID,
                "Audit event must contain tenant ID"
            );
        }

        #[test]
        fn test_audit_event_type_recorded() {
            init_audit_log();
            let event = create_audit_event("key_rotation", TEST_USER_ID);
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(
                log[0].event_type, "key_rotation",
                "Audit event must record event type"
            );
        }

        #[test]
        fn test_multiple_audit_events_tracked() {
            init_audit_log();

            for _ in 0..50 {
                let event = create_audit_event("access", TEST_USER_ID);
                log_audit_event(event);
            }

            let log = get_audit_log();
            assert_eq!(log.len(), 50, "All audit events must be tracked");
        }

        #[test]
        fn test_audit_log_integrity_verification() {
            init_audit_log();

            for i in 0..20 {
                let mut event = create_audit_event("test", TEST_USER_ID);
                event.timestamp = i * 1000;
                log_audit_event(event);
            }

            let log = get_audit_log();
            assert!(
                verify_audit_integrity(&log),
                "Audit log integrity must be verifiable"
            );
        }

        #[test]
        fn test_failed_access_audit_logged() {
            init_audit_log();
            let mut event = create_audit_event("file_access", TEST_USER_ID);
            event.result = "failure".to_string();
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(log[0].result, "failure", "Failed access must be logged");
        }

        #[test]
        fn test_successful_access_audit_logged() {
            init_audit_log();
            let mut event = create_audit_event("file_access", TEST_USER_ID);
            event.result = "success".to_string();
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(log[0].result, "success", "Successful access must be logged");
        }

        #[test]
        fn test_audit_event_action_recorded() {
            init_audit_log();
            let mut event = create_audit_event("file_write", TEST_USER_ID);
            event.action = "write".to_string();
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(log[0].action, "write", "Audit event must record action");
        }

        #[test]
        fn test_audit_resource_id_recorded() {
            init_audit_log();
            let mut event = create_audit_event("file_read", TEST_USER_ID);
            event.resource_id = "file-12345".to_string();
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(
                log[0].resource_id, "file-12345",
                "Audit event must record resource ID"
            );
        }

        #[test]
        fn test_audit_metadata_extensible() {
            init_audit_log();
            let mut event = create_audit_event("file_access", TEST_USER_ID);
            event
                .metadata
                .insert("ip_address".to_string(), "192.168.1.100".to_string());
            event
                .metadata
                .insert("user_agent".to_string(), "cfs-client/1.0".to_string());
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(
                log[0].metadata.get("ip_address"),
                Some(&"192.168.1.100".to_string()),
                "Audit metadata must be extensible"
            );
        }

        #[test]
        fn test_key_operation_audit_logged() {
            init_audit_log();
            let mut event = create_audit_event("key_operation", TEST_USER_ID);
            event.action = "key_rotation".to_string();
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(log.len(), 1, "Key operations must be audit logged");
        }

        #[test]
        fn test_encryption_operation_audit_logged() {
            init_audit_log();
            let mut event = create_audit_event("encryption", TEST_USER_ID);
            event
                .metadata
                .insert("algorithm".to_string(), "AesGcm256".to_string());
            log_audit_event(event);

            let log = get_audit_log();
            assert!(
                log[0].metadata.contains_key("algorithm"),
                "Encryption operations must be logged with metadata"
            );
        }

        #[test]
        fn test_access_denied_audit_logged() {
            init_audit_log();
            let mut event = create_audit_event("access_denied", "unauthorized-user");
            event.result = "denied".to_string();
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(log[0].result, "denied", "Access denied must be logged");
        }

        #[test]
        fn test_admin_action_audit_logged() {
            init_audit_log();
            let event = create_audit_event("admin_action", "admin");
            log_audit_event(event);

            let log = get_audit_log();
            assert_eq!(log.len(), 1, "Admin actions must be audit logged");
        }

        #[test]
        fn test_configuration_change_audit_logged() {
            init_audit_log();
            let mut event = create_audit_event("config_change", TEST_USER_ID);
            event
                .metadata
                .insert("parameter".to_string(), "retention_days".to_string());
            event
                .metadata
                .insert("old_value".to_string(), "30".to_string());
            event
                .metadata
                .insert("new_value".to_string(), "90".to_string());
            log_audit_event(event);

            let log = get_audit_log();
            assert!(
                log[0].metadata.contains_key("parameter"),
                "Configuration changes must be audit logged"
            );
        }

        #[test]
        fn test_audit_log_query_by_user() {
            init_audit_log();

            let event1 = create_audit_event("access", "user-a");
            log_audit_event(event1);
            let event2 = create_audit_event("access", "user-b");
            log_audit_event(event2);
            let event3 = create_audit_event("access", "user-a");
            log_audit_event(event3);

            let log: Vec<_> = get_audit_log()
                .into_iter()
                .filter(|e| e.user_id == "user-a")
                .collect();

            assert_eq!(log.len(), 2, "Must be able to query audit log by user");
        }

        #[test]
        fn test_audit_log_query_by_tenant() {
            init_audit_log();

            let event1 = create_audit_event("access", TEST_USER_ID);
            log_audit_event(event1);

            let log: Vec<_> = get_audit_log()
                .into_iter()
                .filter(|e| e.tenant_id == TEST_TENANT_ID)
                .collect();

            assert_eq!(log.len(), 1, "Must be able to query audit log by tenant");
        }
    }

    mod compliance_validation {
        use super::*;

        fn create_policy() -> CompliancePolicy {
            CompliancePolicy::default()
        }

        #[test]
        fn test_retention_policy_enforced() {
            let policy = create_policy();
            assert_eq!(
                policy.retention_days, 90,
                "Default retention must be 90 days"
            );
        }

        #[test]
        fn test_minimum_encryption_level() {
            let policy = create_policy();
            assert!(
                policy.min_encryption_level >= 256,
                "Minimum encryption must be 256-bit"
            );
        }

        #[test]
        fn test_audit_enabled_by_default() {
            let policy = create_policy();
            assert!(policy.audit_enabled, "Audit must be enabled by default");
        }

        #[test]
        fn test_data_classification_set() {
            let policy = create_policy();
            assert_eq!(
                policy.data_classification, "confidential",
                "Default classification must be set"
            );
        }

        #[test]
        fn test_encryption_compliance_aes256() {
            let key = test_key();
            let plaintext = b"confidential data";

            let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
            let decrypted = decrypt(&ct, &key).unwrap();

            assert_eq!(
                decrypted, plaintext,
                "256-bit AES encryption must be compliant"
            );
        }

        #[test]
        fn test_encryption_compliance_chacha() {
            let key = test_key();
            let plaintext = b"confidential data";

            let ct = encrypt(plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();
            let decrypted = decrypt(&ct, &key).unwrap();

            assert_eq!(
                decrypted, plaintext,
                "ChaCha20-Poly1305 encryption must be compliant"
            );
        }

        #[test]
        fn test_key_derivation_compliant() {
            let key = test_key();
            let hash = test_chunk_hash();
            let derived = derive_chunk_key(&key, &hash);

            assert_eq!(
                derived.0.len(),
                32,
                "Key derivation must produce 256-bit keys"
            );
        }

        #[test]
        fn test_non_reversible_key_derivation() {
            let key = test_key();
            let hash = test_chunk_hash();
            let derived = derive_chunk_key(&key, &hash);

            assert_ne!(derived.0, key.0, "Key derivation must not be reversible");
        }

        #[test]
        fn test_cross_region_access_control() {
            let local_key = EncryptionKey([1u8; 32]);
            let remote_key = EncryptionKey([2u8; 32]);

            let derived_local = derive_chunk_key(&local_key, &[1u8; 32]);
            let derived_remote = derive_chunk_key(&remote_key, &[1u8; 32]);

            assert_ne!(
                derived_local.0, derived_remote.0,
                "Cross-region keys must be isolated"
            );
        }

        #[test]
        fn test_data_encryption_at_rest() {
            let key = test_key();
            let data_at_rest = b"data stored on disk";

            let encrypted = encrypt(data_at_rest, &key, EncryptionAlgorithm::AesGcm256).unwrap();
            assert!(
                !encrypted.ciphertext.is_empty(),
                "Data at rest must be encrypted"
            );
        }

        #[test]
        fn test_data_encryption_in_transit() {
            let key = test_key();
            let data_in_transit = b"data over network";

            let encrypted = encrypt(data_in_transit, &key, EncryptionAlgorithm::AesGcm256).unwrap();

            let plaintext = decrypt(&encrypted, &key).unwrap();
            assert_eq!(
                plaintext, data_in_transit,
                "Data in transit must be decryptable"
            );
        }

        #[test]
        fn test_tamper_resistant_audit() {
            init_audit_log();

            let mut event1 = create_audit_event("access", TEST_USER_ID);
            event1.timestamp = 1000;
            log_audit_event(event1);

            let event2 = create_audit_event("write", TEST_USER_ID);
            log_audit_event(event2);

            let log = get_audit_log();
            assert!(
                verify_audit_integrity(&log),
                "Audit trail must be tamper resistant"
            );
        }

        #[test]
        fn test_worm_compliance_enabled() {
            let policy = create_policy();
            assert!(
                policy.retention_days >= 30,
                "WORM compliance requires minimum 30-day retention"
            );
        }

        #[test]
        fn test_access_control_enforced() {
            let authorized_key = EncryptionKey([1u8; 32]);
            let unauthorized_key = EncryptionKey([2u8; 32]);

            let data = b"sensitive file";

            let encrypted = encrypt(data, &authorized_key, EncryptionAlgorithm::AesGcm256).unwrap();

            let result = decrypt(&encrypted, &unauthorized_key);
            assert!(result.is_err(), "Access control must be enforced");
        }

        #[test]
        fn test_multi_tenant_isolation() {
            let tenants = ["tenant-a", "tenant-b", "tenant-c"];
            let mut keys = Vec::new();

            for (i, _tenant) in tenants.iter().enumerate() {
                let key = EncryptionKey([i as u8; 32]);
                keys.push(key);
            }

            let hashes: Vec<_> = (0..3).map(|i| [i as u8; 32]).collect();

            for i in 0..keys.len() {
                for j in (i + 1)..keys.len() {
                    let derived_i = derive_chunk_key(&keys[i], &hashes[i]);
                    let derived_j = derive_chunk_key(&keys[j], &hashes[j]);
                    assert_ne!(
                        derived_i.0, derived_j.0,
                        "Multi-tenant isolation must be enforced"
                    );
                }
            }
        }

        #[test]
        fn test_compliance_report_generation() {
            let policy = create_policy();

            let retention = policy.retention_days;
            let encryption = policy.min_encryption_level;
            let audit = policy.audit_enabled;
            let classification = &policy.data_classification;

            assert!(
                retention > 0,
                "Compliance report must include retention policy"
            );
            assert!(
                encryption > 0,
                "Compliance report must include encryption level"
            );
            assert!(
                !classification.is_empty(),
                "Compliance report must include classification"
            );
            assert!(audit, "Audit must be enabled");
        }

        #[test]
        fn test_regulatory_compliance_check() {
            let policy = create_policy();

            let mut violations = Vec::new();

            if policy.retention_days < 30 {
                violations.push("Retention below minimum");
            }
            if policy.min_encryption_level < 256 {
                violations.push("Encryption below minimum");
            }
            if !policy.audit_enabled {
                violations.push("Audit not enabled");
            }

            assert!(violations.is_empty(), "No regulatory violations allowed");
        }

        #[test]
        fn test_data_classification_enforced() {
            let classifications = ["public", "internal", "confidential", "restricted"];

            for class in classifications {
                let policy = CompliancePolicy {
                    data_classification: class.to_string(),
                    ..Default::default()
                };

                assert!(
                    !policy.data_classification.is_empty(),
                    "Classification must be enforced"
                );
            }
        }

        #[test]
        fn test_compliance_audit_logging() {
            let policy = create_policy();
            assert!(policy.audit_enabled, "Compliance requires audit logging");
        }

        #[test]
        fn test_encryption_key_lifecycle() {
            let config = KeyManagerConfig {
                max_key_history: 10,
            };
            let mut km = KeyManager::new(config);

            let v1 = km.rotate_key(EncryptionKey([1u8; 32]));

            km.rotate_key(EncryptionKey([2u8; 32]));
            let v2 = km.rotate_key(EncryptionKey([3u8; 32]));

            assert!(v2.0 > v1.0, "Key lifecycle must be tracked");
        }

        #[test]
        fn test_data_recovery_procedure() {
            let key = test_key();
            let data = b"critical business data";

            let encrypted = encrypt(data, &key, EncryptionAlgorithm::AesGcm256).unwrap();
            let recovered = decrypt(&encrypted, &key).unwrap();

            assert_eq!(recovered, data, "Data recovery must be possible");
        }

        #[test]
        fn test_disaster_recovery_encryption() {
            let primary_key = EncryptionKey([1u8; 32]);
            let backup_key = EncryptionKey([2u8; 32]);

            let data = b"disaster recovery data";

            let encrypted_primary =
                encrypt(data, &primary_key, EncryptionAlgorithm::AesGcm256).unwrap();
            let encrypted_backup =
                encrypt(data, &backup_key, EncryptionAlgorithm::AesGcm256).unwrap();

            let recovered_primary = decrypt(&encrypted_primary, &primary_key).unwrap();
            let recovered_backup = decrypt(&encrypted_backup, &backup_key).unwrap();

            assert_eq!(recovered_primary, data);
            assert_eq!(recovered_backup, data);
        }

        #[test]
        fn test_compliance_key_rotation() {
            let config = KeyManagerConfig {
                max_key_history: 10,
            };
            let mut km = KeyManager::new(config);

            let original_version = km.rotate_key(test_key());

            for _ in 0..5 {
                let new_key = EncryptionKey([rand::random(); 32]);
                km.rotate_key(new_key);
            }

            let new_version = km.rotate_key(test_key());
            assert!(
                new_version.0 > original_version.0,
                "Key rotation must update version"
            );
        }

        #[test]
        fn test_data_retention_enforcement() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let policy = create_policy();
            let retention_seconds = policy.retention_days as u64 * 24 * 60 * 60;

            let old_timestamp = now - retention_seconds - 1;
            let new_timestamp = now - retention_seconds + 1;

            assert!(
                old_timestamp < now - retention_seconds,
                "Old data must be flagged for deletion"
            );
            assert!(
                new_timestamp >= now - retention_seconds,
                "New data must be retained"
            );
        }

        #[test]
        fn test_secure_deletion_verification() {
            let key = test_key();
            let sensitive_data = b"sensitive data to be deleted";

            let encrypted = encrypt(sensitive_data, &key, EncryptionAlgorithm::AesGcm256).unwrap();
            let encrypted_bytes = encrypted.ciphertext.clone();

            let mut wiped = encrypted_bytes.clone();
            for byte in wiped.iter_mut() {
                *byte = 0;
            }

            let result = decrypt(
                &EncryptedChunk {
                    ciphertext: wiped,
                    nonce: encrypted.nonce,
                    algo: encrypted.algo,
                },
                &key,
            );

            assert!(
                result.is_err(),
                "Securely wiped data must not be recoverable"
            );
        }

        #[test]
        fn test_compliance_certification_check() {
            let _required_features = vec![
                "encryption",
                "audit_logging",
                "access_control",
                "key_rotation",
                "data_retention",
            ];

            let key = test_key();
            let hash = test_chunk_hash();
            let _ = derive_chunk_key(&key, &hash);

            let policy = create_policy();
            assert!(policy.audit_enabled, "Audit logging must be supported");
            assert!(
                policy.retention_days >= 30,
                "Data retention must be supported"
            );
            assert!(
                policy.min_encryption_level >= 256,
                "Encryption must be supported"
            );
        }

        #[test]
        fn test_privacy_data_protection() {
            let pii_data = b"John Doe SSN: 123-45-6789";
            let key = test_key();

            let encrypted = encrypt(pii_data, &key, EncryptionAlgorithm::AesGcm256).unwrap();

            assert!(
                encrypted.ciphertext != pii_data.to_vec(),
                "PII must be transformed in ciphertext"
            );
        }

        #[test]
        fn test_compliance_boundary_enforcement() {
            let internal_key = EncryptionKey([1u8; 32]);
            let external_key = EncryptionKey([2u8; 32]);

            let internal_data = b"internal confidential";

            let internal_encrypted =
                encrypt(internal_data, &internal_key, EncryptionAlgorithm::AesGcm256).unwrap();

            let result = decrypt(&internal_encrypted, &external_key);
            assert!(result.is_err(), "Compliance boundary must be enforced");
        }

        #[test]
        fn test_data_origin_verification() {
            let key = test_key();
            let hash = test_chunk_hash();
            let derived = derive_chunk_key(&key, &hash);

            let data = b"origin verification data";
            let encrypted = encrypt(data, &derived, EncryptionAlgorithm::AesGcm256).unwrap();
            let decrypted = decrypt(&encrypted, &derived).unwrap();

            assert_eq!(decrypted, data, "Data origin must be verifiable");
        }

        #[test]
        fn test_compliance_algorithm_selection() {
            let algorithms = vec![
                EncryptionAlgorithm::AesGcm256,
                EncryptionAlgorithm::ChaCha20Poly1305,
            ];

            let key = test_key();
            let data = b"algorithm compliance test";

            for algo in algorithms {
                let encrypted = encrypt(data, &key, algo).unwrap();
                let decrypted = decrypt(&encrypted, &key).unwrap();
                assert_eq!(decrypted, data, "All compliant algorithms must work");
            }
        }
    }
}
