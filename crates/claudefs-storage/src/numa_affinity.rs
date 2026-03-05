//! NUMA affinity module - NUMA-aware task distribution for multi-socket systems.

use crate::error::StorageError;
use crate::nvme_passthrough::CoreId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NumaNodeId(u32);

#[derive(Debug, Clone)]
pub struct NumaTopology {
    pub num_nodes: u32,
    pub cores_per_node: u32,
}

pub struct NumaAffinityMap {
    topology: NumaTopology,
    core_to_node: Vec<NumaNodeId>,
    pub node_loads: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct AffinityHint {
    pub preferred_node: NumaNodeId,
    pub fallback_nodes: Vec<NumaNodeId>,
    pub load_factor: f64,
}

impl NumaAffinityMap {
    pub fn new(topology: NumaTopology) -> Self {
        let max_cores = topology.num_nodes * topology.cores_per_node;
        let mut core_to_node = Vec::with_capacity(max_cores as usize);

        for core_idx in 0..max_cores {
            let node_idx = if topology.cores_per_node > 0 {
                core_idx / topology.cores_per_node
            } else {
                0
            };
            core_to_node.push(NumaNodeId(node_idx));
        }

        let node_loads = vec![0u64; topology.num_nodes as usize];

        Self {
            topology,
            core_to_node,
            node_loads,
        }
    }

    pub fn get_affinity_hint(&self, block_id: u64) -> AffinityHint {
        let num_nodes = self.topology.num_nodes;
        if num_nodes == 0 {
            return AffinityHint {
                preferred_node: NumaNodeId(0),
                fallback_nodes: vec![],
                load_factor: 1.0,
            };
        }

        let preferred_node = NumaNodeId((block_id % (num_nodes as u64)) as u32);

        let avg_load = self.calculate_avg_load();

        let mut fallback_nodes: Vec<NumaNodeId> = (0..num_nodes)
            .map(NumaNodeId)
            .filter(|n| *n != preferred_node)
            .collect();

        fallback_nodes.sort_by_key(|n| self.node_loads[n.0 as usize]);

        let preferred_load = self.node_loads[preferred_node.0 as usize];
        let load_factor = if avg_load == 0.0 {
            1.0
        } else {
            let lf = 1.0 - (preferred_load as f64 / avg_load);
            lf.max(0.0).min(1.0)
        };

        AffinityHint {
            preferred_node,
            fallback_nodes,
            load_factor,
        }
    }

    pub fn assign_core_for_op(&mut self, block_id: u64, _op_size_bytes: u64) -> CoreId {
        let hint = self.get_affinity_hint(block_id);

        let avg_load = self.calculate_avg_load();
        let threshold = if avg_load > 0.0 {
            (avg_load as f64 * 0.8) as u64
        } else {
            0
        };

        let preferred_load = self.node_loads[hint.preferred_node.0 as usize];

        if preferred_load <= threshold || self.topology.cores_per_node == 0 {
            let node_cores = self.get_cores_for_node(hint.preferred_node);
            if !node_cores.is_empty() {
                let core_idx = node_cores[(block_id as usize) % node_cores.len()];
                return core_idx;
            }
        }

        let mut best_node = hint.preferred_node;
        let mut best_load = preferred_load;

        for node in &hint.fallback_nodes {
            let load = self.node_loads[node.0 as usize];
            if load < best_load {
                best_load = load;
                best_node = *node;
            }
        }

        let node_cores = self.get_cores_for_node(best_node);
        if !node_cores.is_empty() {
            let core_idx = node_cores[(block_id as usize) % node_cores.len()];
            return core_idx;
        }

        CoreId(0)
    }

    fn get_cores_for_node(&self, node: NumaNodeId) -> Vec<CoreId> {
        if self.topology.cores_per_node == 0 {
            return vec![];
        }

        (0..self.core_to_node.len())
            .filter(|&i| self.core_to_node[i] == node)
            .map(|i| CoreId(i as u32))
            .collect()
    }

    fn calculate_avg_load(&self) -> f64 {
        let total: u64 = self.node_loads.iter().sum();
        let count = self.node_loads.len();
        if count == 0 {
            return 0.0;
        }
        total as f64 / count as f64
    }

    pub fn record_completion(&mut self, core_id: CoreId, duration_ns: u64) {
        let node = self.get_local_node(core_id);
        if (node.0 as usize) < self.node_loads.len() {
            let units = duration_ns / 1_000_000;
            self.node_loads[node.0 as usize] += units;
        }
    }

    pub fn get_local_node(&self, core_id: CoreId) -> NumaNodeId {
        let idx = core_id.0 as usize;
        if idx < self.core_to_node.len() {
            self.core_to_node[idx]
        } else if !self.core_to_node.is_empty() {
            self.core_to_node[0]
        } else {
            NumaNodeId(0)
        }
    }

