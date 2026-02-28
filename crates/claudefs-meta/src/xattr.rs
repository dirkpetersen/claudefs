//! Extended attribute (xattr) operations for the metadata service.
//!
//! Xattrs are stored in the KV store with key format `xattr/{inode_be}/{key}`.
//! Used for ACLs, S3 tiering hints (`claudefs.tier`), replication tags, etc.

use std::sync::Arc;

use crate::kvstore::KvStore;
use crate::types::{InodeId, MetaError};

const XATTR_PREFIX: &[u8] = b"xattr/";

fn xattr_prefix(ino: InodeId) -> Vec<u8> {
    let mut key = XATTR_PREFIX.to_vec();
    key.extend_from_slice(&ino.as_u64().to_be_bytes());
    key.push(b'/');
    key
}

fn xattr_key(ino: InodeId, name: &str) -> Vec<u8> {
    let mut key = xattr_prefix(ino);
    key.extend_from_slice(name.as_bytes());
    key
}

/// Extended attribute store for per-inode xattrs.
pub struct XattrStore {
    kv: Arc<dyn KvStore>,
}

impl XattrStore {
    /// Creates a new XattrStore.
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Sets an extended attribute on an inode.
    pub fn set(&self, ino: InodeId, name: &str, value: &[u8]) -> Result<(), MetaError> {
        let key = xattr_key(ino, name);
        self.kv.put(key, value.to_vec())
    }

    /// Gets an extended attribute value.
    pub fn get(&self, ino: InodeId, name: &str) -> Result<Vec<u8>, MetaError> {
        let key = xattr_key(ino, name);
        self.kv.get(&key)?.ok_or_else(|| MetaError::EntryNotFound {
            parent: ino,
            name: format!("xattr:{}", name),
        })
    }

    /// Lists all xattr names on an inode.
    pub fn list(&self, ino: InodeId) -> Result<Vec<String>, MetaError> {
        let prefix = xattr_prefix(ino);
        let entries = self.kv.scan_prefix(&prefix)?;
        let prefix_len = prefix.len();

        entries
            .into_iter()
            .map(|(key, _)| {
                String::from_utf8(key[prefix_len..].to_vec())
                    .map_err(|e| MetaError::KvError(e.to_string()))
            })
            .collect()
    }

    /// Removes an extended attribute.
    pub fn remove(&self, ino: InodeId, name: &str) -> Result<(), MetaError> {
        let key = xattr_key(ino, name);
        if !self.kv.contains_key(&key)? {
            return Err(MetaError::EntryNotFound {
                parent: ino,
                name: format!("xattr:{}", name),
            });
        }
        self.kv.delete(&key)
    }

    /// Removes all extended attributes on an inode.
    pub fn remove_all(&self, ino: InodeId) -> Result<(), MetaError> {
        let prefix = xattr_prefix(ino);
        let entries = self.kv.scan_prefix(&prefix)?;
        for (key, _) in entries {
            self.kv.delete(&key)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use crate::types::InodeId;

    fn make_store() -> XattrStore {
        XattrStore::new(Arc::new(MemoryKvStore::new()))
    }

    #[test]
    fn test_set_and_get_xattr() {
        let store = make_store();
        let ino = InodeId::new(42);
        store.set(ino, "user.author", b"alice").unwrap();
        assert_eq!(store.get(ino, "user.author").unwrap(), b"alice");
    }

    #[test]
    fn test_get_nonexistent_xattr() {
        let store = make_store();
        match store.get(InodeId::new(42), "user.missing") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_list_xattrs() {
        let store = make_store();
        let ino = InodeId::new(42);
        store.set(ino, "user.author", b"alice").unwrap();
        store.set(ino, "user.project", b"claudefs").unwrap();
        store.set(ino, "claudefs.tier", b"flash").unwrap();

        let names = store.list(ino).unwrap();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"user.author".to_string()));
        assert!(names.contains(&"user.project".to_string()));
        assert!(names.contains(&"claudefs.tier".to_string()));
    }

    #[test]
    fn test_remove_xattr() {
        let store = make_store();
        let ino = InodeId::new(42);
        store.set(ino, "user.author", b"alice").unwrap();
        store.remove(ino, "user.author").unwrap();

        match store.get(ino, "user.author") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_remove_nonexistent_xattr() {
        let store = make_store();
        match store.remove(InodeId::new(42), "user.missing") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_remove_all_xattrs() {
        let store = make_store();
        let ino = InodeId::new(42);
        store.set(ino, "user.author", b"alice").unwrap();
        store.set(ino, "user.project", b"claudefs").unwrap();
        store.remove_all(ino).unwrap();

        let names = store.list(ino).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn test_xattrs_isolated_per_inode() {
        let store = make_store();
        store.set(InodeId::new(1), "user.a", b"1").unwrap();
        store.set(InodeId::new(2), "user.a", b"2").unwrap();

        assert_eq!(store.get(InodeId::new(1), "user.a").unwrap(), b"1");
        assert_eq!(store.get(InodeId::new(2), "user.a").unwrap(), b"2");
    }

    #[test]
    fn test_overwrite_xattr() {
        let store = make_store();
        let ino = InodeId::new(42);
        store.set(ino, "user.v", b"v1").unwrap();
        store.set(ino, "user.v", b"v2").unwrap();
        assert_eq!(store.get(ino, "user.v").unwrap(), b"v2");
    }
}
