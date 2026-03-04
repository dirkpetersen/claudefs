//! High-throughput batch file ingestion.
//!
//! This module provides high-throughput batch file creation for ML training workflows
//! (thousands of file creates per second). Batch ingest groups many CreateInode + CreateEntry
//! operations into a single Raft proposal.

use std::sync::Arc;

use crate::directory::DirectoryStore;
use crate::inode::InodeStore;
use crate::journal::MetadataJournal;
use crate::types::{DirEntry, FileType, InodeAttr, InodeId, LogIndex, MetaError, MetaOp};

/// A single file to create in a batch.
#[derive(Clone, Debug)]
pub struct BatchFileSpec {
    /// Filename (not path).
    pub name: String,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Permission bits.
    pub mode: u32,
    /// Pre-allocated size (0 for empty).
    pub size: u64,
}

/// Result of creating one file in a batch.
#[derive(Clone, Debug)]
pub struct BatchFileResult {
    /// The filename.
    pub name: String,
    /// The created inode ID.
    pub ino: InodeId,
}

/// Configuration for batch ingest behavior.
#[derive(Clone, Debug)]
pub struct BatchIngestConfig {
    /// Max files per batch (default 1000).
    pub max_batch_size: usize,
    /// Flush when this many files queued (default 500).
    pub auto_flush_at: usize,
}

impl Default for BatchIngestConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            auto_flush_at: 500,
        }
    }
}

/// High-throughput batch file creator.
///
/// Queues file creation requests and flushes them as a single Raft proposal
/// via the MetadataJournal for durability.
pub struct BatchIngestor {
    parent_ino: InodeId,
    config: BatchIngestConfig,
    pending: Vec<BatchFileSpec>,
    inode_store: Arc<InodeStore>,
    dir_store: Arc<DirectoryStore>,
    journal: Arc<MetadataJournal>,
    inode_counter_base: u64,
}

impl BatchIngestor {
    /// Create a new batch ingestor for files under `parent_ino`.
    pub fn new(
        parent_ino: InodeId,
        inode_store: Arc<InodeStore>,
        dir_store: Arc<DirectoryStore>,
        journal: Arc<MetadataJournal>,
        config: BatchIngestConfig,
    ) -> Self {
        Self {
            parent_ino,
            config,
            pending: Vec::new(),
            inode_store,
            dir_store,
            journal,
            inode_counter_base: 0,
        }
    }

    /// Queue a file for creation. Automatically flushes if auto_flush_at is reached.
    /// Returns Ok(None) if buffered, Ok(Some(results)) if auto-flushed.
    pub fn add(&mut self, spec: BatchFileSpec) -> Result<Option<Vec<BatchFileResult>>, MetaError> {
        // Check for duplicate names within batch
        for pending_spec in &self.pending {
            if pending_spec.name == spec.name {
                return Err(MetaError::EntryExists {
                    parent: self.parent_ino,
                    name: spec.name,
                });
            }
        }

        // Check batch size limit
        if self.pending.len() >= self.config.max_batch_size {
            return Err(MetaError::RaftError("batch size exceeded".to_string()));
        }

        self.pending.push(spec);

        // Auto-flush if threshold reached
        if self.pending.len() >= self.config.auto_flush_at {
            self.flush().map(Some)
        } else {
            Ok(None)
        }
    }

    /// Flush all pending files. Creates them and appends to journal.
    /// Returns the created file results.
    pub fn flush(&mut self) -> Result<Vec<BatchFileResult>, MetaError> {
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }

        // Capture current pending and reset
        let files = std::mem::take(&mut self.pending);
        let mut results = Vec::with_capacity(files.len());

        // Record starting inode for this batch
        let start_ino = self.inode_store.allocate_inode();
        self.inode_counter_base = start_ino.as_u64();

        // Create each file
        for (i, spec) in files.into_iter().enumerate() {
            let ino = InodeId::new(self.inode_counter_base + i as u64);

            // Create inode attribute
            let attr = InodeAttr::new_file(ino, spec.uid, spec.gid, spec.mode, 1);
            let mut attr = attr;
            attr.size = spec.size;

            // Store inode
            self.inode_store.create_inode(&attr)?;

            // Create directory entry
            let entry = DirEntry {
                name: spec.name.clone(),
                ino,
                file_type: FileType::RegularFile,
            };
            self.dir_store.create_entry(self.parent_ino, &entry)?;

            // Append to journal (batch as single proposal)
            let op = MetaOp::CreateInode { attr };
            let log_index = LogIndex::new(i as u64 + 1);
            self.journal.append(op, log_index)?;

            results.push(BatchFileResult {
                name: spec.name,
                ino,
            });
        }

