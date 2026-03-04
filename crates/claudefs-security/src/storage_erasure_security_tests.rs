//! Storage erasure/superblock/device/compaction/snapshot security tests.
//!
//! Part of A10 Phase 11: Storage erasure & infrastructure security audit

#[cfg(test)]
mod tests {
    use claudefs_storage::block::{BlockId, BlockSize};
    use claudefs_storage::compaction::{
        CompactionConfig, CompactionEngine, CompactionState, CompactionStats, CompactionTask,
        GcCandidate, SegmentId, SegmentInfo,
    };
    use claudefs_storage::device::{
        DeviceConfig, DeviceHealth, DevicePool, DeviceRole, ManagedDevice, NvmeDeviceInfo,
    };
    use claudefs_storage::erasure::{
        EcConfig, EcError, EcProfile, EcShard, EcStripe, ErasureCodingEngine, StripeState,
    };
    use claudefs_storage::snapshot::{
        CowMapping, SnapshotId, SnapshotInfo, SnapshotManager, SnapshotState, SnapshotStats,
    };
    use claudefs_storage::superblock::{
        DeviceRoleCode, Superblock, BLOCK_SIZE, SUPERBLOCK_MAGIC, SUPERBLOCK_VERSION,
    };

    fn make_uuid(a: u32, b: u32, c: u32, d: u32) -> [u8; 16] {
        let mut uuid = [0u8; 16];
        uuid[0..4].copy_from_slice(&a.to_le_bytes());
        uuid[4..8].copy_from_slice(&b.to_le_bytes());
        uuid[8..12].copy_from_slice(&c.to_le_bytes());
        uuid[12..16].copy_from_slice(&d.to_le_bytes());
        uuid
    }

