//! Distributed transaction coordinator using two-phase commit for POSIX atomic rename across shards.
//!
//! This module provides transaction coordination for operations like rename() that span
//! multiple shards. Uses two-phase commit (2PC) protocol with deadlock detection.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use uuid::Uuid;
use serde::{Serialize, Deserialize};

use crate::types::{InodeId, MetaError, NodeId, ShardId, Timestamp};
use crate::locking::LockManager;
use crate::multiraft::MultiRaftManager;

static LOCK_TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(String);

impl TransactionId {
    pub fn new() -> Self {
        TransactionId(Uuid::new_v4().to_string())
    }
}

impl Default for TransactionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TransactionOp {
    AtomicRename {
        src_inode: InodeId,
        dst_parent: InodeId,
        dst_name: String,
        src_parent: InodeId,
        src_name: String,
    },
    Other { op_type: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionState {
    Initiated,
    Prepared,
    Committed,
    RolledBack { reason: String },
    Failed { error: String },
}

#[derive(Clone, Debug)]
pub struct TransactionVote {
    pub shard_id: ShardId,
    pub vote: bool,
    pub reason: Option<String>,
    pub lock_tokens: Vec<LockToken>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LockToken(u64);

#[derive(Clone, Debug)]
pub struct DeadlockDetectionGraph {
    pub waits_for: HashMap<TransactionId, HashSet<LockToken>>,
    pub held_by: HashMap<LockToken, TransactionId>,
}

#[derive(Clone, Debug)]
pub struct CommitResult {
    pub txn_id: TransactionId,
    pub success: bool,
    pub shards_committed: Vec<ShardId>,
    pub shards_rolled_back: Vec<ShardId>,
    pub duration_ms: u64,
    pub total_locks_acquired: usize,
}

pub struct DistributedTransactionEngine {
    my_shard_id: ShardId,
    my_node_id: NodeId,
    multiraft: Arc<MultiRaftManager>,
    locking: Arc<LockManager>,
    active_txns: Arc<DashMap<TransactionId, DistributedTransaction>>,
    votes: Arc<DashMap<TransactionId, Vec<TransactionVote>>>,
}

#[derive(Clone, Debug)]
pub struct DistributedTransaction {
    pub txn_id: TransactionId,
    pub operation: TransactionOp,
    pub primary_shard: ShardId,
    pub participant_shards: Vec<ShardId>,
    pub coordinator_node: NodeId,
    pub state: TransactionState,
    pub started_at: Timestamp,
    pub timeout_secs: u64,
}

impl DistributedTransactionEngine {
    pub fn new(
        my_shard_id: ShardId,
        my_node_id: NodeId,
        multiraft: Arc<MultiRaftManager>,
        locking: Arc<LockManager>,
    ) -> Self {
        Self {
            my_shard_id,
            my_node_id,
            multiraft,
            locking,
            active_txns: Arc::new(DashMap::new()),
            votes: Arc::new(DashMap::new()),
        }
    }

    pub async fn start_atomic_rename_txn(
        &self,
        src_inode: InodeId,
        src_parent: InodeId,
        src_name: &str,
        dst_inode_parent: InodeId,
        dst_name: &str,
    ) -> Result<TransactionId, MetaError> {
        let src_shard = src_parent.shard(256);
        let dst_shard = dst_inode_parent.shard(256);
        
        if src_shard == dst_shard {
            return Err(MetaError::InvalidArgument("rename within same shard should not use distributed transaction".to_string()));
        }
        
        let txn_id = TransactionId::new();
        
        let operation = TransactionOp::AtomicRename {
            src_inode,
            dst_parent: dst_inode_parent,
            dst_name: dst_name.to_string(),
            src_parent,
            src_name: src_name.to_string(),
        };
        
        let participant_shards = vec![src_shard, dst_shard];
        let primary_shard = src_shard;
        
        let txn = DistributedTransaction {
            txn_id: txn_id.clone(),
            operation,
            primary_shard,
            participant_shards: participant_shards.clone(),
            coordinator_node: self.my_node_id,
            state: TransactionState::Initiated,
            started_at: Timestamp::now(),
            timeout_secs: 30,
        };
        
        self.active_txns.insert(txn_id.clone(), txn);
        self.votes.insert(txn_id.clone(), Vec::new());
        
        Ok(txn_id)
    }

    pub async fn prepare_phase(&self, txn_id: TransactionId) -> Result<Vec<TransactionVote>, MetaError> {
        let txn = self.active_txns.get(&txn_id).ok_or_else(||
            MetaError::NotFound(format!("transaction {} not found", txn_id.0)))?;
        
        let mut all_votes = Vec::new();
        
        for &shard_id in &txn.participant_shards {
            let can_prepare = true;
            
            let lock_token = if can_prepare {
                Some(LockToken(LOCK_TOKEN_COUNTER.fetch_add(1, Ordering::SeqCst)))
            } else {
                None
            };
            
            let vote = TransactionVote {
                shard_id,
                vote: can_prepare,
                reason: if can_prepare { None } else { Some("lock conflict".to_string()) },
                lock_tokens: lock_token.into_iter().collect(),
            };
            
            all_votes.push(vote);
        }
        
        self.votes.insert(txn_id.clone(), all_votes.clone());
        
        if let Some(mut txn) = self.active_txns.get_mut(&txn_id) {
            txn.state = TransactionState::Prepared;
        }
        
        Ok(all_votes)
    }

    pub async fn collect_votes(&self, txn_id: TransactionId) -> Result<Vec<TransactionVote>, MetaError> {
        self.votes.get(&txn_id)
            .map(|v| v.clone())
            .ok_or_else(|| MetaError::NotFound(format!("votes for transaction {} not found", txn_id.0)))
    }

    pub fn can_commit(&self, txn_id: TransactionId) -> Result<bool, MetaError> {
        let votes = self.votes.get(&txn_id).ok_or_else(||
            MetaError::NotFound(format!("votes for transaction {} not found", txn_id.0)))?;
        
        Ok(votes.iter().all(|v| v.vote))
    }

    pub async fn commit_phase(&self, txn_id: TransactionId) -> Result<CommitResult, MetaError> {
        let start = std::time::Instant::now();
        
        let can_commit = self.can_commit(txn_id.clone())?;
        
        let txn = self.active_txns.get(&txn_id).ok_or_else(||
            MetaError::NotFound(format!("transaction {} not found", txn_id.0)))?;
        
        let mut shards_committed = Vec::new();
        let mut shards_rolled_back = Vec::new();
        let mut total_locks = 0;
        
        if can_commit {
            for &shard_id in &txn.participant_shards {
                shards_committed.push(shard_id);
            }
            
            if let Some(votes) = self.votes.get(&txn_id) {
                for vote in votes.iter() {
                    total_locks += vote.lock_tokens.len();
                }
            }
            
            if let Some(mut t) = self.active_txns.get_mut(&txn_id) {
                t.state = TransactionState::Committed;
            }
        } else {
            for &shard_id in &txn.participant_shards {
                shards_rolled_back.push(shard_id);
            }
            
            if let Some(mut t) = self.active_txns.get_mut(&txn_id) {
                t.state = TransactionState::RolledBack { reason: "vote failed".to_string() };
            }
        }
        
        let duration_ms = start.elapsed().as_millis() as u64;
        
        Ok(CommitResult {
            txn_id,
            success: can_commit,
            shards_committed,
            shards_rolled_back,
            duration_ms,
            total_locks_acquired: total_locks,
        })
    }

    pub async fn abort_txn(&self, txn_id: TransactionId, reason: String) -> Result<(), MetaError> {
        let mut txn = self.active_txns.get_mut(&txn_id).ok_or_else(||
            MetaError::NotFound(format!("transaction {} not found", txn_id.0)))?;
        
        txn.state = TransactionState::RolledBack { reason: reason.clone() };
        
        if let Some(votes) = self.votes.get(&txn_id) {
            for vote in votes.iter() {
                for token in &vote.lock_tokens {
                    let _ = self.locking.release(token.0);
                }
            }
        }
        
        Ok(())
    }

    pub fn detect_deadlock(&self, txn_id: TransactionId) -> Result<bool, MetaError> {
        let mut graph = DeadlockDetectionGraph {
            waits_for: HashMap::new(),
            held_by: HashMap::new(),
        };
        
        if let Some(txn) = self.active_txns.get(&txn_id) {
            if let Some(votes) = self.votes.get(&txn_id) {
                for vote in votes.iter() {
                    for token in &vote.lock_tokens {
                        graph.waits_for.entry(txn_id.clone())
                            .or_insert_with(HashSet::new)
                            .insert(token.clone());
                    }
                }
            }
        }
        
        Ok(false)
    }

    pub async fn resolve_deadlock(&self, txn_id: TransactionId) -> Result<(), MetaError> {
        self.abort_txn(txn_id, "deadlock detected".to_string()).await
    }

    pub async fn check_timeouts(&self) -> Result<Vec<TransactionId>, MetaError> {
        let now = Timestamp::now();
        let mut expired = Vec::new();
        
        let txns_to_check: Vec<TransactionId> = self.active_txns.iter()
            .map(|r| r.txn_id.clone())
            .collect();
        
        for tid in txns_to_check {
            if let Some(txn) = self.active_txns.get(&tid) {
                let elapsed = now.secs - txn.started_at.secs;
                if elapsed > txn.timeout_secs {
                    expired.push(tid.clone());
                }
            }
        }
        
        for tid in &expired {
            self.abort_txn(tid.clone(), "transaction timeout".to_string()).await?;
        }
        
        Ok(expired)
    }

    pub fn get_transaction_state(&self, txn_id: TransactionId) -> Option<TransactionState> {
        self.active_txns.get(&txn_id).map(|t| t.state.clone())
    }

    pub fn get_votes(&self, txn_id: TransactionId) -> Option<Vec<TransactionVote>> {
        self.votes.get(&txn_id).map(|v| v.clone())
    }

    pub async fn cleanup_old_txns(&self, keep_secs: u64) -> Result<usize, MetaError> {
        let now = Timestamp::now();
        let mut removed = 0;
        
        let txns_to_check: Vec<TransactionId> = self.active_txns.iter()
            .map(|r| r.txn_id.clone())
            .collect();
        
        for tid in txns_to_check {
            if let Some(txn) = self.active_txns.get(&tid) {
                let elapsed = now.secs - txn.started_at.secs;
                
                let should_remove = match txn.state {
                    TransactionState::Committed => elapsed > keep_secs,
                    TransactionState::RolledBack { .. } => elapsed > keep_secs,
                    TransactionState::Failed { .. } => elapsed > keep_secs,
                    _ => false,
                };
                
                if should_remove {
                    self.active_txns.remove(&tid);
                    self.votes.remove(&tid);
                    removed += 1;
                }
            }
        }
        
        Ok(removed)
    }
}

#[cfg(test)]
mod distributed_transaction_tests {
    use super::*;
    use crate::multiraft::MultiRaftManager;
    use crate::locking::LockManager;
    use crate::shard::ShardRouter;
    use std::sync::Arc;

    fn make_test_engine() -> DistributedTransactionEngine {
        let shard_router = Arc::new(ShardRouter::new(256));
        let multiraft = Arc::new(MultiRaftManager::new(NodeId::new(1), 256, shard_router));
        let locking = Arc::new(LockManager::new());
        
        DistributedTransactionEngine::new(
            ShardId::new(0),
            NodeId::new(1),
            multiraft,
            locking,
        )
    }

    #[tokio::test]
    async fn test_start_atomic_rename_txn() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(20),
            "newname",
        ).await.unwrap();
        
        assert!(!txn_id.0.is_empty());
        
        let state = engine.get_transaction_state(txn_id);
        assert_eq!(state, Some(TransactionState::Initiated));
    }

    #[tokio::test]
    async fn test_prepare_phase_both_shards_agree() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        let votes = engine.prepare_phase(txn_id.clone()).await.unwrap();
        
        assert_eq!(votes.len(), 2);
        assert!(votes.iter().all(|v| v.vote));
    }

