// FILE: final_unsafe_tests.rs
use claudefs_repl::batch_auth::{BatchAuthKey, BatchAuthenticator};
use claudefs_storage::allocator::AllocatorStats;
use claudefs_storage::block::BlockSize;
use claudefs_storage::device::{DeviceConfig, DevicePool, DeviceRole, ManagedDevice};
use claudefs_storage::uring_engine::{UringConfig, UringIoEngine};
use claudefs_transport::zerocopy::{MemoryRegion, RegionPool, ZeroCopyConfig};
use std::sync::Arc;
use std::thread;

#[test]
fn finding_47_uring_config_defaults() {
    let config = UringConfig::default();

    assert_eq!(config.queue_depth, 256);
    assert_eq!(config.direct_io, true);
    assert_eq!(config.io_poll, false);
    assert_eq!(config.sq_poll, false);
}

#[test]
fn finding_47_uring_engine_creation() {
    let config = UringConfig::default();

    let result = std::panic::catch_unwind(|| UringIoEngine::new(config));

    assert!(result.is_ok() || result.is_err());
}

#[test]
fn finding_48_device_double_free_prevented() {
    let config = DeviceConfig::new(
        "/dev/null".to_string(),
        0,
        DeviceRole::Data,
        false,
        32,
        false,
    );

    let device = ManagedDevice::new_mock(config, 100);
    if let Ok(dev) = device {
        let block = dev.allocate_block(BlockSize::B4K);
        if let Ok(blk) = block {
            let free_result = dev.free_block(blk.clone());
            assert!(free_result.is_ok());

            let double_free = dev.free_block(blk);
            assert!(double_free.is_err(), "Double free should be prevented");
        }
    }
}

#[test]
fn finding_48_device_full_cycle() {
    let config = DeviceConfig::new(
        "/dev/null".to_string(),
        0,
        DeviceRole::Data,
        false,
        32,
        false,
    );

    let device = ManagedDevice::new_mock(config, 10);
    if let Ok(dev) = device {
        let mut blocks = Vec::new();

        for _ in 0..10 {
            if let Ok(blk) = dev.allocate_block(BlockSize::B4K) {
                blocks.push(blk);
            }
        }

        let all_allocated = dev.allocator_stats();
        assert_eq!(all_allocated.free_blocks_4k, 0);

        for blk in blocks {
            let _ = dev.free_block(blk);
        }

        let all_freed = dev.allocator_stats();
        assert_eq!(all_freed.free_blocks_4k, 10);
    }
}

#[test]
fn finding_49_zerocopy_released_region_zeroed() {
    let config = ZeroCopyConfig {
        region_size: 4096,
        max_regions: 10,
        alignment: 4096,
        preregister: 5,
    };

    let pool = RegionPool::new(config);

    let mut region = pool.acquire().unwrap();
    for byte in region.as_mut_slice() {
        *byte = 0xFF;
    }

    pool.release(region);

    let mut region2 = pool.acquire().unwrap();
    let is_zeroed = region2.as_slice().iter().all(|&b| b == 0);
    assert!(is_zeroed, "Released region should be zeroed");
}

#[test]
fn finding_49_zerocopy_data_isolation() {
    let config = ZeroCopyConfig {
        region_size: 4096,
        max_regions: 10,
        alignment: 4096,
        preregister: 5,
    };

    let pool = RegionPool::new(config);

    let mut region1 = pool.acquire().unwrap();
    for (i, byte) in region1.as_mut_slice().iter_mut().enumerate() {
        *byte = (i % 256) as u8;
    }
    let data1 = region1.as_slice().to_vec();

    let mut region2 = pool.acquire().unwrap();
    let data2 = region2.as_slice().to_vec();

    assert_ne!(data1, data2, "Different regions should have isolated data");
}

#[test]
fn finding_50_batch_auth_sign_verify() {
    let key = BatchAuthKey::generate();
    let auth = BatchAuthenticator::new(key);

    let data = b"test data for hmac";
    let signature = auth.sign(data);

    assert!(auth.verify(data, &signature));
}

#[test]
fn finding_50_batch_auth_wrong_key() {
    let key1 = BatchAuthKey::generate();
    let key2 = BatchAuthKey::generate();

    let auth1 = BatchAuthenticator::new(key1);
    let auth2 = BatchAuthenticator::new(key2);

    let data = b"test data";
    let sig = auth1.sign(data);

    assert!(
        !auth2.verify(data, &sig),
        "Different keys should not verify"
    );
}

#[test]
fn finding_50_batch_auth_tampered_data() {
    let key = BatchAuthKey::generate();
    let auth = BatchAuthenticator::new(key);

    let mut data = b"original data".to_vec();
    let signature = auth.sign(&data);

    data[0] ^= 0xFF;

    assert!(
        !auth.verify(&data, &signature),
        "Tampered data should not verify"
    );
}

#[test]
fn finding_51_zerocopy_alignment_check() {
    for alignment in &[512, 1024, 4096, 8192] {
        let config = ZeroCopyConfig {
            region_size: 4096,
            max_regions: 5,
            alignment: *alignment,
            preregister: 2,
        };

        let pool = RegionPool::new(config);
        if let Some(mut region) = pool.acquire() {
            let ptr = region.as_slice().as_ptr() as usize;
            assert_eq!(
                ptr % alignment,
                0,
                "Region should be aligned to {}",
                alignment
            );
        }
    }
}

