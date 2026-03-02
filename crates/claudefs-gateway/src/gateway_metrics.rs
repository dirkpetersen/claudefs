//! Gateway Prometheus Metrics

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::debug;

/// Protocol tracked in metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricProtocol {
    /// NFSv3 protocol
    Nfs3,
    /// NFSv4 protocol
    Nfs4,
    /// pNFS protocol
    Pnfs,
    /// S3 API protocol
    S3,
    /// SMB3 protocol
    Smb3,
}

impl MetricProtocol {
    /// Returns the protocol name
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricProtocol::Nfs3 => "nfs3",
            MetricProtocol::Nfs4 => "nfs4",
            MetricProtocol::Pnfs => "pnfs",
            MetricProtocol::S3 => "s3",
            MetricProtocol::Smb3 => "smb3",
        }
    }
}

impl std::fmt::Display for MetricProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Operation type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricOperation {
    /// Read operation
    Read,
    /// Write operation
    Write,
    /// Lookup operation
    Lookup,
    /// Get attributes operation
    Getattr,
    /// Set attributes operation
    Setattr,
    /// Create file operation
    Create,
    /// Remove file operation
    Remove,
    /// Rename operation
    Rename,
    /// Read directory operation
    Readdir,
    /// Make directory operation
    Mkdir,
    /// Remove directory operation
    Rmdir,
    /// Create hard link operation
    Link,
    /// Create symbolic link operation
    Symlink,
    /// Get filesystem stats operation
    Statfs,
    /// Commit operation (for NFS)
    Commit,
    /// Open file operation
    Open,
    /// Close file operation
    Close,
    /// Lock operation
    Lock,
    /// Unlock operation
    Unlock,
    /// Custom operation (not in standard set)
    Custom(String),
}

impl MetricOperation {
    /// Returns the operation name
    pub fn as_str(&self) -> String {
        match self {
            MetricOperation::Read => "read".to_string(),
            MetricOperation::Write => "write".to_string(),
            MetricOperation::Lookup => "lookup".to_string(),
            MetricOperation::Getattr => "getattr".to_string(),
            MetricOperation::Setattr => "setattr".to_string(),
            MetricOperation::Create => "create".to_string(),
            MetricOperation::Remove => "remove".to_string(),
            MetricOperation::Rename => "rename".to_string(),
            MetricOperation::Readdir => "readdir".to_string(),
            MetricOperation::Mkdir => "mkdir".to_string(),
            MetricOperation::Rmdir => "rmdir".to_string(),
            MetricOperation::Link => "link".to_string(),
            MetricOperation::Symlink => "symlink".to_string(),
            MetricOperation::Statfs => "statfs".to_string(),
            MetricOperation::Commit => "commit".to_string(),
            MetricOperation::Open => "open".to_string(),
            MetricOperation::Close => "close".to_string(),
            MetricOperation::Lock => "lock".to_string(),
            MetricOperation::Unlock => "unlock".to_string(),
            MetricOperation::Custom(s) => s.clone(),
        }
    }
}

impl From<&str> for MetricOperation {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "read" => MetricOperation::Read,
            "write" => MetricOperation::Write,
            "lookup" => MetricOperation::Lookup,
            "getattr" => MetricOperation::Getattr,
            "setattr" => MetricOperation::Setattr,
            "create" => MetricOperation::Create,
            "remove" => MetricOperation::Remove,
            "rename" => MetricOperation::Rename,
            "readdir" => MetricOperation::Readdir,
            "mkdir" => MetricOperation::Mkdir,
            "rmdir" => MetricOperation::Rmdir,
            "link" => MetricOperation::Link,
            "symlink" => MetricOperation::Symlink,
            "statfs" => MetricOperation::Statfs,
            "commit" => MetricOperation::Commit,
            "open" => MetricOperation::Open,
            "close" => MetricOperation::Close,
            "lock" => MetricOperation::Lock,
            "unlock" => MetricOperation::Unlock,
            other => MetricOperation::Custom(other.to_string()),
        }
    }
}

