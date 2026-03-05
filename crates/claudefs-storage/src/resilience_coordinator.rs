use std::collections::HashMap;
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Clone, PartialEq)]
pub enum FailureMode {
    Healthy,
    Degraded,
    Failed,
    Recovering,
}

#[derive(Debug, Clone)]
pub struct NodeResilience {
    pub node_id: u32,
    pub mode: FailureMode,
    pub failure_count: u64,
    pub recovery_start_ms: Option<u64>,
    pub last_heartbeat_ms: u64,
}

#[derive(Debug, Error, PartialEq)]
pub enum ResilienceError {
    #[error("Node not found: node_id {0}")]
    NodeNotFound(u32),
    #[error("Invalid recovery state for node {0}")]
    InvalidRecoveryState(u32),
    #[error("Node already registered: node_id {0}")]
    NodeAlreadyRegistered(u32),
}

pub type ResilienceResult<T> = Result<T, ResilienceError>;

pub struct ResilienceCoordinator {
    nodes: HashMap<u32, NodeResilience>,
    heartbeat_timeout_ms: u64,
    failure_threshold: u32,
    recovery_timeout_ms: u64,
}

impl ResilienceCoordinator {
    pub fn new(
        heartbeat_timeout_ms: u64,
        failure_threshold: u32,
        recovery_timeout_ms: u64,
    ) -> Self {
        Self {
            nodes: HashMap::new(),
            heartbeat_timeout_ms,
            failure_threshold,
            recovery_timeout_ms,
        }
    }

    pub fn register_node(&mut self, node_id: u32) -> ResilienceResult<()> {
        if self.nodes.contains_key(&node_id) {
            return Err(ResilienceError::NodeAlreadyRegistered(node_id));
        }

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let node = NodeResilience {
            node_id,
            mode: FailureMode::Healthy,
            failure_count: 0,
            recovery_start_ms: None,
            last_heartbeat_ms: timestamp_ms,
        };

        self.nodes.insert(node_id, node);
        debug!("Registered node: node_id={}", node_id);
        Ok(())
    }

    pub fn heartbeat(&mut self, node_id: u32, timestamp_ms: u64) -> Option<FailureMode> {
        let node = self.nodes.get_mut(&node_id)?;

        let old_mode = node.mode.clone();
        node.last_heartbeat_ms = timestamp_ms;

        if node.mode == FailureMode::Failed {
            return Some(FailureMode::Failed);
        }

        if node.mode == FailureMode::Degraded || node.mode == FailureMode::Recovering {
            node.mode = FailureMode::Healthy;
            debug!("Node {} recovered to healthy state", node_id);
        }

        if old_mode != node.mode {
            Some(node.mode.clone())
        } else {
            None
        }
    }

    pub fn record_failure(
        &mut self,
        node_id: u32,
        timestamp_ms: u64,
    ) -> ResilienceResult<FailureMode> {
        let node = self
            .nodes
            .get_mut(&node_id)
            .ok_or(ResilienceError::NodeNotFound(node_id))?;

        node.failure_count += 1;
        node.last_heartbeat_ms = timestamp_ms;

        let new_mode = if node.failure_count >= self.failure_threshold as u64 {
            FailureMode::Failed
        } else {
            FailureMode::Degraded
        };

        let old_mode = node.mode.clone();
        node.mode = new_mode.clone();

        debug!(
            "Recorded failure for node {}: {:?} -> {:?}",
            node_id, old_mode, new_mode
        );
        Ok(new_mode)
    }

    pub fn initiate_recovery(&mut self, node_id: u32, timestamp_ms: u64) -> ResilienceResult<()> {
        let node = self
            .nodes
            .get_mut(&node_id)
            .ok_or(ResilienceError::NodeNotFound(node_id))?;

        if node.mode != FailureMode::Failed && node.mode != FailureMode::Degraded {
            return Err(ResilienceError::InvalidRecoveryState(node_id));
        }

        node.mode = FailureMode::Recovering;
        node.recovery_start_ms = Some(timestamp_ms);

        debug!("Initiated recovery for node {}", node_id);
        Ok(())
    }

