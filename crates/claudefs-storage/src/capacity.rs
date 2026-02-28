//! Capacity tracking with watermark-based eviction.
//!
//! Per D5: High watermark (80%): start evicting segments. Score = `last_access_age × size`
//! — old and bulky first. Low watermark (60%): stop evicting.
//! Per D6: Normal (>80%): evict cached segments. Critical (>95%): write-through mode.
//! Full (100% + S3 unreachable): ENOSPC.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::debug;

/// Configuration for watermark thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatermarkConfig {
    /// High watermark percentage (start eviction). Default: 80
    pub high_watermark_pct: u8,
    /// Low watermark percentage (stop eviction). Default: 60
    pub low_watermark_pct: u8,
    /// Critical watermark percentage (write-through mode). Default: 95
    pub critical_watermark_pct: u8,
}

impl Default for WatermarkConfig {
    fn default() -> Self {
        Self {
            high_watermark_pct: 80,
            low_watermark_pct: 60,
            critical_watermark_pct: 95,
        }
    }
}

/// Capacity level based on usage percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CapacityLevel {
    /// Below low watermark — no action needed
    #[default]
    Normal,
    /// Between low and high watermark — background eviction may be active
    Elevated,
    /// At or above high watermark — actively evicting
    High,
    /// At or above critical watermark — write-through mode
    Critical,
    /// 100% full — ENOSPC
    Full,
}

/// Tier override for segment placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TierOverride {
    /// Automatic tiering (default policy)
    #[default]
    Auto,
    /// Pinned to flash — never evict
    Flash,
    /// Forced to S3 — evict ASAP
    S3,
}

/// Tracks a segment's age and access pattern for eviction scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentTracker {
    /// Segment ID
    pub segment_id: u64,
    /// Size in bytes
    pub size_bytes: u64,
    /// When the segment was created (seconds since epoch)
    pub created_at_secs: u64,
    /// When the segment was last accessed (seconds since epoch)
    pub last_access_secs: u64,
    /// Whether this segment has been confirmed in S3
    pub s3_confirmed: bool,
    /// Placement tier override (flash/s3/auto) from xattr
    pub tier_override: TierOverride,
}

impl SegmentTracker {
    /// Calculate eviction score: age in seconds × size in bytes.
    /// Higher score = better candidate for eviction.
    /// S3-forced segments get maximum priority (infinite score).
    fn eviction_score(&self, now_secs: u64) -> u128 {
        if self.tier_override == TierOverride::S3 {
            return u128::MAX;
        }
        let age_secs = now_secs.saturating_sub(self.last_access_secs);
        (age_secs as u128) * (self.size_bytes as u128)
    }
}

/// Internal state of the capacity tracker.
struct TrackerInner {
    total_capacity_bytes: u64,
    used_bytes: u64,
    segments: BTreeMap<u64, SegmentTracker>,
}

impl TrackerInner {
    fn new(total_capacity_bytes: u64) -> Self {
        Self {
            total_capacity_bytes,
            used_bytes: 0,
            segments: BTreeMap::new(),
        }
    }

    fn update_usage(&mut self, used_bytes: u64) {
        self.used_bytes = used_bytes.min(self.total_capacity_bytes);
    }

    fn usage_pct(&self) -> u8 {
        if self.total_capacity_bytes == 0 {
            return 100;
        }
        let pct = (self.used_bytes * 100) / self.total_capacity_bytes;
        pct.min(100) as u8
    }

    fn level(&self, config: &WatermarkConfig) -> CapacityLevel {
        let pct = self.usage_pct();

        if pct >= 100 {
            CapacityLevel::Full
        } else if pct >= config.critical_watermark_pct {
            CapacityLevel::Critical
        } else if pct >= config.high_watermark_pct {
            CapacityLevel::High
        } else if pct >= config.low_watermark_pct {
            CapacityLevel::Elevated
        } else {
            CapacityLevel::Normal
        }
    }

    fn should_evict(&self, config: &WatermarkConfig) -> bool {
        self.usage_pct() >= config.high_watermark_pct
    }

    fn should_write_through(&self, config: &WatermarkConfig) -> bool {
        self.usage_pct() >= config.critical_watermark_pct
    }

    fn register_segment(&mut self, tracker: SegmentTracker) {
        let segment_id = tracker.segment_id;
        self.segments.insert(segment_id, tracker);
        debug!(
            "Registered segment {}, total tracked: {}",
            segment_id,
            self.segments.len()
        );
    }

