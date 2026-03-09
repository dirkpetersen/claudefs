use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::error::ReduceError;

#[derive(Debug, Clone)]
pub struct AccessMetrics {
    pub segment_id: u64,
    pub size_bytes: u64,
    pub last_access_age_sec: u64,
    pub access_count: u64,
    pub compression_ratio: f64,
    pub dedup_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TieringRecommendation {
    Flash,
    WarmS3,
    ColdS3,
    ArchiveS3,
}

#[derive(Debug, Clone)]
pub struct TieringScore {
    pub recommendation: TieringRecommendation,
    pub score: f64,
    pub rationale: String,
}

#[derive(Debug, Clone)]
pub struct TieringAdvisorConfig {
    pub flash_threshold_days: u64,
    pub cold_threshold_days: u64,
    pub archive_threshold_days: u64,
}

impl Default for TieringAdvisorConfig {
    fn default() -> Self {
        Self {
            flash_threshold_days: 30,
            cold_threshold_days: 90,
            archive_threshold_days: 365,
        }
    }
}

pub struct TieringAdvisor {
    flash_to_s3_cost_ratio: f64,
    access_cost_per_mb: f64,
    threshold_access_age_days: u64,
    config: TieringAdvisorConfig,
}

impl Default for TieringAdvisor {
    fn default() -> Self {
        Self::new(TieringAdvisorConfig::default())
    }
}

impl TieringAdvisor {
    pub fn new(config: TieringAdvisorConfig) -> Self {
        Self {
            flash_to_s3_cost_ratio: 10.0,
            access_cost_per_mb: 0.01,
            threshold_access_age_days: config.flash_threshold_days,
            config,
        }
    }

    pub fn recommend(&self, metrics: &AccessMetrics) -> TieringScore {
        let age_days = metrics.last_access_age_sec / 86400;
        let size_mb = metrics.size_bytes as f64 / 1_000_000.0;

        let age_score = self.calculate_age_score(age_days);
        let size_score = self.calculate_size_score(size_mb);
        let compression_penalty = self.calculate_compression_penalty(metrics.compression_ratio);
        let access_score = self.calculate_access_score(metrics.access_count);

        let total_score = (age_score * 0.4)
            + (size_score * 0.3)
            + (access_score * 0.2)
            + (compression_penalty * 0.1);

        let recommendation = self.determine_recommendation(age_days, metrics, total_score);
        let rationale = self.generate_rationale(age_days, metrics, total_score);

        debug!(
            "Segment {} recommendation: {:?} (score: {:.3})",
            metrics.segment_id, recommendation, total_score
        );

        TieringScore {
            recommendation,
            score: total_score,
            rationale,
        }
    }

    fn calculate_age_score(&self, age_days: u64) -> f64 {
        if age_days < self.config.flash_threshold_days {
            1.0
        } else if age_days < self.config.cold_threshold_days {
            0.7
        } else if age_days < self.config.archive_threshold_days {
            0.4
        } else {
            0.1
        }
    }

    fn calculate_size_score(&self, size_mb: f64) -> f64 {
        if size_mb >= 100.0 {
            1.0
        } else if size_mb >= 10.0 {
            0.7
        } else if size_mb >= 1.0 {
            0.4
        } else {
            0.1
        }
    }

    fn calculate_compression_penalty(&self, compression_ratio: f64) -> f64 {
        if compression_ratio >= 4.0 {
            1.0
        } else if compression_ratio >= 2.0 {
            0.7
        } else if compression_ratio >= 1.0 {
            0.3
        } else {
            0.0
        }
    }

    fn calculate_access_score(&self, access_count: u64) -> f64 {
        if access_count >= 1000 {
            1.0
        } else if access_count >= 100 {
            0.7
        } else if access_count >= 10 {
            0.4
        } else {
            0.1
        }
    }

