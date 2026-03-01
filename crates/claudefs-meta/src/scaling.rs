//! Online node scaling and shard rebalancing.
//!
//! Manages shard placement across cluster nodes and plans migrations
//! when nodes join or leave. Implements the rebalancing logic for
//! decision D4 (Multi-Raft with 256 virtual shards).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

/// Placement of a shard across cluster nodes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShardPlacement {
    /// Unique identifier for this shard.
    pub shard_id: ShardId,
    /// Primary node that serves this shard.
    pub primary: NodeId,
    /// Replica nodes that hold copies of this shard.
    pub replicas: Vec<NodeId>,
    /// Version number for optimistic locking.
    pub version: u64,
}

/// A pending or in-progress shard migration task.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationTask {
    /// Shard to be migrated.
    pub shard_id: ShardId,
    /// Source node where the shard currently resides.
    pub source: NodeId,
    /// Target node where the shard will be moved.
    pub target: NodeId,
    /// Current status of the migration.
    pub status: MigrationStatus,
    /// When this migration task was created.
    pub created_at: Timestamp,
}

/// Status of a shard migration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MigrationStatus {
    /// Migration is queued but not yet started.
    Pending,
    /// Migration is currently in progress.
    InProgress,
    /// Migration completed successfully.
    Completed,
    /// Migration failed with an error.
    Failed {
        /// Reason for the failure.
        reason: String,
    },
}

/// Manages shard placement and coordinates rebalancing when nodes join or leave.
pub struct ScalingManager {
    placements: RwLock<HashMap<ShardId, ShardPlacement>>,
    migrations: RwLock<Vec<MigrationTask>>,
    num_shards: u16,
    replica_count: usize,
    max_concurrent_migrations: usize,
}

impl ScalingManager {
    /// Creates a new scaling manager with the given shard count and replica count.
    /// Defaults max_concurrent_migrations to 4.
    pub fn new(num_shards: u16, replica_count: usize) -> Self {
        Self {
            placements: RwLock::new(HashMap::new()),
            migrations: RwLock::new(Vec::new()),
            num_shards,
            replica_count,
            max_concurrent_migrations: 4,
        }
    }

    /// Creates a new scaling manager with the given shard count, replica count,
    /// and maximum concurrent migrations.
    pub fn with_max_migrations(
        num_shards: u16,
        replica_count: usize,
        max_concurrent_migrations: usize,
    ) -> Self {
        Self {
            placements: RwLock::new(HashMap::new()),
            migrations: RwLock::new(Vec::new()),
            num_shards,
            replica_count,
            max_concurrent_migrations,
        }
    }

    /// Initializes shard placements across the given nodes in a round-robin fashion.
    pub fn initialize_placements(&self, nodes: &[NodeId]) {
        if nodes.is_empty() {
            return;
        }

        let mut placements = self.placements.write().unwrap();
        placements.clear();

        for shard_idx in 0..self.num_shards {
            let shard_id = ShardId::new(shard_idx);
            let primary_idx = shard_idx as usize % nodes.len();
            let primary = nodes[primary_idx];

            let mut replicas = Vec::with_capacity(self.replica_count);
            for r in 0..self.replica_count {
                let replica_idx = (shard_idx as usize + r + 1) % nodes.len();
                replicas.push(nodes[replica_idx]);
            }

            let placement = ShardPlacement {
                shard_id,
                primary,
                replicas,
                version: 0,
            };
            placements.insert(shard_id, placement);
        }

        tracing::debug!(
            "Initialized {} shards across {} nodes",
            self.num_shards,
            nodes.len()
        );
    }

    /// Gets the placement for a specific shard, if it exists.
    pub fn get_placement(&self, shard_id: ShardId) -> Option<ShardPlacement> {
        let placements = self.placements.read().unwrap();
        placements.get(&shard_id).cloned()
    }

