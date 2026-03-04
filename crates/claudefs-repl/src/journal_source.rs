//! Trait-based interface for journal sources (A2 integration boundary).

use crate::error::ReplError;
use crate::journal::JournalEntry;
use std::collections::VecDeque;

/// A batch of journal entries from a source.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceBatch {
    /// Entries from the journal.
    pub entries: Vec<JournalEntry>,
    /// Sequence of the first entry.
    pub first_seq: u64,
    /// Sequence of the last entry.
    pub last_seq: u64,
    /// Which site these entries came from.
    pub source_site_id: u64,
}

/// Cursor position for tracking source progress.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceCursor {
    /// Last sequence polled from the journal.
    pub last_polled: u64,
    /// Last sequence ACK'd by remote.
    pub last_acknowledged: u64,
    /// Consumer identifier.
    pub source_id: String,
}

impl SourceCursor {
    /// Create a new cursor.
    pub fn new(source_id: impl Into<String>) -> Self {
        Self {
            last_polled: 0,
            last_acknowledged: 0,
            source_id: source_id.into(),
        }
    }
}

/// Trait for journal sources (A2 metadata journal, test mock, file replay).
pub trait JournalSource: Send + Sync {
    /// Poll for next batch. Returns None if no entries available yet.
    fn poll_batch(&mut self, max_entries: usize) -> Result<Option<SourceBatch>, ReplError>;
    /// Acknowledge successful replication up to last_seq.
    fn acknowledge(&mut self, last_seq: u64) -> Result<(), ReplError>;
    /// Get the current cursor position.
    fn cursor(&self) -> SourceCursor;
}

/// Mock journal source for testing.
#[derive(Debug)]
pub struct MockJournalSource {
    entries: VecDeque<JournalEntry>,
    cursor: SourceCursor,
    max_batch: usize,
}

impl MockJournalSource {
    /// Create a new mock source.
    pub fn new(source_id: impl Into<String>) -> Self {
        Self {
            entries: VecDeque::new(),
            cursor: SourceCursor::new(source_id),
            max_batch: 100,
        }
    }

    /// Inject a test entry.
    pub fn push_entry(&mut self, entry: JournalEntry) {
        self.entries.push_back(entry);
    }

    /// Inject multiple entries.
    pub fn push_entries(&mut self, entries: Vec<JournalEntry>) {
        for entry in entries {
            self.entries.push_back(entry);
        }
    }

    /// Count of entries remaining.
    pub fn entries_remaining(&self) -> usize {
        self.entries.len()
    }
}

impl JournalSource for MockJournalSource {
    fn poll_batch(&mut self, max_entries: usize) -> Result<Option<SourceBatch>, ReplError> {
        if self.entries.is_empty() {
            return Ok(None);
        }

        let max = std::cmp::min(max_entries, self.max_batch);
        let batch_size = std::cmp::min(max, self.entries.len());

        let mut batch_entries = Vec::with_capacity(batch_size);
        for _ in 0..batch_size {
            if let Some(entry) = self.entries.pop_front() {
                batch_entries.push(entry);
            }
        }

        if batch_entries.is_empty() {
            return Ok(None);
        }

        let first_seq = batch_entries.first().map(|e| e.seq).unwrap_or(0);
        let last_seq = batch_entries.last().map(|e| e.seq).unwrap_or(0);
        let source_site_id = batch_entries.first().map(|e| e.site_id).unwrap_or(0);

        self.cursor.last_polled = last_seq;

        Ok(Some(SourceBatch {
            entries: batch_entries,
            first_seq,
            last_seq,
            source_site_id,
        }))
    }

    fn acknowledge(&mut self, last_seq: u64) -> Result<(), ReplError> {
        self.cursor.last_acknowledged = last_seq;
        Ok(())
    }

    fn cursor(&self) -> SourceCursor {
        self.cursor.clone()
    }
}

/// Journal source that replays from a Vec.
#[derive(Debug)]
pub struct VecJournalSource {
    entries: Vec<JournalEntry>,
    cursor_pos: usize,
    cursor: SourceCursor,
}

impl VecJournalSource {
    /// Create a new vec source.
    pub fn new(source_id: impl Into<String>, entries: Vec<JournalEntry>) -> Self {
        Self {
            entries,
            cursor_pos: 0,
            cursor: SourceCursor::new(source_id),
        }
    }
}

