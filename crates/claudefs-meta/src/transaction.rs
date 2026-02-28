//! Distributed transaction coordinator using two-phase commit for cross-shard operations.
//!
//! This module provides transaction coordination for operations like rename() and link()
//! that span multiple shards. The Raft log serves as the durable store for the 2PC protocol.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::types::*;

/// Unique identifier for a distributed transaction.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(u64);

impl TransactionId {
    /// Creates a new transaction ID from a raw u64 value.
    pub fn new(id: u64) -> Self {
        TransactionId(id)
    }

    /// Returns the raw u64 value of this transaction ID.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "txn-{}", self.0)
    }
}

/// State of a distributed transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionState {
    /// Transaction is being prepared (waiting for participant votes).
    Preparing,
    /// All participants voted to commit.
    Prepared,
    /// Commit decision made, propagating to participants.
    Committing,
    /// Transaction committed successfully.
    Committed,
    /// Abort decision made, propagating to participants.
    Aborting,
    /// Transaction aborted.
    Aborted,
}

/// A participant in a distributed transaction (identified by shard).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionParticipant {
    /// The shard ID of this participant.
    pub shard_id: ShardId,
    /// The vote cast by this participant: None = not yet voted, Some(true) = commit, Some(false) = abort.
    pub voted: Option<bool>,
}

impl TransactionParticipant {
    /// Creates a new transaction participant with no vote recorded.
    pub fn new(shard_id: ShardId) -> Self {
        Self {
            shard_id,
            voted: None,
        }
    }

    /// Records a commit vote for this participant.
    pub fn vote_commit(&mut self) {
        self.voted = Some(true);
    }

    /// Records an abort vote for this participant.
    pub fn vote_abort(&mut self) {
        self.voted = Some(false);
    }
}

/// A distributed transaction record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique transaction identifier.
    pub id: TransactionId,
    /// Current state of the transaction.
    pub state: TransactionState,
    /// The shard coordinating this transaction.
    pub coordinator_shard: ShardId,
    /// Participants in this transaction.
    pub participants: Vec<TransactionParticipant>,
    /// The metadata operation being performed.
    pub operation: MetaOp,
    /// Timestamp when the transaction was created.
    pub created_at: Timestamp,
}

impl Transaction {
    /// Creates a new transaction in the Preparing state.
    pub fn new(
        id: TransactionId,
        coordinator_shard: ShardId,
        participants: Vec<ShardId>,
        operation: MetaOp,
    ) -> Self {
        Self {
            id,
            state: TransactionState::Preparing,
            coordinator_shard,
            participants: participants
                .into_iter()
                .map(TransactionParticipant::new)
                .collect(),
            operation,
            created_at: Timestamp::now(),
        }
    }

    /// Returns true if all participants have voted to commit.
    pub fn all_voted_commit(&self) -> bool {
        self.participants.iter().all(|p| p.voted == Some(true))
    }

    /// Returns true if any participant has voted to abort.
    pub fn any_voted_abort(&self) -> bool {
        self.participants.iter().any(|p| p.voted == Some(false))
    }

    /// Returns true if all participants have voted.
    pub fn all_voted(&self) -> bool {
        self.participants.iter().all(|p| p.voted.is_some())
    }
}

/// Manages distributed transactions using the two-phase commit protocol.
pub struct TransactionManager {
    /// Counter for generating unique transaction IDs.
    next_txn_id: AtomicU64,
    /// Active transactions indexed by transaction ID.
    active_transactions: RwLock<HashMap<TransactionId, Transaction>>,
    /// Timeout in seconds after which transactions are considered timed out.
    timeout_secs: u64,
}

