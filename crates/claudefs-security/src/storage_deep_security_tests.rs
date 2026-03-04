//! Deep security tests for the storage subsystem
//!
//! Part of A10 Phase 3: Storage integrity, recovery, journal, scrub, and hot-swap security audit

#[cfg(test)]
mod tests {
    use claudefs_storage::atomic_write::{
        AtomicWriteBatch, AtomicWriteCapability, AtomicWriteStats,
    };
    use claudefs_storage::block::{BlockId, BlockRef, BlockSize};
    use claudefs_storage::checksum::{Checksum, ChecksumAlgorithm};
    use claudefs_storage::device::DeviceRole;
    use claudefs_storage::hot_swap::{DeviceState, DrainProgress, HotSwapManager, HotSwapStats};
    use claudefs_storage::integrity_chain::{
        IntegrityAlgorithm, IntegrityConfig, IntegrityManager, PipelineStage, VerificationPoint,
        VerificationResult,
    };
    use claudefs_storage::recovery::{
        AllocatorBitmap, RecoveryConfig, RecoveryPhase, RecoveryState,
    };
    use claudefs_storage::scrub::{ScrubConfig, ScrubEngine, ScrubState};
    use claudefs_storage::write_journal::{JournalConfig, JournalOp, SyncMode, WriteJournal};

    // =========================================================================
    // A. Integrity Chain Security Tests
    // =========================================================================

    #[test]
    fn test_integrity_crc32_default_is_weak() {
        let config = IntegrityConfig::default();
        assert_ne!(
            config.default_algorithm,
            IntegrityAlgorithm::Blake3,
            "Security: Default integrity algorithm should use CRC32 (weak) not Blake3 (strong) for performance"
        );
        assert_eq!(config.default_algorithm, IntegrityAlgorithm::Crc32);
    }

    #[test]
    fn test_integrity_chain_expired_ttl_zero() {
        let config = IntegrityConfig {
            chain_ttl_seconds: 0,
            ..Default::default()
        };
        let mut manager = IntegrityManager::new(config);

        let chain = manager
            .create_chain("data-expired".to_string(), Some(0))
            .unwrap();

        assert!(
            chain.expires_at <= chain.created_at,
            "Security: Chain with TTL=0 should expire immediately (expires_at <= created_at)"
        );
    }

    #[test]
    fn test_integrity_checksum_mismatch_detected() {
        let mut manager = IntegrityManager::new(IntegrityConfig::default());

        let chain = manager.create_chain("data-test".to_string(), None).unwrap();

        let original_data = b"original data content";
        let checksum = manager
            .compute_checksum(original_data, &IntegrityAlgorithm::Crc32)
            .unwrap();

        let point = VerificationPoint {
            stage: PipelineStage::ClientWrite,
            checksum,
            algorithm: IntegrityAlgorithm::Crc32,
            timestamp: 1000000,
            data_length: original_data.len() as u64,
            verified: false,
        };

        manager.add_point(&chain.id, point).unwrap();

        let wrong_data = b"modified data content";
        let result = manager
            .verify_point(&chain.id, PipelineStage::ClientWrite, wrong_data)
            .unwrap();

        assert!(
            matches!(result, VerificationResult::Invalid { .. }),
            "Security: Checksum mismatch must be detected and return Invalid result"
        );
    }

    #[test]
    fn test_integrity_gc_removes_expired_chains() {
        let config = IntegrityConfig {
            chain_ttl_seconds: 0,
            ..Default::default()
        };
        let mut manager = IntegrityManager::new(config);

        manager.create_chain("data-1".to_string(), Some(0)).unwrap();
        manager.create_chain("data-2".to_string(), Some(0)).unwrap();
        manager.create_chain("data-3".to_string(), Some(0)).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(100));

