//! Chunk rebalancing when nodes join or leave the cluster.
//!
//! When a node leaves, its chunks must be redistributed to remaining nodes.
//! When a node joins, some chunks from existing nodes can be moved to it.

use serde::{Deserialize, Serialize};

/// Action to take during rebalancing.
#[derive(Debug, Clone)]
pub enum RebalanceAction {
    /// Move a chunk from one node to another
    Move {
        /// Hash of the chunk to move
        chunk_hash: [u8; 32],
        /// Source node ID
        from_node: u64,
        /// Destination node ID
        to_node: u64,
        /// Size of the chunk in bytes
        size_bytes: u64,
    },
    /// Replicate a chunk for redundancy
    Replicate {
        /// Hash of the chunk to replicate
        chunk_hash: [u8; 32],
        /// Source node ID
        source_node: u64,
        /// Destination node ID
        dest_node: u64,
        /// Size of the chunk in bytes
        size_bytes: u64,
    },
}

/// Load information for a single node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLoad {
    /// Node identifier
    pub node_id: u64,
    /// Bytes stored on this node
    pub bytes_stored: u64,
    /// Number of chunks on this node
    pub chunk_count: u64,
}

impl NodeLoad {
    /// Calculate this node's fraction of total cluster bytes
    pub fn load_fraction(&self, total_bytes: u64) -> f64 {
        if total_bytes == 0 {
            return 0.0;
        }
        self.bytes_stored as f64 / total_bytes as f64
    }
}

/// Plan for rebalancing chunks across nodes.
#[derive(Debug)]
pub struct RebalancePlan {
    /// Actions to execute
    pub actions: Vec<RebalanceAction>,
    /// Total bytes moved (not replicated)
    pub total_bytes_moved: u64,
    /// Total bytes replicated
    pub total_bytes_replicated: u64,
}

impl RebalancePlan {
    /// Number of actions in the plan
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }
}

/// Configuration for the rebalancer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalancerConfig {
    /// Maximum imbalance percentage tolerated (e.g., 10.0 = 10%)
    pub max_imbalance_pct: f64,
    /// Maximum number of actions per plan
    pub max_actions_per_plan: usize,
}

impl Default for RebalancerConfig {
    fn default() -> Self {
        Self {
            max_imbalance_pct: 10.0,
            max_actions_per_plan: 100,
        }
    }
}

/// Chunk rebalancer for distributing load across nodes.
#[derive(Debug)]
pub struct ChunkRebalancer {
    config: RebalancerConfig,
}

impl ChunkRebalancer {
    /// Create a new rebalancer with the given configuration
    pub fn new(config: RebalancerConfig) -> Self {
        Self { config }
    }

    /// Check if nodes are balanced within tolerance
    pub fn is_balanced(&self, loads: &[NodeLoad]) -> bool {
        if loads.is_empty() {
            return true;
        }

        let total_bytes: u64 = loads.iter().map(|l| l.bytes_stored).sum();
        if total_bytes == 0 {
            return true;
        }

        let fractions: Vec<f64> = loads.iter().map(|l| l.load_fraction(total_bytes)).collect();

        let max_frac = fractions.iter().cloned().fold(0.0f64, f64::max);
        let min_frac = fractions.iter().cloned().fold(1.0f64, f64::min);

        let imbalance = max_frac - min_frac;
        imbalance <= self.config.max_imbalance_pct / 100.0
    }

