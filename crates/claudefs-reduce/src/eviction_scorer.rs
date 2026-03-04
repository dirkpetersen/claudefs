//! Flash tier eviction scoring per architecture decision D5.
//!
//! High watermark 80% → start evicting. Score = `last_access_age × size`.
//! Old and bulky segments are evicted first. Low watermark 60% → stop evicting.
//! Segments pinned via xattr (`claudefs.tier=flash`) are never evicted.
//! Only evict segments confirmed in S3 (cache mode).

use serde::{Deserialize, Serialize};

/// Configuration for eviction scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvictionConfig {
    /// High watermark percentage to start evicting (default 80.0).
    pub high_watermark_pct: f64,
    /// Low watermark percentage to stop evicting (default 60.0).
    pub low_watermark_pct: f64,
    /// Weight applied to age component of score (default 1.0).
    pub age_weight: f64,
    /// Weight applied to size component of score (default 1.0).
    pub size_weight: f64,
}

impl Default for EvictionConfig {
    fn default() -> Self {
        Self {
            high_watermark_pct: 80.0,
            low_watermark_pct: 60.0,
            age_weight: 1.0,
            size_weight: 1.0,
        }
    }
}

/// Information about a segment used for eviction decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentEvictionInfo {
    /// Unique segment identifier.
    pub segment_id: u64,
    /// Size of the segment in bytes.
    pub size_bytes: u64,
    /// Age since last access in seconds.
    pub last_access_age_secs: u64,
    /// Whether the segment is pinned (should not be evicted).
    pub pinned: bool,
    /// Whether the segment is confirmed present in S3.
    pub confirmed_in_s3: bool,
}

/// A candidate segment for eviction with its score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvictionCandidate {
    /// Segment identifier.
    pub segment_id: u64,
    /// Eviction score (higher = better candidate).
    pub score: f64,
    /// Segment size in bytes.
    pub size_bytes: u64,
}

/// Statistics about eviction operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvictionStats {
    /// Number of segments evicted.
    pub segments_evicted: u64,
    /// Total bytes evicted.
    pub bytes_evicted: u64,
    /// Segments skipped due to being pinned.
    pub segments_pinned: u64,
    /// Segments skipped due to not being in S3.
    pub segments_not_in_s3: u64,
}

/// Eviction scorer that ranks segments for flash tier eviction.
#[derive(Debug, Clone)]
pub struct EvictionScorer {
    config: EvictionConfig,
}

impl EvictionScorer {
    /// Create a new eviction scorer with the given configuration.
    pub fn new(config: EvictionConfig) -> Self {
        Self { config }
    }

    /// Compute the eviction score for a segment.
    ///
    /// Score = age × age_weight × size × size_weight.
    /// Returns 0.0 if the segment is pinned or not confirmed in S3.
    pub fn score(&self, info: &SegmentEvictionInfo) -> f64 {
        if info.pinned || !info.confirmed_in_s3 {
            return 0.0;
        }
        (info.last_access_age_secs as f64)
            * self.config.age_weight
            * (info.size_bytes as f64)
            * self.config.size_weight
    }

    /// Check if eviction should start based on current usage.
    ///
    /// Returns true if usage_pct >= high_watermark_pct.
    pub fn should_evict(&self, usage_pct: f64) -> bool {
        usage_pct >= self.config.high_watermark_pct
    }

    /// Check if eviction should stop based on current usage.
    ///
    /// Returns true if usage_pct <= low_watermark_pct.
    pub fn should_stop_evicting(&self, usage_pct: f64) -> bool {
        usage_pct <= self.config.low_watermark_pct
    }

