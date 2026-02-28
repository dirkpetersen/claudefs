//! Write-ahead journal for crash-consistent write coalescing.
//!
//! Per decisions.md D3: "Write journal — 2x synchronous replication to two
//! different nodes before ack to client."
//!
//! This module manages the write-ahead journal for crash consistency and
//! write coalescing. It buffers writes in memory, tracks replication state,
//! and provides methods for flushing to journal device.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::block::{BlockRef, PlacementHint};
use crate::error::{StorageError, StorageResult};

/// A journal entry representing a pending write operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number
    pub sequence: u64,
    /// Target block reference
    pub block_ref: BlockRef,
    /// The data to be written
    pub data: Vec<u8>,
    /// FDP placement hint
    pub placement_hint: PlacementHint,
    /// Timestamp when the entry was created (seconds since epoch)
    pub timestamp_secs: u64,
}

impl JournalEntry {
    /// Creates a new journal entry with the given parameters.
    pub fn new(
        sequence: u64,
        block_ref: BlockRef,
        data: Vec<u8>,
        placement_hint: PlacementHint,
    ) -> Self {
        let timestamp_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            sequence,
            block_ref,
            data,
            placement_hint,
            timestamp_secs,
        }
    }
}

/// State of a journal entry in the flush pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum JournalEntryState {
    /// Entry is pending, not yet flushed
    #[default]
    Pending,
    /// Entry has been locally flushed to journal device
    LocalFlushed,
    /// Entry has been replicated to at least one remote node
    Replicated,
    /// Entry has been committed (data written to final location)
    Committed,
}

/// Statistics for the write journal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JournalStats {
    /// Number of pending entries
    pub pending_count: u64,
    /// Number of entries committed
    pub committed_count: u64,
    /// Total bytes in pending entries
    pub pending_bytes: u64,
    /// Current sequence number
    pub current_sequence: u64,
    /// High watermark of pending entries seen
    pub high_watermark: u64,
}

/// Configuration for the write journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalConfig {
    /// Maximum number of entries to buffer before forcing a flush
    pub max_pending_entries: usize,
    /// Maximum bytes to buffer before forcing a flush
    pub max_pending_bytes: u64,
    /// Whether to enable replication tracking (for integration with A6)
    pub replication_enabled: bool,
}

impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            max_pending_entries: 1024,
            max_pending_bytes: 64 * 1024 * 1024, // 64MB
            replication_enabled: false,
        }
    }
}

impl JournalConfig {
    /// Creates a new journal configuration with the given parameters.
    pub fn new(
        max_pending_entries: usize,
        max_pending_bytes: u64,
        replication_enabled: bool,
    ) -> Self {
        Self {
            max_pending_entries,
            max_pending_bytes,
            replication_enabled,
        }
    }
}

/// Internal state of the journal.
struct JournalInner {
    entries: VecDeque<(JournalEntry, JournalEntryState)>,
    next_sequence: u64,
    committed_count: u64,
    high_watermark: u64,
}

impl Default for JournalInner {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            next_sequence: 1,
            committed_count: 0,
            high_watermark: 0,
        }
    }
}

/// Write journal for crash-consistent write coalescing.
/// Buffers writes in memory, flushes to journal device, tracks replication state.
pub struct WriteJournal {
    config: JournalConfig,
    inner: Mutex<JournalInner>,
}

impl WriteJournal {
    /// Create a new write journal with the given configuration.
    pub fn new(config: JournalConfig) -> Self {
        tracing::debug!(
            "WriteJournal created: max_entries={}, max_bytes={}, replication={}",
            config.max_pending_entries,
            config.max_pending_bytes,
            config.replication_enabled
        );
        Self {
            config,
            inner: Mutex::new(JournalInner::default()),
        }
    }

