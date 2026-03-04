//! Hot/cold tier tracking for intelligent data placement.
//!
//! Tracks access frequency for chunks to support tiering decisions between
//! NVMe flash (hot), compressed storage (warm), and S3 object store (cold).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

/// Tier classification for a data chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TierClass {
    /// Frequently accessed — keep on NVMe flash layer
    Hot,
    /// Moderately accessed — candidate for background recompression to Zstd
    Warm,
    /// Rarely accessed — tier to S3 object store
    Cold,
}

/// Per-chunk access record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRecord {
    /// Number of accesses since last reset
    pub access_count: u64,
    /// Unix timestamp of last access
    pub last_access_ts: u64,
    /// Unix timestamp of first access (creation)
    pub first_access_ts: u64,
}

/// Configuration for tiering thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierConfig {
    /// Access count >= this → Hot
    pub hot_threshold: u64,
    /// Access count >= this and < hot_threshold → Warm
    pub warm_threshold: u64,
    /// Age (seconds) after which an unaccessed chunk becomes Cold
    pub cold_age_secs: u64,
}

impl Default for TierConfig {
    fn default() -> Self {
        Self {
            hot_threshold: 10,
            warm_threshold: 3,
            cold_age_secs: 86400,
        }
    }
}

/// Tracks access patterns for chunks to drive tiering decisions.
pub struct TierTracker {
    config: TierConfig,
    records: HashMap<u64, AccessRecord>,
}

impl TierTracker {
    /// Creates a new tier tracker with the given configuration.
    pub fn new(config: TierConfig) -> Self {
        Self {
            config,
            records: HashMap::new(),
        }
    }

    /// Record an access to a chunk at the given Unix timestamp.
    pub fn record_access(&mut self, chunk_id: u64, now_ts: u64) {
        match self.records.get_mut(&chunk_id) {
            Some(record) => {
                record.access_count += 1;
                record.last_access_ts = now_ts;
                debug!(
                    "chunk {} accessed, count now {}",
                    chunk_id, record.access_count
                );
            }
            None => {
                self.records.insert(
                    chunk_id,
                    AccessRecord {
                        access_count: 1,
                        last_access_ts: now_ts,
                        first_access_ts: now_ts,
                    },
                );
                debug!("chunk {} first access recorded", chunk_id);
            }
        }
    }

    /// Classify a chunk based on its access record. Returns Cold if not tracked.
    pub fn classify(&self, chunk_id: u64, now_ts: u64) -> TierClass {
        match self.records.get(&chunk_id) {
            Some(record) => {
                let age = now_ts.saturating_sub(record.last_access_ts);
                if age >= self.config.cold_age_secs {
                    return TierClass::Cold;
                }
                if record.access_count >= self.config.hot_threshold {
                    TierClass::Hot
                } else if record.access_count >= self.config.warm_threshold {
                    TierClass::Warm
                } else {
                    TierClass::Cold
                }
            }
            None => TierClass::Cold,
        }
    }

    /// Returns all chunk IDs classified as the given tier at the given timestamp.
    pub fn chunks_in_tier(&self, tier: TierClass, now_ts: u64) -> Vec<u64> {
        self.records
            .keys()
            .filter(|&&chunk_id| self.classify(chunk_id, now_ts) == tier)
            .copied()
            .collect()
    }

    /// Returns the access record for a chunk.
    pub fn get_record(&self, chunk_id: u64) -> Option<&AccessRecord> {
        self.records.get(&chunk_id)
    }

    /// Resets access counts for all chunks (for periodic decay, not timestamps).
    pub fn reset_counts(&mut self) {
        for record in self.records.values_mut() {
            record.access_count = 0;
        }
        debug!("reset access counts for {} chunks", self.records.len());
    }

    /// Evict records for chunks not accessed since before cutoff_ts.
    pub fn evict_stale(&mut self, cutoff_ts: u64) -> usize {
        let stale: Vec<u64> = self
            .records
            .iter()
            .filter(|(_, record)| record.last_access_ts < cutoff_ts)
            .map(|(&id, _)| id)
            .collect();

        let count = stale.len();
        for id in &stale {
            self.records.remove(id);
        }
        if count > 0 {
            debug!("evicted {} stale chunk records", count);
        }
        count
    }

