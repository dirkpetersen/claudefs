#[derive(Debug, Clone)]
pub struct ReplicationFilterConfig {
    pub expected_remote_blocks: usize,
    pub false_positive_rate: f64,
}

impl Default for ReplicationFilterConfig {
    fn default() -> Self {
        Self {
            expected_remote_blocks: 1_000_000,
            false_positive_rate: 0.01,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReplicationFilterStats {
    pub blocks_checked: u64,
    pub blocks_to_replicate: u64,
    pub blocks_already_remote: u64,
    pub false_positives_estimated: u64,
}

impl ReplicationFilterStats {
    pub fn replication_efficiency(&self) -> f64 {
        if self.blocks_checked == 0 {
            return 0.0;
        }
        1.0 - (self.blocks_to_replicate as f64 / self.blocks_checked as f64)
    }
}

struct SimpleBloom {
    bits: Vec<bool>,
    size: usize,
    k: usize,
}

impl SimpleBloom {
    fn new(size: usize, k: usize) -> Self {
        Self {
            bits: vec![false; size],
            size,
            k,
        }
    }

    fn set(&mut self, hash: &[u8; 32]) {
        for i in 0..self.k {
            let idx = self.get_bit_index(hash, i);
            self.bits[idx] = true;
        }
    }

    fn test(&self, hash: &[u8; 32]) -> bool {
        for i in 0..self.k {
            let idx = self.get_bit_index(hash, i);
            if !self.bits[idx] {
                return false;
            }
        }
        true
    }

    fn get_bit_index(&self, hash: &[u8; 32], k: usize) -> usize {
        let offset = (k * 4) % 24;
        let mut val = 0u64;
        for i in 0..8 {
            val = val.wrapping_shl(8) | (hash[(offset + i) % 32] as u64);
        }
        (val as usize) % self.size
    }
}

pub struct ReplicationFilter {
    config: ReplicationFilterConfig,
    bloom: SimpleBloom,
    stats: ReplicationFilterStats,
}

impl ReplicationFilter {
    pub fn new(config: ReplicationFilterConfig) -> Self {
        let n = config.expected_remote_blocks as f64;
        let p = config.false_positive_rate;
        let m = (-(n * p.ln()) / (2f64.ln() * 2f64.ln())) as usize;
        let m = m.max(1024);
        let k = ((m as f64 / n) * 2f64.ln()) as usize;
        let k = k.max(1).min(8);

        Self {
            bloom: SimpleBloom::new(m, k),
            config,
            stats: ReplicationFilterStats::default(),
        }
    }

    pub fn mark_remote_has(&mut self, hash: [u8; 32]) {
        self.bloom.set(&hash);
    }

    pub fn needs_replication(&mut self, hash: &[u8; 32]) -> bool {
        self.stats.blocks_checked += 1;
        if self.bloom.test(hash) {
            self.stats.blocks_already_remote += 1;
            false
        } else {
            self.stats.blocks_to_replicate += 1;
            true
        }
    }

    pub fn filter_batch(&mut self, hashes: &[[u8; 32]]) -> Vec<[u8; 32]> {
        hashes
            .iter()
            .filter(|h| self.needs_replication(h))
            .copied()
            .collect()
    }

    pub fn stats(&self) -> &ReplicationFilterStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(val: u8) -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = val;
        h
    }

    #[test]
    fn replication_filter_config_default() {
        let config = ReplicationFilterConfig::default();
        assert_eq!(config.expected_remote_blocks, 1_000_000);
        assert!((config.false_positive_rate - 0.01).abs() < 0.001);
    }

    #[test]
    fn new_filter_no_remotes() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let needs = filter.needs_replication(&make_hash(1));
        assert!(needs);
    }

    #[test]
    fn mark_remote_has_then_no_replication() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let h = make_hash(1);
        filter.mark_remote_has(h);
        let needs = filter.needs_replication(&h);
        assert!(!needs);
    }

