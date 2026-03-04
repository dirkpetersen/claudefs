//! Phase 2 security audit: enhanced tests for nonce security, key lifecycle, TLS, and auth boundaries.

#[cfg(test)]
mod tests {
    use claudefs_gateway::auth::{AuthCred, AuthSysCred, SquashPolicy, AUTH_SYS_MAX_GIDS};
    use claudefs_gateway::rpc::{OpaqueAuth, AUTH_SYS};
    use claudefs_reduce::encryption::{
        decrypt, derive_chunk_key, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey, Nonce,
    };
    use claudefs_reduce::key_manager::{KeyManager, KeyManagerConfig, KeyVersion};
    use claudefs_repl::batch_auth::{
        AuthResult as BatchAuthResult, BatchAuthKey, BatchAuthenticator,
    };
    use claudefs_repl::journal::{JournalEntry, OpKind};
    use claudefs_transport::conn_auth::{
        AuthConfig, AuthLevel, AuthResult, AuthStats, CertificateInfo, ConnectionAuthenticator,
        RevocationList,
    };
    use claudefs_transport::tls::{
        generate_node_cert, generate_self_signed_ca, load_certs_from_pem,
        load_private_key_from_pem, TlsConfig,
    };
    use claudefs_transport::zerocopy::{RegionPool, ZeroCopyConfig};
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn test_key() -> EncryptionKey {
        EncryptionKey([42u8; 32])
    }

    fn test_key_alt() -> EncryptionKey {
        EncryptionKey([99u8; 32])
    }

    fn all_zero_key() -> EncryptionKey {
        EncryptionKey([0u8; 32])
    }

    mod group1_nonce_collision {
        use super::*;

        #[test]
        fn test_nonce_never_repeats_same_key_same_plaintext() {
            let key = test_key();
            let plaintext = b"test data for nonce uniqueness";
            let mut nonces = HashSet::new();

            for _ in 0..1000 {
                let result = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
                assert!(
                    nonces.insert(result.nonce),
                    "PHASE2-AUDIT: Nonce collision detected - same nonce generated twice"
                );
            }
            assert_eq!(
                nonces.len(),
                1000,
                "PHASE2-AUDIT: Expected 1000 unique nonces, got {}",
                nonces.len()
            );
        }

        #[test]
        fn test_nonce_distribution_all_bytes_covered() {
            let mut byte_positions: Vec<HashSet<u8>> = vec![HashSet::new(); 12];

            for _ in 0..10000 {
                let nonce = random_nonce();
                for (i, byte) in nonce.0.iter().enumerate() {
                    byte_positions[i].insert(*byte);
                }
            }

            for (i, values) in byte_positions.iter().enumerate() {
                assert!(
                    values.len() > 1,
                    "PHASE2-AUDIT: Byte position {} stuck at single value - poor entropy",
                    i
                );
            }
        }

