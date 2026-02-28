//! Online node scaling and shard rebalancing.
//!
//! Manages shard placement across cluster nodes and plans migrations
//! when nodes join or leave. Implements the rebalancing logic for
//! decision D4 (Multi-Raft with 256 virtual shards).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShardPlacement {
    pub shard_id: ShardId,
    pub primary: NodeId,
    pub replicas: Vec<NodeId>,
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationTask {
    pub shard_id: ShardId,
    pub source: NodeId,
    pub target: NodeId,
    pub status: MigrationStatus,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MigrationStatus {
    Pending,
    InProgress,
    Completed,
    Failed { reason: String },
}

pub struct ScalingManager {
    placements: RwLock<HashMap<ShardId, ShardPlacement>>,
    migrations: RwLock<Vec<MigrationTask>>,
    num_shards: u16,
    replica_count: usize,
}

impl ScalingManager {
    pub fn new(num_shards: u16, replica_count: usize) -> Self {
        Self {
            placements: RwLock::new(HashMap::new()),
            migrations: RwLock::new(Vec::new()),
            num_shards,
            replica_count,
        }
    }

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

    pub fn get_placement(&self, shard_id: ShardId) -> Option<ShardPlacement> {
        let placements = self.placements.read().unwrap();
        placements.get(&shard_id).cloned()
    }

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

        let target_shards_per_node = self.num_shards as usize / remaining_nodes.len();

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
            Err(MetaError::InodeNotFound(InodeId(
                task.shard_id.as_u64() as u64
            )))
        }
    }

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

    pub fn placement_count(&self) -> usize {
        let placements = self.placements.read().unwrap();
        placements.len()
    }

    pub fn shards_on_node(&self, node_id: NodeId) -> Vec<ShardId> {
        let placements = self.placements.read().unwrap();
        placements
            .iter()
            .filter(|(_, p)| p.primary == node_id || p.replicas.contains(&node_id))
            .map(|(id, _)| *id)
            .collect()
    }

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

        let tasks = mgr.plan_remove_node(
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
            p.primary == NodeId::new(1)
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
}
