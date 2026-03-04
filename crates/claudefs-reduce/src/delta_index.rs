use std::collections::HashMap;

pub type SuperFeature = u64;

#[derive(Debug, Clone)]
pub struct DeltaIndexEntry {
    pub block_hash: [u8; 32],
    pub features: [SuperFeature; 4],
    pub size_bytes: u32,
}

#[derive(Debug, Clone)]
pub struct DeltaIndexConfig {
    pub max_entries: usize,
    pub similarity_threshold: usize,
}

impl Default for DeltaIndexConfig {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            similarity_threshold: 3,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeltaIndexStats {
    pub inserts: u64,
    pub lookups: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

impl DeltaIndexStats {
    pub fn hit_rate(&self) -> f64 {
        if self.lookups == 0 {
            return 0.0;
        }
        self.hits as f64 / self.lookups as f64
    }
}

pub struct DeltaIndex {
    config: DeltaIndexConfig,
    inverted: HashMap<SuperFeature, Vec<[u8; 32]>>,
    entries: std::collections::HashMap<[u8; 32], DeltaIndexEntry>,
    insertion_order: std::collections::VecDeque<[u8; 32]>,
    stats: DeltaIndexStats,
}

impl DeltaIndex {
    pub fn new(config: DeltaIndexConfig) -> Self {
        Self {
            config,
            inverted: HashMap::new(),
            entries: std::collections::HashMap::new(),
            insertion_order: std::collections::VecDeque::new(),
            stats: DeltaIndexStats::default(),
        }
    }

    pub fn insert(&mut self, entry: DeltaIndexEntry) {
        while self.entries.len() >= self.config.max_entries {
            if let Some(old_hash) = self.insertion_order.pop_front() {
                if let Some(old_entry) = self.entries.remove(&old_hash) {
                    for &feat in &old_entry.features {
                        if let Some(list) = self.inverted.get_mut(&feat) {
                            list.retain(|h| h != &old_hash);
                        }
                    }
                    self.stats.evictions += 1;
                }
            }
        }

        let hash = entry.block_hash;
        for &feat in &entry.features {
            self.inverted.entry(feat).or_default().push(hash);
        }
        self.entries.insert(hash, entry);
        self.insertion_order.push_back(hash);
        self.stats.inserts += 1;
    }

    pub fn find_candidates(&mut self, query_features: &[SuperFeature; 4]) -> Vec<[u8; 32]> {
        self.stats.lookups += 1;
        let mut match_counts: HashMap<[u8; 32], usize> = HashMap::new();

        for &feat in query_features.iter() {
            if let Some(hashes) = self.inverted.get(&feat) {
                for &hash in hashes {
                    *match_counts.entry(hash).or_default() += 1;
                }
            }
        }

        let candidates: Vec<[u8; 32]> = match_counts
            .into_iter()
            .filter(|(_, count)| *count >= self.config.similarity_threshold)
            .map(|(hash, _)| hash)
            .collect();

        if candidates.is_empty() {
            self.stats.misses += 1;
        } else {
            self.stats.hits += 1;
        }
        candidates
    }

    pub fn remove(&mut self, block_hash: &[u8; 32]) -> bool {
        if let Some(entry) = self.entries.remove(block_hash) {
            for &feat in &entry.features {
                if let Some(list) = self.inverted.get_mut(&feat) {
                    list.retain(|h| h != block_hash);
                }
            }
            self.insertion_order.retain(|h| h != block_hash);
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn stats(&self) -> &DeltaIndexStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(v: u8) -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = v;
        h
    }

    fn make_entry(v: u8, feats: [SuperFeature; 4]) -> DeltaIndexEntry {
        DeltaIndexEntry {
            block_hash: make_hash(v),
            features: feats,
            size_bytes: 4096,
        }
    }

    #[test]
    fn delta_index_config_default() {
        let config = DeltaIndexConfig::default();
        assert_eq!(config.max_entries, 100_000);
        assert_eq!(config.similarity_threshold, 3);
    }

    #[test]
    fn new_index_empty() {
        let index = DeltaIndex::new(DeltaIndexConfig::default());
        assert_eq!(index.len(), 0);
        assert!(index.is_empty());
    }

    #[test]
    fn insert_increments_len() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn find_candidates_empty_index() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        let candidates = index.find_candidates(&[1, 2, 3, 4]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn find_candidates_miss() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [10, 20, 30, 40]));
        let candidates = index.find_candidates(&[1, 2, 3, 4]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn find_candidates_hit_with_3_matching() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        let candidates = index.find_candidates(&[1, 2, 3, 100]);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn find_candidates_miss_with_2_matching() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        let candidates = index.find_candidates(&[1, 2, 100, 101]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn find_candidates_hit_all_4_matching() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        let candidates = index.find_candidates(&[1, 2, 3, 4]);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn stats_inserts_increments() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        assert_eq!(index.stats().inserts, 1);
    }

    #[test]
    fn stats_lookups_increments() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.find_candidates(&[1, 2, 3, 4]);
        assert_eq!(index.stats().lookups, 1);
    }

