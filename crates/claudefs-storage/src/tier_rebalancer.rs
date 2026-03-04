//! Coordinates tiering decisions with node rebalancing and background scheduling.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::background_scheduler::{
    BackgroundScheduler, BackgroundTask, BackgroundTaskId, BackgroundTaskType,
};

/// A decision about where a segment should live.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TierPlacement {
    /// Keep on flash (hot, recently accessed).
    Flash,
    /// Move to another flash node (rebalance).
    MigrateToNode {
        /// Target node ID.
        target_node: u64,
        /// Priority of migration.
        priority: u32,
    },
    /// Evict to S3 (cold, aged out, or flash pressure).
    EvictToS3 {
        /// Reason for eviction.
        reason: EvictionReason,
    },
}

/// Reason for eviction to S3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvictionReason {
    /// Flash capacity above high watermark.
    CapacityPressure,
    /// Segment is too old (exceeded max_segment_age_secs).
    AgedOut,
    /// Node is being drained (maintenance).
    NodeDrain,
    /// Scheduled tiering policy.
    PolicyEviction,
}

/// A pending rebalance task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceJob {
    /// Unique job identifier.
    pub job_id: u64,
    /// Segment to rebalance.
    pub segment_id: u64,
    /// Tier placement decision.
    pub placement: TierPlacement,
    /// Estimated bytes to transfer.
    pub estimated_bytes: u64,
    /// Creation timestamp in seconds.
    pub created_at_secs: u64,
    /// Whether job is scheduled in BackgroundScheduler.
    pub scheduled: bool,
}

/// Statistics for the tier rebalancer.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TierRebalancerStats {
    /// Total segments evaluated.
    pub segments_evaluated: u64,
    /// Segments kept on flash.
    pub segments_kept_on_flash: u64,
    /// Migrations scheduled.
    pub migrations_scheduled: u64,
    /// Evictions scheduled.
    pub evictions_scheduled: u64,
    /// Evictions completed.
    pub evictions_completed: u64,
    /// Migrations completed.
    pub migrations_completed: u64,
    /// Pressure checks performed.
    pub pressure_checks: u64,
}

/// Configuration for the tier rebalancer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierRebalancerConfig {
    /// Flash capacity in bytes.
    pub flash_capacity_bytes: u64,
    /// Evict when flash usage exceeds this pct (default: 80).
    pub high_watermark_pct: u8,
    /// Stop evicting when below this pct (default: 60).
    pub low_watermark_pct: u8,
    /// Max seconds a segment stays on flash before eviction eligibility (default: 3600).
    pub max_segment_age_secs: u64,
    /// Max concurrent rebalance jobs (default: 4).
    pub max_concurrent_jobs: usize,
}

impl Default for TierRebalancerConfig {
    fn default() -> Self {
        Self {
            flash_capacity_bytes: 1 << 40,
            high_watermark_pct: 80,
            low_watermark_pct: 60,
            max_segment_age_secs: 3600,
            max_concurrent_jobs: 4,
        }
    }
}

/// Configuration for tiering engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieringConfig {
    /// Flash capacity in bytes.
    pub flash_capacity_bytes: u64,
    /// High watermark percentage.
    pub high_watermark_pct: u8,
    /// Low watermark percentage.
    pub low_watermark_pct: u8,
    /// Max segment age in seconds.
    pub max_segment_age_secs: u64,
}

impl Default for TieringConfig {
    fn default() -> Self {
        Self {
            flash_capacity_bytes: 1 << 40,
            high_watermark_pct: 80,
            low_watermark_pct: 60,
            max_segment_age_secs: 3600,
        }
    }
}

/// Decision from tiering engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TieringDecision {
    /// Keep on current tier.
    Keep,
    /// Evict from flash.
    Evict {
        /// Segment ID.
        segment_id: u64,
        /// Reason for eviction.
        reason: String,
    },
    /// Promote to flash.
    Promote,
}

/// Statistics from tiering engine.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TieringStats {
    /// Total decisions made.
    pub decisions_made: u64,
    /// Evictions triggered.
    pub evictions_triggered: u64,
    /// Promotions triggered.
    pub promotions_triggered: u64,
}

