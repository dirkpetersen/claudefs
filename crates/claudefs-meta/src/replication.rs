//! Cross-site replication state tracking and batch compaction.
//!
//! This module tracks replication state for remote sites and implements
//! the batch compaction optimization from docs/metadata.md: if a file
//! is created and then deleted within the same batch window, the net
//! effect is "nothing" and neither operation needs to be replicated.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::journal::{JournalEntry, MetadataJournal};
use crate::types::{InodeId, MetaError, MetaOp};

/// Tracks replication state for a remote site.
#[derive(Debug, Clone)]
pub struct RemoteSiteState {
    /// Remote site identifier.
    pub site_id: u64,
    /// Last sequence number confirmed received by the remote site.
    pub confirmed_sequence: u64,
    /// Whether the remote site is currently reachable.
    pub is_reachable: bool,
}

/// Manages replication to multiple remote sites.
pub struct ReplicationTracker {
    journal: Arc<MetadataJournal>,
    remote_sites: Arc<RwLock<HashMap<u64, RemoteSiteState>>>,
}

impl ReplicationTracker {
    /// Create a new replication tracker.
    pub fn new(journal: Arc<MetadataJournal>) -> Self {
        Self {
            journal,
            remote_sites: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a remote site for replication.
    pub fn register_site(&self, site_id: u64) -> Result<(), MetaError> {
        let mut sites = self
            .remote_sites
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        sites.insert(
            site_id,
            RemoteSiteState {
                site_id,
                confirmed_sequence: 0,
                is_reachable: false,
            },
        );
        Ok(())
    }

    /// Acknowledge that a remote site has received entries up to the given sequence.
    pub fn acknowledge(&self, site_id: u64, sequence: u64) -> Result<(), MetaError> {
        let mut sites = self
            .remote_sites
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        if let Some(site) = sites.get_mut(&site_id) {
            site.confirmed_sequence = sequence;
            site.is_reachable = true;
        }
        Ok(())
    }

    /// Get pending entries for a remote site (entries not yet confirmed).
    pub fn pending_entries(
        &self,
        site_id: u64,
        limit: usize,
    ) -> Result<Vec<JournalEntry>, MetaError> {
        let sites = self
            .remote_sites
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let from_seq = sites
            .get(&site_id)
            .map(|s| s.confirmed_sequence + 1)
            .unwrap_or(1);
        drop(sites);

        self.journal.read_from(from_seq, limit)
    }

    /// Get replication lag for a specific remote site.
    pub fn lag_for_site(&self, site_id: u64) -> Result<u64, MetaError> {
        let sites = self
            .remote_sites
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let confirmed = sites
            .get(&site_id)
            .map(|s| s.confirmed_sequence)
            .unwrap_or(0);
        drop(sites);

        self.journal.replication_lag(confirmed)
    }

    /// Get all remote site states.
    pub fn site_states(&self) -> Result<Vec<RemoteSiteState>, MetaError> {
        let sites = self
            .remote_sites
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(sites.values().cloned().collect())
    }
}

/// Compacts a batch of journal entries by canceling out complementary operations.
///
/// Per docs/metadata.md: "if a file is created and then deleted within the same
/// batch window, the net effect is nothing — don't replicate intermediate states."
pub fn compact_batch(entries: Vec<JournalEntry>) -> Vec<JournalEntry> {
    // Track created inodes that are later deleted
    let mut created_inodes: HashMap<InodeId, usize> = HashMap::new(); // ino -> index
    let mut deleted_inodes: HashMap<InodeId, usize> = HashMap::new(); // ino -> index

    for (i, entry) in entries.iter().enumerate() {
        match &entry.op {
            MetaOp::CreateInode { attr } => {
                created_inodes.insert(attr.ino, i);
            }
            MetaOp::DeleteInode { ino } => {
                deleted_inodes.insert(*ino, i);
            }
            _ => {}
        }
    }

    // Find inodes that were both created and deleted — these cancel out
    let canceled: std::collections::HashSet<usize> = created_inodes
        .iter()
        .filter_map(|(ino, create_idx)| {
            deleted_inodes.get(ino).and_then(|delete_idx| {
                if delete_idx > create_idx {
                    Some([*create_idx, *delete_idx])
                } else {
                    None
                }
            })
        })
        .flatten()
        .collect();

    entries
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !canceled.contains(i))
        .map(|(_, entry)| entry)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::JournalEntry;
    use crate::types::{FileType, InodeAttr, LogIndex, Timestamp, VectorClock};

    fn make_journal_entry(sequence: u64, op: MetaOp) -> JournalEntry {
        JournalEntry {
            sequence,
            op,
            committed_at: Timestamp::now(),
            log_index: LogIndex::new(sequence),
            vector_clock: VectorClock::new(1, sequence),
        }
    }

    fn create_op(ino: u64) -> MetaOp {
        let attr = InodeAttr {
            ino: InodeId::new(ino),
            file_type: FileType::RegularFile,
            mode: 0o644,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            blocks: 0,
            atime: Timestamp::now(),
            mtime: Timestamp::now(),
            ctime: Timestamp::now(),
            crtime: Timestamp::now(),
            content_hash: None,
            repl_state: crate::types::ReplicationState::Local,
            vector_clock: VectorClock::new(1, 0),
            generation: 0,
            symlink_target: None,
        };
        MetaOp::CreateInode { attr }
    }

    fn delete_op(ino: u64) -> MetaOp {
        MetaOp::DeleteInode {
            ino: InodeId::new(ino),
        }
    }

    fn setattr_op(ino: u64) -> MetaOp {
        let attr = InodeAttr {
            ino: InodeId::new(ino),
            file_type: FileType::RegularFile,
            mode: 0o644,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 100,
            blocks: 0,
            atime: Timestamp::now(),
            mtime: Timestamp::now(),
            ctime: Timestamp::now(),
            crtime: Timestamp::now(),
            content_hash: None,
            repl_state: crate::types::ReplicationState::Local,
            vector_clock: VectorClock::new(1, 0),
            generation: 0,
            symlink_target: None,
        };
        MetaOp::SetAttr {
            ino: InodeId::new(ino),
            attr,
        }
    }

    #[test]
    fn test_register_site() {
        let journal = Arc::new(MetadataJournal::new(1, 100));
        let tracker = ReplicationTracker::new(journal);

        tracker.register_site(10).unwrap();
        tracker.register_site(20).unwrap();

        let states = tracker.site_states().unwrap();
        assert_eq!(states.len(), 2);

        let state10 = states.iter().find(|s| s.site_id == 10).unwrap();
        assert_eq!(state10.confirmed_sequence, 0);
        assert!(!state10.is_reachable);
    }

    #[test]
    fn test_acknowledge_updates_sequence() {
        let journal = Arc::new(MetadataJournal::new(1, 100));
        let tracker = ReplicationTracker::new(journal);

        tracker.register_site(10).unwrap();
        tracker.acknowledge(10, 5).unwrap();

        let states = tracker.site_states().unwrap();
        let state = states.iter().find(|s| s.site_id == 10).unwrap();
        assert_eq!(state.confirmed_sequence, 5);
        assert!(state.is_reachable);
    }

    #[test]
    fn test_pending_entries() {
        let journal = Arc::new(MetadataJournal::new(1, 100));
        journal.append(create_op(100), LogIndex::new(1)).unwrap();
        journal.append(create_op(200), LogIndex::new(2)).unwrap();
        journal.append(create_op(300), LogIndex::new(3)).unwrap();

        let tracker = ReplicationTracker::new(journal.clone());
        tracker.register_site(10).unwrap();

        let pending = tracker.pending_entries(10, 10).unwrap();
        assert_eq!(pending.len(), 3);

        tracker.acknowledge(10, 2).unwrap();

        let pending = tracker.pending_entries(10, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sequence, 3);
    }

    #[test]
    fn test_lag_tracking() {
        let journal = Arc::new(MetadataJournal::new(1, 100));
        journal.append(create_op(100), LogIndex::new(1)).unwrap();
        journal.append(create_op(200), LogIndex::new(2)).unwrap();

        let tracker = ReplicationTracker::new(journal.clone());
        tracker.register_site(10).unwrap();

        let lag = tracker.lag_for_site(10).unwrap();
        assert_eq!(lag, 2);

        tracker.acknowledge(10, 1).unwrap();

        let lag = tracker.lag_for_site(10).unwrap();
        assert_eq!(lag, 1);
    }

    #[test]
    fn test_compact_batch_cancels_create_delete() {
        let entries = vec![
            make_journal_entry(1, create_op(100)),
            make_journal_entry(2, create_op(200)),
            make_journal_entry(3, delete_op(100)), // Canceled: created then deleted
            make_journal_entry(4, setattr_op(200)),
        ];

        let compacted = compact_batch(entries);

        assert_eq!(compacted.len(), 2);
        assert!(
            matches!(&compacted[0].op, MetaOp::CreateInode { attr } if attr.ino.as_u64() == 200)
        );
        assert!(matches!(&compacted[1].op, MetaOp::SetAttr { .. }));
    }

    #[test]
    fn test_compact_batch_preserves_independent_ops() {
        let entries = vec![
            make_journal_entry(1, create_op(100)),
            make_journal_entry(2, create_op(200)),
            make_journal_entry(3, delete_op(300)), // No create, just delete
            make_journal_entry(4, setattr_op(100)),
        ];

        let compacted = compact_batch(entries);

        assert_eq!(compacted.len(), 4);
    }

    #[test]
    fn test_compact_batch_empty() {
        let entries: Vec<JournalEntry> = vec![];
        let compacted = compact_batch(entries);
        assert!(compacted.is_empty());
    }
}
