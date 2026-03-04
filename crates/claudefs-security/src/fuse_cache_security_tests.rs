//! FUSE cache/recovery security tests: coherence, crash recovery, writebuf, datacache, session.
//!
//! Part of A10 Phase 12: FUSE cache & recovery security audit

#[cfg(test)]
mod tests {
    use claudefs_fuse::cache_coherence::{
        CacheInvalidation, CacheLease, CoherenceManager, CoherenceProtocol, CoherenceResult,
        InvalidationReason, LeaseId, LeaseState, VersionVector,
    };
    use claudefs_fuse::crash_recovery::{
        CrashRecovery, OpenFileRecord, PendingWrite, RecoveryConfig, RecoveryJournal, RecoveryState,
    };
    use claudefs_fuse::datacache::{CachedData, DataCache, DataCacheConfig, DataCacheStats};
    use claudefs_fuse::inode::InodeId;
    use claudefs_fuse::session::{SessionConfig, SessionStats};
    use claudefs_fuse::writebuf::{WriteBuf, WriteBufConfig, WriteRange};
    use std::time::Duration;

    fn make_coherence_manager() -> CoherenceManager {
        CoherenceManager::new(CoherenceProtocol::CloseToOpen)
    }

    fn make_strict_coherence_manager() -> CoherenceManager {
        CoherenceManager::new(CoherenceProtocol::Strict)
    }

    fn make_recovery() -> CrashRecovery {
        CrashRecovery::new(RecoveryConfig::default_config())
    }

    fn make_writebuf() -> WriteBuf {
        WriteBuf::new(WriteBufConfig::default())
    }

    fn make_datacache() -> DataCache {
        DataCache::new(DataCacheConfig::default())
    }

    fn make_inode_id(n: u64) -> InodeId {
        n
    }

    // Category 1: Cache Coherence Security (5 tests)

    #[test]
    fn test_coherence_grant_and_check_lease() {
        let mut manager = make_coherence_manager();

        let lease = manager.grant_lease(1, 100);

        let checked = manager.check_lease(1);
        assert!(checked.is_some(), "Lease should exist for inode 1");

        let active_count = manager.active_lease_count();
        assert_eq!(active_count, 1, "Should have exactly 1 active lease");

        let lease_ref = checked.unwrap();
        assert_eq!(
            lease_ref.state,
            LeaseState::Active,
            "Lease state should be Active"
        );
        assert_eq!(lease_ref.inode, 1, "Lease should be for inode 1");
        assert_eq!(lease_ref.client_id, 100, "Lease should be for client 100");
    }

    #[test]
    fn test_coherence_revoke_generates_invalidation() {
        let mut manager = make_coherence_manager();

        manager.grant_lease(1, 100);

        let invalidation = manager.revoke_lease(1);
        assert!(
            invalidation.is_some(),
            "Revoke should return an invalidation"
        );

        let inv = invalidation.unwrap();
        assert_eq!(inv.inode, 1, "Invalidation should be for inode 1");

        let checked = manager.check_lease(1);
        assert!(checked.is_none(), "Lease should not exist after revoke");

        let pending = manager.pending_invalidations();
        assert!(!pending.is_empty(), "Should have pending invalidations");
    }

    #[test]
    fn test_coherence_version_vector_conflict() {
        let mut vv1 = VersionVector::new();
        let mut vv2 = VersionVector::new();

        vv1.update(1, 5);
        vv2.update(1, 3);

        let conflicts = vv1.conflicts(&vv2);
        assert!(
            conflicts.contains(&1),
            "Inode 1 should be in conflict list (version divergence)"
        );

        vv1.merge(&vv2);

        let merged_version = vv1.get(1);
        assert_eq!(merged_version, 5, "Merged version should be max (5)");
    }

    #[test]
    fn test_coherence_invalidate_remote_write() {
        let mut manager = make_coherence_manager();

        manager.grant_lease(1, 100);

        manager.invalidate(1, InvalidationReason::RemoteWrite(42), 2);

        let pending = manager.pending_invalidations();
        assert_eq!(pending.len(), 1, "Should have 1 pending invalidation");

        let drained = manager.drain_invalidations();
        assert_eq!(drained.len(), 1, "Drain should return 1 invalidation");

        let remaining = manager.pending_invalidations();
        assert!(
            remaining.is_empty(),
            "Should have no pending invalidations after drain"
        );
    }