impl JournalSource for VecJournalSource {
    fn poll_batch(&mut self, max_entries: usize) -> Result<Option<SourceBatch>, ReplError> {
        if self.cursor_pos >= self.entries.len() {
            return Ok(None);
        }

        let remaining = self.entries.len() - self.cursor_pos;
        let batch_size = std::cmp::min(max_entries, remaining);

        let batch_entries = self.entries[self.cursor_pos..self.cursor_pos + batch_size].to_vec();
        self.cursor_pos += batch_size;

        let first_seq = batch_entries.first().map(|e| e.seq).unwrap_or(0);
        let last_seq = batch_entries.last().map(|e| e.seq).unwrap_or(0);
        let source_site_id = batch_entries.first().map(|e| e.site_id).unwrap_or(0);

        self.cursor.last_polled = last_seq;

        Ok(Some(SourceBatch {
            entries: batch_entries,
            first_seq,
            last_seq,
            source_site_id,
        }))
    }

    fn acknowledge(&mut self, last_seq: u64) -> Result<(), ReplError> {
        self.cursor.last_acknowledged = last_seq;
        Ok(())
    }

    fn cursor(&self) -> SourceCursor {
        self.cursor.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::OpKind;

    fn make_entry(seq: u64, site_id: u64) -> JournalEntry {
        JournalEntry::new(seq, 0, site_id, 1000 + seq, seq, OpKind::Create, vec![])
    }

    #[test]
    fn test_mock_source_empty_returns_none() {
        let mut source = MockJournalSource::new("test");
        let result = source.poll_batch(10);
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_mock_source_push_and_poll() {
        let mut source = MockJournalSource::new("test");
        source.push_entry(make_entry(1, 1));
        let result = source.poll_batch(10);
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_mock_source_poll_batch_respects_max() {
        let mut source = MockJournalSource::new("test");
        source.push_entry(make_entry(1, 1));
        source.push_entry(make_entry(2, 1));
        source.max_batch = 1;
        let result = source.poll_batch(10).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().entries.len(), 1);
    }

    #[test]
    fn test_mock_source_acknowledges_advances_cursor() {
        let mut source = MockJournalSource::new("test");
        source.push_entry(make_entry(1, 1));
        source.poll_batch(10).unwrap();
        source.acknowledge(1).unwrap();
        let cursor = source.cursor();
        assert_eq!(cursor.last_acknowledged, 1);
    }

    #[test]
    fn test_mock_source_cursor_initial_state() {
        let source = MockJournalSource::new("my-source");
        let cursor = source.cursor();
        assert_eq!(cursor.last_polled, 0);
        assert_eq!(cursor.last_acknowledged, 0);
        assert_eq!(cursor.source_id, "my-source");
    }

    #[test]
    fn test_mock_source_push_entries_batch() {
        let mut source = MockJournalSource::new("test");
        source.push_entries(vec![make_entry(1, 1), make_entry(2, 1), make_entry(3, 1)]);
        let result = source.poll_batch(10).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().entries.len(), 3);
    }

    #[test]
    fn test_mock_source_sequential_polls() {
        let mut source = MockJournalSource::new("test");
        source.push_entries(vec![make_entry(1, 1), make_entry(2, 1), make_entry(3, 1)]);

        let r1 = source.poll_batch(2).unwrap();
        assert_eq!(r1.unwrap().entries.len(), 2);

        let r2 = source.poll_batch(2).unwrap();
        assert!(r2.is_some());
        assert_eq!(r2.unwrap().entries.len(), 1);

        let r3 = source.poll_batch(2).unwrap();
        assert!(r3.is_none());
    }

    #[test]
    fn test_mock_source_entries_remaining_count() {
        let mut source = MockJournalSource::new("test");
        assert_eq!(source.entries_remaining(), 0);
        source.push_entries(vec![make_entry(1, 1), make_entry(2, 1)]);
        assert_eq!(source.entries_remaining(), 2);
        source.poll_batch(1).unwrap();
        assert_eq!(source.entries_remaining(), 1);
    }

    #[test]
    fn test_vec_source_empty() {
        let mut source = VecJournalSource::new("test", vec![]);
        let result = source.poll_batch(10);
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_vec_source_poll_all() {
        let mut source = VecJournalSource::new("test", vec![make_entry(1, 1), make_entry(2, 1)]);
        let r1 = source.poll_batch(10).unwrap();
        assert!(r1.is_some());
        assert_eq!(r1.unwrap().entries.len(), 2);
        let r2 = source.poll_batch(10).unwrap();
        assert!(r2.is_none());
    }

    #[test]
    fn test_vec_source_poll_respects_max() {
        let mut source = VecJournalSource::new(
            "test",
            vec![make_entry(1, 1), make_entry(2, 1), make_entry(3, 1)],
        );
        let result = source.poll_batch(2).unwrap();
        assert_eq!(result.unwrap().entries.len(), 2);
        let result2 = source.poll_batch(2).unwrap();
        assert_eq!(result2.unwrap().entries.len(), 1);
    }

    #[test]
    fn test_vec_source_exhausted_returns_none() {
        let mut source = VecJournalSource::new("test", vec![make_entry(1, 1)]);
        source.poll_batch(10).unwrap();
        let result = source.poll_batch(10).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_vec_source_acknowledge_cursor() {
        let mut source = VecJournalSource::new("test", vec![make_entry(1, 1)]);
        source.poll_batch(10).unwrap();
        source.acknowledge(1).unwrap();
        let cursor = source.cursor();
        assert_eq!(cursor.last_acknowledged, 1);
    }

    #[test]
    fn test_vec_source_cursor_tracks_position() {
        let mut source = VecJournalSource::new("test", vec![make_entry(1, 1), make_entry(2, 1)]);
        source.poll_batch(1).unwrap();
        let cursor = source.cursor();
        assert_eq!(cursor.last_polled, 1);
    }

    #[test]
    fn test_source_batch_fields() {
        let entries = vec![make_entry(5, 2), make_entry(6, 2)];
        let batch = SourceBatch {
            entries,
            first_seq: 5,
            last_seq: 6,
            source_site_id: 2,
        };
        assert_eq!(batch.first_seq, 5);
        assert_eq!(batch.last_seq, 6);
        assert_eq!(batch.source_site_id, 2);
        assert_eq!(batch.entries.len(), 2);
    }

    #[test]
    fn test_source_cursor_clone() {
        let cursor = SourceCursor::new("test");
        let cloned = cursor.clone();
        assert_eq!(cursor, cloned);
    }

    #[test]
    fn test_mock_then_vec_source_integration() {
        let mut mock = MockJournalSource::new("mock");
        mock.push_entries(vec![make_entry(1, 1), make_entry(2, 1)]);

        let batch = mock.poll_batch(10).unwrap().unwrap();
        mock.acknowledge(batch.last_seq).unwrap();

        let vec_source = VecJournalSource::new("vec", batch.entries);
        let cursor = vec_source.cursor();
        assert_eq!(cursor.source_id, "vec");
    }

    #[test]
    fn test_mock_source_multiple_polls_drain() {
        let mut source = MockJournalSource::new("test");
        source.push_entries(vec![make_entry(1, 1), make_entry(2, 1), make_entry(3, 1)]);

        let mut total = 0;
        while let Some(batch) = source.poll_batch(2).unwrap() {
            total += batch.entries.len();
        }
        assert_eq!(total, 3);
    }

    #[test]
    fn test_vec_source_single_entry() {
        let mut source = VecJournalSource::new("test", vec![make_entry(42, 1)]);
        let result = source.poll_batch(10).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().entries.len(), 1);
    }

    #[test]
    fn test_vec_source_large_batch() {
        let entries: Vec<JournalEntry> = (0..100).map(|i| make_entry(i, 1)).collect();
        let mut source = VecJournalSource::new("test", entries);
        let result = source.poll_batch(50).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().entries.len(), 50);
    }

    #[test]
    fn test_mock_source_acknowledge_unknown_seq() {
        let mut source = MockJournalSource::new("test");
        let result = source.acknowledge(999);
        assert!(result.is_ok());
    }

    #[test]
    fn test_source_cursor_last_polled_tracks_poll() {
        let mut source = MockJournalSource::new("test");
        source.push_entry(make_entry(10, 1));
        source.poll_batch(10).unwrap();
        let cursor = source.cursor();
        assert_eq!(cursor.last_polled, 10);
    }

    #[test]
    fn test_source_cursor_last_acked_tracks_ack() {
        let mut source = MockJournalSource::new("test");
        source.push_entry(make_entry(5, 1));
        source.acknowledge(5).unwrap();
        let cursor = source.cursor();
        assert_eq!(cursor.last_acknowledged, 5);
    }

    #[test]
    fn test_mock_source_empty_after_drain() {
        let mut source = MockJournalSource::new("test");
        source.push_entry(make_entry(1, 1));
        source.poll_batch(10).unwrap();
        assert!(source.poll_batch(10).unwrap().is_none());
    }
}

#[cfg(test)]
mod proptest_journal_source {
    use super::*;
    use crate::journal::OpKind;
    use proptest::prelude::*;

    fn arb_entry(seq: u64) -> JournalEntry {
        JournalEntry::new(seq, 0, 1, 1000 + seq, seq, OpKind::Write, vec![seq as u8])
    }

    proptest! {
        #[test]
        fn prop_mock_source_roundtrip(entry_count in 1u32..50, max_batch in 1u32..20) {
            let mut source = MockJournalSource::new("prop-test");
            let entries: Vec<JournalEntry> = (0..entry_count).map(|i| arb_entry(i as u64)).collect();
            source.push_entries(entries.clone());

            let mut total_polled = 0;
            let mut last_seq = 0;

            while let Some(batch) = source.poll_batch(max_batch as usize).unwrap() {
                total_polled += batch.entries.len();
                last_seq = batch.last_seq;
            }

            prop_assert_eq!(total_polled, entry_count as usize);
        }
    }
}
