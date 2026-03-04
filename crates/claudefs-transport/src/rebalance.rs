//! Shard rebalancing coordinator for cluster topology changes.
//!
//! Coordinates shard rebalancing when nodes join or leave the cluster.
//! When a new node joins, determines which shards should migrate to the new node.
//! When a node leaves/fails, redistributes affected shards to remaining healthy nodes.

use crate::shard_map::VirtualShard;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Configuration for rebalancing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceConfig {
    /// Max concurrent shard migrations at once (default: 4).
    pub max_concurrent_migrations: usize,
    /// Target shard count deviation allowed before triggering rebalance (default: 2).
    pub imbalance_threshold: usize,
    /// Cooldown period between rebalance passes in ms (default: 30_000).
    pub cooldown_ms: u64,
    /// Minimum cluster size before rebalancing is triggered (default: 2).
    pub min_cluster_size: usize,
}

impl Default for RebalanceConfig {
    fn default() -> Self {
        Self {
            max_concurrent_migrations: 4,
            imbalance_threshold: 2,
            cooldown_ms: 30_000,
            min_cluster_size: 2,
        }
    }
}

/// Reason for a rebalance being triggered.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RebalanceTrigger {
    /// A new node joined the cluster.
    NodeJoined {
        /// Node ID that joined.
        node_id: [u8; 16],
    },
    /// A node left or failed.
    NodeLeft {
        /// Node ID that left.
        node_id: [u8; 16],
    },
    /// Manual request from administrator.
    ManualRequest,
    /// Periodic check triggered.
    PeriodicCheck,
}

/// State of a migration task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MigrationTaskState {
    /// Task created, not yet started.
    Pending,
    /// Migration in progress.
    InProgress,
    /// Migration completed successfully.
    Completed,
    /// Migration failed.
    Failed {
        /// Failure reason.
        reason: String,
    },
    /// Task cancelled.
    Cancelled,
}

/// A single shard migration task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationTask {
    /// Unique task identifier.
    pub task_id: u64,
    /// Shard to migrate.
    pub shard: VirtualShard,
    /// Source node.
    pub from_node: [u8; 16],
    /// Destination node.
    pub to_node: [u8; 16],
    /// Reason for this migration.
    pub trigger: RebalanceTrigger,
    /// Current state of the migration.
    pub state: MigrationTaskState,
    /// When this task was created (ms since epoch).
    pub created_at_ms: u64,
    /// When this task completed (ms since epoch), if applicable.
    pub completed_at_ms: Option<u64>,
    /// Bytes transferred so far.
    pub bytes_transferred: u64,
}

/// Rebalance plan: a set of migration tasks computed by the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalancePlan {
    /// Unique plan identifier.
    pub plan_id: u64,
    /// Reason for this rebalance.
    pub trigger: RebalanceTrigger,
    /// Migration tasks to execute.
    pub tasks: Vec<MigrationTask>,
    /// When this plan was created (ms since epoch).
    pub created_at_ms: u64,
    /// Estimated total bytes to transfer.
    pub estimated_bytes: u64,
}

/// Error types for rebalance operations.
#[derive(Debug, Error)]
pub enum RebalanceError {
    /// Node not found.
    #[error("node {0:?} not found")]
    NodeNotFound([u8; 16]),
    /// Node already exists.
    #[error("node {0:?} already exists")]
    NodeAlreadyExists([u8; 16]),
    /// No rebalance needed - cluster is balanced.
    #[error("no rebalance needed: cluster is balanced")]
    NoPlanNeeded,
    /// Task not found.
    #[error("task {0} not found")]
    TaskNotFound(u64),
    /// A plan is already in progress.
    #[error("plan already in progress")]
    PlanInProgress,
    /// Cannot remove last node.
    #[error("cannot remove last node: no destination for shards")]
    NoDestination,
}

/// Statistics for rebalance operations.
pub struct RebalanceStats {
    pub active_nodes: AtomicU64,
    pub total_plans_computed: AtomicU64,
    pub total_tasks_created: AtomicU64,
    pub total_tasks_completed: AtomicU64,
    pub total_tasks_failed: AtomicU64,
    pub total_bytes_transferred: AtomicU64,
}

