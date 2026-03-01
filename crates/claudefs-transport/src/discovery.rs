//! SWIM-based service discovery for ClaudeFS cluster.
//!
//! This module provides the discovery layer that uses the SWIM (Scalable Weakly-consistent
//! Infection-style Membership) protocol for failure detection and cluster membership management.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Configuration for SWIM-based discovery.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// Initial seed node addresses for cluster bootstrap.
    pub seed_nodes: Vec<String>,
    /// SWIM probe interval (default: 1s).
    pub probe_interval: Duration,
    /// Probe response timeout (default: 500ms).
    pub probe_timeout: Duration,
    /// Time before suspected node is marked dead (default: 5s).
    pub suspicion_timeout: Duration,
    /// Number of indirect probes per direct probe failure (default: 3).
    pub indirect_probes: usize,
    /// Gossip dissemination interval (default: 200ms).
    pub gossip_interval: Duration,
    /// Number of peers to gossip to per round (default: 3).
    pub gossip_fanout: usize,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            seed_nodes: Vec::new(),
            probe_interval: Duration::from_secs(1),
            probe_timeout: Duration::from_millis(500),
            suspicion_timeout: Duration::from_secs(5),
            indirect_probes: 3,
            gossip_interval: Duration::from_millis(200),
            gossip_fanout: 3,
        }
    }
}

/// State of a cluster member in the SWIM protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// Node is alive and responsive.
    Alive,
    /// Node is suspected of being unreachable.
    Suspected,
    /// Node has been confirmed dead.
    Dead,
    /// Node has gracefully left the cluster.
    Left,
}

/// Information about a cluster member.
#[derive(Debug, Clone)]
pub struct MemberInfo {
    /// Unique node identifier.
    pub node_id: u64,
    /// Network address of the node.
    pub addr: String,
    /// Current state in the membership lifecycle.
    pub state: NodeState,
    /// SWIM incarnation number for refutation.
    pub incarnation: u64,
    /// Last time the node was seen alive.
    pub last_seen: Instant,
    /// Node capabilities, zone, role, etc.
    pub metadata: HashMap<String, String>,
}

impl MemberInfo {
    /// Creates a new MemberInfo with default values.
    pub fn new(node_id: u64, addr: String) -> Self {
        Self {
            node_id,
            addr,
            state: NodeState::Alive,
            incarnation: 0,
            last_seen: Instant::now(),
            metadata: HashMap::new(),
        }
    }

    /// Creates a MemberInfo with metadata.
    pub fn with_metadata(node_id: u64, addr: String, metadata: HashMap<String, String>) -> Self {
        Self {
            node_id,
            addr,
            state: NodeState::Alive,
            incarnation: 0,
            last_seen: Instant::now(),
            metadata,
        }
    }
}

/// Events emitted by the membership layer.
#[derive(Debug, Clone)]
pub enum MembershipEvent {
    /// A new node has joined the cluster.
    NodeJoined {
        /// Node identifier.
        node_id: u64,
        /// Node address.
        addr: String,
    },
    /// A node has gracefully left the cluster.
    NodeLeft {
        /// Node identifier.
        node_id: u64,
    },
    /// A node is suspected of being unreachable.
    NodeSuspected {
        /// Node identifier.
        node_id: u64,
    },
    /// A node has been confirmed dead.
    NodeFailed {
        /// Node identifier.
        node_id: u64,
    },
    /// A suspected or dead node has recovered.
    NodeRecovered {
        /// Node identifier.
        node_id: u64,
    },
}

type EventListener = Arc<dyn Fn(&MembershipEvent) + Send + Sync>;

/// Membership list managing cluster members.
///
/// Thread-safe membership tracking with event listeners for state changes.
pub struct MembershipList {
    members: Mutex<HashMap<u64, MemberInfo>>,
    local_node_id: u64,
    incarnation: AtomicU64,
    event_listeners: Mutex<Vec<EventListener>>,
}

impl MembershipList {
    /// Creates a new empty membership list.
    pub fn new(local_node_id: u64) -> Self {
        Self {
            members: Mutex::new(HashMap::new()),
            local_node_id,
            incarnation: AtomicU64::new(0),
            event_listeners: Mutex::new(Vec::new()),
        }
    }

