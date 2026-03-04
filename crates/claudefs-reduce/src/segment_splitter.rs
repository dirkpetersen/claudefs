//! Segment splitting and merging for EC stripe alignment per D1/D3.
//!
//! D1: EC unit is 2MB packed segments. D3: Write journal entries packed into 2MB segments.
//! When a segment is too large (> max_segment_size), it must be split on chunk boundaries.
//! When segments are too small, they can be merged.

use serde::{Deserialize, Serialize};

/// Configuration for segment splitting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitterConfig {
    /// Maximum segment size in bytes (default 2MB).
    pub max_segment_bytes: u64,
    /// Minimum segment size in bytes (default 64KB).
    pub min_segment_bytes: u64,
    /// Target segment size in bytes (default 2MB).
    pub target_segment_bytes: u64,
}

impl Default for SplitterConfig {
    fn default() -> Self {
        Self {
            max_segment_bytes: 2 * 1024 * 1024,
            min_segment_bytes: 64 * 1024,
            target_segment_bytes: 2 * 1024 * 1024,
        }
    }
}

/// A reference to a chunk within a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRef {
    /// BLAKE3 hash of the chunk content.
    pub hash: [u8; 32],
    /// Offset within the segment.
    pub offset: u64,
    /// Size of the chunk in bytes.
    pub size: u32,
}

/// A plan for packing chunks into a segment.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SegmentPlan {
    /// Chunks to be packed into this segment.
    pub chunks: Vec<ChunkRef>,
    /// Total bytes in this segment.
    pub total_bytes: u64,
}

impl SegmentPlan {
    /// Check if this segment is at or above max capacity.
    pub fn is_full(&self, config: &SplitterConfig) -> bool {
        self.total_bytes >= config.max_segment_bytes
    }

    /// Check if this segment is below minimum size.
    pub fn is_undersized(&self, config: &SplitterConfig) -> bool {
        self.total_bytes < config.min_segment_bytes
    }
}

/// Statistics about segment splitting/merging.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SplitStats {
    /// Number of input chunks.
    pub input_chunks: usize,
    /// Number of output segments.
    pub output_segments: usize,
    /// Average segment size in bytes.
    pub avg_segment_bytes: f64,
    /// Minimum segment size in bytes.
    pub min_segment_bytes: u64,
    /// Maximum segment size in bytes.
    pub max_segment_bytes: u64,
}

/// Segment splitter that packs chunks into segments.
#[derive(Debug, Clone)]
pub struct SegmentSplitter {
    config: SplitterConfig,
}

impl SegmentSplitter {
    /// Create a new segment splitter with the given configuration.
    pub fn new(config: SplitterConfig) -> Self {
        Self { config }
    }

    /// Split chunks into segments, not exceeding max_segment_bytes per segment.
    ///
    /// Chunks are never split across segments. Each segment gets as many chunks as fit.
    pub fn split(&self, chunks: &[ChunkRef]) -> Vec<SegmentPlan> {
        if chunks.is_empty() {
            return Vec::new();
        }

        let mut segments = Vec::new();
        let mut current_segment = SegmentPlan::default();
        let mut current_offset = 0u64;

        for chunk in chunks {
            let chunk_size = chunk.size as u64;

            if current_segment.total_bytes + chunk_size > self.config.max_segment_bytes
                && !current_segment.chunks.is_empty()
            {
                segments.push(current_segment);
                current_segment = SegmentPlan::default();
                current_offset = 0;
            }

            let mut chunk_ref = *chunk;
            chunk_ref.offset = current_offset;

            current_segment.chunks.push(chunk_ref);
            current_segment.total_bytes += chunk_size;
            current_offset += chunk_size;
        }

        if !current_segment.chunks.is_empty() {
            segments.push(current_segment);
        }

        segments
    }

    /// Merge undersized segments into larger ones.
    ///
    /// Greedy left-to-right merging. Does not merge segments already at or above min_segment_bytes.
    pub fn merge(&self, plans: &[SegmentPlan]) -> Vec<SegmentPlan> {
        if plans.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();
        let mut current = SegmentPlan::default();
        let mut current_offset = 0u64;

        for plan in plans {
            if current.total_bytes >= self.config.min_segment_bytes {
                if !current.chunks.is_empty() {
                    result.push(current);
                }
                current = SegmentPlan::default();
                current_offset = 0;
            }

            if current.total_bytes + plan.total_bytes <= self.config.max_segment_bytes {
                for mut chunk in plan.chunks.clone() {
                    chunk.offset = current_offset;
                    current.chunks.push(chunk);
                    current_offset += chunk.size as u64;
                }
                current.total_bytes += plan.total_bytes;
            } else {
                if !current.chunks.is_empty() {
                    result.push(current);
                }
                current = plan.clone();
                current_offset = current.total_bytes;
            }
        }

        if !current.chunks.is_empty() {
            result.push(current);
        }

        result
    }