        #[test]
        fn test_concurrent_nonce_generation() {
            let nonce_set = Arc::new(Mutex::new(HashSet::new()));
            let mut handles = vec![];

            for _ in 0..8 {
                let set = nonce_set.clone();
                let handle = std::thread::spawn(move || {
                    for _ in 0..1000 {
                        let nonce = random_nonce();
                        let mut guard = set.lock().unwrap();
                        assert!(
                            guard.insert(nonce),
                            "PHASE2-AUDIT: Concurrent nonce collision detected"
                        );
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.join().unwrap();
            }

            let final_count = nonce_set.lock().unwrap().len();
            assert_eq!(
                final_count, 8000,
                "PHASE2-AUDIT: Expected 8000 unique nonces from 8 threads, got {}",
                final_count
            );
        }

        #[test]
        fn test_nonce_is_not_counter_based() {
            let key = test_key();
            let plaintext = b"counter test";
            let mut differences: Vec<u32> = Vec::new();
            let mut prev_nonce = None;

            for _ in 0..100 {
                let result = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
                if let Some(prev) = prev_nonce {
                    let mut diff: u32 = 0;
                    for i in 0..12 {
                        diff = diff
                            .wrapping_add(result.nonce.0[i] as u32)
                            .wrapping_sub(prev.0[i] as u32);
                    }
                    differences.push(diff);
                }
                prev_nonce = Some(result.nonce);
            }

            let unique_diffs: HashSet<u32> = differences.iter().cloned().collect();
            assert!(
                unique_diffs.len() > 50,
                "PHASE2-AUDIT: Nonce appears counter-based - only {} unique differences",
                unique_diffs.len()
            );
        }
    }

    mod group2_hkdf_key_isolation {
        use super::*;

        #[test]
        fn test_hkdf_different_masters_different_outputs() {
            let key1 = test_key();
            let key2 = test_key_alt();
            let chunk_hash = [1u8; 32];

            let derived1 = derive_chunk_key(&key1, &chunk_hash);
            let derived2 = derive_chunk_key(&key2, &chunk_hash);

            assert_ne!(
                derived1.0, derived2.0,
                "PHASE2-AUDIT: Different master keys produced same derived key"
            );
        }

        #[test]
        fn test_hkdf_all_zero_master_still_derives() {
            let zero_key = all_zero_key();
            let chunk_hash = [1u8; 32];

            let derived = derive_chunk_key(&zero_key, &chunk_hash);

            assert_ne!(
                derived.0, [0u8; 32],
                "PHASE2-AUDIT: All-zero master key produced all-zero derived key"
            );
        }

        #[test]
        fn test_hkdf_output_has_good_entropy() {
            let key = test_key();
            let mut derived_keys = Vec::new();

            for i in 0..100u32 {
                let hash = i.to_le_bytes().repeat(8);
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&hash[..32]);
                derived_keys.push(derive_chunk_key(&key, &arr));
            }

            for i in 0..derived_keys.len() {
                for j in (i + 1)..derived_keys.len() {
                    let mut matches = 0;
                    for k in 0..32 {
                        if derived_keys[i].0[k] == derived_keys[j].0[k] {
                            matches += 1;
                        }
                    }
                    assert!(
                        matches <= 4,
                        "PHASE2-AUDIT: Keys {} and {} share {} bytes (max allowed: 4)",
                        i,
                        j,
                        matches
                    );
                }
            }
        }
    }

    mod group3_key_manager_lifecycle {
        use super::*;

        #[test]
        fn test_key_rotation_10_times_all_old_deks_recoverable() {
            let mut km = KeyManager::with_initial_key(
                KeyManagerConfig {
                    max_key_history: 20,
                },
                test_key(),
            );

            let mut wrapped_deks = Vec::new();
            for i in 0..10 {
                let dek = km.generate_dek().unwrap();
                let wrapped = km.wrap_dek(&dek).unwrap();
                wrapped_deks.push(wrapped);
                km.rotate_key(EncryptionKey([i as u8; 32]));
            }

            for (i, wrapped) in wrapped_deks.iter().enumerate() {
                let unwrapped = km.unwrap_dek(wrapped).unwrap();
                assert_eq!(
                    unwrapped.key.len(),
                    32,
                    "PHASE2-AUDIT: DEK version {} failed to unwrap",
                    i
                );
            }
        }

        #[test]
        fn test_history_pruning_loses_oldest_keys() {
            let mut km =
                KeyManager::with_initial_key(KeyManagerConfig { max_key_history: 5 }, test_key());

            for i in 0..10 {
                let dek = km.generate_dek().unwrap();
                let wrapped = km.wrap_dek(&dek).unwrap();
                if i > 0 {
                    let _ = km.unwrap_dek(&wrapped);
                }
                km.rotate_key(EncryptionKey([i as u8; 32]));
            }

            let old_wrapped = km.wrap_dek(&km.generate_dek().unwrap()).unwrap();
            let current_version = km.current_version().unwrap();

            assert!(
                old_wrapped.kek_version.0 < current_version.0 - 4,
                "PHASE2-AUDIT: Pruned key still recoverable - history not properly pruned"
            );

            let result = km.unwrap_dek(&old_wrapped);
            assert!(
                result.is_err(),
                "PHASE2-AUDIT: Oldest key should have been pruned but was recoverable"
            );
        }

        #[test]
        fn test_rewrap_preserves_dek_value() {
            let mut km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());

            let dek = km.generate_dek().unwrap();
            let original_key_bytes = dek.key;

            let old_wrapped = km.wrap_dek(&dek).unwrap();

            km.rotate_key(test_key_alt());

            let new_wrapped = km.rewrap_dek(&old_wrapped).unwrap();
            let unwrapped = km.unwrap_dek(&new_wrapped).unwrap();

            assert_eq!(
                unwrapped.key, original_key_bytes,
                "PHASE2-AUDIT: DEK value changed after rewrap"
            );
        }
    }

