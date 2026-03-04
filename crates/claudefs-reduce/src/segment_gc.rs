//! Segment-level garbage collection that integrates the GC engine with the segment catalog.
//!
//! Reclaims segments with low alive ratio, compacts partially alive segments.

use serde::{Deserialize, Serialize};

/// Configuration for segment garbage collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentGcConfig {
    /// Minimum alive ratio to keep a segment (default 0.5).
    pub min_alive_ratio: f64,
    /// Maximum segments to process per GC cycle (default 10).
    pub max_segments_per_cycle: usize,
}

impl Default for SegmentGcConfig {
    fn default() -> Self {
        Self {
            min_alive_ratio: 0.5,
            max_segments_per_cycle: 10,
        }
    }
}

/// Report from a segment GC cycle.
#[derive(Debug, Clone, Default)]
pub struct SegmentGcReport {
    /// Number of segments scanned.
    pub segments_scanned: usize,
    /// Number of segments reclaimed (fully deleted).
    pub segments_reclaimed: usize,
    /// Number of segments compacted (partially alive).
    pub segments_compacted: usize,
    /// Bytes freed from reclaimed segments.
    pub bytes_freed: u64,
    /// Bytes moved during compaction.
    pub bytes_compacted: u64,
}

/// Information about a segment for GC decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentInfo {
    /// Unique segment identifier.
    pub segment_id: u64,
    /// Total chunks in the segment.
    pub total_chunks: usize,
    /// Alive (referenced) chunks in the segment.
    pub alive_chunks: usize,
    /// Total bytes in the segment.
    pub total_bytes: u64,
    /// Alive (referenced) bytes in the segment.
    pub alive_bytes: u64,
}

impl SegmentInfo {
    /// Compute the alive ratio (alive_chunks / total_chunks).
    pub fn alive_ratio(&self) -> f64 {
        if self.total_chunks == 0 {
            return 1.0;
        }
        self.alive_chunks as f64 / self.total_chunks as f64
    }

    /// Compute dead bytes in the segment.
    pub fn dead_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.alive_bytes)
    }

    /// Check if segment should be reclaimed (no alive chunks, below threshold).
    pub fn should_reclaim(&self, min_alive_ratio: f64) -> bool {
        self.alive_ratio() < min_alive_ratio && self.alive_chunks == 0
    }

    /// Check if segment should be compacted (partially alive, below threshold).
    pub fn should_compact(&self, min_alive_ratio: f64) -> bool {
        self.alive_ratio() < min_alive_ratio && self.alive_chunks > 0
    }
}

/// Action to take on a segment during GC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentGcAction {
    /// Keep the segment as-is.
    Keep,
    /// Reclaim the segment (fully delete).
    Reclaim,
    /// Compact the segment (move alive chunks).
    Compact,
}

/// Segment garbage collection engine.
#[derive(Debug, Clone)]
pub struct SegmentGc {
    config: SegmentGcConfig,
}

impl SegmentGc {
    /// Create a new segment GC engine with the given configuration.
    pub fn new(config: SegmentGcConfig) -> Self {
        Self { config }
    }

    /// Evaluate a segment and determine the GC action.
    pub fn evaluate_segment(&self, info: &SegmentInfo) -> SegmentGcAction {
        if info.should_reclaim(self.config.min_alive_ratio) {
            SegmentGcAction::Reclaim
        } else if info.should_compact(self.config.min_alive_ratio) {
            SegmentGcAction::Compact
        } else {
            SegmentGcAction::Keep
        }
    }

    /// Run a GC cycle over the given segments.
    pub fn run_cycle(&mut self, segments: &[SegmentInfo]) -> SegmentGcReport {
        let segments_scanned = segments.len().min(self.config.max_segments_per_cycle);
        let mut report = SegmentGcReport {
            segments_scanned,
            ..Default::default()
        };

        for info in segments.iter().take(self.config.max_segments_per_cycle) {
            match self.evaluate_segment(info) {
                SegmentGcAction::Reclaim => {
                    report.segments_reclaimed += 1;
                    report.bytes_freed += info.total_bytes;
                }
                SegmentGcAction::Compact => {
                    report.segments_compacted += 1;
                    report.bytes_compacted += info.alive_bytes;
                }
                SegmentGcAction::Keep => {}
            }
        }

        report
    }

    /// Get the top N segments by dead bytes (best candidates for GC).
    pub fn top_candidates<'a>(
        &self,
        segments: &'a [SegmentInfo],
        n: usize,
    ) -> Vec<&'a SegmentInfo> {
        use std::cmp::Reverse;
        let mut sorted: Vec<&SegmentInfo> = segments.iter().collect();
        sorted.sort_by_key(|s| Reverse(s.dead_bytes()));
        sorted.into_iter().take(n).collect()
    }
}

