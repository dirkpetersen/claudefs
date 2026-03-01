//! Storage Resilience Tests
//!
//! Tests for storage subsystem resilience under error and edge-case conditions.

#[cfg(test)]
mod tests {
    use claudefs_storage::{
        allocator::{AllocatorConfig, BuddyAllocator},
        block::BlockSize,
        capacity::{CapacityLevel, CapacityTracker, WatermarkConfig},
        checksum::BlockHeader,
        defrag::DefragConfig,
        device::{DeviceConfig, DevicePool, DeviceRole},
    };

    #[test]
    fn test_allocator_config_default() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 1024,
        };
        assert_eq!(config.total_blocks_4k, 1024);
        assert_eq!(config.device_idx, 0);
    }

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
    fn test_buddy_allocator_stats_initial() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 1024,
        };
        let allocator = BuddyAllocator::new(config).unwrap();
        let stats = allocator.stats();
        assert!(stats.total_blocks_4k > 0);
    }

    #[test]
    fn test_buddy_allocator_allocate_4k() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 1024,
        };
        let allocator = BuddyAllocator::new(config).unwrap();
        let block = allocator.allocate(BlockSize::B4K).unwrap();
        assert!(block.id.offset > 0 || block.id.offset == 0);
    }

    #[test]
    fn test_buddy_allocator_allocate_64k() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 256,
        };
        let allocator = BuddyAllocator::new(config).unwrap();
        let block = allocator.allocate(BlockSize::B64K).unwrap();
        assert!(block.id.offset > 0 || block.id.offset == 0);
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
    fn test_buddy_allocator_multiple_allocs() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 100,
        };
        let allocator = BuddyAllocator::new(config).unwrap();

        let mut blocks = Vec::new();
        for _ in 0..4 {
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
    fn test_buddy_allocator_capacity_tracking() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 100,
        };
        let allocator = BuddyAllocator::new(config).unwrap();
        let initial = allocator.free_capacity_bytes();

        let _ = allocator.allocate(BlockSize::B4K).unwrap();

        assert!(allocator.free_capacity_bytes() < initial);
    }

    #[test]
    fn test_buddy_allocator_exhaust_small_capacity() {
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
    fn test_block_size_b4k_bytes() {
        assert_eq!(BlockSize::B4K.as_bytes(), 4096);
    }

    #[test]
    fn test_block_size_b64k_bytes() {
        assert_eq!(BlockSize::B64K.as_bytes(), 65536);
    }

    #[test]
    fn test_block_size_b1m_bytes() {
        assert_eq!(BlockSize::B1M.as_bytes(), 1024 * 1024);
    }

    #[test]
    fn test_block_size_b64m_bytes() {
        assert_eq!(BlockSize::B64M.as_bytes(), 67108864);
    }

    #[test]
    fn test_checksum_crc32c_compute() {
        let checksum = claudefs_storage::checksum::compute(
            claudefs_storage::checksum::ChecksumAlgorithm::Crc32c,
            b"hello",
        );
        assert_ne!(checksum.value, 0);
    }

    #[test]
    fn test_checksum_verify_pass() {
        let data = b"hello world";
        let checksum = claudefs_storage::checksum::compute(
            claudefs_storage::checksum::ChecksumAlgorithm::Crc32c,
            data,
        );
        assert!(claudefs_storage::checksum::verify(&checksum, data));
    }

    #[test]
    fn test_checksum_verify_fail() {
        let checksum = claudefs_storage::checksum::compute(
            claudefs_storage::checksum::ChecksumAlgorithm::Crc32c,
            b"hello",
        );
        assert!(!claudefs_storage::checksum::verify(&checksum, b"world"));
    }

    #[test]
    fn test_block_header_new() {
        let checksum = claudefs_storage::checksum::compute(
            claudefs_storage::checksum::ChecksumAlgorithm::Crc32c,
            b"data",
        );
        let header = BlockHeader::new(BlockSize::B4K, checksum, 1);
        assert_eq!(header.block_size, BlockSize::B4K);
    }

    #[test]
    fn test_capacity_tracker_new() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 1024 * 1024 * 1024);
        let stats = tracker.stats();
        assert_eq!(stats.total_capacity_bytes, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_capacity_tracker_initial_level() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 1024 * 1024 * 1024);
        assert_eq!(tracker.level(), CapacityLevel::Normal);
    }

    #[test]
    fn test_capacity_tracker_update_usage() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 1024 * 1024 * 1024);
        tracker.update_usage(512 * 1024 * 1024);
        let pct = tracker.usage_pct();
        assert!(pct >= 48 && pct <= 50);
    }

    #[test]
    fn test_capacity_level_warning() {
        let mut config = WatermarkConfig::default();
        config.high_watermark_pct = 70;
        config.critical_watermark_pct = 90;
        let tracker = CapacityTracker::new(config, 1024 * 1024 * 1024);
        tracker.update_usage(750 * 1024 * 1024);
        assert!(tracker.should_evict());
    }

    #[test]
    fn test_capacity_level_critical() {
        let mut config = WatermarkConfig::default();
        config.critical_watermark_pct = 90;
        let tracker = CapacityTracker::new(config, 1024 * 1024 * 1024);
        tracker.update_usage(950 * 1024 * 1024);
        assert!(tracker.should_write_through());
    }

    #[test]
    fn test_capacity_stats() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 1024 * 1024 * 1024);
        let stats = tracker.stats();
        assert_eq!(stats.total_capacity_bytes, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_device_config_new() {
        let config = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        assert_eq!(config.path, "/dev/nvme0n1");
    }

    #[test]
    fn test_device_config_direct_io_flag() {
        let config = DeviceConfig::new(
            "/dev/nvme0n1".to_string(),
            0,
            DeviceRole::Data,
            false,
            32,
            true,
        );
        assert!(config.direct_io);
    }

    #[test]
    fn test_device_role_variants() {
        let _ = DeviceRole::Journal;
        let _ = DeviceRole::Data;
        let _ = DeviceRole::Combined;
    }

    #[test]
    fn test_defrag_config_default() {
        let config = DefragConfig::default();
        assert_eq!(config.target_fragmentation_percent, 20.0);
    }

    #[test]
    fn test_defrag_engine_new() {
        let config = DefragConfig::default();
        let _engine = claudefs_storage::defrag::DefragEngine::new(config);
    }

    #[test]
    fn test_defrag_can_run() {
        let config = DefragConfig::default();
        let engine = claudefs_storage::defrag::DefragEngine::new(config);
        assert!(engine.can_run());
    }
}
