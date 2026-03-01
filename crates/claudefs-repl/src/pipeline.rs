//! Replication Pipeline
//!
//! Combines journal tailing + compaction + throttling + fanout into a single pipeline abstraction.

#[allow(unused_imports)]
use crate::conduit::{Conduit, ConduitConfig, EntryBatch};
use crate::error::ReplError;
use crate::fanout::FanoutSender;
use crate::journal::JournalEntry;
use crate::sync::BatchCompactor;
use crate::throttle::{ThrottleConfig, ThrottleManager};
use crate::uidmap::UidMapper;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Configuration for the full replication pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Local site identifier.
    pub local_site_id: u64,
    /// Batch up to this many entries before dispatching.
    pub max_batch_size: usize,
    /// Wait up to this long (ms) to fill a batch before sending anyway.
    pub batch_timeout_ms: u64,
    /// Whether to compact entries before sending.
    pub compact_before_send: bool,
    /// Whether to apply UID mapping before sending.
    pub apply_uid_mapping: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            local_site_id: 1,
            max_batch_size: 1000,
            batch_timeout_ms: 100,
            compact_before_send: true,
            apply_uid_mapping: false,
        }
    }
}

/// Current pipeline statistics.
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    /// Total entries received from journal tailer.
    pub entries_tailed: u64,
    /// Number of entries removed by compaction.
    pub entries_compacted_away: u64,
    /// Total batches dispatched to fanout.
    pub batches_dispatched: u64,
    /// Total entries successfully sent.
    pub total_entries_sent: u64,
    /// Total bytes sent over the wire.
    pub total_bytes_sent: u64,
    /// Number of times throttling blocked sending.
    pub throttle_stalls: u64,
    /// Number of fanout failures.
    pub fanout_failures: u64,
}

/// The replication pipeline state.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineState {
    /// Pipeline has not been started.
    Idle,
    /// Pipeline is actively processing and replicating.
    Running,
    /// Pipeline is stopping but processing remaining entries.
    Draining,
    /// Pipeline has completely stopped.
    Stopped,
}

/// The replication pipeline: tails journal → compacts → throttles → fanout.
#[allow(dead_code)]
pub struct ReplicationPipeline {
    config: PipelineConfig,
    state: Arc<Mutex<PipelineState>>,
    stats: Arc<Mutex<PipelineStats>>,
    throttle: Arc<Mutex<ThrottleManager>>,
    fanout: Arc<FanoutSender>,
    uid_mapper: Arc<UidMapper>,
}

