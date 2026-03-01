//! Connection health monitoring for ClaudeFS transport layer.
//!
//! This module provides health checking, latency tracking, and connection
//! failure detection for RPC connections.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STATUS_UNKNOWN: u8 = 0;
const STATUS_HEALTHY: u8 = 1;
const STATUS_DEGRADED: u8 = 2;
const STATUS_UNHEALTHY: u8 = 3;

/// Health status of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthStatus {
    /// Connection is healthy and operational.
    Healthy,
    /// Connection is degraded (high latency or occasional failures).
    Degraded,
    /// Connection is unhealthy and should be replaced.
    Unhealthy,
    /// Connection state is unknown (not yet measured).
    #[default]
    Unknown,
}

impl From<u8> for HealthStatus {
    fn from(raw: u8) -> Self {
        match raw {
            STATUS_HEALTHY => HealthStatus::Healthy,
            STATUS_DEGRADED => HealthStatus::Degraded,
            STATUS_UNHEALTHY => HealthStatus::Unhealthy,
            _ => HealthStatus::Unknown,
        }
    }
}

impl From<HealthStatus> for u8 {
    fn from(status: HealthStatus) -> Self {
        match status {
            HealthStatus::Healthy => STATUS_HEALTHY,
            HealthStatus::Degraded => STATUS_DEGRADED,
            HealthStatus::Unhealthy => STATUS_UNHEALTHY,
            HealthStatus::Unknown => STATUS_UNKNOWN,
        }
    }
}

/// Configuration for health monitoring.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// Interval between health checks (default: 5 seconds).
    pub check_interval: Duration,
    /// Timeout for a single health check ping (default: 3 seconds).
    pub ping_timeout: Duration,
    /// Number of consecutive failures before marking unhealthy (default: 3).
    pub failure_threshold: u32,
    /// Number of consecutive successes to recover (default: 2).
    pub recovery_threshold: u32,
    /// Latency threshold for degraded status in ms (default: 100ms).
    pub latency_threshold_ms: u64,
    /// Maximum acceptable packet loss ratio (default: 0.1).
    pub max_packet_loss: f64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(5),
            ping_timeout: Duration::from_secs(3),
            failure_threshold: 3,
            recovery_threshold: 2,
            latency_threshold_ms: 100,
            max_packet_loss: 0.1,
        }
    }
}

/// Statistics collected by health monitor.
#[derive(Debug, Clone, Default)]
pub struct HealthStats {
    /// Current health status.
    pub status: HealthStatus,
    /// Current latency in milliseconds.
    pub latency_ms: u64,
    /// Number of successful health checks.
    pub success_count: u64,
    /// Number of failed health checks.
    pub failure_count: u64,
    /// Number of consecutive failures.
    pub consecutive_failures: u32,
    /// Number of consecutive successes.
    pub consecutive_successes: u32,
    /// Average latency over recent checks (ms).
    pub avg_latency_ms: u64,
    /// Minimum observed latency (ms).
    pub min_latency_ms: u64,
    /// Maximum observed latency (ms).
    pub max_latency_ms: u64,
    /// Packet loss ratio (0.0 to 1.0).
    pub packet_loss_ratio: f64,
    /// Timestamp of last successful check (secs since epoch).
    pub last_success_ts: u64,
    /// Timestamp of last check attempt (secs since epoch).
    pub last_check_ts: u64,
}

/// Connection health monitor.
///
/// Tracks connection health metrics including latency, failures, and packet loss.
/// Thread-safe - can be accessed from multiple coroutines.
pub struct ConnectionHealth {
    config: HealthConfig,
    status: AtomicU8,
    consecutive_failures: AtomicU32,
    consecutive_successes: AtomicU32,
    success_count: AtomicU64,
    failure_count: AtomicU64,
    total_latency_ms: AtomicU64,
    min_latency_ms: AtomicU64,
    max_latency_ms: AtomicU64,
    latency_sample_count: AtomicU64,
    last_success_ts: AtomicU64,
    last_check_ts: AtomicU64,
}

impl ConnectionHealth {
    /// Create a new health monitor with default configuration.
    pub fn new() -> Self {
        Self::with_config(HealthConfig::default())
    }

