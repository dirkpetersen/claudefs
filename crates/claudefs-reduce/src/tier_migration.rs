//! Tier migration policies for flash-to-S3 data movement.
//!
//! Implements eviction of cold data from flash to S3, and promotion
//! of hot data from S3 back to flash for performance.

use serde::{Deserialize, Serialize};

/// Direction of migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationDirection {
    /// Evict from flash to S3.
    FlashToS3,
    /// Promote from S3 to flash.
    S3ToFlash,
}

/// A candidate segment for migration.
#[derive(Debug, Clone)]
pub struct MigrationCandidate {
    /// Segment ID to migrate.
    pub segment_id: u64,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Last access timestamp in milliseconds.
    pub last_access_ms: u64,
    /// Number of accesses since last evaluation.
    pub access_count: u64,
    /// Direction of migration.
    pub direction: MigrationDirection,
    /// Score for prioritization (higher = more important).
    pub score: f64,
}

/// Configuration for migration policies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationConfig {
    /// Age threshold for eviction (ms). Default: 7 days.
    pub eviction_age_ms: u64,
    /// Minimum access count for promotion. Default: 3.
    pub promotion_access_count: u32,
    /// Maximum candidates per batch. Default: 16.
    pub batch_size: usize,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            eviction_age_ms: 7 * 24 * 3600 * 1000, // 7 days in ms
            promotion_access_count: 3,
            batch_size: 16,
        }
    }
}

/// Statistics for migrations performed.
#[derive(Debug, Clone, Default)]
pub struct MigrationStats {
    /// Count of flash-to-S3 migrations.
    pub flash_to_s3_count: u64,
    /// Bytes migrated flash-to-S3.
    pub flash_to_s3_bytes: u64,
    /// Count of S3-to-flash migrations.
    pub s3_to_flash_count: u64,
    /// Bytes migrated S3-to-flash.
    pub s3_to_flash_bytes: u64,
}

/// Tier migration evaluator and coordinator.
pub struct TierMigrator {
    config: MigrationConfig,
    stats: MigrationStats,
}

impl Default for TierMigrator {
    fn default() -> Self {
        Self::new(MigrationConfig::default())
    }
}

impl TierMigrator {
    /// Creates a new migrator with the given configuration.
    pub fn new(config: MigrationConfig) -> Self {
        Self {
            config,
            stats: MigrationStats::default(),
        }
    }

    /// Evaluate a segment for eviction (flash to S3).
    /// Returns a candidate if the segment age exceeds the eviction threshold.
    /// Score is based on age and size: older and larger segments score higher.
    pub fn evaluate_eviction(
        &self,
        segment_id: u64,
        size_bytes: u64,
        last_access_ms: u64,
        now_ms: u64,
    ) -> Option<MigrationCandidate> {
        let age_ms = now_ms.saturating_sub(last_access_ms);

        if age_ms <= self.config.eviction_age_ms {
            return None;
        }

        let age_secs = age_ms / 1000;
        let size_mb = size_bytes as f64 / (1024.0 * 1024.0);
        let score = (age_secs as f64 / 1024.0) * size_mb;

        Some(MigrationCandidate {
            segment_id,
            size_bytes,
            last_access_ms,
            access_count: 0,
            direction: MigrationDirection::FlashToS3,
            score,
        })
    }

    /// Evaluate a segment for promotion (S3 to flash).
    /// Returns a candidate if access count meets threshold.
    pub fn evaluate_promotion(
        &self,
        segment_id: u64,
        size_bytes: u64,
        access_count: u64,
    ) -> Option<MigrationCandidate> {
        if access_count < self.config.promotion_access_count as u64 {
            return None;
        }

        Some(MigrationCandidate {
            segment_id,
            size_bytes,
            last_access_ms: 0,
            access_count,
            direction: MigrationDirection::S3ToFlash,
            score: access_count as f64,
        })
    }

    /// Select the top candidates for a migration batch.
    /// Returns up to batch_size candidates sorted by score descending.
    pub fn select_batch(&self, candidates: &[MigrationCandidate]) -> Vec<MigrationCandidate> {
        let mut sorted: Vec<_> = candidates.to_vec();
        sorted.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted.into_iter().take(self.config.batch_size).collect()
    }

    /// Record a completed migration, updating stats.
    pub fn record_migration(&mut self, candidate: &MigrationCandidate) {
        match candidate.direction {
            MigrationDirection::FlashToS3 => {
                self.stats.flash_to_s3_count += 1;
                self.stats.flash_to_s3_bytes += candidate.size_bytes;
            }
            MigrationDirection::S3ToFlash => {
                self.stats.s3_to_flash_count += 1;
                self.stats.s3_to_flash_bytes += candidate.size_bytes;
            }
        }
    }

    /// Returns current migration statistics.
    pub fn stats(&self) -> &MigrationStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_config_default() {
        let config = MigrationConfig::default();
        assert_eq!(config.eviction_age_ms, 7 * 24 * 3600 * 1000);
        assert_eq!(config.promotion_access_count, 3);
        assert_eq!(config.batch_size, 16);
    }