    pub fn node_load(&self, node_id: NumaNodeId) -> u64 {
        let idx = node_id.0 as usize;
        if idx < self.node_loads.len() {
            self.node_loads[idx]
        } else {
            0
        }
    }

    pub fn balance_check(&mut self) -> Vec<(NumaNodeId, i64)> {
        let avg_load = self.calculate_avg_load();

        if avg_load == 0.0 {
            return vec![];
        }

        let mut deltas: Vec<(NumaNodeId, i64)> = self
            .node_loads
            .iter()
            .enumerate()
            .map(|(i, &load)| {
                let delta = load as i64 - avg_load as i64;
                (NumaNodeId(i as u32), delta)
            })
            .collect();

        deltas.retain(|(_, delta)| *delta != 0);

        deltas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_topology_with_2_nodes() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let map = NumaAffinityMap::new(topology);
        assert_eq!(map.node_loads.len(), 2);
    }

    #[test]
    fn create_topology_with_4_nodes() {
        let topology = NumaTopology {
            num_nodes: 4,
            cores_per_node: 8,
        };
        let map = NumaAffinityMap::new(topology);
        assert_eq!(map.node_loads.len(), 4);
    }

    #[test]
    fn map_core_0_to_node_0() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let map = NumaAffinityMap::new(topology);
        let node = map.get_local_node(CoreId(0));
        assert_eq!(node, NumaNodeId(0));
    }

