//! Multi-Raft group manager for distributed metadata.
//!
//! This module manages one RaftNode per virtual shard on this node.
//! Per decision D4: 256 shards, each is an independent Raft group with 3 replicas.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::consensus::{RaftConfig, RaftNode};
use crate::shard::ShardRouter;
use crate::types::*;

/// Manages multiple Raft groups — one per shard on this node.
pub struct MultiRaftManager {
    /// This node's ID.
    node_id: NodeId,
    /// Number of virtual shards in the cluster (used in Phase 3 for shard iteration).
    #[allow(dead_code)]
    num_shards: u16,
    /// Per-shard Raft nodes (only for shards assigned to this node).
    groups: RwLock<HashMap<ShardId, RaftNode>>,
    /// Shard router for inode-to-shard mapping.
    router: Arc<ShardRouter>,
}

impl MultiRaftManager {
    /// Create a new Multi-Raft manager for this node.
    pub fn new(node_id: NodeId, num_shards: u16, router: Arc<ShardRouter>) -> Self {
        Self {
            node_id,
            num_shards,
            groups: RwLock::new(HashMap::new()),
            router,
        }
    }

    /// Initialize a Raft group for a shard on this node.
    /// `peers` are the other replica nodes for this shard (not including self).
    pub fn init_group(&self, shard_id: ShardId, peers: Vec<NodeId>) -> Result<(), MetaError> {
        let config = RaftConfig {
            node_id: self.node_id,
            peers,
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
        };

        let raft_node = RaftNode::new(config);

        let mut groups = self
            .groups
            .write()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire write lock: {}", e)))?;

        groups.insert(shard_id, raft_node);

        tracing::debug!(
            node_id = %self.node_id,
            shard_id = %shard_id,
            "initialized Raft group for shard"
        );

        Ok(())
    }

