//! High-level TransportClient that composes transport modules into a unified RPC client.
//!
//! This module provides a `TransportClient` that integrates:
//! - Connection health monitoring
//! - Circuit breaker for fault tolerance
//! - Retry logic with exponential backoff
//! - Flow control for backpressure
//! - Metrics collection
//! - Deadline propagation

use std::time::Duration;

use crate::circuitbreaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
use crate::error::{Result, TransportError};
use crate::flowcontrol::{FlowControlConfig, FlowControlState, FlowController};
use crate::health::ConnectionHealth;
use crate::metrics::TransportMetrics;
use crate::retry::{RetryConfig, RetryExecutor};

/// Configuration for the TransportClient.
///
/// Composes sub-configurations for retry, circuit breaker, and flow control.
#[derive(Debug, Clone, Default)]
pub struct TransportClientConfig {
    /// Retry configuration.
    pub retry: RetryConfig,
    /// Circuit breaker configuration.
    pub circuit_breaker: CircuitBreakerConfig,
    /// Flow control configuration.
    pub flow_control: FlowControlConfig,
}

/// High-level transport client that composes health, circuit breaker, retry, flow control, and metrics.
///
/// This is a coordination layer that provides a unified interface for RPC communication.
/// Actual sending of frames is handled elsewhere.
pub struct TransportClient {
    #[allow(dead_code)]
    config: TransportClientConfig,
    health: ConnectionHealth,
    circuit_breaker: CircuitBreaker,
    #[allow(dead_code)]
    retry_executor: RetryExecutor,
    flow_controller: FlowController,
    metrics: TransportMetrics,
}

impl TransportClient {
    /// Creates a new TransportClient with the given configuration.
    pub fn new(config: TransportClientConfig) -> Self {
        Self {
            config: config.clone(),
            health: ConnectionHealth::new(),
            circuit_breaker: CircuitBreaker::new(config.circuit_breaker.clone()),
            retry_executor: RetryExecutor::new(config.retry.clone()),
            flow_controller: FlowController::new(config.flow_control.clone()),
            metrics: TransportMetrics::new(),
        }
    }

    /// Returns a reference to the health monitor.
    pub fn health(&self) -> &ConnectionHealth {
        &self.health
    }

    /// Returns a reference to the circuit breaker.
    pub fn circuit_breaker(&self) -> &CircuitBreaker {
        &self.circuit_breaker
    }

    /// Returns a reference to the metrics collector.
    pub fn metrics(&self) -> &TransportMetrics {
        &self.metrics
    }

    /// Returns a reference to the flow controller.
    pub fn flow_controller(&self) -> &FlowController {
        &self.flow_controller
    }

    /// Returns whether the client is available to handle requests.
    ///
    /// A client is available when:
    /// - Circuit breaker is not open
    /// - Flow controller is not blocked
    /// - Health is not failed
    pub fn is_available(&self) -> bool {
        let circuit_ok = self.circuit_breaker.state() != CircuitState::Open;
        let flow_ok = self.flow_controller.state() != FlowControlState::Blocked;
        let health_ok = !self.health.is_failed();
        circuit_ok && flow_ok && health_ok
    }

    /// Checks availability and prepares for a request.
    ///
    /// Returns `Ok(())` if the request can proceed, or `Err(TransportError::NotConnected)` if:
    /// - Circuit breaker is open
    /// - Flow controller is blocked
    pub fn pre_request(&self) -> Result<()> {
        if !self.circuit_breaker.can_execute() {
            return Err(TransportError::NotConnected);
        }

        if self.flow_controller.state() == FlowControlState::Blocked {
            return Err(TransportError::NotConnected);
        }

        self.metrics.inc_requests_sent();
        Ok(())
    }

    /// Records a successful request.
    ///
    /// Updates health, circuit breaker, and metrics with the request latency.
    pub fn post_request_success(&self, latency: Duration) {
        self.health.record_success(latency);
        self.circuit_breaker.record_success();
    }

    /// Records a failed request.
    ///
    /// Updates health, circuit breaker, and metrics to reflect the failure.
    pub fn post_request_failure(&self) {
        self.health.record_failure();
        self.circuit_breaker.record_failure();
        self.metrics.inc_errors_total();
    }
}

