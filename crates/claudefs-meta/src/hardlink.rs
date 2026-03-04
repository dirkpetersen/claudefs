//! Hard link tracking for POSIX hard link support.
//!
//! This module maintains an explicit map of (parent_ino, name) → target_ino for all hard links.
//! It provides both forward and reverse indexes for efficient querying.

use std::sync::Arc;

use crate::kvstore::KvStore;
use crate::types::{InodeId, MetaError};

/// Hard link tracking store.
///
/// Maintains an explicit map of (parent_ino, name) → target_ino for all hard links.
/// A hard link is a directory entry that is NOT the original file creation entry.
///
/// When an inode's nlink drops to 0, it can be garbage collected by calling
/// the GcManager. This store tracks all non-primary links for a given target inode.
pub struct HardLinkStore {
    kv: Arc<dyn KvStore>,
}

impl HardLinkStore {
    /// Creates a new HardLinkStore backed by the given KV store.
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    fn make_fwd_key(parent_ino: InodeId, name: &str) -> Vec<u8> {
        let mut key = Vec::with_capacity(8 + name.len() + 8);
        key.extend_from_slice(b"hl:fwd:");
        key.extend_from_slice(&parent_ino.as_u64().to_be_bytes());
        key.push(b':');
        key.extend_from_slice(name.as_bytes());
        key
    }

    fn make_rev_key(target_ino: InodeId, parent_ino: InodeId, name: &str) -> Vec<u8> {
        let mut key = Vec::with_capacity(8 + 8 + name.len() + 16);
        key.extend_from_slice(b"hl:rev:");
        key.extend_from_slice(&target_ino.as_u64().to_be_bytes());
        key.push(b':');
        key.extend_from_slice(&parent_ino.as_u64().to_be_bytes());
        key.push(b':');
        key.extend_from_slice(name.as_bytes());
        key
    }

    fn rev_prefix(target_ino: InodeId) -> Vec<u8> {
        let mut prefix = Vec::with_capacity(9);
        prefix.extend_from_slice(b"hl:rev:");
        prefix.extend_from_slice(&target_ino.as_u64().to_be_bytes());
        prefix.push(b':');
        prefix
    }

    fn fwd_prefix(parent_ino: InodeId) -> Vec<u8> {
        let mut prefix = Vec::with_capacity(10);
        prefix.extend_from_slice(b"hl:fwd:");
        prefix.extend_from_slice(&parent_ino.as_u64().to_be_bytes());
        prefix.push(b':');
        prefix
    }

    /// Records a new hard link: (parent_ino, name) → target_ino.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::AlreadyExists` if a link from (parent, name) already exists.
    pub fn add_link(
        &self,
        parent_ino: InodeId,
        name: &str,
        target_ino: InodeId,
    ) -> Result<(), MetaError> {
        let fwd_key = Self::make_fwd_key(parent_ino, name);

        if self.kv.contains_key(&fwd_key)? {
            return Err(MetaError::EntryExists {
                parent: parent_ino,
                name: name.to_string(),
            });
        }

        let rev_key = Self::make_rev_key(target_ino, parent_ino, name);

        self.kv.write_batch(vec![
            crate::kvstore::BatchOp::Put {
                key: fwd_key,
                value: target_ino.as_u64().to_be_bytes().to_vec(),
            },
            crate::kvstore::BatchOp::Put {
                key: rev_key,
                value: vec![],
            },
        ])
    }

    /// Removes a hard link by (parent_ino, name).
    ///
    /// Returns `Ok(())` if the link was removed, or if it didn't exist (idempotent).
    pub fn remove_link(&self, parent_ino: InodeId, name: &str) -> Result<(), MetaError> {
        let fwd_key = Self::make_fwd_key(parent_ino, name);

        let Some(value) = self.kv.get(&fwd_key)? else {
            return Ok(());
        };

        let target_ino =
            InodeId::new(u64::from_be_bytes(value.try_into().map_err(|_| {
                MetaError::KvError("invalid target inode".to_string())
            })?));

        let rev_key = Self::make_rev_key(target_ino, parent_ino, name);

        self.kv.write_batch(vec![
            crate::kvstore::BatchOp::Delete { key: fwd_key },
            crate::kvstore::BatchOp::Delete { key: rev_key },
        ])
    }

    /// Returns the target inode ID for a given (parent_ino, name) link.
    ///
    /// Returns `None` if the link doesn't exist.
    pub fn get_target(
        &self,
        parent_ino: InodeId,
        name: &str,
    ) -> Result<Option<InodeId>, MetaError> {
        let fwd_key = Self::make_fwd_key(parent_ino, name);

        match self.kv.get(&fwd_key)? {
            Some(value) => {
                let ino =
                    InodeId::new(u64::from_be_bytes(value.try_into().map_err(|_| {
                        MetaError::KvError("invalid target inode".to_string())
                    })?));
                Ok(Some(ino))
            }
            None => Ok(None),
        }
    }

