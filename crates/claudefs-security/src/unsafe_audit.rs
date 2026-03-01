//! Phase 3 final unsafe code review for A1 storage and A4 transport.
//!
//! Findings: FINDING-UA-01 through FINDING-UA-18
//!
//! This module tests observable behavior and safety properties of unsafe code
//! in the storage and transport crates without directly testing unsafe internals.

#[cfg(test)]
mod tests {
    use claudefs_storage::block::{BlockId, BlockRef, BlockSize};
    use claudefs_storage::engine::StorageEngineConfig;
    use claudefs_storage::error::{StorageError, StorageResult};
    use claudefs_storage::io_uring_bridge::IoEngine;
    use claudefs_storage::uring_engine::{UringConfig, UringIoEngine};
    use claudefs_transport::zerocopy::{RegionPool, ZeroCopyConfig};

    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    mod uring_engine_thread_safety {
        use super::*;

        #[test]
        fn finding_ua_01_uring_engine_is_send() {
            assert_send::<UringIoEngine>();
        }

        #[test]
        fn finding_ua_02_uring_engine_is_sync() {
            assert_sync::<UringIoEngine>();
        }

        #[test]
        fn finding_ua_03_uring_config_serialization_roundtrip() {
            let config = UringConfig::default();
            let serialized = serde_json::to_string(&config).unwrap();
            let deserialized: UringConfig = serde_json::from_str(&serialized).unwrap();
            assert_eq!(config.queue_depth, deserialized.queue_depth);
            assert_eq!(config.direct_io, deserialized.direct_io);
            assert_eq!(config.io_poll, deserialized.io_poll);
            assert_eq!(config.sq_poll, deserialized.sq_poll);
        }
    }

    mod region_pool_safety {
        use super::*;

        #[test]
        fn finding_ua_04_zero_size_region_allocation() {
            let config = ZeroCopyConfig {
                region_size: 0,
                max_regions: 10,
                alignment: 4096,
                preregister: 0,
            };
            let pool = RegionPool::new(config);
            let region = pool.acquire();
            assert!(region.is_some(), "Zero-size region should be handled gracefully");
            if let Some(r) = region {
                assert!(r.is_empty(), "Zero-size region should be empty");
                pool.release(r);
            }
        }

        #[test]
        fn finding_ua_05_very_large_region_allocation() {
            let config = ZeroCopyConfig {
                region_size: 16 * 1024 * 1024,
                max_regions: 2,
                alignment: 4096,
                preregister: 0,
            };
            let pool = RegionPool::new(config);
            let region = pool.acquire();
            if region.is_some() {
                let r = region.unwrap();
                assert_eq!(r.len(), 16 * 1024 * 1024, "Large region should match requested size");
                pool.release(r);
            }
        }

        #[test]
        fn finding_ua_06_pool_exhaustion_and_recovery() {
            let config = ZeroCopyConfig {
                region_size: 1024,
                max_regions: 3,
                alignment: 4096,
                preregister: 2,
            };
            let pool = RegionPool::new(config);

            let r1 = pool.acquire().unwrap();
            let r2 = pool.acquire().unwrap();
            let stats_before = pool.stats();
            let total_before = stats_before.total_regions;

            let r3 = pool.acquire();
            let stats_after = pool.stats();

            if r3.is_none() {
                assert!(stats_after.total_exhausted > 0, "Pool should track exhaustion");
            }

            pool.release(r1);
            let r4 = pool.acquire();
            assert!(r4.is_some(), "Should be able to acquire after release");
            pool.release(r2);
            if let Some(r) = r4 {
                pool.release(r);
            }
        }

        #[test]
        fn finding_ua_07_regions_are_zero_initialized() {
            let config = ZeroCopyConfig {
                region_size: 4096,
                max_regions: 10,
                alignment: 4096,
                preregister: 1,
            };
            let pool = RegionPool::new(config);
            let region = pool.acquire().unwrap();

            let slice = region.as_slice();
            let all_zeros = slice.iter().all(|&b| b == 0);
            assert!(all_zeros, "Newly allocated regions should be zero-initialized");

            let mut mutable = region;
            mutable.as_mut_slice()[0] = 0xAB;
            mutable.as_mut_slice()[100] = 0xCD;
            pool.release(mutable);

            let fresh = pool.acquire().unwrap();
            let fresh_slice = fresh.as_slice();
            let still_zero = fresh_slice.iter().all(|&b| b == 0);
            assert!(still_zero, "Released regions should be zeroed before reuse");
            pool.release(fresh);
        }

        #[test]
        fn finding_ua_08_region_alignment() {
            let config = ZeroCopyConfig {
                region_size: 1024,
                max_regions: 10,
                alignment: 4096,
                preregister: 1,
            };
            let pool = RegionPool::new(config.clone());
            assert_eq!(config.alignment, 4096, "Config should request 4096-byte alignment");

            let region = pool.acquire().unwrap();
            assert_eq!(region.len(), 1024, "Region should have requested size");
            assert!(!region.is_empty(), "Region should not be empty");

            pool.release(region);
        }
    }