    /// Rank all segments by eviction score (descending order).
    ///
    /// Filters out segments with zero score (pinned or not in S3).
    pub fn rank_candidates(&self, segments: &[SegmentEvictionInfo]) -> Vec<EvictionCandidate> {
        let mut candidates: Vec<EvictionCandidate> = segments
            .iter()
            .map(|info| EvictionCandidate {
                segment_id: info.segment_id,
                score: self.score(info),
                size_bytes: info.size_bytes,
            })
            .filter(|c| c.score > 0.0)
            .collect();

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    /// Select a set of eviction candidates to meet a target byte count.
    ///
    /// Greedily selects highest-scored segments until target_bytes is met.
    /// May return fewer bytes than target if insufficient candidates.
    pub fn select_eviction_set(
        &self,
        candidates: &[EvictionCandidate],
        target_bytes: u64,
    ) -> Vec<EvictionCandidate> {
        let mut selected = Vec::new();
        let mut total_bytes = 0u64;

        for candidate in candidates {
            if total_bytes >= target_bytes {
                break;
            }
            selected.push(candidate.clone());
            total_bytes += candidate.size_bytes;
        }

        selected
    }
}

impl Default for EvictionScorer {
    fn default() -> Self {
        Self::new(EvictionConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = EvictionConfig::default();
        assert!((config.high_watermark_pct - 80.0).abs() < f64::EPSILON);
        assert!((config.low_watermark_pct - 60.0).abs() < f64::EPSILON);
        assert!((config.age_weight - 1.0).abs() < f64::EPSILON);
        assert!((config.size_weight - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_pinned_segment_is_zero() {
        let scorer = EvictionScorer::default();
        let info = SegmentEvictionInfo {
            segment_id: 1,
            size_bytes: 1000,
            last_access_age_secs: 100,
            pinned: true,
            confirmed_in_s3: true,
        };
        assert!((scorer.score(&info) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_not_in_s3_is_zero() {
        let scorer = EvictionScorer::default();
        let info = SegmentEvictionInfo {
            segment_id: 1,
            size_bytes: 1000,
            last_access_age_secs: 100,
            pinned: false,
            confirmed_in_s3: false,
        };
        assert!((scorer.score(&info) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_zero_age_is_zero() {
        let scorer = EvictionScorer::default();
        let info = SegmentEvictionInfo {
            segment_id: 1,
            size_bytes: 1000,
            last_access_age_secs: 0,
            pinned: false,
            confirmed_in_s3: true,
        };
        assert!((scorer.score(&info) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_positive() {
        let scorer = EvictionScorer::default();
        let info = SegmentEvictionInfo {
            segment_id: 1,
            size_bytes: 1000,
            last_access_age_secs: 100,
            pinned: false,
            confirmed_in_s3: true,
        };
        let expected = 100.0 * 1.0 * 1000.0 * 1.0;
        assert!((scorer.score(&info) - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_with_custom_weights() {
        let config = EvictionConfig {
            high_watermark_pct: 80.0,
            low_watermark_pct: 60.0,
            age_weight: 2.0,
            size_weight: 0.5,
        };
        let scorer = EvictionScorer::new(config);
        let info = SegmentEvictionInfo {
            segment_id: 1,
            size_bytes: 1000,
            last_access_age_secs: 100,
            pinned: false,
            confirmed_in_s3: true,
        };
        let expected = 100.0 * 2.0 * 1000.0 * 0.5;
        assert!((scorer.score(&info) - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn test_should_evict_above_high_watermark() {
        let scorer = EvictionScorer::default();
        assert!(scorer.should_evict(85.0));
    }

    #[test]
    fn test_should_evict_below_high_watermark() {
        let scorer = EvictionScorer::default();
        assert!(!scorer.should_evict(75.0));
    }

    #[test]
    fn test_should_evict_at_watermark() {
        let scorer = EvictionScorer::default();
        assert!(scorer.should_evict(80.0));
    }

    #[test]
    fn test_should_stop_evicting_below_low() {
        let scorer = EvictionScorer::default();
        assert!(scorer.should_stop_evicting(55.0));
    }

    #[test]
    fn test_should_stop_evicting_above_low() {
        let scorer = EvictionScorer::default();
        assert!(!scorer.should_stop_evicting(65.0));
    }

    #[test]
    fn test_rank_candidates_sorted_descending() {
        let scorer = EvictionScorer::default();
        let segments = vec![
            SegmentEvictionInfo {
                segment_id: 1,
                size_bytes: 1000,
                last_access_age_secs: 10,
                pinned: false,
                confirmed_in_s3: true,
            },
            SegmentEvictionInfo {
                segment_id: 2,
                size_bytes: 1000,
                last_access_age_secs: 100,
                pinned: false,
                confirmed_in_s3: true,
            },
            SegmentEvictionInfo {
                segment_id: 3,
                size_bytes: 1000,
                last_access_age_secs: 50,
                pinned: false,
                confirmed_in_s3: true,
            },
        ];

        let ranked = scorer.rank_candidates(&segments);
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].segment_id, 2);
        assert_eq!(ranked[1].segment_id, 3);
        assert_eq!(ranked[2].segment_id, 1);
    }

    #[test]
    fn test_rank_candidates_filters_pinned() {
        let scorer = EvictionScorer::default();
        let segments = vec![
            SegmentEvictionInfo {
                segment_id: 1,
                size_bytes: 1000,
                last_access_age_secs: 100,
                pinned: true,
                confirmed_in_s3: true,
            },
            SegmentEvictionInfo {
                segment_id: 2,
                size_bytes: 1000,
                last_access_age_secs: 50,
                pinned: false,
                confirmed_in_s3: true,
            },
        ];

        let ranked = scorer.rank_candidates(&segments);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].segment_id, 2);
    }

    #[test]
    fn test_rank_candidates_empty() {
        let scorer = EvictionScorer::default();
        let ranked = scorer.rank_candidates(&[]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn test_select_eviction_set_meets_target() {
        let scorer = EvictionScorer::default();
        let candidates = vec![
            EvictionCandidate {
                segment_id: 1,
                score: 100.0,
                size_bytes: 500,
            },
            EvictionCandidate {
                segment_id: 2,
                score: 50.0,
                size_bytes: 600,
            },
            EvictionCandidate {
                segment_id: 3,
                score: 25.0,
                size_bytes: 400,
            },
        ];

        let selected = scorer.select_eviction_set(&candidates, 800);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].segment_id, 1);
        assert_eq!(selected[1].segment_id, 2);
    }

    #[test]
    fn test_select_eviction_set_empty_candidates() {
        let scorer = EvictionScorer::default();
        let selected = scorer.select_eviction_set(&[], 1000);
        assert!(selected.is_empty());
    }

    #[test]
    fn test_select_eviction_set_insufficient_candidates() {
        let scorer = EvictionScorer::default();
        let candidates = vec![
            EvictionCandidate {
                segment_id: 1,
                score: 100.0,
                size_bytes: 500,
            },
            EvictionCandidate {
                segment_id: 2,
                score: 50.0,
                size_bytes: 300,
            },
        ];

        let selected = scorer.select_eviction_set(&candidates, 10_000);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn test_eviction_stats_default() {
        let stats = EvictionStats::default();
        assert_eq!(stats.segments_evicted, 0);
        assert_eq!(stats.bytes_evicted, 0);
        assert_eq!(stats.segments_pinned, 0);
        assert_eq!(stats.segments_not_in_s3, 0);
    }
}
