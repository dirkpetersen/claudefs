//! Live configuration store with hot-reload support.
//!
//! This module provides a thread-safe configuration store that supports:
//! - Version tracking for configuration changes
//! - Hot-reload of configuration values
//! - Watcher notifications on configuration changes
//! - JSON validation and parsing

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Error types for live config operations
#[derive(Debug, thiserror::Error)]
pub enum LiveConfigError {
    #[error("config key not found: {0}")]
    NotFound(String),
    #[error("validation failed: {0}")]
    ValidationFailed(String),
    #[error("reload in progress")]
    ReloadInProgress,
    #[error("serialize error: {0}")]
    Serialize(String),
}

/// A single live config entry with version tracking
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiveConfigEntry {
    pub key: String,
    pub value: String,
    pub version: u64,
    pub last_updated: u64,
    pub description: String,
}

/// Reload status returned after a reload attempt
#[derive(Debug, Clone, PartialEq)]
pub enum ReloadStatus {
    Success {
        keys_updated: usize,
        keys_unchanged: usize,
    },
    PartialFailure {
        keys_updated: usize,
        errors: Vec<String>,
    },
    NoChanges,
}

/// A watcher that gets notified of config changes
pub struct ConfigWatcher {
    #[allow(dead_code)]
    watched_keys: Vec<String>,
    sender: tokio::sync::mpsc::UnboundedSender<Vec<String>>,
}

impl ConfigWatcher {
    pub fn new(keys: Vec<String>, sender: tokio::sync::mpsc::UnboundedSender<Vec<String>>) -> Self {
        Self { watched_keys: keys, sender }
    }

    pub fn watched_keys(&self) -> &[String] {
        &self.watched_keys
    }

    pub fn matches(&self, changed_keys: &[String]) -> bool {
        changed_keys.iter().any(|k| self.watched_keys.contains(k))
    }

    pub fn notify(&self, changed_keys: Vec<String>) {
        let _ = self.sender.send(changed_keys);
    }
}

/// Hot-reloadable configuration store
/// Thread-safe via Arc<Mutex<...>> — suitable for sharing across async tasks
pub struct LiveConfigStore {
    entries: Arc<Mutex<HashMap<String, LiveConfigEntry>>>,
    watchers: Arc<Mutex<Vec<ConfigWatcher>>>,
    reload_in_progress: Arc<AtomicBool>,
    version: Arc<AtomicU64>,
}

