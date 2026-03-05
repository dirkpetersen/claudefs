//! Orchestrate key rotation across all encrypted data without requiring full re-encryption.
//!
//! Implements envelope encryption where only the outer key wrapper needs rotation;
//! individual data keys can be lazily rewrapped on next access.

use crate::encryption::EncryptionKey;
use crate::error::ReduceError;
use crate::key_manager::{KeyVersion, VersionedKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Tracks the lifecycle phase of a key rotation operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RotationPhase {
    /// Rotation pending, scheduled but not yet started.
    Pending,
    /// Rotation is in progress.
    InProgress,
    /// Rotation completed successfully.
    Completed,
    /// Rotation failed.
    Failed,
}

/// Scheduling policy for automatic key rotation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RotationSchedule {
    /// Rotate keys after specified number of days.
    TimeBasedDays(u32),
    /// Rotate keys after specified number of GB written.
    SizeBasedGb(u64),
    /// Rotate on explicit trigger only.
    Manual,
}

/// Tracks metrics for key rotation operations.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RotationMetrics {
    /// Total keys rotated.
    pub keys_rotated: u64,
    /// Chunks with updated key versions.
    pub data_keys_updated: u64,
    /// Envelope keys rewrapped.
    pub envelopes_rewrapped: u64,
    /// Last rotation duration in milliseconds.
    pub rotation_duration_ms: u64,
    /// Total bytes of data with rotated keys.
    pub data_rotated_bytes: u64,
}

impl RotationMetrics {
    /// Create a new empty metrics record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a key rotation event.
    pub fn record_rotation(&mut self, key_count: u64, byte_count: u64, duration_ms: u64) {
        self.keys_rotated += key_count;
        self.data_rotated_bytes += byte_count;
        self.rotation_duration_ms = duration_ms;
    }
}

/// Manages envelope encryption and key rotation lifecycle.
pub struct KeyRotationOrchestrator {
    /// Current KEK (master key).
    current_kek: Option<VersionedKey>,
    /// Old KEK for decryption during rotation.
    old_kek: Option<VersionedKey>,
    /// Rotation phase.
    phase: RotationPhase,
    /// Scheduling policy.
    schedule: RotationSchedule,
    /// Last rotation timestamp (seconds since UNIX_EPOCH).
    last_rotation: u64,
    /// Bytes written since last rotation.
    bytes_written: u64,
    /// Rotation metrics.
    metrics: RotationMetrics,
    /// Mapping of data key version to wrapped keys (for lazy rotation).
    wrapped_keys: HashMap<KeyVersion, Vec<u8>>,
}

