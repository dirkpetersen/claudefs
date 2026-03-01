use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    Storage,
    Metadata,
    StorageAndMetadata,
    Gateway,
    Client,
}

impl NodeRole {
    pub fn is_storage(&self) -> bool {
        matches!(self, NodeRole::Storage | NodeRole::StorageAndMetadata)
    }

    pub fn is_metadata(&self) -> bool {
        matches!(self, NodeRole::Metadata | NodeRole::StorageAndMetadata)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeState {
    Joining,
    Active,
    Draining,
    Drained,
    Failed,
    Decommissioned,
}

impl NodeState {
    pub fn is_serving(&self) -> bool {
        matches!(self, NodeState::Active)
    }

    pub fn can_transition_to(&self, target: &NodeState) -> bool {
        match (self, target) {
            (NodeState::Joining, NodeState::Active) => true,
            (NodeState::Joining, NodeState::Failed) => true,
            (NodeState::Active, NodeState::Draining) => true,
            (NodeState::Active, NodeState::Failed) => true,
            (NodeState::Draining, NodeState::Drained) => true,
            (NodeState::Draining, NodeState::Failed) => true,
            (NodeState::Drained, NodeState::Decommissioned) => true,
            (NodeState::Failed, NodeState::Decommissioned) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSpec {
    pub node_id: String,
    pub address: String,
    pub role: NodeRole,
    pub nvme_capacity_bytes: u64,
    pub ram_bytes: u64,
    pub cpu_cores: u32,
}

impl NodeSpec {
    pub fn new(
        node_id: impl Into<String>,
        address: impl Into<String>,
        role: NodeRole,
        nvme_capacity_bytes: u64,
        ram_bytes: u64,
        cpu_cores: u32,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            address: address.into(),
            role,
            nvme_capacity_bytes,
            ram_bytes,
            cpu_cores,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNode {
    pub spec: NodeSpec,
    pub state: NodeState,
    pub added_at: u64,
    pub state_changed_at: u64,
    pub data_bytes: u64,
    pub shards: Vec<u32>,
}

impl ClusterNode {
    pub fn new(spec: NodeSpec, now: u64) -> Self {
        Self {
            spec,
            state: NodeState::Joining,
            added_at: now,
            state_changed_at: now,
            data_bytes: 0,
            shards: vec![],
        }
    }

    pub fn transition(&mut self, new_state: NodeState, now: u64) {
        self.state = new_state;
        self.state_changed_at = now;
    }

    pub fn is_serving(&self) -> bool {
        self.state.is_serving()
    }

    pub fn fill_percent(&self) -> f64 {
        if self.spec.nvme_capacity_bytes == 0 {
            return 0.0;
        }
        (self.data_bytes as f64 / self.spec.nvme_capacity_bytes as f64) * 100.0
    }

    pub fn add_shard(&mut self, shard_id: u32) {
        if !self.shards.contains(&shard_id) {
            self.shards.push(shard_id);
        }
    }

    pub fn remove_shard(&mut self, shard_id: u32) {
        self.shards.retain(|&s| s != shard_id);
    }

    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceTask {
    pub task_id: String,
    pub from_node: String,
    pub to_node: String,
    pub shard_id: u32,
    pub bytes_total: u64,
    pub bytes_moved: u64,
    pub started_at: u64,
    pub completed_at: Option<u64>,
}

impl RebalanceTask {
    pub fn new(from: String, to: String, shard_id: u32, bytes_total: u64, now: u64) -> Self {
        Self {
            task_id: format!("{}-{}->{}-{}", shard_id, from, to, now),
            from_node: from,
            to_node: to,
            shard_id,
            bytes_total,
            bytes_moved: 0,
            started_at: now,
            completed_at: None,
        }
    }

    pub fn progress_percent(&self) -> f64 {
        if self.bytes_total == 0 {
            return 100.0;
        }
        (self.bytes_moved as f64 / self.bytes_total as f64) * 100.0
    }

    pub fn is_complete(&self) -> bool {
        self.completed_at.is_some()
    }

    pub fn complete(&mut self, now: u64) {
        self.completed_at = Some(now);
    }

    pub fn update_progress(&mut self, bytes_moved: u64) {
        self.bytes_moved = bytes_moved.min(self.bytes_total);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScalingTrigger {
    NodeAdded(String),
    NodeRemoved(String),
    Manual,
    CapacityThreshold { threshold_percent: f64 },
}

impl ScalingTrigger {
    pub fn description(&self) -> String {
        match self {
            ScalingTrigger::NodeAdded(id) => format!("Node added: {}", id),
            ScalingTrigger::NodeRemoved(id) => format!("Node removed: {}", id),
            ScalingTrigger::Manual => "Manual scaling".to_string(),
            ScalingTrigger::CapacityThreshold { threshold_percent } => {
                format!("Capacity threshold: {}%", threshold_percent)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingPlan {
    pub plan_id: String,
    pub trigger: ScalingTrigger,
    pub tasks: Vec<RebalanceTask>,
    pub created_at: u64,
    pub estimated_bytes: u64,
    pub completed_tasks: usize,
}

impl ScalingPlan {
    pub fn new(
        plan_id: String,
        trigger: ScalingTrigger,
        tasks: Vec<RebalanceTask>,
        now: u64,
    ) -> Self {
        let estimated_bytes = tasks.iter().map(|t| t.bytes_total).sum();
        Self {
            plan_id,
            trigger,
            tasks,
            created_at: now,
            estimated_bytes,
            completed_tasks: 0,
        }
    }

    pub fn total_tasks(&self) -> usize {
        self.tasks.len()
    }

    pub fn progress_percent(&self) -> f64 {
        if self.tasks.is_empty() {
            return 0.0;
        }
        (self.completed_tasks as f64 / self.tasks.len() as f64) * 100.0
    }

    pub fn is_complete(&self) -> bool {
        self.completed_tasks == self.tasks.len()
    }

    pub fn mark_task_complete(&mut self, task_id: &str) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.task_id == task_id) {
            if !task.is_complete() {
                task.completed_at = Some(current_time_ns());
                self.completed_tasks += 1;
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum ScalingError {
    #[error("Node already exists: {0}")]
    NodeAlreadyExists(String),
    #[error("Node not found: {0}")]
    NodeNotFound(String),
    #[error("Invalid transition from {from:?} to {to:?}")]
    InvalidTransition { from: NodeState, to: NodeState },
}

pub struct NodeScalingManager {
    nodes: HashMap<String, ClusterNode>,
    plans: HashMap<String, ScalingPlan>,
}

impl NodeScalingManager {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            plans: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, spec: NodeSpec, now: u64) -> Result<(), ScalingError> {
        if self.nodes.contains_key(&spec.node_id) {
            return Err(ScalingError::NodeAlreadyExists(spec.node_id));
        }

        let node = ClusterNode::new(spec, now);
        self.nodes.insert(node.spec.node_id.clone(), node);
        Ok(())
    }

    pub fn remove_node(&mut self, node_id: &str, now: u64) -> Result<(), ScalingError> {
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| ScalingError::NodeNotFound(node_id.to_string()))?;

        if node.state == NodeState::Draining {
            node.transition(NodeState::Drained, now);
        } else {
            node.transition(NodeState::Decommissioned, now);
        }

        Ok(())
    }

    pub fn transition_node(
        &mut self,
        node_id: &str,
        new_state: NodeState,
        now: u64,
    ) -> Result<(), ScalingError> {
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| ScalingError::NodeNotFound(node_id.to_string()))?;

        if !node.state.can_transition_to(&new_state) {
            return Err(ScalingError::InvalidTransition {
                from: node.state,
                to: new_state,
            });
        }

        node.transition(new_state, now);
        Ok(())
    }

    pub fn get_node(&self, node_id: &str) -> Option<&ClusterNode> {
        self.nodes.get(node_id)
    }

    pub fn active_nodes(&self) -> Vec<&ClusterNode> {
        self.nodes
            .values()
            .filter(|n| n.state == NodeState::Active)
            .collect()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn active_count(&self) -> usize {
        self.active_nodes().len()
    }

    pub fn add_scaling_plan(&mut self, plan: ScalingPlan) {
        self.plans.insert(plan.plan_id.clone(), plan);
    }

    pub fn get_plan(&self, plan_id: &str) -> Option<&ScalingPlan> {
        self.plans.get(plan_id)
    }

    pub fn active_plans(&self) -> Vec<&ScalingPlan> {
        self.plans.values().filter(|p| !p.is_complete()).collect()
    }

    pub fn cluster_fill_percent(&self) -> f64 {
        let active: Vec<&ClusterNode> = self.active_nodes();
        if active.is_empty() {
            return 0.0;
        }

        let total_capacity: u64 = active.iter().map(|n| n.spec.nvme_capacity_bytes).sum();
        let total_data: u64 = active.iter().map(|n| n.data_bytes).sum();

        if total_capacity == 0 {
            return 0.0;
        }

        (total_data as f64 / total_capacity as f64) * 100.0
    }

    pub fn total_capacity_bytes(&self) -> u64 {
        self.active_nodes()
            .iter()
            .map(|n| n.spec.nvme_capacity_bytes)
            .sum()
    }

    pub fn total_data_bytes(&self) -> u64 {
        self.active_nodes().iter().map(|n| n.data_bytes).sum()
    }

    pub fn nodes_by_state(&self, state: NodeState) -> Vec<&ClusterNode> {
        self.nodes.values().filter(|n| n.state == state).collect()
    }

    pub fn all_nodes(&self) -> Vec<&ClusterNode> {
        self.nodes.values().collect()
    }

    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }

    pub fn complete_plan_count(&self) -> usize {
        self.plans.values().filter(|p| p.is_complete()).count()
    }
}

impl Default for NodeScalingManager {
    fn default() -> Self {
        Self::new()
    }
}

fn current_time_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_state_transitions_active_to_draining() {
        assert!(NodeState::Active.can_transition_to(&NodeState::Draining));
    }

    #[test]
    fn test_node_state_transitions_draining_to_drained() {
        assert!(NodeState::Draining.can_transition_to(&NodeState::Drained));
    }

    #[test]
    fn test_node_state_transitions_invalid() {
        assert!(!NodeState::Active.can_transition_to(&NodeState::Joining));
        assert!(!NodeState::Drained.can_transition_to(&NodeState::Active));
    }

    #[test]
    fn test_cluster_node_new_starts_joining() {
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let node = ClusterNode::new(spec, 1000);
        assert_eq!(node.state, NodeState::Joining);
    }

    #[test]
    fn test_cluster_node_transition_updates_state() {
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let mut node = ClusterNode::new(spec, 1000);

        node.transition(NodeState::Active, 2000);

        assert_eq!(node.state, NodeState::Active);
        assert_eq!(node.state_changed_at, 2000);
    }

    #[test]
    fn test_cluster_node_is_serving_true_for_active() {
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let mut node = ClusterNode::new(spec, 1000);
        node.transition(NodeState::Active, 1000);

        assert!(node.is_serving());
    }

    #[test]
    fn test_cluster_node_is_serving_false_for_draining() {
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let mut node = ClusterNode::new(spec, 1000);
        node.transition(NodeState::Draining, 1000);

        assert!(!node.is_serving());
    }

    #[test]
    fn test_cluster_node_fill_percent_empty() {
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let node = ClusterNode::new(spec, 1000);

        assert_eq!(node.fill_percent(), 0.0);
    }

    #[test]
    fn test_cluster_node_fill_percent_half_full() {
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let mut node = ClusterNode::new(spec, 1000);
        node.data_bytes = 500000000;

        assert!((node.fill_percent() - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_rebalance_task_progress_percent_zero() {
        let task = RebalanceTask::new("node1".to_string(), "node2".to_string(), 1, 1000000, 1000);
        assert_eq!(task.progress_percent(), 0.0);
    }

    #[test]
    fn test_rebalance_task_progress_percent_half() {
        let mut task =
            RebalanceTask::new("node1".to_string(), "node2".to_string(), 1, 1000000, 1000);
        task.bytes_moved = 500000;
        assert!((task.progress_percent() - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_rebalance_task_progress_percent_full() {
        let mut task =
            RebalanceTask::new("node1".to_string(), "node2".to_string(), 1, 1000000, 1000);
        task.bytes_moved = 1000000;
        assert_eq!(task.progress_percent(), 100.0);
    }

    #[test]
    fn test_rebalance_task_is_complete_false_initially() {
        let task = RebalanceTask::new("node1".to_string(), "node2".to_string(), 1, 1000000, 1000);
        assert!(!task.is_complete());
    }

    #[test]
    fn test_rebalance_task_is_complete_true_after_complete() {
        let mut task =
            RebalanceTask::new("node1".to_string(), "node2".to_string(), 1, 1000000, 1000);
        task.complete(2000);
        assert!(task.is_complete());
    }

    #[test]
    fn test_scaling_plan_progress_percent_zero() {
        let plan = ScalingPlan::new("plan1".to_string(), ScalingTrigger::Manual, vec![], 1000);
        assert_eq!(plan.progress_percent(), 0.0);
    }

    #[test]
    fn test_scaling_plan_is_complete_true_when_completed_tasks() {
        let mut tasks = vec![
            RebalanceTask::new("n1".to_string(), "n2".to_string(), 1, 1000, 1000),
            RebalanceTask::new("n1".to_string(), "n2".to_string(), 2, 1000, 1000),
        ];
        tasks[0].completed_at = Some(2000);

        let mut plan = ScalingPlan::new("plan1".to_string(), ScalingTrigger::Manual, tasks, 1000);
        plan.completed_tasks = 1;

        assert!(!plan.is_complete());
    }

    #[test]
    fn test_scaling_plan_total_tasks() {
        let tasks = vec![
            RebalanceTask::new("n1".to_string(), "n2".to_string(), 1, 1000, 1000),
            RebalanceTask::new("n1".to_string(), "n2".to_string(), 2, 1000, 1000),
        ];
        let plan = ScalingPlan::new("plan1".to_string(), ScalingTrigger::Manual, tasks, 1000);
        assert_eq!(plan.total_tasks(), 2);
    }

    #[test]
    fn test_node_scaling_manager_add_node() {
        let mut mgr = NodeScalingManager::new();
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );

        mgr.add_node(spec, 1000).unwrap();

        assert!(mgr.get_node("node1").is_some());
    }

    #[test]
    fn test_node_scaling_manager_duplicate_add_returns_error() {
        let mut mgr = NodeScalingManager::new();
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );

        mgr.add_node(spec.clone(), 1000).unwrap();
        let result = mgr.add_node(spec, 1000);

        assert!(matches!(result, Err(ScalingError::NodeAlreadyExists(_))));
    }

    #[test]
    fn test_node_scaling_manager_add_get_remove_round_trip() {
        let mut mgr = NodeScalingManager::new();
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );

        mgr.add_node(spec, 1000).unwrap();

        let node = mgr.get_node("node1");
        assert!(node.is_some());
        assert_eq!(node.unwrap().spec.address, "192.168.1.1");
    }

    #[test]
    fn test_node_scaling_manager_active_nodes() {
        let mut mgr = NodeScalingManager::new();

        let spec1 = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let spec2 = NodeSpec::new(
            "node2",
            "192.168.1.2",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );

        mgr.add_node(spec1, 1000).unwrap();
        mgr.add_node(spec2, 1000).unwrap();

        mgr.transition_node("node1", NodeState::Active, 2000)
            .unwrap();

        let active = mgr.active_nodes();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_node_scaling_manager_remove_node_not_found() {
        let mut mgr = NodeScalingManager::new();

        let result = mgr.remove_node("nonexistent", 1000);
        assert!(matches!(result, Err(ScalingError::NodeNotFound(_))));
    }

    #[test]
    fn test_node_scaling_manager_transition_node() {
        let mut mgr = NodeScalingManager::new();

        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        mgr.add_node(spec, 1000).unwrap();

        mgr.transition_node("node1", NodeState::Active, 2000)
            .unwrap();

        let node = mgr.get_node("node1").unwrap();
        assert_eq!(node.state, NodeState::Active);
    }

    #[test]
    fn test_node_scaling_manager_node_count() {
        let mut mgr = NodeScalingManager::new();

        mgr.add_node(
            NodeSpec::new(
                "node1",
                "192.168.1.1",
                NodeRole::Storage,
                1000000000,
                16000000000,
                32,
            ),
            1000,
        )
        .unwrap();
        mgr.add_node(
            NodeSpec::new(
                "node2",
                "192.168.1.2",
                NodeRole::Storage,
                1000000000,
                16000000000,
                32,
            ),
            1000,
        )
        .unwrap();

        assert_eq!(mgr.node_count(), 2);
    }

    #[test]
    fn test_node_scaling_manager_active_count() {
        let mut mgr = NodeScalingManager::new();

        mgr.add_node(
            NodeSpec::new(
                "node1",
                "192.168.1.1",
                NodeRole::Storage,
                1000000000,
                16000000000,
                32,
            ),
            1000,
        )
        .unwrap();
        mgr.add_node(
            NodeSpec::new(
                "node2",
                "192.168.1.2",
                NodeRole::Storage,
                1000000000,
                16000000000,
                32,
            ),
            1000,
        )
        .unwrap();

        mgr.transition_node("node1", NodeState::Active, 1000)
            .unwrap();

        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn test_node_scaling_manager_total_capacity_bytes() {
        let mut mgr = NodeScalingManager::new();

        mgr.add_node(
            NodeSpec::new(
                "node1",
                "192.168.1.1",
                NodeRole::Storage,
                1000000000,
                16000000000,
                32,
            ),
            1000,
        )
        .unwrap();
        mgr.add_node(
            NodeSpec::new(
                "node2",
                "192.168.1.2",
                NodeRole::Storage,
                2000000000,
                16000000000,
                32,
            ),
            1000,
        )
        .unwrap();

        mgr.transition_node("node1", NodeState::Active, 1000)
            .unwrap();
        mgr.transition_node("node2", NodeState::Active, 1000)
            .unwrap();

        assert_eq!(mgr.total_capacity_bytes(), 3000000000);
    }

    #[test]
    fn test_node_scaling_manager_cluster_fill_percent() {
        let mut mgr = NodeScalingManager::new();

        let mut spec1 = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let mut spec2 = NodeSpec::new(
            "node2",
            "192.168.1.2",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );

        mgr.add_node(spec1, 1000).unwrap();
        mgr.add_node(spec2, 1000).unwrap();

        mgr.transition_node("node1", NodeState::Active, 1000)
            .unwrap();
        mgr.transition_node("node2", NodeState::Active, 1000)
            .unwrap();

        if let Some(node) = mgr.nodes.get_mut("node1") {
            node.data_bytes = 500000000;
        }

        let fill = mgr.cluster_fill_percent();
        assert!((fill - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_node_scaling_manager_add_scaling_plan() {
        let mut mgr = NodeScalingManager::new();

        let tasks = vec![RebalanceTask::new(
            "n1".to_string(),
            "n2".to_string(),
            1,
            1000,
            1000,
        )];
        let plan = ScalingPlan::new("plan1".to_string(), ScalingTrigger::Manual, tasks, 1000);

        mgr.add_scaling_plan(plan);

        assert!(mgr.get_plan("plan1").is_some());
    }

    #[test]
    fn test_node_scaling_manager_get_plan() {
        let mut mgr = NodeScalingManager::new();

        let tasks = vec![RebalanceTask::new(
            "n1".to_string(),
            "n2".to_string(),
            1,
            1000,
            1000,
        )];
        let plan = ScalingPlan::new("plan1".to_string(), ScalingTrigger::Manual, tasks, 1000);

        mgr.add_scaling_plan(plan);

        let retrieved = mgr.get_plan("plan1");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_node_scaling_manager_active_plans() {
        let mut mgr = NodeScalingManager::new();

        let tasks = vec![RebalanceTask::new(
            "n1".to_string(),
            "n2".to_string(),
            1,
            1000,
            1000,
        )];
        let plan = ScalingPlan::new("plan1".to_string(), ScalingTrigger::Manual, tasks, 1000);

        mgr.add_scaling_plan(plan);

        let active = mgr.active_plans();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_scaling_error_display() {
        let err = ScalingError::NodeAlreadyExists("node1".to_string());
        assert!(err.to_string().contains("node1"));

        let err = ScalingError::NodeNotFound("node1".to_string());
        assert!(err.to_string().contains("node1"));

        let err = ScalingError::InvalidTransition {
            from: NodeState::Active,
            to: NodeState::Joining,
        };
        assert!(err.to_string().contains("Active"));
    }

    #[test]
    fn test_node_role_is_storage() {
        assert!(NodeRole::Storage.is_storage());
        assert!(NodeRole::StorageAndMetadata.is_storage());
        assert!(!NodeRole::Metadata.is_storage());
    }

    #[test]
    fn test_node_role_is_metadata() {
        assert!(NodeRole::Metadata.is_metadata());
        assert!(NodeRole::StorageAndMetadata.is_metadata());
        assert!(!NodeRole::Storage.is_metadata());
    }

    #[test]
    fn test_node_state_is_serving() {
        assert!(!NodeState::Joining.is_serving());
        assert!(NodeState::Active.is_serving());
        assert!(!NodeState::Draining.is_serving());
    }

    #[test]
    fn test_scaling_trigger_description() {
        let trigger = ScalingTrigger::NodeAdded("node1".to_string());
        assert!(trigger.description().contains("node1"));

        let trigger = ScalingTrigger::CapacityThreshold {
            threshold_percent: 80.0,
        };
        assert!(trigger.description().contains("80"));
    }

    #[test]
    fn test_node_spec_new() {
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        assert_eq!(spec.node_id, "node1");
        assert_eq!(spec.address, "192.168.1.1");
        assert_eq!(spec.role, NodeRole::Storage);
    }

    #[test]
    fn test_cluster_node_add_remove_shard() {
        let spec = NodeSpec::new(
            "node1",
            "192.168.1.1",
            NodeRole::Storage,
            1000000000,
            16000000000,
            32,
        );
        let mut node = ClusterNode::new(spec, 1000);

        node.add_shard(1);
        node.add_shard(2);
        node.add_shard(1);

        assert_eq!(node.shard_count(), 2);

        node.remove_shard(1);
        assert_eq!(node.shard_count(), 1);
    }
}
