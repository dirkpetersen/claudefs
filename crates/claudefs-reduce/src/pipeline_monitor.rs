//! Real-time monitoring and alerting for the reduction pipeline.
//!
//! Aggregates metrics from multiple pipeline stages and generates alerts
//! when thresholds are exceeded.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metrics for a single pipeline stage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StageMetrics {
    /// Name of the pipeline stage
    pub stage_name: String,
    /// Number of chunks entering this stage
    pub chunks_in: u64,
    /// Number of chunks leaving this stage
    pub chunks_out: u64,
    /// Total bytes entering this stage
    pub bytes_in: u64,
    /// Total bytes leaving this stage
    pub bytes_out: u64,
    /// Number of errors encountered
    pub errors: u64,
    /// Sum of latencies in microseconds
    pub latency_sum_us: u64,
    /// Number of latency measurements
    pub latency_count: u64,
}

impl StageMetrics {
    /// Create new metrics for a named stage
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            stage_name: name.into(),
            ..Default::default()
        }
    }

    /// Average latency in microseconds
    pub fn avg_latency_us(&self) -> f64 {
        if self.latency_count == 0 {
            return 0.0;
        }
        self.latency_sum_us as f64 / self.latency_count as f64
    }

    /// Reduction ratio (bytes_in / bytes_out)
    pub fn reduction_ratio(&self) -> f64 {
        if self.bytes_out == 0 {
            return 1.0;
        }
        self.bytes_in as f64 / self.bytes_out as f64
    }

    /// Error rate (errors / chunks_in)
    pub fn error_rate(&self) -> f64 {
        if self.chunks_in == 0 {
            return 0.0;
        }
        self.errors as f64 / self.chunks_in as f64
    }

    /// Merge another metrics into this one
    pub fn merge(&mut self, other: &StageMetrics) {
        self.chunks_in += other.chunks_in;
        self.chunks_out += other.chunks_out;
        self.bytes_in += other.bytes_in;
        self.bytes_out += other.bytes_out;
        self.errors += other.errors;
        self.latency_sum_us += other.latency_sum_us;
        self.latency_count += other.latency_count;
    }
}

/// Aggregated metrics across all pipeline stages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineMetrics {
    /// Metrics per stage
    pub stages: Vec<StageMetrics>,
    /// Total chunks processed
    pub total_chunks: u64,
    /// Total bytes input
    pub total_bytes_in: u64,
    /// Total bytes output
    pub total_bytes_out: u64,
}

impl PipelineMetrics {
    /// Overall reduction ratio across all stages
    pub fn overall_reduction_ratio(&self) -> f64 {
        if self.total_bytes_out == 0 {
            return 1.0;
        }
        self.total_bytes_in as f64 / self.total_bytes_out as f64
    }
}

/// Alert types for pipeline monitoring.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineAlert {
    /// Error rate exceeded threshold
    HighErrorRate {
        /// Stage name
        stage: String,
        /// Current error rate
        rate: f64,
    },
    /// Reduction ratio below threshold
    LowReduction {
        /// Stage name
        stage: String,
        /// Current reduction ratio
        ratio: f64,
    },
    /// Latency exceeded threshold
    HighLatency {
        /// Stage name
        stage: String,
        /// Current average latency in microseconds
        latency_us: u64,
    },
}

/// Thresholds for generating alerts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThreshold {
    /// Maximum acceptable error rate (0.0-1.0)
    pub max_error_rate: f64,
    /// Minimum acceptable reduction ratio
    pub min_reduction_ratio: f64,
    /// Maximum acceptable latency in microseconds
    pub max_latency_us: u64,
}

impl Default for AlertThreshold {
    fn default() -> Self {
        Self {
            max_error_rate: 0.01,
            min_reduction_ratio: 1.5,
            max_latency_us: 100_000,
        }
    }
}

/// Pipeline monitor that aggregates stage metrics and generates alerts.
#[derive(Debug, Default)]
pub struct PipelineMonitor {
    stages: HashMap<String, StageMetrics>,
}

impl PipelineMonitor {
    /// Create a new empty monitor
    pub fn new() -> Self {
        Self::default()
    }

    /// Record or update metrics for a stage
    pub fn record_stage(&mut self, metrics: StageMetrics) {
        self.stages
            .entry(metrics.stage_name.clone())
            .and_modify(|existing| existing.merge(&metrics))
            .or_insert(metrics);
    }

    /// Get a snapshot of current pipeline metrics
    pub fn snapshot(&self) -> PipelineMetrics {
        let stages: Vec<StageMetrics> = self.stages.values().cloned().collect();

        let total_chunks = stages.iter().map(|s| s.chunks_in).sum();
        let total_bytes_in = stages.iter().map(|s| s.bytes_in).sum();
        let total_bytes_out = stages.iter().map(|s| s.bytes_out).sum();

        PipelineMetrics {
            stages,
            total_chunks,
            total_bytes_in,
            total_bytes_out,
        }
    }

