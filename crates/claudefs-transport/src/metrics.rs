//! Transport layer metrics collection.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
/// Snapshot of transport metrics at a point in time.
pub struct MetricsSnapshot {
    /// Number of requests sent.
    pub requests_sent: u64,
    /// Number of requests received.
    pub requests_received: u64,
    /// Number of responses sent.
    pub responses_sent: u64,
    /// Number of responses received.
    pub responses_received: u64,
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Total number of errors.
    pub errors_total: u64,
    /// Total number of retries.
    pub retries_total: u64,
    /// Total number of timeouts.
    pub timeouts_total: u64,
    /// Number of connections opened.
    pub connections_opened: u64,
    /// Number of connections closed.
    pub connections_closed: u64,
    /// Number of currently active connections.
    pub active_connections: u32,
    /// Total number of health checks performed.
    pub health_checks_total: u64,
    /// Total number of health checks that failed.
    pub health_checks_failed: u64,
}

/// Thread-safe transport layer metrics collector.
pub struct TransportMetrics {
    requests_sent: AtomicU64,
    requests_received: AtomicU64,
    responses_sent: AtomicU64,
    responses_received: AtomicU64,
    bytes_sent: AtomicU64,
    bytes_received: AtomicU64,
    errors_total: AtomicU64,
    retries_total: AtomicU64,
    timeouts_total: AtomicU64,
    connections_opened: AtomicU64,
    connections_closed: AtomicU64,
    active_connections: AtomicU32,
    health_checks_total: AtomicU64,
    health_checks_failed: AtomicU64,
}

