//! Journal tailing API for cross-site replication (A6 integration).
//!
//! Provides a structured streaming interface over the MetadataJournal.
//! The replication agent creates a JournalTailer, then calls `poll_batch()`
//! to get the next batch of operations. Supports cursor persistence for
//! crash recovery and batch compaction per docs/metadata.md.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::journal::{JournalEntry, MetadataJournal};
use crate::types::{InodeId, MetaError, MetaOp};

/// Configuration for a journal tailer.
#[derive(Clone, Debug)]
pub struct TailerConfig {
    /// Unique identifier for this tailer (e.g., remote site ID).
    pub consumer_id: String,
    /// Maximum entries per batch.
    pub batch_size: usize,
    /// Whether to compact batches (remove create+delete pairs).
    pub enable_compaction: bool,
}

impl Default for TailerConfig {
    fn default() -> Self {
        Self {
            consumer_id: "default".to_string(),
            batch_size: 1000,
            enable_compaction: true,
        }
    }
}

/// A batch of journal entries ready for replication.
#[derive(Clone, Debug)]
pub struct ReplicationBatch {
    /// Entries in this batch.
    pub entries: Vec<JournalEntry>,
    /// The sequence number of the first entry in this batch.
    pub first_sequence: u64,
    /// The sequence number of the last entry in this batch.
    pub last_sequence: u64,
    /// Number of entries removed by compaction.
    pub compacted_count: usize,
}

/// Cursor tracking the tailer's position in the journal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TailerCursor {
    /// Consumer identifier.
    pub consumer_id: String,
    /// Last successfully consumed sequence number.
    pub last_consumed: u64,
    /// Last acknowledged sequence number (confirmed by remote site).
    pub last_acknowledged: u64,
}

/// Journal tailer for streaming metadata changes to remote sites.
///
/// Each remote site creates one tailer instance. The tailer tracks
/// its cursor position and provides batched reads with optional
/// compaction (eliminating create+delete pairs within the same batch).
pub struct JournalTailer {
    journal: Arc<MetadataJournal>,
    config: TailerConfig,
    cursor: TailerCursor,
}

impl JournalTailer {
    /// Creates a new journal tailer starting from sequence 0.
    pub fn new(journal: Arc<MetadataJournal>, config: TailerConfig) -> Self {
        let cursor = TailerCursor {
            consumer_id: config.consumer_id.clone(),
            last_consumed: 0,
            last_acknowledged: 0,
        };
        Self {
            journal,
            config,
            cursor,
        }
    }

    /// Creates a tailer resuming from a previously saved cursor.
    pub fn resume(
        journal: Arc<MetadataJournal>,
        config: TailerConfig,
        cursor: TailerCursor,
    ) -> Self {
        Self {
            journal,
            config,
            cursor,
        }
    }

    /// Polls for the next batch of journal entries.
    ///
    /// Returns None if there are no new entries since the last poll.
    /// Returns Some(batch) with up to `batch_size` entries.
    pub fn poll_batch(&mut self) -> Result<Option<ReplicationBatch>, MetaError> {
        let from_seq = self.cursor.last_consumed + 1;
        let entries = self.journal.read_from(from_seq, self.config.batch_size)?;

        if entries.is_empty() {
            return Ok(None);
        }

        let first_sequence = entries.first().map(|e| e.sequence).unwrap_or(0);
        let last_sequence = entries.last().map(|e| e.sequence).unwrap_or(0);

        let (entries, compacted_count) = if self.config.enable_compaction {
            compact_batch(entries)
        } else {
            (entries, 0)
        };

        self.cursor.last_consumed = last_sequence;

        Ok(Some(ReplicationBatch {
            entries,
            first_sequence,
            last_sequence,
            compacted_count,
        }))
    }

    /// Acknowledges that a batch has been successfully applied at the remote site.
    ///
    /// This advances the acknowledged cursor, allowing the journal to compact
    /// entries that all consumers have acknowledged.
    pub fn acknowledge(&mut self, sequence: u64) {
        if sequence > self.cursor.last_acknowledged {
            self.cursor.last_acknowledged = sequence;
        }
    }

    /// Returns the current cursor state for persistence.
    pub fn cursor(&self) -> &TailerCursor {
        &self.cursor
    }

    /// Returns the replication lag (entries behind the journal head).
    pub fn lag(&self) -> Result<u64, MetaError> {
        self.journal.replication_lag(self.cursor.last_consumed)
    }

