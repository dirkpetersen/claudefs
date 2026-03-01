//! Chaos/Fault Injection Utilities - For simulating distributed system failures

use std::collections::HashMap;
use thiserror::Error;

pub type NodeId = u32;

#[derive(Error, Debug)]
pub enum ChaosError {
    #[error("Invalid node: {0}")]
    InvalidNode(String),
    #[error("Injection failed: {0}")]
    InjectionFailed(String),
}

/// Type of fault to inject
#[derive(Debug, Clone, PartialEq)]
pub enum FaultType {
    /// Network partition between two nodes
    NetworkPartition { from: NodeId, to: NodeId },
    /// Complete node crash
    NodeCrash(NodeId),
    /// Packet loss with given rate (0.0-1.0)
    PacketLoss { rate: f64 },
    /// Latency spike in milliseconds
    LatencySpike { delay_ms: u64 },
    /// Disk full condition
    DiskFull(NodeId),
}

impl FaultType {
    pub fn description(&self) -> String {
        match self {
            FaultType::NetworkPartition { from, to } => {
                format!("Network partition: {} <-> {}", from, to)
            }
            FaultType::NodeCrash(node) => format!("Node {} crash", node),
            FaultType::PacketLoss { rate } => format!("Packet loss: {}%", rate * 100.0),
            FaultType::LatencySpike { delay_ms } => format!("Latency spike: {}ms", delay_ms),
            FaultType::DiskFull(node) => format!("Disk full on node {}", node),
        }
    }
}

/// Opaque handle to remove a fault
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaultHandle(u64);

impl FaultHandle {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Manages a set of active faults
#[derive(Debug)]
pub struct FaultInjector {
    faults: HashMap<FaultHandle, FaultType>,
    next_handle: u64,
    topology: NetworkTopology,
}

impl FaultInjector {
    pub fn new() -> Self {
        Self {
            faults: HashMap::new(),
            next_handle: 0,
            topology: NetworkTopology::new(),
        }
    }

    /// Inject a fault and get a handle to remove it
    pub fn inject(&mut self, fault: FaultType) -> FaultHandle {
        let handle = FaultHandle(self.next_handle);
        self.next_handle += 1;

        // Update topology based on fault
        match &fault {
            FaultType::NetworkPartition { from, to } => {
                self.topology.add_partition(*from, *to);
            }
            FaultType::NodeCrash(node) => {
                self.topology.mark_node_down(*node);
            }
            _ => {}
        }

        self.faults.insert(handle, fault);
        handle
    }

    /// Remove a fault by handle
    pub fn clear(&mut self, handle: FaultHandle) {
        if let Some(fault) = self.faults.remove(&handle) {
            // Update topology
            match fault {
                FaultType::NetworkPartition { from, to } => {
                    self.topology.remove_partition(from, to);
                }
                FaultType::NodeCrash(node) => {
                    self.topology.mark_node_up(node);
                }
                _ => {}
            }
        }
    }

    /// Clear all faults
    pub fn clear_all(&mut self) {
        self.faults.clear();
        self.topology = NetworkTopology::new();
    }

    /// Get the number of active faults
    pub fn active_faults(&self) -> usize {
        self.faults.len()
    }

    /// Check if a specific fault type is active
    pub fn has_fault(&self, fault: &FaultType) -> bool {
        self.faults.values().any(|f| f == fault)
    }

    /// Get the topology
    pub fn topology(&self) -> &NetworkTopology {
        &self.topology
    }
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks network connectivity between nodes
#[derive(Debug, Clone)]
pub struct NetworkTopology {
    nodes: HashMap<NodeId, bool>, // node_id -> is_up
    partitions: Vec<(NodeId, NodeId)>,
}

impl NetworkTopology {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            partitions: Vec::new(),
        }
    }

    /// Add a node to the topology
    pub fn add_node(&mut self, node: NodeId) {
        self.nodes.insert(node, true);
    }

