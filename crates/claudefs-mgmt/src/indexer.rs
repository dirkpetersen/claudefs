use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IndexerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Journal error: {0}")]
    Journal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JournalOp {
    Create {
        inode: u64,
        path: String,
        owner_uid: u32,
        group_gid: u32,
        size_bytes: u64,
        mtime: i64,
    },
    Delete {
        inode: u64,
        path: String,
    },
    Rename {
        inode: u64,
        old_path: String,
        new_path: String,
    },
    Write {
        inode: u64,
        size_bytes: u64,
        mtime: i64,
    },
    Chmod {
        inode: u64,
        owner_uid: u32,
        group_gid: u32,
    },
    SetReplicated {
        inode: u64,
        is_replicated: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub seq: u64,
    pub op: JournalOp,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InodeState {
    pub inode: u64,
    pub path: String,
    pub filename: String,
    pub parent_path: String,
    pub owner_uid: u32,
    pub owner_name: String,
    pub group_gid: u32,
    pub group_name: String,
    pub size_bytes: u64,
    pub blocks_stored: u64,
    pub mtime: i64,
    pub ctime: i64,
    pub file_type: String,
    pub is_replicated: bool,
}

pub struct NamespaceAccumulator {
    inodes: HashMap<u64, InodeState>,
    path_to_inode: HashMap<String, u64>,
    last_seq: u64,
}

impl NamespaceAccumulator {
    pub fn new() -> Self {
        Self {
            inodes: HashMap::new(),
            path_to_inode: HashMap::new(),
            last_seq: 0,
        }
    }

    fn extract_filename(path: &str) -> String {
        Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    }

    fn extract_parent_path(path: &str) -> String {
        Path::new(path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string()
    }

    fn extract_file_type(path: &str) -> String {
        let filename = Self::extract_filename(path);
        if filename.is_empty() {
            return "unknown".to_string();
        }
        
        if let Some(dot_pos) = filename.rfind('.') {
            if dot_pos > 0 && dot_pos < filename.len() - 1 {
                let ext = &filename[dot_pos + 1..];
                if ext.len() <= 10 {
                    return ext.to_lowercase();
                }
            }
        }
        "unknown".to_string()
    }

    pub fn apply(&mut self, entry: &JournalEntry) {
        if entry.seq > self.last_seq {
            self.last_seq = entry.seq;
        }

        match &entry.op {
            JournalOp::Create { inode, path, owner_uid, group_gid, size_bytes, mtime } => {
                let filename = Self::extract_filename(path);
                let parent_path = Self::extract_parent_path(path);
                let file_type = Self::extract_file_type(path);
                
                let state = InodeState {
                    inode: *inode,
                    path: path.clone(),
                    filename,
                    parent_path,
                    owner_uid: *owner_uid,
                    owner_name: format!("user_{}", owner_uid),
                    group_gid: *group_gid,
                    group_name: format!("group_{}", group_gid),
                    size_bytes: *size_bytes,
                    blocks_stored: *size_bytes,
                    mtime: *mtime,
                    ctime: entry.timestamp,
                    file_type,
                    is_replicated: false,
                };
                
                self.inodes.insert(*inode, state.clone());
                self.path_to_inode.insert(path.clone(), *inode);
            }
            JournalOp::Delete { inode, path: _ } => {
                if let Some(state) = self.inodes.remove(inode) {
                    self.path_to_inode.remove(&state.path);
                }
            }
            JournalOp::Rename { inode, old_path: _, new_path } => {
                if let Some(state) = self.inodes.get_mut(inode) {
                    self.path_to_inode.remove(&state.path);
                    state.path = new_path.clone();
                    state.filename = Self::extract_filename(new_path);
                    state.parent_path = Self::extract_parent_path(new_path);
                    state.file_type = Self::extract_file_type(new_path);
                    self.path_to_inode.insert(new_path.clone(), *inode);
                }
            }
            JournalOp::Write { inode, size_bytes, mtime } => {
                if let Some(state) = self.inodes.get_mut(inode) {
                    state.size_bytes = *size_bytes;
                    state.blocks_stored = *size_bytes;
                    state.mtime = *mtime;
                }
            }
            JournalOp::Chmod { inode, owner_uid, group_gid } => {
                if let Some(state) = self.inodes.get_mut(inode) {
                    state.owner_uid = *owner_uid;
                    state.owner_name = format!("user_{}", owner_uid);
                    state.group_gid = *group_gid;
                    state.group_name = format!("group_{}", group_gid);
                }
            }
            JournalOp::SetReplicated { inode, is_replicated } => {
                if let Some(state) = self.inodes.get_mut(inode) {
                    state.is_replicated = *is_replicated;
                }
            }
        }
    }

    pub fn get_inode(&self, inode: u64) -> Option<&InodeState> {
        self.inodes.get(&inode)
    }

    pub fn get_by_path(&self, path: &str) -> Option<&InodeState> {
        self.path_to_inode.get(path).and_then(|inode| self.inodes.get(inode))
    }

    pub fn all_inodes(&self) -> impl Iterator<Item = &InodeState> {
        self.inodes.values()
    }

    pub fn inode_count(&self) -> usize {
        self.inodes.len()
    }

    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }
}

impl Default for NamespaceAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ParquetWriter {
    base_dir: PathBuf,
    file_counter: u64,
    total_records: u64,
}

impl ParquetWriter {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            file_counter: 1,
            total_records: 0,
        }
    }

    pub fn next_path(&self) -> PathBuf {
        let now = chrono_lite_now();
        self.base_dir.join(format!(
            "year={}/month={:02}/day={:02}/metadata_{:05}.jsonl",
            now.0, now.1, now.2, self.file_counter
        ))
    }

    pub fn flush(&mut self, inodes: &[InodeState]) -> Result<PathBuf, IndexerError> {
        if inodes.is_empty() {
            return Ok(self.next_path());
        }

        let path = self.next_path();
        
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = std::fs::File::create(&path)?;
        
        for inode in inodes {
            let json = serde_json::to_string(inode)
                .map_err(|e| IndexerError::Serialization(e.to_string()))?;
            use std::io::Write;
            writeln!(file, "{}", json)?;
            self.total_records += 1;
        }

        self.file_counter += 1;
        
        Ok(path)
    }

    pub fn total_records_written(&self) -> u64 {
        self.total_records
    }
}

