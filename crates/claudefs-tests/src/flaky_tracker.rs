//! Flaky Test Tracker
//!
//! Detects intermittently failing tests by tracking run history and calculating flake rates.
//! This module identifies tests with flake rates above a configured threshold and helps
//! developers identify race conditions or environment-dependent issues.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Test status for flaky tracking
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
}

/// Single test run record
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestRun {
    pub timestamp: u64,
    pub status: TestStatus,
    pub duration_ms: u64,
}

/// Complete flaky test record
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlakyTestRecord {
    pub name: String,
    pub total_runs: usize,
    pub failures: usize,
    pub flake_rate: f64,
    pub recent_runs: Vec<TestStatus>,
    pub last_run_time: u64,
}

impl FlakyTestRecord {
    /// Check if test exceeds flake threshold
    pub fn is_suspicious(&self, threshold: f64) -> bool {
        self.flake_rate > threshold
    }

    /// Get human-readable pattern (e.g., "PASS FAIL PASS FAIL")
    pub fn pattern_str(&self) -> String {
        self.recent_runs
            .iter()
            .map(|s| match s {
                TestStatus::Pass => "✓",
                TestStatus::Fail => "✗",
                TestStatus::Skip => "-",
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Flaky test tracker with history persistence
pub struct FlakyTracker {
    history: std::collections::HashMap<String, VecDeque<TestRun>>,
    max_history: usize,
    threshold: f64,
}

impl FlakyTracker {
    /// Create new flaky tracker
    pub fn new(max_history: usize) -> Self {
        Self {
            history: std::collections::HashMap::new(),
            max_history,
            threshold: 0.05, // 5% default
        }
    }

    /// Set flake rate threshold (0.0-1.0)
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Add test result to history
    pub fn add_result(&mut self, test_name: impl Into<String>, status: TestStatus, duration_ms: u64) {
        let test_name = test_name.into();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let run = TestRun {
            timestamp,
            status,
            duration_ms,
        };

        let entry = self.history.entry(test_name).or_insert_with(VecDeque::new);
        entry.push_back(run);

        // Trim to max history
        while entry.len() > self.max_history {
            entry.pop_front();
        }
    }

    /// Calculate flake rate for a test (failures / total)
    pub fn calculate_flake_rate(&self, test_name: &str) -> f64 {
        match self.history.get(test_name) {
            None => 0.0,
            Some(runs) => {
                if runs.is_empty() {
                    0.0
                } else {
                    let failures = runs.iter().filter(|r| r.status == TestStatus::Fail).count();
                    failures as f64 / runs.len() as f64
                }
            }
        }
    }

    /// Get full record for a test
    pub fn get_record(&self, test_name: &str) -> Option<FlakyTestRecord> {
        self.history.get(test_name).map(|runs| {
            let failures = runs.iter().filter(|r| r.status == TestStatus::Fail).count();
            let flake_rate = if runs.is_empty() {
                0.0
            } else {
                failures as f64 / runs.len() as f64
            };

            let recent_runs: Vec<TestStatus> = runs.iter().map(|r| r.status).collect();
            let last_run_time = runs.back().map(|r| r.timestamp).unwrap_or(0);

            FlakyTestRecord {
                name: test_name.to_string(),
                total_runs: runs.len(),
                failures,
                flake_rate,
                recent_runs,
                last_run_time,
            }
        })
    }

    /// Find all tests exceeding flake threshold
    pub fn suspicious_tests(&self) -> Vec<FlakyTestRecord> {
        self.history
            .keys()
            .filter_map(|name| self.get_record(name))
            .filter(|r| r.is_suspicious(self.threshold))
            .collect()
    }

    /// Load history from JSON file
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new(40));
        }

        let content = fs::read_to_string(path)?;
        let history: std::collections::HashMap<String, Vec<TestRun>> = serde_json::from_str(&content)?;

        let mut tracker = Self::new(40);
        for (name, runs) in history {
            let mut deque = VecDeque::new();
            for run in runs {
                deque.push_back(run);
            }
            tracker.history.insert(name, deque);
        }

        Ok(tracker)
    }

    /// Save history to JSON file
    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        // Convert to serializable format
        let mut save_data: std::collections::HashMap<String, Vec<TestRun>> =
            std::collections::HashMap::new();

        for (name, runs) in &self.history {
            save_data.insert(name.clone(), runs.iter().cloned().collect());
        }

        let json = serde_json::to_string_pretty(&save_data)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        fs::write(path, json)?;
        Ok(())
    }