    /// Returns the consumer ID.
    pub fn consumer_id(&self) -> &str {
        &self.config.consumer_id
    }

    /// Returns true if there are unacknowledged entries.
    pub fn has_pending(&self) -> bool {
        self.cursor.last_consumed > self.cursor.last_acknowledged
    }

    /// Returns the number of unacknowledged entries.
    pub fn pending_count(&self) -> u64 {
        self.cursor
            .last_consumed
            .saturating_sub(self.cursor.last_acknowledged)
    }
}

/// Compacts a batch by removing operations that cancel out.
///
/// Per docs/metadata.md: "if a file is created and then deleted within the
/// same batch window, the net effect is 'nothing' — don't replicate
/// intermediate states."
///
/// Returns (compacted_entries, number_removed).
fn compact_batch(entries: Vec<JournalEntry>) -> (Vec<JournalEntry>, usize) {
    use std::collections::HashMap;

    let mut created_inodes: HashMap<InodeId, usize> = HashMap::new();
    let mut deleted_inodes: HashMap<InodeId, usize> = HashMap::new();

    for (idx, entry) in entries.iter().enumerate() {
        match &entry.op {
            MetaOp::CreateInode { attr } => {
                created_inodes.insert(attr.ino, idx);
            }
            MetaOp::DeleteInode { ino } => {
                deleted_inodes.insert(*ino, idx);
            }
            _ => {}
        }
    }

    let mut skip_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (ino, create_idx) in &created_inodes {
        if let Some(delete_idx) = deleted_inodes.get(ino) {
            if delete_idx > create_idx {
                skip_indices.insert(*create_idx);
                skip_indices.insert(*delete_idx);
            }
        }
    }

    let original_len = entries.len();
    let compacted: Vec<JournalEntry> = entries
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !skip_indices.contains(idx))
        .map(|(_, entry)| entry)
        .collect();
    let removed = original_len - compacted.len();

    (compacted, removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{InodeAttr, LogIndex};

    fn make_journal() -> Arc<MetadataJournal> {
        Arc::new(MetadataJournal::new(1, 10000))
    }

    fn create_op(ino: u64) -> MetaOp {
        let attr = InodeAttr::new_file(InodeId::new(ino), 1000, 1000, 0o644, 1);
        MetaOp::CreateInode { attr }
    }

    fn delete_op(ino: u64) -> MetaOp {
        MetaOp::DeleteInode {
            ino: InodeId::new(ino),
        }
    }

    #[test]
    fn test_poll_empty_journal() {
        let journal = make_journal();
        let config = TailerConfig::default();
        let mut tailer = JournalTailer::new(journal, config);

        let batch = tailer.poll_batch().unwrap();
        assert!(batch.is_none());
    }

    #[test]
    fn test_poll_batch_returns_entries() {
        let journal = make_journal();
        journal.append(create_op(100), LogIndex::new(1)).unwrap();
        journal.append(create_op(200), LogIndex::new(2)).unwrap();
        journal.append(create_op(300), LogIndex::new(3)).unwrap();

        let config = TailerConfig {
            batch_size: 10,
            enable_compaction: false,
            ..Default::default()
        };
        let mut tailer = JournalTailer::new(journal, config);

        let batch = tailer.poll_batch().unwrap().unwrap();
        assert_eq!(batch.entries.len(), 3);
        assert_eq!(batch.first_sequence, 1);
        assert_eq!(batch.last_sequence, 3);
    }

    #[test]
    fn test_poll_respects_batch_size() {
        let journal = make_journal();
        for i in 1..=10 {
            journal
                .append(create_op(100 + i), LogIndex::new(i))
                .unwrap();
        }

        let config = TailerConfig {
            batch_size: 3,
            enable_compaction: false,
            ..Default::default()
        };
        let mut tailer = JournalTailer::new(journal, config);

        let batch1 = tailer.poll_batch().unwrap().unwrap();
        assert_eq!(batch1.entries.len(), 3);
        assert_eq!(batch1.first_sequence, 1);
        assert_eq!(batch1.last_sequence, 3);

        let batch2 = tailer.poll_batch().unwrap().unwrap();
        assert_eq!(batch2.entries.len(), 3);
        assert_eq!(batch2.first_sequence, 4);
        assert_eq!(batch2.last_sequence, 6);
    }

    #[test]
    fn test_poll_no_new_entries() {
        let journal = make_journal();
        journal.append(create_op(100), LogIndex::new(1)).unwrap();

        let config = TailerConfig {
            batch_size: 10,
            enable_compaction: false,
            ..Default::default()
        };
        let mut tailer = JournalTailer::new(journal.clone(), config);

        let batch1 = tailer.poll_batch().unwrap().unwrap();
        assert_eq!(batch1.entries.len(), 1);

        let batch2 = tailer.poll_batch().unwrap();
        assert!(batch2.is_none());

        journal.append(create_op(200), LogIndex::new(2)).unwrap();
        let batch3 = tailer.poll_batch().unwrap().unwrap();
        assert_eq!(batch3.entries.len(), 1);
        assert_eq!(batch3.first_sequence, 2);
    }

    #[test]
    fn test_acknowledge_advances_cursor() {
        let journal = make_journal();
        journal.append(create_op(100), LogIndex::new(1)).unwrap();

        let config = TailerConfig::default();
        let mut tailer = JournalTailer::new(journal, config);

        tailer.poll_batch().unwrap();
        assert!(tailer.has_pending());
        assert_eq!(tailer.pending_count(), 1);

        tailer.acknowledge(1);
        assert!(!tailer.has_pending());
        assert_eq!(tailer.pending_count(), 0);
    }

    #[test]
    fn test_lag() {
        let journal = make_journal();
        for i in 1..=5 {
            journal
                .append(create_op(100 + i), LogIndex::new(i))
                .unwrap();
        }

        let config = TailerConfig {
            batch_size: 2,
            enable_compaction: false,
            ..Default::default()
        };
        let mut tailer = JournalTailer::new(journal, config);

        assert_eq!(tailer.lag().unwrap(), 5);

        tailer.poll_batch().unwrap();
        assert_eq!(tailer.lag().unwrap(), 3);
    }

    #[test]
    fn test_resume_from_cursor() {
        let journal = make_journal();
        for i in 1..=5 {
            journal
                .append(create_op(100 + i), LogIndex::new(i))
                .unwrap();
        }

        let cursor = TailerCursor {
            consumer_id: "site-b".to_string(),
            last_consumed: 3,
            last_acknowledged: 2,
        };
        let config = TailerConfig {
            consumer_id: "site-b".to_string(),
            batch_size: 10,
            enable_compaction: false,
        };
        let mut tailer = JournalTailer::resume(journal, config, cursor);

        let batch = tailer.poll_batch().unwrap().unwrap();
        assert_eq!(batch.entries.len(), 2);
        assert_eq!(batch.first_sequence, 4);
        assert_eq!(batch.last_sequence, 5);
    }

    #[test]
    fn test_compact_batch_create_then_delete() {
        let journal = make_journal();
        journal.append(create_op(100), LogIndex::new(1)).unwrap();
        journal.append(create_op(200), LogIndex::new(2)).unwrap();
        journal.append(delete_op(100), LogIndex::new(3)).unwrap();

        let config = TailerConfig {
            batch_size: 10,
            enable_compaction: true,
            ..Default::default()
        };
        let mut tailer = JournalTailer::new(journal, config);

        let batch = tailer.poll_batch().unwrap().unwrap();
        assert_eq!(batch.entries.len(), 1);
        assert_eq!(batch.compacted_count, 2);
    }

    #[test]
    fn test_compact_batch_no_compaction_possible() {
        let journal = make_journal();
        journal.append(create_op(100), LogIndex::new(1)).unwrap();
        journal.append(create_op(200), LogIndex::new(2)).unwrap();
        journal.append(create_op(300), LogIndex::new(3)).unwrap();

        let config = TailerConfig {
            batch_size: 10,
            enable_compaction: true,
            ..Default::default()
        };
        let mut tailer = JournalTailer::new(journal, config);

        let batch = tailer.poll_batch().unwrap().unwrap();
        assert_eq!(batch.entries.len(), 3);
        assert_eq!(batch.compacted_count, 0);
    }

    #[test]
    fn test_consumer_id() {
        let journal = make_journal();
        let config = TailerConfig {
            consumer_id: "site-b".to_string(),
            ..Default::default()
        };
        let tailer = JournalTailer::new(journal, config);
        assert_eq!(tailer.consumer_id(), "site-b");
    }
}