    #[test]
    fn migration_stats_default() {
        let stats = MigrationStats::default();
        assert_eq!(stats.flash_to_s3_count, 0);
        assert_eq!(stats.flash_to_s3_bytes, 0);
        assert_eq!(stats.s3_to_flash_count, 0);
        assert_eq!(stats.s3_to_flash_bytes, 0);
    }

    #[test]
    fn evaluate_eviction_too_young() {
        let config = MigrationConfig {
            eviction_age_ms: 1000,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let result = migrator.evaluate_eviction(1, 1024, 500, 1000);
        assert!(result.is_none());
    }

    #[test]
    fn evaluate_eviction_old_enough() {
        let config = MigrationConfig {
            eviction_age_ms: 1000,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let result = migrator.evaluate_eviction(1, 1024, 0, 2000);
        assert!(result.is_some());

        let candidate = result.unwrap();
        assert_eq!(candidate.segment_id, 1);
        assert_eq!(candidate.direction, MigrationDirection::FlashToS3);
    }

    #[test]
    fn evaluate_eviction_score_increases_with_age() {
        let config = MigrationConfig {
            eviction_age_ms: 1000,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let c1 = migrator.evaluate_eviction(1, 1024 * 1024, 0, 2000).unwrap();
        let c2 = migrator.evaluate_eviction(2, 1024 * 1024, 0, 5000).unwrap();

        assert!(c2.score > c1.score);
    }

    #[test]
    fn evaluate_promotion_too_few_accesses() {
        let config = MigrationConfig {
            promotion_access_count: 3,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let result = migrator.evaluate_promotion(1, 1024, 2);
        assert!(result.is_none());
    }

    #[test]
    fn evaluate_promotion_enough_accesses() {
        let config = MigrationConfig {
            promotion_access_count: 3,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let result = migrator.evaluate_promotion(1, 1024, 5);
        assert!(result.is_some());

        let candidate = result.unwrap();
        assert_eq!(candidate.segment_id, 1);
        assert_eq!(candidate.direction, MigrationDirection::S3ToFlash);
        assert_eq!(candidate.score, 5.0);
    }

    #[test]
    fn select_batch_respects_batch_size() {
        let config = MigrationConfig {
            batch_size: 2,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let candidates = vec![
            MigrationCandidate {
                segment_id: 1,
                size_bytes: 100,
                last_access_ms: 0,
                access_count: 0,
                direction: MigrationDirection::FlashToS3,
                score: 1.0,
            },
            MigrationCandidate {
                segment_id: 2,
                size_bytes: 100,
                last_access_ms: 0,
                access_count: 0,
                direction: MigrationDirection::FlashToS3,
                score: 2.0,
            },
            MigrationCandidate {
                segment_id: 3,
                size_bytes: 100,
                last_access_ms: 0,
                access_count: 0,
                direction: MigrationDirection::FlashToS3,
                score: 3.0,
            },
        ];

        let batch = migrator.select_batch(&candidates);
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn select_batch_sorted_by_score() {
        let config = MigrationConfig {
            batch_size: 10,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let candidates = vec![
            MigrationCandidate {
                segment_id: 1,
                size_bytes: 100,
                last_access_ms: 0,
                access_count: 0,
                direction: MigrationDirection::FlashToS3,
                score: 1.0,
            },
            MigrationCandidate {
                segment_id: 2,
                size_bytes: 100,
                last_access_ms: 0,
                access_count: 0,
                direction: MigrationDirection::FlashToS3,
                score: 5.0,
            },
            MigrationCandidate {
                segment_id: 3,
                size_bytes: 100,
                last_access_ms: 0,
                access_count: 0,
                direction: MigrationDirection::FlashToS3,
                score: 3.0,
            },
        ];

        let batch = migrator.select_batch(&candidates);

        assert_eq!(batch[0].segment_id, 2);
        assert_eq!(batch[1].segment_id, 3);
        assert_eq!(batch[2].segment_id, 1);
    }

    #[test]
    fn select_batch_empty_returns_empty() {
        let migrator = TierMigrator::new(MigrationConfig::default());
        let batch = migrator.select_batch(&[]);
        assert!(batch.is_empty());
    }

    #[test]
    fn select_batch_fewer_than_batch_size() {
        let config = MigrationConfig {
            batch_size: 10,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let candidates = vec![MigrationCandidate {
            segment_id: 1,
            size_bytes: 100,
            last_access_ms: 0,
            access_count: 0,
            direction: MigrationDirection::FlashToS3,
            score: 1.0,
        }];

        let batch = migrator.select_batch(&candidates);
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn record_migration_flash_to_s3_updates_stats() {
        let mut migrator = TierMigrator::new(MigrationConfig::default());

        let candidate = MigrationCandidate {
            segment_id: 1,
            size_bytes: 1024,
            last_access_ms: 0,
            access_count: 0,
            direction: MigrationDirection::FlashToS3,
            score: 1.0,
        };

        migrator.record_migration(&candidate);

        let stats = migrator.stats();
        assert_eq!(stats.flash_to_s3_count, 1);
        assert_eq!(stats.flash_to_s3_bytes, 1024);
        assert_eq!(stats.s3_to_flash_count, 0);
    }

    #[test]
    fn record_migration_s3_to_flash_updates_stats() {
        let mut migrator = TierMigrator::new(MigrationConfig::default());

        let candidate = MigrationCandidate {
            segment_id: 1,
            size_bytes: 2048,
            last_access_ms: 0,
            access_count: 5,
            direction: MigrationDirection::S3ToFlash,
            score: 5.0,
        };

        migrator.record_migration(&candidate);

        let stats = migrator.stats();
        assert_eq!(stats.s3_to_flash_count, 1);
        assert_eq!(stats.s3_to_flash_bytes, 2048);
        assert_eq!(stats.flash_to_s3_count, 0);
    }

    #[test]
    fn migration_direction_equality() {
        assert_eq!(MigrationDirection::FlashToS3, MigrationDirection::FlashToS3);
        assert_eq!(MigrationDirection::S3ToFlash, MigrationDirection::S3ToFlash);
        assert_ne!(MigrationDirection::FlashToS3, MigrationDirection::S3ToFlash);
    }

    #[test]
    fn evaluate_eviction_at_exactly_threshold() {
        let config = MigrationConfig {
            eviction_age_ms: 1000,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        // Age exactly at threshold should NOT be evicted
        let result = migrator.evaluate_eviction(1, 1024, 0, 1000);
        assert!(result.is_none());

        // Age just over threshold should be evicted
        let result = migrator.evaluate_eviction(1, 1024, 0, 1001);
        assert!(result.is_some());
    }

    #[test]
    fn migration_candidate_clone() {
        let candidate = MigrationCandidate {
            segment_id: 1,
            size_bytes: 1024,
            last_access_ms: 500,
            access_count: 3,
            direction: MigrationDirection::FlashToS3,
            score: 2.5,
        };

        let cloned = candidate.clone();
        assert_eq!(cloned.segment_id, 1);
        assert_eq!(cloned.size_bytes, 1024);
        assert_eq!(cloned.score, 2.5);
    }

    #[test]
    fn evaluate_promotion_score_is_access_count() {
        let config = MigrationConfig {
            promotion_access_count: 3,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        let c1 = migrator.evaluate_promotion(1, 1024, 5).unwrap();
        let c2 = migrator.evaluate_promotion(2, 1024, 10).unwrap();

        assert_eq!(c1.score, 5.0);
        assert_eq!(c2.score, 10.0);
        assert!(c2.score > c1.score);
    }

    #[test]
    fn multiple_migrations_accumulate_stats() {
        let mut migrator = TierMigrator::new(MigrationConfig::default());

        migrator.record_migration(&MigrationCandidate {
            segment_id: 1,
            size_bytes: 1000,
            last_access_ms: 0,
            access_count: 0,
            direction: MigrationDirection::FlashToS3,
            score: 1.0,
        });

        migrator.record_migration(&MigrationCandidate {
            segment_id: 2,
            size_bytes: 2000,
            last_access_ms: 0,
            access_count: 0,
            direction: MigrationDirection::FlashToS3,
            score: 2.0,
        });

        migrator.record_migration(&MigrationCandidate {
            segment_id: 3,
            size_bytes: 500,
            last_access_ms: 0,
            access_count: 5,
            direction: MigrationDirection::S3ToFlash,
            score: 5.0,
        });

        let stats = migrator.stats();
        assert_eq!(stats.flash_to_s3_count, 2);
        assert_eq!(stats.flash_to_s3_bytes, 3000);
        assert_eq!(stats.s3_to_flash_count, 1);
        assert_eq!(stats.s3_to_flash_bytes, 500);
    }

    #[test]
    fn eviction_score_with_size() {
        let config = MigrationConfig {
            eviction_age_ms: 1000,
            ..Default::default()
        };
        let migrator = TierMigrator::new(config);

        // Same age, different sizes
        let c_small = migrator.evaluate_eviction(1, 1024 * 1024, 0, 2000).unwrap();
        let c_large = migrator
            .evaluate_eviction(2, 10 * 1024 * 1024, 0, 2000)
            .unwrap();

        // Larger size should have higher score
        assert!(c_large.score > c_small.score);
    }

    #[test]
    fn migration_config_clone() {
        let config = MigrationConfig {
            eviction_age_ms: 1000,
            promotion_access_count: 5,
            batch_size: 32,
        };
        let cloned = config.clone();
        assert_eq!(cloned.eviction_age_ms, 1000);
        assert_eq!(cloned.promotion_access_count, 5);
        assert_eq!(cloned.batch_size, 32);
    }

    #[test]
    fn tier_migrator_default() {
        let migrator = TierMigrator::default();
        let stats = migrator.stats();
        assert_eq!(stats.flash_to_s3_count, 0);
        assert_eq!(stats.s3_to_flash_count, 0);
    }
}
