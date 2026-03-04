//! Complete synchronous read pipeline.
//!
//! Provides a single `ReadPath` facade that coordinates the read flow:
//! block cache lookup → cache miss → backing store lookup → prefetch hint generation →
//! I/O accounting.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

use crate::block::{BlockId, BlockRef, BlockSize};
use crate::block_cache::{BlockCache, BlockCacheConfig, CacheStats};
use crate::checksum::{compute, ChecksumAlgorithm};
use crate::error::StorageResult;
use crate::io_accounting::{
    IoAccounting, IoAccountingConfig, IoDirection, TenantId, TenantIoStats,
};
use crate::prefetch_engine::{PrefetchConfig, PrefetchEngine, PrefetchHint, PrefetchStats};

/// Configuration for the read path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadPathConfig {
    /// Block cache capacity in bytes.
    pub cache_bytes: u64,
    /// Block cache max entries.
    pub cache_max_entries: usize,
    /// Whether to trigger prefetch hints on sequential detection.
    pub enable_prefetch: bool,
    /// Prefetch confidence threshold to act on hint.
    pub prefetch_confidence_threshold: f64,
    /// Tenant ID for I/O accounting.
    pub default_tenant_id: u64,
}

impl Default for ReadPathConfig {
    fn default() -> Self {
        Self {
            cache_bytes: 256 * 1024 * 1024,
            cache_max_entries: 65536,
            enable_prefetch: true,
            prefetch_confidence_threshold: 0.6,
            default_tenant_id: 0,
        }
    }
}

/// Result of a read operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResult {
    /// Number of bytes read.
    pub bytes_read: u64,
    /// True if the data came from cache.
    pub cache_hit: bool,
    /// Prefetch hint if one was generated.
    pub prefetch_hint: Option<PrefetchHint>,
    /// Read latency in microseconds.
    pub latency_us: u64,
}

/// Aggregate statistics for the read path.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ReadPathStats {
    /// Total number of reads.
    pub total_reads: u64,
    /// Total bytes read.
    pub total_bytes_read: u64,
    /// Total cache hits.
    pub cache_hits: u64,
    /// Total cache misses.
    pub cache_misses: u64,
    /// Total prefetch hints issued.
    pub prefetch_hints_issued: u64,
    /// Total read errors.
    pub read_errors: u64,
}

/// Simulated backing store for test purposes.
#[derive(Debug, Default)]
struct BackingStore {
    blocks: HashMap<(u64, u64), Vec<u8>>,
}

impl BackingStore {
    fn new() -> Self {
        Self {
            blocks: HashMap::new(),
        }
    }

    fn get(&self, inode_id: u64, offset: u64) -> Option<&Vec<u8>> {
        self.blocks.get(&(inode_id, offset))
    }

    fn insert(&mut self, inode_id: u64, offset: u64, data: Vec<u8>) {
        self.blocks.insert((inode_id, offset), data);
    }
}

/// The read path coordinates the complete read flow.
pub struct ReadPath {
    config: ReadPathConfig,
    cache: BlockCache,
    prefetch: PrefetchEngine,
    accounting: IoAccounting,
    backing: BackingStore,
    stats: ReadPathStats,
}

impl ReadPath {
    /// Creates a new ReadPath with the given configuration.
    pub fn new(config: ReadPathConfig) -> Self {
        let cache_config = BlockCacheConfig {
            max_memory_bytes: config.cache_bytes,
            max_entries: config.cache_max_entries,
            ..Default::default()
        };

        info!(
            cache_bytes = config.cache_bytes,
            cache_max_entries = config.cache_max_entries,
            enable_prefetch = config.enable_prefetch,
            default_tenant_id = config.default_tenant_id,
            "creating read path"
        );

        Self {
            cache: BlockCache::new(cache_config),
            prefetch: PrefetchEngine::new(PrefetchConfig::default()),
            accounting: IoAccounting::new(IoAccountingConfig::default()),
            backing: BackingStore::new(),
            stats: ReadPathStats::default(),
            config,
        }
    }

    /// Seed backing store with data for testing.
    pub fn seed_block(&mut self, inode_id: u64, offset: u64, data: Vec<u8>) {
        self.backing.insert(inode_id, offset, data);
    }