    /// Adds or updates a member in the list.
    ///
    /// Returns `true` if this is a new member, `false` if updated.
    pub fn add_member(&self, info: MemberInfo) -> bool {
        let mut members = self.members.lock().unwrap();
        let is_new = !members.contains_key(&info.node_id);

        let old_state = members.get(&info.node_id).map(|m| m.state);
        members.insert(info.node_id, info.clone());

        drop(members);

        if is_new {
            self.notify_listeners(&MembershipEvent::NodeJoined {
                node_id: info.node_id,
                addr: info.addr,
            });
        } else if old_state == Some(NodeState::Dead) && info.state == NodeState::Alive {
            self.notify_listeners(&MembershipEvent::NodeRecovered {
                node_id: info.node_id,
            });
        }

        is_new
    }

    /// Removes a member from the list.
    ///
    /// Returns the removed member info, or None if not found.
    pub fn remove_member(&self, node_id: u64) -> Option<MemberInfo> {
        let mut members = self.members.lock().unwrap();
        let removed = members.remove(&node_id);

        if removed.is_some() {
            self.notify_listeners(&MembershipEvent::NodeLeft { node_id });
        }

        removed
    }

    /// Gets member info by node ID.
    pub fn get_member(&self, node_id: u64) -> Option<MemberInfo> {
        let members = self.members.lock().unwrap();
        members.get(&node_id).cloned()
    }

    /// Marks a node as suspected (possibly unreachable).
    ///
    /// Returns `true` if the state changed from Alive to Suspected.
    pub fn mark_suspected(&self, node_id: u64) -> bool {
        let mut members = self.members.lock().unwrap();

        if let Some(member) = members.get_mut(&node_id) {
            if member.state == NodeState::Alive {
                member.state = NodeState::Suspected;
                drop(members);
                self.notify_listeners(&MembershipEvent::NodeSuspected { node_id });
                return true;
            }
        }

        false
    }

    /// Marks a node as dead (confirmed unreachable).
    ///
    /// Returns `true` if the state changed from Suspected to Dead.
    pub fn mark_dead(&self, node_id: u64) -> bool {
        let mut members = self.members.lock().unwrap();

        if let Some(member) = members.get_mut(&node_id) {
            if member.state == NodeState::Suspected || member.state == NodeState::Alive {
                member.state = NodeState::Dead;
                member.last_seen = Instant::now();
                drop(members);
                self.notify_listeners(&MembershipEvent::NodeFailed { node_id });
                return true;
            }
        }

        false
    }

    /// Marks a node as alive (refutation of suspicion/death).
    ///
    /// Returns `true` if the state changed from Dead/Suspected to Alive.
    pub fn mark_alive(&self, node_id: u64) -> bool {
        let mut members = self.members.lock().unwrap();

        if let Some(member) = members.get_mut(&node_id) {
            if member.state != NodeState::Alive {
                member.state = NodeState::Alive;
                member.last_seen = Instant::now();
                member.incarnation = self.incarnation.load(Ordering::Relaxed);
                drop(members);
                self.notify_listeners(&MembershipEvent::NodeRecovered { node_id });
                return true;
            }
        }

        false
    }

    /// Returns all alive members.
    pub fn alive_members(&self) -> Vec<MemberInfo> {
        let members = self.members.lock().unwrap();
        members
            .values()
            .filter(|m| m.state == NodeState::Alive)
            .cloned()
            .collect()
    }

    /// Returns all members regardless of state.
    pub fn all_members(&self) -> Vec<MemberInfo> {
        let members = self.members.lock().unwrap();
        members.values().cloned().collect()
    }

    /// Returns the total number of members.
    pub fn member_count(&self) -> usize {
        let members = self.members.lock().unwrap();
        members.len()
    }

    /// Returns the number of alive members.
    pub fn alive_count(&self) -> usize {
        let members = self.members.lock().unwrap();
        members
            .values()
            .filter(|m| m.state == NodeState::Alive)
            .count()
    }

