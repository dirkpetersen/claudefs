//! Symlink operations for the metadata service.
//!
//! This module implements symlink creation, resolution, and readlink operations
//! with loop detection to prevent infinite symlink chains.

use std::sync::{Arc, Mutex};

use crate::kvstore::{KvStore, MemoryKvStore};
use crate::types::{InodeId, MetaError};

const SYMLINK_PREFIX: &[u8] = b"symlink:";
const MAX_SYMLINK_TARGET_LEN: usize = 4096;

/// Storage backend for symlink targets.
pub struct SymlinkStore {
    kv: Arc<Mutex<MemoryKvStore>>,
}

impl SymlinkStore {
    /// Creates a new symlink store backed by the given KV store.
    pub fn new(kv: Arc<Mutex<MemoryKvStore>>) -> Self {
        Self { kv }
    }

    fn symlink_key(ino: InodeId) -> Vec<u8> {
        let mut key = SYMLINK_PREFIX.to_vec();
        key.extend_from_slice(&ino.as_u64().to_be_bytes());
        key
    }

    /// Stores the target of a symlink inode.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::InvalidArgument` if the target is invalid.
    pub fn create(&self, ino: InodeId, target: &str) -> Result<(), MetaError> {
        Self::validate_target(target)?;
        let key = Self::symlink_key(ino);
        let value = bincode::serialize(target)
            .map_err(|e| MetaError::KvError(format!("serialize: {}", e)))?;
        self.kv
            .lock()
            .map_err(|e| MetaError::KvError(e.to_string()))?
            .put(key, value)
    }

    /// Reads the target of a symlink inode.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::InodeNotFound` if no symlink target exists for the inode.
    pub fn readlink(&self, ino: InodeId) -> Result<String, MetaError> {
        let key = Self::symlink_key(ino);
        let kv = self
            .kv
            .lock()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        match kv.get(&key)? {
            Some(value) => {
                let target: String = bincode::deserialize(&value)
                    .map_err(|e| MetaError::KvError(format!("deserialize: {}", e)))?;
                Ok(target)
            }
            None => Err(MetaError::InodeNotFound(ino)),
        }
    }

    /// Deletes a symlink target.
    pub fn delete(&self, ino: InodeId) -> Result<(), MetaError> {
        let key = Self::symlink_key(ino);
        self.kv
            .lock()
            .map_err(|e| MetaError::KvError(e.to_string()))?
            .delete(&key)
    }

    /// Resolves a symlink chain up to `max_depth` hops.
    ///
    /// Uses a lookup callback to resolve path components. The callback receives
    /// the parent inode and a path component name, returning the inode ID and
    /// a boolean indicating if it's a symlink.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::LoopDetected` if the chain exceeds `max_depth`.
    pub fn resolve<F>(
        &self,
        start_ino: InodeId,
        max_depth: usize,
        lookup_fn: F,
    ) -> Result<InodeId, MetaError>
    where
        F: Fn(InodeId, &str) -> Option<(InodeId, bool)>,
    {
        let mut current_ino = start_ino;
        let mut depth = 0;

        loop {
            let target = match self.readlink(current_ino) {
                Ok(t) => t,
                Err(MetaError::InodeNotFound(_)) => return Ok(current_ino),
                Err(e) => return Err(e),
            };

            if depth >= max_depth {
                return Err(MetaError::KvError("symlink loop detected".into()));
            }
            depth += 1;

            let components: Vec<&str> = target
                .trim_start_matches('/')
                .split('/')
                .filter(|s| !s.is_empty() && *s != ".")
                .collect();

            current_ino = InodeId::ROOT_INODE;

            for component in components {
                if component == ".." {
                    continue;
                }

                match lookup_fn(current_ino, component) {
                    Some((ino, is_symlink)) => {
                        if is_symlink {
                            current_ino = ino;
                            let nested_target = self.readlink(current_ino)?;
                            depth += 1;
                            if depth > max_depth {
                                return Err(MetaError::KvError("symlink loop detected".into()));
                            }
                            if let Some((resolved_ino, _)) = lookup_fn(
                                current_ino,
                                nested_target
                                    .trim_start_matches('/')
                                    .split('/')
                                    .next()
                                    .unwrap_or(""),
                            ) {
                                current_ino = resolved_ino;
                            }
                        } else {
                            current_ino = ino;
                        }
                    }
                    None => {
                        return Err(MetaError::InodeNotFound(InodeId::new(0)));
                    }
                }
            }
        }
    }

    /// Validates a symlink target path.
    ///
    /// # Errors
    ///
    /// Returns `MetaError::InvalidArgument` if:
    /// - The target is empty
    /// - The target contains null bytes
    /// - The target exceeds 4096 bytes
    pub fn validate_target(target: &str) -> Result<(), MetaError> {
        if target.is_empty() {
            return Err(MetaError::InvalidArgument("symlink target is empty".into()));
        }
        if target.contains('\0') {
            return Err(MetaError::InvalidArgument(
                "symlink target contains null byte".into(),
            ));
        }
        if target.len() > MAX_SYMLINK_TARGET_LEN {
            return Err(MetaError::InvalidArgument(format!(
                "symlink target exceeds {} bytes",
                MAX_SYMLINK_TARGET_LEN
            )));
        }
        Ok(())
    }