    fn mark_s3_confirmed(&mut self, segment_id: u64) -> bool {
        if let Some(segment) = self.segments.get_mut(&segment_id) {
            segment.s3_confirmed = true;
            debug!("Marked segment {} as S3-confirmed", segment_id);
            true
        } else {
            false
        }
    }

    fn mark_accessed(&mut self, segment_id: u64) -> bool {
        if let Some(segment) = self.segments.get_mut(&segment_id) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            segment.last_access_secs = now;
            debug!("Marked segment {} as accessed at {}", segment_id, now);
            true
        } else {
            false
        }
    }

    fn eviction_candidates(&self, count: usize) -> Vec<u64> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut candidates: Vec<(u128, u64)> = self
            .segments
            .iter()
            .filter(|(_, seg)| seg.s3_confirmed && seg.tier_override != TierOverride::Flash)
            .map(|(&id, seg)| (seg.eviction_score(now), id))
            .collect();

        candidates.sort_by(|a, b| b.0.cmp(&a.0));

        candidates
            .into_iter()
            .take(count)
            .map(|(_, id)| id)
            .collect()
    }

    fn remove_segment(&mut self, segment_id: u64) -> bool {
        if let Some(segment) = self.segments.remove(&segment_id) {
            self.used_bytes = self.used_bytes.saturating_sub(segment.size_bytes);
            debug!(
                "Removed segment {}, used_bytes now {}",
                segment_id, self.used_bytes
            );
            true
        } else {
            false
        }
    }

    fn stats(&self, level: CapacityLevel) -> CapacityTrackerStats {
        let s3_confirmed_segments = self
            .segments
            .values()
            .filter(|seg| seg.s3_confirmed && seg.tier_override != TierOverride::Flash)
            .count();

        let evictable_bytes: u64 = self
            .segments
            .values()
            .filter(|seg| seg.s3_confirmed && seg.tier_override != TierOverride::Flash)
            .map(|seg| seg.size_bytes)
            .sum();

        CapacityTrackerStats {
            total_capacity_bytes: self.total_capacity_bytes,
            used_bytes: self.used_bytes,
            usage_pct: self.usage_pct(),
            level,
            tracked_segments: self.segments.len(),
            s3_confirmed_segments,
            evictable_bytes,
        }
    }
}

/// Capacity tracker with watermark-based eviction.
/// Thread-safe via internal mutex.
pub struct CapacityTracker {
    config: WatermarkConfig,
    inner: Mutex<TrackerInner>,
}

impl CapacityTracker {
    /// Create a new capacity tracker with the given config and total capacity.
    pub fn new(config: WatermarkConfig, total_capacity_bytes: u64) -> Self {
        debug!(
            "CapacityTracker created: capacity={} bytes, high={}%, low={}%, critical={}%",
            total_capacity_bytes,
            config.high_watermark_pct,
            config.low_watermark_pct,
            config.critical_watermark_pct
        );
        Self {
            config,
            inner: Mutex::new(TrackerInner::new(total_capacity_bytes)),
        }
    }

    /// Update current usage in bytes.
    pub fn update_usage(&self, used_bytes: u64) {
        let mut inner = self.inner.lock().unwrap();
        inner.update_usage(used_bytes);
    }

    /// Get current capacity level.
    pub fn level(&self) -> CapacityLevel {
        let inner = self.inner.lock().unwrap();
        inner.level(&self.config)
    }

    /// Get usage percentage (0-100).
    pub fn usage_pct(&self) -> u8 {
        let inner = self.inner.lock().unwrap();
        inner.usage_pct()
    }

    /// True if at or above high watermark.
    pub fn should_evict(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.should_evict(&self.config)
    }

    /// True if at or above critical watermark.
    pub fn should_write_through(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.should_write_through(&self.config)
    }

    /// Register a segment for eviction tracking.
    pub fn register_segment(&self, tracker: SegmentTracker) {
        let mut inner = self.inner.lock().unwrap();
        inner.register_segment(tracker);
    }

    /// Mark segment as S3-confirmed (safe to evict).
    pub fn mark_s3_confirmed(&self, segment_id: u64) -> bool {
        let mut inner = self.inner.lock().unwrap();
        inner.mark_s3_confirmed(segment_id)
    }