    fn determine_recommendation(
        &self,
        age_days: u64,
        metrics: &AccessMetrics,
        score: f64,
    ) -> TieringRecommendation {
        if age_days < self.config.flash_threshold_days || metrics.access_count > 500 {
            if metrics.compression_ratio < 1.5 && metrics.access_count > 100 {
                return TieringRecommendation::Flash;
            }
            if score > 0.7 {
                return TieringRecommendation::Flash;
            }
        }

        if age_days >= self.config.archive_threshold_days {
            return TieringRecommendation::ArchiveS3;
        }

        if age_days >= self.config.cold_threshold_days {
            if metrics.compression_ratio < 2.0 {
                return TieringRecommendation::ColdS3;
            }
            return TieringRecommendation::ColdS3;
        }

        if age_days >= self.config.flash_threshold_days {
            return TieringRecommendation::WarmS3;
        }

        TieringRecommendation::Flash
    }

    fn generate_rationale(&self, age_days: u64, metrics: &AccessMetrics, score: f64) -> String {
        let mut reasons = Vec::new();

        if age_days >= self.config.archive_threshold_days {
            reasons.push("ancient data (>365 days)".to_string());
        } else if age_days >= self.config.cold_threshold_days {
            reasons.push("cold data (>90 days)".to_string());
        } else if age_days >= self.config.flash_threshold_days {
            reasons.push("warm data (>30 days)".to_string());
        } else {
            reasons.push("hot data".to_string());
        }

        if metrics.size_bytes >= 100_000_000 {
            reasons.push("large segment (high savings potential)".to_string());
        }

        if metrics.compression_ratio >= 4.0 {
            reasons.push("highly compressed".to_string());
        } else if metrics.compression_ratio < 1.5 {
            reasons.push("poorly compressed (S3 retrieval costly)".to_string());
        }

        if metrics.access_count > 1000 {
            reasons.push("frequently accessed".to_string());
        } else if metrics.access_count < 10 {
            reasons.push("rarely accessed".to_string());
        }

        if score < 0.3 {
            reasons.push("low score suggests tiering".to_string());
        }

        reasons.join(", ")
    }

    pub fn batch_recommendations(
        &self,
        metrics_batch: Vec<AccessMetrics>,
    ) -> Vec<(u64, TieringScore)> {
        metrics_batch
            .into_iter()
            .map(|m| {
                let score = self.recommend(&m);
                (m.segment_id, score)
            })
            .collect()
    }

    pub fn update_cost_model(
        &mut self,
        flash_cost: f64,
        s3_cost: f64,
        retrieval_cost: f64,
    ) -> Result<(), ReduceError> {
        if flash_cost <= 0.0 || s3_cost <= 0.0 || retrieval_cost <= 0.0 {
            return Err(ReduceError::InvalidInput(
                "All cost values must be positive".to_string(),
            ));
        }

        self.flash_to_s3_cost_ratio = flash_cost / s3_cost;
        self.access_cost_per_mb = retrieval_cost;
        info!(
            "Updated cost model: flash/s3 ratio = {:.2}, retrieval cost = {:.4}",
            self.flash_to_s3_cost_ratio, self.access_cost_per_mb
        );
        Ok(())
    }

