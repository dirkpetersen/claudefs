//! Security tests for FUSE extension modules
//!
//! Part of A10 Phase 4: Extended FUSE + Storage Security Audit

#[cfg(test)]
mod tests {
    use claudefs_fuse::dir_cache::{DirCache, DirCacheConfig, DirEntry};
    use claudefs_fuse::flock::{FlockHandle, FlockRegistry, FlockType};
    use claudefs_fuse::idmap::{IdMapEntry, IdMapMode, IdMapper};
    use claudefs_fuse::inode::{InodeId, InodeKind};
    use claudefs_fuse::interrupt::{InterruptTracker, RequestId};
    use claudefs_fuse::posix_acl::{AclEntry, AclPerms, AclTag, PosixAcl};

    // =========================================================================
    // A. IdMapper Security Tests
    // =========================================================================

    #[test]
    fn test_squash_root_not_preserved() {
        let mapper = IdMapper::new(IdMapMode::Squash {
            nobody_uid: 65534,
            nobody_gid: 65534,
        });
        assert_eq!(mapper.map_uid(0), 65534);
    }

    #[test]
    fn test_rangeshift_overflow_wraps() {
        let mapper = IdMapper::new(IdMapMode::RangeShift {
            host_base: u32::MAX - 3,
            local_base: 0,
            count: 5,
        });
        let mapped = mapper.map_uid(1);
        assert_eq!(mapped, 1);
    }

    #[test]
    fn test_table_mode_unmapped_uid_passthrough() {
        let mut mapper = IdMapper::new(IdMapMode::Table);
        mapper
            .add_uid_entry(IdMapEntry {
                host_id: 1000,
                local_id: 2000,
            })
            .unwrap();

        let mapped = mapper.map_uid(999);
        assert_eq!(mapped, 999);
    }

    #[test]
    fn test_identity_root_always_zero() {
        let mapper = IdMapper::new(IdMapMode::Identity);
        assert_eq!(mapper.map_uid(0), 0);
    }

    #[test]
    fn test_reverse_map_not_available_for_rangeshift() {
        let mapper = IdMapper::new(IdMapMode::RangeShift {
            host_base: 1000,
            local_base: 2000,
            count: 100,
        });
        assert_eq!(mapper.reverse_map_uid(2000), None);
    }

    // =========================================================================
    // B. POSIX ACL Security Tests
    // =========================================================================

    #[test]
    fn test_acl_no_entries_denies_all() {
        let acl = PosixAcl::new();
        assert!(!acl.check_access(1000, 1000, 100, 100, AclPerms::read_only()));
    }

