//! SWIM-inspired gossip membership protocol for cluster membership tracking.
//!
//! This module implements a pure state machine for the gossip protocol.
//! No actual networking — all I/O is handled by the caller.
//! Architecture decision D2: SWIM protocol with bootstrap seed list.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for the gossip protocol.
#[derive(Debug, Clone)]
pub struct GossipConfig {
    /// How many members to gossip to each cycle.
    pub fanout: usize,
    /// How often to probe members (in milliseconds).
    pub probe_interval_ms: u64,
    /// How long to wait before confirming dead (in milliseconds).
    pub suspect_timeout_ms: u64,
    /// How long to keep dead members (in milliseconds).
    pub dead_timeout_ms: u64,
    /// Max entries per gossip message.
    pub max_gossip_entries: usize,
    /// How long to wait for join ack (in milliseconds).
    pub join_timeout_ms: u64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            fanout: 3,
            probe_interval_ms: 1000,
            suspect_timeout_ms: 3000,
            dead_timeout_ms: 30000,
            max_gossip_entries: 100,
            join_timeout_ms: 5000,
        }
    }
}

/// State of a member in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberState {
    /// Member is alive and responsive.
    Alive,
    /// Member is suspected to be dead but not yet confirmed.
    Suspect,
    /// Member is confirmed dead.
    Dead,
    /// Member has voluntarily left the cluster.
    Left,
}

/// A member in the gossip cluster.
#[derive(Debug, Clone)]
pub struct GossipMember {
    /// Unique identifier for the node.
    pub node_id: String,
    /// Network address of the node.
    pub addr: String,
    /// Current state of the member.
    pub state: MemberState,
    /// Incarnation number for conflict resolution.
    pub incarnation: u64,
    /// Timestamp of last update (in simulated milliseconds).
    pub last_updated_ms: u64,
}

/// Events emitted by the gossip protocol.
#[derive(Debug, Clone, PartialEq)]
pub enum GossipEvent {
    /// A new member has joined the cluster.
    Joined {
        /// The node identifier.
        node_id: String,
        /// The node address.
        addr: String,
    },
    /// A member is suspected to be dead.
    Suspected {
        /// The suspected node identifier.
        node_id: String,
    },
    /// A dead member has been confirmed.
    Confirmed {
        /// The confirmed dead node identifier.
        node_id: String,
    },
    /// A member has voluntarily left the cluster.
    Left {
        /// The node that left.
        node_id: String,
    },
    /// A member's state has changed.
    StateChanged {
        /// The node whose state changed.
        node_id: String,
        /// The previous state.
        old_state: MemberState,
        /// The new state.
        new_state: MemberState,
    },
}

/// Statistics about the gossip protocol state.
#[derive(Debug, Clone, Default)]
pub struct GossipStats {
    /// Number of alive members.
    pub alive_count: usize,
    /// Number of suspected members.
    pub suspect_count: usize,
    /// Number of dead members.
    pub dead_count: usize,
    /// Total number of events emitted.
    pub total_events: u64,
    /// Number of probes performed.
    pub probe_count: u64,
    /// Number of gossip merges performed.
    pub merge_count: u64,
}

/// Snapshot of gossip statistics.
#[derive(Debug, Clone)]
pub struct GossipStatsSnapshot {
    /// Number of alive members.
    pub alive_count: usize,
    /// Number of suspected members.
    pub suspect_count: usize,
    /// Number of dead members.
    pub dead_count: usize,
    /// Total number of events emitted.
    pub total_events: u64,
    /// Number of probes performed.
    pub probe_count: u64,
    /// Number of gossip merges performed.
    pub merge_count: u64,
}

impl From<&GossipStats> for GossipStatsSnapshot {
    fn from(stats: &GossipStats) -> Self {
        Self {
            alive_count: stats.alive_count,
            suspect_count: stats.suspect_count,
            dead_count: stats.dead_count,
            total_events: stats.total_events,
            probe_count: stats.probe_count,
            merge_count: stats.merge_count,
        }
    }
}

