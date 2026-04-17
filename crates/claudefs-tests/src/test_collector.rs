//! Test Result Collector Module
//!
//! Parses cargo test JSON output and generates per-crate JSON test reports.
//! This module is the foundation for GitHub Actions integration, flaky test detection,
//! and CHANGELOG automation.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Test status enum matching cargo test JSON format
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
}

/// Individual test case result
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestCaseResult {
    pub name: String,
    pub status: TestStatus,
    pub duration_ms: u64,
}

impl TestCaseResult {
    pub fn new(name: String, status: TestStatus, duration_ms: u64) -> Self {
        Self {
            name,
            status,
            duration_ms,
        }
    }
}

/// Test suite report for a single crate
#[derive(Serialize, Deserialize, Debug)]
pub struct TestSuiteReport {
    pub crate_name: String,
    pub timestamp: u64,
    pub duration_secs: f64,
    pub total_tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub flaky_count: usize,
    pub tests: Vec<TestCaseResult>,
}

impl TestSuiteReport {
    pub fn new(crate_name: String, timestamp: u64, duration_secs: f64) -> Self {
        Self {
            crate_name,
            timestamp,
            duration_secs,
            total_tests: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            flaky_count: 0,
            tests: Vec::new(),
        }
    }

    pub fn add_result(&mut self, result: TestCaseResult) {
        match result.status {
            TestStatus::Pass => self.passed += 1,
            TestStatus::Fail => self.failed += 1,
            TestStatus::Skip => self.skipped += 1,
        }
        self.total_tests += 1;
        self.tests.push(result);
    }
}

/// Aggregated test summary across all crates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSummary {
    pub total_tests: usize,
    pub total_passed: usize,
    pub total_failed: usize,
    pub total_skipped: usize,
    pub crates: Vec<String>,
}

/// Cargo test JSON event types
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CargoTestEvent {
    #[serde(rename = "started")]
    Started { name: String },
    #[serde(rename = "ok")]
    Ok { name: String, duration: Option<f64> },
    #[serde(rename = "failed")]
    Failed { name: String, duration: Option<f64> },
    #[serde(rename = "skipped")]
    Skipped { name: String },
    #[serde(rename = "test")]
    Test {
        name: String,
        status: String,
        duration: Option<f64>,
    },
    #[serde(rename = "suite")]
    Suite { crate_name: String, timestamp: u64 },
}

/// Test result with metadata for duration calculation
#[derive(Debug)]
struct TestEvent {
    name: String,
    status: TestStatus,
    start_time: Option<u64>,
    end_time: Option<u64>,
}

/// Test collector for parsing and aggregating cargo test output
pub struct TestCollector {
    results: HashMap<String, Vec<TestCaseResult>>,
    test_times: HashMap<String, (u64, u64)>,
    suite_timestamps: HashMap<String, u64>,
}