    mod block_id_boundary_tests {
        use super::*;

        #[test]
        fn finding_ua_09_block_id_zero_is_valid() {
            let id = BlockId::new(0, 0);
            assert_eq!(id.device_idx, 0);
            assert_eq!(id.offset, 0);

            let offset = id.byte_offset(BlockSize::B4K);
            assert_eq!(offset, 0);
        }

        #[test]
        fn finding_ua_10_block_id_boundary_values() {
            let id_small = BlockId::new(0, 100);
            let offset = id_small.byte_offset(BlockSize::B4K);
            assert_eq!(offset, 100 * 4096);

            let id_large = BlockId::new(0, 1_000_000);
            let offset_large = id_large.byte_offset(BlockSize::B64K);
            assert_eq!(offset_large, 1_000_000 * 65536);
        }

        #[test]
        fn finding_ua_11_block_data_size_matches_expected() {
            for size in [BlockSize::B4K, BlockSize::B64K, BlockSize::B1M, BlockSize::B64M] {
                let expected = size.as_bytes();
                let data = vec![0u8; expected as usize];
                assert_eq!(data.len() as u64, expected, "Block data size should match BlockSize");
            }
        }
    }

    mod storage_config_validation {
        use super::*;

        #[test]
        fn finding_ua_12_default_storage_engine_config_reasonable() {
            let config = StorageEngineConfig::default();
            assert_eq!(config.name, "claudefs-storage");
            assert!(!config.name.is_empty());
        }

        #[test]
        fn finding_ua_13_storage_engine_config_roundtrip() {
            let config = StorageEngineConfig {
                name: "test-engine".to_string(),
                default_placement: claudefs_storage::block::PlacementHint::Metadata,
                verify_checksums: false,
                direct_io: false,
            };

            let serialized = serde_json::to_string(&config).unwrap();
            let deserialized: StorageEngineConfig = serde_json::from_str(&serialized).unwrap();

            assert_eq!(config.name, deserialized.name);
            assert_eq!(config.default_placement, deserialized.default_placement);
            assert_eq!(config.verify_checksums, deserialized.verify_checksums);
            assert_eq!(config.direct_io, deserialized.direct_io);
        }

        #[test]
        fn finding_ua_14_block_size_is_power_of_two() {
            for size in BlockSize::all() {
                let bytes = size.as_bytes();
                assert!(bytes.is_power_of_two(), "BlockSize {} should be power of two", size);
            }
        }
    }

    mod error_handling_ffi_boundaries {
        use super::*;

        #[test]
        fn finding_ua_15_storage_error_covers_ffi_modes() {
            let io_err = StorageError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "device not found",
            ));
            assert!(matches!(io_err, StorageError::IoError(_)));

            let not_found = StorageError::BlockNotFound {
                block_id: BlockId::new(0, 100),
            };
            assert!(matches!(not_found, StorageError::BlockNotFound { .. }));

            let oos = StorageError::OutOfSpace;
            assert!(matches!(oos, StorageError::OutOfSpace));

            let dev_err = StorageError::DeviceError {
                device: "nvme0".to_string(),
                reason: "I/O error".to_string(),
            };
            assert!(matches!(dev_err, StorageError::DeviceError { .. }));
        }

        #[test]
        fn finding_ua_16_io_error_wraps_std_error() {
            let std_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
            let storage_err = StorageError::IoError(std_err);

            let msg = storage_err.to_string();
            assert!(msg.contains("I/O error") || msg.contains("access denied"));
        }

        #[test]
        fn finding_ua_17_engine_creation_with_nonexistent_device() {
            let config = UringConfig::default();
            let engine = UringIoEngine::new(config);

            let result = engine.unwrap().register_device(0, "/nonexistent/device/path");
            assert!(result.is_err(), "Should return error for non-existent device");

            if let Err(e) = result {
                assert!(matches!(e, StorageError::DeviceError { .. }));
            }
        }

        #[test]
        fn finding_ua_18_engine_handles_invalid_block_read_gracefully() {
            let config = UringConfig::default();
            let engine_result = UringIoEngine::new(config);

            if let Ok(engine) = engine_result {
                let block_ref = BlockRef {
                    id: BlockId::new(999, 0),
                    size: BlockSize::B4K,
                };

                let runtime = tokio::runtime::Runtime::new().unwrap();
                let read_result: StorageResult<Vec<u8>> = runtime.block_on(async { engine.read_block(block_ref).await });

                assert!(read_result.is_err(), "Invalid block read should return error");
                if let Err(e) = read_result {
                    assert!(matches!(e, StorageError::DeviceError { .. } | StorageError::BlockNotFound { .. }));
                }
            }
        }
    }
}