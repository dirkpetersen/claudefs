//! Persistence and recovery for key rotation operations.
//!
//! Checkpoints rotation progress periodically to enable recovery from crashes
//! or failures during key rotation. Supports resuming incomplete rotations.

use crate::error::ReduceError;
use crate::key_manager::KeyVersion;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Represents the progress and state of a key rotation operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotationCheckpoint {
    /// Unique rotation ID.
    pub rotation_id: u64,
    /// Rotation phase (0: pending, 1: in_progress, 2: complete, 3: failed).
    pub phase: u8,
    /// Timestamp when checkpoint was created (seconds since UNIX_EPOCH).
    pub timestamp: u64,
    /// Source key version being rotated from.
    pub from_key_version: KeyVersion,
    /// Target key version being rotated to.
    pub to_key_version: KeyVersion,
    /// Number of chunks processed so far.
    pub chunks_processed: u64,
    /// Total chunks estimated for rotation.
    pub chunks_estimated: u64,
    /// Bytes of data rotated so far.
    pub bytes_rotated: u64,
    /// Last chunk ID processed (for resumption).
    pub last_chunk_id: u64,
    /// Checksum for integrity verification (CRC32).
    pub checksum: u32,
    /// Metadata about the rotation (e.g., reason, duration estimate).
    pub metadata: HashMap<String, String>,
}

impl RotationCheckpoint {
    /// Create a new checkpoint.
    pub fn new(
        rotation_id: u64,
        phase: u8,
        from_version: KeyVersion,
        to_version: KeyVersion,
    ) -> Self {
        let checkpoint = Self {
            rotation_id,
            phase,
            timestamp: Self::now(),
            from_key_version: from_version,
            to_key_version: to_version,
            chunks_processed: 0,
            chunks_estimated: 0,
            bytes_rotated: 0,
            last_chunk_id: 0,
            checksum: 0,
            metadata: HashMap::new(),
        };

        // Recompute checksum for consistency
        checkpoint.recompute_checksum()
    }

    /// Recompute and set the checkpoint's checksum.
    pub fn recompute_checksum(&self) -> Self {
        let mut checkpoint = self.clone();
        checkpoint.checksum = 0; // Reset before computing

        let serialized = format!("{:?}", checkpoint);
        checkpoint.checksum = Self::compute_crc32(&serialized);

        checkpoint
    }

    /// Verify the checkpoint's integrity.
    pub fn verify_integrity(&self) -> Result<(), ReduceError> {
        let mut temp = self.clone();
        temp.checksum = 0;
        let serialized = format!("{:?}", temp);
        let expected = Self::compute_crc32(&serialized);

        if expected == self.checksum {
            Ok(())
        } else {
            Err(ReduceError::ChecksumMismatch)
        }
    }

    /// Update progress metrics in the checkpoint.
    pub fn update_progress(&mut self, chunks: u64, bytes: u64, last_chunk_id: u64) {
        self.chunks_processed += chunks;
        self.bytes_rotated += bytes;
        self.last_chunk_id = last_chunk_id;
        self.timestamp = Self::now();
        *self = self.clone().recompute_checksum();
    }

    /// Set the total estimated chunks for rotation.
    pub fn set_estimated_chunks(&mut self, total: u64) {
        self.chunks_estimated = total;
        *self = self.clone().recompute_checksum();
    }

    /// Add metadata to the checkpoint.
    pub fn set_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
        *self = self.clone().recompute_checksum();
    }

    /// Get progress percentage (0-100).
    pub fn progress_percentage(&self) -> u8 {
        if self.chunks_estimated == 0 {
            0
        } else {
            ((self.chunks_processed * 100) / self.chunks_estimated) as u8
        }
    }

    /// Compute CRC32 checksum of a byte string.
    fn compute_crc32(data: &str) -> u32 {
        // Simple CRC32 implementation for integrity checking
        let mut crc = 0xffffffff_u32;
        for byte in data.as_bytes() {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                if (crc & 1) != 0 {
                    crc = (crc >> 1) ^ 0xedb88320;
                } else {
                    crc >>= 1;
                }
            }
        }
        crc ^ 0xffffffff
    }

    /// Get current system time as UNIX timestamp.
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// Store and retrieve rotation checkpoints for crash recovery.
pub struct RotationCheckpointStore {
    /// In-memory checkpoint storage (keyed by rotation_id).
    checkpoints: HashMap<u64, RotationCheckpoint>,
    /// Checkpoint history for auditing.
    history: Vec<RotationCheckpoint>,
}

impl RotationCheckpointStore {
    /// Create a new checkpoint store.
    pub fn new() -> Self {
        Self {
            checkpoints: HashMap::new(),
            history: Vec::new(),
        }
    }

