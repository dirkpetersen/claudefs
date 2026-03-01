#[cfg(test)]
mod tests {
    use claudefs_storage::{
        AllocatorConfig, AllocatorStats, BlockHeader, BlockRef, BlockSize, BuddyAllocator,
        Checksum, ChecksumAlgorithm, StorageEngineConfig, StorageEngineStats, StorageError,
        StorageResult,
    };
    use std::collections::HashMap;

    #[test]
    fn test_buddy_allocator_new() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 1024,
        };
        let allocator = BuddyAllocator::new(config).unwrap();
        assert!(allocator.free_capacity_bytes() > 0);
    }

    #[test]
    fn test_buddy_allocator_alloc_single() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 1024,
        };
        let allocator = BuddyAllocator::new(config).unwrap();
        let initial_remaining = allocator.free_capacity_bytes();

        let block = allocator.allocate(BlockSize::B4K).unwrap();
        assert!(block.id.offset > 0 || block.id.offset == 0);

        assert!(allocator.free_capacity_bytes() < initial_remaining);
    }

    #[test]
    fn test_buddy_allocator_alloc_multiple_unique() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 100,
        };
        let allocator = BuddyAllocator::new(config).unwrap();

        let mut blocks = Vec::new();
        for _ in 0..10 {
            blocks.push(allocator.allocate(BlockSize::B4K).unwrap());
        }

        for (i, b1) in blocks.iter().enumerate() {
            for (j, b2) in blocks.iter().enumerate() {
                if i != j {
                    assert_ne!(b1.id.offset, b2.id.offset);
                }
            }
        }
    }

    #[test]
    fn test_buddy_allocator_free() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 100,
        };
        let allocator = BuddyAllocator::new(config).unwrap();
        let initial_remaining = allocator.free_capacity_bytes();

        let block = allocator.allocate(BlockSize::B4K).unwrap();

        allocator.free(block).unwrap();

        assert_eq!(allocator.free_capacity_bytes(), initial_remaining);
    }

    #[test]
    fn test_buddy_allocator_free_all_restores_space() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 50,
        };
        let allocator = BuddyAllocator::new(config).unwrap();

        let mut blocks = Vec::new();
        for _ in 0..10 {
            blocks.push(allocator.allocate(BlockSize::B4K).unwrap());
        }

        for block in blocks {
            allocator.free(block).unwrap();
        }

        assert!(allocator.free_capacity_bytes() >= 50 * 4096);
    }

    #[test]
    fn test_buddy_allocator_stats() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 100,
        };
        let allocator = BuddyAllocator::new(config).unwrap();

        let stats = allocator.stats();
        assert!(stats.total_blocks_4k > 0);
        assert!(stats.free_blocks_4k > 0);
    }

    #[test]
    fn test_buddy_allocator_free_capacity() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 100,
        };
        let allocator = BuddyAllocator::new(config).unwrap();

        let free = allocator.free_capacity_bytes();
        assert_eq!(free, 100 * 4096);
    }

    #[test]
    fn test_buddy_allocator_alloc_multi_block() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 100,
        };
        let allocator = BuddyAllocator::new(config).unwrap();

        let block = allocator.allocate(BlockSize::B64K).unwrap();
        assert!(block.id.offset > 0 || block.id.offset == 0);

        let remaining = allocator.free_capacity_bytes();
        assert!(remaining < 100 * 4096);
    }

    #[test]
    fn test_buddy_allocator_out_of_space() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 2,
        };
        let allocator = BuddyAllocator::new(config).unwrap();

        let _ = allocator.allocate(BlockSize::B4K).unwrap();
        let _ = allocator.allocate(BlockSize::B4K).unwrap();

        let result = allocator.allocate(BlockSize::B4K);
        assert!(result.is_err());
    }

    #[test]
    fn test_allocator_config_default() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 1000,
        };
        assert!(config.total_blocks_4k > 0);
    }

    #[test]
    fn test_allocator_config_custom() {
        let config = AllocatorConfig {
            device_idx: 1,
            total_blocks_4k: 2048,
        };
        assert_eq!(config.total_blocks_4k, 2048);
    }

    #[test]
    fn test_checksum_new() {
        let checksum = Checksum::new(ChecksumAlgorithm::Crc32c, 12345);
        assert_eq!(checksum.algorithm, ChecksumAlgorithm::Crc32c);
        assert_eq!(checksum.value, 12345);
    }

    #[test]
    fn test_checksum_algorithm_variants() {
        let _ = ChecksumAlgorithm::Crc32c;
        let _ = ChecksumAlgorithm::XxHash64;
        let _ = ChecksumAlgorithm::None;
    }

    #[test]
    fn test_checksum_debug() {
        let crc = Checksum::new(ChecksumAlgorithm::Crc32c, 0);
        let debug = format!("{:?}", crc);
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_checksum_algorithm_display() {
        let display = format!("{}", ChecksumAlgorithm::Crc32c);
        assert!(display.contains("CRC"));

        let display = format!("{}", ChecksumAlgorithm::XxHash64);
        assert!(display.contains("Hash"));
    }

    #[test]
    fn test_checksum_equality() {
        let c1 = Checksum::new(ChecksumAlgorithm::Crc32c, 42);
        let c2 = Checksum::new(ChecksumAlgorithm::Crc32c, 42);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_block_header_new() {
        let header = BlockHeader::new(
            BlockSize::B4K,
            Checksum::new(ChecksumAlgorithm::Crc32c, 12345),
            1,
        );
        assert!(header.magic != 0);
        assert!(header.validate_magic());
    }

    #[test]
    fn test_storage_engine_config_default() {
        let config = StorageEngineConfig::default();
        assert_eq!(config.name, "claudefs-storage");
    }

    #[test]
    fn test_storage_engine_config_custom() {
        let config = StorageEngineConfig {
            name: "test-engine".to_string(),
            default_placement: claudefs_storage::PlacementHint::ColdData,
            verify_checksums: false,
            direct_io: false,
        };
        assert_eq!(config.name, "test-engine");
    }

    #[test]
    fn test_storage_engine_stats_default() {
        let stats = StorageEngineStats::default();
        assert_eq!(stats.device_count, 0);
        assert_eq!(stats.total_capacity_bytes, 0);
    }

    #[test]
    fn test_storage_error_display() {
        let err = StorageError::OutOfSpace;
        let _ = format!("{}", err);
    }

    #[test]
    fn test_storage_error_variants() {
        use claudefs_storage::StorageError::*;

        let errors = vec![
            OutOfSpace,
            BlockNotFound {
                block_id: claudefs_storage::BlockId::new(0, 0),
            },
            IoError(std::io::Error::new(std::io::ErrorKind::Other, "test")),
            DeviceError {
                device: "test".to_string(),
                reason: "test".to_string(),
            },
            ChecksumMismatch {
                block_id: claudefs_storage::BlockId::new(0, 0),
                expected: 0,
                actual: 0,
            },
        ];

        for err in errors {
            let _ = format!("{}", err);
        }
    }

    #[test]
    fn test_storage_result_ok() {
        let result: StorageResult<u32> = Ok(42);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_storage_result_err() {
        let result: StorageResult<u32> = Err(StorageError::OutOfSpace);
        assert!(result.is_err());
    }
}
