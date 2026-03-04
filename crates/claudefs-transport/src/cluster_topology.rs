//! Rack/datacenter-aware cluster topology for optimized data placement.
//!
//! This module tracks which node is in which rack and datacenter,
//! providing topology-aware preference ordering for multi-site ClaudeFS.
//! Architecture decision D8: prefer local rack for EC stripe placement.

use std::collections::{HashMap, HashSet};

/// Datacenter label (e.g., "us-west-2a").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DatacenterId(pub String);

impl DatacenterId {
    /// Creates a new datacenter ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the datacenter ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Rack label within a datacenter (e.g., "rack-07").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RackId(pub String);

impl RackId {
    /// Creates a new rack ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the rack ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Physical location of a node.
#[derive(Debug, Clone)]
pub struct TopologyLabel {
    /// Datacenter where the node resides.
    pub datacenter: DatacenterId,
    /// Rack within the datacenter.
    pub rack: RackId,
    /// Optional hostname for logging.
    pub hostname: String,
}

impl TopologyLabel {
    /// Creates a new topology label.
    pub fn new(
        datacenter: impl Into<String>,
        rack: impl Into<String>,
        hostname: impl Into<String>,
    ) -> Self {
        Self {
            datacenter: DatacenterId::new(datacenter),
            rack: RackId::new(rack),
            hostname: hostname.into(),
        }
    }
}

/// Proximity between two nodes (lower = closer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Proximity {
    /// Same node (distance 0).
    SameNode,
    /// Same rack, different node (distance 1).
    SameRack,
    /// Same datacenter, different rack (distance 2).
    SameDatacenter,
    /// Different datacenter (distance 3).
    RemoteDatacenter,
}

impl Proximity {
    /// Numeric cost (0=SameNode, 1=SameRack, 2=SameDatacenter, 3=RemoteDatacenter).
    pub fn cost(&self) -> u32 {
        match self {
            Proximity::SameNode => 0,
            Proximity::SameRack => 1,
            Proximity::SameDatacenter => 2,
            Proximity::RemoteDatacenter => 3,
        }
    }
}

/// Cluster topology: maps node IDs to their physical location.
pub struct ClusterTopology {
    nodes: HashMap<u64, TopologyLabel>,
}

impl ClusterTopology {
    /// Creates a new empty topology.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    /// Adds a node with its topology label.
    pub fn add_node(&mut self, node_id: u64, label: TopologyLabel) {
        self.nodes.insert(node_id, label);
    }

    /// Removes a node from the topology.
    ///
    /// Returns the removed label, if any.
    pub fn remove_node(&mut self, node_id: u64) -> Option<TopologyLabel> {
        self.nodes.remove(&node_id)
    }

    /// Returns the topology label for a node.
    pub fn get_label(&self, node_id: u64) -> Option<&TopologyLabel> {
        self.nodes.get(&node_id)
    }

    /// Returns the total number of nodes in the topology.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Computes proximity between two nodes.
    ///
    /// Returns `Proximity::RemoteDatacenter` if either node is not in the topology.
    pub fn proximity(&self, a: u64, b: u64) -> Proximity {
        if a == b {
            return Proximity::SameNode;
        }

        let label_a = match self.nodes.get(&a) {
            Some(l) => l,
            None => return Proximity::RemoteDatacenter,
        };
        let label_b = match self.nodes.get(&b) {
            Some(l) => l,
            None => return Proximity::RemoteDatacenter,
        };

        if label_a.datacenter != label_b.datacenter {
            return Proximity::RemoteDatacenter;
        }

        if label_a.rack != label_b.rack {
            return Proximity::SameDatacenter;
        }

        Proximity::SameRack
    }

    /// Returns all nodes sorted by proximity to `origin` (closest first).
    ///
    /// Ties are broken by node ID for determinism.
    pub fn sorted_by_proximity(&self, origin: u64) -> Vec<u64> {
        let mut nodes_with_proximity: Vec<(u64, Proximity)> = self
            .nodes
            .keys()
            .map(|&id| (id, self.proximity(origin, id)))
            .collect();

        nodes_with_proximity.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

        nodes_with_proximity.into_iter().map(|(id, _)| id).collect()
    }

