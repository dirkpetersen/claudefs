//! Write-ahead journal for the storage engine.
//!
//! This module implements the D3 architecture decision: writes go to a 2x
//! synchronous write journal before being acked to the client, then are
//! asynchronously packed into 2MB EC segments.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::checksum::{compute, verify, Checksum, ChecksumAlgorithm};
use crate::error::StorageResult;

/// Operation types that can be recorded in the journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalOp {
    /// Write operation with associated data payload.
    Write {
        /// The data payload to write.
        data: Vec<u8>,
    },
    /// Truncate operation to shrink/extend a file.
    Truncate {
        /// The new size of the file in bytes.
        new_size: u64,
    },
    /// Delete operation to remove an inode.
    Delete,
    /// Mkdir operation to create a directory.
    Mkdir,
    /// Fsync marker to indicate a sync point.
    Fsync,
}

impl JournalOp {
    /// Returns the size of the data payload in bytes, if any.
    pub fn data_len(&self) -> u32 {
        match self {
            JournalOp::Write { data } => data.len() as u32,
            JournalOp::Truncate { .. } => 0,
            JournalOp::Delete => 0,
            JournalOp::Mkdir => 0,
            JournalOp::Fsync => 0,
        }
    }
}

/// Sync mode for journal persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SyncMode {
    /// fsync after every write (safest, slowest).
    #[default]
    Sync,
    /// fsync after batching N writes (balanced).
    BatchSync,
    /// Periodic fsync (fastest, risk of recent data loss).
    AsyncSync,
}

/// Configuration for the write journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalConfig {
    /// Maximum journal size in bytes (default 256MB).
    pub max_journal_size: u64,
    /// Sync strategy for journal persistence.
    pub sync_mode: SyncMode,
    /// Default checksum algorithm for entries.
    pub checksum_algo: ChecksumAlgorithm,
    /// Maximum number of entries per batch (default 64).
    pub max_batch_size: usize,
    /// Batch timeout in microseconds (default 500).
    pub batch_timeout_us: u64,
}

impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            max_journal_size: 256 * 1024 * 1024, // 256MB
            sync_mode: SyncMode::BatchSync,
            checksum_algo: ChecksumAlgorithm::Crc32c,
            max_batch_size: 64,
            batch_timeout_us: 500,
        }
    }
}

/// Statistics for the write journal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JournalStats {
    /// Total number of entries appended to the journal.
    pub entries_appended: u64,
    /// Total number of entries committed (synced).
    pub entries_committed: u64,
    /// Total number of entries truncated (removed after segment packing).
    pub entries_truncated: u64,
    /// Total bytes written to the journal.
    pub bytes_written: u64,
    /// Total number of commit operations.
    pub commits: u64,
    /// Number of batch flush operations.
    pub batch_flushes: u64,
}

/// A single entry in the write journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number.
    pub sequence: u64,
    /// Nanosecond timestamp of the entry.
    pub timestamp_ns: u64,
    /// Inode being written.
    pub inode: u64,
    /// Byte offset within the file.
    pub offset: u64,
    /// Integrity check of the data.
    pub data_checksum: Checksum,
    /// Length of data payload.
    pub data_len: u32,
    /// The operation type.
    pub op: JournalOp,
}

/// Write-ahead journal implementation.
///
/// Manages a circular buffer of journal entries that are synced
/// before being acknowledged to clients.
pub struct WriteJournal {
    /// Journal configuration.
    config: JournalConfig,
    /// In-memory buffer of journal entries.
    entries: Vec<JournalEntry>,
    /// Next sequence number to assign.
    next_sequence: u64,
    /// Total bytes currently in the journal.
    total_bytes: u64,
    /// Last committed (synced) sequence number.
    committed_sequence: u64,
    /// Journal statistics.
    stats: JournalStats,
}

