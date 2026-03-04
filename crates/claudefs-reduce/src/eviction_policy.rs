use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvictionStrategy {
    LruFirst,
    LargestFirst,
    AgeWeightedSize,
    Never,
}

#[derive(Debug, Clone)]
pub struct EvictableSegment {
    pub segment_id: u64,
    pub size_bytes: u64,
    pub last_access_secs: u64,
    pub current_time_secs: u64,
    pub pinned: bool,
}

impl EvictableSegment {
    pub fn age_secs(&self) -> u64 {
        self.current_time_secs.saturating_sub(self.last_access_secs)
    }

    pub fn eviction_score(&self) -> u64 {
        self.age_secs().saturating_mul(self.size_bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvictionPolicyConfig {
    pub strategy: EvictionStrategy,
    pub target_free_bytes: u64,
    pub max_segments_per_pass: usize,
}

impl Default for EvictionPolicyConfig {
    fn default() -> Self {
        Self {
            strategy: EvictionStrategy::AgeWeightedSize,
            target_free_bytes: 10 * 1024 * 1024 * 1024,
            max_segments_per_pass: 1000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EvictionPassStats {
    pub segments_selected: usize,
    pub bytes_to_free: u64,
    pub candidates_evaluated: usize,
    pub pinned_skipped: usize,
}

pub struct EvictionPolicy {
    config: EvictionPolicyConfig,
}

impl EvictionPolicy {
    pub fn new(config: EvictionPolicyConfig) -> Self {
        Self { config }
    }

    pub fn select_for_eviction(
        &self,
        mut candidates: Vec<EvictableSegment>,
    ) -> (Vec<EvictableSegment>, EvictionPassStats) {
        let mut stats = EvictionPassStats {
            candidates_evaluated: candidates.len(),
            ..Default::default()
        };

        let pinned_count = candidates.iter().filter(|c| c.pinned).count();
        stats.pinned_skipped = pinned_count;
        candidates.retain(|c| !c.pinned);

        match self.config.strategy {
            EvictionStrategy::LruFirst => {
                candidates.sort_by_key(|c| c.last_access_secs);
            }
            EvictionStrategy::LargestFirst => {
                candidates.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
            }
            EvictionStrategy::AgeWeightedSize => {
                candidates.sort_by(|a, b| b.eviction_score().cmp(&a.eviction_score()));
            }
            EvictionStrategy::Never => {
                return (Vec::new(), stats);
            }
        }

        let mut selected = Vec::new();
        let mut freed = 0u64;
        for candidate in candidates {
            if freed >= self.config.target_free_bytes
                || selected.len() >= self.config.max_segments_per_pass
            {
                break;
            }
            if freed + candidate.size_bytes > self.config.target_free_bytes
                && self.config.target_free_bytes > 0
            {
                break;
            }
            freed += candidate.size_bytes;
            selected.push(candidate);
        }

        stats.segments_selected = selected.len();
        stats.bytes_to_free = freed;
        (selected, stats)
    }

    pub fn strategy(&self) -> &EvictionStrategy {
        &self.config.strategy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_segment(
        id: u64,
        size: u64,
        last_access: u64,
        current: u64,
        pinned: bool,
    ) -> EvictableSegment {
        EvictableSegment {
            segment_id: id,
            size_bytes: size,
            last_access_secs: last_access,
            current_time_secs: current,
            pinned,
        }
    }

    #[test]
    fn eviction_policy_config_default() {
        let config = EvictionPolicyConfig::default();
        assert_eq!(config.strategy, EvictionStrategy::AgeWeightedSize);
        assert_eq!(config.target_free_bytes, 10 * 1024 * 1024 * 1024);
    }

    #[test]
    fn evictable_segment_age_secs() {
        let seg = make_segment(1, 1000, 100, 200, false);
        assert_eq!(seg.age_secs(), 100);
    }

    #[test]
    fn evictable_segment_age_saturates() {
        let seg = make_segment(1, 1000, 300, 200, false);
        assert_eq!(seg.age_secs(), 0);
    }

    #[test]
    fn eviction_score_age_times_size() {
        let seg = make_segment(1, 100, 100, 200, false);
        assert_eq!(seg.eviction_score(), 100 * 100);
    }

    #[test]
    fn eviction_score_zero_age() {
        let seg = make_segment(1, 1000, 200, 200, false);
        assert_eq!(seg.eviction_score(), 0);
    }

    #[test]
    fn select_empty_candidates() {
        let policy = EvictionPolicy::new(EvictionPolicyConfig::default());
        let (selected, stats) = policy.select_for_eviction(Vec::new());
        assert!(selected.is_empty());
        assert_eq!(stats.segments_selected, 0);
    }

    #[test]
    fn select_never_strategy() {
        let config = EvictionPolicyConfig {
            strategy: EvictionStrategy::Never,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![make_segment(1, 1000, 100, 200, false)];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert!(selected.is_empty());
    }

    #[test]
    fn select_lru_orders_oldest_first() {
        let config = EvictionPolicyConfig {
            strategy: EvictionStrategy::LruFirst,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(2, 100, 150, 200, false),
            make_segment(1, 100, 100, 200, false),
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected[0].segment_id, 1);
    }

    #[test]
    fn select_largest_first() {
        let config = EvictionPolicyConfig {
            strategy: EvictionStrategy::LargestFirst,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(1, 100, 100, 200, false),
            make_segment(2, 200, 100, 200, false),
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected[0].segment_id, 2);
    }

    #[test]
    fn select_age_weighted_highest_score_first() {
        let config = EvictionPolicyConfig {
            strategy: EvictionStrategy::AgeWeightedSize,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(1, 100, 100, 200, false), // age 100, score 10000
            make_segment(2, 50, 100, 200, false),  // age 100, score 5000
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected[0].segment_id, 1);
    }

    #[test]
    fn select_skips_pinned() {
        let policy = EvictionPolicy::new(EvictionPolicyConfig::default());
        let candidates = vec![
            make_segment(1, 100, 100, 200, true),
            make_segment(2, 100, 100, 200, false),
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].segment_id, 2);
    }

    #[test]
    fn stats_pinned_skipped_count() {
        let policy = EvictionPolicy::new(EvictionPolicyConfig::default());
        let candidates = vec![
            make_segment(1, 100, 100, 200, true),
            make_segment(2, 100, 100, 200, true),
            make_segment(3, 100, 100, 200, false),
        ];
        let (_, stats) = policy.select_for_eviction(candidates);
        assert_eq!(stats.pinned_skipped, 2);
    }

    #[test]
    fn stats_candidates_evaluated() {
        let policy = EvictionPolicy::new(EvictionPolicyConfig::default());
        let candidates = vec![
            make_segment(1, 100, 100, 200, false),
            make_segment(2, 100, 100, 200, false),
        ];
        let (_, stats) = policy.select_for_eviction(candidates);
        assert_eq!(stats.candidates_evaluated, 2);
    }

    #[test]
    fn stats_segments_selected() {
        let config = EvictionPolicyConfig {
            target_free_bytes: 1000,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(1, 100, 100, 200, false),
            make_segment(2, 200, 100, 200, false),
        ];
        let (_, stats) = policy.select_for_eviction(candidates);
        assert_eq!(stats.segments_selected, 2);
    }

    #[test]
    fn stats_bytes_to_free() {
        let config = EvictionPolicyConfig {
            target_free_bytes: 1000,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(1, 100, 100, 200, false),
            make_segment(2, 200, 100, 200, false),
        ];
        let (_, stats) = policy.select_for_eviction(candidates);
        assert_eq!(stats.bytes_to_free, 300);
    }

    #[test]
    fn max_segments_per_pass_limits() {
        let config = EvictionPolicyConfig {
            target_free_bytes: 1_000_000_000,
            max_segments_per_pass: 2,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(1, 100, 100, 200, false),
            make_segment(2, 100, 100, 200, false),
            make_segment(3, 100, 100, 200, false),
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn target_free_bytes_stops_selection() {
        let config = EvictionPolicyConfig {
            target_free_bytes: 150,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(1, 100, 100, 200, false),
            make_segment(2, 100, 100, 200, false),
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn select_all_pinned() {
        let policy = EvictionPolicy::new(EvictionPolicyConfig::default());
        let candidates = vec![
            make_segment(1, 100, 100, 200, true),
            make_segment(2, 200, 100, 200, true),
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert!(selected.is_empty());
    }

    #[test]
    fn select_one_candidate() {
        let policy = EvictionPolicy::new(EvictionPolicyConfig::default());
        let candidates = vec![make_segment(1, 100, 100, 200, false)];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn select_multiple_candidates() {
        let config = EvictionPolicyConfig {
            target_free_bytes: 1000,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(1, 100, 100, 200, false),
            make_segment(2, 200, 100, 200, false),
            make_segment(3, 300, 100, 200, false),
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn lru_strategy_last_access_ordering() {
        let config = EvictionPolicyConfig {
            strategy: EvictionStrategy::LruFirst,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(3, 100, 300, 400, false),
            make_segment(1, 100, 100, 400, false),
            make_segment(2, 100, 200, 400, false),
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected[0].segment_id, 1);
        assert_eq!(selected[1].segment_id, 2);
        assert_eq!(selected[2].segment_id, 3);
    }

    #[test]
    fn age_weighted_prefers_old_large() {
        let config = EvictionPolicyConfig {
            strategy: EvictionStrategy::AgeWeightedSize,
            ..Default::default()
        };
        let policy = EvictionPolicy::new(config);
        let candidates = vec![
            make_segment(1, 1000, 100, 200, false), // age 100, score 100000
            make_segment(2, 2000, 190, 200, false), // age 10, score 20000
        ];
        let (selected, _) = policy.select_for_eviction(candidates);
        assert_eq!(selected[0].segment_id, 1);
    }

    #[test]
    fn age_weighted_vs_lru() {
        let lru_config = EvictionPolicyConfig {
            strategy: EvictionStrategy::LruFirst,
            ..Default::default()
        };
        let lru_policy = EvictionPolicy::new(lru_config);
        let age_config = EvictionPolicyConfig {
            strategy: EvictionStrategy::AgeWeightedSize,
            ..Default::default()
        };
        let age_policy = EvictionPolicy::new(age_config);
        let candidates = vec![
            make_segment(1, 10, 100, 200, false), // old but small, score 1000
            make_segment(2, 1000, 199, 200, false), // young but large, score 1000
        ];
        let (lru_selected, _) = lru_policy.select_for_eviction(candidates.clone());
        let (age_selected, _) = age_policy.select_for_eviction(candidates);
        assert_eq!(lru_selected[0].segment_id, 1);
        assert_eq!(age_selected[0].segment_id, 1); // tie-breaker: stable sort preserves order
    }

    #[test]
    fn eviction_pass_stats_default() {
        let stats = EvictionPassStats::default();
        assert_eq!(stats.segments_selected, 0);
        assert_eq!(stats.bytes_to_free, 0);
        assert_eq!(stats.candidates_evaluated, 0);
        assert_eq!(stats.pinned_skipped, 0);
    }
}
