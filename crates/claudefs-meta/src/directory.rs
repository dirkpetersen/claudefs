//! Directory operations for the metadata service.
//!
//! Directory entries are stored in the KV store with key format
//! `dirent/{parent_inode_be}/{entry_name}`. This allows efficient prefix
//! scans for readdir() operations.

use std::sync::Arc;

use crate::inode::InodeStore;
use crate::kvstore::KvStore;
use crate::types::{DirEntry, FileType, InodeId, MetaError};

/// Key prefix for directory entries in the KV store.
const DIRENT_PREFIX: &[u8] = b"dirent/";

/// Builds the KV store key prefix for all entries in a directory.
fn dirent_prefix(parent: InodeId) -> Vec<u8> {
    let mut key = DIRENT_PREFIX.to_vec();
    key.extend_from_slice(&parent.as_u64().to_be_bytes());
    key.push(b'/');
    key
}

/// Builds the KV store key for a specific directory entry.
fn dirent_key(parent: InodeId, name: &str) -> Vec<u8> {
    let mut key = dirent_prefix(parent);
    key.extend_from_slice(name.as_bytes());
    key
}

/// Manages directory entry operations on top of the KV store.
pub struct DirectoryStore {
    kv: Arc<dyn KvStore>,
    inodes: Arc<InodeStore>,
}

impl DirectoryStore {
    /// Creates a new DirectoryStore.
    pub fn new(kv: Arc<dyn KvStore>, inodes: Arc<InodeStore>) -> Self {
        Self { kv, inodes }
    }

