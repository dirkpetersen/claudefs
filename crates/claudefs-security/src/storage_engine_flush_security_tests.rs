//! Storage engine and write journal security tests.
//!
//! Part of A10 Phase 28: Storage engine + flush security audit

#[cfg(test)]
mod tests {
    use claudefs_storage::block::{BlockId, BlockRef, BlockSize, PlacementHint};
    use claudefs_storage::device::DeviceRole;
    use claudefs_storage::engine::{StorageEngine, StorageEngineConfig, StorageEngineStats};
    use claudefs_storage::error::StorageError;
    use claudefs_storage::flush::{
        JournalConfig, JournalEntry, JournalEntryState, JournalStats, WriteJournal,
    };
    use claudefs_storage::io_uring_bridge::MockIoEngine;

    fn test_block_ref() -> BlockRef {
        BlockRef {
            id: BlockId::new(0, 100),
            size: BlockSize::B4K,
        }
    }

    fn make_engine() -> StorageEngine<MockIoEngine> {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        StorageEngine::new(config, mock_io)
    }

    fn make_engine_with_device(capacity_4k: u64) -> StorageEngine<MockIoEngine> {
        let config = StorageEngineConfig::default();
        let mock_io = MockIoEngine::new();
        let mut engine = StorageEngine::new(config, mock_io);
        engine.add_mock_device(0, DeviceRole::Data, capacity_4k).unwrap();
        engine
    }

    // =========================================================================
    // 1. Storage Engine Allocation Security (5 tests)
    // =========================================================================

