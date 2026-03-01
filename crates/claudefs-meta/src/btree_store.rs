//! Persistent file-backed KV store with WAL and checkpoint support.
//!
//! Uses an in-memory BTreeMap as read cache, a write-ahead log (WAL) for durability,
//! and periodic checkpoint files for fast recovery.

#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::kvstore::{BatchOp, Key, KvPair, KvStore, Value};
use crate::types::MetaError;

const WAL_FILENAME: &str = "wal.bin";
const CHECKPOINT_FILENAME: &str = "checkpoint.bin";

#[derive(Debug, Serialize, Deserialize, Clone)]
enum WalOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

#[derive(Debug, Serialize, Deserialize)]
struct WalEntry {
    seq: u64,
    op: WalOp,
}

#[derive(Debug, Serialize, Deserialize)]
struct Checkpoint {
    seq: u64,
    entries: Vec<(Vec<u8>, Vec<u8>)>,
}

/// Persistent file-backed KV store.
///
/// Provides durability through a write-ahead log (WAL) and periodic checkpoints.
/// On open, loads the last checkpoint and replays any remaining WAL entries.
pub struct PersistentKvStore {
    data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
    wal: Arc<Mutex<WalWriter>>,
    checkpoint_dir: PathBuf,
    seq: Arc<RwLock<u64>>,
}

#[allow(dead_code)]
struct WalWriter {
    file: File,
    path: PathBuf,
}

impl WalWriter {
    #[allow(dead_code)]
    fn reopen(&mut self) -> std::io::Result<()> {
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        Ok(())
    }

    fn new(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(WalWriter {
            file,
            path: path.to_path_buf(),
        })
    }

    fn append(&mut self, entry: &WalEntry) -> std::io::Result<()> {
        let encoded = bincode::serialize(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let len_bytes = (encoded.len() as u32).to_le_bytes();
        self.file.write_all(&len_bytes)?;
        self.file.write_all(&encoded)?;
        self.file.sync_all()?;
        Ok(())
    }

    fn truncate(&mut self) -> std::io::Result<()> {
        self.file.set_len(0)?;
        self.file.sync_all()?;
        Ok(())
    }
}

impl PersistentKvStore {
    /// Opens or creates a persistent KV store in the given directory.
    ///
    /// Loads the checkpoint if available, then replays WAL entries for recovery.
    pub fn open(dir: &Path) -> Result<Self, MetaError> {
        fs::create_dir_all(dir).map_err(MetaError::IoError)?;

        let wal_path = dir.join(WAL_FILENAME);
        let checkpoint_path = dir.join(CHECKPOINT_FILENAME);

        let wal = match WalWriter::new(&wal_path) {
            Ok(w) => w,
            Err(e) => return Err(MetaError::IoError(e)),
        };

        let mut store = Self {
            data: Arc::new(RwLock::new(BTreeMap::new())),
            wal: Arc::new(Mutex::new(wal)),
            checkpoint_dir: dir.to_path_buf(),
            seq: Arc::new(RwLock::new(0)),
        };

        store.load_checkpoint(&checkpoint_path)?;
        store.replay_wal(&wal_path)?;

        Ok(store)
    }

    fn load_checkpoint(&mut self, path: &Path) -> Result<(), MetaError> {
        if !path.exists() {
            return Ok(());
        }

        let mut file = File::open(path).map_err(MetaError::IoError)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(MetaError::IoError)?;

        if contents.is_empty() {
            return Ok(());
        }

        let checkpoint: Checkpoint = bincode::deserialize(&contents)
            .map_err(|e| MetaError::KvError(format!("failed to deserialize checkpoint: {}", e)))?;

        let mut data = self
            .data
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        for (k, v) in checkpoint.entries {
            data.insert(k, v);
        }

        *self
            .seq
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))? = checkpoint.seq;