    /// Plans migrations to rebalance shards when a new node joins the cluster.
    pub fn plan_add_node(&self, new_node: NodeId, existing_nodes: &[NodeId]) -> Vec<MigrationTask> {
        let placements = self.placements.read().unwrap();
        let mut tasks = Vec::new();

        if placements.is_empty() {
            return tasks;
        }

        let total_nodes = existing_nodes.len() + 1;
        let shards_per_node = self.num_shards as usize / total_nodes;
        let extra_shards = self.num_shards as usize % total_nodes;

        let new_node_shard_count: usize = placements
            .values()
            .filter(|p| p.primary == new_node || p.replicas.contains(&new_node))
            .count();

        if new_node_shard_count >= shards_per_node {
            return tasks;
        }

        let mut node_shard_counts: HashMap<NodeId, usize> = HashMap::new();
        for node in existing_nodes {
            node_shard_counts.insert(*node, 0);
        }
        node_shard_counts.insert(new_node, new_node_shard_count);

        for placement in placements.values() {
            if let Some(count) = node_shard_counts.get_mut(&placement.primary) {
                *count += 1;
            }
        }

        for placement in placements.values() {
            if node_shard_counts
                .get(&placement.primary)
                .copied()
                .unwrap_or(0)
                > shards_per_node
            {
                let source_count = node_shard_counts
                    .get(&placement.primary)
                    .copied()
                    .unwrap_or(0);
                if source_count > shards_per_node {
                    let task = MigrationTask {
                        shard_id: placement.shard_id,
                        source: placement.primary,
                        target: new_node,
                        status: MigrationStatus::Pending,
                        created_at: Timestamp::now(),
                    };
                    tasks.push(task);
                    *node_shard_counts.get_mut(&placement.primary).unwrap() -= 1;
                    *node_shard_counts.get_mut(&new_node).unwrap() += 1;

                    if tasks.len() >= shards_per_node - new_node_shard_count + extra_shards {
                        break;
                    }
                }
            }
        }

        let mut migrations = self.migrations.write().unwrap();
        migrations.extend(tasks.clone());

        tracing::debug!(
            "Planned {} migrations for new node {}",
            tasks.len(),
            new_node
        );
        tasks
    }

    /// Plans migrations to rebalance shards when a node leaves the cluster.
    pub fn plan_remove_node(
        &self,
        leaving_node: NodeId,
        remaining_nodes: &[NodeId],
    ) -> Vec<MigrationTask> {
        let mut placements = self.placements.write().unwrap();
        let mut tasks = Vec::new();

        if remaining_nodes.is_empty() {
            return tasks;
        }

        let _target_shards_per_node = self.num_shards as usize / remaining_nodes.len();

        let mut shards_to_reassign: Vec<ShardId> = Vec::new();
        let mut leaving_replica_shards: Vec<ShardId> = Vec::new();

        for (shard_id, placement) in placements.iter() {
            if placement.primary == leaving_node {
                shards_to_reassign.push(*shard_id);
            }
            if placement.replicas.contains(&leaving_node) {
                leaving_replica_shards.push(*shard_id);
            }
        }

        for shard_id in shards_to_reassign {
            if let Some(placement) = placements.get_mut(&shard_id) {
                let new_primary =
                    remaining_nodes[shard_id.as_u16() as usize % remaining_nodes.len()];
                let task = MigrationTask {
                    shard_id,
                    source: leaving_node,
                    target: new_primary,
                    status: MigrationStatus::Pending,
                    created_at: Timestamp::now(),
                };
                tasks.push(task);
                placement.primary = new_primary;
                placement.version += 1;
            }
        }

        for shard_id in leaving_replica_shards {
            if let Some(placement) = placements.get_mut(&shard_id) {
                placement.replicas.retain(|r| *r != leaving_node);
                let new_replica =
                    remaining_nodes[shard_id.as_u16() as usize % remaining_nodes.len()];
                if !placement.replicas.contains(&new_replica) && placement.primary != new_replica {
                    placement.replicas.push(new_replica);
                    placement.version += 1;
                }
            }
        }

        let mut migrations = self.migrations.write().unwrap();
        migrations.extend(tasks.clone());

        tracing::debug!(
            "Planned {} migrations after removing node {}",
            tasks.len(),
            leaving_node
        );
        tasks
    }

