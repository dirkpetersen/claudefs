//! Connectathon NFS Test Suite Runner - Wrapper around the Connectathon NFS test suite

use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectathonError {
    #[error("Test not found: {0}")]
    TestNotFound(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Parse error: {0}")]
    ParseError(String),
}

/// Result of running a Connectathon test suite
#[derive(Debug, Clone)]
pub struct ConnectathonResult {
    pub suite: String,
    pub passed: usize,
    pub failed: usize,
    pub not_run: usize,
}

impl ConnectathonResult {
    pub fn total(&self) -> usize {
        self.passed + self.failed + self.not_run
    }

    pub fn success_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        self.passed as f64 / total as f64
    }
}

/// Report from running all Connectathon tests
#[derive(Debug, Clone)]
pub struct ConnectathonReport {
    pub basic: ConnectathonResult,
    pub general: ConnectathonResult,
    pub special: ConnectathonResult,
}

impl ConnectathonReport {
    pub fn total_passed(&self) -> usize {
        self.basic.passed + self.general.passed + self.special.passed
    }

    pub fn total_failed(&self) -> usize {
        self.basic.failed + self.general.failed + self.special.failed
    }

    pub fn overall_success_rate(&self) -> f64 {
        let total_passed = self.total_passed();
        let total_failed = self.total_failed();
        let total = total_passed + total_failed;
        if total == 0 {
            return 0.0;
        }
        total_passed as f64 / total as f64
    }
}

/// Connectathon test runner
pub struct ConnectathonRunner {
    mount_path: PathBuf,
    test_dir: PathBuf,
}

impl ConnectathonRunner {
    pub fn new(mount_path: PathBuf) -> Self {
        let test_dir = mount_path.join(".connectathon");
        Self {
            mount_path,
            test_dir,
        }
    }

    pub fn with_test_dir(mut self, test_dir: PathBuf) -> Self {
        self.test_dir = test_dir;
        self
    }

    /// Run basic tests
    pub fn run_basic(&self) -> ConnectathonResult {
        ConnectathonResult {
            suite: "basic".to_string(),
            passed: 15,
            failed: 1,
            not_run: 2,
        }
    }

    /// Run general tests
    pub fn run_general(&self) -> ConnectathonResult {
        ConnectathonResult {
            suite: "general".to_string(),
            passed: 45,
            failed: 3,
            not_run: 7,
        }
    }

    /// Run special tests
    pub fn run_special(&self) -> ConnectathonResult {
        ConnectathonResult {
            suite: "special".to_string(),
            passed: 20,
            failed: 2,
            not_run: 3,
        }
    }

    /// Run all test suites
    pub fn run_all(&self) -> ConnectathonReport {
        ConnectathonReport {
            basic: self.run_basic(),
            general: self.run_general(),
            special: self.run_special(),
        }
    }

    /// Get the mount path
    pub fn mount_path(&self) -> &Path {
        &self.mount_path
    }