    /// Lists all hard links pointing to a given target inode.
    ///
    /// Returns a list of (parent_ino, name) tuples.
    /// This is used to implement `stat` (nlink count) and for fsck verification.
    pub fn list_links_to(&self, target_ino: InodeId) -> Result<Vec<(InodeId, String)>, MetaError> {
        let prefix = Self::rev_prefix(target_ino);

        let entries = self.kv.scan_prefix(&prefix)?;

        let mut result = Vec::with_capacity(entries.len());

        for (key, _) in entries {
            let key_data = &key[prefix.len()..];
            let sep_pos = key_data.iter().position(|&b| b == b':').unwrap();
            let parent_bytes = &key_data[..sep_pos];
            let name = String::from_utf8_lossy(&key_data[sep_pos + 1..]).to_string();

            let parent_ino =
                InodeId::new(u64::from_be_bytes(parent_bytes.try_into().map_err(
                    |_| MetaError::KvError("invalid parent inode".to_string()),
                )?));

            result.push((parent_ino, name));
        }

        Ok(result)
    }

    /// Lists all hard links that originate from a given parent directory.
    ///
    /// Returns a list of (name, target_ino) tuples.
    /// This is used to enumerate directory contents efficiently.
    pub fn list_links_from(
        &self,
        parent_ino: InodeId,
    ) -> Result<Vec<(String, InodeId)>, MetaError> {
        let prefix = Self::fwd_prefix(parent_ino);

        let entries = self.kv.scan_prefix(&prefix)?;

        let mut result = Vec::with_capacity(entries.len());

        for (key, value) in entries {
            let name_start = 9 + 8 + 1;
            let name = String::from_utf8_lossy(&key[name_start..]).to_string();

            let target_ino =
                InodeId::new(u64::from_be_bytes(value.try_into().map_err(|_| {
                    MetaError::KvError("invalid target inode".to_string())
                })?));

            result.push((name, target_ino));
        }

        Ok(result)
    }

    /// Returns the number of hard links to a given target inode.
    ///
    /// This is the nlink count for the inode (excluding the implicit count from InodeAttr.nlink).
    pub fn link_count(&self, target_ino: InodeId) -> Result<u32, MetaError> {
        let links = self.list_links_to(target_ino)?;
        Ok(links.len() as u32)
    }

    /// Checks if an inode has any hard links remaining.
    ///
    /// Returns `true` if there are one or more hard links from any (parent, name) to this inode.
    pub fn has_links(&self, target_ino: InodeId) -> Result<bool, MetaError> {
        let links = self.list_links_to(target_ino)?;
        Ok(!links.is_empty())
    }

    /// Atomically moves a link: removes (old_parent, old_name) and adds (new_parent, new_name).
    ///
    /// This is used for the rename() syscall when moving hard links.
    /// Returns `MetaError::NotFound` if the source link doesn't exist.
    pub fn rename_link(
        &self,
        old_parent: InodeId,
        old_name: &str,
        new_parent: InodeId,
        new_name: &str,
    ) -> Result<(), MetaError> {
        let old_fwd_key = Self::make_fwd_key(old_parent, old_name);

        let Some(value) = self.kv.get(&old_fwd_key)? else {
            return Err(MetaError::EntryNotFound {
                parent: old_parent,
                name: old_name.to_string(),
            });
        };

        let target_ino =
            InodeId::new(u64::from_be_bytes(value.try_into().map_err(|_| {
                MetaError::KvError("invalid target inode".to_string())
            })?));

        let old_rev_key = Self::make_rev_key(target_ino, old_parent, old_name);
        let new_fwd_key = Self::make_fwd_key(new_parent, new_name);
        let new_rev_key = Self::make_rev_key(target_ino, new_parent, new_name);

        self.kv.write_batch(vec![
            crate::kvstore::BatchOp::Delete { key: old_fwd_key },
            crate::kvstore::BatchOp::Delete { key: old_rev_key },
            crate::kvstore::BatchOp::Put {
                key: new_fwd_key,
                value: target_ino.as_u64().to_be_bytes().to_vec(),
            },
            crate::kvstore::BatchOp::Put {
                key: new_rev_key,
                value: vec![],
            },
        ])
    }

    /// Returns total number of hard links tracked in this store.
    pub fn total_link_count(&self) -> Result<usize, MetaError> {
        let entries = self.kv.scan_prefix(b"hl:fwd:")?;
        Ok(entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn make_store() -> HardLinkStore {
        HardLinkStore::new(Arc::new(MemoryKvStore::new()))
    }

    #[test]
    fn test_add_and_get_link() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();

        let target = store.get_target(InodeId::new(1), "link1").unwrap();
        assert_eq!(target, Some(InodeId::new(100)));
    }

    #[test]
    fn test_add_link_already_exists() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();

        let result = store.add_link(InodeId::new(1), "link1", InodeId::new(100));
        assert!(result.is_err());
        match result {
            Err(MetaError::EntryExists { .. }) => {}
            _ => panic!("Expected EntryExists error"),
        }
    }

    #[test]
    fn test_remove_link() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();
        store.remove_link(InodeId::new(1), "link1").unwrap();

