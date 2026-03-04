//! Multicast group management for broadcasting control-plane messages.
//!
//! This module manages named multicast groups for broadcasting control-plane
//! messages (config updates, membership events, shard rebalancing notifications)
//! to sets of cluster nodes. Used by A2 (Metadata Service) for cluster-wide
//! config propagation and by A6 (Replication) for site membership announcements.
//!
//! This is a pure protocol-layer abstraction — does NOT do actual network I/O;
//! callers handle sending to returned member lists.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Unique group identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub String);

impl GroupId {
    /// Creates a new group ID from a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the group name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A node member of a multicast group.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupMember {
    /// Opaque node identifier (16 bytes).
    pub node_id: [u8; 16],
    /// Human-readable label (hostname or address).
    pub label: String,
    /// Timestamp when this member joined (ms since epoch).
    pub joined_at_ms: u64,
}

/// Membership event for group change notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupEvent {
    /// A node joined the group.
    Join {
        /// The group the node joined.
        group: GroupId,
        /// The member that joined.
        member: GroupMember,
    },
    /// A node left the group.
    Leave {
        /// The group the node left.
        group: GroupId,
        /// The node ID that left.
        node_id: [u8; 16],
    },
    /// Group was dissolved (all members removed).
    Dissolved {
        /// The group that was dissolved.
        group: GroupId,
    },
}

/// Result of a broadcast operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastResult {
    /// Number of members in the group at the time of broadcast.
    pub group_size: usize,
    /// Member node_ids that were targeted.
    pub targeted: Vec<[u8; 16]>,
}

/// Configuration for multicast groups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticastGroupConfig {
    /// Maximum number of groups (default: 256).
    pub max_groups: usize,
    /// Maximum members per group (default: 64).
    pub max_members_per_group: usize,
}

impl Default for MulticastGroupConfig {
    fn default() -> Self {
        Self {
            max_groups: 256,
            max_members_per_group: 64,
        }
    }
}

/// Error type for multicast group operations.
#[derive(Debug, Error)]
pub enum MulticastError {
    /// The specified group was not found.
    #[error("group {0:?} not found")]
    GroupNotFound(GroupId),
    /// A group with this ID already exists.
    #[error("group {0:?} already exists")]
    GroupAlreadyExists(GroupId),
    /// The member is already in the specified group.
    #[error("member already in group {0:?}")]
    AlreadyMember(GroupId),
    /// The member is not in the specified group.
    #[error("member not in group {0:?}")]
    NotMember(GroupId),
    /// The maximum number of groups has been reached.
    #[error("group limit reached ({0})")]
    GroupLimitReached(usize),
    /// The maximum number of members for a group has been reached.
    #[error("member limit reached for group {0:?} ({1})")]
    MemberLimitReached(GroupId, usize),
}

/// Statistics for multicast group operations.
pub struct MulticastGroupStats {
    /// Total groups created.
    pub groups_created: AtomicU64,
    /// Total groups dissolved.
    pub groups_dissolved: AtomicU64,
    /// Total join operations.
    pub joins: AtomicU64,
    /// Total leave operations.
    pub leaves: AtomicU64,
    /// Total broadcasts prepared.
    pub broadcasts_prepared: AtomicU64,
    /// Sum of group_size at each broadcast.
    pub total_broadcast_targets: AtomicU64,
}

impl MulticastGroupStats {
    /// Creates a new stats instance.
    pub fn new() -> Self {
        Self {
            groups_created: AtomicU64::new(0),
            groups_dissolved: AtomicU64::new(0),
            joins: AtomicU64::new(0),
            leaves: AtomicU64::new(0),
            broadcasts_prepared: AtomicU64::new(0),
            total_broadcast_targets: AtomicU64::new(0),
        }
    }

    /// Returns a snapshot of current statistics.
    pub fn snapshot(&self, active_groups: usize) -> MulticastGroupStatsSnapshot {
        MulticastGroupStatsSnapshot {
            groups_created: self.groups_created.load(Ordering::Relaxed),
            groups_dissolved: self.groups_dissolved.load(Ordering::Relaxed),
            joins: self.joins.load(Ordering::Relaxed),
            leaves: self.leaves.load(Ordering::Relaxed),
            broadcasts_prepared: self.broadcasts_prepared.load(Ordering::Relaxed),
            total_broadcast_targets: self.total_broadcast_targets.load(Ordering::Relaxed),
            active_groups,
        }
    }
}