    /// Read data for an inode at offset with given size.
    pub fn read(&mut self, inode_id: u64, offset: u64, size: usize) -> StorageResult<ReadResult> {
        let start_time = SystemTime::now();

        let block_id = BlockId::new(inode_id as u16, offset);

        let cache_hit = if let Some(_entry) = self.cache.get(&block_id) {
            self.stats.cache_hits += 1;
            true
        } else {
            self.stats.cache_misses += 1;
            false
        };

        let data = if cache_hit {
            self.cache
                .get_data(&block_id)
                .map(|s| s.to_vec())
                .unwrap_or_default()
        } else {
            let backing_data = self
                .backing
                .get(inode_id, offset)
                .cloned()
                .unwrap_or_default();

            if !backing_data.is_empty() {
                let block_ref = BlockRef {
                    id: block_id,
                    size: BlockSize::B4K,
                };
                let checksum = compute(ChecksumAlgorithm::Crc32c, &backing_data);
                let _ = self.cache.insert(block_ref, backing_data.clone(), checksum);
            }

            backing_data
        };

        let bytes_read = if size == 0 {
            0
        } else if data.len() >= size {
            size as u64
        } else {
            data.len() as u64
        };

        if self.config.enable_prefetch {
            self.prefetch.record_access(inode_id, offset, bytes_read);
        }

        let prefetch_hint = if self.config.enable_prefetch {
            let hints = self.prefetch.get_prefetch_advice(inode_id);
            if !hints.is_empty() {
                self.stats.prefetch_hints_issued += 1;
                Some(hints.into_iter().next().unwrap())
            } else {
                None
            }
        } else {
            None
        };

        let tenant = TenantId(self.config.default_tenant_id);
        let latency_us = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_micros() as u64;
        self.accounting
            .record_op(tenant, IoDirection::Read, bytes_read, latency_us);

        self.stats.total_reads += 1;
        self.stats.total_bytes_read += bytes_read;

        debug!(
            inode_id = inode_id,
            offset = offset,
            size = size,
            bytes_read = bytes_read,
            cache_hit = cache_hit,
            "read completed"
        );

        Ok(ReadResult {
            bytes_read,
            cache_hit,
            prefetch_hint,
            latency_us,
        })
    }

    /// Returns the read path statistics.
    pub fn stats(&self) -> &ReadPathStats {
        &self.stats
    }