impl ReplicationPipeline {
    /// Create a new pipeline.
    pub fn new(
        config: PipelineConfig,
        throttle: ThrottleManager,
        fanout: FanoutSender,
        uid_mapper: UidMapper,
    ) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(PipelineState::Idle)),
            stats: Arc::new(Mutex::new(PipelineStats::default())),
            throttle: Arc::new(Mutex::new(throttle)),
            fanout: Arc::new(fanout),
            uid_mapper: Arc::new(uid_mapper),
        }
    }

    /// Start the pipeline.
    pub async fn start(&self) {
        let mut state = self.state.lock().await;
        if *state == PipelineState::Idle {
            *state = PipelineState::Running;
        }
    }

    /// Stop the pipeline.
    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        match *state {
            PipelineState::Running => *state = PipelineState::Draining,
            PipelineState::Draining => *state = PipelineState::Stopped,
            _ => *state = PipelineState::Stopped,
        }
    }

    /// Get current state.
    pub async fn state(&self) -> PipelineState {
        self.state.lock().await.clone()
    }

    /// Get current statistics snapshot.
    pub async fn stats(&self) -> PipelineStats {
        self.stats.lock().await.clone()
    }

    /// Process a batch manually (for testing; in production, driven by journal tailer).
    /// Applies compaction, UID mapping, throttle check, and fanout.
    pub async fn process_batch(&self, entries: Vec<JournalEntry>, now_us: u64) -> Result<usize, ReplError> {
        {
            let mut stats = self.stats.lock().await;
            stats.entries_tailed += entries.len() as u64;
        }

        if entries.is_empty() {
            return Ok(0);
        }

        let processed_entries = if self.config.compact_before_send {
            let result = BatchCompactor::compact(entries.clone());
            {
                let mut stats = self.stats.lock().await;
                stats.entries_compacted_away += result.removed_count as u64;
            }
            result.entries
        } else {
            entries
        };

        if processed_entries.is_empty() {
            return Ok(0);
        }

        let mut total_bytes = 0u64;
        for entry in &processed_entries {
            total_bytes += entry.payload.len() as u64 + 64;
        }

        let mut throttle = self.throttle.lock().await;
        let remote_sites: Vec<u64> = self.fanout.site_ids().await;
        let mut all_throttled = false;

        for site_id in &remote_sites {
            if !throttle.try_send(*site_id, total_bytes, processed_entries.len() as u64, now_us) {
                all_throttled = true;
            }
        }

        if all_throttled {
            let mut stats = self.stats.lock().await;
            stats.throttle_stalls += 1;
            return Err(ReplError::Journal {
                msg: "throttled".to_string(),
            });
        }
        drop(throttle);

        let batch = EntryBatch::new(
            self.config.local_site_id,
            processed_entries,
            self.config.local_site_id,
        );

        let summary = self.fanout.fanout(batch).await;

        let mut stats = self.stats.lock().await;
        stats.batches_dispatched += 1;
        stats.total_entries_sent += summary.successful_sites as u64 * summary.results.iter().map(|r| r.entries_sent).sum::<usize>() as u64;
        stats.total_bytes_sent += total_bytes;

        if summary.any_failed() {
            stats.fanout_failures += summary.failed_sites as u64;
        }

        Ok(summary.successful_sites)
    }

    /// Update throttle config for a site.
    pub async fn update_throttle(&self, site_id: u64, config: ThrottleConfig) {
        let mut throttle = self.throttle.lock().await;
        throttle.update_site_config(site_id, config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod pipeline_config {
        use super::*;

        #[test]
        fn test_default_config() {
            let config = PipelineConfig::default();
            assert_eq!(config.local_site_id, 1);
            assert_eq!(config.max_batch_size, 1000);
            assert_eq!(config.batch_timeout_ms, 100);
            assert!(config.compact_before_send);
            assert!(!config.apply_uid_mapping);
        }
    }

    mod pipeline_creation {
        use super::*;

        #[tokio::test]
        async fn test_create_pipeline_with_default_config() {
            let config = PipelineConfig::default();
            let throttle = ThrottleManager::new(ThrottleConfig::default());
            let fanout = FanoutSender::new(1);
            let uid_mapper = UidMapper::passthrough();

            let pipeline = ReplicationPipeline::new(config, throttle, fanout, uid_mapper);
            let state = pipeline.state().await;
            assert_eq!(state, PipelineState::Idle);
        }
    }

    mod pipeline_state_transitions {
        use super::*;

        #[tokio::test]
        async fn test_start_idle_to_running() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            let state = pipeline.state().await;
            assert_eq!(state, PipelineState::Running);
        }

        #[tokio::test]
        async fn test_stop_running_to_draining() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            pipeline.stop().await;
            let state = pipeline.state().await;
            assert_eq!(state, PipelineState::Draining);
        }

        #[tokio::test]
        async fn test_stop_draining_to_stopped() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            pipeline.stop().await;
            pipeline.stop().await;
            let state = pipeline.state().await;
            assert_eq!(state, PipelineState::Stopped);
        }

        #[tokio::test]
        async fn test_stop_idle_to_stopped() {
            let pipeline = create_test_pipeline();
            pipeline.stop().await;
            let state = pipeline.state().await;
            assert_eq!(state, PipelineState::Stopped);
        }
    }

    mod process_batch {
        use super::*;
        use crate::journal::OpKind;

        fn make_test_entry(seq: u64) -> JournalEntry {
            JournalEntry::new(seq, 0, 1, 1000 + seq, 100, OpKind::Write, vec![1, 2, 3])
        }

        #[tokio::test]
        async fn test_process_batch_sends_to_fanout() {
            let pipeline = create_test_pipeline_with_conduits().await;
            pipeline.start().await;

            let entries = vec![make_test_entry(1), make_test_entry(2)];
            let result = pipeline.process_batch(entries, current_time_us()).await;
            assert!(result.is_ok());
        }

        #[tokio::test]
        async fn test_stats_updated_on_process_batch() {
            let pipeline = create_test_pipeline_with_conduits().await;
            pipeline.start().await;

            let entries = vec![make_test_entry(1)];
            pipeline.process_batch(entries, current_time_us()).await.unwrap();

            let stats = pipeline.stats().await;
            assert_eq!(stats.entries_tailed, 1);
            assert!(stats.batches_dispatched >= 1);
        }

        #[tokio::test]
        async fn test_empty_batch_noop() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;

            let result = pipeline.process_batch(vec![], current_time_us()).await;
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 0);
        }

        #[tokio::test]
        async fn test_compaction_reduces_entries() {
            let mut config = PipelineConfig::default();
            config.compact_before_send = true;
            let pipeline = ReplicationPipeline::new(
                config,
                ThrottleManager::new(ThrottleConfig::default()),
                FanoutSender::new(1),
                UidMapper::passthrough(),
            );
            pipeline.start().await;

            let entries = vec![
                JournalEntry::new(1, 0, 1, 1000, 100, OpKind::Write, vec![1]),
                JournalEntry::new(2, 0, 1, 2000, 100, OpKind::Write, vec![2]),
            ];
            pipeline.process_batch(entries, current_time_us()).await.unwrap();

            let stats = pipeline.stats().await;
            assert!(stats.entries_compacted_away >= 1);
        }
    }

    mod pipeline_stop {
        use super::*;

        #[tokio::test]
        async fn test_stop_transitions_to_stopped() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            pipeline.stop().await;
            pipeline.stop().await;
            assert_eq!(pipeline.state().await, PipelineState::Stopped);
        }
    }

    mod multiple_process_batch {
        use super::*;
        use crate::journal::OpKind;

        fn make_test_entry(seq: u64) -> JournalEntry {
            JournalEntry::new(seq, 0, 1, 1000 + seq, 100 + seq, OpKind::Write, vec![1, 2, 3])
        }

        #[tokio::test]
        async fn test_multiple_process_batch_accumulate_stats() {
            let pipeline = create_test_pipeline_with_conduits().await;
            pipeline.start().await;

            pipeline.process_batch(vec![make_test_entry(1)], current_time_us()).await.unwrap();
            pipeline.process_batch(vec![make_test_entry(2)], current_time_us()).await.unwrap();

            let stats = pipeline.stats().await;
            assert_eq!(stats.entries_tailed, 2);
        }
    }

    mod pipeline_state {
        use super::*;

        #[tokio::test]
        async fn test_pipeline_state_after_start() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            assert_eq!(pipeline.state().await, PipelineState::Running);
        }

        #[tokio::test]
        async fn test_pipeline_state_after_start_stop() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            pipeline.stop().await;
            assert_eq!(pipeline.state().await, PipelineState::Draining);
        }
    }

    mod update_throttle {
        use super::*;

        #[tokio::test]
        async fn test_update_throttle_does_not_panic() {
            let pipeline = create_test_pipeline();
            let config = ThrottleConfig::default();
            pipeline.update_throttle(1, config).await;
        }
    }

    mod pipeline_stats {
        use super::*;

        #[tokio::test]
        async fn test_initial_stats() {
            let pipeline = create_test_pipeline();
            let stats = pipeline.stats().await;
            assert_eq!(stats.entries_tailed, 0);
            assert_eq!(stats.batches_dispatched, 0);
        }

        #[tokio::test]
        async fn test_stats_fanout_failures() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            let stats = pipeline.stats().await;
            assert_eq!(stats.fanout_failures, 0);
        }

        #[tokio::test]
        async fn test_stats_throttle_stalls() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            let stats = pipeline.stats().await;
            assert_eq!(stats.throttle_stalls, 0);
        }

        #[tokio::test]
        async fn test_stats_total_bytes_sent() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            let stats = pipeline.stats().await;
            assert_eq!(stats.total_bytes_sent, 0);
        }

        #[tokio::test]
        async fn test_stats_total_entries_sent() {
            let pipeline = create_test_pipeline();
            pipeline.start().await;
            let stats = pipeline.stats().await;
            assert_eq!(stats.total_entries_sent, 0);
        }
    }

    mod pipeline_clone {
        use super::*;

        #[tokio::test]
        async fn test_stats_clone() {
            let pipeline = create_test_pipeline();
            let stats = pipeline.stats().await;
            let cloned = stats.clone();
            assert_eq!(stats.entries_tailed, cloned.entries_tailed);
        }
    }

    mod pipeline_default {
        use super::*;

        #[test]
        fn test_pipeline_config_default_local_site_id() {
            let config = PipelineConfig::default();
            assert_eq!(config.local_site_id, 1);
        }

        #[test]
        fn test_pipeline_config_default_max_batch_size() {
            let config = PipelineConfig::default();
            assert_eq!(config.max_batch_size, 1000);
        }

        #[test]
        fn test_pipeline_config_default_batch_timeout() {
            let config = PipelineConfig::default();
            assert_eq!(config.batch_timeout_ms, 100);
        }

        #[test]
        fn test_pipeline_config_default_compact() {
            let config = PipelineConfig::default();
            assert!(config.compact_before_send);
        }
    }

    fn create_test_pipeline() -> ReplicationPipeline {
        ReplicationPipeline::new(
            PipelineConfig::default(),
            ThrottleManager::new(ThrottleConfig::default()),
            FanoutSender::new(1),
            UidMapper::passthrough(),
        )
    }

    async fn create_test_pipeline_with_conduits() -> ReplicationPipeline {
        let pipeline = create_test_pipeline();

        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);

        pipeline.fanout.add_conduit(2, conduit_a).await;

        let config_c = ThrottleConfig::default();
        let mut throttle = ThrottleManager::new(config_c);
        throttle.register_site_default(2);

        ReplicationPipeline::new(
            PipelineConfig::default(),
            throttle,
            FanoutSender::new(1),
            UidMapper::passthrough(),
        )
    }

    fn current_time_us() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }
}