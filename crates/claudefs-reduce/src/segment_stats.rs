use serde::{Deserialize, Serialize};

/// Lifecycle state of a segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentLifecycle {
    Writing,
    Sealed,
    TieredToS3,
    Evicted,
    Repaired,
}

/// Stats for a single segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentStat {
    pub segment_id: u64,
    pub size_bytes: u64,
    pub chunk_count: u32,
    pub dedup_ratio: f32,
    pub compression_ratio: f32,
    pub lifecycle: SegmentLifecycle,
    pub created_at_ms: u64,
    pub sealed_at_ms: Option<u64>,
}

impl SegmentStat {
    /// Space amplification: original / stored. Higher = more savings.
    pub fn space_amplification(&self) -> f32 {
        self.dedup_ratio * self.compression_ratio
    }
}

/// Aggregated stats across all tracked segments.
#[derive(Debug, Clone, Default)]
pub struct AggregatedSegmentStats {
    pub total_segments: usize,
    pub total_bytes: u64,
    pub total_chunks: u64,
    pub sealed_count: usize,
    pub tiered_count: usize,
    pub evicted_count: usize,
    pub repaired_count: usize,
}

impl AggregatedSegmentStats {
    /// Average segments size in bytes.
    pub fn avg_segment_size(&self) -> u64 {
        if self.total_segments == 0 {
            0
        } else {
            self.total_bytes / self.total_segments as u64
        }
    }

    /// Average chunks per segment.
    pub fn avg_chunks_per_segment(&self) -> u64 {
        if self.total_segments == 0 {
            0
        } else {
            self.total_chunks / self.total_segments as u64
        }
    }
}

/// Collects and aggregates segment statistics.
pub struct SegmentStatsCollector {
    segments: std::collections::HashMap<u64, SegmentStat>,
}

impl SegmentStatsCollector {
    pub fn new() -> Self {
        Self {
            segments: std::collections::HashMap::new(),
        }
    }

    /// Register a new segment.
    pub fn register(&mut self, stat: SegmentStat) {
        self.segments.insert(stat.segment_id, stat);
    }

    /// Update the lifecycle state of a segment.
    pub fn update_lifecycle(&mut self, segment_id: u64, lifecycle: SegmentLifecycle) -> bool {
        if let Some(stat) = self.segments.get_mut(&segment_id) {
            stat.lifecycle = lifecycle;
            true
        } else {
            false
        }
    }

    /// Remove a segment from tracking.
    pub fn remove(&mut self, segment_id: u64) -> bool {
        self.segments.remove(&segment_id).is_some()
    }

    /// Get a specific segment's stats.
    pub fn get(&self, segment_id: u64) -> Option<&SegmentStat> {
        self.segments.get(&segment_id)
    }

    /// Compute aggregated stats across all tracked segments.
    pub fn aggregate(&self) -> AggregatedSegmentStats {
        let mut agg = AggregatedSegmentStats::default();
        agg.total_segments = self.segments.len();
        for stat in self.segments.values() {
            agg.total_bytes += stat.size_bytes;
            agg.total_chunks += stat.chunk_count as u64;
            match stat.lifecycle {
                SegmentLifecycle::Sealed => agg.sealed_count += 1,
                SegmentLifecycle::TieredToS3 => agg.tiered_count += 1,
                SegmentLifecycle::Evicted => agg.evicted_count += 1,
                SegmentLifecycle::Repaired => agg.repaired_count += 1,
                SegmentLifecycle::Writing => {}
            }
        }
        agg
    }

