//! EC stripe coordination for distributing data chunks across nodes per D1/D8.

use serde::{Deserialize, Serialize};

/// Node identifier newtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

/// Erasure coding configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcConfig {
    /// Number of data shards
    pub data_shards: u8,
    /// Number of parity shards
    pub parity_shards: u8,
}

impl EcConfig {
    /// Total shards (data + parity).
    pub fn total_shards(&self) -> u8 {
        self.data_shards + self.parity_shards
    }

    /// Minimum surviving shards needed for reconstruction.
    pub fn min_surviving_shards(&self) -> u8 {
        self.data_shards
    }
}

impl Default for EcConfig {
    fn default() -> Self {
        Self {
            data_shards: 4,
            parity_shards: 2,
        }
    }
}

/// Placement of a single shard on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardPlacement {
    /// Index of the shard (0 to total_shards-1)
    pub shard_index: u8,
    /// Node where the shard is stored
    pub node_id: NodeId,
    /// Whether this is a parity shard
    pub is_parity: bool,
}

/// Stripe placement plan for a segment.
#[derive(Debug, Clone)]
pub struct StripePlan {
    /// Segment ID this plan is for
    pub segment_id: u64,
    /// Placements for all shards
    pub placements: Vec<ShardPlacement>,
}

impl StripePlan {
    /// Get nodes holding data shards.
    pub fn data_nodes(&self) -> Vec<NodeId> {
        self.placements
            .iter()
            .filter(|p| !p.is_parity)
            .map(|p| p.node_id)
            .collect()
    }

    /// Get nodes holding parity shards.
    pub fn parity_nodes(&self) -> Vec<NodeId> {
        self.placements
            .iter()
            .filter(|p| p.is_parity)
            .map(|p| p.node_id)
            .collect()
    }

    /// Get the node for a specific shard index.
    pub fn node_for_shard(&self, shard_index: u8) -> Option<NodeId> {
        self.placements
            .iter()
            .find(|p| p.shard_index == shard_index)
            .map(|p| p.node_id)
    }
}

/// Stripe statistics.
#[derive(Debug, Clone, Default)]
pub struct StripeStats {
    /// Total segments planned
    pub segments_planned: u64,
    /// Average nodes per stripe
    pub avg_nodes_per_stripe: f64,
}

/// Prime number for consistent hashing.
const HASH_PRIME: u64 = 0x9E3779B97F4A7C15;

/// Coordinates stripe placement across nodes.
pub struct StripeCoordinator {
    config: EcConfig,
    nodes: Vec<NodeId>,
}

impl StripeCoordinator {
    /// Create a new stripe coordinator with the given config and nodes.
    pub fn new(config: EcConfig, nodes: Vec<NodeId>) -> Self {
        Self { config, nodes }
    }

    /// Plan stripe placement for a segment.
    /// Uses consistent hash to assign shards to nodes deterministically.
    pub fn plan_stripe(&self, segment_id: u64) -> StripePlan {
        let total = self.config.total_shards() as usize;
        let num_nodes = self.nodes.len().max(1);

        let placements: Vec<ShardPlacement> = (0..total)
            .map(|shard_index| {
                let hash = (segment_id
                    .wrapping_mul(HASH_PRIME)
                    .wrapping_add(shard_index as u64))
                    % num_nodes as u64;
                let node_idx = hash as usize;
                let node_id = self.nodes.get(node_idx).copied().unwrap_or(NodeId(0));
                ShardPlacement {
                    shard_index: shard_index as u8,
                    node_id,
                    is_parity: shard_index as u8 >= self.config.data_shards,
                }
            })
            .collect();

        StripePlan {
            segment_id,
            placements,
        }
    }

    /// Check if all placements are on distinct nodes.
    pub fn all_nodes_distinct(&self, plan: &StripePlan) -> bool {
        let mut seen = std::collections::HashSet::new();
        for placement in &plan.placements {
            if !seen.insert(placement.node_id) {
                return false;
            }
        }
        true
    }

    /// Check if the plan can tolerate the given node failures.
    pub fn can_tolerate_failures(&self, plan: &StripePlan, failed_nodes: &[NodeId]) -> bool {
        let failed_set: std::collections::HashSet<_> = failed_nodes.iter().copied().collect();
        let failed_count = plan
            .placements
            .iter()
            .filter(|p| failed_set.contains(&p.node_id))
            .count();
        failed_count as u8 <= self.config.parity_shards
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ec_config_default() {
        let config = EcConfig::default();
        assert_eq!(config.data_shards, 4);
        assert_eq!(config.parity_shards, 2);
    }

    #[test]
    fn ec_config_total_shards() {
        let config = EcConfig::default();
        assert_eq!(config.total_shards(), 6);
    }

    #[test]
    fn ec_config_min_surviving() {
        let config = EcConfig::default();
        assert_eq!(config.min_surviving_shards(), 4);
    }

    #[test]
    fn plan_stripe_creates_correct_shard_count() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert_eq!(plan.placements.len(), 6);
    }

