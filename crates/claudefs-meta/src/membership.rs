//! SWIM-based cluster membership tracking.
//!
//! Tracks cluster node membership using the SWIM protocol (decision D2).
//! Detects node failures, manages state transitions (Alive -> Suspect -> Dead),
//! and emits membership events for shard rebalancing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

/// State of a cluster node in the SWIM protocol.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeState {
    /// Node is alive and responsive.
    Alive,
    /// Node is suspected to be unreachable.
    Suspect,
    /// Node is confirmed dead.
    Dead,
}

/// Information about a cluster member.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemberInfo {
    /// Unique node identifier.
    pub node_id: NodeId,
    /// Network address of the node.
    pub address: String,
    /// Current state of the node.
    pub state: NodeState,
    /// Last heartbeat timestamp.
    pub last_heartbeat: Timestamp,
    /// Timestamp when node joined the cluster.
    pub joined_at: Timestamp,
    /// Generation number for the membership entry.
    pub generation: u64,
}

/// Events emitted by membership changes.
#[derive(Clone, Debug)]
pub enum MembershipEvent {
    /// A new node joined the cluster.
    NodeJoined {
        /// ID of the node that joined.
        node_id: NodeId,
    },
    /// A node is suspected to be unreachable.
    NodeSuspected {
        /// ID of the suspected node.
        node_id: NodeId,
    },
    /// A node is confirmed dead.
    NodeDead {
        /// ID of the dead node.
        node_id: NodeId,
    },
    /// A suspected node recovered.
    NodeRecovered {
        /// ID of the recovered node.
        node_id: NodeId,
    },
}

/// Manages cluster membership using the SWIM protocol.
pub struct MembershipManager {
    /// Member table indexed by NodeId.
    members: RwLock<HashMap<NodeId, MemberInfo>>,
    /// Pending membership events.
    events: RwLock<Vec<MembershipEvent>>,
    /// Local node identifier (used for membership decisions).
    #[allow(dead_code)]
    local_node_id: NodeId,
}

impl MembershipManager {
    /// Creates a new membership manager.
    ///
    /// # Arguments
    /// * `local_node_id` - The local node's identifier
    pub fn new(local_node_id: NodeId) -> Self {
        Self {
            members: RwLock::new(HashMap::new()),
            events: RwLock::new(Vec::new()),
            local_node_id,
        }
    }

    /// Adds a new node to the cluster as Alive.
    ///
    /// # Arguments
    /// * `node_id` - The node to add
    /// * `address` - The node's network address
    ///
    /// # Returns
    /// Ok(()) on success
    pub fn join(&self, node_id: NodeId, address: String) -> Result<(), MetaError> {
        let now = Timestamp::now();

        let member = MemberInfo {
            node_id,
            address,
            state: NodeState::Alive,
            last_heartbeat: now,
            joined_at: now,
            generation: 1,
        };

        let mut members = self.members.write().unwrap();
        members.insert(node_id, member);

        let mut events = self.events.write().unwrap();
        events.push(MembershipEvent::NodeJoined { node_id });

        Ok(())
    }

    /// Removes a node from the cluster.
    ///
    /// # Arguments
    /// * `node_id` - The node to remove
    ///
    /// # Returns
    /// Ok(true) if the node existed, Ok(false) otherwise
    pub fn leave(&self, node_id: NodeId) -> Result<bool, MetaError> {
        let mut members = self.members.write().unwrap();

        let existed = members.remove(&node_id).is_some();

        if existed {
            let mut events = self.events.write().unwrap();
            events.push(MembershipEvent::NodeDead { node_id });
        }

        Ok(existed)
    }

    /// Marks a node as suspected (possibly unreachable).
    ///
    /// # Arguments
    /// * `node_id` - The node to mark as suspected
    ///
    /// # Returns
    /// Ok(()) on success, Err if node not found
    pub fn suspect(&self, node_id: NodeId) -> Result<(), MetaError> {
        let mut members = self.members.write().unwrap();

        let member = members
            .get_mut(&node_id)
            .ok_or_else(|| MetaError::KvError(format!("node {:?} not found", node_id)))?;

        if member.state != NodeState::Dead {
            member.state = NodeState::Suspect;
            member.generation = member.generation.saturating_add(1);

            let mut events = self.events.write().unwrap();
            events.push(MembershipEvent::NodeSuspected { node_id });
        }

        Ok(())
    }