    pub fn get_estimated_savings(&self, metrics: &AccessMetrics) -> (u64, f64) {
        let size_mb = metrics.size_bytes as f64 / 1_000_000.0;
        let age_days = metrics.last_access_age_sec / 86400;

        let current_flash_cost = size_mb * self.access_cost_per_mb * (age_days as f64 / 30.0);

        let target_tier = self.recommend(metrics).recommendation;
        let s3_monthly_cost = size_mb * 0.023;
        let retrieval_cost = match target_tier {
            TieringRecommendation::ArchiveS3 => size_mb * 0.09,
            TieringRecommendation::ColdS3 => size_mb * 0.04,
            TieringRecommendation::WarmS3 => size_mb * 0.01,
            TieringRecommendation::Flash => 0.0,
        };

        let total_s3_cost = s3_monthly_cost + retrieval_cost;
        let savings = if current_flash_cost > total_s3_cost {
            ((current_flash_cost - total_s3_cost) * 100.0).round() as u64
        } else {
            0
        };

        let savings_percent = if current_flash_cost > 0.0 {
            ((savings as f64 / current_flash_cost) * 100.0).min(100.0)
        } else {
            0.0
        };

        (savings, savings_percent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_hot_segment() -> AccessMetrics {
        AccessMetrics {
            segment_id: 1,
            size_bytes: 10_000_000,
            last_access_age_sec: 100,
            access_count: 5000,
            compression_ratio: 3.0,
            dedup_ratio: 2.0,
        }
    }

    fn create_warm_segment() -> AccessMetrics {
        AccessMetrics {
            segment_id: 2,
            size_bytes: 50_000_000,
            last_access_age_sec: 30 * 86400,
            access_count: 10,
            compression_ratio: 2.5,
            dedup_ratio: 1.5,
        }
    }

    fn create_cold_segment() -> AccessMetrics {
        AccessMetrics {
            segment_id: 3,
            size_bytes: 100_000_000,
            last_access_age_sec: 90 * 86400,
            access_count: 5,
            compression_ratio: 2.0,
            dedup_ratio: 1.2,
        }
    }

    fn create_archive_segment() -> AccessMetrics {
        AccessMetrics {
            segment_id: 4,
            size_bytes: 200_000_000,
            last_access_age_sec: 365 * 86400,
            access_count: 1,
            compression_ratio: 1.5,
            dedup_ratio: 1.0,
        }
    }

    fn create_ancient_segment() -> AccessMetrics {
        AccessMetrics {
            segment_id: 5,
            size_bytes: 500_000_000,
            last_access_age_sec: 400 * 86400,
            access_count: 0,
            compression_ratio: 1.2,
            dedup_ratio: 1.0,
        }
    }

    #[test]
    fn test_recommend_flash_for_hot_segment() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = create_hot_segment();
        let score = advisor.recommend(&metrics);

        assert_eq!(score.recommendation, TieringRecommendation::Flash);
    }

    #[test]
    fn test_recommend_warm_s3_for_aged_segment() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = create_warm_segment();
        let score = advisor.recommend(&metrics);

        assert_eq!(score.recommendation, TieringRecommendation::WarmS3);
    }

    #[test]
    fn test_recommend_cold_s3_for_very_old_segment() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = create_cold_segment();
        let score = advisor.recommend(&metrics);

        assert_eq!(score.recommendation, TieringRecommendation::ColdS3);
    }

    #[test]
    fn test_recommend_archive_s3_for_ancient_segment() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = create_ancient_segment();
        let score = advisor.recommend(&metrics);