    /// Increments the incarnation counter for refutation.
    pub fn increment_incarnation(&self) -> u64 {
        self.incarnation.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Adds an event listener callback.
    pub fn add_event_listener(&self, listener: EventListener) {
        let mut listeners = self.event_listeners.lock().unwrap();
        listeners.push(listener);
    }

    /// Returns the local node ID.
    pub fn local_node_id(&self) -> u64 {
        self.local_node_id
    }

    /// Notifies all registered listeners of an event.
    fn notify_listeners(&self, event: &MembershipEvent) {
        let listeners = self.event_listeners.lock().unwrap();
        for listener in listeners.iter() {
            listener(event);
        }
    }

    /// Returns discovery statistics.
    pub fn stats(&self) -> DiscoveryStats {
        let members = self.members.lock().unwrap();
        let total = members.len();
        let alive = members
            .values()
            .filter(|m| m.state == NodeState::Alive)
            .count();
        let suspected = members
            .values()
            .filter(|m| m.state == NodeState::Suspected)
            .count();
        let dead = members
            .values()
            .filter(|m| m.state == NodeState::Dead)
            .count();
        let incarnation = self.incarnation.load(Ordering::Relaxed);

        DiscoveryStats {
            total_members: total,
            alive_members: alive,
            suspected_members: suspected,
            dead_members: dead,
            incarnation,
        }
    }
}

/// Statistics about the discovery layer.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryStats {
    /// Total number of members.
    pub total_members: usize,
    /// Number of alive members.
    pub alive_members: usize,
    /// Number of suspected members.
    pub suspected_members: usize,
    /// Number of dead members.
    pub dead_members: usize,
    /// Current incarnation number.
    pub incarnation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_discovery_config_default() {
        let config = DiscoveryConfig::default();
        assert!(config.seed_nodes.is_empty());
        assert_eq!(config.probe_interval, Duration::from_secs(1));
        assert_eq!(config.probe_timeout, Duration::from_millis(500));
        assert_eq!(config.suspicion_timeout, Duration::from_secs(5));
        assert_eq!(config.indirect_probes, 3);
        assert_eq!(config.gossip_interval, Duration::from_millis(200));
        assert_eq!(config.gossip_fanout, 3);
    }

    #[test]
    fn test_node_state_values() {
        let _ = NodeState::Alive;
        let _ = NodeState::Suspected;
        let _ = NodeState::Dead;
        let _ = NodeState::Left;
    }

    #[test]
    fn test_member_info_creation() {
        let info = MemberInfo::new(1, "192.168.1.1:8080".to_string());
        assert_eq!(info.node_id, 1);
        assert_eq!(info.addr, "192.168.1.1:8080");
        assert_eq!(info.state, NodeState::Alive);
        assert_eq!(info.incarnation, 0);
        assert!(info.metadata.is_empty());
    }

    #[test]
    fn test_membership_list_new() {
        let list = MembershipList::new(100);
        assert_eq!(list.member_count(), 0);
        assert_eq!(list.alive_count(), 0);
        assert_eq!(list.local_node_id(), 100);
    }

    #[test]
    fn test_add_member() {
        let list = MembershipList::new(1);
        let info = MemberInfo::new(2, "192.168.1.2:8080".to_string());

        let is_new = list.add_member(info);
        assert!(is_new);
        assert_eq!(list.member_count(), 1);
    }

    #[test]
    fn test_add_duplicate_member() {
        let list = MembershipList::new(1);
        let info = MemberInfo::new(2, "192.168.1.2:8080".to_string());

        list.add_member(info.clone());
        let is_new = list.add_member(info);

        assert!(!is_new);
        assert_eq!(list.member_count(), 1);
    }

    #[test]
    fn test_remove_member() {
        let list = MembershipList::new(1);
        let info = MemberInfo::new(2, "192.168.1.2:8080".to_string());

        list.add_member(info);
        let removed = list.remove_member(2);

        assert!(removed.is_some());
        assert_eq!(removed.unwrap().node_id, 2);
        assert_eq!(list.member_count(), 0);
    }

    #[test]
    fn test_remove_nonexistent() {
        let list = MembershipList::new(1);
        let removed = list.remove_member(999);
        assert!(removed.is_none());
    }

