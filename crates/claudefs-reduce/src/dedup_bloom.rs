use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BloomConfig {
    pub expected_items: usize,
    pub false_positive_rate: f64,
}

impl Default for BloomConfig {
    fn default() -> Self {
        Self {
            expected_items: 1_000_000,
            false_positive_rate: 0.01,
        }
    }
}

impl BloomConfig {
    pub fn bit_count(&self) -> usize {
        let n = self.expected_items as f64;
        let p = self.false_positive_rate as f64;
        (n * (-1.44) * p.ln() / std::f64::consts::LN_2 / std::f64::consts::LN_2).ceil() as usize
    }

    pub fn hash_count(&self) -> usize {
        ((-(self.false_positive_rate as f64).ln() / std::f64::consts::LN_2).ceil() as usize).max(1)
    }
}

#[derive(Debug, Clone, Default)]
pub struct BloomStats {
    pub items_added: u64,
    pub queries: u64,
    pub definitely_absent: u64,
    pub possibly_present: u64,
}

impl BloomStats {
    pub fn false_negative_rate(&self) -> f64 {
        0.0
    }
}

pub struct DedupBloom {
    bits: Vec<bool>,
    config: BloomConfig,
    stats: BloomStats,
}

impl DedupBloom {
    pub fn new(config: BloomConfig) -> Self {
        Self {
            bits: vec![false; config.bit_count()],
            config,
            stats: BloomStats::default(),
        }
    }

    pub fn add(&mut self, hash: &[u8; 32]) {
        let bit_count = self.bits.len();
        let hash_count = self.config.hash_count();
        for idx in hash_values(hash, hash_count, bit_count) {
            self.bits[idx] = true;
        }
        self.stats.items_added += 1;
    }

    pub fn may_contain(&mut self, hash: &[u8; 32]) -> bool {
        self.stats.queries += 1;
        let bit_count = self.bits.len();
        let hash_count = self.config.hash_count();
        let result = hash_values(hash, hash_count, bit_count)
            .iter()
            .all(|&idx| self.bits[idx]);
        if result {
            self.stats.possibly_present += 1;
        } else {
            self.stats.definitely_absent += 1;
        }
        result
    }

    pub fn definitely_absent(&mut self, hash: &[u8; 32]) -> bool {
        !self.may_contain(hash)
    }

    pub fn stats(&self) -> &BloomStats {
        &self.stats
    }

    pub fn estimated_fill_ratio(&self) -> f64 {
        let total_bits = self.bits.len();
        if total_bits == 0 {
            return 0.0;
        }
        let set_bits = self.bits.iter().filter(|&&b| b).count();
        set_bits as f64 / total_bits as f64
    }
}

