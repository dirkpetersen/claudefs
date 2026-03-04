//! Replication active-active and conduit security tests.
//!
//! Part of A10 Phase 28: Repl active-active + conduit security audit

#[cfg(test)]
mod tests {
    use claudefs_repl::active_active::{
        ActiveActiveController, ActiveActiveStats, ForwardedWrite, LinkStatus,
        SiteRole, WriteConflict,
    };
    use claudefs_repl::conduit::{
        Conduit, ConduitConfig, ConduitState, ConduitStats, ConduitTlsConfig, EntryBatch,
    };
    use claudefs_repl::journal::{JournalEntry, OpKind};

    fn make_entry(seq: u64, inode: u64) -> JournalEntry {
        JournalEntry::new(seq, 0, 1, 1000 + seq, inode, OpKind::Write, vec![1, 2, 3])
    }

    // ============================================================================
    // Category 1: Conflict Resolution Integrity (5 tests)
    // ============================================================================

    #[test]
    fn test_repl_ac_sec_conflict_deterministic_winner() {
        let mut controller_a = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let mut controller_b = ActiveActiveController::new("site-b".to_string(), SiteRole::Secondary);

        controller_a.local_write(b"key".to_vec(), b"value_a".to_vec());
        controller_b.local_write(b"key".to_vec(), b"value_b".to_vec());

        let fw_b = controller_b.drain_pending()[0].clone();
        let conflict_a = controller_a.apply_remote_write(fw_b);

        assert!(conflict_a.is_some());
        let c = conflict_a.unwrap();
        assert_eq!(c.local_time, 1);
        assert_eq!(c.remote_time, 1);
        assert_eq!(c.winner, SiteRole::Primary);
    }

    #[test]
    fn test_repl_ac_sec_conflict_same_site_id_determinism() {
        let mut controller_1 = ActiveActiveController::new("site-x".to_string(), SiteRole::Primary);
        let mut controller_2 = ActiveActiveController::new("site-x".to_string(), SiteRole::Secondary);

        controller_1.local_write(b"key".to_vec(), b"v1".to_vec());
        controller_2.local_write(b"key".to_vec(), b"v2".to_vec());

        let fw_2 = controller_2.drain_pending()[0].clone();
        let conflict = controller_1.apply_remote_write(fw_2);

        assert!(conflict.is_some());
        let c = conflict.unwrap();
        assert_eq!(c.winner, SiteRole::Primary);
    }

