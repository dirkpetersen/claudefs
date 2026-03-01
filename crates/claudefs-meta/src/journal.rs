//! Metadata journal for replication tailing.
//!
//! Records committed metadata operations in sequence order. The cross-site
//! replication agent tails this journal asynchronously, per docs/metadata.md.
//!
//! The journal is append-only and supports:
//! - Appending committed operations with monotonic sequence numbers
//! - Tailing from a specific sequence number
//! - Batch read for replication catchup
//! - Compaction of stale entries

use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use crate::types::{LogIndex, MetaError, MetaOp, Timestamp, VectorClock};

/// A journal entry with a sequence number for replication tracking.
#[derive(Clone, Debug)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number.
    pub sequence: u64,
    /// The committed metadata operation.
    pub op: MetaOp,
    /// Timestamp when the operation was committed.
    pub committed_at: Timestamp,
    /// The Raft log index of this operation.
    pub log_index: LogIndex,
    /// Vector clock for cross-site conflict resolution.
    pub vector_clock: VectorClock,
}

/// The metadata journal — an append-only log of committed operations.
///
/// The replication agent tails this journal to send operations to remote sites.
/// Entries are kept in memory with optional compaction of old entries.
pub struct MetadataJournal {
    entries: Arc<RwLock<VecDeque<JournalEntry>>>,
    next_sequence: Arc<RwLock<u64>>,
    site_id: u64,
    /// Maximum number of entries to keep before compaction.
    max_entries: usize,
}