impl TestCollector {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
            test_times: HashMap::new(),
            suite_timestamps: HashMap::new(),
        }
    }

    /// Parse JSON from cargo test output
    pub fn from_cargo_output(output: &str) -> anyhow::Result<Self> {
        let mut collector = Self::new();
        let mut test_starts: HashMap<String, u64> = HashMap::new();

        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let event: CargoTestEvent = match serde_json::from_str(trimmed) {
                Ok(e) => e,
                Err(_) => continue,
            };

            match event {
                CargoTestEvent::Started { name } => {
                    let start_time = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    test_starts.insert(name.clone(), start_time);
                }
                CargoTestEvent::Ok { name, duration } => {
                    let (start_time, _) = collector.test_times.remove(&name).unwrap_or((0, 0));
                    let end_time = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    let duration_ms = if let Some(d) = duration {
                        (d * 1000.0) as u64
                    } else if start_time > 0 {
                        ((end_time - start_time) * 1000) as u64
                    } else {
                        0
                    };

                    let crate_name = extract_crate_name(&name);
                    let result = TestCaseResult::new(name, TestStatus::Pass, duration_ms);
                    collector.add_result(&crate_name, result);
                }
                CargoTestEvent::Failed { name, duration } => {
                    let (start_time, _) = collector.test_times.remove(&name).unwrap_or((0, 0));
                    let end_time = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    let duration_ms = if let Some(d) = duration {
                        (d * 1000.0) as u64
                    } else if start_time > 0 {
                        ((end_time - start_time) * 1000) as u64
                    } else {
                        0
                    };

                    let crate_name = extract_crate_name(&name);
                    let result = TestCaseResult::new(name, TestStatus::Fail, duration_ms);
                    collector.add_result(&crate_name, result);
                }
                CargoTestEvent::Skipped { name } => {
                    let crate_name = extract_crate_name(&name);
                    let result = TestCaseResult::new(name, TestStatus::Skip, 0);
                    collector.add_result(&crate_name, result);
                }
                CargoTestEvent::Test {
                    name,
                    status,
                    duration,
                } => {
                    let test_status = match status.as_str() {
                        "test" | "passed" => TestStatus::Pass,
                        "failed" => TestStatus::Fail,
                        "skipped" => TestStatus::Skip,
                        _ => TestStatus::Fail,
                    };
                    let duration_ms = duration.map(|d| (d * 1000.0) as u64).unwrap_or(0);

                    let crate_name = extract_crate_name(&name);
                    let result = TestCaseResult::new(name, test_status, duration_ms);
                    collector.add_result(&crate_name, result);
                }
                CargoTestEvent::Suite {
                    crate_name,
                    timestamp,
                } => {
                    collector.suite_timestamps.insert(crate_name, timestamp);
                }
            }
        }

        Ok(collector)
    }

    /// Add a test result to a specific crate
    pub fn add_result(&mut self, crate_name: &str, result: TestCaseResult) {
        self.results
            .entry(crate_name.to_string())
            .or_insert_with(Vec::new)
            .push(result);
    }

    /// Build a report for a specific crate
    pub fn get_report(&self, crate_name: &str) -> Option<TestSuiteReport> {
        let tests = self.results.get(crate_name)?;

        let mut report = TestSuiteReport::new(
            crate_name.to_string(),
            *self
                .suite_timestamps
                .get(crate_name)
                .unwrap_or(&current_timestamp()),
            0.0,
        );

        for test in tests {
            report.add_result(test.clone());
        }

        let total_duration_ms: u64 = tests.iter().map(|t| t.duration_ms).sum();
        report.duration_secs = total_duration_ms as f64 / 1000.0;

        Some(report)
    }

    /// Write all reports as JSON to the output directory
    pub fn write_json_reports(&self, output_dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(output_dir)?;

        for (crate_name, tests) in &self.results {
            let mut report = TestSuiteReport::new(
                crate_name.clone(),
                *self
                    .suite_timestamps
                    .get(crate_name)
                    .unwrap_or(&current_timestamp()),
                0.0,
            );

            for test in tests {
                report.add_result(test.clone());
            }

            let total_duration_ms: u64 = tests.iter().map(|t| t.duration_ms).sum();
            report.duration_secs = total_duration_ms as f64 / 1000.0;

            let json = serde_json::to_string_pretty(&report)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            let file_path = output_dir.join(format!("{}.json", crate_name));
            std::fs::write(file_path, json)?;
        }

        Ok(())
    }

    /// Get aggregated summary across all crates
    pub fn summary(&self) -> TestSummary {
        let mut total_tests = 0;
        let mut total_passed = 0;
        let mut total_failed = 0;
        let mut total_skipped = 0;
        let mut crates = Vec::new();

        for (crate_name, tests) in &self.results {
            crates.push(crate_name.clone());
            total_tests += tests.len();
            for test in tests {
                match test.status {
                    TestStatus::Pass => total_passed += 1,
                    TestStatus::Fail => total_failed += 1,
                    TestStatus::Skip => total_skipped += 1,
                }
            }
        }

        TestSummary {
            total_tests,
            total_passed,
            total_failed,
            total_skipped,
            crates,
        }
    }
}

impl Default for TestCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract crate name from full test name (e.g., "claudefs_storage::tests::test_foo" -> "claudefs_storage")
fn extract_crate_name(test_name: &str) -> String {
    test_name
        .split("::")
        .next()
        .unwrap_or("unknown")
        .to_string()
}