    #[test]
    fn test_repl_ac_sec_conflict_zero_logical_time() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 0,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };

        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_some());
        let c = conflict.unwrap();
        assert_eq!(c.local_time, 0);
        assert_eq!(c.remote_time, 0);
    }

    #[test]
    fn test_repl_ac_sec_logical_time_max_advance() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 100,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };

        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_none());
        assert_eq!(controller.stats().writes_forwarded, 0);
    }

    #[test]
    fn test_repl_ac_sec_conflict_empty_key_value() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.local_write(b"".to_vec(), b"".to_vec());

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 1,
            key: vec![],
            value: vec![],
        };

        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_some());
        let c = conflict.unwrap();
        assert!(c.key.is_empty());
    }

    // ============================================================================
    // Category 2: Logical Clock Manipulation (4 tests)
    // ============================================================================

    #[test]
    fn test_repl_ac_sec_local_write_increments_from_zero() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.stats().writes_forwarded, 0);

        let fw = controller.local_write(b"key".to_vec(), b"value".to_vec());
        assert_eq!(fw.logical_time, 1);
        assert_eq!(controller.stats().writes_forwarded, 1);
    }

    #[test]
    fn test_repl_ac_sec_many_writes_monotonic() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let mut prev_time = 0u64;

        for i in 0..100 {
            let fw = controller.local_write(format!("key{}", i).into_bytes(), b"value".to_vec());
            assert!(fw.logical_time > prev_time);
            prev_time = fw.logical_time;
        }

        assert_eq!(controller.stats().writes_forwarded, 100);
    }

    #[test]
    fn test_repl_ac_sec_max_logical_time_no_overflow() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: u64::MAX - 10,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };

        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_none());
    }

    #[test]
    fn test_repl_ac_sec_remote_from_future_advances_clock() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);

        controller.local_write(b"k1".to_vec(), b"v1".to_vec());
        controller.local_write(b"k2".to_vec(), b"v2".to_vec());

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 1000,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };

        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_none());
    }

    // ============================================================================
    // Category 3: Link Status Flap Detection (4 tests)
    // ============================================================================

    #[test]
    fn test_repl_ac_sec_rapid_flap_cycles() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);

        controller.set_link_status(LinkStatus::Up);
        controller.set_link_status(LinkStatus::Down);
        controller.set_link_status(LinkStatus::Up);
        controller.set_link_status(LinkStatus::Down);
        controller.set_link_status(LinkStatus::Up);

        assert_eq!(controller.stats().link_flaps, 3);
    }

    #[test]
    fn test_repl_ac_sec_degraded_to_up_counts_flap() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);

        controller.set_link_status(LinkStatus::Degraded);
        assert_eq!(controller.stats().link_flaps, 0);

        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 1);
    }

    #[test]
    fn test_repl_ac_sec_down_to_down_no_flap() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);

        controller.set_link_status(LinkStatus::Down);
        assert_eq!(controller.stats().link_flaps, 0);

        controller.set_link_status(LinkStatus::Down);
        assert_eq!(controller.stats().link_flaps, 0);
    }

    #[test]
    fn test_repl_ac_sec_up_to_up_no_flap() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);

        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 1);

        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 1);
    }

    // ============================================================================
    // Category 4: Conduit Channel Isolation (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_ac_sec_send_on_shutdown_conduit_errors() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, _) = Conduit::new_pair(config_a, config_b);
        conduit_a.shutdown().await;

        let batch = EntryBatch::new(1, vec![], 1);
        let result = conduit_a.send_batch(batch).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_repl_ac_sec_send_receive_empty_batch() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let batch = EntryBatch::new(1, vec![], 0);
        conduit_a.send_batch(batch).await.unwrap();

        let received = conduit_b.recv_batch().await.unwrap();
        assert_eq!(received.entries.len(), 0);
        assert_eq!(received.batch_seq, 0);
    }

    #[tokio::test]
    async fn test_repl_ac_sec_large_entry_count_batch() {
        let config_a = ConduitConfig {
            max_batch_size: 2000,
            ..ConduitConfig::new(1, 2)
        };
        let config_b = ConduitConfig {
            max_batch_size: 2000,
            ..ConduitConfig::new(2, 1)
        };

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let entries: Vec<_> = (0..1000).map(|i| make_entry(i, i)).collect();
        let batch = EntryBatch::new(1, entries, 1);

        conduit_a.send_batch(batch).await.unwrap();

        let received = conduit_b.recv_batch().await.unwrap();
        assert_eq!(received.entries.len(), 1000);
    }

    #[tokio::test]
    async fn test_repl_ac_sec_conduit_stats_after_multiple_sends() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        for i in 0..5u64 {
            let entries = vec![make_entry(i, i)];
            conduit_a.send_batch(EntryBatch::new(1, entries, i)).await.unwrap();
        }

        for _ in 0..5 {
            let _ = conduit_b.recv_batch().await;
        }

        let stats = conduit_a.stats();
        assert_eq!(stats.batches_sent, 5);
        assert_eq!(stats.entries_sent, 5);

        let stats_b = conduit_b.stats();
        assert_eq!(stats_b.batches_received, 5);
        assert_eq!(stats_b.entries_received, 5);
    }

    #[tokio::test]
    async fn test_repl_ac_sec_stats_zero_initially() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let stats_a = conduit_a.stats();
        assert_eq!(stats_a.batches_sent, 0);
        assert_eq!(stats_a.batches_received, 0);
        assert_eq!(stats_a.entries_sent, 0);
        assert_eq!(stats_a.entries_received, 0);
        assert_eq!(stats_a.send_errors, 0);
        assert_eq!(stats_a.reconnects, 0);

        let stats_b = conduit_b.stats();
        assert_eq!(stats_b.batches_sent, 0);
        assert_eq!(stats_b.batches_received, 0);
    }

    // ============================================================================
    // Category 5: TLS Config Edge Cases (4 tests)
    // ============================================================================

    #[test]
    fn test_repl_ac_sec_tls_config_empty_fields() {
        let tls = ConduitTlsConfig::new(vec![], vec![], vec![]);

        assert!(tls.cert_pem.is_empty());
        assert!(tls.key_pem.is_empty());
        assert!(tls.ca_pem.is_empty());
    }

    #[test]
    fn test_repl_ac_sec_conduit_config_defaults() {
        let config = ConduitConfig::default();

        assert_eq!(config.local_site_id, 0);
        assert_eq!(config.remote_site_id, 0);
        assert!(config.remote_addrs.is_empty());
        assert!(config.tls.is_none());
        assert_eq!(config.max_batch_size, 1000);
        assert_eq!(config.reconnect_delay_ms, 100);
        assert_eq!(config.max_reconnect_delay_ms, 30000);
    }

    #[test]
    fn test_repl_ac_sec_conduit_config_max_batch_size_edge() {
        let config_small = ConduitConfig {
            max_batch_size: 1,
            ..ConduitConfig::default()
        };
        assert_eq!(config_small.max_batch_size, 1);

        let config_large = ConduitConfig {
            max_batch_size: usize::MAX,
            ..ConduitConfig::default()
        };
        assert_eq!(config_large.max_batch_size, usize::MAX);
    }

    #[test]
    fn test_repl_ac_sec_conduit_config_zero_reconnect_delay() {
        let config = ConduitConfig {
            reconnect_delay_ms: 0,
            ..ConduitConfig::default()
        };

        assert_eq!(config.reconnect_delay_ms, 0);
        assert_eq!(config.max_reconnect_delay_ms, 30000);
    }

    // ============================================================================
    // Category 6: Batch Integrity (6 tests)
    // ============================================================================

    #[test]
    fn test_repl_ac_sec_batch_seq_zero() {
        let entry = make_entry(1, 100);
        let batch = EntryBatch::new(1, vec![entry], 0);

        assert_eq!(batch.batch_seq, 0);
        assert_eq!(batch.source_site_id, 1);
    }

    #[test]
    fn test_repl_ac_sec_batch_seq_max() {
        let entry = make_entry(1, 100);
        let batch = EntryBatch::new(1, vec![entry], u64::MAX);

        assert_eq!(batch.batch_seq, u64::MAX);
    }

    #[test]
    fn test_repl_ac_sec_entry_batch_serialization_roundtrip() {
        let entries = vec![
            make_entry(1, 100),
            make_entry(2, 200),
            make_entry(3, 300),
        ];
        let batch = EntryBatch::new(42, entries, 12345);

        let serialized = bincode::serialize(&batch).unwrap();
        let deserialized: EntryBatch = bincode::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.source_site_id, 42);
        assert_eq!(deserialized.batch_seq, 12345);
        assert_eq!(deserialized.entries.len(), 3);
        assert_eq!(deserialized.entries[0].seq, 1);
        assert_eq!(deserialized.entries[1].seq, 2);
        assert_eq!(deserialized.entries[2].seq, 3);
    }

    #[test]
    fn test_repl_ac_sec_conduit_state_reconnecting_max_attempt() {
        let state = ConduitState::Reconnecting {
            attempt: u32::MAX,
            delay_ms: 1000,
        };

        match state {
            ConduitState::Reconnecting { attempt, delay_ms } => {
                assert_eq!(attempt, u32::MAX);
                assert_eq!(delay_ms, 1000);
            }
            _ => panic!("expected Reconnecting state"),
        }
    }

    #[tokio::test]
    async fn test_repl_ac_sec_bidirectional_send_receive() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let batch_a = EntryBatch::new(1, vec![make_entry(1, 100)], 1);
        let batch_b = EntryBatch::new(2, vec![make_entry(10, 200)], 1);

        conduit_a.send_batch(batch_a.clone()).await.unwrap();
        conduit_b.send_batch(batch_b.clone()).await.unwrap();

        let recv_by_b = conduit_b.recv_batch().await.unwrap();
        assert_eq!(recv_by_b.source_site_id, 1);

        let recv_by_a = conduit_a.recv_batch().await.unwrap();
        assert_eq!(recv_by_a.source_site_id, 2);
    }

    #[tokio::test]
    async fn test_repl_ac_sec_sequential_batches_maintain_order() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        for seq in 1..=10u64 {
            let entries = vec![make_entry(seq, seq * 100)];
            conduit_a.send_batch(EntryBatch::new(1, entries, seq)).await.unwrap();
        }

        for expected_seq in 1..=10u64 {
            let received = conduit_b.recv_batch().await.unwrap();
            assert_eq!(received.batch_seq, expected_seq);
            assert_eq!(received.entries[0].seq, expected_seq);
        }
    }
}