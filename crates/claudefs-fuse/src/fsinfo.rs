//! Filesystem-wide info cache for statfs(2) / statvfs(2) results.
//!
//! This module caches filesystem statistics returned by the statfs system call.
//! Used by df, du, quota tools, and applications that need to check available space.
//! Returns cached (potentially stale) values quickly, with background refresh.

use std::time::{Duration, Instant};

/// Aggregate filesystem statistics (returned by statfs).
#[derive(Debug, Clone, Default)]
pub struct FsStats {
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub available_blocks: u64,
    pub total_inodes: u64,
    pub free_inodes: u64,
    pub block_size: u32,
    pub max_name_len: u32,
    pub last_updated: Option<Instant>,
}

impl FsStats {
    /// Returns the total capacity in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.total_blocks.saturating_mul(self.block_size as u64)
    }

    /// Returns the free capacity in bytes (available to unprivileged users).
    pub fn available_bytes(&self) -> u64 {
        self.available_blocks.saturating_mul(self.block_size as u64)
    }

    /// Returns the used capacity in bytes.
    pub fn used_bytes(&self) -> u64 {
        let total = self.total_blocks.saturating_mul(self.block_size as u64);
        let available = self.available_blocks.saturating_mul(self.block_size as u64);
        total.saturating_sub(available)
    }

    /// Returns used fraction 0.0..=1.0
    pub fn used_fraction(&self) -> f64 {
        if self.total_blocks == 0 {
            return 0.0;
        }
        let used = self.total_blocks - self.available_blocks;
        let fraction = used as f64 / self.total_blocks as f64;
        fraction.clamp(0.0, 1.0)
    }

    /// Returns true if free_blocks < low_water_mark (fraction of total).
    pub fn is_low(&self, low_water_fraction: f64) -> bool {
        if self.total_blocks == 0 {
            return false;
        }
        let used_fraction = self.used_fraction();
        used_fraction > (1.0 - low_water_fraction).clamp(0.0, 1.0)
    }

    /// Returns true if the stats are stale (older than ttl_secs).
    pub fn is_stale(&self, ttl_secs: u64) -> bool {
        self.last_updated
            .map(|t| t.elapsed() > Duration::from_secs(ttl_secs))
            .unwrap_or(true)
    }

    /// Returns the age of the stats in seconds, or 0 if never updated.
    pub fn age_secs(&self) -> u64 {
        self.last_updated
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
    }
}

/// Configuration for the filesystem info cache.
#[derive(Debug, Clone)]
pub struct FsInfoConfig {
    pub ttl_secs: u64,
    pub refresh_interval_secs: u64,
}

impl Default for FsInfoConfig {
    fn default() -> Self {
        Self {
            ttl_secs: 10,
            refresh_interval_secs: 30,
        }
    }
}

/// Cache statistics.
#[derive(Debug, Default, Clone)]
pub struct FsInfoStats {
    pub hits: u64,
    pub refreshes: u64,
    pub age_secs: u64,
}

/// The filesystem info cache.
pub struct FsInfoCache {
    stats: Option<FsStats>,
    config: FsInfoConfig,
    hits: u64,
    refreshes: u64,
}

impl FsInfoCache {
    /// Creates a new fsinfo cache with the given configuration.
    pub fn new(config: FsInfoConfig) -> Self {
        Self {
            stats: None,
            config,
            hits: 0,
            refreshes: 0,
        }
    }

    /// Returns the current cached FsStats (even if stale).
    /// Returns None if no stats have been loaded yet.
    pub fn get(&self) -> Option<FsStats> {
        if self.stats.is_some() {
            Some(self.stats.clone().unwrap())
        } else {
            None
        }
    }

    /// Returns current stats if fresh, None if stale or absent.
    pub fn get_if_fresh(&mut self) -> Option<FsStats> {
        if let Some(ref stats) = self.stats {
            if !stats.is_stale(self.config.ttl_secs) {
                self.hits += 1;
                return Some(stats.clone());
            }
        }
        None
    }

    /// Updates the cached stats (called after an RPC to the metadata service).
    pub fn update(&mut self, mut stats: FsStats) {
        stats.last_updated = Some(Instant::now());
        self.stats = Some(stats);
    }

