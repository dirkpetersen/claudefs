use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseResult {
    pub name: String,
    pub suite: String,
    pub status: TestStatus,
    pub duration: Duration,
    pub message: Option<String>,
    pub tags: Vec<String>,
}

impl TestCaseResult {
    pub fn new(name: &str, suite: &str, status: TestStatus, duration: Duration) -> Self {
        Self {
            name: name.to_string(),
            suite: suite.to_string(),
            status,
            duration,
            message: None,
            tags: vec![],
        }
    }

    pub fn with_message(mut self, message: &str) -> Self {
        self.message = Some(message.to_string());
        self
    }

    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuiteReport {
    pub name: String,
    pub timestamp: u64,
    pub duration: Duration,
    pub cases: Vec<TestCaseResult>,
}

impl TestSuiteReport {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            duration: Duration::default(),
            cases: vec![],
        }
    }

    pub fn add_result(&mut self, result: TestCaseResult) {
        self.cases.push(result);
    }

    pub fn passed(&self) -> usize {
        self.cases
            .iter()
            .filter(|c| matches!(c.status, TestStatus::Pass))
            .count()
    }

    pub fn failed(&self) -> usize {
        self.cases
            .iter()
            .filter(|c| matches!(c.status, TestStatus::Fail | TestStatus::Error))
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.cases
            .iter()
            .filter(|c| matches!(c.status, TestStatus::Skip))
            .count()
    }

    pub fn total(&self) -> usize {
        self.cases.len()
    }

    pub fn pass_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        self.passed() as f64 / total as f64
    }

    pub fn is_passing(&self) -> bool {
        self.failed() == 0
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_junit_xml(&self) -> String {
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"{}\" time=\"{}\">\n",
            self.name,
            self.total(),
            self.failed(),
            self.skipped(),
            self.failed(),
            self.duration.as_secs_f64()
        ));

        for case in &self.cases {
            let status = match case.status {
                TestStatus::Pass => "passed",
                TestStatus::Fail => "failed",
                TestStatus::Skip => "skipped",
                TestStatus::Error => "failed",
            };
            xml.push_str(&format!(
                "  <testcase name=\"{}\" classname=\"{}\" time=\"{}\">\n",
                case.name,
                case.suite,
                case.duration.as_secs_f64()
            ));
            if matches!(case.status, TestStatus::Fail | TestStatus::Error) {
                if let Some(msg) = &case.message {
                    xml.push_str(&format!(
                        "    <failure message=\"{}\">{}</failure>\n",
                        msg, msg
                    ));
                } else {
                    xml.push_str("    <failure message=\"test failed\" />\n");
                }
            } else if matches!(case.status, TestStatus::Skip) {
                xml.push_str("    <skipped />\n");
            }
            xml.push_str("  </testcase>\n");
        }

        xml.push_str("</testsuite>");
        xml
    }

    pub fn summary_line(&self) -> String {
        let total = self.total();
        let passed = self.passed();
        let failed = self.failed();
        let skipped = self.skipped();
        let time = self.duration.as_secs_f64();

        if failed > 0 {
            format!(
                "FAIL {}/{} ({} failed, {} skipped) in {:.2}s",
                passed, total, failed, skipped, time
            )
        } else if skipped > 0 {
            format!(
                "PASS {}/{} ({} skipped) in {:.2}s",
                passed, total, skipped, time
            )
        } else {
            format!("PASS {}/{} in {:.2}s", passed, total, time)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateReport {
    pub suites: Vec<TestSuiteReport>,
    pub generated_at: u64,
}

impl AggregateReport {
    pub fn new() -> Self {
        Self {
            suites: vec![],
            generated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    pub fn add_suite(&mut self, suite: TestSuiteReport) {
        self.suites.push(suite);
    }

    pub fn total_passed(&self) -> usize {
        self.suites.iter().map(|s| s.passed()).sum()
    }

    pub fn total_failed(&self) -> usize {
        self.suites.iter().map(|s| s.failed()).sum()
    }

    pub fn total_tests(&self) -> usize {
        self.suites.iter().map(|s| s.total()).sum()
    }

    pub fn overall_pass_rate(&self) -> f64 {
        let total = self.total_tests();
        if total == 0 {
            return 0.0;
        }
        self.total_passed() as f64 / total as f64
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn print_summary(&self) {
        println!("=== Test Report Summary ===");
        println!("Generated at: {}", self.generated_at);
        println!("Total suites: {}", self.suites.len());
        println!("Total tests: {}", self.total_tests());
        println!("Passed: {}", self.total_passed());
        println!("Failed: {}", self.total_failed());
        println!("Pass rate: {:.1}%", self.overall_pass_rate() * 100.0);
        println!();

        for suite in &self.suites {
            println!("Suite: {}", suite.name);
            println!("  {}", suite.summary_line());
        }
    }
}

impl Default for AggregateReport {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ReportBuilder {
    suite: TestSuiteReport,
    start_time: std::time::Instant,
}

impl ReportBuilder {
    pub fn new(suite_name: &str) -> Self {
        Self {
            suite: TestSuiteReport::new(suite_name),
            start_time: std::time::Instant::now(),
        }
    }

    pub fn pass(&mut self, test_name: &str, duration: Duration) -> &mut Self {
        self.suite.add_result(TestCaseResult::new(
            test_name,
            &self.suite.name,
            TestStatus::Pass,
            duration,
        ));
        self
    }

    pub fn fail(&mut self, test_name: &str, duration: Duration, message: &str) -> &mut Self {
        self.suite.add_result(
            TestCaseResult::new(test_name, &self.suite.name, TestStatus::Fail, duration)
                .with_message(message),
        );
        self
    }

    pub fn skip(&mut self, test_name: &str, reason: &str) -> &mut Self {
        self.suite.add_result(
            TestCaseResult::new(
                test_name,
                &self.suite.name,
                TestStatus::Skip,
                Duration::default(),
            )
            .with_message(reason),
        );
        self
    }

    pub fn build(self) -> TestSuiteReport {
        let elapsed = self.start_time.elapsed();
        let mut suite = self.suite;
        suite.duration = elapsed;
        suite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_case_result_creation() {
        let result = TestCaseResult::new(
            "test1",
            "suite1",
            TestStatus::Pass,
            Duration::from_millis(100),
        );
        assert_eq!(result.name, "test1");
        assert_eq!(result.suite, "suite1");
        assert!(matches!(result.status, TestStatus::Pass));
    }

    #[test]
    fn test_test_suite_report_new() {
        let report = TestSuiteReport::new("my_suite");
        assert_eq!(report.name, "my_suite");
        assert_eq!(report.total(), 0);
    }

    #[test]
    fn test_test_suite_report_add_result() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        assert_eq!(report.total(), 1);
    }

    #[test]
    fn test_passed_count() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        report.add_result(TestCaseResult::new(
            "test2",
            "my_suite",
            TestStatus::Fail,
            Duration::default(),
        ));
        assert_eq!(report.passed(), 1);
    }

    #[test]
    fn test_failed_count() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        report.add_result(TestCaseResult::new(
            "test2",
            "my_suite",
            TestStatus::Fail,
            Duration::default(),
        ));
        assert_eq!(report.failed(), 1);
    }

    #[test]
    fn test_skipped_count() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Skip,
            Duration::default(),
        ));
        report.add_result(TestCaseResult::new(
            "test2",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        assert_eq!(report.skipped(), 1);
    }

    #[test]
    fn test_pass_rate_calculation() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        report.add_result(TestCaseResult::new(
            "test2",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        report.add_result(TestCaseResult::new(
            "test3",
            "my_suite",
            TestStatus::Fail,
            Duration::default(),
        ));
        report.add_result(TestCaseResult::new(
            "test4",
            "my_suite",
            TestStatus::Fail,
            Duration::default(),
        ));
        assert_eq!(report.pass_rate(), 0.5);
    }

    #[test]
    fn test_pass_rate_empty() {
        let report = TestSuiteReport::new("empty");
        assert_eq!(report.pass_rate(), 0.0);
    }

    #[test]
    fn test_is_passing() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        assert!(report.is_passing());
    }

    #[test]
    fn test_is_passing_with_failures() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        report.add_result(TestCaseResult::new(
            "test2",
            "my_suite",
            TestStatus::Fail,
            Duration::default(),
        ));
        assert!(!report.is_passing());
    }

    #[test]
    fn test_to_json_from_json_roundtrip() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::from_millis(100),
        ));
        let json = report.to_json().unwrap();
        let parsed = TestSuiteReport::from_json(&json).unwrap();
        assert_eq!(parsed.name, report.name);
        assert_eq!(parsed.total(), report.total());
    }

    #[test]
    fn test_to_junit_xml() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::from_millis(100),
        ));
        report.add_result(
            TestCaseResult::new(
                "test2",
                "my_suite",
                TestStatus::Fail,
                Duration::from_millis(50),
            )
            .with_message("assertion failed"),
        );
        let xml = report.to_junit_xml();
        assert!(xml.contains("testsuite"));
        assert!(xml.contains("testcase"));
        assert!(xml.contains("failure"));
    }

    #[test]
    fn test_summary_line_passing() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::from_millis(100),
        ));
        report.duration = Duration::from_millis(1234);
        let summary = report.summary_line();
        assert!(summary.contains("PASS"));
    }

    #[test]
    fn test_summary_line_failing() {
        let mut report = TestSuiteReport::new("my_suite");
        report.add_result(TestCaseResult::new(
            "test1",
            "my_suite",
            TestStatus::Pass,
            Duration::default(),
        ));
        report.add_result(TestCaseResult::new(
            "test2",
            "my_suite",
            TestStatus::Fail,
            Duration::default(),
        ));
        report.duration = Duration::from_millis(500);
        let summary = report.summary_line();
        assert!(summary.contains("FAIL"));
    }

    #[test]
    fn test_aggregate_report_new() {
        let report = AggregateReport::new();
        assert_eq!(report.suites.len(), 0);
    }

    #[test]
    fn test_aggregate_report_add_suite() {
        let mut report = AggregateReport::new();
        report.add_suite(TestSuiteReport::new("suite1"));
        report.add_suite(TestSuiteReport::new("suite2"));
        assert_eq!(report.suites.len(), 2);
    }

    #[test]
    fn test_total_passed() {
        let mut report = AggregateReport::new();
        let mut suite1 = TestSuiteReport::new("suite1");
        suite1.add_result(TestCaseResult::new(
            "t1",
            "suite1",
            TestStatus::Pass,
            Duration::default(),
        ));
        let mut suite2 = TestSuiteReport::new("suite2");
        suite2.add_result(TestCaseResult::new(
            "t2",
            "suite2",
            TestStatus::Pass,
            Duration::default(),
        ));
        suite2.add_result(TestCaseResult::new(
            "t3",
            "suite2",
            TestStatus::Pass,
            Duration::default(),
        ));
        report.add_suite(suite1);
        report.add_suite(suite2);
        assert_eq!(report.total_passed(), 3);
    }

    #[test]
    fn test_total_failed() {
        let mut report = AggregateReport::new();
        let mut suite1 = TestSuiteReport::new("suite1");
        suite1.add_result(TestCaseResult::new(
            "t1",
            "suite1",
            TestStatus::Fail,
            Duration::default(),
        ));
        report.add_suite(suite1);
        assert_eq!(report.total_failed(), 1);
    }

    #[test]
    fn test_total_tests() {
        let mut report = AggregateReport::new();
        let mut suite1 = TestSuiteReport::new("suite1");
        suite1.add_result(TestCaseResult::new(
            "t1",
            "suite1",
            TestStatus::Pass,
            Duration::default(),
        ));
        suite1.add_result(TestCaseResult::new(
            "t2",
            "suite1",
            TestStatus::Fail,
            Duration::default(),
        ));
        report.add_suite(suite1);
        assert_eq!(report.total_tests(), 2);
    }

    #[test]
    fn test_overall_pass_rate() {
        let mut report = AggregateReport::new();
        let mut suite1 = TestSuiteReport::new("suite1");
        suite1.add_result(TestCaseResult::new(
            "t1",
            "suite1",
            TestStatus::Pass,
            Duration::default(),
        ));
        suite1.add_result(TestCaseResult::new(
            "t2",
            "suite1",
            TestStatus::Fail,
            Duration::default(),
        ));
        report.add_suite(suite1);
        assert_eq!(report.overall_pass_rate(), 0.5);
    }

    #[test]
    fn test_aggregate_to_json_from_json() {
        let mut report = AggregateReport::new();
        report.add_suite(TestSuiteReport::new("suite1"));
        let json = report.to_json().unwrap();
        let parsed = AggregateReport::from_json(&json).unwrap();
        assert_eq!(parsed.suites.len(), 1);
    }

    #[test]
    fn test_report_builder_pass() {
        let mut builder = ReportBuilder::new("my_suite");
        builder.pass("test1", Duration::from_millis(100));
        let report = builder.build();
        assert_eq!(report.passed(), 1);
    }

    #[test]
    fn test_report_builder_fail() {
        let mut builder = ReportBuilder::new("my_suite");
        builder.fail("test1", Duration::from_millis(100), "assertion failed");
        let report = builder.build();
        assert_eq!(report.failed(), 1);
    }

    #[test]
    fn test_report_builder_skip() {
        let mut builder = ReportBuilder::new("my_suite");
        builder.skip("test1", "not implemented");
        let report = builder.build();
        assert_eq!(report.skipped(), 1);
    }

    #[test]
    fn test_report_builder_build() {
        let builder = ReportBuilder::new("my_suite");
        let report = builder.build();
        assert_eq!(report.name, "my_suite");
    }

    #[test]
    fn test_test_case_result_with_message() {
        let result = TestCaseResult::new("test1", "suite1", TestStatus::Fail, Duration::default())
            .with_message("error message");
        assert_eq!(result.message, Some("error message".to_string()));
    }

    #[test]
    fn test_test_case_result_with_tag() {
        let result = TestCaseResult::new("test1", "suite1", TestStatus::Pass, Duration::default())
            .with_tag("unit")
            .with_tag("fast");
        assert_eq!(result.tags.len(), 2);
    }

    #[test]
    fn test_test_status_variants() {
        assert!(matches!(TestStatus::Pass, TestStatus::Pass));
        assert!(matches!(TestStatus::Fail, TestStatus::Fail));
        assert!(matches!(TestStatus::Skip, TestStatus::Skip));
        assert!(matches!(TestStatus::Error, TestStatus::Error));
    }
}