    /// Lists all symlinks (ino, target pairs) for debugging/fsck.
    pub fn list_all(&self) -> Vec<(InodeId, String)> {
        let kv = match self.kv.lock() {
            Ok(guard) => guard,
            Err(_) => return vec![],
        };
        match kv.scan_prefix(SYMLINK_PREFIX) {
            Ok(pairs) => pairs
                .into_iter()
                .filter_map(|(key, value): (Vec<u8>, Vec<u8>)| {
                    if key.len() != SYMLINK_PREFIX.len() + 8 {
                        return None;
                    }
                    let ino_bytes: [u8; 8] = key[SYMLINK_PREFIX.len()..].try_into().ok()?;
                    let ino = InodeId::new(u64::from_be_bytes(ino_bytes));
                    let target: String = bincode::deserialize(&value).ok()?;
                    Some((ino, target))
                })
                .collect(),
            Err(_) => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_store() -> SymlinkStore {
        SymlinkStore::new(Arc::new(Mutex::new(MemoryKvStore::new())))
    }

    #[test]
    fn test_create_and_readlink() {
        let store = make_test_store();
        let ino = InodeId::new(42);
        let target = "/path/to/target";

        store.create(ino, target).unwrap();
        let retrieved = store.readlink(ino).unwrap();
        assert_eq!(retrieved, target);
    }

    #[test]
    fn test_readlink_not_found() {
        let store = make_test_store();
        let ino = InodeId::new(42);
        let result = store.readlink(ino);
        assert!(matches!(result, Err(MetaError::InodeNotFound(_))));
    }

    #[test]
    fn test_delete_symlink() {
        let store = make_test_store();
        let ino = InodeId::new(42);

        store.create(ino, "/target").unwrap();
        assert!(store.readlink(ino).is_ok());

        store.delete(ino).unwrap();
        assert!(matches!(
            store.readlink(ino),
            Err(MetaError::InodeNotFound(_))
        ));
    }

    #[test]
    fn test_validate_target_empty() {
        let result = SymlinkStore::validate_target("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_target_too_long() {
        let long_target: String = "a".repeat(4097);
        let result = SymlinkStore::validate_target(&long_target);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_target_null_byte() {
        let result = SymlinkStore::validate_target("path\0with\0nulls");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_target_valid() {
        assert!(SymlinkStore::validate_target("/valid/path").is_ok());
        assert!(SymlinkStore::validate_target("relative/path").is_ok());
        assert!(SymlinkStore::validate_target(&"a".repeat(4096)).is_ok());
    }

    #[test]
    fn test_resolve_symlink() {
        let store = make_test_store();

        let symlink_ino = InodeId::new(10);
        let file_ino = InodeId::new(20);

        store.create(symlink_ino, "target.txt").unwrap();

        let lookup_fn = |parent: InodeId, name: &str| -> Option<(InodeId, bool)> {
            if parent == InodeId::ROOT_INODE && name == "target.txt" {
                Some((file_ino, false))
            } else {
                None
            }
        };

        let result = store.resolve(symlink_ino, 10, lookup_fn);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_loop_detection() {
        let store = make_test_store();

        let symlink_ino = InodeId::new(10);
        store.create(symlink_ino, "/link").unwrap();

        let lookup_fn = |_parent: InodeId, _name: &str| -> Option<(InodeId, bool)> {
            Some((symlink_ino, true))
        };

        let result = store.resolve(symlink_ino, 3, lookup_fn);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_all() {
        let store = make_test_store();

        store.create(InodeId::new(1), "/target1").unwrap();
        store.create(InodeId::new(2), "/target2").unwrap();
        store.create(InodeId::new(3), "/target3").unwrap();

        let list = store.list_all();
        assert_eq!(list.len(), 3);

        let inos: std::collections::HashSet<u64> =
            list.iter().map(|(ino, _)| ino.as_u64()).collect();
        assert!(inos.contains(&1));
        assert!(inos.contains(&2));
        assert!(inos.contains(&3));
    }

    #[test]
    fn test_list_all_empty() {
        let store = make_test_store();
        let list = store.list_all();
        assert!(list.is_empty());
    }

    #[test]
    fn test_create_overwrite() {
        let store = make_test_store();
        let ino = InodeId::new(42);

        store.create(ino, "/old").unwrap();
        store.create(ino, "/new").unwrap();

        let retrieved = store.readlink(ino).unwrap();
        assert_eq!(retrieved, "/new");
    }

    #[test]
    fn test_delete_nonexistent() {
        let store = make_test_store();
        let result = store.delete(InodeId::new(42));
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_not_symlink() {
        let store = make_test_store();
        let ino = InodeId::new(42);

        let lookup_fn = |_: InodeId, _: &str| None;
        let result = store.resolve(ino, 10, lookup_fn);
        assert_eq!(result.unwrap(), ino);
    }
}