    /// Returns nodes in the same rack as `origin` (excluding origin itself).
    pub fn same_rack_peers(&self, origin: u64) -> Vec<u64> {
        let origin_label = match self.nodes.get(&origin) {
            Some(l) => l,
            None => return Vec::new(),
        };

        self.nodes
            .iter()
            .filter(|(&id, label)| {
                id != origin
                    && label.datacenter == origin_label.datacenter
                    && label.rack == origin_label.rack
            })
            .map(|(&id, _)| id)
            .collect()
    }

    /// Returns nodes in the same datacenter but different rack.
    pub fn same_dc_cross_rack_peers(&self, origin: u64) -> Vec<u64> {
        let origin_label = match self.nodes.get(&origin) {
            Some(l) => l,
            None => return Vec::new(),
        };

        self.nodes
            .iter()
            .filter(|(&id, label)| {
                id != origin
                    && label.datacenter == origin_label.datacenter
                    && label.rack != origin_label.rack
            })
            .map(|(&id, _)| id)
            .collect()
    }

    /// Returns nodes in remote datacenters.
    pub fn remote_dc_peers(&self, origin: u64) -> Vec<u64> {
        let origin_label = match self.nodes.get(&origin) {
            Some(l) => l,
            None => return Vec::new(),
        };

        self.nodes
            .iter()
            .filter(|(&id, label)| id != origin && label.datacenter != origin_label.datacenter)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Returns all unique datacenter IDs.
    pub fn datacenters(&self) -> Vec<&str> {
        let mut dcs: HashSet<&str> = HashSet::new();
        for label in self.nodes.values() {
            dcs.insert(label.datacenter.as_str());
        }
        let mut result: Vec<&str> = dcs.into_iter().collect();
        result.sort();
        result
    }

    /// Returns all unique rack IDs within a datacenter.
    pub fn racks_in_datacenter(&self, dc: &str) -> Vec<&str> {
        let mut racks: HashSet<&str> = HashSet::new();
        for label in self.nodes.values() {
            if label.datacenter.as_str() == dc {
                racks.insert(label.rack.as_str());
            }
        }
        let mut result: Vec<&str> = racks.into_iter().collect();
        result.sort();
        result
    }

    /// Returns topology statistics.
    pub fn stats(&self) -> TopologyStatsSnapshot {
        let mut datacenters: HashSet<&str> = HashSet::new();
        let mut racks: HashSet<(&str, &str)> = HashSet::new();

        for label in self.nodes.values() {
            datacenters.insert(label.datacenter.as_str());
            racks.insert((label.datacenter.as_str(), label.rack.as_str()));
        }

        TopologyStatsSnapshot {
            node_count: self.nodes.len(),
            datacenter_count: datacenters.len(),
            rack_count: racks.len(),
        }
    }
}

impl Default for ClusterTopology {
    fn default() -> Self {
        Self::new()
    }
}

/// Topology stats snapshot.
#[derive(Debug, Clone)]
pub struct TopologyStatsSnapshot {
    /// Total nodes in topology.
    pub node_count: usize,
    /// Number of unique datacenters.
    pub datacenter_count: usize,
    /// Number of unique racks.
    pub rack_count: usize,
}

#[cfg(test)]
mod tests {
    use crate::cluster_topology::{
        ClusterTopology, DatacenterId, Proximity, RackId, TopologyLabel, TopologyStatsSnapshot,
    };

    #[test]
    fn test_empty_topology() {
        let topo = ClusterTopology::new();
        assert_eq!(topo.node_count(), 0);
        assert!(topo.datacenters().is_empty());
    }

    #[test]
    fn test_single_node_topology() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "host1"));