        Ok(())
    }

    fn replay_wal(&mut self, path: &Path) -> Result<(), MetaError> {
        if !path.exists() {
            return Ok(());
        }

        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) => return Err(MetaError::IoError(e)),
        };

        let mut data = self
            .data
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;

        let mut seq_lock = self
            .seq
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let mut max_seq = *seq_lock;

        loop {
            let mut len_buf = [0u8; 4];
            match file.read_exact(&mut len_buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(MetaError::IoError(e)),
            }

            let len = u32::from_le_bytes(len_buf) as usize;
            let mut op_buf = vec![0u8; len];
            file.read_exact(&mut op_buf).map_err(MetaError::IoError)?;

            let entry: WalEntry = bincode::deserialize(&op_buf).map_err(|e| {
                MetaError::KvError(format!("failed to deserialize WAL entry: {}", e))
            })?;

            if entry.seq > max_seq {
                max_seq = entry.seq;
            }

            match entry.op {
                WalOp::Put { key, value } => {
                    data.insert(key, value);
                }
                WalOp::Delete { key } => {
                    data.remove(&key);
                }
            }
        }

        *seq_lock = max_seq;

        Ok(())
    }

    /// Creates a checkpoint of the current state and truncates the WAL.
    ///
    /// This provides a point-in-time snapshot that can be used for fast recovery.
    pub fn checkpoint(&self) -> Result<(), MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let seq = *self
            .seq
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;

        let entries: Vec<(Vec<u8>, Vec<u8>)> =
            data.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        let checkpoint = Checkpoint { seq, entries };

        let encoded = bincode::serialize(&checkpoint)
            .map_err(|e| MetaError::KvError(format!("failed to serialize checkpoint: {}", e)))?;

        let checkpoint_path = self.checkpoint_dir.join(CHECKPOINT_FILENAME);
        let mut tmp_path = checkpoint_path.clone();
        tmp_path.set_extension("tmp");

        {
            let mut tmp_file = File::create(&tmp_path).map_err(MetaError::IoError)?;
            tmp_file.write_all(&encoded).map_err(MetaError::IoError)?;
            tmp_file.sync_all().map_err(MetaError::IoError)?;
        }

        fs::rename(&tmp_path, &checkpoint_path).map_err(MetaError::IoError)?;

        let mut wal = self
            .wal
            .lock()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        wal.truncate().map_err(MetaError::IoError)?;

        Ok(())
    }

    fn next_seq(&self) -> Result<u64, MetaError> {
        let mut seq = self
            .seq
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        *seq += 1;
        Ok(*seq)
    }

    fn write_wal(&self, op: WalOp) -> Result<(), MetaError> {
        let seq = self.next_seq()?;
        let entry = WalEntry { seq, op };
        let mut wal = self
            .wal
            .lock()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        wal.append(&entry).map_err(MetaError::IoError)?;
        Ok(())
    }
}

