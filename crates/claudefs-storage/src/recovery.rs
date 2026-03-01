//! Crash recovery module for the storage subsystem.
//!
//! Handles recovery of storage state after crash, including:
//! - Superblock validation and loading
//! - Allocator bitmap reconstruction
//! - Journal replay for write-ahead log recovery

use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::error::{StorageError, StorageResult};
use crate::flush::JournalEntry;
use crate::superblock::Superblock;

pub const JOURNAL_CHECKPOINT_MAGIC: u32 = 0x434A4350;

const fn make_crc32c_table() -> [u32; 256] {
    const POLY: u32 = 0x82F63B78;
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
}

const fn crc32c(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = make_crc32c_table();
    let mut crc: u32 = !0;
    let mut i = 0;
    while i < data.len() {
        let idx = ((crc ^ data[i] as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ TABLE[idx];
        i += 1;
    }
    !crc
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    pub cluster_uuid: [u8; 16],
    pub max_journal_replay_entries: usize,
    pub verify_checksums: bool,
    pub allow_partial_recovery: bool,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            cluster_uuid: [0u8; 16],
            max_journal_replay_entries: 100_000,
            verify_checksums: true,
            allow_partial_recovery: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RecoveryPhase {
    #[default]
    NotStarted,
    SuperblockRead,
    BitmapLoaded,
    JournalScanned,
    JournalReplayed,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryState {
    pub phase: RecoveryPhase,
    pub devices_discovered: usize,
    pub devices_valid: usize,
    pub journal_entries_found: usize,
    pub journal_entries_replayed: usize,
    pub errors: Vec<String>,
}

impl Default for RecoveryState {
    fn default() -> Self {
        Self {
            phase: RecoveryPhase::default(),
            devices_discovered: 0,
            devices_valid: 0,
            journal_entries_found: 0,
            journal_entries_replayed: 0,
            errors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocatorBitmap {
    bits: Vec<u8>,
    total_blocks: u64,
}

impl AllocatorBitmap {
    pub fn new(total_blocks: u64) -> Self {
        let bytes_needed = (total_blocks.div_ceil(8)) as usize;
        Self {
            bits: vec![0u8; bytes_needed],
            total_blocks,
        }
    }

    pub fn from_bytes(data: &[u8], total_blocks: u64) -> StorageResult<Self> {
        let bytes_needed = (total_blocks.div_ceil(8)) as usize;
        let mut bits = data.to_vec();

        if bits.len() < bytes_needed {
            bits.resize(bytes_needed, 0);
            warn!(
                "bitmap data too short: {} bytes, expected {}, padding with zeros",
                data.len(),
                bytes_needed
            );
        } else if bits.len() > bytes_needed {
            bits.truncate(bytes_needed);
            debug!(
                "bitmap data too long: {} bytes, truncating to {}",
                data.len(),
                bytes_needed
            );
        }

        Ok(Self { bits, total_blocks })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bits.clone()
    }

    pub fn set_allocated(&mut self, offset_4k: u64, count: u64) {
        for i in 0..count {
            let pos = offset_4k + i;
            if pos < self.total_blocks {
                let byte_idx = (pos / 8) as usize;
                let bit_idx = (pos % 8) as usize;
                self.bits[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    pub fn set_free(&mut self, offset_4k: u64, count: u64) {
        for i in 0..count {
            let pos = offset_4k + i;
            if pos < self.total_blocks {
                let byte_idx = (pos / 8) as usize;
                let bit_idx = (pos % 8) as usize;
                self.bits[byte_idx] &= !(1 << bit_idx);
            }
        }
    }

    pub fn is_allocated(&self, offset_4k: u64) -> bool {
        if offset_4k >= self.total_blocks {
            return false;
        }
        let byte_idx = (offset_4k / 8) as usize;
        let bit_idx = (offset_4k % 8) as usize;
        (self.bits[byte_idx] & (1 << bit_idx)) != 0
    }

    pub fn allocated_count(&self) -> u64 {
        self.bits.iter().map(|b| b.count_ones() as u64).sum()
    }

    pub fn free_count(&self) -> u64 {
        self.total_blocks.saturating_sub(self.allocated_count())
    }

    pub fn allocated_ranges(&self) -> Vec<(u64, u64)> {
        let mut ranges = Vec::new();
        let mut start: Option<u64> = None;
        let mut prev: u64 = 0;

        for pos in 0..self.total_blocks {
            let allocated = self.is_allocated(pos);

            if allocated {
                if start.is_none() {
                    start = Some(pos);
                }
                prev = pos;
            } else if let Some(s) = start {
                ranges.push((s, prev + 1));
                start = None;
            }
        }

        if let Some(s) = start {
            ranges.push((s, prev + 1));
        }

        ranges
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalCheckpoint {
    pub magic: u32,
    pub last_committed_sequence: u64,
    pub last_flushed_sequence: u64,
    pub checkpoint_timestamp_secs: u64,
    pub checksum: u32,
}

impl JournalCheckpoint {
    pub fn new(last_committed: u64, last_flushed: u64) -> Self {
        let timestamp_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut checkpoint = Self {
            magic: JOURNAL_CHECKPOINT_MAGIC,
            last_committed_sequence: last_committed,
            last_flushed_sequence: last_flushed,
            checkpoint_timestamp_secs: timestamp_secs,
            checksum: 0,
        };

        checkpoint.update_checksum();
        checkpoint
    }

    pub fn validate(&self) -> StorageResult<()> {
        if self.magic != JOURNAL_CHECKPOINT_MAGIC {
            debug!(
                expected = JOURNAL_CHECKPOINT_MAGIC,
                actual = self.magic,
                "invalid checkpoint magic"
            );
            return Err(StorageError::CorruptedSuperblock {
                reason: format!(
                    "invalid checkpoint magic: expected {:#x}, got {:#x}",
                    JOURNAL_CHECKPOINT_MAGIC, self.magic
                ),
            });
        }

        let computed = self.compute_checksum();
        if computed != self.checksum {
            debug!(
                expected = self.checksum,
                actual = computed,
                "checkpoint checksum mismatch"
            );
            return Err(StorageError::CorruptedSuperblock {
                reason: format!(
                    "checkpoint checksum mismatch: expected {:#x}, got {:#x}",
                    self.checksum, computed
                ),
            });
        }

        Ok(())
    }

    pub fn to_bytes(&self) -> StorageResult<Vec<u8>> {
        bincode::serialize(self).map_err(|e| StorageError::SerializationError {
            reason: e.to_string(),
        })
    }

    pub fn from_bytes(data: &[u8]) -> StorageResult<Self> {
        bincode::deserialize(data).map_err(|e| StorageError::CorruptedSuperblock {
            reason: format!("checkpoint deserialization failed: {}", e),
        })
    }

    pub fn compute_checksum(&self) -> u32 {
        let clone = Self {
            magic: self.magic,
            last_committed_sequence: self.last_committed_sequence,
            last_flushed_sequence: self.last_flushed_sequence,
            checkpoint_timestamp_secs: self.checkpoint_timestamp_secs,
            checksum: 0,
        };
        let bytes = bincode::serialize(&clone).unwrap_or_default();
        crc32c(&bytes)
    }

    pub fn update_checksum(&mut self) {
        self.checksum = self.compute_checksum();
    }
}

pub struct RecoveryReport {
    pub phase: RecoveryPhase,
    pub devices_discovered: usize,
    pub devices_valid: usize,
    pub journal_entries_found: usize,
    pub journal_entries_replayed: usize,
    pub errors: Vec<String>,
    pub duration_ms: u64,
}

pub struct RecoveryManager {
    config: RecoveryConfig,
    state: RecoveryState,
    start_time_ms: u64,
}

impl RecoveryManager {
    pub fn new(config: RecoveryConfig) -> Self {
        info!(
            cluster_uuid = ?config.cluster_uuid,
            max_journal_entries = config.max_journal_replay_entries,
            verify_checksums = config.verify_checksums,
            allow_partial = config.allow_partial_recovery,
            "creating recovery manager"
        );

        Self {
            config,
            state: RecoveryState::default(),
            start_time_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }

    pub fn validate_superblock(&mut self, data: &[u8]) -> StorageResult<Superblock> {
        self.state.phase = RecoveryPhase::SuperblockRead;
        self.state.devices_discovered += 1;

        let superblock = Superblock::from_bytes(data)?;

        if self.config.verify_checksums {
            superblock.validate()?;
        }

        if !superblock.is_same_cluster(&self.config.cluster_uuid) {
            warn!(
                device_cluster = ?superblock.cluster_uuid,
                expected_cluster = ?self.config.cluster_uuid,
                "device belongs to different cluster"
            );
            if !self.config.allow_partial_recovery {
                return Err(StorageError::CorruptedSuperblock {
                    reason: "device belongs to different cluster".to_string(),
                });
            }
        }

        self.state.devices_valid += 1;
        info!(
            device_idx = superblock.device_idx,
            role = ?superblock.device_role,
            "superblock validated"
        );

        Ok(superblock)
    }

    pub fn load_bitmap(
        &mut self,
        data: &[u8],
        total_blocks: u64,
    ) -> StorageResult<AllocatorBitmap> {
        self.state.phase = RecoveryPhase::BitmapLoaded;

        let bitmap = AllocatorBitmap::from_bytes(data, total_blocks)?;

        let allocated = bitmap.allocated_count();
        let free = bitmap.free_count();

        debug!(
            total_blocks = total_blocks,
            allocated = allocated,
            free = free,
            "bitmap loaded"
        );

        Ok(bitmap)
    }

    pub fn scan_journal_entries(&mut self, data: &[u8]) -> StorageResult<Vec<JournalEntry>> {
        self.state.phase = RecoveryPhase::JournalScanned;

        let mut entries = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            match bincode::deserialize::<JournalEntry>(&data[offset..]) {
                Ok(entry) => {
                    let entry_size = bincode::serialized_size(&entry).unwrap_or(0) as usize;
                    offset += entry_size;

                    if entries.len() >= self.config.max_journal_replay_entries {
                        warn!(
                            max_entries = self.config.max_journal_replay_entries,
                            "reached max journal entries limit"
                        );
                        break;
                    }

                    entries.push(entry);
                }
                Err(e) => {
                    if entries.is_empty() && self.config.allow_partial_recovery {
                        debug!("no valid journal entries found at offset {}", offset);
                        break;
                    }

                    if !self.config.allow_partial_recovery || !entries.is_empty() {
                        warn!(
                            offset = offset,
                            error = %e,
                            "failed to deserialize journal entry"
                        );

                        if !self.config.allow_partial_recovery {
                            return Err(StorageError::SerializationError {
                                reason: format!("failed to scan journal entries: {}", e),
                            });
                        }
                    }
                    break;
                }
            }
        }

        self.state.journal_entries_found = entries.len();

        info!(entries_found = entries.len(), "journal entries scanned");

        Ok(entries)
    }

    pub fn entries_needing_replay(
        &mut self,
        entries: &[JournalEntry],
        checkpoint: &JournalCheckpoint,
    ) -> Vec<JournalEntry> {
        self.state.phase = RecoveryPhase::JournalReplayed;

        let last_sequence = checkpoint.last_committed_sequence;

        let to_replay: Vec<JournalEntry> = entries
            .iter()
            .filter(|e| e.sequence > last_sequence)
            .cloned()
            .collect();

        self.state.journal_entries_replayed = to_replay.len();

        info!(
            last_checkpoint_sequence = last_sequence,
            entries_to_replay = to_replay.len(),
            "determined entries needing replay"
        );

        to_replay
    }

    pub fn report(&self) -> RecoveryReport {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let duration_ms = (now_ms as u64).saturating_sub(self.start_time_ms);

        RecoveryReport {
            phase: self.state.phase,
            devices_discovered: self.state.devices_discovered,
            devices_valid: self.state.devices_valid,
            journal_entries_found: self.state.journal_entries_found,
            journal_entries_replayed: self.state.journal_entries_replayed,
            errors: self.state.errors.clone(),
            duration_ms,
        }
    }

    pub fn state(&self) -> &RecoveryState {
        &self.state
    }

    pub fn mark_complete(&mut self) {
        self.state.phase = RecoveryPhase::Complete;
        info!("recovery marked complete");
    }

    pub fn mark_failed(&mut self, error: String) {
        self.state.phase = RecoveryPhase::Failed;
        self.state.errors.push(error.clone());
        error!(error = %error, "recovery failed");
    }

    pub fn add_error(&mut self, error: String) {
        self.state.errors.push(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockRef, BlockSize, PlacementHint};

    fn create_test_uuid() -> [u8; 16] {
        let mut uuid = [0u8; 16];
        uuid[0..4].copy_from_slice(&0x01020304_u32.to_le_bytes());
        uuid[4..8].copy_from_slice(&0x05060708_u32.to_le_bytes());
        uuid[8..12].copy_from_slice(&0x090A0B0C_u32.to_le_bytes());
        uuid[12..16].copy_from_slice(&0x0D0E0F10_u32.to_le_bytes());
        uuid
    }

    #[test]
    fn test_recovery_config_default() {
        let config = RecoveryConfig::default();
        assert_eq!(config.cluster_uuid, [0u8; 16]);
        assert_eq!(config.max_journal_replay_entries, 100_000);
        assert!(config.verify_checksums);
        assert!(!config.allow_partial_recovery);
    }

    #[test]
    fn test_recovery_config_custom() {
        let uuid = create_test_uuid();
        let config = RecoveryConfig {
            cluster_uuid: uuid,
            max_journal_replay_entries: 50000,
            verify_checksums: false,
            allow_partial_recovery: true,
        };

        assert_eq!(config.cluster_uuid, uuid);
        assert_eq!(config.max_journal_replay_entries, 50000);
        assert!(!config.verify_checksums);
        assert!(config.allow_partial_recovery);
    }

    #[test]
    fn test_recovery_state_default() {
        let state = RecoveryState::default();
        assert_eq!(state.phase, RecoveryPhase::NotStarted);
        assert_eq!(state.devices_discovered, 0);
        assert_eq!(state.devices_valid, 0);
        assert_eq!(state.journal_entries_found, 0);
        assert_eq!(state.journal_entries_replayed, 0);
        assert!(state.errors.is_empty());
    }

    #[test]
    fn test_allocator_bitmap_new() {
        let bitmap = AllocatorBitmap::new(1000);
        assert_eq!(bitmap.total_blocks, 1000);
        assert_eq!(bitmap.allocated_count(), 0);
        assert_eq!(bitmap.free_count(), 1000);
    }

    #[test]
    fn test_allocator_bitmap_set_allocated() {
        let mut bitmap = AllocatorBitmap::new(100);
        bitmap.set_allocated(10, 5);

        assert!(bitmap.is_allocated(10));
        assert!(bitmap.is_allocated(11));
        assert!(bitmap.is_allocated(14));
        assert!(!bitmap.is_allocated(9));
        assert!(!bitmap.is_allocated(15));
    }

    #[test]
    fn test_allocator_bitmap_set_free() {
        let mut bitmap = AllocatorBitmap::new(100);
        bitmap.set_allocated(10, 5);
        bitmap.set_free(12, 2);

        assert!(bitmap.is_allocated(10));
        assert!(bitmap.is_allocated(11));
        assert!(!bitmap.is_allocated(12));
        assert!(!bitmap.is_allocated(13));
        assert!(bitmap.is_allocated(14));
    }

    #[test]
    fn test_allocator_bitmap_out_of_bounds() {
        let mut bitmap = AllocatorBitmap::new(100);

        bitmap.set_allocated(99, 10);
        assert!(bitmap.is_allocated(99));

        bitmap.set_allocated(50, 100);
        assert!(!bitmap.is_allocated(150));
    }

    #[test]
    fn test_allocator_bitmap_from_bytes_exact() {
        let data = vec![0xFF, 0x0F, 0xF0, 0x00];
        let bitmap = AllocatorBitmap::from_bytes(&data, 32).unwrap();

        // byte 0 = 0xFF: all bits set
        for i in 0..8 {
            assert!(
                bitmap.is_allocated(i),
                "block {} should be allocated (byte 0xFF)",
                i
            );
        }
        // byte 1 = 0x0F = 00001111: lower nibble set (blocks 8-11)
        for i in 8..12 {
            assert!(
                bitmap.is_allocated(i),
                "block {} should be allocated (byte 0x0F lower)",
                i
            );
        }
        for i in 12..16 {
            assert!(
                !bitmap.is_allocated(i),
                "block {} should NOT be allocated (byte 0x0F upper)",
                i
            );
        }
        // byte 2 = 0xF0 = 11110000: upper nibble set (blocks 20-23)
        for i in 16..20 {
            assert!(
                !bitmap.is_allocated(i),
                "block {} should NOT be allocated (byte 0xF0 lower)",
                i
            );
        }
        for i in 20..24 {
            assert!(
                bitmap.is_allocated(i),
                "block {} should be allocated (byte 0xF0 upper)",
                i
            );
        }
        // byte 3 = 0x00: all zero
        for i in 24..32 {
            assert!(
                !bitmap.is_allocated(i),
                "block {} should NOT be allocated (byte 0x00)",
                i
            );
        }
    }

    #[test]
    fn test_allocator_bitmap_from_bytes_short() {
        let data = vec![0xAA, 0x55];
        let bitmap = AllocatorBitmap::from_bytes(&data, 100).unwrap();

        assert_eq!(bitmap.bits.len(), 13);

        assert!(bitmap.is_allocated(1));
        assert!(bitmap.is_allocated(3));
        assert!(bitmap.is_allocated(5));
        assert!(bitmap.is_allocated(7));
        assert!(!bitmap.is_allocated(0));
        assert!(!bitmap.is_allocated(2));
    }

    #[test]
    fn test_allocator_bitmap_from_bytes_long() {
        let data = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let bitmap = AllocatorBitmap::from_bytes(&data, 16).unwrap();

        assert_eq!(bitmap.bits.len(), 2);
    }

    #[test]
    fn test_allocator_bitmap_to_bytes() {
        let mut bitmap = AllocatorBitmap::new(64);
        bitmap.set_allocated(0, 8);
        bitmap.set_allocated(16, 8);
        bitmap.set_allocated(32, 8);

        let bytes = bitmap.to_bytes();
        assert_eq!(bytes.len(), 8);

        assert_eq!(bytes[0], 0xFF); // blocks 0-7
        assert_eq!(bytes[1], 0x00); // blocks 8-15
        assert_eq!(bytes[2], 0xFF); // blocks 16-23
        assert_eq!(bytes[3], 0x00); // blocks 24-31
        assert_eq!(bytes[4], 0xFF); // blocks 32-39
    }

    #[test]
    fn test_allocator_bitmap_allocated_count() {
        let mut bitmap = AllocatorBitmap::new(64);

        assert_eq!(bitmap.allocated_count(), 0);

        bitmap.set_allocated(0, 8);
        assert_eq!(bitmap.allocated_count(), 8);

        bitmap.set_allocated(16, 16);
        assert_eq!(bitmap.allocated_count(), 24);
    }

    #[test]
    fn test_allocator_bitmap_free_count() {
        let bitmap = AllocatorBitmap::new(64);

        assert_eq!(bitmap.free_count(), 64);

        let mut bitmap = AllocatorBitmap::new(64);
        bitmap.set_allocated(0, 32);

        assert_eq!(bitmap.free_count(), 32);
    }

    #[test]
    fn test_allocator_bitmap_allocated_ranges() {
        let mut bitmap = AllocatorBitmap::new(100);
        bitmap.set_allocated(5, 3);
        bitmap.set_allocated(10, 2);
        bitmap.set_allocated(50, 10);
        bitmap.set_allocated(80, 5);

        let ranges = bitmap.allocated_ranges();

        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges[0], (5, 8));
        assert_eq!(ranges[1], (10, 12));
        assert_eq!(ranges[2], (50, 60));
        assert_eq!(ranges[3], (80, 85));
    }

    #[test]
    fn test_allocator_bitmap_allocated_ranges_single() {
        let mut bitmap = AllocatorBitmap::new(100);
        bitmap.set_allocated(0, 100);

        let ranges = bitmap.allocated_ranges();

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], (0, 100));
    }

    #[test]
    fn test_allocator_bitmap_allocated_ranges_empty() {
        let bitmap = AllocatorBitmap::new(100);

        let ranges = bitmap.allocated_ranges();

        assert!(ranges.is_empty());
    }

    #[test]
    fn test_journal_checkpoint_new() {
        let checkpoint = JournalCheckpoint::new(100, 200);

        assert_eq!(checkpoint.magic, JOURNAL_CHECKPOINT_MAGIC);
        assert_eq!(checkpoint.last_committed_sequence, 100);
        assert_eq!(checkpoint.last_flushed_sequence, 200);
        assert!(checkpoint.checkpoint_timestamp_secs > 0);
        assert!(checkpoint.checksum != 0);
    }

    #[test]
    fn test_journal_checkpoint_validate_valid() {
        let checkpoint = JournalCheckpoint::new(100, 200);
        assert!(checkpoint.validate().is_ok());
    }

    #[test]
    fn test_journal_checkpoint_validate_invalid_magic() {
        let mut checkpoint = JournalCheckpoint::new(100, 200);
        checkpoint.magic = 0xDEADBEEF;

        let result = checkpoint.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_journal_checkpoint_validate_invalid_checksum() {
        let mut checkpoint = JournalCheckpoint::new(100, 200);
        checkpoint.checksum = 0xDEADBEEF;

        let result = checkpoint.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_journal_checkpoint_serialize_roundtrip() {
        let checkpoint = JournalCheckpoint::new(500, 600);

        let bytes = checkpoint.to_bytes().unwrap();
        let recovered = JournalCheckpoint::from_bytes(&bytes).unwrap();

        assert_eq!(checkpoint.magic, recovered.magic);
        assert_eq!(
            checkpoint.last_committed_sequence,
            recovered.last_committed_sequence
        );
        assert_eq!(
            checkpoint.last_flushed_sequence,
            recovered.last_flushed_sequence
        );
        assert_eq!(checkpoint.checksum, recovered.checksum);
    }

    #[test]
    fn test_journal_checkpoint_compute_checksum() {
        let checkpoint = JournalCheckpoint::new(100, 200);
        let checksum = checkpoint.compute_checksum();

        assert_ne!(checksum, 0);

        let checkpoint2 = JournalCheckpoint::new(100, 200);
        assert_eq!(checksum, checkpoint2.compute_checksum());
    }

    #[test]
    fn test_journal_checkpoint_update_checksum() {
        let mut checkpoint = JournalCheckpoint::new(100, 200);
        let old_checksum = checkpoint.checksum;

        checkpoint.last_committed_sequence = 300;
        checkpoint.update_checksum();

        assert_ne!(checkpoint.checksum, old_checksum);
    }

    #[test]
    fn test_recovery_manager_new() {
        let uuid = create_test_uuid();
        let config = RecoveryConfig {
            cluster_uuid: uuid,
            max_journal_replay_entries: 50000,
            verify_checksums: true,
            allow_partial_recovery: false,
        };

        let manager = RecoveryManager::new(config);

        assert_eq!(manager.state().phase, RecoveryPhase::NotStarted);
    }

    #[test]
    fn test_recovery_manager_validate_superblock() {
        let uuid = create_test_uuid();
        let config = RecoveryConfig {
            cluster_uuid: uuid,
            max_journal_replay_entries: 50000,
            verify_checksums: false,
            allow_partial_recovery: true,
        };

        let mut manager = RecoveryManager::new(config);

        let device_uuid = create_test_uuid();
        let mut superblock = Superblock::new(
            device_uuid,
            uuid,
            0,
            crate::superblock::DeviceRoleCode::Data,
            1_000_000_000_000,
            "test".to_string(),
        );
        superblock.update_checksum();

        let sb_bytes = superblock.to_bytes().unwrap();
        let loaded = manager.validate_superblock(&sb_bytes).unwrap();

        assert_eq!(loaded.device_idx, 0);
        assert_eq!(manager.state().devices_discovered, 1);
        assert_eq!(manager.state().devices_valid, 1);
    }

    #[test]
    fn test_recovery_manager_validate_superblock_wrong_cluster() {
        let uuid = create_test_uuid();
        let other_uuid = {
            let mut u = create_test_uuid();
            u[0] = !u[0];
            u
        };

        let config = RecoveryConfig {
            cluster_uuid: uuid,
            max_journal_replay_entries: 50000,
            verify_checksums: true,
            allow_partial_recovery: false,
        };

        let mut manager = RecoveryManager::new(config);

        let device_uuid = create_test_uuid();
        let mut superblock = Superblock::new(
            device_uuid,
            other_uuid,
            0,
            crate::superblock::DeviceRoleCode::Data,
            1_000_000_000_000,
            "test".to_string(),
        );
        superblock.update_checksum();

        let sb_bytes = superblock.to_bytes().unwrap();
        let result = manager.validate_superblock(&sb_bytes);

        assert!(result.is_err());
    }

    #[test]
    fn test_recovery_manager_load_bitmap() {
        let config = RecoveryConfig::default();
        let mut manager = RecoveryManager::new(config);

        let data = vec![0xFF, 0x0F, 0xF0, 0x00, 0x00];
        let bitmap = manager.load_bitmap(&data, 100).unwrap();

        assert_eq!(bitmap.allocated_count(), 16);
    }

    #[test]
    fn test_recovery_manager_scan_journal_entries() {
        let config = RecoveryConfig::default();
        let mut manager = RecoveryManager::new(config);

        let entry1 = JournalEntry::new(
            1,
            BlockRef {
                id: BlockId::new(0, 0),
                size: BlockSize::B4K,
            },
            vec![0u8; 4096],
            PlacementHint::Journal,
        );
        let entry2 = JournalEntry::new(
            2,
            BlockRef {
                id: BlockId::new(0, 1),
                size: BlockSize::B4K,
            },
            vec![1u8; 4096],
            PlacementHint::Metadata,
        );

        let mut data = Vec::new();
        data.extend_from_slice(&bincode::serialize(&entry1).unwrap());
        data.extend_from_slice(&bincode::serialize(&entry2).unwrap());

        let entries = manager.scan_journal_entries(&data).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[1].sequence, 2);
    }

    #[test]
    fn test_recovery_manager_entries_needing_replay() {
        let config = RecoveryConfig::default();
        let mut manager = RecoveryManager::new(config);

        let entries = vec![
            JournalEntry::new(
                1,
                BlockRef {
                    id: BlockId::new(0, 0),
                    size: BlockSize::B4K,
                },
                vec![0u8; 4096],
                PlacementHint::Journal,
            ),
            JournalEntry::new(
                5,
                BlockRef {
                    id: BlockId::new(0, 1),
                    size: BlockSize::B4K,
                },
                vec![1u8; 4096],
                PlacementHint::Metadata,
            ),
            JournalEntry::new(
                10,
                BlockRef {
                    id: BlockId::new(0, 2),
                    size: BlockSize::B4K,
                },
                vec![2u8; 4096],
                PlacementHint::HotData,
            ),
        ];

        let checkpoint = JournalCheckpoint::new(3, 3);

        let to_replay = manager.entries_needing_replay(&entries, &checkpoint);

        assert_eq!(to_replay.len(), 2);
        assert_eq!(to_replay[0].sequence, 5);
        assert_eq!(to_replay[1].sequence, 10);
    }

    #[test]
    fn test_recovery_manager_report() {
        let config = RecoveryConfig::default();
        let manager = RecoveryManager::new(config);

        let report = manager.report();

        assert_eq!(report.phase, RecoveryPhase::NotStarted);
        assert_eq!(report.duration_ms, 0);
    }

    #[test]
    fn test_recovery_manager_mark_complete() {
        let mut config = RecoveryConfig::default();
        let mut manager = RecoveryManager::new(config);

        manager.mark_complete();

        assert_eq!(manager.state().phase, RecoveryPhase::Complete);
    }

    #[test]
    fn test_recovery_manager_mark_failed() {
        let mut config = RecoveryConfig::default();
        let mut manager = RecoveryManager::new(config);

        manager.mark_failed("test error".to_string());

        assert_eq!(manager.state().phase, RecoveryPhase::Failed);
        assert_eq!(manager.state().errors.len(), 1);
        assert_eq!(manager.state().errors[0], "test error");
    }

    #[test]
    fn test_recovery_manager_add_error() {
        let mut config = RecoveryConfig::default();
        let mut manager = RecoveryManager::new(config);

        manager.add_error("error 1".to_string());
        manager.add_error("error 2".to_string());

        assert_eq!(manager.state().errors.len(), 2);
    }

    #[test]
    fn test_recovery_phase_display() {
        let phases = vec![
            RecoveryPhase::NotStarted,
            RecoveryPhase::SuperblockRead,
            RecoveryPhase::BitmapLoaded,
            RecoveryPhase::JournalScanned,
            RecoveryPhase::JournalReplayed,
            RecoveryPhase::Complete,
            RecoveryPhase::Failed,
        ];

        for phase in phases {
            let _ = format!("{:?}", phase);
        }
    }

    #[test]
    fn test_crc32c_const_fn() {
        let data = b"hello world";
        let hash = crc32c(data);

        assert_ne!(hash, 0);

        let hash2 = crc32c(b"hello world");
        assert_eq!(hash, hash2);

        let hash3 = crc32c(b"different");
        assert_ne!(hash, hash3);
    }

    #[test]
    fn test_journal_checkpoint_magic_constant() {
        assert_eq!(JOURNAL_CHECKPOINT_MAGIC, 0x434A4350);

        let checkpoint = JournalCheckpoint::new(1, 2);
        assert_eq!(checkpoint.magic, JOURNAL_CHECKPOINT_MAGIC);
    }

    #[test]
    fn test_max_entries_limit() {
        let config = RecoveryConfig {
            cluster_uuid: create_test_uuid(),
            max_journal_replay_entries: 3,
            verify_checksums: false,
            allow_partial_recovery: true,
        };

        let mut manager = RecoveryManager::new(config);

        let entries: Vec<JournalEntry> = (1..=10)
            .map(|seq| {
                JournalEntry::new(
                    seq,
                    BlockRef {
                        id: BlockId::new(0, seq),
                        size: BlockSize::B4K,
                    },
                    vec![0u8; 4096],
                    PlacementHint::Journal,
                )
            })
            .collect();

        let mut data = Vec::new();
        for entry in &entries {
            data.extend_from_slice(&bincode::serialize(entry).unwrap());
        }

        let scanned = manager.scan_journal_entries(&data).unwrap();

        assert_eq!(scanned.len(), 3);
    }
}
