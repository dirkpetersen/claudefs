//! Inode operations for the metadata service.
//!
//! Inodes are stored in the embedded KV store with key format `inode/{inode_id_be}`.
//! Serialization uses bincode for compact binary encoding.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::kvstore::KvStore;
use crate::types::{InodeAttr, InodeId, MetaError};

/// Key prefix for inode entries in the KV store.
const INODE_PREFIX: &[u8] = b"inode/";

/// Builds the KV store key for an inode.
fn inode_key(ino: InodeId) -> Vec<u8> {
    let mut key = INODE_PREFIX.to_vec();
    key.extend_from_slice(&ino.as_u64().to_be_bytes());
    key
}

/// Manages inode allocation and CRUD operations on top of the KV store.
pub struct InodeStore {
    kv: Arc<dyn KvStore>,
    next_inode: AtomicU64,
}

impl InodeStore {
    /// Creates a new InodeStore with the given KV store backend.
    /// The next_inode counter starts at 2 (1 is reserved for the root inode).
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self {
            kv,
            next_inode: AtomicU64::new(2),
        }
    }

    /// Allocates a new unique inode ID.
    pub fn allocate_inode(&self) -> InodeId {
        let id = self.next_inode.fetch_add(1, Ordering::Relaxed);
        InodeId::new(id)
    }

    /// Stores an inode's attributes in the KV store.
    pub fn create_inode(&self, attr: &InodeAttr) -> Result<(), MetaError> {
        let key = inode_key(attr.ino);
        let value = bincode::serialize(attr).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key, value)
    }

    /// Retrieves an inode's attributes from the KV store.
    pub fn get_inode(&self, ino: InodeId) -> Result<InodeAttr, MetaError> {
        let key = inode_key(ino);
        match self.kv.get(&key)? {
            Some(value) => {
                bincode::deserialize(&value).map_err(|e| MetaError::KvError(e.to_string()))
            }
            None => Err(MetaError::InodeNotFound(ino)),
        }
    }

    /// Updates an inode's attributes in the KV store.
    pub fn set_inode(&self, attr: &InodeAttr) -> Result<(), MetaError> {
        let key = inode_key(attr.ino);
        // Verify the inode exists
        if !self.kv.contains_key(&key)? {
            return Err(MetaError::InodeNotFound(attr.ino));
        }
        let value = bincode::serialize(attr).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key, value)
    }

    /// Deletes an inode from the KV store.
    pub fn delete_inode(&self, ino: InodeId) -> Result<(), MetaError> {
        let key = inode_key(ino);
        if !self.kv.contains_key(&key)? {
            return Err(MetaError::InodeNotFound(ino));
        }
        self.kv.delete(&key)
    }

    /// Checks if an inode exists.
    pub fn exists(&self, ino: InodeId) -> Result<bool, MetaError> {
        self.kv.contains_key(&inode_key(ino))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use crate::types::{FileType, InodeAttr, InodeId};

    fn make_store() -> InodeStore {
        InodeStore::new(Arc::new(MemoryKvStore::new()))
    }

    #[test]
    fn test_allocate_inode() {
        let store = make_store();
        let id1 = store.allocate_inode();
        let id2 = store.allocate_inode();
        assert_eq!(id1.as_u64(), 2);
        assert_eq!(id2.as_u64(), 3);
    }

    #[test]
    fn test_create_and_get_inode() {
        let store = make_store();
        let attr = InodeAttr::new_file(InodeId::new(2), 1000, 1000, 0o644, 1);
        store.create_inode(&attr).unwrap();

        let retrieved = store.get_inode(InodeId::new(2)).unwrap();
        assert_eq!(retrieved.ino, attr.ino);
        assert_eq!(retrieved.file_type, FileType::RegularFile);
        assert_eq!(retrieved.uid, 1000);
        assert_eq!(retrieved.mode, 0o644);
    }

    #[test]
    fn test_get_nonexistent_inode() {
        let store = make_store();
        match store.get_inode(InodeId::new(999)) {
            Err(MetaError::InodeNotFound(id)) => assert_eq!(id.as_u64(), 999),
            other => panic!("expected InodeNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_set_inode() {
        let store = make_store();
        let mut attr = InodeAttr::new_file(InodeId::new(2), 1000, 1000, 0o644, 1);
        store.create_inode(&attr).unwrap();

        attr.size = 4096;
        attr.mode = 0o755;
        store.set_inode(&attr).unwrap();

        let retrieved = store.get_inode(InodeId::new(2)).unwrap();
        assert_eq!(retrieved.size, 4096);
        assert_eq!(retrieved.mode, 0o755);
    }

    #[test]
    fn test_delete_inode() {
        let store = make_store();
        let attr = InodeAttr::new_file(InodeId::new(2), 1000, 1000, 0o644, 1);
        store.create_inode(&attr).unwrap();

        store.delete_inode(InodeId::new(2)).unwrap();
        assert!(!store.exists(InodeId::new(2)).unwrap());
    }

    #[test]
    fn test_delete_nonexistent_inode() {
        let store = make_store();
        match store.delete_inode(InodeId::new(999)) {
            Err(MetaError::InodeNotFound(_)) => {}
            other => panic!("expected InodeNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_directory_inode() {
        let store = make_store();
        let attr = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        store.create_inode(&attr).unwrap();

        let retrieved = store.get_inode(InodeId::ROOT_INODE).unwrap();
        assert_eq!(retrieved.file_type, FileType::Directory);
        assert_eq!(retrieved.nlink, 2);
    }
}