    /// Add multiple nodes
    pub fn add_nodes(&mut self, nodes: Vec<NodeId>) {
        for node in nodes {
            self.add_node(node);
        }
    }

    /// Add a network partition
    pub fn add_partition(&mut self, from: NodeId, to: NodeId) {
        self.add_node(from);
        self.add_node(to);

        // Ensure the partition exists (both directions)
        let forward = (from, to);
        let backward = (to, from);

        if !self.partitions.contains(&forward) {
            self.partitions.push(forward);
        }
        if !self.partitions.contains(&backward) {
            self.partitions.push(backward);
        }
    }

    /// Remove a network partition
    pub fn remove_partition(&mut self, from: NodeId, to: NodeId) {
        self.partitions.retain(|(a, b)| !(*a == from && *b == to));
    }

    /// Mark a node as down (crashed)
    pub fn mark_node_down(&mut self, node: NodeId) {
        self.nodes.insert(node, false);
    }

    /// Mark a node as up (recovered)
    pub fn mark_node_up(&mut self, node: NodeId) {
        self.nodes.insert(node, true);
    }

    /// Check if two nodes can reach each other
    pub fn can_reach(&self, from: NodeId, to: NodeId) -> bool {
        // Check if both nodes are up
        let from_up = self.nodes.get(&from).copied().unwrap_or(true);
        let to_up = self.nodes.get(&to).copied().unwrap_or(true);

        if !from_up || !to_up {
            return false;
        }

        // Check if there's a partition
        !self.partitions.contains(&(from, to))
    }

    /// Get all up nodes
    pub fn up_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|(_, up)| **up)
            .map(|(&node, _)| node)
            .collect()
    }

    /// Get the number of partitions
    pub fn partition_count(&self) -> usize {
        self.partitions.len() / 2 // Each partition is stored in both directions
    }
}

impl Default for NetworkTopology {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_handle_new() {
        let handle = FaultHandle::new(42);
        assert_eq!(handle.0, 42);
    }

    #[test]
    fn test_fault_type_description() {
        let partition = FaultType::NetworkPartition { from: 1, to: 2 };
        assert!(partition.description().contains("partition"));

        let crash = FaultType::NodeCrash(5);
        assert!(crash.description().contains("crash"));

        let loss = FaultType::PacketLoss { rate: 0.5 };
        assert!(loss.description().contains("loss"));

        let latency = FaultType::LatencySpike { delay_ms: 100 };
        assert!(latency.description().contains("Latency spike"));

        let disk = FaultType::DiskFull(3);
        assert!(disk.description().contains("Disk"));
    }

    #[test]
    fn test_fault_injector_new() {
        let injector = FaultInjector::new();
        assert_eq!(injector.active_faults(), 0);
    }

    #[test]
    fn test_fault_injector_inject() {
        let mut injector = FaultInjector::new();
        let handle = injector.inject(FaultType::NodeCrash(1));
        assert_eq!(injector.active_faults(), 1);
        assert!(injector.has_fault(&FaultType::NodeCrash(1)));
    }

    #[test]
    fn test_fault_injector_clear() {
        let mut injector = FaultInjector::new();
        let handle = injector.inject(FaultType::NodeCrash(1));
        injector.clear(handle);
        assert_eq!(injector.active_faults(), 0);
    }

    #[test]
    fn test_fault_injector_clear_all() {
        let mut injector = FaultInjector::new();
        injector.inject(FaultType::NodeCrash(1));
        injector.inject(FaultType::NodeCrash(2));
        injector.clear_all();
        assert_eq!(injector.active_faults(), 0);
    }

    #[test]
    fn test_network_topology_new() {
        let topology = NetworkTopology::new();
        assert_eq!(topology.partition_count(), 0);
    }

    #[test]
    fn test_network_topology_add_node() {
        let mut topology = NetworkTopology::new();
        topology.add_node(1);
        topology.add_node(2);

        assert!(topology.can_reach(1, 2));
        assert!(topology.can_reach(2, 1));
    }