impl Default for MulticastGroupStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of multicast group statistics.
#[derive(Debug, Clone)]
pub struct MulticastGroupStatsSnapshot {
    /// Total groups created.
    pub groups_created: u64,
    /// Total groups dissolved.
    pub groups_dissolved: u64,
    /// Total join operations.
    pub joins: u64,
    /// Total leave operations.
    pub leaves: u64,
    /// Total broadcasts prepared.
    pub broadcasts_prepared: u64,
    /// Sum of group_size at each broadcast.
    pub total_broadcast_targets: u64,
    /// Number of active groups.
    pub active_groups: usize,
}

/// Manager for named multicast groups.
pub struct MulticastGroupManager {
    config: MulticastGroupConfig,
    groups: RwLock<HashMap<GroupId, Vec<GroupMember>>>,
    stats: Arc<MulticastGroupStats>,
}

impl MulticastGroupManager {
    /// Creates a new multicast group manager with the given configuration.
    pub fn new(config: MulticastGroupConfig) -> Self {
        Self {
            config,
            groups: RwLock::new(HashMap::new()),
            stats: Arc::new(MulticastGroupStats::new()),
        }
    }

    /// Creates a new empty group.
    ///
    /// Returns an error if the group already exists or the group limit is reached.
    pub fn create_group(&self, group: GroupId) -> Result<(), MulticastError> {
        {
            let groups = self
                .groups
                .read()
                .map_err(|_| MulticastError::GroupNotFound(GroupId::new("poisoned lock")))?;
            if groups.contains_key(&group) {
                return Err(MulticastError::GroupAlreadyExists(group));
            }
            if groups.len() >= self.config.max_groups {
                return Err(MulticastError::GroupLimitReached(self.config.max_groups));
            }
        }

        let mut groups = self
            .groups
            .write()
            .map_err(|_| MulticastError::GroupNotFound(GroupId::new("poisoned lock")))?;

        if groups.contains_key(&group) {
            return Err(MulticastError::GroupAlreadyExists(group));
        }
        if groups.len() >= self.config.max_groups {
            return Err(MulticastError::GroupLimitReached(self.config.max_groups));
        }

        groups.insert(group.clone(), Vec::new());
        self.stats.groups_created.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Adds a member to a group.
    ///
    /// Returns an error if the group is not found, the member is already present,
    /// or the member limit is reached.
    pub fn join(&self, group: &GroupId, member: GroupMember) -> Result<GroupEvent, MulticastError> {
        {
            let groups = self
                .groups
                .read()
                .map_err(|_| MulticastError::GroupNotFound(GroupId::new("poisoned lock")))?;
            let members = groups
                .get(group)
                .ok_or_else(|| MulticastError::GroupNotFound(group.clone()))?;

            if members.iter().any(|m| m.node_id == member.node_id) {
                return Err(MulticastError::AlreadyMember(group.clone()));
            }
            if members.len() >= self.config.max_members_per_group {
                return Err(MulticastError::MemberLimitReached(
                    group.clone(),
                    self.config.max_members_per_group,
                ));
            }
        }

        let mut groups = self
            .groups
            .write()
            .map_err(|_| MulticastError::GroupNotFound(GroupId::new("poisoned lock")))?;
        let members = groups
            .get_mut(group)
            .ok_or_else(|| MulticastError::GroupNotFound(group.clone()))?;

        if members.iter().any(|m| m.node_id == member.node_id) {
            return Err(MulticastError::AlreadyMember(group.clone()));
        }
        if members.len() >= self.config.max_members_per_group {
            return Err(MulticastError::MemberLimitReached(
                group.clone(),
                self.config.max_members_per_group,
            ));
        }

        let event = GroupEvent::Join {
            group: group.clone(),
            member: member.clone(),
        };
        members.push(member);
        self.stats.joins.fetch_add(1, Ordering::Relaxed);
        Ok(event)
    }

    /// Removes a member from a group.
    ///
    /// Returns a Leave event on success.
    pub fn leave(&self, group: &GroupId, node_id: &[u8; 16]) -> Result<GroupEvent, MulticastError> {
        let mut groups = self
            .groups
            .write()
            .map_err(|_| MulticastError::GroupNotFound(GroupId::new("poisoned lock")))?;

        let members = groups
            .get_mut(group)
            .ok_or_else(|| MulticastError::GroupNotFound(group.clone()))?;

        let position = members.iter().position(|m| &m.node_id == node_id);
        let pos = match position {
            Some(p) => p,
            None => return Err(MulticastError::NotMember(group.clone())),
        };

        members.remove(pos);
        self.stats.leaves.fetch_add(1, Ordering::Relaxed);

        Ok(GroupEvent::Leave {
            group: group.clone(),
            node_id: *node_id,
        })
    }

    /// Dissolves a group — removes all members.
    ///
    /// Returns a Dissolved event on success.
    pub fn dissolve(&self, group: &GroupId) -> Result<GroupEvent, MulticastError> {
        let mut groups = self
            .groups
            .write()
            .map_err(|_| MulticastError::GroupNotFound(GroupId::new("poisoned lock")))?;

        if groups.remove(group).is_none() {
            return Err(MulticastError::GroupNotFound(group.clone()));
        }

        self.stats.groups_dissolved.fetch_add(1, Ordering::Relaxed);

        Ok(GroupEvent::Dissolved {
            group: group.clone(),
        })
    }

    /// Returns all members of a group.
    pub fn members(&self, group: &GroupId) -> Result<Vec<GroupMember>, MulticastError> {
        let groups = self
            .groups
            .read()
            .map_err(|_| MulticastError::GroupNotFound(GroupId::new("poisoned lock")))?;

        let members = groups
            .get(group)
            .ok_or_else(|| MulticastError::GroupNotFound(group.clone()))?;
        Ok(members.clone())
    }

    /// Checks if a node is a member of a group.
    pub fn is_member(&self, group: &GroupId, node_id: &[u8; 16]) -> bool {
        let groups = match self.groups.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        match groups.get(group) {
            Some(members) => members.iter().any(|m| &m.node_id == node_id),
            None => false,
        }
    }

    /// Prepares a broadcast to a group — returns BroadcastResult with targeted member ids.
    ///
    /// The caller is responsible for actually sending the message to each targeted node.
    pub fn prepare_broadcast(&self, group: &GroupId) -> Result<BroadcastResult, MulticastError> {
        let groups = self
            .groups
            .read()
            .map_err(|_| MulticastError::GroupNotFound(GroupId::new("poisoned lock")))?;

        let members = groups
            .get(group)
            .ok_or_else(|| MulticastError::GroupNotFound(group.clone()))?;

        let targeted: Vec<[u8; 16]> = members.iter().map(|m| m.node_id).collect();
        let group_size = targeted.len();

        self.stats
            .broadcasts_prepared
            .fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_broadcast_targets
            .fetch_add(group_size as u64, Ordering::Relaxed);

        Ok(BroadcastResult {
            group_size,
            targeted,
        })
    }

    /// Lists all group IDs.
    pub fn list_groups(&self) -> Vec<GroupId> {
        let groups = match self.groups.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        groups.keys().cloned().collect()
    }

    /// Returns the number of groups currently registered.
    pub fn group_count(&self) -> usize {
        let groups = match self.groups.read() {
            Ok(g) => g,
            Err(_) => return 0,
        };
        groups.len()
    }

    /// Returns a reference to the stats.
    pub fn stats(&self) -> Arc<MulticastGroupStats> {
        Arc::clone(&self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_id(id: u8) -> [u8; 16] {
        let mut arr = [0u8; 16];
        arr[0] = id;
        arr
    }

    fn make_member(id: u8, label: &str, joined_at: u64) -> GroupMember {
        GroupMember {
            node_id: make_node_id(id),
            label: label.to_string(),
            joined_at_ms: joined_at,
        }
    }

    #[test]
    fn test_group_id_new() {
        let id = GroupId::new("test-group");
        assert_eq!(id.as_str(), "test-group");
    }

    #[test]
    fn test_create_group() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");

        manager.create_group(group.clone()).unwrap();

        let groups = manager.list_groups();
        assert_eq!(groups.len(), 1);
        assert!(groups.contains(&group));
    }

    #[test]
    fn test_create_group_duplicate() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");

        manager.create_group(group.clone()).unwrap();

        let result = manager.create_group(group.clone());
        assert!(matches!(result, Err(MulticastError::GroupAlreadyExists(g)) if g == group));
    }

    #[test]
    fn test_create_group_limit() {
        let config = MulticastGroupConfig {
            max_groups: 2,
            max_members_per_group: 10,
        };
        let manager = MulticastGroupManager::new(config);

        manager.create_group(GroupId::new("g1")).unwrap();
        manager.create_group(GroupId::new("g2")).unwrap();

        let result = manager.create_group(GroupId::new("g3"));
        assert!(matches!(result, Err(MulticastError::GroupLimitReached(2))));
    }

    #[test]
    fn test_join_success() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        let member = make_member(1, "node1", 1000);
        let event = manager.join(&group, member.clone()).unwrap();

        match event {
            GroupEvent::Join {
                group: g,
                member: m,
            } => {
                assert_eq!(g, group);
                assert_eq!(m.node_id, member.node_id);
                assert_eq!(m.label, member.label);
            }
            _ => panic!("Expected Join event"),
        }
    }

