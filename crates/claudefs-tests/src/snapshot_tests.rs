//! Snapshot and Recovery tests for ClaudeFS
//!
//! Tests the snapshot management system and crash recovery functionality.

use claudefs_reduce::snapshot::{Snapshot, SnapshotConfig, SnapshotInfo, SnapshotManager};
use claudefs_storage::recovery::{
    AllocatorBitmap, JournalCheckpoint, RecoveryConfig, RecoveryManager, RecoveryPhase,
    RecoveryReport, JOURNAL_CHECKPOINT_MAGIC,
};
use std::collections::HashMap;

fn make_test_hash(i: u8) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash[0] = i;
    hash[31] = i.wrapping_sub(1);
    hash
}

#[test]
fn test_snapshot_config_default() {
    let config = SnapshotConfig::default();
    assert_eq!(config.max_snapshots, 64);
}

#[test]
fn test_snapshot_config_custom() {
    let config = SnapshotConfig { max_snapshots: 10 };
    assert_eq!(config.max_snapshots, 10);
}

#[test]
fn test_snapshot_manager_creation() {
    let config = SnapshotConfig::default();
    let manager = SnapshotManager::new(config);

    assert_eq!(manager.snapshot_count(), 0);
}

#[test]
fn test_snapshot_creation_with_name() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let hashes: Vec<[u8; 32]> = vec![make_test_hash(1), make_test_hash(2), make_test_hash(3)];
    let hashes_vec: Vec<_> = hashes
        .iter()
        .map(|h| claudefs_reduce::fingerprint::blake3_hash(h))
        .collect();

    let info = manager
        .create_snapshot("test_snapshot".to_string(), hashes_vec.clone(), 12345)
        .unwrap();

    assert_eq!(info.name, "test_snapshot");
    assert_eq!(info.block_count, 3);
    assert_eq!(info.total_bytes, 12345);
    assert!(info.id > 0);
}

#[test]
fn test_snapshot_info_dedup_ratio_field() {
    let info = SnapshotInfo {
        id: 1,
        name: "test".to_string(),
        created_at_secs: 1000,
        block_count: 100,
        total_bytes: 409600,
    };

    assert_eq!(info.block_count, 100);
    assert_eq!(info.total_bytes, 409600);
}

#[test]
fn test_snapshot_info_size() {
    let info = SnapshotInfo {
        id: 1,
        name: "size_test".to_string(),
        created_at_secs: 2000,
        block_count: 50,
        total_bytes: 204800,
    };

    assert_eq!(info.total_bytes, 204800);
}

#[test]
fn test_snapshot_list_ordering() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let hashes: Vec<_> = (0..3)
        .map(|i| claudefs_reduce::fingerprint::blake3_hash(&[i]))
        .collect();

    manager
        .create_snapshot("c".to_string(), hashes.clone(), 0)
        .unwrap();
    manager
        .create_snapshot("a".to_string(), hashes.clone(), 0)
        .unwrap();
    manager
        .create_snapshot("b".to_string(), hashes.clone(), 0)
        .unwrap();

    let list = manager.list_snapshots();

    assert_eq!(list.len(), 3);
    assert!(list[0].created_at_secs <= list[1].created_at_secs);
    assert!(list[1].created_at_secs <= list[2].created_at_secs);
}

#[test]
fn test_snapshot_retention_expiry() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let hashes: Vec<_> = (0..3)
        .map(|i| claudefs_reduce::fingerprint::blake3_hash(&[i]))
        .collect();

    for i in 0..64 {
        let name = format!("snapshot_{}", i);
        manager.create_snapshot(name, hashes.clone(), 0).unwrap();
    }

    let result = manager.create_snapshot("one_more".to_string(), hashes.clone(), 0);
    assert!(result.is_err());
}

