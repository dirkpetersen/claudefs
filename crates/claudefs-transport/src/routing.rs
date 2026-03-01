//! Shard-aware request routing for distributed filesystem.
//!
//! This module provides consistent hashing and shard-based routing for distributing
//! filesystem operations across a cluster of storage nodes.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// Unique identifier for a node in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(u64);

impl NodeId {
    /// Creates a new NodeId from a raw u64 value.
    pub fn new(id: u64) -> Self {
        NodeId(id)
    }

    /// Returns the underlying u64 value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl From<u64> for NodeId {
    fn from(id: u64) -> Self {
        NodeId(id)
    }
}

impl From<NodeId> for u64 {
    fn from(id: NodeId) -> Self {
        id.0
    }
}

impl Default for NodeId {
    fn default() -> Self {
        NodeId(0)
    }
}

/// Unique identifier for a shard in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardId(u32);

impl ShardId {
    /// Creates a new ShardId from a raw u32 value.
    pub fn new(id: u32) -> Self {
        ShardId(id)
    }

    /// Returns the underlying u32 value.
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl From<u32> for ShardId {
    fn from(id: u32) -> Self {
        ShardId(id)
    }
}

impl From<ShardId> for u32 {
    fn from(id: ShardId) -> Self {
        id.0
    }
}

impl Default for ShardId {
    fn default() -> Self {
        ShardId(0)
    }
}

/// Information about a storage node in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique node identifier.
    pub id: NodeId,
    /// Network address of the node.
    pub address: SocketAddr,
    /// Availability zone for rack/region awareness.
    pub zone: String,
    /// Load balancing weight for traffic distribution.
    pub weight: u32,
    /// Whether the node is currently alive/available.
    pub alive: bool,
}

impl NodeInfo {
    /// Creates a new NodeInfo with default values.
    pub fn new(id: NodeId, address: SocketAddr) -> Self {
        NodeInfo {
            id,
            address,
            zone: String::new(),
            weight: 100,
            alive: true,
        }
    }

    /// Creates a new NodeInfo with all fields specified.
    pub fn with_zone_weight(id: NodeId, address: SocketAddr, zone: &str, weight: u32) -> Self {
        NodeInfo {
            id,
            address,
            zone: zone.to_string(),
            weight,
            alive: true,
        }
    }
}

/// Consistent hash ring for distributing keys across nodes.
///
/// Uses virtual nodes to improve distribution balance. Each physical node
/// is represented by multiple virtual nodes distributed around the ring.
#[derive(Debug, Clone)]
pub struct ConsistentHashRing {
    ring: BTreeMap<u64, (NodeId, u32)>,
    nodes: HashMap<NodeId, NodeInfo>,
    virtual_node_count: u32,
}

impl ConsistentHashRing {
    /// Creates a new empty consistent hash ring.
    pub fn new() -> Self {
        ConsistentHashRing {
            ring: BTreeMap::new(),
            nodes: HashMap::new(),
            virtual_node_count: 150,
        }
    }

    /// Adds a node to the hash ring with virtual nodes for load balancing.
    ///
    /// The number of virtual nodes determines how evenly keys are distributed.
    /// More virtual nodes = better distribution but more memory usage.
    pub fn add_node(&mut self, node: NodeInfo, virtual_nodes: u32) {
        let node_id = node.id;

        for i in 0..virtual_nodes {
            let key = self.hash_node(&node_id, i);
            self.ring.insert(key, (node_id, i));
        }

        self.nodes.insert(node_id, node);
    }

    /// Removes a node and all its virtual nodes from the ring.
    pub fn remove_node(&mut self, node_id: NodeId) {
        let mut keys_to_remove = Vec::new();

        for (key, &(stored_node_id, _)) in &self.ring {
            if stored_node_id == node_id {
                keys_to_remove.push(*key);
            }
        }

        for key in keys_to_remove {
            self.ring.remove(&key);
        }

        self.nodes.remove(&node_id);
    }

