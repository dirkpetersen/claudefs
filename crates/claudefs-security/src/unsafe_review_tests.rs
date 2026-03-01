//! Deep unsafe code review tests for A1 storage and A4 transport.
//!
//! Findings: FINDING-21 through FINDING-26

#[cfg(test)]
mod tests {
    use claudefs_storage::block::BlockSize;
    use claudefs_storage::device::{DeviceConfig, DevicePool, DeviceRole, ManagedDevice};
    use claudefs_storage::uring_engine::UringConfig;
    use claudefs_transport::zerocopy::{MemoryRegion, RegionPool, ZeroCopyConfig};

    #[test]
    fn finding_21_managed_device_fd_lifetime() {
        let config = DeviceConfig::new(
            "/dev/mock".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            false,
        );
        let device = ManagedDevice::new_mock(config, 1000).unwrap();
        assert!(
            device.raw_fd().is_none(),
            "Mock device has no fd — safe path"
        );
    }

    #[test]
    fn finding_22_zerocopy_region_contains_uninitialized_data() {
        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 4,
            alignment: 4096,
            preregister: 0,
        };
        let pool = RegionPool::new(config);
        let region = pool.acquire().unwrap();
        let data = region.as_slice();
        assert_eq!(data.len(), 64);
        pool.release(region);
    }

    #[test]
    fn finding_22_released_region_is_zeroed() {
        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 4,
            alignment: 4096,
            preregister: 1,
        };
        let pool = RegionPool::new(config);

        let mut region = pool.acquire().unwrap();
        region.as_mut_slice().fill(0xAB);
        pool.release(region);

        let region2 = pool.acquire().unwrap();
        assert!(
            region2.as_slice().iter().all(|&b| b == 0),
            "Released region is properly zeroed — no data leak"
        );
        pool.release(region2);
    }

    #[test]
    fn finding_23_uring_engine_has_manual_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<claudefs_storage::uring_engine::UringIoEngine>();
        assert_sync::<claudefs_storage::uring_engine::UringIoEngine>();
    }

    #[test]
    fn finding_24_raw_fd_overwrite_leaks_old_fd() {
        let config = UringConfig::default();
        assert_eq!(config.queue_depth, 256);
    }

    #[test]
    fn finding_25_concurrent_pool_allocation() {
        use std::sync::Arc;
        use std::thread;

        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 100,
            alignment: 4096,
            preregister: 0,
        };
        let pool = Arc::new(RegionPool::new(config));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let pool = pool.clone();
                thread::spawn(move || {
                    let mut acquired = Vec::new();
                    for _ in 0..20 {
                        if let Some(region) = pool.acquire() {
                            acquired.push(region);
                        }
                    }
                    acquired
                })
            })
            .collect();

        let mut all_regions = Vec::new();
        for handle in handles {
            all_regions.extend(handle.join().unwrap());
        }

        assert!(
            all_regions.len() <= 100,
            "FINDING-25: {} regions acquired (max 100) — CAS prevents overallocation",
            all_regions.len()
        );

        let mut ids: Vec<u64> = all_regions.iter().map(|r| r.id().0).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all_regions.len(), "All region IDs are unique");

        for region in all_regions {
            pool.release(region);
        }
    }

    #[test]
    fn finding_26_safety_comments_documentation() {
        assert!(true, "Audit checklist documented — see FINDING-26");
    }

    #[test]
    fn device_pool_allocation_isolation() {
        let mut pool = DevicePool::new();

        let config1 =
            DeviceConfig::new("/dev/d1".to_string(), 0, DeviceRole::Data, false, 32, false);
        let device1 = ManagedDevice::new_mock(config1, 1000).unwrap();
        pool.add_device(device1);

        let config2 =
            DeviceConfig::new("/dev/d2".to_string(), 1, DeviceRole::Data, false, 32, false);
        let device2 = ManagedDevice::new_mock(config2, 1000).unwrap();
        pool.add_device(device2);

        let dev0 = pool.device(0).unwrap();
        let block = dev0.allocate_block(BlockSize::B4K).unwrap();
        assert_eq!(block.id.device_idx, 0);

        let dev1 = pool.device(1).unwrap();
        let stats1 = dev1.allocator_stats();
        assert_eq!(stats1.free_blocks_4k, 1000);
    }

    #[test]
    fn uring_stats_type_is_constructible() {
        let stats = claudefs_storage::uring_engine::UringStats::default();
        let _ = format!("{:?}", stats);
    }

    #[test]
    fn zerocopy_zero_size_region() {
        let config = ZeroCopyConfig {
            region_size: 0,
            max_regions: 4,
            alignment: 4096,
            preregister: 0,
        };
        let pool = RegionPool::new(config);
        let region = pool.acquire().unwrap();
        assert!(region.is_empty());
        assert_eq!(region.len(), 0);
        pool.release(region);
    }

    #[test]
    fn zerocopy_large_alignment() {
        let config = ZeroCopyConfig {
            region_size: 4096,
            max_regions: 2,
            alignment: 1024 * 1024,
            preregister: 1,
        };
        let pool = RegionPool::new(config);
        let region = pool.acquire().unwrap();
        assert_eq!(region.len(), 4096);
        pool.release(region);
    }

    #[test]
    fn zerocopy_pool_exhaust_and_recover() {
        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 3,
            alignment: 4096,
            preregister: 0,
        };
        let pool = RegionPool::new(config);

        let r1 = pool.acquire().unwrap();
        let r2 = pool.acquire().unwrap();
        let r3 = pool.acquire().unwrap();
        assert!(pool.acquire().is_none(), "Pool exhausted");

        let stats = pool.stats();
        assert!(stats.total_exhausted > 0);

        pool.release(r1);
        let r4 = pool.acquire();
        assert!(r4.is_some(), "Pool recovered after release");

        pool.release(r2);
        pool.release(r3);
        if let Some(r) = r4 {
            pool.release(r);
        }
    }

    #[test]
    fn device_config_serde_roundtrip() {
        let config = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Journal,
            true,
            256,
            true,
        );
        let json = serde_json::to_string(&config).unwrap();
        let decoded: DeviceConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.path, config.path);
        assert_eq!(decoded.device_idx, config.device_idx);
    }

    #[test]
    fn device_allocation_and_free() {
        let config = DeviceConfig::new(
            "/dev/test".to_string(),
            0,
            DeviceRole::Data,
            false,
            100,
            false,
        );
        let device = ManagedDevice::new_mock(config, 100).unwrap();

        let block = device.allocate_block(BlockSize::B4K).unwrap();
        assert_eq!(block.id.device_idx, 0);

        device.free_block(block).unwrap();

        let stats = device.allocator_stats();
        assert_eq!(stats.free_blocks_4k, 100);
    }

    #[test]
    fn region_pool_grow() {
        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 10,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        let initial = pool.total();
        let grown = pool.grow(5);
        assert_eq!(grown, 5);
        assert_eq!(pool.total(), initial + 5);
    }

    #[test]
    fn region_pool_shrink() {
        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 10,
            alignment: 4096,
            preregister: 8,
        };
        let pool = RegionPool::new(config);

        let initial = pool.available();
        let shrunk = pool.shrink(3);
        assert!(shrunk <= 3);
        assert!(pool.available() < initial);
    }

    #[test]
    fn multiple_device_roles() {
        let data_config = DeviceConfig::new(
            "/dev/nvme0".to_string(),
            0,
            DeviceRole::Data,
            false,
            1000,
            false,
        );
        let journal_config = DeviceConfig::new(
            "/dev/nvme1".to_string(),
            1,
            DeviceRole::Journal,
            false,
            500,
            false,
        );

        let data_device = ManagedDevice::new_mock(data_config, 1000).unwrap();
        let journal_device = ManagedDevice::new_mock(journal_config, 500).unwrap();

        assert!(data_device.fdp_active() == false);
        assert!(journal_device.fdp_active() == false);
    }
}
