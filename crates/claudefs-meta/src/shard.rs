//! Shard routing for distributed metadata.
//!
//! This module provides shard routing functionality that maps inodes to shards
//! and shards to nodes in a distributed metadata cluster.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

/// Information about a single virtual shard.
#[derive(Clone, Debug)]
pub struct ShardInfo {
    /// The unique identifier for this shard.
    pub shard_id: ShardId,
    /// The Raft group members (replica nodes) for this shard.
    pub replicas: Vec<NodeId>,
    /// The current Raft leader for this shard (None if no leader elected).
    pub leader: Option<NodeId>,
    /// Raft term of the current leader.
    pub leader_term: Term,
}

impl ShardInfo {
    /// Creates a new ShardInfo with the given id and replicas.
    fn new(shard_id: ShardId, replicas: Vec<NodeId>) -> Self {
        Self {
            shard_id,
            replicas,
            leader: None,
            leader_term: Term::new(0),
        }
    }
}

/// Routes inodes to shards and shards to nodes.
///
/// Provides thread-safe access to shard topology and leader information
/// for the distributed metadata service.
pub struct ShardRouter {
    /// Total number of shards in the system.
    num_shards: u16,
    /// Map from shard ID to shard info.
    shards: RwLock<HashMap<ShardId, ShardInfo>>,
    /// Map from node ID to which shards it hosts.
    node_shards: RwLock<HashMap<NodeId, Vec<ShardId>>>,
}

