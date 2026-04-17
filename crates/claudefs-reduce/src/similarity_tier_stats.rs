//! Detailed Tier 2 similarity pipeline monitoring and effectiveness metrics.
//!
//! Tracks feature extraction latency, delta compression ratios, similarity hit rates,
//! and effectiveness metrics per workload for Tier 2 tuning and optimization.

use crate::error::ReduceError;
use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::Arc;

/// Per-workload Tier 2 performance metrics.
#[derive(Debug, Clone)]
pub struct TierStats {
    /// Workload identifier.
    pub workload: String,
    /// Number of feature extraction samples recorded.
    pub feature_extraction_samples: u64,
    /// Feature extraction latencies in microseconds (sample for percentile calc).
    pub feature_extraction_latency_us: Vec<u64>,
    /// Total similarity lookups performed.
    pub similarity_lookups: u64,
    /// Successful similarity hits.
    pub similarity_hits: u64,
    /// Delta compressions scheduled.
    pub delta_compressions_scheduled: u64,
    /// Delta compressions completed.
    pub delta_compressions_completed: u64,
    /// Total input bytes before delta compression.
    pub delta_bytes_before: u64,
    /// Total output bytes after delta compression.
    pub delta_bytes_after: u64,
    /// Delta compression latencies in microseconds (samples for percentile).
    pub delta_compression_latency_us: Vec<u64>,
    /// When these stats were last updated.
    pub timestamp_secs: u64,
}

impl Default for TierStats {
    fn default() -> Self {
        Self {
            workload: String::new(),
            feature_extraction_samples: 0,
            feature_extraction_latency_us: Vec::new(),
            similarity_lookups: 0,
            similarity_hits: 0,
            delta_compressions_scheduled: 0,
            delta_compressions_completed: 0,
            delta_bytes_before: 0,
            delta_bytes_after: 0,
            delta_compression_latency_us: Vec::new(),
            timestamp_secs: 0,
        }
    }
}

/// Effectiveness metrics for Tier 2 tuning and optimization.
#[derive(Debug, Clone)]
pub struct EffectivenessMetrics {
    /// Workload identifier.
    pub workload: String,
    /// Feature extraction throughput in GB/s.
    pub feature_extraction_throughput_gb_per_sec: f64,
    /// Similarity hit rate (0.0 to 1.0).
    pub similarity_hit_rate: f64,
    /// Delta compression ratio (output/input).
    pub delta_compression_ratio: f64,
    /// Total bytes saved by delta compression.
    pub total_bytes_saved: u64,
    /// Estimated CPU cost as percentage (0.0 to 100.0).
    pub cpu_cost_percent: f64,
    /// Overall effectiveness score (0.0 to 1.0, composite metric).
    pub overall_effectiveness_score: f64,
}

impl Default for EffectivenessMetrics {
    fn default() -> Self {
        Self {
            workload: String::new(),
            feature_extraction_throughput_gb_per_sec: 0.0,
            similarity_hit_rate: 0.0,
            delta_compression_ratio: 1.0,
            total_bytes_saved: 0,
            cpu_cost_percent: 0.0,
            overall_effectiveness_score: 0.0,
        }
    }
}

/// Configuration for Tier 2 stats collection.
#[derive(Debug, Clone)]
pub struct StatsConfig {
    /// Maximum samples to keep for latency percentile calculation.
    pub sample_window_size: usize,
    /// Interval for effectiveness metric calculation in seconds.
    pub effectiveness_update_interval_secs: u64,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            sample_window_size: 1000,
            effectiveness_update_interval_secs: 60,
        }
    }
}

/// Collector for Tier 2 statistics and metrics.
pub struct SimilarityTierStats {
    /// Per-workload stats (workload name -> TierStats).
    stats_by_workload: Arc<RwLock<HashMap<String, TierStats>>>,
    /// Per-workload effectiveness metrics.
    effectiveness: Arc<RwLock<HashMap<String, EffectivenessMetrics>>>,
    /// Configuration.
    config: StatsConfig,
}

impl Default for SimilarityTierStats {
    fn default() -> Self {
        Self::new(StatsConfig::default())
    }
}

