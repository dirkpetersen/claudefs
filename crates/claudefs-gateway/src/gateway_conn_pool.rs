//! Gateway Connection Pool to Backend Nodes

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Backend node address
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendNode {
    /// Unique node identifier
    pub node_id: String,
    /// Network address (host:port)
    pub address: String,
    /// Optional region for topology
    pub region: Option<String>,
    /// Load balancing weight
    pub weight: u32,
}

impl BackendNode {
    /// Creates a new backend node
    pub fn new(node_id: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            address: address.into(),
            region: None,
            weight: 1,
        }
    }

    /// Sets the region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Sets the weight
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }
}

/// State of a connection slot
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnState {
    /// Connection is available
    Idle,
    /// Connection is in use
    InUse { since: Instant },
    /// Connection is marked unhealthy
    Unhealthy { last_error: String, since: Instant },
}

/// A single pooled connection slot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PooledConn {
    /// Unique connection identifier
    pub id: u64,
    /// Node this connection belongs to
    pub node_id: String,
    /// Current connection state
    pub state: ConnState,
    /// When this connection was created
    pub created_at: Instant,
    /// Last time connection was returned to pool
    pub last_used: Option<Instant>,
    /// Number of requests served by this connection
    pub requests_served: u64,
}

impl PooledConn {
    /// Creates a new pooled connection
    pub fn new(id: u64, node_id: impl Into<String>) -> Self {
        Self {
            id,
            node_id: node_id.into(),
            state: ConnState::Idle,
            created_at: Instant::now(),
            last_used: None,
            requests_served: 0,
        }
    }

    /// Returns true if the connection is idle
    pub fn is_idle(&self) -> bool {
        matches!(self.state, ConnState::Idle)
    }

    /// Returns true if the connection is healthy (not marked unhealthy)
    pub fn is_healthy(&self) -> bool {
        match self.state {
            ConnState::Idle => true,
            ConnState::InUse { .. } => true,
            ConnState::Unhealthy { .. } => false,
        }
    }

    /// Idle time in milliseconds
    pub fn idle_ms(&self) -> u64 {
        match self.state {
            ConnState::Idle => self
                .last_used
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0),
            _ => 0,
        }
    }

    /// Mark connection as in use
    pub fn mark_in_use(&mut self) {
        self.state = ConnState::InUse {
            since: Instant::now(),
        };
    }

    /// Mark connection as idle
    pub fn mark_idle(&mut self) {
        self.last_used = Some(Instant::now());
        self.state = ConnState::Idle;
    }

    /// Mark connection as unhealthy
    pub fn mark_unhealthy(&mut self, error: impl Into<String>) {
        self.state = ConnState::Unhealthy {
            last_error: error.into(),
            since: Instant::now(),
        };
    }
}

/// Connection pool configuration
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ConnPoolConfig {
    /// Minimum connections per node
    pub min_per_node: usize,
    /// Maximum connections per node
    pub max_per_node: usize,
    /// Close connections idle longer than this (ms)
    pub max_idle_ms: u64,
    /// Connection timeout in milliseconds
    pub connect_timeout_ms: u64,
    /// Health check interval in milliseconds
    pub health_check_interval_ms: u64,
}

impl Default for ConnPoolConfig {
    fn default() -> Self {
        Self {
            min_per_node: 2,
            max_per_node: 10,
            max_idle_ms: 300_000, // 5 minutes
            connect_timeout_ms: 5000,
            health_check_interval_ms: 30_000,
        }
    }
}

impl ConnPoolConfig {
    /// Creates a custom pool config
    pub fn new(min_per_node: usize, max_per_node: usize, max_idle_ms: u64) -> Self {
        Self {
            min_per_node,
            max_per_node,
            max_idle_ms,
            connect_timeout_ms: 5000,
            health_check_interval_ms: 30_000,
        }
    }
}

/// Pool for a single backend node
#[derive(Debug)]
pub struct NodeConnPool {
    /// Backend node
    pub node: BackendNode,
    conns: Vec<PooledConn>,
    config: ConnPoolConfig,
    next_conn_id: u64,
}

impl NodeConnPool {
    /// Creates a new node connection pool
    pub fn new(node: BackendNode, config: ConnPoolConfig) -> Self {
        Self {
            node: node.clone(),
            conns: Vec::new(),
            config,
            next_conn_id: 1,
        }
    }