/// Get current Unix timestamp
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_collector_empty() {
        let collector = TestCollector::new();
        let summary = collector.summary();
        assert_eq!(summary.total_tests, 0);
        assert_eq!(summary.crates.len(), 0);
    }

    #[test]
    fn test_collector_single_pass() {
        let mut collector = TestCollector::new();
        collector.add_result(
            "claudefs_storage",
            TestCaseResult::new("test1".to_string(), TestStatus::Pass, 100),
        );

        let report = collector.get_report("claudefs_storage").unwrap();
        assert_eq!(report.total_tests, 1);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn test_collector_mixed_results() {
        let mut collector = TestCollector::new();
        collector.add_result(
            "claudefs_storage",
            TestCaseResult::new("test1".to_string(), TestStatus::Pass, 100),
        );
        collector.add_result(
            "claudefs_storage",
            TestCaseResult::new("test2".to_string(), TestStatus::Fail, 50),
        );
        collector.add_result(
            "claudefs_storage",
            TestCaseResult::new("test3".to_string(), TestStatus::Skip, 0),
        );

        let report = collector.get_report("claudefs_storage").unwrap();
        assert_eq!(report.total_tests, 3);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert_eq!(report.skipped, 1);
    }

    #[test]
    fn test_collector_groups_by_crate() {
        let mut collector = TestCollector::new();
        collector.add_result(
            "claudefs_storage",
            TestCaseResult::new("storage::test1".to_string(), TestStatus::Pass, 10),
        );
        collector.add_result(
            "claudefs_meta",
            TestCaseResult::new("meta::test1".to_string(), TestStatus::Pass, 20),
        );

        let storage_report = collector.get_report("claudefs_storage").unwrap();
        let meta_report = collector.get_report("claudefs_meta").unwrap();

        assert_eq!(storage_report.total_tests, 1);
        assert_eq!(meta_report.total_tests, 1);
    }

    #[test]
    fn test_json_report_format() {
        let mut collector = TestCollector::new();
        collector.add_result(
            "claudefs_storage",
            TestCaseResult::new(
                "block_allocator::tests::test_alloc_basic".to_string(),
                TestStatus::Pass,
                45,
            ),
        );

        let report = collector.get_report("claudefs_storage").unwrap();
        let json = serde_json::to_string(&report).unwrap();

        assert!(json.contains("claudefs_storage"));
        assert!(json.contains("\"passed\": 1"));
        assert!(json.contains("block_allocator"));
    }

    #[test]
    fn test_write_json_reports() {
        let mut collector = TestCollector::new();
        collector.add_result(
            "claudefs_storage",
            TestCaseResult::new("test1".to_string(), TestStatus::Pass, 100),
        );

        let dir = tempdir().unwrap();
        collector.write_json_reports(dir.path()).unwrap();

        let report_file = dir.path().join("claudefs_storage.json");
        assert!(report_file.exists());

        let content = std::fs::read_to_string(report_file).unwrap();
        let parsed: TestSuiteReport = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.crate_name, "claudefs_storage");
    }

    #[test]
    fn test_cargo_json_parsing() {
        let json_output = r#"{"type":"started","name":"test_foo"}
{"type":"ok","name":"test_foo","duration":0.123}
{"type":"started","name":"test_bar"}
{"type":"failed","name":"test_bar","duration":0.456}
{"type":"skipped","name":"test_skipped"}"#;

        let collector = TestCollector::from_cargo_output(json_output).unwrap();
        let summary = collector.summary();

        assert_eq!(summary.total_tests, 3);
        assert_eq!(summary.total_passed, 1);
        assert_eq!(summary.total_failed, 1);
        assert_eq!(summary.total_skipped, 1);
    }

    #[test]
    fn test_cargo_test_event_parsing() {
        let json_output = r#"{"type":"test","name":"my_test","status":"passed","duration":1.0}"#;
        let collector = TestCollector::from_cargo_output(json_output).unwrap();
        let summary = collector.summary();
        assert_eq!(summary.total_passed, 1);
    }

    #[test]
    fn test_extract_crate_name() {
        assert_eq!(
            extract_crate_name("claudefs_storage::tests::test_foo"),
            "claudefs_storage"
        );
        assert_eq!(
            extract_crate_name("claudefs_meta::module::test"),
            "claudefs_meta"
        );
        assert_eq!(extract_crate_name("simple_test"), "simple_test");
    }

    #[test]
    fn test_get_report_nonexistent_crate() {
        let collector = TestCollector::new();
        let report = collector.get_report("nonexistent");
        assert!(report.is_none());
    }

    #[test]
    fn test_summary_empty() {
        let collector = TestCollector::new();
        let summary = collector.summary();
        assert_eq!(summary.total_tests, 0);
        assert_eq!(summary.total_passed, 0);
        assert_eq!(summary.total_failed, 0);
        assert_eq!(summary.total_skipped, 0);
    }

    #[test]
    fn test_multiple_crates_summary() {
        let mut collector = TestCollector::new();
        collector.add_result(
            "crate_a",
            TestCaseResult::new("t1".to_string(), TestStatus::Pass, 10),
        );
        collector.add_result(
            "crate_a",
            TestCaseResult::new("t2".to_string(), TestStatus::Fail, 20),
        );
        collector.add_result(
            "crate_b",
            TestCaseResult::new("t3".to_string(), TestStatus::Pass, 30),
        );

        let summary = collector.summary();
        assert_eq!(summary.total_tests, 3);
        assert_eq!(summary.total_passed, 2);
        assert_eq!(summary.total_failed, 1);
        assert!(summary.crates.contains(&"crate_a".to_string()));
        assert!(summary.crates.contains(&"crate_b".to_string()));
    }
}