    /// Looks up the primary node for a given key.
    ///
    /// Returns the NodeId of the node responsible for the key,
    /// or None if the ring is empty.
    pub fn lookup(&self, key: &[u8]) -> Option<NodeId> {
        if self.ring.is_empty() {
            return None;
        }

        let hash = self.hash_key(key);
        self.find_node(hash)
    }

    /// Looks up n distinct nodes for replication.
    ///
    /// Returns up to n unique NodeIds, useful for replicating data
    /// across multiple nodes for fault tolerance.
    pub fn lookup_n(&self, key: &[u8], n: usize) -> Vec<NodeId> {
        if self.ring.is_empty() || n == 0 {
            return Vec::new();
        }

        let hash = self.hash_key(key);
        let mut result = Vec::new();
        let mut seen = HashMap::new();

        let start_pos = self.find_position(hash);
        let mut pos = start_pos;

        let entries: Vec<_> = self.ring.iter().collect();
        let total = entries.len();

        while result.len() < n && result.len() < total {
            if let Some((&_key, &(node_id, _))) = entries.get(pos % total) {
                if !seen.contains_key(&node_id) {
                    seen.insert(node_id, true);
                    result.push(node_id);
                }
            }
            pos += 1;

            if pos - start_pos >= total {
                break;
            }
        }

        result
    }

    /// Returns the number of physical nodes in the ring.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns a reference to all nodes in the ring.
    pub fn nodes(&self) -> &HashMap<NodeId, NodeInfo> {
        &self.nodes
    }

    /// Hashes a key using FNV-1a.
    fn hash_key(&self, key: &[u8]) -> u64 {
        let hash = if key.len() == 8 {
            u64::from_le_bytes(key.try_into().unwrap())
        } else if key.len() == 4 {
            u64::from(u32::from_le_bytes(key.try_into().unwrap()))
        } else {
            let mut h: u64 = 14695981039346656037;
            for &byte in key {
                h ^= byte as u64;
                h = h.wrapping_mul(1099511628211);
            }
            h
        };

        let mut x = hash.wrapping_add(0x9e3779b97f4a7c15);
        x = x.wrapping_mul(0xbf58476d1ce4e5b9);
        x ^= x >> 30;
        x.wrapping_mul(0x94d049bb133111eb)
    }

    /// Hashes a node ID with virtual node index for ring placement.
    fn hash_node(&self, node_id: &NodeId, virtual_index: u32) -> u64 {
        let combined = (node_id.0 << 16) as u64 | (virtual_index as u64 & 0xFFFF);
        let mut x = combined.wrapping_add(0x9e3779b97f4a7c15);
        x = x.wrapping_mul(0xbf58476d1ce4e5b9);
        x ^= x >> 30;
        x.wrapping_mul(0x94d049bb133111eb)
    }

    /// Finds the position in the ring for a given hash value.
    fn find_position(&self, hash: u64) -> usize {
        let entries: Vec<_> = self.ring.iter().collect();

        if entries.is_empty() {
            return 0;
        }

        let pos = entries.binary_search_by(|&(k, _)| k.cmp(&hash));

        match pos {
            Ok(pos) => pos,
            Err(pos) => pos,
        }
    }

    /// Finds the node responsible for a hash value.
    fn find_node(&self, hash: u64) -> Option<NodeId> {
        let entries: Vec<_> = self.ring.iter().collect();

        if entries.is_empty() {
            return None;
        }

        if hash <= *entries[0].0 {
            let (&_key, &(node_id, _)) = entries[0];
            return Some(node_id);
        }

        let pos = self.find_position(hash);
        let (&_key, &(node_id, _)) = entries[pos % entries.len()];

        Some(node_id)
    }
}

impl Default for ConsistentHashRing {
    fn default() -> Self {
        Self::new()
    }
}

/// Routes filesystem requests from inodes to shards to nodes.
///
/// Provides two-level routing: first maps inode to shard, then shard to node.
/// This enables independent scaling of metadata and data planes.
#[derive(Debug, Clone)]
pub struct ShardRouter {
    shard_count: u32,
    nodes: HashMap<NodeId, NodeInfo>,
    hash_ring: ConsistentHashRing,
    routing_table: RoutingTable,
}