impl SimilarityTierStats {
    /// Create a new stats collector with configuration.
    pub fn new(config: StatsConfig) -> Self {
        Self {
            stats_by_workload: Arc::new(RwLock::new(HashMap::new())),
            effectiveness: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Record a feature extraction sample (latency in microseconds).
    pub fn record_feature_extraction(
        &self,
        workload: &str,
        latency_us: u64,
        _bytes_processed: u64,
    ) {
        let mut stats = self.stats_by_workload.write().unwrap();
        let entry = stats.entry(workload.to_string()).or_insert_with(|| {
            TierStats {
                workload: workload.to_string(),
                timestamp_secs: current_time_secs(),
                ..Default::default()
            }
        });

        entry.feature_extraction_samples += 1;
        entry.feature_extraction_latency_us.push(latency_us);

        // Keep only sample_window_size most recent samples
        if entry.feature_extraction_latency_us.len() > self.config.sample_window_size {
            entry.feature_extraction_latency_us.remove(0);
        }

        entry.timestamp_secs = current_time_secs();
    }

    /// Record a similarity lookup (hit or miss).
    pub fn record_similarity_lookup(&self, workload: &str, hit: bool) {
        let mut stats = self.stats_by_workload.write().unwrap();
        let entry = stats.entry(workload.to_string()).or_insert_with(|| {
            TierStats {
                workload: workload.to_string(),
                timestamp_secs: current_time_secs(),
                ..Default::default()
            }
        });

        entry.similarity_lookups += 1;
        if hit {
            entry.similarity_hits += 1;
        }
        entry.timestamp_secs = current_time_secs();
    }

    /// Record a delta compression operation.
    pub fn record_delta_compression(
        &self,
        workload: &str,
        before_bytes: u64,
        after_bytes: u64,
        latency_us: u64,
    ) {
        let mut stats = self.stats_by_workload.write().unwrap();
        let entry = stats.entry(workload.to_string()).or_insert_with(|| {
            TierStats {
                workload: workload.to_string(),
                timestamp_secs: current_time_secs(),
                ..Default::default()
            }
        });

        entry.delta_compressions_completed += 1;
        entry.delta_bytes_before += before_bytes;
        entry.delta_bytes_after += after_bytes;
        entry.delta_compression_latency_us.push(latency_us);

        // Keep only sample_window_size most recent samples
        if entry.delta_compression_latency_us.len() > self.config.sample_window_size {
            entry.delta_compression_latency_us.remove(0);
        }

        entry.timestamp_secs = current_time_secs();
    }

    /// Calculate effectiveness metrics for a workload.
    pub fn calculate_effectiveness(
        &self,
        workload: &str,
    ) -> Result<EffectivenessMetrics, ReduceError> {
        let stats = self
            .stats_by_workload
            .read()
            .unwrap()
            .get(workload)
            .cloned()
            .ok_or_else(|| ReduceError::NotFound(
                format!("Workload {} not found", workload),
            ))?;

        // Calculate throughput
        let avg_latency_us = if !stats.feature_extraction_latency_us.is_empty() {
            stats.feature_extraction_latency_us.iter().sum::<u64>()
                / stats.feature_extraction_latency_us.len() as u64
        } else {
            0
        };
        let throughput_gb_per_sec = if avg_latency_us > 0 {
            // Assume 64KB chunks (typical), throughput = 64KB / latency
            (64.0 * 1024.0) / (avg_latency_us as f64 / 1_000_000.0) / (1024.0 * 1024.0 * 1024.0)
        } else {
            0.0
        };

        // Calculate hit rate
        let similarity_hit_rate = if stats.similarity_lookups > 0 {
            stats.similarity_hits as f64 / stats.similarity_lookups as f64
        } else {
            0.0
        };

        // Calculate delta compression ratio
        let delta_compression_ratio = if stats.delta_bytes_before > 0 {
            stats.delta_bytes_after as f64 / stats.delta_bytes_before as f64
        } else {
            1.0
        };

        // Calculate bytes saved
        let total_bytes_saved = if stats.delta_bytes_before > stats.delta_bytes_after {
            stats.delta_bytes_before - stats.delta_bytes_after
        } else {
            0
        };

        // Estimate CPU cost (placeholder: assume 5% per 1M ops/sec)
        let ops_per_sec = if stats.feature_extraction_samples > 0 {
            stats.feature_extraction_samples as f64 / 60.0 // Assume 60 second window
        } else {
            0.0
        };
        let cpu_cost_percent = (ops_per_sec / 1_000_000.0) * 5.0;

        // Calculate composite effectiveness score
        // Factors: hit rate (40%), compression ratio benefit (40%), low cpu cost (20%)
        let hit_rate_factor = similarity_hit_rate * 0.4;
        let compression_factor = (1.0 - delta_compression_ratio).max(0.0) * 0.4;
        let cpu_factor = (1.0 - (cpu_cost_percent / 100.0).min(1.0)) * 0.2;
        let overall_score = (hit_rate_factor + compression_factor + cpu_factor).max(0.0).min(1.0);

        let metrics = EffectivenessMetrics {
            workload: workload.to_string(),
            feature_extraction_throughput_gb_per_sec: throughput_gb_per_sec,
            similarity_hit_rate,
            delta_compression_ratio,
            total_bytes_saved,
            cpu_cost_percent,
            overall_effectiveness_score: overall_score,
        };

        // Cache the calculated metrics
        self.effectiveness
            .write()
            .unwrap()
            .insert(workload.to_string(), metrics.clone());

        Ok(metrics)
    }

    /// Get latency percentiles (p50, p95, p99) in microseconds.
    pub fn get_latency_percentiles(
        &self,
        workload: &str,
    ) -> Result<(u64, u64, u64), ReduceError> {
        let stats = self
            .stats_by_workload
            .read()
            .unwrap()
            .get(workload)
            .cloned()
            .ok_or_else(|| ReduceError::NotFound(
                format!("Workload {} not found", workload),
            ))?;

        if stats.feature_extraction_latency_us.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut sorted = stats.feature_extraction_latency_us.clone();
        sorted.sort_unstable();

        let len = sorted.len();
        let p50 = sorted[(len as f64 * 0.5) as usize];
        let p95 = sorted[(len as f64 * 0.95) as usize];
        let p99 = sorted[(len as f64 * 0.99) as usize];

        Ok((p50, p95, p99))
    }

    /// Get workload stats snapshot (read-only).
    pub fn get_stats(&self, workload: &str) -> Option<TierStats> {
        self.stats_by_workload
            .read()
            .unwrap()
            .get(workload)
            .cloned()
    }

    /// List all monitored workloads.
    pub fn list_workloads(&self) -> Vec<String> {
        self.stats_by_workload
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    /// Export stats to Prometheus metrics format.
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();
        output.push_str("# HELP tier2_similarity_hit_rate Tier 2 similarity hit rate (0.0 to 1.0)\n");
        output.push_str("# TYPE tier2_similarity_hit_rate gauge\n");

        let stats = self.stats_by_workload.read().unwrap();
        for (workload, tier_stats) in stats.iter() {
            let hit_rate = if tier_stats.similarity_lookups > 0 {
                tier_stats.similarity_hits as f64 / tier_stats.similarity_lookups as f64
            } else {
                0.0
            };
            output.push_str(&format!(
                "tier2_similarity_hit_rate{{workload=\"{}\"}} {}\n",
                workload, hit_rate
            ));
        }

        output.push_str("# HELP tier2_delta_compression_ratio Delta compression ratio (lower is better)\n");
        output.push_str("# TYPE tier2_delta_compression_ratio gauge\n");
        for (workload, tier_stats) in stats.iter() {
            let ratio = if tier_stats.delta_bytes_before > 0 {
                tier_stats.delta_bytes_after as f64 / tier_stats.delta_bytes_before as f64
            } else {
                1.0
            };
            output.push_str(&format!(
                "tier2_delta_compression_ratio{{workload=\"{}\"}} {}\n",
                workload, ratio
            ));
        }

        output.push_str("# HELP tier2_bytes_saved Total bytes saved by delta compression\n");
        output.push_str("# TYPE tier2_bytes_saved counter\n");
        for (workload, tier_stats) in stats.iter() {
            let saved = if tier_stats.delta_bytes_before > tier_stats.delta_bytes_after {
                tier_stats.delta_bytes_before - tier_stats.delta_bytes_after
            } else {
                0
            };
            output.push_str(&format!(
                "tier2_bytes_saved{{workload=\"{}\"}} {}\n",
                workload, saved
            ));
        }

        output
    }

    /// Reset stats for a workload.
    pub fn reset_workload(&self, workload: &str) {
        self.stats_by_workload.write().unwrap().remove(workload);
        self.effectiveness.write().unwrap().remove(workload);
    }
}

/// Get current time in seconds since epoch.
fn current_time_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_stats_collector() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        assert_eq!(collector.list_workloads().len(), 0);
    }