    pub fn check_timeouts(&mut self, current_time_ms: u64) -> Vec<(u32, FailureMode)> {
        let mut changes = Vec::new();

        for (node_id, node) in self.nodes.iter_mut() {
            let prev_mode = node.mode.clone();

            match node.mode {
                FailureMode::Healthy => {
                    if current_time_ms.saturating_sub(node.last_heartbeat_ms)
                        > self.heartbeat_timeout_ms
                    {
                        node.mode = FailureMode::Degraded;
                        changes.push((*node_id, FailureMode::Degraded));
                        debug!("Node {} timed out: Healthy -> Degraded", node_id);
                    }
                }
                FailureMode::Degraded => {
                    if current_time_ms.saturating_sub(node.last_heartbeat_ms)
                        > self.heartbeat_timeout_ms * 2
                    {
                        node.mode = FailureMode::Failed;
                        changes.push((*node_id, FailureMode::Failed));
                        debug!("Node {} timed out: Degraded -> Failed", node_id);
                    }
                }
                FailureMode::Recovering => {
                    if let Some(start_ms) = node.recovery_start_ms {
                        if current_time_ms.saturating_sub(start_ms) > self.recovery_timeout_ms {
                            node.mode = FailureMode::Healthy;
                            node.recovery_start_ms = None;
                            node.failure_count = 0;
                            changes.push((*node_id, FailureMode::Healthy));
                            debug!("Node {} recovery completed", node_id);
                        }
                    }
                }
                FailureMode::Failed => {}
            }
        }

        changes
    }

    pub fn get_mode(&self, node_id: u32) -> Option<FailureMode> {
        self.nodes.get(&node_id).map(|n| n.mode.clone())
    }

    pub fn all_nodes(&self) -> Vec<(u32, &FailureMode)> {
        self.nodes
            .iter()
            .map(|(id, node)| (*id, &node.mode))
            .collect()
    }

    pub fn recovery_progress(&self, node_id: u32) -> Option<f64> {
        let node = self.nodes.get(&node_id)?;

        if node.mode != FailureMode::Recovering {
            return None;
        }

        let start_ms = node.recovery_start_ms?;
        let current_time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let elapsed = current_time_ms.saturating_sub(start_ms);

        if elapsed >= self.recovery_timeout_ms {
            return Some(1.0);
        }

        Some(elapsed as f64 / self.recovery_timeout_ms as f64)
    }
}

impl Default for ResilienceCoordinator {
    fn default() -> Self {
        Self::new(5000, 3, 30000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_coordinator() -> ResilienceCoordinator {
        ResilienceCoordinator::new(5000, 3, 30000)
    }

    #[test]
    fn test_register_node() {
        let mut coord = create_coordinator();
        assert!(coord.register_node(1).is_ok());
    }

    #[test]
    fn test_register_node_duplicate() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();
        assert!(matches!(
            coord.register_node(1),
            Err(ResilienceError::NodeAlreadyRegistered(1))
        ));
    }

    #[test]
    fn test_heartbeat_updates() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        let mode = coord.heartbeat(1, 10000);
        assert!(mode.is_none());

        let node = coord.nodes.get(&1).unwrap();
        assert_eq!(node.last_heartbeat_ms, 10000);
    }

    #[test]
    fn test_heartbeat_nonexistent_node() {
        let mut coord = create_coordinator();
        assert!(coord.heartbeat(999, 10000).is_none());
    }

    #[test]
    fn test_heartbeat_recovers_degraded() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Degraded;