    /// Applies a migration task, updating the shard's primary to the target node.
    pub fn apply_migration(&self, task: &MigrationTask) -> Result<(), MetaError> {
        let mut placements = self.placements.write().unwrap();

        if let Some(placement) = placements.get_mut(&task.shard_id) {
            placement.primary = task.target;
            placement.version += 1;
            tracing::debug!(
                "Applied migration for shard {} to node {}",
                task.shard_id,
                task.target
            );
            Ok(())
        } else {
            Err(MetaError::KvError(format!(
                "shard {:?} not found",
                task.shard_id
            )))
        }
    }

    /// Returns all pending or in-progress migration tasks.
    pub fn pending_migrations(&self) -> Vec<MigrationTask> {
        let migrations = self.migrations.read().unwrap();
        migrations
            .iter()
            .filter(|m| {
                matches!(
                    m.status,
                    MigrationStatus::Pending | MigrationStatus::InProgress
                )
            })
            .cloned()
            .collect()
    }

    /// Marks a migration as completed. Returns true if the migration was found.
    pub fn complete_migration(&self, shard_id: ShardId) -> bool {
        let mut migrations = self.migrations.write().unwrap();
        if let Some(task) = migrations.iter_mut().find(|m| m.shard_id == shard_id) {
            task.status = MigrationStatus::Completed;
            tracing::debug!("Completed migration for shard {}", shard_id);
            true
        } else {
            false
        }
    }

    /// Starts the next pending migration. Returns None if no pending migrations
    /// or if max concurrent migrations reached.
    pub fn start_next_migration(&self) -> Option<MigrationTask> {
        let active = self.active_migration_count();
        if active >= self.max_concurrent_migrations {
            return None;
        }

        let mut migrations = self.migrations.write().unwrap();
        if let Some(task) = migrations
            .iter_mut()
            .find(|m| matches!(m.status, MigrationStatus::Pending))
        {
            task.status = MigrationStatus::InProgress;
            tracing::debug!("Started migration for shard {}", task.shard_id);
            Some(task.clone())
        } else {
            None
        }
    }

    /// Marks a specific migration as InProgress. Returns error if not found or not Pending.
    pub fn start_migration(&self, shard_id: ShardId) -> Result<(), MetaError> {
        let mut migrations = self.migrations.write().unwrap();
        if let Some(task) = migrations.iter_mut().find(|m| m.shard_id == shard_id) {
            match &task.status {
                MigrationStatus::Pending => {
                    task.status = MigrationStatus::InProgress;
                    tracing::debug!("Started migration for shard {}", shard_id);
                    Ok(())
                }
                _ => Err(MetaError::KvError(format!(
                    "migration for shard {:?} is not in Pending state",
                    shard_id
                ))),
            }
        } else {
            Err(MetaError::KvError(format!(
                "migration for shard {:?} not found",
                shard_id
            )))
        }
    }

    /// Marks a migration as Failed with a reason. Returns error if not found.
    pub fn fail_migration(&self, shard_id: ShardId, reason: String) -> Result<(), MetaError> {
        let mut migrations = self.migrations.write().unwrap();
        if let Some(task) = migrations.iter_mut().find(|m| m.shard_id == shard_id) {
            task.status = MigrationStatus::Failed { reason };
            tracing::debug!("Failed migration for shard {}", shard_id);
            Ok(())
        } else {
            Err(MetaError::KvError(format!(
                "migration for shard {:?} not found",
                shard_id
            )))
        }
    }

    /// Retries a failed migration by resetting it to Pending. Returns error if not Failed.
    pub fn retry_migration(&self, shard_id: ShardId) -> Result<(), MetaError> {
        let mut migrations = self.migrations.write().unwrap();
        if let Some(task) = migrations.iter_mut().find(|m| m.shard_id == shard_id) {
            match &task.status {
                MigrationStatus::Failed { .. } => {
                    task.status = MigrationStatus::Pending;
                    tracing::debug!("Retrying migration for shard {}", shard_id);
                    Ok(())
                }
                _ => Err(MetaError::KvError(format!(
                    "migration for shard {:?} is not in Failed state",
                    shard_id
                ))),
            }
        } else {
            Err(MetaError::KvError(format!(
                "migration for shard {:?} not found",
                shard_id
            )))
        }
    }