impl TransactionManager {
    /// Creates a new transaction manager with the specified timeout.
    ///
    /// # Arguments
    /// * `timeout_secs` - Number of seconds before a transaction is considered timed out
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            next_txn_id: AtomicU64::new(1),
            active_transactions: RwLock::new(HashMap::new()),
            timeout_secs,
        }
    }

    /// Begins a new transaction and returns its ID.
    ///
    /// # Arguments
    /// * `coordinator_shard` - The shard coordinating this transaction
    /// * `participants` - List of participant shards
    /// * `operation` - The metadata operation to perform
    ///
    /// # Returns
    /// The unique transaction ID
    pub fn begin_transaction(
        &self,
        coordinator_shard: ShardId,
        participants: Vec<ShardId>,
        operation: MetaOp,
    ) -> TransactionId {
        let txn_id = TransactionId::new(self.next_txn_id.fetch_add(1, Ordering::SeqCst));
        let txn = Transaction::new(txn_id, coordinator_shard, participants, operation);

        let mut txns = self.active_transactions.write().unwrap();
        txns.insert(txn_id, txn);

        tracing::debug!("Started transaction {} with participants", txn_id);
        txn_id
    }

    /// Records a participant's commit vote.
    ///
    /// # Arguments
    /// * `txn_id` - The transaction ID
    /// * `shard_id` - The shard casting the vote
    pub fn vote_commit(&self, txn_id: TransactionId, shard_id: ShardId) -> Result<(), MetaError> {
        self.record_vote(txn_id, shard_id, true)
    }

    /// Records a participant's abort vote.
    ///
    /// # Arguments
    /// * `txn_id` - The transaction ID
    /// * `shard_id` - The shard casting the vote
    pub fn vote_abort(&self, txn_id: TransactionId, shard_id: ShardId) -> Result<(), MetaError> {
        self.record_vote(txn_id, shard_id, false)
    }

    fn record_vote(
        &self,
        txn_id: TransactionId,
        shard_id: ShardId,
        commit: bool,
    ) -> Result<(), MetaError> {
        let mut txns = self.active_transactions.write().unwrap();
        let txn = txns
            .get_mut(&txn_id)
            .ok_or_else(|| MetaError::KvError(format!("transaction {} not found", txn_id)))?;

        let participant = txn
            .participants
            .iter_mut()
            .find(|p| p.shard_id == shard_id)
            .ok_or_else(|| {
                MetaError::KvError(format!(
                    "shard {} not a participant in transaction {}",
                    shard_id, txn_id
                ))
            })?;

        if commit {
            participant.vote_commit();
        } else {
            participant.vote_abort();
        }

        tracing::debug!(
            "Transaction {}: shard {} voted {}",
            txn_id,
            shard_id,
            if commit { "commit" } else { "abort" }
        );
        Ok(())
    }

    /// Checks all votes and transitions the transaction state accordingly.
    ///
    /// If all participants voted commit, transitions to Prepared then Committing.
    /// If any participant voted abort, transitions to Aborting.
    ///
    /// # Arguments
    /// * `txn_id` - The transaction ID
    ///
    /// # Returns
    /// The resulting transaction state
    pub fn check_votes(&self, txn_id: TransactionId) -> Result<TransactionState, MetaError> {
        let mut txns = self.active_transactions.write().unwrap();
        let txn = txns
            .get_mut(&txn_id)
            .ok_or_else(|| MetaError::KvError(format!("transaction {} not found", txn_id)))?;

        if !txn.all_voted() {
            return Ok(txn.state.clone());
        }

        if txn.any_voted_abort() {
            txn.state = TransactionState::Aborting;
            tracing::info!(
                "Transaction {}: abort decision (some participant voted abort)",
                txn_id
            );
        } else if txn.all_voted_commit() {
            txn.state = TransactionState::Prepared;
            txn.state = TransactionState::Committing;
            tracing::info!(
                "Transaction {}: commit decision (all participants voted commit)",
                txn_id
            );
        }

        Ok(txn.state.clone())
    }

    /// Marks a transaction as committed.
    ///
    /// # Arguments
    /// * `txn_id` - The transaction ID
    pub fn commit(&self, txn_id: TransactionId) -> Result<(), MetaError> {
        let mut txns = self.active_transactions.write().unwrap();
        let txn = txns
            .get_mut(&txn_id)
            .ok_or_else(|| MetaError::KvError(format!("transaction {} not found", txn_id)))?;

        if !matches!(txn.state, TransactionState::Committing) {
            return Err(MetaError::KvError(format!(
                "cannot commit transaction {} in state {:?}",
                txn_id, txn.state
            )));
        }

        txn.state = TransactionState::Committed;
        tracing::info!("Transaction {}: committed", txn_id);
        Ok(())
    }

    /// Marks a transaction as aborted.
    ///
    /// # Arguments
    /// * `txn_id` - The transaction ID
    pub fn abort(&self, txn_id: TransactionId) -> Result<(), MetaError> {
        let mut txns = self.active_transactions.write().unwrap();
        let txn = txns
            .get_mut(&txn_id)
            .ok_or_else(|| MetaError::KvError(format!("transaction {} not found", txn_id)))?;

        if !matches!(
            txn.state,
            TransactionState::Aborting | TransactionState::Preparing
        ) {
            return Err(MetaError::KvError(format!(
                "cannot abort transaction {} in state {:?}",
                txn_id, txn.state
            )));
        }

        txn.state = TransactionState::Aborted;
        tracing::info!("Transaction {}: aborted", txn_id);
        Ok(())
    }

    /// Retrieves a transaction by ID.
    ///
    /// # Arguments
    /// * `txn_id` - The transaction ID
    ///
    /// # Returns
    /// The transaction record
    pub fn get_transaction(&self, txn_id: TransactionId) -> Result<Transaction, MetaError> {
        let txns = self.active_transactions.read().unwrap();
        txns.get(&txn_id)
            .cloned()
            .ok_or_else(|| MetaError::KvError(format!("transaction {} not found", txn_id)))
    }

    /// Returns the number of active transactions.
    pub fn active_count(&self) -> usize {
        let txns = self.active_transactions.read().unwrap();
        txns.len()
    }

    /// Removes committed and aborted transactions.
    ///
    /// # Returns
    /// Number of transactions removed
    pub fn cleanup_completed(&self) -> usize {
        let mut txns = self.active_transactions.write().unwrap();
        let before = txns.len();
        txns.retain(|_, txn| {
            !matches!(
                txn.state,
                TransactionState::Committed | TransactionState::Aborted
            )
        });
        let removed = before - txns.len();
        if removed > 0 {
            tracing::debug!("Cleaned up {} completed transactions", removed);
        }
        removed
    }

    /// Aborts transactions that have exceeded the timeout.
    ///
    /// # Returns
    /// List of transaction IDs that were timed out
    pub fn cleanup_timed_out(&self) -> Vec<TransactionId> {
        let mut txns = self.active_transactions.write().unwrap();
        let now = Timestamp::now();
        let mut timed_out = Vec::new();

        for (txn_id, txn) in txns.iter_mut() {
            if matches!(
                txn.state,
                TransactionState::Preparing
                    | TransactionState::Committing
                    | TransactionState::Aborting
            ) {
                let elapsed = now.secs.saturating_sub(txn.created_at.secs);
                if elapsed >= self.timeout_secs {
                    txn.state = TransactionState::Aborted;
                    timed_out.push(*txn_id);
                }
            }
        }

        if !timed_out.is_empty() {
            tracing::info!("Timed out {} transactions", timed_out.len());
        }
        timed_out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_operation() -> MetaOp {
        MetaOp::CreateEntry {
            parent: InodeId::new(1),
            name: "test".to_string(),
            entry: DirEntry {
                name: "test".to_string(),
                ino: InodeId::new(2),
                file_type: FileType::RegularFile,
            },
        }
    }

    #[test]
    fn test_begin_transaction() {
        let mgr = TransactionManager::new(60);
        let coordinator = ShardId::new(1);
        let participants = vec![ShardId::new(2), ShardId::new(3)];
        let op = create_test_operation();

        let txn_id = mgr.begin_transaction(coordinator, participants.clone(), op.clone());

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.id, txn_id);
        assert_eq!(txn.state, TransactionState::Preparing);
        assert_eq!(txn.coordinator_shard, coordinator);
        assert_eq!(txn.participants.len(), 2);
    }

    #[test]
    fn test_vote_commit_all_participants() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2), ShardId::new(3)];
        let txn_id = mgr.begin_transaction(
            ShardId::new(0),
            participants.clone(),
            create_test_operation(),
        );

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_commit(txn_id, ShardId::new(2)).unwrap();
        mgr.vote_commit(txn_id, ShardId::new(3)).unwrap();

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert!(txn.participants[0].voted == Some(true));
        assert!(txn.participants[1].voted == Some(true));
        assert!(txn.participants[2].voted == Some(true));
    }

    #[test]
    fn test_vote_abort_any_participant() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2)];
        let txn_id = mgr.begin_transaction(
            ShardId::new(0),
            participants.clone(),
            create_test_operation(),
        );

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_abort(txn_id, ShardId::new(2)).unwrap();

        let state = mgr.check_votes(txn_id).unwrap();
        assert_eq!(state, TransactionState::Aborting);
    }

    #[test]
    fn test_check_votes_all_commit() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, create_test_operation());

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_commit(txn_id, ShardId::new(2)).unwrap();

        let state = mgr.check_votes(txn_id).unwrap();
        assert_eq!(state, TransactionState::Committing);
    }

    #[test]
    fn test_check_votes_one_abort() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, create_test_operation());

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_abort(txn_id, ShardId::new(2)).unwrap();

        let state = mgr.check_votes(txn_id).unwrap();
        assert_eq!(state, TransactionState::Aborting);
    }

    #[test]
    fn test_commit_prepared_transaction() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, create_test_operation());

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_commit(txn_id, ShardId::new(2)).unwrap();
        mgr.check_votes(txn_id).unwrap();
        mgr.commit(txn_id).unwrap();

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.state, TransactionState::Committed);
    }

    #[test]
    fn test_abort_transaction() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1), ShardId::new(2)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, create_test_operation());

        mgr.abort(txn_id).unwrap();

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.state, TransactionState::Aborted);
    }

    #[test]
    fn test_get_transaction() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, create_test_operation());

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.id, txn_id);
    }

    #[test]
    fn test_cleanup_completed() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1)];

        let txn_id1 = mgr.begin_transaction(
            ShardId::new(0),
            participants.clone(),
            create_test_operation(),
        );
        let txn_id2 = mgr.begin_transaction(
            ShardId::new(0),
            participants.clone(),
            create_test_operation(),
        );

        mgr.vote_commit(txn_id1, ShardId::new(1)).unwrap();
        mgr.check_votes(txn_id1).unwrap();
        mgr.commit(txn_id1).unwrap();

        mgr.abort(txn_id2).unwrap();

        let cleaned = mgr.cleanup_completed();
        assert_eq!(cleaned, 2);
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn test_cleanup_timed_out() {
        let mgr = TransactionManager::new(0);
        let participants = vec![ShardId::new(1)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, create_test_operation());

        std::thread::sleep(std::time::Duration::from_millis(10));
        let timed_out = mgr.cleanup_timed_out();

        assert!(timed_out.contains(&txn_id));
        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.state, TransactionState::Aborted);
    }

    #[test]
    fn test_vote_nonexistent_transaction() {
        let mgr = TransactionManager::new(60);
        let result = mgr.vote_commit(TransactionId::new(9999), ShardId::new(1));
        assert!(result.is_err());
    }

    #[test]
    fn test_double_vote_same_shard() {
        let mgr = TransactionManager::new(60);
        let participants = vec![ShardId::new(1)];
        let txn_id = mgr.begin_transaction(ShardId::new(0), participants, create_test_operation());

        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();
        mgr.vote_commit(txn_id, ShardId::new(1)).unwrap();

        let txn = mgr.get_transaction(txn_id).unwrap();
        assert_eq!(txn.participants[0].voted, Some(true));
    }
}
