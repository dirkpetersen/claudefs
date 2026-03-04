//! Health monitoring for the FUSE client.
//!
//! Provides health status tracking for client components including
//! transport connectivity, cache performance, and error rates.

use std::time::Instant;

/// Health status of a component or the overall system.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    /// Component is operating normally.
    Healthy,
    /// Component is degraded but still functional.
    Degraded {
        /// Reason for degraded status.
        reason: String,
    },
    /// Component is unhealthy and may need attention.
    Unhealthy {
        /// Reason for unhealthy status.
        reason: String,
    },
}

impl HealthStatus {
    /// Returns `true` if the status is `Healthy`.
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    /// Returns `true` if the status is `Degraded`.
    pub fn is_degraded(&self) -> bool {
        matches!(self, HealthStatus::Degraded { .. })
    }

    /// Returns `true` if the status is `Unhealthy`.
    pub fn is_unhealthy(&self) -> bool {
        matches!(self, HealthStatus::Unhealthy { .. })
    }

    /// Returns the reason for degraded or unhealthy status, if any.
    pub fn reason(&self) -> Option<&str> {
        match self {
            HealthStatus::Healthy => None,
            HealthStatus::Degraded { reason } => Some(reason),
            HealthStatus::Unhealthy { reason } => Some(reason),
        }
    }
}

/// Health information for a single component.
#[derive(Debug, Clone)]
pub struct ComponentHealth {
    /// Name of the component.
    pub name: String,
    /// Current health status.
    pub status: HealthStatus,
    /// Timestamp of the last health check.
    pub last_checked: Instant,
}

impl ComponentHealth {
    /// Creates a healthy component with the given name.
    pub fn healthy(name: &str) -> Self {
        ComponentHealth {
            name: name.to_string(),
            status: HealthStatus::Healthy,
            last_checked: Instant::now(),
        }
    }

    /// Creates a degraded component with the given name and reason.
    pub fn degraded(name: &str, reason: &str) -> Self {
        ComponentHealth {
            name: name.to_string(),
            status: HealthStatus::Degraded {
                reason: reason.to_string(),
            },
            last_checked: Instant::now(),
        }
    }

    /// Creates an unhealthy component with the given name and reason.
    pub fn unhealthy(name: &str, reason: &str) -> Self {
        ComponentHealth {
            name: name.to_string(),
            status: HealthStatus::Unhealthy {
                reason: reason.to_string(),
            },
            last_checked: Instant::now(),
        }
    }
}

/// Thresholds for determining degraded and unhealthy states.
#[derive(Debug, Clone)]
pub struct HealthThresholds {
    /// Cache hit rate below this triggers degraded status.
    pub cache_hit_rate_degraded: f64,
    /// Cache hit rate below this triggers unhealthy status.
    pub cache_hit_rate_unhealthy: f64,
    /// Error rate above this triggers degraded status.
    pub error_rate_degraded: f64,
    /// Error rate above this triggers unhealthy status.
    pub error_rate_unhealthy: f64,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        HealthThresholds {
            cache_hit_rate_degraded: 0.5,
            cache_hit_rate_unhealthy: 0.1,
            error_rate_degraded: 0.01,
            error_rate_unhealthy: 0.1,
        }
    }
}

/// Aggregated health report for all components.
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// Overall system health status.
    pub overall: HealthStatus,
    /// Individual component health statuses.
    pub components: Vec<ComponentHealth>,
    /// Timestamp when the report was generated.
    pub generated_at: Instant,
}

impl HealthReport {
    /// Creates a new report from component health statuses.
    ///
    /// The overall status is determined by the worst component status:
    /// - Any unhealthy component makes the overall status unhealthy.
    /// - Otherwise, any degraded component makes the overall status degraded.
    /// - If all components are healthy, the overall status is healthy.
    pub fn new(components: Vec<ComponentHealth>) -> Self {
        let overall = if components.iter().any(|c| c.status.is_unhealthy()) {
            HealthStatus::Unhealthy {
                reason: "one or more components unhealthy".into(),
            }
        } else if components.iter().any(|c| c.status.is_degraded()) {
            HealthStatus::Degraded {
                reason: "one or more components degraded".into(),
            }
        } else {
            HealthStatus::Healthy
        };

        HealthReport {
            overall,
            components,
            generated_at: Instant::now(),
        }
    }