    /// Confirms a node is alive or recovers a suspected node.
    ///
    /// # Arguments
    /// * `node_id` - The node to confirm
    ///
    /// # Returns
    /// Ok(()) on success
    pub fn confirm_alive(&self, node_id: NodeId) -> Result<(), MetaError> {
        let mut members = self.members.write().unwrap();

        let member = members
            .get_mut(&node_id)
            .ok_or_else(|| MetaError::KvError(format!("node {:?} not found", node_id)))?;

        if member.state == NodeState::Suspect {
            member.state = NodeState::Alive;
            member.generation = member.generation.saturating_add(1);

            let mut events = self.events.write().unwrap();
            events.push(MembershipEvent::NodeRecovered { node_id });
        } else if member.state == NodeState::Alive {
            member.last_heartbeat = Timestamp::now();
        }

        Ok(())
    }

    /// Marks a node as dead.
    ///
    /// # Arguments
    /// * `node_id` - The node to mark as dead
    ///
    /// # Returns
    /// Ok(()) on success, Err if node not found
    pub fn mark_dead(&self, node_id: NodeId) -> Result<(), MetaError> {
        let mut members = self.members.write().unwrap();

        let member = members
            .get_mut(&node_id)
            .ok_or_else(|| MetaError::KvError(format!("node {:?} not found", node_id)))?;

        if member.state != NodeState::Dead {
            member.state = NodeState::Dead;
            member.generation = member.generation.saturating_add(1);

            let mut events = self.events.write().unwrap();
            events.push(MembershipEvent::NodeDead { node_id });
        }

        Ok(())
    }

    /// Updates the last heartbeat timestamp for a node.
    ///
    /// # Arguments
    /// * `node_id` - The node to update
    ///
    /// # Returns
    /// Ok(()) on success, Err if node not found
    pub fn heartbeat(&self, node_id: NodeId) -> Result<(), MetaError> {
        let mut members = self.members.write().unwrap();

        let member = members
            .get_mut(&node_id)
            .ok_or_else(|| MetaError::KvError(format!("node {:?} not found", node_id)))?;

        member.last_heartbeat = Timestamp::now();

        Ok(())
    }

    /// Returns all alive node IDs.
    ///
    /// # Returns
    /// Vector of alive node IDs
    pub fn alive_nodes(&self) -> Vec<NodeId> {
        let members = self.members.read().unwrap();
        members
            .values()
            .filter(|m| m.state == NodeState::Alive)
            .map(|m| m.node_id)
            .collect()
    }

    /// Returns information about all members.
    ///
    /// # Returns
    /// Vector of all member info
    pub fn all_members(&self) -> Vec<MemberInfo> {
        let members = self.members.read().unwrap();
        members.values().cloned().collect()
    }

    /// Returns the total number of members (including suspects and dead).
    pub fn member_count(&self) -> usize {
        let members = self.members.read().unwrap();
        members.len()
    }

    /// Returns the number of alive members.
    pub fn alive_count(&self) -> usize {
        let members = self.members.read().unwrap();
        members
            .values()
            .filter(|m| m.state == NodeState::Alive)
            .count()
    }

    /// Gets member information for a specific node.
    ///
    /// # Arguments
    /// * `node_id` - The node to look up
    ///
    /// # Returns
    /// Member info if found
    pub fn get_member(&self, node_id: NodeId) -> Option<MemberInfo> {
        let members = self.members.read().unwrap();
        members.get(&node_id).cloned()
    }

    /// Drains and returns all pending membership events.
    ///
    /// # Returns
    /// Vector of pending events
    pub fn drain_events(&self) -> Vec<MembershipEvent> {
        let mut events = self.events.write().unwrap();
        let drained: Vec<MembershipEvent> = events.drain(..).collect();
        drained
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_id(id: u64) -> NodeId {
        NodeId::new(id)
    }

    #[test]
    fn test_join() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        assert_eq!(mgr.member_count(), 1);
        assert_eq!(mgr.alive_count(), 1);
    }

