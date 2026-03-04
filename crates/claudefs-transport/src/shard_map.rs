//! Virtual shard → node mapping for distributed routing.
//!
//! Implements the virtual shard → node routing table from D4 (Multi-Raft topology).
//! ClaudeFS uses 256 virtual shards (configurable). Each shard has a Raft group with
//! a current leader and 2 followers. This module tracks the current shard-to-node mapping
//! and handles shard rebalancing events when nodes join/leave.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Virtual shard identifier (0..num_shards).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VirtualShard(pub u32);

/// Role of a node in a shard's Raft group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardRole {
    /// Leader of the Raft group.
    Leader,
    /// Follower in the Raft group.
    Follower,
    /// Learner catching up to the Raft group.
    Learner,
}

/// A node's assignment within a shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardReplica {
    /// Node identifier (16-byte UUID).
    pub node_id: [u8; 16],
    /// Role of this node in the shard.
    pub role: ShardRole,
    /// When this replica was assigned (ms since epoch).
    pub assigned_at_ms: u64,
}

/// Current state of a virtual shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardInfo {
    /// Shard identifier.
    pub shard: VirtualShard,
    /// Replica assignments for this shard.
    pub replicas: Vec<ShardReplica>,
}

impl ShardInfo {
    /// Get the current leader node, or None if no leader.
    pub fn leader(&self) -> Option<&ShardReplica> {
        self.replicas.iter().find(|r| r.role == ShardRole::Leader)
    }

    /// Get follower nodes.
    pub fn followers(&self) -> Vec<&ShardReplica> {
        self.replicas
            .iter()
            .filter(|r| r.role == ShardRole::Follower)
            .collect()
    }

    /// Whether this shard has quorum (majority of replicas assigned).
    pub fn has_quorum(&self, replication_factor: usize) -> bool {
        let assigned = self.replicas.len();
        let majority = (replication_factor / 2) + 1;
        assigned >= majority
    }
}

/// Configuration for the shard map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardMapConfig {
    /// Total number of virtual shards (default: 256, immutable after cluster creation).
    pub num_shards: u32,
    /// Replication factor per shard (default: 3 for Raft).
    pub replication_factor: usize,
}

impl Default for ShardMapConfig {
    fn default() -> Self {
        Self {
            num_shards: 256,
            replication_factor: 3,
        }
    }
}

/// Error for shard map operations.
#[derive(Debug, Error)]
pub enum ShardMapError {
    /// Shard not found.
    #[error("shard {0:?} not found")]
    ShardNotFound(VirtualShard),
    /// Node not assigned to shard.
    #[error("node {0:?} not assigned to shard {1:?}")]
    NodeNotInShard([u8; 16], VirtualShard),
    /// Shard out of range.
    #[error("shard {0:?} out of range (max {1})")]
    ShardOutOfRange(u32, u32),
}

/// Statistics for shard map operations.
pub struct ShardMapStats {
    /// Total leader updates.
    pub leader_updates: AtomicU64,
    /// Total replica assignments.
    pub replica_assignments: AtomicU64,
    /// Total node removals.
    pub node_removals: AtomicU64,
    /// Total key lookups.
    pub key_lookups: AtomicU64,
}

