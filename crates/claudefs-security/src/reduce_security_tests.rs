// FILE: reduce_security_tests.rs

#[cfg(test)]
mod tests {
    use claudefs_reduce::{
        checksum::{compute, verify, ChecksumAlgorithm},
        compression::{compress, decompress, CompressionAlgorithm},
        encryption::{
            decrypt, derive_chunk_key, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey,
        },
        fingerprint::{blake3_hash, ChunkHash, SuperFeatures},
        gc::{GcConfig, GcEngine},
        key_manager::{KeyManager, KeyManagerConfig, KeyVersion, WrappedKey},
        key_rotation_scheduler::KeyRotationScheduler,
        segment::{Segment, SegmentPacker, SegmentPackerConfig},
        snapshot::{SnapshotConfig, SnapshotManager},
        CasIndex,
    };
    use std::collections::HashSet;

    macro_rules! finding {
        ($id:expr, $msg:expr) => {
            eprintln!("FINDING-{}: {}", $id, $msg)
        };
    }

    #[test]
    fn test_gc_sweep_with_incomplete_mark() {
        let mut cas = CasIndex::new();
        let hash1 = blake3_hash(b"chunk1");
        let hash2 = blake3_hash(b"chunk2");
        let hash3 = blake3_hash(b"chunk3");

        cas.insert(hash1.clone());
        cas.insert(hash2.clone());
        cas.insert(hash3.clone());

        cas.release(&hash1);
        cas.release(&hash2);
        cas.release(&hash3);

        let mut gc = GcEngine::new(GcConfig { sweep_threshold: 0 });
        gc.mark_reachable(&[hash1, hash2]);

        let stats = gc.sweep(&mut cas);

        let hash1_present = cas.lookup(&hash1);
        let hash2_present = cas.lookup(&hash2);
        let hash3_present = cas.lookup(&hash3);

        if !hash1_present && !hash2_present {
            finding!("REDUCE-01", "GC can delete live data if mark phase is incomplete - sweep doesn't check reachable set");
        }

        if !hash3_present {
            finding!("REDUCE-01", "Unmarked chunks are reclaimed as expected");
        }

        assert_eq!(stats.chunks_reclaimed, 3);
    }

    #[test]
    fn test_gc_clear_marks_then_sweep() {
        let mut cas = CasIndex::new();
        let hash1 = blake3_hash(b"data1");
        let hash2 = blake3_hash(b"data2");

        cas.insert(hash1.clone());
        cas.insert(hash2.clone());
        cas.release(&hash1);
        cas.release(&hash2);

        let mut gc = GcEngine::new(GcConfig { sweep_threshold: 0 });
        gc.mark_reachable(&[hash1.clone()]);
        gc.clear_marks();

        let stats = gc.sweep(&mut cas);

        let both_reclaimed = !cas.lookup(&hash1) && !cas.lookup(&hash2);
        if both_reclaimed {
            finding!(
                "REDUCE-02",
                "Double-clear mark phase danger: clear_marks() + sweep deletes everything"
            );
        }

        assert_eq!(stats.chunks_scanned, 2);
    }

    #[test]
    fn test_gc_concurrent_insert_during_sweep() {
        let mut cas = CasIndex::new();
        let existing_hash = blake3_hash(b"existing");
        cas.insert(existing_hash.clone());
        cas.release(&existing_hash);

        let mut gc = GcEngine::new(GcConfig { sweep_threshold: 0 });
        gc.mark_reachable(&[existing_hash.clone()]);

        let new_hash = blake3_hash(b"newly_inserted");
        cas.insert(new_hash.clone());
        cas.release(&new_hash);

        let stats = gc.sweep(&mut cas);

        let existing_still_present = cas.lookup(&existing_hash);
        let new_is_gone = !cas.lookup(&new_hash);

        if new_is_gone && existing_still_present {
            finding!(
                "REDUCE-03",
                "TOCTOU in mark-sweep: hash inserted after mark but before sweep is vulnerable"
            );
        }

        assert!(
            existing_still_present || new_is_gone,
            "GC reclaim behavior observed"
        );
    }

    #[test]
    fn test_cas_refcount_underflow() {
        let mut cas = CasIndex::new();
        let hash = blake3_hash(b"test_chunk");

        cas.insert(hash.clone());
        let released_first = cas.release(&hash);
        let released_second = cas.release(&hash);

        if !released_first && released_second {
            finding!(
                "REDUCE-04",
                "Refcount underflow: double release may return true incorrectly"
            );
        }

        if released_first && !released_second {
            finding!(
                "REDUCE-04",
                "Refcount underflow: double release returns true then false"
            );
        }

        assert!(released_first, "First release should succeed");
    }