    /// Split chunks optimally, trying to produce segments close to target_segment_bytes.
    ///
    /// Still respects max_segment_bytes.
    pub fn optimal_split(&self, chunks: &[ChunkRef]) -> Vec<SegmentPlan> {
        if chunks.is_empty() {
            return Vec::new();
        }

        let total_bytes: u64 = chunks.iter().map(|c| c.size as u64).sum();

        if total_bytes <= self.config.target_segment_bytes {
            return self.split(chunks);
        }

        let num_segments =
            ((total_bytes as f64) / (self.config.target_segment_bytes as f64)).ceil() as usize;
        let num_segments = num_segments.max(1);

        let mut segments: Vec<SegmentPlan> =
            (0..num_segments).map(|_| SegmentPlan::default()).collect();
        let mut segment_offsets: Vec<u64> = vec![0; num_segments];

        let mut sorted_chunks: Vec<(usize, ChunkRef)> =
            chunks.iter().copied().enumerate().collect();
        sorted_chunks.sort_by(|a, b| b.1.size.cmp(&a.1.size));

        for (_, chunk) in sorted_chunks {
            let chunk_size = chunk.size as u64;

            let best_idx = segments
                .iter()
                .enumerate()
                .filter(|(_, s)| s.total_bytes + chunk_size <= self.config.max_segment_bytes)
                .min_by_key(|(_, s)| {
                    let diff = (s.total_bytes + chunk_size) as i64
                        - self.config.target_segment_bytes as i64;
                    diff.abs()
                })
                .map(|(i, _)| i);

            if let Some(idx) = best_idx {
                let mut chunk_ref = chunk;
                chunk_ref.offset = segment_offsets[idx];
                segments[idx].chunks.push(chunk_ref);
                segments[idx].total_bytes += chunk_size;
                segment_offsets[idx] += chunk_size;
            } else {
                segments.push(SegmentPlan {
                    chunks: vec![chunk],
                    total_bytes: chunk_size,
                });
                segment_offsets.push(chunk_size);
            }
        }

        segments
            .into_iter()
            .filter(|s| !s.chunks.is_empty())
            .collect()
    }

    /// Compute statistics for a set of segment plans.
    pub fn stats(plans: &[SegmentPlan]) -> SplitStats {
        if plans.is_empty() {
            return SplitStats {
                input_chunks: 0,
                output_segments: 0,
                avg_segment_bytes: 0.0,
                min_segment_bytes: 0,
                max_segment_bytes: 0,
            };
        }

        let input_chunks: usize = plans.iter().map(|p| p.chunks.len()).sum();
        let output_segments = plans.len();
        let total_bytes: u64 = plans.iter().map(|p| p.total_bytes).sum();

        let min_segment_bytes = plans.iter().map(|p| p.total_bytes).min().unwrap_or(0);
        let max_segment_bytes = plans.iter().map(|p| p.total_bytes).max().unwrap_or(0);

        let avg_segment_bytes = if output_segments > 0 {
            total_bytes as f64 / output_segments as f64
        } else {
            0.0
        };

        SplitStats {
            input_chunks,
            output_segments,
            avg_segment_bytes,
            min_segment_bytes,
            max_segment_bytes,
        }
    }
}

impl Default for SegmentSplitter {
    fn default() -> Self {
        Self::new(SplitterConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(size: u32, id: u8) -> ChunkRef {
        let mut hash = [0u8; 32];
        hash[0] = id;
        ChunkRef {
            hash,
            offset: 0,
            size,
        }
    }

    #[test]
    fn test_default_config_values() {
        let config = SplitterConfig::default();
        assert_eq!(config.max_segment_bytes, 2 * 1024 * 1024);
        assert_eq!(config.min_segment_bytes, 64 * 1024);
        assert_eq!(config.target_segment_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn test_split_single_chunk_fits() {
        let splitter = SegmentSplitter::default();
        let chunks = vec![make_chunk(1024, 1)];
        let plans = splitter.split(&chunks);

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].total_bytes, 1024);
        assert_eq!(plans[0].chunks.len(), 1);
    }

    #[test]
    fn test_split_multiple_chunks_fit() {
        let splitter = SegmentSplitter::default();
        let chunks = vec![make_chunk(1024, 1), make_chunk(2048, 2), make_chunk(512, 3)];
        let plans = splitter.split(&chunks);

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].total_bytes, 1024 + 2048 + 512);
        assert_eq!(plans[0].chunks.len(), 3);
    }

