use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HealthError {
    #[error("Node not found: {0}")]
    NodeNotFound(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Offline,
    Unknown,
}

impl HealthStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    pub fn severity(&self) -> u8 {
        match self {
            HealthStatus::Healthy => 0,
            HealthStatus::Unknown => 1,
            HealthStatus::Degraded => 2,
            HealthStatus::Offline => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealth {
    pub node_id: String,
    pub addr: String,
    pub status: HealthStatus,
    pub last_seen: u64,
    pub capacity_total: u64,
    pub capacity_used: u64,
    pub iops_current: u64,
    pub errors: Vec<String>,
    pub drive_count: u32,
    pub drives_healthy: u32,
    pub uptime_secs: u64,
}

impl NodeHealth {
    pub fn new(node_id: String, addr: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            node_id,
            addr,
            status: HealthStatus::Unknown,
            last_seen: now,
            capacity_total: 0,
            capacity_used: 0,
            iops_current: 0,
            errors: Vec::new(),
            drive_count: 0,
            drives_healthy: 0,
            uptime_secs: 0,
        }
    }

    pub fn capacity_percent(&self) -> f64 {
        if self.capacity_total == 0 {
            return 0.0;
        }
        (self.capacity_used as f64 / self.capacity_total as f64) * 100.0
    }

    pub fn is_capacity_warning(&self) -> bool {
        self.capacity_percent() > 80.0
    }

    pub fn is_capacity_critical(&self) -> bool {
        self.capacity_percent() > 95.0
    }

    pub fn all_drives_healthy(&self) -> bool {
        self.drive_count > 0 && self.drives_healthy == self.drive_count
    }

    pub fn add_error(&mut self, error: String) {
        self.errors.push(error);
    }

    pub fn age_secs(&self, current_time: u64) -> u64 {
        current_time.saturating_sub(self.last_seen)
    }

    pub fn is_stale(&self, current_time: u64, stale_threshold_secs: u64) -> bool {
        self.age_secs(current_time) > stale_threshold_secs
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterHealth {
    pub overall_status: HealthStatus,
    pub node_count: u32,
    pub healthy_nodes: u32,
    pub degraded_nodes: u32,
    pub offline_nodes: u32,
    pub total_capacity_bytes: u64,
    pub used_capacity_bytes: u64,
    pub total_iops: u64,
    pub replication_lag_secs: f64,
    pub has_replication_conflicts: bool,
    pub active_alerts: u32,
    pub summary: String,
}

impl ClusterHealth {
    pub fn worst_status(nodes: &[NodeHealth]) -> HealthStatus {
        nodes
            .iter()
            .map(|n| n.status.clone())
            .max_by_key(|s| s.severity())
            .unwrap_or(HealthStatus::Unknown)
    }

    pub fn total_capacity(nodes: &[NodeHealth]) -> u64 {
        nodes.iter().map(|n| n.capacity_total).sum()
    }

    pub fn used_capacity(nodes: &[NodeHealth]) -> u64 {
        nodes.iter().map(|n| n.capacity_used).sum()
    }

    pub fn capacity_percent(nodes: &[NodeHealth]) -> f64 {
        let total = Self::total_capacity(nodes);
        if total == 0 {
            return 0.0;
        }
        (Self::used_capacity(nodes) as f64 / total as f64) * 100.0
    }

    pub fn summarize(nodes: &[NodeHealth], alerts: u32) -> String {
        let _healthy = nodes
            .iter()
            .filter(|n| n.status == HealthStatus::Healthy)
            .count();
        let degraded = nodes
            .iter()
            .filter(|n| n.status == HealthStatus::Degraded)
            .count();
        let total = nodes.len();

        let pct = Self::capacity_percent(nodes);

        if degraded > 0 || alerts > 0 {
            format!(
                "WARNING: {} degraded node{}, {} active alert{}",
                degraded,
                if degraded == 1 { "" } else { "s" },
                alerts,
                if alerts == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "All {} node{} healthy, {:.1}% capacity used",
                total,
                if total == 1 { "" } else { "s" },
                pct
            )
        }
    }
}

pub struct HealthAggregator {
    nodes: HashMap<String, NodeHealth>,
    stale_threshold_secs: u64,
}

impl HealthAggregator {
    pub fn new(stale_threshold_secs: u64) -> Self {
        Self {
            nodes: HashMap::new(),
            stale_threshold_secs,
        }
    }

    pub fn update_node(&mut self, health: NodeHealth) {
        self.nodes.insert(health.node_id.clone(), health);
    }

    pub fn remove_node(&mut self, node_id: &str) -> Option<NodeHealth> {
        self.nodes.remove(node_id)
    }

    pub fn get_node(&self, node_id: &str) -> Option<&NodeHealth> {
        self.nodes.get(node_id)
    }

    pub fn mark_offline(&mut self, node_id: &str) -> Result<(), HealthError> {
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| HealthError::NodeNotFound(node_id.to_string()))?;
        node.status = HealthStatus::Offline;
        Ok(())
    }

    pub fn nodes_with_status(&self, status: &HealthStatus) -> Vec<&NodeHealth> {
        self.nodes
            .values()
            .filter(|n| &n.status == status)
            .collect()
    }

    pub fn cluster_health(&self, active_alerts: u32, replication_lag_secs: f64) -> ClusterHealth {
        let nodes: Vec<NodeHealth> = self.nodes.values().cloned().collect();
        let node_slice: &[NodeHealth] = &nodes;

        let has_conflicts = replication_lag_secs > 60.0;

        ClusterHealth {
            overall_status: ClusterHealth::worst_status(node_slice),
            node_count: self.nodes.len() as u32,
            healthy_nodes: node_slice
                .iter()
                .filter(|n| n.status == HealthStatus::Healthy)
                .count() as u32,
            degraded_nodes: node_slice
                .iter()
                .filter(|n| n.status == HealthStatus::Degraded)
                .count() as u32,
            offline_nodes: node_slice
                .iter()
                .filter(|n| n.status == HealthStatus::Offline)
                .count() as u32,
            total_capacity_bytes: ClusterHealth::total_capacity(node_slice),
            used_capacity_bytes: ClusterHealth::used_capacity(node_slice),
            total_iops: node_slice.iter().map(|n| n.iops_current).sum(),
            replication_lag_secs,
            has_replication_conflicts: has_conflicts,
            active_alerts,
            summary: ClusterHealth::summarize(node_slice, active_alerts),
        }
    }

    pub fn stale_nodes(&self, current_time: u64) -> Vec<&NodeHealth> {
        self.nodes
            .values()
            .filter(|n| n.is_stale(current_time, self.stale_threshold_secs))
            .collect()
    }

    pub fn all_nodes_by_severity(&self) -> Vec<&NodeHealth> {
        let mut nodes: Vec<&NodeHealth> = self.nodes.values().collect();
        nodes.sort_by(|a, b| b.status.severity().cmp(&a.status.severity()));
        nodes
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_is_ok() {
        assert!(HealthStatus::Healthy.is_ok());
        assert!(!HealthStatus::Degraded.is_ok());
        assert!(!HealthStatus::Offline.is_ok());
        assert!(!HealthStatus::Unknown.is_ok());
    }

    #[test]
    fn test_health_status_severity_order() {
        assert!(HealthStatus::Healthy.severity() < HealthStatus::Unknown.severity());
        assert!(HealthStatus::Unknown.severity() < HealthStatus::Degraded.severity());
        assert!(HealthStatus::Degraded.severity() < HealthStatus::Offline.severity());
    }

    #[test]
    fn test_node_health_new() {
        let node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        assert_eq!(node.status, HealthStatus::Unknown);
        assert_eq!(node.node_id, "node1");
    }

    #[test]
    fn test_capacity_percent() {
        let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        node.capacity_total = 1000;
        node.capacity_used = 500;
        assert!((node.capacity_percent() - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_is_capacity_warning() {
        let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        node.capacity_total = 1000;
        node.capacity_used = 810;
        assert!(node.is_capacity_warning());

        node.capacity_used = 790;
        assert!(!node.is_capacity_warning());
    }

    #[test]
    fn test_is_capacity_critical() {
        let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        node.capacity_total = 1000;
        node.capacity_used = 960;
        assert!(node.is_capacity_critical());

        node.capacity_used = 940;
        assert!(!node.is_capacity_critical());
    }

    #[test]
    fn test_all_drives_healthy() {
        let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        node.drive_count = 4;
        node.drives_healthy = 4;
        assert!(node.all_drives_healthy());

        node.drives_healthy = 3;
        assert!(!node.all_drives_healthy());
    }

    #[test]
    fn test_add_error() {
        let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        node.add_error("error 1".to_string());
        node.add_error("error 2".to_string());
        assert_eq!(node.errors.len(), 2);
    }

    #[test]
    fn test_age_secs() {
        let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        node.last_seen = 100;
        assert_eq!(node.age_secs(150), 50);
    }

    #[test]
    fn test_is_stale() {
        let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        node.last_seen = 100;
        assert!(node.is_stale(200, 50));
        assert!(!node.is_stale(140, 50));
    }

    #[test]
    fn test_worst_status_all_healthy() {
        let nodes = vec![
            NodeHealth {
                status: HealthStatus::Healthy,
                ..NodeHealth::new("n1".to_string(), "addr".to_string())
            },
            NodeHealth {
                status: HealthStatus::Healthy,
                ..NodeHealth::new("n2".to_string(), "addr".to_string())
            },
        ];
        assert_eq!(ClusterHealth::worst_status(&nodes), HealthStatus::Healthy);
    }

    #[test]
    fn test_worst_status_with_degraded() {
        let nodes = vec![
            NodeHealth {
                status: HealthStatus::Healthy,
                ..NodeHealth::new("n1".to_string(), "addr".to_string())
            },
            NodeHealth {
                status: HealthStatus::Degraded,
                ..NodeHealth::new("n2".to_string(), "addr".to_string())
            },
        ];
        assert_eq!(ClusterHealth::worst_status(&nodes), HealthStatus::Degraded);
    }

    #[test]
    fn test_total_capacity() {
        let nodes = vec![
            NodeHealth {
                capacity_total: 1000,
                ..NodeHealth::new("n1".to_string(), "addr".to_string())
            },
            NodeHealth {
                capacity_total: 2000,
                ..NodeHealth::new("n2".to_string(), "addr".to_string())
            },
        ];
        assert_eq!(ClusterHealth::total_capacity(&nodes), 3000);
    }

    #[test]
    fn test_cluster_health_capacity_percent() {
        let nodes = vec![
            NodeHealth {
                capacity_total: 1000,
                capacity_used: 500,
                ..NodeHealth::new("n1".to_string(), "addr".to_string())
            },
            NodeHealth {
                capacity_total: 1000,
                capacity_used: 300,
                ..NodeHealth::new("n2".to_string(), "addr".to_string())
            },
        ];
        let pct = ClusterHealth::capacity_percent(&nodes);
        assert!((pct - 40.0).abs() < 0.001);
    }

    #[test]
    fn test_summarize_all_healthy() {
        let nodes = vec![NodeHealth {
            status: HealthStatus::Healthy,
            ..NodeHealth::new("n1".to_string(), "addr".to_string())
        }];
        let summary = ClusterHealth::summarize(&nodes, 0);
        assert!(summary.contains("healthy"));
    }

    #[test]
    fn test_summarize_with_warning() {
        let nodes = vec![NodeHealth {
            status: HealthStatus::Degraded,
            ..NodeHealth::new("n1".to_string(), "addr".to_string())
        }];
        let summary = ClusterHealth::summarize(&nodes, 0);
        assert!(summary.contains("WARNING"));
    }

    #[test]
    fn test_update_node() {
        let mut aggregator = HealthAggregator::new(60);
        let node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        aggregator.update_node(node);
        assert_eq!(aggregator.node_count(), 1);
    }

    #[test]
    fn test_mark_offline() {
        let mut aggregator = HealthAggregator::new(60);
        let node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
        aggregator.update_node(node);
        aggregator.mark_offline("node1").unwrap();
        let node = aggregator.get_node("node1").unwrap();
        assert_eq!(node.status, HealthStatus::Offline);
    }

    #[test]
    fn test_nodes_with_status() {
        let mut aggregator = HealthAggregator::new(60);
        aggregator.update_node(NodeHealth {
            status: HealthStatus::Healthy,
            ..NodeHealth::new("n1".to_string(), "addr".to_string())
        });
        aggregator.update_node(NodeHealth {
            status: HealthStatus::Degraded,
            ..NodeHealth::new("n2".to_string(), "addr".to_string())
        });

        let healthy = aggregator.nodes_with_status(&HealthStatus::Healthy);
        assert_eq!(healthy.len(), 1);
    }

    #[test]
    fn test_stale_nodes() {
        let mut aggregator = HealthAggregator::new(60);

        let mut node1 = NodeHealth::new("n1".to_string(), "addr".to_string());
        node1.last_seen = 100;

        let mut node2 = NodeHealth::new("n2".to_string(), "addr".to_string());
        node2.last_seen = 180;

        aggregator.update_node(node1);
        aggregator.update_node(node2);

        let stale = aggregator.stale_nodes(200);
        assert_eq!(stale.len(), 1);
    }

    #[test]
    fn test_cluster_health_aggregates() {
        let mut aggregator = HealthAggregator::new(60);

        let mut node = NodeHealth::new("n1".to_string(), "addr".to_string());
        node.status = HealthStatus::Healthy;
        node.capacity_total = 1000;
        node.capacity_used = 500;
        node.iops_current = 1000;

        aggregator.update_node(node);

        let health = aggregator.cluster_health(0, 5.0);
        assert_eq!(health.node_count, 1);
        assert_eq!(health.total_iops, 1000);
    }

    #[test]
    fn test_all_nodes_by_severity() {
        let mut aggregator = HealthAggregator::new(60);

        aggregator.update_node(NodeHealth {
            status: HealthStatus::Healthy,
            ..NodeHealth::new("n1".to_string(), "addr".to_string())
        });
        aggregator.update_node(NodeHealth {
            status: HealthStatus::Offline,
            ..NodeHealth::new("n2".to_string(), "addr".to_string())
        });

        let sorted = aggregator.all_nodes_by_severity();
        assert_eq!(sorted[0].node_id, "n2");
    }
}
