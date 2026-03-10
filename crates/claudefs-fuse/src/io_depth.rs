//! io_uring I/O depth management for the FUSE hot path.
//!
//! Tracks submission queue depth, enforces per-file and global depth limits,
//! and collects statistics for adaptive tuning of queue depths.

use std::collections::HashMap;

/// Error type for I/O depth management.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoDepthError {
    /// I/O submission queue depth limit exceeded.
    Throttled,
}

/// Configuration for I/O queue depth management.
#[derive(Debug, Clone)]
pub struct IoDepthConfig {
    /// Maximum global in-flight I/O operations across all files.
    pub global_max: usize,
    /// Maximum in-flight I/O operations per inode.
    pub per_inode_max: usize,
    /// Target queue depth for optimal throughput (used by adaptive logic).
    pub target_depth: usize,
    /// Number of depth histogram buckets (exponential: 1, 2, 4, 8, ...).
    pub histogram_buckets: usize,
}

impl Default for IoDepthConfig {
    fn default() -> Self {
        IoDepthConfig {
            global_max: 256,
            per_inode_max: 32,
            target_depth: 16,
            histogram_buckets: 8,
        }
    }
}

/// Depth statistics snapshot.
#[derive(Debug, Clone, Default)]
pub struct DepthStats {
    /// Current number of in-flight operations.
    pub current_depth: usize,
    /// Peak depth observed since last reset.
    pub peak_depth: usize,
    /// Total operations submitted since last reset.
    pub total_submitted: u64,
    /// Total operations completed since last reset.
    pub total_completed: u64,
    /// Number of times depth limit was hit (throttled).
    pub throttle_count: u64,
}

/// Manages I/O queue depth across multiple inodes.
pub struct IoDepthManager {
    config: IoDepthConfig,
    global_inflight: usize,
    per_inode_inflight: HashMap<u64, usize>,
    stats: DepthStats,
}

impl IoDepthManager {
    /// Creates a new IoDepthManager with the given config.
    pub fn new(config: IoDepthConfig) -> Self {
        IoDepthManager {
            config,
            global_inflight: 0,
            per_inode_inflight: HashMap::new(),
            stats: DepthStats::default(),
        }
    }

    /// Attempts to submit a new I/O for the given inode.
    ///
    /// Returns `Ok(())` if within limits, `Err(IoDepthError::Throttled)` if throttled.
    pub fn try_submit(&mut self, inode: u64) -> Result<(), IoDepthError> {
        let inode_inflight = self.per_inode_inflight.get(&inode).copied().unwrap_or(0);
        if self.global_inflight >= self.config.global_max
            || inode_inflight >= self.config.per_inode_max
        {
            self.stats.throttle_count += 1;
            return Err(IoDepthError::Throttled);
        }
        self.global_inflight += 1;
        *self.per_inode_inflight.entry(inode).or_insert(0) += 1;
        self.stats.total_submitted += 1;
        if self.global_inflight > self.stats.peak_depth {
            self.stats.peak_depth = self.global_inflight;
        }
        self.stats.current_depth = self.global_inflight;
        Ok(())
    }