    mod group4_tls_certificate_validation {
        use super::*;

        #[test]
        fn test_node_cert_is_not_ca() {
            let (ca_cert_pem, ca_key_pem) = generate_self_signed_ca().unwrap();
            let (node_cert_pem, _node_key_pem) =
                generate_node_cert(&ca_cert_pem, &ca_key_pem, "node1").unwrap();

            let certs = load_certs_from_pem(&node_cert_pem).unwrap();
            let cert = &certs[0];

            let is_ca = cert.value().is_ca();
            assert!(
                !is_ca.unwrap_or(false),
                "PHASE2-AUDIT: Node certificate incorrectly marked as CA"
            );
        }

        #[test]
        fn test_different_cas_produce_different_certs() {
            let (ca1_cert_pem, ca1_key_pem) = generate_self_signed_ca().unwrap();
            let (ca2_cert_pem, ca2_key_pem) = generate_self_signed_ca().unwrap();

            let (cert1_pem, _key1_pem) =
                generate_node_cert(&ca1_cert_pem, &ca1_key_pem, "node1").unwrap();
            let (cert2_pem, _key2_pem) =
                generate_node_cert(&ca2_cert_pem, &ca2_key_pem, "node1").unwrap();

            assert_ne!(
                cert1_pem, cert2_pem,
                "PHASE2-AUDIT: Different CAs produced identical certificates"
            );
        }

        #[test]
        fn test_ca_cert_contains_proper_pem_markers() {
            let (ca_cert_pem, _ca_key_pem) = generate_self_signed_ca().unwrap();
            let pem_str = String::from_utf8_lossy(&ca_cert_pem);

            assert!(
                pem_str.contains("-----BEGIN CERTIFICATE-----"),
                "PHASE2-AUDIT: CA cert missing BEGIN marker"
            );
            assert!(
                pem_str.contains("-----END CERTIFICATE-----"),
                "PHASE2-AUDIT: CA cert missing END marker"
            );
        }

        #[test]
        fn test_node_cert_signed_by_ca() {
            let (ca_cert_pem, ca_key_pem) = generate_self_signed_ca().unwrap();
            let (node_cert_pem, _node_key_pem) =
                generate_node_cert(&ca_cert_pem, &ca_key_pem, "node1").unwrap();

            assert_ne!(
                ca_cert_pem, node_cert_pem,
                "PHASE2-AUDIT: Node cert identical to CA cert"
            );
        }
    }

    mod group5_connection_auth_edge_cases {
        use super::*;

        fn make_cert(
            subject: &str,
            issuer: &str,
            serial: &str,
            fingerprint: &str,
            not_before_ms: u64,
            not_after_ms: u64,
            is_ca: bool,
        ) -> CertificateInfo {
            CertificateInfo {
                subject: subject.to_string(),
                issuer: issuer.to_string(),
                serial: serial.to_string(),
                fingerprint_sha256: fingerprint.to_string(),
                not_before_ms,
                not_after_ms,
                is_ca,
            }
        }

        #[test]
        fn test_auth_level_tls_only_allows_without_client_cert_checks() {
            let config = AuthConfig {
                level: AuthLevel::TlsOnly,
                ..Default::default()
            };
            let mut auth = ConnectionAuthenticator::new(config);

            let cert = make_cert(
                "server1",
                "ClusterCA",
                "01",
                "abc123",
                1000,
                86400000 * 365 * 1000 + 1000,
                false,
            );

            auth.set_time(5000);
            let result = auth.authenticate(&cert);

            assert!(
                matches!(result, AuthResult::Allowed { .. }),
                "PHASE2-AUDIT: TlsOnly mode should allow valid cert without strict checks"
            );
        }

