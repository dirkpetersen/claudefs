use crate::{FuseError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tracing::{debug, trace, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BarrierId(u64);

impl BarrierId {
    pub fn new(id: u64) -> Self {
        BarrierId(id)
    }
}

impl fmt::Display for BarrierId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "barrier:{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BarrierKind {
    DataOnly,
    MetadataOnly,
    DataAndMetadata,
    JournalCommit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BarrierState {
    Pending,
    Flushing,
    Committed,
    Failed(String),
}

pub struct WriteBarrier {
    barrier_id: BarrierId,
    inode: u64,
    kind: BarrierKind,
    state: BarrierState,
    created_at: Instant,
    sequence: u64,
}

impl WriteBarrier {
    pub fn new(barrier_id: BarrierId, inode: u64, kind: BarrierKind, sequence: u64) -> Self {
        WriteBarrier {
            barrier_id,
            inode,
            kind,
            state: BarrierState::Pending,
            created_at: Instant::now(),
            sequence,
        }
    }

    pub fn is_complete(&self) -> bool {
        matches!(
            self.state,
            BarrierState::Committed | BarrierState::Failed(_)
        )
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.state, BarrierState::Pending | BarrierState::Flushing)
    }

    pub fn mark_flushing(&mut self) {
        self.state = BarrierState::Flushing;
    }

    pub fn mark_committed(&mut self) {
        self.state = BarrierState::Committed;
    }

    pub fn mark_failed(&mut self, reason: &str) {
        self.state = BarrierState::Failed(reason.to_string());
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.created_at.elapsed().as_millis() as u64
    }

    pub fn barrier_id(&self) -> BarrierId {
        self.barrier_id
    }

    pub fn inode(&self) -> u64 {
        self.inode
    }

    pub fn kind(&self) -> BarrierKind {
        self.kind
    }

    pub fn state(&self) -> &BarrierState {
        &self.state
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FsyncMode {
    Sync,
    Async,
    Ordered { max_delay_ms: u64 },
}

impl Default for FsyncMode {
    fn default() -> Self {
        FsyncMode::Ordered { max_delay_ms: 100 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    entry_id: u64,
    inode: u64,
    operation: String,
    version: u64,
    timestamp: SystemTime,
}

impl JournalEntry {
    pub fn new(entry_id: u64, inode: u64, operation: &str, version: u64) -> Self {
        JournalEntry {
            entry_id,
            inode,
            operation: operation.to_string(),
            version,
            timestamp: SystemTime::now(),
        }
    }

    pub fn entry_id(&self) -> u64 {
        self.entry_id
    }

    pub fn inode(&self) -> u64 {
        self.inode
    }

    pub fn operation(&self) -> &str {
        &self.operation
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn timestamp(&self) -> SystemTime {
        self.timestamp
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JournalError {
    #[error("Journal is full, cannot append new entries")]
    JournalFull,
}

pub struct FsyncJournal {
    entries: Vec<JournalEntry>,
    next_entry_id: u64,
    max_entries: usize,
}

impl FsyncJournal {
    pub fn new(max_entries: usize) -> Self {
        FsyncJournal {
            entries: Vec::new(),
            next_entry_id: 1,
            max_entries,
        }
    }

    pub fn append(&mut self, inode: u64, operation: &str, version: u64) -> Result<u64> {
        if self.is_full() {
            return Err(FuseError::InvalidArgument {
                msg: "Journal is full".to_string(),
            });
        }
        let entry_id = self.next_entry_id;
        self.next_entry_id += 1;
        self.entries
            .push(JournalEntry::new(entry_id, inode, operation, version));
        debug!("Appended journal entry {} for inode {}", entry_id, inode);
        Ok(entry_id)
    }

    pub fn commit_up_to(&mut self, entry_id: u64) -> usize {
        let original_len = self.entries.len();
        self.entries.retain(|e| e.entry_id > entry_id);
        let removed = original_len - self.entries.len();
        if removed > 0 {
            debug!("Committed up to entry {}, removed {}", entry_id, removed);
        }
        removed
    }

    pub fn pending_count(&self) -> usize {
        self.entries.len()
    }

    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.max_entries
    }

    pub fn entries_for_inode(&self, inode: u64) -> Vec<&JournalEntry> {
        self.entries.iter().filter(|e| e.inode == inode).collect()
    }

    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }
}

pub struct BarrierManager {
    barriers: HashMap<u64, WriteBarrier>,
    journal: FsyncJournal,
    next_barrier_id: u64,
    next_sequence: u64,
    fsync_mode: FsyncMode,
}

impl BarrierManager {
    pub fn new(mode: FsyncMode) -> Self {
        BarrierManager {
            barriers: HashMap::new(),
            journal: FsyncJournal::new(1000),
            next_barrier_id: 1,
            next_sequence: 1,
            fsync_mode: mode,
        }
    }

    pub fn create_barrier(&mut self, inode: u64, kind: BarrierKind) -> BarrierId {
        let barrier_id = BarrierId::new(self.next_barrier_id);
        self.next_barrier_id += 1;
        let sequence = self.next_sequence;
        self.next_sequence += 1;

        let barrier = WriteBarrier::new(barrier_id, inode, kind, sequence);
        trace!(
            "Created barrier {} for inode {:?} with kind {:?}",
            barrier_id,
            inode,
            kind
        );
        self.barriers.insert(barrier_id.0, barrier);
        barrier_id
    }

    pub fn get_barrier(&self, id: &BarrierId) -> Option<&WriteBarrier> {
        self.barriers.get(&id.0)
    }

    pub fn get_barrier_mut(&mut self, id: &BarrierId) -> Option<&mut WriteBarrier> {
        self.barriers.get_mut(&id.0)
    }

    pub fn flush_barrier(&mut self, id: &BarrierId) -> Result<()> {
        match self.barriers.get_mut(&id.0) {
            Some(barrier) => {
                barrier.mark_flushing();
                trace!("Flushing barrier {}", id);
                Ok(())
            }
            None => Err(FuseError::InvalidArgument {
                msg: format!("Barrier not found: {}", id),
            }),
        }
    }

    pub fn commit_barrier(&mut self, id: &BarrierId) -> Result<()> {
        match self.barriers.get_mut(&id.0) {
            Some(barrier) => {
                barrier.mark_committed();
                trace!("Committed barrier {}", id);
                Ok(())
            }
            None => Err(FuseError::InvalidArgument {
                msg: format!("Barrier not found: {}", id),
            }),
        }
    }

    pub fn fail_barrier(&mut self, id: &BarrierId, reason: &str) -> Result<()> {
        match self.barriers.get_mut(&id.0) {
            Some(barrier) => {
                barrier.mark_failed(reason);
                warn!("Failed barrier {}: {}", id, reason);
                Ok(())
            }
            None => Err(FuseError::InvalidArgument {
                msg: format!("Barrier not found: {}", id),
            }),
        }
    }

    pub fn pending_barriers(&self) -> Vec<&WriteBarrier> {
        self.barriers.values().filter(|b| b.is_pending()).collect()
    }

    pub fn committed_count(&self) -> usize {
        self.barriers
            .values()
            .filter(|b| matches!(b.state(), BarrierState::Committed))
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.barriers
            .values()
            .filter(|b| matches!(b.state(), BarrierState::Failed(_)))
            .count()
    }

    pub fn record_fsync(&mut self, inode: u64, version: u64) -> Result<u64> {
        self.journal.append(inode, "fsync", version)
    }

    pub fn journal(&self) -> &FsyncJournal {
        &self.journal
    }

    pub fn journal_mut(&mut self) -> &mut FsyncJournal {
        &mut self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_barrier_id_display() {
        let id = BarrierId::new(42);
        assert_eq!(format!("{}", id), "barrier:42");
        let id2 = BarrierId::new(0);
        assert_eq!(format!("{}", id2), "barrier:0");
        let id3 = BarrierId::new(999);
        assert_eq!(format!("{}", id3), "barrier:999");
    }

    #[test]
    fn test_write_barrier_lifecycle() {
        let barrier_id = BarrierId::new(1);
        let mut barrier = WriteBarrier::new(barrier_id, 100, BarrierKind::DataAndMetadata, 1);

        assert!(barrier.is_pending());
        assert!(!barrier.is_complete());

        barrier.mark_flushing();
        assert!(barrier.is_pending());
        assert!(!barrier.is_complete());
        assert!(matches!(barrier.state(), BarrierState::Flushing));

        barrier.mark_committed();
        assert!(!barrier.is_pending());
        assert!(barrier.is_complete());
        assert!(matches!(barrier.state(), BarrierState::Committed));
    }

    #[test]
    fn test_write_barrier_fail() {
        let barrier_id = BarrierId::new(1);
        let mut barrier = WriteBarrier::new(barrier_id, 100, BarrierKind::DataOnly, 1);

        barrier.mark_flushing();
        barrier.mark_failed("disk error");

        assert!(barrier.is_complete());
        assert!(!barrier.is_pending());
        if let BarrierState::Failed(msg) = barrier.state() {
            assert_eq!(msg, "disk error");
        } else {
            panic!("Expected Failed state");
        }
    }

    #[test]
    fn test_write_barrier_elapsed_ms() {
        let barrier_id = BarrierId::new(1);
        let barrier = WriteBarrier::new(barrier_id, 100, BarrierKind::MetadataOnly, 1);
        std::thread::sleep(Duration::from_millis(5));
        assert!(barrier.elapsed_ms() >= 4);
    }

    #[test]
    fn test_fsync_journal_append() {
        let mut journal = FsyncJournal::new(10);
        let id1 = journal.append(1, "write", 1).unwrap();
        let id2 = journal.append(1, "write", 2).unwrap();
        let id3 = journal.append(2, "fsync", 1).unwrap();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        assert_eq!(journal.pending_count(), 3);
    }

    #[test]
    fn test_fsync_journal_commit_up_to() {
        let mut journal = FsyncJournal::new(10);
        journal.append(1, "write", 1).unwrap();
        journal.append(1, "write", 2).unwrap();
        journal.append(2, "write", 1).unwrap();
        journal.append(2, "fsync", 2).unwrap();

        let removed = journal.commit_up_to(2);
        assert_eq!(removed, 2);
        assert_eq!(journal.pending_count(), 2);

        let entries: Vec<_> = journal.entries().iter().map(|e| e.entry_id).collect();
        assert_eq!(entries, vec![3, 4]);
    }

    #[test]
    fn test_fsync_journal_is_full() {
        let mut journal = FsyncJournal::new(3);
        assert!(!journal.is_full());

        journal.append(1, "write", 1).unwrap();
        journal.append(1, "write", 2).unwrap();
        journal.append(1, "write", 3).unwrap();
        assert!(journal.is_full());

        let result = journal.append(1, "write", 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_fsync_journal_entries_for_inode() {
        let mut journal = FsyncJournal::new(10);
        journal.append(1, "write", 1).unwrap();
        journal.append(2, "write", 1).unwrap();
        journal.append(1, "fsync", 2).unwrap();
        journal.append(3, "write", 1).unwrap();

        let inode1_entries = journal.entries_for_inode(1);
        assert_eq!(inode1_entries.len(), 2);

        let inode2_entries = journal.entries_for_inode(2);
        assert_eq!(inode2_entries.len(), 1);

        let inode99_entries = journal.entries_for_inode(99);
        assert!(inode99_entries.is_empty());
    }

    #[test]
    fn test_barrier_manager_create_barrier() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let id1 = manager.create_barrier(100, BarrierKind::DataOnly);
        let id2 = manager.create_barrier(200, BarrierKind::MetadataOnly);
        let id3 = manager.create_barrier(100, BarrierKind::DataAndMetadata);

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);

        let barrier = manager.get_barrier(&id1).unwrap();
        assert_eq!(barrier.inode(), 100);
        assert_eq!(barrier.kind(), BarrierKind::DataOnly);
    }

    #[test]
    fn test_barrier_manager_flush_commit() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let id = manager.create_barrier(100, BarrierKind::DataAndMetadata);

        manager.flush_barrier(&id).unwrap();
        assert!(matches!(
            manager.get_barrier(&id).unwrap().state(),
            BarrierState::Flushing
        ));

        manager.commit_barrier(&id).unwrap();
        assert!(matches!(
            manager.get_barrier(&id).unwrap().state(),
            BarrierState::Committed
        ));
    }

    #[test]
    fn test_barrier_manager_fail() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let id = manager.create_barrier(100, BarrierKind::DataOnly);

        manager.flush_barrier(&id).unwrap();
        manager.fail_barrier(&id, "IO error").unwrap();

        if let BarrierState::Failed(msg) = manager.get_barrier(&id).unwrap().state() {
            assert_eq!(msg, "IO error");
        } else {
            panic!("Expected Failed state");
        }
    }

    #[test]
    fn test_barrier_manager_invalid_id() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let id = BarrierId::new(999);

        assert!(manager.flush_barrier(&id).is_err());
        assert!(manager.commit_barrier(&id).is_err());
        assert!(manager.fail_barrier(&id, "test").is_err());
        assert!(manager.get_barrier(&id).is_none());
    }

    #[test]
    fn test_barrier_manager_pending_barriers() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let id1 = manager.create_barrier(100, BarrierKind::DataOnly);
        let id2 = manager.create_barrier(100, BarrierKind::MetadataOnly);
        let id3 = manager.create_barrier(200, BarrierKind::DataAndMetadata);

        manager.flush_barrier(&id1).unwrap();
        manager.commit_barrier(&id2).unwrap();

        let pending = manager.pending_barriers();
        assert_eq!(pending.len(), 2);

        let pending_ids: Vec<_> = pending.iter().map(|b| b.barrier_id()).collect();
        assert!(pending_ids.contains(&id1));
        assert!(pending_ids.contains(&id3));
    }

    #[test]
    fn test_barrier_manager_counts() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let id1 = manager.create_barrier(100, BarrierKind::DataOnly);
        let id2 = manager.create_barrier(100, BarrierKind::DataOnly);
        let id3 = manager.create_barrier(100, BarrierKind::DataOnly);
        let id4 = manager.create_barrier(100, BarrierKind::DataOnly);

        manager.commit_barrier(&id1).unwrap();
        manager.commit_barrier(&id2).unwrap();
        manager.fail_barrier(&id3, "error").unwrap();

        assert_eq!(manager.committed_count(), 2);
        assert_eq!(manager.failed_count(), 1);
        assert_eq!(manager.pending_barriers().len(), 1);
    }

    #[test]
    fn test_barrier_manager_record_fsync() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let entry_id = manager.record_fsync(100, 5).unwrap();

        assert_eq!(entry_id, 1);
        assert_eq!(manager.journal().pending_count(), 1);

        manager.journal_mut().commit_up_to(entry_id);
        assert_eq!(manager.journal().pending_count(), 0);
    }

    #[test]
    fn test_fsync_mode_default() {
        let mode = FsyncMode::default();
        if let FsyncMode::Ordered { max_delay_ms } = mode {
            assert_eq!(max_delay_ms, 100);
        } else {
            panic!("Expected Ordered mode");
        }
    }

    #[test]
    fn test_barrier_kind_serialize() {
        let kinds = vec![
            BarrierKind::DataOnly,
            BarrierKind::MetadataOnly,
            BarrierKind::DataAndMetadata,
            BarrierKind::JournalCommit,
        ];

        for kind in &kinds {
            let serialized = serde_json::to_string(kind).unwrap();
            let deserialized: BarrierKind = serde_json::from_str(&serialized).unwrap();
            assert_eq!(*kind, deserialized);
        }
    }

    #[test]
    fn test_journal_entry_timestamp() {
        let entry = JournalEntry::new(1, 100, "write", 5);
        let before = SystemTime::now();
        let entry = JournalEntry::new(1, 100, "write", 5);
        let after = SystemTime::now();

        assert!(entry.timestamp() >= before);
        assert!(entry.timestamp() <= after);
    }

    #[test]
    fn test_multiple_barriers_same_inode() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let id1 = manager.create_barrier(100, BarrierKind::DataOnly);
        let id2 = manager.create_barrier(100, BarrierKind::MetadataOnly);
        let id3 = manager.create_barrier(100, BarrierKind::DataAndMetadata);

        manager.flush_barrier(&id1).unwrap();
        manager.commit_barrier(&id1).unwrap();
        manager.flush_barrier(&id2).unwrap();

        let pending = manager.pending_barriers();
        assert_eq!(pending.len(), 2);

        manager.commit_barrier(&id2).unwrap();
        manager.commit_barrier(&id3).unwrap();

        assert_eq!(manager.committed_count(), 3);
    }

    #[test]
    fn test_journal_full_error() {
        let mut journal = FsyncJournal::new(2);
        journal.append(1, "write", 1).unwrap();
        journal.append(1, "write", 2).unwrap();

        let result = journal.append(1, "write", 3);
        assert!(result.is_err());
    }
}
