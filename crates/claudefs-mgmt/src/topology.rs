use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeRole {
    Storage,
    Client,
    Gateway,
    Conduit,
    Management,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeStatus {
    Online,
    Offline,
    Draining,
    Degraded,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub site_id: String,
    pub rack_id: String,
    pub role: NodeRole,
    pub status: NodeStatus,
    pub ip: String,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
}

impl NodeInfo {
    pub fn utilization(&self) -> f64 {
        if self.capacity_bytes == 0 {
            0.0
        } else {
            self.used_bytes as f64 / self.capacity_bytes as f64
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TopologyMap {
    nodes: Mutex<HashMap<String, NodeInfo>>,
}

impl TopologyMap {
    pub fn new() -> Self {
        Self {
            nodes: Mutex::new(HashMap::new()),
        }
    }

    pub fn upsert_node(&self, node: NodeInfo) {
        let mut nodes = self.nodes.lock().unwrap();
        nodes.insert(node.id.clone(), node);
    }

    pub fn remove_node(&self, id: &str) -> bool {
        let mut nodes = self.nodes.lock().unwrap();
        nodes.remove(id).is_some()
    }

    pub fn get_node(&self, id: &str) -> Option<NodeInfo> {
        let nodes = self.nodes.lock().unwrap();
        nodes.get(id).cloned()
    }

    pub fn nodes_in_site(&self, site_id: &str) -> Vec<NodeInfo> {
        let nodes = self.nodes.lock().unwrap();
        nodes
            .values()
            .filter(|n| n.site_id == site_id)
            .cloned()
            .collect()
    }

    pub fn nodes_in_rack(&self, rack_id: &str) -> Vec<NodeInfo> {
        let nodes = self.nodes.lock().unwrap();
        nodes
            .values()
            .filter(|n| n.rack_id == rack_id)
            .cloned()
            .collect()
    }

    pub fn nodes_by_role(&self, role: NodeRole) -> Vec<NodeInfo> {
        let nodes = self.nodes.lock().unwrap();
        nodes.values().filter(|n| n.role == role).cloned().collect()
    }

    pub fn nodes_by_status(&self, status: NodeStatus) -> Vec<NodeInfo> {
        let nodes = self.nodes.lock().unwrap();
        nodes
            .values()
            .filter(|n| n.status == status)
            .cloned()
            .collect()
    }

    pub fn site_ids(&self) -> Vec<String> {
        let nodes = self.nodes.lock().unwrap();
        let mut site_ids: Vec<String> = nodes.values().map(|n| n.site_id.clone()).collect();
        site_ids.sort();
        site_ids.dedup();
        site_ids
    }

    pub fn rack_ids_in_site(&self, site_id: &str) -> Vec<String> {
        let nodes = self.nodes.lock().unwrap();
        let mut rack_ids: Vec<String> = nodes
            .values()
            .filter(|n| n.site_id == site_id)
            .map(|n| n.rack_id.clone())
            .collect();
        rack_ids.sort();
        rack_ids.dedup();
        rack_ids
    }

    pub fn node_count(&self) -> usize {
        let nodes = self.nodes.lock().unwrap();
        nodes.len()
    }

    pub fn total_capacity_bytes(&self) -> u64 {
        let nodes = self.nodes.lock().unwrap();
        nodes
            .values()
            .filter(|n| matches!(n.role, NodeRole::Storage))
            .map(|n| n.capacity_bytes)
            .sum()
    }

    pub fn total_used_bytes(&self) -> u64 {
        let nodes = self.nodes.lock().unwrap();
        nodes
            .values()
            .filter(|n| matches!(n.role, NodeRole::Storage))
            .map(|n| n.used_bytes)
            .sum()
    }
}

impl Default for TopologyMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(
        id: &str,
        site_id: &str,
        rack_id: &str,
        role: NodeRole,
        status: NodeStatus,
        capacity: u64,
        used: u64,
    ) -> NodeInfo {
        NodeInfo {
            id: id.to_string(),
            site_id: site_id.to_string(),
            rack_id: rack_id.to_string(),
            role,
            status,
            ip: format!("192.168.1.{}", id),
            capacity_bytes: capacity,
            used_bytes: used,
        }
    }

    #[test]
    fn test_upsert_adds_node() {
        let topology = TopologyMap::new();
        let node = make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        );
        topology.upsert_node(node);
        assert_eq!(topology.node_count(), 1);
        assert!(topology.get_node("n1").is_some());
    }

    #[test]
    fn test_upsert_replaces_node() {
        let topology = TopologyMap::new();
        let node1 = make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        );
        topology.upsert_node(node1);

        let node2 = make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Offline,
            2000,
            1000,
        );
        topology.upsert_node(node2);

        assert_eq!(topology.node_count(), 1);
        let retrieved = topology.get_node("n1").unwrap();
        assert_eq!(retrieved.status, NodeStatus::Offline);
        assert_eq!(retrieved.capacity_bytes, 2000);
    }

    #[test]
    fn test_remove_node_returns_true_for_existing() {
        let topology = TopologyMap::new();
        let node = make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        );
        topology.upsert_node(node);
        assert!(topology.remove_node("n1"));
        assert_eq!(topology.node_count(), 0);
    }

    #[test]
    fn test_remove_node_returns_false_for_missing() {
        let topology = TopologyMap::new();
        assert!(!topology.remove_node("n1"));
    }

    #[test]
    fn test_nodes_in_site_filters_correctly() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack2",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));
        topology.upsert_node(make_node(
            "n3",
            "site2",
            "rack1",
            NodeRole::Gateway,
            NodeStatus::Online,
            0,
            0,
        ));

        let site1_nodes = topology.nodes_in_site("site1");
        assert_eq!(site1_nodes.len(), 2);
        assert!(site1_nodes.iter().all(|n| n.site_id == "site1"));
    }

    #[test]
    fn test_nodes_in_site_empty_for_missing() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));

        let site2_nodes = topology.nodes_in_site("site2");
        assert!(site2_nodes.is_empty());
    }

    #[test]
    fn test_nodes_in_rack_filters_correctly() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));
        topology.upsert_node(make_node(
            "n3",
            "site1",
            "rack2",
            NodeRole::Gateway,
            NodeStatus::Online,
            0,
            0,
        ));

        let rack1_nodes = topology.nodes_in_rack("rack1");
        assert_eq!(rack1_nodes.len(), 2);
        assert!(rack1_nodes.iter().all(|n| n.rack_id == "rack1"));
    }

    #[test]
    fn test_nodes_in_rack_empty_for_missing() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));

        let rack2_nodes = topology.nodes_in_rack("rack2");
        assert!(rack2_nodes.is_empty());
    }

    #[test]
    fn test_nodes_by_role_filters_correctly() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));
        topology.upsert_node(make_node(
            "n3",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            2000,
            1000,
        ));

        let storage_nodes = topology.nodes_by_role(NodeRole::Storage);
        assert_eq!(storage_nodes.len(), 2);
        assert!(storage_nodes
            .iter()
            .all(|n| matches!(n.role, NodeRole::Storage)));
    }

    #[test]
    fn test_nodes_by_role_empty_for_missing() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));

        let gateway_nodes = topology.nodes_by_role(NodeRole::Gateway);
        assert!(gateway_nodes.is_empty());
    }

    #[test]
    fn test_nodes_by_status_filters_correctly() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Offline,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n3",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            2000,
            1000,
        ));

        let online_nodes = topology.nodes_by_status(NodeStatus::Online);
        assert_eq!(online_nodes.len(), 2);
        assert!(online_nodes
            .iter()
            .all(|n| matches!(n.status, NodeStatus::Online)));
    }

    #[test]
    fn test_nodes_by_status_empty_for_missing() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));

        let draining_nodes = topology.nodes_by_status(NodeStatus::Draining);
        assert!(draining_nodes.is_empty());
    }

    #[test]
    fn test_site_ids_returns_sorted_unique_ids() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site2",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n3",
            "site2",
            "rack2",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));

        let ids = topology.site_ids();
        assert_eq!(ids, vec!["site1", "site2"]);
    }

    #[test]
    fn test_site_ids_empty_when_no_nodes() {
        let topology = TopologyMap::new();
        let ids = topology.site_ids();
        assert!(ids.is_empty());
    }

    #[test]
    fn test_rack_ids_in_site_returns_sorted_unique_ids() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack2",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n3",
            "site2",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));

        let rack_ids = topology.rack_ids_in_site("site1");
        assert_eq!(rack_ids, vec!["rack1", "rack2"]);
    }

    #[test]
    fn test_rack_ids_in_site_empty_for_missing_site() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));

        let rack_ids = topology.rack_ids_in_site("site2");
        assert!(rack_ids.is_empty());
    }

    #[test]
    fn test_total_capacity_bytes_sums_storage_nodes_only() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));
        topology.upsert_node(make_node(
            "n3",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            2000,
            1000,
        ));

        let total = topology.total_capacity_bytes();
        assert_eq!(total, 3000);
    }

    #[test]
    fn test_total_capacity_bytes_excludes_non_storage() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Gateway,
            NodeStatus::Online,
            2000,
            1000,
        ));

        let total = topology.total_capacity_bytes();
        assert_eq!(total, 0);
    }

    #[test]
    fn test_total_capacity_bytes_empty_when_no_nodes() {
        let topology = TopologyMap::new();
        let total = topology.total_capacity_bytes();
        assert_eq!(total, 0);
    }

    #[test]
    fn test_total_used_bytes_sums_storage_nodes_only() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));
        topology.upsert_node(make_node(
            "n3",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            2000,
            1000,
        ));

        let total = topology.total_used_bytes();
        assert_eq!(total, 1500);
    }

    #[test]
    fn test_total_used_bytes_excludes_non_storage() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Gateway,
            NodeStatus::Online,
            2000,
            1000,
        ));

        let total = topology.total_used_bytes();
        assert_eq!(total, 0);
    }

    #[test]
    fn test_utilization_returns_fraction() {
        let node = make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        );
        assert!((node.utilization() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_utilization_returns_zero_for_zero_capacity() {
        let node = make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            0,
            0,
        );
        assert_eq!(node.utilization(), 0.0);
    }

    #[test]
    fn test_utilization_full() {
        let node = make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            1000,
        );
        assert!((node.utilization() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_node_count_correct_after_add() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        assert_eq!(topology.node_count(), 1);

        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));
        assert_eq!(topology.node_count(), 2);
    }

    #[test]
    fn test_node_count_correct_after_remove() {
        let topology = TopologyMap::new();
        topology.upsert_node(make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "n2",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Online,
            0,
            0,
        ));
        assert_eq!(topology.node_count(), 2);

        topology.remove_node("n1");
        assert_eq!(topology.node_count(), 1);

        topology.remove_node("n2");
        assert_eq!(topology.node_count(), 0);
    }

    #[test]
    fn test_get_node_returns_none_for_missing() {
        let topology = TopologyMap::new();
        assert!(topology.get_node("n1").is_none());
    }

    #[test]
    fn test_get_node_returns_some_for_existing() {
        let topology = TopologyMap::new();
        let node = make_node(
            "n1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        );
        topology.upsert_node(node.clone());

        let retrieved = topology.get_node("n1").unwrap();
        assert_eq!(retrieved.id, "n1");
        assert_eq!(retrieved.site_id, "site1");
        assert_eq!(retrieved.rack_id, "rack1");
    }

    #[test]
    fn test_all_roles_and_statuses_work() {
        let topology = TopologyMap::new();

        topology.upsert_node(make_node(
            "s1",
            "site1",
            "rack1",
            NodeRole::Storage,
            NodeStatus::Online,
            1000,
            500,
        ));
        topology.upsert_node(make_node(
            "c1",
            "site1",
            "rack1",
            NodeRole::Client,
            NodeStatus::Offline,
            0,
            0,
        ));
        topology.upsert_node(make_node(
            "g1",
            "site1",
            "rack1",
            NodeRole::Gateway,
            NodeStatus::Draining,
            0,
            0,
        ));
        topology.upsert_node(make_node(
            "co1",
            "site1",
            "rack1",
            NodeRole::Conduit,
            NodeStatus::Degraded,
            0,
            0,
        ));
        topology.upsert_node(make_node(
            "m1",
            "site1",
            "rack1",
            NodeRole::Management,
            NodeStatus::Unknown,
            0,
            0,
        ));

        assert_eq!(topology.nodes_by_role(NodeRole::Storage).len(), 1);
        assert_eq!(topology.nodes_by_role(NodeRole::Client).len(), 1);
        assert_eq!(topology.nodes_by_role(NodeRole::Gateway).len(), 1);
        assert_eq!(topology.nodes_by_role(NodeRole::Conduit).len(), 1);
        assert_eq!(topology.nodes_by_role(NodeRole::Management).len(), 1);

        assert_eq!(topology.nodes_by_status(NodeStatus::Online).len(), 1);
        assert_eq!(topology.nodes_by_status(NodeStatus::Offline).len(), 1);
        assert_eq!(topology.nodes_by_status(NodeStatus::Draining).len(), 1);
        assert_eq!(topology.nodes_by_status(NodeStatus::Degraded).len(), 1);
        assert_eq!(topology.nodes_by_status(NodeStatus::Unknown).len(), 1);
    }

    #[test]
    fn test_default() {
        let topology = TopologyMap::default();
        assert_eq!(topology.node_count(), 0);
    }

    #[test]
    fn test_new() {
        let topology = TopologyMap::new();
        assert_eq!(topology.node_count(), 0);
    }
}