    #[test]
    fn plan_stripe_deterministic() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan1 = coordinator.plan_stripe(12345);
        let plan2 = coordinator.plan_stripe(12345);
        assert_eq!(plan1.placements.len(), plan2.placements.len());
        for (p1, p2) in plan1.placements.iter().zip(plan2.placements.iter()) {
            assert_eq!(p1.node_id, p2.node_id);
        }
    }

    #[test]
    fn plan_stripe_different_segments_different_placements() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan1 = coordinator.plan_stripe(12345);
        let plan2 = coordinator.plan_stripe(54321);
        let nodes1: Vec<NodeId> = plan1.placements.iter().map(|p| p.node_id).collect();
        let nodes2: Vec<NodeId> = plan2.placements.iter().map(|p| p.node_id).collect();
        assert_ne!(
            nodes1, nodes2,
            "different segments should have different shard placements"
        );
    }

    #[test]
    fn data_nodes_count() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert_eq!(plan.data_nodes().len(), 4);
    }

    #[test]
    fn parity_nodes_count() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert_eq!(plan.parity_nodes().len(), 2);
    }

    #[test]
    fn node_for_shard_found() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert!(plan.node_for_shard(0).is_some());
        assert!(plan.node_for_shard(5).is_some());
    }

    #[test]
    fn node_for_shard_not_found() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert!(plan.node_for_shard(100).is_none());
    }

    #[test]
    fn can_tolerate_one_failure() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        let first_node = plan.placements[0].node_id;
        assert!(coordinator.can_tolerate_failures(&plan, &[first_node]));
    }

    #[test]
    fn can_tolerate_two_failures() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        let n1 = plan.placements[0].node_id;
        let n2 = plan.placements[1].node_id;
        assert!(coordinator.can_tolerate_failures(&plan, &[n1, n2]));
    }

    #[test]
    fn cannot_tolerate_three_failures() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        let n1 = plan.placements[0].node_id;
        let n2 = plan.placements[1].node_id;
        let n3 = plan.placements[2].node_id;
        assert!(!coordinator.can_tolerate_failures(&plan, &[n1, n2, n3]));
    }

    #[test]
    fn plan_stripe_wraps_nodes() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..3).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert_eq!(plan.placements.len(), 6);
        let unique_nodes: std::collections::HashSet<_> =
            plan.placements.iter().map(|p| p.node_id).collect();
        assert!(unique_nodes.len() <= 3);
    }

    #[test]
    fn stripe_plan_parity_is_marked() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config.clone(), nodes);
        let plan = coordinator.plan_stripe(12345);
        for placement in &plan.placements {
            if placement.shard_index < config.data_shards {
                assert!(!placement.is_parity);
            } else {
                assert!(placement.is_parity);
            }
        }
    }

    #[test]
    fn ec_config_custom_values() {
        let config = EcConfig {
            data_shards: 8,
            parity_shards: 4,
        };
        assert_eq!(config.total_shards(), 12);
        assert_eq!(config.min_surviving_shards(), 8);
    }

    #[test]
    fn all_nodes_distinct_true() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert!(coordinator.all_nodes_distinct(&plan));
    }

    #[test]
    fn all_nodes_distinct_false() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..3).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert!(!coordinator.all_nodes_distinct(&plan));
    }

    #[test]
    fn can_tolerate_zero_failures() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert!(coordinator.can_tolerate_failures(&plan, &[]));
    }

    #[test]
    fn can_tolerate_parity_shard_failures() {
        let config = EcConfig {
            data_shards: 4,
            parity_shards: 2,
        };
        let nodes: Vec<NodeId> = (0..6).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        let parity_nodes: Vec<NodeId> = plan.parity_nodes();
        assert!(coordinator.can_tolerate_failures(&plan, &parity_nodes));
    }

    #[test]
    fn empty_nodes_list_plan() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = vec![];
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert_eq!(plan.placements.len(), 6);
        for placement in &plan.placements {
            assert_eq!(placement.node_id, NodeId(0));
        }
    }

    #[test]
    fn single_node_plan() {
        let config = EcConfig::default();
        let nodes: Vec<NodeId> = vec![NodeId(1)];
        let coordinator = StripeCoordinator::new(config, nodes);
        let plan = coordinator.plan_stripe(12345);
        assert_eq!(plan.placements.len(), 6);
        for placement in &plan.placements {
            assert_eq!(placement.node_id, NodeId(1));
        }
    }

    #[test]
    fn parity_count_matches_config() {
        let config = EcConfig {
            data_shards: 6,
            parity_shards: 3,
        };
        let nodes: Vec<NodeId> = (0..9).map(NodeId).collect();
        let coordinator = StripeCoordinator::new(config.clone(), nodes);
        let plan = coordinator.plan_stripe(12345);
        assert_eq!(plan.parity_nodes().len(), config.parity_shards as usize);
    }
}