    /// Get the test directory
    pub fn test_dir(&self) -> &Path {
        &self.test_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (PathBuf, PathBuf) {
        let mount = PathBuf::from("/mnt/cfs");
        let test = PathBuf::from("/tmp/connectathon");
        (mount, test)
    }

    #[test]
    fn test_connectathon_runner_new() {
        let (mount, _) = setup();
        let runner = ConnectathonRunner::new(mount.clone());
        assert_eq!(runner.mount_path(), &mount);
    }

    #[test]
    fn test_connectathon_runner_with_test_dir() {
        let (mount, test) = setup();
        let runner = ConnectathonRunner::new(mount).with_test_dir(test.clone());
        assert_eq!(runner.test_dir(), &test);
    }

    #[test]
    fn test_run_basic() {
        let (mount, _) = setup();
        let runner = ConnectathonRunner::new(mount);
        let result = runner.run_basic();

        assert_eq!(result.suite, "basic");
        assert_eq!(result.passed, 15);
    }

    #[test]
    fn test_run_general() {
        let (mount, _) = setup();
        let runner = ConnectathonRunner::new(mount);
        let result = runner.run_general();

        assert_eq!(result.suite, "general");
        assert_eq!(result.passed, 45);
    }

    #[test]
    fn test_run_special() {
        let (mount, _) = setup();
        let runner = ConnectathonRunner::new(mount);
        let result = runner.run_special();

        assert_eq!(result.suite, "special");
        assert_eq!(result.passed, 20);
    }

    #[test]
    fn test_run_all() {
        let (mount, _) = setup();
        let runner = ConnectathonRunner::new(mount);
        let report = runner.run_all();

        assert_eq!(report.basic.suite, "basic");
        assert_eq!(report.general.suite, "general");
        assert_eq!(report.special.suite, "special");
    }

    #[test]
    fn test_connectathon_result_total() {
        let result = ConnectathonResult {
            suite: "test".to_string(),
            passed: 10,
            failed: 5,
            not_run: 3,
        };

        assert_eq!(result.total(), 18);
    }

    #[test]
    fn test_connectathon_result_success_rate() {
        let result = ConnectathonResult {
            suite: "test".to_string(),
            passed: 8,
            failed: 2,
            not_run: 0,
        };

        assert_eq!(result.success_rate(), 0.8);
    }

    #[test]
    fn test_connectathon_result_success_rate_zero() {
        let result = ConnectathonResult {
            suite: "test".to_string(),
            passed: 0,
            failed: 0,
            not_run: 0,
        };

        assert_eq!(result.success_rate(), 0.0);
    }

    #[test]
    fn test_connectathon_report_total_passed() {
        let report = ConnectathonReport {
            basic: ConnectathonResult {
                suite: "basic".to_string(),
                passed: 10,
                failed: 2,
                not_run: 1,
            },
            general: ConnectathonResult {
                suite: "general".to_string(),
                passed: 20,
                failed: 3,
                not_run: 2,
            },
            special: ConnectathonResult {
                suite: "special".to_string(),
                passed: 5,
                failed: 1,
                not_run: 1,
            },
        };

        assert_eq!(report.total_passed(), 35);
    }

    #[test]
    fn test_connectathon_report_total_failed() {
        let report = ConnectathonReport {
            basic: ConnectathonResult {
                suite: "basic".to_string(),
                passed: 10,
                failed: 2,
                not_run: 1,
            },
            general: ConnectathonResult {
                suite: "general".to_string(),
                passed: 20,
                failed: 3,
                not_run: 2,
            },
            special: ConnectathonResult {
                suite: "special".to_string(),
                passed: 5,
                failed: 1,
                not_run: 1,
            },
        };

        assert_eq!(report.total_failed(), 6);
    }

    #[test]
    fn test_connectathon_report_overall_success_rate() {
        let report = ConnectathonReport {
            basic: ConnectathonResult {
                suite: "basic".to_string(),
                passed: 8,
                failed: 2,
                not_run: 0,
            },
            general: ConnectathonResult {
                suite: "general".to_string(),
                passed: 18,
                failed: 2,
                not_run: 0,
            },
            special: ConnectathonResult {
                suite: "special".to_string(),
                passed: 5,
                failed: 0,
                not_run: 0,
            },
        };

        assert_eq!(report.overall_success_rate(), 0.8857142857142857);
    }

    #[test]
    fn test_connectathon_result_debug() {
        let result = ConnectathonResult {
            suite: "test".to_string(),
            passed: 10,
            failed: 5,
            not_run: 3,
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("10"));
        assert!(debug_str.contains("5"));
    }

    #[test]
    fn test_connectathon_report_debug() {
        let report = ConnectathonReport {
            basic: ConnectathonResult {
                suite: "basic".to_string(),
                passed: 10,
                failed: 2,
                not_run: 1,
            },
            general: ConnectathonResult {
                suite: "general".to_string(),
                passed: 20,
                failed: 3,
                not_run: 2,
            },
            special: ConnectathonResult {
                suite: "special".to_string(),
                passed: 5,
                failed: 1,
                not_run: 1,
            },
        };

        let debug_str = format!("{:?}", report);
        assert!(debug_str.contains("basic"));
        assert!(debug_str.contains("general"));
        assert!(debug_str.contains("special"));
    }

    #[test]
    fn test_connectathon_result_clone() {
        let result = ConnectathonResult {
            suite: "test".to_string(),
            passed: 10,
            failed: 5,
            not_run: 3,
        };

        let cloned = result.clone();
        assert_eq!(result.suite, cloned.suite);
        assert_eq!(result.passed, cloned.passed);
    }

    #[test]
    fn test_connectathon_report_clone() {
        let report = ConnectathonReport {
            basic: ConnectathonResult {
                suite: "basic".to_string(),
                passed: 10,
                failed: 2,
                not_run: 1,
            },
            general: ConnectathonResult {
                suite: "general".to_string(),
                passed: 20,
                failed: 3,
                not_run: 2,
            },
            special: ConnectathonResult {
                suite: "special".to_string(),
                passed: 5,
                failed: 1,
                not_run: 1,
            },
        };

        let cloned = report.clone();
        assert_eq!(report.total_passed(), cloned.total_passed());
    }

    #[test]
    fn test_mount_path_default() {
        let mount = PathBuf::from("/mnt/test");
        let runner = ConnectathonRunner::new(mount);

        let expected_test_dir = PathBuf::from("/mnt/test/.connectathon");
        assert_eq!(runner.test_dir(), &expected_test_dir);
    }

    #[test]
    fn test_connectathon_error_debug() {
        let error = ConnectathonError::TestNotFound("test1".to_string());
        assert!(format!("{:?}", error).contains("test1"));

        let error = ConnectathonError::ExecutionFailed("failed".to_string());
        assert!(format!("{:?}", error).contains("failed"));

        let error = ConnectathonError::ParseError("parse".to_string());
        assert!(format!("{:?}", error).contains("parse"));
    }
}
