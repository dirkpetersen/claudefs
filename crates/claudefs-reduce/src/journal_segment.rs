//! Write-ahead journal segment for crash-consistent writes (D3).
//!
//! D3: Write journal — 2x synchronous replication to two nodes before ack to client.
//! The journal segment packs multiple chunk writes into a sequential log for durability.
//! On crash recovery, the journal is replayed to reconstruct the write-ahead state.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A single entry in the write-ahead journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number.
    pub sequence: u64,
    /// Inode being written to.
    pub inode_id: u64,
    /// Byte offset within the inode.
    pub offset: u64,
    /// BLAKE3 hash of the chunk.
    pub chunk_hash: [u8; 32],
    /// Size of the chunk in bytes.
    pub chunk_size: u32,
    /// The chunk data payload.
    pub data: Vec<u8>,
}

/// Configuration for a journal segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalConfig {
    /// Maximum number of entries before the journal is considered full.
    pub max_entries: usize,
    /// Maximum total bytes of data before the journal is considered full.
    pub max_bytes: usize,
}

impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            max_entries: 4096,
            max_bytes: 32 * 1024 * 1024, // 32MB
        }
    }
}

/// State of a journal segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalState {
    /// Open for writes.
    Open,
    /// Sealed — no more writes allowed, awaiting replication.
    Sealed,
    /// Checkpointed — replicated and safe.
    Checkpointed,
}

/// A write-ahead journal segment.
#[derive(Debug)]
pub struct JournalSegment {
    config: JournalConfig,
    state: JournalState,
    entries: Vec<JournalEntry>,
    total_bytes: usize,
}

impl JournalSegment {
    /// Create a new journal segment with the given configuration.
    pub fn new(config: JournalConfig) -> Self {
        Self {
            config,
            state: JournalState::Open,
            entries: Vec::new(),
            total_bytes: 0,
        }
    }

    /// Append an entry to the journal.
    /// Returns an error if the journal is sealed or full.
    pub fn append(&mut self, entry: JournalEntry) -> Result<(), JournalError> {
        if self.state != JournalState::Open {
            return Err(JournalError::Sealed);
        }

        if self.is_full() {
            return Err(JournalError::Full);
        }

        let new_bytes = self.total_bytes + entry.data.len();
        if self.entries.len() >= self.config.max_entries || new_bytes > self.config.max_bytes {
            return Err(JournalError::Full);
        }

        if let Some(last) = self.entries.last() {
            if entry.sequence <= last.sequence {
                return Err(JournalError::InvalidSequence);
            }
        }

        self.total_bytes = new_bytes;
        self.entries.push(entry);
        Ok(())
    }

    /// Seal the journal — no more writes allowed.
    pub fn seal(&mut self) {
        self.state = JournalState::Sealed;
    }

    /// Checkpoint the journal — replicated and safe.
    pub fn checkpoint(&mut self) {
        self.state = JournalState::Checkpointed;
    }

    /// Returns the current state of the journal.
    pub fn state(&self) -> JournalState {
        self.state
    }

    /// Returns a slice of all entries.
    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    /// Returns the number of entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the total bytes of all entry data.
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Returns true if the journal is at capacity.
    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.config.max_entries || self.total_bytes >= self.config.max_bytes
    }

    /// Returns all entries in sequence order.
    pub fn replay(&self) -> Vec<&JournalEntry> {
        self.entries.iter().collect()
    }

    /// Returns entries with sequence greater than the given value.
    pub fn since(&self, sequence: u64) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|e| e.sequence > sequence)
            .collect()
    }
}

/// Errors that can occur during journal operations.
#[derive(Debug, Error)]
pub enum JournalError {
    /// Journal is at capacity.
    #[error("journal is full")]
    Full,
    /// Journal is sealed and cannot accept more writes.
    #[error("journal is sealed")]
    Sealed,
    /// Sequence number is out of order.
    #[error("invalid sequence number")]
    InvalidSequence,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(sequence: u64, inode_id: u64, offset: u64, data_size: usize) -> JournalEntry {
        JournalEntry {
            sequence,
            inode_id,
            offset,
            chunk_hash: [sequence as u8; 32],
            chunk_size: data_size as u32,
            data: vec![0u8; data_size],
        }
    }

    #[test]
    fn new_journal_is_open_state() {
        let journal = JournalSegment::new(JournalConfig::default());
        assert_eq!(journal.state(), JournalState::Open);
    }

    #[test]
    fn new_journal_is_empty() {
        let journal = JournalSegment::new(JournalConfig::default());
        assert!(journal.entries().is_empty());
        assert_eq!(journal.entry_count(), 0);
        assert_eq!(journal.total_bytes(), 0);
    }

