//! Per-shard statistics for monitoring and rebalancing decisions.
//!
//! Tracks inode count, operation rate, and latency per shard to identify
//! hot shards that may need splitting or rebalancing (D4, docs/metadata.md).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::*;

/// Statistics for a single shard.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ShardStats {
    /// Shard identifier.
    pub shard_id: ShardId,
    /// Number of inodes in this shard.
    pub inode_count: u64,
    /// Total read operations served.
    pub read_ops: u64,
    /// Total write operations served.
    pub write_ops: u64,
    /// Cumulative read latency in microseconds (for averaging).
    pub total_read_latency_us: u64,
    /// Cumulative write latency in microseconds.
    pub total_write_latency_us: u64,
    /// Peak read latency seen (microseconds).
    pub peak_read_latency_us: u64,
    /// Peak write latency seen (microseconds).
    pub peak_write_latency_us: u64,
    /// Number of lease grants for inodes in this shard.
    pub lease_grants: u64,
    /// Number of lock contentions (concurrent writes to same shard).
    pub lock_contentions: u64,
}

impl ShardStats {
    /// Creates new stats for a shard.
    pub fn new(shard_id: ShardId) -> Self {
        Self {
            shard_id,
            ..Default::default()
        }
    }

    /// Records a read operation with latency.
    pub fn record_read(&mut self, latency_us: u64) {
        self.read_ops += 1;
        self.total_read_latency_us += latency_us;
        if latency_us > self.peak_read_latency_us {
            self.peak_read_latency_us = latency_us;
        }
    }

    /// Records a write operation with latency.
    pub fn record_write(&mut self, latency_us: u64) {
        self.write_ops += 1;
        self.total_write_latency_us += latency_us;
        if latency_us > self.peak_write_latency_us {
            self.peak_write_latency_us = latency_us;
        }
    }

    /// Records a lock contention event.
    pub fn record_contention(&mut self) {
        self.lock_contentions += 1;
    }

    /// Returns average read latency in microseconds.
    pub fn avg_read_latency_us(&self) -> u64 {
        if self.read_ops == 0 { 0 } else { self.total_read_latency_us / self.read_ops }
    }

    /// Returns average write latency in microseconds.
    pub fn avg_write_latency_us(&self) -> u64 {
        if self.write_ops == 0 { 0 } else { self.total_write_latency_us / self.write_ops }
    }

    /// Total operations (read + write).
    pub fn total_ops(&self) -> u64 {
        self.read_ops + self.write_ops
    }

    /// Returns the write ratio (0.0-1.0).
    pub fn write_ratio(&self) -> f64 {
        let total = self.total_ops();
        if total == 0 { 0.0 } else { self.write_ops as f64 / total as f64 }
    }
}

/// Aggregated statistics across all shards.
#[derive(Clone, Debug, Default)]
pub struct ClusterShardStats {
    /// Per-shard statistics.
    pub shards: HashMap<ShardId, ShardStats>,
}

impl ClusterShardStats {
    /// Creates a new cluster stats tracker.
    pub fn new() -> Self {
        Self {
            shards: HashMap::new(),
        }
    }

    /// Gets or creates stats for a shard.
    pub fn shard(&mut self, shard_id: ShardId) -> &mut ShardStats {
        self.shards.entry(shard_id).or_insert_with(|| ShardStats::new(shard_id))
    }

    /// Returns stats for a specific shard.
    pub fn get_shard(&self, shard_id: &ShardId) -> Option<&ShardStats> {
        self.shards.get(shard_id)
    }

    /// Returns the total number of tracked shards.
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    /// Returns total operations across all shards.
    pub fn total_ops(&self) -> u64 {
        self.shards.values().map(|s| s.total_ops()).sum()
    }

    /// Returns total inodes across all shards.
    pub fn total_inodes(&self) -> u64 {
        self.shards.values().map(|s| s.inode_count).sum()
    }

    /// Returns the hottest shard by total operations.
    pub fn hottest_shard(&self) -> Option<&ShardStats> {
        self.shards.values().max_by_key(|s| s.total_ops())
    }

    /// Returns the coldest shard by total operations.
    pub fn coldest_shard(&self) -> Option<&ShardStats> {
        self.shards.values().min_by_key(|s| s.total_ops())
    }

    /// Returns the imbalance ratio (hottest/coldest ops). 
    /// Value of 1.0 = perfectly balanced. Higher = more skewed.
    pub fn imbalance_ratio(&self) -> f64 {
        let hot = self.hottest_shard().map(|s| s.total_ops()).unwrap_or(0);
        let cold = self.coldest_shard().map(|s| s.total_ops()).unwrap_or(0);
        if cold == 0 { 
            if hot == 0 { 1.0 } else { f64::MAX }
        } else { 
            hot as f64 / cold as f64 
        }
    }

