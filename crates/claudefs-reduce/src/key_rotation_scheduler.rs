//! Background key rotation scheduler for envelope encryption.
//!
//! Tracks which chunk DEKs need re-wrapping after a KEK rotation event.

use crate::error::ReduceError;
use crate::key_manager::KeyManager;
use crate::key_manager::{KeyVersion, WrappedKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

#[derive(Debug, Clone, PartialEq)]
pub enum RotationStatus {
    Idle,
    Scheduled {
        target_version: KeyVersion,
    },
    InProgress {
        target_version: KeyVersion,
        rewrapped: usize,
        total: usize,
    },
    Complete {
        version: KeyVersion,
        rewrapped: usize,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationEntry {
    pub chunk_id: u64,
    pub wrapped_key: WrappedKey,
    pub needs_rotation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationConfig {
    pub batch_size: usize,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self { batch_size: 100 }
    }
}

pub struct KeyRotationScheduler {
    status: RotationStatus,
    entries: HashMap<u64, RotationEntry>,
}

impl KeyRotationScheduler {
    pub fn new() -> Self {
        Self {
            status: RotationStatus::Idle,
            entries: HashMap::new(),
        }
    }

    pub fn register_chunk(&mut self, chunk_id: u64, wrapped: WrappedKey) {
        self.entries.insert(
            chunk_id,
            RotationEntry {
                chunk_id,
                wrapped_key: wrapped,
                needs_rotation: false,
            },
        );
    }

    pub fn schedule_rotation(&mut self, target_version: KeyVersion) -> Result<(), ReduceError> {
        if !matches!(self.status, RotationStatus::Idle) {
            return Err(ReduceError::EncryptionFailed(
                "rotation already scheduled".to_string(),
            ));
        }
        self.status = RotationStatus::Scheduled { target_version };
        Ok(())
    }

    pub fn mark_needs_rotation(&mut self, old_version: KeyVersion) {
        for entry in self.entries.values_mut() {
            if entry.wrapped_key.kek_version == old_version {
                entry.needs_rotation = true;
            }
        }
    }

    pub fn rewrap_next(&mut self, km: &mut KeyManager) -> Result<Option<u64>, ReduceError> {
        let target_version = match &self.status {
            RotationStatus::Scheduled { target_version } => {
                let total = self.entries.values().filter(|e| e.needs_rotation).count();
                self.status = RotationStatus::InProgress {
                    target_version: *target_version,
                    rewrapped: 0,
                    total,
                };
                *target_version
            }
            RotationStatus::InProgress { target_version, .. } => *target_version,
            _ => {
                return Err(ReduceError::EncryptionFailed(
                    "no rotation scheduled".to_string(),
                ));
            }
        };

        for (chunk_id, entry) in self.entries.iter_mut() {
            if entry.needs_rotation {
                entry.needs_rotation = false;
                entry.wrapped_key = km.rewrap_dek(&entry.wrapped_key)?;
                entry.wrapped_key.kek_version = target_version;

                if let RotationStatus::InProgress {
                    rewrapped, total, ..
                } = &mut self.status
                {
                    *rewrapped += 1;
                    if *rewrapped >= *total {
                        self.status = RotationStatus::Complete {
                            version: target_version,
                            rewrapped: *rewrapped,
                        };
                    }
                }

                debug!(
                    "rewrapped chunk {} to version {:?}",
                    chunk_id, target_version
                );
                return Ok(Some(*chunk_id));
            }
        }

        if let RotationStatus::InProgress {
            rewrapped, total, ..
        } = &mut self.status
        {
            self.status = RotationStatus::Complete {
                version: target_version,
                rewrapped: *rewrapped,
            };
        }

        Ok(None)
    }

    pub fn status(&self) -> &RotationStatus {
        &self.status
    }

    pub fn pending_count(&self) -> usize {
        self.entries.values().filter(|e| e.needs_rotation).count()
    }

    pub fn total_chunks(&self) -> usize {
        self.entries.len()
    }

    pub fn get_wrapped_key(&self, chunk_id: u64) -> Option<&WrappedKey> {
        self.entries.get(&chunk_id).map(|e| &e.wrapped_key)
    }
}

impl Default for KeyRotationScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encryption::EncryptionKey;
    use crate::key_manager::KeyManagerConfig;

    fn test_key_manager() -> KeyManager {
        KeyManager::with_initial_key(KeyManagerConfig::default(), EncryptionKey([42u8; 32]))
    }

    #[test]
    fn test_new_scheduler_is_idle() {
        let scheduler = KeyRotationScheduler::new();
        assert!(matches!(scheduler.status(), RotationStatus::Idle));
    }

    #[test]
    fn test_register_chunk() {
        let mut scheduler = KeyRotationScheduler::new();
        let wrapped = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };
        scheduler.register_chunk(1, wrapped);
        assert_eq!(scheduler.total_chunks(), 1);
        assert_eq!(scheduler.pending_count(), 0);
    }

    #[test]
    fn test_schedule_rotation_from_idle() {
        let mut scheduler = KeyRotationScheduler::new();
        let result = scheduler.schedule_rotation(KeyVersion(1));
        assert!(result.is_ok());
        assert!(matches!(
            scheduler.status(),
            RotationStatus::Scheduled {
                target_version: KeyVersion(1)
            }
        ));
    }

    #[test]
    fn test_schedule_rotation_fails_if_already_scheduled() {
        let mut scheduler = KeyRotationScheduler::new();
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();
        let result = scheduler.schedule_rotation(KeyVersion(2));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Encryption failed: rotation already scheduled"
        );
    }

    #[test]
    fn test_mark_needs_rotation() {
        let mut scheduler = KeyRotationScheduler::new();
        let wrapped = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };
        scheduler.register_chunk(1, wrapped);
        scheduler.mark_needs_rotation(KeyVersion(0));
        assert_eq!(scheduler.pending_count(), 1);
    }

    #[test]
    fn test_rewrap_next_no_rotation_err() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();
        let result = scheduler.rewrap_next(&mut km);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Encryption failed: no rotation scheduled"
        );
    }

    #[test]
    fn test_rewrap_next_single_chunk() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();

        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        scheduler.register_chunk(1, wrapped);

        scheduler.mark_needs_rotation(KeyVersion(0));
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();

        let result = scheduler.rewrap_next(&mut km);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(1));

        let key = scheduler.get_wrapped_key(1).unwrap();
        assert_eq!(key.kek_version, KeyVersion(1));
    }

    #[test]
    fn test_rewrap_completes_when_all_done() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();

        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        scheduler.register_chunk(1, wrapped);

        scheduler.mark_needs_rotation(KeyVersion(0));
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();

        scheduler.rewrap_next(&mut km).unwrap();

        match scheduler.status() {
            RotationStatus::Complete { version, rewrapped } => {
                assert_eq!(*version, KeyVersion(1));
                assert_eq!(*rewrapped, 1);
            }
            _ => panic!("Expected Complete status"),
        }
    }

    #[test]
    fn test_pending_count() {
        let mut scheduler = KeyRotationScheduler::new();
        let wrapped = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };
        scheduler.register_chunk(1, wrapped.clone());
        scheduler.register_chunk(2, wrapped);
        scheduler.mark_needs_rotation(KeyVersion(0));
        assert_eq!(scheduler.pending_count(), 2);
    }

    #[test]
    fn test_total_chunks() {
        let mut scheduler = KeyRotationScheduler::new();
        let wrapped = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };
        scheduler.register_chunk(1, wrapped.clone());
        scheduler.register_chunk(2, wrapped);
        assert_eq!(scheduler.total_chunks(), 2);
    }

    #[test]
    fn test_get_wrapped_key() {
        let mut scheduler = KeyRotationScheduler::new();
        let wrapped = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };
        scheduler.register_chunk(1, wrapped.clone());
        let retrieved = scheduler.get_wrapped_key(1);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().kek_version, KeyVersion(0));

        assert!(scheduler.get_wrapped_key(999).is_none());
    }

    #[test]
    fn test_rotation_in_progress_tracks_progress() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();

        for i in 1..=3 {
            let dek = km.generate_dek().unwrap();
            let wrapped = km.wrap_dek(&dek).unwrap();
            scheduler.register_chunk(i, wrapped);
        }

        scheduler.mark_needs_rotation(KeyVersion(0));
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();

        match scheduler.status() {
            RotationStatus::Scheduled { .. } => {}
            _ => panic!("Expected Scheduled status"),
        }

        scheduler.rewrap_next(&mut km).unwrap();

        match scheduler.status() {
            RotationStatus::InProgress {
                rewrapped, total, ..
            } => {
                assert_eq!(*rewrapped, 1);
                assert_eq!(*total, 3);
            }
            _ => panic!("Expected InProgress status"),
        }
    }

    #[test]
    fn test_rotation_complete_state() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();

        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        scheduler.register_chunk(1, wrapped);

        scheduler.mark_needs_rotation(KeyVersion(0));
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();
        scheduler.rewrap_next(&mut km).unwrap();

        assert!(matches!(
            scheduler.status(),
            RotationStatus::Complete { .. }
        ));
    }

    #[test]
    fn test_mark_needs_rotation_only_matching_version() {
        let mut scheduler = KeyRotationScheduler::new();

        let wrapped_v0 = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };
        let wrapped_v1 = WrappedKey {
            ciphertext: vec![2u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(1),
        };

        scheduler.register_chunk(1, wrapped_v0);
        scheduler.register_chunk(2, wrapped_v1);

        scheduler.mark_needs_rotation(KeyVersion(0));

        assert_eq!(scheduler.pending_count(), 1);
    }

    #[test]
    fn test_rotation_config_default() {
        let config = RotationConfig::default();
        assert_eq!(config.batch_size, 100);
    }

    #[test]
    fn test_rewrap_uses_current_kek() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();

        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        scheduler.register_chunk(1, wrapped);

        km.rotate_key(EncryptionKey([99u8; 32]));

        scheduler.mark_needs_rotation(KeyVersion(0));
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();

        let result = scheduler.rewrap_next(&mut km);
        assert!(result.is_ok());

        let key = scheduler.get_wrapped_key(1).unwrap();
        assert!(km.is_current_version(key));
    }

    #[test]
    fn test_rewrap_next_returns_none_when_idle() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();

        let wrapped = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };
        scheduler.register_chunk(1, wrapped);

        let result = scheduler.rewrap_next(&mut km);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_chunks_rotation() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();

        for i in 1..=5 {
            let dek = km.generate_dek().unwrap();
            let wrapped = km.wrap_dek(&dek).unwrap();
            scheduler.register_chunk(i, wrapped);
        }

        scheduler.mark_needs_rotation(KeyVersion(0));
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();

        let mut rewrapped_count = 0;
        while let Ok(Some(_)) = scheduler.rewrap_next(&mut km) {
            rewrapped_count += 1;
        }

        assert_eq!(rewrapped_count, 5);
        assert!(matches!(
            scheduler.status(),
            RotationStatus::Complete { rewrapped: 5, .. }
        ));
    }

    #[test]
    fn test_register_overwrites_existing() {
        let mut scheduler = KeyRotationScheduler::new();

        let wrapped1 = WrappedKey {
            ciphertext: vec![1u8; 60],
            nonce: [0u8; 12],
            kek_version: KeyVersion(0),
        };
        scheduler.register_chunk(1, wrapped1);

        let wrapped2 = WrappedKey {
            ciphertext: vec![2u8; 60],
            nonce: [1u8; 12],
            kek_version: KeyVersion(1),
        };
        scheduler.register_chunk(1, wrapped2);

        assert_eq!(scheduler.total_chunks(), 1);
        let key = scheduler.get_wrapped_key(1).unwrap();
        assert_eq!(key.kek_version, KeyVersion(1));
    }

    #[test]
    fn test_schedule_rotation_from_complete_fails() {
        let mut scheduler = KeyRotationScheduler::new();
        let mut km = test_key_manager();

        let dek = km.generate_dek().unwrap();
        let wrapped = km.wrap_dek(&dek).unwrap();
        scheduler.register_chunk(1, wrapped);

        scheduler.mark_needs_rotation(KeyVersion(0));
        scheduler.schedule_rotation(KeyVersion(1)).unwrap();
        scheduler.rewrap_next(&mut km).unwrap();

        let result = scheduler.schedule_rotation(KeyVersion(2));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Encryption failed: rotation already scheduled"
        );
    }
}
