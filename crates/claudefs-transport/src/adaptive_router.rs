//! Adaptive router for intelligent request routing.
//!
//! Routes requests based on endpoint health, latency percentiles, and load.
//! Uses score-based selection with automatic failover.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, trace};

use crate::wire_diag::RttSeries;

use crate::wire_diag::RttSample;

/// Endpoint identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EndpointId(pub u64);

impl EndpointId {
    /// Creates a new endpoint ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl Default for EndpointId {
    fn default() -> Self {
        Self(0)
    }
}

/// Metrics for a single endpoint.
#[derive(Debug, Clone)]
pub struct EndpointMetrics {
    pub endpoint_id: EndpointId,
    pub rtt_p50_us: u64,
    pub rtt_p99_us: u64,
    pub rtt_max_us: u64,
    pub availability: f64,
    pub queue_depth: usize,
    pub bytes_in_flight: u64,
    pub healthy: bool,
}

/// Routing decision containing primary and failover endpoints.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub primary_endpoint: EndpointId,
    pub failover_endpoints: Vec<EndpointId>,
    pub score: f64,
}

/// Routing policy configuration.
#[derive(Debug, Clone)]
pub struct RoutingPolicy {
    pub prefer_latency: bool,
    pub unhealthy_threshold: f64,
    pub queue_depth_weight: f64,
    pub rtt_weight: f64,
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        Self {
            prefer_latency: true,
            unhealthy_threshold: 0.5,
            queue_depth_weight: 0.3,
            rtt_weight: 0.7,
        }
    }
}

/// Adaptive router configuration.
#[derive(Debug, Clone)]
pub struct AdaptiveRouterConfig {
    pub max_endpoints: usize,
    pub rtt_percentile_window: usize,
    pub policy: RoutingPolicy,
}

impl Default for AdaptiveRouterConfig {
    fn default() -> Self {
        Self {
            max_endpoints: 256,
            rtt_percentile_window: 128,
            policy: RoutingPolicy::default(),
        }
    }
}

/// Internal state for an endpoint.
struct EndpointState {
    rtt_series: RttSeries,
    availability: f64,
    queue_depth: usize,
    bytes_in_flight: u64,
    healthy: bool,
    last_seen_ns: u64,
    consecutive_failures: AtomicUsize,
    total_requests: AtomicU64,
    successful_requests: AtomicU64,
}

impl EndpointState {
    fn new(window_size: usize) -> Self {
        Self {
            rtt_series: RttSeries::new(window_size),
            availability: 1.0,
            queue_depth: 0,
            bytes_in_flight: 0,
            healthy: true,
            last_seen_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            consecutive_failures: AtomicUsize::new(0),
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
        }
    }

    fn compute_score(&self, policy: &RoutingPolicy) -> f64 {
        if !self.healthy {
            return 0.0;
        }

        let max_rtt_us = self.rtt_series.max_us().unwrap_or(100_000);
        let p99_us = self.rtt_series.p99_us().unwrap_or(max_rtt_us);

        let p99 = p99_us as f64 / 1000.0;
        let max_rtt = max_rtt_us as f64 / 1000.0;

        let latency_score = if policy.prefer_latency {
            1.0 / (1.0 + p99 / 1000.0)
        } else {
            1.0 / (1.0 + max_rtt / 1000.0)
        };

        let queue_factor = 1.0 / (1.0 + (self.queue_depth as f64) * policy.queue_depth_weight);
        let rtt_factor = 1.0 / (1.0 + p99 * policy.rtt_weight);

        self.availability * latency_score * queue_factor * rtt_factor
    }

    fn record_success(&mut self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.successful_requests.fetch_add(1, Ordering::Relaxed);

        let total = self.total_requests.load(Ordering::Relaxed);
        let successful = self.successful_requests.load(Ordering::Relaxed);
        if total > 0 {
            self.availability = successful as f64 / total as f64;
        }
    }

    fn record_failure(&mut self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if failures >= 5 {
            self.healthy = false;
        }
    }

    fn metrics(&self, endpoint_id: EndpointId) -> EndpointMetrics {
        EndpointMetrics {
            endpoint_id,
            rtt_p50_us: self.rtt_series.mean_us().unwrap_or(0),
            rtt_p99_us: self.rtt_series.p99_us().unwrap_or(0),
            rtt_max_us: self.rtt_series.max_us().unwrap_or(0),
            availability: self.availability,
            queue_depth: self.queue_depth,
            bytes_in_flight: self.bytes_in_flight,
            healthy: self.healthy,
        }
    }
}