    #[test]
    fn test_record_feature_extraction() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_feature_extraction("workload1", 100, 64000);
        let stats = collector.get_stats("workload1").unwrap();
        assert_eq!(stats.feature_extraction_samples, 1);
        assert_eq!(stats.feature_extraction_latency_us.len(), 1);
    }

    #[test]
    fn test_record_similarity_hit() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_similarity_lookup("workload1", true);
        collector.record_similarity_lookup("workload1", false);
        let stats = collector.get_stats("workload1").unwrap();
        assert_eq!(stats.similarity_lookups, 2);
        assert_eq!(stats.similarity_hits, 1);
    }

    #[test]
    fn test_record_delta_compression() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_delta_compression("workload1", 10000, 5000, 500);
        let stats = collector.get_stats("workload1").unwrap();
        assert_eq!(stats.delta_compressions_completed, 1);
        assert_eq!(stats.delta_bytes_before, 10000);
        assert_eq!(stats.delta_bytes_after, 5000);
    }

    #[test]
    fn test_similarity_hit_rate() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_similarity_lookup("workload1", true);
        collector.record_similarity_lookup("workload1", true);
        collector.record_similarity_lookup("workload1", false);
        let stats = collector.get_stats("workload1").unwrap();
        assert_eq!(stats.similarity_lookups, 3);
        assert_eq!(stats.similarity_hits, 2);
    }

    #[test]
    fn test_latency_percentiles() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        for i in 0..100 {
            collector.record_feature_extraction("workload1", (i * 100) as u64, 64000);
        }
        let (p50, p95, p99) = collector.get_latency_percentiles("workload1").unwrap();
        assert!(p50 > 0);
        assert!(p95 >= p50);
        assert!(p99 >= p95);
    }

    #[test]
    fn test_effectiveness_metrics() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_feature_extraction("workload1", 100, 64000);
        collector.record_similarity_lookup("workload1", true);
        collector.record_delta_compression("workload1", 10000, 5000, 500);

        let metrics = collector.calculate_effectiveness("workload1").unwrap();
        assert_eq!(metrics.workload, "workload1");
        assert_eq!(metrics.similarity_hit_rate, 1.0);
        assert!(metrics.delta_compression_ratio < 1.0);
    }

    #[test]
    fn test_export_prometheus() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_similarity_lookup("workload1", true);
        collector.record_delta_compression("workload1", 10000, 5000, 500);

        let output = collector.export_prometheus();
        assert!(output.contains("tier2_similarity_hit_rate"));
        assert!(output.contains("workload1"));
    }

    #[test]
    fn test_reset_workload() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_feature_extraction("workload1", 100, 64000);
        assert!(collector.get_stats("workload1").is_some());

        collector.reset_workload("workload1");
        assert!(collector.get_stats("workload1").is_none());
    }

    #[test]
    fn test_multiple_workloads() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_feature_extraction("workload1", 100, 64000);
        collector.record_feature_extraction("workload2", 200, 64000);

        assert_eq!(collector.list_workloads().len(), 2);
        assert!(collector.get_stats("workload1").is_some());
        assert!(collector.get_stats("workload2").is_some());
    }

    #[test]
    fn test_bytes_saved_calculation() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_delta_compression("workload1", 10000, 5000, 500);

        let metrics = collector.calculate_effectiveness("workload1").unwrap();
        assert_eq!(metrics.total_bytes_saved, 5000);
    }

    #[test]
    fn test_effectiveness_score_range() {
        let collector = SimilarityTierStats::new(StatsConfig::default());
        collector.record_feature_extraction("workload1", 100, 64000);
        collector.record_similarity_lookup("workload1", true);
        collector.record_delta_compression("workload1", 10000, 5000, 500);

        let metrics = collector.calculate_effectiveness("workload1").unwrap();
        assert!(metrics.overall_effectiveness_score >= 0.0);
        assert!(metrics.overall_effectiveness_score <= 1.0);
    }
}
