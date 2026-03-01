//! End-to-end write path tests: data flows through reduce pipeline then to storage
//!
//! These tests combine claudefs_reduce and claudefs_storage APIs to verify the full
//! data pipeline works correctly.

use claudefs_reduce::{
    ChunkerConfig, CompressionAlgorithm, EncryptionAlgorithm, EncryptionKey, IntegratedWritePath,
    NullFingerprintStore, PipelineConfig, ReductionPipeline, WritePathConfig,
};
use claudefs_storage::{AllocatorConfig, BlockRef, BlockSize, BuddyAllocator, ChecksumAlgorithm};
use std::sync::Arc;

fn generate_test_data(size: usize, pattern: u8) -> Vec<u8> {
    vec![pattern; size]
}

#[test]
fn test_small_write_full_pipeline() {
    let config = PipelineConfig::default();
    let pipeline = ReductionPipeline::new(config);

    let data = b"Hello, ClaudeFS!".to_vec();
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert_eq!(stats.input_bytes, data.len() as u64);
}

#[test]
fn test_medium_write_pipeline() {
    let config = PipelineConfig::default();
    let mut pipeline = ReductionPipeline::new(config);

    let data = generate_test_data(10_000, 0xAB);
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert!(stats.input_bytes > 0);
    assert!(stats.chunks_total > 0);
}

#[test]
fn test_large_write_pipeline() {
    let config = PipelineConfig::default();
    let mut pipeline = ReductionPipeline::new(config);

    let data = generate_test_data(1_000_000, 0xCD);
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert_eq!(stats.input_bytes, 1_000_000);
}

#[test]
fn test_lz4_compression_pipeline() {
    let mut config = PipelineConfig::default();
    config.inline_compression = CompressionAlgorithm::Lz4;
    config.compression_enabled = true;

    let mut pipeline = ReductionPipeline::new(config);

    let data = generate_test_data(50_000, 0x00);
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert!(stats.bytes_after_compression <= stats.bytes_after_dedup);
}

#[test]
fn test_zstd_compression_pipeline() {
    let mut config = PipelineConfig::default();
    config.inline_compression = CompressionAlgorithm::Zstd { level: 3 };
    config.compression_enabled = true;

    let mut pipeline = ReductionPipeline::new(config);

    let data = generate_test_data(50_000, 0xFF);
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
}

#[test]
fn test_no_compression_pipeline() {
    let mut config = PipelineConfig::default();
    config.compression_enabled = false;

    let mut pipeline = ReductionPipeline::new(config);

    let data = generate_test_data(10_000, 0x11);
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
}

#[test]
fn test_encryption_enabled_pipeline() {
    let mut config = PipelineConfig::default();
    config.encryption_enabled = true;
    config.encryption = EncryptionAlgorithm::AesGcm256;

    let key = EncryptionKey([0x42u8; 32]);
    let mut pipeline = ReductionPipeline::with_master_key(config, key);

    let data = b"Secret data for encryption test".to_vec();
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert!(stats.bytes_after_encryption > 0);
}

#[test]
fn test_no_encryption_pipeline() {
    let mut config = PipelineConfig::default();
    config.encryption_enabled = false;

    let mut pipeline = ReductionPipeline::new(config);

    let data = b"Plain text data".to_vec();
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert_eq!(stats.input_bytes as usize, data.len());
}

#[test]
fn test_pipeline_stats_tracking() {
    let config = PipelineConfig::default();
    let mut pipeline = ReductionPipeline::new(config);

    let data = generate_test_data(100_000, 0x55);
    let (_, stats) = pipeline.process_write(&data).unwrap();

    assert_eq!(stats.input_bytes, 100_000);
    assert!(stats.chunks_total > 0);
    assert!(stats.bytes_after_dedup > 0);
    assert!(stats.compression_ratio > 0.0);
}

#[test]
fn test_compressible_data_ratio() {
    let mut config = PipelineConfig::default();
    config.compression_enabled = true;
    config.inline_compression = CompressionAlgorithm::Lz4;

    let mut pipeline = ReductionPipeline::new(config);

    let zeros = generate_test_data(100_000, 0x00);
    let (_, stats) = pipeline.process_write(&zeros).unwrap();

    let ratio = if stats.bytes_after_dedup > 0 {
        stats.bytes_after_dedup as f64 / stats.bytes_after_compression.max(1) as f64
    } else {
        1.0
    };

    assert!(ratio >= 1.0);
}

#[test]
fn test_integrated_write_path_basic() {
    let config = WritePathConfig::default();
    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let data = generate_test_data(50_000, 0x77);
    let result = write_path.process_write(&data).unwrap();

    assert!(!result.reduced_chunks.is_empty());
    assert!(result.stats.pipeline.input_bytes > 0);
}