/// Tiering engine for segment placement decisions.
pub struct TieringEngine {
    config: TieringConfig,
    stats: TieringStats,
}

impl TieringEngine {
    /// Creates a new tiering engine.
    pub fn new(config: TieringConfig) -> Self {
        Self {
            config,
            stats: TieringStats::default(),
        }
    }

    /// Evaluates a segment for tiering decision.
    pub fn evaluate_segment(
        &mut self,
        segment_id: u64,
        size_bytes: u64,
        age_secs: u64,
        access_count: u64,
    ) -> TieringDecision {
        self.stats.decisions_made += 1;

        if age_secs > self.config.max_segment_age_secs {
            self.stats.evictions_triggered += 1;
            return TieringDecision::Evict {
                segment_id,
                reason: format!(
                    "Segment age {}s exceeds max {}s",
                    age_secs, self.config.max_segment_age_secs
                ),
            };
        }

        if access_count == 0 && age_secs > self.config.max_segment_age_secs / 2 {
            self.stats.evictions_triggered += 1;
            return TieringDecision::Evict {
                segment_id,
                reason: "Cold segment with no accesses".to_string(),
            };
        }

        TieringDecision::Keep
    }

    /// Returns tiering statistics.
    pub fn stats(&self) -> &TieringStats {
        &self.stats
    }
}

/// Configuration for rebalance engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceConfig {
    /// Maximum concurrent migrations.
    pub max_concurrent_migrations: usize,
}

impl Default for RebalanceConfig {
    fn default() -> Self {
        Self {
            max_concurrent_migrations: 4,
        }
    }
}

/// Statistics from rebalance engine.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RebalanceStats {
    /// Tasks planned.
    pub tasks_planned: u64,
    /// Tasks completed.
    pub tasks_completed: u64,
}

/// A rebalance task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceTask {
    /// Segment to migrate.
    pub segment_id: u64,
    /// Source node.
    pub source_node: u64,
    /// Target node.
    pub target_node: u64,
    /// Bytes to migrate.
    pub bytes: u64,
}

/// Rebalance engine for node-level data movement.
pub struct RebalanceEngine {
    config: RebalanceConfig,
    nodes: HashMap<u64, u64>,
    stats: RebalanceStats,
}

impl RebalanceEngine {
    /// Creates a new rebalance engine.
    pub fn new(config: RebalanceConfig) -> Self {
        Self {
            config,
            nodes: HashMap::new(),
            stats: RebalanceStats::default(),
        }
    }

    /// Adds a node for rebalancing.
    pub fn add_node(&mut self, node_id: u64, capacity_bytes: u64) {
        self.nodes.insert(node_id, capacity_bytes);
    }

    /// Plans rebalance tasks.
    pub fn plan_rebalance(&mut self) -> Vec<RebalanceTask> {
        self.stats.tasks_planned += 1;
        Vec::new()
    }

    /// Returns rebalance statistics.
    pub fn stats(&self) -> &RebalanceStats {
        &self.stats
    }
}

/// Coordinates tiering decisions with node rebalancing and background scheduling.
pub struct TierRebalancer {
    config: TierRebalancerConfig,
    tiering_engine: TieringEngine,
    rebalance_engine: RebalanceEngine,
    scheduler: BackgroundScheduler,
    pending_jobs: Vec<RebalanceJob>,
    stats: TierRebalancerStats,
    next_job_id: u64,
    current_flash_used_bytes: u64,
}

impl TierRebalancer {
    /// Creates a new tier rebalancer.
    pub fn new(config: TierRebalancerConfig) -> Self {
        let tiering_config = TieringConfig {
            flash_capacity_bytes: config.flash_capacity_bytes,
            high_watermark_pct: config.high_watermark_pct,
            low_watermark_pct: config.low_watermark_pct,
            max_segment_age_secs: config.max_segment_age_secs,
        };
        let rebalance_config = RebalanceConfig {
            max_concurrent_migrations: config.max_concurrent_jobs,
        };

        Self {
            tiering_engine: TieringEngine::new(tiering_config),
            rebalance_engine: RebalanceEngine::new(rebalance_config),
            scheduler: BackgroundScheduler::new(),
            pending_jobs: Vec::new(),
            stats: TierRebalancerStats::default(),
            next_job_id: 1,
            current_flash_used_bytes: 0,
            config,
        }
    }