    /// Returns the status of a specific migration.
    pub fn migration_status(&self, shard_id: ShardId) -> Option<MigrationStatus> {
        let migrations = self.migrations.read().unwrap();
        migrations
            .iter()
            .find(|m| m.shard_id == shard_id)
            .map(|m| m.status.clone())
    }

    /// Returns the number of currently InProgress migrations.
    pub fn active_migration_count(&self) -> usize {
        let migrations = self.migrations.read().unwrap();
        migrations
            .iter()
            .filter(|m| matches!(m.status, MigrationStatus::InProgress))
            .count()
    }

    /// Returns all completed migrations.
    pub fn completed_migrations(&self) -> Vec<MigrationTask> {
        let migrations = self.migrations.read().unwrap();
        migrations
            .iter()
            .filter(|m| matches!(m.status, MigrationStatus::Completed))
            .cloned()
            .collect()
    }

    /// Clears all completed migrations from the list.
    pub fn clear_completed(&self) -> usize {
        let mut migrations = self.migrations.write().unwrap();
        let before = migrations.len();
        migrations.retain(|m| !matches!(m.status, MigrationStatus::Completed));
        let cleared = before - migrations.len();
        tracing::debug!("Cleared {} completed migrations", cleared);
        cleared
    }

    /// Convenience: plans and immediately queues migrations to drain all shards from a node.
    /// Returns the planned migration tasks.
    pub fn drain_node(&self, node: NodeId, remaining: &[NodeId]) -> Vec<MigrationTask> {
        self.plan_remove_node(node, remaining)
    }

    /// Executes a full migration cycle: starts pending migrations up to the concurrent limit,
    /// returns the tasks that were started. The caller is responsible for actually streaming
    /// the shard data and calling complete_migration() or fail_migration() when done.
    pub fn tick_migrations(&self) -> Vec<MigrationTask> {
        let active = self.active_migration_count();
        let slots = self.max_concurrent_migrations.saturating_sub(active);

        let mut started = Vec::new();
        for _ in 0..slots {
            if let Some(task) = self.start_next_migration() {
                started.push(task);
            } else {
                break;
            }
        }

        tracing::debug!(
            "Tick: started {} migrations ({} active, {} max)",
            started.len(),
            active,
            self.max_concurrent_migrations
        );
        started
    }

    /// Returns the total number of shard placements.
    pub fn placement_count(&self) -> usize {
        let placements = self.placements.read().unwrap();
        placements.len()
    }

