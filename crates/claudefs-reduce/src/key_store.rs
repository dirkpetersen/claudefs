//! Persistent-style encryption key store for managing data encryption keys by version.
//!
//! In ClaudeFS, chunk data is encrypted per D7. Keys are versioned; old keys must be retained
//! to decrypt existing data. New keys can be generated for new data.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for the key store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyStoreConfig {
    /// Maximum number of key versions to retain.
    pub max_versions: usize,
    /// Interval between automatic key rotations in milliseconds.
    pub rotation_interval_ms: u64,
}

impl Default for KeyStoreConfig {
    fn default() -> Self {
        Self {
            max_versions: 100,
            rotation_interval_ms: 30 * 24 * 3600 * 1000, // 30 days
        }
    }
}

/// A stored encryption key with version and lifecycle tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredKey {
    /// The key version number.
    pub version: u32,
    /// The raw key bytes (256-bit AES key).
    pub key_bytes: [u8; 32],
    /// Creation timestamp in milliseconds.
    pub created_at_ms: u64,
    /// When this key was marked deprecated (no longer used for new encryption).
    pub deprecated_at_ms: Option<u64>,
    /// When this key was retired (no longer used for decryption).
    pub retired_at_ms: Option<u64>,
}

impl StoredKey {
    /// Returns true if the key is active (available for new encryption).
    pub fn is_active(&self) -> bool {
        self.deprecated_at_ms.is_none() && self.retired_at_ms.is_none()
    }

    /// Returns true if the key is deprecated (not for new encryption, but still for decryption).
    pub fn is_deprecated(&self) -> bool {
        self.deprecated_at_ms.is_some() && self.retired_at_ms.is_none()
    }

    /// Returns true if the key is retired (no longer used for decryption).
    pub fn is_retired(&self) -> bool {
        self.retired_at_ms.is_some()
    }
}

/// Statistics about the key store.
#[derive(Debug, Clone, Default)]
pub struct KeyStoreStats {
    /// Number of active keys.
    pub active_keys: usize,
    /// Number of deprecated keys.
    pub deprecated_keys: usize,
    /// Number of retired keys.
    pub retired_keys: usize,
    /// Total number of key versions.
    pub total_versions: usize,
}

/// In-memory key store for managing versioned encryption keys.
pub struct KeyStore {
    config: KeyStoreConfig,
    keys: HashMap<u32, StoredKey>,
}

impl KeyStore {
    /// Creates a new empty key store.
    pub fn new(config: KeyStoreConfig) -> Self {
        Self {
            config,
            keys: HashMap::new(),
        }
    }

    /// Generates a new key with the given version.
    pub fn generate_key(&mut self, version: u32, now_ms: u64) -> &StoredKey {
        let mut key_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key_bytes);

        let key = StoredKey {
            version,
            key_bytes,
            created_at_ms: now_ms,
            deprecated_at_ms: None,
            retired_at_ms: None,
        };