impl LiveConfigStore {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            watchers: Arc::new(Mutex::new(Vec::new())),
            reload_in_progress: Arc::new(AtomicBool::new(false)),
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set(&self, key: &str, value: &str, description: &str) -> Result<(), LiveConfigError> {
        if self.reload_in_progress.load(Ordering::SeqCst) {
            return Err(LiveConfigError::ReloadInProgress);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| LiveConfigError::Serialize(e.to_string()))?
            .as_secs();

        let new_version = self.version.fetch_add(1, Ordering::SeqCst) + 1;

        let entry = LiveConfigEntry {
            key: key.to_string(),
            value: value.to_string(),
            version: new_version,
            last_updated: now,
            description: description.to_string(),
        };

        let changed_keys = {
            let mut entries = self.entries.lock().unwrap();
            let is_new = !entries.contains_key(key);
            entries.insert(key.to_string(), entry);
            if is_new {
                vec![key.to_string()]
            } else {
                vec![key.to_string()]
            }
        };

        self.notify_watchers(&changed_keys);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<LiveConfigEntry, LiveConfigError> {
        let entries = self.entries.lock().unwrap();
        entries
            .get(key)
            .cloned()
            .ok_or_else(|| LiveConfigError::NotFound(key.to_string()))
    }

    pub fn keys(&self) -> Vec<String> {
        let entries = self.entries.lock().unwrap();
        entries.keys().cloned().collect()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    pub fn remove(&self, key: &str) -> Result<(), LiveConfigError> {
        if self.reload_in_progress.load(Ordering::SeqCst) {
            return Err(LiveConfigError::ReloadInProgress);
        }

        let removed = {
            let mut entries = self.entries.lock().unwrap();
            entries.remove(key).is_some()
        };

        if removed {
            self.version.fetch_add(1, Ordering::SeqCst);
            self.notify_watchers(&[key.to_string()]);
            Ok(())
        } else {
            Err(LiveConfigError::NotFound(key.to_string()))
        }
    }

    pub fn reload(
        &self,
        new_entries: HashMap<String, (String, String)>,
    ) -> ReloadStatus {
        self.reload_in_progress.store(true, Ordering::SeqCst);

        let mut keys_updated = 0;
        let mut keys_unchanged = 0;
        let mut changed_keys = Vec::new();
        #[allow(dead_code)]
        let mut errors = Vec::new();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        {
            let mut entries = self.entries.lock().unwrap();

            let current_keys: Vec<String> = entries.keys().cloned().collect();
            let new_keys: Vec<String> = new_entries.keys().cloned().collect();

            for key in current_keys {
                if !new_keys.contains(&key) {
                    entries.remove(&key);
                    changed_keys.push(key);
                    keys_updated += 1;
                }
            }

            for (key, (value, description)) in new_entries {
                let should_update = match entries.get(&key) {
                    Some(existing) => existing.value != value,
                    None => true,
                };

                if should_update {
                    let new_version = self.version.fetch_add(1, Ordering::SeqCst) + 1;
                    let entry = LiveConfigEntry {
                        key: key.clone(),
                        value: value.clone(),
                        version: new_version,
                        last_updated: now,
                        description: description.clone(),
                    };
                    entries.insert(key.clone(), entry);
                    changed_keys.push(key);
                    keys_updated += 1;
                } else {
                    keys_unchanged += 1;
                }
            }
        }

        self.notify_watchers(&changed_keys);
        self.reload_in_progress.store(false, Ordering::SeqCst);

        if keys_updated == 0 && changed_keys.is_empty() {
            ReloadStatus::NoChanges
        } else if errors.is_empty() {
            ReloadStatus::Success {
                keys_updated,
                keys_unchanged,
            }
        } else {
            ReloadStatus::PartialFailure {
                keys_updated,
                errors,
            }
        }
    }

    pub fn watch(&self, keys: Vec<String>) -> tokio::sync::mpsc::UnboundedReceiver<Vec<String>> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let watcher = ConfigWatcher::new(keys, sender);
        let mut watchers = self.watchers.lock().unwrap();
        watchers.push(watcher);
        receiver
    }

    pub fn watcher_count(&self) -> usize {
        let watchers = self.watchers.lock().unwrap();
        watchers.len()
    }

    fn notify_watchers(&self, changed_keys: &[String]) {
        let watchers = self.watchers.lock().unwrap();
        for watcher in watchers.iter() {
            if watcher.matches(changed_keys) {
                watcher.notify(changed_keys.to_vec());
            }
        }
    }
}

impl Default for LiveConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate that a string is valid JSON
pub fn validate_json(value: &str) -> Result<(), LiveConfigError> {
    serde_json::from_str::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|e| LiveConfigError::ValidationFailed(e.to_string()))
}

