use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigVersion {
    pub version: u64,
    pub timestamp_ms: u64,
    pub author: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub version: ConfigVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    Synced,
    Pending(usize),
    Conflict(String),
}

#[derive(Debug, Error)]
pub enum ConfigSyncError {
    #[error("Key not found: {0}")]
    KeyNotFound(String),
    #[error("Version conflict: {0}")]
    VersionConflict(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

#[derive(Debug)]
pub struct ConfigStore {
    entries: HashMap<String, ConfigEntry>,
    version_counter: u64,
}

impl ConfigStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            version_counter: 0,
        }
    }

    pub fn put(&mut self, key: &str, value: &str, author: &str) -> ConfigVersion {
        self.version_counter += 1;
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let version = ConfigVersion {
            version: self.version_counter,
            timestamp_ms,
            author: author.to_string(),
        };

        let entry = ConfigEntry {
            key: key.to_string(),
            value: value.to_string(),
            version: version.clone(),
        };

        self.entries.insert(key.to_string(), entry);
        version
    }

    pub fn get(&self, key: &str) -> Option<ConfigEntry> {
        self.entries.get(key).cloned()
    }

    pub fn delete(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }

    #[must_use]
    pub fn list_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.entries.keys().cloned().collect();
        keys.sort();
        keys
    }

    #[must_use]
    pub fn current_version(&self) -> u64 {
        self.version_counter
    }

    #[must_use]
    pub fn entries_since(&self, version: u64) -> Vec<ConfigEntry> {
        let mut result: Vec<ConfigEntry> = self
            .entries
            .values()
            .filter(|e| e.version.version > version)
            .cloned()
            .collect();

        result.sort_by_key(|e| e.version.version);
        result
    }
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn new_store_has_version_0() {
        let store = ConfigStore::new();
        assert_eq!(store.current_version(), 0);
    }

    #[test]
    fn put_increments_version() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        assert_eq!(store.current_version(), 1);
        store.put("key2", "value2", "author2");
        assert_eq!(store.current_version(), 2);
    }

    #[test]
    fn put_stores_entry_accessible_via_get() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        let entry = store.get("key1");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().value, "value1");
    }

    #[test]
    fn put_with_same_key_updates_value_and_increments_version() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        assert_eq!(store.current_version(), 1);
        store.put("key1", "value2", "author2");
        assert_eq!(store.current_version(), 2);
        let entry = store.get("key1");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().value, "value2");
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let store = ConfigStore::new();
        let entry = store.get("nonexistent");
        assert!(entry.is_none());
    }

    #[test]
    fn delete_returns_true_for_existing_key() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        let result = store.delete("key1");
        assert!(result);
    }

    #[test]
    fn delete_returns_false_for_missing_key() {
        let mut store = ConfigStore::new();
        let result = store.delete("nonexistent");
        assert!(!result);
    }

    #[test]
    fn delete_removes_key_from_list_keys() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        store.put("key2", "value2", "author2");
        store.delete("key1");
        let keys = store.list_keys();
        assert!(!keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
    }

    #[test]
    fn list_keys_returns_empty_vec_for_new_store() {
        let store = ConfigStore::new();
        let keys = store.list_keys();
        assert!(keys.is_empty());
    }

    #[test]
    fn list_keys_returns_sorted_keys() {
        let mut store = ConfigStore::new();
        store.put("zebra", "value1", "author1");
        store.put("apple", "value2", "author2");
        store.put("banana", "value3", "author3");
        let keys = store.list_keys();
        assert_eq!(keys, vec!["apple", "banana", "zebra"]);
    }

    #[test]
    fn list_keys_excludes_deleted_keys() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        store.put("key2", "value2", "author2");
        store.delete("key1");
        let keys = store.list_keys();
        assert_eq!(keys, vec!["key2"]);
    }

    #[test]
    fn current_version_tracks_all_puts() {
        let mut store = ConfigStore::new();
        store.put("a", "1", "author");
        store.put("b", "2", "author");
        store.put("c", "3", "author");
        assert_eq!(store.current_version(), 3);
    }

    #[test]
    fn entries_since_zero_returns_all_entries() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        store.put("key2", "value2", "author2");
        let entries = store.entries_since(0);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn entries_since_n_returns_only_newer_entries() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        store.put("key2", "value2", "author2");
        store.put("key3", "value3", "author3");
        let entries = store.entries_since(2);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "key3");
    }

    #[test]
    fn entries_since_returns_entries_sorted_by_version() {
        let mut store = ConfigStore::new();
        store.put("a", "1", "author");
        store.put("b", "2", "author");
        store.put("c", "3", "author");
        let entries = store.entries_since(0);
        assert_eq!(entries[0].version.version, 1);
        assert_eq!(entries[1].version.version, 2);
        assert_eq!(entries[2].version.version, 3);
    }

    #[test]
    fn author_is_stored_in_config_version() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "testauthor");
        let entry = store.get("key1").unwrap();
        assert_eq!(entry.version.author, "testauthor");
    }

    #[test]
    fn timestamp_ms_is_set() {
        let mut store = ConfigStore::new();
        store.put("key1", "value1", "author1");
        let entry = store.get("key1").unwrap();
        assert!(entry.version.timestamp_ms > 0);
    }

    #[test]
    fn put_5_keys_list_keys_returns_all_5() {
        let mut store = ConfigStore::new();
        store.put("e", "1", "author");
        store.put("a", "2", "author");
        store.put("c", "3", "author");
        store.put("b", "4", "author");
        store.put("d", "5", "author");
        let keys = store.list_keys();
        assert_eq!(keys.len(), 5);
    }

    #[test]
    fn delete_middle_key_list_keys_returns_4() {
        let mut store = ConfigStore::new();
        store.put("a", "1", "author");
        store.put("b", "2", "author");
        store.put("c", "3", "author");
        store.put("d", "4", "author");
        store.put("e", "5", "author");
        store.delete("c");
        let keys = store.list_keys();
        assert_eq!(keys.len(), 4);
    }

    #[test]
    fn concurrent_puts_increment_version_safely() {
        let store = Arc::new(Mutex::new(ConfigStore::new()));
        let mut handles = vec![];

        for i in 0..10 {
            let store = Arc::clone(&store);
            let handle = std::thread::spawn(move || {
                let mut store = store.lock().unwrap();
                store.put(&format!("key{}", i), "value", "author");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let store = store.lock().unwrap();
        assert_eq!(store.current_version(), 10);
    }
}