    #[test]
    fn unknown_block_needs_replication() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let needs = filter.needs_replication(&make_hash(99));
        assert!(needs);
    }

    #[test]
    fn stats_blocks_checked_increments() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let _ = filter.needs_replication(&make_hash(1));
        let _ = filter.needs_replication(&make_hash(2));
        assert_eq!(filter.stats().blocks_checked, 2);
    }

    #[test]
    fn stats_blocks_to_replicate_increments() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let _ = filter.needs_replication(&make_hash(1));
        assert_eq!(filter.stats().blocks_to_replicate, 1);
    }

    #[test]
    fn stats_already_remote_increments() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let h = make_hash(1);
        filter.mark_remote_has(h);
        let _ = filter.needs_replication(&h);
        assert_eq!(filter.stats().blocks_already_remote, 1);
    }

    #[test]
    fn filter_batch_empty() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let result = filter.filter_batch(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_batch_all_new() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let hashes = [make_hash(1), make_hash(2), make_hash(3)];
        let result = filter.filter_batch(&hashes);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn filter_batch_all_known() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        for i in 1..=3 {
            filter.mark_remote_has(make_hash(i));
        }
        let hashes = [make_hash(1), make_hash(2), make_hash(3)];
        let result = filter.filter_batch(&hashes);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_batch_partial() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        filter.mark_remote_has(make_hash(1));
        filter.mark_remote_has(make_hash(3));
        let hashes = [make_hash(1), make_hash(2), make_hash(3)];
        let result = filter.filter_batch(&hashes);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn replication_efficiency_zero_when_none_checked() {
        let stats = ReplicationFilterStats::default();
        assert!((stats.replication_efficiency() - 0.0).abs() < 0.001);
    }

    #[test]
    fn replication_efficiency_zero_when_all_replicated() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let _ = filter.needs_replication(&make_hash(1));
        let _ = filter.needs_replication(&make_hash(2));
        assert!((filter.stats().replication_efficiency() - 0.0).abs() < 0.001);
    }

    #[test]
    fn replication_efficiency_one_when_none_replicated() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        filter.mark_remote_has(make_hash(1));
        filter.mark_remote_has(make_hash(2));
        let _ = filter.needs_replication(&make_hash(1));
        let _ = filter.needs_replication(&make_hash(2));
        assert!((filter.stats().replication_efficiency() - 1.0).abs() < 0.001);
    }

    #[test]
    fn mark_multiple_remotes() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        filter.mark_remote_has(make_hash(1));
        filter.mark_remote_has(make_hash(2));
        filter.mark_remote_has(make_hash(3));
        assert!(!filter.needs_replication(&make_hash(1)));
        assert!(!filter.needs_replication(&make_hash(2)));
        assert!(!filter.needs_replication(&make_hash(3)));
    }

    #[test]
    fn filter_empty_hash() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let h = [0u8; 32];
        let needs = filter.needs_replication(&h);
        assert!(needs);
    }

    #[test]
    fn different_hashes_different_results() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        filter.mark_remote_has(make_hash(1));
        assert!(!filter.needs_replication(&make_hash(1)));
        assert!(filter.needs_replication(&make_hash(2)));
    }

    #[test]
    fn filter_large_batch() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let mut hashes = Vec::new();
        for i in 0..100 {
            hashes.push(make_hash(i));
        }
        for i in 0..50 {
            filter.mark_remote_has(make_hash(i));
        }
        let result = filter.filter_batch(&hashes);
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn simple_bloom_no_false_negatives() {
        let mut bloom = SimpleBloom::new(1024, 4);
        let h = make_hash(1);
        bloom.set(&h);
        assert!(bloom.test(&h));
    }

    #[test]
    fn simple_bloom_new_returns_false() {
        let bloom = SimpleBloom::new(1024, 4);
        let h = make_hash(99);
        assert!(!bloom.test(&h));
    }

    #[test]
    fn multiple_marks_same_hash_idempotent() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        let h = make_hash(1);
        filter.mark_remote_has(h);
        filter.mark_remote_has(h);
        assert!(!filter.needs_replication(&h));
    }

    #[test]
    fn stats_efficiency_partial() {
        let mut filter = ReplicationFilter::new(ReplicationFilterConfig::default());
        filter.mark_remote_has(make_hash(1));
        filter.mark_remote_has(make_hash(2));
        let _ = filter.needs_replication(&make_hash(1));
        let _ = filter.needs_replication(&make_hash(2));
        let _ = filter.needs_replication(&make_hash(3));
        let eff = filter.stats().replication_efficiency();
        assert!(eff > 0.0 && eff < 1.0);
    }
}