    /// Returns shards that exceed an operation threshold.
    pub fn hot_shards(&self, ops_threshold: u64) -> Vec<&ShardStats> {
        self.shards.values().filter(|s| s.total_ops() > ops_threshold).collect()
    }

    /// Resets all stats (e.g., for periodic collection).
    pub fn reset(&mut self) {
        for stats in self.shards.values_mut() {
            stats.read_ops = 0;
            stats.write_ops = 0;
            stats.total_read_latency_us = 0;
            stats.total_write_latency_us = 0;
            stats.peak_read_latency_us = 0;
            stats.peak_write_latency_us = 0;
            stats.lease_grants = 0;
            stats.lock_contentions = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_read() {
        let mut stats = ShardStats::new(ShardId::new(1));
        stats.record_read(100);
        stats.record_read(200);
        assert_eq!(stats.read_ops, 2);
        assert_eq!(stats.avg_read_latency_us(), 150);
        assert_eq!(stats.peak_read_latency_us, 200);
    }

    #[test]
    fn test_record_write() {
        let mut stats = ShardStats::new(ShardId::new(1));
        stats.record_write(500);
        assert_eq!(stats.write_ops, 1);
        assert_eq!(stats.avg_write_latency_us(), 500);
    }

    #[test]
    fn test_write_ratio() {
        let mut stats = ShardStats::new(ShardId::new(1));
        stats.record_read(100);
        stats.record_read(100);
        stats.record_write(100);
        assert!((stats.write_ratio() - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_total_ops() {
        let mut stats = ShardStats::new(ShardId::new(1));
        stats.record_read(100);
        stats.record_write(_eq!(stats.total100);
        assert_ops(), 2);
    }

    #[test]
    fn test_contention() {
        let mut stats = ShardStats::new(ShardId::new(1));
        stats.record_contention();
        stats.record_contention();
        assert_eq!(stats.lock_contentions, 2);
    }

    #[test]
    fn test_cluster_shard_ops() {
        let mut cluster = ClusterShardStats::new();
        cluster.shard(ShardId::new(1)).record_read(100);
        cluster.shard(ShardId::new(2)).record_write(200);
        assert_eq!(cluster.total_ops(), 2);
    }

    #[test]
    fn test_hottest_shard() {
        let mut cluster = ClusterShardStats::new();
        cluster.shard(ShardId::new(1)).record_read(100);
        cluster.shard(ShardId::new(2)).record_read(100);
        cluster.shard(ShardId::new(2)).record_read(100);

        let hot = cluster.hottest_shard().unwrap();
        assert_eq!(hot.shard_id, ShardId::new(2));
    }

    #[test]
    fn test_coldest_shard() {
        let mut cluster = ClusterShardStats::new();
        cluster.shard(ShardId::new(1)).record_read(100);
        cluster.shard(ShardId::new(2)).record_read(100);
        cluster.shard(ShardId::new(2)).record_read(100);

        let cold = cluster.coldest_shard().unwrap();
        assert_eq!(cold.shard_id, ShardId::new(1));
    }

    #[test]
    fn test_imbalance_ratio() {
        let mut cluster = ClusterShardStats::new();
        cluster.shard(ShardId::new(1)).record_read(100);
        cluster.shard(ShardId::new(2)).record_read(100);
        cluster.shard(ShardId::new(2)).record_read(100);

        let ratio = cluster.imbalance_ratio();
        assert!((ratio - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_hot_shards() {
        let mut cluster = ClusterShardStats::new();
        cluster.shard(ShardId::new(1)).record_read(100);
        for _ in 0..5 {
            cluster.shard(ShardId::new(2)).record_read(100);
        }

        let hot = cluster.hot_shards(3);
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0].shard_id, ShardId::new(2));
    }

    #[test]
    fn test_reset() {
        let mut cluster = ClusterShardStats::new();
        cluster.shard(ShardId::new(1)).record_read(100);
        cluster.shard(ShardId::new(1)).record_write(200);
        cluster.reset();

        let stats = cluster.get_shard(&ShardId::new(1)).unwrap();
        assert_eq!(stats.read_ops, 0);
        assert_eq!(stats.write_ops, 0);
    }

    #[test]
    fn test_total_inodes() {
        let mut cluster = ClusterShardStats::new();
        cluster.shard(ShardId::new(1)).inode_count = 100;
        cluster.shard(ShardId::new(2)).inode_count = 200;
        assert_eq!(cluster.total_inodes(), 300);
    }

    #[test]
    fn test_empty_cluster() {
        let cluster = ClusterShardStats::new();
        assert_eq!(cluster.shard_count(), 0);
        assert_eq!(cluster.total_ops(), 0);
        assert!(cluster.hottest_shard().is_none());
        assert!((cluster.imbalance_ratio() - 1.0).abs() < 0.01);
    }
}