    /// Save a checkpoint.
    pub fn save(&mut self, checkpoint: RotationCheckpoint) -> Result<(), ReduceError> {
        // Verify integrity before saving
        checkpoint.verify_integrity()?;

        self.checkpoints
            .insert(checkpoint.rotation_id, checkpoint.clone());
        self.history.push(checkpoint);

        Ok(())
    }

    /// Retrieve a checkpoint by rotation ID.
    pub fn get(&self, rotation_id: u64) -> Option<&RotationCheckpoint> {
        self.checkpoints.get(&rotation_id)
    }

    /// Get the latest checkpoint for a rotation.
    pub fn get_latest(&self) -> Option<&RotationCheckpoint> {
        self.history.last()
    }

    /// List all active checkpoints (not completed).
    pub fn list_active(&self) -> Vec<&RotationCheckpoint> {
        self.checkpoints
            .values()
            .filter(|cp| cp.phase != 2 && cp.phase != 3) // 2=complete, 3=failed
            .collect()
    }

    /// Delete a checkpoint after rotation completes.
    pub fn delete(&mut self, rotation_id: u64) -> Result<(), ReduceError> {
        self.checkpoints.remove(&rotation_id);
        Ok(())
    }

    /// Get checkpoint history.
    pub fn history(&self) -> &[RotationCheckpoint] {
        &self.history
    }

    /// Clear old checkpoints (keep only recent).
    pub fn cleanup_old(&mut self, keep_count: usize) -> Result<usize, ReduceError> {
        let removed = if self.history.len() > keep_count {
            self.history.len() - keep_count
        } else {
            0
        };

        if removed > 0 {
            self.history.drain(0..removed);
            self.checkpoints.clear();
            for cp in &self.history {
                self.checkpoints.insert(cp.rotation_id, cp.clone());
            }
        }

        Ok(removed)
    }

    /// Get total checkpoints stored.
    pub fn total_count(&self) -> usize {
        self.checkpoints.len()
    }
}

impl Default for RotationCheckpointStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Recovery handler for incomplete rotations.
pub struct RotationRecovery;

impl RotationRecovery {
    /// Resume a rotation from a checkpoint.
    pub fn resume_from_checkpoint(
        checkpoint: &RotationCheckpoint,
    ) -> Result<RecoveryInfo, ReduceError> {
        checkpoint.verify_integrity()?;

        Ok(RecoveryInfo {
            rotation_id: checkpoint.rotation_id,
            last_chunk_id: checkpoint.last_chunk_id,
            from_version: checkpoint.from_key_version,
            to_version: checkpoint.to_key_version,
            processed_so_far: checkpoint.chunks_processed,
            bytes_so_far: checkpoint.bytes_rotated,
        })
    }

    /// Validate checkpoint integrity.
    pub fn validate(checkpoint: &RotationCheckpoint) -> Result<(), ReduceError> {
        checkpoint.verify_integrity()
    }

    /// Detect incomplete rotations needing recovery.
    pub fn detect_incomplete(store: &RotationCheckpointStore) -> Vec<&RotationCheckpoint> {
        store.list_active()
    }
}

/// Information needed to resume a rotation.
#[derive(Clone, Debug)]
pub struct RecoveryInfo {
    /// ID of the rotation to resume.
    pub rotation_id: u64,
    /// Last chunk ID that was processed.
    pub last_chunk_id: u64,
    /// Source key version.
    pub from_version: KeyVersion,
    /// Target key version.
    pub to_version: KeyVersion,
    /// Number of chunks processed before failure.
    pub processed_so_far: u64,
    /// Bytes rotated before failure.
    pub bytes_so_far: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_creation() {
        let cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        assert_eq!(cp.rotation_id, 1);
        assert_eq!(cp.phase, 1);
        assert_eq!(cp.chunks_processed, 0);
    }

    #[test]
    fn test_checkpoint_integrity() {
        let cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        assert!(cp.verify_integrity().is_ok());
    }

    #[test]
    fn test_checkpoint_progress_update() {
        let mut cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        cp.set_estimated_chunks(100);
        cp.update_progress(25, 1000, 25);

        assert_eq!(cp.chunks_processed, 25);
        assert_eq!(cp.bytes_rotated, 1000);
        assert_eq!(cp.progress_percentage(), 25);
    }

    #[test]
    fn test_checkpoint_checksum() {
        let cp1 = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        let cp2 = cp1.clone().recompute_checksum();

        assert_eq!(cp1.checksum, cp2.checksum);
    }

    #[test]
    fn test_checkpoint_metadata() {
        let mut cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        cp.set_metadata("reason".to_string(), "scheduled".to_string());
        cp.set_metadata("duration_est_sec".to_string(), "3600".to_string());

        assert_eq!(cp.metadata.get("reason"), Some(&"scheduled".to_string()));
    }