impl Default for TransportMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TransportMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportMetrics")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl TransportMetrics {
    /// Creates a new TransportMetrics instance with all counters initialized to zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            requests_sent: AtomicU64::new(0),
            requests_received: AtomicU64::new(0),
            responses_sent: AtomicU64::new(0),
            responses_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
            retries_total: AtomicU64::new(0),
            timeouts_total: AtomicU64::new(0),
            connections_opened: AtomicU64::new(0),
            connections_closed: AtomicU64::new(0),
            active_connections: AtomicU32::new(0),
            health_checks_total: AtomicU64::new(0),
            health_checks_failed: AtomicU64::new(0),
        }
    }

    /// Increments the requests sent counter.
    pub fn inc_requests_sent(&self) {
        self.requests_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the requests received counter.
    pub fn inc_requests_received(&self) {
        self.requests_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the responses sent counter.
    pub fn inc_responses_sent(&self) {
        self.responses_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the responses received counter.
    pub fn inc_responses_received(&self) {
        self.responses_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Adds to the bytes sent counter.
    pub fn add_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Adds to the bytes received counter.
    pub fn add_bytes_received(&self, bytes: u64) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Increments the errors total counter.
    pub fn inc_errors_total(&self) {
        self.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the retries total counter.
    pub fn inc_retries_total(&self) {
        self.retries_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the timeouts total counter.
    pub fn inc_timeouts_total(&self) {
        self.timeouts_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the connections opened counter.
    pub fn inc_connections_opened(&self) {
        self.connections_opened.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the connections closed counter.
    pub fn inc_connections_closed(&self) {
        self.connections_closed.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a new connection opened, incrementing both opened counter and active connections.
    pub fn connection_opened(&self) {
        self.inc_connections_opened();
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a connection closed, decrementing active connections.
    pub fn connection_closed(&self) {
        self.inc_connections_closed();
        let _ = self
            .active_connections
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                if v > 0 {
                    Some(v - 1)
                } else {
                    Some(0)
                }
            });
    }

    /// Increments the health checks total counter.
    pub fn inc_health_checks_total(&self) {
        self.health_checks_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the health checks failed counter.
    pub fn inc_health_checks_failed(&self) {
        self.health_checks_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Takes a snapshot of all current metric values.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_sent: self.requests_sent.load(Ordering::Relaxed),
            requests_received: self.requests_received.load(Ordering::Relaxed),
            responses_sent: self.responses_sent.load(Ordering::Relaxed),
            responses_received: self.responses_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
            retries_total: self.retries_total.load(Ordering::Relaxed),
            timeouts_total: self.timeouts_total.load(Ordering::Relaxed),
            connections_opened: self.connections_opened.load(Ordering::Relaxed),
            connections_closed: self.connections_closed.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            health_checks_total: self.health_checks_total.load(Ordering::Relaxed),
            health_checks_failed: self.health_checks_failed.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::task::JoinSet;

    #[test]
    fn test_metrics_new() {
        let metrics = TransportMetrics::new();
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.requests_sent, 0);
        assert_eq!(snapshot.requests_received, 0);
        assert_eq!(snapshot.responses_sent, 0);
        assert_eq!(snapshot.responses_received, 0);
        assert_eq!(snapshot.bytes_sent, 0);
        assert_eq!(snapshot.bytes_received, 0);
        assert_eq!(snapshot.errors_total, 0);
        assert_eq!(snapshot.retries_total, 0);
        assert_eq!(snapshot.timeouts_total, 0);
        assert_eq!(snapshot.connections_opened, 0);
        assert_eq!(snapshot.connections_closed, 0);
        assert_eq!(snapshot.active_connections, 0);
        assert_eq!(snapshot.health_checks_total, 0);
        assert_eq!(snapshot.health_checks_failed, 0);
    }

    #[test]
    fn test_inc_counters() {
        let metrics = TransportMetrics::new();

        metrics.inc_requests_sent();
        metrics.inc_requests_received();
        metrics.inc_responses_sent();
        metrics.inc_responses_received();
        metrics.inc_errors_total();
        metrics.inc_retries_total();
        metrics.inc_timeouts_total();
        metrics.inc_connections_opened();
        metrics.inc_connections_closed();
        metrics.inc_health_checks_total();
        metrics.inc_health_checks_failed();

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.requests_sent, 1);
        assert_eq!(snapshot.requests_received, 1);
        assert_eq!(snapshot.responses_sent, 1);
        assert_eq!(snapshot.responses_received, 1);
        assert_eq!(snapshot.errors_total, 1);
        assert_eq!(snapshot.retries_total, 1);
        assert_eq!(snapshot.timeouts_total, 1);
        assert_eq!(snapshot.connections_opened, 1);
        assert_eq!(snapshot.connections_closed, 1);
        assert_eq!(snapshot.health_checks_total, 1);
        assert_eq!(snapshot.health_checks_failed, 1);
    }

    #[test]
    fn test_bytes_tracking() {
        let metrics = TransportMetrics::new();

        metrics.add_bytes_sent(1024);
        metrics.add_bytes_received(2048);
        metrics.add_bytes_sent(512);

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.bytes_sent, 1536);
        assert_eq!(snapshot.bytes_received, 2048);
    }

    #[test]
    fn test_connection_tracking() {
        let metrics = TransportMetrics::new();

        metrics.connection_opened();
        metrics.connection_opened();
        metrics.connection_opened();

        assert_eq!(metrics.snapshot().active_connections, 3);
        assert_eq!(metrics.snapshot().connections_opened, 3);

        metrics.connection_closed();

        assert_eq!(metrics.snapshot().active_connections, 2);
        assert_eq!(metrics.snapshot().connections_closed, 1);
    }

    #[test]
    fn test_connection_close_saturating() {
        let metrics = TransportMetrics::new();

        metrics.connection_closed();
        metrics.connection_closed();
        metrics.connection_closed();

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.active_connections, 0);
        assert_eq!(snapshot.connections_closed, 3);
    }

    #[test]
    fn test_snapshot_is_consistent() {
        let metrics = TransportMetrics::new();

        metrics.inc_requests_sent();
        metrics.inc_requests_received();
        metrics.add_bytes_sent(100);
        metrics.add_bytes_received(200);
        metrics.inc_errors_total();
        metrics.connection_opened();
        metrics.inc_health_checks_total();
        metrics.inc_health_checks_failed();

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.requests_sent, metrics.requests_sent.load(Ordering::Relaxed));
        assert_eq!(snapshot.requests_received, metrics.requests_received.load(Ordering::Relaxed));
        assert_eq!(snapshot.bytes_sent, metrics.bytes_sent.load(Ordering::Relaxed));
        assert_eq!(snapshot.bytes_received, metrics.bytes_received.load(Ordering::Relaxed));
        assert_eq!(snapshot.errors_total, metrics.errors_total.load(Ordering::Relaxed));
        assert_eq!(snapshot.active_connections, metrics.active_connections.load(Ordering::Relaxed));
        assert_eq!(snapshot.connections_opened, metrics.connections_opened.load(Ordering::Relaxed));
        assert_eq!(snapshot.connections_closed, metrics.connections_closed.load(Ordering::Relaxed));
        assert_eq!(snapshot.health_checks_total, metrics.health_checks_total.load(Ordering::Relaxed));
        assert_eq!(snapshot.health_checks_failed, metrics.health_checks_failed.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_concurrent_metrics() {
        let metrics = Arc::new(TransportMetrics::new());
        let num_tasks = 10;
        let increments_per_task = 100;

        let mut join_set = JoinSet::new();

        for _ in 0..num_tasks {
            let metrics = Arc::clone(&metrics);
            join_set.spawn(async move {
                for _ in 0..increments_per_task {
                    metrics.inc_requests_sent();
                    metrics.inc_requests_received();
                    metrics.add_bytes_sent(10);
                    metrics.add_bytes_received(20);
                }
            });
        }

        while join_set.join_next().await.is_some() {}

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.requests_sent, num_tasks as u64 * increments_per_task as u64);
        assert_eq!(snapshot.requests_received, num_tasks as u64 * increments_per_task as u64);
        assert_eq!(snapshot.bytes_sent, num_tasks as u64 * increments_per_task as u64 * 10);
        assert_eq!(snapshot.bytes_received, num_tasks as u64 * increments_per_task as u64 * 20);
    }
}