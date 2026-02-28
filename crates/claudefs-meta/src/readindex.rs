//! Linearizable reads via ReadIndex protocol
//!
//! Implements the ReadIndex protocol from the Raft paper (Section 8) to handle
//! read-only operations without writing to the log while maintaining linearizability.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::Duration;

use crate::types::*;

/// A pending read request waiting for linearizability confirmation.
#[derive(Clone, Debug)]
pub struct PendingRead {
    /// Unique ID for this read request.
    pub id: u64,
    /// The commit index at the time the read was registered.
    pub read_index: LogIndex,
    /// The inode being read.
    pub ino: InodeId,
    /// Which nodes have confirmed the leader via heartbeat response.
    pub confirmations: HashSet<NodeId>,
    /// The total number of nodes in the cluster (including self).
    pub cluster_size: usize,
    /// When this read request was created.
    pub created_at: Timestamp,
}

/// Result of checking a read request's status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadStatus {
    /// Read is confirmed linearizable — state machine is up-to-date.
    Ready,
    /// Waiting for heartbeat confirmations from a quorum.
    WaitingForQuorum,
    /// State machine hasn't caught up to the read index yet.
    WaitingForApply,
    /// Timed out waiting for quorum or apply.
    TimedOut,
}

/// Manages the ReadIndex protocol for linearizable reads.
pub struct ReadIndexManager {
    /// Counter for generating unique read IDs.
    next_read_id: AtomicU64,
    /// Pending read requests.
    pending_reads: RwLock<HashMap<u64, PendingRead>>,
    /// Timeout for pending reads in seconds.
    timeout_secs: u64,
}