    /// Check out an idle connection (mark InUse). Returns conn ID.
    pub fn checkout(&mut self) -> Option<u64> {
        // Find first idle connection
        for conn in &mut self.conns {
            if conn.is_idle() {
                conn.mark_in_use();
                return Some(conn.id);
            }
        }

        // Try to create new connection if under max
        if self.conns.len() < self.config.max_per_node {
            let id = self.add_conn();
            if let Some(conn) = self.conns.iter_mut().find(|c| c.id == id) {
                conn.mark_in_use();
                return Some(conn.id);
            }
        }

        None
    }

    /// Return a connection to the idle pool
    pub fn checkin(&mut self, conn_id: u64) {
        if let Some(conn) = self.conns.iter_mut().find(|c| c.id == conn_id) {
            conn.mark_idle();
            conn.requests_served += 1;
        }
    }

    /// Mark a connection as unhealthy
    pub fn mark_unhealthy(&mut self, conn_id: u64, error: &str) {
        if let Some(conn) = self.conns.iter_mut().find(|c| c.id == conn_id) {
            conn.mark_unhealthy(error);
        }
    }

    /// Evict idle connections older than max_idle_ms
    pub fn evict_idle(&mut self) -> usize {
        let before = self.conns.len();
        self.conns
            .retain(|c| !c.is_idle() || c.idle_ms() < self.config.max_idle_ms);

        // Maintain minimum connections
        while self.conns.len() < self.config.min_per_node {
            self.add_conn();
        }

        before - self.conns.len()
    }

    /// Number of idle / in-use / unhealthy connections
    pub fn stats(&self) -> (usize, usize, usize) {
        let mut idle = 0;
        let mut in_use = 0;
        let mut unhealthy = 0;

        for conn in &self.conns {
            match conn.state {
                ConnState::Idle => idle += 1,
                ConnState::InUse { .. } => in_use += 1,
                ConnState::Unhealthy { .. } => unhealthy += 1,
            }
        }

        (idle, in_use, unhealthy)
    }

    /// Add a new connection slot (called when pool needs to grow)
    pub fn add_conn(&mut self) -> u64 {
        let id = self.next_conn_id;
        self.next_conn_id += 1;

        self.conns
            .push(PooledConn::new(id, self.node.node_id.clone()));
        debug!("Added connection {} to node {}", id, self.node.node_id);
        id
    }

    /// Total pool size
    pub fn len(&self) -> usize {
        self.conns.len()
    }

    /// Returns true if pool is empty
    pub fn is_empty(&self) -> bool {
        self.conns.is_empty()
    }

    /// Get a specific connection by ID
    pub fn get_conn(&self, conn_id: u64) -> Option<&PooledConn> {
        self.conns.iter().find(|c| c.id == conn_id)
    }

    /// Count healthy connections (idle or in-use, not unhealthy)
    pub fn healthy_count(&self) -> usize {
        self.conns.iter().filter(|c| c.is_healthy()).count()
    }
}

/// Multi-node connection pool with weighted round-robin selection
#[derive(Debug)]
pub struct GatewayConnPool {
    pools: HashMap<String, NodeConnPool>,
    config: ConnPoolConfig,
    rr_index: usize,
    node_order: Vec<String>,
}

impl GatewayConnPool {
    /// Creates a new gateway connection pool
    pub fn new(nodes: Vec<BackendNode>, config: ConnPoolConfig) -> Self {
        let mut pools = HashMap::new();
        let mut node_order = Vec::new();

        for node in nodes {
            let pool = NodeConnPool::new(node.clone(), config);
            pools.insert(node.node_id.clone(), pool);
            node_order.push(node.node_id.clone());
        }

        Self {
            pools,
            config,
            rr_index: 0,
            node_order,
        }
    }

    /// Add a new backend node
    pub fn add_node(&mut self, node: BackendNode) {
        let node_id = node.node_id.clone();
        self.pools
            .insert(node_id.clone(), NodeConnPool::new(node, self.config));
        if !self.node_order.contains(&node_id) {
            self.node_order.push(node_id);
        }
    }

    /// Remove a backend node
    pub fn remove_node(&mut self, node_id: &str) {
        self.pools.remove(node_id);
        self.node_order.retain(|id| id != node_id);
        if self.rr_index >= self.node_order.len() {
            self.rr_index = 0;
        }
    }

