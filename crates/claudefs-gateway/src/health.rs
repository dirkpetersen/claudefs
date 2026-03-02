//! Gateway health check and readiness probe

use std::sync::Mutex;

/// Overall health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// All systems operational
    Healthy,
    /// Degraded but functional
    Degraded,
    /// Not functioning correctly
    Unhealthy,
    /// Starting up, not ready
    Starting,
}

impl HealthStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, HealthStatus::Healthy | HealthStatus::Degraded)
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Unhealthy => "unhealthy",
            HealthStatus::Starting => "starting",
        }
    }
}

/// Result of a single health check
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Check name/identifier
    pub name: String,
    /// Check status
    pub status: HealthStatus,
    /// Optional message/details
    pub message: String,
    /// Check duration in milliseconds
    pub duration_ms: u64,
}

impl CheckResult {
    pub fn ok(name: &str, duration_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Healthy,
            message: String::new(),
            duration_ms,
        }
    }

    pub fn degraded(name: &str, message: &str, duration_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Degraded,
            message: message.to_string(),
            duration_ms,
        }
    }

    pub fn unhealthy(name: &str, message: &str, duration_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Unhealthy,
            message: message.to_string(),
            duration_ms,
        }
    }
}

/// Composite health report from all checks
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// Overall aggregated status
    pub overall: HealthStatus,
    /// Individual check results
    pub checks: Vec<CheckResult>,
    /// Report timestamp (unix epoch)
    pub timestamp: u64,
}

impl HealthReport {
    pub fn new(checks: Vec<CheckResult>, timestamp: u64) -> Self {
        let overall = if checks.is_empty() {
            HealthStatus::Starting
        } else {
            let has_unhealthy = checks.iter().any(|c| c.status == HealthStatus::Unhealthy);
            let has_degraded = checks.iter().any(|c| c.status == HealthStatus::Degraded);

            if has_unhealthy {
                HealthStatus::Unhealthy
            } else if has_degraded {
                HealthStatus::Degraded
            } else {
                HealthStatus::Healthy
            }
        };

        Self {
            overall,
            checks,
            timestamp,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.overall.is_ok()
    }

    pub fn passed_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == HealthStatus::Healthy)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == HealthStatus::Unhealthy || c.status == HealthStatus::Starting)
            .count()
    }
}

