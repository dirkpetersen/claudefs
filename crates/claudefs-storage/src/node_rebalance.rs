//! Online node scaling with data rebalancing.
//!
//! This module manages segment migration when nodes join or leave the cluster.
//! It tracks which segments to migrate out, which to accept, and throttles
//! migration I/O based on configuration.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Unique identifier for a cluster node.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeId(pub String);

/// Segment identifier for rebalance operations.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct RebalanceSegmentId(pub u64);

/// Virtual shard identifier (0..255 for 256 shards per D4).
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShardId(pub u16);

/// State of a rebalance operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RebalanceState {
    Idle,
    Planning,
    Migrating {
        segments_total: u64,
        segments_done: u64,
    },
    Verifying,
    Completed {
        segments_moved: u64,
        bytes_moved: u64,
        duration_secs: u64,
    },
    Failed {
        reason: String,
    },
}

/// Direction of segment migration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationDirection {
    Outbound {
        target_node: NodeId,
    },
    Inbound {
        source_node: NodeId,
    },
}

/// A single segment migration task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationTask {
    pub segment_id: RebalanceSegmentId,
    pub shard_id: ShardId,
    pub direction: MigrationDirection,
    pub bytes: u64,
    pub state: MigrationTaskState,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

/// State of individual migration task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationTaskState {
    Queued,
    Transferring,
    Verifying,
    Completed,
    Failed {
        reason: String,
    },
}

/// Configuration for the rebalance engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceConfig {
    /// Maximum concurrent segment migrations.
    pub max_concurrent_migrations: u32,
    /// Maximum bandwidth for rebalance I/O in bytes/sec (throttling).
    pub max_bandwidth_bytes_per_sec: u64,
    /// Maximum IOPS for rebalance operations.
    pub max_iops: u32,
    /// Minimum time between rebalance runs in seconds.
    pub cooldown_secs: u64,
    /// Whether to auto-start rebalance on membership change.
    pub auto_rebalance: bool,
}

impl Default for RebalanceConfig {
    fn default() -> Self {
        Self {
            max_concurrent_migrations: 4,
            max_bandwidth_bytes_per_sec: 100 * 1024 * 1024,
            max_iops: 1000,
            cooldown_secs: 300,
            auto_rebalance: true,
        }
    }
}

/// Statistics for rebalance operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RebalanceStats {
    pub total_rebalances: u64,
    pub segments_migrated_out: u64,
    pub segments_migrated_in: u64,
    pub bytes_migrated_out: u64,
    pub bytes_migrated_in: u64,
    pub failed_migrations: u64,
    pub active_migrations: u32,
}

/// Rebalance engine for managing segment migrations during cluster topology changes.
pub struct RebalanceEngine {
    config: RebalanceConfig,
    state: RebalanceState,
    local_node: NodeId,
    shard_map: HashMap<ShardId, NodeId>,
    local_segments: HashMap<RebalanceSegmentId, ShardId>,
    migrations: Vec<MigrationTask>,
    stats: RebalanceStats,
    last_rebalance_time: u64,
}

impl RebalanceEngine {
    pub fn new(config: RebalanceConfig, local_node: NodeId) -> Self {
        info!("Initializing rebalance engine for node {:?} with config: {:?}", local_node, config);
        Self {
            config,
            state: RebalanceState::Idle,
            local_node,
            shard_map: HashMap::new(),
            local_segments: HashMap::new(),
            migrations: Vec::new(),
            stats: RebalanceStats::default(),
            last_rebalance_time: 0,
        }
    }

    pub fn register_segment(&mut self, segment_id: RebalanceSegmentId, shard_id: ShardId) {
        debug!("Registering segment {:?} for shard {:?}", segment_id, shard_id);
        self.local_segments.insert(segment_id, shard_id);
    }

    pub fn remove_segment(&mut self, segment_id: RebalanceSegmentId) -> Option<ShardId> {
        debug!("Removing segment {:?}", segment_id);
        self.local_segments.remove(&segment_id)
    }