impl RebalanceStats {
    pub fn new() -> Self {
        Self {
            active_nodes: AtomicU64::new(0),
            total_plans_computed: AtomicU64::new(0),
            total_tasks_created: AtomicU64::new(0),
            total_tasks_completed: AtomicU64::new(0),
            total_tasks_failed: AtomicU64::new(0),
            total_bytes_transferred: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> RebalanceStatsSnapshot {
        RebalanceStatsSnapshot {
            active_nodes: self.active_nodes.load(Ordering::Relaxed),
            total_plans_computed: self.total_plans_computed.load(Ordering::Relaxed),
            total_tasks_created: self.total_tasks_created.load(Ordering::Relaxed),
            total_tasks_completed: self.total_tasks_completed.load(Ordering::Relaxed),
            total_tasks_failed: self.total_tasks_failed.load(Ordering::Relaxed),
            total_bytes_transferred: self.total_bytes_transferred.load(Ordering::Relaxed),
        }
    }
}

impl Default for RebalanceStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of rebalance statistics at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceStatsSnapshot {
    /// Number of active nodes in the cluster.
    pub active_nodes: u64,
    /// Total rebalance plans computed.
    pub total_plans_computed: u64,
    /// Total migration tasks created.
    pub total_tasks_created: u64,
    /// Total migration tasks completed.
    pub total_tasks_completed: u64,
    /// Total migration tasks failed.
    pub total_tasks_failed: u64,
    /// Total bytes transferred during migrations.
    pub total_bytes_transferred: u64,
}

struct NodeInfo {
    node_id: [u8; 16],
    capacity_shards: usize,
    current_shards: Vec<VirtualShard>,
}

/// The rebalance coordinator.
pub struct RebalanceCoordinator {
    config: RebalanceConfig,
    nodes: RwLock<HashMap<[u8; 16], NodeInfo>>,
    tasks: RwLock<HashMap<u64, MigrationTask>>,
    plan_in_progress: RwLock<Option<u64>>,
    total_shards: u32,
    next_task_id: AtomicU64,
    next_plan_id: AtomicU64,
    stats: Arc<RebalanceStats>,
    last_plan_ms: RwLock<u64>,
}

impl RebalanceCoordinator {
    /// Create a new rebalance coordinator with the given configuration.
    pub fn new(config: RebalanceConfig) -> Self {
        Self {
            config: config.clone(),
            nodes: RwLock::new(HashMap::new()),
            tasks: RwLock::new(HashMap::new()),
            plan_in_progress: RwLock::new(None),
            total_shards: 256,
            next_task_id: AtomicU64::new(1),
            next_plan_id: AtomicU64::new(1),
            stats: Arc::new(RebalanceStats::new()),
            last_plan_ms: RwLock::new(0),
        }
    }

    /// Register a node as active.
    pub fn add_node(
        &self,
        node_id: [u8; 16],
        capacity_shards: usize,
    ) -> Result<(), RebalanceError> {
        let mut nodes = self
            .nodes
            .write()
            .map_err(|_| RebalanceError::NodeNotFound(node_id))?;

        if nodes.contains_key(&node_id) {
            return Err(RebalanceError::NodeAlreadyExists(node_id));
        }

        nodes.insert(
            node_id,
            NodeInfo {
                node_id,
                capacity_shards,
                current_shards: Vec::new(),
            },
        );

        self.stats.active_nodes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Mark a node as gone (failed or graceful departure).
    pub fn remove_node(
        &self,
        node_id: [u8; 16],
        trigger: RebalanceTrigger,
        now_ms: u64,
    ) -> Result<RebalancePlan, RebalanceError> {
        let shards_to_redistribute = {
            let mut nodes = self
                .nodes
                .write()
                .map_err(|_| RebalanceError::NodeNotFound(node_id))?;

            let node_info = nodes
                .remove(&node_id)
                .ok_or(RebalanceError::NodeNotFound(node_id))?;

            if nodes.is_empty() {
                return Err(RebalanceError::NoDestination);
            }

            self.stats.active_nodes.fetch_sub(1, Ordering::Relaxed);
            node_info.current_shards
        };

        if shards_to_redistribute.is_empty() {
            return Err(RebalanceError::NoPlanNeeded);
        }

        self.redistribute_shards(shards_to_redistribute, trigger, now_ms)
    }

    fn redistribute_shards(
        &self,
        shards: Vec<VirtualShard>,
        trigger: RebalanceTrigger,
        now_ms: u64,
    ) -> Result<RebalancePlan, RebalanceError> {
        let nodes = self
            .nodes
            .read()
            .map_err(|_| RebalanceError::NoPlanNeeded)?;

        if nodes.is_empty() {
            return Err(RebalanceError::NoDestination);
        }

        let node_ids: Vec<[u8; 16]> = nodes.keys().cloned().collect();
        let mut tasks = Vec::new();

        for (i, shard) in shards.iter().enumerate() {
            let target_node = node_ids[i % node_ids.len()];

            let task = MigrationTask {
                task_id: self.next_task_id.fetch_add(1, Ordering::Relaxed),
                shard: *shard,
                from_node: [0u8; 16],
                to_node: target_node,
                trigger,
                state: MigrationTaskState::Pending,
                created_at_ms: now_ms,
                completed_at_ms: None,
                bytes_transferred: 0,
            };

            tasks.push(task);
            self.stats
                .total_tasks_created
                .fetch_add(1, Ordering::Relaxed);
        }

        let plan_id = self.next_plan_id.fetch_add(1, Ordering::Relaxed);
        let plan = RebalancePlan {
            plan_id,
            trigger,
            tasks: tasks.clone(),
            created_at_ms: now_ms,
            estimated_bytes: shards.len() as u64 * 64_000_000,
        };

        let mut tasks_lock = self.tasks.write().unwrap_or_else(|e| e.into_inner());
        for task in tasks {
            tasks_lock.insert(task.task_id, task);
        }

        self.stats
            .total_plans_computed
            .fetch_add(1, Ordering::Relaxed);

        Ok(plan)
    }

    /// Compute a rebalance plan for the current cluster state.
    pub fn compute_plan(
        &self,
        trigger: RebalanceTrigger,
        now_ms: u64,
    ) -> Result<RebalancePlan, RebalanceError> {
        let nodes = self
            .nodes
            .read()
            .map_err(|_| RebalanceError::NoPlanNeeded)?;

        if nodes.len() < self.config.min_cluster_size {
            return Err(RebalanceError::NoPlanNeeded);
        }

        let last_plan = self.last_plan_ms.read().map(|l| *l).unwrap_or(0);
        if now_ms.saturating_sub(last_plan) < self.config.cooldown_ms {
            return Err(RebalanceError::NoPlanNeeded);
        }

        let target_per_node = self.total_shards as f64 / nodes.len() as f64;
        let threshold = target_per_node as i64 + self.config.imbalance_threshold as i64;

        let mut node_shard_counts: Vec<(&[u8; 16], usize)> = nodes
            .iter()
            .map(|(id, info)| (id, info.current_shards.len()))
            .collect();

        node_shard_counts.sort_by_key(|(_, count)| *count);

        let min_count = node_shard_counts.first().map(|(_, c)| *c).unwrap_or(0);
        let max_count = node_shard_counts.last().map(|(_, c)| *c).unwrap_or(0);

        if (max_count as i64 - min_count as i64) <= threshold as i64 {
            return Err(RebalanceError::NoPlanNeeded);
        }

        let most_loaded = node_shard_counts.last().map(|(id, _)| **id).unwrap();
        let least_loaded = node_shard_counts.first().map(|(id, _)| **id).unwrap();

        let num_to_move = ((max_count - min_count) / 2).max(1);
        let mut tasks = Vec::new();

        let mut nodes_write = self.nodes.write().unwrap_or_else(|e| e.into_inner());

        if let Some(source_node) = nodes_write.get_mut(&most_loaded) {
            let shards_to_move: Vec<VirtualShard> = source_node
                .current_shards
                .drain(..num_to_move.min(source_node.current_shards.len()))
                .collect();

            for shard in shards_to_move {
                let task = MigrationTask {
                    task_id: self.next_task_id.fetch_add(1, Ordering::Relaxed),
                    shard,
                    from_node: most_loaded,
                    to_node: least_loaded,
                    trigger,
                    state: MigrationTaskState::Pending,
                    created_at_ms: now_ms,
                    completed_at_ms: None,
                    bytes_transferred: 0,
                };

                tasks.push(task);
                self.stats
                    .total_tasks_created
                    .fetch_add(1, Ordering::Relaxed);
            }

            if let Some(target_node) = nodes_write.get_mut(&least_loaded) {
                for task in &tasks {
                    target_node.current_shards.push(task.shard);
                }
            }
        }

        if tasks.is_empty() {
            return Err(RebalanceError::NoPlanNeeded);
        }

        let plan_id = self.next_plan_id.fetch_add(1, Ordering::Relaxed);
        let plan = RebalancePlan {
            plan_id,
            trigger,
            tasks: tasks.clone(),
            created_at_ms: now_ms,
            estimated_bytes: tasks.len() as u64 * 64_000_000,
        };

        drop(nodes_write);

        let mut tasks_lock = self.tasks.write().unwrap_or_else(|e| e.into_inner());
        for task in tasks {
            tasks_lock.insert(task.task_id, task);
        }

        *self.last_plan_ms.write().unwrap_or_else(|e| e.into_inner()) = now_ms;
        self.stats
            .total_plans_computed
            .fetch_add(1, Ordering::Relaxed);

        Ok(plan)
    }

    /// Acknowledge a migration task as completed.
    pub fn complete_task(
        &self,
        task_id: u64,
        bytes_transferred: u64,
        now_ms: u64,
    ) -> Result<(), RebalanceError> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| RebalanceError::TaskNotFound(task_id))?;

        let task = tasks
            .get_mut(&task_id)
            .ok_or(RebalanceError::TaskNotFound(task_id))?;

        match &task.state {
            MigrationTaskState::Pending | MigrationTaskState::InProgress => {
                task.state = MigrationTaskState::Completed;
                task.completed_at_ms = Some(now_ms);
                task.bytes_transferred = bytes_transferred;

                self.stats
                    .total_tasks_completed
                    .fetch_add(1, Ordering::Relaxed);
                self.stats
                    .total_bytes_transferred
                    .fetch_add(bytes_transferred, Ordering::Relaxed);
                Ok(())
            }
            _ => Err(RebalanceError::TaskNotFound(task_id)),
        }
    }

    /// Acknowledge a migration task as failed.
    pub fn fail_task(&self, task_id: u64, reason: String) -> Result<(), RebalanceError> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| RebalanceError::TaskNotFound(task_id))?;