    #[test]
    fn test_gc_stats_accuracy() {
        let mut cas = CasIndex::new();
        let hash1 = blake3_hash(b"reachable1");
        let hash2 = blake3_hash(b"reachable2");
        let hash3 = blake3_hash(b"unreachable");

        cas.insert(hash1.clone());
        cas.insert(hash2.clone());
        cas.insert(hash3.clone());

        cas.release(&hash1);
        cas.release(&hash2);
        cas.release(&hash3);

        let mut gc = GcEngine::new(GcConfig { sweep_threshold: 0 });
        let stats = gc.run_cycle(&mut cas, &[hash1.clone(), hash2.clone()]);

        assert_eq!(stats.chunks_scanned, 3);
        finding!(
            "REDUCE-11",
            "GC stats shows all 3 chunks scanned even when 2 marked reachable"
        );
    }

    #[test]
    fn test_key_manager_no_key_generate_dek() {
        let km = KeyManager::new(KeyManagerConfig { max_key_history: 5 });
        let result = km.generate_dek();

        match result {
            Ok(_) => {
                finding!(
                    "REDUCE-10",
                    "Key manager allows DEK generation without KEK initialized"
                );
            }
            Err(e) => eprintln!("Generate DEK without key error: {:?}", e),
        }
    }

    #[test]
    fn test_key_manager_wrap_unwrap_roundtrip() {
        let key = EncryptionKey([0u8; 32]);
        let mut km = KeyManager::with_initial_key(KeyManagerConfig { max_key_history: 5 }, key);

        let dek = km.generate_dek().expect("Failed to generate DEK");
        let wrapped = km.wrap_dek(&dek).expect("Failed to wrap DEK");
        let unwrapped = km.unwrap_dek(&wrapped).expect("Failed to unwrap DEK");

        assert_eq!(dek.key, unwrapped.key);
    }

    #[test]
    fn test_key_manager_unwrap_after_clear_history() {
        let key1 = EncryptionKey([1u8; 32]);
        let key2 = EncryptionKey([2u8; 32]);

        let mut km = KeyManager::with_initial_key(KeyManagerConfig { max_key_history: 5 }, key1);
        let dek = km.generate_dek().expect("Failed to generate DEK");
        let wrapped = km.wrap_dek(&dek).expect("Failed to wrap DEK");

        km.rotate_key(key2);
        km.clear_history();

        let result = km.unwrap_dek(&wrapped);
        if result.is_err() {
            finding!(
                "REDUCE-05",
                "Key loss on history clear: cannot unwrap old wrapped key after clear_history()"
            );
        }
    }

    #[test]
    fn test_key_rotation_scheduler_rewrap_without_schedule() {
        let mut scheduler = KeyRotationScheduler::new();
        let key = EncryptionKey([0u8; 32]);
        let mut km = KeyManager::with_initial_key(KeyManagerConfig { max_key_history: 5 }, key);

        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        scheduler.register_chunk(1, wrapped);

        let result = scheduler.rewrap_next(&mut km);

        match result {
            Ok(None) => {}
            Ok(Some(_)) => finding!(
                "REDUCE-06",
                "Rewrap succeeded without schedule - potential double schedule race"
            ),
            Err(e) => eprintln!("Rewrap error: {:?}", e),
        }
    }

    #[test]
    fn test_key_rotation_scheduler_double_schedule() {
        let mut scheduler = KeyRotationScheduler::new();
        let wrapped = WrappedKey {
            ciphertext: vec![0u8; 32],
            nonce: [0u8; 12],
            kek_version: KeyVersion(1),
        };
        scheduler.register_chunk(1, wrapped);

        let key = EncryptionKey([0u8; 32]);
        let mut km = KeyManager::with_initial_key(KeyManagerConfig { max_key_history: 5 }, key);

        let result1 = scheduler.schedule_rotation(KeyVersion(2));
        let result2 = scheduler.schedule_rotation(KeyVersion(2));

        if result1.is_ok() && result2.is_ok() {
            finding!(
                "REDUCE-06",
                "Double schedule race: scheduling rotation twice succeeds without error"
            );
        }
    }