impl MetadataJournal {
    /// Create a new metadata journal for the given site.
    pub fn new(site_id: u64, max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::new())),
            next_sequence: Arc::new(RwLock::new(1)),
            site_id,
            max_entries,
        }
    }

    /// Append a committed operation to the journal.
    /// Returns the assigned sequence number.
    pub fn append(&self, op: MetaOp, log_index: LogIndex) -> Result<u64, MetaError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let mut seq = self
            .next_sequence
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;

        let sequence = *seq;
        *seq += 1;

        let entry = JournalEntry {
            sequence,
            op,
            committed_at: Timestamp::now(),
            log_index,
            vector_clock: VectorClock::new(self.site_id, sequence),
        };

        entries.push_back(entry);

        // Compact if needed
        while entries.len() > self.max_entries {
            entries.pop_front();
        }

        Ok(sequence)
    }

    /// Read entries starting from the given sequence number.
    /// Returns up to `limit` entries.
    pub fn read_from(
        &self,
        from_sequence: u64,
        limit: usize,
    ) -> Result<Vec<JournalEntry>, MetaError> {
        let entries = self
            .entries
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;

        let result: Vec<_> = entries
            .iter()
            .filter(|e| e.sequence >= from_sequence)
            .take(limit)
            .cloned()
            .collect();

        Ok(result)
    }

    /// Get the latest sequence number.
    pub fn latest_sequence(&self) -> Result<u64, MetaError> {
        let seq = self
            .next_sequence
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(seq.saturating_sub(1))
    }

    /// Get the number of entries currently in the journal.
    pub fn len(&self) -> Result<usize, MetaError> {
        let entries = self
            .entries
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(entries.len())
    }

    /// Returns true if the journal is empty.
    pub fn is_empty(&self) -> Result<bool, MetaError> {
        Ok(self.len()? == 0)
    }

    /// Get the replication lag (difference between latest sequence and the
    /// given remote sequence number).
    pub fn replication_lag(&self, remote_sequence: u64) -> Result<u64, MetaError> {
        let latest = self.latest_sequence()?;
        Ok(latest.saturating_sub(remote_sequence))
    }

    /// Compact the journal, removing entries older than the given sequence number.
    pub fn compact_before(&self, sequence: u64) -> Result<usize, MetaError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let original_len = entries.len();

        while let Some(front) = entries.front() {
            if front.sequence < sequence {
                entries.pop_front();
            } else {
                break;
            }
        }

        Ok(original_len - entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{InodeAttr, InodeId};

    fn create_test_op(ino: u64) -> MetaOp {
        let attr = InodeAttr::new_file(InodeId::new(ino), 1000, 1000, 0o644, 1);
        MetaOp::CreateInode { attr }
    }

    #[test]
    fn test_append_and_read() {
        let journal = MetadataJournal::new(1, 100);

        let seq1 = journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        let seq2 = journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();

        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);

        let entries = journal.read_from(1, 10).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[1].sequence, 2);
    }

    #[test]
    fn test_read_from_sequence() {
        let journal = MetadataJournal::new(1, 100);

        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();
        journal
            .append(create_test_op(300), LogIndex::new(3))
            .unwrap();

        let entries = journal.read_from(2, 10).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 2);
        assert_eq!(entries[1].sequence, 3);
    }

    #[test]
    fn test_latest_sequence() {
        let journal = MetadataJournal::new(1, 100);

        assert_eq!(journal.latest_sequence().unwrap(), 0);

        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        assert_eq!(journal.latest_sequence().unwrap(), 1);

        journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();
        assert_eq!(journal.latest_sequence().unwrap(), 2);
    }

    #[test]
    fn test_replication_lag() {
        let journal = MetadataJournal::new(1, 100);

        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();

        let lag = journal.replication_lag(0).unwrap();
        assert_eq!(lag, 2);

        let lag = journal.replication_lag(1).unwrap();
        assert_eq!(lag, 1);

        let lag = journal.replication_lag(2).unwrap();
        assert_eq!(lag, 0);

        let lag = journal.replication_lag(100).unwrap();
        assert_eq!(lag, 0);
    }

    #[test]
    fn test_compact_before() {
        let journal = MetadataJournal::new(1, 100);

        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();
        journal
            .append(create_test_op(300), LogIndex::new(3))
            .unwrap();

        let removed = journal.compact_before(3).unwrap();
        assert_eq!(removed, 2);

        let entries = journal.read_from(1, 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sequence, 3);
    }

    #[test]
    fn test_max_entries_compaction() {
        let journal = MetadataJournal::new(1, 3);

        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();
        journal
            .append(create_test_op(300), LogIndex::new(3))
            .unwrap();
        journal
            .append(create_test_op(400), LogIndex::new(4))
            .unwrap();

        let entries = journal.read_from(1, 10).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].sequence, 2);
    }

    #[test]
    fn test_empty_journal() {
        let journal = MetadataJournal::new(1, 100);

        assert!(journal.is_empty().unwrap());
        assert_eq!(journal.len().unwrap(), 0);
        assert_eq!(journal.latest_sequence().unwrap(), 0);

        let entries = journal.read_from(1, 10).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_read_from_future_sequence() {
        let journal = MetadataJournal::new(1, 100);
        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();

        let entries = journal.read_from(999, 10).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_read_with_limit() {
        let journal = MetadataJournal::new(1, 100);
        for i in 1..=10 {
            journal
                .append(create_test_op(i * 100), LogIndex::new(i))
                .unwrap();
        }

        let entries = journal.read_from(1, 3).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[2].sequence, 3);
    }

    #[test]
    fn test_compact_before_first_entry() {
        let journal = MetadataJournal::new(1, 100);
        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();

        // Compact before sequence 1 — nothing should be removed
        let removed = journal.compact_before(1).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(journal.len().unwrap(), 2);
    }

    #[test]
    fn test_compact_all() {
        let journal = MetadataJournal::new(1, 100);
        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();

        let removed = journal.compact_before(u64::MAX).unwrap();
        assert_eq!(removed, 2);
        assert!(journal.is_empty().unwrap());
    }

    #[test]
    fn test_sequences_are_monotonic() {
        let journal = MetadataJournal::new(1, 100);
        let s1 = journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();
        let s2 = journal
            .append(create_test_op(200), LogIndex::new(2))
            .unwrap();
        let s3 = journal
            .append(create_test_op(300), LogIndex::new(3))
            .unwrap();

        assert!(s1 < s2);
        assert!(s2 < s3);
    }

    #[test]
    fn test_vector_clock_in_entries() {
        let journal = MetadataJournal::new(42, 100);
        journal
            .append(create_test_op(100), LogIndex::new(1))
            .unwrap();

        let entries = journal.read_from(1, 10).unwrap();
        assert_eq!(entries[0].vector_clock.site_id, 42);
        assert_eq!(entries[0].vector_clock.sequence, 1);
    }
}