    #[test]
    fn test_get_member() {
        let list = MembershipList::new(1);
        let info = MemberInfo::new(2, "192.168.1.2:8080".to_string());

        list.add_member(info);
        let retrieved = list.get_member(2);

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().node_id, 2);
    }

    #[test]
    fn test_mark_suspected() {
        let list = MembershipList::new(1);
        let info = MemberInfo::new(2, "192.168.1.2:8080".to_string());

        list.add_member(info);
        let changed = list.mark_suspected(2);

        assert!(changed);

        let member = list.get_member(2).unwrap();
        assert_eq!(member.state, NodeState::Suspected);
    }

    #[test]
    fn test_mark_dead() {
        let list = MembershipList::new(1);
        let info = MemberInfo::new(2, "192.168.1.2:8080".to_string());

        list.add_member(info);
        list.mark_suspected(2);

        let changed = list.mark_dead(2);

        assert!(changed);

        let member = list.get_member(2).unwrap();
        assert_eq!(member.state, NodeState::Dead);
    }

    #[test]
    fn test_mark_alive_refutation() {
        let list = MembershipList::new(1);
        let info = MemberInfo::new(2, "192.168.1.2:8080".to_string());

        list.add_member(info);
        list.mark_suspected(2);
        list.mark_dead(2);

        let changed = list.mark_alive(2);

        assert!(changed);

        let member = list.get_member(2).unwrap();
        assert_eq!(member.state, NodeState::Alive);
    }

    #[test]
    fn test_alive_members() {
        let list = MembershipList::new(1);

        list.add_member(MemberInfo::new(2, "192.168.1.2:8080".to_string()));
        list.add_member(MemberInfo::new(3, "192.168.1.3:8080".to_string()));
        list.add_member(MemberInfo::new(4, "192.168.1.4:8080".to_string()));

        list.mark_dead(3);
        list.mark_dead(4);

        let alive = list.alive_members();

        assert_eq!(alive.len(), 1);
        assert!(alive.iter().any(|m| m.node_id == 2));
    }

    #[test]
    fn test_member_count() {
        let list = MembershipList::new(1);

        list.add_member(MemberInfo::new(2, "192.168.1.2:8080".to_string()));
        list.add_member(MemberInfo::new(3, "192.168.1.3:8080".to_string()));

        assert_eq!(list.member_count(), 2);
    }

    #[test]
    fn test_alive_count() {
        let list = MembershipList::new(1);

        list.add_member(MemberInfo::new(2, "192.168.1.2:8080".to_string()));
        list.add_member(MemberInfo::new(3, "192.168.1.3:8080".to_string()));

        list.mark_dead(3);

        assert_eq!(list.alive_count(), 1);
    }

    #[test]
    fn test_increment_incarnation() {
        let list = MembershipList::new(1);

        let inc1 = list.increment_incarnation();
        let inc2 = list.increment_incarnation();

        assert_eq!(inc1, 1);
        assert_eq!(inc2, 2);
    }

    #[test]
    fn test_stats() {
        let list = MembershipList::new(1);

        list.add_member(MemberInfo::new(2, "192.168.1.2:8080".to_string()));
        list.add_member(MemberInfo::new(3, "192.168.1.3:8080".to_string()));
        list.add_member(MemberInfo::new(4, "192.168.1.4:8080".to_string()));

        list.mark_suspected(3);

        let stats = list.stats();

        assert_eq!(stats.total_members, 3);
        assert_eq!(stats.alive_members, 2);
        assert_eq!(stats.suspected_members, 1);
        assert_eq!(stats.dead_members, 0);
    }

    #[test]
    fn test_membership_event_variants() {
        let _ = MembershipEvent::NodeJoined {
            node_id: 1,
            addr: "addr1".to_string(),
        };
        let _ = MembershipEvent::NodeLeft { node_id: 1 };
        let _ = MembershipEvent::NodeSuspected { node_id: 1 };
        let _ = MembershipEvent::NodeFailed { node_id: 1 };
        let _ = MembershipEvent::NodeRecovered { node_id: 1 };
    }

    #[test]
    fn test_event_listener() {
        let list = MembershipList::new(1);
        let event_count = Arc::new(AtomicUsize::new(0));

        let count_clone = Arc::clone(&event_count);
        let listener = Arc::new(move |_event: &MembershipEvent| {
            count_clone.fetch_add(1, Ordering::Relaxed);
        });

        list.add_event_listener(listener);
        list.add_member(MemberInfo::new(2, "192.168.1.2:8080".to_string()));

        assert_eq!(event_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_mark_alive_no_change() {
        let list = MembershipList::new(1);
        let info = MemberInfo::new(2, "192.168.1.2:8080".to_string());

        list.add_member(info);

        let changed = list.mark_alive(2);

        assert!(!changed);
    }

    #[test]
    fn test_all_members() {
        let list = MembershipList::new(1);

        list.add_member(MemberInfo::new(2, "192.168.1.2:8080".to_string()));
        list.add_member(MemberInfo::new(3, "192.168.1.3:8080".to_string()));

        list.mark_dead(3);

        let all = list.all_members();
        assert_eq!(all.len(), 2);
    }
}