    #[test]
    fn append_single_entry() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        let entry = make_entry(1, 42, 0, 100);
        journal.append(entry).unwrap();
        assert_eq!(journal.entry_count(), 1);
    }

    #[test]
    fn append_increments_count() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        journal.append(make_entry(1, 42, 0, 100)).unwrap();
        journal.append(make_entry(2, 42, 100, 100)).unwrap();
        assert_eq!(journal.entry_count(), 2);
    }

    #[test]
    fn seal_changes_state() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        assert_eq!(journal.state(), JournalState::Open);
        journal.seal();
        assert_eq!(journal.state(), JournalState::Sealed);
    }

    #[test]
    fn append_after_seal_returns_error() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        journal.seal();
        let entry = make_entry(1, 42, 0, 100);
        let result = journal.append(entry);
        assert!(matches!(result, Err(JournalError::Sealed)));
    }

    #[test]
    fn checkpoint_changes_state() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        journal.seal();
        journal.checkpoint();
        assert_eq!(journal.state(), JournalState::Checkpointed);
    }

    #[test]
    fn is_full_by_entry_count() {
        let config = JournalConfig {
            max_entries: 5,
            max_bytes: 1024 * 1024,
        };
        let mut journal = JournalSegment::new(config);

        for i in 0..5 {
            journal.append(make_entry(i + 1, 42, i * 100, 10)).unwrap();
        }

        assert!(journal.is_full());
    }

    #[test]
    fn is_full_by_bytes() {
        let config = JournalConfig {
            max_entries: 1000,
            max_bytes: 100,
        };
        let mut journal = JournalSegment::new(config);

        journal.append(make_entry(1, 42, 0, 60)).unwrap();
        assert!(!journal.is_full());

        journal.append(make_entry(2, 42, 60, 40)).unwrap();
        assert!(journal.is_full());
    }

    #[test]
    fn append_when_full_returns_error() {
        let config = JournalConfig {
            max_entries: 2,
            max_bytes: 1024 * 1024,
        };
        let mut journal = JournalSegment::new(config);

        journal.append(make_entry(1, 42, 0, 100)).unwrap();
        journal.append(make_entry(2, 42, 100, 100)).unwrap();

        let result = journal.append(make_entry(3, 42, 200, 100));
        assert!(matches!(result, Err(JournalError::Full)));
    }

    #[test]
    fn replay_returns_entries_in_order() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        journal.append(make_entry(1, 42, 0, 10)).unwrap();
        journal.append(make_entry(2, 42, 10, 10)).unwrap();
        journal.append(make_entry(3, 42, 20, 10)).unwrap();

        let entries = journal.replay();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[1].sequence, 2);
        assert_eq!(entries[2].sequence, 3);
    }

    #[test]
    fn since_returns_entries_after_sequence() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        journal.append(make_entry(1, 42, 0, 10)).unwrap();
        journal.append(make_entry(2, 42, 10, 10)).unwrap();
        journal.append(make_entry(3, 42, 20, 10)).unwrap();
        journal.append(make_entry(4, 42, 30, 10)).unwrap();

        let entries = journal.since(2);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 3);
        assert_eq!(entries[1].sequence, 4);
    }

    #[test]
    fn since_with_zero_returns_all() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        journal.append(make_entry(1, 42, 0, 10)).unwrap();
        journal.append(make_entry(2, 42, 10, 10)).unwrap();

        let entries = journal.since(0);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn entry_count_increments() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        assert_eq!(journal.entry_count(), 0);
        journal.append(make_entry(1, 42, 0, 10)).unwrap();
        assert_eq!(journal.entry_count(), 1);
        journal.append(make_entry(2, 42, 10, 10)).unwrap();
        assert_eq!(journal.entry_count(), 2);
    }

    #[test]
    fn total_bytes_sums_data_sizes() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        journal.append(make_entry(1, 42, 0, 100)).unwrap();
        journal.append(make_entry(2, 42, 100, 200)).unwrap();
        assert_eq!(journal.total_bytes(), 300);
    }

    #[test]
    fn journal_config_default_values() {
        let config = JournalConfig::default();
        assert_eq!(config.max_entries, 4096);
        assert_eq!(config.max_bytes, 32 * 1024 * 1024);
    }

    #[test]
    fn multiple_inodes_in_journal() {
        let mut journal = JournalSegment::new(JournalConfig::default());
        journal.append(make_entry(1, 42, 0, 10)).unwrap();
        journal.append(make_entry(2, 99, 0, 10)).unwrap();
        journal.append(make_entry(3, 42, 10, 10)).unwrap();

        let entries = journal.entries();
        assert_eq!(entries[0].inode_id, 42);
        assert_eq!(entries[1].inode_id, 99);
        assert_eq!(entries[2].inode_id, 42);
    }
}