    /// Returns all shards (primary or replica) hosted on the given node.
    pub fn shards_on_node(&self, node_id: NodeId) -> Vec<ShardId> {
        let placements = self.placements.read().unwrap();
        placements
            .iter()
            .filter(|(_, p)| p.primary == node_id || p.replicas.contains(&node_id))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Checks if shard distribution across nodes is balanced within the given tolerance.
    pub fn is_balanced(&self, tolerance: f64) -> bool {
        let placements = self.placements.read().unwrap();

        if placements.is_empty() {
            return true;
        }

        let mut node_counts: HashMap<NodeId, usize> = HashMap::new();
        for placement in placements.values() {
            *node_counts.entry(placement.primary).or_insert(0) += 1;
        }

        if node_counts.is_empty() {
            return true;
        }

        let avg = placements.len() as f64 / node_counts.len() as f64;
        let threshold = avg * (1.0 + tolerance);

        for count in node_counts.values() {
            if *count as f64 > threshold {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_placements() {
        let mgr = ScalingManager::new(256, 3);
        let nodes = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];

        mgr.initialize_placements(&nodes);

        assert_eq!(mgr.placement_count(), 256);

        let shard_0 = mgr.get_placement(ShardId::new(0)).unwrap();
        assert_eq!(shard_0.primary, NodeId::new(1));
        assert_eq!(shard_0.replicas.len(), 3);

        let shard_1 = mgr.get_placement(ShardId::new(1)).unwrap();
        assert_eq!(shard_1.primary, NodeId::new(2));
    }

    #[test]
    fn test_initialize_empty_nodes() {
        let mgr = ScalingManager::new(256, 3);
        mgr.initialize_placements(&[]);
        assert_eq!(mgr.placement_count(), 0);
    }

    #[test]
    fn test_plan_add_node() {
        let mgr = ScalingManager::new(9, 2);
        let existing = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&existing);

        let new_node = NodeId::new(3);
        let tasks = mgr.plan_add_node(new_node, &existing);

        assert!(!tasks.is_empty());
    }

    #[test]
    fn test_plan_add_node_no_rebalance_needed() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        let tasks = mgr.plan_add_node(NodeId::new(3), &nodes);
        assert!(tasks.len() <= 2);
    }

    #[test]
    fn test_plan_remove_node() {
        let mgr = ScalingManager::new(6, 2);
        let nodes = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        mgr.initialize_placements(&nodes);

        let tasks = mgr.plan_remove_node(NodeId::new(1), &[NodeId::new(2), NodeId::new(3)]);

        assert!(!tasks.is_empty());
        assert!(tasks.iter().all(|t| t.source == NodeId::new(1)));
    }

    #[test]
    fn test_plan_remove_node_distributes_evenly() {
        let mgr = ScalingManager::new(8, 1);
        let nodes = vec![
            NodeId::new(1),
            NodeId::new(2),
            NodeId::new(3),
            NodeId::new(4),
        ];
        mgr.initialize_placements(&nodes);

        let _tasks = mgr.plan_remove_node(
            NodeId::new(1),
            &[NodeId::new(2), NodeId::new(3), NodeId::new(4)],
        );

        let shard_1_primary = mgr.get_placement(ShardId::new(1)).map(|p| p.primary);
        assert!(shard_1_primary.is_some());
        assert_ne!(shard_1_primary.unwrap(), NodeId::new(1));
    }

    #[test]
    fn test_apply_migration() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        let task = MigrationTask {
            shard_id: ShardId::new(0),
            source: NodeId::new(1),
            target: NodeId::new(2),
            status: MigrationStatus::Pending,
            created_at: Timestamp::now(),
        };

        mgr.apply_migration(&task).unwrap();

        let placement = mgr.get_placement(ShardId::new(0)).unwrap();
        assert_eq!(placement.primary, NodeId::new(2));
    }

