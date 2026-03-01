//! Connection pool module for health-aware, load-balanced connection pooling across cluster nodes.
//!
//! This module provides a connection pool that tracks endpoint health, manages idle connections,
//! and performs load balancing based on endpoint health status.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tracing::debug;

use crate::circuitbreaker::{CircuitBreaker, CircuitBreakerConfig};
use crate::health::{ConnectionHealth, HealthStatus};
use crate::metrics::TransportMetrics;
use crate::transport::Connection;

/// Configuration for the connection pool.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections per endpoint.
    pub max_connections_per_endpoint: usize,
    /// Minimum number of idle connections to maintain per endpoint.
    pub min_idle_per_endpoint: usize,
    /// Idle timeout for connections.
    pub idle_timeout: Duration,
    /// Interval between health checks.
    pub health_check_interval: Duration,
    /// Maximum total connections across all endpoints.
    pub max_total_connections: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections_per_endpoint: 4,
            min_idle_per_endpoint: 1,
            idle_timeout: Duration::from_secs(300),
            health_check_interval: Duration::from_secs(30),
            max_total_connections: 256,
        }
    }
}

/// Statistics about the connection pool.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total number of connections in the pool.
    pub total_connections: usize,
    /// Total number of idle connections.
    pub total_idle: usize,
    /// Total number of active (checked out) connections.
    pub total_active: usize,
    /// Number of registered endpoints.
    pub endpoints: usize,
    /// Number of healthy endpoints.
    pub healthy_endpoints: usize,
}

/// Internal state for a single endpoint.
struct EndpointState {
    /// Idle connections available for checkout.
    connections: Vec<PooledConn>,
    /// Health monitoring for this endpoint.
    health: ConnectionHealth,
    /// Circuit breaker for this endpoint.
    circuit_breaker: CircuitBreaker,
    /// Number of currently checked out connections.
    active_count: usize,
}

/// A pooled connection with metadata.
struct PooledConn {
    /// The underlying connection.
    conn: Box<dyn Connection>,
    /// When this connection was created.
    #[allow(dead_code)]
    created_at: Instant,
    /// When this connection was last used.
    #[allow(dead_code)]
    last_used: Instant,
}

/// A connection pool with health-aware, load-balanced connection distribution.
///
/// The pool manages connections to multiple endpoints, tracks their health,
/// and provides load balancing based on endpoint status.
pub struct ConnectionPool {
    /// Pool configuration.
    config: PoolConfig,
    /// Per-endpoint state.
    endpoints: Mutex<HashMap<String, EndpointState>>,
    /// Transport metrics for the pool.
    #[allow(dead_code)]
    metrics: TransportMetrics,
    /// Total connection count (across all endpoints).
    total_connections: AtomicUsize,
}

