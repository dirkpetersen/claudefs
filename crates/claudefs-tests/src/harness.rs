//! Test Harness - General-purpose test environment setup

use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[derive(Debug)]
pub struct TestEnv {
    temp_dir: TempDir,
    test_name: String,
}

impl TestEnv {
    pub fn new(test_name: &str) -> Self {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        Self {
            temp_dir,
            test_name: test_name.to_string(),
        }
    }

    pub fn tempdir(&self) -> &Path {
        self.temp_dir.path()
    }

    pub fn test_name(&self) -> &str {
        &self.test_name
    }
}

#[derive(Debug, Clone)]
pub struct TestCluster {
    nodes: Vec<NodeConfig>,
}

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub node_id: u32,
    pub address: String,
    pub port: u16,
}

impl TestCluster {
    pub fn single_node() -> Self {
        Self {
            nodes: vec![NodeConfig {
                node_id: 0,
                address: "127.0.0.1".to_string(),
                port: 7000,
            }],
        }
    }

    pub fn three_node() -> Self {
        Self {
            nodes: vec![
                NodeConfig {
                    node_id: 0,
                    address: "127.0.0.1".to_string(),
                    port: 7000,
                },
                NodeConfig {
                    node_id: 1,
                    address: "127.0.0.1".to_string(),
                    port: 7001,
                },
                NodeConfig {
                    node_id: 2,
                    address: "127.0.0.1".to_string(),
                    port: 7002,
                },
            ],
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn node_ids(&self) -> Vec<u32> {
        self.nodes.iter().map(|n| n.node_id).collect()
    }

    pub fn node(&self, id: u32) -> Option<&NodeConfig> {
        self.nodes.iter().find(|n| n.node_id == id)
    }

    pub fn addresses(&self) -> Vec<String> {
        self.nodes
            .iter()
            .map(|n| format!("{}:{}", n.address, n.port))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_creation() {
        let env = TestEnv::new("test_creation");
        assert!(env.tempdir().exists());
        assert_eq!(env.test_name(), "test_creation");
    }

    #[test]
    fn test_env_tempdir_unique() {
        let env1 = TestEnv::new("test1");
        let env2 = TestEnv::new("test2");
        assert_ne!(env1.tempdir(), env2.tempdir());
    }

    #[test]
    fn test_cluster_single_node() {
        let cluster = TestCluster::single_node();
        assert_eq!(cluster.node_count(), 1);
        assert_eq!(cluster.node_ids(), vec![0]);
    }

    #[test]
    fn test_cluster_three_node() {
        let cluster = TestCluster::three_node();
        assert_eq!(cluster.node_count(), 3);
        assert_eq!(cluster.node_ids(), vec![0, 1, 2]);
    }

    #[test]
    fn test_cluster_node_access() {
        let cluster = TestCluster::three_node();
        let node = cluster.node(1).expect("node 1 should exist");
        assert_eq!(node.port, 7001);
    }

    #[test]
    fn test_cluster_addresses() {
        let cluster = TestCluster::three_node();
        let addrs = cluster.addresses();
        assert_eq!(addrs.len(), 3);
        assert!(addrs.contains(&"127.0.0.1:7000".to_string()));
    }

    #[test]
    fn test_cluster_node_not_found() {
        let cluster = TestCluster::single_node();
        assert!(cluster.node(99).is_none());
    }

    #[test]
    fn test_temp_dir_persists_during_lifetime() {
        let env = TestEnv::new("test_persist");
        let path = env.tempdir().to_path_buf();
        assert!(path.exists());
    }

    #[test]
    fn test_node_config_debug() {
        let config = NodeConfig {
            node_id: 42,
            address: "192.168.1.1".to_string(),
            port: 9000,
        };
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("42"));
        assert!(debug_str.contains("9000"));
    }

    #[test]
    fn test_cluster_debug() {
        let cluster = TestCluster::three_node();
        let debug_str = format!("{:?}", cluster);
        assert!(debug_str.contains("NodeConfig"));
    }

    #[test]
    fn test_env_multiple_instances() {
        let env1 = TestEnv::new("test1");
        let env2 = TestEnv::new("test2");
        let env3 = TestEnv::new("test3");

        assert!(env1.tempdir().exists());
        assert!(env2.tempdir().exists());
        assert!(env3.tempdir().exists());

        assert_ne!(env1.tempdir(), env2.tempdir());
        assert_ne!(env2.tempdir(), env3.tempdir());
    }

    #[test]
    fn test_cluster_five_node() {
        let nodes = (0..5)
            .map(|i| NodeConfig {
                node_id: i as u32,
                address: "127.0.0.1".to_string(),
                port: 7000 + i as u16,
            })
            .collect();
        let cluster = TestCluster { nodes };
        assert_eq!(cluster.node_count(), 5);
    }

    #[test]
    fn test_temp_dir_has_valid_path() {
        let env = TestEnv::new("test_path");
        let path = env.tempdir();
        assert!(path.is_dir());
        assert!(path.to_string_lossy().len() > 0);
    }

    #[test]
    fn test_cluster_clone() {
        let cluster = TestCluster::three_node();
        let cloned = cluster.clone();
        assert_eq!(cloned.node_count(), 3);
    }

    #[test]
    fn test_node_config_clone() {
        let config = NodeConfig {
            node_id: 1,
            address: "localhost".to_string(),
            port: 8000,
        };
        let cloned = config.clone();
        assert_eq!(cloned.node_id, 1);
    }
}