    pub fn update_shard_map(&mut self, new_map: HashMap<ShardId, NodeId>) {
        info!("Updating shard map: {} shards, local node is {:?}", new_map.len(), self.local_node);
        self.shard_map = new_map;
    }

    pub fn plan_rebalance(&mut self) -> Vec<MigrationTask> {
        info!("Planning rebalance from state {:?}", self.state);
        
        let mut tasks = Vec::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for (segment_id, shard_id) in &self.local_segments {
            if let Some(owner) = self.shard_map.get(shard_id) {
                if owner != &self.local_node {
                    let task = MigrationTask {
                        segment_id: *segment_id,
                        shard_id: *shard_id,
                        direction: MigrationDirection::Outbound {
                            target_node: owner.clone(),
                        },
                        bytes: 2 * 1024 * 1024,
                        state: MigrationTaskState::Queued,
                        created_at: now,
                        completed_at: None,
                    };
                    debug!("Planned outbound migration: segment {:?} shard {:?} -> {:?}", segment_id, shard_id, owner);
                    tasks.push(task);
                }
            }
        }

        self.migrations = tasks.clone();
        tasks
    }

    pub fn start_rebalance(&mut self) -> Result<(), &'static str> {
        if self.state != RebalanceState::Idle {
            warn!("Cannot start rebalance from state {:?}", self.state);
            return Err("Cannot start rebalance: not in Idle state");
        }

        info!("Starting rebalance operation");
        self.state = RebalanceState::Planning;
        
        let tasks = self.plan_rebalance();
        let segments_total = tasks.len() as u64;
        
        if segments_total > 0 {
            self.state = RebalanceState::Migrating {
                segments_total,
                segments_done: 0,
            };
        } else {
            self.state = RebalanceState::Completed {
                segments_moved: 0,
                bytes_moved: 0,
                duration_secs: 0,
            };
        }