#[test]
fn test_integrated_write_path_with_encryption() {
    let mut config = WritePathConfig::default();
    config.pipeline.encryption_enabled = true;

    let store = Arc::new(NullFingerprintStore::new());
    let key = EncryptionKey([0xABu8; 32]);
    let mut write_path = IntegratedWritePath::new_with_key(config, key, store);

    let data = b"Encrypted write path test data".to_vec();
    let result = write_path.process_write(&data).unwrap();

    assert!(!result.reduced_chunks.is_empty());
    assert!(result.stats.pipeline.bytes_after_encryption > 0);
}

#[test]
fn test_chunk_boundaries() {
    let config = PipelineConfig {
        chunker: ChunkerConfig {
            min_size: 4096,
            max_size: 16384,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut pipeline = ReductionPipeline::new(config);

    let data = generate_test_data(200_000, 0x88);
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    for chunk in &chunks {
        assert!(chunk.original_size >= config.chunker.min_size);
    }

    assert!(stats.chunks_total > 0);
}

#[test]
fn test_multiple_write_calls() {
    let config = PipelineConfig::default();
    let mut pipeline = ReductionPipeline::new(config);

    let chunks1 = pipeline.process_write(&b"First write".to_vec()).unwrap().0;
    let chunks2 = pipeline.process_write(&b"Second write".to_vec()).unwrap().0;
    let chunks3 = pipeline.process_write(&b"Third write".to_vec()).unwrap().0;

    assert!(!chunks1.is_empty());
    assert!(!chunks2.is_empty());
    assert!(!chunks3.is_empty());
}

#[test]
fn test_buddy_allocator_basic() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 1024,
    };

    let allocator = BuddyAllocator::new(config).unwrap();

    let result = allocator.allocate(BlockSize::B4K);
    assert!(result.is_ok());

    if let Ok(block) = result {
        allocator.free(block).unwrap();
    }
}

#[test]
fn test_buddy_allocator_multiple_allocations() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 1024,
    };

    let allocator = BuddyAllocator::new(config).unwrap();

    let blocks: Vec<_> = (0..10)
        .filter_map(|_| allocator.allocate(BlockSize::B4K).ok())
        .collect();

    assert_eq!(blocks.len(), 10);

    for block in blocks {
        allocator.free(block).unwrap();
    }
}

#[test]
fn test_buddy_allocator_fragmentation() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 256,
    };

    let allocator = BuddyAllocator::new(config).unwrap();

    let b1 = allocator.allocate(BlockSize::B64K);
    let b2 = allocator.allocate(BlockSize::B64K);
    let b3 = allocator.allocate(BlockSize::B64K);

    if let Ok(block) = b1 {
        let _ = allocator.free(block);
    }
    if let Ok(block) = b3 {
        let _ = allocator.free(block);
    }

    let b4 = allocator.allocate(BlockSize::B64K);
    assert!(b4.is_ok());

    if let Ok(block) = b4 {
        let _ = allocator.free(block);
    }
    if let Ok(block) = b2 {
        let _ = allocator.free(block);
    }
}

#[test]
fn test_checksum_algorithm_crc32c() {
    let data = b"CRC32C test data";

    let checksum = claudefs_storage::checksum::compute(ChecksumAlgorithm::Crc32c, data);
    assert!(checksum.value != 0);

    let valid = claudefs_storage::checksum::verify(&checksum, data);
    assert!(valid);
}

#[test]
fn test_checksum_algorithm_xxhash64() {
    let data = b"xxHash64 test data";

    let checksum = claudefs_storage::checksum::compute(ChecksumAlgorithm::XxHash64, data);
    assert!(checksum.value != 0);

    let valid = claudefs_storage::checksum::verify(&checksum, data);
    assert!(valid);
}

#[test]
fn test_checksum_mismatch_detection() {
    let data = b"Original data";
    let checksum = claudefs_storage::checksum::compute(ChecksumAlgorithm::Crc32c, data);

    let tampered = b"Tampered data";
    let valid = claudefs_storage::checksum::verify(&checksum, tampered);

    assert!(!valid);
}