    fn far_past_time() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(300)
    }

    // Category 1: Erasure Coding Security

    #[test]
    fn test_ec_profile_overhead() {
        let profile_4_2 = EcProfile::ec_4_2();
        assert_eq!(profile_4_2.total_shards(), 6);
        assert!((profile_4_2.storage_overhead() - 1.5).abs() < 0.001);
        assert_eq!(profile_4_2.can_tolerate_failures(), 2);

        let profile_2_1 = EcProfile::ec_2_1();
        assert_eq!(profile_2_1.total_shards(), 3);
        assert_eq!(profile_2_1.can_tolerate_failures(), 1);
    }

    #[test]
    fn test_ec_encode_decode_roundtrip() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let original: Vec<u8> = b"hello world".to_vec().repeat(100);
        let stripe = engine.encode_segment(1, &original).unwrap();

        let decoded = engine.decode_stripe(&stripe).unwrap();

        assert_eq!(&decoded[..original.len()], original.as_slice());
    }

    #[test]
    fn test_ec_reconstruct_missing_shard() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let original: Vec<u8> = (0..100).map(|i| i as u8).collect();
        let stripe = engine.encode_segment(1, &original).unwrap();

        let mut stripe_missing = stripe;
        stripe_missing.shards[0] = None;

        if let StripeState::Degraded { missing_shards } = &mut stripe_missing.state {
            missing_shards.push(0);
        } else {
            stripe_missing.state = StripeState::Degraded {
                missing_shards: vec![0],
            };
        }

        engine.register_stripe(stripe_missing);
        let result = engine.reconstruct_shard_by_id(1, 0);
        assert!(result.is_ok());

        let stripe_after = engine.get_stripe(1).unwrap().clone();
        assert!(stripe_after.shards[0].is_some());

        let decoded = engine.decode_stripe(&stripe_after).unwrap();
        assert_eq!(&decoded[..original.len()], original.as_slice());
    }

    #[test]
    fn test_ec_too_many_missing_shards() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();

        let mut stripe_missing = stripe;
        stripe_missing.shards[0] = None;
        stripe_missing.shards[1] = None;
        stripe_missing.shards[4] = None;

        if let StripeState::Degraded { missing_shards } = &mut stripe_missing.state {
            missing_shards.push(0);
            missing_shards.push(1);
            missing_shards.push(4);
        } else {
            stripe_missing.state = StripeState::Degraded {
                missing_shards: vec![0, 1, 4],
            };
        }

        engine.register_stripe(stripe_missing);

        let stripe_for_decode = engine.get_stripe(1).unwrap();
        let result = engine.decode_stripe(stripe_for_decode);
        assert!(matches!(result, Err(EcError::TooManyMissing { .. })));
    }

    #[test]
    fn test_ec_shard_index_bounds() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.register_stripe(stripe);

        let result = engine.mark_shard_missing(1, 10);
        assert!(matches!(result, Err(EcError::ShardIndexOutOfRange { .. })));
    }

    // Category 2: Superblock Validation

    #[test]
    fn test_superblock_new_and_validate() {
        let device_uuid = make_uuid(1, 2, 3, 4);
        let cluster_uuid = make_uuid(5, 6, 7, 8);

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "test-device".to_string(),
        );

        assert_eq!(sb.magic, SUPERBLOCK_MAGIC);
        assert_eq!(sb.version, SUPERBLOCK_VERSION);
        assert_eq!(sb.block_size, BLOCK_SIZE);

        sb.update_checksum();
        assert!(sb.validate().is_ok());
    }

    #[test]
    fn test_superblock_checksum_integrity() {
        let device_uuid = make_uuid(1, 2, 3, 4);
        let cluster_uuid = make_uuid(5, 6, 7, 8);

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "test-device".to_string(),
        );

        sb.update_checksum();
        let valid_checksum = sb.checksum;

        sb.mount_count += 1;
        let computed = sb.compute_checksum();

        assert_ne!(valid_checksum, computed);
    }

    #[test]
    fn test_superblock_serialize_roundtrip() {
        let device_uuid = make_uuid(1, 2, 3, 4);
        let cluster_uuid = make_uuid(5, 6, 7, 8);

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            1,
            DeviceRoleCode::Combined,
            500_000_000_000,
            "roundtrip-test".to_string(),
        );
        sb.update_checksum();

        let bytes = sb.to_bytes().unwrap();
        assert_eq!(bytes.len(), BLOCK_SIZE as usize);

        let sb2 = Superblock::from_bytes(&bytes).unwrap();

        assert_eq!(sb.magic, sb2.magic);
        assert_eq!(sb.version, sb2.version);
        assert_eq!(sb.device_uuid, sb2.device_uuid);
        assert_eq!(sb.cluster_uuid, sb2.cluster_uuid);
        assert_eq!(sb.checksum, sb2.checksum);
        assert!(sb2.validate().is_ok());
    }

    #[test]
    fn test_superblock_corrupt_magic() {
        let device_uuid = make_uuid(1, 2, 3, 4);
        let cluster_uuid = make_uuid(5, 6, 7, 8);

        let mut sb = Superblock::new(
            device_uuid,
            cluster_uuid,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "test".to_string(),
        );
        sb.update_checksum();

        let mut bytes = sb.to_bytes().unwrap();
        bytes[0] = 0xDE;
        bytes[1] = 0xAD;
        bytes[2] = 0xBE;
        bytes[3] = 0xEF;

        let result = Superblock::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_superblock_cluster_identity() {
        let device_uuid = make_uuid(1, 2, 3, 4);
        let cluster_a = make_uuid(5, 6, 7, 8);
        let mut cluster_b = make_uuid(9, 10, 11, 12);
        cluster_b[0] = !cluster_b[0];

        let mut sb = Superblock::new(
            device_uuid,
            cluster_a,
            0,
            DeviceRoleCode::Data,
            1_000_000_000_000,
            "test".to_string(),
        );

        assert!(sb.is_same_cluster(&cluster_a));
        assert!(!sb.is_same_cluster(&cluster_b));

        let original_mount_count = sb.mount_count;
        sb.increment_mount_count();
        assert_eq!(sb.mount_count, original_mount_count + 1);
    }

    // Category 3: Device Pool Management

    #[test]
    fn test_device_pool_add_and_query() {
        let mut pool = DevicePool::new();

        let config1 = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let device1 = ManagedDevice::new_mock(config1, 16384).unwrap();
        pool.add_device(device1);

        let config2 = DeviceConfig::new(
            "/dev/nvme1n1".to_string(),
            1,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let device2 = ManagedDevice::new_mock(config2, 32768).unwrap();
        pool.add_device(device2);

        let config3 = DeviceConfig::new(
            "/dev/nvme2n1".to_string(),
            2,
            DeviceRole::Journal,
            false,
            32,
            true,
        );
        let device3 = ManagedDevice::new_mock(config3, 8192).unwrap();
        pool.add_device(device3);

        assert_eq!(pool.len(), 3);

        let queried = pool.device(1);
        assert!(queried.is_some());
        assert_eq!(queried.unwrap().config.device_idx, 1);
    }

    #[test]
    fn test_device_pool_role_filtering() {
        let mut pool = DevicePool::new();

        let config1 = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let device1 = ManagedDevice::new_mock(config1, 16384).unwrap();
        pool.add_device(device1);

        let config2 = DeviceConfig::new(
            "/dev/nvme1n1".to_string(),
            1,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let device2 = ManagedDevice::new_mock(config2, 16384).unwrap();
        pool.add_device(device2);

        let config3 = DeviceConfig::new(
            "/dev/nvme2n1".to_string(),
            2,
            DeviceRole::Journal,
            false,
            32,
            true,
        );
        let device3 = ManagedDevice::new_mock(config3, 16384).unwrap();
        pool.add_device(device3);

        let data_devices = pool.devices_by_role(DeviceRole::Data);
        assert_eq!(data_devices.len(), 2);

        let journal_devices = pool.devices_by_role(DeviceRole::Journal);
        assert_eq!(journal_devices.len(), 1);
    }

    #[test]
    fn test_device_health_defaults() {
        let health = DeviceHealth::default();
        assert_eq!(health.temperature_celsius, 0);
        assert_eq!(health.percentage_used, 0);
        assert_eq!(health.available_spare, 100);
        assert!(!health.critical_warning);
        assert_eq!(health.unsafe_shutdowns, 0);
    }

    #[test]
    fn test_device_pool_capacity() {
        let mut pool = DevicePool::new();

        let config1 = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let device1 = ManagedDevice::new_mock(config1, 262144).unwrap();
        pool.add_device(device1);

        let config2 = DeviceConfig::new(
            "/dev/nvme1n1".to_string(),
            1,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let device2 = ManagedDevice::new_mock(config2, 262144).unwrap();
        pool.add_device(device2);

        let total = pool.total_capacity_bytes();
        assert_eq!(total, 2 * 262144 * 4096);

        let initial_free = pool.free_capacity_bytes();

        if let Some(d) = pool.device_mut(0) {
            let _ = d.allocate_block(BlockSize::B4K);
        }

        let after_free = pool.free_capacity_bytes();
        assert!(after_free < initial_free);
    }

    #[test]
    fn test_device_fdp_zns_flags() {
        let config_fdp = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Data,
            true,
            32,
            true,
        );
        let device_fdp = ManagedDevice::new_mock(config_fdp, 1000).unwrap();
        assert!(device_fdp.fdp_active());

        let info_zns = NvmeDeviceInfo::new(
            "/dev/nvme1n1".to_string(),
            "SN123".to_string(),
            "WD SN770".to_string(),
            "2.0".to_string(),
            2_000_000_000_000,
            1,
            false,
            true,
            4096,
            512 * 1024,
            32,
        );
        let config_zns = DeviceConfig::new(
            "/dev/nvme1n1".to_string(),
            1,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        let result = ManagedDevice::new(config_zns, info_zns);

        if let Ok(device) = result {
            assert!(device.zns_supported());
        }
    }

    // Category 4: Compaction State Machine

    #[test]
    fn test_compaction_register_and_candidates() {
        let config = CompactionConfig::default();
        let min_dead_pct = config.min_dead_pct;
        let mut engine = CompactionEngine::new(config);

        let segment1 = SegmentInfo::new(
            SegmentId::new(1),
            2_000_000,
            1_000_000,
            488,
            244,
            far_past_time(),
        );
        let segment2 = SegmentInfo::new(
            SegmentId::new(2),
            2_000_000,
            1_800_000,
            488,
            244,
            far_past_time(),
        );
        let segment3 = SegmentInfo::new(
            SegmentId::new(3),
            2_000_000,
            400_000,
            488,
            244,
            far_past_time(),
        );

        engine.register_segment(segment1);
        engine.register_segment(segment2);
        engine.register_segment(segment3);

        let candidates = engine.find_candidates();

        assert!(candidates.len() >= 1);

        for candidate in &candidates {
            assert!(candidate.dead_pct >= min_dead_pct);
        }
    }

    #[test]
    fn test_compaction_register_and_candidates() {
        let config = CompactionConfig::default();
        let min_dead_pct = config.min_dead_pct;
        let mut engine = CompactionEngine::new(config);

        let segment1 = SegmentInfo::new(
            SegmentId::new(1),
            2_000_000,
            1_000_000,
            488,
            244,
            far_past_time(),
        );
        let segment2 = SegmentInfo::new(
            SegmentId::new(2),
            2_000_000,
            1_800_000,
            488,
            244,
            far_past_time(),
        );
        let segment3 = SegmentInfo::new(
            SegmentId::new(3),
            2_000_000,
            400_000,
            488,
            244,
            far_past_time(),
        );

        engine.register_segment(segment1);
        engine.register_segment(segment2);
        engine.register_segment(segment3);

        let candidates = engine.find_candidates();

        assert!(candidates.len() >= 1);

        for candidate in &candidates {
            assert!(candidate.dead_pct >= min_dead_pct);
        }
    }

    #[test]
    fn test_compaction_task_state_machine() {
        let config = CompactionConfig::default();
        let mut engine = CompactionEngine::new(config);

        let segment = SegmentInfo::new(
            SegmentId::new(1),
            2_000_000,
            1_000_000,
            488,
            244,
            far_past_time(),
        );
        engine.register_segment(segment);

        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();

        let state1 = engine.advance_task(0).unwrap();
        assert!(matches!(state1, CompactionState::Selecting));

        let state2 = engine.advance_task(0).unwrap();
        assert!(matches!(state2, CompactionState::Reading));

        let state3 = engine.advance_task(0).unwrap();
        assert!(matches!(state3, CompactionState::Writing));

        let state4 = engine.advance_task(0).unwrap();
        assert!(matches!(state4, CompactionState::Verifying));

        let state5 = engine.advance_task(0).unwrap();
        assert!(matches!(state5, CompactionState::Completed));

        engine.complete_task(0, 500_000).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.total_compactions, 1);
    }

    #[test]
    fn test_compaction_max_concurrent_limit() {
        let mut config = CompactionConfig::default();
        config.max_concurrent = 1;
        let mut engine = CompactionEngine::new(config);

        let segment1 = SegmentInfo::new(
            SegmentId::new(1),
            2_000_000,
            1_000_000,
            488,
            244,
            far_past_time(),
        );
        let segment2 = SegmentInfo::new(
            SegmentId::new(2),
            2_000_000,
            1_000_000,
            488,
            244,
            far_past_time(),
        );

        engine.register_segment(segment1);
        engine.register_segment(segment2);

        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();

        assert!(!engine.can_start_compaction());

        engine.complete_task(0, 100_000).unwrap();

        assert!(engine.can_start_compaction());
    }

    #[test]
    fn test_compaction_fail_task() {
        let config = CompactionConfig::default();
        let mut engine = CompactionEngine::new(config);

        let segment = SegmentInfo::new(
            SegmentId::new(1),
            2_000_000,
            1_000_000,
            488,
            244,
            far_past_time(),
        );
        engine.register_segment(segment);

        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();

        engine.advance_task(0).unwrap();

        engine.fail_task(0, "test failure".to_string()).unwrap();

        if let CompactionState::Failed(msg) = &engine.tasks[0].state {
            assert_eq!(msg, "test failure");
        } else {
            panic!("Expected Failed state");
        }

        let stats = engine.stats();
        assert!(stats.active_compactions == 0);
    }

    // Category 5: Snapshot CoW Correctness

    #[test]
    fn test_snapshot_create_and_list() {
        let mut mgr = SnapshotManager::new();

        let id1 = mgr.create_snapshot("snap1", None).unwrap();
        let id2 = mgr.create_snapshot("snap2", None).unwrap();
        let id3 = mgr.create_snapshot("snap3", None).unwrap();

        assert_eq!(mgr.snapshot_count(), 3);

        let list = mgr.list_snapshots();
        assert_eq!(list.len(), 3);

        assert!(list.iter().any(|s| s.id == id1));
        assert!(list.iter().any(|s| s.id == id2));
        assert!(list.iter().any(|s| s.id == id3));

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }

    #[test]
    fn test_snapshot_cow_mapping() {
        let mut mgr = SnapshotManager::new();

        let snap_id = mgr.create_snapshot("test-snap", None).unwrap();

        let original = BlockId::new(0, 100);
        let copy = BlockId::new(1, 200);

        mgr.cow_block(snap_id, original, copy, BlockSize::B4K)
            .unwrap();

        assert_eq!(mgr.cow_count(snap_id), 1);

        let resolved = mgr.resolve_block(snap_id, original);
        assert_eq!(resolved, copy);

        let resolved_orig = mgr.resolve_block(snap_id, BlockId::new(0, 999));
        assert_eq!(resolved_orig, BlockId::new(0, 999));
    }

    #[test]
    fn test_snapshot_refcount() {
        let mut mgr = SnapshotManager::new();

        let block = BlockId::new(0, 100);

        mgr.increment_ref(block);
        assert_eq!(mgr.refcount(block), 1);

        mgr.increment_ref(block);
        assert_eq!(mgr.refcount(block), 2);

        let count = mgr.decrement_ref(block);
        assert_eq!(count, 1);

        let count = mgr.decrement_ref(block);
        assert_eq!(count, 0);

        assert_eq!(mgr.refcount(block), 0);
    }

    #[test]
    fn test_snapshot_parent_child() {
        let mut mgr = SnapshotManager::new();

        let parent = mgr.create_snapshot("parent", None).unwrap();
        let child = mgr.create_snapshot("child", Some(parent)).unwrap();

        let child_info = mgr.get_snapshot(child).unwrap();
        assert_eq!(child_info.parent_id, Some(parent));

        mgr.delete_snapshot(parent).unwrap();

        let child_after = mgr.get_snapshot(child).unwrap();
        assert_eq!(child_after.parent_id, Some(parent));
    }

    #[test]
    fn test_snapshot_gc_candidates() {
        let mut mgr = SnapshotManager::new();

        let id1 = mgr.create_snapshot("snap1", None).unwrap();
        let id2 = mgr.create_snapshot("snap2", None).unwrap();
        let id3 = mgr.create_snapshot("snap3", None).unwrap();

        mgr.delete_snapshot(id2).unwrap();

        let block = BlockId::new(0, 100);
        mgr.increment_ref(block);
        mgr.decrement_ref(block);

        let candidates = mgr.gc_candidates();

        assert!(candidates.contains(&id2));
        assert!(!candidates.contains(&id1));
        assert!(!candidates.contains(&id3));
    }
}