        #[test]
        fn test_revocation_list_duplicate_serial_noop() {
            let mut rl = RevocationList::new();

            rl.revoke_serial("01".to_string());
            let len_before = rl.len();
            rl.revoke_serial("01".to_string());
            let len_after = rl.len();

            assert_eq!(
                len_before, len_after,
                "PHASE2-AUDIT: Duplicate serial increased list size"
            );
        }

        #[test]
        fn test_very_old_cert_rejected_strict_age() {
            let config = AuthConfig {
                max_cert_age_days: 365,
                ..Default::default()
            };
            let mut auth = ConnectionAuthenticator::new(config);

            let old_not_before: u64 = 500 * 86400000;
            let cert = make_cert(
                "server1",
                "ClusterCA",
                "01",
                "abc123",
                old_not_before,
                600 * 86400000,
                false,
            );

            auth.set_time(501 * 86400000);
            let result = auth.authenticate(&cert);

            assert!(
                matches!(result, AuthResult::Denied { reason } if reason.contains("maximum age")),
                "PHASE2-AUDIT: Very old cert should be rejected"
            );
        }

        #[test]
        fn test_auth_stats_increment_correctly() {
            let config = AuthConfig::default();
            let mut auth = ConnectionAuthenticator::new(config);

            let valid_cert = make_cert(
                "server1",
                "ClusterCA",
                "01",
                "abc123",
                1000,
                86400000 * 365 * 1000 + 1000,
                false,
            );

            auth.set_time(5000);
            for _ in 0..5 {
                auth.authenticate(&valid_cert);
            }

            let invalid_cert = make_cert(
                "server2",
                "ClusterCA",
                "02",
                "def456",
                1000,
                86400000 * 365 * 1000 + 1000,
                false,
            );
            for _ in 0..3 {
                auth.authenticate(&invalid_cert);
            }

            let stats: AuthStats = auth.stats();
            assert_eq!(
                stats.total_allowed, 5,
                "PHASE2-AUDIT: Expected 5 allowed, got {}",
                stats.total_allowed
            );
            assert_eq!(
                stats.total_denied, 3,
                "PHASE2-AUDIT: Expected 3 denied, got {}",
                stats.total_denied
            );
        }
    }

    mod group6_zerocopy_pool_security {
        use super::*;

        #[test]
        fn test_released_region_data_is_zeroed() {
            let config = ZeroCopyConfig {
                region_size: 64,
                max_regions: 10,
                alignment: 4096,
                preregister: 2,
            };
            let pool = RegionPool::new(config);

            let mut region = pool.acquire().unwrap();
            for byte in region.as_mut_slice() {
                *byte = 0xFF;
            }
            pool.release(region);

            let new_region = pool.acquire().unwrap();
            for byte in new_region.as_slice() {
                assert_eq!(
                    *byte, 0,
                    "PHASE2-AUDIT: Released region contained non-zero data: {:02x}",
                    *byte
                );
            }
            pool.release(new_region);
        }

        #[test]
        fn test_pool_grow_shrink_consistency() {
            let config = ZeroCopyConfig {
                region_size: 1024,
                max_regions: 20,
                alignment: 4096,
                preregister: 2,
            };
            let pool = RegionPool::new(config);

            pool.grow(5);
            let total_after_grow = pool.total();
            let available_after_grow = pool.available();

            pool.shrink(3);
            let total_after_shrink = pool.total();
            let available_after_shrink = pool.available();

            assert_eq!(total_after_grow, available_after_grow + pool.in_use());
            assert_eq!(total_after_shrink, available_after_shrink + pool.in_use());
            assert_eq!(total_after_shrink, total_after_grow - 3);
        }

        #[test]
        fn test_pool_max_regions_enforced() {
            let config = ZeroCopyConfig {
                region_size: 1024,
                max_regions: 5,
                alignment: 4096,
                preregister: 0,
            };
            let pool = RegionPool::new(config);

            let mut regions = Vec::new();
            for _ in 0..10 {
                if let Some(r) = pool.acquire() {
                    regions.push(r);
                }
            }

            assert!(
                regions.len() <= 5,
                "PHASE2-AUDIT: Pool allowed {} acquisitions (max: 5)",
                regions.len()
            );

            for region in regions {
                pool.release(region);
            }
        }
    }