    /// Plan a rebalance to even out load across nodes
    pub fn plan_rebalance(
        &self,
        loads: &[NodeLoad],
        chunks: &[([u8; 32], u64, u64)],
    ) -> RebalancePlan {
        if chunks.is_empty() || loads.len() < 2 {
            return RebalancePlan {
                actions: Vec::new(),
                total_bytes_moved: 0,
                total_bytes_replicated: 0,
            };
        }

        let total_bytes: u64 = loads.iter().map(|l| l.bytes_stored).sum();
        if total_bytes == 0 {
            return RebalancePlan {
                actions: Vec::new(),
                total_bytes_moved: 0,
                total_bytes_replicated: 0,
            };
        }

        let avg_fraction = 1.0 / loads.len() as f64;
        let threshold = avg_fraction + (self.config.max_imbalance_pct / 100.0 / 2.0);

        let mut overloaded: Vec<&NodeLoad> = loads
            .iter()
            .filter(|l| l.load_fraction(total_bytes) > threshold)
            .collect();
        let mut underloaded: Vec<&NodeLoad> = loads
            .iter()
            .filter(|l| {
                l.load_fraction(total_bytes)
                    < avg_fraction - (self.config.max_imbalance_pct / 100.0 / 2.0)
            })
            .collect();

        if overloaded.is_empty() || underloaded.is_empty() {
            return RebalancePlan {
                actions: Vec::new(),
                total_bytes_moved: 0,
                total_bytes_replicated: 0,
            };
        }

        overloaded.sort_by(|a, b| b.bytes_stored.cmp(&a.bytes_stored));
        underloaded.sort_by(|a, b| a.bytes_stored.cmp(&b.bytes_stored));

        let mut actions = Vec::new();
        let mut total_bytes_moved = 0u64;

        let overloaded_node_ids: std::collections::HashSet<u64> =
            overloaded.iter().map(|l| l.node_id).collect();

        let chunks_on_overloaded: Vec<&([u8; 32], u64, u64)> = chunks
            .iter()
            .filter(|(_, node_id, _)| overloaded_node_ids.contains(node_id))
            .collect();

        let mut underloaded_idx = 0;
        let underloaded_nodes: Vec<u64> = underloaded.iter().map(|l| l.node_id).collect();

        for (hash, from_node, size_bytes) in chunks_on_overloaded {
            if actions.len() >= self.config.max_actions_per_plan {
                break;
            }
            if underloaded_idx >= underloaded_nodes.len() {
                break;
            }

            let to_node = underloaded_nodes[underloaded_idx];
            actions.push(RebalanceAction::Move {
                chunk_hash: *hash,
                from_node: *from_node,
                to_node,
                size_bytes: *size_bytes,
            });
            total_bytes_moved += size_bytes;

            underloaded_idx = (underloaded_idx + 1) % underloaded_nodes.len();
        }

        RebalancePlan {
            actions,
            total_bytes_moved,
            total_bytes_replicated: 0,
        }
    }

    /// Plan redistribution of chunks from a departed node
    pub fn plan_node_departure(
        &self,
        departed_node_id: u64,
        remaining_nodes: &[u64],
        chunks: &[([u8; 32], u64)],
    ) -> RebalancePlan {
        if remaining_nodes.is_empty() || chunks.is_empty() {
            return RebalancePlan {
                actions: Vec::new(),
                total_bytes_moved: 0,
                total_bytes_replicated: 0,
            };
        }

        let mut actions = Vec::new();
        let mut total_bytes_moved = 0u64;

        for (idx, (hash, size_bytes)) in chunks.iter().enumerate() {
            if actions.len() >= self.config.max_actions_per_plan {
                break;
            }

            let target_idx = idx % remaining_nodes.len();
            let to_node = remaining_nodes[target_idx];

            actions.push(RebalanceAction::Move {
                chunk_hash: *hash,
                from_node: departed_node_id,
                to_node,
                size_bytes: *size_bytes,
            });
            total_bytes_moved += size_bytes;
        }

        RebalancePlan {
            actions,
            total_bytes_moved,
            total_bytes_replicated: 0,
        }
    }
}

