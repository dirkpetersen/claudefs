//! Persistent Raft log store for crash-safe consensus state.
//!
//! This module wraps a KvStore to persist Raft log entries and hard state
//! (term, voted_for, commit_index) across restarts.

use std::sync::Arc;

use bincode::Options;

use crate::kvstore::{BatchOp, KvStore};
use crate::types::{LogEntry, LogIndex, MetaError, NodeId, Term};

const KEY_TERM: &[u8] = b"raft/term";
const KEY_VOTED_FOR: &[u8] = b"raft/voted_for";
const KEY_COMMIT_INDEX: &[u8] = b"raft/commit_index";
const PREFIX_LOG: &[u8] = b"raft/log/";

fn log_entry_key(index: LogIndex) -> Vec<u8> {
    let mut key = PREFIX_LOG.to_vec();
    key.extend_from_slice(&index.as_u64().to_be_bytes());
    key
}

fn u64_from_be_bytes(bytes: &[u8]) -> u64 {
    let mut arr = [0u8; 8];
    arr.copy_from_slice(bytes);
    u64::from_be_bytes(arr)
}

/// Persistent storage for Raft state using any KvStore implementation.
///
/// This store persists:
/// - Current term
/// - Voted-for node
/// - Commit index
/// - Log entries
pub struct RaftLogStore {
    kv: Arc<dyn KvStore>,
}

impl RaftLogStore {
    /// Creates a new Raft log store backed by the given KV store.
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Persists the current term.
    pub fn save_term(&self, term: Term) -> Result<(), MetaError> {
        let bytes = term.as_u64().to_be_bytes().to_vec();
        self.kv.put(KEY_TERM.to_vec(), bytes)
    }

    /// Loads the current term. Returns Term(0) if not set.
    pub fn load_term(&self) -> Result<Term, MetaError> {
        match self.kv.get(KEY_TERM)? {
            Some(bytes) if bytes.len() == 8 => Ok(Term::new(u64_from_be_bytes(&bytes))),
            Some(_) => Err(MetaError::KvError("invalid term value".to_string())),
            None => Ok(Term::new(0)),
        }
    }

    /// Persists the voted_for node.
    pub fn save_voted_for(&self, node: Option<NodeId>) -> Result<(), MetaError> {
        match node {
            Some(n) => {
                let bytes = n.as_u64().to_be_bytes().to_vec();
                self.kv.put(KEY_VOTED_FOR.to_vec(), bytes)
            }
            None => self.kv.delete(KEY_VOTED_FOR),
        }
    }

    /// Loads the voted_for node. Returns None if not set.
    pub fn load_voted_for(&self) -> Result<Option<NodeId>, MetaError> {
        match self.kv.get(KEY_VOTED_FOR)? {
            Some(bytes) if bytes.len() == 8 => Ok(Some(NodeId::new(u64_from_be_bytes(&bytes)))),
            Some(_) => Err(MetaError::KvError("invalid voted_for value".to_string())),
            None => Ok(None),
        }
    }

    /// Persists the commit index.
    pub fn save_commit_index(&self, index: LogIndex) -> Result<(), MetaError> {
        let bytes = index.as_u64().to_be_bytes().to_vec();
        self.kv.put(KEY_COMMIT_INDEX.to_vec(), bytes)
    }

    /// Loads the commit index. Returns LogIndex::ZERO if not set.
    pub fn load_commit_index(&self) -> Result<LogIndex, MetaError> {
        match self.kv.get(KEY_COMMIT_INDEX)? {
            Some(bytes) if bytes.len() == 8 => Ok(LogIndex::new(u64_from_be_bytes(&bytes))),
            Some(_) => Err(MetaError::KvError("invalid commit_index value".to_string())),
            None => Ok(LogIndex::ZERO),
        }
    }

    fn serialize_entry(entry: &LogEntry) -> Result<Vec<u8>, MetaError> {
        bincode::DefaultOptions::new()
            .serialize(entry)
            .map_err(|e| MetaError::KvError(e.to_string()))
    }

    fn deserialize_entry(bytes: &[u8]) -> Result<LogEntry, MetaError> {
        bincode::DefaultOptions::new()
            .deserialize(bytes)
            .map_err(|e| MetaError::KvError(e.to_string()))
    }