impl ShardRouter {
    /// Creates a new router with the given number of shards.
    pub fn new(num_shards: u16) -> Self {
        Self {
            num_shards,
            shards: RwLock::new(HashMap::new()),
            node_shards: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the shard that owns the given inode.
    ///
    /// Uses consistent hashing based on inode number modulo shard count.
    pub fn shard_for_inode(&self, ino: InodeId) -> ShardId {
        ino.shard(self.num_shards)
    }

    /// Returns the leader node for the given shard.
    ///
    /// Returns `MetaError::NotLeader` if no leader has been elected for this shard.
    pub fn leader_for_shard(&self, shard_id: ShardId) -> Result<NodeId, MetaError> {
        let shards = self
            .shards
            .read()
            .map_err(|e| MetaError::KvError(format!("failed to acquire read lock: {}", e)))?;

        let shard_info = shards
            .get(&shard_id)
            .ok_or_else(|| MetaError::KvError(format!("shard {} not found", shard_id)))?;

        shard_info
            .leader
            .ok_or(MetaError::NotLeader { leader_hint: None })
    }

    /// Returns the leader node for the shard that owns the given inode.
    ///
    /// This is a convenience method that combines `shard_for_inode` and `leader_for_shard`.
    pub fn leader_for_inode(&self, ino: InodeId) -> Result<NodeId, MetaError> {
        let shard_id = self.shard_for_inode(ino);
        self.leader_for_shard(shard_id)
    }

    /// Returns all replica nodes for the given shard.
    pub fn replicas_for_shard(&self, shard_id: ShardId) -> Result<Vec<NodeId>, MetaError> {
        let shards = self
            .shards
            .read()
            .map_err(|e| MetaError::KvError(format!("failed to acquire read lock: {}", e)))?;

        let shard_info = shards
            .get(&shard_id)
            .ok_or_else(|| MetaError::KvError(format!("shard {} not found", shard_id)))?;

        Ok(shard_info.replicas.clone())
    }

    /// Returns all shards hosted on the given node.
    pub fn shards_on_node(&self, node_id: NodeId) -> Vec<ShardId> {
        let node_shards = self.node_shards.read().ok();
        node_shards
            .and_then(|m| m.get(&node_id).cloned())
            .unwrap_or_default()
    }

    /// Updates the shard info with new state.
    ///
    /// This can be used to update leader, replicas, or both.
    pub fn update_shard_info(&self, info: ShardInfo) -> Result<(), MetaError> {
        let mut shards = self
            .shards
            .write()
            .map_err(|e| MetaError::KvError(format!("failed to acquire write lock: {}", e)))?;

        let shard_id = info.shard_id;

        // Update node_shards mapping based on new replicas
        let mut node_shards = self
            .node_shards
            .write()
            .map_err(|e| MetaError::KvError(format!("failed to acquire write lock: {}", e)))?;

        if let Some(old_info) = shards.get(&shard_id) {
            // Remove shard from old nodes
            for &node in &old_info.replicas {
                if let Some(shards_on_node) = node_shards.get_mut(&node) {
                    shards_on_node.retain(|&s| s != shard_id);
                }
            }
        }

        // Add shard to new nodes
        for &node in &info.replicas {
            node_shards
                .entry(node)
                .or_insert_with(Vec::new)
                .push(shard_id);
        }

        shards.insert(shard_id, info);
        Ok(())
    }

    /// Assigns a shard to a set of replica nodes.
    ///
    /// This is the initial assignment of a shard to nodes.
    pub fn assign_shard(&self, shard_id: ShardId, replicas: Vec<NodeId>) -> Result<(), MetaError> {
        if replicas.is_empty() {
            return Err(MetaError::KvError(
                "shard must have at least one replica".to_string(),
            ));
        }

        let info = ShardInfo::new(shard_id, replicas);
        self.update_shard_info(info)
    }

    /// Updates the leader for a shard after an election.
    pub fn update_leader(
        &self,
        shard_id: ShardId,
        leader: NodeId,
        term: Term,
    ) -> Result<(), MetaError> {
        let mut shards = self
            .shards
            .write()
            .map_err(|e| MetaError::KvError(format!("failed to acquire write lock: {}", e)))?;

        let shard_info = shards
            .get_mut(&shard_id)
            .ok_or_else(|| MetaError::KvError(format!("shard {} not found", shard_id)))?;

        if !shard_info.replicas.contains(&leader) {
            return Err(MetaError::KvError(format!(
                "node {} is not a replica of shard {}",
                leader, shard_id
            )));
        }

        shard_info.leader = Some(leader);
        shard_info.leader_term = term;

        tracing::debug!(
            "shard {} leader updated to node {} term {}",
            shard_id,
            leader,
            term.as_u64()
        );

        Ok(())
    }

    /// Removes a node from the cluster.
    ///
    /// Returns the list of shards that were affected and need rebalancing.
    pub fn remove_node(&self, node_id: NodeId) -> Result<Vec<ShardId>, MetaError> {
        let mut affected_shards = Vec::new();

        // Remove from node_shards mapping
        {
            let mut node_shards = self
                .node_shards
                .write()
                .map_err(|e| MetaError::KvError(format!("failed to acquire write lock: {}", e)))?;
            node_shards.remove(&node_id);
        }

        // Remove node from all shard replicas
        {
            let mut shards = self
                .shards
                .write()
                .map_err(|e| MetaError::KvError(format!("failed to acquire write lock: {}", e)))?;

            for (shard_id, shard_info) in shards.iter_mut() {
                if shard_info.replicas.contains(&node_id) {
                    shard_info.replicas.retain(|&n| n != node_id);

                    // Clear leader if it was the removed node
                    if shard_info.leader == Some(node_id) {
                        shard_info.leader = None;
                        shard_info.leader_term = Term::new(0);
                    }

                    // Mark as affected if shard lost all replicas (needs rebalancing)
                    if shard_info.replicas.is_empty() {
                        affected_shards.push(*shard_id);
                    }
                }
            }
        }

        tracing::debug!(
            "removed node {}, {} shards affected",
            node_id,
            affected_shards.len()
        );
        Ok(affected_shards)
    }

    /// Returns information about all shards.
    pub fn all_shards(&self) -> Vec<ShardInfo> {
        let shards = self.shards.read().ok();
        shards
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }
}

/// Distributes shards across nodes for the metadata cluster.
pub struct ShardAssigner;

impl ShardAssigner {
    /// Distributes shards evenly across the given nodes.
    ///
    /// Each shard gets `replication_factor` replicas on different nodes.
    /// Returns a Vec of (ShardId, Vec<NodeId>) assignments.
    ///
    /// # Errors
    ///
    /// Returns an error if there aren't enough nodes for the replication factor.
    pub fn distribute(
        num_shards: u16,
        nodes: &[NodeId],
        replication_factor: usize,
    ) -> Result<Vec<(ShardId, Vec<NodeId>)>, MetaError> {
        if nodes.is_empty() {
            return Err(MetaError::KvError(
                "no nodes available for shard distribution".to_string(),
            ));
        }

        if replication_factor == 0 {
            return Err(MetaError::KvError(
                "replication factor must be greater than 0".to_string(),
            ));
        }

        if nodes.len() < replication_factor {
            return Err(MetaError::KvError(format!(
                "not enough nodes ({}) for replication factor {}",
                nodes.len(),
                replication_factor
            )));
        }

        // Calculate target shards per node for balanced distribution
        let total_replicas = num_shards as usize * replication_factor;
        let shards_per_node = total_replicas / nodes.len();
        let remainder = total_replicas % nodes.len();

        // Track how many shards each node should get
        let mut node_quota: Vec<usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, _)| shards_per_node + if i < remainder { 1 } else { 0 })
            .collect();

        // Assign replicas round-robin style for balance
        let mut assignments: Vec<(ShardId, Vec<NodeId>)> = Vec::with_capacity(num_shards as usize);

        for shard_idx in 0..num_shards as usize {
            let shard_id = ShardId::new(shard_idx as u16);
            let mut replicas = Vec::with_capacity(replication_factor);

            // Assign first replica to node based on shard index for better locality
            let first_node_idx = shard_idx % nodes.len();
            replicas.push(nodes[first_node_idx]);
            node_quota[first_node_idx] -= 1;

            // Assign remaining replicas to nodes with available quota
            let mut attempts = 0;
            while replicas.len() < replication_factor && attempts < nodes.len() * 2 {
                // Find node with highest quota that isn't already assigned to this shard
                let mut best_node_idx = None;
                let mut best_quota = 0isize;

                for (i, &node) in nodes.iter().enumerate() {
                    if replicas.contains(&node) {
                        continue;
                    }
                    let quota = node_quota[i] as isize;
                    if quota > best_quota {
                        best_quota = quota;
                        best_node_idx = Some(i);
                    }
                }

                if let Some(node_idx) = best_node_idx {
                    if node_quota[node_idx] > 0 {
                        replicas.push(nodes[node_idx]);
                        node_quota[node_idx] -= 1;
                    }
                }

                attempts += 1;
            }

            // If we couldn't fill all replicas via quota, just pick any available nodes
            while replicas.len() < replication_factor {
                for (i, &node) in nodes.iter().enumerate() {
                    if !replicas.contains(&node) {
                        replicas.push(nodes[i]);
                        if replicas.len() >= replication_factor {
                            break;
                        }
                    }
                }
                // Prevent infinite loop
                if replicas.len() >= replication_factor {
                    break;
                }
            }

            if replicas.len() != replication_factor {
                return Err(MetaError::KvError(format!(
                    "failed to assign {} replicas for shard {}",
                    replication_factor, shard_id
                )));
            }

            assignments.push((shard_id, replicas));
        }

        tracing::debug!(
            "distributed {} shards across {} nodes with factor {}",
            num_shards,
            nodes.len(),
            replication_factor
        );

        Ok(assignments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_for_inode() {
        let router = ShardRouter::new(256);

        // Same inode always maps to same shard
        let ino1 = InodeId::new(100);
        let ino2 = InodeId::new(100);
        assert_eq!(router.shard_for_inode(ino1), router.shard_for_inode(ino2));

        // Different inodes can map to same shard (expected with modulo)
        let ino3 = InodeId::new(100);
        let ino4 = InodeId::new(356); // 356 % 256 = 100
        assert_eq!(router.shard_for_inode(ino3), router.shard_for_inode(ino4));
    }

    #[test]
    fn test_assign_and_lookup() {
        let router = ShardRouter::new(256);

        // Assign a shard to nodes
        let shard_id = ShardId::new(0);
        let replicas = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        router.assign_shard(shard_id, replicas.clone()).unwrap();

        // Verify replicas are returned
        let found_replicas = router.replicas_for_shard(shard_id).unwrap();
        assert_eq!(found_replicas, replicas);

        // Update leader
        router
            .update_leader(shard_id, NodeId::new(2), Term::new(1))
            .unwrap();

        // Verify leader lookup works
        let leader = router.leader_for_shard(shard_id).unwrap();
        assert_eq!(leader, NodeId::new(2));
    }

    #[test]
    fn test_update_leader() {
        let router = ShardRouter::new(256);

        let shard_id = ShardId::new(5);
        let replicas = vec![NodeId::new(10), NodeId::new(20), NodeId::new(30)];
        router.assign_shard(shard_id, replicas).unwrap();

        // Initially no leader
        let result = router.leader_for_shard(shard_id);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MetaError::NotLeader { .. }));

        // Update leader
        router
            .update_leader(shard_id, NodeId::new(20), Term::new(5))
            .unwrap();

        // Verify leader is now returned
        let leader = router.leader_for_shard(shard_id).unwrap();
        assert_eq!(leader, NodeId::new(20));
    }

