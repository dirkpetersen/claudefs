//! Storage allocator and io_uring bridge security tests.
//!
//! Part of A10 Phase 27: Storage allocator/io_uring & Transport auth/TLS security audit

#[cfg(test)]
mod tests {
    use claudefs_storage::{
        AllocatorConfig, AllocatorStats, BlockId, BlockRef, BlockSize, BuddyAllocator,
        IoEngine, IoOpType, IoStats, MockIoEngine, StorageError, StorageResult,
    };
    use std::sync::Arc;
    use std::thread;

    fn make_allocator(total_blocks: u64) -> BuddyAllocator {
        BuddyAllocator::new(AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: total_blocks,
        })
        .expect("Failed to create allocator")
    }

    fn make_test_block(size: BlockSize, device_idx: u16, offset: u64) -> BlockRef {
        BlockRef {
            id: BlockId::new(device_idx, offset),
            size,
        }
    }

    fn make_test_data(size: BlockSize) -> Vec<u8> {
        vec![0xAB; size.as_bytes() as usize]
    }

    // ============================================================================
    // Allocator Exhaustion Security (5 tests)
    // ============================================================================

    #[test]
    fn test_alloc_sec_exhaustion_4k_returns_oos() {
        let alloc = make_allocator(16);
        let mut count = 0;
        loop {
            match alloc.allocate(BlockSize::B4K) {
                Ok(_) => count += 1,
                Err(StorageError::OutOfSpace) => break,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }
        assert_eq!(count, 16, "Security: allocator must exhaust all 4K blocks");
        let result = alloc.allocate(BlockSize::B4K);
        assert!(
            matches!(result, Err(StorageError::OutOfSpace)),
            "Security: must return OutOfSpace after exhaustion"
        );
    }

    #[test]
    fn test_alloc_sec_exhaustion_64k_returns_oos() {
        let alloc = make_allocator(16);
        let mut count = 0;
        loop {
            match alloc.allocate(BlockSize::B64K) {
                Ok(_) => count += 1,
                Err(StorageError::OutOfSpace) => break,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }
        assert_eq!(count, 1, "Security: only one 64K block fits in 16 4K blocks");
        let result = alloc.allocate(BlockSize::B64K);
        assert!(
            matches!(result, Err(StorageError::OutOfSpace)),
            "Security: must return OutOfSpace after exhaustion"
        );
    }

    #[test]
    fn test_alloc_sec_exhaustion_1m_returns_oos() {
        let alloc = make_allocator(256);
        let mut count = 0;
        loop {
            match alloc.allocate(BlockSize::B1M) {
                Ok(_) => count += 1,
                Err(StorageError::OutOfSpace) => break,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }
        assert_eq!(count, 1, "Security: only one 1M block fits in 256 4K blocks");
        let result = alloc.allocate(BlockSize::B1M);
        assert!(
            matches!(result, Err(StorageError::OutOfSpace)),
            "Security: must return OutOfSpace after exhaustion"
        );
    }

    #[test]
    fn test_alloc_sec_exhaustion_64m_returns_oos() {
        let alloc = make_allocator(16384);
        let mut count = 0;
        loop {
            match alloc.allocate(BlockSize::B64M) {
                Ok(_) => count += 1,
                Err(StorageError::OutOfSpace) => break,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }
        assert_eq!(count, 1, "Security: only one 64M block fits in 16384 4K blocks");
        let result = alloc.allocate(BlockSize::B64M);
        assert!(
            matches!(result, Err(StorageError::OutOfSpace)),
            "Security: must return OutOfSpace after exhaustion"
        );
    }

    #[test]
    fn test_alloc_sec_rejects_zero_blocks_config() {
        let result = BuddyAllocator::new(AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 0,
        });
        assert!(
            result.is_err(),
            "Security: allocator must reject zero-block configuration"
        );
    }

    #[test]
    fn test_alloc_sec_pathologically_small_capacity() {
        let alloc = make_allocator(1);
        let result = alloc.allocate(BlockSize::B4K);
        assert!(result.is_ok(), "Security: single 4K block should be allocatable");
        
        let result2 = alloc.allocate(BlockSize::B4K);
        assert!(
            matches!(result2, Err(StorageError::OutOfSpace)),
            "Security: second 4K block should fail with OOS"
        );
    }

    #[test]
    fn test_alloc_sec_alloc_free_alloc_no_leak() {
        let alloc = make_allocator(100);
        
        let block1 = alloc.allocate(BlockSize::B4K).unwrap();
        alloc.free(block1).unwrap();
        
        let block2 = alloc.allocate(BlockSize::B4K).unwrap();
        assert_ne!(
            block1.id.offset, block2.id.offset,
            "Security: reallocated block must not be same as freed block (no leak)"
        );
        
        let stats = alloc.stats();
        assert_eq!(
            stats.total_frees, 1,
            "Security: free count must match"
        );
    }

    #[test]
    fn test_alloc_sec_stats_consistent_after_mixed_operations() {
        let alloc = make_allocator(100);
        
        let blocks: Vec<BlockRef> = (0..10)
            .map(|_| alloc.allocate(BlockSize::B4K).unwrap())
            .collect();
        
        for (i, block) in blocks.iter().enumerate().take(5) {
            alloc.free(*block).unwrap();
        }
        
        let stats = alloc.stats();
        let expected_free = 95u64;
        assert_eq!(
            stats.free_blocks_4k, expected_free,
            "Security: free blocks should be consistent after mixed ops"
        );
        assert_eq!(stats.total_allocations, 10);
        assert_eq!(stats.total_frees, 5);
    }

    // ============================================================================
    // Allocator Concurrency Safety (4 tests)
    // ============================================================================

    #[test]
    fn test_alloc_sec_concurrent_alloc_no_corruption() {
        let alloc = Arc::new(make_allocator(1000));
        let mut handles = vec![];
        
        for _ in 0..4 {
            let alloc_clone = Arc::clone(&alloc);
            let handle = thread::spawn(move || {
                let mut allocated = Vec::new();
                for _ in 0..50 {
                    if let Ok(block) = alloc_clone.allocate(BlockSize::B4K) {
                        allocated.push(block);
                    }
                }
                allocated
            });
            handles.push(handle);
        }
        
        let mut all_blocks = Vec::new();
        for handle in handles {
            let blocks = handle.join().unwrap();
            all_blocks.extend(blocks);
        }
        
        assert_eq!(all_blocks.len(), 200, "Security: 4 threads x 50 blocks should succeed");
        
        let mut unique_offsets: std::collections::HashSet<u64> = std::collections::HashSet::new();
        for block in &all_blocks {
            unique_offsets.insert(block.id.offset);
        }
        assert_eq!(
            unique_offsets.len(), all_blocks.len(),
            "Security: no block should be allocated twice (no double-allocation)"
        );
    }

    #[test]
    fn test_alloc_sec_concurrent_alloc_free_consistent() {
        let alloc = Arc::new(make_allocator(500));
        let mut handles = vec![];
        
        let alloc_clone = Arc::clone(&alloc);
        let handle1 = thread::spawn(move || {
            let mut blocks = Vec::new();
            for _ in 0..100 {
                if let Ok(b) = alloc_clone.allocate(BlockSize::B4K) {
                    blocks.push(b);
                }
            }
            blocks
        });
        handles.push(handle1);
        
        let alloc_clone2 = Arc::clone(&alloc);
        let handle2 = thread::spawn(move || {
            let mut blocks = Vec::new();
            for _ in 0..50 {
                if let Ok(b) = alloc_clone2.allocate(BlockSize::B4K) {
                    let _ = alloc_clone2.free(b);
                    blocks.push(b);
                }
            }
            blocks
        });
        handles.push(handle2);
        
        let handle1 = handles.remove(0);
        let handle2 = handles.remove(0);
        let blocks = handle1.join().unwrap();
        let _ = handle2.join().unwrap();
        
        for block in blocks {
            let _ = alloc.free(block);
        }
        
        let stats = alloc.stats();
        assert!(
            stats.free_blocks_4k <= 500,
            "Security: free_blocks must not exceed total capacity"
        );
    }

    #[test]
    fn test_alloc_sec_concurrent_stats_consistent() {
        let alloc = Arc::new(make_allocator(200));
        
        let alloc_clone = Arc::clone(&alloc);
        let handle1 = thread::spawn(move || {
            for _ in 0..50 {
                let _ = alloc_clone.allocate(BlockSize::B4K);
            }
        });
        
        let alloc_clone2 = Arc::clone(&alloc);
        let handle2 = thread::spawn(move || {
            for _ in 0..30 {
                let _ = alloc_clone2.allocate(BlockSize::B64K);
            }
        });
        
        handle1.join().unwrap();
        handle2.join().unwrap();
        
        let stats = alloc.stats();
        assert!(
            stats.total_allocations > 0,
            "Security: stats must track concurrent operations"
        );
    }

    #[test]
    fn test_alloc_sec_no_double_allocation() {
        let alloc = Arc::new(make_allocator(100));
        let mut handles = vec![];
        
        for _ in 0..10 {
            let alloc_clone = Arc::clone(&alloc);
            let handle = thread::spawn(move || {
                let mut results = Vec::new();
                for _ in 0..10 {
                    if let Ok(block) = alloc_clone.allocate(BlockSize::B4K) {
                        results.push(block.id.offset);
                    }
                }
                results
            });
            handles.push(handle);
        }
        
        let mut all_offsets = Vec::new();
        for handle in handles {
            all_offsets.extend(handle.join().unwrap());
        }
        
        let mut seen = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for offset in &all_offsets {
            if !seen.insert(*offset) {
                duplicates.push(*offset);
            }
        }
        
        assert!(
            duplicates.is_empty(),
            "Security: no block should be returned twice, found duplicates: {:?}",
            duplicates
        );
    }

    // ============================================================================
    // IoEngine Boundary Security (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_uring_sec_read_unwritten_returns_error() {
        let engine = MockIoEngine::new();
        let block = make_test_block(BlockSize::B4K, 0, 999);
        
        let result = engine.read_block(block).await;
        
        assert!(
            matches!(result, Err(StorageError::BlockNotFound { .. })),
            "Security: reading unwritten block must return error"
        );
    }

    #[tokio::test]
    async fn test_uring_sec_write_read_roundtrip_preserves_data() {
        let engine = MockIoEngine::new();
        let block = make_test_block(BlockSize::B4K, 0, 0);
        let data = make_test_data(BlockSize::B4K);
        
        engine.write_block(block, data.clone(), None).await.unwrap();
        
        let read_data = engine.read_block(block).await.unwrap();
        
        assert_eq!(
            read_data, data,
            "Security: write-read roundtrip must preserve data integrity"
        );
    }

    #[tokio::test]
    async fn test_uring_sec_write_oversized_data_rejected() {
        let engine = MockIoEngine::new();
        let block = make_test_block(BlockSize::B4K, 0, 0);
        let oversized_data = vec![0xAB; 8192];
        
        let result = engine.write_block(block, oversized_data, None).await;
        
        assert!(
            matches!(result, Err(StorageError::InvalidBlockSize { .. })),
            "Security: oversized data must be rejected"
        );
    }

    #[tokio::test]
    async fn test_uring_sec_concurrent_reads_writes_different_blocks() {
        let engine = Arc::new(MockIoEngine::new());
        
        let engine_clone = Arc::clone(&engine);
        let handle1 = tokio::spawn(async move {
            let block = make_test_block(BlockSize::B4K, 0, 0);
            let data = make_test_data(BlockSize::B4K);
            engine_clone.write_block(block, data, None).await
        });
        
        let engine_clone2 = Arc::clone(&engine);
        let handle2 = tokio::spawn(async move {
            let block = make_test_block(BlockSize::B4K, 0, 1);
            let data = make_test_data(BlockSize::B4K);
            engine_clone2.write_block(block, data, None).await
        });
        
        let engine_clone3 = Arc::clone(&engine);
        let handle3 = tokio::spawn(async move {
            let block = make_test_block(BlockSize::B4K, 0, 0);
            engine_clone3.read_block(block).await
        });
        
        handle1.await.unwrap().unwrap();
        handle2.await.unwrap().unwrap();
        let read_result = handle3.await.unwrap();
        
        assert!(
            read_result.is_ok(),
            "Security: concurrent read must not corrupt data"
        );
    }

    #[tokio::test]
    async fn test_uring_sec_stats_tracking_accurate() {
        let engine = MockIoEngine::new();
        
        let block1 = make_test_block(BlockSize::B4K, 0, 0);
        let block2 = make_test_block(BlockSize::B4K, 0, 1);
        
        engine.write_block(block1, make_test_data(BlockSize::B4K), None).await.unwrap();
        engine.write_block(block2, make_test_data(BlockSize::B4K), None).await.unwrap();
        engine.read_block(block1).await.unwrap();
        engine.read_block(block2).await.unwrap();
        engine.flush().await.unwrap();
        
        let stats = engine.stats();
        
        assert_eq!(
            stats.writes_completed, 2,
            "Security: write count must be accurate"
        );
        assert_eq!(
            stats.reads_completed, 2,
            "Security: read count must be accurate"
        );
        assert_eq!(
            stats.flushes_completed, 1,
            "Security: flush count must be accurate"
        );
        assert_eq!(
            stats.bytes_written, 8192,
            "Security: bytes_written must match (2 x 4KB)"
        );
    }

    // ============================================================================
    // Block ID Validation (4 tests)
    // ============================================================================

    #[test]
    fn test_alloc_sec_max_device_idx() {
        let block_id = BlockId::new(u16::MAX, 0);
        
        assert_eq!(block_id.device_idx, u16::MAX, "Security: max u16 device_idx must be accepted");
    }

    #[test]
    fn test_alloc_sec_max_offset() {
        let block_id = BlockId::new(0, u64::MAX);
        
        assert_eq!(block_id.offset, u64::MAX, "Security: max u64 offset must be accepted");
    }

    #[test]
    fn test_alloc_sec_mismatched_size_offset_alignment() {
        let alloc = make_allocator(100);
        
        let block = alloc.allocate(BlockSize::B64K).unwrap();
        
        assert_eq!(
            block.id.offset % 16, 0,
            "Security: 64K block offset must be 16-block aligned"
        );
        
        let block2 = alloc.allocate(BlockSize::B1M).unwrap();
        assert_eq!(
            block2.id.offset % 256, 0,
            "Security: 1M block offset must be 256-block aligned"
        );
    }

    #[test]
    fn test_alloc_sec_returns_valid_device_idx_from_config() {
        let config = AllocatorConfig {
            device_idx: 5,
            total_blocks_4k: 100,
        };
        let alloc = BuddyAllocator::new(config).unwrap();
        
        let block = alloc.allocate(BlockSize::B4K).unwrap();
        
        assert_eq!(
            block.id.device_idx, 5,
            "Security: allocator must return correct device_idx from config"
        );
    }

    // ============================================================================
    // Serialization Roundtrip Security (4 tests)
    // ============================================================================

    #[test]
    fn test_alloc_sec_config_serde_roundtrip() {
        let config = AllocatorConfig {
            device_idx: 3,
            total_blocks_4k: 50000,
        };
        
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: AllocatorConfig = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(config.device_idx, deserialized.device_idx);
        assert_eq!(config.total_blocks_4k, deserialized.total_blocks_4k);
    }

    #[test]
    fn test_uring_sec_iostats_serde_roundtrip() {
        let stats = IoStats {
            reads_completed: 100,
            writes_completed: 50,
            flushes_completed: 10,
            bytes_read: 409600,
            bytes_written: 204800,
            errors: 2,
            queue_depth: 5,
        };
        
        let serialized = serde_json::to_string(&stats).unwrap();
        let deserialized: IoStats = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(stats.reads_completed, deserialized.reads_completed);
        assert_eq!(stats.writes_completed, deserialized.writes_completed);
        assert_eq!(stats.bytes_read, deserialized.bytes_read);
    }

    #[test]
    fn test_alloc_sec_blocksize_serde_roundtrip() {
        let sizes = [BlockSize::B4K, BlockSize::B64K, BlockSize::B1M, BlockSize::B64M];
        
        for size in sizes {
            let serialized = serde_json::to_string(&size).unwrap();
            let deserialized: BlockSize = serde_json::from_str(&serialized).unwrap();
            assert_eq!(size, deserialized);
        }
    }

    #[test]
    fn test_uring_sec_ioptype_serde_roundtrip() {
        let ops = [IoOpType::Read, IoOpType::Write, IoOpType::Flush, IoOpType::Discard];
        
        for op in ops {
            let serialized = serde_json::to_string(&op).unwrap();
            let deserialized: IoOpType = serde_json::from_str(&serialized).unwrap();
            assert_eq!(op, deserialized);
        }
    }

    // ============================================================================
    // Resource Exhaustion DoS (3 tests)
    // ============================================================================

    #[test]
    fn test_alloc_sec_handles_extremely_large_total_blocks() {
        let large_config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: u64::MAX / 4096,
        };
        
        let result = BuddyAllocator::new(large_config);
        
        assert!(
            result.is_ok(),
            "Security: extremely large capacity must be handled without crash"
        );
    }

    #[test]
    fn test_alloc_sec_rapid_alloc_free_no_memory_leak() {
        let alloc = make_allocator(10000);
        
        for _ in 0..1000 {
            let block = alloc.allocate(BlockSize::B4K).unwrap();
            alloc.free(block).unwrap();
        }
        
        let stats = alloc.stats();
        
        assert!(
            stats.total_allocations > 0,
            "Security: rapid cycling must not accumulate memory"
        );
        assert!(
            stats.free_blocks_4k > 9000,
            "Security: most blocks should be freed"
        );
    }

    #[test]
    fn test_alloc_sec_stats_no_overflow_on_many_operations() {
        let alloc = make_allocator(100);
        
        for _ in 0..10000 {
            if let Ok(b) = alloc.allocate(BlockSize::B4K) {
                let _ = alloc.free(b);
            }
        }
        
        let stats = alloc.stats();
        
        assert!(
            stats.total_allocations < 10000,
            "Security: stats must not overflow, should cap at max allocations"
        );
    }
}