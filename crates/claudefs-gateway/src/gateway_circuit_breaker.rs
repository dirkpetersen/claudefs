//! Circuit breaker implementation for gateway backend connections.
//!
//! Provides fault tolerance by preventing cascading failures when backends become unavailable.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl Default for CircuitState {
    fn default() -> Self {
        Self::Closed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub open_duration_ms: u64,
    pub timeout_ms: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            open_duration_ms: 30_000,
            timeout_ms: 5_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerMetrics {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub rejected_calls: u64,
    pub state_changes: u64,
    pub current_state: CircuitState,
}

impl Default for CircuitBreakerMetrics {
    fn default() -> Self {
        Self {
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            rejected_calls: 0,
            state_changes: 0,
            current_state: CircuitState::Closed,
        }
    }
}

#[derive(Debug, Error)]
pub enum CircuitBreakerError {
    #[error("Circuit '{name}' is open, request rejected")]
    CircuitOpen { name: String },
    #[error("Operation failed: {0}")]
    OperationFailed(String),
    #[error("Operation timed out for '{name}' after {ms}ms")]
    Timeout { name: String, ms: u64 },
}

pub struct CircuitBreaker {
    name: String,
    config: CircuitBreakerConfig,
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    opened_at: Option<Instant>,
    metrics: CircuitBreakerMetrics,
}

impl CircuitBreaker {
    pub fn new(name: String, config: CircuitBreakerConfig) -> Self {
        Self {
            name,
            config,
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            opened_at: None,
            metrics: CircuitBreakerMetrics::default(),
        }
    }

    pub fn call<F, R>(&mut self, f: F) -> Result<R, CircuitBreakerError>
    where
        F: FnOnce() -> Result<R, CircuitBreakerError>,
    {
        self.metrics.total_calls += 1;

        if self.state == CircuitState::Open {
            self.check_and_transition_to_half_open();
        }

        if self.state == CircuitState::Open {
            self.metrics.rejected_calls += 1;
            return Err(CircuitBreakerError::CircuitOpen {
                name: self.name.clone(),
            });
        }

        let start = Instant::now();
        let result = f();
        let elapsed = start.elapsed().as_millis() as u64;

        if elapsed > self.config.timeout_ms {
            self.record_failure();
            return Err(CircuitBreakerError::Timeout {
                name: self.name.clone(),
                ms: elapsed,
            });
        }

        match result {
            Ok(value) => {
                self.record_success();
                Ok(value)
            }
            Err(e) => {
                if matches!(e, CircuitBreakerError::CircuitOpen { .. }) {
                    self.metrics.rejected_calls += 1;
                } else {
                    self.record_failure();
                }
                Err(e)
            }
        }
    }

    fn check_and_transition_to_half_open(&mut self) {
        if let Some(opened_at) = self.opened_at {
            let elapsed = opened_at.elapsed().as_millis() as u64;
            if elapsed >= self.config.open_duration_ms {
                self.transition_to(CircuitState::HalfOpen);
                self.success_count = 0;
                info!(
                    "Circuit '{}' transitioned from Open to HalfOpen after {}ms",
                    self.name, elapsed
                );
            }
        }
    }

    fn transition_to(&mut self, new_state: CircuitState) {
        if self.state != new_state {
            self.state = new_state;
            self.metrics.state_changes += 1;
            self.metrics.current_state = new_state;
        }
    }

    pub fn record_success(&mut self) {
        self.metrics.successful_calls += 1;

        match self.state {
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    self.transition_to(CircuitState::Closed);
                    self.failure_count = 0;
                    self.success_count = 0;
                    info!(
                        "Circuit '{}' transitioned from HalfOpen to Closed",
                        self.name
                    );
                }
            }
            CircuitState::Open => {}
        }

        debug!(
            "Circuit '{}' recorded success: state={:?}, failure_count={}, success_count={}",
            self.name, self.state, self.failure_count, self.success_count
        );
    }

    pub fn record_failure(&mut self) {
        self.metrics.failed_calls += 1;

        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.config.failure_threshold {
                    self.transition_to(CircuitState::Open);
                    self.opened_at = Some(Instant::now());
                    warn!(
                        "Circuit '{}' transitioned from Closed to Open after {} failures",
                        self.name, self.failure_count
                    );
                }
            }
            CircuitState::HalfOpen => {
                self.transition_to(CircuitState::Open);
                self.opened_at = Some(Instant::now());
                self.success_count = 0;
                warn!(
                    "Circuit '{}' transitioned from HalfOpen to Open on failure",
                    self.name
                );
            }
            CircuitState::Open => {}
        }

        debug!(
            "Circuit '{}' recorded failure: state={:?}, failure_count={}",
            self.name, self.state, self.failure_count
        );
    }

    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.opened_at = None;
        self.metrics.state_changes += 1;
        self.metrics.current_state = CircuitState::Closed;
        info!("Circuit '{}' reset to Closed", self.name);
    }

    pub fn trip(&mut self) {
        self.transition_to(CircuitState::Open);
        self.opened_at = Some(Instant::now());
        self.failure_count = self.config.failure_threshold;
        self.success_count = 0;
        warn!("Circuit '{}' tripped to Open", self.name);
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }

    pub fn failure_count(&self) -> u32 {
        self.failure_count
    }

    pub fn success_count(&self) -> u32 {
        self.success_count
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn metrics(&self) -> CircuitBreakerMetrics {
        self.metrics.clone()
    }

    #[cfg(test)]
    pub fn set_opened_at(&mut self, time: Instant) {
        self.opened_at = Some(time);
    }
}