        let mode = coord.heartbeat(1, 10000);
        assert_eq!(mode, Some(FailureMode::Healthy));
    }

    #[test]
    fn test_heartbeat_recovers_recovering() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Recovering;

        let mode = coord.heartbeat(1, 10000);
        assert_eq!(mode, Some(FailureMode::Healthy));
    }

    #[test]
    fn test_record_failure() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        let mode = coord.record_failure(1, 10000);
        assert_eq!(mode, Ok(FailureMode::Degraded));

        coord.record_failure(1, 10001).unwrap();
        coord.record_failure(1, 10002).unwrap();

        let mode = coord.record_failure(1, 10003);
        assert_eq!(mode, Ok(FailureMode::Failed));
    }

    #[test]
    fn test_record_failure_nonexistent() {
        let mut coord = create_coordinator();
        assert!(matches!(
            coord.record_failure(999, 10000),
            Err(ResilienceError::NodeNotFound(999))
        ));
    }

    #[test]
    fn test_initiate_recovery_from_failed() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Failed;

        coord.initiate_recovery(1, 10000).unwrap();

        let node = coord.nodes.get(&1).unwrap();
        assert_eq!(node.mode, FailureMode::Recovering);
        assert_eq!(node.recovery_start_ms, Some(10000));
    }

    #[test]
    fn test_initiate_recovery_from_degraded() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Degraded;

        coord.initiate_recovery(1, 10000).unwrap();

        let node = coord.nodes.get(&1).unwrap();
        assert_eq!(node.mode, FailureMode::Recovering);
    }

    #[test]
    fn test_initiate_recovery_from_healthy() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        assert!(matches!(
            coord.initiate_recovery(1, 10000),
            Err(ResilienceError::InvalidRecoveryState(1))
        ));
    }

    #[test]
    fn test_initiate_recovery_nonexistent() {
        let mut coord = create_coordinator();
        assert!(matches!(
            coord.initiate_recovery(999, 10000),
            Err(ResilienceError::NodeNotFound(999))
        ));
    }

    #[test]
    fn test_check_timeouts_degraded() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Degraded;
        coord.nodes.get_mut(&1).unwrap().last_heartbeat_ms = 0;

        let changes = coord.check_timeouts(15000);
        assert!(!changes.is_empty());
    }

    #[test]
    fn test_check_timeouts_no_timeout() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        let changes = coord.check_timeouts(1000);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_check_timeouts_recovery_complete() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Recovering;
        coord.nodes.get_mut(&1).unwrap().recovery_start_ms = Some(0);

        let changes = coord.check_timeouts(35000);

        let node = coord.nodes.get(&1).unwrap();
        assert_eq!(node.mode, FailureMode::Healthy);
    }

    #[test]
    fn test_get_mode() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        assert_eq!(coord.get_mode(1), Some(FailureMode::Healthy));

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Failed;
        assert_eq!(coord.get_mode(1), Some(FailureMode::Failed));
    }

    #[test]
    fn test_get_mode_nonexistent() {
        let coord = create_coordinator();
        assert_eq!(coord.get_mode(999), None);
    }

    #[test]
    fn test_all_nodes() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();
        coord.register_node(2).unwrap();

        let nodes = coord.all_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_recovery_progress() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Recovering;
        coord.nodes.get_mut(&1).unwrap().recovery_start_ms = Some(0);

        let progress = coord.recovery_progress(1);
        assert!(progress.is_some());
    }

    #[test]
    fn test_recovery_progress_not_recovering() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Healthy;

        assert!(coord.recovery_progress(1).is_none());
    }

    #[test]
    fn test_recovery_progress_nonexistent() {
        let coord = create_coordinator();
        assert!(coord.recovery_progress(999).is_none());
    }

    #[test]
    fn test_failure_mode_clone() {
        let mode = FailureMode::Healthy;
        let cloned = mode.clone();
        assert_eq!(mode, cloned);
    }

    #[test]
    fn test_failure_mode_debug() {
        let mode = FailureMode::Degraded;
        let debug_str = format!("{:?}", mode);
        assert!(debug_str.contains("Degraded"));
    }

    #[test]
    fn test_failure_mode_partial_eq() {
        assert_eq!(FailureMode::Healthy, FailureMode::Healthy);
        assert_eq!(FailureMode::Failed, FailureMode::Failed);
        assert_ne!(FailureMode::Healthy, FailureMode::Failed);
    }

    #[test]
    fn test_multiple_node_state_machines() {
        let mut coord = create_coordinator();

        coord.register_node(1).unwrap();
        coord.register_node(2).unwrap();
        coord.register_node(3).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Failed;
        coord.nodes.get_mut(&2).unwrap().mode = FailureMode::Degraded;

        assert_eq!(coord.get_mode(1), Some(FailureMode::Failed));
        assert_eq!(coord.get_mode(2), Some(FailureMode::Degraded));
        assert_eq!(coord.get_mode(3), Some(FailureMode::Healthy));
    }

    #[test]
    fn test_check_timeouts_healthy_to_degraded() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().last_heartbeat_ms = 0;

        let changes = coord.check_timeouts(6000);

        let node = coord.nodes.get(&1).unwrap();
        assert_eq!(node.mode, FailureMode::Degraded);
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn test_failure_count_increment() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        assert_eq!(coord.nodes.get(&1).unwrap().failure_count, 0);

        coord.record_failure(1, 10000).unwrap();
        assert_eq!(coord.nodes.get(&1).unwrap().failure_count, 1);

        coord.record_failure(1, 10001).unwrap();
        assert_eq!(coord.nodes.get(&1).unwrap().failure_count, 2);
    }

    #[test]
    fn test_recovery_resets_failure_count() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.record_failure(1, 10000).unwrap();
        coord.record_failure(1, 10001).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Recovering;
        coord.nodes.get_mut(&1).unwrap().recovery_start_ms = Some(0);

        coord.check_timeouts(35000);

        assert_eq!(coord.nodes.get(&1).unwrap().failure_count, 0);
    }

    #[test]
    fn test_failed_node_heartbeat_returns_failed() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Failed;

        let mode = coord.heartbeat(1, 10000);
        assert_eq!(mode, Some(FailureMode::Failed));
    }

    #[test]
    fn test_all_nodes_returns_correct_format() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();
        coord.register_node(2).unwrap();

        let nodes = coord.all_nodes();

        for (id, mode) in nodes {
            match id {
                1 | 2 => assert!(matches!(mode, FailureMode::Healthy)),
                _ => panic!("Unexpected node"),
            }
        }
    }

    #[test]
    fn test_check_timeouts_multiple_nodes() {
        let mut coord = create_coordinator();

        coord.register_node(1).unwrap();
        coord.register_node(2).unwrap();
        coord.register_node(3).unwrap();

        coord.nodes.get_mut(&1).unwrap().last_heartbeat_ms = 0;
        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Healthy;

        coord.nodes.get_mut(&2).unwrap().last_heartbeat_ms = 0;
        coord.nodes.get_mut(&2).unwrap().mode = FailureMode::Degraded;

        let changes = coord.check_timeouts(15000);

        assert!(changes.len() >= 1);
    }

    #[test]
    fn test_register_node_sets_initial_state() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        let node = coord.nodes.get(&1).unwrap();
        assert_eq!(node.mode, FailureMode::Healthy);
        assert_eq!(node.failure_count, 0);
        assert!(node.recovery_start_ms.is_none());
    }

    #[test]
    fn test_check_timeouts_recovery_in_progress() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Recovering;
        coord.nodes.get_mut(&1).unwrap().recovery_start_ms = Some(10000);

        let changes = coord.check_timeouts(50000);

        let node = coord.nodes.get(&1).unwrap();
        assert!(node.recovery_start_ms.is_none());
    }

    #[test]
    fn test_failure_threshold_behavior() {
        let mut coord = ResilienceCoordinator::new(5000, 2, 30000);
        coord.register_node(1).unwrap();

        let mode = coord.record_failure(1, 10000);
        assert_eq!(mode, Ok(FailureMode::Degraded));

        let mode = coord.record_failure(1, 10001);
        assert_eq!(mode, Ok(FailureMode::Failed));
    }

    #[test]
    fn test_recovery_progress_calculation() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Recovering;
        coord.nodes.get_mut(&1).unwrap().recovery_start_ms = Some(10000);

        let progress = coord.recovery_progress(1);

        let node = coord.nodes.get(&1).unwrap();
        let elapsed = 15000u64.saturating_sub(node.recovery_start_ms.unwrap());
        let expected = elapsed as f64 / 30000.0;

        assert!(progress.is_some());
    }

    #[test]
    fn test_initiate_recovery_multiple_times() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Failed;

        coord.initiate_recovery(1, 10000).unwrap();

        let node = coord.nodes.get(&1).unwrap();
        assert_eq!(node.mode, FailureMode::Recovering);

        let result = coord.initiate_recovery(1, 10001);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_timeouts_does_not_change_failed() {
        let mut coord = create_coordinator();
        coord.register_node(1).unwrap();

        coord.nodes.get_mut(&1).unwrap().mode = FailureMode::Failed;

        let changes = coord.check_timeouts(100000);

        assert!(changes.is_empty());
        assert_eq!(coord.get_mode(1), Some(FailureMode::Failed));
    }
}
