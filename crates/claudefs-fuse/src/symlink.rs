use crate::error::{FuseError, Result};
use crate::inode::InodeId;
use std::collections::HashMap;

const MAX_SYMLINK_LEN: usize = 4096;

pub struct SymlinkStore {
    targets: HashMap<InodeId, String>,
}

impl SymlinkStore {
    pub fn new() -> Self {
        Self {
            targets: HashMap::new(),
        }
    }

    pub fn insert(&mut self, ino: InodeId, target: &str) -> Result<()> {
        validate_symlink_target(target)?;
        self.targets.insert(ino, target.to_string());
        Ok(())
    }

    pub fn get(&self, ino: InodeId) -> Option<&str> {
        self.targets.get(&ino).map(|s| s.as_str())
    }

    pub fn remove(&mut self, ino: InodeId) {
        self.targets.remove(&ino);
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for SymlinkStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn validate_symlink_target(target: &str) -> Result<()> {
    if target.is_empty() {
        return Err(FuseError::InvalidArgument {
            msg: "symlink target cannot be empty".to_string(),
        });
    }
    if target.len() > MAX_SYMLINK_LEN {
        return Err(FuseError::InvalidArgument {
            msg: format!(
                "symlink target exceeds maximum length of {}",
                MAX_SYMLINK_LEN
            ),
        });
    }
    Ok(())
}

pub fn is_circular_symlink(target: &str, start_path: &str) -> bool {
    if target == start_path {
        return true;
    }
    if start_path.ends_with('/') {
        target.starts_with(start_path)
    } else {
        target.starts_with(&format!("{}/", start_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> SymlinkStore {
        SymlinkStore::new()
    }

    #[test]
    fn test_insert_and_get_symlink() {
        let mut store = make_store();
        let ino = 2;

        store.insert(ino, "/some/path").unwrap();
        let result = store.get(ino);

        assert_eq!(result, Some("/some/path"));
    }

    #[test]
    fn test_get_non_existent_returns_none() {
        let store = make_store();
        let ino = 2;

        let result = store.get(ino);
        assert_eq!(result, None);
    }

    #[test]
    fn test_insert_empty_target_returns_error() {
        let mut store = make_store();
        let ino = 2;

        let result = store.insert(ino, "");
        assert!(matches!(result, Err(FuseError::InvalidArgument { .. })));
    }

    #[test]
    fn test_insert_target_too_long_returns_error() {
        let mut store = make_store();
        let ino = 2;

        let long_target = "x".repeat(4097);
        let result = store.insert(ino, &long_target);
        assert!(matches!(result, Err(FuseError::InvalidArgument { .. })));
    }

    #[test]
    fn test_remove_existing() {
        let mut store = make_store();
        let ino = 2;

        store.insert(ino, "/some/path").unwrap();
        store.remove(ino);

        assert_eq!(store.get(ino), None);
    }

    #[test]
    fn test_len_counts_correctly() {
        let mut store = make_store();

        store.insert(2, "/path1").unwrap();
        store.insert(3, "/path2").unwrap();

        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_validate_symlink_target_valid_path() {
        let result = validate_symlink_target("/some/valid/path");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_symlink_target_empty() {
        let result = validate_symlink_target("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_symlink_target_too_long() {
        let long = "x".repeat(4097);
        let result = validate_symlink_target(&long);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_circular_symlink_same_path() {
        let result = is_circular_symlink("/foo/bar", "/foo/bar");
        assert!(result);
    }

    #[test]
    fn test_is_circular_symlink_child_path() {
        let result = is_circular_symlink("/foo/bar/baz", "/foo/bar");
        assert!(result);
    }

    #[test]
    fn test_is_circular_symlink_not_circular() {
        let result = is_circular_symlink("/foo/bar", "/baz");
        assert!(!result);
    }

    #[test]
    fn test_is_circular_symlink_different_branch() {
        let result = is_circular_symlink("/foo/baz", "/foo/bar");
        assert!(!result);
    }

    #[test]
    fn test_remove_non_existent() {
        let mut store = make_store();
        store.remove(999);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_overwrite_existing_symlink() {
        let mut store = make_store();
        let ino = 2;

        store.insert(ino, "/old/path").unwrap();
        store.insert(ino, "/new/path").unwrap();

        assert_eq!(store.get(ino), Some("/new/path"));
    }

    #[test]
    fn test_max_length_target_allowed() {
        let mut store = make_store();
        let ino = 2;

        let max_target = "x".repeat(MAX_SYMLINK_LEN);
        let result = store.insert(ino, &max_target);
        assert!(result.is_ok());
    }
}