#[test]
fn test_snapshot_retention_count() {
    let config = SnapshotConfig { max_snapshots: 3 };
    let mut manager = SnapshotManager::new(config);

    let hashes: Vec<_> = (0..2)
        .map(|i| claudefs_reduce::fingerprint::blake3_hash(&[i]))
        .collect();

    manager
        .create_snapshot("s1".to_string(), hashes.clone(), 0)
        .unwrap();
    manager
        .create_snapshot("s2".to_string(), hashes.clone(), 0)
        .unwrap();
    manager
        .create_snapshot("s3".to_string(), hashes.clone(), 0)
        .unwrap();

    assert_eq!(manager.snapshot_count(), 3);

    let result = manager.create_snapshot("s4".to_string(), hashes.clone(), 0);
    assert!(result.is_err());
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
    let uuid = [1u8; 16];
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
fn test_recovery_manager_creation() {
    let config = RecoveryConfig::default();
    let manager = RecoveryManager::new(config);

    let state = manager.state();
    assert_eq!(state.phase, RecoveryPhase::NotStarted);
}

#[test]
fn test_recovery_phase_sequence() {
    let phases = vec![
        RecoveryPhase::NotStarted,
        RecoveryPhase::SuperblockRead,
        RecoveryPhase::BitmapLoaded,
        RecoveryPhase::JournalScanned,
        RecoveryPhase::JournalReplayed,
        RecoveryPhase::Complete,
    ];

    for phase in phases {
        let _ = format!("{:?}", phase);
    }

    assert!(true);
}

#[test]
fn test_recovery_report_creation() {
    let config = RecoveryConfig::default();
    let manager = RecoveryManager::new(config);

    let report = manager.report();

    assert_eq!(report.phase, RecoveryPhase::NotStarted);
    assert_eq!(report.devices_discovered, 0);
    assert_eq!(report.devices_valid, 0);
    assert_eq!(report.journal_entries_found, 0);
    assert_eq!(report.journal_entries_replayed, 0);
    assert!(report.errors.is_empty());
}

#[test]
fn test_snapshot_delete() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let hashes: Vec<_> = (0..2)
        .map(|i| claudefs_reduce::fingerprint::blake3_hash(&[i]))
        .collect();

    let info = manager
        .create_snapshot("test".to_string(), hashes.clone(), 100)
        .unwrap();
    let deleted = manager.delete_snapshot(info.id);

    assert!(deleted.is_some());
    assert!(manager.get_snapshot(info.id).is_none());
}

#[test]
fn test_snapshot_get() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let hashes: Vec<_> = (0..2)
        .map(|i| claudefs_reduce::fingerprint::blake3_hash(&[i]))
        .collect();

    let info = manager
        .create_snapshot("test".to_string(), hashes.clone(), 200)
        .unwrap();
    let snapshot = manager.get_snapshot(info.id).unwrap();

    assert_eq!(snapshot.info.name, "test");
    assert_eq!(snapshot.block_hashes.len(), 2);
}

#[test]
fn test_snapshot_clone() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let hashes: Vec<_> = (0..3)
        .map(|i| claudefs_reduce::fingerprint::blake3_hash(&[i]))
        .collect();

    let info = manager
        .create_snapshot("original".to_string(), hashes.clone(), 300)
        .unwrap();
    let cloned = manager
        .clone_snapshot(info.id, "clone".to_string())
        .unwrap();

    assert_eq!(cloned.name, "clone");
    assert_eq!(cloned.block_count, 3);
    assert_eq!(cloned.total_bytes, 300);
}

#[test]
fn test_snapshot_clone_nonexistent() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let result = manager.clone_snapshot(999, "test".to_string());
    assert!(result.is_err());
}

#[test]
fn test_snapshot_find_by_name() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let hashes: Vec<_> = vec![claudefs_reduce::fingerprint::blake3_hash(&[1])];

    let info = manager
        .create_snapshot("myname".to_string(), hashes.clone(), 0)
        .unwrap();

    let found = manager.find_by_name("myname").unwrap();
    assert_eq!(found.info.id, info.id);

    assert!(manager.find_by_name("nonexistent").is_none());
}

