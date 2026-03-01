//! Operational metrics aggregation and cluster health scoring
//!
//! Provides per-node metrics collection and cluster-wide aggregated metrics
//! for operational dashboards and health monitoring.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Error)]
pub enum OpsMetricsError {
    #[error("node not found: {0}")]
    NodeNotFound(String),
    #[error("no data available")]
    NoData,
    #[error("invalid metric value: {0}")]
    InvalidValue(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetricsSnapshot {
    pub node_id: String,
    pub timestamp: u64,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub disk_used_percent: f64,
    pub iops_read: u64,
    pub iops_write: u64,
    pub throughput_read_mbps: f64,
    pub throughput_write_mbps: f64,
    pub error_rate: f64,
    pub latency_p99_us: u64,
    pub network_rx_mbps: f64,
    pub network_tx_mbps: f64,
}

#[derive(Debug, Clone)]
pub struct ClusterOpsMetrics {
    #[allow(dead_code)]
    pub node_count: usize,
    pub avg_cpu_percent: f64,
    pub max_cpu_percent: f64,
    pub avg_memory_percent: f64,
    pub max_memory_percent: f64,
    pub avg_disk_used_percent: f64,
    pub max_disk_used_percent: f64,
    pub total_iops_read: u64,
    pub total_iops_write: u64,
    pub total_throughput_read_mbps: f64,
    pub total_throughput_write_mbps: f64,
    pub avg_error_rate: f64,
    pub max_latency_p99_us: u64,
    pub unhealthy_nodes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ClusterHealthScore {
    pub score: u8,
    pub cpu_score: u8,
    pub memory_score: u8,
    pub disk_score: u8,
    pub error_rate_score: u8,
    pub latency_score: u8,
    pub node_availability_score: u8,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrendDirection {
    Improving,
    Stable,
    Degrading,
}

#[derive(Debug, Clone)]
pub struct MetricTrend {
    pub metric_name: String,
    pub direction: TrendDirection,
    pub change_percent: f64,
    pub sample_count: usize,
}

pub struct OpsMetricsAggregator {
    snapshots: Arc<Mutex<HashMap<String, Vec<NodeMetricsSnapshot>>>>,
    max_history: usize,
}

impl OpsMetricsAggregator {
    pub fn new(max_history: usize) -> Self {
        Self {
            snapshots: Arc::new(Mutex::new(HashMap::new())),
            max_history,
        }
    }

    pub fn record(&self, snapshot: NodeMetricsSnapshot) {
        let mut snapshots = self.snapshots.lock().unwrap();
        let node_snapshots = snapshots
            .entry(snapshot.node_id.clone())
            .or_insert_with(Vec::new);
        node_snapshots.push(snapshot);
        if node_snapshots.len() > self.max_history {
            node_snapshots.remove(0);
        }
    }

    pub fn latest(&self, node_id: &str) -> Result<NodeMetricsSnapshot, OpsMetricsError> {
        let snapshots = self.snapshots.lock().unwrap();
        let node_snapshots = snapshots
            .get(node_id)
            .ok_or_else(|| OpsMetricsError::NodeNotFound(node_id.to_string()))?;
        node_snapshots
            .last()
            .cloned()
            .ok_or(OpsMetricsError::NoData)
    }

    pub fn all_latest(&self) -> Vec<NodeMetricsSnapshot> {
        let snapshots = self.snapshots.lock().unwrap();
        let mut result = Vec::new();
        for node_snapshots in snapshots.values() {
            if let Some(latest) = node_snapshots.last() {
                result.push(latest.clone());
            }
        }
        result
    }

    pub fn cluster_metrics(&self) -> Result<ClusterOpsMetrics, OpsMetricsError> {
        let all = self.all_latest();
        if all.is_empty() {
            return Err(OpsMetricsError::NoData);
        }

        let node_count = all.len();
        let mut sum_cpu = 0.0;
        let mut max_cpu: f64 = 0.0;
        let mut sum_mem = 0.0;
        let mut max_mem: f64 = 0.0;
        let mut sum_disk = 0.0;
        let mut max_disk: f64 = 0.0;
        let mut total_iops_read = 0u64;
        let mut total_iops_write = 0u64;
        let mut total_throughput_read = 0.0;
        let mut total_throughput_write = 0.0;
        let mut sum_error_rate = 0.0;
        let mut max_latency = 0u64;
        let mut unhealthy_nodes = Vec::new();

        for node in &all {
            sum_cpu += node.cpu_percent;
            max_cpu = max_cpu.max(node.cpu_percent as f64);
            sum_mem += node.memory_percent;
            max_mem = max_mem.max(node.memory_percent as f64);
            sum_disk += node.disk_used_percent;
            max_disk = max_disk.max(node.disk_used_percent as f64);
            total_iops_read += node.iops_read;
            total_iops_write += node.iops_write;
            total_throughput_read += node.throughput_read_mbps;
            total_throughput_write += node.throughput_write_mbps;
            sum_error_rate += node.error_rate;
            max_latency = max_latency.max(node.latency_p99_us as u64);

            if node.disk_used_percent > 90.0 || node.error_rate > 1.0 {
                unhealthy_nodes.push(node.node_id.clone());
            }
        }

        Ok(ClusterOpsMetrics {
            node_count,
            avg_cpu_percent: sum_cpu / node_count as f64,
            max_cpu_percent: max_cpu,
            avg_memory_percent: sum_mem / node_count as f64,
            max_memory_percent: max_mem,
            avg_disk_used_percent: sum_disk / node_count as f64,
            max_disk_used_percent: max_disk,
            total_iops_read,
            total_iops_write,
            total_throughput_read_mbps: total_throughput_read,
            total_throughput_write_mbps: total_throughput_write,
            avg_error_rate: sum_error_rate / node_count as f64,
            max_latency_p99_us: max_latency,
            unhealthy_nodes,
        })
    }

    pub fn health_score(&self) -> Result<ClusterHealthScore, OpsMetricsError> {
        let metrics = self.cluster_metrics()?;

        let avg_cpu = metrics.avg_cpu_percent;
        let avg_mem = metrics.avg_memory_percent;
        let avg_disk = metrics.avg_disk_used_percent;
        let avg_err = metrics.avg_error_rate;
        let max_lat = metrics.max_latency_p99_us;
        let total_nodes = metrics.node_count;
        let unhealthy_count = metrics.unhealthy_nodes.len();

        let cpu_score = (100.0_f64 - avg_cpu).max(0.0_f64) as u8;
        let memory_score = (100.0_f64 - avg_mem).max(0.0_f64) as u8;
        let disk_score = (100.0_f64 - avg_disk).max(0.0_f64) as u8;

        let error_rate_score = if avg_err == 0.0 {
            100
        } else {
            (100.0_f64 - avg_err * 20.0).max(0.0_f64) as u8
        };

        let latency_score = if max_lat <= 1000 {
            100
        } else {
            (100.0_f64 - ((max_lat / 1000) as i64 - 1) as f64 * 10.0).max(0.0_f64) as u8
        };

        let node_availability_score = if total_nodes == 0 {
            0
        } else {
            ((total_nodes - unhealthy_count) as f64 / total_nodes as f64 * 100.0) as u8
        };

        let overall = (cpu_score as f64 * 0.20
            + memory_score as f64 * 0.15
            + disk_score as f64 * 0.20
            + error_rate_score as f64 * 0.25
            + latency_score as f64 * 0.10
            + node_availability_score as f64 * 0.10) as u8;

        let factors = [
            (cpu_score, "high CPU"),
            (memory_score, "high memory"),
            (disk_score, "disk pressure"),
            (error_rate_score, "high error rate"),
            (latency_score, "high latency"),
            (node_availability_score, "node availability"),
        ];
        let worst_factor = factors
            .iter()
            .min_by_key(|(score, _)| *score)
            .map(|(_, name)| name)
            .unwrap_or(&"unknown");

        let summary = if overall >= 90 {
            "Healthy".to_string()
        } else if overall >= 70 {
            format!("Warning: {}", worst_factor)
        } else if overall >= 50 {
            format!("Degraded: {}", worst_factor)
        } else {
            format!("Critical: {}", worst_factor)
        };

        Ok(ClusterHealthScore {
            score: overall,
            cpu_score,
            memory_score,
            disk_score,
            error_rate_score,
            latency_score,
            node_availability_score,
            summary,
        })
    }

    pub fn compute_trend(
        &self,
        node_id: &str,
        metric: &str,
        window: usize,
    ) -> Result<MetricTrend, OpsMetricsError> {
        let snapshots = self.snapshots.lock().unwrap();
        let node_snapshots = snapshots
            .get(node_id)
            .ok_or_else(|| OpsMetricsError::NodeNotFound(node_id.to_string()))?;

        if node_snapshots.len() < 2 {
            return Err(OpsMetricsError::NoData);
        }

        let actual_window = window.min(node_snapshots.len());
        let data: Vec<&NodeMetricsSnapshot> =
            node_snapshots.iter().rev().take(actual_window).collect();

        let half = data.len() / 2;
        if half == 0 {
            return Err(OpsMetricsError::NoData);
        }

        let first_half: Vec<f64> = data[half..]
            .iter()
            .filter_map(|s| Self::get_metric_value(s, metric))
            .collect();

        let second_half: Vec<f64> = data[..half]
            .iter()
            .filter_map(|s| Self::get_metric_value(s, metric))
            .collect();

        if first_half.is_empty() || second_half.is_empty() {
            return Err(OpsMetricsError::NoData);
        }

        let avg_first: f64 = first_half.iter().sum::<f64>() / first_half.len() as f64;
        let avg_second: f64 = second_half.iter().sum::<f64>() / second_half.len() as f64;

        let change_percent = if avg_first == 0.0 && avg_second == 0.0 {
            0.0
        } else if avg_first == 0.0 {
            100.0
        } else {
            ((avg_second - avg_first) / avg_first) * 100.0
        };

        let direction = if change_percent > 5.0 {
            TrendDirection::Degrading
        } else if change_percent < -5.0 {
            TrendDirection::Improving
        } else {
            TrendDirection::Stable
        };

        Ok(MetricTrend {
            metric_name: metric.to_string(),
            direction,
            change_percent,
            sample_count: data.len(),
        })
    }

    fn get_metric_value(snapshot: &NodeMetricsSnapshot, metric: &str) -> Option<f64> {
        match metric {
            "cpu_percent" => Some(snapshot.cpu_percent),
            "memory_percent" => Some(snapshot.memory_percent),
            "disk_used_percent" => Some(snapshot.disk_used_percent),
            "error_rate" => Some(snapshot.error_rate),
            "latency_p99_us" => Some(snapshot.latency_p99_us as f64),
            "throughput_write_mbps" => Some(snapshot.throughput_write_mbps),
            "throughput_read_mbps" => Some(snapshot.throughput_read_mbps),
            _ => None,
        }
    }

    pub fn node_count(&self) -> usize {
        let snapshots = self.snapshots.lock().unwrap();
        snapshots.len()
    }

    pub fn clear(&self) {
        let mut snapshots = self.snapshots.lock().unwrap();
        snapshots.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(
        node_id: &str,
        cpu: f64,
        mem: f64,
        disk: f64,
        errors: f64,
        latency: u64,
    ) -> NodeMetricsSnapshot {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        NodeMetricsSnapshot {
            node_id: node_id.to_string(),
            timestamp: now,
            cpu_percent: cpu,
            memory_percent: mem,
            disk_used_percent: disk,
            iops_read: 1000,
            iops_write: 500,
            throughput_read_mbps: 100.0,
            throughput_write_mbps: 50.0,
            error_rate: errors,
            latency_p99_us: latency,
            network_rx_mbps: 10.0,
            network_tx_mbps: 5.0,
        }
    }

    #[test]
    fn test_new_aggregator_empty() {
        let agg = OpsMetricsAggregator::new(10);
        assert_eq!(agg.node_count(), 0);
    }

    #[test]
    fn test_record_and_latest() {
        let agg = OpsMetricsAggregator::new(10);
        let snap = make_snapshot("node1", 50.0, 40.0, 30.0, 0.0, 100);
        agg.record(snap);
        let result = agg.latest("node1").unwrap();
        assert_eq!(result.node_id, "node1");
        assert_eq!(result.cpu_percent, 50.0);
    }

    #[test]
    fn test_latest_not_found() {
        let agg = OpsMetricsAggregator::new(10);
        let result = agg.latest("unknown");
        assert!(matches!(result, Err(OpsMetricsError::NodeNotFound(_))));
    }

    #[test]
    fn test_record_multiple_nodes() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 50.0, 40.0, 30.0, 0.0, 100));
        agg.record(make_snapshot("node2", 60.0, 50.0, 40.0, 0.0, 200));
        assert_eq!(agg.node_count(), 2);
    }

    #[test]
    fn test_all_latest_returns_one_per_node() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("A", 10.0, 10.0, 10.0, 0.0, 100));
        agg.record(make_snapshot("A", 20.0, 20.0, 20.0, 0.0, 100));
        agg.record(make_snapshot("A", 30.0, 30.0, 30.0, 0.0, 100));
        agg.record(make_snapshot("B", 40.0, 40.0, 40.0, 0.0, 100));
        agg.record(make_snapshot("B", 50.0, 50.0, 50.0, 0.0, 100));
        let all = agg.all_latest();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_max_history_trimmed() {
        let max_history = 5;
        let agg = OpsMetricsAggregator::new(max_history);
        for i in 0..(max_history + 3) {
            let mut snap = make_snapshot("node1", 10.0 + i as f64, 10.0, 10.0, 0.0, 100);
            snap.timestamp = i as u64;
            agg.record(snap);
        }
        let latest = agg.latest("node1").unwrap();
        assert_eq!(latest.cpu_percent, 17.0);
    }

    #[test]
    fn test_cluster_metrics_no_data() {
        let agg = OpsMetricsAggregator::new(10);
        let result = agg.cluster_metrics();
        assert!(matches!(result, Err(OpsMetricsError::NoData)));
    }

    #[test]
    fn test_cluster_metrics_single_node() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 50.0, 40.0, 30.0, 0.5, 500));
        let metrics = agg.cluster_metrics().unwrap();
        assert_eq!(metrics.node_count, 1);
        assert_eq!(metrics.avg_cpu_percent, 50.0);
        assert_eq!(metrics.max_cpu_percent, 50.0);
        assert_eq!(metrics.avg_memory_percent, 40.0);
    }

    #[test]
    fn test_cluster_metrics_two_nodes() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 40.0, 30.0, 20.0, 0.0, 100));
        agg.record(make_snapshot("node2", 60.0, 50.0, 40.0, 0.0, 200));
        let metrics = agg.cluster_metrics().unwrap();
        assert_eq!(metrics.node_count, 2);
        assert_eq!(metrics.avg_cpu_percent, 50.0);
        assert_eq!(metrics.max_cpu_percent, 60.0);
        assert_eq!(metrics.avg_memory_percent, 40.0);
        assert_eq!(metrics.max_memory_percent, 50.0);
    }

    #[test]
    fn test_cluster_metrics_total_iops() {
        let agg = OpsMetricsAggregator::new(10);
        let mut s1 = make_snapshot("node1", 50.0, 40.0, 30.0, 0.0, 100);
        s1.iops_read = 1000;
        s1.iops_write = 500;
        let mut s2 = make_snapshot("node2", 50.0, 40.0, 30.0, 0.0, 100);
        s2.iops_read = 2000;
        s2.iops_write = 800;
        agg.record(s1);
        agg.record(s2);
        let metrics = agg.cluster_metrics().unwrap();
        assert_eq!(metrics.total_iops_read, 3000);
        assert_eq!(metrics.total_iops_write, 1300);
    }

    #[test]
    fn test_health_score_healthy() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 5.0, 10.0, 20.0, 0.0, 100));
        agg.record(make_snapshot("node2", 5.0, 10.0, 20.0, 0.0, 200));
        let score = agg.health_score().unwrap();
        assert!(score.score >= 90);
        assert_eq!(score.summary, "Healthy");
    }

    #[test]
    fn test_health_score_high_cpu() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 95.0, 30.0, 40.0, 0.0, 100));
        let score = agg.health_score().unwrap();
        assert!(score.score < 90);
        assert!(score.summary.contains("Warning") || score.summary.contains("Degraded"));
    }

    #[test]
    fn test_health_score_full_disk() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 30.0, 30.0, 95.0, 0.0, 100));
        let score = agg.health_score().unwrap();
        assert!(score.disk_score < 20);
    }

    #[test]
    fn test_health_score_high_error_rate() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 30.0, 30.0, 40.0, 5.0, 100));
        let score = agg.health_score().unwrap();
        assert!(score.error_rate_score < 20);
    }

    #[test]
    fn test_health_score_high_latency() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 30.0, 30.0, 40.0, 0.0, 50000));
        let score = agg.health_score().unwrap();
        assert!(score.latency_score < 50);
    }

    #[test]
    fn test_health_score_no_data() {
        let agg = OpsMetricsAggregator::new(10);
        let result = agg.health_score();
        assert!(matches!(result, Err(OpsMetricsError::NoData)));
    }

    #[test]
    fn test_compute_trend_improving() {
        let agg = OpsMetricsAggregator::new(10);
        for i in (0..5).rev() {
            let mut snap = make_snapshot("node1", 50.0 + i as f64 * 10.0, 30.0, 40.0, 0.0, 100);
            snap.timestamp = i as u64;
            agg.record(snap);
        }
        let trend = agg.compute_trend("node1", "cpu_percent", 5).unwrap();
        assert_eq!(trend.direction, TrendDirection::Improving);
    }

    #[test]
    fn test_compute_trend_degrading() {
        let agg = OpsMetricsAggregator::new(10);
        for i in 0..5 {
            let mut snap = make_snapshot("node1", 20.0 + i as f64 * 10.0, 30.0, 40.0, 0.0, 100);
            snap.timestamp = i as u64;
            agg.record(snap);
        }
        let trend = agg.compute_trend("node1", "cpu_percent", 5).unwrap();
        assert_eq!(trend.direction, TrendDirection::Degrading);
    }

    #[test]
    fn test_compute_trend_stable() {
        let agg = OpsMetricsAggregator::new(10);
        for _ in 0..5 {
            let mut snap = make_snapshot("node1", 50.0, 30.0, 40.0, 0.0, 100);
            snap.timestamp = 0;
            agg.record(snap);
        }
        let trend = agg.compute_trend("node1", "cpu_percent", 5).unwrap();
        assert_eq!(trend.direction, TrendDirection::Stable);
    }

    #[test]
    fn test_compute_trend_not_found() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 50.0, 30.0, 40.0, 0.0, 100));
        let result = agg.compute_trend("unknown", "cpu_percent", 10);
        assert!(matches!(result, Err(OpsMetricsError::NodeNotFound(_))));
    }

    #[test]
    fn test_compute_trend_no_data() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 50.0, 30.0, 40.0, 0.0, 100));
        let result = agg.compute_trend("node1", "cpu_percent", 10);
        assert!(matches!(result, Err(OpsMetricsError::NoData)));
    }

    #[test]
    fn test_unhealthy_nodes_detected() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 30.0, 30.0, 95.0, 0.0, 100));
        agg.record(make_snapshot("node2", 30.0, 30.0, 40.0, 0.0, 100));
        let metrics = agg.cluster_metrics().unwrap();
        assert!(metrics.unhealthy_nodes.contains(&"node1".to_string()));
    }

    #[test]
    fn test_cluster_health_critical() {
        let agg = OpsMetricsAggregator::new(10);
        agg.record(make_snapshot("node1", 95.0, 95.0, 98.0, 10.0, 100000));
        let score = agg.health_score().unwrap();
        assert!(score.score < 50);
        assert!(score.summary.contains("Critical"));
    }
}