    /// Returns the number of healthy components.
    pub fn healthy_count(&self) -> usize {
        self.components
            .iter()
            .filter(|c| c.status.is_healthy())
            .count()
    }

    /// Returns the number of degraded components.
    pub fn degraded_count(&self) -> usize {
        self.components
            .iter()
            .filter(|c| c.status.is_degraded())
            .count()
    }

    /// Returns the number of unhealthy components.
    pub fn unhealthy_count(&self) -> usize {
        self.components
            .iter()
            .filter(|c| c.status.is_unhealthy())
            .count()
    }

    /// Looks up a component by name.
    pub fn component(&self, name: &str) -> Option<&ComponentHealth> {
        self.components.iter().find(|c| c.name == name)
    }
}

/// Health checker for monitoring client components.
pub struct HealthChecker {
    thresholds: HealthThresholds,
    check_count: u64,
}

impl HealthChecker {
    /// Creates a new health checker with custom thresholds.
    pub fn new(thresholds: HealthThresholds) -> Self {
        HealthChecker {
            thresholds,
            check_count: 0,
        }
    }

    /// Creates a new health checker with default thresholds.
    pub fn with_defaults() -> Self {
        Self::new(HealthThresholds::default())
    }

    /// Checks transport health based on connection status.
    pub fn check_transport(&self, connected: bool) -> ComponentHealth {
        if connected {
            ComponentHealth::healthy("transport")
        } else {
            ComponentHealth::unhealthy("transport", "not connected")
        }
    }

    /// Checks cache health based on hit and miss counts.
    pub fn check_cache(&self, hits: u64, misses: u64) -> ComponentHealth {
        let total = hits + misses;
        if total == 0 {
            return ComponentHealth::healthy("cache");
        }

        let hit_rate = hits as f64 / total as f64;

        if hit_rate < self.thresholds.cache_hit_rate_unhealthy {
            ComponentHealth::unhealthy("cache", "hit rate critically low")
        } else if hit_rate < self.thresholds.cache_hit_rate_degraded {
            ComponentHealth::degraded("cache", "hit rate below threshold")
        } else {
            ComponentHealth::healthy("cache")
        }
    }

    /// Checks error health based on error and total operation counts.
    pub fn check_errors(&self, error_ops: u64, total_ops: u64) -> ComponentHealth {
        if total_ops == 0 {
            return ComponentHealth::healthy("errors");
        }

        let error_rate = error_ops as f64 / total_ops as f64;

        if error_rate > self.thresholds.error_rate_unhealthy {
            ComponentHealth::unhealthy("errors", "error rate critically high")
        } else if error_rate > self.thresholds.error_rate_degraded {
            ComponentHealth::degraded("errors", "error rate above threshold")
        } else {
            ComponentHealth::healthy("errors")
        }
    }

    /// Builds a health report from component health statuses.
    pub fn build_report(&mut self, components: Vec<ComponentHealth>) -> HealthReport {
        self.check_count += 1;
        HealthReport::new(components)
    }