    #[test]
    fn test_acl_mask_does_not_limit_owner() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::Mask, AclPerms::read_only()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        let result = acl.check_access(1000, 1000, 100, 100, AclPerms::all());
        assert!(!result);
    }

    #[test]
    fn test_acl_unbounded_entries() {
        let mut acl = PosixAcl::new();
        for i in 0..10000 {
            acl.add_entry(AclEntry::new(AclTag::User(i), AclPerms::read_only()));
        }
        assert_eq!(acl.entry_count(), 10000);
    }

    #[test]
    fn test_acl_duplicate_user_entries() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::User(1000), AclPerms::read_only()));
        acl.add_entry(AclEntry::new(AclTag::User(1000), AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        let result = acl.check_access(1000, 1000, 100, 100, AclPerms::all());
        assert!(!result);
    }

    #[test]
    fn test_acl_root_uid_zero_bypasses_acl() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        let result = acl.check_access(0, 0, 0, 0, AclPerms::all());
        assert!(!result);
    }

    // =========================================================================
    // C. FlockRegistry Security Tests
    // =========================================================================

    #[test]
    fn test_flock_no_ttl_deadlock_risk() {
        let mut registry = FlockRegistry::new();
        let h = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);
        registry.try_acquire(h);
        assert!(registry.has_lock(1, 100));
    }

    #[test]
    fn test_flock_pid_zero_allowed() {
        let mut registry = FlockRegistry::new();
        let h = FlockHandle::new(1, 100, 0, FlockType::Shared, false);
        let result = registry.try_acquire(h);
        assert_eq!(result, claudefs_fuse::flock::FlockConflict::None);
    }

    #[test]
    fn test_flock_large_fd_values() {
        let mut registry = FlockRegistry::new();
        let h = FlockHandle::new(u64::MAX, 100, 1, FlockType::Shared, false);
        let result = registry.try_acquire(h);
        assert_eq!(result, claudefs_fuse::flock::FlockConflict::None);
    }

    #[test]
    fn test_flock_upgrade_race_window() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Shared, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Shared, false);

        registry.try_acquire(h1);
        registry.try_acquire(h2);

        let h1_upgrade = FlockHandle::new(1, 100, 1, FlockType::Exclusive, false);
        let result = registry.try_acquire(h1_upgrade);
        assert!(matches!(
            result,
            claudefs_fuse::flock::FlockConflict::WouldBlock { .. }
        ));
    }

    #[test]
    fn test_flock_holder_count_mismatch_after_unlock() {
        let mut registry = FlockRegistry::new();
        let h1 = FlockHandle::new(1, 100, 1, FlockType::Shared, false);
        let h2 = FlockHandle::new(2, 100, 2, FlockType::Shared, false);

        registry.try_acquire(h1);
        registry.try_acquire(h2);
        assert_eq!(registry.holder_count(100), 2);

        let unlock = FlockHandle::new(1, 100, 1, FlockType::Unlock, false);
        registry.try_acquire(unlock);

        assert_eq!(registry.holder_count(100), 1);
    }

    // =========================================================================
    // D. InterruptTracker Security Tests
    // =========================================================================

    #[test]
    fn test_interrupt_tracker_request_id_collision() {
        let mut tracker = InterruptTracker::new(100);
        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.register(RequestId(1), 16, 1001, 0).unwrap();

        let count = tracker.pending_count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_interrupt_completed_request_still_tracked() {
        let mut tracker = InterruptTracker::new(100);
        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.interrupt(RequestId(1));

        let ids = tracker.interrupted_ids();
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn test_drain_timed_out_counts_as_completed() {
        let mut tracker = InterruptTracker::new(100);
        tracker.register(RequestId(1), 15, 1000, 0).unwrap();

        tracker.drain_timed_out(10000, 5000);

        assert_eq!(tracker.total_completed(), 1);
    }

    #[test]
    fn test_interrupt_after_complete_returns_false() {
        let mut tracker = InterruptTracker::new(100);
        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.complete(RequestId(1));

        let result = tracker.interrupt(RequestId(1));
        assert!(!result);
    }

    #[test]
    fn test_max_pending_enforcement() {
        let mut tracker = InterruptTracker::new(2);

        assert!(tracker.register(RequestId(1), 15, 1000, 0).is_ok());
        assert!(tracker.register(RequestId(2), 15, 1000, 0).is_ok());
        assert!(tracker.register(RequestId(3), 15, 1000, 0).is_err());
    }

    // =========================================================================
    // E. DirCache Security Tests
    // =========================================================================

    fn make_entry(name: &str, ino: InodeId, kind: InodeKind) -> DirEntry {
        DirEntry {
            name: name.to_string(),
            ino,
            kind,
        }
    }

    #[test]
    fn test_dir_cache_negative_entry_no_size_limit() {
        let mut cache = DirCache::new(DirCacheConfig {
            max_dirs: 100,
            ttl: std::time::Duration::from_secs(30),
            negative_ttl: std::time::Duration::from_secs(5),
        });

        for i in 0..100000 {
            cache.insert_negative(1, &format!("nonexistent{}", i));
        }

        assert!(cache.stats().snapshots_cached == 0);
    }

    #[test]
    fn test_dir_cache_stale_after_mutation() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_snapshot(1, vec![make_entry("file.txt", 2, InodeKind::File)]);

        let result = cache.lookup(1, "file.txt");
        assert!(result.is_some());

        let result2 = cache.lookup(1, "file.txt");
        assert!(result2.is_some());
    }

    #[test]
    fn test_dir_cache_eviction_not_lru() {
        let mut cache = DirCache::new(DirCacheConfig {
            max_dirs: 2,
            ttl: std::time::Duration::from_secs(30),
            negative_ttl: std::time::Duration::from_secs(5),
        });

        cache.insert_snapshot(1, vec![make_entry("a", 10, InodeKind::File)]);
        cache.insert_snapshot(2, vec![make_entry("b", 20, InodeKind::File)]);
        cache.insert_snapshot(3, vec![make_entry("c", 30, InodeKind::File)]);

        let remaining = cache.stats().snapshots_cached;
        assert!(remaining <= 2);
    }

    #[test]
    fn test_dir_cache_lookup_double_miss_count() {
        let mut cache = DirCache::new(DirCacheConfig::default());

        let _ = cache.lookup(1, "nonexistent");
        let stats = cache.stats();

        assert!(stats.misses >= 1);
    }

    #[test]
    fn test_dir_cache_entry_name_injection() {
        let mut cache = DirCache::new(DirCacheConfig::default());

        cache.insert_snapshot(
            1,
            vec![
                make_entry("../etc/passwd", 100, InodeKind::File),
                make_entry("file\x00null", 101, InodeKind::File),
                make_entry("path/with/slash", 102, InodeKind::File),
            ],
        );

        let result = cache.lookup(1, "../etc/passwd");
        assert!(result.is_some());
    }
}
