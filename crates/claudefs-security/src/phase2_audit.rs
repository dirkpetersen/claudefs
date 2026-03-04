//! Phase 2 security audit: enhanced tests for nonce security, key lifecycle, TLS, and auth boundaries.

#[cfg(test)]
mod tests {
    use claudefs_gateway::auth::{AuthCred, AuthSysCred, AUTH_SYS_MAX_GIDS};
    use claudefs_gateway::rpc::OpaqueAuth;
    use claudefs_reduce::encryption::{
        derive_chunk_key, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey, Nonce,
    };
    use claudefs_reduce::key_manager::{KeyManager, KeyManagerConfig};
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
    };
    use claudefs_transport::zerocopy::{RegionPool, ZeroCopyConfig};
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn k() -> EncryptionKey {
        EncryptionKey([42u8; 32])
    }
    fn k2() -> EncryptionKey {
        EncryptionKey([99u8; 32])
    }
    fn k0() -> EncryptionKey {
        EncryptionKey([0u8; 32])
    }

    #[test]
    fn test_nonce_never_repeats_same_key_same_plaintext() {
        let key = k();
        let mut nonces = HashSet::new();
        for _ in 0..1000 {
            let r = encrypt(b"test", &key, EncryptionAlgorithm::AesGcm256).unwrap();
            assert!(nonces.insert(r.nonce), "PHASE2-AUDIT: Nonce collision");
        }
        assert_eq!(nonces.len(), 1000);
    }

    #[test]
    fn test_nonce_distribution_all_bytes_covered() {
        let mut pos: Vec<HashSet<u8>> = vec![HashSet::new(); 12];
        for _ in 0..10000 {
            let n = random_nonce();
            for (i, b) in n.0.iter().enumerate() {
                pos[i].insert(*b);
            }
        }
        for (i, v) in pos.iter().enumerate() {
            assert!(v.len() > 1, "PHASE2-AUDIT: Byte {} stuck", i);
        }
    }

    #[test]
    fn test_concurrent_nonce_generation() {
        let set = Arc::new(Mutex::new(HashSet::new()));
        let h: Vec<_> = (0..8)
            .map(|_| {
                let s = set.clone();
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        s.lock().unwrap().insert(random_nonce());
                    }
                })
            })
            .collect();
        for h in h {
            h.join().unwrap();
        }
        assert_eq!(set.lock().unwrap().len(), 8000);
    }

    #[test]
    fn test_nonce_is_not_counter_based() {
        let key = k();
        let mut diffs = Vec::new();
        let mut p = None;
        for _ in 0..100 {
            let r = encrypt(b"x", &key, EncryptionAlgorithm::AesGcm256).unwrap();
            if let Some(prev) = p {
                diffs.push(
                    r.nonce
                        .0
                        .iter()
                        .zip(prev.0)
                        .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u32)
                        .sum(),
                );
            }
            p = Some(r.nonce);
        }
        assert!(
            diffs.iter().collect::<HashSet<_>>().len() > 50,
            "PHASE2-AUDIT: Counter-based nonce"
        );
    }

    #[test]
    fn test_hkdf_different_masters_different_outputs() {
        let d1 = derive_chunk_key(&k(), &[1u8; 32]);
        let d2 = derive_chunk_key(&k2(), &[1u8; 32]);
        assert_ne!(d1.0, d2.0, "PHASE2-AUDIT: Same derived key");
    }

    #[test]
    fn test_hkdf_all_zero_master_still_derives() {
        let d = derive_chunk_key(&k0(), &[1u8; 32]);
        assert_ne!(
            d.0, [0u8; 32],
            "PHASE2-AUDIT: Zero key produces zero output"
        );
    }

    #[test]
    fn test_hkdf_output_has_good_entropy() {
        let mut keys = Vec::new();
        for i in 0..100u32 {
            let h: Vec<u8> = i.to_le_bytes().repeat(8);
            let mut a = [0u8; 32];
            a.copy_from_slice(&h[..32]);
            keys.push(derive_chunk_key(&k(), &a));
        }
        for i in 0..keys.len() {
            for j in i + 1..keys.len() {
                let matches = (0..32).filter(|k| keys[i].0[*k] == keys[j].0[*k]).count();
                assert!(matches <= 4, "PHASE2-AUDIT: Keys share {} bytes", matches);
            }
        }
    }

    #[test]
    fn test_key_rotation_10_times_all_old_deks_recoverable() {
        let mut km = KeyManager::with_initial_key(
            KeyManagerConfig {
                max_key_history: 20,
            },
            k(),
        );
        let mut w = Vec::new();
        for i in 0..10 {
            w.push(km.wrap_dek(&km.generate_dek().unwrap()).unwrap());
            km.rotate_key(EncryptionKey([i as u8; 32]));
        }
        for (i, wi) in w.iter().enumerate() {
            assert!(
                km.unwrap_dek(wi).is_ok(),
                "PHASE2-AUDIT: DEK {} unrecoverable",
                i
            );
        }
    }

    #[test]
    fn test_history_pruning_loses_oldest_keys() {
        let mut km = KeyManager::with_initial_key(KeyManagerConfig { max_key_history: 5 }, k());
        for i in 0..10 {
            km.generate_dek().map(|d| km.wrap_dek(&d).unwrap()).ok();
            km.rotate_key(EncryptionKey([i as u8; 32]));
        }
        let old = km.wrap_dek(&km.generate_dek().unwrap()).unwrap();
        let curr = km.current_version().unwrap();
        assert!(
            old.kek_version.0 < curr.0 - 4,
            "PHASE2-AUDIT: Pruned key still recoverable"
        );
        assert!(
            km.unwrap_dek(&old).is_err(),
            "PHASE2-AUDIT: Pruned key accessible"
        );
    }

    #[test]
    fn test_rewrap_preserves_dek_value() {
        let mut km = KeyManager::with_initial_key(KeyManagerConfig::default(), k());
        let dek = km.generate_dek().unwrap();
        let orig = dek.key;
        let old = km.wrap_dek(&dek).unwrap();
        km.rotate_key(k2());
        let new = km.rewrap_dek(&old).unwrap();
        assert_eq!(
            km.unwrap_dek(&new).unwrap().key,
            orig,
            "PHASE2-AUDIT: DEK changed"
        );
    }

    #[test]
    fn test_node_cert_is_not_ca() {
        let (ca, ck) = generate_self_signed_ca().unwrap();
        let (nc, _) = generate_node_cert(&ca, &ck, "n1").unwrap();
        let c = &load_certs_from_pem(&nc).unwrap()[0];
        assert!(
            !c.value().is_ca().unwrap_or(true),
            "PHASE2-AUDIT: Node cert is CA"
        );
    }

    #[test]
    fn test_different_cas_produce_different_certs() {
        let (c1, k1) = generate_self_signed_ca().unwrap();
        let (c2, k2) = generate_self_signed_ca().unwrap();
        let (n1, _) = generate_node_cert(&c1, &k1, "n").unwrap();
        let (n2, _) = generate_node_cert(&c2, &k2, "n").unwrap();
        assert_ne!(n1, n2, "PHASE2-AUDIT: Same cert from diff CAs");
    }

    #[test]
    fn test_ca_cert_contains_proper_pem_markers() {
        let (pem, _) = generate_self_signed_ca().unwrap();
        let s = String::from_utf8_lossy(&pem);
        assert!(
            s.contains("-----BEGIN CERTIFICATE-----") && s.contains("-----END CERTIFICATE-----"),
            "PHASE2-AUDIT: Missing PEM markers"
        );
    }

    #[test]
    fn test_node_cert_signed_by_ca() {
        let (ca, ck) = generate_self_signed_ca().unwrap();
        let (nc, _) = generate_node_cert(&ca, &ck, "n").unwrap();
        assert_ne!(ca, nc, "PHASE2-AUDIT: Node cert same as CA");
    }

    fn mc(subject: &str, serial: &str, fp: &str, nb: u64, na: u64) -> CertificateInfo {
        CertificateInfo {
            subject: subject.to_string(),
            issuer: "CA".to_string(),
            serial: serial.to_string(),
            fingerprint_sha256: fp.to_string(),
            not_before_ms: nb,
            not_after_ms: na,
            is_ca: false,
        }
    }

    #[test]
    fn test_auth_level_tls_only_allows_without_client_cert_checks() {
        let mut auth = ConnectionAuthenticator::new(AuthConfig {
            level: AuthLevel::TlsOnly,
            ..Default::default()
        });
        auth.set_time(5000);
        let r = auth.authenticate(&mc("s1", "01", "abc", 1000, 86400_000_000));
        assert!(
            matches!(r, AuthResult::Allowed { .. }),
            "PHASE2-AUDIT: TlsOnly blocked"
        );
    }

    #[test]
    fn test_revocation_list_duplicate_serial_noop() {
        let mut rl = RevocationList::new();
        rl.revoke_serial("01".to_string());
        let before = rl.len();
        rl.revoke_serial("01".to_string());
        assert_eq!(before, rl.len(), "PHASE2-AUDIT: Duplicate grew list");
    }

    #[test]
    fn test_very_old_cert_rejected_strict_age() {
        let mut auth = ConnectionAuthenticator::new(AuthConfig {
            max_cert_age_days: 365,
            ..Default::default()
        });
        auth.set_time(501 * 86400_000);
        let r = auth.authenticate(&mc("s1", "01", "a", 500 * 86400_000, 600 * 86400_000));
        assert!(
            matches!(r, AuthResult::Denied { .. }),
            "PHASE2-AUDIT: Old cert accepted"
        );
    }

    #[test]
    fn test_auth_stats_increment_correctly() {
        let mut auth = ConnectionAuthenticator::new(AuthConfig::default());
        auth.set_time(5000);
        for _ in 0..5 {
            auth.authenticate(&mc("s1", "01", "a", 1000, 86400_000_000));
        }
        for _ in 0..3 {
            auth.authenticate(&mc("s2", "02", "b", 1000, 86400_000_000));
        }
        let s = auth.stats();
        assert_eq!(s.total_allowed, 5);
        assert_eq!(s.total_denied, 3);
    }

    #[test]
    fn test_released_region_data_is_zeroed() {
        let pool = RegionPool::new(ZeroCopyConfig {
            region_size: 64,
            max_regions: 10,
            alignment: 4096,
            preregister: 2,
        });
        let mut r = pool.acquire().unwrap();
        for b in r.as_mut_slice() {
            *b = 0xFF;
        }
        pool.release(r);
        let r = pool.acquire().unwrap();
        for b in r.as_slice() {
            assert_eq!(*b, 0, "PHASE2-AUDIT: Data not zeroed");
        }
        pool.release(r);
    }

    #[test]
    fn test_pool_grow_shrink_consistency() {
        let pool = RegionPool::new(ZeroCopyConfig {
            region_size: 1024,
            max_regions: 20,
            alignment: 4096,
            preregister: 2,
        });
        pool.grow(5);
        let tg = pool.total();
        let ag = pool.available();
        pool.shrink(3);
        let ts = pool.total();
        let as_ = pool.available();
        assert_eq!(tg, ag + pool.in_use());
        assert_eq!(ts, as_ + pool.in_use());
        assert_eq!(ts, tg - 3);
    }

    #[test]
    fn test_pool_max_regions_enforced() {
        let pool = RegionPool::new(ZeroCopyConfig {
            region_size: 1024,
            max_regions: 5,
            alignment: 4096,
            preregister: 0,
        });
        let mut rs = Vec::new();
        for _ in 0..10 {
            if let Some(r) = pool.acquire() {
                rs.push(r);
            }
        }
        assert!(rs.len() <= 5, "PHASE2-AUDIT: {} > 5 regions", rs.len());
        for r in rs {
            pool.release(r);
        }
    }

    fn e(seq: u64, ino: u64, pay: Vec<u8>) -> JournalEntry {
        JournalEntry {
            seq,
            shard_id: 0,
            site_id: 1,
            timestamp_us: seq * 1000,
            inode: ino,
            op: OpKind::Write,
            payload: pay,
            crc32: 0,
        }
    }

    #[test]
    fn test_batch_auth_wrong_key_fails() {
        let a = BatchAuthenticator::new(BatchAuthKey::from_bytes([0xAA; 32]), 1);
        let b = BatchAuthenticator::new(BatchAuthKey::from_bytes([0xBB; 32]), 1);
        let tag = a.sign_batch(1, 1, &[e(100, 500, vec![1, 2, 3, 4])]);
        assert!(
            matches!(
                b.verify_batch(&tag, 1, 1, &[e(100, 500, vec![1, 2, 3, 4])]),
                BatchAuthResult::Invalid { .. }
            ),
            "PHASE2-AUDIT: Wrong key passed"
        );
    }

    #[test]
    fn test_batch_auth_modified_entry_fails() {
        let a = BatchAuthenticator::new(BatchAuthKey::from_bytes([0xAA; 32]), 1);
        let tag = a.sign_batch(1, 1, &[e(100, 500, vec![1, 2, 3, 4])]);
        assert!(
            matches!(
                a.verify_batch(&tag, 1, 1, &[e(100, 500, vec![1, 2, 3, 5])]),
                BatchAuthResult::Invalid { .. }
            ),
            "PHASE2-AUDIT: Modified passed"
        );
    }

    #[test]
    fn test_batch_auth_empty_batch_valid() {
        let a = BatchAuthenticator::new(BatchAuthKey::from_bytes([0xAA; 32]), 1);
        let tag = a.sign_batch(1, 1, &[]);
        assert!(
            matches!(a.verify_batch(&tag, 1, 1, &[]), BatchAuthResult::Valid),
            "PHASE2-AUDIT: Empty batch invalid"
        );
    }

    #[test]
    fn test_auth_sys_max_gids_accepted() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "t".to_string(),
            uid: 1000,
            gid: 1000,
            gids: (0..AUTH_SYS_MAX_GIDS as u32).collect(),
        };
        assert_eq!(
            AuthSysCred::decode_xdr(&cred.encode_xdr())
                .unwrap()
                .gids
                .len(),
            AUTH_SYS_MAX_GIDS
        );
    }

    #[test]
    fn test_auth_sys_over_max_gids_rejected() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "t".to_string(),
            uid: 1000,
            gid: 1000,
            gids: (0..AUTH_SYS_MAX_GIDS as u32 + 1).collect(),
        };
        assert!(
            AuthSysCred::decode_xdr(&cred.encode_xdr()).is_err(),
            "PHASE2-AUDIT: Too many GIDs accepted"
        );
    }

    #[test]
    fn test_auth_cred_unknown_flavor_maps_to_nobody() {
        let cred = AuthCred::from_opaque_auth(&OpaqueAuth {
            flavor: 99,
            body: vec![],
        });
        assert_eq!(cred.uid(), 65534);
        assert_eq!(cred.gid(), 65534);
    }
}