impl ShardRouter {
    /// Creates a new ShardRouter with the specified number of shards.
    pub fn new(shard_count: u32) -> Self {
        ShardRouter {
            shard_count,
            nodes: HashMap::new(),
            hash_ring: ConsistentHashRing::new(),
            routing_table: RoutingTable::new(shard_count),
        }
    }

    /// Maps an inode to a shard using modulo-based distribution.
    ///
    /// Uses simple modulo for deterministic shard assignment.
    pub fn inode_to_shard(&self, inode: u64) -> ShardId {
        ShardId((inode % self.shard_count as u64) as u32)
    }

    /// Routes a shard to its primary node.
    ///
    /// Returns the NodeId responsible for the shard, or None if
    /// no node is assigned to the shard.
    pub fn route(&self, shard: ShardId) -> Option<NodeId> {
        self.routing_table.lookup(shard)
    }

    /// Routes a key directly to a node using consistent hashing.
    ///
    /// Bypasses the shard routing layer and directly maps
    /// the key to a node using the hash ring.
    pub fn route_key(&self, key: &[u8]) -> Option<NodeId> {
        self.hash_ring.lookup(key)
    }

    /// Adds a node to the router.
    ///
    /// The node will be added to the hash ring and assigned shards
    /// based on the current routing table configuration.
    pub fn add_node(&mut self, info: NodeInfo) {
        let node_id = info.id;
        self.nodes.insert(node_id, info.clone());
        self.hash_ring.add_node(info, 150);
        self.rebalance();
    }

    /// Removes a node from the router.
    ///
    /// Shards previously assigned to this node will be rebalanced
    /// to remaining nodes.
    pub fn remove_node(&mut self, id: NodeId) {
        self.nodes.remove(&id);
        self.hash_ring.remove_node(id);

        let shards_to_reassign: Vec<ShardId> =
            self.routing_table.node_shards(id).into_iter().collect();

        for shard in shards_to_reassign {
            self.routing_table.assign(shard, NodeId::default());
        }

        self.rebalance();
    }

    /// Returns all nodes in the router.
    pub fn all_nodes(&self) -> Vec<NodeInfo> {
        self.nodes.values().cloned().collect()
    }

    /// Rebalances shard assignments across available nodes.
    ///
    /// Called after topology changes (node addition/removal) to
    /// evenly distribute load across the cluster.
    pub fn rebalance(&mut self) {
        if self.nodes.is_empty() {
            return;
        }

        let nodes: Vec<NodeId> = self.nodes.keys().copied().collect();
        let node_count = nodes.len() as u32;

        if node_count == 0 {
            return;
        }

        for shard_id in 0..self.shard_count {
            let shard = ShardId(shard_id);
            let node_index = (shard_id as usize) % nodes.len();
            self.routing_table.assign(shard, nodes[node_index]);
        }
    }

    /// Returns the hash of a key using FNV-1a.
    fn hash_key(&self, key: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for &byte in key {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }
}

/// Lookup table for shard-to-node assignments.
///
/// Provides O(1) lookup for determining which node handles
/// operations for a particular shard.
#[derive(Debug, Clone)]
pub struct RoutingTable {
    shards: HashMap<ShardId, NodeId>,
    node_shards: HashMap<NodeId, Vec<ShardId>>,
}

impl RoutingTable {
    /// Creates a new empty routing table.
    pub fn new(shard_count: u32) -> Self {
        let mut shards = HashMap::new();
        for i in 0..shard_count {
            shards.insert(ShardId(i), NodeId::default());
        }

        RoutingTable {
            shards,
            node_shards: HashMap::new(),
        }
    }

    /// Assigns a shard to a node.
    pub fn assign(&mut self, shard: ShardId, node: NodeId) {
        if let Some(&old_node) = self.shards.get(&shard) {
            if old_node != NodeId::default() {
                if let Some(shards) = self.node_shards.get_mut(&old_node) {
                    shards.retain(|s| *s != shard);
                }
            }
        }

        self.shards.insert(shard, node);

        if node != NodeId::default() {
            self.node_shards
                .entry(node)
                .or_insert_with(Vec::new)
                .push(shard);
        }
    }