    /// Returns the total number of tracked segments.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Returns segments in a given lifecycle state.
    pub fn by_lifecycle(&self, lifecycle: &SegmentLifecycle) -> Vec<&SegmentStat> {
        self.segments
            .values()
            .filter(|s| &s.lifecycle == lifecycle)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_lifecycle_variants() {
        let _ = SegmentLifecycle::Writing;
        let _ = SegmentLifecycle::Sealed;
        let _ = SegmentLifecycle::TieredToS3;
        let _ = SegmentLifecycle::Evicted;
        let _ = SegmentLifecycle::Repaired;
    }

    #[test]
    fn segment_stat_space_amplification() {
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 1000,
            chunk_count: 10,
            dedup_ratio: 2.0,
            compression_ratio: 3.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        assert_eq!(stat.space_amplification(), 6.0);
    }

    #[test]
    fn new_collector_empty() {
        let collector = SegmentStatsCollector::new();
        assert_eq!(collector.segment_count(), 0);
    }

    #[test]
    fn register_increments_count() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 1000,
            chunk_count: 10,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat);
        assert_eq!(collector.segment_count(), 1);
    }

    #[test]
    fn get_registered_segment() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 42,
            size_bytes: 1000,
            chunk_count: 10,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat);
        assert!(collector.get(42).is_some());
    }

    #[test]
    fn get_nonexistent_segment() {
        let collector = SegmentStatsCollector::new();
        assert!(collector.get(999).is_none());
    }

    #[test]
    fn update_lifecycle_success() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 1000,
            chunk_count: 10,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat);
        let result = collector.update_lifecycle(1, SegmentLifecycle::Sealed);
        assert!(result);
        assert_eq!(
            collector.get(1).unwrap().lifecycle,
            SegmentLifecycle::Sealed
        );
    }

    #[test]
    fn update_lifecycle_nonexistent() {
        let mut collector = SegmentStatsCollector::new();
        let result = collector.update_lifecycle(999, SegmentLifecycle::Sealed);
        assert!(!result);
    }

    #[test]
    fn remove_segment_success() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 1000,
            chunk_count: 10,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat);
        let result = collector.remove(1);
        assert!(result);
        assert!(collector.get(1).is_none());
    }

    #[test]
    fn remove_nonexistent() {
        let mut collector = SegmentStatsCollector::new();
        let result = collector.remove(999);
        assert!(!result);
    }

    #[test]
    fn aggregate_empty() {
        let collector = SegmentStatsCollector::new();
        let agg = collector.aggregate();
        assert_eq!(agg.total_segments, 0);
        assert_eq!(agg.total_bytes, 0);
        assert_eq!(agg.total_chunks, 0);
    }

    #[test]
    fn aggregate_total_segments() {
        let mut collector = SegmentStatsCollector::new();
        for i in 0..5 {
            let stat = SegmentStat {
                segment_id: i,
                size_bytes: 100,
                chunk_count: 5,
                dedup_ratio: 1.0,
                compression_ratio: 1.0,
                lifecycle: SegmentLifecycle::Writing,
                created_at_ms: 1000,
                sealed_at_ms: None,
            };
            collector.register(stat);
        }
        let agg = collector.aggregate();
        assert_eq!(agg.total_segments, 5);
    }

    #[test]
    fn aggregate_total_bytes() {
        let mut collector = SegmentStatsCollector::new();
        for i in 0..3 {
            let stat = SegmentStat {
                segment_id: i,
                size_bytes: 100 * (i + 1),
                chunk_count: 5,
                dedup_ratio: 1.0,
                compression_ratio: 1.0,
                lifecycle: SegmentLifecycle::Writing,
                created_at_ms: 1000,
                sealed_at_ms: None,
            };
            collector.register(stat);
        }
        let agg = collector.aggregate();
        assert_eq!(agg.total_bytes, 600);
    }

    #[test]
    fn aggregate_total_chunks() {
        let mut collector = SegmentStatsCollector::new();
        for i in 0..3 {
            let stat = SegmentStat {
                segment_id: i,
                size_bytes: 100,
                chunk_count: 10 * (i + 1) as u32,
                dedup_ratio: 1.0,
                compression_ratio: 1.0,
                lifecycle: SegmentLifecycle::Writing,
                created_at_ms: 1000,
                sealed_at_ms: None,
            };
            collector.register(stat);
        }
        let agg = collector.aggregate();
        assert_eq!(agg.total_chunks, 60);
    }

    #[test]
    fn aggregate_sealed_count() {
        let mut collector = SegmentStatsCollector::new();
        let stat1 = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Sealed,
            created_at_ms: 1000,
            sealed_at_ms: Some(2000),
        };
        let stat2 = SegmentStat {
            segment_id: 2,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat1);
        collector.register(stat2);
        let agg = collector.aggregate();
        assert_eq!(agg.sealed_count, 1);
    }

    #[test]
    fn aggregate_tiered_count() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::TieredToS3,
            created_at_ms: 1000,
            sealed_at_ms: Some(2000),
        };
        collector.register(stat);
        let agg = collector.aggregate();
        assert_eq!(agg.tiered_count, 1);
    }

    #[test]
    fn aggregate_evicted_count() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Evicted,
            created_at_ms: 1000,
            sealed_at_ms: Some(2000),
        };
        collector.register(stat);
        let agg = collector.aggregate();
        assert_eq!(agg.evicted_count, 1);
    }

    #[test]
    fn aggregate_repaired_count() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Repaired,
            created_at_ms: 1000,
            sealed_at_ms: Some(2000),
        };
        collector.register(stat);
        let agg = collector.aggregate();
        assert_eq!(agg.repaired_count, 1);
    }

    #[test]
    fn avg_segment_size_correct() {
        let mut collector = SegmentStatsCollector::new();
        let stat1 = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        let stat2 = SegmentStat {
            segment_id: 2,
            size_bytes: 300,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat1);
        collector.register(stat2);
        let agg = collector.aggregate();
        assert_eq!(agg.avg_segment_size(), 200);
    }

    #[test]
    fn avg_segment_size_zero_when_empty() {
        let collector = SegmentStatsCollector::new();
        let agg = collector.aggregate();
        assert_eq!(agg.avg_segment_size(), 0);
    }

    #[test]
    fn avg_chunks_per_segment() {
        let mut collector = SegmentStatsCollector::new();
        let stat1 = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 10,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        let stat2 = SegmentStat {
            segment_id: 2,
            size_bytes: 100,
            chunk_count: 20,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat1);
        collector.register(stat2);
        let agg = collector.aggregate();
        assert_eq!(agg.avg_chunks_per_segment(), 15);
    }

    #[test]
    fn by_lifecycle_returns_matching() {
        let mut collector = SegmentStatsCollector::new();
        let stat1 = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Sealed,
            created_at_ms: 1000,
            sealed_at_ms: Some(2000),
        };
        let stat2 = SegmentStat {
            segment_id: 2,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat1);
        collector.register(stat2);
        let sealed = collector.by_lifecycle(&SegmentLifecycle::Sealed);
        assert_eq!(sealed.len(), 1);
        assert_eq!(sealed[0].segment_id, 1);
    }

    #[test]
    fn by_lifecycle_empty_when_none() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat);
        let sealed = collector.by_lifecycle(&SegmentLifecycle::Sealed);
        assert!(sealed.is_empty());
    }

    #[test]
    fn update_then_aggregate_reflects_change() {
        let mut collector = SegmentStatsCollector::new();
        let stat = SegmentStat {
            segment_id: 1,
            size_bytes: 100,
            chunk_count: 5,
            dedup_ratio: 1.0,
            compression_ratio: 1.0,
            lifecycle: SegmentLifecycle::Writing,
            created_at_ms: 1000,
            sealed_at_ms: None,
        };
        collector.register(stat);
        collector.update_lifecycle(1, SegmentLifecycle::Sealed);
        let agg = collector.aggregate();
        assert_eq!(agg.sealed_count, 1);
    }
}