impl ShardMapStats {
    /// Create new shard map statistics.
    pub fn new() -> Self {
        Self {
            leader_updates: AtomicU64::new(0),
            replica_assignments: AtomicU64::new(0),
            node_removals: AtomicU64::new(0),
            key_lookups: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of current statistics.
    pub fn snapshot(
        &self,
        total_shards: u32,
        with_leader: usize,
        without_quorum: usize,
    ) -> ShardMapStatsSnapshot {
        ShardMapStatsSnapshot {
            leader_updates: self.leader_updates.load(Ordering::Relaxed),
            replica_assignments: self.replica_assignments.load(Ordering::Relaxed),
            node_removals: self.node_removals.load(Ordering::Relaxed),
            key_lookups: self.key_lookups.load(Ordering::Relaxed),
            total_shards,
            shards_with_leader: with_leader,
            shards_without_quorum: without_quorum,
        }
    }
}

impl Default for ShardMapStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of shard map statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct ShardMapStatsSnapshot {
    /// Total leader updates.
    pub leader_updates: u64,
    /// Total replica assignments.
    pub replica_assignments: u64,
    /// Total node removals.
    pub node_removals: u64,
    /// Total key lookups.
    pub key_lookups: u64,
    /// Total number of shards.
    pub total_shards: u32,
    /// Number of shards with an elected leader.
    pub shards_with_leader: usize,
    /// Number of shards without quorum.
    pub shards_without_quorum: usize,
}

/// Maps virtual shards to their Raft replicas.
pub struct ShardMap {
    config: ShardMapConfig,
    shards: RwLock<HashMap<VirtualShard, ShardInfo>>,
    stats: Arc<ShardMapStats>,
}

impl ShardMap {
    /// Create a new shard map.
    pub fn new(config: ShardMapConfig) -> Self {
        Self {
            config,
            shards: RwLock::new(HashMap::new()),
            stats: Arc::new(ShardMapStats::new()),
        }
    }

    /// Compute the shard for an inode or key (hash(key) % num_shards).
    pub fn shard_for_key(&self, key: u64) -> VirtualShard {
        self.stats.key_lookups.fetch_add(1, Ordering::Relaxed);
        VirtualShard((key % self.config.num_shards as u64) as u32)
    }

    /// Get shard info (replicas + leader).
    pub fn get_shard(&self, shard: VirtualShard) -> Result<ShardInfo, ShardMapError> {
        if shard.0 >= self.config.num_shards {
            return Err(ShardMapError::ShardOutOfRange(
                shard.0,
                self.config.num_shards,
            ));
        }

        match self.shards.read() {
            Ok(shards) => shards
                .get(&shard)
                .cloned()
                .ok_or(ShardMapError::ShardNotFound(shard)),
            Err(_) => Err(ShardMapError::ShardNotFound(shard)),
        }
    }

    /// Get the leader node for a shard. Returns None if no leader elected.
    pub fn leader_for_shard(&self, shard: VirtualShard) -> Result<Option<[u8; 16]>, ShardMapError> {
        let info = self.get_shard(shard)?;
        Ok(info.leader().map(|r| r.node_id))
    }

    /// Get the leader for the shard responsible for a key.
    pub fn leader_for_key(&self, key: u64) -> Option<[u8; 16]> {
        let shard = self.shard_for_key(key);
        self.leader_for_shard(shard).ok().flatten()
    }

    /// Update the leader for a shard (Raft leader election result).
    pub fn update_leader(
        &self,
        shard: VirtualShard,
        new_leader: [u8; 16],
        now_ms: u64,
    ) -> Result<(), ShardMapError> {
        if shard.0 >= self.config.num_shards {
            return Err(ShardMapError::ShardOutOfRange(
                shard.0,
                self.config.num_shards,
            ));
        }

        match self.shards.write() {
            Ok(mut shards) => {
                if let Some(info) = shards.get_mut(&shard) {
                    let mut found = false;
                    for replica in info.replicas.iter_mut() {
                        if replica.node_id == new_leader {
                            replica.role = ShardRole::Leader;
                            replica.assigned_at_ms = now_ms;
                            found = true;
                        } else if replica.role == ShardRole::Leader {
                            replica.role = ShardRole::Follower;
                        }
                    }
                    if !found {
                        info.replicas.push(ShardReplica {
                            node_id: new_leader,
                            role: ShardRole::Leader,
                            assigned_at_ms: now_ms,
                        });
                    }
                    self.stats.leader_updates.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                } else {
                    let mut info = ShardInfo {
                        shard,
                        replicas: Vec::new(),
                    };
                    info.replicas.push(ShardReplica {
                        node_id: new_leader,
                        role: ShardRole::Leader,
                        assigned_at_ms: now_ms,
                    });
                    shards.insert(shard, info);
                    self.stats.leader_updates.fetch_add(1, Ordering::Relaxed);
                    self.stats
                        .replica_assignments
                        .fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
            }
            Err(_) => Err(ShardMapError::ShardNotFound(shard)),
        }
    }

    /// Assign a replica set to a shard (initial cluster setup or rebalancing).
    pub fn assign_replicas(
        &self,
        shard: VirtualShard,
        replicas: Vec<ShardReplica>,
    ) -> Result<(), ShardMapError> {
        if shard.0 >= self.config.num_shards {
            return Err(ShardMapError::ShardOutOfRange(
                shard.0,
                self.config.num_shards,
            ));
        }

        match self.shards.write() {
            Ok(mut shards) => {
                let info = ShardInfo { shard, replicas };
                shards.insert(shard, info);
                self.stats
                    .replica_assignments
                    .fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(_) => Err(ShardMapError::ShardNotFound(shard)),
        }
    }

    /// Remove a node from all shards (node leaving). Returns list of affected shards.
    pub fn remove_node(&self, node_id: &[u8; 16]) -> Vec<VirtualShard> {
        let mut affected = Vec::new();

        match self.shards.write() {
            Ok(mut shards) => {
                for (shard, info) in shards.iter_mut() {
                    let initial_len = info.replicas.len();
                    info.replicas.retain(|r| r.node_id != *node_id);
                    if info.replicas.len() < initial_len {
                        affected.push(*shard);
                    }
                }
                if !affected.is_empty() {
                    self.stats.node_removals.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(_) => {}
        }

        affected
    }

    /// Get all shards where a specific node is a replica.
    pub fn shards_for_node(&self, node_id: &[u8; 16]) -> Vec<VirtualShard> {
        match self.shards.read() {
            Ok(shards) => shards
                .iter()
                .filter(|(_, info)| info.replicas.iter().any(|r| r.node_id == *node_id))
                .map(|(shard, _)| *shard)
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Number of shards with an elected leader.
    pub fn shards_with_leader(&self) -> usize {
        match self.shards.read() {
            Ok(shards) => shards
                .values()
                .filter(|info| info.leader().is_some())
                .count(),
            Err(_) => 0,
        }
    }

    /// Number of shards without quorum.
    pub fn shards_without_quorum(&self) -> usize {
        match self.shards.read() {
            Ok(shards) => shards
                .values()
                .filter(|info| !info.has_quorum(self.config.replication_factor))
                .count(),
            Err(_) => 0,
        }
    }

    /// Get statistics.
    pub fn stats(&self) -> Arc<ShardMapStats> {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_id(seed: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[0] = seed;
        id
    }

    fn make_replica(seed: u8, role: ShardRole, now_ms: u64) -> ShardReplica {
        ShardReplica {
            node_id: make_node_id(seed),
            role,
            assigned_at_ms: now_ms,
        }
    }

    #[test]
    fn test_shard_for_key_range() {
        let map = ShardMap::new(ShardMapConfig::default());
        for key in 0..1000 {
            let shard = map.shard_for_key(key);
            assert!(shard.0 < 256);
        }
    }

    #[test]
    fn test_shard_for_key_deterministic() {
        let map = ShardMap::new(ShardMapConfig::default());
        let s1 = map.shard_for_key(12345);
        let s2 = map.shard_for_key(12345);
        let s3 = map.shard_for_key(12345);
        assert_eq!(s1, s2);
        assert_eq!(s2, s3);
    }

    #[test]
    fn test_assign_replicas() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
            make_replica(3, ShardRole::Follower, 0),
        ];

        let result = map.assign_replicas(VirtualShard(0), replicas);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_shard() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];
        map.assign_replicas(VirtualShard(10), replicas).unwrap();

        let info = map.get_shard(VirtualShard(10)).unwrap();
        assert_eq!(info.shard, VirtualShard(10));
        assert_eq!(info.replicas.len(), 2);
    }

    #[test]
    fn test_get_shard_not_found() {
        let map = ShardMap::new(ShardMapConfig::default());
        let result = map.get_shard(VirtualShard(0));
        assert!(matches!(result, Err(ShardMapError::ShardNotFound(_))));
    }

    #[test]
    fn test_shard_out_of_range() {
        let map = ShardMap::new(ShardMapConfig::default());
        let result = map.get_shard(VirtualShard(300));
        assert!(matches!(
            result,
            Err(ShardMapError::ShardOutOfRange(300, 256))
        ));
    }

    #[test]
    fn test_leader_for_shard() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];
        map.assign_replicas(VirtualShard(0), replicas).unwrap();

        let leader = map.leader_for_shard(VirtualShard(0)).unwrap();
        assert_eq!(leader, Some(make_node_id(1)));
    }

    #[test]
    fn test_leader_for_shard_no_leader() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas = vec![
            make_replica(1, ShardRole::Follower, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];
        map.assign_replicas(VirtualShard(0), replicas).unwrap();

        let leader = map.leader_for_shard(VirtualShard(0)).unwrap();
        assert!(leader.is_none());
    }

    #[test]
    fn test_update_leader() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];
        map.assign_replicas(VirtualShard(0), replicas).unwrap();

        map.update_leader(VirtualShard(0), make_node_id(2), 100)
            .unwrap();

        let info = map.get_shard(VirtualShard(0)).unwrap();
        let leader = info.leader().unwrap();
        assert_eq!(leader.node_id, make_node_id(2));
        assert_eq!(leader.assigned_at_ms, 100);
    }

    #[test]
    fn test_remove_node_from_shards() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas1 = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];
        let replicas2 = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(3, ShardRole::Follower, 0),
        ];
        map.assign_replicas(VirtualShard(0), replicas1).unwrap();
        map.assign_replicas(VirtualShard(1), replicas2).unwrap();