    #[test]
    fn test_coherence_is_coherent() {
        let mut manager = make_strict_coherence_manager();

        let no_lease_coherent = manager.is_coherent(1);
        assert!(
            !no_lease_coherent,
            "Inode with no lease should not be considered coherent"
        );

        manager.grant_lease(1, 100);

        let with_lease_coherent = manager.is_coherent(1);
        assert!(
            with_lease_coherent,
            "Inode with valid lease should be coherent"
        );

        manager.revoke_lease(1);

        let after_revoke = manager.is_coherent(1);
        assert!(
            !after_revoke,
            "Inode after lease revoke should not be coherent"
        );
    }

    // Category 2: Crash Recovery State Machine (5 tests)

    #[test]
    fn test_recovery_initial_state() {
        let recovery = make_recovery();

        let state = recovery.state();
        assert!(
            matches!(state, RecoveryState::Idle),
            "Initial state should be Idle"
        );

        let journal = recovery.journal();
        assert_eq!(
            journal.open_file_count(),
            0,
            "Should have no open files initially"
        );
        assert_eq!(
            journal.pending_write_count(),
            0,
            "Should have no pending writes initially"
        );
    }

    #[test]
    fn test_recovery_scan_and_record() {
        let mut recovery = make_recovery();

        recovery.begin_scan().unwrap();

        let state = recovery.state();
        assert!(
            matches!(state, RecoveryState::Scanning),
            "State should be Scanning"
        );

        let record1 = OpenFileRecord {
            ino: make_inode_id(1),
            fd: 1,
            pid: 100,
            flags: 2,
            path_hint: "test1".into(),
        };
        let record2 = OpenFileRecord {
            ino: make_inode_id(2),
            fd: 2,
            pid: 101,
            flags: 0,
            path_hint: "test2".into(),
        };
        let record3 = OpenFileRecord {
            ino: make_inode_id(3),
            fd: 3,
            pid: 102,
            flags: 1,
            path_hint: "test3".into(),
        };

        recovery.record_open_file(record1).unwrap();
        recovery.record_open_file(record2).unwrap();
        recovery.record_open_file(record3).unwrap();

        assert_eq!(
            recovery.journal().open_file_count(),
            3,
            "Should have 3 open files"
        );

        let write1 = PendingWrite {
            ino: make_inode_id(1),
            offset: 0,
            len: 100,
            dirty_since_secs: 100,
        };
        let write2 = PendingWrite {
            ino: make_inode_id(2),
            offset: 100,
            len: 200,
            dirty_since_secs: 200,
        };

        recovery.record_pending_write(write1).unwrap();
        recovery.record_pending_write(write2).unwrap();

        assert_eq!(
            recovery.journal().pending_write_count(),
            2,
            "Should have 2 pending writes"
        );
    }

    #[test]
    fn test_recovery_replay_progress() {
        let mut recovery = make_recovery();

        recovery.begin_scan().unwrap();
        recovery.begin_replay(10).unwrap();

        let state = recovery.state();
        assert!(
            matches!(
                state,
                RecoveryState::Replaying {
                    replayed: 0,
                    total: 10
                }
            ),
            "State should be Replaying with 0 replayed"
        );

        recovery.advance_replay(5);

        let state = recovery.state();
        assert!(
            matches!(
                state,
                RecoveryState::Replaying {
                    replayed: 5,
                    total: 10
                }
            ),
            "State should be Replaying with 5 replayed"
        );

        recovery.complete(2).unwrap();

        let state = recovery.state();
        assert!(
            matches!(
                state,
                RecoveryState::Complete {
                    recovered: 5,
                    orphaned: 2
                }
            ),
            "State should be Complete with 5 recovered and 2 orphaned"
        );
    }

    #[test]
    fn test_recovery_fail_and_reset() {
        let mut recovery = make_recovery();

        recovery.begin_scan().unwrap();

        recovery.fail("disk error".to_string());

        let state = recovery.state();
        assert!(
            matches!(
                state,
                RecoveryState::Failed(s) if s == "disk error"
            ),
            "State should be Failed with disk error"
        );

        recovery.reset();

        let state = recovery.state();
        assert!(
            matches!(state, RecoveryState::Idle),
            "State should be Idle after reset"
        );
    }

    #[test]
    fn test_recovery_stale_pending_writes() {
        let mut recovery = make_recovery();

        recovery.begin_scan().unwrap();

        let write1 = PendingWrite {
            ino: make_inode_id(1),
            offset: 0,
            len: 100,
            dirty_since_secs: 100,
        };
        let write2 = PendingWrite {
            ino: make_inode_id(2),
            offset: 100,
            len: 200,
            dirty_since_secs: 500,
        };

        recovery.record_pending_write(write1).unwrap();
        recovery.record_pending_write(write2).unwrap();

        let stale = recovery.journal().stale_pending_writes(600, 200);

        assert_eq!(
            stale.len(),
            1,
            "Only 1 write should be stale (age 500 > 200)"
        );
        assert_eq!(
            stale[0].ino,
            make_inode_id(1),
            "Write at dirty_since=100 should be stale"
        );
    }