    #[tokio::test]
    async fn test_stor_ef_sec_allocate_empty_engine() {
        let engine = make_engine();

        let result = engine.allocate(BlockSize::B4K, None);
        assert!(
            matches!(result, Err(StorageError::OutOfSpace)),
            "Security: Allocating from empty engine must return OutOfSpace, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_stor_ef_sec_allocate_preferred_role_nonexistent() {
        let mut engine = make_engine();
        engine.add_mock_device(0, DeviceRole::Data, 1024).unwrap();

        let result = engine.allocate(BlockSize::B4K, Some(DeviceRole::Journal));
        assert!(
            result.is_ok(),
            "Security: Allocating with nonexistent preferred role should fall back to any device"
        );
        let block_ref = result.unwrap();
        assert_eq!(block_ref.id.device_idx, 0);
    }

    #[tokio::test]
    async fn test_stor_ef_sec_allocate_until_out_of_space() {
        let mut engine = make_engine();
        engine.add_mock_device(0, DeviceRole::Data, 2).unwrap();

        let _block1 = engine.allocate(BlockSize::B4K, None).unwrap();
        let _block2 = engine.allocate(BlockSize::B4K, None).unwrap();

        let result = engine.allocate(BlockSize::B4K, None);
        assert!(
            matches!(result, Err(StorageError::OutOfSpace)),
            "Security: Must return OutOfSpace when capacity exhausted"
        );
    }

    #[tokio::test]
    async fn test_stor_ef_sec_free_nonexistent_device() {
        let engine = make_engine();

        let block_ref = BlockRef {
            id: BlockId::new(999, 0),
            size: BlockSize::B4K,
        };

        let result = engine.free(block_ref);
        assert!(
            matches!(result, Err(StorageError::DeviceError { .. })),
            "Security: Freeing block from non-existent device must error"
        );
    }

    #[tokio::test]
    async fn test_stor_ef_sec_double_free_no_protection() {
        let mut engine = make_engine();
        engine.add_mock_device(0, DeviceRole::Data, 10).unwrap();

        let block_ref = engine.allocate(BlockSize::B4K, None).unwrap();

        engine.free(block_ref).unwrap();

        let result = engine.free(block_ref);
        assert!(
            result.is_ok(),
            "Security GAP: Allocator does not detect double-free - accepted second free silently"
        );
    }

    // =========================================================================
    // 2. Storage Engine Config Defaults (3 tests)
    // =========================================================================

    #[test]
    fn test_stor_ef_sec_default_config_values() {
        let config = StorageEngineConfig::default();

        assert_eq!(config.name, "claudefs-storage");
        assert_eq!(config.default_placement, PlacementHint::HotData);
        assert!(config.verify_checksums, "Security: Default should verify checksums");
        assert!(config.direct_io, "Security: Default should use direct I/O");
    }

    #[test]
    fn test_stor_ef_sec_stats_empty_engine() {
        let engine = make_engine();
        let stats = engine.stats();

        assert_eq!(stats.device_count, 0);
        assert_eq!(stats.total_capacity_bytes, 0);
        assert_eq!(stats.free_capacity_bytes, 0);
        assert_eq!(stats.io_stats.reads_completed, 0);
        assert_eq!(stats.io_stats.writes_completed, 0);
    }

    #[test]
    fn test_stor_ef_sec_engine_custom_config_name() {
        let config = StorageEngineConfig {
            name: "custom-security-engine".to_string(),
            ..Default::default()
        };
        let mock_io = MockIoEngine::new();
        let engine: StorageEngine<MockIoEngine> = StorageEngine::new(config, mock_io);

        assert_eq!(engine.config().name, "custom-security-engine");
    }

    // =========================================================================
    // 3. Journal State Machine Violations (5 tests)
    // =========================================================================

    #[test]
    fn test_stor_ef_sec_commit_pending_entry() {
        let journal = WriteJournal::new(JournalConfig::default());

        let seq = journal
            .append(test_block_ref(), vec![0u8; 4096], PlacementHint::Journal)
            .unwrap();

        let result = journal.commit(seq);
        assert!(
            result.is_err(),
            "Security: Cannot commit Pending entry - must be LocalFlushed or Replicated first"
        );
    }

    #[test]
    fn test_stor_ef_sec_mark_local_flushed_nonexistent_sequence() {
        let journal = WriteJournal::new(JournalConfig::default());

        let result = journal.mark_local_flushed(99999);
        assert!(
            result.is_err(),
            "Security: Marking non-existent sequence as LocalFlushed must error"
        );
    }

    #[test]
    fn test_stor_ef_sec_mark_replicated_nonexistent_sequence() {
        let journal = WriteJournal::new(JournalConfig::default());

        let result = journal.mark_replicated(99999);
        assert!(
            result.is_err(),
            "Security: Marking non-existent sequence as Replicated must error"
        );
    }

    #[test]
    fn test_stor_ef_sec_commit_nonexistent_sequence() {
        let journal = WriteJournal::new(JournalConfig::default());

        let result = journal.commit(99999);
        assert!(
            result.is_err(),
            "Security: Committing non-existent sequence must error"
        );
    }

    #[test]
    fn test_stor_ef_sec_double_commit_same_sequence() {
        let journal = WriteJournal::new(JournalConfig::default());

        let seq = journal
            .append(test_block_ref(), vec![0u8; 4096], PlacementHint::Journal)
            .unwrap();

        journal.mark_local_flushed(seq).unwrap();
        journal.commit(seq).unwrap();

        let result = journal.commit(seq);
        assert!(
            result.is_err(),
            "Security: Double-commit of same sequence must be rejected"
        );
    }

    // =========================================================================
    // 4. Journal Flush Thresholds (4 tests)
    // =========================================================================

    #[test]
    fn test_stor_ef_sec_needs_flush_when_empty() {
        let journal = WriteJournal::new(JournalConfig::default());

        assert!(
            !journal.needs_flush(),
            "Security: Empty journal should not need flush"
        );
    }

    #[test]
    fn test_stor_ef_sec_needs_flush_entries_threshold() {
        let config = JournalConfig::new(3, u64::MAX, false);
        let journal = WriteJournal::new(config);

        for i in 0..3 {
            let block_ref = BlockRef {
                id: BlockId::new(0, i),
                size: BlockSize::B4K,
            };
            journal.append(block_ref, vec![0u8; 4096], PlacementHint::Journal).unwrap();
        }

        assert!(
            journal.needs_flush(),
            "Security: Journal must need flush when max_pending_entries exceeded"
        );
    }

    #[test]
    fn test_stor_ef_sec_needs_flush_bytes_threshold() {
        let config = JournalConfig::new(usize::MAX, 8192, false);
        let journal = WriteJournal::new(config);

        journal
            .append(test_block_ref(), vec![0u8; 4096], PlacementHint::Journal)
            .unwrap();

        assert!(!journal.needs_flush(), "Should not need flush yet");

        journal
            .append(test_block_ref(), vec![0u8; 4096], PlacementHint::Journal)
            .unwrap();

        assert!(
            journal.needs_flush(),
            "Security: Journal must need flush when max_pending_bytes exceeded"
        );
    }

    #[test]
    fn test_stor_ef_sec_journal_threshold_one_entry() {
        let config = JournalConfig::new(1, u64::MAX, false);
        let journal = WriteJournal::new(config);

        assert!(!journal.needs_flush());

        journal
            .append(test_block_ref(), vec![0u8; 4096], PlacementHint::Journal)
            .unwrap();

        assert!(
            journal.needs_flush(),
            "Security: Journal with max_pending_entries=1 must trigger flush after 1 entry"
        );
    }

    // =========================================================================
    // 5. Journal Ordering and Sequence (5 tests)
    // =========================================================================

    #[test]
    fn test_stor_ef_sec_sequences_monotonic_increasing() {
        let journal = WriteJournal::new(JournalConfig::default());

        let seq1 = journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();
        let seq2 = journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();
        let seq3 = journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();

        assert!(seq1 < seq2, "Security: Sequences must be monotonically increasing");
        assert!(seq2 < seq3, "Security: Sequences must be monotonically increasing");

        let stats = journal.stats();
        assert_eq!(stats.current_sequence, seq3);
    }

    #[test]
    fn test_stor_ef_sec_out_of_order_commits() {
        let journal = WriteJournal::new(JournalConfig::default());

        let seq1 = journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();
        let seq2 = journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();

        journal.mark_local_flushed(seq2).unwrap();
        journal.commit(seq2).unwrap();

        let pending = journal.pending_entries();
        assert_eq!(pending.len(), 1, "seq1 must still be pending");

        let stats = journal.stats();
        assert_eq!(stats.committed_count, 1);
    }

    #[test]
    fn test_stor_ef_sec_committed_removed_from_front_only() {
        let journal = WriteJournal::new(JournalConfig::default());

        let seq1 = journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();
        let seq2 = journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();
        let seq3 = journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();

        journal.mark_local_flushed(seq3).unwrap();
        journal.commit(seq3).unwrap();

        assert_eq!(journal.pending_count(), 2, "seq1 and seq2 must remain pending (seq3 committed but not at front)");

        journal.mark_local_flushed(seq2).unwrap();
        journal.commit(seq2).unwrap();

        assert_eq!(journal.pending_count(), 1, "seq1 still pending, seq2 and seq3 committed but blocked by seq1");

        journal.mark_local_flushed(seq1).unwrap();
        journal.commit(seq1).unwrap();

        assert_eq!(journal.pending_count(), 0, "All committed entries removed from front");
    }

    #[test]
    fn test_stor_ef_sec_high_watermark_tracking() {
        let journal = WriteJournal::new(JournalConfig::default());

        for _ in 0..5 {
            journal
                .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
                .unwrap();
        }

        let stats = journal.stats();
        assert_eq!(stats.high_watermark, 5);

        journal.mark_local_flushed(1).unwrap();
        journal.commit(1).unwrap();

        let stats = journal.stats();
        assert_eq!(stats.high_watermark, 5, "Security: High watermark must track maximum pending seen");
    }

    #[test]
    fn test_stor_ef_sec_stats_pending_bytes_correct() {
        let journal = WriteJournal::new(JournalConfig::default());

        journal
            .append(test_block_ref(), vec![0u8; 1024], PlacementHint::Journal)
            .unwrap();
        journal
            .append(test_block_ref(), vec![0u8; 2048], PlacementHint::Journal)
            .unwrap();

        let stats = journal.stats();
        assert_eq!(stats.pending_bytes, 1024 + 2048);

        journal.mark_local_flushed(1).unwrap();
        journal.commit(1).unwrap();

        let stats = journal.stats();
        assert_eq!(stats.pending_bytes, 2048);
    }

    // =========================================================================
    // 6. Journal Entry Security (6 tests)
    // =========================================================================

    #[test]
    fn test_stor_ef_sec_append_zero_length_data() {
        let journal = WriteJournal::new(JournalConfig::default());

        let seq = journal
            .append(test_block_ref(), vec![], PlacementHint::Journal)
            .unwrap();

        let entries = journal.pending_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sequence, seq);
        assert_eq!(entries[0].data.len(), 0);
    }