    /// Propose a metadata operation. Routes to the correct shard's Raft group.
    /// Returns the shard-targeted AppendEntries messages for followers.
    pub fn propose(
        &self,
        ino: InodeId,
        op: MetaOp,
    ) -> Result<Vec<(NodeId, ShardId, RaftMessage)>, MetaError> {
        let shard_id = self.shard_for_inode(ino);

        let mut groups = self
            .groups
            .write()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire write lock: {}", e)))?;

        let raft_node = groups.get_mut(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        let messages = raft_node.propose(op)?;

        let result: Vec<(NodeId, ShardId, RaftMessage)> = messages
            .into_iter()
            .map(|(node_id, msg)| (node_id, shard_id, msg))
            .collect();

        Ok(result)
    }

    /// Start an election for a specific shard's Raft group.
    /// Returns the RequestVote message to broadcast to shard peers.
    pub fn start_election(&self, shard_id: ShardId) -> Result<(ShardId, RaftMessage), MetaError> {
        let mut groups = self
            .groups
            .write()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire write lock: {}", e)))?;

        let raft_node = groups.get_mut(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        let msg = raft_node.start_election();

        Ok((shard_id, msg))
    }

    /// Handle a RequestVote message for a specific shard.
    pub fn handle_request_vote(
        &self,
        shard_id: ShardId,
        msg: &RaftMessage,
    ) -> Result<RaftMessage, MetaError> {
        let mut groups = self
            .groups
            .write()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire write lock: {}", e)))?;

        let raft_node = groups.get_mut(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        let response = raft_node.handle_request_vote(msg);

        Ok(response)
    }

    /// Handle a vote response for a specific shard.
    /// Returns heartbeat messages if this node became leader.
    pub fn handle_vote_response(
        &self,
        shard_id: ShardId,
        from: NodeId,
        msg: &RaftMessage,
    ) -> Result<Option<Vec<(NodeId, RaftMessage)>>, MetaError> {
        let mut groups = self
            .groups
            .write()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire write lock: {}", e)))?;

        let raft_node = groups.get_mut(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        let result = raft_node.handle_vote_response(from, msg);

        Ok(result)
    }

    /// Handle an AppendEntries message for a specific shard.
    pub fn handle_append_entries(
        &self,
        shard_id: ShardId,
        msg: &RaftMessage,
    ) -> Result<RaftMessage, MetaError> {
        let mut groups = self
            .groups
            .write()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire write lock: {}", e)))?;

        let raft_node = groups.get_mut(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        let response = raft_node.handle_append_entries(msg);

        Ok(response)
    }

    /// Handle an AppendEntries response for a specific shard.
    pub fn handle_append_response(
        &self,
        shard_id: ShardId,
        from: NodeId,
        msg: &RaftMessage,
    ) -> Result<Vec<LogEntry>, MetaError> {
        let mut groups = self
            .groups
            .write()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire write lock: {}", e)))?;

        let raft_node = groups.get_mut(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        let entries = raft_node.handle_append_response(from, msg);

        Ok(entries)
    }

    /// Take committed entries for a shard.
    pub fn take_committed(&self, shard_id: ShardId) -> Result<Vec<LogEntry>, MetaError> {
        let mut groups = self
            .groups
            .write()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire write lock: {}", e)))?;

        let raft_node = groups.get_mut(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        let entries = raft_node.take_committed_entries();

        Ok(entries)
    }

    /// Get the current term for a shard's Raft group.
    pub fn current_term(&self, shard_id: ShardId) -> Result<Term, MetaError> {
        let groups = self
            .groups
            .read()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire read lock: {}", e)))?;

        let raft_node = groups.get(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        Ok(raft_node.current_term())
    }

    /// Get the state of a shard's Raft group.
    pub fn state(&self, shard_id: ShardId) -> Result<RaftState, MetaError> {
        let groups = self
            .groups
            .read()
            .map_err(|e| MetaError::RaftError(format!("failed to acquire read lock: {}", e)))?;

        let raft_node = groups.get(&shard_id).ok_or_else(|| {
            MetaError::RaftError(format!("shard {} not managed by this node", shard_id))
        })?;

        Ok(raft_node.state())
    }

    /// Get all shard IDs managed by this node.
    pub fn managed_shards(&self) -> Vec<ShardId> {
        let groups = self.groups.read().ok();
        groups
            .map(|g| g.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Check if this node is leader for the given shard.
    pub fn is_leader(&self, shard_id: ShardId) -> bool {
        let groups = match self.groups.read() {
            Ok(g) => g,
            Err(_) => return false,
        };
        match groups.get(&shard_id) {
            Some(node) => node.state() == RaftState::Leader,
            None => false,
        }
    }

    /// Get the shard that owns a given inode.
    pub fn shard_for_inode(&self, ino: InodeId) -> ShardId {
        self.router.shard_for_inode(ino)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shard::ShardRouter;

    fn make_manager() -> (MultiRaftManager, Arc<ShardRouter>) {
        let router = Arc::new(ShardRouter::new(256));
        let mgr = MultiRaftManager::new(NodeId::new(1), 256, router.clone());
        (mgr, router)
    }

    #[test]
    fn test_init_group() {
        let (mgr, _) = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();
        assert_eq!(mgr.managed_shards().len(), 1);
        assert!(mgr.managed_shards().contains(&ShardId::new(0)));
    }

    #[test]
    fn test_init_multiple_groups() {
        let (mgr, _) = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();
        mgr.init_group(ShardId::new(5), vec![NodeId::new(2), NodeId::new(4)])
            .unwrap();
        assert_eq!(mgr.managed_shards().len(), 2);
    }

    #[test]
    fn test_shard_for_inode() {
        let (mgr, _) = make_manager();
        let shard = mgr.shard_for_inode(InodeId::new(42));
        assert_eq!(shard, InodeId::new(42).shard(256));
    }

    #[test]
    fn test_start_election() {
        let (mgr, _) = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();
        let (shard_id, msg) = mgr.start_election(ShardId::new(0)).unwrap();
        assert_eq!(shard_id, ShardId::new(0));
        assert!(matches!(msg, RaftMessage::RequestVote { .. }));
    }

    #[test]
    fn test_election_unknown_shard() {
        let (mgr, _) = make_manager();
        assert!(mgr.start_election(ShardId::new(99)).is_err());
    }

    #[test]
    fn test_propose_routes_to_correct_shard() {
        let (mgr, _) = make_manager();
        // Shard for inode 0 with 256 shards = shard 0
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();

        // Make node 1 leader for shard 0
        mgr.start_election(ShardId::new(0)).unwrap();
        let vote_resp = RaftMessage::RequestVoteResponse {
            term: Term::new(1),
            vote_granted: true,
        };
        mgr.handle_vote_response(ShardId::new(0), NodeId::new(2), &vote_resp)
            .unwrap();

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(0), 0, 0, 0o644, 1),
        };
        let messages = mgr.propose(InodeId::new(0), op).unwrap();
        assert!(!messages.is_empty());
        // All messages target shard 0
        for (_, shard, _) in &messages {
            assert_eq!(*shard, ShardId::new(0));
        }
    }

    #[test]
    fn test_propose_not_leader() {
        let (mgr, _) = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();
        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(0), 0, 0, 0o644, 1),
        };
        let result = mgr.propose(InodeId::new(0), op);
        assert!(matches!(result, Err(MetaError::NotLeader { .. })));
    }

    #[test]
    fn test_is_leader() {
        let (mgr, _) = make_manager();
        mgr.init_group(ShardId::new(0), vec![NodeId::new(2), NodeId::new(3)])
            .unwrap();
        assert!(!mgr.is_leader(ShardId::new(0)));

        mgr.start_election(ShardId::new(0)).unwrap();
        let vote_resp = RaftMessage::RequestVoteResponse {
            term: Term::new(1),
            vote_granted: true,
        };
        mgr.handle_vote_response(ShardId::new(0), NodeId::new(2), &vote_resp)
            .unwrap();

        assert!(mgr.is_leader(ShardId::new(0)));
    }
}
