use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootstrapState {
    Uninitialized,
    InProgress,
    Complete,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSpec {
    pub node_id: String,
    pub address: String,
    pub role: String,
    pub capacity_gb: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapConfig {
    pub cluster_name: String,
    pub site_id: String,
    pub nodes: Vec<NodeSpec>,
    pub erasure_k: u8,
    pub erasure_m: u8,
}

#[derive(Error, Debug)]
pub enum BootstrapError {
    #[error("Invalid config: {0}")]
    InvalidConfig(String),
    #[error("Wrong state: {0}")]
    WrongState(String),
    #[error("Node already registered: {0}")]
    NodeAlreadyRegistered(String),
}

pub struct BootstrapManager {
    config: BootstrapConfig,
    state: Arc<Mutex<BootstrapState>>,
    joined_nodes: Arc<Mutex<Vec<String>>>,
}

impl BootstrapManager {
    pub fn new(config: BootstrapConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(BootstrapState::Uninitialized)),
            joined_nodes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start(&self) -> Result<(), BootstrapError> {
        let mut state = self.state.lock().unwrap();

        if !matches!(*state, BootstrapState::Uninitialized) {
            return Err(BootstrapError::WrongState(
                "bootstrap already started".to_string(),
            ));
        }

        if self.config.cluster_name.is_empty() {
            return Err(BootstrapError::InvalidConfig(
                "cluster_name cannot be empty".to_string(),
            ));
        }

        if self.config.nodes.is_empty() {
            return Err(BootstrapError::InvalidConfig(
                "nodes cannot be empty".to_string(),
            ));
        }

        if self.config.erasure_k < 2 {
            return Err(BootstrapError::InvalidConfig(
                "erasure_k must be >= 2".to_string(),
            ));
        }

        if self.config.erasure_m < 1 {
            return Err(BootstrapError::InvalidConfig(
                "erasure_m must be >= 1".to_string(),
            ));
        }

        *state = BootstrapState::InProgress;
        Ok(())
    }

    pub fn register_node(&self, node_id: &str) -> Result<(), BootstrapError> {
        let state = self.state.lock().unwrap();

        if !matches!(*state, BootstrapState::InProgress) {
            return Err(BootstrapError::WrongState(
                "bootstrap not in progress".to_string(),
            ));
        }

        drop(state);

        let mut joined = self.joined_nodes.lock().unwrap();
        if joined.contains(&node_id.to_string()) {
            return Err(BootstrapError::NodeAlreadyRegistered(node_id.to_string()));
        }
        joined.push(node_id.to_string());

        Ok(())
    }

    pub fn complete(&self) -> Result<(), BootstrapError> {
        let mut state = self.state.lock().unwrap();

        if !matches!(*state, BootstrapState::InProgress) {
            return Err(BootstrapError::WrongState(
                "bootstrap not in progress".to_string(),
            ));
        }

        *state = BootstrapState::Complete;
        Ok(())
    }

    pub fn fail(&self, reason: &str) -> Result<(), BootstrapError> {
        let mut state = self.state.lock().unwrap();
        *state = BootstrapState::Failed(reason.to_string());
        Ok(())
    }

    pub fn state(&self) -> BootstrapState {
        self.state.lock().unwrap().clone()
    }

    pub fn joined_count(&self) -> usize {
        self.joined_nodes.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> BootstrapConfig {
        BootstrapConfig {
            cluster_name: "test-cluster".to_string(),
            site_id: "site1".to_string(),
            nodes: vec![NodeSpec {
                node_id: "n1".to_string(),
                address: "192.168.1.1".to_string(),
                role: "storage".to_string(),
                capacity_gb: 1000,
            }],
            erasure_k: 4,
            erasure_m: 2,
        }
    }

    #[test]
    fn new_bootstrap_manager_starts_in_uninitialized_state() {
        let manager = BootstrapManager::new(make_config());
        assert_eq!(manager.state(), BootstrapState::Uninitialized);
    }

    #[test]
    fn start_succeeds_with_valid_config() {
        let manager = BootstrapManager::new(make_config());
        let result = manager.start();
        assert!(result.is_ok());
    }

    #[test]
    fn start_fails_with_empty_cluster_name() {
        let mut config = make_config();
        config.cluster_name = "".to_string();
        let manager = BootstrapManager::new(config);
        let result = manager.start();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BootstrapError::InvalidConfig(_)
        ));
    }

    #[test]
    fn start_fails_with_empty_nodes_list() {
        let mut config = make_config();
        config.nodes = vec![];
        let manager = BootstrapManager::new(config);
        let result = manager.start();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BootstrapError::InvalidConfig(_)
        ));
    }

    #[test]
    fn start_fails_with_erasure_k_less_than_2() {
        let mut config = make_config();
        config.erasure_k = 1;
        let manager = BootstrapManager::new(config);
        let result = manager.start();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BootstrapError::InvalidConfig(_)
        ));
    }

    #[test]
    fn start_fails_with_erasure_m_less_than_1() {
        let mut config = make_config();
        config.erasure_m = 0;
        let manager = BootstrapManager::new(config);
        let result = manager.start();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BootstrapError::InvalidConfig(_)
        ));
    }

    #[test]
    fn start_transitions_state_to_in_progress() {
        let manager = BootstrapManager::new(make_config());
        manager.start().unwrap();
        assert_eq!(manager.state(), BootstrapState::InProgress);
    }

    #[test]
    fn register_node_succeeds_when_in_progress() {
        let manager = BootstrapManager::new(make_config());
        manager.start().unwrap();
        let result = manager.register_node("n1");
        assert!(result.is_ok());
    }

    #[test]
    fn register_node_fails_when_not_in_progress_uninitialized() {
        let manager = BootstrapManager::new(make_config());
        let result = manager.register_node("n1");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BootstrapError::WrongState(_)));
    }

    #[test]
    fn register_node_increments_joined_count() {
        let manager = BootstrapManager::new(make_config());
        manager.start().unwrap();
        assert_eq!(manager.joined_count(), 0);
        manager.register_node("n1").unwrap();
        assert_eq!(manager.joined_count(), 1);
        manager.register_node("n2").unwrap();
        assert_eq!(manager.joined_count(), 2);
    }

    #[test]
    fn duplicate_node_registration_returns_error() {
        let manager = BootstrapManager::new(make_config());
        manager.start().unwrap();
        manager.register_node("n1").unwrap();
        let result = manager.register_node("n1");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BootstrapError::NodeAlreadyRegistered(_)
        ));
    }

    #[test]
    fn complete_transitions_to_complete() {
        let manager = BootstrapManager::new(make_config());
        manager.start().unwrap();
        manager.register_node("n1").unwrap();
        manager.complete().unwrap();
        assert_eq!(manager.state(), BootstrapState::Complete);
    }

    #[test]
    fn complete_fails_when_not_in_progress() {
        let manager = BootstrapManager::new(make_config());
        let result = manager.complete();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BootstrapError::WrongState(_)));
    }

    #[test]
    fn fail_transitions_to_failed() {
        let manager = BootstrapManager::new(make_config());
        manager.start().unwrap();
        manager.fail("test failure").unwrap();
        assert_eq!(
            manager.state(),
            BootstrapState::Failed("test failure".to_string())
        );
    }

    #[test]
    fn fail_captures_reason_string() {
        let manager = BootstrapManager::new(make_config());
        manager.start().unwrap();
        manager.fail("network error").unwrap();
        if let BootstrapState::Failed(reason) = manager.state() {
            assert_eq!(reason, "network error");
        } else {
            panic!("Expected Failed state");
        }
    }

    #[test]
    fn state_returns_current_state() {
        let manager = BootstrapManager::new(make_config());
        assert_eq!(manager.state(), BootstrapState::Uninitialized);
        manager.start().unwrap();
        assert_eq!(manager.state(), BootstrapState::InProgress);
    }

    #[test]
    fn joined_count_starts_at_0() {
        let manager = BootstrapManager::new(make_config());
        assert_eq!(manager.joined_count(), 0);
    }

    #[test]
    fn full_lifecycle_start_register_3_nodes_complete() {
        let mut config = make_config();
        config.nodes = vec![
            NodeSpec {
                node_id: "n1".to_string(),
                address: "192.168.1.1".to_string(),
                role: "storage".to_string(),
                capacity_gb: 1000,
            },
            NodeSpec {
                node_id: "n2".to_string(),
                address: "192.168.1.2".to_string(),
                role: "storage".to_string(),
                capacity_gb: 1000,
            },
            NodeSpec {
                node_id: "n3".to_string(),
                address: "192.168.1.3".to_string(),
                role: "storage".to_string(),
                capacity_gb: 1000,
            },
        ];
        let manager = BootstrapManager::new(config);

        manager.start().unwrap();
        assert_eq!(manager.state(), BootstrapState::InProgress);

        manager.register_node("n1").unwrap();
        manager.register_node("n2").unwrap();
        manager.register_node("n3").unwrap();
        assert_eq!(manager.joined_count(), 3);

        manager.complete().unwrap();
        assert_eq!(manager.state(), BootstrapState::Complete);
    }

    #[test]
    fn fail_after_start_transitions_correctly() {
        let manager = BootstrapManager::new(make_config());
        assert_eq!(manager.state(), BootstrapState::Uninitialized);

        manager.start().unwrap();
        assert_eq!(manager.state(), BootstrapState::InProgress);

        manager.fail("configuration error").unwrap();
        assert_eq!(
            manager.state(),
            BootstrapState::Failed("configuration error".to_string())
        );
    }

    #[test]
    fn cannot_start_twice_already_in_progress() {
        let manager = BootstrapManager::new(make_config());
        manager.start().unwrap();

        let result = manager.start();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BootstrapError::WrongState(_)));
    }
}
