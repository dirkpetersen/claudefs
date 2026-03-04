//! Deep security tests for claudefs-reduce: encryption, key mgmt, dedup, compression, GC.
//!
//! Part of A10 Phase 6: Reduce deep security audit

#[cfg(test)]
mod tests {
    use claudefs_reduce::{
        checksum::{
            compute as compute_checksum, verify as verify_checksum, ChecksumAlgorithm,
            ChecksummedBlock, DataChecksum,
        },
        compression::{compress, decompress, is_compressible, CompressionAlgorithm},
        dedupe::{CasIndex, Chunker, ChunkerConfig},
        encryption::{
            decrypt, derive_chunk_key, encrypt, random_nonce, EncryptedChunk, EncryptionAlgorithm,
            EncryptionKey,
        },
        error::ReduceError,
        fingerprint::{blake3_hash, super_features, ChunkHash, SuperFeatures},
        gc::{GcConfig, GcEngine},
        key_manager::{
            DataKey, KeyManager, KeyManagerConfig, KeyVersion, VersionedKey, WrappedKey,
        },
        pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats},
        segment::{Segment, SegmentEntry, SegmentPacker, SegmentPackerConfig},
        snapshot::{Snapshot, SnapshotConfig, SnapshotInfo, SnapshotManager},
    };

    fn make_master_key() -> EncryptionKey {
        EncryptionKey([0x42u8; 32])
    }

    fn make_data(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    fn make_chunk_hash(val: u8) -> ChunkHash {
        blake3_hash(&[val])
    }

    // =========================================================================
    // Category 1: Encryption & Key Management (5 tests)
    // =========================================================================

    #[test]
    fn test_encryption_deterministic_dek() {
        // FINDING-REDUCE-DEEP-01: deterministic DEK means same plaintext always
        // encrypts identically when nonce is reused (not the case here as nonce is random)
        // but the derive_chunk_key IS deterministic which is correct
        let master_key = make_master_key();
        let chunk_hash = [1u8; 32];

        let key1 = derive_chunk_key(&master_key, &chunk_hash);
        let key2 = derive_chunk_key(&master_key, &chunk_hash);

        assert_eq!(key1.0, key2.0, "derive_chunk_key must be deterministic");
    }

    #[test]
    fn test_encryption_different_chunks_different_keys() {
        let master_key = make_master_key();

        let key_a = derive_chunk_key(&master_key, &[1u8; 32]);
        let key_b = derive_chunk_key(&master_key, &[2u8; 32]);

        assert_ne!(
            key_a.0, key_b.0,
            "Different chunk hashes must produce different keys"
        );
    }

    #[test]
    fn test_key_rotation_preserves_decryption() {
        let config = KeyManagerConfig {
            max_key_history: 10,
        };
        let key_v0 = EncryptionKey([0xAAu8; 32]);
        let mut km = KeyManager::with_initial_key(config, key_v0);

        let dek = km.generate_dek().unwrap();
        let wrapped_v0 = km.wrap_dek(&dek).unwrap();
        assert_eq!(wrapped_v0.kek_version, KeyVersion(0));

        let key_v1 = EncryptionKey([0xBBu8; 32]);
        km.rotate_key(key_v1);

        let unwrapped = km
            .unwrap_dek(&wrapped_v0)
            .expect("Should be able to unwrap old wrapped DEK using historical KEK");
        assert_eq!(dek.key, unwrapped.key, "Historical key should still work");
    }

    #[test]
    fn test_key_wrap_tamper_detection() {
        let config = KeyManagerConfig { max_key_history: 5 };
        let key = make_master_key();
        let km = KeyManager::with_initial_key(config, key);

        let dek = km.generate_dek().unwrap();
        let mut wrapped = km.wrap_dek(&dek).unwrap();

        wrapped.ciphertext[0] ^= 0xFF;

        let result = km.unwrap_dek(&wrapped);
        assert!(
            matches!(result, Err(ReduceError::DecryptionAuthFailed)),
            "Tampered ciphertext must fail AEAD authentication"
        );
    }

    #[test]
    fn test_nonce_uniqueness() {
        use std::collections::HashSet;

        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let nonce = random_nonce();
            assert!(
                seen.insert(nonce.0.to_vec()),
                "Duplicate nonce detected in 1000 iterations"
            );
        }
    }

    // =========================================================================
    // Category 2: Dedup & Fingerprint Security (5 tests)
    // =========================================================================

    #[test]
    fn test_cas_refcount_underflow() {
        // FINDING-REDUCE-DEEP-04: double-release on refcount=0 returns true incorrectly
        // The release() function returns true when setting from any value <= 1 to 0,
        // so double-release returns true both times, potentially causing GC to think
        // a chunk was reclaimed when it wasn't.
        let mut cas = CasIndex::new();
        let hash = make_chunk_hash(1);

        cas.insert(hash);
        assert_eq!(cas.refcount(&hash), 1);

        let released_first = cas.release(&hash);
        assert!(released_first, "First release should return true");
        assert_eq!(cas.refcount(&hash), 0, "Refcount should be 0 after release");

        let released_second = cas.release(&hash);
        // BUG: Returns true even though refcount already 0 - this is the security finding
        assert!(
            released_second,
            "FINDING-REDUCE-DEEP-04: double-release incorrectly returns true"
        );
        assert_eq!(cas.refcount(&hash), 0, "Refcount stays at 0");
    }

    #[test]
    fn test_cas_drain_unreferenced() {
        let mut cas = CasIndex::new();
        let hash_a = make_chunk_hash(1);
        let hash_b = make_chunk_hash(2);

        cas.insert(hash_a);
        cas.insert(hash_a); // refcount = 2
        cas.insert(hash_b); // refcount = 1

        cas.release(&hash_a); // refcount = 1
        cas.release(&hash_b); // refcount = 0

        let removed = cas.drain_unreferenced();

        assert!(
            removed.contains(&hash_b),
            "B should be removed (refcount 0)"
        );
        assert!(
            !removed.contains(&hash_a),
            "A should NOT be removed (refcount 1)"
        );
    }

    #[test]
    fn test_blake3_deterministic() {
        let data = b"hello world this is test data";
        let hash1 = blake3_hash(data);
        let hash2 = blake3_hash(data);
        assert_eq!(hash1, hash2, "blake3_hash must be deterministic");
    }

    #[test]
    fn test_super_features_tiny_data() {
        // FINDING-REDUCE-DEEP-02: all tiny chunks (< 4 bytes) produce same features [0,0,0,0]
        // causing false-positive similarity detection
        let tiny1 = b"hi";
        let tiny2 = b"ab";

        let sf1 = super_features(tiny1);
        let sf2 = super_features(tiny2);

        assert_eq!(sf1, SuperFeatures([0u64; 4]));
        assert_eq!(sf2, SuperFeatures([0u64; 4]));
        assert_eq!(
            sf1, sf2,
            "Tiny data produces identical features - potential false positive"
        );
    }

    #[test]
    fn test_chunker_reassembly() {
        let chunker = Chunker::new();
        let data = make_data(200_000);
        let chunks = chunker.chunk(&data);

        assert!(!chunks.is_empty(), "Non-empty data should produce chunks");

        let reassembled: Vec<u8> = chunks.iter().flat_map(|c| c.data.iter().copied()).collect();

        assert_eq!(
            reassembled, data,
            "Concatenating chunk.data must equal original"
        );
    }

    // =========================================================================
    // Category 3: Compression Security (5 tests)
    // =========================================================================

    #[test]
    fn test_compression_roundtrip_lz4() {
        let data = make_data(50_000);
        let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed, data, "LZ4 roundtrip must preserve data");
    }

    #[test]
    fn test_compression_roundtrip_zstd() {
        let data = make_data(50_000);
        let compressed = compress(&data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        let decompressed =
            decompress(&compressed, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert_eq!(decompressed, data, "Zstd roundtrip must preserve data");
    }

    #[test]
    fn test_compression_none_passthrough() {
        let data = make_data(1000);
        let compressed = compress(&data, CompressionAlgorithm::None).unwrap();
        assert_eq!(
            compressed, data,
            "CompressionAlgorithm::None must be passthrough"
        );
    }

    #[test]
    fn test_compressible_detection() {
        let repetitive = vec![b'A'; 1000];
        let random: Vec<u8> = (0..1000).map(|_| rand::random::<u8>()).collect();

        assert!(
            is_compressible(&repetitive),
            "Repetitive data should be compressible"
        );
        assert!(
            !is_compressible(&random),
            "Random data should not be compressible"
        );
    }

    #[test]
    fn test_compression_empty_data() {
        let empty = vec![];

        let compressed_lz4 = compress(&empty, CompressionAlgorithm::Lz4).unwrap();
        let decompressed_lz4 = decompress(&compressed_lz4, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed_lz4, empty, "LZ4 empty roundtrip");

        let compressed_zstd = compress(&empty, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        let decompressed_zstd =
            decompress(&compressed_zstd, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert_eq!(decompressed_zstd, empty, "Zstd empty roundtrip");
    }

    // =========================================================================
    // Category 4: Checksum & Integrity (5 tests)
    // =========================================================================

    #[test]
    fn test_checksum_blake3_corruption() {
        let data = b"important data that must not be corrupted";
        let checksum = compute_checksum(data, ChecksumAlgorithm::Blake3);

        let mut corrupted = data.to_vec();
        corrupted[0] ^= 0xFF;

        let result = verify_checksum(&corrupted, &checksum);
        assert!(
            matches!(result, Err(ReduceError::ChecksumMismatch)),
            "Corrupted data must fail BLAKE3 verification"
        );
    }

    #[test]
    fn test_checksum_crc32c_collision_risk() {
        // FINDING-REDUCE-DEEP-03: CRC32C is non-cryptographic - only 4 bytes (32 bits)
        // High collision probability for large datasets. Not suitable for malicious
        // tampering detection, only for accidental corruption detection.
        let data1 = b"hello";
        let data2 = b"world";

        let checksum1 = compute_checksum(data1, ChecksumAlgorithm::Crc32c);
        let checksum2 = compute_checksum(data2, ChecksumAlgorithm::Crc32c);

        assert_ne!(
            checksum1.bytes, checksum2.bytes,
            "Different data produces different CRC32C"
        );
        assert_eq!(
            checksum1.bytes.len(),
            4,
            "CRC32C is only 4 bytes - vulnerable to collisions"
        );
    }

    #[test]
    fn test_checksummed_block_roundtrip() {
        let data = make_data(1000);
        let block = ChecksummedBlock::new(data.clone(), ChecksumAlgorithm::Blake3);

        let result = block.verify();
        assert!(
            result.is_ok(),
            "ChecksummedBlock::verify should pass for valid block"
        );
    }

    #[test]
    fn test_checksum_algorithm_downgrade() {
        let data = b"test data for algorithm downgrade check";

        let checksum_blake3 = compute_checksum(data, ChecksumAlgorithm::Blake3);

        let result = verify_checksum(data, &checksum_blake3);
        assert!(result.is_ok(), "Verify with same algorithm should pass");

        let wrong_algo_checksum = DataChecksum {
            algorithm: ChecksumAlgorithm::Crc32c,
            bytes: checksum_blake3.bytes.clone(),
        };

        let result2 = verify_checksum(data, &wrong_algo_checksum);
        assert!(
            result2.is_err(),
            "Verify with different algorithm should fail"
        );
    }

    #[test]
    fn test_checksum_empty_data() {
        let empty = vec![];

        let blake3 = compute_checksum(&empty, ChecksumAlgorithm::Blake3);
        let crc32c = compute_checksum(&empty, ChecksumAlgorithm::Crc32c);
        let xxhash = compute_checksum(&empty, ChecksumAlgorithm::Xxhash64);

        assert_eq!(blake3.bytes.len(), 32);
        assert_eq!(crc32c.bytes.len(), 4);
        assert_eq!(xxhash.bytes.len(), 8);

        let blake3_again = compute_checksum(&empty, ChecksumAlgorithm::Blake3);
        assert_eq!(
            blake3, blake3_again,
            "Empty data checksums must be deterministic"
        );
    }

    // =========================================================================
    // Category 5: Pipeline & GC Security (5 tests)
    // =========================================================================

    #[test]
    fn test_pipeline_write_read_roundtrip() {
        let config = PipelineConfig {
            encryption_enabled: false,
            dedup_enabled: true,
            compression_enabled: true,
            inline_compression: CompressionAlgorithm::Lz4,
            ..Default::default()
        };

        let mut pipeline = ReductionPipeline::new(config);
        let data = make_data(100_000);

        let (chunks, stats) = pipeline.process_write(&data).unwrap();

        assert_eq!(stats.input_bytes as usize, data.len());
        assert!(
            chunks.iter().all(|c| !c.is_duplicate),
            "First write should have no duplicates"
        );

        let recovered = pipeline.process_read(&chunks).unwrap();
        assert_eq!(recovered, data, "Recovered data must match original");
    }

    #[test]
    fn test_pipeline_dedup_detection() {
        let config = PipelineConfig {
            encryption_enabled: false,
            dedup_enabled: true,
            compression_enabled: false,
            ..Default::default()
        };

        let mut pipeline = ReductionPipeline::new(config);
        let data = make_data(50_000);

        let (chunks1, stats1) = pipeline.process_write(&data).unwrap();
        assert_eq!(
            stats1.chunks_deduplicated, 0,
            "First write has no duplicates"
        );

        let (_chunks2, stats2) = pipeline.process_write(&data).unwrap();
        assert_eq!(
            stats2.chunks_deduplicated,
            chunks1.len(),
            "Second write should detect all as duplicates"
        );
    }

    #[test]
    fn test_gc_sweep_unreferenced() {
        // FINDING-REDUCE-DEEP-05: sweep() ignores the reachable mark set by mark_reachable()
        // The GcEngine::sweep() only checks refcount==0, it doesn't check is_marked()
        // This means marked reachable chunks with refcount=0 will still be collected
        let mut cas = CasIndex::new();
        let hash_a = make_chunk_hash(1);
        let hash_b = make_chunk_hash(2);

        cas.insert(hash_a);
        cas.insert(hash_b);

        cas.release(&hash_a);
        cas.release(&hash_b);

        let mut gc = GcEngine::new(GcConfig::default());
        gc.mark_reachable(&[hash_a]);

        let stats = gc.sweep(&mut cas);

        // BUG: sweep() doesn't check reachable marks - it reclaims ALL zero-refcount entries
        // FINDING-REDUCE-DEEP-05: Both A and B are collected despite A being marked reachable
        assert!(
            !cas.lookup(&hash_a),
            "FINDING-REDUCE-DEEP-05: A incorrectly collected despite being marked"
        );
        assert!(!cas.lookup(&hash_b), "B correctly collected");
        assert_eq!(
            stats.chunks_reclaimed, 2,
            "Both zero-refcount chunks collected"
        );
    }

    #[test]
    fn test_snapshot_max_limit() {
        let mut manager = SnapshotManager::new(SnapshotConfig { max_snapshots: 2 });

        let info1 = manager
            .create_snapshot("snap1".to_string(), vec![], 1000)
            .unwrap();
        let info2 = manager
            .create_snapshot("snap2".to_string(), vec![], 2000)
            .unwrap();

        let result = manager.create_snapshot("snap3".to_string(), vec![], 3000);

        assert!(result.is_err(), "Should fail when max_snapshots reached");
        assert_eq!(manager.snapshot_count(), 2, "Should only have 2 snapshots");
        assert_eq!(info1.id, 1);
        assert_eq!(info2.id, 2);
    }

    #[test]
    fn test_segment_packing() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 4096 });

        let hash1 = make_chunk_hash(1);
        let hash2 = make_chunk_hash(2);
        let payload1 = make_data(500);
        let payload2 = make_data(700);

        packer.add_chunk(hash1, &payload1, 500);
        packer.add_chunk(hash2, &payload2, 700);

        let segment = packer.flush().expect("Should return sealed segment");

        assert!(segment.sealed);
        assert_eq!(segment.entries.len(), 2, "Segment should have 2 entries");
        assert_eq!(
            segment.payload.len(),
            1200,
            "Payload should contain both chunks"
        );

        let result = segment.verify_integrity();
        assert!(result.is_ok(), "Sealed segment should have valid checksum");
    }
}