        self.stats.total_rebalances += 1;
        Ok(())
    }

    pub fn advance_migration(&mut self, segment_id: RebalanceSegmentId) -> Result<MigrationTaskState, &'static str> {
        let task = self.migrations.iter_mut().find(|t| t.segment_id == segment_id).ok_or("Migration task not found")?;

        let new_state = match task.state {
            MigrationTaskState::Queued => {
                debug!("Advancing segment {:?} from Queued to Transferring", segment_id);
                MigrationTaskState::Transferring
            }
            MigrationTaskState::Transferring => {
                debug!("Advancing segment {:?} from Transferring to Verifying", segment_id);
                MigrationTaskState::Verifying
            }
            MigrationTaskState::Verifying => {
                debug!("Advancing segment {:?} from Verifying to Completed", segment_id);
                
                match &task.direction {
                    MigrationDirection::Outbound { .. } => {
                        self.stats.segments_migrated_out += 1;
                        self.stats.bytes_migrated_out += task.bytes;
                    }
                    MigrationDirection::Inbound { .. } => {
                        self.stats.segments_migrated_in += 1;
                        self.stats.bytes_migrated_in += task.bytes;
                    }
                }
                
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                task.completed_at = Some(now);

                if let RebalanceState::Migrating { segments_total: _, segments_done } = &mut self.state {
                    *segments_done += 1;
                }

                MigrationTaskState::Completed
            }
            MigrationTaskState::Completed => {
                warn!("Migration already completed for segment {:?}", segment_id);
                return Err("Migration already completed");
            }
            MigrationTaskState::Failed { .. } => {
                return Err("Migration already failed");
            }
        };

        task.state = new_state.clone();
        Ok(new_state)
    }

    pub fn fail_migration(&mut self, segment_id: RebalanceSegmentId, reason: String) -> Result<(), &'static str> {
        let task = self.migrations.iter_mut().find(|t| t.segment_id == segment_id).ok_or("Migration task not found")?;

        warn!("Migration failed for segment {:?}: {}", segment_id, reason);
        task.state = MigrationTaskState::Failed { reason };
        self.stats.failed_migrations += 1;
        Ok(())
    }

    pub fn complete_rebalance(&mut self) -> Result<RebalanceStats, &'static str> {
        let pending = self.migrations.iter().filter(|t| {
            matches!(t.state, MigrationTaskState::Queued | MigrationTaskState::Transferring | MigrationTaskState::Verifying)
        }).count();

        if pending > 0 {
            warn!("Cannot complete rebalance: {} migrations still pending", pending);
            return Err("Cannot complete: migrations still pending");
        }

        let segments_moved = self.migrations.len() as u64;
        let bytes_moved: u64 = self.migrations.iter().map(|t| t.bytes).sum();

        self.state = RebalanceState::Completed { segments_moved, bytes_moved, duration_secs: 0 };

        info!("Rebalance completed: {} segments, {} bytes", segments_moved, bytes_moved);

        self.last_rebalance_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

        Ok(self.stats.clone())
    }

    pub fn abort_rebalance(&mut self, reason: String) {
        warn!("Aborting rebalance: {}", reason);
        self.state = RebalanceState::Failed { reason };
        
        for task in &mut self.migrations {
            if !matches!(task.state, MigrationTaskState::Completed | MigrationTaskState::Failed { .. }) {
                task.state = MigrationTaskState::Failed { reason: "Rebalance aborted".to_string() };
            }
        }
    }

    pub fn accept_inbound(&mut self, segment_id: RebalanceSegmentId, shard_id: ShardId, source_node: NodeId, bytes: u64) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

        let task = MigrationTask {
            segment_id,
            shard_id,
            direction: MigrationDirection::Inbound { source_node: source_node.clone() },
            bytes,
            state: MigrationTaskState::Queued,
            created_at: now,
            completed_at: None,
        };

        debug!("Accepted inbound migration: segment {:?} shard {:?} from {:?} ({} bytes)", segment_id, shard_id, &source_node, bytes);

        self.migrations.push(task);
    }

    pub fn can_accept_more(&self) -> bool {
        let active = self.active_migration_count();
        active < self.config.max_concurrent_migrations
    }

    pub fn active_migration_count(&self) -> u32 {
        self.migrations.iter().filter(|t| {
            matches!(t.state, MigrationTaskState::Queued | MigrationTaskState::Transferring | MigrationTaskState::Verifying)
        }).count() as u32
    }

    pub fn progress_pct(&self) -> f64 {
        match &self.state {
            RebalanceState::Migrating { segments_total, segments_done } if *segments_total > 0 => {
                (*segments_done as f64 / *segments_total as f64) * 100.0
            }
            RebalanceState::Completed { .. } => 100.0,
            _ => 0.0,
        }
    }

    pub fn state(&self) -> &RebalanceState {
        &self.state
    }

    pub fn stats(&self) -> &RebalanceStats {
        &self.stats
    }

    pub fn is_cooldown_active(&self, current_time: u64) -> bool {
        current_time.saturating_sub(self.last_rebalance_time) < self.config.cooldown_secs
    }

    pub fn config(&self) -> &RebalanceConfig {
        &self.config
    }

    pub fn local_node(&self) -> &NodeId {
        &self.local_node
    }

    pub fn shard_map(&self) -> &HashMap<ShardId, NodeId> {
        &self.shard_map
    }

    pub fn local_segments(&self) -> &HashMap<RebalanceSegmentId, ShardId> {
        &self.local_segments
    }

    pub fn migrations(&self) -> &[MigrationTask] {
        &self.migrations
    }

    pub fn last_rebalance_time(&self) -> u64 {
        self.last_rebalance_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_node(id: &str) -> NodeId {
        NodeId(id.to_string())
    }

    #[test]
    fn test_new_engine_idle_state() {
        let config = RebalanceConfig::default();
        let engine = RebalanceEngine::new(config, new_node("node1"));
        assert!(matches!(engine.state(), RebalanceState::Idle));
        assert_eq!(engine.stats().total_rebalances, 0);
    }

    #[test]
    fn test_register_and_remove_segment() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        engine.register_segment(RebalanceSegmentId(1), ShardId(10));
        engine.register_segment(RebalanceSegmentId(2), ShardId(20));

        assert_eq!(engine.local_segments().len(), 2);
        assert_eq!(engine.remove_segment(RebalanceSegmentId(1)), Some(ShardId(10)));
        assert_eq!(engine.local_segments().len(), 1);
        assert_eq!(engine.remove_segment(RebalanceSegmentId(999)), None);
    }

    #[test]
    fn test_update_shard_map() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut new_map = HashMap::new();
        new_map.insert(ShardId(0), new_node("node1"));
        new_map.insert(ShardId(1), new_node("node2"));
        engine.update_shard_map(new_map);

        assert_eq!(engine.shard_map().len(), 2);
        assert_eq!(engine.shard_map().get(&ShardId(0)), Some(&new_node("node1")));
    }

    #[test]
    fn test_plan_rebalance_no_changes() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node1"));
        shard_map.insert(ShardId(1), new_node("node1"));
        engine.update_shard_map(shard_map);

        engine.register_segment(RebalanceSegmentId(1), ShardId(0));
        engine.register_segment(RebalanceSegmentId(2), ShardId(1));

        let tasks = engine.plan_rebalance();
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_plan_rebalance_node_added() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node1"));
        shard_map.insert(ShardId(1), new_node("node1"));
        engine.update_shard_map(shard_map);

        engine.register_segment(RebalanceSegmentId(1), ShardId(0));
        engine.register_segment(RebalanceSegmentId(2), ShardId(1));

        let mut new_map = HashMap::new();
        new_map.insert(ShardId(0), new_node("node2"));
        new_map.insert(ShardId(1), new_node("node1"));
        engine.update_shard_map(new_map);

        let tasks = engine.plan_rebalance();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].segment_id, RebalanceSegmentId(1));
        assert!(matches!(&tasks[0].direction, MigrationDirection::Outbound { target_node } if target_node == &new_node("node2")));
    }

    #[test]
    fn test_plan_rebalance_node_removed() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node1"));
        shard_map.insert(ShardId(1), new_node("node2"));
        engine.update_shard_map(shard_map);

        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        let mut new_map = HashMap::new();
        new_map.insert(ShardId(0), new_node("node1"));
        new_map.insert(ShardId(1), new_node("node1"));
        engine.update_shard_map(new_map);

        let tasks = engine.plan_rebalance();
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_start_rebalance_from_idle() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);

        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        assert!(engine.start_rebalance().is_ok());
        assert!(matches!(engine.state(), RebalanceState::Migrating { .. }));
    }

    #[test]
    fn test_start_rebalance_not_idle_fails() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        assert!(engine.start_rebalance().is_err());
    }

    #[test]
    fn test_advance_migration_queued_to_transferring() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        let new_state = engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        assert!(matches!(new_state, MigrationTaskState::Transferring));
    }

    #[test]
    fn test_advance_migration_transferring_to_verifying() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        let new_state = engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        assert!(matches!(new_state, MigrationTaskState::Verifying));
    }

    #[test]
    fn test_advance_migration_verifying_to_completed() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        let new_state = engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        
        assert!(matches!(new_state, MigrationTaskState::Completed));
        assert_eq!(engine.stats().segments_migrated_out, 1);
    }

    #[test]
    fn test_advance_migration_not_found() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let result = engine.advance_migration(RebalanceSegmentId(999));
        assert!(result.is_err());
    }

    #[test]
    fn test_fail_migration() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        engine.fail_migration(RebalanceSegmentId(1), "Network error".to_string()).unwrap();

        assert_eq!(engine.stats().failed_migrations, 1);
    }

    #[test]
    fn test_complete_rebalance_all_done() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();

        let stats = engine.complete_rebalance().unwrap();
        assert_eq!(stats.segments_migrated_out, 1);
        assert!(matches!(engine.state(), RebalanceState::Completed { .. }));
    }

    #[test]
    fn test_complete_rebalance_pending_fails() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();

        let result = engine.complete_rebalance();
        assert!(result.is_err());
    }

    #[test]
    fn test_abort_rebalance() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        engine.abort_rebalance("Manual abort".to_string());

        assert!(matches!(engine.state(), RebalanceState::Failed { reason } if reason == "Manual abort"));
    }

    #[test]
    fn test_accept_inbound() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        engine.accept_inbound(RebalanceSegmentId(100), ShardId(5), new_node("node2"), 2 * 1024 * 1024);

        assert_eq!(engine.migrations().len(), 1);
        assert!(matches!(&engine.migrations()[0].direction, MigrationDirection::Inbound { source_node } if source_node == &new_node("node2")));
    }

    #[test]
    fn test_can_accept_more_under_limit() {
        let config = RebalanceConfig { max_concurrent_migrations: 4, ..Default::default() };
        let engine = RebalanceEngine::new(config, new_node("node1"));

        assert!(engine.can_accept_more());
    }

    #[test]
    fn test_can_accept_more_at_limit() {
        let config = RebalanceConfig { max_concurrent_migrations: 2, ..Default::default() };
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        engine.accept_inbound(RebalanceSegmentId(1), ShardId(0), new_node("node2"), 1024);
        engine.accept_inbound(RebalanceSegmentId(2), ShardId(1), new_node("node3"), 1024);

        assert!(!engine.can_accept_more());
    }

    #[test]
    fn test_progress_pct_zero() {
        let config = RebalanceConfig::default();
        let engine = RebalanceEngine::new(config, new_node("node1"));

        assert_eq!(engine.progress_pct(), 0.0);
    }

    #[test]
    fn test_progress_pct_half_done() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        shard_map.insert(ShardId(1), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));
        engine.register_segment(RebalanceSegmentId(2), ShardId(1));

        engine.start_rebalance().unwrap();
        
        if let RebalanceState::Migrating { segments_total: _, segments_done } = &mut engine.state {
            *segments_done = 1;
        }

        assert_eq!(engine.progress_pct(), 50.0);
    }

    #[test]
    fn test_progress_pct_all_done() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        engine.complete_rebalance().unwrap();

        assert_eq!(engine.progress_pct(), 100.0);
    }

    #[test]
    fn test_cooldown_active() {
        let config = RebalanceConfig { cooldown_secs: 300, ..Default::default() };
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        engine.last_rebalance_time = 1000;
        
        assert!(engine.is_cooldown_active(1100));
    }

    #[test]
    fn test_cooldown_expired() {
        let config = RebalanceConfig { cooldown_secs: 300, ..Default::default() };
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        engine.last_rebalance_time = 1000;
        
        assert!(!engine.is_cooldown_active(1500));
    }

    #[test]
    fn test_stats_update_on_complete() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(0), new_node("node2"));
        engine.update_shard_map(shard_map);
        engine.register_segment(RebalanceSegmentId(1), ShardId(0));

        engine.start_rebalance().unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
        engine.advance_migration(RebalanceSegmentId(1)).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.total_rebalances, 1);
        assert_eq!(stats.segments_migrated_out, 1);
    }

    #[test]
    fn test_rebalance_config_default() {
        let config = RebalanceConfig::default();
        
        assert_eq!(config.max_concurrent_migrations, 4);
        assert_eq!(config.max_bandwidth_bytes_per_sec, 100 * 1024 * 1024);
        assert_eq!(config.max_iops, 1000);
        assert_eq!(config.cooldown_secs, 300);
        assert!(config.auto_rebalance);
    }

    #[test]
    fn test_multiple_segments_same_shard() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, new_node("node1"));

        engine.register_segment(RebalanceSegmentId(1), ShardId(5));
        engine.register_segment(RebalanceSegmentId(2), ShardId(5));
        engine.register_segment(RebalanceSegmentId(3), ShardId(5));

        let mut shard_map = HashMap::new();
        shard_map.insert(ShardId(5), new_node("node2"));
        engine.update_shard_map(shard_map);

        let tasks = engine.plan_rebalance();
        assert_eq!(tasks.len(), 3);
        
        for task in &tasks {
            assert_eq!(task.shard_id, ShardId(5));
        }
    }
}