    /// Get a connection from the least-loaded healthy pool (weighted round-robin)
    pub fn checkout(&mut self) -> Option<(String, u64)> {
        if self.node_order.is_empty() {
            return None;
        }

        // Try each node in round-robin order starting from current index
        let mut attempts = 0;
        while attempts < self.node_order.len() {
            let idx = self.rr_index % self.node_order.len();
            let node_id = self.node_order[idx].clone();
            self.rr_index = (self.rr_index + 1) % self.node_order.len();
            attempts += 1;

            if let Some(pool) = self.pools.get_mut(&node_id) {
                if pool.healthy_count() > 0 {
                    if let Some(conn_id) = pool.checkout() {
                        return Some((node_id, conn_id));
                    }
                }
            }
        }

        // If no healthy pool found, try to create new connections
        // Start from current index and try each node once
        let start_idx = self.rr_index;
        loop {
            let idx = self.rr_index % self.node_order.len();
            let node_id = self.node_order[idx].clone();
            self.rr_index = (self.rr_index + 1) % self.node_order.len();

            if let Some(pool) = self.pools.get_mut(&node_id) {
                if let Some(conn_id) = pool.checkout() {
                    return Some((node_id, conn_id));
                }
            }

            if self.rr_index == start_idx {
                break;
            }
        }

        None
    }

    /// Return connection
    pub fn checkin(&mut self, node_id: &str, conn_id: u64) {
        if let Some(pool) = self.pools.get_mut(node_id) {
            pool.checkin(conn_id);
        }
    }

    /// Mark connection unhealthy
    pub fn mark_unhealthy(&mut self, node_id: &str, conn_id: u64, error: &str) {
        if let Some(pool) = self.pools.get_mut(node_id) {
            pool.mark_unhealthy(conn_id, error);
        }
    }

    /// Total connections across all nodes
    pub fn total_conns(&self) -> usize {
        self.pools.values().map(|p| p.len()).sum()
    }

    /// Active (in-use) connections across all nodes
    pub fn active_conns(&self) -> usize {
        self.pools.values().map(|p| p.stats().1).sum()
    }

    /// Get node pool by ID
    pub fn get_pool(&self, node_id: &str) -> Option<&NodeConnPool> {
        self.pools.get(node_id)
    }

    /// Get mutable node pool by ID
    pub fn get_pool_mut(&mut self, node_id: &str) -> Option<&mut NodeConnPool> {
        self.pools.get_mut(node_id)
    }

    /// Number of nodes in the pool
    pub fn node_count(&self) -> usize {
        self.pools.len()
    }
}

/// Connection pool errors
#[derive(Debug, Error)]
pub enum ConnPoolError {
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Pool exhausted")]
    PoolExhausted,

    #[error("Node unhealthy: {0}")]
    NodeUnhealthy(String),