    #[test]
    fn test_network_topology_add_nodes() {
        let mut topology = NetworkTopology::new();
        topology.add_nodes(vec![1, 2, 3]);

        assert!(topology.can_reach(1, 2));
        assert!(topology.can_reach(2, 3));
    }

    #[test]
    fn test_network_topology_partition() {
        let mut topology = NetworkTopology::new();
        topology.add_partition(1, 2);

        assert!(!topology.can_reach(1, 2));
        assert!(!topology.can_reach(2, 1));
    }

    #[test]
    fn test_network_topology_remove_partition() {
        let mut topology = NetworkTopology::new();
        topology.add_partition(1, 2);
        topology.remove_partition(1, 2);

        assert!(topology.can_reach(1, 2));
    }

    #[test]
    fn test_network_topology_node_down() {
        let mut topology = NetworkTopology::new();
        topology.add_node(1);
        topology.add_node(2);

        topology.mark_node_down(1);

        assert!(!topology.can_reach(1, 2));
        assert!(!topology.can_reach(2, 1));
    }

    #[test]
    fn test_network_topology_node_up() {
        let mut topology = NetworkTopology::new();
        topology.add_node(1);
        topology.mark_node_down(1);
        topology.mark_node_up(1);

        assert!(topology.can_reach(1, 1));
    }

    #[test]
    fn test_network_topology_up_nodes() {
        let mut topology = NetworkTopology::new();
        topology.add_nodes(vec![1, 2, 3]);
        topology.mark_node_down(2);

        let up = topology.up_nodes();
        assert!(up.contains(&1));
        assert!(!up.contains(&2));
        assert!(up.contains(&3));
    }

    #[test]
    fn test_network_topology_partition_count() {
        let mut topology = NetworkTopology::new();
        topology.add_partition(1, 2);

        assert_eq!(topology.partition_count(), 1);
    }

    #[test]
    fn test_fault_injector_topology() {
        let mut injector = FaultInjector::new();
        injector.inject(FaultType::NetworkPartition { from: 1, to: 2 });

        let topology = injector.topology();
        assert!(!topology.can_reach(1, 2));
    }

    #[test]
    fn test_multiple_faults() {
        let mut injector = FaultInjector::new();

        injector.inject(FaultType::NodeCrash(1));
        injector.inject(FaultType::NodeCrash(2));
        injector.inject(FaultType::LatencySpike { delay_ms: 100 });

        assert_eq!(injector.active_faults(), 3);
    }

    #[test]
    fn test_fault_handle_equality() {
        let h1 = FaultHandle(1);
        let h2 = FaultHandle(1);
        let h3 = FaultHandle(2);

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_fault_type_equality() {
        let f1 = FaultType::NodeCrash(1);
        let f2 = FaultType::NodeCrash(1);
        let f3 = FaultType::NodeCrash(2);

        assert_eq!(f1, f2);
        assert_ne!(f1, f3);
    }

    #[test]
    fn test_network_topology_default() {
        let topology = NetworkTopology::default();
        assert!(topology.up_nodes().is_empty());
    }

    #[test]
    fn test_fault_injector_default() {
        let injector = FaultInjector::default();
        assert_eq!(injector.active_faults(), 0);
    }

    #[test]
    fn test_network_topology_can_reach_unknown_nodes() {
        let topology = NetworkTopology::new();
        // Unknown nodes are considered reachable (not partitioned)
        assert!(topology.can_reach(999, 888));
    }

    #[test]
    fn test_packet_loss_rate() {
        let loss1 = FaultType::PacketLoss { rate: 0.0 };
        let loss2 = FaultType::PacketLoss { rate: 0.5 };
        let loss3 = FaultType::PacketLoss { rate: 1.0 };

        assert!(loss1.description().contains("0%"));
        assert!(loss2.description().contains("50%"));
        assert!(loss3.description().contains("100%"));
    }

    #[test]
    fn test_latency_spike_values() {
        let latency = FaultType::LatencySpike { delay_ms: 5000 };
        assert!(latency.description().contains("5000"));
    }
}
