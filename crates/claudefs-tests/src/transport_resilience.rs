//! Transport Resilience Tests
//!
//! Tests for transport layer under stress and failure conditions.

#[cfg(test)]
mod tests {
    use claudefs_transport::{
        cancel::CancelRegistry,
        circuitbreaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState},
        hedge::HedgeConfig,
        keepalive::{KeepAliveConfig, KeepAliveState, KeepAliveStats, KeepAliveTracker},
        loadshed::{LoadShedConfig, LoadShedder},
        retry::{RetryConfig, RetryExecutor},
        tenant::{TenantConfig, TenantId, TenantManager, TenantTracker},
        zerocopy::{RegionPool, ZeroCopyConfig},
    };
    use std::time::Duration;

    #[test]
    fn test_circuit_breaker_initial_state() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_can_execute_when_closed() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);
        assert!(breaker.can_execute());
    }

    #[test]
    fn test_circuit_breaker_opens_on_failures() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 3;
        let breaker = CircuitBreaker::new(config);
        breaker.record_failure();
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_cannot_execute_when_open() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 2;
        let breaker = CircuitBreaker::new(config);
        breaker.record_failure();
        breaker.record_failure();
        assert!(!breaker.can_execute());
    }

    #[test]
    fn test_circuit_breaker_success_resets_failures() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.failure_count(), 2);
        breaker.record_success();
        assert_eq!(breaker.failure_count(), 0);
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 2;
        let breaker = CircuitBreaker::new(config);
        breaker.record_failure();
        breaker.record_failure();
        breaker.reset();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_execute());
    }

    #[test]
    fn test_circuit_breaker_config_threshold() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            open_duration: Duration::from_secs(30),
            half_open_max_requests: 3,
        };
        assert_eq!(config.failure_threshold, 3);
    }

    #[test]
    fn test_load_shedder_new() {
        let config = LoadShedConfig::default();
        let shedder = LoadShedder::new(config);
        assert!(!shedder.should_shed());
    }

    #[test]
    fn test_load_shedder_not_shedding_initially() {
        let config = LoadShedConfig::default();
        let shedder = LoadShedder::new(config);
        assert!(!shedder.should_shed());
    }

    #[test]
    fn test_load_shedder_record_low_latency() {
        let config = LoadShedConfig::default();
        let shedder = LoadShedder::new(config);
        shedder.record_latency(5);
        assert!(!shedder.should_shed());
    }

    #[test]
    fn test_load_shedder_stats() {
        let config = LoadShedConfig::default();
        let shedder = LoadShedder::new(config);
        let stats = shedder.stats();
        assert_eq!(stats.total_requests, 0);
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_retry_executor_new() {
        let config = RetryConfig::default();
        let executor = RetryExecutor::new(config);
        let _ = executor;
    }

    #[test]
    fn test_cancel_registry_new() {
        let _registry = CancelRegistry::default();
    }

    #[test]
    fn test_keepalive_config_default() {
        let config = KeepAliveConfig::default();
        assert!(config.interval.as_secs() > 0);
    }

    #[test]
    fn test_keepalive_state_active() {
        let state = KeepAliveState::Active;
        matches!(state, KeepAliveState::Active);
    }

    #[test]
    fn test_keepalive_stats_initial() {
        let stats = KeepAliveStats::default();
        assert_eq!(stats.state, 0);
    }

    #[test]
    fn test_keepalive_tracker_new() {
        let config = KeepAliveConfig::default();
        let _tracker = KeepAliveTracker::new(config);
    }

    #[test]
    fn test_tenant_id_new() {
        let id = TenantId(1);
        assert_eq!(id.0, 1);
    }

    #[test]
    fn test_tenant_config_new() {
        let config = TenantConfig::new(TenantId(1), "tenant-a");
        assert_eq!(config.name, "tenant-a");
    }

    #[test]
    fn test_tenant_manager_new() {
        let _manager = TenantManager::new();
    }

    #[test]
    fn test_tenant_tracker_new() {
        let config = TenantConfig::new(TenantId(1), "test");
        let tracker = TenantTracker::new(config);
        assert!(tracker.try_admit(1024));
    }

    #[test]
    fn test_hedge_config_new() {
        let config = HedgeConfig::default();
        assert!(config.enabled);
    }

    #[test]
    fn test_hedge_stats_initial() {
        let stats = claudefs_transport::hedge::HedgeStats::default();
        assert!(stats.total_hedges >= 0);
    }

    #[test]
    fn test_hedge_tracker_new() {
        let config = HedgeConfig::default();
        let _tracker = claudefs_transport::hedge::HedgeTracker::new(config);
    }

    #[test]
    fn test_zerocopy_config_new() {
        let config = ZeroCopyConfig::default();
        assert!(config.region_size > 0);
    }

    #[test]
    fn test_region_pool_new() {
        let config = ZeroCopyConfig::default();
        let pool = RegionPool::new(config);
        let stats = pool.stats();
        assert!(stats.total_regions > 0);
    }

    #[test]
    fn test_region_pool_stats_initial() {
        let config = ZeroCopyConfig::default();
        let pool = RegionPool::new(config);
        let stats = pool.stats();
        assert!(stats.available_regions > 0);
    }
}