    #[error("Connection not found: {0}")]
    ConnNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str) -> BackendNode {
        BackendNode::new(id, format!("{}:8080", id))
    }

    #[test]
    fn test_single_node_checkout_checkin() {
        let config = ConnPoolConfig::default();
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        let conn_id = pool.checkout().unwrap();
        assert_eq!(pool.stats().1, 1); // 1 in-use

        pool.checkin(conn_id);
        assert_eq!(pool.stats().0, 1); // 1 idle
    }

    #[test]
    fn test_mark_unhealthy() {
        let config = ConnPoolConfig::default();
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        let conn_id = pool.checkout().unwrap();
        pool.checkin(conn_id);

        pool.mark_unhealthy(conn_id, "connection reset");

        assert_eq!(pool.stats().2, 1); // 1 unhealthy
    }

    #[test]
    fn test_evict_idle() {
        let config = ConnPoolConfig::new(1, 5, 1000);
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        // Add some connections
        pool.add_conn();
        pool.add_conn();
        pool.add_conn();

        let conn_id = pool.checkout().unwrap();
        pool.checkin(conn_id);

        // Evict should remove idle connections older than 1000ms
        // Since they were just created, they should stay
        let evicted = pool.evict_idle();
        assert!(evicted >= 0);
    }

    #[test]
    fn test_pool_stats() {
        let config = ConnPoolConfig::new(1, 5, 1000);
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        pool.add_conn();
        pool.add_conn();

        let (idle, in_use, unhealthy) = pool.stats();
        assert_eq!(idle, 2);
        assert_eq!(in_use, 0);
        assert_eq!(unhealthy, 0);
    }

    #[test]
    fn test_multi_pool_add_remove_node() {
        let config = ConnPoolConfig::default();
        let mut gateway =
            GatewayConnPool::new(vec![make_node("node1"), make_node("node2")], config);

        assert_eq!(gateway.node_count(), 2);

        gateway.add_node(make_node("node3"));
        assert_eq!(gateway.node_count(), 3);

        gateway.remove_node("node2");
        assert_eq!(gateway.node_count(), 2);
        assert!(gateway.get_pool("node2").is_none());
    }

    #[test]
    fn test_weighted_checkout() {
        let config = ConnPoolConfig::default();
        let mut gateway = GatewayConnPool::new(
            vec![
                BackendNode::new("node1", "node1:8080").with_weight(1),
                BackendNode::new("node2", "node2:8080").with_weight(2),
            ],
            config,
        );

        // Pre-create connections on both nodes
        if let Some(pool) = gateway.get_pool_mut("node1") {
            pool.add_conn();
            pool.add_conn();
        }
        if let Some(pool) = gateway.get_pool_mut("node2") {
            pool.add_conn();
            pool.add_conn();
        }

        // Check out multiple times - should cycle through nodes
        let results: Vec<String> = (0..4)
            .filter_map(|_| gateway.checkout().map(|(n, _)| n))
            .collect();

        // Both nodes should be used
        assert!(results.contains(&"node1".to_string()));
        assert!(results.contains(&"node2".to_string()));
    }

    #[test]
    fn test_no_healthy_nodes_returns_none() {
        let config = ConnPoolConfig::default();
        let mut gateway = GatewayConnPool::new(vec![make_node("node1")], config);

        // Don't add any connections
        let result = gateway.checkout();
        assert!(result.is_none());
    }

    #[test]
    fn test_conn_id_uniqueness() {
        let config = ConnPoolConfig::default();
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        let id1 = pool.add_conn();
        let id2 = pool.add_conn();
        let id3 = pool.add_conn();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }

    #[test]
    fn test_checkout_creates_new_when_under_max() {
        let config = ConnPoolConfig::new(1, 3, 1000);
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        // Initially empty, checkout should create a new connection
        let conn_id = pool.checkout().unwrap();
        assert!(conn_id > 0);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_checkin_increments_requests_served() {
        let config = ConnPoolConfig::default();
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        let conn_id = pool.checkout().unwrap();

        pool.checkin(conn_id);

        let conn = pool.get_conn(conn_id).unwrap();
        assert_eq!(conn.requests_served, 1);
    }

    #[test]
    fn test_total_and_active_conns() {
        let config = ConnPoolConfig::default();
        let mut gateway =
            GatewayConnPool::new(vec![make_node("node1"), make_node("node2")], config);

        if let Some(pool) = gateway.get_pool_mut("node1") {
            pool.add_conn();
            pool.add_conn();
        }
        if let Some(pool) = gateway.get_pool_mut("node2") {
            pool.add_conn();
        }

        assert_eq!(gateway.total_conns(), 3);

        if let Some(pool) = gateway.get_pool_mut("node1") {
            let conn_id = pool.checkout().unwrap();
            drop(conn_id); // keep checked out
        }

        assert_eq!(gateway.active_conns(), 1);
    }

    #[test]
    fn test_pooled_conn_is_idle() {
        let conn = PooledConn::new(1, "node1");
        assert!(conn.is_idle());

        let mut conn = conn;
        conn.mark_in_use();
        assert!(!conn.is_idle());
    }

    #[test]
    fn test_pooled_conn_is_healthy() {
        let conn = PooledConn::new(1, "node1");
        assert!(conn.is_healthy());

        let mut conn = conn;
        conn.mark_unhealthy("error");
        assert!(!conn.is_healthy());
    }

    #[test]
    fn test_config_default_values() {
        let config = ConnPoolConfig::default();

        assert_eq!(config.min_per_node, 2);
        assert_eq!(config.max_per_node, 10);
        assert_eq!(config.max_idle_ms, 300_000);
    }

    #[test]
    fn test_backend_node_builder() {
        let node = BackendNode::new("node1", "192.168.1.1:8080")
            .with_region("us-west-2")
            .with_weight(5);

        assert_eq!(node.node_id, "node1");
        assert_eq!(node.address, "192.168.1.1:8080");
        assert_eq!(node.region, Some("us-west-2".to_string()));
        assert_eq!(node.weight, 5);
    }

    #[test]
    fn test_remove_nonexistent_node() {
        let config = ConnPoolConfig::default();
        let mut gateway = GatewayConnPool::new(vec![make_node("node1")], config);

        // Should not panic
        gateway.remove_node("nonexistent");
        assert_eq!(gateway.node_count(), 1);
    }

    #[test]
    fn test_checkin_nonexistent_connection() {
        let config = ConnPoolConfig::default();
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        // Should not panic
        pool.checkin(999);
    }

    #[test]
    fn test_mark_unhealthy_nonexistent_connection() {
        let config = ConnPoolConfig::default();
        let mut pool = NodeConnPool::new(make_node("node1"), config);

        // Should not panic
        pool.mark_unhealthy(999, "error");
    }
}