    /// Clear all recorded metrics
    pub fn reset(&mut self) {
        self.stages.clear();
    }

    /// Check for alert conditions based on thresholds
    pub fn check_alerts(&self, threshold: &AlertThreshold) -> Vec<PipelineAlert> {
        let mut alerts = Vec::new();

        for (name, metrics) in &self.stages {
            let error_rate = metrics.error_rate();
            if error_rate > threshold.max_error_rate {
                alerts.push(PipelineAlert::HighErrorRate {
                    stage: name.clone(),
                    rate: error_rate,
                });
            }

            let ratio = metrics.reduction_ratio();
            if ratio < threshold.min_reduction_ratio {
                alerts.push(PipelineAlert::LowReduction {
                    stage: name.clone(),
                    ratio,
                });
            }

            let avg_latency = metrics.avg_latency_us() as u64;
            if avg_latency > threshold.max_latency_us {
                alerts.push(PipelineAlert::HighLatency {
                    stage: name.clone(),
                    latency_us: avg_latency,
                });
            }
        }

        alerts
    }

    /// Get metrics for a specific stage
    pub fn get_stage(&self, name: &str) -> Option<&StageMetrics> {
        self.stages.get(name)
    }

    /// Number of stages being tracked
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_monitor_has_no_stages() {
        let monitor = PipelineMonitor::new();
        assert_eq!(monitor.stage_count(), 0);
        assert!(monitor.snapshot().stages.is_empty());
    }

    #[test]
    fn test_record_stage_adds_stage() {
        let mut monitor = PipelineMonitor::new();
        let metrics = StageMetrics {
            stage_name: "compress".to_string(),
            chunks_in: 100,
            chunks_out: 100,
            bytes_in: 10000,
            bytes_out: 5000,
            errors: 0,
            latency_sum_us: 1000,
            latency_count: 100,
        };

        monitor.record_stage(metrics);

        assert_eq!(monitor.stage_count(), 1);
        assert!(monitor.get_stage("compress").is_some());
    }

    #[test]
    fn test_snapshot_no_stages() {
        let monitor = PipelineMonitor::new();
        let snap = monitor.snapshot();

        assert!(snap.stages.is_empty());
        assert_eq!(snap.total_chunks, 0);
        assert_eq!(snap.total_bytes_in, 0);
        assert_eq!(snap.total_bytes_out, 0);
    }

    #[test]
    fn test_snapshot_aggregates_multiple_stages() {
        let mut monitor = PipelineMonitor::new();

        monitor.record_stage(StageMetrics {
            stage_name: "dedup".to_string(),
            chunks_in: 100,
            bytes_in: 10000,
            bytes_out: 8000,
            ..Default::default()
        });

        monitor.record_stage(StageMetrics {
            stage_name: "compress".to_string(),
            chunks_in: 80,
            bytes_in: 8000,
            bytes_out: 4000,
            ..Default::default()
        });

        let snap = monitor.snapshot();

        assert_eq!(snap.stages.len(), 2);
        assert_eq!(snap.total_bytes_in, 18000);
        assert_eq!(snap.total_bytes_out, 12000);
    }

    #[test]
    fn test_total_bytes_summed_correctly() {
        let mut monitor = PipelineMonitor::new();

        monitor.record_stage(StageMetrics {
            stage_name: "stage1".to_string(),
            bytes_in: 1000,
            bytes_out: 500,
            ..Default::default()
        });

        monitor.record_stage(StageMetrics {
            stage_name: "stage2".to_string(),
            bytes_in: 2000,
            bytes_out: 1000,
            ..Default::default()
        });

        let snap = monitor.snapshot();

        assert_eq!(snap.total_bytes_in, 3000);
        assert_eq!(snap.total_bytes_out, 1500);
    }

    #[test]
    fn test_overall_reduction_ratio_with_data() {
        let metrics = PipelineMetrics {
            stages: vec![],
            total_chunks: 100,
            total_bytes_in: 10000,
            total_bytes_out: 5000,
        };

        assert_eq!(metrics.overall_reduction_ratio(), 2.0);
    }

    #[test]
    fn test_overall_reduction_ratio_zero_bytes_out() {
        let metrics = PipelineMetrics {
            stages: vec![],
            total_chunks: 100,
            total_bytes_in: 10000,
            total_bytes_out: 0,
        };

        assert_eq!(metrics.overall_reduction_ratio(), 1.0);
    }

    #[test]
    fn test_stage_avg_latency_zero_count() {
        let metrics = StageMetrics {
            stage_name: "test".to_string(),
            latency_sum_us: 0,
            latency_count: 0,
            ..Default::default()
        };

        assert_eq!(metrics.avg_latency_us(), 0.0);
    }