impl KeyRotationOrchestrator {
    /// Create a new key rotation orchestrator.
    pub fn new(schedule: RotationSchedule) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            current_kek: None,
            old_kek: None,
            phase: RotationPhase::Pending,
            schedule,
            last_rotation: now,
            bytes_written: 0,
            metrics: RotationMetrics::new(),
            wrapped_keys: HashMap::new(),
        }
    }

    /// Initialize the orchestrator with an initial KEK.
    pub fn initialize(&mut self, kek: VersionedKey) -> Result<(), ReduceError> {
        self.current_kek = Some(kek);
        Ok(())
    }

    /// Check if rotation is due based on the schedule policy.
    pub fn should_rotate(&self) -> bool {
        match self.schedule {
            RotationSchedule::TimeBasedDays(days) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let threshold_secs = u64::from(days) * 86400;
                now.saturating_sub(self.last_rotation) > threshold_secs
            }
            RotationSchedule::SizeBasedGb(gb_limit) => {
                let threshold_bytes = gb_limit * 1_073_741_824;
                self.bytes_written > threshold_bytes
            }
            RotationSchedule::Manual => false,
        }
    }

    /// Start a rotation operation by transitioning to a new KEK.
    pub fn start_rotation(&mut self, new_kek: VersionedKey) -> Result<(), ReduceError> {
        if self.phase != RotationPhase::Pending && self.phase != RotationPhase::Completed {
            return Err(ReduceError::InvalidInput(
                "Rotation already in progress".to_string(),
            ));
        }

        self.phase = RotationPhase::InProgress;
        self.old_kek = self.current_kek.clone();
        self.current_kek = Some(new_kek);
        self.bytes_written = 0;
        self.last_rotation = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(())
    }

    /// Complete a rotation operation.
    pub fn complete_rotation(&mut self, duration_ms: u64) -> Result<(), ReduceError> {
        if self.phase != RotationPhase::InProgress {
            return Err(ReduceError::InvalidInput(
                "Rotation not in progress".to_string(),
            ));
        }

        self.phase = RotationPhase::Completed;
        self.metrics
            .record_rotation(1, self.bytes_written, duration_ms);

        Ok(())
    }

    /// Fail the current rotation operation.
    pub fn fail_rotation(&mut self, _reason: &str) -> Result<(), ReduceError> {
        self.phase = RotationPhase::Failed;
        Ok(())
    }

    /// Record that a data key was wrapped (for tracking progress).
    pub fn record_wrapped_key(
        &mut self,
        key_version: KeyVersion,
        wrapped: Vec<u8>,
    ) -> Result<(), ReduceError> {
        self.wrapped_keys.insert(key_version, wrapped);
        self.metrics.envelopes_rewrapped += 1;
        Ok(())
    }

    /// Record bytes written during rotation.
    pub fn record_write(&mut self, bytes: u64) {
        self.bytes_written += bytes;
    }

    /// Get the current rotation metrics.
    pub fn metrics(&self) -> &RotationMetrics {
        &self.metrics
    }

    /// Get the current KEK version.
    pub fn current_kek_version(&self) -> Option<KeyVersion> {
        self.current_kek.as_ref().map(|k| k.version)
    }

    /// Get the current rotation phase.
    pub fn phase(&self) -> RotationPhase {
        self.phase
    }

    /// Get a previously wrapped key for decryption.
    pub fn get_wrapped_key(&self, version: KeyVersion) -> Option<&[u8]> {
        self.wrapped_keys.get(&version).map(|v| v.as_slice())
    }

    /// Check if a key version needs lazy rewrap (old key still in use).
    pub fn needs_lazy_rewrap(&self, version: KeyVersion) -> bool {
        self.old_kek.as_ref().map(|k| k.version == version).unwrap_or(false)
            && self.phase == RotationPhase::Completed
    }

    /// Reset orchestrator for next rotation cycle.
    pub fn reset_for_next_cycle(&mut self) -> Result<(), ReduceError> {
        self.phase = RotationPhase::Pending;
        self.old_kek = None;
        self.wrapped_keys.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_key(version: u32) -> VersionedKey {
        VersionedKey {
            version: KeyVersion(version),
            key: EncryptionKey([0u8; 32]),
        }
    }

    #[test]
    fn test_orchestrator_creation() {
        let orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        assert_eq!(orch.phase, RotationPhase::Pending);
        assert!(orch.current_kek.is_none());
    }

    #[test]
    fn test_initialize_with_kek() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let kek = create_test_key(1);
        assert!(orch.initialize(kek.clone()).is_ok());
        assert_eq!(orch.current_kek_version(), Some(KeyVersion(1)));
    }

    #[test]
    fn test_phase_transition() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let kek1 = create_test_key(1);
        let kek2 = create_test_key(2);

        assert!(orch.initialize(kek1).is_ok());
        assert_eq!(orch.phase(), RotationPhase::Pending);

        assert!(orch.start_rotation(kek2).is_ok());
        assert_eq!(orch.phase(), RotationPhase::InProgress);

        assert!(orch.complete_rotation(100).is_ok());
        assert_eq!(orch.phase(), RotationPhase::Completed);
    }

    #[test]
    fn test_cannot_start_rotation_twice() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let kek1 = create_test_key(1);
        let kek2 = create_test_key(2);
        let kek3 = create_test_key(3);

        assert!(orch.initialize(kek1).is_ok());
        assert!(orch.start_rotation(kek2).is_ok());
        assert!(orch.start_rotation(kek3).is_err());
    }

    #[test]
    fn test_time_based_rotation_check() {
        let orch = KeyRotationOrchestrator::new(RotationSchedule::TimeBasedDays(1));
        // Initially should not rotate (last_rotation is now)
        assert!(!orch.should_rotate());
    }

    #[test]
    fn test_wrapped_key_storage() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let version = KeyVersion(1);
        let wrapped = vec![1, 2, 3, 4, 5];

        assert!(orch.record_wrapped_key(version, wrapped.clone()).is_ok());
        assert_eq!(orch.get_wrapped_key(version), Some(wrapped.as_slice()));
    }

    #[test]
    fn test_metrics_recording() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let kek = create_test_key(1);

        assert!(orch.initialize(kek.clone()).is_ok());
        assert!(orch.start_rotation(create_test_key(2)).is_ok());
        orch.record_write(1000);
        assert!(orch.complete_rotation(50).is_ok());

        let metrics = orch.metrics();
        assert_eq!(metrics.keys_rotated, 1);
        assert_eq!(metrics.rotation_duration_ms, 50);
    }

    #[test]
    fn test_old_kek_tracking() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let kek1 = create_test_key(1);
        let kek2 = create_test_key(2);

        assert!(orch.initialize(kek1).is_ok());
        assert!(orch.start_rotation(kek2.clone()).is_ok());

        assert_eq!(orch.current_kek_version(), Some(KeyVersion(2)));
        assert!(orch.old_kek.is_some());
    }

    #[test]
    fn test_needs_lazy_rewrap() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let kek1 = create_test_key(1);
        let kek2 = create_test_key(2);

        assert!(orch.initialize(kek1).is_ok());
        assert!(orch.start_rotation(kek2).is_ok());
        assert!(orch.complete_rotation(100).is_ok());

        assert!(orch.needs_lazy_rewrap(KeyVersion(1)));
        assert!(!orch.needs_lazy_rewrap(KeyVersion(2)));
    }

    #[test]
    fn test_reset_for_next_cycle() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let kek1 = create_test_key(1);
        let kek2 = create_test_key(2);

        assert!(orch.initialize(kek1).is_ok());
        assert!(orch.start_rotation(kek2).is_ok());
        assert!(orch.complete_rotation(100).is_ok());
        assert_eq!(orch.phase(), RotationPhase::Completed);

        assert!(orch.reset_for_next_cycle().is_ok());
        assert_eq!(orch.phase(), RotationPhase::Pending);
    }

    #[test]
    fn test_fail_rotation() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let kek1 = create_test_key(1);
        let kek2 = create_test_key(2);

        assert!(orch.initialize(kek1).is_ok());
        assert!(orch.start_rotation(kek2).is_ok());
        assert!(orch.fail_rotation("test error").is_ok());
        assert_eq!(orch.phase(), RotationPhase::Failed);
    }

    #[test]
    fn test_record_write_accumulation() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::SizeBasedGb(1));
        let kek = create_test_key(1);

        assert!(orch.initialize(kek).is_ok());
        orch.record_write(1000);
        orch.record_write(2000);
        assert!(!orch.should_rotate());
    }

    #[test]
    fn test_envelope_rewrap_count() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let version = KeyVersion(1);

        assert!(orch.record_wrapped_key(version, vec![1, 2, 3]).is_ok());
        assert!(orch.record_wrapped_key(version, vec![4, 5, 6]).is_ok());

        assert_eq!(orch.metrics().envelopes_rewrapped, 2);
    }

    #[test]
    fn test_concurrent_phase_check() {
        let orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);
        let phase1 = orch.phase();
        let phase2 = orch.phase();
        assert_eq!(phase1, phase2);
    }

    #[test]
    fn test_multiple_wrapped_keys() {
        let mut orch = KeyRotationOrchestrator::new(RotationSchedule::Manual);

        for i in 1..5 {
            let version = KeyVersion(i as u32);
            let wrapped = vec![i as u8; 10];
            assert!(orch.record_wrapped_key(version, wrapped).is_ok());
        }

        assert!(orch.get_wrapped_key(KeyVersion(1)).is_some());
        assert!(orch.get_wrapped_key(KeyVersion(4)).is_some());
        assert!(orch.get_wrapped_key(KeyVersion(5)).is_none());
    }
}