/// Latency histogram bucket boundaries (microseconds)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyHistogram {
    /// Bucket upper bounds in microseconds
    pub buckets: Vec<u64>,
    /// Count of observations in each bucket
    pub counts: Vec<u64>,
    /// Total sum of all observations
    pub sum_us: u64,
    /// Total number of observations
    pub count: u64,
}

impl LatencyHistogram {
    /// Creates a new histogram with default buckets
    pub fn new() -> Self {
        Self {
            buckets: vec![100, 500, 1000, 5000, 10000, 50000, 100000, u64::MAX],
            counts: vec![0; 8],
            sum_us: 0,
            count: 0,
        }
    }

    /// Record a latency observation
    pub fn observe(&mut self, latency_us: u64) {
        self.count += 1;
        self.sum_us += latency_us;

        for (i, bound) in self.buckets.iter().enumerate() {
            if latency_us <= *bound {
                self.counts[i] += 1;
                return;
            }
        }
    }

    /// 50th percentile latency in microseconds
    pub fn p50_us(&self) -> u64 {
        self.percentile(0.5)
    }

    /// 99th percentile latency in microseconds
    pub fn p99_us(&self) -> u64 {
        self.percentile(0.99)
    }

    /// 99.9th percentile latency in microseconds
    pub fn p999_us(&self) -> u64 {
        self.percentile(0.999)
    }

    /// Mean latency in microseconds
    pub fn mean_us(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.sum_us / self.count
        }
    }

    fn percentile(&self, p: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }

        let target = (self.count as f64 * p).ceil() as u64;
        let mut cumulative = 0u64;

        for (i, count) in self.counts.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return self.buckets[i];
            }
        }

        *self.buckets.last().unwrap_or(&u64::MAX)
    }

    /// Reset all counters
    pub fn reset(&mut self) {
        for count in &mut self.counts {
            *count = 0;
        }
        self.sum_us = 0;
        self.count = 0;
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-protocol, per-operation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationMetrics {
    /// Total number of requests
    pub total_requests: u64,
    /// Number of successful requests
    pub success_count: u64,
    /// Number of failed requests
    pub error_count: u64,
    /// Total bytes read
    pub bytes_read: u64,
    /// Total bytes written
    pub bytes_written: u64,
    /// Latency histogram
    pub latency: LatencyHistogram,
}

impl OperationMetrics {
    /// Creates new operation metrics
    pub fn new() -> Self {
        Self {
            total_requests: 0,
            success_count: 0,
            error_count: 0,
            bytes_read: 0,
            bytes_written: 0,
            latency: LatencyHistogram::new(),
        }
    }

    /// Record a successful operation
    pub fn record_success(&mut self, latency_us: u64, bytes_read: u64, bytes_written: u64) {
        self.total_requests += 1;
        self.success_count += 1;
        self.bytes_read += bytes_read;
        self.bytes_written += bytes_written;
        self.latency.observe(latency_us);
    }

    /// Record a failed operation
    pub fn record_error(&mut self, latency_us: u64) {
        self.total_requests += 1;
        self.error_count += 1;
        self.latency.observe(latency_us);
    }

    /// Error rate (errors / total)
    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.error_count as f64 / self.total_requests as f64
        }
    }
}

impl Default for OperationMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Top-level gateway metrics registry
#[derive(Debug)]
pub struct GatewayMetrics {
    ops: HashMap<(String, String), OperationMetrics>,
    /// Active connections per protocol
    pub active_connections: HashMap<String, u64>,
    /// Circuit breaker open count (per backend)
    pub circuit_breakers_open: HashMap<String, bool>,
    /// Total backend errors (across all connections)
    pub backend_errors_total: u64,
    /// Gateway uptime start
    pub started_at: std::time::SystemTime,
}

impl GatewayMetrics {
    /// Creates new gateway metrics
    pub fn new() -> Self {
        Self {
            ops: HashMap::new(),
            active_connections: HashMap::new(),
            circuit_breakers_open: HashMap::new(),
            backend_errors_total: 0,
            started_at: std::time::SystemTime::now(),
        }
    }

    fn key(protocol: &MetricProtocol, op: &MetricOperation) -> (String, String) {
        (protocol.as_str().to_string(), op.as_str())
    }