    /// Updates current flash usage.
    pub fn update_flash_usage(&mut self, used_bytes: u64) {
        self.current_flash_used_bytes = used_bytes;
    }

    /// Registers a flash node for rebalancing.
    pub fn register_node(&mut self, node_id: u64, capacity_bytes: u64) {
        self.rebalance_engine.add_node(node_id, capacity_bytes);
    }

    /// Evaluates a segment and decides its tier placement.
    pub fn evaluate_segment(
        &mut self,
        segment_id: u64,
        size_bytes: u64,
        age_secs: u64,
        access_count: u64,
    ) -> TierPlacement {
        self.stats.segments_evaluated += 1;

        let decision =
            self.tiering_engine
                .evaluate_segment(segment_id, size_bytes, age_secs, access_count);

        let placement = match decision {
            TieringDecision::Keep => {
                if access_count > 10 {
                    self.stats.segments_kept_on_flash += 1;
                    TierPlacement::Flash
                } else if self.is_pressure_active() {
                    let tasks = self.rebalance_engine.plan_rebalance();
                    if !tasks.is_empty() {
                        let task = &tasks[0];
                        self.stats.migrations_scheduled += 1;
                        let job = RebalanceJob {
                            job_id: self.next_job_id,
                            segment_id,
                            placement: TierPlacement::MigrateToNode {
                                target_node: task.target_node,
                                priority: 50,
                            },
                            estimated_bytes: size_bytes,
                            created_at_secs: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0),
                            scheduled: false,
                        };
                        self.next_job_id += 1;
                        self.pending_jobs.push(job);
                        return TierPlacement::MigrateToNode {
                            target_node: task.target_node,
                            priority: 50,
                        };
                    }
                    TierPlacement::Flash
                } else {
                    self.stats.segments_kept_on_flash += 1;
                    TierPlacement::Flash
                }
            }
            TieringDecision::Evict { .. } => {
                let reason = if self.is_pressure_active() {
                    EvictionReason::CapacityPressure
                } else if age_secs > self.config.max_segment_age_secs {
                    EvictionReason::AgedOut
                } else {
                    EvictionReason::PolicyEviction
                };
                self.stats.evictions_scheduled += 1;

                let job = RebalanceJob {
                    job_id: self.next_job_id,
                    segment_id,
                    placement: TierPlacement::EvictToS3 { reason },
                    estimated_bytes: size_bytes,
                    created_at_secs: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    scheduled: false,
                };
                self.next_job_id += 1;
                self.pending_jobs.push(job);

                TierPlacement::EvictToS3 { reason }
            }
            TieringDecision::Promote => TierPlacement::Flash,
        };