        let affected = map.remove_node(&make_node_id(1));
        assert_eq!(affected.len(), 2);

        let info0 = map.get_shard(VirtualShard(0)).unwrap();
        assert_eq!(info0.replicas.len(), 1);

        let info1 = map.get_shard(VirtualShard(1)).unwrap();
        assert_eq!(info1.replicas.len(), 1);
    }

    #[test]
    fn test_shards_for_node() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas1 = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];
        let replicas2 = vec![
            make_replica(3, ShardRole::Leader, 0),
            make_replica(1, ShardRole::Follower, 0),
        ];
        let replicas3 = vec![
            make_replica(4, ShardRole::Leader, 0),
            make_replica(5, ShardRole::Follower, 0),
        ];
        map.assign_replicas(VirtualShard(0), replicas1).unwrap();
        map.assign_replicas(VirtualShard(1), replicas2).unwrap();
        map.assign_replicas(VirtualShard(2), replicas3).unwrap();

        let shards = map.shards_for_node(&make_node_id(1));
        assert_eq!(shards.len(), 2);
        assert!(shards.contains(&VirtualShard(0)));
        assert!(shards.contains(&VirtualShard(1)));

        let shards2 = map.shards_for_node(&make_node_id(4));
        assert_eq!(shards2.len(), 1);
        assert!(shards2.contains(&VirtualShard(2)));
    }

    #[test]
    fn test_shards_with_leader_count() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas1 = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];
        let replicas2 = vec![
            make_replica(3, ShardRole::Follower, 0),
            make_replica(4, ShardRole::Follower, 0),
        ];
        map.assign_replicas(VirtualShard(0), replicas1).unwrap();
        map.assign_replicas(VirtualShard(1), replicas2).unwrap();

        assert_eq!(map.shards_with_leader(), 1);
    }

    #[test]
    fn test_shards_without_quorum_count() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas1 = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
            make_replica(3, ShardRole::Follower, 0),
        ];
        let replicas2 = vec![make_replica(4, ShardRole::Follower, 0)];
        map.assign_replicas(VirtualShard(0), replicas1).unwrap();
        map.assign_replicas(VirtualShard(1), replicas2).unwrap();

        assert_eq!(map.shards_without_quorum(), 1);
    }

    #[test]
    fn test_leader_for_key() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];

        let shard = map.shard_for_key(12345);
        map.assign_replicas(shard, replicas).unwrap();

        let leader = map.leader_for_key(12345);
        assert_eq!(leader, Some(make_node_id(1)));
    }

    #[test]
    fn test_has_quorum_true() {
        let info = ShardInfo {
            shard: VirtualShard(0),
            replicas: vec![
                make_replica(1, ShardRole::Leader, 0),
                make_replica(2, ShardRole::Follower, 0),
                make_replica(3, ShardRole::Follower, 0),
            ],
        };

        assert!(info.has_quorum(3));
    }

    #[test]
    fn test_has_quorum_false() {
        let info = ShardInfo {
            shard: VirtualShard(0),
            replicas: vec![make_replica(1, ShardRole::Leader, 0)],
        };

        assert!(!info.has_quorum(3));
    }

    #[test]
    fn test_stats_counts() {
        let map = ShardMap::new(ShardMapConfig::default());
        let stats = map.stats();

        let replicas = vec![
            make_replica(1, ShardRole::Leader, 0),
            make_replica(2, ShardRole::Follower, 0),
        ];
        map.assign_replicas(VirtualShard(0), replicas.clone())
            .unwrap();
        map.update_leader(VirtualShard(0), make_node_id(2), 100)
            .unwrap();

        let _ = map.shard_for_key(12345);
        let _ = map.shard_for_key(67890);

        let snapshot = stats.snapshot(256, map.shards_with_leader(), map.shards_without_quorum());
        assert_eq!(snapshot.replica_assignments, 1);
        assert_eq!(snapshot.leader_updates, 1);
        assert_eq!(snapshot.key_lookups, 2);
    }

    #[test]
    fn test_shard_info_followers() {
        let info = ShardInfo {
            shard: VirtualShard(0),
            replicas: vec![
                make_replica(1, ShardRole::Leader, 0),
                make_replica(2, ShardRole::Follower, 0),
                make_replica(3, ShardRole::Follower, 0),
            ],
        };

        let followers = info.followers();
        assert_eq!(followers.len(), 2);
    }

    #[test]
    fn test_config_default() {
        let config = ShardMapConfig::default();
        assert_eq!(config.num_shards, 256);
        assert_eq!(config.replication_factor, 3);
    }

    #[test]
    fn test_assign_replicas_out_of_range() {
        let map = ShardMap::new(ShardMapConfig::default());
        let replicas = vec![make_replica(1, ShardRole::Leader, 0)];
        let result = map.assign_replicas(VirtualShard(300), replicas);
        assert!(matches!(
            result,
            Err(ShardMapError::ShardOutOfRange(300, 256))
        ));
    }

    #[test]
    fn test_update_leader_creates_shard() {
        let map = ShardMap::new(ShardMapConfig::default());
        map.update_leader(VirtualShard(0), make_node_id(1), 0)
            .unwrap();

        let info = map.get_shard(VirtualShard(0)).unwrap();
        assert_eq!(info.replicas.len(), 1);
        assert_eq!(info.leader().unwrap().node_id, make_node_id(1));
    }

    #[test]
    fn test_virtual_shard_equality() {
        let s1 = VirtualShard(10);
        let s2 = VirtualShard(10);
        let s3 = VirtualShard(20);
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn test_shard_role_equality() {
        assert_eq!(ShardRole::Leader, ShardRole::Leader);
        assert_eq!(ShardRole::Follower, ShardRole::Follower);
        assert_ne!(ShardRole::Leader, ShardRole::Follower);
    }

    #[test]
    fn test_has_quorum_two_of_three() {
        let info = ShardInfo {
            shard: VirtualShard(0),
            replicas: vec![
                make_replica(1, ShardRole::Leader, 0),
                make_replica(2, ShardRole::Follower, 0),
            ],
        };

        assert!(info.has_quorum(3));
    }
}