        self.keys.insert(version, key);
        self.keys.get(&version).unwrap()
    }

    /// Gets a key by version.
    pub fn get(&self, version: u32) -> Option<&StoredKey> {
        self.keys.get(&version)
    }

    /// Returns the highest active key version.
    pub fn current_version(&self) -> Option<u32> {
        self.keys
            .values()
            .filter(|k| k.is_active())
            .map(|k| k.version)
            .max()
    }

    /// Marks a key as deprecated.
    pub fn deprecate(&mut self, version: u32, now_ms: u64) -> bool {
        if let Some(key) = self.keys.get_mut(&version) {
            if key.is_active() {
                key.deprecated_at_ms = Some(now_ms);
                return true;
            }
        }
        false
    }

    /// Marks a key as retired.
    pub fn retire(&mut self, version: u32, now_ms: u64) -> bool {
        if let Some(key) = self.keys.get_mut(&version) {
            if key.retired_at_ms.is_none() {
                key.retired_at_ms = Some(now_ms);
                return true;
            }
        }
        false
    }

    /// Lists all active keys sorted by version ascending.
    pub fn list_active(&self) -> Vec<&StoredKey> {
        let mut active: Vec<_> = self.keys.values().filter(|k| k.is_active()).collect();
        active.sort_by_key(|k| k.version);
        active
    }

    /// Checks if a key rotation is needed based on the rotation interval.
    pub fn needs_rotation(&self, last_rotation_ms: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(last_rotation_ms) >= self.config.rotation_interval_ms
    }

    /// Returns statistics about the key store.
    pub fn stats(&self) -> KeyStoreStats {
        let active_keys = self.keys.values().filter(|k| k.is_active()).count();
        let deprecated_keys = self.keys.values().filter(|k| k.is_deprecated()).count();
        let retired_keys = self.keys.values().filter(|k| k.is_retired()).count();

        KeyStoreStats {
            active_keys,
            deprecated_keys,
            retired_keys,
            total_versions: self.keys.len(),
        }
    }

    /// Removes all but the last N retired keys.
    pub fn prune_retired(&mut self, keep_last_n: usize) {
        let mut retired_versions: Vec<u32> = self
            .keys
            .values()
            .filter(|k| k.is_retired())
            .map(|k| k.version)
            .collect();
        retired_versions.sort();

        if retired_versions.len() > keep_last_n {
            let to_remove = retired_versions.len() - keep_last_n;
            for version in retired_versions.iter().take(to_remove) {
                self.keys.remove(version);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_store_config_default() {
        let config = KeyStoreConfig::default();
        assert_eq!(config.max_versions, 100);
        assert_eq!(config.rotation_interval_ms, 30 * 24 * 3600 * 1000);
    }

    #[test]
    fn generate_key_creates_entry() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        let key = store.generate_key(1, 1000);

        assert_eq!(key.version, 1);
        assert_eq!(key.created_at_ms, 1000);
        assert!(key.is_active());
        assert!(!key.key_bytes.iter().all(|&b| b == 0));
    }

    #[test]
    fn get_existing_key() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);

        let key = store.get(1);
        assert!(key.is_some());
        assert_eq!(key.unwrap().version, 1);
    }

    #[test]
    fn get_missing_key() {
        let store = KeyStore::new(KeyStoreConfig::default());
        assert!(store.get(999).is_none());
    }

    #[test]
    fn current_version_empty() {
        let store = KeyStore::new(KeyStoreConfig::default());
        assert!(store.current_version().is_none());
    }

    #[test]
    fn current_version_with_key() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.generate_key(5, 2000);
        store.generate_key(3, 1500);

        assert_eq!(store.current_version(), Some(5));
    }

    #[test]
    fn deprecate_key() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);

        let result = store.deprecate(1, 2000);
        assert!(result);

        let key = store.get(1).unwrap();
        assert!(key.is_deprecated());
        assert_eq!(key.deprecated_at_ms, Some(2000));
    }

    #[test]
    fn deprecate_unknown_returns_false() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        let result = store.deprecate(999, 2000);
        assert!(!result);
    }

    #[test]
    fn retire_key() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.deprecate(1, 2000);

        let result = store.retire(1, 3000);
        assert!(result);

        let key = store.get(1).unwrap();
        assert!(key.is_retired());
        assert_eq!(key.retired_at_ms, Some(3000));
    }

    #[test]
    fn is_active_true() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        let key = store.get(1).unwrap();
        assert!(key.is_active());
    }

    #[test]
    fn is_deprecated_true() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.deprecate(1, 2000);
        let key = store.get(1).unwrap();
        assert!(key.is_deprecated());
        assert!(!key.is_active());
        assert!(!key.is_retired());
    }

    #[test]
    fn is_retired_true() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.deprecate(1, 2000);
        store.retire(1, 3000);
        let key = store.get(1).unwrap();
        assert!(key.is_retired());
        assert!(!key.is_active());
        assert!(!key.is_deprecated());
    }

    #[test]
    fn list_active_sorted() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(5, 1000);
        store.generate_key(1, 2000);
        store.generate_key(3, 3000);

        let active = store.list_active();
        assert_eq!(active.len(), 3);
        assert_eq!(active[0].version, 1);
        assert_eq!(active[1].version, 3);
        assert_eq!(active[2].version, 5);
    }

    #[test]
    fn needs_rotation_false() {
        let store = KeyStore::new(KeyStoreConfig::default());
        let interval = store.config.rotation_interval_ms;

        assert!(!store.needs_rotation(1000, 1000 + interval - 1));
    }

    #[test]
    fn needs_rotation_true() {
        let store = KeyStore::new(KeyStoreConfig::default());
        let interval = store.config.rotation_interval_ms;

        assert!(store.needs_rotation(1000, 1000 + interval));
        assert!(store.needs_rotation(1000, 1000 + interval + 1));
    }

    #[test]
    fn stats_counts_correctly() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.generate_key(2, 2000);
        store.generate_key(3, 3000);

        store.deprecate(1, 4000);
        store.deprecate(2, 4000);
        store.retire(1, 5000);

        let stats = store.stats();
        assert_eq!(stats.active_keys, 1);
        assert_eq!(stats.deprecated_keys, 1);
        assert_eq!(stats.retired_keys, 1);
        assert_eq!(stats.total_versions, 3);
    }

    #[test]
    fn prune_retired_keeps_last_n() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.generate_key(2, 2000);
        store.generate_key(3, 3000);
        store.generate_key(4, 4000);

        for v in 1..=4 {
            store.deprecate(v, 5000);
            store.retire(v, 6000);
        }

        store.prune_retired(2);

        assert!(store.get(1).is_none());
        assert!(store.get(2).is_none());
        assert!(store.get(3).is_some());
        assert!(store.get(4).is_some());
    }

    #[test]
    fn deprecate_already_deprecated_returns_false() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.deprecate(1, 2000);

        let result = store.deprecate(1, 3000);
        assert!(!result);
    }

    #[test]
    fn retire_unknown_returns_false() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        let result = store.retire(999, 2000);
        assert!(!result);
    }

    #[test]
    fn list_active_excludes_deprecated() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.generate_key(2, 2000);
        store.deprecate(1, 3000);

        let active = store.list_active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].version, 2);
    }

    #[test]
    fn list_active_empty_when_all_deprecated() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.deprecate(1, 2000);

        let active = store.list_active();
        assert!(active.is_empty());
    }

    #[test]
    fn current_version_excludes_deprecated() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(5, 1000);
        store.deprecate(5, 2000);
        store.generate_key(3, 3000);

        assert_eq!(store.current_version(), Some(3));
    }

    #[test]
    fn prune_retired_no_retired_keys() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);

        store.prune_retired(0);

        assert!(store.get(1).is_some());
    }

    #[test]
    fn prune_retired_keeps_all_if_fewer_than_n() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);
        store.deprecate(1, 2000);
        store.retire(1, 3000);

        store.prune_retired(5);

        assert!(store.get(1).is_some());
    }

    #[test]
    fn key_bytes_are_random() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        let k1 = store.generate_key(1, 1000).clone();
        let k2 = store.generate_key(2, 2000).clone();

        assert_ne!(k1.key_bytes, k2.key_bytes);
    }

    #[test]
    fn stored_key_clone() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        let key = store.generate_key(1, 1000);
        let cloned = key.clone();

        assert_eq!(key.version, cloned.version);
        assert_eq!(key.key_bytes, cloned.key_bytes);
    }

    #[test]
    fn key_store_config_clone() {
        let config = KeyStoreConfig::default();
        let cloned = config.clone();

        assert_eq!(config.max_versions, cloned.max_versions);
        assert_eq!(config.rotation_interval_ms, cloned.rotation_interval_ms);
    }

    #[test]
    fn needs_rotation_with_zero_last() {
        let store = KeyStore::new(KeyStoreConfig::default());
        assert!(store.needs_rotation(0, store.config.rotation_interval_ms));
    }

    #[test]
    fn retire_without_deprecate() {
        let mut store = KeyStore::new(KeyStoreConfig::default());
        store.generate_key(1, 1000);

        let result = store.retire(1, 2000);
        assert!(result);

        let key = store.get(1).unwrap();
        assert!(key.is_retired());
    }
}