    /// Returns true if the cache needs a refresh (stale or absent).
    pub fn needs_refresh(&self) -> bool {
        match &self.stats {
            None => true,
            Some(stats) => stats.is_stale(self.config.ttl_secs),
        }
    }

    /// Invalidates the cache (forces next get to miss).
    pub fn invalidate(&mut self) {
        self.stats = None;
    }

    /// Returns statistics about the cache.
    pub fn stats(&self) -> FsInfoStats {
        let age = self.stats.as_ref().map(|s| s.age_secs()).unwrap_or(0);
        FsInfoStats {
            hits: self.hits,
            refreshes: self.refreshes,
            age_secs: age,
        }
    }

    /// Records a background refresh event.
    pub fn record_refresh(&mut self) {
        self.refreshes += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stats() -> FsStats {
        FsStats {
            total_blocks: 1000,
            free_blocks: 500,
            available_blocks: 400,
            total_inodes: 10000,
            free_inodes: 8000,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        }
    }

    #[test]
    fn new_cache_returns_none_from_get() {
        let cache = FsInfoCache::new(FsInfoConfig::default());
        assert!(cache.get().is_none());
    }

    #[test]
    fn after_update_get_returns_stats() {
        let mut cache = FsInfoCache::new(FsInfoConfig::default());
        cache.update(make_stats());
        assert!(cache.get().is_some());
    }

    #[test]
    fn get_if_fresh_returns_none_on_stale() {
        let config = FsInfoConfig {
            ttl_secs: 0,
            refresh_interval_secs: 30,
        };
        let mut cache = FsInfoCache::new(config);
        cache.update(make_stats());
        std::thread::sleep(Duration::from_millis(10));
        assert!(cache.get_if_fresh().is_none());
    }

    #[test]
    fn get_if_fresh_returns_some_when_fresh() {
        let config = FsInfoConfig {
            ttl_secs: 10,
            refresh_interval_secs: 30,
        };
        let mut cache = FsInfoCache::new(config);
        cache.update(make_stats());
        assert!(cache.get_if_fresh().is_some());
    }

    #[test]
    fn needs_refresh_true_when_empty() {
        let cache = FsInfoCache::new(FsInfoConfig::default());
        assert!(cache.needs_refresh());
    }

    #[test]
    fn needs_refresh_true_when_stale() {
        let config = FsInfoConfig {
            ttl_secs: 0,
            refresh_interval_secs: 30,
        };
        let mut cache = FsInfoCache::new(config);
        cache.update(make_stats());
        std::thread::sleep(Duration::from_millis(10));
        assert!(cache.needs_refresh());
    }

    #[test]
    fn needs_refresh_false_when_fresh() {
        let config = FsInfoConfig {
            ttl_secs: 10,
            refresh_interval_secs: 30,
        };
        let mut cache = FsInfoCache::new(config);
        cache.update(make_stats());
        assert!(!cache.needs_refresh());
    }

    #[test]
    fn invalidate_causes_needs_refresh_true() {
        let mut cache = FsInfoCache::new(FsInfoConfig::default());
        cache.update(make_stats());
        cache.invalidate();
        assert!(cache.needs_refresh());
    }

    #[test]
    fn invalidate_causes_get_if_fresh_none() {
        let mut cache = FsInfoCache::new(FsInfoConfig::default());
        cache.update(make_stats());
        cache.invalidate();
        assert!(cache.get_if_fresh().is_none());
    }

    #[test]
    fn update_with_new_stats_replaces_old() {
        let mut cache = FsInfoCache::new(FsInfoConfig::default());
        cache.update(make_stats());
        let mut new_stats = make_stats();
        new_stats.total_blocks = 2000;
        cache.update(new_stats);
        assert_eq!(cache.get().unwrap().total_blocks, 2000);
    }

    #[test]
    fn total_bytes_calculation() {
        let stats = FsStats {
            total_blocks: 1000,
            free_blocks: 0,
            available_blocks: 0,
            total_inodes: 0,
            free_inodes: 0,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        };
        assert_eq!(stats.total_bytes(), 4096 * 1000);
    }

    #[test]
    fn available_bytes_calculation() {
        let stats = FsStats {
            total_blocks: 0,
            free_blocks: 0,
            available_blocks: 500,
            total_inodes: 0,
            free_inodes: 0,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        };
        assert_eq!(stats.available_bytes(), 4096 * 500);
    }

    #[test]
    fn used_bytes_calculation() {
        let stats = FsStats {
            total_blocks: 1000,
            free_blocks: 0,
            available_blocks: 400,
            total_inodes: 0,
            free_inodes: 0,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        };
        assert_eq!(stats.used_bytes(), 4096 * 600);
    }

    #[test]
    fn used_fraction_half_full() {
        let stats = FsStats {
            total_blocks: 1000,
            free_blocks: 0,
            available_blocks: 500,
            total_inodes: 0,
            free_inodes: 0,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        };
        let fraction = stats.used_fraction();
        assert!((fraction - 0.5).abs() < 0.001);
    }

    #[test]
    fn used_fraction_empty() {
        let stats = FsStats {
            total_blocks: 1000,
            free_blocks: 0,
            available_blocks: 1000,
            total_inodes: 0,
            free_inodes: 0,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        };
        assert_eq!(stats.used_fraction(), 0.0);
    }

    #[test]
    fn is_low_true_when_used_over_90() {
        let stats = FsStats {
            total_blocks: 1000,
            free_blocks: 0,
            available_blocks: 50,
            total_inodes: 0,
            free_inodes: 0,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        };
        assert!(stats.is_low(0.1));
    }

    #[test]
    fn is_low_false_when_half_full() {
        let stats = FsStats {
            total_blocks: 1000,
            free_blocks: 0,
            available_blocks: 500,
            total_inodes: 0,
            free_inodes: 0,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        };
        assert!(!stats.is_low(0.1));
    }

    #[test]
    fn stats_hits_increases_on_get_if_fresh() {
        let mut cache = FsInfoCache::new(FsInfoConfig::default());
        cache.update(make_stats());
        cache.get_if_fresh();
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn stats_refreshes_increases_on_record_refresh() {
        let mut cache = FsInfoCache::new(FsInfoConfig::default());
        cache.record_refresh();
        assert_eq!(cache.stats().refreshes, 1);
    }

    #[test]
    fn stats_age_secs_reflects_elapsed() {
        let mut cache = FsInfoCache::new(FsInfoConfig::default());
        cache.update(make_stats());
        std::thread::sleep(Duration::from_millis(1100));
        let age = cache.stats().age_secs;
        assert!(age >= 1);
    }

    #[test]
    fn multiple_updates_latest_wins() {
        let mut cache = FsInfoCache::new(FsInfoConfig::default());
        cache.update(make_stats());
        let mut s2 = make_stats();
        s2.free_blocks = 800;
        s2.available_blocks = 750;
        cache.update(s2);
        let stats = cache.get().unwrap();
        assert_eq!(stats.free_blocks, 800);
        assert_eq!(stats.available_blocks, 750);
    }

    #[test]
    fn is_stale_with_ttl_zero() {
        let stats = FsStats {
            total_blocks: 100,
            free_blocks: 50,
            available_blocks: 50,
            total_inodes: 100,
            free_inodes: 50,
            block_size: 4096,
            max_name_len: 255,
            last_updated: Some(Instant::now()),
        };
        std::thread::sleep(Duration::from_millis(10));
        assert!(stats.is_stale(0));
    }

    #[test]
    fn get_always_returns_cached() {
        let config = FsInfoConfig {
            ttl_secs: 0,
            refresh_interval_secs: 30,
        };
        let mut cache = FsInfoCache::new(config);
        cache.update(make_stats());
        std::thread::sleep(Duration::from_millis(10));
        assert!(cache.get().is_some());
    }

    #[test]
    fn is_stale_returns_true_when_never_updated() {
        let stats = FsStats::default();
        assert!(stats.is_stale(10));
    }

    #[test]
    fn age_secs_zero_when_never_updated() {
        let stats = FsStats::default();
        assert_eq!(stats.age_secs(), 0);
    }

    #[test]
    fn is_low_false_on_zero_total() {
        let stats = FsStats::default();
        assert!(!stats.is_low(0.1));
    }
}
