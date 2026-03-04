//! Replication engine and fanout security tests.
//!
//! Part of A10 Phase 29: Repl engine + fanout security audit

#[cfg(test)]
mod tests {
    use claudefs_repl::conduit::{Conduit, ConduitConfig, EntryBatch};
    use claudefs_repl::engine::{EngineConfig, EngineState, ReplicationEngine, SiteReplicationStats};
    use claudefs_repl::fanout::{FanoutResult, FanoutSender, FanoutSummary};
    use claudefs_repl::journal::{JournalEntry, OpKind};
    use claudefs_repl::topology::{ReplicationRole, ReplicationTopology, SiteInfo};

    fn make_entry(seq: u64, inode: u64) -> JournalEntry {
        JournalEntry::new(seq, 0, 1, 1000 + seq, inode, OpKind::Write, vec![1, 2, 3])
    }

    fn make_site(site_id: u64) -> SiteInfo {
        SiteInfo::new(
            site_id,
            format!("region-{}", site_id),
            vec![format!("grpc://10.0.0.{}:50051", site_id)],
            ReplicationRole::Primary,
        )
    }

    fn make_conduit(local_id: u64, remote_id: u64) -> Conduit {
        let config_a = ConduitConfig::new(local_id, remote_id);
        let config_b = ConduitConfig::new(remote_id, local_id);
        let (a, _b) = Conduit::new_pair(config_a, config_b);
        a
    }

    fn make_engine(local_site_id: u64) -> ReplicationEngine {
        let topology = ReplicationTopology::new(local_site_id);
        let config = EngineConfig {
            local_site_id,
            ..EngineConfig::default()
        };
        ReplicationEngine::new(config, topology)
    }

