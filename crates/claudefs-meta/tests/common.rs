//! Common test utilities and fixtures for integration tests.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Test configuration with sensible defaults for fast testing
pub struct TestConfig {
    pub election_timeout_ms: u64,
    pub heartbeat_interval_ms: u64,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            election_timeout_ms: 50,    // Fast election for quick testing
            heartbeat_interval_ms: 10,  // Frequent heartbeats
        }
    }
}

/// Represents a single node in an in-process test cluster
pub struct TestNode {
    node_id: u64,
    state: Arc<RwLock<NodeState>>,
}

#[derive(Clone, Debug)]
enum NodeState {
    Running,
    Stopped,
    Partitioned,
}

impl TestNode {
    pub fn new(node_id: u64) -> Self {
        Self {
            node_id,
            state: Arc::new(RwLock::new(NodeState::Running)),
        }
    }

    pub fn id(&self) -> u64 {
        self.node_id
    }

    pub async fn is_alive(&self) -> bool {
        matches!(*self.state.read().await, NodeState::Running)
    }
}

/// Represents an in-process cluster for testing
pub struct TestCluster {
    nodes: Vec<TestNode>,
}

impl TestCluster {
    /// Create a new test cluster with the given number of nodes
    pub fn new(num_nodes: usize) -> Self {
        let nodes = (0..num_nodes)
            .map(|i| TestNode::new(i as u64))
            .collect();

        Self { nodes }
    }

    /// Get a node by ID
    pub fn node(&self, id: usize) -> Option<&TestNode> {
        self.nodes.get(id)
    }

    /// Get all alive nodes
    pub async fn alive_nodes(&self) -> Vec<u64> {
        let mut alive = Vec::new();
        for node in &self.nodes {
            if node.is_alive().await {
                alive.push(node.id());
            }
        }
        alive
    }

    /// Partition a set of nodes from the rest
    pub async fn partition(&self, isolated: Vec<usize>) {
        for node in &self.nodes {
            if isolated.contains(&(node.id() as usize)) {
                *node.state.write().await = NodeState::Partitioned;
            }
        }
    }

    /// Heal a partition
    pub async fn heal_partition(&self) {
        for node in &self.nodes {
            *node.state.write().await = NodeState::Running;
        }
    }

    /// Stop a node
    pub async fn stop_node(&self, id: usize) {
        if let Some(node) = self.node(id) {
            *node.state.write().await = NodeState::Stopped;
        }
    }

    /// Restart a node
    pub async fn start_node(&self, id: usize) {
        if let Some(node) = self.node(id) {
            *node.state.write().await = NodeState::Running;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cluster_creation() {
        let cluster = TestCluster::new(3);
        assert_eq!(cluster.alive_nodes().await.len(), 3);
    }

    #[tokio::test]
    async fn test_node_partition() {
        let cluster = TestCluster::new(3);
        cluster.partition(vec![0]).await;

        let alive = cluster.alive_nodes().await;
        assert_eq!(alive.len(), 2);
        assert!(!alive.contains(&0));
    }

    #[tokio::test]
    async fn test_partition_healing() {
        let cluster = TestCluster::new(3);
        cluster.partition(vec![0]).await;
        assert_eq!(cluster.alive_nodes().await.len(), 2);

        cluster.heal_partition().await;
        assert_eq!(cluster.alive_nodes().await.len(), 3);
    }

    #[tokio::test]
    async fn test_stop_and_restart() {
        let cluster = TestCluster::new(3);
        cluster.stop_node(1).await;
        assert_eq!(cluster.alive_nodes().await.len(), 2);

        cluster.start_node(1).await;
        assert_eq!(cluster.alive_nodes().await.len(), 3);
    }
}