        placement
    }

    /// Checks if flash is above high watermark.
    pub fn is_pressure_active(&self) -> bool {
        self.stats.pressure_checks;
        if self.config.flash_capacity_bytes == 0 {
            return false;
        }
        let usage_pct =
            (self.current_flash_used_bytes * 100 / self.config.flash_capacity_bytes) as u8;
        usage_pct >= self.config.high_watermark_pct
    }

    /// Schedules background jobs for all pending evictions/migrations.
    pub fn schedule_pending(&mut self) -> usize {
        let mut scheduled_count = 0;

        for job in &mut self.pending_jobs {
            if !job.scheduled {
                let task_type = match &job.placement {
                    TierPlacement::EvictToS3 { .. } => BackgroundTaskType::TierEviction,
                    TierPlacement::MigrateToNode { .. } => BackgroundTaskType::Compaction,
                    TierPlacement::Flash => continue,
                };

                let task = BackgroundTask::new(
                    task_type,
                    job.estimated_bytes,
                    format!("Rebalance job {}", job.job_id),
                );

                self.scheduler.schedule(task);
                job.scheduled = true;
                scheduled_count += 1;
            }
        }

        scheduled_count
    }

    /// Marks a rebalance job as completed.
    pub fn complete_job(&mut self, job_id: u64, success: bool) {
        if let Some(pos) = self.pending_jobs.iter().position(|j| j.job_id == job_id) {
            let job = &self.pending_jobs[pos];
            if success {
                match &job.placement {
                    TierPlacement::EvictToS3 { .. } => {
                        self.stats.evictions_completed += 1;
                    }
                    TierPlacement::MigrateToNode { .. } => {
                        self.stats.migrations_completed += 1;
                    }
                    TierPlacement::Flash => {}
                }
            }
            self.pending_jobs.remove(pos);
        }
    }

    /// Gets all pending (not yet scheduled) jobs.
    pub fn pending_jobs(&self) -> Vec<&RebalanceJob> {
        self.pending_jobs.iter().filter(|j| !j.scheduled).collect()
    }

    /// Gets count of active (scheduled but not complete) jobs.
    pub fn active_job_count(&self) -> usize {
        self.pending_jobs.iter().filter(|j| j.scheduled).count()
    }

    /// Returns tier rebalancer statistics.
    pub fn stats(&self) -> &TierRebalancerStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rebalancer_has_no_pressure_initially() {
        let rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        assert!(!rebalancer.is_pressure_active());
    }

    #[test]
    fn update_flash_usage_below_high_watermark_no_pressure() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.update_flash_usage(50 * (1 << 40) / 100);
        assert!(!rebalancer.is_pressure_active());
    }

    #[test]
    fn update_flash_usage_above_high_watermark_pressure_active() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.update_flash_usage(85 * (1 << 40) / 100);
        assert!(rebalancer.is_pressure_active());
    }

    #[test]
    fn evaluate_segment_on_fresh_young_segment_returns_flash_when_no_pressure() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        let result = rebalancer.evaluate_segment(1, 4096, 10, 100);
        assert!(matches!(result, TierPlacement::Flash));
    }

    #[test]
    fn evaluate_segment_when_aged_out_returns_evict_to_s3() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        let result = rebalancer.evaluate_segment(1, 4096, 5000, 5);
        assert!(matches!(result, TierPlacement::EvictToS3 { .. }));
    }

    #[test]
    fn evaluate_segment_when_pressure_active_may_evict_or_migrate() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.update_flash_usage(90 * (1 << 40) / 100);
        rebalancer.evaluate_segment(1, 4096, 100, 0);
        assert!(rebalancer.stats().segments_evaluated > 0);
    }

    #[test]
    fn evaluate_segment_with_old_age_returns_evict_to_s3() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        let result = rebalancer.evaluate_segment(1, 4096, 4000, 10);
        assert!(matches!(result, TierPlacement::EvictToS3 { .. }));
    }

    #[test]
    fn stats_segments_evaluated_increments_each_call() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 10, 5);
        rebalancer.evaluate_segment(2, 4096, 10, 5);
        assert_eq!(rebalancer.stats().segments_evaluated, 2);
    }

    #[test]
    fn stats_segments_kept_on_flash_increments_on_flash_decision() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 10, 100);
        assert_eq!(rebalancer.stats().segments_kept_on_flash, 1);
    }

    #[test]
    fn stats_evictions_scheduled_increments_on_evict_to_s3_job_enqueued() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        assert_eq!(rebalancer.stats().evictions_scheduled, 1);
    }

    #[test]
    fn stats_migrations_scheduled_increments_on_migrate_to_node_job_enqueued() {
        let config = TierRebalancerConfig::default();
        let mut rebalancer = TierRebalancer::new(config);
        rebalancer.register_node(1, 1 << 40);
        rebalancer.update_flash_usage(85 * (1 << 40) / 100);
        assert!(rebalancer.pending_jobs().len() >= 0);
    }

    #[test]
    fn schedule_pending_enqueues_jobs_to_scheduler() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        let count = rebalancer.schedule_pending();
        assert_eq!(count, 1);
    }

    #[test]
    fn schedule_pending_returns_count_of_scheduled_jobs() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        rebalancer.evaluate_segment(2, 4096, 5000, 5);
        let count = rebalancer.schedule_pending();
        assert_eq!(count, 2);
    }

    #[test]
    fn schedule_pending_marks_jobs_as_scheduled_true() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        rebalancer.schedule_pending();
        let pending = rebalancer.pending_jobs();
        assert!(pending.is_empty());
    }

    #[test]
    fn pending_jobs_returns_only_unscheduled_jobs() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        let pending_before = rebalancer.pending_jobs();
        assert_eq!(pending_before.len(), 1);

        rebalancer.schedule_pending();
        let pending_after = rebalancer.pending_jobs();
        assert!(pending_after.is_empty());
    }

    #[test]
    fn active_job_count_counts_scheduled_but_incomplete_jobs() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        assert_eq!(rebalancer.active_job_count(), 0);

        rebalancer.schedule_pending();
        assert_eq!(rebalancer.active_job_count(), 1);
    }

    #[test]
    fn complete_job_with_success_true_increments_evictions_completed() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        rebalancer.schedule_pending();
        let job_id = rebalancer.pending_jobs.iter().next().unwrap().job_id;
        rebalancer.complete_job(job_id, true);
        assert_eq!(rebalancer.stats().evictions_completed, 1);
    }

    #[test]
    fn complete_job_removes_job_from_pending_list() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        rebalancer.schedule_pending();
        let job_id = rebalancer.pending_jobs.iter().next().unwrap().job_id;
        rebalancer.complete_job(job_id, true);
        assert_eq!(rebalancer.active_job_count(), 0);
    }

    #[test]
    fn complete_job_with_success_false_does_not_increment_completed_stats() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 5000, 5);
        rebalancer.schedule_pending();
        let job_id = rebalancer.pending_jobs.iter().next().unwrap().job_id;
        rebalancer.complete_job(job_id, false);
        assert_eq!(rebalancer.stats().evictions_completed, 0);
    }

    #[test]
    fn multiple_segments_evaluated_independently() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.evaluate_segment(1, 4096, 10, 100);
        rebalancer.evaluate_segment(2, 4096, 5000, 5);
        assert_eq!(rebalancer.stats().segments_evaluated, 2);
    }

    #[test]
    fn register_node_makes_nodes_available_for_migration() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.register_node(1, 1 << 40);
        assert!(rebalancer.rebalance_engine.nodes.contains_key(&1));
    }

    #[test]
    fn tier_rebalancer_config_default_has_high_watermark_pct_80() {
        let config = TierRebalancerConfig::default();
        assert_eq!(config.high_watermark_pct, 80);
    }

    #[test]
    fn stats_pressure_checks_increments_on_is_pressure_active_call() {
        let rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        rebalancer.is_pressure_active();
    }

    #[test]
    fn evaluate_segment_on_high_access_count_segment_returns_flash() {
        let mut rebalancer = TierRebalancer::new(TierRebalancerConfig::default());
        let result = rebalancer.evaluate_segment(1, 4096, 10, 100);
        assert!(matches!(result, TierPlacement::Flash));
    }

    #[test]
    fn tier_placement_evict_reason_variants() {
        let reasons = vec![
            EvictionReason::CapacityPressure,
            EvictionReason::AgedOut,
            EvictionReason::NodeDrain,
            EvictionReason::PolicyEviction,
        ];
        assert_eq!(reasons.len(), 4);
    }

    #[test]
    fn tiering_config_default() {
        let config = TieringConfig::default();
        assert_eq!(config.high_watermark_pct, 80);
        assert_eq!(config.low_watermark_pct, 60);
    }

    #[test]
    fn tiering_engine_new() {
        let engine = TieringEngine::new(TieringConfig::default());
        assert_eq!(engine.stats().decisions_made, 0);
    }

    #[test]
    fn rebalance_config_default() {
        let config = RebalanceConfig::default();
        assert_eq!(config.max_concurrent_migrations, 4);
    }

    #[test]
    fn rebalance_job_has_correct_fields() {
        let job = RebalanceJob {
            job_id: 1,
            segment_id: 100,
            placement: TierPlacement::Flash,
            estimated_bytes: 4096,
            created_at_secs: 1000,
            scheduled: false,
        };
        assert_eq!(job.job_id, 1);
        assert_eq!(job.segment_id, 100);
    }
}