    #[test]
    fn test_join_emits_event() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        let events = mgr.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            MembershipEvent::NodeJoined { node_id } if node_id == make_node_id(2)
        ));
    }

    #[test]
    fn test_leave() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        let removed = mgr.leave(make_node_id(2)).expect("leave failed");
        assert!(removed);

        assert_eq!(mgr.member_count(), 0);
    }

    #[test]
    fn test_leave_emits_dead_event() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.leave(make_node_id(2)).expect("leave failed");

        let events = mgr.drain_events();
        assert!(matches!(
            events[1],
            MembershipEvent::NodeDead { node_id } if node_id == make_node_id(2)
        ));
    }

    #[test]
    fn test_leave_not_found() {
        let mgr = MembershipManager::new(make_node_id(1));

        let result = mgr.leave(make_node_id(2)).expect("leave failed");
        assert!(!result);
    }

    #[test]
    fn test_suspect() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.suspect(make_node_id(2)).expect("suspect failed");

        let member = mgr.get_member(make_node_id(2)).expect("member not found");
        assert_eq!(member.state, NodeState::Suspect);
    }

    #[test]
    fn test_confirm_alive_emits_recovered_event() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.suspect(make_node_id(2)).expect("suspect failed");
        mgr.confirm_alive(make_node_id(2))
            .expect("confirm_alive failed");

        let events = mgr.drain_events();
        let last_event = events.last().expect("no events");
        assert!(matches!(
            last_event,
            MembershipEvent::NodeRecovered { node_id } if *node_id == make_node_id(2)
        ));
    }

    #[test]
    fn test_confirm_alive_from_suspect() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.suspect(make_node_id(2)).expect("suspect failed");
        mgr.confirm_alive(make_node_id(2))
            .expect("confirm_alive failed");

        let member = mgr.get_member(make_node_id(2)).expect("member not found");
        assert_eq!(member.state, NodeState::Alive);
    }

    #[test]
    fn test_confirm_alive_updates_heartbeat_for_alive() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.confirm_alive(make_node_id(2))
            .expect("confirm_alive failed");

        let member = mgr.get_member(make_node_id(2)).expect("member not found");
        assert_eq!(member.state, NodeState::Alive);
    }

    #[test]
    fn test_mark_dead() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.mark_dead(make_node_id(2)).expect("mark_dead failed");

        let member = mgr.get_member(make_node_id(2)).expect("member not found");
        assert_eq!(member.state, NodeState::Dead);
    }

    #[test]
    fn test_mark_dead_emits_event() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.mark_dead(make_node_id(2)).expect("mark_dead failed");

        let events = mgr.drain_events();
        let last_event = events.last().expect("no events");
        assert!(matches!(
            last_event,
            MembershipEvent::NodeDead { node_id } if *node_id == make_node_id(2)
        ));
    }

    #[test]
    fn test_heartbeat() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.heartbeat(make_node_id(2)).expect("heartbeat failed");

        let member = mgr.get_member(make_node_id(2)).expect("member not found");
        assert!(member.last_heartbeat.secs > 0);
    }

    #[test]
    fn test_alive_nodes() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");
        mgr.join(make_node_id(3), "192.168.1.3:8080".to_string())
            .expect("join failed");
        mgr.join(make_node_id(4), "192.168.1.4:8080".to_string())
            .expect("join failed");

        mgr.suspect(make_node_id(3)).expect("suspect failed");
        mgr.mark_dead(make_node_id(4)).expect("mark_dead failed");

        let alive = mgr.alive_nodes();
        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0], make_node_id(2));
    }

    #[test]
    fn test_all_members() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");
        mgr.join(make_node_id(3), "192.168.1.3:8080".to_string())
            .expect("join failed");

        let members = mgr.all_members();
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn test_drain_events() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        let events1 = mgr.drain_events();
        assert_eq!(events1.len(), 1);

        let events2 = mgr.drain_events();
        assert!(events2.is_empty());
    }

    #[test]
    fn test_multiple_state_transitions() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        mgr.suspect(make_node_id(2)).expect("suspect failed");
        assert_eq!(mgr.alive_count(), 0);

        mgr.confirm_alive(make_node_id(2))
            .expect("confirm_alive failed");
        assert_eq!(mgr.alive_count(), 1);

        mgr.mark_dead(make_node_id(2)).expect("mark_dead failed");
        assert_eq!(mgr.alive_count(), 0);
    }

    #[test]
    fn test_generation_increments() {
        let mgr = MembershipManager::new(make_node_id(1));

        mgr.join(make_node_id(2), "192.168.1.2:8080".to_string())
            .expect("join failed");

        let member1 = mgr.get_member(make_node_id(2)).expect("member not found");
        let gen1 = member1.generation;

        mgr.suspect(make_node_id(2)).expect("suspect failed");

        let member2 = mgr.get_member(make_node_id(2)).expect("member not found");
        assert!(member2.generation > gen1);
    }
}