impl KvStore for PersistentKvStore {
    fn get(&self, key: &[u8]) -> Result<Option<Value>, MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(data.get(key).cloned())
    }

    fn put(&self, key: Key, value: Value) -> Result<(), MetaError> {
        self.write_wal(WalOp::Put {
            key: key.clone(),
            value: value.clone(),
        })?;

        let mut data = self
            .data
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        data.insert(key, value);
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<(), MetaError> {
        self.write_wal(WalOp::Delete { key: key.to_vec() })?;

        let mut data = self
            .data
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        data.remove(key);
        Ok(())
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<KvPair>, MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let mut result = Vec::new();
        for (k, v) in data.range::<Vec<u8>, _>(prefix.to_vec()..) {
            if !k.starts_with(prefix) {
                break;
            }
            result.push((k.clone(), v.clone()));
        }
        Ok(result)
    }

    fn scan_range(&self, start: &[u8], end: &[u8]) -> Result<Vec<KvPair>, MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let result: Vec<_> = data
            .range::<Vec<u8>, _>((
                Bound::Included(start.to_vec()),
                Bound::Excluded(end.to_vec()),
            ))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(result)
    }

    fn contains_key(&self, key: &[u8]) -> Result<bool, MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(data.contains_key(key))
    }

    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), MetaError> {
        for op in &ops {
            match op {
                BatchOp::Put { key, value } => {
                    self.write_wal(WalOp::Put {
                        key: key.clone(),
                        value: value.clone(),
                    })?;
                }
                BatchOp::Delete { key } => {
                    self.write_wal(WalOp::Delete { key: key.clone() })?;
                }
            }
        }

        let mut data = self
            .data
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        for op in ops {
            match op {
                BatchOp::Put { key, value } => {
                    data.insert(key, value);
                }
                BatchOp::Delete { key } => {
                    data.remove(&key);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::BatchOp;
    use tempfile::tempdir;

    #[test]
    fn test_put_get() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.get(b"key2").unwrap(), None);
    }

    #[test]
    fn test_delete() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        store.delete(b"key1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), None);
    }

    #[test]
    fn test_scan_prefix() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"dir/a".to_vec(), b"1".to_vec()).unwrap();
        store.put(b"dir/b".to_vec(), b"2".to_vec()).unwrap();
        store.put(b"dir/c".to_vec(), b"3".to_vec()).unwrap();
        store.put(b"other/x".to_vec(), b"4".to_vec()).unwrap();

        let result = store.scan_prefix(b"dir/").unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, b"dir/a");
        assert_eq!(result[1].0, b"dir/b");
        assert_eq!(result[2].0, b"dir/c");
    }

    #[test]
    fn test_scan_range() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"a".to_vec(), b"1".to_vec()).unwrap();
        store.put(b"b".to_vec(), b"2".to_vec()).unwrap();
        store.put(b"c".to_vec(), b"3".to_vec()).unwrap();
        store.put(b"d".to_vec(), b"4".to_vec()).unwrap();

        let result = store.scan_range(b"b", b"d").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, b"b");
        assert_eq!(result[1].0, b"c");
    }

    #[test]
    fn test_write_batch() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"existing".to_vec(), b"old".to_vec()).unwrap();

        store
            .write_batch(vec![
                BatchOp::Put {
                    key: b"new1".to_vec(),
                    value: b"v1".to_vec(),
                },
                BatchOp::Put {
                    key: b"new2".to_vec(),
                    value: b"v2".to_vec(),
                },
                BatchOp::Delete {
                    key: b"existing".to_vec(),
                },
            ])
            .unwrap();

        assert_eq!(store.get(b"new1").unwrap(), Some(b"v1".to_vec()));
        assert_eq!(store.get(b"new2").unwrap(), Some(b"v2".to_vec()));
        assert_eq!(store.get(b"existing").unwrap(), None);
    }

    #[test]
    fn test_contains_key() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        assert!(!store.contains_key(b"key").unwrap());
        store.put(b"key".to_vec(), b"value".to_vec()).unwrap();
        assert!(store.contains_key(b"key").unwrap());
    }

    #[test]
    fn test_overwrite() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"key".to_vec(), b"v1".to_vec()).unwrap();
        store.put(b"key".to_vec(), b"v2".to_vec()).unwrap();
        assert_eq!(store.get(b"key").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn test_crash_recovery_wal_replay() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
            store.put(b"key2".to_vec(), b"value2".to_vec()).unwrap();
            store.delete(b"key1").unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"key1").unwrap(), None);
        assert_eq!(store.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_checkpoint_and_reload() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"a".to_vec(), b"1".to_vec()).unwrap();
            store.put(b"b".to_vec(), b"2".to_vec()).unwrap();
            store.checkpoint().unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(store.get(b"b").unwrap(), Some(b"2".to_vec()));
    }

    #[test]
    fn test_persistence_across_close_reopen() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"persistent".to_vec(), b"data".to_vec()).unwrap();
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        assert_eq!(store.get(b"persistent").unwrap(), Some(b"data".to_vec()));
    }

    #[test]
    fn test_checkpoint_truncates_wal() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            store.put(b"key".to_vec(), b"value".to_vec()).unwrap();
            store.checkpoint().unwrap();
        }

        let wal_path = dir_path.join(WAL_FILENAME);
        let metadata = fs::metadata(&wal_path).unwrap();
        assert_eq!(metadata.len(), 0);
    }

    #[test]
    fn test_recovery_after_multiple_writes() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        {
            let store = PersistentKvStore::open(&dir_path).unwrap();
            for i in 0..100u32 {
                let key = format!("key{}", i);
                let value = format!("value{}", i);
                store.put(key.into_bytes(), value.into_bytes()).unwrap();
            }
        }

        let store = PersistentKvStore::open(&dir_path).unwrap();
        for i in 0..100u32 {
            let key = format!("key{}", i);
            let value = format!("value{}", i);
            assert_eq!(store.get(key.as_bytes()).unwrap(), Some(value.into_bytes()));
        }
    }

    #[test]
    fn test_empty_scan_prefix() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"other".to_vec(), b"val".to_vec()).unwrap();
        let result = store.scan_prefix(b"nonexistent/").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_empty_scan_range() {
        let dir = tempdir().unwrap();
        let store = PersistentKvStore::open(dir.path()).unwrap();
        store.put(b"a".to_vec(), b"1".to_vec()).unwrap();
        let result = store.scan_range(b"x", b"z").unwrap();
        assert!(result.is_empty());
    }
}