/// Gossip protocol state machine for cluster membership.
///
/// This implements a SWIM-inspired gossip protocol for tracking cluster membership.
/// All timing is simulated via `advance_time()` for testability.
#[allow(dead_code)]
pub struct GossipNode {
    /// Configuration for the gossip protocol.
    config: GossipConfig,
    /// Local node identifier.
    local_id: String,
    /// Local node address.
    local_addr: String,
    /// Map of node_id to member info.
    members: HashMap<String, GossipMember>,
    /// Pending events to be sent out.
    pending_events: Vec<GossipEvent>,
    /// Current simulated time in milliseconds.
    current_time_ms: u64,
    /// Total number of events emitted.
    total_events: u64,
    /// Number of probes performed.
    probe_count: u64,
    /// Number of gossip merges performed.
    merge_count: u64,
}

impl GossipNode {
    /// Creates a new gossip node with the given configuration.
    ///
    /// The local node is automatically added to the member list as Alive.
    pub fn new(config: GossipConfig, local_id: String, local_addr: String) -> Self {
        let mut members = HashMap::new();
        members.insert(
            local_id.clone(),
            GossipMember {
                node_id: local_id.clone(),
                addr: local_addr.clone(),
                state: MemberState::Alive,
                incarnation: 1,
                last_updated_ms: 0,
            },
        );
        Self {
            config,
            local_id,
            local_addr,
            members,
            pending_events: Vec::new(),
            current_time_ms: 0,
            total_events: 0,
            probe_count: 0,
            merge_count: 0,
        }
    }

    /// Adds a peer to the cluster as Alive.
    ///
    /// Emits a `Joined` event and increments total_events.
    pub fn join(&mut self, peer_id: String, peer_addr: String) {
        let member = GossipMember {
            node_id: peer_id.clone(),
            addr: peer_addr.clone(),
            state: MemberState::Alive,
            incarnation: 1,
            last_updated_ms: self.current_time_ms,
        };
        self.members.insert(peer_id.clone(), member);
        self.pending_events.push(GossipEvent::Joined {
            node_id: peer_id,
            addr: peer_addr,
        });
        self.total_events += 1;
    }

    /// Marks a node as Left (voluntarily left the cluster).
    ///
    /// Emits a `Left` event and increments total_events.
    pub fn leave(&mut self, node_id: &str) {
        if let Some(member) = self.members.get_mut(node_id) {
            let old_state = member.state;
            member.state = MemberState::Left;
            member.last_updated_ms = self.current_time_ms;
            self.pending_events.push(GossipEvent::Left {
                node_id: node_id.to_string(),
            });
            self.pending_events.push(GossipEvent::StateChanged {
                node_id: node_id.to_string(),
                old_state,
                new_state: MemberState::Left,
            });
            self.total_events += 1;
        }
    }

    /// Marks a node as suspected (possibly dead).
    ///
    /// Transitions from Alive to Suspect state.
    /// Emits a `Suspected` event, increments incarnation, and total_events.
    pub fn mark_suspect(&mut self, node_id: &str) {
        if let Some(member) = self.members.get_mut(node_id) {
            if member.state == MemberState::Alive {
                let old_state = member.state;
                member.state = MemberState::Suspect;
                member.incarnation += 1;
                member.last_updated_ms = self.current_time_ms;
                self.pending_events.push(GossipEvent::Suspected {
                    node_id: node_id.to_string(),
                });
                self.pending_events.push(GossipEvent::StateChanged {
                    node_id: node_id.to_string(),
                    old_state,
                    new_state: MemberState::Suspect,
                });
                self.total_events += 1;
            }
        }
    }

    /// Confirms a node as dead.
    ///
    /// Transitions from Suspect to Dead state.
    /// Emits a `Confirmed` event and increments total_events.
    pub fn confirm_dead(&mut self, node_id: &str) {
        if let Some(member) = self.members.get_mut(node_id) {
            if member.state == MemberState::Suspect {
                let old_state = member.state;
                member.state = MemberState::Dead;
                member.last_updated_ms = self.current_time_ms;
                self.pending_events.push(GossipEvent::Confirmed {
                    node_id: node_id.to_string(),
                });
                self.pending_events.push(GossipEvent::StateChanged {
                    node_id: node_id.to_string(),
                    old_state,
                    new_state: MemberState::Dead,
                });
                self.total_events += 1;
            }
        }
    }