    /// Create a health monitor with custom configuration.
    pub fn with_config(config: HealthConfig) -> Self {
        Self {
            config,
            status: AtomicU8::new(STATUS_UNKNOWN),
            consecutive_failures: AtomicU32::new(0),
            consecutive_successes: AtomicU32::new(0),
            success_count: AtomicU64::new(0),
            failure_count: AtomicU64::new(0),
            total_latency_ms: AtomicU64::new(0),
            min_latency_ms: AtomicU64::new(u64::MAX),
            max_latency_ms: AtomicU64::new(0),
            latency_sample_count: AtomicU64::new(0),
            last_success_ts: AtomicU64::new(0),
            last_check_ts: AtomicU64::new(0),
        }
    }

    fn get_current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Record a successful health check with the given latency.
    pub fn record_success(&self, latency: Duration) {
        let latency_ms = latency.as_millis() as u64;
        let now = Self::get_current_timestamp();

        self.success_count.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.consecutive_successes.fetch_add(1, Ordering::Relaxed);

        self.total_latency_ms.fetch_add(latency_ms, Ordering::Relaxed);
        self.latency_sample_count.fetch_add(1, Ordering::Relaxed);

        let current_min = self.min_latency_ms.load(Ordering::Relaxed);
        if latency_ms < current_min {
            self.min_latency_ms.store(latency_ms, Ordering::Relaxed);
        }

        let current_max = self.max_latency_ms.load(Ordering::Relaxed);
        if latency_ms > current_max {
            self.max_latency_ms.store(latency_ms, Ordering::Relaxed);
        }

        self.last_success_ts.store(now, Ordering::Relaxed);
        self.last_check_ts.store(now, Ordering::Relaxed);

        self.update_status();
    }

    /// Record a failed health check.
    pub fn record_failure(&self) {
        let now = Self::get_current_timestamp();

        self.failure_count.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        self.consecutive_successes.store(0, Ordering::Relaxed);

        self.last_check_ts.store(now, Ordering::Relaxed);

        self.update_status();
    }

    /// Update health status based on current metrics.
    fn update_status(&self) {
        let failures = self.consecutive_failures.load(Ordering::Relaxed);
        let successes = self.consecutive_successes.load(Ordering::Relaxed);
        let current_status: HealthStatus = self.status.load(Ordering::Relaxed).into();

        let new_status: HealthStatus = if failures >= self.config.failure_threshold {
            HealthStatus::Unhealthy
        } else if failures > 0 || self.is_latency_degraded() {
            HealthStatus::Degraded
        } else if successes >= self.config.recovery_threshold
            || (current_status == HealthStatus::Unknown && successes > 0 && !self.is_latency_degraded())
            || current_status == HealthStatus::Healthy
        {
            HealthStatus::Healthy
        } else if successes > 0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Unknown
        };

        self.status.store(new_status.into(), Ordering::Relaxed);
    }

    /// Check if current latency is in degraded range.
    fn is_latency_degraded(&self) -> bool {
        let avg = self.get_avg_latency();
        avg > self.config.latency_threshold_ms
    }

    /// Get current health status.
    pub fn status(&self) -> HealthStatus {
        self.status.load(Ordering::Relaxed).into()
    }

    /// Get current latency in milliseconds.
    pub fn latency_ms(&self) -> u64 {
        self.get_avg_latency()
    }

    /// Get average latency in milliseconds.
    pub fn get_avg_latency(&self) -> u64 {
        let total = self.total_latency_ms.load(Ordering::Relaxed);
        let count = self.latency_sample_count.load(Ordering::Relaxed);
        if count > 0 {
            total / count
        } else {
            0
        }
    }

    /// Get minimum latency in milliseconds.
    pub fn min_latency_ms(&self) -> u64 {
        let min = self.min_latency_ms.load(Ordering::Relaxed);
        if min == u64::MAX {
            0
        } else {
            min
        }
    }

    /// Get maximum latency in milliseconds.
    pub fn max_latency_ms(&self) -> u64 {
        self.max_latency_ms.load(Ordering::Relaxed)
    }

    /// Get number of consecutive failures.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }

    /// Get number of consecutive successes.
    pub fn consecutive_successes(&self) -> u32 {
        self.consecutive_successes.load(Ordering::Relaxed)
    }

    /// Get total success count.
    pub fn success_count(&self) -> u64 {
        self.success_count.load(Ordering::Relaxed)
    }

    /// Get total failure count.
    pub fn failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::Relaxed)
    }

    /// Get packet loss ratio (0.0 to 1.0).
    pub fn packet_loss_ratio(&self) -> f64 {
        let success = self.success_count.load(Ordering::Relaxed);
        let failure = self.failure_count.load(Ordering::Relaxed);
        let total = success + failure;
        if total == 0 {
            0.0
        } else {
            failure as f64 / total as f64
        }
    }

    /// Check if connection should be considered failed.
    pub fn is_failed(&self) -> bool {
        self.status() == HealthStatus::Unhealthy || self.packet_loss_ratio() > self.config.max_packet_loss
    }

    /// Get full health statistics.
    pub fn stats(&self) -> HealthStats {
        HealthStats {
            status: self.status(),
            latency_ms: self.latency_ms(),
            success_count: self.success_count(),
            failure_count: self.failure_count(),
            consecutive_failures: self.consecutive_failures(),
            consecutive_successes: self.consecutive_successes(),
            avg_latency_ms: self.get_avg_latency(),
            min_latency_ms: self.min_latency_ms(),
            max_latency_ms: self.max_latency_ms(),
            packet_loss_ratio: self.packet_loss_ratio(),
            last_success_ts: self.last_success_ts.load(Ordering::Relaxed),
            last_check_ts: self.last_check_ts.load(Ordering::Relaxed),
        }
    }

    /// Reset health metrics.
    pub fn reset(&self) {
        self.status.store(STATUS_UNKNOWN, Ordering::Relaxed);
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.consecutive_successes.store(0, Ordering::Relaxed);
    }

    /// Get the configuration.
    pub fn config(&self) -> &HealthConfig {
        &self.config
    }
}

