//! Key management for envelope encryption with key rotation support.

use crate::encryption::EncryptionKey;
use crate::error::ReduceError;
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

/// A key version identifier for tracking which KEK was used to wrap a DEK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
pub struct KeyVersion(pub u32);

/// A Data Encryption Key (DEK) — used to encrypt actual chunk data.
/// In envelope encryption, the DEK is stored wrapped (encrypted) by the KEK.
#[derive(Clone, Serialize, Deserialize)]
pub struct DataKey {
    /// Raw 32-byte key material
    pub key: [u8; 32],
}

impl Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DataKey([REDACTED])")
    }
}

/// A DEK wrapped (encrypted) with a KEK for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrappedKey {
    /// The encrypted DEK bytes (AES-256-GCM: 32 bytes + 12 nonce + 16 tag = 60 bytes)
    pub ciphertext: Vec<u8>,
    /// The nonce used for wrapping
    pub nonce: [u8; 12],
    /// Which key version (KEK) was used to wrap this DEK
    pub kek_version: KeyVersion,
}

/// A versioned Key Encryption Key (KEK / master key).
#[derive(Clone)]
pub struct VersionedKey {
    /// The key version
    pub version: KeyVersion,
    /// The encryption key material
    pub key: EncryptionKey,
}

impl Debug for VersionedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VersionedKey {{ version: {:?}, key: [REDACTED] }}\"",
            self.version
        )
    }
}

/// Configuration for the key manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyManagerConfig {
    /// Maximum number of previous KEK versions to keep for decryption of old data.
    pub max_key_history: usize,
}

impl Default for KeyManagerConfig {
    fn default() -> Self {
        Self {
            max_key_history: 10,
        }
    }
}

/// Key manager for envelope encryption with key rotation support.
pub struct KeyManager {
    config: KeyManagerConfig,
    current_kek: Option<VersionedKey>,
    kek_history: HashMap<KeyVersion, VersionedKey>,
}

impl KeyManager {
    /// Creates a new key manager without any loaded key.
    pub fn new(config: KeyManagerConfig) -> Self {
        Self {
            config,
            current_kek: None,
            kek_history: HashMap::new(),
        }
    }

    /// Creates a key manager with an initial KEK at version 0.
    pub fn with_initial_key(config: KeyManagerConfig, key: EncryptionKey) -> Self {
        let versioned_key = VersionedKey {
            version: KeyVersion(0),
            key,
        };
        Self {
            config,
            current_kek: Some(versioned_key),
            kek_history: HashMap::new(),
        }
    }

    /// Returns the current KEK version, or None if no key loaded.
    pub fn current_version(&self) -> Option<KeyVersion> {
        self.current_kek.as_ref().map(|k| k.version)
    }

    /// Rotate to a new master key (KEK). The new key gets version = current_version + 1.
    /// The old key is saved in history for decrypting old data.
    pub fn rotate_key(&mut self, new_key: EncryptionKey) -> KeyVersion {
        let new_version = match &self.current_kek {
            Some(current) => KeyVersion(current.version.0 + 1),
            None => KeyVersion(0),
        };

        if let Some(current) = self.current_kek.take() {
            self.kek_history.insert(current.version, current);
        }

        while self.kek_history.len() > self.config.max_key_history {
            if let Some(min_version) = self.kek_history.keys().min().copied() {
                self.kek_history.remove(&min_version);
            }
        }

        let versioned_key = VersionedKey {
            version: new_version,
            key: new_key,
        };
        self.current_kek = Some(versioned_key);

        new_version
    }

    /// Generate a fresh random DEK for encrypting a new chunk.
    pub fn generate_dek(&self) -> Result<DataKey, ReduceError> {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        Ok(DataKey { key })
    }

    /// Wrap a DEK with the current KEK using AES-256-GCM.
    /// Returns the wrapped key for storage alongside the ciphertext.
    pub fn wrap_dek(&self, dek: &DataKey) -> Result<WrappedKey, ReduceError> {
        let kek = self.current_kek.as_ref().ok_or(ReduceError::MissingKey)?;

        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);

        let cipher = Aes256Gcm::new_from_slice(&kek.key.0)
            .map_err(|e| ReduceError::EncryptionFailed(format!("{}", e)))?;

        let n = aes_gcm::Nonce::from_slice(&nonce);
        let ciphertext = cipher
            .encrypt(n, dek.key.as_ref())
            .map_err(|e| ReduceError::EncryptionFailed(format!("{}", e)))?;