        Ok(results)
    }

    /// Number of files currently queued.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Whether the buffer needs flushing.
    pub fn needs_flush(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::{KvStore, MemoryKvStore};

    fn create_test_ingestor() -> (
        BatchIngestor,
        Arc<InodeStore>,
        Arc<DirectoryStore>,
        Arc<MetadataJournal>,
    ) {
        let kv: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let inode_store = Arc::new(InodeStore::new(kv.clone()));
        let dir_store = Arc::new(DirectoryStore::new(kv, inode_store.clone()));
        let journal = Arc::new(MetadataJournal::new(1, 1000));
        let config = BatchIngestConfig::default();

        let ingestor = BatchIngestor::new(
            InodeId::ROOT_INODE,
            inode_store.clone(),
            dir_store.clone(),
            journal.clone(),
            config,
        );

        (ingestor, inode_store, dir_store, journal)
    }

    #[test]
    fn test_batch_ingest_single_file() {
        let (mut ingestor, _, dir_store, _) = create_test_ingestor();

        let spec = BatchFileSpec {
            name: "test.txt".to_string(),
            uid: 1000,
            gid: 1000,
            mode: 0o644,
            size: 0,
        };

        let result = ingestor.add(spec).unwrap();
        assert!(result.is_none());
        assert_eq!(ingestor.pending_count(), 1);

        let results = ingestor.flush().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "test.txt");

        // Verify entry exists in directory
        let entry = dir_store.lookup(InodeId::ROOT_INODE, "test.txt").unwrap();
        assert_eq!(entry.name, "test.txt");
    }

    #[test]
    fn test_batch_ingest_many_files() {
        let (mut ingestor, _, dir_store, _) = create_test_ingestor();

        for i in 0..100 {
            let spec = BatchFileSpec {
                name: format!("file_{}.txt", i),
                uid: 1000,
                gid: 1000,
                mode: 0o644,
                size: 0,
            };
            ingestor.add(spec).unwrap();
        }

        assert!(ingestor.needs_flush());

        let results = ingestor.flush().unwrap();
        assert_eq!(results.len(), 100);

        // Verify all entries exist
        for i in 0..100 {
            let name = format!("file_{}.txt", i);
            let entry = dir_store.lookup(InodeId::ROOT_INODE, &name).unwrap();
            assert_eq!(entry.name, name);
        }
    }

    #[test]
    fn test_batch_auto_flush() {
        let kv: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let inode_store = Arc::new(InodeStore::new(kv.clone()));
        let dir_store = Arc::new(DirectoryStore::new(kv, inode_store.clone()));
        let journal = Arc::new(MetadataJournal::new(1, 1000));

        // Config with auto_flush_at = 3
        let config = BatchIngestConfig {
            max_batch_size: 100,
            auto_flush_at: 3,
        };

        let mut ingestor =
            BatchIngestor::new(InodeId::ROOT_INODE, inode_store, dir_store, journal, config);

        // Add 3 files - should auto-flush
        for i in 0..3 {
            let spec = BatchFileSpec {
                name: format!("auto_{}.txt", i),
                uid: 1000,
                gid: 1000,
                mode: 0o644,
                size: 0,
            };
            let result = ingestor.add(spec).unwrap();
            assert!(result.is_some()); // Auto-flushed
            assert_eq!(ingestor.pending_count(), 0);
        }
    }

    #[test]
    fn test_batch_ingest_names_unique() {
        let (mut ingestor, _, _, _) = create_test_ingestor();

        // Add first file
        let spec1 = BatchFileSpec {
            name: "duplicate.txt".to_string(),
            uid: 1000,
            gid: 1000,
            mode: 0o644,
            size: 0,
        };
        ingestor.add(spec1).unwrap();

        // Try to add duplicate
        let spec2 = BatchFileSpec {
            name: "duplicate.txt".to_string(),
            uid: 1000,
            gid: 1000,
            mode: 0o644,
            size: 0,
        };
        let result = ingestor.add(spec2);
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_ingest_inode_ids_unique() {
        let (mut ingestor, _, _, _) = create_test_ingestor();

        // Add many files
        for i in 0..50 {
            let spec = BatchFileSpec {
                name: format!("file_{}.txt", i),
                uid: 1000,
                gid: 1000,
                mode: 0o644,
                size: 0,
            };
            ingestor.add(spec).unwrap();
        }

        let results = ingestor.flush().unwrap();

        // Verify all inodes are unique
        let mut ino_set = std::collections::HashSet::new();
        for result in &results {
            assert!(ino_set.insert(result.ino.as_u64()));
        }
        assert_eq!(ino_set.len(), 50);
    }

    #[test]
    fn test_batch_ingest_journal_entries() {
        let (mut ingestor, _, _, journal) = create_test_ingestor();

        // Add files
        for i in 0..5 {
            let spec = BatchFileSpec {
                name: format!("journal_{}.txt", i),
                uid: 1000,
                gid: 1000,
                mode: 0o644,
                size: 0,
            };
            ingestor.add(spec).unwrap();
        }

        let _ = ingestor.flush().unwrap();

        // Read from journal
        let entries = journal.read_from(1, 10).unwrap();
        assert_eq!(entries.len(), 5);

        for (i, entry) in entries.iter().enumerate() {
            match &entry.op {
                MetaOp::CreateInode { attr } => {
                    assert_eq!(attr.file_type, FileType::RegularFile);
                }
                _ => panic!("expected CreateInode operation"),
            }
        }
    }

    #[test]
    fn test_batch_empty_flush() {
        let (mut ingestor, _, _, _) = create_test_ingestor();

        let results = ingestor.flush().unwrap();
        assert!(results.is_empty());
        assert!(!ingestor.needs_flush());
    }

    #[test]
    fn test_batch_ingest_large_batch() {
        let (mut ingestor, _, _, _) = create_test_ingestor();

        // Add 1000 files (max default)
        for i in 0..1000 {
            let spec = BatchFileSpec {
                name: format!("large_{}.txt", i),
                uid: 1000,
                gid: 1000,
                mode: 0o644,
                size: 0,
            };
            ingestor.add(spec).unwrap();
        }

        let results = ingestor.flush().unwrap();
        assert_eq!(results.len(), 1000);
    }
}
