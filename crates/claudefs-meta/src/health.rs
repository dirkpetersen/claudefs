//! Metadata node health diagnostics and readiness probes.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Health status of a component or node
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Component is healthy
    Healthy,
    /// Component is degraded but functional
    Degraded,
    /// Component is unhealthy
    Unhealthy,
    /// Health status is unknown (not yet checked)
    Unknown,
}

impl HealthStatus {
    /// Returns true if status is Healthy or Degraded (operational)
    pub fn is_ok(&self) -> bool {
        matches!(self, HealthStatus::Healthy | HealthStatus::Degraded)
    }
}

/// Health information for a single component
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentHealth {
    /// Component name
    pub name: String,
    /// Current health status
    pub status: HealthStatus,
    /// Optional status message
    pub message: Option<String>,
    /// Timestamp of last check (milliseconds since epoch)
    pub last_check_ms: u64,
    /// Latency of last check (microseconds)
    pub latency_us: u64,
}

/// Complete health report for a metadata node
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthReport {
    /// Node identifier
    pub node_id: u64,
    /// Overall health status
    pub overall: HealthStatus,
    /// Component health details
    pub components: Vec<ComponentHealth>,
    /// Node uptime in seconds
    pub uptime_secs: u64,
    /// Report generation timestamp (milliseconds since epoch)
    pub checked_at_ms: u64,
}

impl HealthReport {
    /// Returns true if overall status is Healthy
    pub fn is_healthy(&self) -> bool {
        self.overall == HealthStatus::Healthy
    }

    /// Returns list of unhealthy components
    pub fn unhealthy_components(&self) -> Vec<&ComponentHealth> {
        self.components
            .iter()
            .filter(|c| c.status == HealthStatus::Unhealthy)
            .collect()
    }

    /// Returns list of degraded components
    pub fn degraded_components(&self) -> Vec<&ComponentHealth> {
        self.components
            .iter()
            .filter(|c| c.status == HealthStatus::Degraded)
            .collect()
    }
}

/// Internal storage for registered health checks
struct RegisteredCheck {
    name: String,
    status: HealthStatus,
    message: Option<String>,
    latency_us: u64,
    last_check_ms: u64,
}

/// Health checker for metadata nodes
pub struct HealthChecker {
    node_id: u64,
    start_time_ms: u64,
    components: Mutex<Vec<RegisteredCheck>>,
}

impl HealthChecker {
    /// Creates a new health checker
    pub fn new(node_id: u64, start_time_ms: u64) -> Self {
        Self {
            node_id,
            start_time_ms,
            components: Mutex::new(Vec::new()),
        }
    }

    /// Registers a new component with Unknown status
    pub fn register(&self, name: &str) {
        let mut components = self.components.lock().unwrap();
        if !components.iter().any(|c| c.name == name) {
            components.push(RegisteredCheck {
                name: name.to_string(),
                status: HealthStatus::Unknown,
                message: None,
                latency_us: 0,
                last_check_ms: 0,
            });
        }
    }

    /// Updates component health status
    /// Silently ignores if component is not registered
    pub fn update(
        &self,
        name: &str,
        status: HealthStatus,
        message: Option<String>,
        latency_us: u64,
        now_ms: u64,
    ) {
        let mut components = self.components.lock().unwrap();
        if let Some(check) = components.iter_mut().find(|c| c.name == name) {
            check.status = status;
            check.message = message;
            check.latency_us = latency_us;
            check.last_check_ms = now_ms;
        }
    }

    /// Generates a health report
    /// Overall status: Unhealthy if any Unhealthy, else Degraded if any Degraded,
    /// else Unknown if any Unknown, else Healthy
    pub fn report(&self, now_ms: u64) -> HealthReport {
        let components = self.components.lock().unwrap();

        let overall = {
            let has_unhealthy = components
                .iter()
                .any(|c| c.status == HealthStatus::Unhealthy);
            let has_degraded = components
                .iter()
                .any(|c| c.status == HealthStatus::Degraded);
            let has_unknown = components.iter().any(|c| c.status == HealthStatus::Unknown);

            if has_unhealthy {
                HealthStatus::Unhealthy
            } else if has_degraded {
                HealthStatus::Degraded
            } else if has_unknown {
                HealthStatus::Unknown
            } else {
                HealthStatus::Healthy
            }
        };

        let component_health: Vec<ComponentHealth> = components
            .iter()
            .map(|c| ComponentHealth {
                name: c.name.clone(),
                status: c.status,
                message: c.message.clone(),
                last_check_ms: c.last_check_ms,
                latency_us: c.latency_us,
            })
            .collect();

        let uptime_secs = (now_ms - self.start_time_ms) / 1000;

        HealthReport {
            node_id: self.node_id,
            overall,
            components: component_health,
            uptime_secs,
            checked_at_ms: now_ms,
        }
    }