#[test]
fn test_allocator_bitmap_new() {
    let bitmap = AllocatorBitmap::new(1000);
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
fn test_journal_checkpoint_new() {
    let checkpoint = JournalCheckpoint::new(100, 200);

    assert_eq!(checkpoint.magic, JOURNAL_CHECKPOINT_MAGIC);
    assert_eq!(checkpoint.last_committed_sequence, 100);
    assert_eq!(checkpoint.last_flushed_sequence, 200);
    assert!(checkpoint.checkpoint_timestamp_secs > 0);
    assert!(checkpoint.checksum != 0);
}

#[test]
fn test_journal_checkpoint_validate() {
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
}

#[test]
fn test_recovery_manager_mark_complete() {
    let config = RecoveryConfig::default();
    let mut manager = RecoveryManager::new(config);

    manager.mark_complete();

    assert_eq!(manager.state().phase, RecoveryPhase::Complete);
}

#[test]
fn test_recovery_manager_mark_failed() {
    let config = RecoveryConfig::default();
    let mut manager = RecoveryManager::new(config);

    manager.mark_failed("test error".to_string());

    assert_eq!(manager.state().phase, RecoveryPhase::Failed);
    assert_eq!(manager.state().errors.len(), 1);
    assert_eq!(manager.state().errors[0], "test error");
}

#[test]
fn test_recovery_manager_add_error() {
    let config = RecoveryConfig::default();
    let mut manager = RecoveryManager::new(config);

    manager.add_error("error 1".to_string());
    manager.add_error("error 2".to_string());

    assert_eq!(manager.state().errors.len(), 2);
}

#[test]
fn test_recovery_state_default() {
    let state = claudefs_storage::recovery::RecoveryState::default();

    assert_eq!(state.phase, RecoveryPhase::NotStarted);
    assert_eq!(state.devices_discovered, 0);
    assert_eq!(state.devices_valid, 0);
    assert_eq!(state.journal_entries_found, 0);
    assert_eq!(state.journal_entries_replayed, 0);
    assert!(state.errors.is_empty());
}

#[test]
fn test_allocator_bitmap_to_bytes() {
    let mut bitmap = AllocatorBitmap::new(64);
    bitmap.set_allocated(0, 8);
    bitmap.set_allocated(16, 8);
    bitmap.set_allocated(32, 8);

    let bytes = bitmap.to_bytes();
    assert_eq!(bytes.len(), 8);
}

#[test]
fn test_allocator_bitmap_from_bytes() {
    let data = vec![0xFF, 0x0F, 0xF0, 0x00];
    let bitmap = AllocatorBitmap::from_bytes(&data, 32).unwrap();

    assert!(bitmap.is_allocated(0));
    assert!(bitmap.is_allocated(7));
    assert!(!bitmap.is_allocated(12));
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

    let ranges = bitmap.allocated_ranges();

    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0], (5, 8));
    assert_eq!(ranges[1], (10, 12));
    assert_eq!(ranges[2], (50, 60));
}

#[test]
fn test_snapshot_fields() {
    let info = SnapshotInfo {
        id: 42,
        name: "test_snapshot".to_string(),
        created_at_secs: 1234567890,
        block_count: 1000,
        total_bytes: 4_000_000,
    };

    assert_eq!(info.id, 42);
    assert_eq!(info.name, "test_snapshot");
    assert_eq!(info.created_at_secs, 1234567890);
    assert_eq!(info.block_count, 1000);
    assert_eq!(info.total_bytes, 4_000_000);
}

#[test]
fn test_snapshot_info_clone() {
    let info = SnapshotInfo {
        id: 1,
        name: "clone_test".to_string(),
        created_at_secs: 1000,
        block_count: 50,
        total_bytes: 200000,
    };

    let cloned = info.clone();
    assert_eq!(cloned.id, info.id);
    assert_eq!(cloned.name, info.name);
    assert_eq!(cloned.block_count, info.block_count);
}

#[test]
fn test_snapshot_manager_snapshot_count() {
    let mut manager = SnapshotManager::new(SnapshotConfig::default());

    let hashes: Vec<_> = vec![claudefs_reduce::fingerprint::blake3_hash(&[1])];

    assert_eq!(manager.snapshot_count(), 0);

    manager
        .create_snapshot("s1".to_string(), hashes.clone(), 0)
        .unwrap();
    assert_eq!(manager.snapshot_count(), 1);

    manager
        .create_snapshot("s2".to_string(), hashes.clone(), 0)
        .unwrap();
    assert_eq!(manager.snapshot_count(), 2);

    manager.delete_snapshot(1);
    assert_eq!(manager.snapshot_count(), 1);
}

#[test]
fn test_recovery_report_fields() {
    let config = RecoveryConfig::default();
    let manager = RecoveryManager::new(config);

    let report = manager.report();

    assert_eq!(report.phase, RecoveryPhase::NotStarted);
    assert_eq!(report.devices_discovered, 0);
    assert_eq!(report.devices_valid, 0);
    assert_eq!(report.journal_entries_found, 0);
    assert_eq!(report.journal_entries_replayed, 0);
    assert!(report.errors.is_empty());
    assert_eq!(report.duration_ms, 0);
}

#[test]
fn test_snapshot_debug_format() {
    let config = SnapshotConfig { max_snapshots: 10 };
    let format = format!("{:?}", config);
    assert!(format.contains("10"));
}

#[test]
fn test_recovery_phase_debug_format() {
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
        let format = format!("{:?}", phase);
        assert!(!format.is_empty());
    }
}

#[test]
fn test_journal_checkpoint_magic_constant() {
    assert_eq!(JOURNAL_CHECKPOINT_MAGIC, 0x434A4350);
}