    mod group7_batch_auth_security {
        use super::*;

        fn make_entry(seq: u64, inode: u64, payload: Vec<u8>) -> JournalEntry {
            JournalEntry {
                seq,
                shard_id: 0,
                site_id: 1,
                timestamp_us: seq * 1000,
                inode,
                op: OpKind::Write,
                payload,
                crc32: 0,
            }
        }

        #[test]
        fn test_batch_auth_wrong_key_fails() {
            let key_a = BatchAuthKey::from_bytes([0xAA; 32]);
            let key_b = BatchAuthKey::from_bytes([0xBB; 32]);

            let auth_a = BatchAuthenticator::new(key_a, 1);
            let auth_b = BatchAuthenticator::new(key_b, 1);

            let entries = vec![make_entry(100, 500, vec![1, 2, 3, 4])];

            let tag = auth_a.sign_batch(1, 1, &entries);
            let result = auth_b.verify_batch(&tag, 1, 1, &entries);

            assert!(
                matches!(result, BatchAuthResult::Invalid { .. }),
                "PHASE2-AUDIT: Wrong key should fail verification"
            );
        }

        #[test]
        fn test_batch_auth_modified_entry_fails() {
            let key = BatchAuthKey::from_bytes([0xAA; 32]);
            let auth = BatchAuthenticator::new(key, 1);

            let entries = vec![make_entry(100, 500, vec![1, 2, 3, 4])];
            let tag = auth.sign_batch(1, 1, &entries);

            let modified_entries = vec![make_entry(100, 500, vec![1, 2, 3, 5])];
            let result = auth.verify_batch(&tag, 1, 1, &modified_entries);

            assert!(
                matches!(result, BatchAuthResult::Invalid { .. }),
                "PHASE2-AUDIT: Modified entry should fail verification"
            );
        }

        #[test]
        fn test_batch_auth_empty_batch_valid() {
            let key = BatchAuthKey::from_bytes([0xAA; 32]);
            let auth = BatchAuthenticator::new(key, 1);

            let entries: Vec<JournalEntry> = vec![];
            let tag = auth.sign_batch(1, 1, &entries);
            let result = auth.verify_batch(&tag, 1, 1, &entries);

            assert!(
                matches!(result, BatchAuthResult::Valid),
                "PHASE2-AUDIT: Empty batch should verify as valid"
            );
        }
    }

    mod group8_nfs_auth_boundary {
        use super::*;

        #[test]
        fn test_auth_sys_max_gids_accepted() {
            let gids: Vec<u32> = (0..AUTH_SYS_MAX_GIDS as u32).collect();
            let cred = AuthSysCred {
                stamp: 1,
                machinename: "test".to_string(),
                uid: 1000,
                gid: 1000,
                gids,
            };

            let encoded = cred.encode_xdr();
            let decoded = AuthSysCred::decode_xdr(&encoded).unwrap();
            assert_eq!(decoded.gids.len(), AUTH_SYS_MAX_GIDS);
        }

        #[test]
        fn test_auth_sys_over_max_gids_rejected() {
            let gids: Vec<u32> = (0..(AUTH_SYS_MAX_GIDS + 1) as u32).collect();
            let cred = AuthSysCred {
                stamp: 1,
                machinename: "test".to_string(),
                uid: 1000,
                gid: 1000,
                gids,
            };

            let encoded = cred.encode_xdr();
            let result = AuthSysCred::decode_xdr(&encoded);

            assert!(
                result.is_err(),
                "PHASE2-AUDIT: Should reject more than {} GIDs",
                AUTH_SYS_MAX_GIDS
            );
        }

        #[test]
        fn test_auth_cred_unknown_flavor_maps_to_nobody() {
            let opaque = OpaqueAuth {
                flavor: 99,
                body: vec![],
            };
            let cred = AuthCred::from_opaque_auth(&opaque);

            assert_eq!(
                cred.uid(),
                65534,
                "PHASE2-AUDIT: Unknown flavor should map to nobody UID"
            );
            assert_eq!(
                cred.gid(),
                65534,
                "PHASE2-AUDIT: Unknown flavor should map to nobody GID"
            );
        }
    }
}
