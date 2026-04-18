//! Inline (hot path) dedup decision engine.

#[derive(Debug, Clone)]
pub struct InlineDedupConfig {
    pub min_chunk_size: usize,
    pub max_chunk_size: usize,
    pub skip_high_entropy: bool,
}

impl Default for InlineDedupConfig {
    fn default() -> Self {
        Self {
            min_chunk_size: 4096,
            max_chunk_size: 4 * 1024 * 1024,
            skip_high_entropy: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupDecision {
    Deduplicate { existing_hash: [u8; 32] },
    WriteThrough,
    Skipped { reason: SkipReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    TooSmall,
    TooLarge,
    HighEntropy,
}

#[derive(Debug, Clone, Default)]
pub struct InlineDedupStats {
    pub chunks_evaluated: u64,
    pub chunks_deduplicated: u64,
    pub chunks_written_through: u64,
    pub chunks_skipped: u64,
    pub bytes_saved: u64,
}

impl InlineDedupStats {
    pub fn dedup_ratio(&self) -> f64 {
        if self.chunks_evaluated == 0 {
            0.0
        } else {
            self.chunks_deduplicated as f64 / self.chunks_evaluated as f64
        }
    }
}

pub struct InlineDedupIndex {
    hashes: std::collections::HashSet<[u8; 32]>,
}

impl Default for InlineDedupIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl InlineDedupIndex {
    pub fn new() -> Self {
        Self {
            hashes: std::collections::HashSet::new(),
        }
    }
    pub fn insert(&mut self, hash: [u8; 32]) {
        self.hashes.insert(hash);
    }
    pub fn contains(&self, hash: &[u8; 32]) -> bool {
        self.hashes.contains(hash)
    }
    pub fn len(&self) -> usize {
        self.hashes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }
}

pub struct InlineDedup {
    config: InlineDedupConfig,
    stats: InlineDedupStats,
}

impl InlineDedup {
    pub fn new(config: InlineDedupConfig) -> Self {
        Self {
            config,
            stats: InlineDedupStats::default(),
        }
    }

    pub fn evaluate(
        &mut self,
        data: &[u8],
        hash: [u8; 32],
        index: &InlineDedupIndex,
    ) -> DedupDecision {
        self.stats.chunks_evaluated += 1;

        if data.len() < self.config.min_chunk_size {
            self.stats.chunks_skipped += 1;
            return DedupDecision::Skipped {
                reason: SkipReason::TooSmall,
            };
        }
        if data.len() > self.config.max_chunk_size {
            self.stats.chunks_skipped += 1;
            return DedupDecision::Skipped {
                reason: SkipReason::TooLarge,
            };
        }

        if self.config.skip_high_entropy && is_high_entropy(&data[..data.len().min(64)]) {
            self.stats.chunks_skipped += 1;
            return DedupDecision::Skipped {
                reason: SkipReason::HighEntropy,
            };
        }

        if index.contains(&hash) {
            self.stats.chunks_deduplicated += 1;
            self.stats.bytes_saved += data.len() as u64;
            return DedupDecision::Deduplicate {
                existing_hash: hash,
            };
        }

        self.stats.chunks_written_through += 1;
        DedupDecision::WriteThrough
    }

    pub fn stats(&self) -> &InlineDedupStats {
        &self.stats
    }
}

fn is_high_entropy(sample: &[u8]) -> bool {
    let mut seen = [false; 256];
    for &b in sample {
        seen[b as usize] = true;
    }
    seen.iter().filter(|&&v| v).count() >= 48
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_dedup_config_default() {
        let config = InlineDedupConfig::default();
        assert_eq!(config.min_chunk_size, 4096);
        assert_eq!(config.max_chunk_size, 4 * 1024 * 1024);
        assert!(!config.skip_high_entropy);
    }

    #[test]
    fn evaluate_too_small() {
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let index = InlineDedupIndex::new();
        let data = vec![0u8; 1024];
        let hash = [0u8; 32];
        let result = dedup.evaluate(&data, hash, &index);
        assert_eq!(
            result,
            DedupDecision::Skipped {
                reason: SkipReason::TooSmall
            }
        );
    }

    #[test]
    fn evaluate_too_large() {
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let index = InlineDedupIndex::new();
        let data = vec![0u8; 8 * 1024 * 1024];
        let hash = [0u8; 32];
        let result = dedup.evaluate(&data, hash, &index);
        assert_eq!(
            result,
            DedupDecision::Skipped {
                reason: SkipReason::TooLarge
            }
        );
    }

    #[test]
    fn evaluate_exact_min_size_not_skipped() {
        let config = InlineDedupConfig {
            min_chunk_size: 4096,
            max_chunk_size: 8 * 1024 * 1024,
            skip_high_entropy: false,
        };
        let mut dedup = InlineDedup::new(config);
        let index = InlineDedupIndex::new();
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        let result = dedup.evaluate(&data, hash, &index);
        assert_ne!(
            result,
            DedupDecision::Skipped {
                reason: SkipReason::TooSmall
            }
        );
    }

    #[test]
    fn evaluate_hit_in_index() {
        let mut index = InlineDedupIndex::new();
        index.insert([1u8; 32]);
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        let result = dedup.evaluate(&data, hash, &index);
        assert_eq!(
            result,
            DedupDecision::Deduplicate {
                existing_hash: hash
            }
        );
    }

    #[test]
    fn evaluate_miss_in_index() {
        let index = InlineDedupIndex::new();
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        let result = dedup.evaluate(&data, hash, &index);
        assert_eq!(result, DedupDecision::WriteThrough);
    }

    #[test]
    fn stats_evaluated_increments() {
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let index = InlineDedupIndex::new();
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        dedup.evaluate(&data, hash, &index);
        assert_eq!(dedup.stats().chunks_evaluated, 1);
    }

    #[test]
    fn stats_deduped_increments() {
        let mut index = InlineDedupIndex::new();
        index.insert([1u8; 32]);
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        dedup.evaluate(&data, hash, &index);
        assert_eq!(dedup.stats().chunks_deduplicated, 1);
    }

    #[test]
    fn stats_written_through_increments() {
        let index = InlineDedupIndex::new();
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        dedup.evaluate(&data, hash, &index);
        assert_eq!(dedup.stats().chunks_written_through, 1);
    }

    #[test]
    fn stats_skipped_increments() {
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let index = InlineDedupIndex::new();
        let data = vec![0u8; 1024];
        let hash = [1u8; 32];
        dedup.evaluate(&data, hash, &index);
        assert_eq!(dedup.stats().chunks_skipped, 1);
    }

    #[test]
    fn bytes_saved_on_dedup() {
        let mut index = InlineDedupIndex::new();
        index.insert([1u8; 32]);
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        dedup.evaluate(&data, hash, &index);
        assert_eq!(dedup.stats().bytes_saved, 4096);
    }

    #[test]
    fn bytes_saved_zero_on_miss() {
        let index = InlineDedupIndex::new();
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        dedup.evaluate(&data, hash, &index);
        assert_eq!(dedup.stats().bytes_saved, 0);
    }

    #[test]
    fn dedup_ratio_zero_when_none_evaluated() {
        let dedup = InlineDedup::new(InlineDedupConfig::default());
        assert_eq!(dedup.stats().dedup_ratio(), 0.0);
    }

    #[test]
    fn dedup_ratio_one_when_all_deduped() {
        let mut index = InlineDedupIndex::new();
        index.insert([1u8; 32]);
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        dedup.evaluate(&data, hash, &index);
        assert_eq!(dedup.stats().dedup_ratio(), 1.0);
    }

    #[test]
    fn dedup_ratio_zero_when_none_deduped() {
        let index = InlineDedupIndex::new();
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        let hash = [1u8; 32];
        dedup.evaluate(&data, hash, &index);
        assert_eq!(dedup.stats().dedup_ratio(), 0.0);
    }

    #[test]
    fn dedup_ratio_half() {
        let mut index = InlineDedupIndex::new();
        index.insert([1u8; 32]);
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        dedup.evaluate(&data, [1u8; 32], &index);
        dedup.evaluate(&data, [2u8; 32], &index);
        assert_eq!(dedup.stats().dedup_ratio(), 0.5);
    }

    #[test]
    fn skip_high_entropy_disabled() {
        let config = InlineDedupConfig {
            min_chunk_size: 64,
            max_chunk_size: 1024 * 1024,
            skip_high_entropy: false,
        };
        let mut dedup = InlineDedup::new(config);
        let index = InlineDedupIndex::new();
        let mut data = vec![0u8; 64];
        for i in 0..64 {
            data[i] = (i * 7) as u8;
        }
        let result = dedup.evaluate(&data, [1u8; 32], &index);
        assert_ne!(
            result,
            DedupDecision::Skipped {
                reason: SkipReason::HighEntropy
            }
        );
    }

    #[test]
    fn skip_high_entropy_enabled() {
        let config = InlineDedupConfig {
            min_chunk_size: 64,
            max_chunk_size: 1024 * 1024,
            skip_high_entropy: true,
        };
        let mut dedup = InlineDedup::new(config);
        let index = InlineDedupIndex::new();
        let mut data = vec![0u8; 64];
        for i in 0..64 {
            data[i] = (i * 7) as u8;
        }
        let result = dedup.evaluate(&data, [1u8; 32], &index);
        assert_eq!(
            result,
            DedupDecision::Skipped {
                reason: SkipReason::HighEntropy
            }
        );
    }

    #[test]
    fn is_high_entropy_low() {
        let data = vec![0u8; 10];
        assert!(!is_high_entropy(&data));
    }

    #[test]
    fn is_high_entropy_high() {
        let mut data = vec![0u8; 50];
        for i in 0..50 {
            data[i] = (i * 5) as u8;
        }
        assert!(is_high_entropy(&data));
    }

    #[test]
    fn inline_dedup_index_empty() {
        let index = InlineDedupIndex::new();
        assert!(index.is_empty());
    }

    #[test]
    fn inline_dedup_index_insert() {
        let mut index = InlineDedupIndex::new();
        index.insert([1u8; 32]);
        assert!(index.contains(&[1u8; 32]));
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn inline_dedup_index_miss() {
        let index = InlineDedupIndex::new();
        assert!(!index.contains(&[1u8; 32]));
    }

    #[test]
    fn multiple_evaluations_accumulate_stats() {
        let mut index = InlineDedupIndex::new();
        index.insert([1u8; 32]);
        let mut dedup = InlineDedup::new(InlineDedupConfig::default());
        let data = vec![0u8; 4096];
        dedup.evaluate(&data, [1u8; 32], &index);
        dedup.evaluate(&data, [2u8; 32], &index);
        dedup.evaluate(&data, [3u8; 32], &index);
        assert_eq!(dedup.stats().chunks_evaluated, 3);
    }
}
