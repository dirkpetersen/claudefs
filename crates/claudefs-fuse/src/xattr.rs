use crate::error::{FuseError, Result};
use crate::inode::InodeId;
use std::collections::HashMap;
use std::ffi::OsStr;

const MAX_NAME_LEN: usize = 255;
const MAX_VALUE_LEN: usize = 65536;

pub struct XattrStore {
    attrs: HashMap<InodeId, HashMap<String, Vec<u8>>>,
}

impl XattrStore {
    pub fn new() -> Self {
        Self {
            attrs: HashMap::new(),
        }
    }

    pub fn set(&mut self, ino: InodeId, name: &OsStr, value: &[u8]) -> Result<()> {
        let name_str = name.to_str().ok_or_else(|| FuseError::InvalidArgument {
            msg: "xattr name is not valid UTF-8".to_string(),
        })?;

        if name_str.is_empty() {
            return Err(FuseError::InvalidArgument {
                msg: "xattr name cannot be empty".to_string(),
            });
        }

        if name_str.len() > MAX_NAME_LEN {
            return Err(FuseError::InvalidArgument {
                msg: format!("xattr name exceeds maximum length of {}", MAX_NAME_LEN),
            });
        }

        if value.len() > MAX_VALUE_LEN {
            return Err(FuseError::InvalidArgument {
                msg: format!("xattr value exceeds maximum length of {}", MAX_VALUE_LEN),
            });
        }

        self.attrs
            .entry(ino)
            .or_default()
            .insert(name_str.to_string(), value.to_vec());
        Ok(())
    }

    pub fn get(&self, ino: InodeId, name: &OsStr) -> Option<&[u8]> {
        let name_str = name.to_str()?;
        self.attrs.get(&ino)?.get(name_str).map(|v| v.as_slice())
    }

    pub fn list(&self, ino: InodeId) -> Vec<String> {
        let mut names: Vec<String> = self
            .attrs
            .get(&ino)
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    pub fn remove(&mut self, ino: InodeId, name: &OsStr) -> Result<()> {
        let name_str = name.to_str().ok_or_else(|| FuseError::InvalidArgument {
            msg: "xattr name is not valid UTF-8".to_string(),
        })?;

        let inode_attrs = self
            .attrs
            .get_mut(&ino)
            .ok_or(FuseError::NotFound { ino })?;

        if inode_attrs.remove(name_str).is_none() {
            return Err(FuseError::NotFound { ino });
        }

        if inode_attrs.is_empty() {
            self.attrs.remove(&ino);
        }

        Ok(())
    }

    pub fn list_size(&self, ino: InodeId) -> u32 {
        self.attrs
            .get(&ino)
            .map(|map| map.keys().map(|name| name.len() + 1).sum::<usize>() as u32)
            .unwrap_or(0)
    }

    pub fn clear_inode(&mut self, ino: InodeId) {
        self.attrs.remove(&ino);
    }

    pub fn len(&self) -> usize {
        self.attrs.values().map(|m| m.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }
}

impl Default for XattrStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> XattrStore {
        XattrStore::new()
    }

    #[test]
    fn test_set_and_get_basic_xattr() {
        let mut store = make_store();
        let ino = 2;

        store.set(ino, OsStr::new("user.test"), b"value").unwrap();
        let result = store.get(ino, OsStr::new("user.test"));

        assert_eq!(result, Some(b"value".as_slice()));
    }

    #[test]
    fn test_set_empty_name_returns_error() {
        let mut store = make_store();
        let ino = 2;

        let result = store.set(ino, OsStr::new(""), b"value");
        assert!(matches!(result, Err(FuseError::InvalidArgument { .. })));
    }

    #[test]
    fn test_set_name_too_long_returns_error() {
        let mut store = make_store();
        let ino = 2;

        let long_name = "x".repeat(256);
        let result = store.set(ino, OsStr::new(&long_name), b"value");
        assert!(matches!(result, Err(FuseError::InvalidArgument { .. })));
    }

    #[test]
    fn test_set_value_too_long_returns_error() {
        let mut store = make_store();
        let ino = 2;

        let long_value = vec![0u8; 65537];
        let result = store.set(ino, OsStr::new("user.test"), &long_value);
        assert!(matches!(result, Err(FuseError::InvalidArgument { .. })));
    }

    #[test]
    fn test_get_non_existent_name_returns_none() {
        let store = make_store();
        let ino = 2;

        let result = store.get(ino, OsStr::new("user.nonexistent"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_list_returns_sorted_names() {
        let mut store = make_store();
        let ino = 2;

        store.set(ino, OsStr::new("user.z"), b"z").unwrap();
        store.set(ino, OsStr::new("user.a"), b"a").unwrap();
        store.set(ino, OsStr::new("user.m"), b"m").unwrap();

        let names = store.list(ino);
        assert_eq!(names, vec!["user.a", "user.m", "user.z"]);
    }

    #[test]
    fn test_list_size_counts_null_terminators() {
        let mut store = make_store();
        let ino = 2;

        store.set(ino, OsStr::new("a"), b"v1").unwrap();
        store.set(ino, OsStr::new("bb"), b"v2").unwrap();

        let size = store.list_size(ino);
        // "a" + null (1+1=2) + "bb" + null (2+1=3) = 5
        assert_eq!(size, 5);
    }

    #[test]
    fn test_remove_existing_xattr() {
        let mut store = make_store();
        let ino = 2;

        store.set(ino, OsStr::new("user.test"), b"value").unwrap();
        store.remove(ino, OsStr::new("user.test")).unwrap();

        let result = store.get(ino, OsStr::new("user.test"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_remove_non_existent_returns_error() {
        let mut store = make_store();
        let ino = 2;

        let result = store.remove(ino, OsStr::new("user.nonexistent"));
        assert!(matches!(result, Err(FuseError::NotFound { .. })));
    }

    #[test]
    fn test_clear_inode_removes_all() {
        let mut store = make_store();
        let ino1 = 2;
        let ino2 = 3;

        store.set(ino1, OsStr::new("user.a"), b"a").unwrap();
        store.set(ino1, OsStr::new("user.b"), b"b").unwrap();
        store.set(ino2, OsStr::new("user.c"), b"c").unwrap();

        store.clear_inode(ino1);

        assert!(store.list(ino1).is_empty());
        assert_eq!(store.list(ino2), vec!["user.c"]);
    }

    #[test]
    fn test_multiple_inodes_are_isolated() {
        let mut store = make_store();
        let ino1 = 2;
        let ino2 = 3;

        store.set(ino1, OsStr::new("user.test"), b"value1").unwrap();
        store.set(ino2, OsStr::new("user.test"), b"value2").unwrap();

        assert_eq!(
            store.get(ino1, OsStr::new("user.test")),
            Some(b"value1".as_slice())
        );
        assert_eq!(
            store.get(ino2, OsStr::new("user.test")),
            Some(b"value2".as_slice())
        );
    }

    #[test]
    fn test_overwrite_existing_xattr_value() {
        let mut store = make_store();
        let ino = 2;

        store.set(ino, OsStr::new("user.test"), b"value1").unwrap();
        store.set(ino, OsStr::new("user.test"), b"value2").unwrap();

        let result = store.get(ino, OsStr::new("user.test"));
        assert_eq!(result, Some(b"value2".as_slice()));
    }
}