impl Default for ConnectionHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ConnectionHealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionHealth")
            .field("status", &self.status())
            .field("consecutive_failures", &self.consecutive_failures())
            .field("consecutive_successes", &self.consecutive_successes())
            .field("latency_ms", &self.latency_ms())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_config_default() {
        let config = HealthConfig::default();
        assert_eq!(config.check_interval, Duration::from_secs(5));
        assert_eq!(config.ping_timeout, Duration::from_secs(3));
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.recovery_threshold, 2);
        assert_eq!(config.latency_threshold_ms, 100);
    }

    #[test]
    fn test_health_status_default() {
        let status = HealthStatus::default();
        assert_eq!(status, HealthStatus::Unknown);
    }

    #[test]
    fn test_health_status_conversion() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::from(STATUS_HEALTHY));
        assert_eq!(HealthStatus::Degraded, HealthStatus::from(STATUS_DEGRADED));
        assert_eq!(HealthStatus::Unhealthy, HealthStatus::from(STATUS_UNHEALTHY));
        assert_eq!(HealthStatus::Unknown, HealthStatus::from(STATUS_UNKNOWN));
        assert_eq!(u8::from(HealthStatus::Healthy), STATUS_HEALTHY);
        assert_eq!(u8::from(HealthStatus::Degraded), STATUS_DEGRADED);
        assert_eq!(u8::from(HealthStatus::Unhealthy), STATUS_UNHEALTHY);
        assert_eq!(u8::from(HealthStatus::Unknown), STATUS_UNKNOWN);
    }

    #[test]
    fn test_connection_health_new() {
        let health = ConnectionHealth::new();
        assert_eq!(health.status(), HealthStatus::Unknown);
        assert_eq!(health.consecutive_failures(), 0);
    }

    #[test]
    fn test_record_success() {
        let health = ConnectionHealth::new();

        health.record_success(Duration::from_millis(50));
        assert_eq!(health.success_count(), 1);
        assert_eq!(health.consecutive_failures(), 0);
        assert!(health.consecutive_successes() > 0);
    }

    #[test]
    fn test_record_failure() {
        let health = ConnectionHealth::new();

        health.record_failure();
        assert_eq!(health.failure_count(), 1);
        assert_eq!(health.consecutive_failures(), 1);
    }

    #[test]
    fn test_latency_tracking() {
        let health = ConnectionHealth::new();

        health.record_success(Duration::from_millis(10));
        health.record_success(Duration::from_millis(20));
        health.record_success(Duration::from_millis(30));

        assert_eq!(health.min_latency_ms(), 10);
        assert_eq!(health.max_latency_ms(), 30);
        assert_eq!(health.get_avg_latency(), 20);
    }

    #[test]
    fn test_packet_loss_ratio() {
        let health = ConnectionHealth::new();

        for _ in 0..9 {
            health.record_success(Duration::from_millis(10));
        }
        health.record_failure();

        let ratio = health.packet_loss_ratio();
        assert!(ratio > 0.09 && ratio < 0.11);
    }

    #[test]
    fn test_unhealthy_threshold() {
        let config = HealthConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let health = ConnectionHealth::with_config(config);

        for _ in 0..3 {
            health.record_failure();
        }

        assert_eq!(health.status(), HealthStatus::Unhealthy);
        assert!(health.is_failed());
    }

    #[test]
    fn test_recovery_from_unhealthy() {
        let config = HealthConfig {
            failure_threshold: 2,
            recovery_threshold: 2,
            ..Default::default()
        };
        let health = ConnectionHealth::with_config(config);

        health.record_failure();
        health.record_failure();
        assert_eq!(health.status(), HealthStatus::Unhealthy);

        health.record_success(Duration::from_millis(10));
        assert_eq!(health.status(), HealthStatus::Degraded);

        health.record_success(Duration::from_millis(10));
        assert_eq!(health.status(), HealthStatus::Healthy);
    }

    #[test]
    fn test_latency_degraded() {
        let config = HealthConfig {
            latency_threshold_ms: 50,
            ..Default::default()
        };
        let health = ConnectionHealth::with_config(config);

        health.record_success(Duration::from_millis(10));
        assert_eq!(health.status(), HealthStatus::Healthy);

        health.record_success(Duration::from_millis(100));
        assert_eq!(health.status(), HealthStatus::Degraded);
    }

    #[test]
    fn test_reset() {
        let health = ConnectionHealth::new();

        health.record_failure();
        health.record_failure();
        assert_eq!(health.consecutive_failures(), 2);

        health.reset();
        assert_eq!(health.status(), HealthStatus::Unknown);
        assert_eq!(health.consecutive_failures(), 0);
    }

    #[test]
    fn test_stats() {
        let health = ConnectionHealth::new();

        health.record_success(Duration::from_millis(25));
        health.record_success(Duration::from_millis(35));

        let stats = health.stats();
        assert_eq!(stats.status, HealthStatus::Healthy);
        assert_eq!(stats.success_count, 2);
        assert_eq!(stats.avg_latency_ms, 30);
        assert_eq!(stats.min_latency_ms, 25);
        assert_eq!(stats.max_latency_ms, 35);
        assert_eq!(stats.packet_loss_ratio, 0.0);
    }

    #[tokio::test]
    async fn test_concurrent_updates() {
        use std::sync::Arc;

        let health = Arc::new(ConnectionHealth::new());

        let health1 = Arc::clone(&health);
        let handle1 = tokio::spawn(async move {
            for _ in 0..100 {
                health1.record_success(Duration::from_millis(10));
            }
        });

        let health2 = Arc::clone(&health);
        let handle2 = tokio::spawn(async move {
            for _ in 0..100 {
                health2.record_failure();
            }
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        assert_eq!(health.success_count(), 100);
        assert_eq!(health.failure_count(), 100);
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_latency_tracking_random(latency_ms in 1u64..10000) {
            let health = ConnectionHealth::new();
            health.record_success(Duration::from_millis(latency_ms));
            prop_assert_eq!(health.min_latency_ms(), latency_ms);
            prop_assert_eq!(health.max_latency_ms(), latency_ms);
            prop_assert_eq!(health.get_avg_latency(), latency_ms);
        }

        #[test]
        fn test_packet_loss_ratio_random(
            successes in 0u64..1000,
            failures in 0u64..1000,
        ) {
            let health = ConnectionHealth::new();
            for _ in 0..successes {
                health.record_success(Duration::from_millis(10));
            }
            for _ in 0..failures {
                health.record_failure();
            }

            let total = successes + failures;
            if total > 0 {
                let expected_ratio = failures as f64 / total as f64;
                let actual_ratio = health.packet_loss_ratio();
                prop_assert!((expected_ratio - actual_ratio).abs() < 0.0001);
            } else {
                prop_assert_eq!(health.packet_loss_ratio(), 0.0);
            }
        }

        #[test]
        fn test_consecutive_failures_count(count in 0u32..10) {
            let health = ConnectionHealth::new();
            for _ in 0..count {
                health.record_failure();
            }
            prop_assert_eq!(health.consecutive_failures(), count);
        }
    }
}