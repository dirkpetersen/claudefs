//! Advanced cluster diagnostics with health checks and root cause analysis.

use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticCheck {
    pub name: String,
    pub level: DiagnosticLevel,
    pub passed: bool,
    pub message: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticReport {
    pub checks: Vec<DiagnosticCheck>,
    pub total_duration_ms: u64,
    pub generated_at_ms: u64,
}

impl DiagnosticReport {
    pub fn passed_count(&self) -> usize {
        self.checks.iter().filter(|c| c.passed).count()
    }

    pub fn failed_count(&self) -> usize {
        self.checks.iter().filter(|c| !c.passed).count()
    }

    pub fn critical_failures(&self) -> Vec<&DiagnosticCheck> {
        self.checks
            .iter()
            .filter(|c| c.level == DiagnosticLevel::Critical && !c.passed)
            .collect()
    }

    pub fn is_healthy(&self) -> bool {
        !self.checks.iter().any(|c| {
            (c.level == DiagnosticLevel::Critical || c.level == DiagnosticLevel::Error) && !c.passed
        })
    }
}

pub struct CheckBuilder {
    name: String,
    level: DiagnosticLevel,
}

impl CheckBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            level: DiagnosticLevel::Info,
        }
    }

    pub fn level(mut self, level: DiagnosticLevel) -> Self {
        self.level = level;
        self
    }

    pub fn pass(self, message: &str, duration_ms: u64) -> DiagnosticCheck {
        DiagnosticCheck {
            name: self.name,
            level: self.level,
            passed: true,
            message: message.to_string(),
            duration_ms,
        }
    }

    pub fn fail(self, message: &str, duration_ms: u64) -> DiagnosticCheck {
        DiagnosticCheck {
            name: self.name,
            level: self.level,
            passed: false,
            message: message.to_string(),
            duration_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegisteredCheck {
    pub name: String,
    pub level: DiagnosticLevel,
}

pub struct DiagnosticsRunner {
    checks: Vec<RegisteredCheck>,
}

impl DiagnosticsRunner {
    pub fn new() -> Self {
        Self { checks: Vec::new() }
    }

    pub fn register(&mut self, name: &str, level: DiagnosticLevel) {
        self.checks.push(RegisteredCheck {
            name: name.to_string(),
            level,
        });
    }

    pub fn check_count(&self) -> usize {
        self.checks.len()
    }

    pub fn run_mock(&self, all_pass: bool) -> DiagnosticReport {
        let generated_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let mut total_duration: u64 = 0;
        let checks: Vec<DiagnosticCheck> = self
            .checks
            .iter()
            .enumerate()
            .map(|(i, registered)| {
                let passed = all_pass || i % 2 == 0;
                let message = if passed {
                    "OK".to_string()
                } else {
                    "FAILED".to_string()
                };
                let duration_ms = (i as u64 + 1) * 10;
                total_duration += duration_ms;
                DiagnosticCheck {
                    name: registered.name.clone(),
                    level: registered.level,
                    passed,
                    message,
                    duration_ms,
                }
            })
            .collect();

        DiagnosticReport {
            checks,
            total_duration_ms: total_duration,
            generated_at_ms,
        }
    }
}

impl Default for DiagnosticsRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Error)]
pub enum DiagnosticsError {
    #[error("Check failed: {0}")]
    CheckFailed(String),
    #[error("Timeout: {0}")]
    Timeout(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_builder_pass_creates_passed_true() {
        let check = CheckBuilder::new("test").pass("OK", 100);
        assert!(check.passed);
        assert_eq!(check.message, "OK");
    }

    #[test]
    fn check_builder_fail_creates_passed_false() {
        let check = CheckBuilder::new("test").fail("Failed", 50);
        assert!(!check.passed);
        assert_eq!(check.message, "Failed");
    }

    #[test]
    fn check_builder_level_sets_diagnostic_level() {
        let check = CheckBuilder::new("test")
            .level(DiagnosticLevel::Critical)
            .pass("OK", 10);
        assert_eq!(check.level, DiagnosticLevel::Critical);
    }

    #[test]
    fn diagnostic_report_passed_count_all_passed() {
        let report = DiagnosticReport {
            checks: vec![
                DiagnosticCheck {
                    name: "c1".into(),
                    level: DiagnosticLevel::Info,
                    passed: true,
                    message: "OK".into(),
                    duration_ms: 10,
                },
                DiagnosticCheck {
                    name: "c2".into(),
                    level: DiagnosticLevel::Info,
                    passed: true,
                    message: "OK".into(),
                    duration_ms: 10,
                },
            ],
            total_duration_ms: 20,
            generated_at_ms: 1000,
        };
        assert_eq!(report.passed_count(), 2);
    }

    #[test]
    fn diagnostic_report_failed_count_some_failed() {
        let report = DiagnosticReport {
            checks: vec![
                DiagnosticCheck {
                    name: "c1".into(),
                    level: DiagnosticLevel::Info,
                    passed: true,
                    message: "OK".into(),
                    duration_ms: 10,
                },
                DiagnosticCheck {
                    name: "c2".into(),
                    level: DiagnosticLevel::Info,
                    passed: false,
                    message: "Fail".into(),
                    duration_ms: 10,
                },
            ],
            total_duration_ms: 20,
            generated_at_ms: 1000,
        };
        assert_eq!(report.failed_count(), 1);
    }

    #[test]
    fn diagnostic_report_critical_failures_returns_only_critical_failed() {
        let report = DiagnosticReport {
            checks: vec![
                DiagnosticCheck {
                    name: "c1".into(),
                    level: DiagnosticLevel::Critical,
                    passed: false,
                    message: "Fail".into(),
                    duration_ms: 10,
                },
                DiagnosticCheck {
                    name: "c2".into(),
                    level: DiagnosticLevel::Error,
                    passed: false,
                    message: "Fail".into(),
                    duration_ms: 10,
                },
                DiagnosticCheck {
                    name: "c3".into(),
                    level: DiagnosticLevel::Critical,
                    passed: true,
                    message: "OK".into(),
                    duration_ms: 10,
                },
            ],
            total_duration_ms: 30,
            generated_at_ms: 1000,
        };
        let critical_failures = report.critical_failures();
        assert_eq!(critical_failures.len(), 1);
        assert_eq!(critical_failures[0].name, "c1");
    }

    #[test]
    fn diagnostic_report_is_healthy_true_no_critical_error() {
        let report = DiagnosticReport {
            checks: vec![
                DiagnosticCheck {
                    name: "c1".into(),
                    level: DiagnosticLevel::Warning,
                    passed: false,
                    message: "Warn".into(),
                    duration_ms: 10,
                },
                DiagnosticCheck {
                    name: "c2".into(),
                    level: DiagnosticLevel::Info,
                    passed: true,
                    message: "OK".into(),
                    duration_ms: 10,
                },
            ],
            total_duration_ms: 20,
            generated_at_ms: 1000,
        };
        assert!(report.is_healthy());
    }

    #[test]
    fn diagnostic_report_is_healthy_false_critical_failure() {
        let report = DiagnosticReport {
            checks: vec![DiagnosticCheck {
                name: "c1".into(),
                level: DiagnosticLevel::Critical,
                passed: false,
                message: "Fail".into(),
                duration_ms: 10,
            }],
            total_duration_ms: 10,
            generated_at_ms: 1000,
        };
        assert!(!report.is_healthy());
    }

    #[test]
    fn diagnostic_report_is_healthy_false_error_failure() {
        let report = DiagnosticReport {
            checks: vec![DiagnosticCheck {
                name: "c1".into(),
                level: DiagnosticLevel::Error,
                passed: false,
                message: "Fail".into(),
                duration_ms: 10,
            }],
            total_duration_ms: 10,
            generated_at_ms: 1000,
        };
        assert!(!report.is_healthy());
    }

    #[test]
    fn diagnostic_report_is_healthy_true_only_warning_failures() {
        let report = DiagnosticReport {
            checks: vec![DiagnosticCheck {
                name: "c1".into(),
                level: DiagnosticLevel::Warning,
                passed: false,
                message: "Warn".into(),
                duration_ms: 10,
            }],
            total_duration_ms: 10,
            generated_at_ms: 1000,
        };
        assert!(report.is_healthy());
    }

    #[test]
    fn diagnostics_runner_new_has_zero_checks() {
        let runner = DiagnosticsRunner::new();
        assert_eq!(runner.check_count(), 0);
    }

    #[test]
    fn diagnostics_runner_register_increments_count() {
        let mut runner = DiagnosticsRunner::new();
        runner.register("check1", DiagnosticLevel::Info);
        runner.register("check2", DiagnosticLevel::Warning);
        assert_eq!(runner.check_count(), 2);
    }

    #[test]
    fn diagnostics_runner_run_mock_all_pass_creates_all_passing() {
        let mut runner = DiagnosticsRunner::new();
        runner.register("check1", DiagnosticLevel::Info);
        runner.register("check2", DiagnosticLevel::Warning);
        let report = runner.run_mock(true);
        assert_eq!(report.passed_count(), 2);
        assert_eq!(report.failed_count(), 0);
        for check in &report.checks {
            assert_eq!(check.message, "OK");
        }
    }

    #[test]
    fn diagnostics_runner_run_mock_all_pass_false_creates_mixed() {
        let mut runner = DiagnosticsRunner::new();
        runner.register("check1", DiagnosticLevel::Info);
        runner.register("check2", DiagnosticLevel::Warning);
        runner.register("check3", DiagnosticLevel::Error);
        let report = runner.run_mock(false);
        assert_eq!(report.passed_count(), 2);
        assert_eq!(report.failed_count(), 1);
    }

    #[test]
    fn run_mock_report_total_duration_ms_non_zero() {
        let mut runner = DiagnosticsRunner::new();
        runner.register("check1", DiagnosticLevel::Info);
        let report = runner.run_mock(true);
        assert!(report.total_duration_ms > 0);
    }

    #[test]
    fn run_mock_report_generated_at_ms_non_zero() {
        let mut runner = DiagnosticsRunner::new();
        runner.register("check1", DiagnosticLevel::Info);
        let report = runner.run_mock(true);
        assert!(report.generated_at_ms > 0);
    }

    #[test]
    fn check_name_preserved_in_diagnostic_check() {
        let check = CheckBuilder::new("my_check").pass("OK", 100);
        assert_eq!(check.name, "my_check");
    }

    #[test]
    fn check_message_preserved() {
        let check = CheckBuilder::new("test").pass("All systems operational", 50);
        assert_eq!(check.message, "All systems operational");
    }

    #[test]
    fn check_duration_ms_preserved() {
        let check = CheckBuilder::new("test").pass("OK", 1234);
        assert_eq!(check.duration_ms, 1234);
    }

    #[test]
    fn empty_diagnostics_runner_run_mock_returns_empty_checks() {
        let runner = DiagnosticsRunner::new();
        let report = runner.run_mock(true);
        assert!(report.checks.is_empty());
        assert_eq!(report.total_duration_ms, 0);
    }
}