    #[test]
    fn test_join_unknown_group() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("nonexistent");

        let result = manager.join(&group, make_member(1, "node1", 1000));
        assert!(matches!(result, Err(MulticastError::GroupNotFound(g)) if g == group));
    }

    #[test]
    fn test_join_duplicate_member() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        let member = make_member(1, "node1", 1000);
        manager.join(&group, member.clone()).unwrap();

        let result = manager.join(&group, member);
        assert!(matches!(result, Err(MulticastError::AlreadyMember(g)) if g == group));
    }

    #[test]
    fn test_join_member_limit() {
        let config = MulticastGroupConfig {
            max_groups: 10,
            max_members_per_group: 2,
        };
        let manager = MulticastGroupManager::new(config);
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        manager.join(&group, make_member(1, "node1", 1000)).unwrap();
        manager.join(&group, make_member(2, "node2", 2000)).unwrap();

        let result = manager.join(&group, make_member(3, "node3", 3000));
        assert!(matches!(result, Err(MulticastError::MemberLimitReached(g, 2)) if g == group));
    }

    #[test]
    fn test_leave_success() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        let member = make_member(1, "node1", 1000);
        manager.join(&group, member.clone()).unwrap();

        let event = manager.leave(&group, &member.node_id).unwrap();

        match event {
            GroupEvent::Leave { group: g, node_id } => {
                assert_eq!(g, group);
                assert_eq!(node_id, member.node_id);
            }
            _ => panic!("Expected Leave event"),
        }

        let members = manager.members(&group).unwrap();
        assert!(members.is_empty());
    }

    #[test]
    fn test_leave_not_member() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        let result = manager.leave(&group, &make_node_id(1));
        assert!(matches!(result, Err(MulticastError::NotMember(g)) if g == group));
    }

    #[test]
    fn test_dissolve_removes_all() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        manager.join(&group, make_member(1, "node1", 1000)).unwrap();
        manager.join(&group, make_member(2, "node2", 2000)).unwrap();
        manager.join(&group, make_member(3, "node3", 3000)).unwrap();

        let event = manager.dissolve(&group).unwrap();

        match event {
            GroupEvent::Dissolved { group: g } => {
                assert_eq!(g, group);
            }
            _ => panic!("Expected Dissolved event"),
        }

        let result = manager.members(&group);
        assert!(matches!(result, Err(MulticastError::GroupNotFound(g)) if g == group));
    }

    #[test]
    fn test_dissolve_unknown_group() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("nonexistent");

        let result = manager.dissolve(&group);
        assert!(matches!(result, Err(MulticastError::GroupNotFound(g)) if g == group));
    }

    #[test]
    fn test_is_member_true() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        let member = make_member(1, "node1", 1000);
        manager.join(&group, member.clone()).unwrap();

        assert!(manager.is_member(&group, &member.node_id));
    }

    #[test]
    fn test_is_member_false() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        let result = manager.is_member(&group, &make_node_id(99));
        assert!(!result);
    }

    #[test]
    fn test_prepare_broadcast_returns_all_members() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("config-updates");
        manager.create_group(group.clone()).unwrap();

        manager.join(&group, make_member(1, "node1", 1000)).unwrap();
        manager.join(&group, make_member(2, "node2", 2000)).unwrap();
        manager.join(&group, make_member(3, "node3", 3000)).unwrap();

        let result = manager.prepare_broadcast(&group).unwrap();

        assert_eq!(result.group_size, 3);
        assert_eq!(result.targeted.len(), 3);
        assert!(result.targeted.contains(&make_node_id(1)));
        assert!(result.targeted.contains(&make_node_id(2)));
        assert!(result.targeted.contains(&make_node_id(3)));
    }

    #[test]
    fn test_prepare_broadcast_empty_group() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());
        let group = GroupId::new("empty-group");
        manager.create_group(group.clone()).unwrap();

        let result = manager.prepare_broadcast(&group).unwrap();

        assert_eq!(result.group_size, 0);
        assert_eq!(result.targeted.len(), 0);
    }

    #[test]
    fn test_stats_counts() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());

        let g1 = GroupId::new("g1");
        let g2 = GroupId::new("g2");
        manager.create_group(g1.clone()).unwrap();
        manager.create_group(g2.clone()).unwrap();

        manager.join(&g1, make_member(1, "n1", 1000)).unwrap();
        manager.join(&g1, make_member(2, "n2", 2000)).unwrap();
        manager.leave(&g1, &make_node_id(1)).unwrap();

        manager.dissolve(&g2).unwrap();

        manager.prepare_broadcast(&g1).unwrap();

        let snapshot = manager.stats().snapshot(manager.group_count());

        assert_eq!(snapshot.groups_created, 2);
        assert_eq!(snapshot.groups_dissolved, 1);
        assert_eq!(snapshot.joins, 2);
        assert_eq!(snapshot.leaves, 1);
        assert_eq!(snapshot.broadcasts_prepared, 1);
        assert_eq!(snapshot.total_broadcast_targets, 1);
        assert_eq!(snapshot.active_groups, 1);
    }

    #[test]
    fn test_multiple_groups_independent() {
        let manager = MulticastGroupManager::new(MulticastGroupConfig::default());

        let g1 = GroupId::new("config-updates");
        let g2 = GroupId::new("repl-events");
        manager.create_group(g1.clone()).unwrap();
        manager.create_group(g2.clone()).unwrap();

        manager.join(&g1, make_member(1, "node1", 1000)).unwrap();
        manager.join(&g1, make_member(2, "node2", 2000)).unwrap();
        manager.join(&g2, make_member(3, "node3", 3000)).unwrap();

        let m1 = manager.members(&g1).unwrap();
        let m2 = manager.members(&g2).unwrap();

        assert_eq!(m1.len(), 2);
        assert_eq!(m2.len(), 1);

        assert!(m1.iter().any(|m| m.node_id == make_node_id(1)));
        assert!(m1.iter().any(|m| m.node_id == make_node_id(2)));
        assert!(!m1.iter().any(|m| m.node_id == make_node_id(3)));

        assert!(!m2.iter().any(|m| m.node_id == make_node_id(1)));
        assert!(m2.iter().any(|m| m.node_id == make_node_id(3)));
    }

    #[test]
    fn test_group_member_serde() {
        let member = GroupMember {
            node_id: make_node_id(42),
            label: "test-node".to_string(),
            joined_at_ms: 1234567890,
        };

        let json = serde_json::to_string(&member).unwrap();
        let decoded: GroupMember = serde_json::from_str(&json).unwrap();

        assert_eq!(member.node_id, decoded.node_id);
        assert_eq!(member.label, decoded.label);
        assert_eq!(member.joined_at_ms, decoded.joined_at_ms);
    }

    #[test]
    fn test_group_event_serde() {
        let event = GroupEvent::Join {
            group: GroupId::new("test"),
            member: make_member(1, "node1", 1000),
        };

        let json = serde_json::to_string(&event).unwrap();
        let decoded: GroupEvent = serde_json::from_str(&json).unwrap();

        match decoded {
            GroupEvent::Join { group, member } => {
                assert_eq!(group.as_str(), "test");
                assert_eq!(member.node_id, make_node_id(1));
            }
            _ => panic!("Expected Join event"),
        }
    }

    #[test]
    fn test_broadcast_result_serde() {
        let result = BroadcastResult {
            group_size: 5,
            targeted: vec![make_node_id(1), make_node_id(2), make_node_id(3)],
        };

        let json = serde_json::to_string(&result).unwrap();
        let decoded: BroadcastResult = serde_json::from_str(&json).unwrap();

        assert_eq!(result.group_size, decoded.group_size);
        assert_eq!(result.targeted.len(), decoded.targeted.len());
    }

    #[test]
    fn test_group_id_equality() {
        let id1 = GroupId::new("test");
        let id2 = GroupId::new("test");
        let id3 = GroupId::new("other");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_group_id_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(GroupId::new("a"));
        set.insert(GroupId::new("b"));
        set.insert(GroupId::new("a"));

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_default_config() {
        let config = MulticastGroupConfig::default();
        assert_eq!(config.max_groups, 256);
        assert_eq!(config.max_members_per_group, 64);
    }
}