        let task = tasks
            .get_mut(&task_id)
            .ok_or(RebalanceError::TaskNotFound(task_id))?;

        match &task.state {
            MigrationTaskState::Pending | MigrationTaskState::InProgress => {
                task.state = MigrationTaskState::Failed { reason };
                self.stats
                    .total_tasks_failed
                    .fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            _ => Err(RebalanceError::TaskNotFound(task_id)),
        }
    }

    /// Cancel a pending migration task.
    pub fn cancel_task(&self, task_id: u64) -> Result<(), RebalanceError> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| RebalanceError::TaskNotFound(task_id))?;

        let task = tasks
            .get_mut(&task_id)
            .ok_or(RebalanceError::TaskNotFound(task_id))?;

        match &task.state {
            MigrationTaskState::Pending => {
                task.state = MigrationTaskState::Cancelled;
                Ok(())
            }
            _ => Err(RebalanceError::TaskNotFound(task_id)),
        }
    }

    /// List all active/pending migration tasks.
    pub fn active_tasks(&self) -> Vec<MigrationTask> {
        let tasks = match self.tasks.read() {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };

        tasks
            .values()
            .filter(|t| {
                matches!(
                    t.state,
                    MigrationTaskState::Pending | MigrationTaskState::InProgress
                )
            })
            .cloned()
            .collect()
    }

    /// Get stats snapshot.
    pub fn stats(&self) -> RebalanceStatsSnapshot {
        self.stats.snapshot()
    }

    fn set_initial_shards(&self, node_id: [u8; 16], shards: Vec<VirtualShard>) {
        if let Ok(mut nodes) = self.nodes.write() {
            if let Some(node) = nodes.get_mut(&node_id) {
                node.current_shards = shards;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_id(seed: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[0] = seed;
        id
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn test_create_coordinator() {
        let config = RebalanceConfig::default();
        let coordinator = RebalanceCoordinator::new(config);
        let stats = coordinator.stats();
        assert_eq!(stats.active_nodes, 0);
    }

    #[test]
    fn test_add_single_node() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());

        let result = coordinator.add_node(make_node_id(1), 100);
        assert!(result.is_ok());

        let stats = coordinator.stats();
        assert_eq!(stats.active_nodes, 1);
    }

    #[test]
    fn test_add_duplicate_node() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());

        coordinator.add_node(make_node_id(1), 100).unwrap();
        let result = coordinator.add_node(make_node_id(1), 100);

        assert!(matches!(result, Err(RebalanceError::NodeAlreadyExists(_))));
    }

    #[test]
    fn test_remove_unknown_node() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());

        let result = coordinator.remove_node(
            make_node_id(1),
            RebalanceTrigger::NodeLeft {
                node_id: make_node_id(1),
            },
            now_ms(),
        );
        assert!(matches!(result, Err(RebalanceError::NodeNotFound(_))));
    }

    #[test]
    fn test_remove_last_node() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());
        coordinator.add_node(make_node_id(1), 100).unwrap();

        let result = coordinator.remove_node(
            make_node_id(1),
            RebalanceTrigger::NodeLeft {
                node_id: make_node_id(1),
            },
            now_ms(),
        );
        assert!(matches!(result, Err(RebalanceError::NoDestination)));
    }

    #[test]
    fn test_compute_plan_single_node() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());
        coordinator.add_node(make_node_id(1), 100).unwrap();

        let result = coordinator.compute_plan(RebalanceTrigger::PeriodicCheck, now_ms());
        assert!(matches!(result, Err(RebalanceError::NoPlanNeeded)));
    }

    #[test]
    fn test_compute_plan_balanced_cluster() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![VirtualShard(0), VirtualShard(1), VirtualShard(2)],
        );
        coordinator.set_initial_shards(
            make_node_id(2),
            vec![VirtualShard(3), VirtualShard(4), VirtualShard(5)],
        );

        let result = coordinator.compute_plan(RebalanceTrigger::PeriodicCheck, now_ms());
        assert!(matches!(result, Err(RebalanceError::NoPlanNeeded)));
    }

    #[test]
    fn test_compute_plan_imbalanced() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let result = coordinator.compute_plan(RebalanceTrigger::PeriodicCheck, now_ms());
        assert!(result.is_ok());
    }

    #[test]
    fn test_complete_task() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let plan = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms())
            .unwrap();
        let task_id = plan.tasks[0].task_id;

        let result = coordinator.complete_task(task_id, 1_000_000, now_ms());
        assert!(result.is_ok());

        let stats = coordinator.stats();
        assert_eq!(stats.total_tasks_completed, 1);
    }

    #[test]
    fn test_fail_task() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let plan = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms())
            .unwrap();
        let task_id = plan.tasks[0].task_id;

        let result = coordinator.fail_task(task_id, "disk error".to_string());
        assert!(result.is_ok());

        let stats = coordinator.stats();
        assert_eq!(stats.total_tasks_failed, 1);
    }

    #[test]
    fn test_cancel_task() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let plan = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms())
            .unwrap();
        let task_id = plan.tasks[0].task_id;

        let result = coordinator.cancel_task(task_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_complete_unknown_task() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());

        let result = coordinator.complete_task(999, 0, now_ms());
        assert!(matches!(result, Err(RebalanceError::TaskNotFound(_))));
    }

    #[test]
    fn test_fail_unknown_task() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());

        let result = coordinator.fail_task(999, "error".to_string());
        assert!(matches!(result, Err(RebalanceError::TaskNotFound(_))));
    }

    #[test]
    fn test_cancel_unknown_task() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());

        let result = coordinator.cancel_task(999);
        assert!(matches!(result, Err(RebalanceError::TaskNotFound(_))));
    }

    #[test]
    fn test_active_tasks_empty() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());

        let tasks = coordinator.active_tasks();
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_active_tasks_returns_pending() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let _ = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms())
            .unwrap();

        let tasks = coordinator.active_tasks();
        assert!(!tasks.is_empty());
    }

    #[test]
    fn test_stats_snapshot() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());
        coordinator.add_node(make_node_id(1), 100).unwrap();

        let snapshot = coordinator.stats();

        assert_eq!(snapshot.active_nodes, 1);
    }

    #[test]
    fn test_config_defaults() {
        let config = RebalanceConfig::default();

        assert_eq!(config.max_concurrent_migrations, 4);
        assert_eq!(config.imbalance_threshold, 2);
        assert_eq!(config.cooldown_ms, 30_000);
        assert_eq!(config.min_cluster_size, 2);
    }

    #[test]
    fn test_migration_task_state_variants() {
        let pending = MigrationTaskState::Pending;
        let in_progress = MigrationTaskState::InProgress;
        let completed = MigrationTaskState::Completed;
        let failed = MigrationTaskState::Failed {
            reason: "test".to_string(),
        };
        let cancelled = MigrationTaskState::Cancelled;

        assert!(matches!(pending, MigrationTaskState::Pending));
        assert!(matches!(in_progress, MigrationTaskState::InProgress));
        assert!(matches!(completed, MigrationTaskState::Completed));
        assert!(matches!(failed, MigrationTaskState::Failed { .. }));
        assert!(matches!(cancelled, MigrationTaskState::Cancelled));
    }

    #[test]
    fn test_rebalance_trigger_variants() {
        let node_joined = RebalanceTrigger::NodeJoined {
            node_id: make_node_id(1),
        };
        let node_left = RebalanceTrigger::NodeLeft {
            node_id: make_node_id(1),
        };
        let manual = RebalanceTrigger::ManualRequest;
        let periodic = RebalanceTrigger::PeriodicCheck;

        assert!(matches!(node_joined, RebalanceTrigger::NodeJoined { .. }));
        assert!(matches!(node_left, RebalanceTrigger::NodeLeft { .. }));
        assert!(matches!(manual, RebalanceTrigger::ManualRequest));
        assert!(matches!(periodic, RebalanceTrigger::PeriodicCheck));
    }

    #[test]
    fn test_remove_node_redistributes() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig::default());
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(make_node_id(1), vec![VirtualShard(0), VirtualShard(1)]);

        let result = coordinator.remove_node(
            make_node_id(1),
            RebalanceTrigger::NodeLeft {
                node_id: make_node_id(1),
            },
            now_ms(),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_plan_creation() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let plan = coordinator
            .compute_plan(
                RebalanceTrigger::NodeJoined {
                    node_id: make_node_id(3),
                },
                now_ms(),
            )
            .unwrap();

        assert!(!plan.tasks.is_empty());
        assert_eq!(plan.estimated_bytes > 0, true);
    }

    #[test]
    fn test_task_id_unique() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            cooldown_ms: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let plan1 = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms())
            .unwrap();
        let plan2 = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms() + 100_000)
            .unwrap();

        assert_ne!(plan1.plan_id, plan2.plan_id);
    }

    #[test]
    fn test_cooldown_prevents_frequent_plans() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            cooldown_ms: 60_000,
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let _ = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms())
            .unwrap();
        let result = coordinator.compute_plan(RebalanceTrigger::PeriodicCheck, now_ms() + 10_000);

        assert!(matches!(result, Err(RebalanceError::NoPlanNeeded)));
    }

    #[test]
    fn test_complete_completed_task_fails() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let plan = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms())
            .unwrap();
        let task_id = plan.tasks[0].task_id;

        coordinator
            .complete_task(task_id, 1_000_000, now_ms())
            .unwrap();
        let result = coordinator.complete_task(task_id, 1_000_000, now_ms());

        assert!(matches!(result, Err(RebalanceError::TaskNotFound(_))));
    }

    #[test]
    fn test_cancel_completed_task_fails() {
        let coordinator = RebalanceCoordinator::new(RebalanceConfig {
            imbalance_threshold: 0,
            ..Default::default()
        });
        coordinator.add_node(make_node_id(1), 50).unwrap();
        coordinator.add_node(make_node_id(2), 50).unwrap();

        coordinator.set_initial_shards(
            make_node_id(1),
            vec![
                VirtualShard(0),
                VirtualShard(1),
                VirtualShard(2),
                VirtualShard(3),
                VirtualShard(4),
                VirtualShard(5),
            ],
        );

        let plan = coordinator
            .compute_plan(RebalanceTrigger::PeriodicCheck, now_ms())
            .unwrap();
        let task_id = plan.tasks[0].task_id;

        coordinator
            .complete_task(task_id, 1_000_000, now_ms())
            .unwrap();
        let result = coordinator.cancel_task(task_id);

        assert!(matches!(result, Err(RebalanceError::TaskNotFound(_))));
    }
}