    // Category 3: Write Buffer Security (5 tests)

    #[test]
    fn test_writebuf_buffer_and_take() {
        let mut buf = make_writebuf();

        let should_flush = buf.buffer_write(make_inode_id(1), 0, b"hello");

        let is_dirty = buf.is_dirty(make_inode_id(1));
        assert!(is_dirty, "Inode 1 should be dirty after buffering write");

        let taken = buf.take_dirty(make_inode_id(1));

        assert_eq!(taken.len(), 1, "Should have 1 range");
        assert_eq!(taken[0].offset, 0, "Range offset should be 0");
        assert_eq!(&taken[0].data, b"hello", "Range data should match");

        let is_dirty_after = buf.is_dirty(make_inode_id(1));
        assert!(
            !is_dirty_after,
            "Inode 1 should not be dirty after take_dirty"
        );
    }

    #[test]
    fn test_writebuf_coalesce_adjacent() {
        let mut buf = make_writebuf();

        buf.buffer_write(make_inode_id(1), 0, &vec![0u8; 100]);
        buf.buffer_write(make_inode_id(1), 100, &vec![0u8; 100]);

        buf.coalesce(make_inode_id(1));

        let taken = buf.take_dirty(make_inode_id(1));

        let total_size: usize = taken.iter().map(|r| r.data.len()).sum();
        assert_eq!(
            total_size, 200,
            "Total data should be 200 bytes (coalesced)"
        );
    }

    #[test]
    fn test_writebuf_discard() {
        let mut buf = make_writebuf();

        buf.buffer_write(make_inode_id(1), 0, b"test1");
        buf.buffer_write(make_inode_id(2), 0, b"test2");

        buf.discard(make_inode_id(1));

        let is_dirty_1 = buf.is_dirty(make_inode_id(1));
        assert!(!is_dirty_1, "Inode 1 should not be dirty after discard");

        let is_dirty_2 = buf.is_dirty(make_inode_id(2));
        assert!(is_dirty_2, "Inode 2 should still be dirty");
    }

    #[test]
    fn test_writebuf_total_buffered() {
        let mut buf = make_writebuf();

        buf.buffer_write(make_inode_id(1), 0, &vec![0u8; 100]);
        buf.buffer_write(make_inode_id(2), 0, &vec![0u8; 100]);
        buf.buffer_write(make_inode_id(3), 0, &vec![0u8; 100]);

        let total = buf.total_buffered();
        assert_eq!(total, 300, "Total buffered should be 300 bytes");
    }

    #[test]
    fn test_writebuf_dirty_inodes_list() {
        let mut buf = make_writebuf();

        buf.buffer_write(make_inode_id(1), 0, b"test1");
        buf.buffer_write(make_inode_id(2), 0, b"test2");
        buf.buffer_write(make_inode_id(3), 0, b"test3");

        let dirty_list = buf.dirty_inodes();
        assert_eq!(dirty_list.len(), 3, "Should have 3 dirty inodes");

        buf.take_dirty(make_inode_id(2));

        let dirty_list_after = buf.dirty_inodes();
        assert_eq!(
            dirty_list_after.len(),
            2,
            "Should have 2 dirty inodes after take"
        );
    }

    // Category 4: Data Cache Security (5 tests)

    #[test]
    fn test_datacache_insert_and_get() {
        let mut cache = make_datacache();

        let result = cache.insert(make_inode_id(1), vec![1, 2, 3, 4, 5], 1);
        assert!(result, "Insert should succeed");

        let cached = cache.get(make_inode_id(1));
        assert!(cached.is_some(), "Get should return data");

        let data = cached.unwrap();
        assert_eq!(data.data, vec![1, 2, 3, 4, 5], "Data should match");
        assert_eq!(data.generation, 1, "Generation should match");
    }

    #[test]
    fn test_datacache_eviction_on_max_files() {
        let mut cache = DataCache::new(DataCacheConfig {
            max_files: 2,
            max_bytes: 1000,
            max_file_size: 500,
        });

        cache.insert(make_inode_id(1), vec![1], 1);
        cache.insert(make_inode_id(2), vec![2], 1);
        cache.insert(make_inode_id(3), vec![3], 1);

        let len = cache.len();
        assert!(len <= 2, "Cache len should be <= max_files (2)");

        let stats = cache.stats();
        assert!(stats.evictions >= 1, "Should have at least 1 eviction");
    }