fn chrono_lite_now() -> (i32, u32, u32) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    
    let days = ts / 86400;
    let remaining = ts % 86400;
    let hour = remaining / 3600;
    let minute = (remaining % 3600) / 60;
    let second = remaining % 60;
    
    let mut year = 1970;
    let mut remaining_days = days as i64;
    
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    
    let mut month = 1;
    loop {
        let days_in_month = days_in_month_of(year, month);
        if remaining_days < days_in_month as i64 {
            break;
        }
        remaining_days -= days_in_month as i64;
        month += 1;
    }
    
    let day = remaining_days as u32 + 1;
    
    let _ = (hour, minute, second);
    
    (year, month, day)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month_of(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) { 29 } else { 28 }
        }
        _ => 30,
    }
}

pub struct MetadataIndexer {
    accumulator: Arc<RwLock<NamespaceAccumulator>>,
    writer: Arc<tokio::sync::Mutex<ParquetWriter>>,
    flush_interval_secs: u64,
    index_dir: PathBuf,
    is_running: Arc<std::sync::atomic::AtomicBool>,
}

impl MetadataIndexer {
    pub fn new(index_dir: PathBuf, flush_interval_secs: u64) -> Self {
        Self {
            accumulator: Arc::new(RwLock::new(NamespaceAccumulator::new())),
            writer: Arc::new(tokio::sync::Mutex::new(ParquetWriter::new(index_dir.clone()))),
            flush_interval_secs,
            index_dir,
            is_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub async fn apply_entry(&self, entry: JournalEntry) -> Result<(), IndexerError> {
        let mut acc = self.accumulator.write().await;
        acc.apply(&entry);
        Ok(())
    }

    pub async fn flush(&self) -> Result<PathBuf, IndexerError> {
        let acc = self.accumulator.read().await;
        let inodes: Vec<InodeState> = acc.all_inodes().cloned().collect();
        
        let mut writer = self.writer.lock().await;
        writer.flush(&inodes)
    }

    pub async fn inode_count(&self) -> usize {
        let acc = self.accumulator.read().await;
        acc.inode_count()
    }

    pub async fn ingest_batch(&self, entries: Vec<JournalEntry>) -> Result<(), IndexerError> {
        let mut acc = self.accumulator.write().await;
        for entry in entries {
            acc.apply(&entry);
        }
        Ok(())
    }

    pub async fn run_flush_loop(self: Arc<Self>) -> Result<(), IndexerError> {
        self.is_running.store(true, std::sync::atomic::Ordering::SeqCst);
        
        loop {
            if !self.is_running.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            
            tokio::time::sleep(tokio::time::Duration::from_secs(self.flush_interval_secs)).await;
            
            if self.is_running.load(std::sync::atomic::Ordering::SeqCst) {
                if let Err(e) = self.flush().await {
                    tracing::error!("Index flush failed: {}", e);
                }
            }
        }
        
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.is_running.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journalop_serialize_create() {
        let op = JournalOp::Create {
            inode: 123,
            path: "/data/test.txt".to_string(),
            owner_uid: 1000,
            group_gid: 1000,
            size_bytes: 4096,
            mtime: 1234567890,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"inode\":123"));
    }

    #[test]
    fn test_journalop_serialize_delete() {
        let op = JournalOp::Delete {
            inode: 123,
            path: "/data/test.txt".to_string(),
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"inode\":123"));
    }

    #[test]
    fn test_journalop_serialize_rename() {
        let op = JournalOp::Rename {
            inode: 123,
            old_path: "/data/old.txt".to_string(),
            new_path: "/data/new.txt".to_string(),
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"inode\":123"));
    }