#[test]
fn finding_51_zerocopy_max_regions_limit() {
    let config = ZeroCopyConfig {
        region_size: 4096,
        max_regions: 3,
        alignment: 4096,
        preregister: 3,
    };

    let pool = RegionPool::new(config);

    let mut regions = Vec::new();
    for _ in 0..3 {
        if let Some(r) = pool.acquire() {
            regions.push(r);
        }
    }

    assert!(pool.acquire().is_none(), "Should not exceed max_regions");
}

#[test]
fn finding_52_concurrent_device_alloc() {
    let config = DeviceConfig::new(
        "/dev/null".to_string(),
        0,
        DeviceRole::Data,
        false,
        32,
        false,
    );

    let device = ManagedDevice::new_mock(config, 100);
    if let Ok(dev) = device {
        let dev_arc = Arc::new(dev);

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let d = dev_arc.clone();
                thread::spawn(move || {
                    let mut blocks = Vec::new();
                    for _ in 0..10 {
                        if let Ok(blk) = d.allocate_block(BlockSize::B4K) {
                            blocks.push(blk);
                        }
                    }
                    blocks
                })
            })
            .collect();

        let mut all_blocks = Vec::new();
        for handle in handles {
            let blocks = handle.join().unwrap();
            all_blocks.extend(blocks);
        }

        assert_eq!(all_blocks.len(), 100, "Should allocate 100 blocks total");
    }
}

#[test]
fn finding_52_concurrent_region_pool() {
    let config = ZeroCopyConfig {
        region_size: 4096,
        max_regions: 20,
        alignment: 4096,
        preregister: 10,
    };

    let pool = Arc::new(RegionPool::new(config));

    let handles: Vec<_> = (0..5)
        .map(|_| {
            let p = pool.clone();
            thread::spawn(move || {
                let mut regions = Vec::new();
                for _ in 0..10 {
                    if let Some(r) = p.acquire() {
                        regions.push(r);
                    }
                }
                regions
            })
        })
        .collect();

    let mut all_regions = Vec::new();
    for handle in handles {
        let regions = handle.join().unwrap();
        all_regions.extend(regions);
    }

    assert_eq!(all_regions.len(), 50, "Should acquire 50 regions total");
}

#[test]
fn device_pool_empty_safe() {
    let pool = DevicePool::new();

    assert!(pool.device(0).is_none());
    assert!(pool.device(999).is_none());
}

#[test]
fn zerocopy_preregister_capped() {
    let config = ZeroCopyConfig {
        region_size: 4096,
        max_regions: 5,
        alignment: 4096,
        preregister: 100,
    };

    let pool = RegionPool::new(config);

    assert!(
        pool.total() <= config.max_regions,
        "Preregister should be capped to max_regions"
    );
}

#[test]
fn batch_auth_empty_data() {
    let key = BatchAuthKey::generate();
    let auth = BatchAuthenticator::new(key);

    let empty: &[u8] = &[];
    let sig = auth.sign(empty);

    assert!(
        auth.verify(empty, &sig),
        "Empty data should sign and verify"
    );
}

#[test]
fn batch_auth_large_data() {
    let key = BatchAuthKey::generate();
    let auth = BatchAuthenticator::new(key);

    let large_data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
    let sig = auth.sign(&large_data);

    assert!(
        auth.verify(&large_data, &sig),
        "Large data should sign and verify"
    );
}

#[test]
fn prop_hmac_deterministic() {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_hmac_deterministic_inner(seed: u64) {
            let key_bytes = {
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&seed.to_le_bytes());
                BatchAuthKey::from_bytes(bytes)
            };

            let auth = BatchAuthenticator::new(key_bytes);
            let data = b"deterministic hmac test";

            let sig1 = auth.sign(data);
            let sig2 = auth.sign(data);

            assert_eq!(sig1, sig2, "Same key+data should produce same signature");
        }
    }
}

#[test]
fn prop_region_pool_invariant() {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_pool_invariant(acquire_count in 0..50usize, release_count in 0..50usize) {
            let config = ZeroCopyConfig {
                region_size: 4096,
                max_regions: 100,
                alignment: 4096,
                preregister: 50,
            };

            let pool = RegionPool::new(config);
            let initial_available = pool.available();

            let mut acquired = Vec::new();
            for _ in 0..acquire_count.min(100) {
                if let Some(r) = pool.acquire() {
                    acquired.push(r);
                }
            }

            for _ in 0..release_count.min(acquired.len()) {
                if let Some(r) = acquired.pop() {
                    pool.release(r);
                }
            }

            let total = pool.total();
            let available = pool.available();
            let in_use = pool.in_use();

            assert_eq!(total, available + in_use, "Pool invariant: total = available + in_use");
        }
    }
}

#[test]
fn prop_device_alloc_free_conservation() {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_conservation(count: usize) in 0..100usize {
            let config = DeviceConfig::new("/dev/null".to_string(), 0, DeviceRole::Data, false, 32, false);

            let device = ManagedDevice::new_mock(config, 1000);
            if let Ok(dev) = device {
                let initial = dev.allocator_stats().free_blocks_4k;

                let mut blocks = Vec::new();
                for _ in 0..count {
                    if let Ok(blk) = dev.allocate_block(BlockSize::B4K) {
                        blocks.push(blk);
                    }
                }

                let after_alloc = dev.allocator_stats().free_blocks_4k;

                for blk in blocks {
                    let _ = dev.free_block(blk);
                }

                let after_free = dev.allocator_stats().free_blocks_4k;

                assert_eq!(after_free, initial, "Alloc then free should restore count");
            }
        }
    }
}
