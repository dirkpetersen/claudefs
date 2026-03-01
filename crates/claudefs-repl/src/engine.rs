//! The central replication engine that manages all site replication.

use crate::sync::ConflictDetector;
use crate::topology::{ReplicationTopology, SiteInfo};
use crate::wal::{ReplicationCursor, ReplicationWal};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Configuration for the replication engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// The local site ID.
    pub local_site_id: u64,
    /// Maximum number of entries to send in a single batch.
    pub max_batch_size: usize,
    /// Batch timeout in milliseconds.
    pub batch_timeout_ms: u64,
    /// Whether to compact entries before sending.
    pub compact_before_send: bool,
    /// Maximum concurrent send operations.
    pub max_concurrent_sends: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            local_site_id: 0,
            max_batch_size: 1000,
            batch_timeout_ms: 100,
            compact_before_send: true,
            max_concurrent_sends: 4,
        }
    }
}

/// Per-remote-site replication statistics.
#[derive(Debug, Clone)]
pub struct SiteReplicationStats {
    /// Remote site identifier.
    pub remote_site_id: u64,
    /// Total entries sent to this site.
    pub entries_sent: u64,
    /// Total entries received from this site.
    pub entries_received: u64,
    /// Total batches sent to this site.
    pub batches_sent: u64,
    /// Total batches received from this site.
    pub batches_received: u64,
    /// Total conflicts detected for this site.
    pub conflicts_detected: u64,
    /// Current replication lag in entries.
    pub current_lag_entries: u64,
}

impl SiteReplicationStats {
    fn new(remote_site_id: u64) -> Self {
        Self {
            remote_site_id,
            entries_sent: 0,
            entries_received: 0,
            batches_sent: 0,
            batches_received: 0,
            conflicts_detected: 0,
            current_lag_entries: 0,
        }
    }
}

/// The replication engine state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineState {
    /// Engine is idle and not running.
    Idle,
    /// Engine is actively running.
    Running,
    /// Engine is draining (finishing pending work).
    Draining,
    /// Engine has stopped.
    Stopped,
}

/// The central replication engine.
pub struct ReplicationEngine {
    #[allow(dead_code)]
    config: EngineConfig,
    state: Arc<Mutex<EngineState>>,
    topology: Arc<tokio::sync::RwLock<ReplicationTopology>>,
    wal: Arc<tokio::sync::Mutex<ReplicationWal>>,
    detector: Arc<ConflictDetector>,
    site_stats: Arc<tokio::sync::Mutex<HashMap<u64, SiteReplicationStats>>>,
}