    /// Records the completion of an I/O for the given inode.
    pub fn complete(&mut self, inode: u64) {
        if self.global_inflight > 0 {
            self.global_inflight -= 1;
        }
        if let Some(count) = self.per_inode_inflight.get_mut(&inode) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                self.per_inode_inflight.remove(&inode);
            }
        }
        self.stats.total_completed += 1;
        self.stats.current_depth = self.global_inflight;
    }

    /// Returns the current global in-flight count.
    pub fn current_depth(&self) -> usize {
        self.global_inflight
    }

    /// Returns the per-inode in-flight count.
    pub fn inode_depth(&self, inode: u64) -> usize {
        self.per_inode_inflight.get(&inode).copied().unwrap_or(0)
    }

    /// Returns true if the global depth limit is reached.
    pub fn is_global_full(&self) -> bool {
        self.global_inflight >= self.config.global_max
    }

    /// Returns true if the per-inode depth limit is reached.
    pub fn is_inode_full(&self, inode: u64) -> bool {
        self.per_inode_inflight.get(&inode).copied().unwrap_or(0) >= self.config.per_inode_max
    }

    /// Returns a snapshot of depth statistics.
    pub fn stats(&self) -> DepthStats {
        self.stats.clone()
    }

    /// Resets statistics counters (but not inflight counts).
    pub fn reset_stats(&mut self) {
        self.stats.total_submitted = 0;
        self.stats.total_completed = 0;
        self.stats.throttle_count = 0;
        self.stats.peak_depth = self.global_inflight;
    }

    /// Returns the number of inodes with active in-flight I/Os.
    pub fn active_inodes(&self) -> usize {
        self.per_inode_inflight.len()
    }

    /// Returns the configuration.
    pub fn config(&self) -> &IoDepthConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager_is_empty() {
        let mgr = IoDepthManager::new(IoDepthConfig::default());
        assert_eq!(mgr.current_depth(), 0);
        assert_eq!(mgr.active_inodes(), 0);
    }

    #[test]
    fn test_submit_increments_depth() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        assert!(mgr.try_submit(1).is_ok());
        assert_eq!(mgr.current_depth(), 1);
        assert_eq!(mgr.inode_depth(1), 1);
    }

    #[test]
    fn test_complete_decrements_depth() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        mgr.try_submit(1).unwrap();
        mgr.complete(1);
        assert_eq!(mgr.current_depth(), 0);
        assert_eq!(mgr.inode_depth(1), 0);
    }

    #[test]
    fn test_global_limit_enforced() {
        let config = IoDepthConfig {
            global_max: 2,
            per_inode_max: 32,
            ..Default::default()
        };
        let mut mgr = IoDepthManager::new(config);
        assert!(mgr.try_submit(1).is_ok());
        assert!(mgr.try_submit(2).is_ok());
        assert!(mgr.try_submit(3).is_err());
        assert_eq!(mgr.stats().throttle_count, 1);
    }

    #[test]
    fn test_per_inode_limit_enforced() {
        let config = IoDepthConfig {
            global_max: 256,
            per_inode_max: 2,
            ..Default::default()
        };
        let mut mgr = IoDepthManager::new(config);
        assert!(mgr.try_submit(1).is_ok());
        assert!(mgr.try_submit(1).is_ok());
        assert!(mgr.try_submit(1).is_err());
        assert!(mgr.try_submit(2).is_ok()); // different inode still allowed
    }

    #[test]
    fn test_is_global_full() {
        let config = IoDepthConfig {
            global_max: 1,
            per_inode_max: 32,
            ..Default::default()
        };
        let mut mgr = IoDepthManager::new(config);
        assert!(!mgr.is_global_full());
        mgr.try_submit(1).unwrap();
        assert!(mgr.is_global_full());
    }

    #[test]
    fn test_is_inode_full() {
        let config = IoDepthConfig {
            global_max: 256,
            per_inode_max: 1,
            ..Default::default()
        };
        let mut mgr = IoDepthManager::new(config);
        assert!(!mgr.is_inode_full(1));
        mgr.try_submit(1).unwrap();
        assert!(mgr.is_inode_full(1));
    }

    #[test]
    fn test_complete_removes_empty_inode() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        mgr.try_submit(42).unwrap();
        assert_eq!(mgr.active_inodes(), 1);
        mgr.complete(42);
        assert_eq!(mgr.active_inodes(), 0);
    }

    #[test]
    fn test_peak_depth_tracked() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        mgr.try_submit(1).unwrap();
        mgr.try_submit(2).unwrap();
        mgr.try_submit(3).unwrap();
        mgr.complete(1);
        assert_eq!(mgr.stats().peak_depth, 3);
    }

    #[test]
    fn test_stats_total_submitted_and_completed() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        mgr.try_submit(1).unwrap();
        mgr.try_submit(2).unwrap();
        mgr.complete(1);
        let stats = mgr.stats();
        assert_eq!(stats.total_submitted, 2);
        assert_eq!(stats.total_completed, 1);
    }

    #[test]
    fn test_reset_stats_clears_counters() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        mgr.try_submit(1).unwrap();
        mgr.complete(1);
        mgr.reset_stats();
        let stats = mgr.stats();
        assert_eq!(stats.total_submitted, 0);
        assert_eq!(stats.total_completed, 0);
    }

    #[test]
    fn test_complete_unknown_inode_no_panic() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        mgr.complete(999); // should not panic
        assert_eq!(mgr.current_depth(), 0);
    }

    #[test]
    fn test_multiple_inodes_independent() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        mgr.try_submit(1).unwrap();
        mgr.try_submit(2).unwrap();
        mgr.try_submit(3).unwrap();
        assert_eq!(mgr.active_inodes(), 3);
        mgr.complete(2);
        assert_eq!(mgr.active_inodes(), 2);
        assert_eq!(mgr.inode_depth(1), 1);
        assert_eq!(mgr.inode_depth(2), 0);
        assert_eq!(mgr.inode_depth(3), 1);
    }

    #[test]
    fn test_default_config_values() {
        let config = IoDepthConfig::default();
        assert_eq!(config.global_max, 256);
        assert_eq!(config.per_inode_max, 32);
        assert_eq!(config.target_depth, 16);
    }

    #[test]
    fn test_config_getter() {
        let config = IoDepthConfig {
            global_max: 128,
            per_inode_max: 16,
            target_depth: 8,
            histogram_buckets: 4,
        };
        let mgr = IoDepthManager::new(config.clone());
        assert_eq!(mgr.config().global_max, 128);
    }

    #[test]
    fn test_throttle_after_complete_allows_new_submit() {
        let config = IoDepthConfig {
            global_max: 1,
            per_inode_max: 32,
            ..Default::default()
        };
        let mut mgr = IoDepthManager::new(config);
        mgr.try_submit(1).unwrap();
        assert!(mgr.try_submit(2).is_err());
        mgr.complete(1);
        assert!(mgr.try_submit(2).is_ok()); // now there's room
    }

    #[test]
    fn test_current_depth_reflects_completions() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        for i in 0..5 {
            mgr.try_submit(i).unwrap();
        }
        assert_eq!(mgr.current_depth(), 5);
        for i in 0..3 {
            mgr.complete(i);
        }
        assert_eq!(mgr.current_depth(), 2);
    }

    #[test]
    fn test_same_inode_multiple_ops() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        for _ in 0..5 {
            mgr.try_submit(1).unwrap();
        }
        assert_eq!(mgr.inode_depth(1), 5);
        for _ in 0..3 {
            mgr.complete(1);
        }
        assert_eq!(mgr.inode_depth(1), 2);
    }

    #[test]
    fn test_stats_current_depth_updates() {
        let mut mgr = IoDepthManager::new(IoDepthConfig::default());
        mgr.try_submit(1).unwrap();
        mgr.try_submit(2).unwrap();
        assert_eq!(mgr.stats().current_depth, 2);
        mgr.complete(1);
        assert_eq!(mgr.stats().current_depth, 1);
    }
}