    /// Removes a component from health checks
    pub fn unregister(&self, name: &str) {
        let mut components = self.components.lock().unwrap();
        components.retain(|c| c.name != name);
    }

    /// Returns the number of registered components
    pub fn component_count(&self) -> usize {
        self.components.lock().unwrap().len()
    }

    /// Returns true if node is ready (healthy/degraded with no unknown components)
    pub fn is_ready(&self, now_ms: u64) -> bool {
        let report = self.report(now_ms);
        report.overall.is_ok()
            && !report
                .components
                .iter()
                .any(|c| c.status == HealthStatus::Unknown)
    }
}

/// Health thresholds for determining component health
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthThresholds {
    /// Maximum allowed Raft log lag (entries)
    pub max_raft_lag_entries: u64,
    /// Maximum allowed KV store latency (microseconds)
    pub max_kv_latency_us: u64,
    /// Maximum allowed shard imbalance percentage
    pub max_shard_imbalance_pct: f64,
    /// Minimum number of healthy peers required
    pub min_healthy_peers: usize,
    /// Stale check threshold (seconds)
    pub stale_check_secs: u64,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            max_raft_lag_entries: 1000,
            max_kv_latency_us: 10000,
            max_shard_imbalance_pct: 20.0,
            min_healthy_peers: 2,
            stale_check_secs: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_is_ok() {
        assert!(HealthStatus::Healthy.is_ok());
        assert!(HealthStatus::Degraded.is_ok());
    }

    #[test]
    fn test_health_status_not_ok() {
        assert!(!HealthStatus::Unhealthy.is_ok());
        assert!(!HealthStatus::Unknown.is_ok());
    }

    #[test]
    fn test_empty_checker_healthy() {
        let checker = HealthChecker::new(1, 1000);
        let report = checker.report(2000);
        assert_eq!(report.overall, HealthStatus::Healthy);
        assert!(report.is_healthy());
        assert!(report.unhealthy_components().is_empty());
        assert!(report.degraded_components().is_empty());
    }

    #[test]
    fn test_register_component() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        assert_eq!(checker.component_count(), 1);
        let report = checker.report(2000);
        assert_eq!(report.components.len(), 1);
        assert_eq!(report.components[0].name, "raft");
        assert_eq!(report.components[0].status, HealthStatus::Unknown);
    }

    #[test]
    fn test_update_component() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.update(
            "raft",
            HealthStatus::Healthy,
            Some("all good".to_string()),
            100,
            1500,
        );
        let report = checker.report(2000);
        assert_eq!(report.components[0].status, HealthStatus::Healthy);
        assert_eq!(report.components[0].message, Some("all good".to_string()));
        assert_eq!(report.components[0].latency_us, 100);
    }

    #[test]
    fn test_report_all_healthy() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.register("kvstore");
        checker.update("raft", HealthStatus::Healthy, None, 100, 1500);
        checker.update("kvstore", HealthStatus::Healthy, None, 200, 1500);
        let report = checker.report(2000);
        assert_eq!(report.overall, HealthStatus::Healthy);
        assert!(report.is_healthy());
    }

    #[test]
    fn test_report_one_degraded() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.register("kvstore");
        checker.update("raft", HealthStatus::Healthy, None, 100, 1500);
        checker.update(
            "kvstore",
            HealthStatus::Degraded,
            Some("slow".to_string()),
            200,
            1500,
        );
        let report = checker.report(2000);
        assert_eq!(report.overall, HealthStatus::Degraded);
        assert!(!report.is_healthy());
    }

    #[test]
    fn test_report_one_unhealthy() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.register("kvstore");
        checker.update("raft", HealthStatus::Healthy, None, 100, 1500);
        checker.update(
            "kvstore",
            HealthStatus::Unhealthy,
            Some("failed".to_string()),
            200,
            1500,
        );
        let report = checker.report(2000);
        assert_eq!(report.overall, HealthStatus::Unhealthy);
        assert!(!report.is_healthy());
    }

    #[test]
    fn test_report_unknown_overrides_healthy() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.register("kvstore");
        checker.update("raft", HealthStatus::Healthy, None, 100, 1500);
        // kvstore remains Unknown
        let report = checker.report(2000);
        assert_eq!(report.overall, HealthStatus::Unknown);
    }

    #[test]
    fn test_unhealthy_components_filter() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.register("kvstore");
        checker.register("membership");
        checker.update("raft", HealthStatus::Healthy, None, 100, 1500);
        checker.update("kvstore", HealthStatus::Unhealthy, None, 200, 1500);
        checker.update("membership", HealthStatus::Unhealthy, None, 300, 1500);
        let report = checker.report(2000);
        let unhealthy = report.unhealthy_components();
        assert_eq!(unhealthy.len(), 2);
    }

    #[test]
    fn test_degraded_components_filter() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.register("kvstore");
        checker.update("raft", HealthStatus::Degraded, None, 100, 1500);
        checker.update("kvstore", HealthStatus::Healthy, None, 200, 1500);
        let report = checker.report(2000);
        let degraded = report.degraded_components();
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].name, "raft");
    }

    #[test]
    fn test_uptime_calculation() {
        let checker = HealthChecker::new(1, 1000);
        let report = checker.report(11000);
        assert_eq!(report.uptime_secs, 10);
    }

    #[test]
    fn test_unregister_component() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.register("kvstore");
        assert_eq!(checker.component_count(), 2);
        checker.unregister("raft");
        assert_eq!(checker.component_count(), 1);
        let report = checker.report(2000);
        assert_eq!(report.components[0].name, "kvstore");
    }

    #[test]
    fn test_component_count() {
        let checker = HealthChecker::new(1, 1000);
        assert_eq!(checker.component_count(), 0);
        checker.register("raft");
        checker.register("kvstore");
        checker.register("membership");
        assert_eq!(checker.component_count(), 3);
    }

    #[test]
    fn test_is_ready_when_healthy() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.update("raft", HealthStatus::Healthy, None, 100, 1500);
        assert!(checker.is_ready(2000));
    }

    #[test]
    fn test_is_ready_false_when_unknown() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        // raft is Unknown
        assert!(!checker.is_ready(2000));
    }

    #[test]
    fn test_is_ready_false_when_unhealthy() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.update("raft", HealthStatus::Unhealthy, None, 100, 1500);
        assert!(!checker.is_ready(2000));
    }

    #[test]
    fn test_is_ready_true_when_degraded() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.update("raft", HealthStatus::Degraded, None, 100, 1500);
        assert!(checker.is_ready(2000));
    }

    #[test]
    fn test_update_nonexistent() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        // Update non-existent component - should be silently ignored
        checker.update("nonexistent", HealthStatus::Healthy, None, 100, 1500);
        let report = checker.report(2000);
        assert_eq!(report.components.len(), 1);
        assert_eq!(report.components[0].name, "raft");
        assert_eq!(report.components[0].status, HealthStatus::Unknown);
    }

    #[test]
    fn test_report_node_id() {
        let checker = HealthChecker::new(42, 1000);
        let report = checker.report(2000);
        assert_eq!(report.node_id, 42);
    }

    #[test]
    fn test_component_latency() {
        let checker = HealthChecker::new(1, 1000);
        checker.register("raft");
        checker.update("raft", HealthStatus::Healthy, None, 5000, 1500);
        let report = checker.report(2000);
        assert_eq!(report.components[0].latency_us, 5000);
    }

    #[test]
    fn test_default_thresholds() {
        let thresholds = HealthThresholds::default();
        assert_eq!(thresholds.max_raft_lag_entries, 1000);
        assert_eq!(thresholds.max_kv_latency_us, 10000);
        assert!((thresholds.max_shard_imbalance_pct - 20.0).abs() < f64::EPSILON);
        assert_eq!(thresholds.min_healthy_peers, 2);
        assert_eq!(thresholds.stale_check_secs, 30);
    }
}