    // ============================================================================
    // Category 1: Engine State Machine Security (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_ef_sec_initial_state_is_idle() {
        let engine = make_engine(1);
        assert_eq!(engine.state().await, EngineState::Idle);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_start_from_idle_to_running() {
        let engine = make_engine(1);
        engine.start().await;
        assert_eq!(engine.state().await, EngineState::Running);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_stop_from_running_to_stopped() {
        let engine = make_engine(1);
        engine.start().await;
        assert_eq!(engine.state().await, EngineState::Running);
        engine.stop().await;
        assert_eq!(engine.state().await, EngineState::Stopped);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_start_from_stopped_stays_stopped() {
        let engine = make_engine(1);
        engine.start().await;
        engine.stop().await;
        assert_eq!(engine.state().await, EngineState::Stopped);
        engine.start().await;
        assert_eq!(engine.state().await, EngineState::Stopped);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_stop_from_idle_to_stopped() {
        let engine = make_engine(1);
        assert_eq!(engine.state().await, EngineState::Idle);
        engine.stop().await;
        assert_eq!(engine.state().await, EngineState::Stopped);
    }

    // ============================================================================
    // Category 2: Engine Site Management (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_ef_sec_add_site_creates_stats_entry() {
        let engine = make_engine(1);
        let site = make_site(2);
        engine.add_site(site).await;

        let stats = engine.site_stats(2).await;
        assert!(stats.is_some());
        let s = stats.unwrap();
        assert_eq!(s.remote_site_id, 2);
        assert_eq!(s.entries_sent, 0);
        assert_eq!(s.entries_received, 0);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_remove_site_removes_stats() {
        let engine = make_engine(1);
        engine.add_site(make_site(2)).await;
        assert!(engine.site_stats(2).await.is_some());

        engine.remove_site(2).await;
        assert!(engine.site_stats(2).await.is_none());
    }

    #[tokio::test]
    async fn test_repl_ef_sec_add_10_sites_rapidly() {
        let engine = make_engine(1);

        for site_id in 2..=11u64 {
            engine.add_site(make_site(site_id)).await;
        }

        let all_stats = engine.all_site_stats().await;
        assert_eq!(all_stats.len(), 10);

        for site_id in 2..=11u64 {
            assert!(engine.site_stats(site_id).await.is_some());
        }
    }

    #[tokio::test]
    async fn test_repl_ef_sec_site_stats_nonexistent_returns_none() {
        let engine = make_engine(1);
        let stats = engine.site_stats(999).await;
        assert!(stats.is_none());
    }

    #[tokio::test]
    async fn test_repl_ef_sec_record_on_nonexistent_site_silently_ignored() {
        let engine = make_engine(1);

        engine.record_send(999, 100, 5).await;
        engine.record_receive(999, 50, 2).await;
        engine.record_conflict(999).await;
        engine.update_lag(999, 1000).await;

        let stats = engine.site_stats(999).await;
        assert!(stats.is_none());
    }

    // ============================================================================
    // Category 3: Engine Stats Accumulation (4 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_ef_sec_record_send_accumulates() {
        let engine = make_engine(1);
        engine.add_site(make_site(2)).await;

        engine.record_send(2, 100, 5).await;
        engine.record_send(2, 50, 3).await;
        engine.record_send(2, 25, 2).await;

        let stats = engine.site_stats(2).await.unwrap();
        assert_eq!(stats.entries_sent, 175);
        assert_eq!(stats.batches_sent, 10);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_record_receive_accumulates() {
        let engine = make_engine(1);
        engine.add_site(make_site(2)).await;

        engine.record_receive(2, 200, 4).await;
        engine.record_receive(2, 300, 6).await;

        let stats = engine.site_stats(2).await.unwrap();
        assert_eq!(stats.entries_received, 500);
        assert_eq!(stats.batches_received, 10);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_record_conflict_increments() {
        let engine = make_engine(1);
        engine.add_site(make_site(2)).await;

        for _ in 0..5 {
            engine.record_conflict(2).await;
        }

        let stats = engine.site_stats(2).await.unwrap();
        assert_eq!(stats.conflicts_detected, 5);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_update_lag_sets_value() {
        let engine = make_engine(1);
        engine.add_site(make_site(2)).await;

        engine.update_lag(2, 100).await;
        let stats = engine.site_stats(2).await.unwrap();
        assert_eq!(stats.current_lag_entries, 100);

        engine.update_lag(2, 500).await;
        let stats = engine.site_stats(2).await.unwrap();
        assert_eq!(stats.current_lag_entries, 500);

        engine.update_lag(2, 0).await;
        let stats = engine.site_stats(2).await.unwrap();
        assert_eq!(stats.current_lag_entries, 0);
    }

    // ============================================================================
    // Category 4: Fanout Empty/Edge Cases (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_ef_sec_fanout_to_0_sites_empty_summary() {
        let sender = FanoutSender::new(1);
        let batch = EntryBatch::new(1, vec![], 1);

        let summary = sender.fanout_to(batch, &[]).await;

        assert_eq!(summary.total_sites, 0);
        assert_eq!(summary.successful_sites, 0);
        assert_eq!(summary.failed_sites, 0);
        assert!(summary.results.is_empty());
    }

    #[tokio::test]
    async fn test_repl_ef_sec_fanout_to_nonexistent_site_failure() {
        let sender = FanoutSender::new(1);
        let batch = EntryBatch::new(1, vec![make_entry(1, 100)], 1);

        let summary = sender.fanout_to(batch, &[999]).await;

        assert_eq!(summary.total_sites, 1);
        assert_eq!(summary.failed_sites, 1);
        assert_eq!(summary.successful_sites, 0);
        assert!(!summary.results[0].success);
        assert!(summary.results[0].error.is_some());
    }

    #[tokio::test]
    async fn test_repl_ef_sec_fanout_with_empty_entries_succeeds() {
        let sender = FanoutSender::new(1);

        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);

        sender.add_conduit(2, conduit_a).await;

        let batch = EntryBatch::new(1, vec![], 1);
        let summary = sender.fanout_to(batch, &[2]).await;

        assert_eq!(summary.total_sites, 1);
        assert_eq!(summary.successful_sites, 1);
        assert_eq!(summary.results[0].entries_sent, 0);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_fanout_summary_zero_sites_failure_rate_zero() {
        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 0,
            successful_sites: 0,
            failed_sites: 0,
            results: vec![],
        };

        assert_eq!(summary.failure_rate(), 0.0);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_fanout_summary_all_succeeded_false_zero_sites() {
        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 0,
            successful_sites: 0,
            failed_sites: 0,
            results: vec![],
        };

        assert!(!summary.all_succeeded());
    }

    // ============================================================================
    // Category 5: Fanout Conduit Management (4 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_ef_sec_add_and_remove_conduit() {
        let sender = FanoutSender::new(1);
        let conduit = make_conduit(1, 2);

        sender.add_conduit(2, conduit).await;
        assert_eq!(sender.conduit_count().await, 1);

        let removed = sender.remove_conduit(2).await;
        assert!(removed);
        assert_eq!(sender.conduit_count().await, 0);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_remove_nonexistent_conduit_false() {
        let sender = FanoutSender::new(1);

        let removed = sender.remove_conduit(999).await;
        assert!(!removed);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_conduit_count_tracks_correctly() {
        let sender = FanoutSender::new(1);

        assert_eq!(sender.conduit_count().await, 0);

        let c2 = make_conduit(1, 2);
        sender.add_conduit(2, c2).await;
        assert_eq!(sender.conduit_count().await, 1);

        let c3 = make_conduit(1, 3);
        sender.add_conduit(3, c3).await;
        assert_eq!(sender.conduit_count().await, 2);

        let c4 = make_conduit(1, 4);
        sender.add_conduit(4, c4).await;
        assert_eq!(sender.conduit_count().await, 3);

        sender.remove_conduit(3).await;
        assert_eq!(sender.conduit_count().await, 2);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_site_ids_returns_sorted() {
        let sender = FanoutSender::new(1);

        let c5 = make_conduit(1, 5);
        sender.add_conduit(5, c5).await;
        let c2 = make_conduit(1, 2);
        sender.add_conduit(2, c2).await;
        let c8 = make_conduit(1, 8);
        sender.add_conduit(8, c8).await;
        let c1 = make_conduit(1, 1);
        sender.add_conduit(1, c1).await;

        let ids = sender.site_ids().await;
        assert_eq!(ids, vec![1, 2, 5, 8]);
    }

    // ============================================================================
    // Category 6: Fanout Summary Analytics (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_ef_sec_successful_site_ids_filters() {
        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 4,
            successful_sites: 2,
            failed_sites: 2,
            results: vec![
                FanoutResult {
                    site_id: 1,
                    success: true,
                    entries_sent: 10,
                    error: None,
                    latency_us: 100,
                },
                FanoutResult {
                    site_id: 2,
                    success: false,
                    entries_sent: 0,
                    error: Some("err".to_string()),
                    latency_us: 50,
                },
                FanoutResult {
                    site_id: 3,
                    success: true,
                    entries_sent: 10,
                    error: None,
                    latency_us: 120,
                },
                FanoutResult {
                    site_id: 4,
                    success: false,
                    entries_sent: 0,
                    error: Some("err".to_string()),
                    latency_us: 30,
                },
            ],
        };

        let successful = summary.successful_site_ids();
        assert_eq!(successful, vec![1, 3]);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_failed_site_ids_filters() {
        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 3,
            successful_sites: 1,
            failed_sites: 2,
            results: vec![
                FanoutResult {
                    site_id: 10,
                    success: true,
                    entries_sent: 5,
                    error: None,
                    latency_us: 200,
                },
                FanoutResult {
                    site_id: 20,
                    success: false,
                    entries_sent: 0,
                    error: Some("timeout".to_string()),
                    latency_us: 1000,
                },
                FanoutResult {
                    site_id: 30,
                    success: false,
                    entries_sent: 0,
                    error: Some("conduit not found".to_string()),
                    latency_us: 0,
                },
            ],
        };

        let failed = summary.failed_site_ids();
        assert_eq!(failed, vec![20, 30]);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_failure_rate_calculation() {
        let summary_50pct = FanoutSummary {
            batch_seq: 1,
            total_sites: 4,
            successful_sites: 2,
            failed_sites: 2,
            results: vec![],
        };
        assert_eq!(summary_50pct.failure_rate(), 0.5);

        let summary_25pct = FanoutSummary {
            batch_seq: 1,
            total_sites: 4,
            successful_sites: 3,
            failed_sites: 1,
            results: vec![],
        };
        assert_eq!(summary_25pct.failure_rate(), 0.25);

        let summary_100pct = FanoutSummary {
            batch_seq: 1,
            total_sites: 3,
            successful_sites: 0,
            failed_sites: 3,
            results: vec![],
        };
        assert_eq!(summary_100pct.failure_rate(), 1.0);

        let summary_0pct = FanoutSummary {
            batch_seq: 1,
            total_sites: 5,
            successful_sites: 5,
            failed_sites: 0,
            results: vec![],
        };
        assert_eq!(summary_0pct.failure_rate(), 0.0);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_fanout_result_fields_accessible() {
        let result = FanoutResult {
            site_id: 42,
            success: true,
            entries_sent: 100,
            error: None,
            latency_us: 12345,
        };

        assert_eq!(result.site_id, 42);
        assert!(result.success);
        assert_eq!(result.entries_sent, 100);
        assert!(result.error.is_none());
        assert_eq!(result.latency_us, 12345);
    }

    #[tokio::test]
    async fn test_repl_ef_sec_latency_field_nonnegative() {
        let sender = FanoutSender::new(1);

        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);

        sender.add_conduit(2, conduit_a).await;

        let batch = EntryBatch::new(1, vec![make_entry(1, 100)], 1);
        let summary = sender.fanout_to(batch, &[2]).await;

        assert_eq!(summary.results.len(), 1);
        let result = &summary.results[0];
        assert!(result.success);
        assert!(result.latency_us >= 0);
    }
}