fn hash_values(hash: &[u8; 32], count: usize, bit_count: usize) -> Vec<usize> {
    (0..count)
        .map(|i| {
            let offset = i * 8;
            if offset + 8 <= 32 {
                let bytes: [u8; 8] = hash[offset..offset + 8].try_into().unwrap();
                (u64::from_le_bytes(bytes) as usize) % bit_count
            } else {
                let bytes: [u8; 8] = [
                    hash[offset % 32],
                    hash[(offset + 1) % 32],
                    hash[(offset + 2) % 32],
                    hash[(offset + 3) % 32],
                    hash[(offset + 4) % 32],
                    hash[(offset + 5) % 32],
                    hash[(offset + 6) % 32],
                    hash[(offset + 7) % 32],
                ];
                (u64::from_le_bytes(bytes) as usize) % bit_count
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloom_config_default() {
        let config = BloomConfig::default();
        assert_eq!(config.expected_items, 1_000_000);
        assert!((config.false_positive_rate - 0.01).abs() < 1e-10);
    }

    #[test]
    fn bloom_bit_count_calculation() {
        let config = BloomConfig {
            expected_items: 1_000_000,
            false_positive_rate: 0.01,
        };
        let bit_count = config.bit_count();
        assert!(bit_count > 0);
    }

    #[test]
    fn bloom_hash_count_calculation() {
        let config = BloomConfig {
            expected_items: 1_000_000,
            false_positive_rate: 0.01,
        };
        let hash_count = config.hash_count();
        assert!(hash_count >= 6 && hash_count <= 8);
    }

    #[test]
    fn add_and_may_contain_true() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let hash: [u8; 32] = [0x42; 32];
        bloom.add(&hash);
        assert!(bloom.may_contain(&hash));
    }

    #[test]
    fn definitely_absent_for_not_added() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let hash: [u8; 32] = [0x99; 32];
        assert!(bloom.definitely_absent(&hash));
    }

    #[test]
    fn add_multiple_hashes() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        for i in 0..10 {
            let hash: [u8; 32] = [i; 32];
            bloom.add(&hash);
        }
        assert_eq!(bloom.stats().items_added, 10);
    }

    #[test]
    fn may_contain_all_added() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let mut hashes = Vec::new();
        for i in 0..10 {
            let hash: [u8; 32] = [i; 32];
            bloom.add(&hash);
            hashes.push(hash);
        }
        for hash in &hashes {
            assert!(bloom.may_contain(hash));
        }
    }

    #[test]
    fn stats_items_added() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let hash: [u8; 32] = [1; 32];
        bloom.add(&hash);
        assert_eq!(bloom.stats().items_added, 1);
        bloom.add(&hash);
        assert_eq!(bloom.stats().items_added, 2);
    }

    #[test]
    fn stats_queries_after_check() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let hash: [u8; 32] = [1; 32];
        bloom.may_contain(&hash);
        assert_eq!(bloom.stats().queries, 1);
        bloom.definitely_absent(&hash);
        assert_eq!(bloom.stats().queries, 2);
    }

    #[test]
    fn stats_definitely_absent() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let hash: [u8; 32] = [1; 32];
        bloom.may_contain(&hash);
        assert_eq!(bloom.stats().definitely_absent, 1);
        assert_eq!(bloom.stats().possibly_present, 0);
    }

    #[test]
    fn stats_possibly_present() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let hash: [u8; 32] = [1; 32];
        bloom.add(&hash);
        bloom.may_contain(&hash);
        assert_eq!(bloom.stats().possibly_present, 1);
        assert_eq!(bloom.stats().definitely_absent, 0);
    }

    #[test]
    fn estimated_fill_ratio_increases() {
        let config = BloomConfig {
            expected_items: 1000,
            false_positive_rate: 0.01,
        };
        let mut bloom = DedupBloom::new(config);
        let initial_ratio = bloom.estimated_fill_ratio();
        for i in 0u64..50u64 {
            let mut hash = [0u8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            bloom.add(&hash);
        }
        let final_ratio = bloom.estimated_fill_ratio();
        assert!(final_ratio > initial_ratio);
    }

    #[test]
    fn bloom_no_false_negatives() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let hashes: Vec<[u8; 32]> = (0..100).map(|i| [i; 32]).collect();
        for hash in &hashes {
            bloom.add(hash);
        }
        for hash in &hashes {
            assert!(!bloom.definitely_absent(hash));
        }
    }

    #[test]
    fn empty_bloom_all_absent() {
        let config = BloomConfig::default();
        let mut bloom = DedupBloom::new(config);
        let hash: [u8; 32] = [0xAA; 32];
        assert!(bloom.definitely_absent(&hash));
    }

    #[test]
    fn false_positive_rate_reasonable() {
        let config = BloomConfig {
            expected_items: 1_000_000,
            false_positive_rate: 0.01,
        };
        let mut bloom = DedupBloom::new(config);
        let added_hashes: Vec<[u8; 32]> = (0u64..10000u64)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0..8].copy_from_slice(&i.to_le_bytes());
                h
            })
            .collect();
        for hash in &added_hashes {
            bloom.add(hash);
        }
        let not_added_hashes: Vec<[u8; 32]> = (100000u64..101000u64)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0..8].copy_from_slice(&i.to_le_bytes());
                h
            })
            .collect();
        let mut false_positives = 0;
        for hash in &not_added_hashes {
            if bloom.may_contain(hash) {
                false_positives += 1;
            }
        }
        let fpr = false_positives as f64 / not_added_hashes.len() as f64;
        assert!(fpr < 0.1);
    }
}