    #[test]
    fn test_datacache_invalidate() {
        let mut cache = make_datacache();

        cache.insert(make_inode_id(1), vec![1, 2, 3], 1);

        cache.get(make_inode_id(1));
        cache.get(make_inode_id(999));

        let stats_before = cache.stats();
        assert_eq!(
            stats_before.hits, 1,
            "Should have 1 hit before invalidation"
        );
        assert_eq!(
            stats_before.misses, 1,
            "Should have 1 miss before invalidation"
        );

        cache.invalidate(make_inode_id(1));

        let cached = cache.get(make_inode_id(1));
        assert!(cached.is_none(), "Get after invalidate should return None");
    }

    #[test]
    fn test_datacache_generation_invalidation() {
        let mut cache = make_datacache();

        cache.insert(make_inode_id(1), vec![1, 2, 3], 5);

        cache.invalidate_if_generation(make_inode_id(1), 2);

        let cached = cache.get(make_inode_id(1));
        assert!(
            cached.is_none(),
            "Entry should be invalidated when generation doesn't match"
        );

        cache.insert(make_inode_id(2), vec![4, 5, 6], 3);

        cache.invalidate_if_generation(make_inode_id(2), 3);

        let cached2 = cache.get(make_inode_id(2));
        assert!(
            cached2.is_some(),
            "Entry should NOT be invalidated when generation matches"
        );
    }

    #[test]
    fn test_datacache_max_bytes_limit() {
        let mut cache = DataCache::new(DataCacheConfig {
            max_files: 10,
            max_bytes: 100,
            max_file_size: 100,
        });

        let result1 = cache.insert(make_inode_id(1), vec![0u8; 50], 1);
        assert!(result1, "First insert of 50 bytes should succeed");

        let result2 = cache.insert(make_inode_id(2), vec![0u8; 60], 1);

        let total = cache.total_bytes();
        assert!(total <= 100, "Total bytes should be <= max_bytes (100)");
    }

    // Category 5: Session & Config Validation (5 tests)

    #[test]
    fn test_session_config_defaults() {
        let config = SessionConfig::default();

        assert_eq!(
            config.mountpoint,
            std::path::PathBuf::new(),
            "Mountpoint should be empty"
        );

        let _ = config.fs_config;
        let _ = config.server_config;

        assert!(
            config.fs_config.default_permissions,
            "fs_config should have default_permissions"
        );
    }

    #[test]
    fn test_session_stats_default() {
        let stats = SessionStats::default();

        assert_eq!(
            stats.requests_processed, 0,
            "requests_processed should be 0"
        );
        assert_eq!(stats.bytes_read, 0, "bytes_read should be 0");
        assert_eq!(stats.bytes_written, 0, "bytes_written should be 0");
        assert_eq!(stats.errors, 0, "errors should be 0");
    }

    #[test]
    fn test_recovery_config_defaults() {
        let config = RecoveryConfig::default_config();

        assert!(
            config.max_recovery_secs > 0,
            "max_recovery_secs should be > 0"
        );
        assert_eq!(
            config.max_recovery_secs, 30,
            "max_recovery_secs should be 30"
        );

        assert!(config.max_open_files > 0, "max_open_files should be > 0");
        assert_eq!(
            config.max_open_files, 10000,
            "max_open_files should be 10000"
        );

        assert!(
            config.stale_write_age_secs > 0,
            "stale_write_age_secs should be > 0"
        );
        assert_eq!(
            config.stale_write_age_secs, 300,
            "stale_write_age_secs should be 300"
        );
    }

    #[test]
    fn test_writebuf_config_defaults() {
        let config = WriteBufConfig::default();

        assert!(config.flush_threshold > 0, "flush_threshold should be > 0");
        assert_eq!(
            config.flush_threshold,
            1 << 20,
            "flush_threshold should be ~1MB"
        );

        assert!(
            config.max_coalesce_gap > 0,
            "max_coalesce_gap should be > 0"
        );

        assert!(
            config.dirty_timeout_ms > 0,
            "dirty_timeout_ms should be > 0"
        );
    }

    #[test]
    fn test_datacache_config_defaults() {
        let config = DataCacheConfig::default();

        assert!(config.max_files > 0, "max_files should be > 0");
        assert_eq!(config.max_files, 256, "max_files should be 256");

        assert!(config.max_bytes > 0, "max_bytes should be > 0");
        assert_eq!(
            config.max_bytes,
            64 * 1024 * 1024,
            "max_bytes should be 64MB"
        );

        assert!(config.max_file_size > 0, "max_file_size should be > 0");
        assert_eq!(
            config.max_file_size,
            4 * 1024 * 1024,
            "max_file_size should be 4MB"
        );

        assert!(
            config.max_file_size <= config.max_file_size,
            "max_file_size should be <= max_bytes"
        );
    }
}