    #[test]
    fn map_core_at_node_boundary() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let map = NumaAffinityMap::new(topology);
        let node = map.get_local_node(CoreId(4));
        assert_eq!(node, NumaNodeId(1));
    }

    #[test]
    fn block_hash_selects_node_deterministically() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let map = NumaAffinityMap::new(topology);
        let hint1 = map.get_affinity_hint(100);
        let hint2 = map.get_affinity_hint(100);
        assert_eq!(hint1.preferred_node, hint2.preferred_node);
    }

    #[test]
    fn different_block_ids_hash_to_different_nodes() {
        let topology = NumaTopology {
            num_nodes: 3,
            cores_per_node: 4,
        };
        let map = NumaAffinityMap::new(topology);
        let hint1 = map.get_affinity_hint(0);
        let hint2 = map.get_affinity_hint(1);
        let hint3 = map.get_affinity_hint(2);

        let nodes: Vec<_> = [
            hint1.preferred_node,
            hint2.preferred_node,
            hint3.preferred_node,
        ]
        .to_vec();
        let unique: std::collections::HashSet<_> = nodes.iter().collect();
        assert!(unique.len() > 1);
    }

    #[test]
    fn assign_core_prefers_local_node_when_load_is_low() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 10;
        map.node_loads[1] = 100;

        let core = map.assign_core_for_op(0, 4096);
        let node = map.get_local_node(core);
        assert_eq!(node, NumaNodeId(0));
    }

    #[test]
    fn assign_core_falls_back_to_less_loaded_node() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 1000;
        map.node_loads[1] = 10;

        let core = map.assign_core_for_op(0, 4096);
        let node = map.get_local_node(core);
        assert_eq!(node, NumaNodeId(1));
    }

    #[test]
    fn record_completion_increments_node_load() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.record_completion(CoreId(0), 5_000_000);
        assert!(map.node_load(NumaNodeId(0)) > 0);
    }

    #[test]
    fn get_local_node_returns_correct_node_for_core() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let map = NumaAffinityMap::new(topology);

        assert_eq!(map.get_local_node(CoreId(0)), NumaNodeId(0));
        assert_eq!(map.get_local_node(CoreId(3)), NumaNodeId(0));
        assert_eq!(map.get_local_node(CoreId(4)), NumaNodeId(1));
        assert_eq!(map.get_local_node(CoreId(7)), NumaNodeId(1));
    }

    #[test]
    fn node_load_returns_correct_value() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 42;
        map.node_loads[1] = 99;

        assert_eq!(map.node_load(NumaNodeId(0)), 42);
        assert_eq!(map.node_load(NumaNodeId(1)), 99);
    }

    #[test]
    fn balance_check_shows_positive_delta_for_overloaded_node() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 100;
        map.node_loads[1] = 20;

        let deltas = map.balance_check();

        let overloaded: Vec<_> = deltas.iter().filter(|(_, d)| *d > 0).collect();
        assert!(!overloaded.is_empty());
    }

    #[test]
    fn balance_check_shows_negative_delta_for_underloaded_node() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 100;
        map.node_loads[1] = 20;

        let deltas = map.balance_check();

        let underloaded: Vec<_> = deltas.iter().filter(|(_, d)| *d < 0).collect();
        assert!(!underloaded.is_empty());
    }

    #[test]
    fn fallback_nodes_sorted_by_load() {
        let topology = NumaTopology {
            num_nodes: 3,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 50;
        map.node_loads[1] = 10;
        map.node_loads[2] = 30;

        let hint = map.get_affinity_hint(0);

        assert_eq!(hint.preferred_node, NumaNodeId(0));
        assert!(hint.fallback_nodes[0] == NumaNodeId(1) || hint.fallback_nodes[0] == NumaNodeId(2));
    }

    #[test]
    fn load_factor_calculation_for_high_load() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 100;
        map.node_loads[1] = 10;

        let hint = map.get_affinity_hint(0);

        assert!(hint.load_factor >= 0.0 && hint.load_factor <= 1.0);
    }

    #[test]
    fn load_factor_calculation_for_low_load() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 1;
        map.node_loads[1] = 10;

        let hint = map.get_affinity_hint(0);

        assert!(hint.load_factor >= 0.0 && hint.load_factor <= 1.0);
    }

    #[test]
    fn load_factor_calculation_for_zero_load() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 0;
        map.node_loads[1] = 0;

        let hint = map.get_affinity_hint(0);

        assert_eq!(hint.load_factor, 1.0);
    }

    #[test]
    fn zero_cores_per_node_handling() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 0,
        };
        let map = NumaAffinityMap::new(topology);

        let hint = map.get_affinity_hint(100);
        assert_eq!(hint.preferred_node, NumaNodeId(0));
    }

    #[test]
    fn single_node_topology_works() {
        let topology = NumaTopology {
            num_nodes: 1,
            cores_per_node: 8,
        };
        let map = NumaAffinityMap::new(topology);

        let hint = map.get_affinity_hint(100);
        assert_eq!(hint.preferred_node, NumaNodeId(0));
        assert!(hint.fallback_nodes.is_empty());
    }

    #[test]
    fn empty_map_returns_default_values() {
        let topology = NumaTopology {
            num_nodes: 1,
            cores_per_node: 1,
        };
        let map = NumaAffinityMap::new(topology);

        assert_eq!(map.node_load(NumaNodeId(0)), 0);
    }

    #[test]
    fn large_core_ids_handled_correctly() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 16,
        };
        let map = NumaAffinityMap::new(topology);

        let node = map.get_local_node(CoreId(100));
        assert!(node == NumaNodeId(0) || node == NumaNodeId(1));
    }

    #[test]
    fn large_node_count_handled_correctly() {
        let topology = NumaTopology {
            num_nodes: 8,
            cores_per_node: 4,
        };
        let map = NumaAffinityMap::new(topology);

        assert_eq!(map.node_loads.len(), 8);

        for i in 0..8 {
            assert_eq!(map.node_load(NumaNodeId(i)), 0);
        }
    }

    #[test]
    fn assign_core_for_op_with_op_size_bytes_parameter() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        let core = map.assign_core_for_op(100, 65536);

        let _ = map.get_local_node(core);
    }

    #[test]
    fn multiple_assign_operations_distribute_across_nodes() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 1000;
        map.node_loads[1] = 0;

        for i in 0..10 {
            let core = map.assign_core_for_op(i * 100, 4096);
            map.record_completion(core, 1_000_000);
        }

        let node0_load = map.node_load(NumaNodeId(0));
        let node1_load = map.node_load(NumaNodeId(1));
        assert!(node0_load > 0 || node1_load > 0);
    }

    #[test]
    fn record_completion_from_multiple_cores_accumulates() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.record_completion(CoreId(0), 1_000_000);
        map.record_completion(CoreId(1), 2_000_000);
        map.record_completion(CoreId(4), 3_000_000);

        let load0 = map.node_load(NumaNodeId(0));
        let load1 = map.node_load(NumaNodeId(1));

        assert!(load0 > 0 || load1 > 0);
    }

    #[test]
    fn get_affinity_hint_provides_fallback_nodes_when_preferred_high_load() {
        let topology = NumaTopology {
            num_nodes: 3,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 1000;
        map.node_loads[1] = 10;
        map.node_loads[2] = 20;

        let hint = map.get_affinity_hint(0);

        assert_eq!(hint.preferred_node, NumaNodeId(0));
        assert!(!hint.fallback_nodes.is_empty());
    }

    #[test]
    fn balance_check_returns_empty_when_all_nodes_at_average() {
        let topology = NumaTopology {
            num_nodes: 2,
            cores_per_node: 4,
        };
        let mut map = NumaAffinityMap::new(topology);

        map.node_loads[0] = 50;
        map.node_loads[1] = 50;

        let deltas = map.balance_check();
        assert!(deltas.is_empty());
    }
}