impl Default for ChunkRebalancer {
    fn default() -> Self {
        Self::new(RebalancerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebalancer_config_default() {
        let config = RebalancerConfig::default();
        assert_eq!(config.max_imbalance_pct, 10.0);
        assert_eq!(config.max_actions_per_plan, 100);
    }

    #[test]
    fn node_load_fraction_normal() {
        let load = NodeLoad {
            node_id: 1,
            bytes_stored: 500,
            chunk_count: 10,
        };
        let fraction = load.load_fraction(1000);
        assert!((fraction - 0.5).abs() < 1e-10);
    }

    #[test]
    fn node_load_fraction_zero_total() {
        let load = NodeLoad {
            node_id: 1,
            bytes_stored: 500,
            chunk_count: 10,
        };
        let fraction = load.load_fraction(0);
        assert_eq!(fraction, 0.0);
    }

    #[test]
    fn is_balanced_balanced() {
        let rebalancer = ChunkRebalancer::default();
        let loads = vec![
            NodeLoad {
                node_id: 1,
                bytes_stored: 100,
                chunk_count: 5,
            },
            NodeLoad {
                node_id: 2,
                bytes_stored: 100,
                chunk_count: 5,
            },
            NodeLoad {
                node_id: 3,
                bytes_stored: 100,
                chunk_count: 5,
            },
        ];
        assert!(rebalancer.is_balanced(&loads));
    }

    #[test]
    fn is_balanced_imbalanced() {
        let rebalancer = ChunkRebalancer::default();
        let loads = vec![
            NodeLoad {
                node_id: 1,
                bytes_stored: 300,
                chunk_count: 15,
            },
            NodeLoad {
                node_id: 2,
                bytes_stored: 50,
                chunk_count: 2,
            },
            NodeLoad {
                node_id: 3,
                bytes_stored: 50,
                chunk_count: 2,
            },
        ];
        assert!(!rebalancer.is_balanced(&loads));
    }

    #[test]
    fn is_balanced_empty() {
        let rebalancer = ChunkRebalancer::default();
        let loads: Vec<NodeLoad> = vec![];
        assert!(rebalancer.is_balanced(&loads));
    }

    #[test]
    fn plan_rebalance_empty() {
        let rebalancer = ChunkRebalancer::default();
        let loads = vec![
            NodeLoad {
                node_id: 1,
                bytes_stored: 100,
                chunk_count: 5,
            },
            NodeLoad {
                node_id: 2,
                bytes_stored: 100,
                chunk_count: 5,
            },
        ];
        let chunks: Vec<([u8; 32], u64, u64)> = vec![];
        let plan = rebalancer.plan_rebalance(&loads, &chunks);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.total_bytes_moved, 0);
    }

    #[test]
    fn plan_rebalance_balanced_no_actions() {
        let rebalancer = ChunkRebalancer::default();
        let loads = vec![
            NodeLoad {
                node_id: 1,
                bytes_stored: 100,
                chunk_count: 5,
            },
            NodeLoad {
                node_id: 2,
                bytes_stored: 100,
                chunk_count: 5,
            },
        ];
        let chunks = vec![([1u8; 32], 1u64, 50u64), ([2u8; 32], 2u64, 50u64)];
        let plan = rebalancer.plan_rebalance(&loads, &chunks);
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn plan_node_departure_distributes_to_remaining() {
        let rebalancer = ChunkRebalancer::default();
        let remaining = vec![2u64, 3u64, 4u64];
        let chunks = vec![
            ([1u8; 32], 100u64),
            ([2u8; 32], 200u64),
            ([3u8; 32], 300u64),
        ];
        let plan = rebalancer.plan_node_departure(1, &remaining, &chunks);
        assert_eq!(plan.action_count(), 3);
        assert_eq!(plan.total_bytes_moved, 600);
    }

    #[test]
    fn plan_node_departure_round_robin() {
        let rebalancer = ChunkRebalancer::default();
        let remaining = vec![10u64, 20u64];
        let chunks = vec![
            ([1u8; 32], 100u64),
            ([2u8; 32], 100u64),
            ([3u8; 32], 100u64),
            ([4u8; 32], 100u64),
        ];
        let plan = rebalancer.plan_node_departure(1, &remaining, &chunks);

        let target_nodes: Vec<u64> = plan
            .actions
            .iter()
            .filter_map(|a| match a {
                RebalanceAction::Move { to_node, .. } => Some(*to_node),
                _ => None,
            })
            .collect();

        assert_eq!(target_nodes, vec![10, 20, 10, 20]);
    }

    #[test]
    fn plan_node_departure_empty_chunks() {
        let rebalancer = ChunkRebalancer::default();
        let remaining = vec![2u64, 3u64];
        let chunks: Vec<([u8; 32], u64)> = vec![];
        let plan = rebalancer.plan_node_departure(1, &remaining, &chunks);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.total_bytes_moved, 0);
    }

    #[test]
    fn rebalance_plan_action_count() {
        let plan = RebalancePlan {
            actions: vec![
                RebalanceAction::Move {
                    chunk_hash: [1; 32],
                    from_node: 1,
                    to_node: 2,
                    size_bytes: 100,
                },
                RebalanceAction::Replicate {
                    chunk_hash: [2; 32],
                    source_node: 1,
                    dest_node: 3,
                    size_bytes: 200,
                },
            ],
            total_bytes_moved: 100,
            total_bytes_replicated: 200,
        };
        assert_eq!(plan.action_count(), 2);
    }

    #[test]
    fn rebalance_plan_total_bytes_moved() {
        let plan = RebalancePlan {
            actions: vec![RebalanceAction::Move {
                chunk_hash: [1; 32],
                from_node: 1,
                to_node: 2,
                size_bytes: 500,
            }],
            total_bytes_moved: 500,
            total_bytes_replicated: 0,
        };
        assert_eq!(plan.total_bytes_moved, 500);
    }

    #[test]
    fn plan_respects_max_actions_per_plan() {
        let config = RebalancerConfig {
            max_imbalance_pct: 10.0,
            max_actions_per_plan: 2,
        };
        let rebalancer = ChunkRebalancer::new(config);

        let remaining = vec![2u64, 3u64];
        let chunks = vec![
            ([1u8; 32], 100u64),
            ([2u8; 32], 100u64),
            ([3u8; 32], 100u64),
        ];
        let plan = rebalancer.plan_node_departure(1, &remaining, &chunks);
        assert_eq!(plan.action_count(), 2);
    }
}