    #[test]
    fn test_stor_ef_sec_append_large_data_1mb() {
        let journal = WriteJournal::new(JournalConfig::default());

        let large_data = vec![0xAB; 1024 * 1024];
        let seq = journal
            .append(test_block_ref(), large_data.clone(), PlacementHint::Journal)
            .unwrap();

        let entries = journal.pending_entries();
        assert_eq!(entries[0].sequence, seq);
        assert_eq!(entries[0].data, large_data);

        let stats = journal.stats();
        assert_eq!(stats.pending_bytes, 1024 * 1024);
    }

    #[test]
    fn test_stor_ef_sec_entry_timestamp_reasonable() {
        let journal = WriteJournal::new(JournalConfig::default());

        journal
            .append(test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
            .unwrap();

        let entries = journal.pending_entries();
        assert!(
            entries[0].timestamp_secs > 0,
            "Security: Entry timestamp must be non-zero"
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(
            entries[0].timestamp_secs <= now,
            "Security: Entry timestamp must not be in the future"
        );
    }

    #[test]
    fn test_stor_ef_sec_config_zero_max_entries() {
        let config = JournalConfig::new(0, u64::MAX, false);
        let journal = WriteJournal::new(config);

        assert!(
            journal.needs_flush(),
            "Security: Journal with max_pending_entries=0 must immediately need flush"
        );
    }

    #[test]
    fn test_stor_ef_sec_config_zero_max_bytes() {
        let config = JournalConfig::new(usize::MAX, 0, false);
        let journal = WriteJournal::new(config);

        assert!(
            journal.needs_flush(),
            "Security: Journal with max_pending_bytes=0 must immediately need flush"
        );
    }

    #[test]
    fn test_stor_ef_sec_replication_disabled_mark_replicated() {
        let config = JournalConfig::new(100, 100000, false);
        let journal = WriteJournal::new(config);

        let seq = journal
            .append(test_block_ref(), vec![0u8; 4096], PlacementHint::Journal)
            .unwrap();

        journal.mark_local_flushed(seq).unwrap();

        let result = journal.mark_replicated(seq);
        assert!(
            result.is_ok(),
            "Security: mark_replicated must succeed even when replication disabled (logs warning)"
        );

        let replicated = journal.pending_entries_by_state(JournalEntryState::Replicated);
        assert_eq!(replicated.len(), 1);
        assert_eq!(replicated[0].sequence, seq);
    }
}