    /// Record an operation result
    pub fn record_op(
        &mut self,
        protocol: MetricProtocol,
        op: MetricOperation,
        latency_us: u64,
        bytes_read: u64,
        bytes_written: u64,
        success: bool,
    ) {
        let key = Self::key(&protocol, &op);
        let metrics = self.ops.entry(key).or_insert_with(OperationMetrics::new);

        if success {
            metrics.record_success(latency_us, bytes_read, bytes_written);
        } else {
            metrics.record_error(latency_us);
            self.backend_errors_total += 1;
        }

        debug!(
            "Recorded {} {} op: success={}, latency={}us",
            protocol,
            op.as_str(),
            success,
            latency_us
        );
    }

    /// Get metrics for a specific operation
    pub fn get_op_metrics(
        &self,
        protocol: &MetricProtocol,
        op: &MetricOperation,
    ) -> Option<&OperationMetrics> {
        let key = Self::key(protocol, op);
        self.ops.get(&key)
    }

    /// Aggregate: total requests across all protocols
    pub fn total_requests(&self) -> u64 {
        self.ops.values().map(|m| m.total_requests).sum()
    }

    /// Aggregate: total errors
    pub fn total_errors(&self) -> u64 {
        self.ops.values().map(|m| m.error_count).sum()
    }

    /// Overall error rate
    pub fn overall_error_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            0.0
        } else {
            self.total_errors() as f64 / total as f64
        }
    }

    /// Gateway uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().map(|d| d.as_secs()).unwrap_or(0)
    }

    /// Export metrics as a simple text format (Prometheus-like)
    pub fn export_text(&self) -> String {
        let mut lines = Vec::new();

        // Operation metrics
        for ((protocol, op), metrics) in &self.ops {
            lines.push(format!(
                "gateway_requests_total{{protocol=\"{}\",op=\"{}\"}} {}",
                protocol, op, metrics.total_requests
            ));
            lines.push(format!(
                "gateway_requests_success_total{{protocol=\"{}\",op=\"{}\"}} {}",
                protocol, op, metrics.success_count
            ));
            lines.push(format!(
                "gateway_requests_error_total{{protocol=\"{}\",op=\"{}\"}} {}",
                protocol, op, metrics.error_count
            ));
            lines.push(format!(
                "gateway_bytes_read_total{{protocol=\"{}\",op=\"{}\"}} {}",
                protocol, op, metrics.bytes_read
            ));
            lines.push(format!(
                "gateway_bytes_written_total{{protocol=\"{}\",op=\"{}\"}} {}",
                protocol, op, metrics.bytes_written
            ));
            lines.push(format!(
                "gateway_latency_mean_us{{protocol=\"{}\",op=\"{}\"}} {}",
                protocol,
                op,
                metrics.latency.mean_us()
            ));
            lines.push(format!(
                "gateway_latency_p99_us{{protocol=\"{}\",op=\"{}\"}} {}",
                protocol,
                op,
                metrics.latency.p99_us()
            ));
        }

        // Aggregate metrics
        lines.push(format!("gateway_requests_total {}", self.total_requests()));
        lines.push(format!("gateway_errors_total {}", self.total_errors()));
        lines.push(format!("gateway_error_rate {}", self.overall_error_rate()));
        lines.push(format!("gateway_uptime_seconds {}", self.uptime_secs()));
        lines.push(format!(
            "gateway_backend_errors_total {}",
            self.backend_errors_total
        ));

        // Active connections
        for (protocol, count) in &self.active_connections {
            lines.push(format!(
                "gateway_active_connections{{protocol=\"{}\"}} {}",
                protocol, count
            ));
        }

        lines.join("\n")
    }

    /// Reset all metrics (for testing)
    pub fn reset(&mut self) {
        self.ops.clear();
        self.active_connections.clear();
        self.circuit_breakers_open.clear();
        self.backend_errors_total = 0;
        self.started_at = std::time::SystemTime::now();
    }
}

