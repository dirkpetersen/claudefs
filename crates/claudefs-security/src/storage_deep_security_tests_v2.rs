//! Deep security tests v2 for claudefs-storage: allocator, cache, quota, wear, hot swap.
//!
//! Part of A10 Phase 7: Storage deep security audit v2

#[cfg(test)]
mod tests {
    use claudefs_storage::allocator::{AllocatorConfig, AllocatorStats, BuddyAllocator};
    use claudefs_storage::block::{BlockId, BlockRef, BlockSize};
    use claudefs_storage::block_cache::{BlockCache, BlockCacheConfig, CacheEntry, CacheStats};
    use claudefs_storage::checksum::{Checksum, ChecksumAlgorithm};
    use claudefs_storage::device::DeviceRole;
    use claudefs_storage::hot_swap::{DeviceState, DrainProgress, HotSwapManager, HotSwapStats};
    use claudefs_storage::quota::{QuotaLimit, QuotaManager, QuotaStats, QuotaStatus, TenantQuota};
    use claudefs_storage::wear_leveling::{
        PlacementAdvice, WearAlert, WearConfig, WearLevel, WearLevelingEngine, WritePattern,
        ZoneWear,
    };

    fn make_block_ref(device_idx: u16, offset: u64, size: BlockSize) -> BlockRef {
        BlockRef {
            id: BlockId::new(device_idx, offset),
            size,
        }
    }

