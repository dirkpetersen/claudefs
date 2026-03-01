//! Cross-crate pipeline integration tests
//!
//! Tests that exercise interactions between multiple crates.

use claudefs_gateway::session::{SessionManager, SessionProtocol};
use claudefs_gateway::wire::{validate_nfs_fh, validate_nfs_filename};
use claudefs_reduce::compression::CompressionAlgorithm;
use claudefs_reduce::pipeline::PipelineConfig;
use claudefs_reduce::pipeline::ReductionPipeline;
use claudefs_repl::conduit::{ConduitConfig, EntryBatch};
use claudefs_repl::journal::{JournalEntry, OpKind};
use claudefs_storage::allocator::AllocatorConfig;
use claudefs_storage::block::BlockSize;
use claudefs_storage::checksum::ChecksumAlgorithm;

#[test]
fn test_reduce_pipeline_default_config() {
    let config = PipelineConfig::default();
    assert!(config.compression_enabled);
    assert!(config.dedup_enabled);
}

#[test]
fn test_reduce_pipeline_with_lz4() {
    let config = PipelineConfig {
        inline_compression: CompressionAlgorithm::Lz4,
        compression_enabled: true,
        ..Default::default()
    };
    let _pipeline = ReductionPipeline::new(config);
}

#[test]
fn test_reduce_pipeline_with_zstd() {
    let config = PipelineConfig {
        inline_compression: CompressionAlgorithm::Zstd { level: 3 },
        compression_enabled: true,
        ..Default::default()
    };
    let _pipeline = ReductionPipeline::new(config);
}

#[test]
fn test_reduce_pipeline_process_small_data() {
    let config = PipelineConfig::default();
    let mut pipeline = ReductionPipeline::new(config);

    let data = b"Hello, ClaudeFS!".to_vec();
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert_eq!(stats.input_bytes, data.len() as u64);
}

#[test]
fn test_reduce_pipeline_process_medium_data() {
    let config = PipelineConfig::default();
    let mut pipeline = ReductionPipeline::new(config);

    let data = vec![0xAB; 10_000];
    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert!(stats.input_bytes > 0);
}

#[test]
fn test_storage_allocator_config() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 1_000_000,
    };
    assert_eq!(config.device_idx, 0);
    assert_eq!(config.total_blocks_4k, 1_000_000);
}

#[test]
fn test_checksum_algorithm_values() {
    let _ = ChecksumAlgorithm::None;
    let _ = ChecksumAlgorithm::Crc32c;
    let _ = ChecksumAlgorithm::XxHash64;
}

#[test]
fn test_block_size_values() {
    assert_eq!(BlockSize::B4K.as_bytes(), 4096);
    assert_eq!(BlockSize::B64K.as_bytes(), 65536);
    assert_eq!(BlockSize::B1M.as_bytes(), 1_048_576);
    assert_eq!(BlockSize::B64M.as_bytes(), 67_108_864);
}

#[test]
fn test_meta_inode_id_operations() {
    use claudefs_meta::types::InodeId;
    let ino = InodeId::new(12345);
    assert_eq!(ino.as_u64(), 12345);
}

#[test]
fn test_entry_batch_creation() {
    let batch = EntryBatch {
        batch_seq: 1,
        source_site_id: 42,
        entries: vec![],
    };
    assert_eq!(batch.batch_seq, 1);
    assert_eq!(batch.source_site_id, 42);
}

#[test]
fn test_entry_batch_serialization_roundtrip() {
    let batch = EntryBatch {
        batch_seq: 42,
        source_site_id: 1,
        entries: vec![],
    };
    let encoded = bincode::serialize(&batch).unwrap();
    let decoded: EntryBatch = bincode::deserialize(&encoded).unwrap();
    assert_eq!(decoded.batch_seq, 42);
    assert_eq!(decoded.source_site_id, 1);
}

#[test]
fn test_entry_batch_with_entries_serialization() {
    let entry = JournalEntry::new(1, 0, 1, 1000, 100, OpKind::Write, vec![1, 2, 3]);
    let batch = EntryBatch {
        batch_seq: 1,
        source_site_id: 1,
        entries: vec![entry],
    };
    let encoded = bincode::serialize(&batch).unwrap();
    let decoded: EntryBatch = bincode::deserialize(&encoded).unwrap();
    assert_eq!(decoded.entries.len(), 1);
    assert_eq!(decoded.entries[0].inode, 100);
}

#[test]
fn test_gateway_wire_validation_nfs_fh() {
    let result = validate_nfs_fh(&[1, 2, 3]);
    assert!(result.is_ok());
}

#[test]
fn test_gateway_wire_validation_nfs_filename() {
    let result = validate_nfs_filename("test.txt");
    assert!(result.is_ok());
}

#[test]
fn test_gateway_session_manager_create() {
    let manager = SessionManager::new();
    let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
    assert!(id.as_u64() > 0);
}

#[test]
fn test_gateway_session_manager_operations() {
    let manager = SessionManager::new();
    let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);

    manager.record_op(id, 150, 4096);
    let session = manager.get_session(id).unwrap();
    assert_eq!(session.op_count, 1);
}

#[test]
fn test_gateway_session_expire_idle() {
    let manager = SessionManager::new();
    let _id1 = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
    let id2 = manager.create_session(SessionProtocol::Nfs3, "10.0.0.1", 500, 500, 300);

    manager.touch_session(id2, 350);

    let expired = manager.expire_idle(400, 60);
    assert_eq!(expired, 1);
    assert_eq!(manager.count(), 1);
}

#[test]
fn test_conduit_config_creation() {
    let config = ConduitConfig::new(1, 2);
    assert_eq!(config.local_site_id, 1);
    assert_eq!(config.remote_site_id, 2);
    assert_eq!(config.max_batch_size, 1000);
}

#[test]
fn test_pipeline_compression_toggle() {
    let mut config = PipelineConfig::default();
    config.compression_enabled = false;
    let mut pipeline = ReductionPipeline::new(config);

    let data = b"Test data for compression toggle".to_vec();
    let (chunks, _) = pipeline.process_write(&data).unwrap();
    assert!(!chunks.is_empty());
}

#[test]
fn test_pipeline_dedup_toggle() {
    let mut config = PipelineConfig::default();
    config.dedup_enabled = false;
    let mut pipeline = ReductionPipeline::new(config);

    let data = vec![0x00; 1000];
    let (chunks, _) = pipeline.process_write(&data).unwrap();
    assert!(!chunks.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reduce_pipeline_encryption_disabled_by_default() {
        let config = PipelineConfig::default();
        assert!(!config.encryption_enabled);
    }

    #[test]
    fn test_storage_allocator_new() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 100_000,
        };
        use claudefs_storage::allocator::BuddyAllocator;
        let _allocator = BuddyAllocator::new(config).unwrap();
    }
}