    #[test]
    fn test_stage_avg_latency_with_data() {
        let metrics = StageMetrics {
            stage_name: "test".to_string(),
            latency_sum_us: 1000,
            latency_count: 10,
            ..Default::default()
        };

        assert_eq!(metrics.avg_latency_us(), 100.0);
    }

    #[test]
    fn test_stage_reduction_ratio() {
        let metrics = StageMetrics {
            stage_name: "compress".to_string(),
            bytes_in: 10000,
            bytes_out: 5000,
            ..Default::default()
        };

        assert_eq!(metrics.reduction_ratio(), 2.0);
    }

    #[test]
    fn test_stage_error_rate() {
        let metrics = StageMetrics {
            stage_name: "test".to_string(),
            chunks_in: 100,
            errors: 5,
            ..Default::default()
        };

        assert_eq!(metrics.error_rate(), 0.05);
    }

    #[test]
    fn test_check_alerts_no_alerts_healthy() {
        let mut monitor = PipelineMonitor::new();

        monitor.record_stage(StageMetrics {
            stage_name: "healthy".to_string(),
            chunks_in: 100,
            errors: 0,
            bytes_in: 10000,
            bytes_out: 5000,
            latency_sum_us: 1000,
            latency_count: 100,
            ..Default::default()
        });

        let threshold = AlertThreshold::default();
        let alerts = monitor.check_alerts(&threshold);

        assert!(alerts.is_empty());
    }

    #[test]
    fn test_check_alerts_high_error_rate() {
        let mut monitor = PipelineMonitor::new();

        monitor.record_stage(StageMetrics {
            stage_name: "failing".to_string(),
            chunks_in: 100,
            chunks_out: 100,
            errors: 50,
            bytes_in: 10000,
            bytes_out: 5000,
            latency_sum_us: 100,
            latency_count: 100,
        });

        let threshold = AlertThreshold {
            max_error_rate: 0.01,
            min_reduction_ratio: 1.0,
            max_latency_us: 1_000_000,
        };

        let alerts = monitor.check_alerts(&threshold);

        assert_eq!(alerts.len(), 1);
        assert!(matches!(
            &alerts[0],
            PipelineAlert::HighErrorRate { stage, .. } if stage == "failing"
        ));
    }

    #[test]
    fn test_check_alerts_low_reduction() {
        let mut monitor = PipelineMonitor::new();

        monitor.record_stage(StageMetrics {
            stage_name: "bad_compress".to_string(),
            bytes_in: 100,
            bytes_out: 99, // Almost no reduction
            ..Default::default()
        });

        let threshold = AlertThreshold {
            min_reduction_ratio: 1.5,
            ..Default::default()
        };

        let alerts = monitor.check_alerts(&threshold);

        assert_eq!(alerts.len(), 1);
        assert!(matches!(
            &alerts[0],
            PipelineAlert::LowReduction { stage, .. } if stage == "bad_compress"
        ));
    }

    #[test]
    fn test_check_alerts_high_latency() {
        let mut monitor = PipelineMonitor::new();

        monitor.record_stage(StageMetrics {
            stage_name: "slow".to_string(),
            latency_sum_us: 1_000_000,
            latency_count: 10,
            chunks_in: 100,
            chunks_out: 100,
            errors: 0,
            bytes_in: 10000,
            bytes_out: 5000,
        });

        let threshold = AlertThreshold {
            max_latency_us: 50_000,
            max_error_rate: 1.0,
            min_reduction_ratio: 1.0,
        };

        let alerts = monitor.check_alerts(&threshold);

        assert_eq!(alerts.len(), 1);
        assert!(matches!(
            &alerts[0],
            PipelineAlert::HighLatency { stage, .. } if stage == "slow"
        ));
    }

    #[test]
    fn test_reset_clears_metrics() {
        let mut monitor = PipelineMonitor::new();

        monitor.record_stage(StageMetrics {
            stage_name: "test".to_string(),
            chunks_in: 100,
            ..Default::default()
        });

        assert_eq!(monitor.stage_count(), 1);

        monitor.reset();

        assert_eq!(monitor.stage_count(), 0);
    }

    #[test]
    fn test_record_stage_updates_existing() {
        let mut monitor = PipelineMonitor::new();

        monitor.record_stage(StageMetrics {
            stage_name: "compress".to_string(),
            chunks_in: 100,
            bytes_in: 10000,
            ..Default::default()
        });

        monitor.record_stage(StageMetrics {
            stage_name: "compress".to_string(),
            chunks_in: 50,
            bytes_in: 5000,
            ..Default::default()
        });

        let stage = monitor.get_stage("compress").unwrap();
        assert_eq!(stage.chunks_in, 150);
        assert_eq!(stage.bytes_in, 15000);
    }
}