impl ReadIndexManager {
    /// Creates a new ReadIndexManager with the specified timeout.
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            next_read_id: AtomicU64::new(1),
            pending_reads: RwLock::new(HashMap::new()),
            timeout_secs,
        }
    }

    /// Registers a new pending read and returns the read ID.
    pub fn register_read(&self, read_index: LogIndex, ino: InodeId, cluster_size: usize) -> u64 {
        let read_id = self.next_read_id.fetch_add(1, Ordering::Relaxed);
        let pending_read = PendingRead {
            id: read_id,
            read_index,
            ino,
            confirmations: HashSet::new(),
            cluster_size,
            created_at: Timestamp::now(),
        };
        self.pending_reads
            .write()
            .expect("lock poisoned")
            .insert(read_id, pending_read);
        read_id
    }

    /// Records a heartbeat confirmation from a peer.
    pub fn confirm_heartbeat(&self, read_id: u64, from: NodeId) -> Result<(), MetaError> {
        let mut pending = self.pending_reads.write().expect("lock poisoned");
        let read = pending
            .get_mut(&read_id)
            .ok_or_else(|| MetaError::KvError(format!("read {} not found", read_id)))?;
        read.confirmations.insert(from);
        Ok(())
    }

    /// Checks if the read is ready (quorum confirmed AND state machine applied through read_index).
    pub fn check_status(&self, read_id: u64, last_applied: LogIndex) -> ReadStatus {
        let pending = self.pending_reads.read().expect("lock poisoned");
        let read = match pending.get(&read_id) {
            Some(r) => r,
            None => return ReadStatus::TimedOut,
        };

        if !self.has_quorum_internal(&read.confirmations, read.cluster_size) {
            return ReadStatus::WaitingForQuorum;
        }

        if last_applied.as_u64() < read.read_index.as_u64() {
            return ReadStatus::WaitingForApply;
        }

        ReadStatus::Ready
    }

    /// Internal helper to check quorum.
    fn has_quorum_internal(&self, confirmations: &HashSet<NodeId>, cluster_size: usize) -> bool {
        let quorum = (cluster_size / 2) + 1;
        confirmations.len() >= quorum
    }

    /// Checks if enough heartbeat confirmations received (> cluster_size/2, counting self).
    pub fn has_quorum(&self, read_id: u64) -> bool {
        let pending = self.pending_reads.read().expect("lock poisoned");
        let read = match pending.get(&read_id) {
            Some(r) => r,
            None => return false,
        };
        self.has_quorum_internal(&read.confirmations, read.cluster_size)
    }

    /// Removes and returns the completed read.
    pub fn complete_read(&self, read_id: u64) -> Result<PendingRead, MetaError> {
        self.pending_reads
            .write()
            .expect("lock poisoned")
            .remove(&read_id)
            .ok_or_else(|| MetaError::KvError(format!("read {} not found", read_id)))
    }

    /// Removes and returns timed-out read IDs.
    pub fn cleanup_timed_out(&self) -> Vec<u64> {
        let now = std::time::SystemTime::now();
        let timeout = Duration::from_secs(self.timeout_secs);
        let mut pending = self.pending_reads.write().expect("lock poisoned");
        let mut timed_out = Vec::new();

        pending.retain(|id, read| {
            let read_time = std::time::UNIX_EPOCH
                .checked_add(Duration::from_secs(read.created_at.secs))
                .unwrap_or(std::time::UNIX_EPOCH);
            let age = now.duration_since(read_time).unwrap_or(Duration::ZERO);
            if age > timeout {
                timed_out.push(*id);
                false
            } else {
                true
            }
        });

        timed_out
    }

    /// Returns the number of pending read requests.
    pub fn pending_count(&self) -> usize {
        self.pending_reads.read().expect("lock poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn create_test_attr(uid: u32, gid: u32, mode: u32) -> InodeAttr {
        InodeAttr {
            ino: InodeId::new(1),
            file_type: FileType::Directory,
            mode,
            nlink: 2,
            uid,
            gid,
            size: 4096,
            blocks: 8,
            atime: Timestamp::now(),
            mtime: Timestamp::now(),
            ctime: Timestamp::now(),
            crtime: Timestamp::now(),
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(1, 0),
            generation: 0,
            symlink_target: None,
        }
    }

    #[test]
    fn test_register_read() {
        let manager = ReadIndexManager::new(5);
        let read_id = manager.register_read(LogIndex::new(10), InodeId::new(1), 3);
        assert_eq!(read_id, 1);
        assert_eq!(manager.pending_count(), 1);
    }

    #[test]
    fn test_confirm_heartbeat() {
        let manager = ReadIndexManager::new(5);
        let read_id = manager.register_read(LogIndex::new(10), InodeId::new(1), 3);
        manager.confirm_heartbeat(read_id, NodeId::new(2)).unwrap();
        manager.confirm_heartbeat(read_id, NodeId::new(3)).unwrap();

        let pending = manager.pending_reads.read().unwrap();
        let read = pending.get(&read_id).unwrap();
        assert_eq!(read.confirmations.len(), 2);
    }

    #[test]
    fn test_check_status_waiting_quorum() {
        let manager = ReadIndexManager::new(5);
        let read_id = manager.register_read(LogIndex::new(10), InodeId::new(1), 3);

        let status = manager.check_status(read_id, LogIndex::new(15));
        assert_eq!(status, ReadStatus::WaitingForQuorum);
    }

    #[test]
    fn test_check_status_waiting_apply() {
        let manager = ReadIndexManager::new(5);
        let read_id = manager.register_read(LogIndex::new(20), InodeId::new(1), 3);

        // Get quorum but state machine hasn't caught up
        manager.confirm_heartbeat(read_id, NodeId::new(2)).unwrap();
        manager.confirm_heartbeat(read_id, NodeId::new(3)).unwrap();

        let status = manager.check_status(read_id, LogIndex::new(15));
        assert_eq!(status, ReadStatus::WaitingForApply);
    }

    #[test]
    fn test_check_status_ready() {
        let manager = ReadIndexManager::new(5);
        let read_id = manager.register_read(LogIndex::new(10), InodeId::new(1), 3);

        // Get quorum and state machine is caught up
        manager.confirm_heartbeat(read_id, NodeId::new(2)).unwrap();
        manager.confirm_heartbeat(read_id, NodeId::new(3)).unwrap();

        let status = manager.check_status(read_id, LogIndex::new(15));
        assert_eq!(status, ReadStatus::Ready);
    }

    #[test]
    fn test_has_quorum_3_node_cluster() {
        let manager = ReadIndexManager::new(5);
        let read_id = manager.register_read(LogIndex::new(10), InodeId::new(1), 3);

        assert!(!manager.has_quorum(read_id)); // Need 2, have 0

        manager.confirm_heartbeat(read_id, NodeId::new(2)).unwrap();
        assert!(!manager.has_quorum(read_id)); // Need 2, have 1

        manager.confirm_heartbeat(read_id, NodeId::new(3)).unwrap();
        assert!(manager.has_quorum(read_id)); // Need 2, have 2
    }

    #[test]
    fn test_has_quorum_5_node_cluster() {
        let manager = ReadIndexManager::new(5);
        let read_id = manager.register_read(LogIndex::new(10), InodeId::new(1), 5);

        assert!(!manager.has_quorum(read_id)); // Need 3, have 0

        manager.confirm_heartbeat(read_id, NodeId::new(2)).unwrap();
        assert!(!manager.has_quorum(read_id)); // Need 3, have 1

        manager.confirm_heartbeat(read_id, NodeId::new(3)).unwrap();
        assert!(!manager.has_quorum(read_id)); // Need 3, have 2

        manager.confirm_heartbeat(read_id, NodeId::new(4)).unwrap();
        assert!(manager.has_quorum(read_id)); // Need 3, have 3 - quorum achieved
    }

    #[test]
    fn test_complete_read() {
        let manager = ReadIndexManager::new(5);
        let read_id = manager.register_read(LogIndex::new(10), InodeId::new(1), 3);

        let read = manager.complete_read(read_id).unwrap();
        assert_eq!(read.id, read_id);
        assert_eq!(manager.pending_count(), 0);
    }

    #[test]
    fn test_cleanup_timed_out() {
        let manager = ReadIndexManager::new(0); // Immediate timeout
        let read_id = manager.register_read(LogIndex::new(10), InodeId::new(1), 3);

        std::thread::sleep(std::time::Duration::from_millis(10));

        let timed_out = manager.cleanup_timed_out();
        assert!(timed_out.contains(&read_id));
        assert_eq!(manager.pending_count(), 0);
    }

    #[test]
    fn test_pending_count() {
        let manager = ReadIndexManager::new(5);
        assert_eq!(manager.pending_count(), 0);

        manager.register_read(LogIndex::new(10), InodeId::new(1), 3);
        manager.register_read(LogIndex::new(20), InodeId::new(2), 3);
        assert_eq!(manager.pending_count(), 2);

        manager.complete_read(1).unwrap();
        assert_eq!(manager.pending_count(), 1);
    }
}