/// Parse a LiveConfigEntry value into a typed struct
pub fn parse_entry<T: serde::de::DeserializeOwned>(
    entry: &LiveConfigEntry,
) -> Result<T, LiveConfigError> {
    serde_json::from_str(&entry.value).map_err(|e| LiveConfigError::Serialize(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_store_empty() {
        let store = LiveConfigStore::new();
        assert!(store.keys().is_empty());
        assert_eq!(store.version(), 0);
    }

    #[test]
    fn test_set_and_get() {
        let store = LiveConfigStore::new();
        store.set("test_key", r#""test_value""#, "test description").unwrap();
        let entry = store.get("test_key").unwrap();
        assert_eq!(entry.key, "test_key");
        assert_eq!(entry.value, r#""test_value""#);
        assert_eq!(entry.description, "test description");
        assert!(entry.version > 0);
    }

    #[test]
    fn test_set_updates_version() {
        let store = LiveConfigStore::new();
        let v1 = store.version();
        store.set("key1", "value1", "desc1").unwrap();
        let v2 = store.version();
        store.set("key2", "value2", "desc2").unwrap();
        let v3 = store.version();
        assert!(v2 > v1);
        assert!(v3 > v2);
    }

    #[test]
    fn test_get_not_found() {
        let store = LiveConfigStore::new();
        let result = store.get("nonexistent");
        assert!(matches!(result, Err(LiveConfigError::NotFound(_))));
    }

    #[test]
    fn test_remove_key() {
        let store = LiveConfigStore::new();
        store.set("removeme", "value", "desc").unwrap();
        store.remove("removeme").unwrap();
        let result = store.get("removeme");
        assert!(matches!(result, Err(LiveConfigError::NotFound(_))));
    }

    #[test]
    fn test_remove_not_found() {
        let store = LiveConfigStore::new();
        let result = store.remove("nonexistent");
        assert!(matches!(result, Err(LiveConfigError::NotFound(_))));
    }

    #[test]
    fn test_keys_list() {
        let store = LiveConfigStore::new();
        store.set("a", "1", "desc").unwrap();
        store.set("b", "2", "desc").unwrap();
        store.set("c", "3", "desc").unwrap();
        let keys = store.keys();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"a".to_string()));
        assert!(keys.contains(&"b".to_string()));
        assert!(keys.contains(&"c".to_string()));
    }

    #[test]
    fn test_reload_new_keys() {
        let store = LiveConfigStore::new();
        let mut new_entries = HashMap::new();
        new_entries.insert("key1".to_string(), ("value1".to_string(), "desc1".to_string()));
        new_entries.insert("key2".to_string(), ("value2".to_string(), "desc2".to_string()));
        let status = store.reload(new_entries);
        assert!(matches!(status, ReloadStatus::Success { keys_updated: 2, keys_unchanged: 0 }));
        assert_eq!(store.get("key1").unwrap().value, "value1");
        assert_eq!(store.get("key2").unwrap().value, "value2");
    }

    #[test]
    fn test_reload_unchanged() {
        let store = LiveConfigStore::new();
        store.set("key", "value", "desc").unwrap();
        let mut new_entries = HashMap::new();
        new_entries.insert("key".to_string(), ("value".to_string(), "desc".to_string()));
        let status = store.reload(new_entries);
        assert!(matches!(status, ReloadStatus::NoChanges));
    }

    #[test]
    fn test_reload_partial() {
        let store = LiveConfigStore::new();
        store.set("existing", "old_value", "desc").unwrap();
        let mut new_entries = HashMap::new();
        new_entries.insert("existing".to_string(), ("new_value".to_string(), "desc".to_string()));
        new_entries.insert("new_key".to_string(), ("new_value".to_string(), "desc".to_string()));
        let status = store.reload(new_entries);
        assert!(matches!(
            status,
            ReloadStatus::Success { keys_updated: 2, keys_unchanged: 0 }
        ));
        assert_eq!(store.get("existing").unwrap().value, "new_value");
    }

    #[test]
    fn test_reload_removes_deleted() {
        let store = LiveConfigStore::new();
        store.set("keep", "value", "desc").unwrap();
        store.set("remove", "value", "desc").unwrap();
        let mut new_entries = HashMap::new();
        new_entries.insert("keep".to_string(), ("value".to_string(), "desc".to_string()));
        let status = store.reload(new_entries);
        assert!(matches!(
            status,
            ReloadStatus::Success { keys_updated: 1, keys_unchanged: 1 }
        ));
        assert!(store.get("remove").is_err());
    }

    #[test]
    fn test_reload_success_counts() {
        let store = LiveConfigStore::new();
        store.set("key1", "val1", "desc").unwrap();
        let mut new_entries = HashMap::new();
        new_entries.insert("key1".to_string(), ("val1".to_string(), "desc".to_string()));
        new_entries.insert("key2".to_string(), ("val2".to_string(), "desc".to_string()));
        new_entries.insert("key3".to_string(), ("val3".to_string(), "desc".to_string()));
        let status = store.reload(new_entries);
        assert!(matches!(
            status,
            ReloadStatus::Success {
                keys_updated: 2,
                keys_unchanged: 1
            }
        ));
    }

    #[tokio::test]
    async fn test_watcher_notified_on_set() {
        let store = LiveConfigStore::new();
        let mut receiver = store.watch(vec!["watched_key".to_string()]);
        store.set("watched_key", "value", "desc").unwrap();
        let notified = receiver.try_recv();
        assert!(notified.is_ok());
        assert!(notified.unwrap().contains(&"watched_key".to_string()));
    }

    #[tokio::test]
    async fn test_watcher_not_notified_for_other_key() {
        let store = LiveConfigStore::new();
        let mut receiver = store.watch(vec!["watched_key".to_string()]);
        store.set("other_key", "value", "desc").unwrap();
        let notified = receiver.try_recv();
        assert!(notified.is_err());
    }

    #[tokio::test]
    async fn test_watcher_count() {
        let store = LiveConfigStore::new();
        store.watch(vec!["key1".to_string()]);
        store.watch(vec!["key2".to_string()]);
        assert_eq!(store.watcher_count(), 2);
    }

    #[test]
    fn test_validate_json_valid() {
        assert!(validate_json(r#"{"key": "value"}"#).is_ok());
        assert!(validate_json("123").is_ok());
        assert!(validate_json("true").is_ok());
    }

    #[test]
    fn test_validate_json_invalid() {
        assert!(validate_json("invalid json").is_err());
        assert!(matches!(
            validate_json("invalid json"),
            Err(LiveConfigError::ValidationFailed(_))
        ));
    }

    #[test]
    fn test_parse_entry_i64() {
        let store = LiveConfigStore::new();
        store.set("num", "42", "desc").unwrap();
        let entry = store.get("num").unwrap();
        let parsed: i64 = parse_entry(&entry).unwrap();
        assert_eq!(parsed, 42);
    }

    #[test]
    fn test_parse_entry_bool() {
        let store = LiveConfigStore::new();
        store.set("flag", "true", "desc").unwrap();
        let entry = store.get("flag").unwrap();
        let parsed: bool = parse_entry(&entry).unwrap();
        assert!(parsed);
    }

    #[test]
    fn test_parse_entry_wrong_type() {
        let store = LiveConfigStore::new();
        store.set("flag", "true", "desc").unwrap();
        let entry = store.get("flag").unwrap();
        let result: Result<i64, _> = parse_entry(&entry);
        assert!(result.is_err());
    }

    #[test]
    fn test_reload_no_changes_status() {
        let store = LiveConfigStore::new();
        store.set("key", "value", "desc").unwrap();
        let version_before = store.version();
        let mut new_entries = HashMap::new();
        new_entries.insert("key".to_string(), ("value".to_string(), "desc".to_string()));
        let status = store.reload(new_entries);
        assert!(matches!(status, ReloadStatus::NoChanges));
    }

    #[test]
    fn test_watcher_matches() {
        let (sender, _) = tokio::sync::mpsc::unbounded_channel();
        let watcher = ConfigWatcher::new(vec!["a".to_string(), "b".to_string()], sender);
        assert!(watcher.matches(&["c".to_string(), "a".to_string()]));
        assert!(!watcher.matches(&["c".to_string(), "d".to_string()]));
    }
}