impl Default for GatewayMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics errors
#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("Operation not found: {0}")]
    OperationNotFound(String),

    #[error("Invalid metric: {0}")]
    InvalidMetric(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_histogram_observe() {
        let mut hist = LatencyHistogram::new();

        hist.observe(50);
        hist.observe(200);
        hist.observe(800);
        hist.observe(5000);

        assert_eq!(hist.count, 4);
        assert_eq!(hist.sum_us, 50 + 200 + 800 + 5000);
    }

    #[test]
    fn test_latency_histogram_p50() {
        let mut hist = LatencyHistogram::new();

        for _ in 0..100 {
            hist.observe(100);
        }

        // All values in first bucket (100)
        assert_eq!(hist.p50_us(), 100);
    }

    #[test]
    fn test_latency_histogram_p99() {
        let mut hist = LatencyHistogram::new();

        // Add 99 values at 100us
        for _ in 0..99 {
            hist.observe(100);
        }
        // Add 1 value at 100000us
        hist.observe(100000);

        // P99 should be in a higher bucket
        let p99 = hist.p99_us();
        assert!(p99 >= 100);
    }

    #[test]
    fn test_latency_histogram_p999() {
        let mut hist = LatencyHistogram::new();

        for i in 0..1000 {
            hist.observe(if i < 999 { 100 } else { 100000 });
        }

        let p999 = hist.p999_us();
        assert!(p999 >= 100);
    }

    #[test]
    fn test_latency_histogram_mean() {
        let mut hist = LatencyHistogram::new();

        hist.observe(100);
        hist.observe(200);
        hist.observe(300);

        assert_eq!(hist.mean_us(), 200);
    }

    #[test]
    fn test_latency_histogram_reset() {
        let mut hist = LatencyHistogram::new();

        hist.observe(100);
        hist.observe(200);

        hist.reset();

        assert_eq!(hist.count, 0);
        assert_eq!(hist.sum_us, 0);
    }

    #[test]
    fn test_operation_metrics_record_success() {
        let mut metrics = OperationMetrics::new();

        metrics.record_success(100, 4096, 0);
        metrics.record_success(200, 8192, 0);

        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.success_count, 2);
        assert_eq!(metrics.error_count, 0);
        assert_eq!(metrics.bytes_read, 12288);
    }

    #[test]
    fn test_operation_metrics_record_error() {
        let mut metrics = OperationMetrics::new();

        metrics.record_error(100);
        metrics.record_error(200);

        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.success_count, 0);
        assert_eq!(metrics.error_count, 2);
    }

    #[test]
    fn test_operation_metrics_error_rate() {
        let mut metrics = OperationMetrics::new();

        metrics.record_success(100, 0, 0);
        metrics.record_success(100, 0, 0);
        metrics.record_error(100);

        assert!((metrics.error_rate() - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_gateway_metrics_record_op() {
        let mut metrics = GatewayMetrics::new();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );
        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Write,
            200,
            0,
            8192,
            true,
        );
        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            150,
            0,
            0,
            false,
        );

        assert_eq!(metrics.total_requests(), 3);
        assert_eq!(metrics.total_errors(), 1);
    }

    #[test]
    fn test_gateway_metrics_total_requests() {
        let mut metrics = GatewayMetrics::new();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );
        metrics.record_op(
            MetricProtocol::Nfs4,
            MetricOperation::Write,
            200,
            0,
            4096,
            true,
        );

        assert_eq!(metrics.total_requests(), 2);
    }

    #[test]
    fn test_gateway_metrics_total_errors() {
        let mut metrics = GatewayMetrics::new();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            false,
        );
        metrics.record_op(
            MetricProtocol::Nfs4,
            MetricOperation::Write,
            200,
            0,
            4096,
            true,
        );

        assert_eq!(metrics.total_errors(), 1);
    }

    #[test]
    fn test_gateway_metrics_overall_error_rate() {
        let mut metrics = GatewayMetrics::new();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            false,
        );
        metrics.record_op(
            MetricProtocol::Nfs4,
            MetricOperation::Write,
            200,
            0,
            4096,
            true,
        );

        assert!((metrics.overall_error_rate() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_gateway_metrics_export_text() {
        let mut metrics = GatewayMetrics::new();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );

        let text = metrics.export_text();
        assert!(text.contains("gateway_requests_total"));
        assert!(text.contains("nfs3"));
        assert!(text.contains("read"));
    }

    #[test]
    fn test_gateway_metrics_uptime() {
        let metrics = GatewayMetrics::new();

        // Just created, should be 0 or very small
        let uptime = metrics.uptime_secs();
        assert!(uptime <= 1);
    }

    #[test]
    fn test_gateway_metrics_active_connections() {
        let mut metrics = GatewayMetrics::new();

        *metrics
            .active_connections
            .entry("nfs3".to_string())
            .or_insert(0) += 5;
        *metrics
            .active_connections
            .entry("s3".to_string())
            .or_insert(0) += 10;

        assert_eq!(*metrics.active_connections.get("nfs3").unwrap(), 5);
        assert_eq!(*metrics.active_connections.get("s3").unwrap(), 10);
    }

    #[test]
    fn test_gateway_metrics_reset() {
        let mut metrics = GatewayMetrics::new();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );
        metrics.active_connections.insert("nfs3".to_string(), 5);

        metrics.reset();

        assert_eq!(metrics.total_requests(), 0);
        assert!(metrics.active_connections.is_empty());
    }

    #[test]
    fn test_metric_protocol_as_str() {
        assert_eq!(MetricProtocol::Nfs3.as_str(), "nfs3");
        assert_eq!(MetricProtocol::Nfs4.as_str(), "nfs4");
        assert_eq!(MetricProtocol::Pnfs.as_str(), "pnfs");
        assert_eq!(MetricProtocol::S3.as_str(), "s3");
        assert_eq!(MetricProtocol::Smb3.as_str(), "smb3");
    }

    #[test]
    fn test_metric_operation_as_str() {
        assert_eq!(MetricOperation::Read.as_str(), "read");
        assert_eq!(MetricOperation::Write.as_str(), "write");
        assert_eq!(MetricOperation::Lookup.as_str(), "lookup");
    }

    #[test]
    fn test_metric_operation_from_str() {
        assert!(matches!(
            MetricOperation::from("read"),
            MetricOperation::Read
        ));
        assert!(matches!(
            MetricOperation::from("write"),
            MetricOperation::Write
        ));
        assert!(matches!(
            MetricOperation::from("custom_op"),
            MetricOperation::Custom(_)
        ));
    }

    #[test]
    fn test_get_op_metrics() {
        let mut metrics = GatewayMetrics::new();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );

        let op_metrics = metrics.get_op_metrics(&MetricProtocol::Nfs3, &MetricOperation::Read);
        assert!(op_metrics.is_some());
        assert_eq!(op_metrics.unwrap().total_requests, 1);

        let no_metrics = metrics.get_op_metrics(&MetricProtocol::Nfs4, &MetricOperation::Read);
        assert!(no_metrics.is_none());
    }

    #[test]
    fn test_empty_histogram_percentiles() {
        let hist = LatencyHistogram::new();

        assert_eq!(hist.p50_us(), 0);
        assert_eq!(hist.p99_us(), 0);
        assert_eq!(hist.p999_us(), 0);
        assert_eq!(hist.mean_us(), 0);
    }

    #[test]
    fn test_empty_metrics_error_rate() {
        let metrics = OperationMetrics::new();

        assert_eq!(metrics.error_rate(), 0.0);
    }

    #[test]
    fn test_circuit_breakers_tracking() {
        let mut metrics = GatewayMetrics::new();

        metrics
            .circuit_breakers_open
            .insert("node1".to_string(), true);
        metrics
            .circuit_breakers_open
            .insert("node2".to_string(), false);

        assert!(*metrics.circuit_breakers_open.get("node1").unwrap());
        assert!(!*metrics.circuit_breakers_open.get("node2").unwrap());
    }

    #[test]
    fn test_backend_errors_tracking() {
        let mut metrics = GatewayMetrics::new();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            0,
            0,
            false,
        );
        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            0,
            0,
            false,
        );
        metrics.record_op(MetricProtocol::Nfs3, MetricOperation::Read, 100, 0, 0, true);

        assert_eq!(metrics.backend_errors_total, 2);
    }
}