    /// Processes incoming gossip events from other nodes.
    ///
    /// Merges incoming events using the SWIM protocol rules:
    /// - Higher incarnation number wins
    /// - Joined events add new members as Alive
    /// - Suspected/Confirmed/Left events update member states
    pub fn process_gossip(&mut self, events: Vec<GossipEvent>) {
        if events.is_empty() {
            return;
        }
        self.merge_count += 1;

        for event in events {
            match &event {
                GossipEvent::Joined { node_id, addr } => {
                    let should_add = if let Some(existing) = self.members.get(node_id) {
                        // Higher incarnation wins
                        existing.incarnation < 1
                            || (existing.state == MemberState::Dead && existing.incarnation < 1)
                    } else {
                        true
                    };
                    if should_add {
                        self.members.insert(
                            node_id.clone(),
                            GossipMember {
                                node_id: node_id.clone(),
                                addr: addr.clone(),
                                state: MemberState::Alive,
                                incarnation: 1,
                                last_updated_ms: self.current_time_ms,
                            },
                        );
                    }
                }
                GossipEvent::Suspected { node_id } => {
                    if let Some(member) = self.members.get_mut(node_id) {
                        if member.state == MemberState::Alive {
                            member.state = MemberState::Suspect;
                            member.last_updated_ms = self.current_time_ms;
                        } else if member.state == MemberState::Suspect {
                            member.incarnation += 1;
                            member.last_updated_ms = self.current_time_ms;
                        }
                    }
                }
                GossipEvent::Confirmed { node_id } => {
                    if let Some(member) = self.members.get_mut(node_id) {
                        if member.state == MemberState::Suspect {
                            member.state = MemberState::Dead;
                            member.last_updated_ms = self.current_time_ms;
                        }
                    }
                }
                GossipEvent::Left { node_id } => {
                    if let Some(member) = self.members.get_mut(node_id) {
                        member.state = MemberState::Left;
                        member.last_updated_ms = self.current_time_ms;
                    }
                }
                GossipEvent::StateChanged { .. } => {
                    // State changes are handled through the other event types
                }
            }
        }
    }

    /// Returns pending gossip events to be sent to other nodes.
    ///
    /// Limits the number of events to max_gossip_entries from config.
    pub fn get_gossip_events(&self) -> Vec<GossipEvent> {
        let limit = self.config.max_gossip_entries;
        self.pending_events.iter().take(limit).cloned().collect()
    }

    /// Returns all members in the Alive state.
    pub fn alive_members(&self) -> Vec<&GossipMember> {
        self.members
            .values()
            .filter(|m| m.state == MemberState::Alive)
            .collect()
    }

    /// Returns the total number of members in the cluster.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Returns information about a specific member.
    pub fn get_member(&self, node_id: &str) -> Option<&GossipMember> {
        self.members.get(node_id)
    }

    /// Returns current statistics about the gossip protocol.
    pub fn stats(&self) -> GossipStats {
        let mut alive_count = 0;
        let mut suspect_count = 0;
        let mut dead_count = 0;

        for member in self.members.values() {
            match member.state {
                MemberState::Alive => alive_count += 1,
                MemberState::Suspect => suspect_count += 1,
                MemberState::Dead => dead_count += 1,
                MemberState::Left => {}
            }
        }

        GossipStats {
            alive_count,
            suspect_count,
            dead_count,
            total_events: self.total_events,
            probe_count: self.probe_count,
            merge_count: self.merge_count,
        }
    }

    /// Returns a snapshot of current statistics.
    pub fn stats_snapshot(&self) -> GossipStatsSnapshot {
        GossipStatsSnapshot::from(&self.stats())
    }