impl ReplicationEngine {
    /// Create a new replication engine.
    pub fn new(config: EngineConfig, topology: ReplicationTopology) -> Self {
        let detector = ConflictDetector::new(config.local_site_id);
        Self {
            config,
            state: Arc::new(Mutex::new(EngineState::Idle)),
            topology: Arc::new(tokio::sync::RwLock::new(topology)),
            wal: Arc::new(tokio::sync::Mutex::new(ReplicationWal::new())),
            detector: Arc::new(detector),
            site_stats: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Start the engine (returns immediately; engine runs in background tasks).
    pub async fn start(&self) {
        let mut state = self.state.lock().await;
        if *state == EngineState::Idle {
            *state = EngineState::Running;
        }
    }

    /// Stop the engine gracefully.
    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        if *state == EngineState::Running {
            *state = EngineState::Draining;
        }
        // Simulate draining completion
        *state = EngineState::Stopped;
    }

    /// Get the current engine state.
    pub async fn state(&self) -> EngineState {
        self.state.lock().await.clone()
    }

    /// Register a remote site for replication.
    pub async fn add_site(&self, info: SiteInfo) {
        {
            let mut topo = self.topology.write().await;
            topo.upsert_site(info.clone());
        }
        {
            let mut stats = self.site_stats.lock().await;
            stats.insert(info.site_id, SiteReplicationStats::new(info.site_id));
        }
    }

    /// Remove a remote site from replication.
    pub async fn remove_site(&self, site_id: u64) {
        {
            let mut topo = self.topology.write().await;
            topo.remove_site(site_id);
        }
        {
            let mut stats = self.site_stats.lock().await;
            stats.remove(&site_id);
        }
    }

    /// Get per-site replication statistics.
    pub async fn site_stats(&self, site_id: u64) -> Option<SiteReplicationStats> {
        let stats = self.site_stats.lock().await;
        stats.get(&site_id).cloned()
    }

    /// Get all site statistics.
    pub async fn all_site_stats(&self) -> Vec<SiteReplicationStats> {
        let stats = self.site_stats.lock().await;
        stats.values().cloned().collect()
    }

    /// Record that entries have been sent to a remote site.
    pub async fn record_send(&self, remote_site_id: u64, entry_count: u64, batch_count: u64) {
        let mut stats = self.site_stats.lock().await;
        if let Some(s) = stats.get_mut(&remote_site_id) {
            s.entries_sent += entry_count;
            s.batches_sent += batch_count;
        }
    }

    /// Record that entries have been received from a remote site.
    pub async fn record_receive(&self, remote_site_id: u64, entry_count: u64, batch_count: u64) {
        let mut stats = self.site_stats.lock().await;
        if let Some(s) = stats.get_mut(&remote_site_id) {
            s.entries_received += entry_count;
            s.batches_received += batch_count;
        }
    }

    /// Record a detected conflict for a remote site.
    pub async fn record_conflict(&self, remote_site_id: u64) {
        let mut stats = self.site_stats.lock().await;
        if let Some(s) = stats.get_mut(&remote_site_id) {
            s.conflicts_detected += 1;
        }
    }

    /// Update the lag for a remote site.
    pub async fn update_lag(&self, remote_site_id: u64, lag_entries: u64) {
        let mut stats = self.site_stats.lock().await;
        if let Some(s) = stats.get_mut(&remote_site_id) {
            s.current_lag_entries = lag_entries;
        }
    }

    /// Get the WAL for inspection.
    pub async fn wal_snapshot(&self) -> Vec<ReplicationCursor> {
        let wal = self.wal.lock().await;
        wal.all_cursors()
    }

    /// Get the conflict detector for admin reporting.
    pub fn detector(&self) -> Arc<ConflictDetector> {
        self.detector.clone()
    }

    /// Get the topology for read access.
    pub async fn topology_snapshot(&self) -> Vec<SiteInfo> {
        let topo = self.topology.read().await;
        topo.all_sites().into_iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod engine_config {
        use super::*;

        #[test]
        fn test_default_config() {
            let config = EngineConfig::default();
            assert_eq!(config.local_site_id, 0);
            assert_eq!(config.max_batch_size, 1000);
            assert_eq!(config.batch_timeout_ms, 100);
            assert!(config.compact_before_send);
            assert_eq!(config.max_concurrent_sends, 4);
        }

        #[test]
        fn test_custom_config() {
            let config = EngineConfig {
                local_site_id: 1,
                max_batch_size: 500,
                batch_timeout_ms: 50,
                compact_before_send: false,
                max_concurrent_sends: 8,
            };
            assert_eq!(config.local_site_id, 1);
            assert_eq!(config.max_batch_size, 500);
        }

        #[test]
        fn test_config_clone() {
            let config = EngineConfig::default();
            let cloned = config.clone();
            assert_eq!(cloned.max_batch_size, config.max_batch_size);
        }
    }

    mod create_engine {
        use super::*;

        #[test]
        fn test_create_with_default_config() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);
            // Engine created with default state
            assert_eq!(engine.config.local_site_id, 0);
        }

        #[test]
        fn test_create_with_custom_config() {
            let config = EngineConfig {
                local_site_id: 42,
                max_batch_size: 500,
                batch_timeout_ms: 200,
                compact_before_send: false,
                max_concurrent_sends: 8,
            };
            let topology = ReplicationTopology::new(42);
            let engine = ReplicationEngine::new(config, topology);
            assert_eq!(engine.config.local_site_id, 42);
            assert_eq!(engine.config.max_batch_size, 500);
        }

        #[test]
        fn test_engine_has_wal() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);
            // Can get WAL snapshot (empty initially)
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let snapshot = rt.block_on(async { engine.wal_snapshot().await });
            assert!(snapshot.is_empty());
        }
    }

    mod start_stop {
        use super::*;

        #[tokio::test]
        async fn test_start_transitions_to_running() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            engine.start().await;
            assert_eq!(engine.state().await, EngineState::Running);
        }

        #[tokio::test]
        async fn test_stop_transitions_to_stopped() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            engine.start().await;
            engine.stop().await;
            assert_eq!(engine.state().await, EngineState::Stopped);
        }

        #[tokio::test]
        async fn test_initial_state_is_idle() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            assert_eq!(engine.state().await, EngineState::Idle);
        }

        #[tokio::test]
        async fn test_start_from_stopped_no_change() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            engine.start().await;
            engine.stop().await;
            engine.start().await; // Should not change from Stopped
            assert_eq!(engine.state().await, EngineState::Stopped);
        }
    }

    mod add_remove_sites {
        use super::*;

        #[tokio::test]
        async fn test_add_site() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let site = SiteInfo::new(
                2,
                "us-west-2".to_string(),
                vec!["grpc://1.2.3.4:50051".to_string()],
                crate::topology::ReplicationRole::Primary,
            );
            engine.add_site(site).await;

            let topo_snapshot = engine.topology_snapshot().await;
            assert_eq!(topo_snapshot.len(), 1);
            assert_eq!(topo_snapshot[0].site_id, 2);
        }

        #[tokio::test]
        async fn test_remove_site() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let site = SiteInfo::new(
                2,
                "us-west-2".to_string(),
                vec![],
                crate::topology::ReplicationRole::Primary,
            );
            engine.add_site(site).await;
            engine.remove_site(2).await;

            let topo_snapshot = engine.topology_snapshot().await;
            assert!(topo_snapshot.is_empty());
        }

        #[tokio::test]
        async fn test_add_multiple_sites() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            for i in 2..=5u64 {
                let site = SiteInfo::new(
                    i,
                    format!("site-{}", i),
                    vec![],
                    crate::topology::ReplicationRole::Primary,
                );
                engine.add_site(site).await;
            }

            let topo_snapshot = engine.topology_snapshot().await;
            assert_eq!(topo_snapshot.len(), 4);
        }
    }

    mod stats {
        use super::*;

        #[tokio::test]
        async fn test_site_stats_returns_correct_values() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let site = SiteInfo::new(
                2,
                "site2".to_string(),
                vec![],
                crate::topology::ReplicationRole::Primary,
            );
            engine.add_site(site).await;

            engine.record_send(2, 100, 5).await;
            engine.record_receive(2, 50, 2).await;
            engine.record_conflict(2).await;

            let stats = engine.site_stats(2).await;
            assert!(stats.is_some());
            let s = stats.unwrap();
            assert_eq!(s.entries_sent, 100);
            assert_eq!(s.batches_sent, 5);
            assert_eq!(s.entries_received, 50);
            assert_eq!(s.batches_received, 2);
            assert_eq!(s.conflicts_detected, 1);
        }

        #[tokio::test]
        async fn test_site_stats_nonexistent() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let stats = engine.site_stats(999).await;
            assert!(stats.is_none());
        }

        #[tokio::test]
        async fn test_all_site_stats() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let site1 = SiteInfo::new(2, "site2".to_string(), vec![], crate::topology::ReplicationRole::Primary);
            let site2 = SiteInfo::new(3, "site3".to_string(), vec![], crate::topology::ReplicationRole::Primary);
            engine.add_site(site1).await;
            engine.add_site(site2).await;

            let all_stats = engine.all_site_stats().await;
            assert_eq!(all_stats.len(), 2);
        }

        #[tokio::test]
        async fn test_update_lag() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let site = SiteInfo::new(2, "site2".to_string(), vec![], crate::topology::ReplicationRole::Primary);
            engine.add_site(site).await;

            engine.update_lag(2, 150).await;

            let stats = engine.site_stats(2).await.unwrap();
            assert_eq!(stats.current_lag_entries, 150);
        }

        #[tokio::test]
        async fn test_stats_accumulate() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let site = SiteInfo::new(2, "site2".to_string(), vec![], crate::topology::ReplicationRole::Primary);
            engine.add_site(site).await;

            engine.record_send(2, 10, 1).await;
            engine.record_send(2, 20, 1).await;

            let stats = engine.site_stats(2).await.unwrap();
            assert_eq!(stats.entries_sent, 30);
            assert_eq!(stats.batches_sent, 2);
        }
    }

    mod snapshots {
        use super::*;

        #[tokio::test]
        async fn test_wal_snapshot_returns_cursors() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            // Direct access to WAL to add cursors
            {
                let mut wal = engine.wal.lock().await;
                wal.advance(2, 0, 100, 1000, 100);
                wal.advance(2, 1, 200, 2000, 200);
            }

            let snapshot = engine.wal_snapshot().await;
            assert_eq!(snapshot.len(), 2);
        }

        #[tokio::test]
        async fn test_topology_snapshot_after_add_remove() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let site1 = SiteInfo::new(2, "site2".to_string(), vec![], crate::topology::ReplicationRole::Primary);
            let site2 = SiteInfo::new(3, "site3".to_string(), vec![], crate::topology::ReplicationRole::Replica { primary_site_id: 1 });
            engine.add_site(site1).await;
            engine.add_site(site2).await;

            let snapshot = engine.topology_snapshot().await;
            assert_eq!(snapshot.len(), 2);

            engine.remove_site(2).await;

            let snapshot = engine.topology_snapshot().await;
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].site_id, 3);
        }

        #[tokio::test]
        async fn test_detector_access() {
            let topology = ReplicationTopology::new(1);
            let config = EngineConfig {
                local_site_id: 1,
                ..Default::default()
            };
            let engine = ReplicationEngine::new(config, topology);

            let detector = engine.detector();
            // Detector is accessible and returns the same Arc
            assert!(Arc::ptr_eq(&detector, &engine.detector()));
        }
    }

    mod concurrent_operations {
        use super::*;

        #[tokio::test]
        async fn test_concurrent_record_send() {
            let topology = ReplicationTopology::new(1);
            let engine = ReplicationEngine::new(EngineConfig::default(), topology);

            let site = SiteInfo::new(2, "site2".to_string(), vec![], crate::topology::ReplicationRole::Primary);
            engine.add_site(site).await;

            // Spawn multiple concurrent record_send calls
            let engine_clone = Arc::new(engine);
            let mut handles = vec![];

            for _ in 0..10 {
                let e = engine_clone.clone();
                let handle = tokio::spawn(async move {
                    e.record_send(2, 5, 1).await;
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.await.unwrap();
            }

            let stats = engine_clone.site_stats(2).await.unwrap();
            assert_eq!(stats.entries_sent, 50);
            assert_eq!(stats.batches_sent, 10);
        }

        #[tokio::test]
        async fn test_concurrent_stats_updates() {
            let topology = ReplicationTopology::new(1);
            let config = EngineConfig { local_site_id: 1, ..Default::default() };
            let engine = ReplicationEngine::new(config, topology);

            let site = SiteInfo::new(2, "site2".to_string(), vec![], crate::topology::ReplicationRole::Primary);
            engine.add_site(site).await;

            let engine_clone = Arc::new(engine);
            let mut handles = vec![];

            for i in 0..5u64 {
                let e = engine_clone.clone();
                let handle = tokio::spawn(async move {
                    e.record_send(2, i * 10, 1).await;
                    e.record_receive(2, i * 5, 1).await;
                    e.record_conflict(2).await;
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.await.unwrap();
            }

            let stats = engine_clone.site_stats(2).await.unwrap();
            assert_eq!(stats.batches_sent, 5);
            assert_eq!(stats.batches_received, 5);
            assert_eq!(stats.conflicts_detected, 5);
        }
    }

    mod engine_state {
        use super::*;

        #[test]
        fn test_engine_state_variants() {
            assert_eq!(EngineState::Idle, EngineState::Idle);
            assert_eq!(EngineState::Running, EngineState::Running);
            assert_eq!(EngineState::Draining, EngineState::Draining);
            assert_eq!(EngineState::Stopped, EngineState::Stopped);
        }

        #[test]
        fn test_engine_state_inequality() {
            assert_ne!(EngineState::Idle, EngineState::Running);
            assert_ne!(EngineState::Running, EngineState::Stopped);
        }
    }

    mod site_replication_stats {
        use super::*;

        #[test]
        fn test_stats_new() {
            let stats = SiteReplicationStats::new(42);
            assert_eq!(stats.remote_site_id, 42);
            assert_eq!(stats.entries_sent, 0);
            assert_eq!(stats.entries_received, 0);
        }

        #[test]
        fn test_stats_clone() {
            let stats = SiteReplicationStats::new(1);
            let cloned = stats.clone();
            assert_eq!(cloned.remote_site_id, stats.remote_site_id);
        }
    }
}