        let removed = manager.gc_expired_chains().unwrap();
        assert_eq!(
            removed, 3,
            "Security: GC must remove all expired chains to prevent stale integrity data"
        );
        assert_eq!(manager.chain_count(), 0);
    }

    #[test]
    fn test_integrity_verify_nonexistent_chain() {
        let manager = IntegrityManager::new(IntegrityConfig::default());

        let result = manager
            .verify_point("nonexistent-chain-id", PipelineStage::ClientWrite, b"data")
            .unwrap();

        assert!(
            matches!(result, VerificationResult::ChainNotFound { .. }),
            "Security: Verifying non-existent chain must return ChainNotFound, not panic or return Valid"
        );
    }

    // =========================================================================
    // B. Atomic Write Security Tests
    // =========================================================================

    #[test]
    fn test_atomic_write_unsupported_capability() {
        let cap = AtomicWriteCapability::unsupported();

        assert!(
            !cap.supported,
            "Security: Unsupported capability must have supported=false"
        );
        assert!(
            !cap.can_atomic_write(4096),
            "Security: Unsupported capability must reject all atomic writes"
        );
        assert!(
            !cap.can_atomic_write(1),
            "Security: Unsupported capability must reject even tiny writes"
        );
    }

    #[test]
    fn test_atomic_write_stats_overflow_resilience() {
        let mut stats = AtomicWriteStats::default();

        for _ in 0..1000 {
            stats.submitted();
            stats.completed(4096);
        }

        assert_eq!(stats.atomic_writes_submitted, 1000);
        assert_eq!(stats.atomic_writes_completed, 1000);
        assert_eq!(
            stats.bytes_written_atomic,
            4096 * 1000,
            "Security: Stats must accumulate correctly without overflow for 1000 iterations"
        );
    }

    #[test]
    fn test_atomic_write_zero_size_request() {
        let cap = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };

        assert!(
            !cap.can_atomic_write(0),
            "Security: Zero-size atomic write must be rejected to prevent empty write attacks"
        );
    }

    #[test]
    fn test_atomic_write_exceeds_max_atomic_bytes() {
        let cap = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };

        assert!(
            !cap.can_atomic_write(8192),
            "Security: Writes exceeding max_atomic_write_bytes must be rejected"
        );
        assert!(
            !cap.can_atomic_write(4097),
            "Security: Even 1 byte over the limit must be rejected"
        );
    }

    #[test]
    fn test_atomic_write_batch_with_unsupported() {
        let cap = AtomicWriteCapability::unsupported();
        let mut batch = AtomicWriteBatch::new(cap);

        let block_ref = BlockRef {
            id: BlockId::new(0, 100),
            size: BlockSize::B4K,
        };

        let result = batch.validate();
        assert!(
            result.is_err(),
            "Security: Batch validate must fail when capability is unsupported"
        );
    }

    // =========================================================================
    // C. Recovery Security Tests
    // =========================================================================

    #[test]
    fn test_bitmap_truncated_data_acceptance() {
        let truncated_data = vec![0xFF, 0xAA];
        let bitmap = AllocatorBitmap::from_bytes(&truncated_data, 1000).unwrap();

        assert_eq!(
            bitmap.allocated_count(),
            8 + 4,
            "Security: Truncated bitmap data is padded with zeros and accepted"
        );
    }

    #[test]
    fn test_bitmap_out_of_range_allocation() {
        let mut bitmap = AllocatorBitmap::new(100);

        bitmap.set_allocated(200, 10);

        assert!(
            !bitmap.is_allocated(200),
            "Security: Out-of-range allocation must be silently ignored (no panic, no corruption)"
        );
        assert_eq!(
            bitmap.allocated_count(),
            0,
            "Security: Out-of-range allocation must not affect allocated count"
        );
    }

    #[test]
    fn test_bitmap_allocated_free_roundtrip() {
        let mut bitmap = AllocatorBitmap::new(100);

        bitmap.set_allocated(10, 5);
        assert!(bitmap.is_allocated(10));
        assert!(bitmap.is_allocated(14));
        assert_eq!(bitmap.allocated_count(), 5);

        bitmap.set_free(10, 5);
        assert!(!bitmap.is_allocated(10));
        assert!(!bitmap.is_allocated(14));
        assert_eq!(
            bitmap.allocated_count(),
            0,
            "Security: Free must correctly reverse allocation for consistent state"
        );
    }

    #[test]
    fn test_recovery_config_defaults_secure() {
        let config = RecoveryConfig::default();

        assert!(
            config.verify_checksums,
            "Security: Default RecoveryConfig must have verify_checksums=true to detect corruption"
        );
        assert!(
            !config.allow_partial_recovery,
            "Security: Default RecoveryConfig must have allow_partial_recovery=false for strict recovery"
        );
    }

    #[test]
    fn test_recovery_phase_transitions_correct() {
        let state = RecoveryState::default();

        assert_eq!(
            state.phase,
            RecoveryPhase::NotStarted,
            "Security: RecoveryPhase must start at NotStarted to ensure clean recovery state"
        );
    }

    // =========================================================================
    // D. Write Journal Security Tests
    // =========================================================================

    #[test]
    fn test_journal_append_returns_incrementing_sequences() {
        let mut journal = WriteJournal::new(JournalConfig::default());

        let seq1 = journal
            .append(
                JournalOp::Write {
                    data: vec![0u8; 100],
                },
                1,
                0,
            )
            .unwrap();
        let seq2 = journal
            .append(
                JournalOp::Write {
                    data: vec![1u8; 100],
                },
                1,
                100,
            )
            .unwrap();
        let seq3 = journal
            .append(JournalOp::Truncate { new_size: 1000 }, 2, 0)
            .unwrap();

        assert!(
            seq1 < seq2,
            "Security: Sequence numbers must be strictly increasing"
        );
        assert!(
            seq2 < seq3,
            "Security: Sequence numbers must be strictly increasing across operation types"
        );
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
        assert_eq!(seq3, 3);
    }

    #[test]
    fn test_journal_commit_advances_committed_sequence() {
        let mut journal = WriteJournal::new(JournalConfig::default());

        journal
            .append(
                JournalOp::Write {
                    data: vec![0u8; 100],
                },
                1,
                0,
            )
            .unwrap();
        journal
            .append(
                JournalOp::Write {
                    data: vec![1u8; 100],
                },
                1,
                100,
            )
            .unwrap();

        assert_eq!(journal.committed_sequence(), 0);

        let committed_seq = journal.commit().unwrap();
        assert_eq!(
            committed_seq, 2,
            "Security: Commit must advance committed_sequence to latest appended"
        );
        assert_eq!(journal.committed_sequence(), 2);
    }

    #[test]
    fn test_journal_entries_since_zero_returns_all() {
        let mut journal = WriteJournal::new(JournalConfig::default());

        journal
            .append(
                JournalOp::Write {
                    data: vec![0u8; 50],
                },
                1,
                0,
            )
            .unwrap();
        journal
            .append(
                JournalOp::Write {
                    data: vec![1u8; 50],
                },
                1,
                50,
            )
            .unwrap();
        journal.append(JournalOp::Mkdir, 2, 0).unwrap();

        let entries = journal.entries_since(0);
        assert_eq!(
            entries.len(),
            3,
            "Security: entries_since(0) must return all appended entries for complete audit trail"
        );
    }

    #[test]
    fn test_journal_truncate_removes_old_entries() {
        let mut journal = WriteJournal::new(JournalConfig::default());

        for i in 0..5 {
            journal
                .append(
                    JournalOp::Write {
                        data: vec![i as u8; 100],
                    },
                    1,
                    i * 100,
                )
                .unwrap();
        }

        journal.commit().unwrap();

        let removed = journal.truncate_before(3).unwrap();
        assert_eq!(
            removed, 2,
            "Security: Truncate must remove entries with sequence < threshold"
        );

        let entries = journal.entries_since(0);
        assert_eq!(
            entries.len(),
            3,
            "Security: Truncated entries must not be accessible via entries_since"
        );
    }

    #[test]
    fn test_journal_verify_entry_detects_corruption() {
        let mut journal = WriteJournal::new(JournalConfig::default());

        let data = b"original data for integrity check";
        journal
            .append(
                JournalOp::Write {
                    data: data.to_vec(),
                },
                1,
                0,
            )
            .unwrap();

        let entries = journal.entries_since(1);
        let original_entry = entries[0];
        assert!(
            journal.verify_entry(original_entry),
            "Security: Original entry must pass verification"
        );

        let mut corrupted_entry = original_entry.clone();
        corrupted_entry.data_checksum = Checksum::new(ChecksumAlgorithm::Crc32c, 0xDEADBEEF);

        assert!(
            !journal.verify_entry(&corrupted_entry),
            "Security: Modified checksum must be detected as corruption"
        );
    }

    // =========================================================================
    // E. Scrub and Hot Swap Security Tests
    // =========================================================================

    #[test]
    fn test_scrub_verify_block_corrupted_data() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        let block_ref = BlockRef {
            id: BlockId::new(0, 100),
            size: BlockSize::B4K,
        };

        let original_data = b"this is valid data";
        let checksum =
            claudefs_storage::checksum::compute(ChecksumAlgorithm::Crc32c, original_data);

        let corrupted_data = b"this is CORRUPTED!";
        let is_valid = engine.verify_block(block_ref, corrupted_data, &checksum);

        assert!(
            !is_valid,
            "Security: Corrupted data must be detected during scrub verification"
        );
        assert_eq!(
            engine.errors().len(),
            1,
            "Security: Scrub errors list must record the corruption"
        );
    }

    #[test]
    fn test_scrub_state_machine_idle_to_running() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        engine.schedule_block(
            BlockRef {
                id: BlockId::new(0, 1),
                size: BlockSize::B4K,
            },
            Checksum::new(ChecksumAlgorithm::Crc32c, 12345),
        );

        assert!(matches!(engine.state(), ScrubState::Idle));

        engine.start();

        assert!(
            matches!(engine.state(), ScrubState::Running { .. }),
            "Security: Scrub state machine must transition Idle -> Running on start()"
        );
    }

    #[test]
    fn test_hot_swap_invalid_state_transition() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let result = manager.complete_drain(0);
        assert!(
            result.is_err(),
            "Security: complete_drain on Active device must fail (invalid state transition)"
        );

        assert_eq!(manager.device_state(0), Some(DeviceState::Active));
    }

    #[test]
    fn test_hot_swap_fail_device_any_state() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();

        let result = manager.fail_device(0, "test failure in Initializing".to_string());
        assert!(
            result.is_ok(),
            "Security: fail_device must work from Initializing state"
        );
        assert_eq!(manager.device_state(0), Some(DeviceState::Failed));

        manager
            .register_device(1, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(1).unwrap();

        let result = manager.fail_device(1, "test failure in Active".to_string());
        assert!(
            result.is_ok(),
            "Security: fail_device must work from Active state"
        );
        assert_eq!(manager.device_state(1), Some(DeviceState::Failed));

        manager
            .register_device(2, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(2).unwrap();
        manager.start_drain(2, vec![]).unwrap();

        let result = manager.fail_device(2, "test failure in Draining".to_string());
        assert!(
            result.is_ok(),
            "Security: fail_device must work from Draining state"
        );
        assert_eq!(manager.device_state(2), Some(DeviceState::Failed));
    }

    #[test]
    fn test_drain_progress_overcounting() {
        let mut progress = DrainProgress::new(0, 100);

        progress.record_migrated(50);
        assert!((progress.progress_pct() - 50.0).abs() < 0.01);

        progress.record_migrated(100);
        assert!((progress.progress_pct() - 150.0).abs() < 0.01);

        assert!(
            progress.is_complete(),
            "Security: Drain progress can exceed 100% - overcounting allowed but still marks complete"
        );
    }
}