    /// Update last access time.
    pub fn mark_accessed(&self, segment_id: u64) -> bool {
        let mut inner = self.inner.lock().unwrap();
        inner.mark_accessed(segment_id)
    }

    /// Get top-N segment IDs to evict, scored by age × size.
    /// Only returns S3-confirmed segments, excluding Flash-pinned.
    pub fn eviction_candidates(&self, count: usize) -> Vec<u64> {
        let inner = self.inner.lock().unwrap();
        inner.eviction_candidates(count)
    }

    /// Remove a segment after eviction.
    pub fn remove_segment(&self, segment_id: u64) -> bool {
        let mut inner = self.inner.lock().unwrap();
        inner.remove_segment(segment_id)
    }

    /// Returns current capacity tracker statistics.
    pub fn stats(&self) -> CapacityTrackerStats {
        let inner = self.inner.lock().unwrap();
        let level = inner.level(&self.config);
        inner.stats(level)
    }
}

/// Statistics about the capacity tracker.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapacityTrackerStats {
    /// Total capacity in bytes
    pub total_capacity_bytes: u64,
    /// Used bytes
    pub used_bytes: u64,
    /// Usage percentage (0-100)
    pub usage_pct: u8,
    /// Current capacity level
    pub level: CapacityLevel,
    /// Number of tracked segments
    pub tracked_segments: usize,
    /// Number of S3-confirmed segments (evictable)
    pub s3_confirmed_segments: usize,
    /// Total evictable bytes
    pub evictable_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current_time_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    #[test]
    fn test_watermark_defaults() {
        let config = WatermarkConfig::default();
        assert_eq!(config.high_watermark_pct, 80);
        assert_eq!(config.low_watermark_pct, 60);
        assert_eq!(config.critical_watermark_pct, 95);
    }

    #[test]
    fn test_capacity_levels() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 1000);

        assert_eq!(tracker.level(), CapacityLevel::Normal);
        assert_eq!(tracker.usage_pct(), 0);

        tracker.update_usage(600);
        assert_eq!(tracker.level(), CapacityLevel::Elevated);
        assert_eq!(tracker.usage_pct(), 60);

        tracker.update_usage(800);
        assert_eq!(tracker.level(), CapacityLevel::High);
        assert_eq!(tracker.usage_pct(), 80);

        tracker.update_usage(950);
        assert_eq!(tracker.level(), CapacityLevel::Critical);
        assert_eq!(tracker.usage_pct(), 95);

        tracker.update_usage(1000);
        assert_eq!(tracker.level(), CapacityLevel::Full);
        assert_eq!(tracker.usage_pct(), 100);

        tracker.update_usage(1100);
        assert_eq!(tracker.level(), CapacityLevel::Full);
        assert_eq!(tracker.usage_pct(), 100);
    }

    #[test]
    fn test_eviction_scoring() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 10000);

        let now = current_time_secs();

        tracker.register_segment(SegmentTracker {
            segment_id: 1,
            size_bytes: 1000,
            created_at_secs: now - 1000,
            last_access_secs: now - 1000,
            s3_confirmed: true,
            tier_override: TierOverride::Auto,
        });

        tracker.register_segment(SegmentTracker {
            segment_id: 2,
            size_bytes: 2000,
            created_at_secs: now - 500,
            last_access_secs: now - 500,
            s3_confirmed: true,
            tier_override: TierOverride::Auto,
        });

        tracker.register_segment(SegmentTracker {
            segment_id: 3,
            size_bytes: 500,
            created_at_secs: now - 2000,
            last_access_secs: now - 2000,
            s3_confirmed: true,
            tier_override: TierOverride::Auto,
        });

        let candidates = tracker.eviction_candidates(3);
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0], 1);
        assert_eq!(candidates[1], 2);
        assert_eq!(candidates[2], 3);
    }

    #[test]
    fn test_s3_confirmation() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 10000);

        let now = current_time_secs();

        tracker.register_segment(SegmentTracker {
            segment_id: 1,
            size_bytes: 1000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: false,
            tier_override: TierOverride::Auto,
        });

        tracker.register_segment(SegmentTracker {
            segment_id: 2,
            size_bytes: 2000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: false,
            tier_override: TierOverride::Auto,
        });

        let candidates = tracker.eviction_candidates(10);
        assert!(
            candidates.is_empty(),
            "Should not evict non-S3-confirmed segments"
        );

        tracker.mark_s3_confirmed(1);

        let candidates = tracker.eviction_candidates(10);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0], 1);
    }

    #[test]
    fn test_flash_pinned() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 10000);

        let now = current_time_secs();

        tracker.register_segment(SegmentTracker {
            segment_id: 1,
            size_bytes: 1000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: true,
            tier_override: TierOverride::Flash,
        });

        tracker.register_segment(SegmentTracker {
            segment_id: 2,
            size_bytes: 2000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: true,
            tier_override: TierOverride::Auto,
        });

        let candidates = tracker.eviction_candidates(10);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0], 2, "Flash-pinned should not be evictable");
    }

    #[test]
    fn test_s3_forced() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 10000);

        let now = current_time_secs();

        tracker.register_segment(SegmentTracker {
            segment_id: 1,
            size_bytes: 1000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: true,
            tier_override: TierOverride::S3,
        });

        tracker.register_segment(SegmentTracker {
            segment_id: 2,
            size_bytes: 2000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: true,
            tier_override: TierOverride::Auto,
        });

        let candidates = tracker.eviction_candidates(2);
        assert_eq!(candidates[0], 1, "S3-forced should be prioritized");
    }

    #[test]
    fn test_capacity_tracker_stats() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 10000);

        let now = current_time_secs();

        tracker.update_usage(5000);
        tracker.register_segment(SegmentTracker {
            segment_id: 1,
            size_bytes: 1000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: true,
            tier_override: TierOverride::Auto,
        });
        tracker.register_segment(SegmentTracker {
            segment_id: 2,
            size_bytes: 2000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: false,
            tier_override: TierOverride::Auto,
        });

        let stats = tracker.stats();
        assert_eq!(stats.total_capacity_bytes, 10000);
        assert_eq!(stats.used_bytes, 5000);
        assert_eq!(stats.usage_pct, 50);
        assert_eq!(stats.level, CapacityLevel::Normal);
        assert_eq!(stats.tracked_segments, 2);
        assert_eq!(stats.s3_confirmed_segments, 1);
        assert_eq!(stats.evictable_bytes, 1000);
    }

    #[test]
    fn test_mark_accessed() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 10000);

        let now = current_time_secs();

        tracker.register_segment(SegmentTracker {
            segment_id: 1,
            size_bytes: 1000,
            created_at_secs: now - 1000,
            last_access_secs: now - 1000,
            s3_confirmed: true,
            tier_override: TierOverride::Auto,
        });

        let candidates_before = tracker.eviction_candidates(1);
        assert_eq!(candidates_before[0], 1);

        std::thread::sleep(std::time::Duration::from_millis(10));
        let new_now = current_time_secs();
        tracker.mark_accessed(1);

        let inner = tracker.inner.lock().unwrap();
        let seg = inner.segments.get(&1).unwrap();
        assert!(seg.last_access_secs >= new_now);
    }

    #[test]
    fn test_remove_segment() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 10000);

        let now = current_time_secs();

        tracker.update_usage(5000);
        tracker.register_segment(SegmentTracker {
            segment_id: 1,
            size_bytes: 1000,
            created_at_secs: now,
            last_access_secs: now,
            s3_confirmed: true,
            tier_override: TierOverride::Auto,
        });

        let stats_before = tracker.stats();
        assert_eq!(stats_before.tracked_segments, 1);
        assert_eq!(stats_before.used_bytes, 5000);

        tracker.remove_segment(1);

        let stats_after = tracker.stats();
        assert_eq!(stats_after.tracked_segments, 0);
        assert_eq!(stats_after.used_bytes, 4000);
    }

    #[test]
    fn test_write_through_mode() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 1000);

        tracker.update_usage(940);
        assert!(!tracker.should_write_through());

        tracker.update_usage(950);
        assert!(tracker.should_write_through());
        assert!(tracker.should_evict());
    }

    #[test]
    fn test_should_evict() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 1000);

        tracker.update_usage(799);
        assert!(!tracker.should_evict());

        tracker.update_usage(800);
        assert!(tracker.should_evict());
    }

    #[test]
    fn test_zero_capacity() {
        let config = WatermarkConfig::default();
        let tracker = CapacityTracker::new(config, 0);

        assert_eq!(tracker.usage_pct(), 100);
        assert_eq!(tracker.level(), CapacityLevel::Full);
    }
}
