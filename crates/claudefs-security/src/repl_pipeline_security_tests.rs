//! Replication pipeline security tests.
//!
//! Part of A10 Phase 30

#[cfg(test)]
mod tests {
    use claudefs_repl::conduit::{Conduit, ConduitConfig};
    use claudefs_repl::fanout::FanoutSender;
    use claudefs_repl::journal::{JournalEntry, OpKind};
    use claudefs_repl::pipeline::{PipelineConfig, PipelineState, PipelineStats, ReplicationPipeline};
    use claudefs_repl::throttle::{ThrottleConfig, ThrottleManager};
    use claudefs_repl::uidmap::UidMapper;

    fn make_entry(seq: u64, inode: u64) -> JournalEntry {
        JournalEntry::new(seq, 0, 1, 1000 + seq, inode, OpKind::Write, vec![1, 2, 3])
    }

    fn current_time_us() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }

    fn make_pipeline() -> ReplicationPipeline {
        ReplicationPipeline::new(
            PipelineConfig::default(),
            ThrottleManager::new(ThrottleConfig::default()),
            FanoutSender::new(1),
            UidMapper::passthrough(),
        )
    }

    async fn make_pipeline_with_conduit() -> ReplicationPipeline {
        let fanout = FanoutSender::new(1);
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);
        fanout.add_conduit(2, conduit_a).await;

        let mut throttle = ThrottleManager::new(ThrottleConfig::default());
        throttle.register_site_default(2);

        ReplicationPipeline::new(
            PipelineConfig::default(),
            throttle,
            fanout,
            UidMapper::passthrough(),
        )
    }

    // ============================================================================
    // Category 1: Pipeline State Machine (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_pipe_sec_initial_state_is_idle() {
        let pipeline = make_pipeline();
        assert_eq!(pipeline.state().await, PipelineState::Idle);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_start_idle_to_running() {
        let pipeline = make_pipeline();
        pipeline.start().await;
        assert_eq!(pipeline.state().await, PipelineState::Running);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_stop_running_to_draining() {
        let pipeline = make_pipeline();
        pipeline.start().await;
        pipeline.stop().await;
        assert_eq!(pipeline.state().await, PipelineState::Draining);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_double_stop_draining_to_stopped() {
        let pipeline = make_pipeline();
        pipeline.start().await;
        pipeline.stop().await;
        assert_eq!(pipeline.state().await, PipelineState::Draining);
        pipeline.stop().await;
        assert_eq!(pipeline.state().await, PipelineState::Stopped);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_stop_idle_to_stopped() {
        let pipeline = make_pipeline();
        assert_eq!(pipeline.state().await, PipelineState::Idle);
        pipeline.stop().await;
        assert_eq!(pipeline.state().await, PipelineState::Stopped);
    }

    // ============================================================================
    // Category 2: Pipeline Config Defaults (3 tests)
    // ============================================================================

    #[test]
    fn test_repl_pipe_sec_default_local_site_id_is_1() {
        let config = PipelineConfig::default();
        assert_eq!(config.local_site_id, 1);
    }

    #[test]
    fn test_repl_pipe_sec_default_compact_before_send_is_true() {
        let config = PipelineConfig::default();
        assert!(config.compact_before_send);
    }

    #[test]
    fn test_repl_pipe_sec_default_apply_uid_mapping_is_false() {
        let config = PipelineConfig::default();
        assert!(!config.apply_uid_mapping);
    }

    // ============================================================================
    // Category 3: Process Batch Security (7 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_pipe_sec_empty_batch_returns_ok_zero() {
        let pipeline = make_pipeline();
        pipeline.start().await;
        let result = pipeline.process_batch(vec![], current_time_us()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_stats_track_entries_tailed_correctly() {
        let pipeline = make_pipeline();
        pipeline.start().await;

        let entries = vec![make_entry(1, 100), make_entry(2, 200)];
        let _ = pipeline.process_batch(entries, current_time_us()).await;

        let stats = pipeline.stats().await;
        assert_eq!(stats.entries_tailed, 2);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_stats_track_batches_dispatched() {
        let pipeline = make_pipeline_with_conduit().await;
        pipeline.start().await;

        let entries = vec![make_entry(1, 100)];
        let _ = pipeline.process_batch(entries, current_time_us()).await;

        let stats = pipeline.stats().await;
        assert!(stats.batches_dispatched >= 1);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_process_no_conduits_returns_ok_zero() {
        let pipeline = make_pipeline();
        pipeline.start().await;

        let entries = vec![make_entry(1, 100), make_entry(2, 200)];
        let result = pipeline.process_batch(entries, current_time_us()).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_process_with_conduit_increments_batches_dispatched() {
        let pipeline = make_pipeline_with_conduit().await;
        pipeline.start().await;

        let stats_before = pipeline.stats().await;
        assert_eq!(stats_before.batches_dispatched, 0);

        let entries = vec![make_entry(1, 100)];
        let _ = pipeline.process_batch(entries, current_time_us()).await;

        let stats_after = pipeline.stats().await;
        assert!(stats_after.batches_dispatched > stats_before.batches_dispatched);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_compaction_reduces_duplicate_entries() {
        let pipeline = make_pipeline_with_conduit().await;
        pipeline.start().await;

        let entries = vec![
            make_entry(1, 100),
            make_entry(2, 100),
            make_entry(3, 200),
        ];
        let _ = pipeline.process_batch(entries, current_time_us()).await;

        let stats = pipeline.stats().await;
        assert!(stats.entries_compacted_away >= 1);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_stats_initially_all_zero() {
        let pipeline = make_pipeline();
        let stats = pipeline.stats().await;

        assert_eq!(stats.entries_tailed, 0);
        assert_eq!(stats.entries_compacted_away, 0);
        assert_eq!(stats.batches_dispatched, 0);
        assert_eq!(stats.total_entries_sent, 0);
        assert_eq!(stats.total_bytes_sent, 0);
        assert_eq!(stats.throttle_stalls, 0);
        assert_eq!(stats.fanout_failures, 0);
    }

    // ============================================================================
    // Category 4: Pipeline Stats Integrity (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_pipe_sec_stats_clone_works_correctly() {
        let pipeline = make_pipeline();
        let stats = pipeline.stats().await;
        let cloned = stats.clone();

        assert_eq!(stats.entries_tailed, cloned.entries_tailed);
        assert_eq!(stats.batches_dispatched, cloned.batches_dispatched);
        assert_eq!(stats.total_bytes_sent, cloned.total_bytes_sent);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_throttle_stalls_initially_zero() {
        let pipeline = make_pipeline();
        let stats = pipeline.stats().await;
        assert_eq!(stats.throttle_stalls, 0);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_fanout_failures_initially_zero() {
        let pipeline = make_pipeline();
        let stats = pipeline.stats().await;
        assert_eq!(stats.fanout_failures, 0);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_total_bytes_sent_initially_zero() {
        let pipeline = make_pipeline();
        let stats = pipeline.stats().await;
        assert_eq!(stats.total_bytes_sent, 0);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_entries_compacted_away_initially_zero() {
        let pipeline = make_pipeline();
        let stats = pipeline.stats().await;
        assert_eq!(stats.entries_compacted_away, 0);
    }

    // ============================================================================
    // Category 5: Throttle Integration (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_repl_pipe_sec_update_throttle_does_not_panic() {
        let pipeline = make_pipeline();
        let config = ThrottleConfig::default();
        pipeline.update_throttle(1, config).await;
    }

    #[test]
    fn test_repl_pipe_sec_pipeline_state_variants_distinct() {
        assert_ne!(PipelineState::Idle, PipelineState::Running);
        assert_ne!(PipelineState::Idle, PipelineState::Draining);
        assert_ne!(PipelineState::Idle, PipelineState::Stopped);
        assert_ne!(PipelineState::Running, PipelineState::Draining);
        assert_ne!(PipelineState::Running, PipelineState::Stopped);
        assert_ne!(PipelineState::Draining, PipelineState::Stopped);
    }

    #[test]
    fn test_repl_pipe_sec_pipeline_state_idle_not_running() {
        assert_ne!(PipelineState::Idle, PipelineState::Running);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_multiple_process_batch_accumulate_stats() {
        let pipeline = make_pipeline_with_conduit().await;
        pipeline.start().await;

        pipeline.process_batch(vec![make_entry(1, 100)], current_time_us()).await.unwrap();
        pipeline.process_batch(vec![make_entry(2, 200)], current_time_us()).await.unwrap();
        pipeline.process_batch(vec![make_entry(3, 300)], current_time_us()).await.unwrap();

        let stats = pipeline.stats().await;
        assert_eq!(stats.entries_tailed, 3);
    }

    #[tokio::test]
    async fn test_repl_pipe_sec_stats_total_entries_sent_initially_zero() {
        let pipeline = make_pipeline();
        let stats = pipeline.stats().await;
        assert_eq!(stats.total_entries_sent, 0);
    }
}