        assert_eq!(topo.node_count(), 1);
        assert!(topo.get_label(1).is_some());
        assert!(topo.get_label(2).is_none());
    }

    #[test]
    fn test_add_remove_node() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "host1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack1", "host2"));

        assert_eq!(topo.node_count(), 2);

        let removed = topo.remove_node(1);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().hostname, "host1");
        assert_eq!(topo.node_count(), 1);
        assert!(topo.get_label(1).is_none());
    }

    #[test]
    fn test_proximity_same_node() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "host1"));

        assert_eq!(topo.proximity(1, 1), Proximity::SameNode);
        assert_eq!(topo.proximity(1, 1).cost(), 0);
    }

    #[test]
    fn test_proximity_same_rack() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "host1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack1", "host2"));

        assert_eq!(topo.proximity(1, 2), Proximity::SameRack);
        assert_eq!(topo.proximity(1, 2).cost(), 1);
    }

    #[test]
    fn test_proximity_same_dc_different_rack() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "host1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack2", "host2"));

        assert_eq!(topo.proximity(1, 2), Proximity::SameDatacenter);
        assert_eq!(topo.proximity(1, 2).cost(), 2);
    }

    #[test]
    fn test_proximity_remote_dc() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "host1"));
        topo.add_node(2, TopologyLabel::new("dc2", "rack1", "host2"));

        assert_eq!(topo.proximity(1, 2), Proximity::RemoteDatacenter);
        assert_eq!(topo.proximity(1, 2).cost(), 3);
    }

    #[test]
    fn test_proximity_node_not_in_topology() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "host1"));

        assert_eq!(topo.proximity(1, 99), Proximity::RemoteDatacenter);
        assert_eq!(topo.proximity(99, 1), Proximity::RemoteDatacenter);
        assert_eq!(topo.proximity(99, 88), Proximity::RemoteDatacenter);
    }

    #[test]
    fn test_sorted_by_proximity_single_node() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "host1"));

        let sorted = topo.sorted_by_proximity(1);
        assert_eq!(sorted, vec![1]);
    }

    #[test]
    fn test_sorted_by_proximity_multi_tier() {
        let mut topo = ClusterTopology::new();

        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack1", "h2"));
        topo.add_node(3, TopologyLabel::new("dc1", "rack2", "h3"));
        topo.add_node(4, TopologyLabel::new("dc2", "rack1", "h4"));

        let sorted = topo.sorted_by_proximity(1);

        assert_eq!(sorted[0], 1);
        assert_eq!(sorted[1], 2);
        assert!(sorted[2] == 3 || sorted[2] == 4);
    }

    #[test]
    fn test_sorted_by_proximity_deterministic_ties() {
        let mut topo = ClusterTopology::new();
        topo.add_node(10, TopologyLabel::new("dc1", "rack1", "h10"));
        topo.add_node(5, TopologyLabel::new("dc1", "rack1", "h5"));
        topo.add_node(20, TopologyLabel::new("dc1", "rack1", "h20"));

        let sorted = topo.sorted_by_proximity(1);
        assert!(!sorted.is_empty());
        let same_rack: Vec<u64> = sorted.iter().filter(|&&id| id != 1).copied().collect();
        let mut expected = same_rack.clone();
        expected.sort();
        assert_eq!(same_rack, expected);
    }

    #[test]
    fn test_same_rack_peers() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack1", "h2"));
        topo.add_node(3, TopologyLabel::new("dc1", "rack2", "h3"));

        let peers = topo.same_rack_peers(1);
        assert_eq!(peers.len(), 1);
        assert!(peers.contains(&2));
        assert!(!peers.contains(&1));
        assert!(!peers.contains(&3));
    }

    #[test]
    fn test_same_rack_peers_empty() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));

        let peers = topo.same_rack_peers(1);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_same_dc_cross_rack_peers() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack1", "h2"));
        topo.add_node(3, TopologyLabel::new("dc1", "rack2", "h3"));
        topo.add_node(4, TopologyLabel::new("dc2", "rack1", "h4"));

        let peers = topo.same_dc_cross_rack_peers(1);
        assert_eq!(peers.len(), 1);
        assert!(peers.contains(&3));
    }

    #[test]
    fn test_remote_dc_peers() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack2", "h2"));
        topo.add_node(3, TopologyLabel::new("dc2", "rack1", "h3"));
        topo.add_node(4, TopologyLabel::new("dc2", "rack2", "h4"));

        let peers = topo.remote_dc_peers(1);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&3));
        assert!(peers.contains(&4));
    }

    #[test]
    fn test_datacenters() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));
        topo.add_node(2, TopologyLabel::new("dc2", "rack1", "h2"));
        topo.add_node(3, TopologyLabel::new("dc1", "rack2", "h3"));

        let dcs = topo.datacenters();
        assert_eq!(dcs.len(), 2);
        assert!(dcs.contains(&"dc1"));
        assert!(dcs.contains(&"dc2"));
    }

    #[test]
    fn test_racks_in_datacenter() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack2", "h2"));
        topo.add_node(3, TopologyLabel::new("dc1", "rack1", "h3"));
        topo.add_node(4, TopologyLabel::new("dc2", "rack1", "h4"));

        let racks_dc1 = topo.racks_in_datacenter("dc1");
        assert_eq!(racks_dc1.len(), 2);
        assert!(racks_dc1.contains(&"rack1"));
        assert!(racks_dc1.contains(&"rack2"));

        let racks_dc2 = topo.racks_in_datacenter("dc2");
        assert_eq!(racks_dc2.len(), 1);
        assert!(racks_dc2.contains(&"rack1"));

        let racks_unknown = topo.racks_in_datacenter("unknown");
        assert!(racks_unknown.is_empty());
    }

    #[test]
    fn test_stats() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack2", "h2"));
        topo.add_node(3, TopologyLabel::new("dc2", "rack1", "h3"));

        let stats = topo.stats();
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.datacenter_count, 2);
        assert_eq!(stats.rack_count, 3);
    }

    #[test]
    fn test_large_topology() {
        let mut topo = ClusterTopology::new();

        for i in 0..20 {
            let dc = if i < 10 { "dc1" } else { "dc2" };
            let rack = match i % 5 {
                0 => "rack1",
                1 => "rack2",
                2 => "rack3",
                3 => "rack4",
                _ => "rack5",
            };
            topo.add_node(i, TopologyLabel::new(dc, rack, format!("h{}", i)));
        }

        assert_eq!(topo.node_count(), 20);

        let sorted = topo.sorted_by_proximity(0);
        assert_eq!(sorted.len(), 20);
        assert_eq!(sorted[0], 0);

        for i in 1..sorted.len() {
            let prev_prox = topo.proximity(0, sorted[i - 1]);
            let curr_prox = topo.proximity(0, sorted[i]);
            assert!(prev_prox <= curr_prox);
        }

        let stats = topo.stats();
        assert_eq!(stats.node_count, 20);
        assert_eq!(stats.datacenter_count, 2);
        assert_eq!(stats.rack_count, 10);
    }

    #[test]
    fn test_datacenter_id() {
        let dc1 = DatacenterId::new("us-west-2a");
        let dc2 = DatacenterId::new("us-west-2a");
        let dc3 = DatacenterId::new("us-east-1");

        assert_eq!(dc1, dc2);
        assert_ne!(dc1, dc3);
        assert_eq!(dc1.as_str(), "us-west-2a");
    }

    #[test]
    fn test_rack_id() {
        let r1 = RackId::new("rack-01");
        let r2 = RackId::new("rack-01");
        let r3 = RackId::new("rack-02");

        assert_eq!(r1, r2);
        assert_ne!(r1, r3);
        assert_eq!(r1.as_str(), "rack-01");
    }

    #[test]
    fn test_topology_label() {
        let label = TopologyLabel::new("dc1", "rack1", "host1");
        assert_eq!(label.datacenter.as_str(), "dc1");
        assert_eq!(label.rack.as_str(), "rack1");
        assert_eq!(label.hostname, "host1");
    }

    #[test]
    fn test_proximity_ordering() {
        assert!(Proximity::SameNode < Proximity::SameRack);
        assert!(Proximity::SameRack < Proximity::SameDatacenter);
        assert!(Proximity::SameDatacenter < Proximity::RemoteDatacenter);
    }

    #[test]
    fn test_same_rack_peers_unknown_node() {
        let topo = ClusterTopology::new();
        let peers = topo.same_rack_peers(999);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_remote_dc_peers_all_same_dc() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));
        topo.add_node(2, TopologyLabel::new("dc1", "rack2", "h2"));

        let peers = topo.remote_dc_peers(1);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_sorted_by_proximity_unknown_origin() {
        let mut topo = ClusterTopology::new();
        topo.add_node(1, TopologyLabel::new("dc1", "rack1", "h1"));

        let sorted = topo.sorted_by_proximity(999);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0], 1);
    }
}