        Ok(WrappedKey {
            ciphertext,
            nonce,
            kek_version: kek.version,
        })
    }

    /// Unwrap a WrappedKey to recover the original DEK.
    /// Automatically selects the correct historical KEK based on kek_version.
    pub fn unwrap_dek(&self, wrapped: &WrappedKey) -> Result<DataKey, ReduceError> {
        let kek = if let Some(current) = &self.current_kek {
            if current.version == wrapped.kek_version {
                Some(&current.key)
            } else {
                None
            }
        } else {
            None
        };

        let kek = kek.or_else(|| self.kek_history.get(&wrapped.kek_version).map(|vk| &vk.key));

        let kek = kek.ok_or(ReduceError::MissingKey)?;

        let cipher = Aes256Gcm::new_from_slice(&kek.0)
            .map_err(|e| ReduceError::EncryptionFailed(format!("{}", e)))?;

        let n = aes_gcm::Nonce::from_slice(&wrapped.nonce);
        let decrypted = cipher
            .decrypt(n, wrapped.ciphertext.as_ref())
            .map_err(|_| ReduceError::DecryptionAuthFailed)?;

        let mut key = [0u8; 32];
        key.copy_from_slice(&decrypted);
        Ok(DataKey { key })
    }

    /// Re-wrap a DEK from an old KEK version to the current KEK.
    /// This is the core of key rotation: called once per chunk during a rotation pass.
    pub fn rewrap_dek(&mut self, old_wrapped: &WrappedKey) -> Result<WrappedKey, ReduceError> {
        let dek = self.unwrap_dek(old_wrapped)?;
        self.wrap_dek(&dek)
    }

    /// Returns true if the given WrappedKey uses the current KEK version.
    pub fn is_current_version(&self, wrapped: &WrappedKey) -> bool {
        self.current_kek
            .as_ref()
            .map(|k| k.version == wrapped.kek_version)
            .unwrap_or(false)
    }

    /// Number of historical KEK versions retained.
    pub fn history_size(&self) -> usize {
        self.kek_history.len()
    }

    /// Clears all historical KEK versions.
    pub fn clear_history(&mut self) {
        self.kek_history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> EncryptionKey {
        EncryptionKey([42u8; 32])
    }

    #[test]
    fn test_generate_dek_is_random() {
        let km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());
        let dek1 = km.generate_dek().unwrap();
        let dek2 = km.generate_dek().unwrap();
        assert_ne!(dek1.key, dek2.key);
    }

    #[test]
    fn test_wrap_unwrap_roundtrip() {
        let km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());
        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        let unwrapped = km.unwrap_dek(&wrapped).unwrap();
        assert_eq!(dek.key, unwrapped.key);
    }

    #[test]
    fn test_unwrap_with_wrong_version_fails() {
        let mut km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());
        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();

        let new_key = EncryptionKey([99u8; 32]);
        km.rotate_key(new_key);

        km.kek_history.clear();

        let result = km.unwrap_dek(&wrapped);
        assert!(matches!(result, Err(ReduceError::MissingKey)));
    }

    #[test]
    fn test_rotate_key_increments_version() {
        let mut km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());
        assert_eq!(km.current_version(), Some(KeyVersion(0)));

        let new_version = km.rotate_key(EncryptionKey([1u8; 32]));
        assert_eq!(new_version, KeyVersion(1));
        assert_eq!(km.current_version(), Some(KeyVersion(1)));
    }

    #[test]
    fn test_rotate_key_keeps_history() {
        let mut km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());

        let dek = km.generate_dek().unwrap();
        let wrapped_v0 = km.wrap_dek(&dek).unwrap();
        assert_eq!(wrapped_v0.kek_version, KeyVersion(0));

        km.rotate_key(EncryptionKey([1u8; 32]));
        km.rotate_key(EncryptionKey([2u8; 32]));

        assert!(km.history_size() >= 2);

        let unwrapped = km.unwrap_dek(&wrapped_v0).unwrap();
        assert_eq!(dek.key, unwrapped.key);
    }

    #[test]
    fn test_rewrap_dek() {
        let mut km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());

        let dek = km.generate_dek().unwrap();
        let old_wrapped = km.wrap_dek(&dek).unwrap();

        let new_key = EncryptionKey([99u8; 32]);
        km.rotate_key(new_key);

        let rewrapped = km.rewrap_dek(&old_wrapped).unwrap();

        let unwrapped = km.unwrap_dek(&rewrapped).unwrap();
        assert_eq!(dek.key, unwrapped.key);

        assert!(km.is_current_version(&rewrapped));
    }

    #[test]
    fn test_is_current_version() {
        let mut km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());

        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        assert!(km.is_current_version(&wrapped));

        km.rotate_key(EncryptionKey([99u8; 32]));

        assert!(!km.is_current_version(&wrapped));
    }

    #[test]
    fn test_history_pruning() {
        let mut km = KeyManager::with_initial_key(KeyManagerConfig::default(), test_key());
        let max_history = km.config.max_key_history;

        for i in 0..max_history + 2 {
            km.rotate_key(EncryptionKey([i as u8; 32]));
        }

        assert!(km.history_size() <= max_history);
    }

    #[test]
    fn test_no_key_returns_missing_key() {
        let km = KeyManager::new(KeyManagerConfig::default());
        let dek = km.generate_dek().unwrap();
        let result = km.wrap_dek(&dek);
        assert!(matches!(result, Err(ReduceError::MissingKey)));
    }
}