    #[test]
    fn test_remove_node() {
        let router = ShardRouter::new(4);

        // Assign shards to nodes - shard3 has only node 1 as replica
        let shard1 = ShardId::new(1);
        let shard2 = ShardId::new(2);
        let shard3 = ShardId::new(3);
        router
            .assign_shard(shard1, vec![NodeId::new(1), NodeId::new(2)])
            .unwrap();
        router
            .assign_shard(shard2, vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();
        router.assign_shard(shard3, vec![NodeId::new(1)]).unwrap();

        // Set leaders
        router
            .update_leader(shard1, NodeId::new(2), Term::new(1))
            .unwrap();
        router
            .update_leader(shard3, NodeId::new(1), Term::new(1))
            .unwrap();

        // Remove node 1
        let affected = router.remove_node(NodeId::new(1)).unwrap();

        // shard3 should be affected (it had only node 1, now has no replicas)
        assert!(affected.contains(&shard3));
        assert!(!affected.contains(&shard1));
        assert!(!affected.contains(&shard2));

        // Leader for shard1 should still work
        let leader = router.leader_for_shard(shard1).unwrap();
        assert_eq!(leader, NodeId::new(2));

        // Leader for shard3 should be cleared (shard lost)
        let result = router.leader_for_shard(shard3);
        assert!(result.is_err());
    }

    #[test]
    fn test_shards_on_node() {
        let router = ShardRouter::new(256);

        // Assign multiple shards to same node
        router
            .assign_shard(ShardId::new(0), vec![NodeId::new(1), NodeId::new(2)])
            .unwrap();
        router
            .assign_shard(ShardId::new(1), vec![NodeId::new(1), NodeId::new(3)])
            .unwrap();
        router
            .assign_shard(ShardId::new(2), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();

        // Check shards on node 1
        let shards = router.shards_on_node(NodeId::new(1));
        assert_eq!(shards.len(), 2);
        assert!(shards.contains(&ShardId::new(0)));
        assert!(shards.contains(&ShardId::new(1)));

        // Check shards on node that doesn't exist
        let empty = router.shards_on_node(NodeId::new(99));
        assert!(empty.is_empty());
    }

    #[test]
    fn test_distribute_shards_balanced() {
        let nodes = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];

        let assignments = ShardAssigner::distribute(9, &nodes, 3).unwrap();

        // Each node should have exactly 9 shards (3 replicas * 9 shards / 3 nodes)
        let mut node_shard_count = HashMap::new();
        for (_, replicas) in &assignments {
            for &node in replicas {
                *node_shard_count.entry(node).or_insert(0) += 1;
            }
        }

        for &node in &nodes {
            assert_eq!(
                node_shard_count[&node], 9,
                "node {} should have 9 replicas",
                node
            );
        }
    }

    #[test]
    fn test_distribute_insufficient_nodes() {
        let nodes = vec![NodeId::new(1), NodeId::new(2)];

        let result = ShardAssigner::distribute(10, &nodes, 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_distribute_replication_factor() {
        let nodes = vec![
            NodeId::new(1),
            NodeId::new(2),
            NodeId::new(3),
            NodeId::new(4),
        ];

        let assignments = ShardAssigner::distribute(8, &nodes, 2).unwrap();

        // Each shard should have exactly 2 replicas
        for (shard_id, replicas) in &assignments {
            assert_eq!(
                replicas.len(),
                2,
                "shard {} should have 2 replicas but has {}",
                shard_id,
                replicas.len()
            );

            // No duplicate nodes in replicas
            let unique: std::collections::HashSet<_> = replicas.iter().collect();
            assert_eq!(
                unique.len(),
                replicas.len(),
                "shard {} has duplicate replicas",
                shard_id
            );
        }
    }

    #[test]
    fn test_all_shards() {
        let router = ShardRouter::new(256);

        // Initially empty
        let all = router.all_shards();
        assert!(all.is_empty());

        // Add some shards
        router
            .assign_shard(ShardId::new(5), vec![NodeId::new(1)])
            .unwrap();
        router
            .assign_shard(ShardId::new(10), vec![NodeId::new(2)])
            .unwrap();

        let all = router.all_shards();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_leader_for_inode_no_leader() {
        let router = ShardRouter::new(4);

        // Inode 0 maps to shard 0 (0 % 4 = 0)
        router
            .assign_shard(ShardId::new(0), vec![NodeId::new(1), NodeId::new(2)])
            .unwrap();

        // leader_for_inode should return NotLeader when no leader
        let ino = InodeId::new(0);
        let result = router.leader_for_inode(ino);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MetaError::NotLeader { .. }));
    }

    #[test]
    fn test_leader_for_inode_with_leader() {
        let router = ShardRouter::new(4);

        // Inode 0 maps to shard 0 (0 % 4 = 0)
        router
            .assign_shard(ShardId::new(0), vec![NodeId::new(1), NodeId::new(2)])
            .unwrap();
        router
            .update_leader(ShardId::new(0), NodeId::new(1), Term::new(1))
            .unwrap();

        // leader_for_inode should return the leader
        let ino = InodeId::new(0);
        let leader = router.leader_for_inode(ino).unwrap();
        assert_eq!(leader, NodeId::new(1));
    }
}