impl Default for TransportClient {
    fn default() -> Self {
        Self::new(TransportClientConfig::default())
    }
}

impl std::fmt::Debug for TransportClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportClient")
            .field("health", &self.health)
            .field("circuit_breaker", &self.circuit_breaker.state())
            .field("flow_controller", &self.flow_controller.state())
            .field("metrics", &self.metrics.snapshot())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthStatus;
    use std::time::Duration;

    #[test]
    fn test_transport_client_config_default() {
        let config = TransportClientConfig::default();
        assert_eq!(config.retry.max_retries, 3);
        assert_eq!(config.circuit_breaker.failure_threshold, 5);
        assert_eq!(config.flow_control.max_inflight_requests, 1024);
    }

    #[test]
    fn test_client_is_available() {
        let client = TransportClient::default();
        assert!(client.is_available());
    }

    #[test]
    fn test_client_pre_request() {
        let client = TransportClient::default();
        client.pre_request().expect("pre_request should succeed");
        assert_eq!(client.metrics().snapshot().requests_sent, 1);
    }

    #[test]
    fn test_client_post_success() {
        let client = TransportClient::default();

        client.pre_request().expect("pre_request should succeed");
        client.post_request_success(Duration::from_millis(50));

        assert_eq!(client.health().status(), HealthStatus::Healthy);
        assert_eq!(client.health().success_count(), 1);
    }

    #[test]
    fn test_client_post_failure() {
        let client = TransportClient::default();

        client.pre_request().expect("pre_request should succeed");
        client.post_request_failure();

        assert_eq!(client.circuit_breaker().failure_count(), 1);
        assert!(!client.is_available());
    }

    #[test]
    fn test_client_circuit_breaker_integration() {
        let config = TransportClientConfig {
            circuit_breaker: CircuitBreakerConfig {
                failure_threshold: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        let client = TransportClient::new(config);

        assert!(client.is_available());

        for _ in 0..3 {
            client.pre_request().expect("pre_request should succeed");
            client.post_request_failure();
        }

        assert!(!client.is_available());

        let result = client.pre_request();
        assert!(result.is_err());
    }

    #[test]
    fn test_client_metrics_tracking() {
        let client = TransportClient::default();

        client.pre_request().expect("pre_request should succeed");
        client.post_request_success(Duration::from_millis(10));

        let snapshot = client.metrics().snapshot();
        assert_eq!(snapshot.requests_sent, 1);

        client.pre_request().expect("pre_request should succeed");
        client.post_request_failure();

        let snapshot = client.metrics().snapshot();
        assert_eq!(snapshot.requests_sent, 2);
        assert_eq!(snapshot.errors_total, 1);
    }

    #[test]
    fn test_client_accessors() {
        let client = TransportClient::default();

        let _health = client.health();
        let _circuit_breaker = client.circuit_breaker();
        let _metrics = client.metrics();
        let _flow_controller = client.flow_controller();
    }

    #[test]
    fn test_client_flow_control_blocked() {
        let config = TransportClientConfig {
            flow_control: FlowControlConfig {
                max_inflight_requests: 1,
                max_inflight_bytes: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let client = TransportClient::new(config);

        let _permit = client
            .flow_controller()
            .try_acquire(1)
            .expect("should get permit");

        assert!(!client.is_available());

        let result = client.pre_request();
        assert!(result.is_err());
    }

    #[test]
    fn test_client_health_failed() {
        let config = TransportClientConfig {
            circuit_breaker: CircuitBreakerConfig {
                failure_threshold: 10,
                ..Default::default()
            },
            ..Default::default()
        };

        let client = TransportClient::new(config);

        for _ in 0..3 {
            client.pre_request().expect("pre_request should succeed");
            client.post_request_failure();
        }

        assert!(!client.is_available());
    }

    #[test]
    fn test_client_debug_format() {
        let client = TransportClient::default();
        let debug_str = format!("{:?}", client);
        assert!(debug_str.contains("TransportClient"));
    }
}