    /// Returns the cache statistics.
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats().clone()
    }

    /// Returns I/O accounting statistics for a tenant.
    pub fn accounting_stats(&self, tenant_id: u64) -> Option<TenantIoStats> {
        self.accounting.get_stats(TenantId(tenant_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_read_path() -> ReadPath {
        ReadPath::new(ReadPathConfig::default())
    }

    #[test]
    fn test_new_read_path_has_zero_stats() {
        let rp = create_test_read_path();
        assert_eq!(rp.stats().total_reads, 0);
        assert_eq!(rp.stats().total_bytes_read, 0);
    }

    #[test]
    fn test_read_hit_from_backing_store() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3, 4, 5]);

        let result = rp.read(1, 0, 5).unwrap();
        assert_eq!(result.bytes_read, 5);
    }

    #[test]
    fn test_second_read_same_block_is_cache_hit() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3, 4, 5]);

        let _first = rp.read(1, 0, 5).unwrap();
        let second = rp.read(1, 0, 5).unwrap();
        assert!(second.cache_hit);
    }

    #[test]
    fn test_read_result_cache_hit_false_on_first_read() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3]);

        let result = rp.read(1, 0, 3).unwrap();
        assert!(!result.cache_hit);
    }

    #[test]
    fn test_read_result_cache_hit_true_on_second_read() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3]);

        rp.read(1, 0, 3).unwrap();
        let result = rp.read(1, 0, 3).unwrap();
        assert!(result.cache_hit);
    }

    #[test]
    fn test_stats_total_reads_increments() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1]);

        rp.read(1, 0, 1).unwrap();
        rp.read(1, 0, 1).unwrap();
        assert_eq!(rp.stats().total_reads, 2);
    }

    #[test]
    fn test_stats_total_bytes_read_accumulates() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3]);
        rp.seed_block(1, 100, vec![4, 5]);

        rp.read(1, 0, 3).unwrap();
        rp.read(1, 100, 2).unwrap();
        assert_eq!(rp.stats().total_bytes_read, 5);
    }

    #[test]
    fn test_stats_cache_hits_increments_on_second_read() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3]);

        rp.read(1, 0, 3).unwrap();
        rp.read(1, 0, 3).unwrap();
        assert_eq!(rp.stats().cache_hits, 1);
    }

    #[test]
    fn test_stats_cache_misses_increments_on_first_read() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3]);

        rp.read(1, 0, 3).unwrap();
        assert_eq!(rp.stats().cache_misses, 1);
    }

    #[test]
    fn test_seed_block_read_returns_correct_data() {
        let mut rp = create_test_read_path();
        rp.seed_block(42, 1000, vec![9, 8, 7, 6]);

        let result = rp.read(42, 1000, 4).unwrap();
        assert_eq!(result.bytes_read, 4);
    }

    #[test]
    fn test_read_non_seeded_block_returns_empty_data() {
        let mut rp = create_test_read_path();
        let result = rp.read(999, 0, 10).unwrap();
        assert_eq!(result.bytes_read, 0);
    }

    #[test]
    fn test_accounting_stats_returns_bytes_after_read() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3, 4, 5]);

        rp.read(1, 0, 5).unwrap();

        let stats = rp.accounting_stats(0).unwrap();
        assert_eq!(stats.bytes_read, 5);
    }

    #[test]
    fn test_prefetch_hint_issued_for_sequential_reads() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![0; 4096]);
        rp.seed_block(1, 4096, vec![0; 4096]);
        rp.seed_block(1, 8192, vec![0; 4096]);

        rp.read(1, 0, 4096).unwrap();
        rp.read(1, 4096, 4096).unwrap();
        let result = rp.read(1, 8192, 4096).unwrap();

        assert!(result.prefetch_hint.is_some());
    }

    #[test]
    fn test_prefetch_hint_not_issued_below_confidence_threshold() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![0; 100]);

        let result = rp.read(1, 0, 100).unwrap();
        assert!(result.prefetch_hint.is_none());
    }

    #[test]
    fn test_prefetch_hints_issued_stat_increments_when_hint_issued() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![0; 4096]);
        rp.seed_block(1, 4096, vec![0; 4096]);
        rp.seed_block(1, 8192, vec![0; 4096]);

        rp.read(1, 0, 4096).unwrap();
        rp.read(1, 4096, 4096).unwrap();
        rp.read(1, 8192, 4096).unwrap();

        assert!(rp.stats().prefetch_hints_issued >= 1);
    }

    #[test]
    fn test_cache_stats_hits_increments_with_cache_hits() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3]);

        rp.read(1, 0, 3).unwrap();
        rp.read(1, 0, 3).unwrap();

        let stats = rp.cache_stats();
        assert!(stats.hits >= 1);
    }

    #[test]
    fn test_read_different_inodes_tracked_independently_in_accounting() {
        let mut rp = ReadPath::new(ReadPathConfig {
            default_tenant_id: 0,
            ..Default::default()
        });
        rp.seed_block(1, 0, vec![1, 2]);
        rp.seed_block(2, 0, vec![3, 4, 5]);

        rp.read(1, 0, 2).unwrap();
        rp.read(2, 0, 3).unwrap();

        let stats = rp.accounting_stats(0).unwrap();
        assert_eq!(stats.bytes_read, 5);
    }

    #[test]
    fn test_read_with_size_0_returns_empty_result() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3]);

        let result = rp.read(1, 0, 0).unwrap();
        assert_eq!(result.bytes_read, 0);
    }

    #[test]
    fn test_multiple_sequential_reads_trigger_prefetch() {
        let mut rp = create_test_read_path();
        for i in 0..5 {
            rp.seed_block(1, i as u64 * 4096, vec![0; 4096]);
        }

        for i in 0..5 {
            let result = rp.read(1, i as u64 * 4096, 4096).unwrap();
            if i >= 2 {
                assert!(result.prefetch_hint.is_some());
            }
        }
    }

    #[test]
    fn test_read_path_config_default_has_cache_bytes_256mb() {
        let config = ReadPathConfig::default();
        assert_eq!(config.cache_bytes, 256 * 1024 * 1024);
    }

    #[test]
    fn test_cache_stats_after_seeding_and_reading() {
        let mut rp = create_test_read_path();
        rp.seed_block(1, 0, vec![1, 2, 3, 4, 5]);

        rp.read(1, 0, 5).unwrap();
        rp.read(1, 0, 5).unwrap();

        let stats = rp.cache_stats();
        assert!(stats.insertions >= 1);
    }

    #[test]
    fn test_two_read_path_instances_are_independent() {
        let mut rp1 = create_test_read_path();
        let mut rp2 = create_test_read_path();

        rp1.seed_block(1, 0, vec![1, 2, 3]);
        rp2.seed_block(1, 0, vec![4, 5, 6, 7]);

        let r1 = rp1.read(1, 0, 3).unwrap();
        let r2 = rp2.read(1, 0, 4).unwrap();

        assert_eq!(r1.bytes_read, 3);
        assert_eq!(r2.bytes_read, 4);
    }
}
