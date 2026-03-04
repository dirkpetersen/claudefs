//! Gateway NFS copy offload and referral/migration security tests.
//!
//! Part of A10 Phase 22: Gateway copy-offload/referral security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::nfs_copy_offload::{
        AsyncCopyHandle, CloneRequest, CloneResult, CopyOffloadError, CopyOffloadManager,
        CopyResult, CopySegment, CopyState, WriteStable,
    };
    use claudefs_gateway::nfs_referral::{
        FsLocation, FsLocations, FsServer, ReferralDatabase, ReferralEntry, ReferralError,
        ReferralSerializer, ReferralTarget, ReferralType,
    };

    // ============================================================================
    // Category 1: Copy Offload Manager Lifecycle (5 tests)
    // ============================================================================

    #[test]
    fn test_copy_start_and_poll() {
        let mut manager = CopyOffloadManager::new(5);
        let segments = vec![CopySegment::new(0, 0, 4096)];

        let copy_id = manager
            .start_copy("/src/file", "/dst/file", segments)
            .unwrap();

        assert!(copy_id > 0, "copy_id should be greater than 0");

        let handle = manager.poll_copy(copy_id).expect("copy should exist");
        assert_eq!(handle.state, CopyState::InProgress);
        assert_eq!(handle.src_file, "/src/file");
        assert_eq!(handle.dst_file, "/dst/file");
    }

    #[test]
    fn test_copy_concurrent_limit() {
        let mut manager = CopyOffloadManager::new(2);

        let id1 = manager
            .start_copy("/src1", "/dst1", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();
        let id2 = manager
            .start_copy("/src2", "/dst2", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();

        assert!(manager.poll_copy(id1).is_some());
        assert!(manager.poll_copy(id2).is_some());

        let result = manager.start_copy("/src3", "/dst3", vec![CopySegment::new(0, 0, 1000)]);
        assert!(matches!(result, Err(CopyOffloadError::LimitExceeded(_))));

        // FINDING-GW-COPY-01: Concurrent copy limit prevents resource exhaustion
        manager.complete_copy(id1, 1000).unwrap();

        let id3 = manager
            .start_copy("/src3", "/dst3", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();
        assert!(manager.poll_copy(id3).is_some());
    }

    #[test]
    fn test_copy_complete_lifecycle() {
        let mut manager = CopyOffloadManager::new(5);
        let id = manager
            .start_copy("/src", "/dst", vec![CopySegment::new(0, 0, 4096)])
            .unwrap();

        manager.complete_copy(id, 4096).unwrap();

        let handle = manager.poll_copy(id).unwrap();
        assert_eq!(handle.state, CopyState::Completed);
        assert_eq!(handle.bytes_copied, 4096);

        // FINDING-GW-COPY-02: Double-complete prevented
        let result = manager.complete_copy(id, 4096);
        assert!(matches!(result, Err(CopyOffloadError::AlreadyComplete(_))));
    }

    #[test]
    fn test_copy_fail_lifecycle() {
        let mut manager = CopyOffloadManager::new(5);
        let id = manager
            .start_copy("/src", "/dst", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();

        manager.fail_copy(id).unwrap();

        let handle = manager.poll_copy(id).unwrap();
        assert_eq!(handle.state, CopyState::Failed);

        let result = manager.fail_copy(id);
        assert!(matches!(result, Err(CopyOffloadError::AlreadyComplete(_))));

        let result = manager.complete_copy(id, 1000);
        assert!(matches!(result, Err(CopyOffloadError::AlreadyComplete(_))));
    }

    #[test]
    fn test_copy_cancel() {
        let mut manager = CopyOffloadManager::new(5);
        let id = manager
            .start_copy("/src", "/dst", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();

        let cancelled = manager.cancel_copy(id);
        assert!(cancelled, "cancel should return true for in-progress copy");

        let handle = manager.poll_copy(id).unwrap();
        assert_eq!(handle.state, CopyState::Cancelled);

        let result = manager.cancel_copy(999);
        assert!(!result, "cancel non-existent should return false");

        let result = manager.cancel_copy(id);
        assert!(!result, "cancel already-cancelled should return false");
    }

    // ============================================================================
    // Category 2: Copy Progress & Cleanup (5 tests)
    // ============================================================================

    #[test]
    fn test_copy_progress_percent() {
        let mut handle =
            AsyncCopyHandle::new(1, "src".to_string(), "dst".to_string(), vec![], 1000);

        assert_eq!(handle.progress_percent(), 0.0);

        handle.bytes_copied = 500;
        assert_eq!(handle.progress_percent(), 50.0);

        handle.bytes_copied = 1000;
        assert_eq!(handle.progress_percent(), 100.0);
    }

    #[test]
    fn test_copy_progress_zero_total() {
        let handle = AsyncCopyHandle::new(1, "src".to_string(), "dst".to_string(), vec![], 0);

        // FINDING-GW-COPY-03: Zero-total-bytes produces 100% — no division by zero
        assert_eq!(handle.progress_percent(), 100.0);
    }

    #[test]
    fn test_copy_purge_finished() {
        let mut manager = CopyOffloadManager::new(5);

        let id1 = manager
            .start_copy("/src1", "/dst1", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();
        let id2 = manager
            .start_copy("/src2", "/dst2", vec![CopySegment::new(0, 0, 2000)])
            .unwrap();
        let id3 = manager
            .start_copy("/src3", "/dst3", vec![CopySegment::new(0, 0, 3000)])
            .unwrap();

        manager.complete_copy(id1, 1000).unwrap();
        manager.fail_copy(id2).unwrap();
        assert!(manager.cancel_copy(id3));

        assert_eq!(manager.active_count(), 0);

        let purged = manager.purge_finished();
        assert_eq!(purged, 3);
        assert_eq!(manager.total_handles(), 0);
    }

    #[test]
    fn test_copy_nonexistent_operations() {
        let mut manager = CopyOffloadManager::new(5);

        let result = manager.complete_copy(999, 0);
        assert!(matches!(result, Err(CopyOffloadError::NotFound(_))));

        let result = manager.fail_copy(999);
        assert!(matches!(result, Err(CopyOffloadError::NotFound(_))));

        let result = manager.poll_copy(999);
        assert!(result.is_none());
    }

    #[test]
    fn test_copy_segment_validation() {
        let segment = CopySegment::new(0, 0, 4096);
        assert!(segment.is_valid(), "normal segment should be valid");

        let zero_segment = CopySegment::new(0, 0, 0);
        assert!(zero_segment.is_valid(), "zero count means to-end-of-file");

        let segments = vec![
            CopySegment::new(0, 0, 4096),
            CopySegment::new(4096, 4096, 4096),
        ];

        let mut manager = CopyOffloadManager::new(5);
        let id = manager.start_copy("/src", "/dst", segments).unwrap();

        let handle = manager.poll_copy(id).unwrap();
        assert_eq!(handle.segments.len(), 2);
    }

    // ============================================================================
    // Category 3: Clone & Copy Result (3 tests)
    // ============================================================================

    #[test]
    fn test_clone_request_builder() {
        let req = CloneRequest::new("/src/file".to_string(), "/dst/file".to_string());

        assert_eq!(req.src_offset, 0);
        assert_eq!(req.dst_offset, 0);
        assert_eq!(req.length, 0);

        let req = req
            .with_src_offset(4096)
            .with_dst_offset(8192)
            .with_length(1024);

        assert_eq!(req.src_offset, 4096);
        assert_eq!(req.dst_offset, 8192);
        assert_eq!(req.length, 1024);
    }

    #[test]
    fn test_copy_result_stability() {
        let result = CopyResult::new(4096, true, WriteStable::FileSync);
        assert_eq!(result.bytes_written, 4096);
        assert!(result.consecutive);
        assert!(matches!(result.stable, WriteStable::FileSync));

        let result = CopyResult::new(1024, false, WriteStable::Unstable);
        assert!(matches!(result.stable, WriteStable::Unstable));

        let result = CopyResult::new(2048, true, WriteStable::DataSync);
        assert!(matches!(result.stable, WriteStable::DataSync));
    }

    #[test]
    fn test_clone_result() {
        let result = CloneResult::new("/src/file".to_string(), "/dst/file".to_string(), 8192);

        assert_eq!(result.src_file, "/src/file");
        assert_eq!(result.dst_file, "/dst/file");
        assert_eq!(result.cloned_bytes, 8192);
    }

    // ============================================================================
    // Category 4: Referral Target & Entry Validation (5 tests)
    // ============================================================================

    #[test]
    fn test_referral_target_validation() {
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        assert!(target.validate().is_ok());

        let target = ReferralTarget::new("".to_string(), 2049, "/exports/data".to_string());
        let result = target.validate();
        assert!(matches!(result, Err(ReferralError::InvalidTarget(_))));

        let target = ReferralTarget::new(
            "server/with/slash".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let result = target.validate();
        assert!(matches!(result, Err(ReferralError::InvalidTarget(_))));

        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            0,
            "/exports/data".to_string(),
        );
        let result = target.validate();
        assert!(matches!(result, Err(ReferralError::InvalidTarget(_))));
    }

    #[test]
    fn test_referral_entry_validation() {
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        assert!(entry.validate().is_ok());

        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("data".to_string(), vec![target], ReferralType::Referral);
        let result = entry.validate();
        assert!(matches!(result, Err(ReferralError::InvalidPath(_))));

        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new(
            "/data//subdir".to_string(),
            vec![target],
            ReferralType::Referral,
        );
        let result = entry.validate();
        assert!(matches!(result, Err(ReferralError::InvalidPath(_))));

        let entry = ReferralEntry::new("/data".to_string(), vec![], ReferralType::Referral);
        let result = entry.validate();
        assert!(matches!(result, Err(ReferralError::EmptyTargets)));
    }

    #[test]
    fn test_referral_type_default() {
        assert_eq!(ReferralType::default(), ReferralType::Referral);

        let _ = ReferralType::Referral;
        let _ = ReferralType::Migration;
        let _ = ReferralType::Replication;
    }

    #[test]
    fn test_referral_database_add_and_lookup() {
        let mut db = ReferralDatabase::new();

        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);

        let result = db.add_referral(entry);
        assert!(result.is_ok());
        assert_eq!(db.referral_count(), 1);

        let lookup = db.lookup("/data");
        assert!(lookup.is_some());
        assert_eq!(lookup.unwrap().local_path, "/data");

        let lookup = db.lookup("/nonexistent");
        assert!(lookup.is_none());
    }

    #[test]
    fn test_referral_duplicate_rejected() {
        let mut db = ReferralDatabase::new();

        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);

        db.add_referral(entry.clone()).unwrap();

        let result = db.add_referral(entry);
        assert!(matches!(result, Err(ReferralError::DuplicatePath(_))));

        assert_eq!(db.referral_count(), 1);
    }

    // ============================================================================
    // Category 5: Referral Database Operations (7 tests)
    // ============================================================================

    #[test]
    fn test_referral_prefix_lookup() {
        let mut db = ReferralDatabase::new();

        let target1 = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry1 = ReferralEntry::new("/data".to_string(), vec![target1], ReferralType::Referral);
        db.add_referral(entry1).unwrap();

        let target2 = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/data/sub".to_string(),
        );
        let entry2 = ReferralEntry::new(
            "/data/sub".to_string(),
            vec![target2],
            ReferralType::Migration,
        );
        db.add_referral(entry2).unwrap();

        let result = db.lookup_by_prefix("/data/sub/file.txt");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/data/sub");

        let result = db.lookup_by_prefix("/other");
        assert!(result.is_none());
    }

    #[test]
    fn test_referral_enable_disable() {
        let mut db = ReferralDatabase::new();

        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();

        assert!(db.disable_referral("/data"));
        let lookup = db.lookup("/data").unwrap();
        assert!(!lookup.enabled);

        assert!(db.enable_referral("/data"));
        let lookup = db.lookup("/data").unwrap();
        assert!(lookup.enabled);

        assert!(!db.enable_referral("/nonexistent"));
        assert!(!db.disable_referral("/nonexistent"));
    }

    #[test]
    fn test_referral_remove() {
        let mut db = ReferralDatabase::new();

        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();

        assert!(db.remove_referral("/data"));
        assert_eq!(db.referral_count(), 0);

        assert!(!db.remove_referral("/nonexistent"));
    }

    #[test]
    fn test_referral_list() {
        let mut db = ReferralDatabase::new();

        let target1 = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data1".to_string(),
        );
        let entry1 =
            ReferralEntry::new("/data1".to_string(), vec![target1], ReferralType::Referral);
        db.add_referral(entry1).unwrap();

        let target2 = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/data2".to_string(),
        );
        let entry2 =
            ReferralEntry::new("/data2".to_string(), vec![target2], ReferralType::Migration);
        db.add_referral(entry2).unwrap();

        let target3 = ReferralTarget::new(
            "server3.example.com".to_string(),
            2049,
            "/exports/data3".to_string(),
        );
        let entry3 = ReferralEntry::new(
            "/data3".to_string(),
            vec![target3],
            ReferralType::Replication,
        );
        db.add_referral(entry3).unwrap();

        assert_eq!(db.list_referrals().len(), 3);
    }

    #[test]
    fn test_referral_serializer_to_fs_locations() {
        let serializer = ReferralSerializer::new();

        let target = ReferralTarget::new("server1".to_string(), 2049, "/exports/data".to_string());
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);

        let fs_locations = serializer.to_fs_locations(&entry);

        assert_eq!(fs_locations.root, vec!["data"]);
        assert_eq!(fs_locations.locations.len(), 1);
        assert_eq!(fs_locations.locations[0].servers[0].server, "server1");
        assert_eq!(fs_locations.locations[0].rootpath, vec!["exports", "data"]);
    }

    #[test]
    fn test_referral_serializer_multiple_targets() {
        let serializer = ReferralSerializer::new();

        let target1 = ReferralTarget::new("server1".to_string(), 2049, "/exports/data".to_string());
        let target2 = ReferralTarget::new("server2".to_string(), 2049, "/exports/data".to_string());
        let entry = ReferralEntry::new(
            "/data".to_string(),
            vec![target1, target2],
            ReferralType::Replication,
        );

        let fs_locations = serializer.to_fs_locations(&entry);

        assert_eq!(fs_locations.locations.len(), 2);
        assert_eq!(fs_locations.locations[0].servers[0].server, "server1");
        assert_eq!(fs_locations.locations[1].servers[0].server, "server2");
    }

    #[test]
    fn test_referral_root_path_and_prefix() {
        let mut db = ReferralDatabase::new();

        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports".to_string(),
        );
        let entry = ReferralEntry::new("/".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();

        let result = db.lookup_by_prefix("/some/deep/path");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/");

        let target = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/project1".to_string(),
        );
        let entry = ReferralEntry::new(
            "/project1".to_string(),
            vec![target],
            ReferralType::Migration,
        );
        db.add_referral(entry).unwrap();

        let result = db.lookup_by_prefix("/project1/file");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/project1");
    }
}