    #[test]
    fn test_wrapped_key_tampered_ciphertext() {
        use claudefs_reduce::error::ReduceError;

        let key = EncryptionKey([0u8; 32]);
        let km = KeyManager::with_initial_key(KeyManagerConfig { max_key_history: 5 }, key);

        let dek = km.generate_dek().unwrap();
        let mut wrapped = km.wrap_dek(&dek).unwrap();

        if !wrapped.ciphertext.is_empty() {
            wrapped.ciphertext[0] ^= 0xFF;
        }

        let result = km.unwrap_dek(&wrapped);
        match result {
            Err(ReduceError::DecryptionAuthFailed) => {}
            Ok(_) => panic!("Tampered ciphertext should fail auth"),
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_nonce_uniqueness() {
        let mut nonces: HashSet<Vec<u8>> = HashSet::new();

        for _ in 0..100 {
            let nonce = random_nonce();
            let bytes: Vec<u8> = nonce.0.to_vec();
            if !nonces.insert(bytes) {
                finding!(
                    "REDUCE-07",
                    "Nonce reuse probability: duplicate nonce detected"
                );
                panic!("Duplicate nonce found!");
            }
        }
    }

    #[test]
    fn test_encrypt_empty_plaintext() {
        let key = EncryptionKey([0u8; 32]);
        let result = encrypt(b"", &key, EncryptionAlgorithm::AesGcm256);

        match result {
            Ok(chunk) => {
                let decrypted = decrypt(&chunk, &key);
                assert!(decrypted.is_ok());
                assert_eq!(decrypted.unwrap(), b"");
            }
            Err(e) => panic!("Encrypt empty should succeed, got: {:?}", e),
        }
    }

    #[test]
    fn test_decrypt_wrong_key() {
        use claudefs_reduce::error::ReduceError;

        let key_a = EncryptionKey([0xAAu8; 32]);
        let key_b = EncryptionKey([0xBBu8; 32]);

        let encrypted = encrypt(b"secret data", &key_a, EncryptionAlgorithm::AesGcm256).unwrap();
        let result = decrypt(&encrypted, &key_b);

        match result {
            Err(ReduceError::DecryptionAuthFailed) => {}
            Ok(_) => panic!("Decryption with wrong key should fail"),
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_derive_chunk_key_deterministic() {
        let master_key = EncryptionKey([1u8; 32]);
        let chunk_hash = [2u8; 32];

        let key1 = derive_chunk_key(&master_key, &chunk_hash);
        let key2 = derive_chunk_key(&master_key, &chunk_hash);

        assert_eq!(key1.0, key2.0, "derive_chunk_key should be deterministic");
    }

    #[test]
    fn test_checksum_tampered_data() {
        use claudefs_reduce::error::ReduceError;

        let data = b"important data";
        let checksum = compute(data, ChecksumAlgorithm::Blake3);

        let mut tampered = data.to_vec();
        tampered[0] ^= 0xFF;

        let result = verify(&tampered, &checksum);
        match result {
            Err(ReduceError::ChecksumMismatch) => {}
            Ok(_) => panic!("Tampered data should fail checksum verification"),
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_segment_integrity_tampered_payload() {
        use claudefs_reduce::error::ReduceError;

        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 4096 });
        let hash = blake3_hash(b"test_payload");
        packer.add_chunk(hash, b"test payload data", 17);

        let segment = packer.flush().unwrap();

        let mut tampered_payload = segment.payload.clone();
        tampered_payload[0] ^= 0xFF;

        let tampered_segment = Segment {
            id: segment.id,
            entries: segment.entries,
            payload: tampered_payload,
            sealed: segment.sealed,
            created_at_secs: segment.created_at_secs,
            payload_checksum: segment.payload_checksum,
        };

        let result = tampered_segment.verify_integrity();
        match result {
            Err(ReduceError::ChecksumMismatch) => {}
            Ok(_) => panic!("Tampered payload should fail integrity check"),
            Err(e) => eprintln!(
                "FINDING-REDUCE-08: Segment tamper detection result: {:?}",
                e
            ),
        }
    }

    #[test]
    fn test_snapshot_max_limit() {
        let mut manager = SnapshotManager::new(SnapshotConfig { max_snapshots: 2 });

        let _ = manager.create_snapshot("snap1".to_string(), vec![], 0);
        let _ = manager.create_snapshot("snap2".to_string(), vec![], 0);
        let result = manager.create_snapshot("snap3".to_string(), vec![], 0);

        match result {
            Ok(info) => {
                if info.id == 3 {
                    finding!(
                        "REDUCE-09",
                        "Snapshot limit enforcement: oldest snapshot not auto-deleted"
                    );
                }
            }
            Err(_) => {}
        }

        assert_eq!(manager.snapshot_count(), 2);
    }

    #[test]
    fn test_snapshot_clone_nonexistent() {
        let mut manager = SnapshotManager::new(SnapshotConfig { max_snapshots: 5 });
        let result = manager.clone_snapshot(999, "clone_of_nonexistent".to_string());

        match result {
            Err(e) => eprintln!("Clone nonexistent snapshot error: {:?}", e),
            Ok(_) => panic!("Should fail cloning nonexistent snapshot"),
        }
    }

    #[test]
    fn test_compression_decompression_roundtrip_all_algos() {
        let original = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(10);

        let compressed_lz4 = compress(&original, CompressionAlgorithm::Lz4).unwrap();
        let decompressed_lz4 = decompress(&compressed_lz4, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(original.to_vec(), decompressed_lz4);

        let compressed_zstd = compress(&original, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        let decompressed_zstd =
            decompress(&compressed_zstd, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert_eq!(original.to_vec(), decompressed_zstd);
    }
}
