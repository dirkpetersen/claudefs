use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntryData {
    pub seq: u64,
    pub inode_id: u64,
    pub offset: u64,
    pub len: u32,
    pub hash: [u8; 32],
    pub committed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteJournalConfig {
    pub max_entries: usize,
    pub flush_threshold: usize,
}

impl Default for WriteJournalConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            flush_threshold: 1_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WriteJournalStats {
    pub entries_appended: u64,
    pub entries_committed: u64,
    pub entries_flushed: u64,
    pub current_seq: u64,
}

pub struct WriteJournal {
    entries: Vec<JournalEntryData>,
    config: WriteJournalConfig,
    stats: WriteJournalStats,
    next_seq: u64,
}

impl WriteJournal {
    pub fn new(config: WriteJournalConfig) -> Self {
        Self {
            entries: Vec::new(),
            config,
            stats: WriteJournalStats::default(),
            next_seq: 0,
        }
    }

    pub fn append(&mut self, inode_id: u64, offset: u64, len: u32, hash: [u8; 32]) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.stats.entries_appended += 1;
        self.stats.current_seq = self.next_seq;

        self.entries.push(JournalEntryData {
            seq,
            inode_id,
            offset,
            len,
            hash,
            committed: false,
        });

        seq
    }

    pub fn commit(&mut self, seq: u64) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.seq == seq) {
            if !entry.committed {
                entry.committed = true;
                self.stats.entries_committed += 1;
            }
            return true;
        }
        false
    }

    pub fn flush_committed(&mut self, before_seq: u64) -> usize {
        let original_len = self.entries.len();
        self.entries.retain(|e| !e.committed || e.seq >= before_seq);
        let flushed = original_len - self.entries.len();
        self.stats.entries_flushed += flushed as u64;
        flushed
    }

    pub fn pending_for_inode(&self, inode_id: u64) -> Vec<&JournalEntryData> {
        self.entries
            .iter()
            .filter(|e| e.inode_id == inode_id && !e.committed)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn stats(&self) -> &WriteJournalStats {
        &self.stats
    }

    pub fn needs_flush(&self) -> bool {
        self.entries.len() >= self.config.flush_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_journal_config_default() {
        let config = WriteJournalConfig::default();
        assert_eq!(config.max_entries, 10_000);
        assert_eq!(config.flush_threshold, 1_000);
    }

    #[test]
    fn write_journal_new_empty() {
        let journal = WriteJournal::new(WriteJournalConfig::default());
        assert!(journal.is_empty());
    }

    #[test]
    fn append_single_entry() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        journal.append(1, 0, 4096, hash);
        assert_eq!(journal.len(), 1);
    }

    #[test]
    fn append_returns_seq() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        let seq = journal.append(1, 0, 4096, hash);
        assert_eq!(seq, 0);
    }

    #[test]
    fn append_seq_increments() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        let seq1 = journal.append(1, 0, 4096, hash);
        let seq2 = journal.append(1, 4096, 4096, hash);
        let seq3 = journal.append(1, 8192, 4096, hash);
        assert_eq!(seq1, 0);
        assert_eq!(seq2, 1);
        assert_eq!(seq3, 2);
    }

    #[test]
    fn commit_entry() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        let seq = journal.append(1, 0, 4096, hash);
        let result = journal.commit(seq);
        assert!(result);
        let entries = journal.pending_for_inode(1);
        assert!(entries.is_empty());
    }

    #[test]
    fn commit_nonexistent() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let result = journal.commit(999);
        assert!(!result);
    }

    #[test]
    fn flush_committed_removes_entries() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        let seq1 = journal.append(1, 0, 4096, hash);
        let seq2 = journal.append(1, 4096, 4096, hash);
        journal.commit(seq1);
        journal.flush_committed(seq2);
        assert!(journal.is_empty());
    }

    #[test]
    fn flush_uncommitted_stays() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        journal.append(1, 0, 4096, hash);
        journal.flush_committed(100);
        assert_eq!(journal.len(), 1);
    }

    #[test]
    fn pending_for_inode_empty() {
        let journal = WriteJournal::new(WriteJournalConfig::default());
        let pending = journal.pending_for_inode(999);
        assert!(pending.is_empty());
    }

    #[test]
    fn pending_for_inode_has_entries() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        journal.append(1, 0, 4096, hash);
        journal.append(2, 0, 4096, hash);
        let pending = journal.pending_for_inode(1);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn stats_entries_appended() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        journal.append(1, 0, 4096, hash);
        journal.append(1, 4096, 4096, hash);
        let stats = journal.stats();
        assert_eq!(stats.entries_appended, 2);
    }

    #[test]
    fn stats_entries_committed() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        let seq = journal.append(1, 0, 4096, hash);
        journal.commit(seq);
        let stats = journal.stats();
        assert_eq!(stats.entries_committed, 1);
    }

    #[test]
    fn stats_entries_flushed() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        let seq = journal.append(1, 0, 4096, hash);
        journal.commit(seq);
        journal.flush_committed(seq + 1);
        let stats = journal.stats();
        assert_eq!(stats.entries_flushed, 1);
    }

    #[test]
    fn needs_flush_false_when_empty() {
        let journal = WriteJournal::new(WriteJournalConfig::default());
        assert!(!journal.needs_flush());
    }

    #[test]
    fn needs_flush_true_at_threshold() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        for _ in 0..1000 {
            journal.append(1, 0, 4096, hash);
        }
        assert!(journal.needs_flush());
    }

    #[test]
    fn flush_then_empty() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        let seq = journal.append(1, 0, 4096, hash);
        journal.commit(seq);
        journal.flush_committed(seq + 1);
        assert!(journal.is_empty());
    }

    #[test]
    fn multiple_inodes() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        journal.append(1, 0, 4096, hash);
        journal.append(2, 0, 4096, hash);
        journal.append(1, 4096, 4096, hash);
        let pending1 = journal.pending_for_inode(1);
        let pending2 = journal.pending_for_inode(2);
        assert_eq!(pending1.len(), 2);
        assert_eq!(pending2.len(), 1);
    }

    #[test]
    fn double_commit_seq() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        let seq = journal.append(1, 0, 4096, hash);
        journal.commit(seq);
        journal.commit(seq);
        let stats = journal.stats();
        assert_eq!(stats.entries_committed, 1);
    }

    #[test]
    fn flush_before_any_commits() {
        let mut journal = WriteJournal::new(WriteJournalConfig::default());
        let hash = [0u8; 32];
        journal.append(1, 0, 4096, hash);
        let flushed = journal.flush_committed(100);
        assert_eq!(flushed, 0);
    }
}