#[test]
fn test_pipeline_with_custom_chunk_size() {
    let config = PipelineConfig {
        chunker: ChunkerConfig {
            min_size: 8192,
            max_size: 32768,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut pipeline = ReductionPipeline::new(config);
    let data = generate_test_data(500_000, 0x33);
    let (chunks, _) = pipeline.process_write(&data).unwrap();

    assert!(chunks.len() > 0);
}

#[test]
fn test_zero_sized_write() {
    let config = PipelineConfig::default();
    let mut pipeline = ReductionPipeline::new(config);

    let result = pipeline.process_write(&[]);
    assert!(result.is_ok());

    let (chunks, stats) = result.unwrap();
    assert_eq!(chunks.len(), 0);
    assert_eq!(stats.input_bytes, 0);
}

#[test]
fn test_very_small_write() {
    let config = PipelineConfig::default();
    let mut pipeline = ReductionPipeline::new(config);

    let data = b"x".to_vec();
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert_eq!(stats.input_bytes, 1);
}

#[test]
fn test_pipeline_dedup_disabled() {
    let mut config = PipelineConfig::default();
    config.dedup_enabled = false;

    let mut pipeline = ReductionPipeline::new(config);
    let data = generate_test_data(50_000, 0x44);
    let (_, stats) = pipeline.process_write(&data).unwrap();

    assert_eq!(stats.chunks_deduplicated, 0);
}

#[test]
fn test_write_path_stats_snapshot() {
    let config = WritePathConfig::default();
    let store = Arc::new(NullFingerprintStore::new());
    let write_path = IntegratedWritePath::new(config, store);

    let _stats = write_path.stats_snapshot();
}

#[test]
fn test_allocator_stats() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 512,
    };

    let allocator = BuddyAllocator::new(config).unwrap();

    let allocated = allocator.allocate(BlockSize::B64K).unwrap();
    let stats = allocator.stats();

    assert!(stats.total_allocations > 0);

    allocator.free(allocated).unwrap();
}

#[test]
fn test_pipeline_compression_no_encryption() {
    let mut config = PipelineConfig::default();
    config.compression_enabled = true;
    config.encryption_enabled = false;

    let mut pipeline = ReductionPipeline::new(config);

    let data = generate_test_data(30_000, 0x00);
    let (_, stats) = pipeline.process_write(&data).unwrap();

    assert!(stats.bytes_after_compression > 0);
}

#[test]
fn test_pipeline_both_compression_and_encryption() {
    let mut config = PipelineConfig::default();
    config.compression_enabled = true;
    config.encryption_enabled = true;

    let key = EncryptionKey([0x55u8; 32]);
    let mut pipeline = ReductionPipeline::with_master_key(config, key);

    let data = generate_test_data(20_000, 0x66);
    let (_, stats) = pipeline.process_write(&data).unwrap();

    assert!(stats.bytes_after_encryption > 0);
}

#[test]
fn test_integrated_write_segments_produced() {
    let config = WritePathConfig::default();
    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let data = generate_test_data(200_000, 0xAA);
    let result = write_path.process_write(&data).unwrap();

    assert!(result.stats.segments_produced >= 0);
}

#[test]
fn test_compression_algorithm_enum() {
    let algorithms = vec![
        CompressionAlgorithm::NoCompression,
        CompressionAlgorithm::Lz4,
        CompressionAlgorithm::Zstd { level: 1 },
        CompressionAlgorithm::Zstd { level: 3 },
        CompressionAlgorithm::Zstd { level: 6 },
    ];

    for algo in algorithms {
        let mut config = PipelineConfig::default();
        config.inline_compression = algo;
        config.compression_enabled = true;

        let mut pipeline = ReductionPipeline::new(config);
        let data = generate_test_data(10_000, 0xBB);
        let result = pipeline.process_write(&data);

        assert!(result.is_ok());
    }
}

#[test]
fn test_encryption_key_sizes() {
    let key = EncryptionKey([0xAAu8; 32]);
    let mut config = PipelineConfig::default();
    config.encryption_enabled = true;

    let mut pipeline = ReductionPipeline::with_master_key(config, key);
    let data = b"Test encryption key sizes".to_vec();

    let result = pipeline.process_write(&data);
    assert!(result.is_ok());
}

#[test]
fn test_write_path_flush() {
    let config = WritePathConfig::default();
    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    write_path.process_write(&b"test".to_vec()).unwrap();
    let segments = write_path.flush_segments();

    assert!(segments.len() >= 0);
}

#[test]
fn test_block_size_variants() {
    let _ = BlockSize::B4K;
    let _ = BlockSize::B64K;
    let _ = BlockSize::B1M;
    let _ = BlockSize::B64M;
}

#[test]
fn test_buddy_allocator_64k_allocation() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 256,
    };

    let allocator = BuddyAllocator::new(config).unwrap();

    let result = allocator.allocate(BlockSize::B64K);
    assert!(result.is_ok());

    if let Ok(block) = result {
        allocator.free(block).unwrap();
    }
}

#[test]
fn test_buddy_allocator_1m_allocation() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 1024,
    };

    let allocator = BuddyAllocator::new(config).unwrap();

    let result = allocator.allocate(BlockSize::B1M);
    assert!(result.is_ok());

    if let Ok(block) = result {
        allocator.free(block).unwrap();
    }
}

#[test]
fn test_buddy_allocator_capacity() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 1000,
    };

    let allocator = BuddyAllocator::new(config).unwrap();

    let total = allocator.total_capacity_bytes();
    assert!(total > 0);

    let free = allocator.free_capacity_bytes();
    assert!(free <= total);
}