pub struct CircuitBreakerRegistry {
    breakers: HashMap<String, CircuitBreaker>,
}

impl CircuitBreakerRegistry {
    pub fn new() -> Self {
        Self {
            breakers: HashMap::new(),
        }
    }

    pub fn get_or_create(
        &mut self,
        name: &str,
        config: CircuitBreakerConfig,
    ) -> &mut CircuitBreaker {
        self.breakers
            .entry(name.to_string())
            .or_insert_with(|| CircuitBreaker::new(name.to_string(), config))
    }

    pub fn get(&self, name: &str) -> Option<&CircuitBreaker> {
        self.breakers.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut CircuitBreaker> {
        self.breakers.get_mut(name)
    }

    pub fn all_metrics(&self) -> Vec<(&str, CircuitBreakerMetrics)> {
        self.breakers
            .iter()
            .map(|(k, v)| (k.as_str(), v.metrics()))
            .collect()
    }

    pub fn reset_all(&mut self) {
        for breaker in self.breakers.values_mut() {
            breaker.reset();
        }
    }

    pub fn count(&self) -> usize {
        self.breakers.len()
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_default_config() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 2);
        assert_eq!(config.open_duration_ms, 30_000);
        assert_eq!(config.timeout_ms, 5_000);
    }

    #[test]
    fn test_circuit_breaker_initial_state() {
        let cb = CircuitBreaker::new("test".to_string(), CircuitBreakerConfig::default());
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.success_count(), 0);
    }

    #[test]
    fn test_normal_call_flow() {
        let mut cb = CircuitBreaker::new("test".to_string(), CircuitBreakerConfig::default());

        let result: Result<(), _> = cb.call(|| Ok(()));
        assert!(result.is_ok());

        let metrics = cb.metrics();
        assert_eq!(metrics.total_calls, 1);
        assert_eq!(metrics.successful_calls, 1);
        assert_eq!(metrics.current_state, CircuitState::Closed);
    }

    #[test]
    fn test_operation_failed_in_closed_state() {
        let mut cb = CircuitBreaker::new("test".to_string(), CircuitBreakerConfig::default());

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(cb.failure_count(), 1);

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(cb.failure_count(), 2);

        let metrics = cb.metrics();
        assert_eq!(metrics.failed_calls, 2);
    }

    #[test]
    fn test_failure_accumulation_opens_circuit() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".to_string(), config);

        for _ in 0..3 {
            let _: Result<(), _> =
                cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        }

        assert_eq!(cb.state(), CircuitState::Open);
        assert_eq!(cb.failure_count(), 3);
    }

    #[test]
    fn test_open_state_rejects_calls() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".to_string(), config);

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));

        assert_eq!(cb.state(), CircuitState::Open);

        let result: Result<(), _> = cb.call(|| Ok(()));
        assert!(matches!(
            result,
            Err(CircuitBreakerError::CircuitOpen { .. })
        ));

        let metrics = cb.metrics();
        assert_eq!(metrics.rejected_calls, 1);
    }

    #[test]
    fn test_open_to_halfopen_after_duration() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            open_duration_ms: 100,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".to_string(), config);

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(cb.state(), CircuitState::Open);

        cb.opened_at = Some(Instant::now() - Duration::from_millis(150));

        let _ = cb.call(|| Ok(()));

        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_halfopen_to_closed_on_success_threshold() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            open_duration_ms: 10,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".to_string(), config);

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(cb.state(), CircuitState::Open);

        cb.opened_at = Some(Instant::now() - Duration::from_millis(100));
        let _ = cb.call(|| Ok(()));
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        let _ = cb.call(|| Ok(()));
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_halfopen_to_open_on_failure() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            open_duration_ms: 10,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".to_string(), config);

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        cb.opened_at = Some(Instant::now() - Duration::from_millis(100));

        let _ = cb.call(|| Ok(()));
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_reset_forces_closed_state() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".to_string(), config);

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(cb.state(), CircuitState::Open);

        cb.reset();

        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn test_trip_forces_open_state() {
        let mut cb = CircuitBreaker::new("test".to_string(), CircuitBreakerConfig::default());

        cb.trip();

        assert_eq!(cb.state(), CircuitState::Open);

        let result: Result<(), _> = cb.call(|| Ok(()));
        assert!(matches!(
            result,
            Err(CircuitBreakerError::CircuitOpen { .. })
        ));
    }

    #[test]
    fn test_timeout_counts_as_failure() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_ms: 10,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".to_string(), config);

        let _ = cb.call(|| {
            std::thread::sleep(Duration::from_millis(20));
            Ok(())
        });

        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_metrics_tracking() {
        let mut cb = CircuitBreaker::new("test".to_string(), CircuitBreakerConfig::default());

        cb.call(|| Ok::<(), _>(())).unwrap();
        cb.call(|| Err::<(), _>(CircuitBreakerError::OperationFailed("fail".to_string())))
            .unwrap_err();

        let metrics = cb.metrics();
        assert_eq!(metrics.total_calls, 2);
        assert_eq!(metrics.successful_calls, 1);
        assert_eq!(metrics.failed_calls, 1);
    }

    #[test]
    fn test_state_changes_metric() {
        let mut cb = CircuitBreaker::new("test".to_string(), CircuitBreakerConfig::default());

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));

        let metrics = cb.metrics();
        assert_eq!(metrics.state_changes, 1);
        assert_eq!(metrics.current_state, CircuitState::Open);
    }

    #[test]
    fn test_registry_get_or_create() {
        let mut registry = CircuitBreakerRegistry::new();

        {
            let cb1 = registry.get_or_create("backend1", CircuitBreakerConfig::default());
            let name1 = cb1.name();
            assert_eq!(name1, "backend1");
        }

        {
            let cb2 = registry.get_or_create("backend1", CircuitBreakerConfig::default());
            let name2 = cb2.name();
            assert_eq!(name2, "backend1");
        }

        let cb3 = registry.get_or_create("backend2", CircuitBreakerConfig::default());
        assert_eq!(cb3.name(), "backend2");

        assert_eq!(registry.count(), 2);
    }

    #[test]
    fn test_registry_get() {
        let mut registry = CircuitBreakerRegistry::new();

        let _ = registry.get_or_create("backend1", CircuitBreakerConfig::default());

        let cb = registry.get("backend1");
        assert!(cb.is_some());
        assert_eq!(cb.unwrap().name(), "backend1");

        let cb = registry.get("nonexistent");
        assert!(cb.is_none());
    }

    #[test]
    fn test_registry_get_mut() {
        let mut registry = CircuitBreakerRegistry::new();

        {
            let cb = registry.get_or_create("backend1", CircuitBreakerConfig::default());
            cb.record_success();
        }

        {
            let cb = registry.get_mut("backend1").unwrap();
            cb.record_success();
        }
    }

    #[test]
    fn test_registry_all_metrics() {
        let mut registry = CircuitBreakerRegistry::new();

        let _ = registry.get_or_create("backend1", CircuitBreakerConfig::default());

        let metrics = registry.all_metrics();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].0, "backend1");
    }

    #[test]
    fn test_registry_reset_all() {
        let mut registry = CircuitBreakerRegistry::new();

        {
            let cb = registry.get_or_create("backend1", CircuitBreakerConfig::default());
            let _: Result<(), _> =
                cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        }

        registry.reset_all();

        let cb = registry.get("backend1").unwrap();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_successful_call_clears_failure_count() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".to_string(), config);

        let _: Result<(), _> =
            cb.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        let _ = cb.call(|| Ok(()));

        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn test_name_accessor() {
        let cb = CircuitBreaker::new("my-circuit".to_string(), CircuitBreakerConfig::default());
        assert_eq!(cb.name(), "my-circuit");
    }
}
