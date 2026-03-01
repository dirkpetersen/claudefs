use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, trace};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TierClass {
    Hot,
    Warm,
    Cold,
    Frozen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TierOverridePolicy {
    Auto,
    PinFlash,
    ForceS3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRecord {
    pub segment_id: u64,
    pub access_count: u64,
    pub last_access_time: u64,
    pub first_access_time: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub sequential_read_count: u64,
    pub random_read_count: u64,
    pub size_bytes: u64,
}

impl AccessRecord {
    fn new(segment_id: u64, size_bytes: u64, current_time: u64) -> Self {
        Self {
            segment_id,
            access_count: 0,
            last_access_time: current_time,
            first_access_time: current_time,
            bytes_read: 0,
            bytes_written: 0,
            sequential_read_count: 0,
            random_read_count: 0,
            size_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessPattern {
    Sequential,
    Random,
    WriteOnceReadMany,
    WriteHeavy,
    ReadOnce,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieringDecision {
    pub segment_id: u64,
    pub current_tier: TierClass,
    pub recommended_tier: TierClass,
    pub score: f64,
    pub pattern: AccessPattern,
    pub override_policy: TierOverridePolicy,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieringPolicyConfig {
    pub analysis_window_secs: u64,
    pub hot_threshold: u64,
    pub warm_threshold: u64,
    pub frozen_after_secs: u64,
    pub recency_weight: f64,
    pub size_weight: f64,
    pub frequency_weight: f64,
    pub high_watermark: f64,
    pub low_watermark: f64,
}

impl Default for TieringPolicyConfig {
    fn default() -> Self {
        Self {
            analysis_window_secs: 3600,
            hot_threshold: 100,
            warm_threshold: 10,
            frozen_after_secs: 86400,
            recency_weight: 1.0,
            size_weight: 0.5,
            frequency_weight: 0.3,
            high_watermark: 0.8,
            low_watermark: 0.6,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TieringPolicyStats {
    pub decisions_made: u64,
    pub promotions_to_hot: u64,
    pub demotions_to_cold: u64,
    pub demotions_to_frozen: u64,
    pub overrides_applied: u64,
    pub patterns_detected: u64,
    pub eviction_candidates: u64,
}

pub struct TieringPolicyEngine {
    config: TieringPolicyConfig,
    access_records: HashMap<u64, AccessRecord>,
    overrides: HashMap<u64, TierOverridePolicy>,
    current_tiers: HashMap<u64, TierClass>,
    stats: TieringPolicyStats,
}

impl TieringPolicyEngine {
    pub fn new(config: TieringPolicyConfig) -> Self {
        trace!("Initializing TieringPolicyEngine with config: {:?}", config);
        Self {
            config,
            access_records: HashMap::new(),
            overrides: HashMap::new(),
            current_tiers: HashMap::new(),
            stats: TieringPolicyStats::default(),
        }
    }

    pub fn record_access(
        &mut self,
        segment_id: u64,
        bytes: u64,
        is_sequential: bool,
        is_write: bool,
        current_time: u64,
    ) {
        let record = self
            .access_records
            .entry(segment_id)
            .or_insert_with(|| AccessRecord::new(segment_id, 0, current_time));

        record.access_count += 1;
        record.last_access_time = current_time;

        if is_write {
            record.bytes_written += bytes;
        } else {
            record.bytes_read += bytes;
            if is_sequential {
                record.sequential_read_count += 1;
            } else {
                record.random_read_count += 1;
            }
        }

        debug!(
            segment_id,
            access_count = record.access_count,
            is_sequential,
            is_write,
            "Recorded access"
        );
    }

    pub fn set_override(&mut self, segment_id: u64, policy: TierOverridePolicy) {
        self.overrides.insert(segment_id, policy);
        self.stats.overrides_applied += 1;
        debug!(segment_id, ?policy, "Override set");
    }

    pub fn get_override(&self, segment_id: u64) -> TierOverridePolicy {
        self.overrides
            .get(&segment_id)
            .copied()
            .unwrap_or(TierOverridePolicy::Auto)
    }

    pub fn classify_segment(&self, segment_id: u64, current_time: u64) -> TierClass {
        let Some(record) = self.access_records.get(&segment_id) else {
            return TierClass::Cold;
        };

        let age = current_time.saturating_sub(record.last_access_time);

        if age > self.config.frozen_after_secs {
            return TierClass::Frozen;
        }

        if record.access_count >= self.config.hot_threshold {
            return TierClass::Hot;
        }

        if record.access_count >= self.config.warm_threshold {
            return TierClass::Warm;
        }

        TierClass::Cold
    }

    pub fn detect_pattern(&self, segment_id: u64) -> AccessPattern {
        let Some(record) = self.access_records.get(&segment_id) else {
            return AccessPattern::Unknown;
        };

        if record.bytes_written > 0 && record.bytes_read == 0 && record.access_count == 1 {
            return AccessPattern::WriteOnceReadMany;
        }

        if record.bytes_written > record.bytes_read * 10 && record.bytes_written > 1024 * 1024 {
            return AccessPattern::WriteHeavy;
        }

        if record.access_count == 1 && record.bytes_read > 0 {
            return AccessPattern::ReadOnce;
        }

        let total_reads = record.sequential_read_count + record.random_read_count;
        if total_reads > 0 {
            let sequential_ratio = record.sequential_read_count as f64 / total_reads as f64;
            if sequential_ratio > 0.8 {
                return AccessPattern::Sequential;
            } else if sequential_ratio < 0.2 {
                return AccessPattern::Random;
            }
        }

        if record.bytes_written > 0 && record.bytes_read > 0 {
            if record.bytes_read > record.bytes_written * 5 {
                return AccessPattern::WriteOnceReadMany;
            }
        }

        AccessPattern::Unknown
    }

    pub fn compute_eviction_score(&self, segment_id: u64, current_time: u64) -> f64 {
        let Some(record) = self.access_records.get(&segment_id) else {
            return 0.0;
        };

        let age = (current_time.saturating_sub(record.last_access_time)) as f64;
        let frequency = record.access_count as f64;
        let size = record.size_bytes as f64;

        let age_score = age * self.config.recency_weight;
        let size_penalty = size * self.config.size_weight;
        let frequency_bonus = frequency * self.config.frequency_weight;

        age_score + size_penalty - frequency_bonus
    }

    pub fn get_eviction_candidates(
        &mut self,
        current_time: u64,
        count: usize,
    ) -> Vec<TieringDecision> {
        self.stats.eviction_candidates = count as u64;

        let mut candidates: Vec<TieringDecision> = self
            .access_records
            .keys()
            .filter(|&&segment_id| {
                let override_policy = self.get_override(segment_id);
                override_policy == TierOverridePolicy::Auto
            })
            .map(|&segment_id| {
                let record = &self.access_records[&segment_id];
                let current_tier = self
                    .current_tiers
                    .get(&segment_id)
                    .copied()
                    .unwrap_or(TierClass::Cold);
                let pattern = self.detect_pattern(segment_id);
                let score = self.compute_eviction_score(segment_id, current_time);
                let override_policy = self.get_override(segment_id);

                TieringDecision {
                    segment_id,
                    current_tier,
                    recommended_tier: TierClass::Cold,
                    score,
                    pattern,
                    override_policy,
                    reason: String::new(),
                }
            })
            .collect();

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(count);

        debug!(count = candidates.len(), "Retrieved eviction candidates");
        candidates
    }

    pub fn make_decision(&mut self, segment_id: u64, current_time: u64) -> TieringDecision {
        let override_policy = self.get_override(segment_id);
        let current_tier = self
            .current_tiers
            .get(&segment_id)
            .copied()
            .unwrap_or(TierClass::Cold);
        let pattern = self.detect_pattern(segment_id);
        let score = self.compute_eviction_score(segment_id, current_time);

        if pattern != AccessPattern::Unknown {
            self.stats.patterns_detected += 1;
        }

        let (recommended_tier, reason) = match override_policy {
            TierOverridePolicy::PinFlash => {
                (TierClass::Hot, "Override: pinned to flash".to_string())
            }
            TierOverridePolicy::ForceS3 => (TierClass::Cold, "Override: forced to S3".to_string()),
            TierOverridePolicy::Auto => {
                let classified = self.classify_segment(segment_id, current_time);
                let reason = match classified {
                    TierClass::Hot => format!(
                        "Access count {} >= hot threshold {}",
                        self.access_records
                            .get(&segment_id)
                            .map(|r| r.access_count)
                            .unwrap_or(0),
                        self.config.hot_threshold
                    ),
                    TierClass::Warm => format!(
                        "Access count {} >= warm threshold {}",
                        self.access_records
                            .get(&segment_id)
                            .map(|r| r.access_count)
                            .unwrap_or(0),
                        self.config.warm_threshold
                    ),
                    TierClass::Cold => "Low access frequency".to_string(),
                    TierClass::Frozen => format!(
                        "No access for {} seconds",
                        current_time.saturating_sub(
                            self.access_records
                                .get(&segment_id)
                                .map(|r| r.last_access_time)
                                .unwrap_or(0)
                        )
                    ),
                };
                (classified, reason)
            }
        };

        if recommended_tier != current_tier {
            if recommended_tier == TierClass::Hot && current_tier != TierClass::Hot {
                self.stats.promotions_to_hot += 1;
            } else if recommended_tier == TierClass::Cold && current_tier != TierClass::Cold {
                self.stats.demotions_to_cold += 1;
            } else if recommended_tier == TierClass::Frozen {
                self.stats.demotions_to_frozen += 1;
            }
            self.current_tiers.insert(segment_id, recommended_tier);
        }

        self.stats.decisions_made += 1;

        debug!(
            segment_id,
            current = ?current_tier,
            recommended = ?recommended_tier,
            ?override_policy,
            "Made tiering decision"
        );

        TieringDecision {
            segment_id,
            current_tier,
            recommended_tier,
            score,
            pattern,
            override_policy,
            reason,
        }
    }

    pub fn register_segment(&mut self, segment_id: u64, size_bytes: u64, current_time: u64) {
        self.access_records
            .entry(segment_id)
            .or_insert_with(|| AccessRecord::new(segment_id, size_bytes, current_time));
        if !self.current_tiers.contains_key(&segment_id) {
            self.current_tiers.insert(segment_id, TierClass::Cold);
        }
        trace!(segment_id, size_bytes, "Registered segment");
    }

    pub fn remove_segment(&mut self, segment_id: u64) {
        self.access_records.remove(&segment_id);
        self.overrides.remove(&segment_id);
        self.current_tiers.remove(&segment_id);
        trace!(segment_id, "Removed segment");
    }

    pub fn segment_count(&self) -> usize {
        self.access_records.len()
    }

    pub fn stats(&self) -> &TieringPolicyStats {
        &self.stats
    }

    pub fn get_tier(&self, segment_id: u64) -> Option<&TierClass> {
        self.current_tiers.get(&segment_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> TieringPolicyConfig {
        TieringPolicyConfig::default()
    }

    #[test]
    fn test_new_engine_empty() {
        let config = create_test_config();
        let engine = TieringPolicyEngine::new(config);
        assert_eq!(engine.segment_count(), 0);
    }

    #[test]
    fn test_register_segment() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);

        assert_eq!(engine.segment_count(), 1);
        assert_eq!(engine.get_tier(1), Some(&TierClass::Cold));
    }

    #[test]
    fn test_remove_segment() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        assert_eq!(engine.segment_count(), 1);

        engine.remove_segment(1);
        assert_eq!(engine.segment_count(), 0);
    }

    #[test]
    fn test_record_access_creates_record() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.record_access(1, 1024, true, false, 200);

        let record = engine.access_records.get(&1).unwrap();
        assert_eq!(record.access_count, 1);
    }

    #[test]
    fn test_record_access_increments_count() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);

        for _ in 0..5 {
            engine.record_access(1, 1024, true, false, 200);
        }

        let record = engine.access_records.get(&1).unwrap();
        assert_eq!(record.access_count, 5);
    }

    #[test]
    fn test_record_access_sequential_tracking() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.record_access(1, 4096, true, false, 200);
        engine.record_access(1, 4096, true, false, 300);

        let record = engine.access_records.get(&1).unwrap();
        assert_eq!(record.sequential_read_count, 2);
    }

    #[test]
    fn test_record_access_random_tracking() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.record_access(1, 4096, false, false, 200);
        engine.record_access(1, 4096, false, false, 300);

        let record = engine.access_records.get(&1).unwrap();
        assert_eq!(record.random_read_count, 2);
    }

    #[test]
    fn test_classify_hot_segment() {
        let mut config = create_test_config();
        config.hot_threshold = 10;
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        for _ in 0..15 {
            engine.record_access(1, 1024, true, false, 200);
        }

        assert_eq!(engine.classify_segment(1, 500), TierClass::Hot);
    }

    #[test]
    fn test_classify_warm_segment() {
        let mut config = create_test_config();
        config.hot_threshold = 100;
        config.warm_threshold = 10;
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        for _ in 0..15 {
            engine.record_access(1, 1024, true, false, 200);
        }

        assert_eq!(engine.classify_segment(1, 500), TierClass::Warm);
    }

    #[test]
    fn test_classify_cold_segment() {
        let config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);

        assert_eq!(engine.classify_segment(1, 500), TierClass::Cold);
    }

    #[test]
    fn test_classify_frozen_segment() {
        let mut config = create_test_config();
        config.frozen_after_secs = 100;
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);

        assert_eq!(engine.classify_segment(1, 300), TierClass::Frozen);
    }

    #[test]
    fn test_detect_pattern_sequential() {
        let config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        for _ in 0..10 {
            engine.record_access(1, 4096, true, false, 200);
        }

        assert_eq!(engine.detect_pattern(1), AccessPattern::Sequential);
    }

    #[test]
    fn test_detect_pattern_random() {
        let config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        for _ in 0..10 {
            engine.record_access(1, 4096, false, false, 200);
        }

        assert_eq!(engine.detect_pattern(1), AccessPattern::Random);
    }

    #[test]
    fn test_detect_pattern_write_heavy() {
        let config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        for _ in 0..100 {
            engine.record_access(1, 4096, true, true, 200);
        }

        assert_eq!(engine.detect_pattern(1), AccessPattern::WriteHeavy);
    }

    #[test]
    fn test_detect_pattern_write_once_read_many() {
        let config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.record_access(1, 4096, true, true, 200);

        for _ in 0..10 {
            engine.record_access(1, 4096, true, false, 300);
        }

        assert_eq!(engine.detect_pattern(1), AccessPattern::WriteOnceReadMany);
    }

    #[test]
    fn test_detect_pattern_unknown() {
        let config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);

        assert_eq!(engine.detect_pattern(1), AccessPattern::Unknown);
    }

    #[test]
    fn test_eviction_score_old_large_segment() {
        let mut config = create_test_config();
        config.recency_weight = 1.0;
        config.size_weight = 0.5;
        config.frequency_weight = 0.3;
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 1024 * 1024 * 10, 100);

        let score = engine.compute_eviction_score(1, 10000);

        assert!(score > 0.0);
    }

    #[test]
    fn test_eviction_score_recent_small_segment() {
        let mut config = create_test_config();
        config.recency_weight = 1.0;
        config.size_weight = 0.5;
        config.frequency_weight = 0.3;
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 1024, 100);
        for _ in 0..10 {
            engine.record_access(1, 1024, true, false, 150);
        }

        let score = engine.compute_eviction_score(1, 200);

        assert!(score < 1000.0);
    }

    #[test]
    fn test_override_pin_flash() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.set_override(1, TierOverridePolicy::PinFlash);

        assert_eq!(engine.get_override(1), TierOverridePolicy::PinFlash);

        let decision = engine.make_decision(1, 500);
        assert_eq!(decision.recommended_tier, TierClass::Hot);
    }

    #[test]
    fn test_override_force_s3() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.set_override(1, TierOverridePolicy::ForceS3);

        assert_eq!(engine.get_override(1), TierOverridePolicy::ForceS3);

        let decision = engine.make_decision(1, 500);
        assert_eq!(decision.recommended_tier, TierClass::Cold);
    }

    #[test]
    fn test_override_auto() {
        let config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);

        assert_eq!(engine.get_override(1), TierOverridePolicy::Auto);
    }

    #[test]
    fn test_get_eviction_candidates_sorted() {
        let mut config = create_test_config();
        config.recency_weight = 1.0;
        config.size_weight = 0.5;
        config.frequency_weight = 0.1;
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.register_segment(2, 1024 * 1024, 100);
        engine.register_segment(3, 4096, 100);

        engine.record_access(1, 1024, true, false, 200);

        let candidates = engine.get_eviction_candidates(10000, 3);

        assert_eq!(candidates.len(), 3);

        for i in 1..candidates.len() {
            assert!(candidates[i - 1].score >= candidates[i].score);
        }
    }

    #[test]
    fn test_make_decision_with_override() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.set_override(1, TierOverridePolicy::PinFlash);

        let decision = engine.make_decision(1, 500);

        assert_eq!(decision.override_policy, TierOverridePolicy::PinFlash);
        assert_eq!(decision.recommended_tier, TierClass::Hot);
    }

    #[test]
    fn test_make_decision_auto() {
        let mut config = create_test_config();
        config.hot_threshold = 5;
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        for _ in 0..10 {
            engine.record_access(1, 1024, true, false, 200);
        }

        let decision = engine.make_decision(1, 500);

        assert_eq!(decision.override_policy, TierOverridePolicy::Auto);
        assert_eq!(decision.recommended_tier, TierClass::Hot);
    }

    #[test]
    fn test_stats_tracking() {
        let mut config = create_test_config();
        config.hot_threshold = 5;
        let mut engine = TieringPolicyEngine::new(config);

        engine.register_segment(1, 4096, 100);
        engine.set_override(1, TierOverridePolicy::PinFlash);

        for _ in 0..10 {
            engine.record_access(1, 1024, true, false, 200);
        }

        let _ = engine.make_decision(1, 500);

        let stats = engine.stats();
        assert_eq!(stats.decisions_made, 1);
        assert_eq!(stats.overrides_applied, 1);
    }

    #[test]
    fn test_tiering_config_default() {
        let config = TieringPolicyConfig::default();

        assert_eq!(config.analysis_window_secs, 3600);
        assert_eq!(config.hot_threshold, 100);
        assert_eq!(config.warm_threshold, 10);
        assert_eq!(config.frozen_after_secs, 86400);
        assert_eq!(config.recency_weight, 1.0);
        assert_eq!(config.size_weight, 0.5);
        assert_eq!(config.frequency_weight, 0.3);
        assert_eq!(config.high_watermark, 0.8);
        assert_eq!(config.low_watermark, 0.6);
    }

    #[test]
    fn test_segment_count() {
        let mut config = create_test_config();
        let mut engine = TieringPolicyEngine::new(config);

        assert_eq!(engine.segment_count(), 0);

        engine.register_segment(1, 4096, 100);
        assert_eq!(engine.segment_count(), 1);

        engine.register_segment(2, 4096, 100);
        assert_eq!(engine.segment_count(), 2);

        engine.remove_segment(1);
        assert_eq!(engine.segment_count(), 1);
    }
}