    fn make_test_data(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 256) as u8).collect()
    }

    fn make_checksum(data: &[u8]) -> Checksum {
        claudefs_storage::checksum::compute(ChecksumAlgorithm::Crc32c, data)
    }

    // =========================================================================
    // Category 1: Allocator Boundary Security (5 tests)
    // =========================================================================

    #[test]
    fn test_allocator_stats_after_alloc_free() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 1024,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let block = alloc.allocate(BlockSize::B4K).unwrap();
        alloc.free(block).unwrap();

        let stats = alloc.stats();
        assert!(
            stats.total_allocations >= 1,
            "Stats must track at least 1 allocation"
        );
        assert!(stats.total_frees >= 1, "Stats must track at least 1 free");
    }

    #[test]
    fn test_allocator_exhaust_capacity() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 16,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let mut count = 0;
        while alloc.allocate(BlockSize::B4K).is_ok() {
            count += 1;
        }

        assert_eq!(count, 16, "Should allocate exactly 16 blocks");

        let result = alloc.allocate(BlockSize::B4K);
        assert!(
            result.is_err(),
            "Allocation must fail when capacity exhausted"
        );
    }

    #[test]
    fn test_allocator_large_block_alignment() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 4096,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let block1 = alloc.allocate(BlockSize::B1M).unwrap();
        assert_eq!(block1.size, BlockSize::B1M);
        assert_eq!(
            block1.id.offset % 256,
            0,
            "1MB block must be 256-block aligned"
        );

        alloc.free(block1).unwrap();

        let block2 = alloc.allocate(BlockSize::B1M).unwrap();
        assert_eq!(
            block2.size,
            BlockSize::B1M,
            "Reallocation after free must succeed"
        );
    }

    #[test]
    fn test_allocator_free_returns_to_pool() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 64,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let mut allocated = Vec::new();
        for _ in 0..64 {
            let b = alloc.allocate(BlockSize::B4K).unwrap();
            allocated.push(b);
        }

        for b in &allocated[32..] {
            alloc.free(*b).unwrap();
        }

        let reallocated = alloc.allocate(BlockSize::B4K);
        assert!(
            reallocated.is_ok(),
            "Allocation must succeed for freed capacity"
        );
    }

    #[test]
    fn test_allocator_zero_capacity_rejected() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 0,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let result = alloc.allocate(BlockSize::B4K);
        assert!(
            result.is_err(),
            "FINDING-STOR-DEEP2-01: Zero capacity allocator accepts creation but all allocations fail"
        );
    }

    // =========================================================================
    // Category 2: Block Cache Poisoning (5 tests)
    // =========================================================================

    #[test]
    fn test_cache_insert_get_roundtrip() {
        let config = BlockCacheConfig {
            max_entries: 100,
            max_memory_bytes: 1024 * 1024,
            ..Default::default()
        };
        let mut cache = BlockCache::new(config);

        let block_ref = make_block_ref(0, 100, BlockSize::B4K);
        let data = make_test_data(4096);
        let checksum = make_checksum(&data);

        cache.insert(block_ref, data.clone(), checksum).unwrap();

        let retrieved = cache.get(&block_ref.id);
        assert!(retrieved.is_some(), "Cache must return inserted entry");
        assert_eq!(
            retrieved.unwrap().data,
            data,
            "Retrieved data must match inserted data"
        );
    }

    #[test]
    fn test_cache_eviction_at_capacity() {
        let mut config = BlockCacheConfig::default();
        config.max_entries = 3;
        config.eviction_batch_size = 1;
        let mut cache = BlockCache::new(config);

        let blocks: Vec<_> = (0..4)
            .map(|i| make_block_ref(0, i, BlockSize::B4K))
            .collect();

        for (i, block_ref) in blocks.iter().enumerate() {
            let data = make_test_data(100);
            let checksum = make_checksum(&data);
            cache.insert(*block_ref, data, checksum).unwrap();
        }

        assert!(
            !cache.contains(&BlockId::new(0, 0)),
            "Oldest entry (offset 0) must be evicted when capacity exceeded"
        );
        assert!(
            cache.contains(&BlockId::new(0, 1)),
            "Entry at offset 1 must still be present"
        );
        assert!(
            cache.contains(&BlockId::new(0, 2)),
            "Entry at offset 2 must still be present"
        );
        assert!(
            cache.contains(&BlockId::new(0, 3)),
            "Newest entry (offset 3) must be present"
        );
    }

    #[test]
    fn test_cache_dirty_entry_tracking() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref1 = make_block_ref(0, 1, BlockSize::B4K);
        let data1 = make_test_data(4096);
        let checksum1 = make_checksum(&data1);
        cache.insert_dirty(block_ref1, data1, checksum1).unwrap();

        let block_ref2 = make_block_ref(0, 2, BlockSize::B4K);
        let data2 = make_test_data(4096);
        let checksum2 = make_checksum(&data2);
        cache.insert(block_ref2, data2, checksum2).unwrap();

        let dirty_entries = cache.dirty_entries();
        assert_eq!(
            dirty_entries.len(),
            1,
            "Exactly 1 dirty entry should be tracked"
        );
        assert!(
            dirty_entries[0].dirty,
            "Dirty entry must have dirty flag set"
        );
    }

    #[test]
    fn test_cache_checksum_stored_correctly() {
        let config = BlockCacheConfig::default();
        let mut cache = BlockCache::new(config);

        let block_ref = make_block_ref(0, 100, BlockSize::B4K);
        let data = make_test_data(4096);
        let original_checksum = make_checksum(&data);

        cache
            .insert(block_ref, data.clone(), original_checksum)
            .unwrap();

        let retrieved = cache.get(&block_ref.id).unwrap();
        assert_eq!(
            retrieved.checksum.algorithm, original_checksum.algorithm,
            "Checksum algorithm must match"
        );
        assert_eq!(
            retrieved.checksum.value,
            original_checksum.value,
            "FINDING-STOR-DEEP2-02: Checksum value must be stored correctly without silent corruption"
        );
    }

    #[test]
    fn test_cache_pinned_entry_survives_eviction() {
        let mut config = BlockCacheConfig::default();
        config.max_entries = 2;
        config.eviction_batch_size = 1;
        let mut cache = BlockCache::new(config);

        let pinned_block = make_block_ref(0, 0, BlockSize::B4K);
        let data0 = make_test_data(100);
        let checksum0 = make_checksum(&data0);
        cache.insert(pinned_block, data0, checksum0).unwrap();
        cache.pin(&pinned_block.id);

        let block1 = make_block_ref(0, 1, BlockSize::B4K);
        let data1 = make_test_data(100);
        let checksum1 = make_checksum(&data1);
        cache.insert(block1, data1, checksum1).unwrap();

        let block2 = make_block_ref(0, 2, BlockSize::B4K);
        let data2 = make_test_data(100);
        let checksum2 = make_checksum(&data2);
        cache.insert(block2, data2, checksum2).unwrap();

        assert!(
            cache.contains(&pinned_block.id),
            "FINDING-STOR-DEEP2-03: Pinned entry must survive eviction"
        );
        assert!(
            cache.contains(&block1.id) || cache.contains(&block2.id),
            "One unpinned entry should remain"
        );
    }

    // =========================================================================
    // Category 3: Storage Quota Enforcement (5 tests)
    // =========================================================================

    #[test]
    fn test_storage_quota_hard_limit_blocks() {
        let limit = QuotaLimit {
            bytes_hard: 1000,
            bytes_soft: 800,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 3600,
        };
        let mut quota = TenantQuota::new("test-tenant".to_string(), limit);
        quota.usage.bytes_used = 999;

        let can_alloc = quota.can_allocate(2, 0);
        assert!(
            !can_alloc,
            "Allocation must be denied when it would exceed hard limit"
        );
    }

    #[test]
    fn test_storage_quota_soft_limit_grace() {
        let limit = QuotaLimit {
            bytes_hard: 200,
            bytes_soft: 100,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 3600,
        };
        let mut quota = TenantQuota::new("test-tenant".to_string(), limit);
        quota.usage.bytes_used = 150;
        quota.usage.soft_exceeded_since = Some(0);

        let status_at_1800 = quota.check_status(1800);
        assert!(
            matches!(status_at_1800, QuotaStatus::SoftExceeded { .. }),
            "Status should be SoftExceeded within grace period"
        );

        let status_at_3601 = quota.check_status(3601);
        assert!(
            matches!(status_at_3601, QuotaStatus::GraceExpired),
            "Status should be GraceExpired after grace period"
        );
    }

    #[test]
    fn test_storage_quota_zero_limits() {
        let limit = QuotaLimit {
            bytes_hard: 0,
            bytes_soft: 0,
            inodes_hard: 0,
            inodes_soft: 0,
            grace_period_secs: 0,
        };
        let quota = TenantQuota::new("test-tenant".to_string(), limit);

        let can_alloc = quota.can_allocate(1, 0);
        assert!(
            !can_alloc,
            "FINDING-STOR-DEEP2-04: Zero hard limit means allocation permanently blocked"
        );
    }

    #[test]
    fn test_storage_quota_usage_at_exactly_hard() {
        let limit = QuotaLimit {
            bytes_hard: 100,
            bytes_soft: 80,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 3600,
        };
        let mut quota = TenantQuota::new("test-tenant".to_string(), limit);
        quota.usage.bytes_used = 100;

        let status = quota.check_status(0);
        // FINDING-STOR-DEEP2-05: At exactly hard limit (100), returns SoftExceeded since soft=80 < hard=100
        // The check_status checks soft limit first before hard limit
        if matches!(status, QuotaStatus::SoftExceeded { .. }) {
            // Expected: soft limit (80) is exceeded before hard limit (100)
        } else {
            panic!(
                "Expected SoftExceeded status at usage=100 with soft=80, got {:?}",
                status
            );
        }
    }

    #[test]
    fn test_storage_quota_stats_tracking() {
        let limit = QuotaLimit {
            bytes_hard: 100,
            bytes_soft: 80,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 100,
        };
        let mut manager = QuotaManager::new(limit.clone());

        manager.add_tenant("soft-tenant", limit.clone());
        manager.add_tenant("hard-tenant", limit.clone());
        manager.add_tenant("ok-tenant", limit.clone());

        if let Some(t) = manager.get_tenant("soft-tenant") {
            assert_eq!(t.usage.bytes_used, 0);
        }

        let tenants: Vec<_> = manager.all_tenants();
        let mut at_soft = 0;
        let mut at_hard = 0;

        for t in &tenants {
            let status = t.check_status(0);
            match status {
                QuotaStatus::SoftExceeded { .. } => at_soft += 1,
                QuotaStatus::HardExceeded | QuotaStatus::GraceExpired => at_hard += 1,
                QuotaStatus::Ok => {}
            }
        }

        assert_eq!(at_soft, 0, "No tenant initially at soft limit");
        assert_eq!(at_hard, 0, "No tenant initially at hard limit");
    }

    // =========================================================================
    // Category 4: Wear Leveling Security (5 tests)
    // =========================================================================

    #[test]
    fn test_wear_leveling_hot_zone_detection() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 80.0;
        let mut engine = WearLevelingEngine::new(config);

        engine.register_zone(0);
        engine.register_zone(1);

        for _ in 0..200 {
            engine.record_write(0, 100 * 1024 * 1024, 1000).unwrap();
        }

        let alerts: Vec<_> = engine.alerts().iter().filter(|a| a.zone_id == 0).collect();

        assert!(
            !alerts.is_empty(),
            "High wear alert must be generated for zone exceeding threshold"
        );
    }

    #[test]
    fn test_wear_advice_after_writes() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 50.0;
        config.cold_zone_target_pct = 20.0;
        let mut engine = WearLevelingEngine::new(config);

        engine.register_zone(0);
        engine.register_zone(1);
        engine.register_zone(2);

        for _ in 0..100 {
            engine.record_write(0, 1024 * 1024, 1000).unwrap();
        }

        let advice = engine.get_placement_advice(4096, WritePattern::Random);

        assert!(
            advice.preferred_zone.is_some(),
            "Advice must have a preferred zone"
        );
        if let Some(preferred) = advice.preferred_zone {
            assert_ne!(
                preferred, 0,
                "Preferred zone must not be the hot zone (zone 0)"
            );
        }
    }

    #[test]
    fn test_wear_alert_severity() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 10.0;
        let mut engine = WearLevelingEngine::new(config);

        engine.register_zone(0);

        for _ in 0..500 {
            engine.record_write(0, 10 * 1024 * 1024, 1000).unwrap();
        }

        let alerts: Vec<WearAlert> = engine.alerts().to_vec();
        assert!(!alerts.is_empty(), "Alerts must be generated for high wear");

        for alert in &alerts {
            assert!(alert.wear_level > 0.0, "Alert must have wear level > 0");
        }
    }

    #[test]
    fn test_wear_no_writes_no_alerts() {
        let config = WearConfig::default();
        let mut engine = WearLevelingEngine::new(config);

        engine.register_zone(0);
        engine.register_zone(1);

        let alerts: Vec<WearAlert> = engine.alerts().to_vec();
        assert!(
            alerts.is_empty(),
            "No alerts should be generated when no writes have occurred"
        );
    }

    #[test]
    fn test_wear_write_pattern_tracking() {
        let mut config = WearConfig::default();
        let mut engine = WearLevelingEngine::new(config);

        engine.register_zone(0);
        engine.register_zone(1);

        for i in 0..10 {
            engine.record_write(0, 4096, 1000 + i).unwrap();
        }

        for _ in 0..10 {
            engine.record_write(1, 4096, 2000).unwrap();
        }

        let zone0 = engine.get_zone(0).expect("Zone 0 must exist");
        let zone1 = engine.get_zone(1).expect("Zone 1 must exist");

        assert_eq!(
            zone0.write_count, 10,
            "Zone 0 must have sequential writes tracked"
        );
        assert!(
            zone0.last_written_at > 0,
            "Zone 0 must have timestamp from sequential writes"
        );

        let advice = engine.get_placement_advice(4096, WritePattern::Sequential);
        assert_eq!(
            advice.pattern,
            WritePattern::Sequential,
            "Write pattern must be tracked and returned in advice"
        );
    }

    // =========================================================================
    // Category 5: Hot Swap State Machine (5 tests)
    // =========================================================================

    #[test]
    fn test_hot_swap_register_and_drain() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let allocated_blocks: Vec<BlockRef> = (0..100)
            .map(|i| make_block_ref(0, i, BlockSize::B4K))
            .collect();

        let progress = manager.start_drain(0, allocated_blocks).unwrap();

        assert_eq!(progress.device_idx, 0, "Drain progress must track device 0");
        assert_eq!(
            progress.total_blocks_to_migrate, 100,
            "Drain progress must show total blocks to migrate"
        );
    }

    #[test]
    fn test_hot_swap_drain_unregistered_fails() {
        let manager = HotSwapManager::new();

        let result = manager.start_drain(99, vec![]);
        assert!(result.is_err(), "Drain must fail for unregistered device");
    }

    #[test]
    fn test_hot_swap_double_register_fails() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();

        let result = manager.register_device(0, DeviceRole::Data, 1_000_000_000);
        assert!(
            result.is_err(),
            "FINDING-STOR-DEEP2-06: Double registration must fail to prevent duplicate devices"
        );
    }

    #[test]
    fn test_hot_swap_remove_active_device() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        let result = manager.remove_device(0);
        assert!(
            result.is_err(),
            "FINDING-STOR-DEEP2-07: Removal of active device must fail without drain first"
        );
    }

    #[test]
    fn test_hot_swap_fail_device_state() {
        let manager = HotSwapManager::new();

        manager
            .register_device(0, DeviceRole::Data, 1_000_000_000)
            .unwrap();
        manager.activate_device(0).unwrap();

        manager.fail_device(0, "media error".to_string()).unwrap();

        let state = manager.device_state(0);
        assert_eq!(
            state,
            Some(DeviceState::Failed),
            "Device state must be Failed after fail_device"
        );

        let allocated_blocks: Vec<BlockRef> = vec![make_block_ref(0, 0, BlockSize::B4K)];
        let drain_result = manager.start_drain(0, allocated_blocks);

        assert!(
            drain_result.is_err(),
            "Drain must not start on failed device"
        );
    }
}