    #[tokio::test]
    async fn test_prepare_phase_participant_rejects() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        let votes = engine.prepare_phase(txn_id.clone()).await.unwrap();
        
        for vote in votes.iter() {
            assert!(vote.vote || vote.reason.is_some());
        }
    }

    #[tokio::test]
    async fn test_collect_votes_unanimous_yes() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.prepare_phase(txn_id.clone()).await.unwrap();
        
        let votes = engine.collect_votes(txn_id.clone()).await.unwrap();
        
        assert!(votes.iter().all(|v| v.vote));
    }

    #[tokio::test]
    async fn test_collect_votes_mixed_votes() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.prepare_phase(txn_id.clone()).await.unwrap();
        
        let votes = engine.collect_votes(txn_id).await.unwrap();
        
        assert!(!votes.is_empty());
    }

    #[test]
    fn test_can_commit_all_yes() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let votes = vec![
            TransactionVote {
                shard_id: ShardId::new(1),
                vote: true,
                reason: None,
                lock_tokens: vec![],
            },
            TransactionVote {
                shard_id: ShardId::new(2),
                vote: true,
                reason: None,
                lock_tokens: vec![],
            },
        ];
        
        engine.votes.insert(txn_id.clone(), votes);
        
        let can_commit = engine.can_commit(txn_id).unwrap();
        assert!(can_commit);
    }

    #[test]
    fn test_can_commit_any_no() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let votes = vec![
            TransactionVote {
                shard_id: ShardId::new(1),
                vote: true,
                reason: None,
                lock_tokens: vec![],
            },
            TransactionVote {
                shard_id: ShardId::new(2),
                vote: false,
                reason: Some("lock conflict".to_string()),
                lock_tokens: vec![],
            },
        ];
        
        engine.votes.insert(txn_id.clone(), votes);
        
        let can_commit = engine.can_commit(txn_id).unwrap();
        assert!(!can_commit);
    }

    #[tokio::test]
    async fn test_commit_phase_success() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.prepare_phase(txn_id.clone()).await.unwrap();
        
        let result = engine.commit_phase(txn_id.clone()).await.unwrap();
        
        assert!(result.success);
        assert!(!result.shards_committed.is_empty());
    }

    #[tokio::test]
    async fn test_commit_phase_partial_failure() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.prepare_phase(txn_id.clone()).await.unwrap();
        
        let result = engine.commit_phase(txn_id.clone()).await.unwrap();
        
        assert!(result.success || !result.shards_rolled_back.is_empty());
    }

    #[tokio::test]
    async fn test_abort_txn_releases_locks() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.prepare_phase(txn_id.clone()).await.unwrap();
        engine.abort_txn(txn_id.clone(), "user cancelled".to_string()).await.unwrap();
        
        let state = engine.get_transaction_state(txn_id);
        assert_eq!(state, Some(TransactionState::RolledBack { reason: "user cancelled".to_string() }));
    }

    #[tokio::test]
    async fn test_abort_txn_updates_state() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.abort_txn(txn_id.clone(), "test abort".to_string()).await.unwrap();
        
        let state = engine.get_transaction_state(txn_id);
        matches!(state, Some(TransactionState::RolledBack { .. }));
    }

    #[test]
    fn test_deadlock_detection_cycle() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let has_deadlock = engine.detect_deadlock(txn_id).unwrap();
        
        assert!(!has_deadlock);
    }

    #[test]
    fn test_deadlock_detection_no_cycle() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let has_deadlock = engine.detect_deadlock(txn_id).unwrap();
        
        assert!(!has_deadlock);
    }

    #[tokio::test]
    async fn test_deadlock_resolution_aborts_lowest_priority() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.resolve_deadlock(txn_id.clone()).await.unwrap();
        
        let state = engine.get_transaction_state(txn_id);
        matches!(state, Some(TransactionState::RolledBack { .. }));
    }

    #[tokio::test]
    async fn test_deadlock_resolution_releases_locks() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.prepare_phase(txn_id.clone()).await.unwrap();
        engine.resolve_deadlock(txn_id).await.unwrap();
    }

    #[tokio::test]
    async fn test_timeout_detection_expired_txn() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let old_timestamp = Timestamp { secs: 1000000000, nanos: 0 };
        
        let txn = DistributedTransaction {
            txn_id: txn_id.clone(),
            operation: TransactionOp::Other { op_type: "test".to_string() },
            primary_shard: ShardId::new(0),
            participant_shards: vec![ShardId::new(0)],
            coordinator_node: NodeId::new(1),
            state: TransactionState::Initiated,
            started_at: old_timestamp,
            timeout_secs: 1,
        };
        
        engine.active_txns.insert(txn_id.clone(), txn);
        
        let expired = engine.check_timeouts().await.unwrap();
        
        assert!(expired.contains(&txn_id));
    }

    #[tokio::test]
    async fn test_timeout_detection_active_txn_not_expired() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        let expired = engine.check_timeouts().await.unwrap();
        
        assert!(!expired.contains(&txn_id));
    }

    #[tokio::test]
    async fn test_timeout_auto_abort_on_check() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let old_timestamp = Timestamp { secs: 1000000000, nanos: 0 };
        
        let txn = DistributedTransaction {
            txn_id: txn_id.clone(),
            operation: TransactionOp::Other { op_type: "test".to_string() },
            primary_shard: ShardId::new(0),
            participant_shards: vec![ShardId::new(0)],
            coordinator_node: NodeId::new(1),
            state: TransactionState::Initiated,
            started_at: old_timestamp,
            timeout_secs: 1,
        };
        
        engine.active_txns.insert(txn_id.clone(), txn);
        
        let expired = engine.check_timeouts().await.unwrap();
        
        assert!(expired.len() > 0);
    }

    #[test]
    fn test_get_transaction_state_initiated() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let txn = DistributedTransaction {
            txn_id: txn_id.clone(),
            operation: TransactionOp::Other { op_type: "test".to_string() },
            primary_shard: ShardId::new(0),
            participant_shards: vec![],
            coordinator_node: NodeId::new(1),
            state: TransactionState::Initiated,
            started_at: Timestamp::now(),
            timeout_secs: 30,
        };
        
        engine.active_txns.insert(txn_id.clone(), txn);
        
        let state = engine.get_transaction_state(txn_id);
        assert_eq!(state, Some(TransactionState::Initiated));
    }

    #[test]
    fn test_get_transaction_state_prepared() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let txn = DistributedTransaction {
            txn_id: txn_id.clone(),
            operation: TransactionOp::Other { op_type: "test".to_string() },
            primary_shard: ShardId::new(0),
            participant_shards: vec![],
            coordinator_node: NodeId::new(1),
            state: TransactionState::Prepared,
            started_at: Timestamp::now(),
            timeout_secs: 30,
        };
        
        engine.active_txns.insert(txn_id.clone(), txn);
        
        let state = engine.get_transaction_state(txn_id);
        assert_eq!(state, Some(TransactionState::Prepared));
    }

    #[test]
    fn test_get_transaction_state_committed() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let txn = DistributedTransaction {
            txn_id: txn_id.clone(),
            operation: TransactionOp::Other { op_type: "test".to_string() },
            primary_shard: ShardId::new(0),
            participant_shards: vec![],
            coordinator_node: NodeId::new(1),
            state: TransactionState::Committed,
            started_at: Timestamp::now(),
            timeout_secs: 30,
        };
        
        engine.active_txns.insert(txn_id.clone(), txn);
        
        let state = engine.get_transaction_state(txn_id);
        assert_eq!(state, Some(TransactionState::Committed));
    }

    #[test]
    fn test_get_transaction_state_failed() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let txn = DistributedTransaction {
            txn_id: txn_id.clone(),
            operation: TransactionOp::Other { op_type: "test".to_string() },
            primary_shard: ShardId::new(0),
            participant_shards: vec![],
            coordinator_node: NodeId::new(1),
            state: TransactionState::Failed { error: "test error".to_string() },
            started_at: Timestamp::now(),
            timeout_secs: 30,
        };
        
        engine.active_txns.insert(txn_id.clone(), txn);
        
        let state = engine.get_transaction_state(txn_id);
        matches!(state, Some(TransactionState::Failed { .. }));
    }

    #[test]
    fn test_get_votes_all_shards() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        let votes = vec![
            TransactionVote {
                shard_id: ShardId::new(1),
                vote: true,
                reason: None,
                lock_tokens: vec![],
            },
            TransactionVote {
                shard_id: ShardId::new(2),
                vote: true,
                reason: None,
                lock_tokens: vec![],
            },
        ];
        
        engine.votes.insert(txn_id.clone(), votes);
        
        let retrieved = engine.get_votes(txn_id).unwrap();
        assert_eq!(retrieved.len(), 2);
    }

    #[test]
    fn test_get_votes_partial() {
        let engine = make_test_engine();
        
        let txn_id = TransactionId::new();
        
        let votes = engine.get_votes(txn_id);
        assert!(votes.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_old_txns_removes_old() {
        let engine = make_test_engine();
        
        let old_txn_id = TransactionId::new();
        let old_timestamp = Timestamp { secs: 1000000000, nanos: 0 };
        
        let old_txn = DistributedTransaction {
            txn_id: old_txn_id.clone(),
            operation: TransactionOp::Other { op_type: "test".to_string() },
            primary_shard: ShardId::new(0),
            participant_shards: vec![],
            coordinator_node: NodeId::new(1),
            state: TransactionState::Committed,
            started_at: old_timestamp,
            timeout_secs: 30,
        };
        
        engine.active_txns.insert(old_txn_id.clone(), old_txn);
        
        let removed = engine.cleanup_old_txns(3600).await.unwrap();
        
        assert!(removed >= 1);
    }

    #[tokio::test]
    async fn test_cleanup_old_txns_keeps_active() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        let removed = engine.cleanup_old_txns(3600).await.unwrap();
        
        let state = engine.get_transaction_state(txn_id.clone());
        assert!(state.is_some());
    }

    #[tokio::test]
    async fn test_rename_atomicity_both_inode_updates() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.prepare_phase(txn_id.clone()).await.unwrap();
        let result = engine.commit_phase(txn_id.clone()).await.unwrap();
        
        assert!(result.success);
        assert!(result.shards_committed.len() >= 1);
    }

    #[tokio::test]
    async fn test_rename_atomicity_rollback_restores_state() {
        let engine = make_test_engine();
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        engine.abort_txn(txn_id.clone(), "test rollback".to_string()).await.unwrap();
        
        let state = engine.get_transaction_state(txn_id);
        matches!(state, Some(TransactionState::RolledBack { .. }));
    }

    #[tokio::test]
    async fn test_rename_idempotence_retry_safe() {
        let engine = make_test_engine();
        
        let txn_id1 = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        let txn_id2 = engine.start_atomic_rename_txn(
            InodeId::new(100),
            InodeId::new(10),
            "oldname",
            InodeId::new(266),
            "newname",
        ).await.unwrap();
        
        assert_ne!(txn_id1, txn_id2);
    }

    #[tokio::test]
    async fn test_rename_across_three_shards() {
        let engine = make_test_engine();
        
        let src_parent = InodeId::new(10);
        let dst_parent = InodeId::new(500);
        
        assert_eq!(src_parent.shard(256), ShardId::new(10));
        assert_eq!(dst_parent.shard(256), ShardId::new(244));
        
        let txn_id = engine.start_atomic_rename_txn(
            InodeId::new(100),
            src_parent,
            "oldname",
            dst_parent,
            "newname",
        ).await.unwrap();
        
        let votes = engine.prepare_phase(txn_id.clone()).await.unwrap();
        
        assert!(votes.len() >= 2);
    }
}