    #[test]
    fn stats_hits_increments() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        index.find_candidates(&[1, 2, 3, 4]);
        assert_eq!(index.stats().hits, 1);
    }

    #[test]
    fn stats_misses_increments() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.find_candidates(&[1, 2, 3, 4]);
        assert_eq!(index.stats().misses, 1);
    }

    #[test]
    fn hit_rate_zero_when_no_lookups() {
        let index = DeltaIndex::new(DeltaIndexConfig::default());
        assert_eq!(index.stats().hit_rate(), 0.0);
    }

    #[test]
    fn hit_rate_one_when_all_hits() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        index.find_candidates(&[1, 2, 3, 4]);
        assert_eq!(index.stats().hit_rate(), 1.0);
    }

    #[test]
    fn remove_existing_entry() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        let entry = make_entry(1, [1, 2, 3, 4]);
        let hash = entry.block_hash;
        index.insert(entry);
        let removed = index.remove(&hash);
        assert!(removed);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn remove_nonexistent() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        let removed = index.remove(&make_hash(99));
        assert!(!removed);
    }

    #[test]
    fn remove_then_not_found() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        index.remove(&make_hash(1));
        let candidates = index.find_candidates(&[1, 2, 3, 4]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn eviction_at_capacity() {
        let mut index = DeltaIndex::new(DeltaIndexConfig {
            max_entries: 2,
            similarity_threshold: 1,
        });
        index.insert(make_entry(1, [1, 1, 1, 1]));
        index.insert(make_entry(2, [2, 2, 2, 2]));
        index.insert(make_entry(3, [3, 3, 3, 3]));
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn eviction_increments_stats() {
        let mut index = DeltaIndex::new(DeltaIndexConfig {
            max_entries: 1,
            similarity_threshold: 1,
        });
        index.insert(make_entry(1, [1, 1, 1, 1]));
        index.insert(make_entry(2, [2, 2, 2, 2]));
        assert_eq!(index.stats().evictions, 1);
    }

    #[test]
    fn multiple_blocks_same_feature() {
        let mut index = DeltaIndex::new(DeltaIndexConfig {
            max_entries: 100,
            similarity_threshold: 1,
        });
        index.insert(make_entry(1, [1, 2, 3, 4]));
        index.insert(make_entry(2, [1, 5, 6, 7]));
        let candidates = index.find_candidates(&[1, 100, 101, 102]);
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn insert_duplicate_hash() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        index.insert(make_entry(1, [5, 6, 7, 8]));
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn find_candidates_returns_multiple() {
        let mut index = DeltaIndex::new(DeltaIndexConfig::default());
        index.insert(make_entry(1, [1, 2, 3, 4]));
        index.insert(make_entry(2, [1, 2, 3, 5]));
        index.insert(make_entry(3, [6, 7, 8, 9]));
        let candidates = index.find_candidates(&[1, 2, 3, 100]);
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn threshold_1_finds_any_match() {
        let mut index = DeltaIndex::new(DeltaIndexConfig {
            max_entries: 100,
            similarity_threshold: 1,
        });
        index.insert(make_entry(1, [1, 2, 3, 4]));
        let candidates = index.find_candidates(&[1, 100, 101, 102]);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn large_index_insert_and_find() {
        let mut index = DeltaIndex::new(DeltaIndexConfig {
            max_entries: 1000,
            similarity_threshold: 2,
        });
        for i in 0..1000 {
            index.insert(make_entry(
                i as u8,
                [
                    i as SuperFeature,
                    i as SuperFeature + 1,
                    i as SuperFeature + 2,
                    i as SuperFeature + 3,
                ],
            ));
        }
        let candidates = index.find_candidates(&[500, 501, 1000, 1001]);
        assert!(!candidates.is_empty());
    }
}