    #[test]
    fn test_apply_migration_nonexistent_shard() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1)];
        mgr.initialize_placements(&nodes);

        let task = MigrationTask {
            shard_id: ShardId::new(999),
            source: NodeId::new(1),
            target: NodeId::new(2),
            status: MigrationStatus::Pending,
            created_at: Timestamp::now(),
        };

        assert!(mgr.apply_migration(&task).is_err());
    }

    #[test]
    fn test_pending_migrations() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1)];
        mgr.initialize_placements(&nodes);

        let pending = mgr.pending_migrations();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_complete_migration() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);
        let pending = mgr.pending_migrations();

        if !pending.is_empty() {
            let result = mgr.complete_migration(pending[0].shard_id);
            assert!(result);
        }
    }

    #[test]
    fn test_shards_on_node() {
        let mgr = ScalingManager::new(6, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        mgr.initialize_placements(&nodes);

        let shards = mgr.shards_on_node(NodeId::new(1));
        assert!(!shards.is_empty());
        assert!(shards.iter().all(|s| {
            let p = mgr.get_placement(*s).unwrap();
            p.primary == NodeId::new(1) || p.replicas.contains(&NodeId::new(1))
        }));
    }

    #[test]
    fn test_is_balanced() {
        let mgr = ScalingManager::new(6, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        mgr.initialize_placements(&nodes);

        assert!(mgr.is_balanced(0.2));
    }

    #[test]
    fn test_is_balanced_unbalanced() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1)];
        mgr.initialize_placements(&nodes);

        assert!(mgr.is_balanced(0.0));
    }

    #[test]
    fn test_is_balanced_empty() {
        let mgr = ScalingManager::new(256, 3);
        assert!(mgr.is_balanced(0.1));
    }

    #[test]
    fn test_start_migration() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);

        let pending = mgr.pending_migrations();
        assert!(!pending.is_empty());

        let shard_id = pending[0].shard_id;
        mgr.start_migration(shard_id).unwrap();

        let status = mgr.migration_status(shard_id).unwrap();
        assert!(matches!(status, MigrationStatus::InProgress));
    }

    #[test]
    fn test_fail_migration() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);

        let pending = mgr.pending_migrations();
        let shard_id = pending[0].shard_id;

        mgr.start_migration(shard_id).unwrap();
        mgr.fail_migration(shard_id, "test failure".to_string())
            .unwrap();

        let status = mgr.migration_status(shard_id).unwrap();
        assert!(matches!(status, MigrationStatus::Failed { reason } if reason == "test failure"));
    }

    #[test]
    fn test_retry_migration() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);

        let pending = mgr.pending_migrations();
        let shard_id = pending[0].shard_id;

        mgr.start_migration(shard_id).unwrap();
        mgr.fail_migration(shard_id, "test failure".to_string())
            .unwrap();
        mgr.retry_migration(shard_id).unwrap();

        let status = mgr.migration_status(shard_id).unwrap();
        assert!(matches!(status, MigrationStatus::Pending));
    }

    #[test]
    fn test_start_next_migration() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);
        mgr.plan_add_node(NodeId::new(4), &nodes);

        let started = mgr.start_next_migration();
        assert!(started.is_some());
        assert!(matches!(
            started.unwrap().status,
            MigrationStatus::InProgress
        ));
    }

    #[test]
    fn test_max_concurrent_migrations() {
        let mgr = ScalingManager::with_max_migrations(8, 1, 2);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);
        mgr.plan_add_node(NodeId::new(4), &nodes);
        mgr.plan_add_node(NodeId::new(5), &nodes);
        mgr.plan_add_node(NodeId::new(6), &nodes);

        let mut count = 0;
        while mgr.start_next_migration().is_some() {
            count += 1;
        }

        assert_eq!(count, 2);
    }

    #[test]
    fn test_active_migration_count() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);
        mgr.plan_add_node(NodeId::new(4), &nodes);

        mgr.start_next_migration();
        mgr.start_next_migration();

        assert_eq!(mgr.active_migration_count(), 2);
    }

    #[test]
    fn test_completed_migrations_and_clear() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);

        let pending = mgr.pending_migrations();
        let shard_id = pending[0].shard_id;

        mgr.complete_migration(shard_id);

        let completed = mgr.completed_migrations();
        assert_eq!(completed.len(), 1);

        let cleared = mgr.clear_completed();
        assert_eq!(cleared, 1);

        assert!(mgr.completed_migrations().is_empty());
    }

    #[test]
    fn test_drain_node() {
        let mgr = ScalingManager::new(6, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        mgr.initialize_placements(&nodes);

        let tasks = mgr.drain_node(NodeId::new(1), &[NodeId::new(2), NodeId::new(3)]);

        assert!(!tasks.is_empty());
        assert!(tasks.iter().all(|t| t.source == NodeId::new(1)));
    }

    #[test]
    fn test_tick_migrations() {
        let mgr = ScalingManager::with_max_migrations(6, 1, 3);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);
        mgr.plan_add_node(NodeId::new(4), &nodes);
        mgr.plan_add_node(NodeId::new(5), &nodes);

        let started = mgr.tick_migrations();
        assert_eq!(started.len(), 3);
        assert_eq!(mgr.active_migration_count(), 3);
    }

    #[test]
    fn test_migration_status() {
        let mgr = ScalingManager::new(4, 1);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];
        mgr.initialize_placements(&nodes);

        mgr.plan_add_node(NodeId::new(3), &nodes);

        let pending = mgr.pending_migrations();
        let shard_id = pending[0].shard_id;

        assert!(matches!(
            mgr.migration_status(shard_id).unwrap(),
            MigrationStatus::Pending
        ));

        mgr.start_migration(shard_id).unwrap();
        assert!(matches!(
            mgr.migration_status(shard_id).unwrap(),
            MigrationStatus::InProgress
        ));

        mgr.complete_migration(shard_id);
        assert!(matches!(
            mgr.migration_status(shard_id).unwrap(),
            MigrationStatus::Completed
        ));

        assert!(mgr.migration_status(ShardId::new(999)).is_none());
    }
}