impl WriteJournal {
    /// Creates a new WriteJournal with the given configuration.
    pub fn new(config: JournalConfig) -> Self {
        tracing::debug!(
            max_journal_size = config.max_journal_size,
            sync_mode = ?config.sync_mode,
            "creating write journal"
        );
        Self {
            config,
            entries: Vec::new(),
            next_sequence: 1,
            total_bytes: 0,
            committed_sequence: 0,
            stats: JournalStats::default(),
        }
    }

    /// Appends a new operation to the journal.
    ///
    /// Returns the sequence number assigned to this entry.
    pub fn append(&mut self, op: JournalOp, inode: u64, offset: u64) -> StorageResult<u64> {
        let sequence = self.next_sequence;
        self.next_sequence += 1;

        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let data_len = op.data_len();

        let data_checksum = match &op {
            JournalOp::Write { data } => compute(self.config.checksum_algo, data),
            _ => Checksum::new(self.config.checksum_algo, 0),
        };

        let entry_size = estimate_entry_size(&op);

        self.entries.push(JournalEntry {
            sequence,
            timestamp_ns,
            inode,
            offset,
            data_checksum,
            data_len,
            op,
        });

        self.total_bytes += entry_size as u64;
        self.stats.entries_appended += 1;
        self.stats.bytes_written += entry_size as u64;

        tracing::debug!(
            sequence = sequence,
            inode = inode,
            offset = offset,
            data_len = data_len,
            total_entries = self.entries.len(),
            "appended journal entry"
        );

        Ok(sequence)
    }

    /// Commits all pending entries, returning the committed sequence number.
    ///
    /// This marks all entries up to the current maximum sequence as durable.
    pub fn commit(&mut self) -> StorageResult<u64> {
        if self.entries.is_empty() {
            tracing::debug!("commit called on empty journal");
            return Ok(self.committed_sequence);
        }

        let committed_seq = self.next_sequence - 1;
        let newly_committed = if self.committed_sequence == 0 {
            self.entries.len()
        } else {
            self.entries
                .iter()
                .filter(|e| e.sequence > self.committed_sequence)
                .count()
        };

        self.committed_sequence = committed_seq;
        self.stats.entries_committed += newly_committed as u64;
        self.stats.commits += 1;

        tracing::debug!(
            committed_sequence = committed_seq,
            newly_committed = newly_committed,
            "committed journal entries"
        );

        Ok(committed_seq)
    }

    /// Returns all entries since the given sequence number (inclusive).
    ///
    /// This is used for recovery and segment packing.
    pub fn entries_since(&self, sequence: u64) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|e| e.sequence >= sequence)
            .collect()
    }

    /// Truncates entries before the given sequence number.
    ///
    /// This is called after entries have been packed into segments.
    /// Returns the number of entries removed.
    pub fn truncate_before(&mut self, sequence: u64) -> StorageResult<usize> {
        let (removed_entries, retained): (Vec<JournalEntry>, Vec<JournalEntry>) =
            self.entries.drain(..).partition(|e| e.sequence < sequence);

        let removed_count = removed_entries.len();

        self.total_bytes = retained
            .iter()
            .map(|e| estimate_entry_size(&e.op) as u64)
            .sum();
        self.stats.entries_truncated += removed_count as u64;

        if removed_count > 0 {
            tracing::debug!(
                removed_count = removed_count,
                retained_count = retained.len(),
                sequence = sequence,
                "truncated journal entries"
            );
        }

        self.entries = retained;
        Ok(removed_count)
    }

    /// Checks if the journal has reached its maximum size.
    pub fn is_full(&self) -> bool {
        self.total_bytes >= self.config.max_journal_size
    }

    /// Returns the number of entries not yet committed.
    pub fn pending_count(&self) -> usize {
        if self.committed_sequence == 0 {
            self.entries.len()
        } else {
            self.entries
                .iter()
                .filter(|e| e.sequence > self.committed_sequence)
                .count()
        }
    }

    /// Returns the total number of entries in the journal.
    pub fn total_entries(&self) -> usize {
        self.entries.len()
    }

    /// Returns a reference to the journal statistics.
    pub fn stats(&self) -> &JournalStats {
        &self.stats
    }

    /// Returns the current sequence number.
    pub fn current_sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    /// Returns the last committed sequence number.
    pub fn committed_sequence(&self) -> u64 {
        self.committed_sequence
    }

    /// Returns the configuration.
    pub fn config(&self) -> &JournalConfig {
        &self.config
    }

    /// Returns the total bytes in the journal.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Verifies the checksum of an entry's data (if applicable).
    pub fn verify_entry(&self, entry: &JournalEntry) -> bool {
        match &entry.op {
            JournalOp::Write { data } => verify(&entry.data_checksum, data),
            _ => true,
        }
    }
}