    /// Looks up the node responsible for a shard.
    pub fn lookup(&self, shard: ShardId) -> Option<NodeId> {
        self.shards.get(&shard).copied()
    }

    /// Returns all shards assigned to a node.
    pub fn node_shards(&self, node: NodeId) -> Vec<ShardId> {
        self.node_shards.get(&node).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::str::FromStr;

    fn make_addr(port: u16) -> SocketAddr {
        SocketAddr::from_str(&format!("192.168.1.1:{}", port)).unwrap()
    }

    #[test]
    fn test_node_id_basic() {
        let id = NodeId::new(42);
        assert_eq!(id.as_u64(), 42);
        assert_eq!(u64::from(id), 42);
    }

    #[test]
    fn test_shard_id_basic() {
        let id = ShardId::new(10);
        assert_eq!(id.as_u32(), 10);
        assert_eq!(u32::from(id), 10);
    }

    #[test]
    fn test_node_info() {
        let addr = make_addr(9000);
        let info = NodeInfo::with_zone_weight(NodeId::new(1), addr, "zone-a", 100);

        assert_eq!(info.id.as_u64(), 1);
        assert_eq!(info.zone, "zone-a");
        assert_eq!(info.weight, 100);
        assert!(info.alive);
    }

    #[test]
    fn test_consistent_hash_ring_empty() {
        let ring = ConsistentHashRing::new();
        assert!(ring.lookup(b"test").is_none());
        assert!(ring.lookup_n(b"test", 3).is_empty());
    }

    #[test]
    fn test_consistent_hash_ring_single_node() {
        let mut ring = ConsistentHashRing::new();
        let addr = make_addr(9000);
        let node = NodeInfo::with_zone_weight(NodeId::new(1), addr, "zone-a", 100);
        ring.add_node(node, 150);

        assert_eq!(ring.node_count(), 1);
        assert_eq!(ring.lookup(b"test").unwrap().as_u64(), 1);
        assert_eq!(ring.lookup_n(b"test", 3).len(), 1);
    }

    #[test]
    fn test_consistent_hash_ring_distribution() {
        let mut ring = ConsistentHashRing::new();

        for i in 1..=5u64 {
            let addr = make_addr(9000 + i as u16);
            let node = NodeInfo::with_zone_weight(NodeId::new(i), addr, "zone-a", 100);
            ring.add_node(node, 150);
        }

        let mut counts = HashMap::new();
        for i in 0..10000u64 {
            if let Some(node_id) = ring.lookup(&i.to_le_bytes()) {
                *counts.entry(node_id).or_insert(0) += 1;
            }
        }

        let min_count = *counts.values().min().unwrap();
        let max_count = *counts.values().max().unwrap();

        assert!(
            max_count - min_count < 3000,
            "Distribution too uneven: min={}, max={}",
            min_count,
            max_count
        );
    }

    #[test]
    fn test_consistent_hash_ring_lookup_n() {
        let mut ring = ConsistentHashRing::new();

        for i in 1..=3u64 {
            let addr = make_addr(9000 + i as u16);
            let node = NodeInfo::with_zone_weight(NodeId::new(i), addr, "zone-a", 100);
            ring.add_node(node, 150);
        }

        let result = ring.lookup_n(b"test-key", 3);
        assert_eq!(result.len(), 3);

        let unique: HashSet<_> = result.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn test_consistent_hash_ring_remove_node() {
        let mut ring = ConsistentHashRing::new();

        let addr1 = make_addr(9001);
        let node1 = NodeInfo::with_zone_weight(NodeId::new(1), addr1, "zone-a", 100);
        ring.add_node(node1, 150);

        let addr2 = make_addr(9002);
        let node2 = NodeInfo::with_zone_weight(NodeId::new(2), addr2, "zone-a", 100);
        ring.add_node(node2, 150);

        assert_eq!(ring.node_count(), 2);

        ring.remove_node(NodeId::new(1));
        assert_eq!(ring.node_count(), 1);
        assert!(ring.lookup(b"test").unwrap().as_u64() != 1);
    }

    #[test]
    fn test_shard_router_inode_to_shard() {
        let router = ShardRouter::new(256);

        let shard = router.inode_to_shard(0);
        assert!(shard.as_u32() < 256);

        let shard_same = router.inode_to_shard(256 * 256);
        assert_eq!(shard.as_u32(), shard_same.as_u32());
    }

    #[test]
    fn test_shard_router_add_remove_nodes() {
        let mut router = ShardRouter::new(256);

        let addr1 = make_addr(9001);
        let node1 = NodeInfo::with_zone_weight(NodeId::new(1), addr1, "zone-a", 100);
        router.add_node(node1);

        let addr2 = make_addr(9002);
        let node2 = NodeInfo::with_zone_weight(NodeId::new(2), addr2, "zone-b", 100);
        router.add_node(node2);

        assert_eq!(router.all_nodes().len(), 2);

        let shard = ShardId(0);
        let node = router.route(shard);
        assert!(node.is_some());

        router.remove_node(NodeId::new(1));
        assert_eq!(router.all_nodes().len(), 1);
    }

    #[test]
    fn test_shard_router_route_key() {
        let mut router = ShardRouter::new(256);

        let addr = make_addr(9001);
        let node = NodeInfo::with_zone_weight(NodeId::new(1), addr, "zone-a", 100);
        router.add_node(node);

        let result = router.route_key(b"test-key");
        assert!(result.is_some());
    }

    #[test]
    fn test_shard_router_rebalance() {
        let mut router = ShardRouter::new(10);

        let addr1 = make_addr(9001);
        let node1 = NodeInfo::with_zone_weight(NodeId::new(1), addr1, "zone-a", 100);
        router.add_node(node1);

        let shard = ShardId(0);
        let node = router.route(shard);
        assert!(node.is_some());

        let addr2 = make_addr(9002);
        let node2 = NodeInfo::with_zone_weight(NodeId::new(2), addr2, "zone-b", 100);
        router.add_node(node2);

        assert_eq!(router.all_nodes().len(), 2);
    }

    #[test]
    fn test_routing_table_assign_lookup() {
        let mut table = RoutingTable::new(10);

        table.assign(ShardId(0), NodeId::new(1));
        table.assign(ShardId(1), NodeId::new(2));

        assert_eq!(table.lookup(ShardId(0)).unwrap().as_u64(), 1);
        assert_eq!(table.lookup(ShardId(1)).unwrap().as_u64(), 2);
    }

    #[test]
    fn test_routing_table_node_shards() {
        let mut table = RoutingTable::new(10);

        table.assign(ShardId(0), NodeId::new(1));
        table.assign(ShardId(1), NodeId::new(1));
        table.assign(ShardId(2), NodeId::new(2));

        let shards = table.node_shards(NodeId::new(1));
        assert_eq!(shards.len(), 2);
    }

    #[test]
    fn test_consistent_hash_deterministic() {
        let mut ring1 = ConsistentHashRing::new();
        let mut ring2 = ConsistentHashRing::new();

        let addr = make_addr(9001);
        let node = NodeInfo::with_zone_weight(NodeId::new(1), addr, "zone-a", 100);

        ring1.add_node(node.clone(), 150);
        ring2.add_node(node, 150);

        let key = b"consistent-key";
        assert_eq!(ring1.lookup(key), ring2.lookup(key));
    }

    #[test]
    fn test_shard_router_shard_distribution() {
        let mut router = ShardRouter::new(100);

        for i in 1..=4u64 {
            let addr = make_addr(9000 + i as u16);
            let node = NodeInfo::with_zone_weight(NodeId::new(i), addr, "zone-a", 100);
            router.add_node(node);
        }

        let mut node_shard_counts: HashMap<u64, usize> = HashMap::new();

        for shard_id in 0..100u32 {
            if let Some(node_id) = router.route(ShardId(shard_id)) {
                *node_shard_counts.entry(node_id.as_u64()).or_insert(0) += 1;
            }
        }

        let min = *node_shard_counts.values().min().unwrap();
        let max = *node_shard_counts.values().max().unwrap();

        assert!(
            max - min <= 2,
            "Shard distribution uneven: min={}, max={}",
            min,
            max
        );
    }
}