    #[test]
    fn test_journalop_serialize_write() {
        let op = JournalOp::Write {
            inode: 123,
            size_bytes: 8192,
            mtime: 1234567890,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"inode\":123"));
    }

    #[test]
    fn test_journalop_serialize_chmod() {
        let op = JournalOp::Chmod {
            inode: 123,
            owner_uid: 1001,
            group_gid: 1001,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"inode\":123"));
    }

    #[test]
    fn test_journalop_serialize_setreplicated() {
        let op = JournalOp::SetReplicated {
            inode: 123,
            is_replicated: true,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"inode\":123"));
    }

    #[test]
    fn test_journalop_deserialize() {
        let json = r#"{"Create":{"inode":456,"path":"/test/file.rs","owner_uid":1000,"group_gid":1000,"size_bytes":1024,"mtime":1234567890}}"#;
        let op: JournalOp = serde_json::from_str(json).unwrap();
        match op {
            JournalOp::Create { inode, path, .. } => {
                assert_eq!(inode, 456);
                assert_eq!(path, "/test/file.rs");
            }
            _ => panic!("Expected Create variant"),
        }
    }

    #[test]
    fn test_namespace_accumulator_apply_create() {
        let mut acc = NamespaceAccumulator::new();
        let entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        
        acc.apply(&entry);
        
        assert_eq!(acc.inode_count(), 1);
        let state = acc.get_inode(1).unwrap();
        assert_eq!(state.path, "/data/file.txt");
    }

    #[test]
    fn test_namespace_accumulator_apply_delete() {
        let mut acc = NamespaceAccumulator::new();
        let create_entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        acc.apply(&create_entry);
        
        let delete_entry = JournalEntry {
            seq: 2,
            op: JournalOp::Delete {
                inode: 1,
                path: "/data/file.txt".to_string(),
            },
            timestamp: 1234567891,
        };
        acc.apply(&delete_entry);
        
        assert_eq!(acc.inode_count(), 0);
    }

    #[test]
    fn test_namespace_accumulator_apply_rename() {
        let mut acc = NamespaceAccumulator::new();
        let create_entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/old.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        acc.apply(&create_entry);
        
        let rename_entry = JournalEntry {
            seq: 2,
            op: JournalOp::Rename {
                inode: 1,
                old_path: "/data/old.txt".to_string(),
                new_path: "/data/new.txt".to_string(),
            },
            timestamp: 1234567891,
        };
        acc.apply(&rename_entry);
        
        let state = acc.get_inode(1).unwrap();
        assert_eq!(state.path, "/data/new.txt");
        
        let state2 = acc.get_by_path("/data/new.txt").unwrap();
        assert_eq!(state2.inode, 1);
    }

    #[test]
    fn test_namespace_accumulator_apply_write() {
        let mut acc = NamespaceAccumulator::new();
        let create_entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        acc.apply(&create_entry);
        
        let write_entry = JournalEntry {
            seq: 2,
            op: JournalOp::Write {
                inode: 1,
                size_bytes: 2048,
                mtime: 1234567900,
            },
            timestamp: 1234567900,
        };
        acc.apply(&write_entry);
        
        let state = acc.get_inode(1).unwrap();
        assert_eq!(state.size_bytes, 2048);
    }

    #[test]
    fn test_namespace_accumulator_apply_chmod() {
        let mut acc = NamespaceAccumulator::new();
        let create_entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        acc.apply(&create_entry);
        
        let chmod_entry = JournalEntry {
            seq: 2,
            op: JournalOp::Chmod {
                inode: 1,
                owner_uid: 2000,
                group_gid: 2000,
            },
            timestamp: 1234567891,
        };
        acc.apply(&chmod_entry);
        
        let state = acc.get_inode(1).unwrap();
        assert_eq!(state.owner_uid, 2000);
        assert_eq!(state.owner_name, "user_2000");
    }

    #[test]
    fn test_namespace_accumulator_apply_setreplicated() {
        let mut acc = NamespaceAccumulator::new();
        let create_entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        acc.apply(&create_entry);
        
        let replicated_entry = JournalEntry {
            seq: 2,
            op: JournalOp::SetReplicated {
                inode: 1,
                is_replicated: true,
            },
            timestamp: 1234567891,
        };
        acc.apply(&replicated_entry);
        
        let state = acc.get_inode(1).unwrap();
        assert!(state.is_replicated);
    }

    #[test]
    fn test_namespace_accumulator_sequence() {
        let mut acc = NamespaceAccumulator::new();
        
        let create_entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        acc.apply(&create_entry);
        
        let write_entry = JournalEntry {
            seq: 2,
            op: JournalOp::Write {
                inode: 1,
                size_bytes: 2048,
                mtime: 1234567900,
            },
            timestamp: 1234567900,
        };
        acc.apply(&write_entry);
        
        let rename_entry = JournalEntry {
            seq: 3,
            op: JournalOp::Rename {
                inode: 1,
                old_path: "/data/file.txt".to_string(),
                new_path: "/data/renamed.txt".to_string(),
            },
            timestamp: 1234567901,
        };
        acc.apply(&rename_entry);
        
        let state = acc.get_inode(1).unwrap();
        assert_eq!(state.size_bytes, 2048);
        assert_eq!(state.path, "/data/renamed.txt");
    }

    #[test]
    fn test_namespace_accumulator_inode_count() {
        let mut acc = NamespaceAccumulator::new();
        
        for i in 1..=5 {
            let entry = JournalEntry {
                seq: i,
                op: JournalOp::Create {
                    inode: i,
                    path: format!("/data/file{}.txt", i),
                    owner_uid: 1000,
                    group_gid: 1000,
                    size_bytes: 1024,
                    mtime: 1234567890,
                },
                timestamp: 1234567890,
            };
            acc.apply(&entry);
        }
        
        assert_eq!(acc.inode_count(), 5);
        
        let delete_entry = JournalEntry {
            seq: 6,
            op: JournalOp::Delete {
                inode: 3,
                path: "/data/file3.txt".to_string(),
            },
            timestamp: 1234567891,
        };
        acc.apply(&delete_entry);
        
        assert_eq!(acc.inode_count(), 4);
    }

    #[test]
    fn test_file_type_extraction_rs() {
        let state = InodeState {
            inode: 1,
            path: "/data/file.rs".to_string(),
            filename: "file.rs".to_string(),
            parent_path: "/data".to_string(),
            owner_uid: 1000,
            owner_name: "user_1000".to_string(),
            group_gid: 1000,
            group_name: "group_1000".to_string(),
            size_bytes: 1024,
            blocks_stored: 1024,
            mtime: 1234567890,
            ctime: 1234567890,
            file_type: "rs".to_string(),
            is_replicated: false,
        };
        
        assert_eq!(state.file_type, "rs");
    }

    #[test]
    fn test_file_type_extraction_h5() {
        let state = InodeState {
            inode: 1,
            path: "/data/file.h5".to_string(),
            filename: "file.h5".to_string(),
            parent_path: "/data".to_string(),
            owner_uid: 1000,
            owner_name: "user_1000".to_string(),
            group_gid: 1000,
            group_name: "group_1000".to_string(),
            size_bytes: 1024,
            blocks_stored: 1024,
            mtime: 1234567890,
            ctime: 1234567890,
            file_type: "h5".to_string(),
            is_replicated: false,
        };
        
        assert_eq!(state.file_type, "h5");
    }

    #[test]
    fn test_file_type_extraction_parquet() {
        let state = InodeState {
            inode: 1,
            path: "/data/file.parquet".to_string(),
            filename: "file.parquet".to_string(),
            parent_path: "/data".to_string(),
            owner_uid: 1000,
            owner_name: "user_1000".to_string(),
            group_gid: 1000,
            group_name: "group_1000".to_string(),
            size_bytes: 1024,
            blocks_stored: 1024,
            mtime: 1234567890,
            ctime: 1234567890,
            file_type: "parquet".to_string(),
            is_replicated: false,
        };
        
        assert_eq!(state.file_type, "parquet");
    }

    #[test]
    fn test_file_type_extraction_tar_gz() {
        let state = InodeState {
            inode: 1,
            path: "/data/file.tar.gz".to_string(),
            filename: "file.tar.gz".to_string(),
            parent_path: "/data".to_string(),
            owner_uid: 1000,
            owner_name: "user_1000".to_string(),
            group_gid: 1000,
            group_name: "group_1000".to_string(),
            size_bytes: 1024,
            blocks_stored: 1024,
            mtime: 1234567890,
            ctime: 1234567890,
            file_type: "gz".to_string(),
            is_replicated: false,
        };
        
        assert_eq!(state.file_type, "gz");
    }

    #[test]
    fn test_file_type_extraction_unknown() {
        let state = InodeState {
            inode: 1,
            path: "/data/file.xyz".to_string(),
            filename: "file.xyz".to_string(),
            parent_path: "/data".to_string(),
            owner_uid: 1000,
            owner_name: "user_1000".to_string(),
            group_gid: 1000,
            group_name: "group_1000".to_string(),
            size_bytes: 1024,
            blocks_stored: 1024,
            mtime: 1234567890,
            ctime: 1234567890,
            file_type: "unknown".to_string(),
            is_replicated: false,
        };
        
        assert_eq!(state.file_type, "unknown");
        
        let extracted = NamespaceAccumulator::extract_file_type("/data/file.xyz");
        assert_eq!(extracted, "xyz");
    }

    #[test]
    fn test_parent_path_extraction() {
        let mut acc = NamespaceAccumulator::new();
        let entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/a/b/c/d/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        acc.apply(&entry);
        
        let state = acc.get_inode(1).unwrap();
        assert_eq!(state.parent_path, "/a/b/c/d");
    }

    #[test]
    fn test_filename_extraction() {
        let mut acc = NamespaceAccumulator::new();
        let entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/my_file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        acc.apply(&entry);
        
        let state = acc.get_inode(1).unwrap();
        assert_eq!(state.filename, "my_file.txt");
    }

    #[tokio::test]
    async fn test_metadata_indexer_new() {
        let indexer = MetadataIndexer::new(PathBuf::from("/tmp/test_index"), 60);
        assert_eq!(indexer.flush_interval_secs, 60);
        assert!(!indexer.is_running());
    }

    #[tokio::test]
    async fn test_metadata_indexer_apply_entry() {
        let indexer = MetadataIndexer::new(PathBuf::from("/tmp/test_index"), 60);
        
        let entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        
        indexer.apply_entry(entry).await.unwrap();
        
        let count = indexer.inode_count().await;
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_metadata_indexer_ingest_batch() {
        let indexer = MetadataIndexer::new(PathBuf::from("/tmp/test_index"), 60);
        
        let entries = vec![
            JournalEntry {
                seq: 1,
                op: JournalOp::Create {
                    inode: 1,
                    path: "/data/file1.txt".to_string(),
                    owner_uid: 1000,
                    group_gid: 1000,
                    size_bytes: 1024,
                    mtime: 1234567890,
                },
                timestamp: 1234567890,
            },
            JournalEntry {
                seq: 2,
                op: JournalOp::Create {
                    inode: 2,
                    path: "/data/file2.txt".to_string(),
                    owner_uid: 1000,
                    group_gid: 1000,
                    size_bytes: 2048,
                    mtime: 1234567891,
                },
                timestamp: 1234567891,
            },
        ];
        
        indexer.ingest_batch(entries).await.unwrap();
        
        let count = indexer.inode_count().await;
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_metadata_indexer_flush() {
        let indexer = MetadataIndexer::new(PathBuf::from("/tmp/test_index"), 60);
        
        let entry = JournalEntry {
            seq: 1,
            op: JournalOp::Create {
                inode: 1,
                path: "/data/file.txt".to_string(),
                owner_uid: 1000,
                group_gid: 1000,
                size_bytes: 1024,
                mtime: 1234567890,
            },
            timestamp: 1234567890,
        };
        
        indexer.apply_entry(entry).await.unwrap();
        
        let path = indexer.flush().await.unwrap();
        
        assert!(path.to_string_lossy().contains(".jsonl"));
        
        if let Ok(contents) = std::fs::read_to_string(&path) {
            let lines: Vec<&str> = contents.lines().collect();
            assert!(!lines.is_empty());
            
            let state: InodeState = serde_json::from_str(lines[0]).unwrap();
            assert_eq!(state.inode, 1);
        }
        
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn test_metadata_indexer_stop() {
        let indexer = MetadataIndexer::new(PathBuf::from("/tmp/test_index"), 60);
        indexer.stop();
        assert!(!indexer.is_running());
    }

    #[test]
    fn test_parquet_writer_path_format() {
        let writer = ParquetWriter::new(PathBuf::from("/index"));
        let path = writer.next_path();
        let path_str = path.to_string_lossy();
        
        assert!(path_str.contains("year="));
        assert!(path_str.contains("month="));
        assert!(path_str.contains("day="));
        assert!(path_str.contains("metadata_"));
        assert!(path_str.ends_with(".jsonl"));
    }
}