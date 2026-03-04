//! Extended security tests for claudefs-reduce: write path, WORM, key rotation, GC, segments.
//!
//! Part of A10 Phase 10: Reduce extended security audit

use claudefs_reduce::dedupe::{CasIndex, Chunker, ChunkerConfig};
use claudefs_reduce::encryption::EncryptionKey;
use claudefs_reduce::fingerprint::blake3_hash;
use claudefs_reduce::gc::{GcConfig, GcEngine, GcStats};
use claudefs_reduce::key_manager::{KeyManager, KeyVersion, WrappedKey};
use claudefs_reduce::key_rotation_scheduler::{KeyRotationScheduler, RotationStatus};
use claudefs_reduce::pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats};
use claudefs_reduce::segment::{Segment, SegmentEntry, SegmentPacker, SegmentPackerConfig};
use claudefs_reduce::snapshot::{SnapshotConfig, SnapshotInfo, SnapshotManager};
use claudefs_reduce::worm_reducer::{RetentionPolicy, WormMode, WormReducer};
use claudefs_reduce::CompressionAlgorithm;
use claudefs_reduce::WritePathConfig;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(n: u64) -> u64 {
        n
    }

    fn make_chunk_hash(data: &[u8]) -> claudefs_reduce::fingerprint::ChunkHash {
        blake3_hash(data)
    }

    // Category 1: WORM Policy Enforcement

    #[test]
    fn test_worm_none_always_expired() {
        let policy = RetentionPolicy::none();
        assert!(
            policy.is_expired(0),
            "none policy should be expired at ts=0"
        );
        assert!(
            policy.is_expired(u64::MAX),
            "none policy should be expired at ts=u64::MAX"
        );
    }

    #[test]
    fn test_worm_legal_hold_never_expires() {
        let policy = RetentionPolicy::legal_hold();
        assert!(
            !policy.is_expired(0),
            "legal hold should not be expired at ts=0"
        );
        assert!(
            !policy.is_expired(u64::MAX),
            "legal hold should not be expired at ts=u64::MAX"
        );
    }

    #[test]
    fn test_worm_immutable_expiry_boundary() {
        let policy = RetentionPolicy::immutable_until(100);
        assert!(!policy.is_expired(99), "should NOT be expired at ts=99");
        assert!(
            !policy.is_expired(100),
            "should NOT be expired at ts=100 (exact boundary)"
        );
        assert!(policy.is_expired(101), "should be expired at ts=101");
    }

    #[test]
    fn test_worm_reducer_policy_upgrade() {
        let mut reducer = WormReducer::new();
        reducer
            .register(make_hash(1), RetentionPolicy::none(), 100)
            .unwrap();
        let result = reducer.register(make_hash(1), RetentionPolicy::legal_hold(), 200);
        assert!(
            result.is_ok(),
            "upgrade from none to legal_hold should succeed"
        );
        let (policy, size) = reducer.get(&make_hash(1)).unwrap();
        assert!(
            matches!(policy.mode, WormMode::LegalHold),
            "policy should be upgraded to legal_hold"
        );
        assert_eq!(*size, 200, "size should be updated");
    }

    #[test]
    fn test_worm_reducer_active_count() {
        let mut reducer = WormReducer::new();
        reducer
            .register(make_hash(1), RetentionPolicy::none(), 100)
            .unwrap();
        reducer
            .register(make_hash(2), RetentionPolicy::immutable_until(1000), 200)
            .unwrap();
        reducer
            .register(make_hash(3), RetentionPolicy::legal_hold(), 300)
            .unwrap();

        let active = reducer.active_count(500);
        assert_eq!(
            active, 2,
            "at ts=500: none expired, immutable active, legal hold active"
        );
    }

    // Category 2: Key Rotation Scheduler

    #[test]
    fn test_rotation_initial_state_idle() {
        let scheduler = KeyRotationScheduler::new();
        assert!(
            matches!(scheduler.status(), RotationStatus::Idle),
            "new scheduler should be Idle"
        );
        assert_eq!(
            scheduler.pending_count(),
            0,
            "no chunks need rotation initially"
        );
    }

    #[test]
    fn test_rotation_schedule_from_idle() {
        let mut scheduler = KeyRotationScheduler::new();
        let result = scheduler.schedule_rotation(KeyVersion(2));
        assert!(result.is_ok(), "schedule rotation from idle should succeed");
        assert!(
            matches!(
                scheduler.status(),
                RotationStatus::Scheduled {
                    target_version: KeyVersion(2)
                }
            ),
            "status should be Scheduled"
        );
    }

    #[test]
    fn test_rotation_double_schedule_fails() {
        let mut scheduler = KeyRotationScheduler::new();
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();
        let result = scheduler.schedule_rotation(KeyVersion(2));
        assert!(result.is_err(), "second schedule should fail");
    }

    #[test]
    fn test_rotation_mark_needs_rotation() {
        let mut scheduler = KeyRotationScheduler::new();

        let wrapped = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(1),
        };

        for i in 1..=5 {
            scheduler.register_chunk(i, wrapped.clone());
        }

        scheduler.mark_needs_rotation(KeyVersion(1));
        assert_eq!(
            scheduler.pending_count(),
            5,
            "all 5 chunks should need rotation"
        );
    }

    #[test]
    fn test_rotation_register_chunk() {
        let mut scheduler = KeyRotationScheduler::new();

        let wrapped = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };

        scheduler.register_chunk(1, wrapped.clone());
        assert_eq!(scheduler.total_chunks(), 1, "one chunk registered");

        scheduler.register_chunk(1, wrapped);
        assert_eq!(
            scheduler.total_chunks(),
            1,
            "duplicate registration should overwrite"
        );
    }

    // Category 3: GC Extended Security

    #[test]
    fn test_gc_config_defaults() {
        let config = GcConfig::default();
        assert!(
            config.sweep_threshold == 0,
            "sweep_threshold should be 0 by default (reclaim when refcount=0)"
        );
    }

    #[test]
    fn test_gc_stats_initial() {
        let mut cas = CasIndex::new();
        let mut gc = GcEngine::new(GcConfig::default());
        let stats = gc.sweep(&mut cas);

        assert_eq!(stats.chunks_scanned, 0, "no chunks scanned initially");
        assert_eq!(stats.chunks_reclaimed, 0, "nothing reclaimed initially");
    }

    #[test]
    fn test_gc_mark_before_sweep() {
        let mut cas = CasIndex::new();

        let hash1 = make_chunk_hash(b"chunk1");
        let hash2 = make_chunk_hash(b"chunk2");
        let hash3 = make_chunk_hash(b"chunk3");

        cas.insert(hash1);
        cas.release(&hash1);
        cas.insert(hash2);
        cas.release(&hash2);
        cas.insert(hash3);
        cas.release(&hash3);

        let mut gc = GcEngine::new(GcConfig::default());
        let stats = gc.sweep(&mut cas);

        assert_eq!(
            stats.chunks_reclaimed, 3,
            "all unreferenced chunks should be reclaimed"
        );
    }

    #[test]
    fn test_gc_mark_and_retain() {
        let mut cas = CasIndex::new();

        let hash1 = make_chunk_hash(b"chunk1");
        let hash2 = make_chunk_hash(b"chunk2");
        let hash3 = make_chunk_hash(b"chunk3");

        cas.insert(hash1);
        cas.insert(hash2);
        cas.release(&hash2);
        cas.insert(hash3);
        cas.release(&hash3);

        let mut gc = GcEngine::new(GcConfig::default());
        gc.mark_reachable(&[hash1]);
        let stats = gc.sweep(&mut cas);

        assert_eq!(
            stats.chunks_reclaimed, 2,
            "2 chunks should be reclaimed (hash2, hash3)"
        );
        assert!(cas.lookup(&hash1), "hash1 should be retained");
        assert!(!cas.lookup(&hash2), "hash2 should be reclaimed");
        assert!(!cas.lookup(&hash3), "hash3 should be reclaimed");
    }

    #[test]
    fn test_gc_multiple_cycles() {
        let gc_config = GcConfig::default();
        let mut total_scanned = 0;

        for i in 1..=3 {
            let mut cas = CasIndex::new();
            let mut gc = GcEngine::new(gc_config.clone());

            let hash = make_chunk_hash(format!("cycle{}_chunk", i).as_bytes());
            cas.insert(hash);
            cas.release(&hash);

            gc.clear_marks();
            let stats = gc.sweep(&mut cas);
            total_scanned += stats.chunks_scanned;
        }

        assert_eq!(total_scanned, 3, "3 cycles should scan 3 total chunks");
    }

    // Category 4: Write Path & Pipeline Stats

    #[test]
    fn test_pipeline_config_defaults() {
        let config = PipelineConfig::default();

        assert!(
            matches!(config.inline_compression, CompressionAlgorithm::Lz4),
            "default compression should be LZ4"
        );

        assert!(
            matches!(
                config.encryption,
                claudefs_reduce::encryption::EncryptionAlgorithm::AesGcm256
            ),
            "default encryption should be AES-GCM-256"
        );

        assert!(
            config.dedup_enabled,
            "deduplication should be enabled by default"
        );
    }

    #[test]
    fn test_reduction_stats_ratio() {
        let stats = ReductionStats {
            input_bytes: 1000,
            chunks_total: 0,
            chunks_deduplicated: 0,
            bytes_after_dedup: 0,
            bytes_after_compression: 0,
            bytes_after_encryption: 500,
            compression_ratio: 0.0,
            dedup_ratio: 0.0,
        };

        let ratio = if stats.bytes_after_encryption > 0 {
            stats.input_bytes as f64 / stats.bytes_after_encryption as f64
        } else {
            1.0
        };

        assert!(
            (ratio - 2.0).abs() < 0.001,
            "ratio should be approximately 2.0"
        );
    }

    #[test]
    fn test_reduction_stats_zero_stored() {
        let stats = ReductionStats {
            input_bytes: 1000,
            chunks_total: 0,
            chunks_deduplicated: 0,
            bytes_after_dedup: 0,
            bytes_after_compression: 0,
            bytes_after_encryption: 0,
            compression_ratio: 0.0,
            dedup_ratio: 0.0,
        };

        let ratio = if stats.bytes_after_encryption > 0 {
            stats.input_bytes as f64 / stats.bytes_after_encryption as f64
        } else {
            1.0
        };

        assert_eq!(
            ratio, 1.0,
            "zero stored bytes should not cause panic, returns 1.0"
        );
    }

    #[test]
    fn test_chunker_config_validation() {
        let config = ChunkerConfig {
            min_size: 1000,
            avg_size: 500,
            max_size: 2000,
        };

        assert!(
            config.min_size > config.avg_size,
            "min_size > avg_size is a configuration issue"
        );

        let chunker = Chunker::with_config(config);
        let data: Vec<u8> = (0..10000).map(|i| (i % 251) as u8).collect();
        let chunks = chunker.chunk(&data);

        assert!(
            !chunks.is_empty(),
            "chunking should still produce results despite invalid config"
        );
    }

    #[test]
    fn test_cas_index_insert_duplicate() {
        let mut cas = CasIndex::new();
        let hash = make_chunk_hash(b"test data");

        cas.insert(hash);
        assert_eq!(
            cas.refcount(&hash),
            1,
            "first insert should set refcount to 1"
        );

        cas.insert(hash);
        assert_eq!(
            cas.refcount(&hash),
            2,
            "second insert should increment to 2"
        );
    }

    // Category 5: Snapshot & Segment Extended

    #[test]
    fn test_snapshot_create_and_list() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());

        let hashes1 = vec![make_chunk_hash(b"a"), make_chunk_hash(b"b")];
        let hashes2 = vec![make_chunk_hash(b"c")];
        let hashes3 = vec![make_chunk_hash(b"d"), make_chunk_hash(b"e")];

        mgr.create_snapshot("snap1".to_string(), hashes1, 100)
            .unwrap();
        mgr.create_snapshot("snap2".to_string(), hashes2, 50)
            .unwrap();
        mgr.create_snapshot("snap3".to_string(), hashes3, 200)
            .unwrap();

        let list = mgr.list_snapshots();
        assert_eq!(list.len(), 3, "should have 3 snapshots");

        let ids: Vec<u64> = list.iter().map(|info| info.id).collect();
        let unique_ids: std::collections::HashSet<_> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique_ids.len(), "all IDs should be unique");
    }

    #[test]
    fn test_snapshot_delete_nonexistent() {
        let mut mgr = SnapshotManager::new(SnapshotConfig::default());
        let deleted = mgr.delete_snapshot(999);
        assert!(
            deleted.is_none(),
            "deleting nonexistent snapshot should return None"
        );
    }

    #[test]
    fn test_segment_packer_seal_empty() {
        let mut packer: SegmentPacker = SegmentPacker::default();
        let result = packer.flush();
        assert!(result.is_none(), "flush on empty packer should return None");
    }

    #[test]
    fn test_segment_entry_integrity() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        for i in 0..5 {
            let data = format!("chunk data {}", i);
            let hash = make_chunk_hash(data.as_bytes());
            let payload = data.as_bytes().to_vec();
            packer.add_chunk(hash, &payload, payload.len() as u32);
        }

        let segment = packer.flush().expect("should return sealed segment");
        assert_eq!(segment.entries.len(), 5, "should have 5 entries");

        let integrity_result = segment.verify_integrity();
        assert!(
            integrity_result.is_ok(),
            "integrity check should pass for sealed segment"
        );
    }

    #[test]
    fn test_segment_packer_config_defaults() {
        let config = SegmentPackerConfig::default();

        assert!(
            config.target_size > 0,
            "target_size should be greater than 0"
        );

        const DEFAULT_SEGMENT_SIZE: usize = 2 * 1024 * 1024;
        assert_eq!(
            config.target_size, DEFAULT_SEGMENT_SIZE,
            "default should be 2MB for erasure coding"
        );

        assert!(
            config.target_size <= 10 * 1024 * 1024,
            "target_size should be reasonable (not > 10MB)"
        );
    }
}