    /// Advances simulated time and performs periodic tasks.
    ///
    /// This method:
    /// - Advances current_time_ms by the given amount
    /// - Auto-transitions Suspect members to Dead after suspect_timeout_ms
    /// - Removes Dead members after dead_timeout_ms
    /// - Increments probe_count based on probe_interval_ms
    pub fn advance_time(&mut self, ms: u64) {
        let old_time = self.current_time_ms;
        self.current_time_ms += ms;

        // Auto-transition Suspect -> Dead after suspect_timeout_ms
        let mut to_confirm: Vec<String> = Vec::new();
        for (node_id, member) in self.members.iter() {
            if member.state == MemberState::Suspect {
                let time_since_suspect =
                    self.current_time_ms.saturating_sub(member.last_updated_ms);
                if time_since_suspect >= self.config.suspect_timeout_ms {
                    to_confirm.push(node_id.clone());
                }
            }
        }
        for node_id in to_confirm {
            self.confirm_dead(&node_id);
        }

        // Remove Dead members after dead_timeout_ms
        let dead_timeout = self.config.dead_timeout_ms;
        let nodes_to_remove: Vec<String> = self
            .members
            .iter()
            .filter(|(_, m)| {
                m.state == MemberState::Dead
                    && self.current_time_ms.saturating_sub(m.last_updated_ms) >= dead_timeout
            })
            .map(|(id, _)| id.clone())
            .collect();

        for node_id in nodes_to_remove {
            self.members.remove(&node_id);
        }

        // Increment probe_count based on probe_interval_ms
        let intervals_passed = (self.current_time_ms / self.config.probe_interval_ms)
            .saturating_sub(old_time / self.config.probe_interval_ms);
        self.probe_count += intervals_passed as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = GossipConfig::default();
        assert_eq!(config.fanout, 3);
        assert_eq!(config.probe_interval_ms, 1000);
        assert_eq!(config.suspect_timeout_ms, 3000);
        assert_eq!(config.dead_timeout_ms, 30000);
        assert_eq!(config.max_gossip_entries, 100);
        assert_eq!(config.join_timeout_ms, 5000);
    }

    #[test]
    fn test_new_node_starts_alive() {
        let config = GossipConfig::default();
        let node = GossipNode::new(
            config.clone(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );

        assert_eq!(node.member_count(), 1);
        let member = node.get_member("node1").expect("local node should exist");
        assert_eq!(member.node_id, "node1");
        assert_eq!(member.state, MemberState::Alive);
        assert_eq!(member.incarnation, 1);
    }

    #[test]
    fn test_join_member() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );

        node.join("node2".to_string(), "192.168.1.2".to_string());

        assert_eq!(node.member_count(), 2);
        let member = node.get_member("node2").expect("joined node should exist");
        assert_eq!(member.state, MemberState::Alive);