    /// Generate GitHub issue body for suspicious tests
    pub fn generate_issue_body(&self) -> Option<String> {
        let suspicious = self.suspicious_tests();
        if suspicious.is_empty() {
            return None;
        }

        let mut body = String::from("## 🐛 Flaky Tests Detected\n\n");
        body.push_str(&format!("Found {} tests with flake rate > {:.1}%\n\n", suspicious.len(), self.threshold * 100.0));

        for test in suspicious.iter().take(5) {
            body.push_str(&format!("### {}\n\n", test.name));
            body.push_str(&format!("**Flake Rate:** {:.1}% ({}/{} failures)\n",
                test.flake_rate * 100.0, test.failures, test.total_runs));
            body.push_str(&format!("**Pattern:** {}\n\n", test.pattern_str()));
        }

        if suspicious.len() > 5 {
            body.push_str(&format!("... and {} more flaky tests\n\n", suspicious.len() - 5));
        }

        body.push_str("### Recommendations\n");
        body.push_str("- Increase timing buffers for timing-sensitive tests\n");
        body.push_str("- Review for race conditions or resource cleanup\n");
        body.push_str("- Add test isolation/fixtures\n");
        body.push_str("- Mark as `#[ignore]` if blocking CI\n");

        Some(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tracker() {
        let tracker = FlakyTracker::new(40);
        assert_eq!(tracker.calculate_flake_rate("nonexistent"), 0.0);
        assert!(tracker.suspicious_tests().is_empty());
    }

    #[test]
    fn test_single_pass() {
        let mut tracker = FlakyTracker::new(40);
        tracker.add_result("test1", TestStatus::Pass, 100);
        assert_eq!(tracker.calculate_flake_rate("test1"), 0.0);
        assert!(tracker.get_record("test1").is_some());
    }

    #[test]
    fn test_flake_rate_calculation() {
        let mut tracker = FlakyTracker::new(40);
        for _ in 0..8 {
            tracker.add_result("test2", TestStatus::Pass, 50);
        }
        for _ in 0..2 {
            tracker.add_result("test2", TestStatus::Fail, 50);
        }
        assert_eq!(tracker.calculate_flake_rate("test2"), 0.2); // 2/10 = 20%
    }

    #[test]
    fn test_suspicious_tests() {
        let mut tracker = FlakyTracker::with_threshold(FlakyTracker::new(40), 0.1); // 10%

        // Non-suspicious: 1/50 = 2%
        for _ in 0..49 {
            tracker.add_result("stable", TestStatus::Pass, 50);
        }
        tracker.add_result("stable", TestStatus::Fail, 50);

        // Suspicious: 5/40 = 12.5%
        for _ in 0..35 {
            tracker.add_result("flaky", TestStatus::Pass, 50);
        }
        for _ in 0..5 {
            tracker.add_result("flaky", TestStatus::Fail, 50);
        }

        let suspicious = tracker.suspicious_tests();
        assert_eq!(suspicious.len(), 1);
        assert_eq!(suspicious[0].name, "flaky");
    }

    #[test]
    fn test_max_history_rotation() {
        let mut tracker = FlakyTracker::new(3);
        tracker.add_result("test", TestStatus::Pass, 50);
        tracker.add_result("test", TestStatus::Pass, 50);
        tracker.add_result("test", TestStatus::Pass, 50);
        tracker.add_result("test", TestStatus::Pass, 50); // Should pop first

        let record = tracker.get_record("test").unwrap();
        assert_eq!(record.total_runs, 3); // Only last 3 kept
    }

    #[test]
    fn test_record_structure() {
        let mut tracker = FlakyTracker::new(40);
        tracker.add_result("test", TestStatus::Pass, 100);
        tracker.add_result("test", TestStatus::Fail, 120);

        let record = tracker.get_record("test").unwrap();
        assert_eq!(record.name, "test");
        assert_eq!(record.total_runs, 2);
        assert_eq!(record.failures, 1);
        assert_eq!(record.flake_rate, 0.5);
        assert_eq!(record.recent_runs.len(), 2);
    }

    #[test]
    fn test_pattern_string() {
        let record = FlakyTestRecord {
            name: "test".to_string(),
            total_runs: 5,
            failures: 2,
            flake_rate: 0.4,
            recent_runs: vec![TestStatus::Pass, TestStatus::Fail, TestStatus::Pass, TestStatus::Fail, TestStatus::Pass],
            last_run_time: 12345,
        };
        assert_eq!(record.pattern_str(), "✓ ✗ ✓ ✗ ✓");
    }

    #[test]
    fn test_is_suspicious() {
        let record = FlakyTestRecord {
            name: "test".to_string(),
            total_runs: 100,
            failures: 7,
            flake_rate: 0.07,
            recent_runs: vec![],
            last_run_time: 0,
        };
        assert!(!record.is_suspicious(0.1)); // 7% < 10%
        assert!(record.is_suspicious(0.05)); // 7% > 5%
    }
}