    #[test]
    fn test_split_overflow_creates_two_segments() {
        let max_bytes = 1024u64;
        let config = SplitterConfig {
            max_segment_bytes: max_bytes,
            min_segment_bytes: 64,
            target_segment_bytes: max_bytes,
        };
        let splitter = SegmentSplitter::new(config);

        let chunks = vec![make_chunk(400, 1), make_chunk(400, 2), make_chunk(400, 3)];
        let plans = splitter.split(&chunks);

        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].chunks.len(), 2);
        assert_eq!(plans[0].total_bytes, 800);
        assert_eq!(plans[1].chunks.len(), 1);
        assert_eq!(plans[1].total_bytes, 400);
    }

    #[test]
    fn test_split_many_small_chunks() {
        let config = SplitterConfig {
            max_segment_bytes: 10 * 1024,
            min_segment_bytes: 1024,
            target_segment_bytes: 10 * 1024,
        };
        let splitter = SegmentSplitter::new(config);
        let max_bytes = splitter.config.max_segment_bytes;
        let chunk_size = 1024u32;

        let chunks: Vec<ChunkRef> = (0..30u8).map(|i| make_chunk(chunk_size, i)).collect();

        let plans = splitter.split(&chunks);

        assert!(plans.len() >= 3);
        for (i, plan) in plans.iter().enumerate() {
            if i < plans.len() - 1 {
                assert!(plan.total_bytes <= max_bytes);
            }
        }
    }

    #[test]
    fn test_split_chunk_larger_than_max() {
        let config = SplitterConfig {
            max_segment_bytes: 1024,
            min_segment_bytes: 64,
            target_segment_bytes: 1024,
        };
        let splitter = SegmentSplitter::new(config);

        let chunks = vec![make_chunk(2048, 1)];
        let plans = splitter.split(&chunks);

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].total_bytes, 2048);
    }

    #[test]
    fn test_split_empty() {
        let splitter = SegmentSplitter::default();
        let plans = splitter.split(&[]);
        assert!(plans.is_empty());
    }

    #[test]
    fn test_merge_two_small_into_one() {
        let config = SplitterConfig {
            max_segment_bytes: 128 * 1024,
            min_segment_bytes: 64 * 1024,
            target_segment_bytes: 128 * 1024,
        };
        let splitter = SegmentSplitter::new(config);

        let plans = vec![
            SegmentPlan {
                chunks: vec![make_chunk(32 * 1024, 1)],
                total_bytes: 32 * 1024,
            },
            SegmentPlan {
                chunks: vec![make_chunk(32 * 1024, 2)],
                total_bytes: 32 * 1024,
            },
        ];

        let merged = splitter.merge(&plans);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].total_bytes, 64 * 1024);
    }

    #[test]
    fn test_merge_three_small_into_two() {
        let config = SplitterConfig {
            max_segment_bytes: 2 * 1024 * 1024,
            min_segment_bytes: 64 * 1024,
            target_segment_bytes: 2 * 1024 * 1024,
        };
        let splitter = SegmentSplitter::new(config);

        let plans = vec![
            SegmentPlan {
                chunks: vec![make_chunk(768 * 1024, 1)],
                total_bytes: 768 * 1024,
            },
            SegmentPlan {
                chunks: vec![make_chunk(768 * 1024, 2)],
                total_bytes: 768 * 1024,
            },
            SegmentPlan {
                chunks: vec![make_chunk(768 * 1024, 3)],
                total_bytes: 768 * 1024,
            },
        ];

        let merged = splitter.merge(&plans);
        assert!(merged.len() >= 1 && merged.len() <= 3);
    }

    #[test]
    fn test_merge_already_large() {
        let config = SplitterConfig {
            max_segment_bytes: 128 * 1024,
            min_segment_bytes: 32 * 1024,
            target_segment_bytes: 128 * 1024,
        };
        let splitter = SegmentSplitter::new(config);

        let plans = vec![
            SegmentPlan {
                chunks: vec![make_chunk(64 * 1024, 1)],
                total_bytes: 64 * 1024,
            },
            SegmentPlan {
                chunks: vec![make_chunk(16 * 1024, 2)],
                total_bytes: 16 * 1024,
            },
        ];

        let merged = splitter.merge(&plans);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].total_bytes, 64 * 1024);
        assert_eq!(merged[1].total_bytes, 16 * 1024);
    }

    #[test]
    fn test_merge_empty() {
        let splitter = SegmentSplitter::default();
        let merged = splitter.merge(&[]);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_is_full_false() {
        let config = SplitterConfig::default();
        let plan = SegmentPlan {
            chunks: vec![make_chunk(1024, 1)],
            total_bytes: 1024,
        };
        assert!(!plan.is_full(&config));
    }

    #[test]
    fn test_is_full_true() {
        let config = SplitterConfig::default();
        let plan = SegmentPlan {
            chunks: vec![make_chunk(2 * 1024 * 1024, 1)],
            total_bytes: 2 * 1024 * 1024,
        };
        assert!(plan.is_full(&config));
    }

    #[test]
    fn test_is_undersized_true() {
        let config = SplitterConfig::default();
        let plan = SegmentPlan {
            chunks: vec![make_chunk(1024, 1)],
            total_bytes: 1024,
        };
        assert!(plan.is_undersized(&config));
    }

    #[test]
    fn test_is_undersized_false() {
        let config = SplitterConfig::default();
        let plan = SegmentPlan {
            chunks: vec![make_chunk(128 * 1024, 1)],
            total_bytes: 128 * 1024,
        };
        assert!(!plan.is_undersized(&config));
    }

    #[test]
    fn test_stats_single_segment() {
        let plans = vec![SegmentPlan {
            chunks: vec![make_chunk(1024, 1), make_chunk(2048, 2)],
            total_bytes: 3072,
        }];

        let stats = SegmentSplitter::stats(&plans);
        assert_eq!(stats.input_chunks, 2);
        assert_eq!(stats.output_segments, 1);
        assert!((stats.avg_segment_bytes - 3072.0).abs() < f64::EPSILON);
        assert_eq!(stats.min_segment_bytes, 3072);
        assert_eq!(stats.max_segment_bytes, 3072);
    }

    #[test]
    fn test_stats_multiple_segments() {
        let plans = vec![
            SegmentPlan {
                chunks: vec![make_chunk(1024, 1)],
                total_bytes: 1024,
            },
            SegmentPlan {
                chunks: vec![make_chunk(2048, 2), make_chunk(4096, 3)],
                total_bytes: 6144,
            },
        ];

        let stats = SegmentSplitter::stats(&plans);
        assert_eq!(stats.input_chunks, 3);
        assert_eq!(stats.output_segments, 2);
        assert!((stats.avg_segment_bytes - 3584.0).abs() < f64::EPSILON);
        assert_eq!(stats.min_segment_bytes, 1024);
        assert_eq!(stats.max_segment_bytes, 6144);
    }

    #[test]
    fn test_stats_empty() {
        let stats = SegmentSplitter::stats(&[]);
        assert_eq!(stats.input_chunks, 0);
        assert_eq!(stats.output_segments, 0);
        assert!((stats.avg_segment_bytes - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_optimal_split_basic() {
        let splitter = SegmentSplitter::default();
        let chunks: Vec<ChunkRef> = (0..10).map(|i| make_chunk(200 * 1024, i)).collect();

        let plans = splitter.optimal_split(&chunks);
        assert!(!plans.is_empty());

        let stats = SegmentSplitter::stats(&plans);
        assert_eq!(stats.input_chunks, 10);
    }

    #[test]
    fn test_split_sets_offsets() {
        let splitter = SegmentSplitter::default();
        let chunks = vec![make_chunk(100, 1), make_chunk(200, 2), make_chunk(300, 3)];

        let plans = splitter.split(&chunks);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].chunks[0].offset, 0);
        assert_eq!(plans[0].chunks[1].offset, 100);
        assert_eq!(plans[0].chunks[2].offset, 300);
    }

    #[test]
    fn test_merge_preserves_chunks() {
        let splitter = SegmentSplitter::default();
        let plans = vec![
            SegmentPlan {
                chunks: vec![make_chunk(1024, 1)],
                total_bytes: 1024,
            },
            SegmentPlan {
                chunks: vec![make_chunk(2048, 2)],
                total_bytes: 2048,
            },
        ];

        let merged = splitter.merge(&plans);
        let total_chunks: usize = merged.iter().map(|p| p.chunks.len()).sum();
        assert_eq!(total_chunks, 2);
    }

    #[test]
    fn test_chunk_ref_equality() {
        let c1 = make_chunk(1024, 1);
        let c2 = make_chunk(1024, 1);
        let c3 = make_chunk(1024, 2);

        assert_eq!(c1, c2);
        assert_ne!(c1, c3);
    }

    #[test]
    fn test_segment_plan_default() {
        let plan = SegmentPlan::default();
        assert!(plan.chunks.is_empty());
        assert_eq!(plan.total_bytes, 0);
    }
}