impl Default for SegmentGc {
    fn default() -> Self {
        Self::new(SegmentGcConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let config = SegmentGcConfig::default();
        assert!((config.min_alive_ratio - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.max_segments_per_cycle, 10);
    }

    #[test]
    fn segment_alive_ratio_full() {
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 100,
            total_bytes: 1024,
            alive_bytes: 1024,
        };
        assert!((info.alive_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn segment_alive_ratio_empty() {
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 0,
            alive_chunks: 0,
            total_bytes: 0,
            alive_bytes: 0,
        };
        assert!((info.alive_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn segment_alive_ratio_half() {
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 50,
            total_bytes: 1024,
            alive_bytes: 512,
        };
        assert!((info.alive_ratio() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn should_reclaim_true() {
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 0,
            total_bytes: 1024,
            alive_bytes: 0,
        };
        assert!(info.should_reclaim(0.5));
    }

    #[test]
    fn should_reclaim_false_has_alive() {
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 10,
            total_bytes: 1024,
            alive_bytes: 100,
        };
        assert!(!info.should_reclaim(0.5));
    }

    #[test]
    fn should_compact_true() {
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 30,
            total_bytes: 1024,
            alive_bytes: 300,
        };
        assert!(info.should_compact(0.5));
    }

    #[test]
    fn should_compact_false_above_threshold() {
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 60,
            total_bytes: 1024,
            alive_bytes: 600,
        };
        assert!(!info.should_compact(0.5));
    }

    #[test]
    fn evaluate_keep() {
        let gc = SegmentGc::default();
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 80,
            total_bytes: 1024,
            alive_bytes: 800,
        };
        assert_eq!(gc.evaluate_segment(&info), SegmentGcAction::Keep);
    }

    #[test]
    fn evaluate_reclaim() {
        let gc = SegmentGc::default();
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 0,
            total_bytes: 1024,
            alive_bytes: 0,
        };
        assert_eq!(gc.evaluate_segment(&info), SegmentGcAction::Reclaim);
    }

    #[test]
    fn evaluate_compact() {
        let gc = SegmentGc::default();
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 30,
            total_bytes: 1024,
            alive_bytes: 300,
        };
        assert_eq!(gc.evaluate_segment(&info), SegmentGcAction::Compact);
    }

    #[test]
    fn run_cycle_counts_actions() {
        let mut gc = SegmentGc::default();
        let segments = vec![
            SegmentInfo {
                segment_id: 1,
                total_chunks: 100,
                alive_chunks: 0,
                total_bytes: 1024,
                alive_bytes: 0,
            },
            SegmentInfo {
                segment_id: 2,
                total_chunks: 100,
                alive_chunks: 30,
                total_bytes: 2048,
                alive_bytes: 600,
            },
            SegmentInfo {
                segment_id: 3,
                total_chunks: 100,
                alive_chunks: 80,
                total_bytes: 4096,
                alive_bytes: 3200,
            },
        ];

        let report = gc.run_cycle(&segments);
        assert_eq!(report.segments_scanned, 3);
        assert_eq!(report.segments_reclaimed, 1);
        assert_eq!(report.segments_compacted, 1);
        assert_eq!(report.bytes_freed, 1024);
        assert_eq!(report.bytes_compacted, 600);
    }

    #[test]
    fn top_candidates_sorted_by_dead_bytes() {
        let gc = SegmentGc::default();
        let segments = vec![
            SegmentInfo {
                segment_id: 1,
                total_chunks: 100,
                alive_chunks: 90,
                total_bytes: 1024,
                alive_bytes: 900,
            },
            SegmentInfo {
                segment_id: 2,
                total_chunks: 100,
                alive_chunks: 10,
                total_bytes: 2048,
                alive_bytes: 100,
            },
            SegmentInfo {
                segment_id: 3,
                total_chunks: 100,
                alive_chunks: 50,
                total_bytes: 4096,
                alive_bytes: 2000,
            },
        ];

        let top = gc.top_candidates(&segments, 3);
        assert_eq!(top.len(), 3);
        assert_eq!(top[0].segment_id, 3);
        assert_eq!(top[1].segment_id, 2);
        assert_eq!(top[2].segment_id, 1);
    }

    #[test]
    fn top_candidates_limited() {
        let gc = SegmentGc::default();
        let segments = vec![
            SegmentInfo {
                segment_id: 1,
                total_chunks: 100,
                alive_chunks: 50,
                total_bytes: 1024,
                alive_bytes: 512,
            },
            SegmentInfo {
                segment_id: 2,
                total_chunks: 100,
                alive_chunks: 10,
                total_bytes: 2048,
                alive_bytes: 100,
            },
        ];

        let top = gc.top_candidates(&segments, 1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].segment_id, 2);
    }

    #[test]
    fn run_cycle_empty() {
        let mut gc = SegmentGc::default();
        let report = gc.run_cycle(&[]);
        assert_eq!(report.segments_scanned, 0);
        assert_eq!(report.segments_reclaimed, 0);
        assert_eq!(report.segments_compacted, 0);
    }

    #[test]
    fn dead_bytes_calculation() {
        let info = SegmentInfo {
            segment_id: 1,
            total_chunks: 100,
            alive_chunks: 30,
            total_bytes: 1024,
            alive_bytes: 300,
        };
        assert_eq!(info.dead_bytes(), 724);
    }
}