impl ConnectionPool {
    /// Creates a new connection pool with the given configuration.
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            endpoints: Mutex::new(HashMap::new()),
            metrics: TransportMetrics::new(),
            total_connections: AtomicUsize::new(0),
        }
    }

    /// Pre-registers an endpoint with empty connections.
    ///
    /// This allows the pool to track the endpoint even before any connections are created.
    pub fn add_endpoint(&self, addr: &str) {
        let mut endpoints = self.endpoints.lock().unwrap();
        if !endpoints.contains_key(addr) {
            endpoints.insert(
                addr.to_string(),
                EndpointState {
                    connections: Vec::new(),
                    health: ConnectionHealth::new(),
                    circuit_breaker: CircuitBreaker::new(CircuitBreakerConfig::default()),
                    active_count: 0,
                },
            );
            debug!("Added endpoint {} to connection pool", addr);
        }
    }

    /// Removes an endpoint from the pool.
    ///
    /// Returns the number of connections that were dropped.
    pub fn remove_endpoint(&self, addr: &str) -> usize {
        let mut endpoints = self.endpoints.lock().unwrap();
        if let Some(state) = endpoints.remove(addr) {
            let dropped = state.connections.len() + state.active_count;
            self.total_connections.fetch_sub(dropped, Ordering::Relaxed);
            debug!(
                "Removed endpoint {} from connection pool, dropped {} connections",
                addr, dropped
            );
            dropped
        } else {
            0
        }
    }

    /// Gets the health status of an endpoint.
    ///
    /// Returns `None` if the endpoint is not registered.
    pub fn endpoint_health(&self, addr: &str) -> Option<HealthStatus> {
        let endpoints = self.endpoints.lock().unwrap();
        endpoints.get(addr).map(|state| state.health.status())
    }

    /// Records a successful request to an endpoint.
    pub fn record_success(&self, addr: &str, latency: Duration) {
        let endpoints = self.endpoints.lock().unwrap();
        if let Some(state) = endpoints.get(addr) {
            state.health.record_success(latency);
            state.circuit_breaker.record_success();
        }
    }

    /// Records a failed request to an endpoint.
    pub fn record_failure(&self, addr: &str) {
        let endpoints = self.endpoints.lock().unwrap();
        if let Some(state) = endpoints.get(addr) {
            state.health.record_failure();
            state.circuit_breaker.record_failure();
        }
    }

    /// Gets pool-wide statistics.
    pub fn stats(&self) -> PoolStats {
        let endpoints = self.endpoints.lock().unwrap();
        let mut total_idle = 0;
        let mut total_active = 0;
        let mut healthy_endpoints = 0;

        for state in endpoints.values() {
            total_idle += state.connections.len();
            total_active += state.active_count;
            if state.health.status() == HealthStatus::Healthy {
                healthy_endpoints += 1;
            }
        }

        PoolStats {
            total_connections: self.total_connections.load(Ordering::Relaxed),
            total_idle,
            total_active,
            endpoints: endpoints.len(),
            healthy_endpoints,
        }
    }

    /// Returns the current total connection count.
    pub fn total_connections(&self) -> usize {
        self.total_connections.load(Ordering::Relaxed)
    }

    /// Selects the healthiest endpoint from the given candidates.
    ///
    /// Filters out endpoints with open circuit breakers.
    /// Among remaining endpoints, prefers those with the lowest failure count.
    /// Returns `None` if no endpoints are available.
    pub fn select_endpoint(&self, candidates: &[&str]) -> Option<String> {
        let endpoints = self.endpoints.lock().unwrap();

        let mut best_addr: Option<String> = None;
        let mut best_failure_count: Option<u64> = None;

        for &addr in candidates {
            if let Some(state) = endpoints.get(addr) {
                // Skip endpoints with open circuit breakers
                if !state.circuit_breaker.can_execute() {
                    continue;
                }

                let failure_count = state.health.failure_count();

                // Select endpoint with lowest failure count
                if best_failure_count.is_none() || failure_count < best_failure_count.unwrap() {
                    best_addr = Some(addr.to_string());
                    best_failure_count = Some(failure_count);
                }
            }
        }

        best_addr
    }

    /// Returns a connection to the idle pool for a given endpoint.
    ///
    /// The connection is added back to the pool if it's under the limit
    /// and the endpoint still exists.
    pub fn return_connection(&self, addr: &str, conn: Box<dyn Connection>) {
        let mut endpoints = self.endpoints.lock().unwrap();
        if let Some(state) = endpoints.get_mut(addr) {
            if state.connections.len() < self.config.max_connections_per_endpoint {
                state.connections.push(PooledConn {
                    conn,
                    created_at: Instant::now(),
                    last_used: Instant::now(),
                });
                state.active_count = state.active_count.saturating_sub(1);
            } else {
                // Pool is full, drop the connection
                self.total_connections.fetch_sub(1, Ordering::Relaxed);
            }
        } else {
            // Endpoint no longer exists, drop the connection
            self.total_connections.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Takes an idle connection from the pool for a given endpoint.
    ///
    /// Returns `None` if no idle connections are available.
    pub fn take_idle_connection(&self, addr: &str) -> Option<Box<dyn Connection>> {
        let mut endpoints = self.endpoints.lock().unwrap();
        if let Some(state) = endpoints.get_mut(addr) {
            if let Some(pooled) = state.connections.pop() {
                state.active_count += 1;
                return Some(pooled.conn);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections_per_endpoint, 4);
        assert_eq!(config.min_idle_per_endpoint, 1);
        assert_eq!(config.idle_timeout, Duration::from_secs(300));
        assert_eq!(config.health_check_interval, Duration::from_secs(30));
        assert_eq!(config.max_total_connections, 256);
    }

    #[test]
    fn test_pool_new_empty() {
        let pool = ConnectionPool::new(PoolConfig::default());
        let stats = pool.stats();
        assert_eq!(stats.total_connections, 0);
        assert_eq!(stats.endpoints, 0);
    }

    #[test]
    fn test_pool_add_endpoint() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        let stats = pool.stats();
        assert_eq!(stats.endpoints, 1);
    }

    #[test]
    fn test_pool_add_multiple_endpoints() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        pool.add_endpoint("192.168.1.2:9000");
        pool.add_endpoint("192.168.1.3:9000");
        let stats = pool.stats();
        assert_eq!(stats.endpoints, 3);
    }

    #[test]
    fn test_pool_remove_endpoint() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        let dropped = pool.remove_endpoint("192.168.1.1:9000");
        let stats = pool.stats();
        assert_eq!(dropped, 0);
        assert_eq!(stats.endpoints, 0);
    }

    #[test]
    fn test_pool_remove_nonexistent() {
        let pool = ConnectionPool::new(PoolConfig::default());
        let dropped = pool.remove_endpoint("192.168.1.1:9000");
        assert_eq!(dropped, 0);
    }

    #[test]
    fn test_pool_stats_empty() {
        let pool = ConnectionPool::new(PoolConfig::default());
        let stats = pool.stats();
        assert_eq!(stats.total_connections, 0);
        assert_eq!(stats.total_idle, 0);
        assert_eq!(stats.total_active, 0);
        assert_eq!(stats.endpoints, 0);
        assert_eq!(stats.healthy_endpoints, 0);
    }

    #[test]
    fn test_pool_total_connections_atomic() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        assert_eq!(pool.total_connections(), 0);
    }

    #[test]
    fn test_pool_endpoint_health_unknown() {
        let pool = ConnectionPool::new(PoolConfig::default());
        let health = pool.endpoint_health("192.168.1.1:9000");
        assert!(health.is_none());
    }

    #[test]
    fn test_pool_endpoint_health_registered() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        let health = pool.endpoint_health("192.168.1.1:9000");
        assert!(health.is_some());
        assert_eq!(health.unwrap(), HealthStatus::Unknown);
    }

    #[test]
    fn test_select_endpoint_empty_candidates() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        let selected = pool.select_endpoint(&[]);
        assert!(selected.is_none());
    }

    #[test]
    fn test_select_endpoint_prefers_healthy() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        pool.add_endpoint("192.168.1.2:9000");

        // Record failures on endpoint 1
        for _ in 0..3 {
            pool.record_failure("192.168.1.1:9000");
        }

        // Select should prefer endpoint 2 (no failures) over endpoint 1
        let candidates = vec!["192.168.1.1:9000", "192.168.1.2:9000"];
        let selected = pool.select_endpoint(&candidates);
        assert_eq!(selected, Some("192.168.1.2:9000".to_string()));
    }

    #[test]
    fn test_select_endpoint_skips_open_circuit() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        pool.add_endpoint("192.168.1.2:9000");

        // Record enough failures to open the circuit breaker on endpoint 1
        for _ in 0..5 {
            pool.record_failure("192.168.1.1:9000");
        }

        // Select should skip endpoint 1 due to open circuit breaker
        let candidates = vec!["192.168.1.1:9000", "192.168.1.2:9000"];
        let selected = pool.select_endpoint(&candidates);
        assert_eq!(selected, Some("192.168.1.2:9000".to_string()));
    }

    #[test]
    fn test_pool_record_success() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        pool.record_success("192.168.1.1:9000", Duration::from_millis(50));

        let health = pool.endpoint_health("192.168.1.1:9000");
        assert!(health.is_some());
    }

    #[test]
    fn test_pool_record_failure() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        pool.record_failure("192.168.1.1:9000");

        let health = pool.endpoint_health("192.168.1.1:9000");
        assert!(health.is_some());
    }

    #[test]
    fn test_pool_stats_format() {
        let pool = ConnectionPool::new(PoolConfig::default());
        pool.add_endpoint("192.168.1.1:9000");
        let stats = pool.stats();
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("PoolStats"));
    }
}