    #[test]
    fn test_checkpoint_store_save_and_get() {
        let mut store = RotationCheckpointStore::new();
        let cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));

        assert!(store.save(cp.clone()).is_ok());
        assert_eq!(store.get(1).map(|c| c.rotation_id), Some(1));
    }

    #[test]
    fn test_checkpoint_store_get_latest() {
        let mut store = RotationCheckpointStore::new();
        let cp1 = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        let cp2 = RotationCheckpoint::new(2, 1, KeyVersion(2), KeyVersion(3));

        assert!(store.save(cp1).is_ok());
        assert!(store.save(cp2).is_ok());
        assert_eq!(store.get_latest().map(|c| c.rotation_id), Some(2));
    }

    #[test]
    fn test_checkpoint_store_list_active() {
        let mut store = RotationCheckpointStore::new();
        let mut cp1 = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        let mut cp2 = RotationCheckpoint::new(2, 2, KeyVersion(2), KeyVersion(3));

        cp1.phase = 1; // in_progress
        cp2.phase = 2; // completed
        assert!(store.save(cp1).is_ok());
        assert!(store.save(cp2).is_ok());

        assert_eq!(store.list_active().len(), 1);
    }

    #[test]
    fn test_checkpoint_store_delete() {
        let mut store = RotationCheckpointStore::new();
        let cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));

        assert!(store.save(cp).is_ok());
        assert_eq!(store.total_count(), 1);
        assert!(store.delete(1).is_ok());
        assert_eq!(store.total_count(), 0);
    }

    #[test]
    fn test_recovery_from_checkpoint() {
        let cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        let recovery = RotationRecovery::resume_from_checkpoint(&cp);

        assert!(recovery.is_ok());
        let info = recovery.unwrap();
        assert_eq!(info.rotation_id, 1);
    }

    #[test]
    fn test_recovery_validation() {
        let cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        assert!(RotationRecovery::validate(&cp).is_ok());
    }

    #[test]
    fn test_recovery_detect_incomplete() {
        let mut store = RotationCheckpointStore::new();
        let mut cp1 = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        let mut cp2 = RotationCheckpoint::new(2, 2, KeyVersion(2), KeyVersion(3));

        cp1.phase = 1; // in_progress
        cp2.phase = 2; // completed

        assert!(store.save(cp1).is_ok());
        assert!(store.save(cp2).is_ok());

        let incomplete = RotationRecovery::detect_incomplete(&store);
        assert_eq!(incomplete.len(), 1);
    }

    #[test]
    fn test_progress_percentage() {
        let mut cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        cp.set_estimated_chunks(100);

        cp.chunks_processed = 0;
        assert_eq!(cp.progress_percentage(), 0);

        cp.chunks_processed = 50;
        assert_eq!(cp.progress_percentage(), 50);

        cp.chunks_processed = 100;
        assert_eq!(cp.progress_percentage(), 100);
    }

    #[test]
    fn test_checkpoint_store_cleanup_old() {
        let mut store = RotationCheckpointStore::new();

        for i in 1..=5 {
            let cp = RotationCheckpoint::new(i, 1, KeyVersion(1), KeyVersion(2));
            assert!(store.save(cp).is_ok());
        }

        assert_eq!(store.total_count(), 5);
        assert!(store.cleanup_old(2).is_ok());
        assert_eq!(store.total_count(), 2);
    }

    #[test]
    fn test_checkpoint_timestamp_updates() {
        let cp1 = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        let ts1 = cp1.timestamp;

        std::thread::sleep(std::time::Duration::from_millis(100));

        let mut cp2 = cp1.clone();
        cp2.update_progress(1, 100, 1);
        let ts2 = cp2.timestamp;

        assert!(ts2 >= ts1);
    }

    #[test]
    fn test_multiple_checkpoints_same_rotation() {
        let mut store = RotationCheckpointStore::new();
        let mut cp1 = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        cp1.update_progress(10, 1000, 10);

        let mut cp1_updated = cp1.clone();
        cp1_updated.update_progress(10, 1000, 20); // Add 10 more to reach 20 total

        assert!(store.save(cp1).is_ok());
        assert!(store.save(cp1_updated).is_ok());

        assert_eq!(store.get(1).map(|c| c.chunks_processed), Some(20));
    }

    #[test]
    fn test_crc32_consistency() {
        let data = "test checkpoint data";
        let crc1 = RotationCheckpoint::compute_crc32(data);
        let crc2 = RotationCheckpoint::compute_crc32(data);
        assert_eq!(crc1, crc2);
    }

    #[test]
    fn test_recovery_info_accuracy() {
        let mut cp = RotationCheckpoint::new(1, 1, KeyVersion(1), KeyVersion(2));
        cp.set_estimated_chunks(1000);
        cp.update_progress(100, 50000, 100);

        let recovery = RotationRecovery::resume_from_checkpoint(&cp).unwrap();
        assert_eq!(recovery.processed_so_far, 100);
        assert_eq!(recovery.bytes_so_far, 50000);
    }
}