/// Health check registry and aggregator
pub struct HealthChecker {
    results: Mutex<Vec<CheckResult>>,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            results: Mutex::new(Vec::new()),
        }
    }

    pub fn register_result(&self, result: CheckResult) {
        let mut results = self.results.lock().unwrap();
        if let Some(existing) = results.iter_mut().find(|r| r.name == result.name) {
            *existing = result;
        } else {
            results.push(result);
        }
    }

    pub fn update_result(&self, name: &str, status: HealthStatus, message: &str) -> bool {
        let mut results = self.results.lock().unwrap();
        if let Some(existing) = results.iter_mut().find(|r| r.name == name) {
            existing.status = status;
            existing.message = message.to_string();
            true
        } else {
            false
        }
    }

    pub fn report(&self, timestamp: u64) -> HealthReport {
        let results = self.results.lock().unwrap();
        HealthReport::new(results.clone(), timestamp)
    }

    pub fn is_healthy(&self) -> bool {
        let results = self.results.lock().unwrap();
        !results.is_empty()
            && results
                .iter()
                .all(|r| r.status == HealthStatus::Healthy || r.status == HealthStatus::Degraded)
    }

    pub fn is_ready(&self) -> bool {
        let results = self.results.lock().unwrap();
        !results.is_empty()
            && results
                .iter()
                .all(|r| r.status != HealthStatus::Unhealthy && r.status != HealthStatus::Starting)
    }

    pub fn check_count(&self) -> usize {
        self.results.lock().unwrap().len()
    }

    pub fn remove_check(&self, name: &str) -> bool {
        let mut results = self.results.lock().unwrap();
        let initial_len = results.len();
        results.retain(|r| r.name != name);
        results.len() < initial_len
    }

    pub fn clear(&self) {
        self.results.lock().unwrap().clear();
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_is_ok() {
        assert!(HealthStatus::Healthy.is_ok());
        assert!(HealthStatus::Degraded.is_ok());
        assert!(!HealthStatus::Unhealthy.is_ok());
        assert!(!HealthStatus::Starting.is_ok());
    }

    #[test]
    fn test_health_status_to_str() {
        assert_eq!(HealthStatus::Healthy.to_str(), "healthy");
        assert_eq!(HealthStatus::Degraded.to_str(), "degraded");
        assert_eq!(HealthStatus::Unhealthy.to_str(), "unhealthy");
        assert_eq!(HealthStatus::Starting.to_str(), "starting");
    }

    #[test]
    fn test_check_result_ok() {
        let result = CheckResult::ok("test", 10);
        assert_eq!(result.name, "test");
        assert_eq!(result.status, HealthStatus::Healthy);
        assert_eq!(result.message, "");
        assert_eq!(result.duration_ms, 10);
    }

    #[test]
    fn test_check_result_degraded() {
        let result = CheckResult::degraded("test", "slow response", 100);
        assert_eq!(result.status, HealthStatus::Degraded);
        assert_eq!(result.message, "slow response");
    }

    #[test]
    fn test_check_result_unhealthy() {
        let result = CheckResult::unhealthy("test", "connection failed", 500);
        assert_eq!(result.status, HealthStatus::Unhealthy);
        assert_eq!(result.message, "connection failed");
    }

    #[test]
    fn test_health_report_new_all_healthy() {
        let checks = vec![CheckResult::ok("check1", 10), CheckResult::ok("check2", 20)];
        let report = HealthReport::new(checks, 1000);
        assert_eq!(report.overall, HealthStatus::Healthy);
    }

    #[test]
    fn test_health_report_new_with_degraded() {
        let checks = vec![
            CheckResult::ok("check1", 10),
            CheckResult::degraded("check2", "slow", 20),
        ];
        let report = HealthReport::new(checks, 1000);
        assert_eq!(report.overall, HealthStatus::Degraded);
    }

    #[test]
    fn test_health_report_new_with_unhealthy() {
        let checks = vec![
            CheckResult::ok("check1", 10),
            CheckResult::unhealthy("check2", "failed", 20),
        ];
        let report = HealthReport::new(checks, 1000);
        assert_eq!(report.overall, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_health_report_new_empty() {
        let report = HealthReport::new(vec![], 1000);
        assert_eq!(report.overall, HealthStatus::Starting);
    }

    #[test]
    fn test_health_report_is_ready() {
        let checks = vec![
            CheckResult::ok("check1", 10),
            CheckResult::degraded("check2", "slow", 20),
        ];
        let report = HealthReport::new(checks, 1000);
        assert!(report.is_ready());
    }

    #[test]
    fn test_health_report_is_ready_not() {
        let checks = vec![CheckResult::unhealthy("check1", "failed", 10)];
        let report = HealthReport::new(checks, 1000);
        assert!(!report.is_ready());
    }

    #[test]
    fn test_health_report_passed_count() {
        let checks = vec![
            CheckResult::ok("check1", 10),
            CheckResult::degraded("check2", "slow", 20),
            CheckResult::unhealthy("check3", "failed", 30),
        ];
        let report = HealthReport::new(checks, 1000);
        assert_eq!(report.passed_count(), 1);
    }

    #[test]
    fn test_health_report_failed_count() {
        let checks = vec![
            CheckResult::ok("check1", 10),
            CheckResult::degraded("check2", "slow", 20),
            CheckResult::unhealthy("check3", "failed", 30),
            CheckResult {
                name: "check4".to_string(),
                status: HealthStatus::Starting,
                message: String::new(),
                duration_ms: 40,
            },
        ];
        let report = HealthReport::new(checks, 1000);
        assert_eq!(report.failed_count(), 2);
    }

    #[test]
    fn test_health_checker_new() {
        let checker = HealthChecker::new();
        assert_eq!(checker.check_count(), 0);
    }

    #[test]
    fn test_health_checker_register_result() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::ok("check1", 10));
        assert_eq!(checker.check_count(), 1);
    }

    #[test]
    fn test_health_checker_update_result() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::ok("check1", 10));
        assert!(checker.update_result("check1", HealthStatus::Degraded, "slightly slow"));
    }

    #[test]
    fn test_health_checker_update_result_not_found() {
        let checker = HealthChecker::new();
        assert!(!checker.update_result("nonexistent", HealthStatus::Degraded, "message"));
    }

    #[test]
    fn test_health_checker_report() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::ok("check1", 10));
        let report = checker.report(1000);
        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.timestamp, 1000);
    }

    #[test]
    fn test_health_checker_is_healthy() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::ok("check1", 10));
        assert!(checker.is_healthy());
    }

    #[test]
    fn test_health_checker_is_healthy_with_unhealthy() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::unhealthy("check1", "failed", 10));
        assert!(!checker.is_healthy());
    }

    #[test]
    fn test_health_checker_is_healthy_empty() {
        let checker = HealthChecker::new();
        assert!(!checker.is_healthy());
    }

    #[test]
    fn test_health_checker_is_ready() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::ok("check1", 10));
        assert!(checker.is_ready());
    }

    #[test]
    fn test_health_checker_is_ready_not() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::unhealthy("check1", "failed", 10));
        assert!(!checker.is_ready());
    }

    #[test]
    fn test_health_checker_remove_check() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::ok("check1", 10));
        assert!(checker.remove_check("check1"));
        assert_eq!(checker.check_count(), 0);
    }

    #[test]
    fn test_health_checker_remove_check_not_found() {
        let checker = HealthChecker::new();
        assert!(!checker.remove_check("nonexistent"));
    }

    #[test]
    fn test_health_checker_clear() {
        let checker = HealthChecker::new();
        checker.register_result(CheckResult::ok("check1", 10));
        checker.register_result(CheckResult::ok("check2", 20));
        checker.clear();
        assert_eq!(checker.check_count(), 0);
    }
}