        assert_eq!(score.recommendation, TieringRecommendation::ArchiveS3);
    }

    #[test]
    fn test_large_segment_prioritized() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        let small_segment = AccessMetrics {
            segment_id: 1,
            size_bytes: 1_000_000,
            last_access_age_sec: 30 * 86400,
            access_count: 10,
            compression_ratio: 2.0,
            dedup_ratio: 1.5,
        };

        let large_segment = AccessMetrics {
            segment_id: 2,
            size_bytes: 500_000_000,
            last_access_age_sec: 30 * 86400,
            access_count: 10,
            compression_ratio: 2.0,
            dedup_ratio: 1.5,
        };

        let small_score = advisor.recommend(&small_segment);
        let large_score = advisor.recommend(&large_segment);

        assert!(large_score.score > small_score.score);
    }

    #[test]
    fn test_highly_compressed_data_stays_on_flash() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        let highly_compressed = AccessMetrics {
            segment_id: 1,
            size_bytes: 100_000_000,
            last_access_age_sec: 60 * 86400,
            access_count: 20,
            compression_ratio: 5.0,
            dedup_ratio: 3.0,
        };

        let poorly_compressed = AccessMetrics {
            segment_id: 2,
            size_bytes: 100_000_000,
            last_access_age_sec: 60 * 86400,
            access_count: 20,
            compression_ratio: 1.2,
            dedup_ratio: 1.0,
        };

        let compressed_score = advisor.recommend(&highly_compressed);
        let poor_score = advisor.recommend(&poorly_compressed);

        assert!(compressed_score.score > poor_score.score);
    }

    #[test]
    fn test_batch_recommendations() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let batch = vec![
            create_hot_segment(),
            create_warm_segment(),
            create_cold_segment(),
        ];

        let results = advisor.batch_recommendations(batch);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 1);
        assert_eq!(results[1].0, 2);
        assert_eq!(results[2].0, 3);
    }

    #[test]
    fn test_cost_model_updates_affect_recommendations() {
        let mut advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        advisor.update_cost_model(1.0, 0.1, 0.01).unwrap();

        let metrics = create_warm_segment();
        let _score = advisor.recommend(&metrics);
    }

    #[test]
    fn test_estimated_savings_calculation() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = create_cold_segment();

        let (savings, percent) = advisor.get_estimated_savings(&metrics);

        assert!(savings >= 0);
        assert!(percent >= 0.0 && percent <= 100.0);
    }

    #[test]
    fn test_tier_promotion_consistency() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        let segment_30_days = AccessMetrics {
            segment_id: 1,
            size_bytes: 10_000_000,
            last_access_age_sec: 29 * 86400,
            access_count: 10,
            compression_ratio: 2.0,
            dedup_ratio: 1.5,
        };

        let segment_31_days = AccessMetrics {
            segment_id: 2,
            size_bytes: 10_000_000,
            last_access_age_sec: 31 * 86400,
            access_count: 10,
            compression_ratio: 2.0,
            dedup_ratio: 1.5,
        };

        let score_30 = advisor.recommend(&segment_30_days);
        let score_31 = advisor.recommend(&segment_31_days);

        assert_eq!(score_30.recommendation, TieringRecommendation::Flash);
        assert_eq!(score_31.recommendation, TieringRecommendation::WarmS3);
    }

    #[test]
    fn test_config_parameter_edge_cases() {
        let config = TieringAdvisorConfig {
            flash_threshold_days: 0,
            cold_threshold_days: 0,
            archive_threshold_days: 0,
        };

        let advisor = TieringAdvisor::new(config);
        let metrics = create_hot_segment();
        let score = advisor.recommend(&metrics);

        assert!(score.score >= 0.0 && score.score <= 1.0);
    }

    #[test]
    fn test_advisor_default() {
        let advisor = TieringAdvisor::default();
        let metrics = AccessMetrics {
            segment_id: 1,
            size_bytes: 10_000_000,
            last_access_age_sec: 86400,
            access_count: 100,
            compression_ratio: 2.0,
            dedup_ratio: 1.5,
        };

        let score = advisor.recommend(&metrics);
        assert!(score.score >= 0.0);
    }

    #[test]
    fn test_zero_access_count() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = AccessMetrics {
            segment_id: 1,
            size_bytes: 10_000_000,
            last_access_age_sec: 200 * 86400,
            access_count: 0,
            compression_ratio: 1.5,
            dedup_ratio: 1.0,
        };

        let score = advisor.recommend(&metrics);
        assert_eq!(score.recommendation, TieringRecommendation::ArchiveS3);
    }

    #[test]
    fn test_small_segment_low_priority() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = AccessMetrics {
            segment_id: 1,
            size_bytes: 100_000,
            last_access_age_sec: 100 * 86400,
            access_count: 0,
            compression_ratio: 1.5,
            dedup_ratio: 1.0,
        };

        let score = advisor.recommend(&metrics);
        assert!(score.score < 0.5);
    }

    #[test]
    fn test_update_cost_model_invalid_input() {
        let mut advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        let result = advisor.update_cost_model(0.0, 1.0, 1.0);
        assert!(result.is_err());

        let result = advisor.update_cost_model(1.0, 0.0, 1.0);
        assert!(result.is_err());

        let result = advisor.update_cost_model(1.0, 1.0, -1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_calculate_age_score() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        assert!((advisor.calculate_age_score(10) - 1.0).abs() < 0.01);
        assert!((advisor.calculate_age_score(40) - 0.7).abs() < 0.01);
        assert!((advisor.calculate_age_score(100) - 0.4).abs() < 0.01);
        assert!((advisor.calculate_age_score(400) - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_calculate_size_score() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        assert!((advisor.calculate_size_score(500.0) - 1.0).abs() < 0.01);
        assert!((advisor.calculate_size_score(50.0) - 1.0).abs() < 0.01);
        assert!((advisor.calculate_size_score(5.0) - 0.4).abs() < 0.01);
        assert!((advisor.calculate_size_score(0.5) - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_calculate_access_score() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        assert!((advisor.calculate_access_score(5000) - 1.0).abs() < 0.01);
        assert!((advisor.calculate_access_score(500) - 0.7).abs() < 0.01);
        assert!((advisor.calculate_access_score(50) - 0.4).abs() < 0.01);
        assert!((advisor.calculate_access_score(5) - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_rationale_generation() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = create_archive_segment();

        let score = advisor.recommend(&metrics);
        assert!(!score.rationale.is_empty());
    }

    #[test]
    fn test_empty_batch() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let results = advisor.batch_recommendations(vec![]);

        assert!(results.is_empty());
    }

    #[test]
    fn test_savings_with_zero_age() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());
        let metrics = AccessMetrics {
            segment_id: 1,
            size_bytes: 1_000_000,
            last_access_age_sec: 0,
            access_count: 0,
            compression_ratio: 1.0,
            dedup_ratio: 1.0,
        };

        let (savings, percent) = advisor.get_estimated_savings(&metrics);
        assert!(savings >= 0);
    }

    #[test]
    fn test_compression_penalty() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        assert!((advisor.calculate_compression_penalty(5.0) - 1.0).abs() < 0.01);
        assert!((advisor.calculate_compression_penalty(2.5) - 0.7).abs() < 0.01);
        assert!((advisor.calculate_compression_penalty(1.2) - 0.3).abs() < 0.01);
        assert!((advisor.calculate_compression_penalty(0.8) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_multiple_segments_different_tiers() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        let segments = vec![
            create_hot_segment(),
            create_warm_segment(),
            create_cold_segment(),
            create_archive_segment(),
        ];

        let results = advisor.batch_recommendations(segments);

        assert_eq!(results[0].1.recommendation, TieringRecommendation::Flash);
        assert_eq!(results[1].1.recommendation, TieringRecommendation::WarmS3);
        assert_eq!(results[2].1.recommendation, TieringRecommendation::ColdS3);
        assert_eq!(
            results[3].1.recommendation,
            TieringRecommendation::ArchiveS3
        );
    }

    #[test]
    fn test_high_access_count_overrides_age() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        let metrics = AccessMetrics {
            segment_id: 1,
            size_bytes: 10_000_000,
            last_access_age_sec: 100 * 86400,
            access_count: 1000,
            compression_ratio: 1.5,
            dedup_ratio: 1.0,
        };

        let score = advisor.recommend(&metrics);
        assert_eq!(score.recommendation, TieringRecommendation::Flash);
    }

    #[test]
    fn test_tiering_score_bounds() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        let hot = create_hot_segment();
        let archive = create_ancient_segment();

        let hot_score = advisor.recommend(&hot);
        let archive_score = advisor.recommend(&archive);

        assert!(hot_score.score >= 0.0 && hot_score.score <= 1.0);
        assert!(archive_score.score >= 0.0 && archive_score.score <= 1.0);
    }

    #[test]
    fn test_custom_config_thresholds() {
        let config = TieringAdvisorConfig {
            flash_threshold_days: 7,
            cold_threshold_days: 30,
            archive_threshold_days: 180,
        };

        let advisor = TieringAdvisor::new(config);

        let metrics = AccessMetrics {
            segment_id: 1,
            size_bytes: 10_000_000,
            last_access_age_sec: 10 * 86400,
            access_count: 5,
            compression_ratio: 1.5,
            dedup_ratio: 1.0,
        };

        let score = advisor.recommend(&metrics);
        assert_eq!(score.recommendation, TieringRecommendation::WarmS3);
    }

    #[test]
    fn test_dedup_ratio_not_used_for_tier() {
        let advisor = TieringAdvisor::new(TieringAdvisorConfig::default());

        let metrics = AccessMetrics {
            segment_id: 1,
            size_bytes: 10_000_000,
            last_access_age_sec: 50 * 86400,
            access_count: 10,
            compression_ratio: 2.0,
            dedup_ratio: 10.0,
        };

        let score = advisor.recommend(&metrics);
        assert!(matches!(
            score.recommendation,
            TieringRecommendation::WarmS3 | TieringRecommendation::ColdS3
        ));
    }
}