    /// Creates a directory entry in the parent directory.
    /// Validates that the parent is a directory and the entry doesn't already exist.
    pub fn create_entry(&self, parent: InodeId, entry: &DirEntry) -> Result<(), MetaError> {
        // Verify parent is a directory
        let parent_attr = self.inodes.get_inode(parent)?;
        if parent_attr.file_type != FileType::Directory {
            return Err(MetaError::NotADirectory(parent));
        }

        let key = dirent_key(parent, &entry.name);
        if self.kv.contains_key(&key)? {
            return Err(MetaError::EntryExists {
                parent,
                name: entry.name.clone(),
            });
        }

        let value = bincode::serialize(entry).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key, value)
    }

    /// Deletes a directory entry from the parent directory.
    pub fn delete_entry(&self, parent: InodeId, name: &str) -> Result<DirEntry, MetaError> {
        let key = dirent_key(parent, name);
        let value = self.kv.get(&key)?.ok_or_else(|| MetaError::EntryNotFound {
            parent,
            name: name.to_string(),
        })?;

        let entry: DirEntry =
            bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.delete(&key)?;
        Ok(entry)
    }

    /// Looks up a directory entry by name.
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<DirEntry, MetaError> {
        let key = dirent_key(parent, name);
        let value = self.kv.get(&key)?.ok_or_else(|| MetaError::EntryNotFound {
            parent,
            name: name.to_string(),
        })?;

        bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))
    }

    /// Lists all entries in a directory, sorted by name.
    pub fn list_entries(&self, parent: InodeId) -> Result<Vec<DirEntry>, MetaError> {
        let prefix = dirent_prefix(parent);
        let entries = self.kv.scan_prefix(&prefix)?;

        entries
            .into_iter()
            .map(|(_key, value)| {
                bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))
            })
            .collect()
    }

    /// Returns true if the directory has any entries.
    pub fn is_empty(&self, parent: InodeId) -> Result<bool, MetaError> {
        let prefix = dirent_prefix(parent);
        let entries = self.kv.scan_prefix(&prefix)?;
        Ok(entries.is_empty())
    }

    /// Renames a directory entry. Supports cross-directory renames.
    pub fn rename(
        &self,
        src_parent: InodeId,
        src_name: &str,
        dst_parent: InodeId,
        dst_name: &str,
    ) -> Result<(), MetaError> {
        // Look up the source entry
        let entry = self.lookup(src_parent, src_name)?;

        // Create a new entry with the destination name
        let new_entry = DirEntry {
            name: dst_name.to_string(),
            ino: entry.ino,
            file_type: entry.file_type,
        };

        // If destination exists, remove it first (POSIX rename semantics)
        let dst_key = dirent_key(dst_parent, dst_name);
        if self.kv.contains_key(&dst_key)? {
            self.kv.delete(&dst_key)?;
        }

        // Write new entry and delete old one atomically via batch
        let src_key = dirent_key(src_parent, src_name);
        let value =
            bincode::serialize(&new_entry).map_err(|e| MetaError::KvError(e.to_string()))?;

        use crate::kvstore::BatchOp;
        self.kv.write_batch(vec![
            BatchOp::Put {
                key: dst_key,
                value,
            },
            BatchOp::Delete { key: src_key },
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use crate::types::{FileType, InodeAttr, InodeId};

    fn make_stores() -> (Arc<dyn KvStore>, Arc<InodeStore>, DirectoryStore) {
        let kv: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());

        // Create root directory inode
        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        inodes.create_inode(&root).unwrap();

        (kv, inodes, dirs)
    }

    #[test]
    fn test_create_and_lookup_entry() {
        let (_kv, inodes, dirs) = make_stores();

        let child_ino = inodes.allocate_inode();
        let child_attr = InodeAttr::new_file(child_ino, 1000, 1000, 0o644, 1);
        inodes.create_inode(&child_attr).unwrap();

        let entry = DirEntry {
            name: "hello.txt".to_string(),
            ino: child_ino,
            file_type: FileType::RegularFile,
        };
        dirs.create_entry(InodeId::ROOT_INODE, &entry).unwrap();

        let found = dirs.lookup(InodeId::ROOT_INODE, "hello.txt").unwrap();
        assert_eq!(found.ino, child_ino);
        assert_eq!(found.file_type, FileType::RegularFile);
    }

    #[test]
    fn test_create_duplicate_entry() {
        let (_kv, inodes, dirs) = make_stores();

        let child_ino = inodes.allocate_inode();
        let child_attr = InodeAttr::new_file(child_ino, 1000, 1000, 0o644, 1);
        inodes.create_inode(&child_attr).unwrap();

        let entry = DirEntry {
            name: "file.txt".to_string(),
            ino: child_ino,
            file_type: FileType::RegularFile,
        };
        dirs.create_entry(InodeId::ROOT_INODE, &entry).unwrap();

        match dirs.create_entry(InodeId::ROOT_INODE, &entry) {
            Err(MetaError::EntryExists { .. }) => {}
            other => panic!("expected EntryExists, got {:?}", other),
        }
    }

    #[test]
    fn test_delete_entry() {
        let (_kv, inodes, dirs) = make_stores();

        let child_ino = inodes.allocate_inode();
        let child_attr = InodeAttr::new_file(child_ino, 1000, 1000, 0o644, 1);
        inodes.create_inode(&child_attr).unwrap();

        let entry = DirEntry {
            name: "file.txt".to_string(),
            ino: child_ino,
            file_type: FileType::RegularFile,
        };
        dirs.create_entry(InodeId::ROOT_INODE, &entry).unwrap();

        let deleted = dirs.delete_entry(InodeId::ROOT_INODE, "file.txt").unwrap();
        assert_eq!(deleted.ino, child_ino);

        match dirs.lookup(InodeId::ROOT_INODE, "file.txt") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_list_entries() {
        let (_kv, inodes, dirs) = make_stores();

        for i in 0..3 {
            let ino = inodes.allocate_inode();
            let attr = InodeAttr::new_file(ino, 1000, 1000, 0o644, 1);
            inodes.create_inode(&attr).unwrap();

            let entry = DirEntry {
                name: format!("file{}.txt", i),
                ino,
                file_type: FileType::RegularFile,
            };
            dirs.create_entry(InodeId::ROOT_INODE, &entry).unwrap();
        }

        let entries = dirs.list_entries(InodeId::ROOT_INODE).unwrap();
        assert_eq!(entries.len(), 3);
        // BTreeMap preserves order, so entries should be sorted
        assert_eq!(entries[0].name, "file0.txt");
        assert_eq!(entries[1].name, "file1.txt");
        assert_eq!(entries[2].name, "file2.txt");
    }

    #[test]
    fn test_is_empty() {
        let (_kv, inodes, dirs) = make_stores();

        assert!(dirs.is_empty(InodeId::ROOT_INODE).unwrap());

        let ino = inodes.allocate_inode();
        let attr = InodeAttr::new_file(ino, 1000, 1000, 0o644, 1);
        inodes.create_inode(&attr).unwrap();

        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "file.txt".to_string(),
                ino,
                file_type: FileType::RegularFile,
            },
        )
        .unwrap();

        assert!(!dirs.is_empty(InodeId::ROOT_INODE).unwrap());
    }

    #[test]
    fn test_rename_same_directory() {
        let (_kv, inodes, dirs) = make_stores();

        let ino = inodes.allocate_inode();
        let attr = InodeAttr::new_file(ino, 1000, 1000, 0o644, 1);
        inodes.create_inode(&attr).unwrap();

        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "old.txt".to_string(),
                ino,
                file_type: FileType::RegularFile,
            },
        )
        .unwrap();

        dirs.rename(
            InodeId::ROOT_INODE,
            "old.txt",
            InodeId::ROOT_INODE,
            "new.txt",
        )
        .unwrap();

        match dirs.lookup(InodeId::ROOT_INODE, "old.txt") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }

        let found = dirs.lookup(InodeId::ROOT_INODE, "new.txt").unwrap();
        assert_eq!(found.ino, ino);
    }

    #[test]
    fn test_rename_cross_directory() {
        let (_kv, inodes, dirs) = make_stores();

        // Create a subdirectory
        let subdir_ino = inodes.allocate_inode();
        let subdir_attr = InodeAttr::new_directory(subdir_ino, 1000, 1000, 0o755, 1);
        inodes.create_inode(&subdir_attr).unwrap();

        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "subdir".to_string(),
                ino: subdir_ino,
                file_type: FileType::Directory,
            },
        )
        .unwrap();

        // Create a file in root
        let file_ino = inodes.allocate_inode();
        let file_attr = InodeAttr::new_file(file_ino, 1000, 1000, 0o644, 1);
        inodes.create_inode(&file_attr).unwrap();

        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "file.txt".to_string(),
                ino: file_ino,
                file_type: FileType::RegularFile,
            },
        )
        .unwrap();

        // Move file from root to subdir
        dirs.rename(InodeId::ROOT_INODE, "file.txt", subdir_ino, "moved.txt")
            .unwrap();

        match dirs.lookup(InodeId::ROOT_INODE, "file.txt") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }

        let found = dirs.lookup(subdir_ino, "moved.txt").unwrap();
        assert_eq!(found.ino, file_ino);
    }

    #[test]
    fn test_not_a_directory() {
        let (_kv, inodes, dirs) = make_stores();

        // Create a file inode
        let file_ino = inodes.allocate_inode();
        let file_attr = InodeAttr::new_file(file_ino, 1000, 1000, 0o644, 1);
        inodes.create_inode(&file_attr).unwrap();

        // Try to create an entry in a non-directory
        let entry = DirEntry {
            name: "child.txt".to_string(),
            ino: InodeId::new(999),
            file_type: FileType::RegularFile,
        };
        match dirs.create_entry(file_ino, &entry) {
            Err(MetaError::NotADirectory(_)) => {}
            other => panic!("expected NotADirectory, got {:?}", other),
        }
    }

    #[test]
    fn test_rename_nonexistent_source() {
        let (_kv, _inodes, dirs) = make_stores();
        match dirs.rename(InodeId::ROOT_INODE, "nonexistent", InodeId::ROOT_INODE, "target") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_rename_overwrites_existing() {
        let (_kv, inodes, dirs) = make_stores();
        let ino_a = inodes.allocate_inode();
        inodes
            .create_inode(&InodeAttr::new_file(ino_a, 1000, 1000, 0o644, 1))
            .unwrap();
        let ino_b = inodes.allocate_inode();
        inodes
            .create_inode(&InodeAttr::new_file(ino_b, 1000, 1000, 0o644, 1))
            .unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry { name: "a".into(), ino: ino_a, file_type: FileType::RegularFile },
        )
        .unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry { name: "b".into(), ino: ino_b, file_type: FileType::RegularFile },
        )
        .unwrap();
        dirs.rename(InodeId::ROOT_INODE, "a", InodeId::ROOT_INODE, "b")
            .unwrap();
        assert!(matches!(
            dirs.lookup(InodeId::ROOT_INODE, "a"),
            Err(MetaError::EntryNotFound { .. })
        ));
        assert_eq!(dirs.lookup(InodeId::ROOT_INODE, "b").unwrap().ino, ino_a);
    }

    #[test]
    fn test_delete_nonexistent_entry() {
        let (_kv, _inodes, dirs) = make_stores();
        assert!(matches!(
            dirs.delete_entry(InodeId::ROOT_INODE, "nonexistent"),
            Err(MetaError::EntryNotFound { .. })
        ));
    }

    #[test]
    fn test_lookup_nonexistent_entry() {
        let (_kv, _inodes, dirs) = make_stores();
        assert!(matches!(
            dirs.lookup(InodeId::ROOT_INODE, "nonexistent"),
            Err(MetaError::EntryNotFound { .. })
        ));
    }

    #[test]
    fn test_list_empty_directory() {
        let (_kv, _inodes, dirs) = make_stores();
        assert!(dirs.list_entries(InodeId::ROOT_INODE).unwrap().is_empty());
    }

    #[test]
    fn test_create_entry_in_nonexistent_parent() {
        let (_kv, inodes, dirs) = make_stores();
        let ino = inodes.allocate_inode();
        inodes
            .create_inode(&InodeAttr::new_file(ino, 1000, 1000, 0o644, 1))
            .unwrap();
        let entry = DirEntry { name: "f.txt".into(), ino, file_type: FileType::RegularFile };
        assert!(matches!(
            dirs.create_entry(InodeId::new(999), &entry),
            Err(MetaError::InodeNotFound(_))
        ));
    }

    #[test]
    fn test_multiple_directories() {
        let (_kv, inodes, dirs) = make_stores();
        let d1 = inodes.allocate_inode();
        inodes
            .create_inode(&InodeAttr::new_directory(d1, 1000, 1000, 0o755, 1))
            .unwrap();
        let d2 = inodes.allocate_inode();
        inodes
            .create_inode(&InodeAttr::new_directory(d2, 1000, 1000, 0o755, 1))
            .unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry { name: "d1".into(), ino: d1, file_type: FileType::Directory },
        )
        .unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry { name: "d2".into(), ino: d2, file_type: FileType::Directory },
        )
        .unwrap();
        let f1 = inodes.allocate_inode();
        inodes
            .create_inode(&InodeAttr::new_file(f1, 1000, 1000, 0o644, 1))
            .unwrap();
        let f2 = inodes.allocate_inode();
        inodes
            .create_inode(&InodeAttr::new_file(f2, 1000, 1000, 0o644, 1))
            .unwrap();
        dirs.create_entry(
            d1,
            &DirEntry { name: "f1.txt".into(), ino: f1, file_type: FileType::RegularFile },
        )
        .unwrap();
        dirs.create_entry(
            d2,
            &DirEntry { name: "f2.txt".into(), ino: f2, file_type: FileType::RegularFile },
        )
        .unwrap();
        let e1 = dirs.list_entries(d1).unwrap();
        assert_eq!(e1.len(), 1);
        assert_eq!(e1[0].name, "f1.txt");
        let e2 = dirs.list_entries(d2).unwrap();
        assert_eq!(e2.len(), 1);
        assert_eq!(e2[0].name, "f2.txt");
    }
}