/// Errors during routing operations.
#[derive(Error, Debug)]
pub enum RoutingError {
    #[error("No healthy endpoints available")]
    NoHealthyEndpoints,

    #[error("Insufficient endpoints: {reason}")]
    InsufficientEndpoints {
        reason: String,
    },
}

/// Adaptive router for intelligent request routing.
pub struct AdaptiveRouter {
    config: AdaptiveRouterConfig,
    endpoints: RwLock<HashMap<EndpointId, Arc<RwLock<EndpointState>>>>,
}

impl AdaptiveRouter {
    /// Creates a new adaptive router.
    pub fn new(config: AdaptiveRouterConfig) -> Self {
        Self {
            config,
            endpoints: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a new endpoint.
    pub async fn register_endpoint(&self, endpoint_id: EndpointId) {
        let mut endpoints = self.endpoints.write().await;
        if endpoints.len() >= self.config.max_endpoints {
            return;
        }

        endpoints.insert(
            endpoint_id,
            Arc::new(RwLock::new(EndpointState::new(self.config.rtt_percentile_window))),
        );
        debug!(endpoint_id = ?endpoint_id, "Endpoint registered");
    }

    /// Unregisters an endpoint.
    pub async fn unregister_endpoint(&self, endpoint_id: EndpointId) {
        let mut endpoints = self.endpoints.write().await;
        endpoints.remove(&endpoint_id);
        debug!(endpoint_id = ?endpoint_id, "Endpoint unregistered");
    }

    /// Records RTT for an endpoint.
    pub async fn record_rtt(&self, endpoint_id: EndpointId, rtt_us: u64) {
        let endpoints = self.endpoints.read().await;
        if let Some(state_arc) = endpoints.get(&endpoint_id) {
            let seq = {
                let state = state_arc.read().await;
                state.total_requests.load(Ordering::Relaxed)
            };
            let mut state = state_arc.write().await;
            state.rtt_series.push(RttSample {
                seq,
                rtt_us,
                sent_at_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            });
            state.record_success();
            trace!(endpoint_id = ?endpoint_id, rtt_us = rtt_us, "RTT recorded");
        }
    }

    /// Records an RTT timeout (failure).
    pub async fn record_rtt_timeout(&self, endpoint_id: EndpointId) {
        let endpoints = self.endpoints.read().await;
        if let Some(state_arc) = endpoints.get(&endpoint_id) {
            let mut state = state_arc.write().await;
            state.record_failure();
            trace!(endpoint_id = ?endpoint_id, "RTT timeout recorded");
        }
    }

    /// Sets the queue depth for an endpoint.
    pub async fn set_queue_depth(&self, endpoint_id: EndpointId, depth: usize) {
        let endpoints = self.endpoints.read().await;
        if let Some(state_arc) = endpoints.get(&endpoint_id) {
            let mut state = state_arc.write().await;
            state.queue_depth = depth;
        }
    }

    /// Sets bytes in flight for an endpoint.
    pub async fn set_bytes_in_flight(&self, endpoint_id: EndpointId, bytes: u64) {
        let endpoints = self.endpoints.read().await;
        if let Some(state_arc) = endpoints.get(&endpoint_id) {
            let mut state = state_arc.write().await;
            state.bytes_in_flight = bytes;
        }
    }

    /// Marks an endpoint as unhealthy.
    pub async fn mark_unhealthy(&self, endpoint_id: EndpointId) {
        let endpoints = self.endpoints.read().await;
        if let Some(state_arc) = endpoints.get(&endpoint_id) {
            let mut state = state_arc.write().await;
            state.healthy = false;
            debug!(endpoint_id = ?endpoint_id, "Endpoint marked unhealthy");
        }
    }

    /// Marks an endpoint as healthy.
    pub async fn mark_healthy(&self, endpoint_id: EndpointId) {
        let endpoints = self.endpoints.read().await;
        if let Some(state_arc) = endpoints.get(&endpoint_id) {
            let mut state = state_arc.write().await;
            state.healthy = true;
            state.consecutive_failures.store(0, Ordering::Relaxed);
            debug!(endpoint_id = ?endpoint_id, "Endpoint marked healthy");
        }
    }

    /// Selects the best route based on current metrics.
    pub async fn select_route(&self) -> Result<RoutingDecision, RoutingError> {
        let endpoints = self.endpoints.read().await;

        let mut scored: Vec<(EndpointId, f64)> = Vec::new();
        for (id, state_arc) in endpoints.iter() {
            let state = state_arc.read().await;
            let score = state.compute_score(&self.config.policy);
            if score > 0.0 {
                scored.push((*id, score));
            }
        }

        if scored.is_empty() {
            return Err(RoutingError::NoHealthyEndpoints);
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let primary = scored[0].0;
        let primary_score = scored[0].1;

        let failover: Vec<EndpointId> = scored
            .iter()
            .skip(1)
            .take(3)
            .map(|(id, _)| *id)
            .collect();

        debug!(
            primary = ?primary,
            failover_count = failover.len(),
            score = primary_score,
            "Route selected"
        );

        Ok(RoutingDecision {
            primary_endpoint: primary,
            failover_endpoints: failover,
            score: primary_score,
        })
    }

    /// Returns metrics for all endpoints.
    pub async fn stats(&self) -> Vec<EndpointMetrics> {
        let endpoints = self.endpoints.read().await;
        let mut result = Vec::new();
        for (id, state_arc) in endpoints.iter() {
            let state = state_arc.read().await;
            result.push(state.metrics(*id));
        }
        result
    }

    /// Returns metrics for a specific endpoint.
    pub async fn endpoint_stats(&self, endpoint_id: EndpointId) -> Option<EndpointMetrics> {
        let endpoints = self.endpoints.read().await;
        if let Some(s) = endpoints.get(&endpoint_id) {
            let state = s.read().await;
            Some(state.metrics(endpoint_id))
        } else {
            None
        }
    }

    /// Gets the number of registered endpoints.
    pub async fn endpoint_count(&self) -> usize {
        self.endpoints.read().await.len()
    }

    /// Gets healthy endpoint count.
    pub async fn healthy_count(&self) -> usize {
        let endpoints = self.endpoints.read().await;
        let mut count = 0;
        for state_arc in endpoints.values() {
            let state = state_arc.read().await;
            if state.healthy {
                count += 1;
            }
        }
        count
    }
}

impl Default for AdaptiveRouter {
    fn default() -> Self {
        Self::new(AdaptiveRouterConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_id_default() {
        let id = EndpointId::default();
        assert_eq!(id.0, 0);
    }

    #[test]
    fn test_routing_policy_default() {
        let policy = RoutingPolicy::default();
        assert!(policy.prefer_latency);
        assert_eq!(policy.unhealthy_threshold, 0.5);
        assert_eq!(policy.queue_depth_weight, 0.3);
        assert_eq!(policy.rtt_weight, 0.7);
    }

    #[test]
    fn test_router_config_defaults() {
        let config = AdaptiveRouterConfig::default();
        assert_eq!(config.max_endpoints, 256);
        assert_eq!(config.rtt_percentile_window, 128);
    }

    #[tokio::test]
    async fn test_register_endpoint() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;

        let count = router.endpoint_count().await;
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_unregister_endpoint() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.unregister_endpoint(EndpointId(1)).await;

        let count = router.endpoint_count().await;
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_record_rtt() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.record_rtt(EndpointId(1), 100).await;
        router.record_rtt(EndpointId(1), 200).await;
        router.record_rtt(EndpointId(1), 300).await;

        let metrics = router.endpoint_stats(EndpointId(1)).await;
        assert!(metrics.is_some());
        let m = metrics.unwrap();
        assert_eq!(m.rtt_p50_us, 200);
    }

    #[tokio::test]
    async fn test_rtt_percentile_calculation() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;

        for i in 1..=200u64 {
            router.record_rtt(EndpointId(1), i * 100).await;
        }

        let metrics = router.endpoint_stats(EndpointId(1)).await;
        assert!(metrics.is_some());
        let m = metrics.unwrap();
        assert!(m.rtt_p99_us > 0);
    }

    #[tokio::test]
    async fn test_set_queue_depth() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.set_queue_depth(EndpointId(1), 100).await;

        let metrics = router.endpoint_stats(EndpointId(1)).await;
        assert_eq!(metrics.unwrap().queue_depth, 100);
    }

    #[tokio::test]
    async fn test_set_bytes_in_flight() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.set_bytes_in_flight(EndpointId(1), 1000).await;

        let metrics = router.endpoint_stats(EndpointId(1)).await;
        assert_eq!(metrics.unwrap().bytes_in_flight, 1000);
    }

    #[tokio::test]
    async fn test_mark_unhealthy() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.mark_unhealthy(EndpointId(1)).await;

        let metrics = router.endpoint_stats(EndpointId(1)).await;
        assert!(!metrics.unwrap().healthy);
    }