    /// Append a write operation to the journal.
    /// Returns the assigned sequence number.
    pub fn append(
        &self,
        block_ref: BlockRef,
        data: Vec<u8>,
        hint: PlacementHint,
    ) -> StorageResult<u64> {
        let mut inner = self.inner.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire journal lock".to_string())
        })?;

        let sequence = inner.next_sequence;
        inner.next_sequence += 1;

        let entry = JournalEntry::new(sequence, block_ref, data, hint);
        let entry_size = entry.data.len() as u64;

        inner.entries.push_back((entry, JournalEntryState::Pending));

        // Update high watermark
        let pending_count = inner.entries.len() as u64;
        if pending_count > inner.high_watermark {
            inner.high_watermark = pending_count;
        }

        tracing::debug!(
            "Appended entry seq={}, pending={}, size={} bytes",
            sequence,
            inner.entries.len(),
            entry_size
        );

        Ok(sequence)
    }

    /// Find the index of an entry by sequence number.
    fn find_by_sequence(
        entries: &VecDeque<(JournalEntry, JournalEntryState)>,
        sequence: u64,
    ) -> Option<usize> {
        entries.iter().position(|(e, _)| e.sequence == sequence)
    }

    /// Mark an entry as locally flushed.
    pub fn mark_local_flushed(&self, sequence: u64) -> StorageResult<()> {
        let mut inner = self.inner.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire journal lock".to_string())
        })?;

        let idx = Self::find_by_sequence(&inner.entries, sequence).ok_or(
            StorageError::AllocatorError(format!("Sequence {} not found", sequence)),
        )?;

        inner.entries[idx].1 = JournalEntryState::LocalFlushed;
        tracing::debug!("Marked seq={} as LocalFlushed", sequence);
        Ok(())
    }

    /// Mark an entry as replicated.
    pub fn mark_replicated(&self, sequence: u64) -> StorageResult<()> {
        if !self.config.replication_enabled {
            tracing::warn!("Replication is not enabled but mark_replicated called");
        }

        let mut inner = self.inner.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire journal lock".to_string())
        })?;

        let idx = Self::find_by_sequence(&inner.entries, sequence).ok_or(
            StorageError::AllocatorError(format!("Sequence {} not found", sequence)),
        )?;

        inner.entries[idx].1 = JournalEntryState::Replicated;
        tracing::debug!("Marked seq={} as Replicated", sequence);
        Ok(())
    }

    /// Mark an entry as committed and remove it from the journal.
    pub fn commit(&self, sequence: u64) -> StorageResult<()> {
        let mut inner = self.inner.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire journal lock".to_string())
        })?;

        let idx = Self::find_by_sequence(&inner.entries, sequence).ok_or(
            StorageError::AllocatorError(format!("Sequence {} not found", sequence)),
        )?;

        // Verify the entry is in a valid state to commit
        let state = inner.entries[idx].1;
        if !matches!(
            state,
            JournalEntryState::LocalFlushed | JournalEntryState::Replicated
        ) {
            return Err(StorageError::AllocatorError(format!(
                "Cannot commit entry seq={} in state {:?}",
                sequence, state
            )));
        }

        inner.entries[idx].1 = JournalEntryState::Committed;
        inner.committed_count += 1;

        // Remove committed entries from the front (in order)
        while inner.entries.front().map(|(_, s)| *s) == Some(JournalEntryState::Committed) {
            inner.entries.pop_front();
        }

        tracing::debug!(
            "Committed seq={}, total_committed={}",
            sequence,
            inner.committed_count
        );
        Ok(())
    }

    /// Get all pending entries (not yet committed).
    pub fn pending_entries(&self) -> Vec<JournalEntry> {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return Vec::new(),
        };

        inner
            .entries
            .iter()
            .filter(|(_, state)| *state != JournalEntryState::Committed)
            .map(|(e, _)| e.clone())
            .collect()
    }

    /// Get pending entries filtered by state.
    pub fn pending_entries_by_state(&self, state: JournalEntryState) -> Vec<JournalEntry> {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return Vec::new(),
        };

        inner
            .entries
            .iter()
            .filter(|(_, s)| *s == state)
            .map(|(e, _)| e.clone())
            .collect()
    }

    /// Check if the journal needs to be flushed (exceeds thresholds).
    pub fn needs_flush(&self) -> bool {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return true,
        };

        if inner.entries.len() >= self.config.max_pending_entries {
            tracing::debug!(
                "Needs flush: {} entries >= {} max",
                inner.entries.len(),
                self.config.max_pending_entries
            );
            return true;
        }

        let total_bytes: u64 = inner.entries.iter().map(|(e, _)| e.data.len() as u64).sum();
        if total_bytes >= self.config.max_pending_bytes {
            tracing::debug!(
                "Needs flush: {} bytes >= {} max",
                total_bytes,
                self.config.max_pending_bytes
            );
            return true;
        }

        false
    }

    /// Get journal statistics.
    pub fn stats(&self) -> JournalStats {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return JournalStats::default(),
        };

        let pending_bytes: u64 = inner
            .entries
            .iter()
            .filter(|(_, s)| *s != JournalEntryState::Committed)
            .map(|(e, _)| e.data.len() as u64)
            .sum();

        let pending_count = inner
            .entries
            .iter()
            .filter(|(_, s)| *s != JournalEntryState::Committed)
            .count() as u64;

        JournalStats {
            pending_count,
            committed_count: inner.committed_count,
            pending_bytes,
            current_sequence: inner.next_sequence.saturating_sub(1),
            high_watermark: inner.high_watermark,
        }
    }

    /// Returns the number of pending entries.
    pub fn pending_count(&self) -> usize {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return 0,
        };

        inner
            .entries
            .iter()
            .filter(|(_, s)| *s != JournalEntryState::Committed)
            .count()
    }

    /// Returns the current configuration.
    pub fn config(&self) -> &JournalConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSize};

    fn test_block_ref() -> BlockRef {
        BlockRef {
            id: BlockId::new(0, 100),
            size: BlockSize::B4K,
        }
    }

    #[test]
    fn test_journal_append() {
        let config = JournalConfig::default();
        let journal = WriteJournal::new(config);

        let data1 = vec![0u8; 4096];
        let seq1 = journal
            .append(test_block_ref(), data1.clone(), PlacementHint::Journal)
            .unwrap();
        assert_eq!(seq1, 1);

        let data2 = vec![1u8; 8192];
        let seq2 = journal
            .append(test_block_ref(), data2.clone(), PlacementHint::Metadata)
            .unwrap();
        assert_eq!(seq2, 2);

        let stats = journal.stats();
        assert_eq!(stats.current_sequence, 2);
    }

    #[test]
    fn test_journal_state_transitions() {
        let config = JournalConfig::default();
        let journal = WriteJournal::new(config);

        let data = vec![0u8; 4096];
        let seq = journal
            .append(test_block_ref(), data, PlacementHint::Journal)
            .unwrap();

        // Initially Pending
        let pending = journal.pending_entries_by_state(JournalEntryState::Pending);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sequence, seq);

        // Mark as LocalFlushed
        journal.mark_local_flushed(seq).unwrap();
        let local_flushed = journal.pending_entries_by_state(JournalEntryState::LocalFlushed);
        assert_eq!(local_flushed.len(), 1);

        // Mark as Replicated
        journal.mark_replicated(seq).unwrap();
        let replicated = journal.pending_entries_by_state(JournalEntryState::Replicated);
        assert_eq!(replicated.len(), 1);

        // Commit
        journal.commit(seq).unwrap();
        let stats = journal.stats();
        assert_eq!(stats.committed_count, 1);
    }

    #[test]
    fn test_journal_commit_removes_entry() {
        let config = JournalConfig::default();
        let journal = WriteJournal::new(config);

        let data = vec![0u8; 4096];
        let seq = journal
            .append(test_block_ref(), data, PlacementHint::Journal)
            .unwrap();

        journal.mark_local_flushed(seq).unwrap();
        journal.commit(seq).unwrap();

        // Entry should be removed
        let pending = journal.pending_entries();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_journal_needs_flush() {
        let config = JournalConfig::new(3, 10000, false);
        let journal = WriteJournal::new(config);

        // Should not need flush initially
        assert!(!journal.needs_flush());

        // Add entries until we hit max_pending_entries
        for _i in 0..3 {
            let data = vec![0u8; 1000];
            journal
                .append(test_block_ref(), data, PlacementHint::Journal)
                .unwrap();
        }

        // Now should need flush
        assert!(journal.needs_flush());
    }

    #[test]
    fn test_journal_stats() {
        let config = JournalConfig::new(100, 100000, false);
        let journal = WriteJournal::new(config);

        let data1 = vec![0u8; 4096];
        journal
            .append(test_block_ref(), data1, PlacementHint::Journal)
            .unwrap();

        let data2 = vec![1u8; 8192];
        journal
            .append(test_block_ref(), data2, PlacementHint::Metadata)
            .unwrap();

        let stats = journal.stats();
        assert_eq!(stats.pending_count, 2);
        assert_eq!(stats.pending_bytes, 4096 + 8192);
        assert_eq!(stats.high_watermark, 2);
    }

    #[test]
    fn test_journal_pending_entries() {
        let config = JournalConfig::default();
        let journal = WriteJournal::new(config);

        let data = vec![0u8; 4096];
        let seq = journal
            .append(test_block_ref(), data, PlacementHint::Journal)
            .unwrap();

        let pending = journal.pending_entries();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sequence, seq);
    }

    #[test]
    fn test_journal_config_defaults() {
        let config = JournalConfig::default();
        assert_eq!(config.max_pending_entries, 1024);
        assert_eq!(config.max_pending_bytes, 64 * 1024 * 1024);
        assert!(!config.replication_enabled);
    }

    #[test]
    fn test_journal_invalid_operations() {
        let config = JournalConfig::default();
        let journal = WriteJournal::new(config);

        // Mark local flushed on non-existent sequence
        let result = journal.mark_local_flushed(999);
        assert!(result.is_err());

        // Mark replicated on non-existent sequence
        let result = journal.mark_replicated(999);
        assert!(result.is_err());

        // Commit on non-existent sequence
        let result = journal.commit(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_journal_commit_order() {
        let config = JournalConfig::default();
        let journal = WriteJournal::new(config);

        // Add multiple entries
        let seq1 = journal
            .append(test_block_ref(), vec![0u8; 1000], PlacementHint::Journal)
            .unwrap();
        let seq2 = journal
            .append(test_block_ref(), vec![1u8; 1000], PlacementHint::Journal)
            .unwrap();
        let seq3 = journal
            .append(test_block_ref(), vec![2u8; 1000], PlacementHint::Journal)
            .unwrap();

        // Mark and commit out of order
        journal.mark_local_flushed(seq2).unwrap();
        journal.mark_local_flushed(seq1).unwrap();

        // Commit seq2 first - should not remove seq1
        journal.commit(seq2).unwrap();
        let stats = journal.stats();
        assert_eq!(stats.committed_count, 1);
        assert_eq!(journal.pending_count(), 2);

        // Commit seq1 - seq3 should also be removed since it's at front now
        journal.commit(seq1).unwrap();
        assert_eq!(journal.pending_count(), 1);

        // Commit seq3
        journal.mark_local_flushed(seq3).unwrap();
        journal.commit(seq3).unwrap();
        assert_eq!(journal.pending_count(), 0);
    }

    #[test]
    fn test_journal_replication_disabled() {
        let config = JournalConfig::new(100, 100000, false);
        let journal = WriteJournal::new(config);

        let data = vec![0u8; 4096];
        let seq = journal
            .append(test_block_ref(), data, PlacementHint::Journal)
            .unwrap();

        // Should still work but log warning
        journal.mark_local_flushed(seq).unwrap();
        journal.mark_replicated(seq).unwrap();

        let replicated = journal.pending_entries_by_state(JournalEntryState::Replicated);
        assert_eq!(replicated.len(), 1);
    }

    #[test]
    fn test_journal_entry_timestamp() {
        let config = JournalConfig::default();
        let journal = WriteJournal::new(config);

        let data = vec![0u8; 4096];
        let _entry = journal
            .append(test_block_ref(), data, PlacementHint::Journal)
            .unwrap();

        let pending = journal.pending_entries();
        assert!(pending[0].timestamp_secs > 0);
    }

    #[test]
    fn test_journal_multiple_commits_same_state() {
        let config = JournalConfig::default();
        let journal = WriteJournal::new(config);

        let data = vec![0u8; 4096];
        let seq = journal
            .append(test_block_ref(), data, PlacementHint::Journal)
            .unwrap();

        // Mark and try to commit twice
        journal.mark_local_flushed(seq).unwrap();
        journal.commit(seq).unwrap();

        // Second commit should fail
        let result = journal.commit(seq);
        assert!(result.is_err());
    }
}