    /// Returns the total number of reports generated.
    pub fn check_count(&self) -> u64 {
        self.check_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_healthy_is_healthy() {
        let status = HealthStatus::Healthy;
        assert!(status.is_healthy());
        assert!(!status.is_degraded());
        assert!(!status.is_unhealthy());
    }

    #[test]
    fn test_health_status_degraded_is_not_healthy() {
        let status = HealthStatus::Degraded {
            reason: "test".into(),
        };
        assert!(!status.is_healthy());
        assert!(status.is_degraded());
    }

    #[test]
    fn test_health_status_unhealthy_is_not_healthy() {
        let status = HealthStatus::Unhealthy {
            reason: "test".into(),
        };
        assert!(!status.is_healthy());
        assert!(status.is_unhealthy());
    }

    #[test]
    fn test_health_status_healthy_reason_is_none() {
        let status = HealthStatus::Healthy;
        assert!(status.reason().is_none());
    }

    #[test]
    fn test_health_status_degraded_reason_is_some() {
        let status = HealthStatus::Degraded {
            reason: "degraded".into(),
        };
        assert_eq!(status.reason(), Some("degraded"));
    }

    #[test]
    fn test_health_status_unhealthy_reason_is_some() {
        let status = HealthStatus::Unhealthy {
            reason: "unhealthy".into(),
        };
        assert_eq!(status.reason(), Some("unhealthy"));
    }

    #[test]
    fn test_component_health_healthy_constructor() {
        let ch = ComponentHealth::healthy("test");
        assert_eq!(ch.name, "test");
        assert!(ch.status.is_healthy());
    }

    #[test]
    fn test_component_health_degraded_constructor() {
        let ch = ComponentHealth::degraded("test", "reason");
        assert_eq!(ch.name, "test");
        assert!(ch.status.is_degraded());
        assert_eq!(ch.status.reason(), Some("reason"));
    }

    #[test]
    fn test_component_health_unhealthy_constructor() {
        let ch = ComponentHealth::unhealthy("test", "reason");
        assert_eq!(ch.name, "test");
        assert!(ch.status.is_unhealthy());
    }

    #[test]
    fn test_health_thresholds_default_values() {
        let t = HealthThresholds::default();
        assert!((t.cache_hit_rate_degraded - 0.5).abs() < f64::EPSILON);
        assert!((t.cache_hit_rate_unhealthy - 0.1).abs() < f64::EPSILON);
        assert!((t.error_rate_degraded - 0.01).abs() < f64::EPSILON);
        assert!((t.error_rate_unhealthy - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_health_report_all_healthy_overall_is_healthy() {
        let components = vec![ComponentHealth::healthy("a"), ComponentHealth::healthy("b")];
        let report = HealthReport::new(components);
        assert!(report.overall.is_healthy());
    }

    #[test]
    fn test_health_report_one_degraded_overall_is_degraded() {
        let components = vec![
            ComponentHealth::healthy("a"),
            ComponentHealth::degraded("b", "reason"),
        ];
        let report = HealthReport::new(components);
        assert!(report.overall.is_degraded());
    }

    #[test]
    fn test_health_report_one_unhealthy_overall_is_unhealthy() {
        let components = vec![
            ComponentHealth::healthy("a"),
            ComponentHealth::unhealthy("b", "reason"),
        ];
        let report = HealthReport::new(components);
        assert!(report.overall.is_unhealthy());
    }

    #[test]
    fn test_health_report_counts_correct() {
        let components = vec![
            ComponentHealth::healthy("a"),
            ComponentHealth::degraded("b", "r"),
            ComponentHealth::unhealthy("c", "r"),
        ];
        let report = HealthReport::new(components);
        assert_eq!(report.healthy_count(), 1);
        assert_eq!(report.degraded_count(), 1);
        assert_eq!(report.unhealthy_count(), 1);
    }

    #[test]
    fn test_health_report_component_lookup_by_name() {
        let components = vec![
            ComponentHealth::healthy("transport"),
            ComponentHealth::healthy("cache"),
        ];
        let report = HealthReport::new(components);
        assert!(report.component("transport").is_some());
        assert!(report.component("nonexistent").is_none());
    }

    #[test]
    fn test_checker_transport_connected_is_healthy() {
        let checker = HealthChecker::with_defaults();
        let ch = checker.check_transport(true);
        assert!(ch.status.is_healthy());
    }

    #[test]
    fn test_checker_transport_disconnected_is_unhealthy() {
        let checker = HealthChecker::with_defaults();
        let ch = checker.check_transport(false);
        assert!(ch.status.is_unhealthy());
    }

    #[test]
    fn test_checker_cache_no_ops_is_healthy() {
        let checker = HealthChecker::with_defaults();
        let ch = checker.check_cache(0, 0);
        assert!(ch.status.is_healthy());
    }

    #[test]
    fn test_checker_cache_low_hit_rate_is_unhealthy() {
        let checker = HealthChecker::with_defaults();
        let ch = checker.check_cache(5, 100);
        assert!(ch.status.is_unhealthy());
    }

    #[test]
    fn test_checker_errors_high_rate_is_unhealthy() {
        let checker = HealthChecker::with_defaults();
        let ch = checker.check_errors(50, 100);
        assert!(ch.status.is_unhealthy());
    }
}