/// Estimates the size of an entry in bytes for accounting purposes.
fn estimate_entry_size(op: &JournalOp) -> usize {
    match op {
        JournalOp::Write { data } => {
            // Base size + data size
            // sequence(8) + timestamp_ns(8) + inode(8) + offset(8) + data_checksum(16) + data_len(4) + op discriminant
            64 + data.len()
        }
        JournalOp::Truncate { .. } => 64,
        JournalOp::Delete => 64,
        JournalOp::Mkdir => 64,
        JournalOp::Fsync => 64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_journal() -> WriteJournal {
        WriteJournal::new(JournalConfig::default())
    }

    #[test]
    fn test_journal_config_default() {
        let config = JournalConfig::default();
        assert_eq!(config.max_journal_size, 256 * 1024 * 1024);
        assert_eq!(config.sync_mode, SyncMode::BatchSync);
        assert_eq!(config.checksum_algo, ChecksumAlgorithm::Crc32c);
        assert_eq!(config.max_batch_size, 64);
        assert_eq!(config.batch_timeout_us, 500);
    }

    #[test]
    fn test_basic_append_and_commit() {
        let mut journal = create_test_journal();

        let seq = journal
            .append(
                JournalOp::Write {
                    data: b"hello".to_vec(),
                },
                1,
                0,
            )
            .unwrap();

        assert_eq!(seq, 1);

        let committed = journal.commit().unwrap();
        assert_eq!(committed, 1);

        assert_eq!(journal.pending_count(), 0);
        assert_eq!(journal.total_entries(), 1);
    }

    #[test]
    fn test_sequence_number_monotonicity() {
        let mut journal = create_test_journal();

        let seq1 = journal
            .append(
                JournalOp::Write {
                    data: b"a".to_vec(),
                },
                1,
                0,
            )
            .unwrap();

        let seq2 = journal
            .append(
                JournalOp::Write {
                    data: b"b".to_vec(),
                },
                1,
                10,
            )
            .unwrap();

        let seq3 = journal
            .append(JournalOp::Truncate { new_size: 100 }, 1, 0)
            .unwrap();

        assert!(seq1 < seq2);
        assert!(seq2 < seq3);
        assert_eq!(journal.current_sequence(), 3);
    }

    #[test]
    fn test_entries_since() {
        let mut journal = create_test_journal();

        journal
            .append(
                JournalOp::Write {
                    data: b"a".to_vec(),
                },
                1,
                0,
            )
            .unwrap();
        journal
            .append(
                JournalOp::Write {
                    data: b"b".to_vec(),
                },
                1,
                10,
            )
            .unwrap();
        journal
            .append(
                JournalOp::Write {
                    data: b"c".to_vec(),
                },
                1,
                20,
            )
            .unwrap();

        let entries = journal.entries_since(1);
        assert_eq!(entries.len(), 3);

        let entries = journal.entries_since(2);
        assert_eq!(entries.len(), 2);

        let entries = journal.entries_since(3);
        assert_eq!(entries.len(), 1);

        let entries = journal.entries_since(4);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_truncate_reclaims_space() {
        let mut journal = create_test_journal();

        for i in 0..10 {
            journal
                .append(
                    JournalOp::Write {
                        data: vec![i as u8; 100],
                    },
                    1,
                    i as u64 * 100,
                )
                .unwrap();
        }

        assert_eq!(journal.total_entries(), 10);

        // Commit all entries up to sequence 5
        journal.commit().unwrap();

        // Truncate entries before sequence 5
        let removed = journal.truncate_before(5).unwrap();
        assert_eq!(removed, 4); // sequences 1-4 removed

        assert_eq!(journal.total_entries(), 6); // 5,6,7,8,9,10 remain
    }

    #[test]
    fn test_journal_full_detection() {
        let mut config = JournalConfig::default();
        config.max_journal_size = 1000; // Small limit for testing

        let mut journal = WriteJournal::new(config);

        let mut appended = 0;
        while !journal.is_full() {
            journal
                .append(
                    JournalOp::Write {
                        data: vec![0u8; 100],
                    },
                    1,
                    appended as u64 * 100,
                )
                .unwrap();
            appended += 1;
        }

        assert!(appended > 0);
        assert!(journal.is_full());
    }

    #[test]
    fn test_batch_commit_groups_entries() {
        let mut journal = create_test_journal();

        // Append multiple entries
        for i in 0..5 {
            journal
                .append(
                    JournalOp::Write {
                        data: vec![i as u8; 10],
                    },
                    1,
                    i as u64 * 10,
                )
                .unwrap();
        }

        let committed = journal.commit().unwrap();

        // All 5 entries should be committed
        assert_eq!(committed, 5);
        assert_eq!(journal.committed_sequence(), 5);
        assert_eq!(journal.pending_count(), 0);
    }

    #[test]
    fn test_stats_tracking() {
        let mut journal = create_test_journal();

        // Append entries
        journal
            .append(
                JournalOp::Write {
                    data: b"test".to_vec(),
                },
                1,
                0,
            )
            .unwrap();
        journal.append(JournalOp::Mkdir, 2, 0).unwrap();
        journal.append(JournalOp::Fsync, 1, 0).unwrap();

        let stats = journal.stats();
        assert_eq!(stats.entries_appended, 3);

        // Commit
        journal.commit().unwrap();

        let stats = journal.stats();
        assert_eq!(stats.commits, 1);
        assert_eq!(stats.entries_committed, 3);
    }

    #[test]
    fn test_various_journal_op_types() {
        let mut journal = create_test_journal();

        // Test Write
        let seq = journal
            .append(
                JournalOp::Write {
                    data: b"hello world".to_vec(),
                },
                1,
                0,
            )
            .unwrap();
        assert_eq!(seq, 1);

        // Test Truncate
        let seq = journal
            .append(JournalOp::Truncate { new_size: 4096 }, 1, 0)
            .unwrap();
        assert_eq!(seq, 2);

        // Test Delete
        let seq = journal.append(JournalOp::Delete, 2, 0).unwrap();
        assert_eq!(seq, 3);

        // Test Mkdir
        let seq = journal.append(JournalOp::Mkdir, 3, 0).unwrap();
        assert_eq!(seq, 4);

        // Test Fsync
        let seq = journal.append(JournalOp::Fsync, 1, 0).unwrap();
        assert_eq!(seq, 5);

        journal.commit().unwrap();

        assert_eq!(journal.total_entries(), 5);
    }

    #[test]
    fn test_empty_journal_operations() {
        let journal = create_test_journal();

        // Empty journal operations
        assert!(!journal.is_full());
        assert_eq!(journal.pending_count(), 0);
        assert_eq!(journal.total_entries(), 0);
        assert_eq!(journal.committed_sequence(), 0);

        let entries = journal.entries_since(0);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_empty_commit() {
        let mut journal = create_test_journal();

        let committed = journal.commit().unwrap();

        assert_eq!(committed, 0);
        assert_eq!(journal.committed_sequence(), 0);
    }

    #[test]
    fn test_empty_truncate() {
        let mut journal = create_test_journal();

        // Truncate on empty journal
        let removed = journal.truncate_before(5).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_large_batch_handling() {
        let mut journal = create_test_journal();

        // Add many entries
        for i in 0..100 {
            journal
                .append(
                    JournalOp::Write {
                        data: vec![i as u8; 100],
                    },
                    i % 10,
                    i as u64 * 100,
                )
                .unwrap();
        }

        assert_eq!(journal.total_entries(), 100);

        let committed = journal.commit().unwrap();
        assert_eq!(committed, 100);

        let stats = journal.stats();
        assert_eq!(stats.entries_appended, 100);
        assert_eq!(stats.commits, 1);
    }

    #[test]
    fn test_truncate_with_no_matching_entries() {
        let mut journal = create_test_journal();

        // Add some entries
        journal
            .append(
                JournalOp::Write {
                    data: b"a".to_vec(),
                },
                1,
                0,
            )
            .unwrap();
        journal
            .append(
                JournalOp::Write {
                    data: b"b".to_vec(),
                },
                1,
                10,
            )
            .unwrap();

        // Try to truncate with sequence lower than any entry
        let removed = journal.truncate_before(0).unwrap();

        // Should remove nothing since all entries have sequence > 0
        assert_eq!(removed, 0);
        assert_eq!(journal.total_entries(), 2);
    }

    #[test]
    fn test_checksum_verification() {
        let mut journal = create_test_journal();

        let data = b"test data for checksum";
        let _seq = journal
            .append(
                JournalOp::Write {
                    data: data.to_vec(),
                },
                1,
                0,
            )
            .unwrap();

        let entry = &journal.entries[0];
        assert!(journal.verify_entry(entry));

        // Modify the data in the entry to test verification failure
        let mut corrupted_entry = entry.clone();
        if let JournalOp::Write { data } = &mut corrupted_entry.op {
            data[0] = 0xFF;
        }
        assert!(!journal.verify_entry(&corrupted_entry));
    }

    #[test]
    fn test_multiple_commit_cycles() {
        let mut journal = create_test_journal();

        // First cycle
        for _ in 0..5 {
            journal
                .append(
                    JournalOp::Write {
                        data: b"test".to_vec(),
                    },
                    1,
                    0,
                )
                .unwrap();
        }
        journal.commit().unwrap();

        // Second cycle
        for _ in 0..3 {
            journal
                .append(
                    JournalOp::Write {
                        data: b"test".to_vec(),
                    },
                    1,
                    0,
                )
                .unwrap();
        }
        journal.commit().unwrap();

        // Third cycle
        for _ in 0..7 {
            journal
                .append(
                    JournalOp::Write {
                        data: b"test".to_vec(),
                    },
                    1,
                    0,
                )
                .unwrap();
        }
        journal.commit().unwrap();

        let stats = journal.stats();
        assert_eq!(stats.commits, 3);
        assert_eq!(stats.entries_appended, 15);
        assert_eq!(stats.entries_committed, 15);
        assert_eq!(journal.committed_sequence(), 15);
    }

    #[test]
    fn test_pending_count_accuracy() {
        let mut journal = create_test_journal();

        // No pending initially
        assert_eq!(journal.pending_count(), 0);

        // Add some entries
        journal
            .append(
                JournalOp::Write {
                    data: b"a".to_vec(),
                },
                1,
                0,
            )
            .unwrap();
        journal
            .append(
                JournalOp::Write {
                    data: b"b".to_vec(),
                },
                1,
                10,
            )
            .unwrap();

        // Should have 2 pending
        assert_eq!(journal.pending_count(), 2);

        // Commit one
        journal.commit().unwrap();

        // Should have 0 pending after commit
        assert_eq!(journal.pending_count(), 0);

        // Add more
        journal
            .append(
                JournalOp::Write {
                    data: b"c".to_vec(),
                },
                1,
                20,
            )
            .unwrap();

        // Should have 1 pending
        assert_eq!(journal.pending_count(), 1);
    }

    #[test]
    fn test_journal_op_data_len() {
        let write_op = JournalOp::Write {
            data: vec![0u8; 1234],
        };
        assert_eq!(write_op.data_len(), 1234);

        let truncate_op = JournalOp::Truncate { new_size: 1000 };
        assert_eq!(truncate_op.data_len(), 0);

        let delete_op = JournalOp::Delete;
        assert_eq!(delete_op.data_len(), 0);

        let mkdir_op = JournalOp::Mkdir;
        assert_eq!(mkdir_op.data_len(), 0);

        let fsync_op = JournalOp::Fsync;
        assert_eq!(fsync_op.data_len(), 0);
    }

    #[test]
    fn test_sync_mode_variants() {
        let sync = SyncMode::Sync;
        let batch_sync = SyncMode::BatchSync;
        let async_sync = SyncMode::AsyncSync;

        assert_ne!(sync, batch_sync);
        assert_ne!(batch_sync, async_sync);
        assert_ne!(sync, async_sync);
    }

    #[test]
    fn test_journal_stats_clone() {
        let stats = JournalStats {
            entries_appended: 100,
            entries_committed: 80,
            entries_truncated: 50,
            bytes_written: 5000,
            commits: 10,
            batch_flushes: 5,
        };

        let cloned = stats.clone();
        assert_eq!(stats.entries_appended, cloned.entries_appended);
        assert_eq!(stats.commits, cloned.commits);
    }

    #[test]
    fn test_config_custom_values() {
        let config = JournalConfig {
            max_journal_size: 512 * 1024 * 1024,
            sync_mode: SyncMode::AsyncSync,
            checksum_algo: ChecksumAlgorithm::XxHash64,
            max_batch_size: 128,
            batch_timeout_us: 1000,
        };

        let journal = WriteJournal::new(config);

        assert_eq!(journal.config().max_journal_size, 512 * 1024 * 1024);
        assert_eq!(journal.config().sync_mode, SyncMode::AsyncSync);
        assert_eq!(journal.config().checksum_algo, ChecksumAlgorithm::XxHash64);
    }

    #[test]
    fn test_total_bytes_calculation() {
        let mut config = JournalConfig::default();
        config.max_journal_size = 10000;

        let mut journal = WriteJournal::new(config);

        // Add entries and check total_bytes increases
        let initial_bytes = journal.total_bytes();

        journal
            .append(
                JournalOp::Write {
                    data: vec![0u8; 100],
                },
                1,
                0,
            )
            .unwrap();
        let after_first = journal.total_bytes();
        assert!(after_first > initial_bytes);

        journal
            .append(
                JournalOp::Write {
                    data: vec![0u8; 200],
                },
                1,
                100,
            )
            .unwrap();
        let after_second = journal.total_bytes();
        assert!(after_second > after_first);

        // Commit doesn't change bytes
        journal.commit().unwrap();
        assert_eq!(journal.total_bytes(), after_second);

        // Truncate should decrease bytes (truncate_before(2) removes sequence 1)
        journal.truncate_before(2).unwrap();
        let after_truncate = journal.total_bytes();
        assert!(after_truncate < after_second);
    }

    #[test]
    fn test_entry_sequence_after_truncate() {
        let mut journal = create_test_journal();

        // Add entries
        for i in 0..5 {
            journal
                .append(
                    JournalOp::Write {
                        data: vec![i as u8],
                    },
                    1,
                    i as u64,
                )
                .unwrap();
        }

        // Commit
        journal.commit().unwrap();

        // Truncate all
        journal.truncate_before(5).unwrap();

        // After truncate, append should continue from next_sequence
        let next_seq = journal
            .append(
                JournalOp::Write {
                    data: b"new".to_vec(),
                },
                1,
                100,
            )
            .unwrap();

        assert_eq!(next_seq, 6); // Should be 6, not wrap back to 1
    }
}