    #[tokio::test]
    async fn test_mark_healthy() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.mark_unhealthy(EndpointId(1)).await;
        router.mark_healthy(EndpointId(1)).await;

        let metrics = router.endpoint_stats(EndpointId(1)).await;
        assert!(metrics.unwrap().healthy);
    }

    #[tokio::test]
    async fn test_select_route_single_endpoint() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;

        let decision = router.select_route().await;
        assert!(decision.is_ok());
        let d = decision.unwrap();
        assert_eq!(d.primary_endpoint, EndpointId(1));
        assert!(d.failover_endpoints.is_empty());
    }

    #[tokio::test]
    async fn test_select_route_picks_best() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;

        router.record_rtt(EndpointId(1), 10000).await;
        router.record_rtt(EndpointId(2), 100).await;

        let decision = router.select_route().await;
        assert!(decision.is_ok());
        let d = decision.unwrap();
        assert_eq!(d.primary_endpoint, EndpointId(2));
    }

    #[tokio::test]
    async fn test_failover_list_ordering() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;
        router.register_endpoint(EndpointId(3)).await;

        router.record_rtt(EndpointId(1), 100).await;
        router.record_rtt(EndpointId(2), 200).await;
        router.record_rtt(EndpointId(3), 300).await;

        let decision = router.select_route().await.unwrap();
        assert_eq!(decision.primary_endpoint, EndpointId(1));
        assert_eq!(decision.failover_endpoints.len(), 2);
    }

    #[tokio::test]
    async fn test_no_healthy_endpoints() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.mark_unhealthy(EndpointId(1)).await;

        let result = router.select_route().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_all_unhealthy_error() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;
        
        router.mark_unhealthy(EndpointId(1)).await;
        router.mark_unhealthy(EndpointId(2)).await;

        let result = router.select_route().await;
        assert!(matches!(result, Err(RoutingError::NoHealthyEndpoints)));
    }

    #[tokio::test]
    async fn test_queue_depth_impact_on_scoring() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;

        router.set_queue_depth(EndpointId(1), 1000).await;
        router.set_queue_depth(EndpointId(2), 10).await;

        router.record_rtt(EndpointId(1), 100).await;
        router.record_rtt(EndpointId(2), 100).await;

        let decision = router.select_route().await.unwrap();
        assert_eq!(decision.primary_endpoint, EndpointId(2));
    }

    #[tokio::test]
    async fn test_availability_impact_on_scoring() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;

        for _ in 0..100 {
            router.record_rtt(EndpointId(1), 100).await;
            router.record_rtt(EndpointId(2), 100).await;
        }

        for _ in 0..50 {
            router.record_rtt_timeout(EndpointId(2)).await;
        }

        let decision = router.select_route().await.unwrap();
        assert_eq!(decision.primary_endpoint, EndpointId(1));
    }

    #[tokio::test]
    async fn test_latency_weight_prefer_latency() {
        let config = AdaptiveRouterConfig {
            policy: RoutingPolicy {
                prefer_latency: true,
                rtt_weight: 1.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let router = AdaptiveRouter::new(config);
        
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;

        router.record_rtt(EndpointId(1), 1000).await;
        router.record_rtt(EndpointId(2), 500).await;

        let decision = router.select_route().await.unwrap();
        assert_eq!(decision.primary_endpoint, EndpointId(2));
    }

    #[tokio::test]
    async fn test_latency_weight_availability() {
        let config = AdaptiveRouterConfig {
            policy: RoutingPolicy {
                prefer_latency: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let router = AdaptiveRouter::new(config);
        
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;

        router.record_rtt(EndpointId(1), 100).await;
        router.record_rtt_timeout(EndpointId(2)).await;
        router.record_rtt_timeout(EndpointId(2)).await;
        router.record_rtt_timeout(EndpointId(2)).await;
        router.record_rtt_timeout(EndpointId(2)).await;
        router.record_rtt_timeout(EndpointId(2)).await;

        let decision = router.select_route().await.unwrap();
        assert_eq!(decision.primary_endpoint, EndpointId(1));
    }

    #[tokio::test]
    async fn test_multiple_concurrent_select_route() {
        use std::sync::Arc;
        use tokio::task;

        let router = Arc::new(AdaptiveRouter::default());
        router.register_endpoint(EndpointId(1)).await;
        
        for _ in 0..100 {
            let router = Arc::clone(&router);
            task::spawn(async move {
                let _ = router.select_route().await;
            });
        }
    }

    #[tokio::test]
    async fn test_stats_returns_all_endpoints() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;
        router.register_endpoint(EndpointId(3)).await;

        let all_stats = router.stats().await;
        assert_eq!(all_stats.len(), 3);
    }

    #[tokio::test]
    async fn test_healthy_count() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;
        router.register_endpoint(EndpointId(3)).await;
        
        router.mark_unhealthy(EndpointId(2)).await;

        let count = router.healthy_count().await;
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_rtt_percentile_edge_empty() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;

        let metrics = router.endpoint_stats(EndpointId(1)).await.unwrap();
        assert_eq!(metrics.rtt_p50_us, 0);
        assert_eq!(metrics.rtt_p99_us, 0);
    }

    #[tokio::test]
    async fn test_rtt_percentile_single_sample() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;
        router.record_rtt(EndpointId(1), 500).await;

        let metrics = router.endpoint_stats(EndpointId(1)).await.unwrap();
        assert_eq!(metrics.rtt_p50_us, 500);
    }

    #[tokio::test]
    async fn test_automatic_unhealthy_detection() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;

        for _ in 0..5 {
            router.record_rtt_timeout(EndpointId(1)).await;
        }

        let metrics = router.endpoint_stats(EndpointId(1)).await.unwrap();
        assert!(!metrics.healthy);
    }

    #[tokio::test]
    async fn test_recovery_of_unhealthy_endpoint() {
        let router = AdaptiveRouter::default();
        router.register_endpoint(EndpointId(1)).await;

        for _ in 0..5 {
            router.record_rtt_timeout(EndpointId(1)).await;
        }

        assert!(!router.endpoint_stats(EndpointId(1)).await.unwrap().healthy);

        for _ in 0..10 {
            router.record_rtt(EndpointId(1), 100).await;
        }

        router.mark_healthy(EndpointId(1)).await;

        assert!(router.endpoint_stats(EndpointId(1)).await.unwrap().healthy);
    }

    #[tokio::test]
    async fn test_concurrent_rtt_recording() {
        use std::sync::Arc;
        use std::thread;

        let router = Arc::new(AdaptiveRouter::default());
        router.register_endpoint(EndpointId(1)).await;

        let mut handles = vec![];
        for _ in 0..10 {
            let router = Arc::clone(&router);
            let handle = thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(async {
                    for _ in 0..100 {
                        router.record_rtt(EndpointId(1), 100).await;
                    }
                });
            });
            handles.push(handle);
        }

        for h in handles {
            h.join().unwrap();
        }

        let metrics = router.endpoint_stats(EndpointId(1)).await.unwrap();
        assert!(metrics.rtt_p50_us > 0);
    }

    #[tokio::test]
    async fn test_max_endpoints_limit() {
        let config = AdaptiveRouterConfig {
            max_endpoints: 2,
            ..Default::default()
        };
        let router = AdaptiveRouter::new(config);
        
        router.register_endpoint(EndpointId(1)).await;
        router.register_endpoint(EndpointId(2)).await;
        router.register_endpoint(EndpointId(3)).await;

        let count = router.endpoint_count().await;
        assert_eq!(count, 2);
    }

    #[test]
    fn test_routing_decision_debug() {
        let decision = RoutingDecision {
            primary_endpoint: EndpointId(1),
            failover_endpoints: vec![EndpointId(2), EndpointId(3)],
            score: 0.95,
        };
        let debug_str = format!("{:?}", decision);
        assert!(debug_str.contains("primary_endpoint"));
    }
}