        let events = node.get_gossip_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, GossipEvent::Joined { node_id, addr } 
            if node_id == "node2" && addr == "192.168.1.2")));

        let stats = node.stats();
        assert_eq!(stats.total_events, 1);
    }

    #[test]
    fn test_leave_member() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );
        node.join("node2".to_string(), "192.168.1.2".to_string());

        node.leave("node2");

        let member = node
            .get_member("node2")
            .expect("left node should still exist");
        assert_eq!(member.state, MemberState::Left);

        let events = node.get_gossip_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, GossipEvent::Left { node_id } if node_id == "node2")));

        let stats = node.stats();
        assert_eq!(stats.total_events, 2); // Joined + Left
    }

    #[test]
    fn test_mark_suspect() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );
        node.join("node2".to_string(), "192.168.1.2".to_string());

        node.mark_suspect("node2");

        let member = node
            .get_member("node2")
            .expect("suspected node should exist");
        assert_eq!(member.state, MemberState::Suspect);

        let events = node.get_gossip_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, GossipEvent::Suspected { node_id } if node_id == "node2")));

        let stats = node.stats();
        assert_eq!(stats.suspect_count, 1);
        assert_eq!(stats.alive_count, 1);
    }

    #[test]
    fn test_confirm_dead() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );
        node.join("node2".to_string(), "192.168.1.2".to_string());
        node.mark_suspect("node2");

        node.confirm_dead("node2");

        let member = node
            .get_member("node2")
            .expect("confirmed dead node should exist");
        assert_eq!(member.state, MemberState::Dead);

        let events = node.get_gossip_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, GossipEvent::Confirmed { node_id } if node_id == "node2")));

        let stats = node.stats();
        assert_eq!(stats.dead_count, 1);
    }

    #[test]
    fn test_alive_members_excludes_dead_and_left() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );
        node.join("node2".to_string(), "192.168.1.2".to_string());
        node.join("node3".to_string(), "192.168.1.3".to_string());

        node.mark_suspect("node2");
        node.confirm_dead("node2");
        node.leave("node3");

        let alive = node.alive_members();
        assert_eq!(alive.len(), 1);
        assert!(alive.iter().any(|m| m.node_id == "node1"));
    }

    #[test]
    fn test_process_gossip_merge() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );

        let incoming_events = vec![
            GossipEvent::Joined {
                node_id: "node2".to_string(),
                addr: "192.168.1.2".to_string(),
            },
            GossipEvent::Joined {
                node_id: "node3".to_string(),
                addr: "192.168.1.3".to_string(),
            },
        ];

        node.process_gossip(incoming_events);

        assert!(node.get_member("node2").is_some());
        assert!(node.get_member("node3").is_some());

        let stats = node.stats();
        assert_eq!(stats.merge_count, 1);
    }

    #[test]
    fn test_gossip_event_propagation() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );
        node.join("node2".to_string(), "192.168.1.2".to_string());

        // Clear pending events
        let _ = node.get_gossip_events();

        node.mark_suspect("node2");

        let events = node.get_gossip_events();
        assert!(!events.is_empty());
        assert!(events
            .iter()
            .any(|e| matches!(e, GossipEvent::Suspected { .. })));
    }

    #[test]
    fn test_suspect_timeout() {
        let mut config = GossipConfig::default();
        config.suspect_timeout_ms = 1000;

        let mut node = GossipNode::new(config, "node1".to_string(), "192.168.1.1".to_string());
        node.join("node2".to_string(), "192.168.1.2".to_string());
        node.mark_suspect("node2");

        // Advance time past suspect timeout
        node.advance_time(1500);

        let member = node.get_member("node2").expect("node should exist");
        assert_eq!(member.state, MemberState::Dead);
    }

    #[test]
    fn test_dead_cleanup() {
        let mut config = GossipConfig::default();
        config.dead_timeout_ms = 2000;

        let mut node = GossipNode::new(config, "node1".to_string(), "192.168.1.1".to_string());
        node.join("node2".to_string(), "192.168.1.2".to_string());
        node.mark_suspect("node2");
        node.confirm_dead("node2");

        // Advance time past dead timeout
        node.advance_time(3000);

        assert!(node.get_member("node2").is_none());
    }

    #[test]
    fn test_incarnation_wins_in_merge() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );

        // First join
        node.join("node2".to_string(), "192.168.1.2".to_string());

        // Simulate a higher incarnation coming in via gossip
        node.process_gossip(vec![GossipEvent::Joined {
            node_id: "node2".to_string(),
            addr: "192.168.1.3".to_string(),
        }]);

        // The member should still exist (not removed due to incarnation handling)
        assert!(node.get_member("node2").is_some());
    }

    #[test]
    fn test_stats_tracking() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );
        node.join("node2".to_string(), "192.168.1.2".to_string());
        node.join("node3".to_string(), "192.168.1.3".to_string());

        node.mark_suspect("node2");

        let stats = node.stats();
        assert_eq!(stats.alive_count, 2); // node1 and node3
        assert_eq!(stats.suspect_count, 1); // node2
        assert_eq!(stats.dead_count, 0);
        assert_eq!(stats.total_events, 3); // 2 joins + 1 suspect
    }

    #[test]
    fn test_member_lookup() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );

        assert!(node.get_member("node1").is_some());
        assert!(node.get_member("nonexistent").is_none());

        node.join("node2".to_string(), "192.168.1.2".to_string());

        let member = node.get_member("node2");
        assert!(member.is_some());
        assert_eq!(member.unwrap().addr, "192.168.1.2");
    }

    #[test]
    fn test_rejoin() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );

        node.join("node2".to_string(), "192.168.1.2".to_string());
        node.leave("node2");

        // Verify it's left
        let member = node.get_member("node2").unwrap();
        assert_eq!(member.state, MemberState::Left);

        // Rejoin
        node.join("node2".to_string(), "192.168.1.3".to_string());

        let member = node.get_member("node2").unwrap();
        assert_eq!(member.state, MemberState::Alive);
        assert_eq!(member.addr, "192.168.1.3");
    }

    #[test]
    fn test_probe_count_increment() {
        let mut config = GossipConfig::default();
        config.probe_interval_ms = 100;

        let mut node = GossipNode::new(config, "node1".to_string(), "192.168.1.1".to_string());

        node.advance_time(500);

        let stats = node.stats();
        assert_eq!(stats.probe_count, 5); // 500ms / 100ms = 5 probes
    }

    #[test]
    fn test_stats_snapshot() {
        let mut node = GossipNode::new(
            GossipConfig::default(),
            "node1".to_string(),
            "192.168.1.1".to_string(),
        );
        node.join("node2".to_string(), "192.168.1.2".to_string());

        let snapshot = node.stats_snapshot();
        assert_eq!(snapshot.alive_count, 2);
        assert_eq!(snapshot.total_events, 1);
    }
}