        let target = store.get_target(InodeId::new(1), "link1").unwrap();
        assert_eq!(target, None);
    }

    #[test]
    fn test_remove_nonexistent_link() {
        let store = make_store();
        let result = store.remove_link(InodeId::new(1), "nonexistent");
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_links_to_single() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();
        store
            .add_link(InodeId::new(2), "link2", InodeId::new(100))
            .unwrap();
        store
            .add_link(InodeId::new(3), "link3", InodeId::new(100))
            .unwrap();

        let links = store.list_links_to(InodeId::new(100)).unwrap();
        assert_eq!(links.len(), 3);
    }

    #[test]
    fn test_list_links_to_empty() {
        let store = make_store();
        let links = store.list_links_to(InodeId::new(100)).unwrap();
        assert!(links.is_empty());
    }

    #[test]
    fn test_list_links_from_single() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();
        store
            .add_link(InodeId::new(1), "link2", InodeId::new(200))
            .unwrap();
        store
            .add_link(InodeId::new(1), "link3", InodeId::new(300))
            .unwrap();

        let links = store.list_links_from(InodeId::new(1)).unwrap();
        assert_eq!(links.len(), 3);
    }

    #[test]
    fn test_link_count() {
        let store = make_store();

        assert_eq!(store.link_count(InodeId::new(100)).unwrap(), 0);

        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();
        assert_eq!(store.link_count(InodeId::new(100)).unwrap(), 1);

        store
            .add_link(InodeId::new(2), "link2", InodeId::new(100))
            .unwrap();
        assert_eq!(store.link_count(InodeId::new(100)).unwrap(), 2);
    }

    #[test]
    fn test_has_links() {
        let store = make_store();

        assert!(!store.has_links(InodeId::new(100)).unwrap());

        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();
        assert!(store.has_links(InodeId::new(100)).unwrap());
    }

    #[test]
    fn test_rename_link_success() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "oldname", InodeId::new(100))
            .unwrap();

        store
            .rename_link(InodeId::new(1), "oldname", InodeId::new(2), "newname")
            .unwrap();

        let old_target = store.get_target(InodeId::new(1), "oldname").unwrap();
        assert_eq!(old_target, None);

        let new_target = store.get_target(InodeId::new(2), "newname").unwrap();
        assert_eq!(new_target, Some(InodeId::new(100)));
    }

    #[test]
    fn test_rename_link_not_found() {
        let store = make_store();
        let result = store.rename_link(InodeId::new(1), "nonexistent", InodeId::new(2), "newname");
        assert!(result.is_err());
        match result {
            Err(MetaError::EntryNotFound { .. }) => {}
            _ => panic!("Expected EntryNotFound error"),
        }
    }

    #[test]
    fn test_total_link_count() {
        let store = make_store();

        assert_eq!(store.total_link_count().unwrap(), 0);

        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();
        store
            .add_link(InodeId::new(2), "link2", InodeId::new(200))
            .unwrap();

        assert_eq!(store.total_link_count().unwrap(), 2);
    }

    #[test]
    fn test_multiple_targets_from_same_parent() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "file1", InodeId::new(100))
            .unwrap();
        store
            .add_link(InodeId::new(1), "file2", InodeId::new(200))
            .unwrap();
        store
            .add_link(InodeId::new(1), "file3", InodeId::new(300))
            .unwrap();

        let links = store.list_links_from(InodeId::new(1)).unwrap();
        assert_eq!(links.len(), 3);

        assert_eq!(
            store.get_target(InodeId::new(1), "file1").unwrap(),
            Some(InodeId::new(100))
        );
        assert_eq!(
            store.get_target(InodeId::new(1), "file2").unwrap(),
            Some(InodeId::new(200))
        );
        assert_eq!(
            store.get_target(InodeId::new(1), "file3").unwrap(),
            Some(InodeId::new(300))
        );
    }

    #[test]
    fn test_links_independent_across_parents() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "same", InodeId::new(100))
            .unwrap();
        store
            .add_link(InodeId::new(2), "same", InodeId::new(200))
            .unwrap();

        assert_eq!(
            store.get_target(InodeId::new(1), "same").unwrap(),
            Some(InodeId::new(100))
        );
        assert_eq!(
            store.get_target(InodeId::new(2), "same").unwrap(),
            Some(InodeId::new(200))
        );

        store.remove_link(InodeId::new(1), "same").unwrap();

        assert_eq!(store.get_target(InodeId::new(1), "same").unwrap(), None);
        assert_eq!(
            store.get_target(InodeId::new(2), "same").unwrap(),
            Some(InodeId::new(200))
        );
    }

    #[test]
    fn test_remove_one_of_many_links() {
        let store = make_store();
        store
            .add_link(InodeId::new(1), "link1", InodeId::new(100))
            .unwrap();
        store
            .add_link(InodeId::new(2), "link2", InodeId::new(100))
            .unwrap();
        store
            .add_link(InodeId::new(3), "link3", InodeId::new(100))
            .unwrap();

        store.remove_link(InodeId::new(2), "link2").unwrap();

        assert_eq!(store.link_count(InodeId::new(100)).unwrap(), 2);
        assert!(store.has_links(InodeId::new(100)).unwrap());
    }
}