    /// Appends a log entry. The entry's index must be set correctly.
    pub fn append_entry(&self, entry: &LogEntry) -> Result<(), MetaError> {
        let key = log_entry_key(entry.index);
        let value = Self::serialize_entry(entry)?;
        self.kv.put(key, value)
    }

    /// Appends multiple log entries atomically.
    pub fn append_entries(&self, entries: &[LogEntry]) -> Result<(), MetaError> {
        if entries.is_empty() {
            return Ok(());
        }
        let ops: Vec<BatchOp> = entries
            .iter()
            .map(|entry| {
                let key = log_entry_key(entry.index);
                let value = Self::serialize_entry(entry).unwrap();
                BatchOp::Put { key, value }
            })
            .collect();
        self.kv.write_batch(ops)
    }

    /// Gets a log entry by index.
    pub fn get_entry(&self, index: LogIndex) -> Result<Option<LogEntry>, MetaError> {
        let key = log_entry_key(index);
        match self.kv.get(&key)? {
            Some(bytes) => Ok(Some(Self::deserialize_entry(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Gets all log entries from start_index to end_index (inclusive).
    pub fn get_entries(&self, start: LogIndex, end: LogIndex) -> Result<Vec<LogEntry>, MetaError> {
        if start.as_u64() > end.as_u64() {
            return Ok(Vec::new());
        }
        let start_key = log_entry_key(start);
        let end_key = log_entry_key(LogIndex::new(end.as_u64().saturating_add(1)));
        let pairs = self.kv.scan_range(&start_key, &end_key)?;
        let mut entries = Vec::with_capacity(pairs.len());
        for (_, value) in pairs {
            entries.push(Self::deserialize_entry(&value)?);
        }
        entries.sort_by_key(|e| e.index);
        Ok(entries)
    }

    /// Gets the last log entry.
    pub fn last_entry(&self) -> Result<Option<LogEntry>, MetaError> {
        let pairs = self.kv.scan_prefix(PREFIX_LOG)?;
        if pairs.is_empty() {
            return Ok(None);
        }
        let mut max_entry: Option<LogEntry> = None;
        for (_, value) in pairs {
            let entry = Self::deserialize_entry(&value)?;
            match &max_entry {
                Some(max) if max.index >= entry.index => {}
                _ => max_entry = Some(entry),
            }
        }
        Ok(max_entry)
    }

    /// Gets the last log index. Returns LogIndex::ZERO if empty.
    pub fn last_index(&self) -> Result<LogIndex, MetaError> {
        match self.last_entry()? {
            Some(entry) => Ok(entry.index),
            None => Ok(LogIndex::ZERO),
        }
    }

    /// Truncates the log from the given index onwards (inclusive).
    /// Used when a leader overwrites conflicting entries.
    pub fn truncate_from(&self, index: LogIndex) -> Result<(), MetaError> {
        let start_key = log_entry_key(index);
        let end_key = log_entry_key(LogIndex::new(u64::MAX));
        let to_delete = self.kv.scan_range(&start_key, &end_key)?;
        if to_delete.is_empty() {
            return Ok(());
        }
        let ops: Vec<BatchOp> = to_delete
            .into_iter()
            .map(|(key, _)| BatchOp::Delete { key })
            .collect();
        self.kv.write_batch(ops)
    }

    /// Returns the number of log entries.
    pub fn entry_count(&self) -> Result<usize, MetaError> {
        let pairs = self.kv.scan_prefix(PREFIX_LOG)?;
        Ok(pairs.len())
    }

    /// Saves term + voted_for + log entries atomically via write_batch.
    /// Used during vote handling to persist hard state in one operation.
    pub fn save_hard_state(
        &self,
        term: Term,
        voted_for: Option<NodeId>,
        commit_index: LogIndex,
    ) -> Result<(), MetaError> {
        let mut ops = vec![
            BatchOp::Put {
                key: KEY_TERM.to_vec(),
                value: term.as_u64().to_be_bytes().to_vec(),
            },
            BatchOp::Put {
                key: KEY_COMMIT_INDEX.to_vec(),
                value: commit_index.as_u64().to_be_bytes().to_vec(),
            },
        ];
        match voted_for {
            Some(n) => ops.push(BatchOp::Put {
                key: KEY_VOTED_FOR.to_vec(),
                value: n.as_u64().to_be_bytes().to_vec(),
            }),
            None => ops.push(BatchOp::Delete {
                key: KEY_VOTED_FOR.to_vec(),
            }),
        }
        self.kv.write_batch(ops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use crate::types::MetaOp;

    fn make_entry(index: u64, term: u64) -> LogEntry {
        LogEntry {
            index: LogIndex::new(index),
            term: Term::new(term),
            op: MetaOp::CreateInode {
                attr: crate::types::InodeAttr::new_file(
                    crate::types::InodeId::new(index),
                    0,
                    0,
                    0o644,
                    0,
                ),
            },
        }
    }

    #[test]
    fn test_save_load_term() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        assert_eq!(store.load_term().unwrap().as_u64(), 0);

        store.save_term(Term::new(5)).unwrap();
        assert_eq!(store.load_term().unwrap().as_u64(), 5);

        store.save_term(Term::new(10)).unwrap();
        assert_eq!(store.load_term().unwrap().as_u64(), 10);
    }

    #[test]
    fn test_save_load_voted_for() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        assert!(store.load_voted_for().unwrap().is_none());

        store.save_voted_for(Some(NodeId::new(1))).unwrap();
        assert_eq!(store.load_voted_for().unwrap().unwrap().as_u64(), 1);

        store.save_voted_for(Some(NodeId::new(2))).unwrap();
        assert_eq!(store.load_voted_for().unwrap().unwrap().as_u64(), 2);

        store.save_voted_for(None).unwrap();
        assert!(store.load_voted_for().unwrap().is_none());
    }

    #[test]
    fn test_save_load_commit_index() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        assert_eq!(store.load_commit_index().unwrap().as_u64(), 0);

        store.save_commit_index(LogIndex::new(100)).unwrap();
        assert_eq!(store.load_commit_index().unwrap().as_u64(), 100);

        store.save_commit_index(LogIndex::new(200)).unwrap();
        assert_eq!(store.load_commit_index().unwrap().as_u64(), 200);
    }

    #[test]
    fn test_append_get_entry() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        let entry = make_entry(1, 1);
        store.append_entry(&entry).unwrap();

        let retrieved = store.get_entry(LogIndex::new(1)).unwrap().unwrap();
        assert_eq!(retrieved.index.as_u64(), 1);
        assert_eq!(retrieved.term.as_u64(), 1);

        assert!(store.get_entry(LogIndex::new(2)).unwrap().is_none());
    }

    #[test]
    fn test_append_entries() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        let entries = vec![make_entry(1, 1), make_entry(2, 1), make_entry(3, 2)];
        store.append_entries(&entries).unwrap();

        assert_eq!(store.entry_count().unwrap(), 3);
    }

    #[test]
    fn test_get_entries_range() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        for i in 1..=5 {
            store.append_entry(&make_entry(i, 1)).unwrap();
        }

        let entries = store
            .get_entries(LogIndex::new(2), LogIndex::new(4))
            .unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].index.as_u64(), 2);
        assert_eq!(entries[1].index.as_u64(), 3);
        assert_eq!(entries[2].index.as_u64(), 4);
    }

    #[test]
    fn test_last_entry() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        assert!(store.last_entry().unwrap().is_none());

        store.append_entry(&make_entry(1, 1)).unwrap();
        assert_eq!(store.last_entry().unwrap().unwrap().index.as_u64(), 1);

        store.append_entry(&make_entry(3, 2)).unwrap();
        store.append_entry(&make_entry(2, 1)).unwrap();
        assert_eq!(store.last_entry().unwrap().unwrap().index.as_u64(), 3);
    }

    #[test]
    fn test_last_index() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        assert_eq!(store.last_index().unwrap().as_u64(), 0);

        store.append_entry(&make_entry(5, 1)).unwrap();
        assert_eq!(store.last_index().unwrap().as_u64(), 5);
    }

    #[test]
    fn test_truncate_from() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        for i in 1..=5 {
            store.append_entry(&make_entry(i, 1)).unwrap();
        }

        store.truncate_from(LogIndex::new(3)).unwrap();

        assert!(store.get_entry(LogIndex::new(1)).unwrap().is_some());
        assert!(store.get_entry(LogIndex::new(2)).unwrap().is_some());
        assert!(store.get_entry(LogIndex::new(3)).unwrap().is_none());
        assert!(store.get_entry(LogIndex::new(4)).unwrap().is_none());
        assert!(store.get_entry(LogIndex::new(5)).unwrap().is_none());
    }

    #[test]
    fn test_entry_count() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        assert_eq!(store.entry_count().unwrap(), 0);

        store.append_entry(&make_entry(1, 1)).unwrap();
        store.append_entry(&make_entry(2, 1)).unwrap();
        assert_eq!(store.entry_count().unwrap(), 2);
    }

    #[test]
    fn test_save_hard_state() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        store
            .save_hard_state(Term::new(5), Some(NodeId::new(2)), LogIndex::new(10))
            .unwrap();

        assert_eq!(store.load_term().unwrap().as_u64(), 5);
        assert_eq!(store.load_voted_for().unwrap().unwrap().as_u64(), 2);
        assert_eq!(store.load_commit_index().unwrap().as_u64(), 10);
    }

    #[test]
    fn test_persistence_across_reopens() {
        let kv1 = Arc::new(MemoryKvStore::new());
        let store1 = RaftLogStore::new(kv1.clone());
        store1.save_term(Term::new(7)).unwrap();
        store1.save_voted_for(Some(NodeId::new(3))).unwrap();
        store1.save_commit_index(LogIndex::new(15)).unwrap();
        store1
            .append_entries(&[make_entry(1, 1), make_entry(2, 1)])
            .unwrap();

        let kv2 = Arc::new(MemoryKvStore::new());
        let _ = kv2.put(
            b"raft/term".to_vec(),
            b"\x00\x00\x00\x00\x00\x00\x00\x07".to_vec(),
        );
        let _ = kv2.put(
            b"raft/voted_for".to_vec(),
            b"\x00\x00\x00\x00\x00\x00\x00\x03".to_vec(),
        );
        let _ = kv2.put(
            b"raft/commit_index".to_vec(),
            b"\x00\x00\x00\x00\x00\x00\x00\x0f".to_vec(),
        );
        let _ = kv2.put(
            b"raft/log/\x00\x00\x00\x00\x00\x00\x00\x01".to_vec(),
            kv1.get(b"raft/log/\x00\x00\x00\x00\x00\x00\x00\x01".as_slice())
                .unwrap()
                .unwrap(),
        );
        let _ = kv2.put(
            b"raft/log/\x00\x00\x00\x00\x00\x00\x00\x02".to_vec(),
            kv1.get(b"raft/log/\x00\x00\x00\x00\x00\x00\x00\x02".as_slice())
                .unwrap()
                .unwrap(),
        );

        let store2 = RaftLogStore::new(kv2);
        assert_eq!(store2.load_term().unwrap().as_u64(), 7);
        assert_eq!(store2.load_voted_for().unwrap().unwrap().as_u64(), 3);
        assert_eq!(store2.load_commit_index().unwrap().as_u64(), 15);
        assert_eq!(store2.entry_count().unwrap(), 2);
        assert_eq!(store2.last_index().unwrap().as_u64(), 2);
    }

    #[test]
    fn test_get_entries_empty_range() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        let entries = store
            .get_entries(LogIndex::new(5), LogIndex::new(3))
            .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_truncate_nonexistent() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        store.append_entry(&make_entry(1, 1)).unwrap();
        store.truncate_from(LogIndex::new(10)).unwrap();

        assert_eq!(store.entry_count().unwrap(), 1);
    }

    #[test]
    fn test_overwrite_entry() {
        let kv = Arc::new(MemoryKvStore::new());
        let store = RaftLogStore::new(kv);

        let entry1 = LogEntry {
            index: LogIndex::new(1),
            term: Term::new(1),
            op: MetaOp::CreateInode {
                attr: crate::types::InodeAttr::new_file(
                    crate::types::InodeId::new(1),
                    0,
                    0,
                    0o644,
                    0,
                ),
            },
        };
        store.append_entry(&entry1).unwrap();

        let entry2 = LogEntry {
            index: LogIndex::new(1),
            term: Term::new(2),
            op: MetaOp::SetAttr {
                ino: crate::types::InodeId::new(1),
                attr: crate::types::InodeAttr::new_file(
                    crate::types::InodeId::new(1),
                    0,
                    0,
                    0o644,
                    0,
                ),
            },
        };
        store.append_entry(&entry2).unwrap();

        let retrieved = store.get_entry(LogIndex::new(1)).unwrap().unwrap();
        assert_eq!(retrieved.term.as_u64(), 2);
    }
}