    /// Returns total number of tracked chunks.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns true if no chunks are tracked.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker_is_empty() {
        let tracker = TierTracker::new(TierConfig::default());
        assert!(tracker.is_empty());
        assert_eq!(tracker.len(), 0);
    }

    #[test]
    fn test_record_access_creates_record() {
        let mut tracker = TierTracker::new(TierConfig::default());
        tracker.record_access(1, 1000);

        assert!(!tracker.is_empty());
        assert_eq!(tracker.len(), 1);

        let record = tracker.get_record(1).unwrap();
        assert_eq!(record.access_count, 1);
        assert_eq!(record.last_access_ts, 1000);
        assert_eq!(record.first_access_ts, 1000);
    }

    #[test]
    fn test_classify_untracked_is_cold() {
        let tracker = TierTracker::new(TierConfig::default());
        assert_eq!(tracker.classify(999, 1000), TierClass::Cold);
    }

    #[test]
    fn test_classify_below_warm_is_cold() {
        let mut tracker = TierTracker::new(TierConfig::default());
        tracker.record_access(1, 1000);

        assert_eq!(tracker.classify(1, 1000), TierClass::Cold);
    }

    #[test]
    fn test_classify_warm_threshold() {
        let config = TierConfig {
            hot_threshold: 10,
            warm_threshold: 3,
            cold_age_secs: 86400,
        };
        let mut tracker = TierTracker::new(config);

        for _ in 0..3 {
            tracker.record_access(1, 1000);
        }

        assert_eq!(tracker.classify(1, 1000), TierClass::Warm);
    }

    #[test]
    fn test_classify_hot_threshold() {
        let config = TierConfig {
            hot_threshold: 10,
            warm_threshold: 3,
            cold_age_secs: 86400,
        };
        let mut tracker = TierTracker::new(config);

        for _ in 0..10 {
            tracker.record_access(1, 1000);
        }

        assert_eq!(tracker.classify(1, 1000), TierClass::Hot);
    }

    #[test]
    fn test_classify_old_chunk_is_cold() {
        let config = TierConfig {
            hot_threshold: 10,
            warm_threshold: 3,
            cold_age_secs: 1000,
        };
        let mut tracker = TierTracker::new(config);

        for _ in 0..5 {
            tracker.record_access(1, 1000);
        }

        assert_eq!(tracker.classify(1, 1000), TierClass::Warm);

        assert_eq!(tracker.classify(1, 2500), TierClass::Cold);
    }

    #[test]
    fn test_record_access_increments_count() {
        let mut tracker = TierTracker::new(TierConfig::default());

        tracker.record_access(1, 1000);
        tracker.record_access(1, 2000);
        tracker.record_access(1, 3000);

        let record = tracker.get_record(1).unwrap();
        assert_eq!(record.access_count, 3);
        assert_eq!(record.last_access_ts, 3000);
    }

    #[test]
    fn test_chunks_in_tier_hot() {
        let config = TierConfig {
            hot_threshold: 5,
            warm_threshold: 2,
            cold_age_secs: 86400,
        };
        let mut tracker = TierTracker::new(config);

        for _ in 0..5 {
            tracker.record_access(1, 1000);
        }
        for _ in 0..5 {
            tracker.record_access(2, 1000);
        }
        tracker.record_access(3, 1000);

        let hot = tracker.chunks_in_tier(TierClass::Hot, 1000);
        assert_eq!(hot.len(), 2);
        assert!(hot.contains(&1));
        assert!(hot.contains(&2));
    }

    #[test]
    fn test_chunks_in_tier_cold() {
        let config = TierConfig {
            hot_threshold: 5,
            warm_threshold: 2,
            cold_age_secs: 86400,
        };
        let mut tracker = TierTracker::new(config);

        for _ in 0..5 {
            tracker.record_access(1, 1000);
        }
        tracker.record_access(2, 1000);

        let cold = tracker.chunks_in_tier(TierClass::Cold, 1000);
        assert_eq!(cold.len(), 1);
        assert!(cold.contains(&2));
    }

    #[test]
    fn test_reset_counts() {
        let mut tracker = TierTracker::new(TierConfig::default());

        for _ in 0..10 {
            tracker.record_access(1, 1000);
        }
        for _ in 0..5 {
            tracker.record_access(2, 1000);
        }

        tracker.reset_counts();

        assert_eq!(tracker.get_record(1).unwrap().access_count, 0);
        assert_eq!(tracker.get_record(2).unwrap().access_count, 0);
    }

    #[test]
    fn test_reset_counts_classifies_cold() {
        let config = TierConfig {
            hot_threshold: 5,
            warm_threshold: 2,
            cold_age_secs: 86400,
        };
        let mut tracker = TierTracker::new(config);

        for _ in 0..10 {
            tracker.record_access(1, 1000);
        }

        assert_eq!(tracker.classify(1, 1000), TierClass::Hot);

        tracker.reset_counts();

        assert_eq!(tracker.classify(1, 1000), TierClass::Cold);
    }

    #[test]
    fn test_evict_stale_removes_old() {
        let mut tracker = TierTracker::new(TierConfig::default());

        tracker.record_access(1, 1000);
        tracker.record_access(2, 2000);
        tracker.record_access(3, 3000);

        let evicted = tracker.evict_stale(2500);

        assert_eq!(evicted, 2);
        assert_eq!(tracker.len(), 1);
        assert!(tracker.get_record(3).is_some());
        assert!(tracker.get_record(1).is_none());
        assert!(tracker.get_record(2).is_none());
    }

    #[test]
    fn test_evict_stale_returns_count() {
        let mut tracker = TierTracker::new(TierConfig::default());

        tracker.record_access(1, 100);
        tracker.record_access(2, 200);
        tracker.record_access(3, 300);

        let count = tracker.evict_stale(150);
        assert_eq!(count, 1);

        let count = tracker.evict_stale(500);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_first_and_last_access_timestamps() {
        let mut tracker = TierTracker::new(TierConfig::default());

        tracker.record_access(1, 1000);
        let record = tracker.get_record(1).unwrap();
        assert_eq!(record.first_access_ts, 1000);
        assert_eq!(record.last_access_ts, 1000);

        tracker.record_access(1, 2000);
        let record = tracker.get_record(1).unwrap();
        assert_eq!(record.first_access_ts, 1000);
        assert_eq!(record.last_access_ts, 2000);

        tracker.record_access(1, 3000);
        let record = tracker.get_record(1).unwrap();
        assert_eq!(record.first_access_ts, 1000);
        assert_eq!(record.last_access_ts, 3000);
    }

    #[test]
    fn test_tier_config_default() {
        let config = TierConfig::default();
        assert_eq!(config.hot_threshold, 10);
        assert_eq!(config.warm_threshold, 3);
        assert_eq!(config.cold_age_secs, 86400);
    }

    #[test]
    fn test_classify_above_hot_threshold() {
        let config = TierConfig {
            hot_threshold: 5,
            warm_threshold: 2,
            cold_age_secs: 86400,
        };
        let mut tracker = TierTracker::new(config);

        for _ in 0..20 {
            tracker.record_access(1, 1000);
        }

        assert_eq!(tracker.classify(1, 1000), TierClass::Hot);
    }

    #[test]
    fn test_chunks_in_tier_warm() {
        let config = TierConfig {
            hot_threshold: 10,
            warm_threshold: 3,
            cold_age_secs: 86400,
        };
        let mut tracker = TierTracker::new(config);

        for _ in 0..5 {
            tracker.record_access(1, 1000);
        }
        for _ in 0..3 {
            tracker.record_access(2, 1000);
        }
        for _ in 0..15 {
            tracker.record_access(3, 1000);
        }

        let warm = tracker.chunks_in_tier(TierClass::Warm, 1000);
        assert_eq!(warm.len(), 2);
        assert!(warm.contains(&1));
        assert!(warm.contains(&2));
        assert!(!warm.contains(&3));
    }

    #[test]
    fn test_evict_stale_keeps_recent() {
        let mut tracker = TierTracker::new(TierConfig::default());

        tracker.record_access(1, 1000);
        tracker.record_access(2, 2000);

        let evicted = tracker.evict_stale(500);

        assert_eq!(evicted, 0);
        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn test_multiple_chunks_tracked() {
        let mut tracker = TierTracker::new(TierConfig::default());

        for i in 1..=10 {
            tracker.record_access(i, i * 100);
        }

        assert_eq!(tracker.len(), 10);

        for i in 1..=10 {
            assert!(tracker.get_record(i).is_some());
        }
    }

    #[test]
    fn test_access_record_clone() {
        let record = AccessRecord {
            access_count: 5,
            last_access_ts: 1000,
            first_access_ts: 500,
        };
        let cloned = record.clone();
        assert_eq!(cloned.access_count, 5);
        assert_eq!(cloned.last_access_ts, 1000);
        assert_eq!(cloned.first_access_ts, 500);
    }

    #[test]
    fn test_tier_class_equality() {
        assert_eq!(TierClass::Hot, TierClass::Hot);
        assert_eq!(TierClass::Warm, TierClass::Warm);
        assert_eq!(TierClass::Cold, TierClass::Cold);
        assert_ne!(TierClass::Hot, TierClass::Warm);
        assert_ne!(TierClass::Warm, TierClass::Cold);
    }

    #[test]
    fn test_tier_config_clone() {
        let config = TierConfig {
            hot_threshold: 15,
            warm_threshold: 5,
            cold_age_secs: 3600,
        };
        let cloned = config.clone();
        assert_eq!(cloned.hot_threshold, 15);
        assert_eq!(cloned.warm_threshold, 5);
        assert_eq!(cloned.cold_age_secs, 3600);
    }

    #[test]
    fn test_saturating_sub_for_age() {
        let mut tracker = TierTracker::new(TierConfig::default());
        tracker.record_access(1, 1000);

        assert_eq!(tracker.classify(1, 500), TierClass::Cold);
    }

    #[test]
    fn test_record_access_overwrites_with_new_record() {
        let mut tracker = TierTracker::new(TierConfig::default());

        tracker.record_access(1, 1000);
        assert_eq!(tracker.len(), 1);

        tracker.record_access(2, 2000);
        assert_eq!(tracker.len(), 2);

        tracker.record_access(1, 3000);
        assert_eq!(tracker.len(), 2);
    }
}
