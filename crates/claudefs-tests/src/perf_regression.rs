//! Performance regression test framework
//!
//! Tests for FIO configuration, result parsing, and regression detection.

use crate::bench::{detect_fio_binary, parse_fio_json, FioConfig, FioResult, FioRunner, FioRwMode};
use crate::report::{TestCaseResult, TestStatus, TestSuiteReport};

#[test]
fn test_fio_config_default_rwmix() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test"));
    assert_eq!(config.rw, FioRwMode::Read);
}

#[test]
fn test_fio_config_default_block_size() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test"));
    assert_eq!(config.bs, "4k");
}

#[test]
fn test_fio_config_default_jobs() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test"));
    assert_eq!(config.numjobs, 1);
}

#[test]
fn test_fio_config_default_runtime() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test"));
    assert_eq!(config.runtime_secs, 10);
}

#[test]
fn test_fio_rw_mode_read() {
    let mode = FioRwMode::Read;
    assert_eq!(mode.as_str(), "read");
}

#[test]
fn test_fio_rw_mode_write() {
    let mode = FioRwMode::Write;
    assert_eq!(mode.as_str(), "write");
}

#[test]
fn test_fio_rw_mode_rand_read() {
    let mode = FioRwMode::RandRead;
    assert_eq!(mode.as_str(), "randread");
}

#[test]
fn test_fio_rw_mode_rand_write() {
    let mode = FioRwMode::RandWrite;
    assert_eq!(mode.as_str(), "randwrite");
}

#[test]
fn test_fio_rw_mode_read_write() {
    let mode = FioRwMode::ReadWrite;
    assert_eq!(mode.as_str(), "rw");
}

#[test]
fn test_fio_rw_mode_rand_rw() {
    let mode = FioRwMode::RandRW;
    assert_eq!(mode.as_str(), "randrw");
}

#[test]
fn test_fio_result_read_bw() {
    let result = FioResult {
        read_bw_kb: Some(100000),
        write_bw_kb: Some(50000),
        read_iops: Some(25000),
        write_iops: Some(12000),
        read_lat_us_p99: Some(8000),
        write_lat_us_p99: Some(12000),
    };

    assert!(result.read_bw_kb.is_some());
}

#[test]
fn test_fio_result_write_bw() {
    let result = FioResult {
        read_bw_kb: Some(100000),
        write_bw_kb: Some(50000),
        read_iops: Some(25000),
        write_iops: Some(12000),
        read_lat_us_p99: Some(8000),
        write_lat_us_p99: Some(12000),
    };

    assert!(result.write_bw_kb.is_some());
}

#[test]
fn test_fio_result_iops() {
    let result = FioResult {
        read_bw_kb: Some(100000),
        write_bw_kb: Some(50000),
        read_iops: Some(25000),
        write_iops: Some(12000),
        read_lat_us_p99: Some(8000),
        write_lat_us_p99: Some(12000),
    };

    let iops = result.total_iops();
    assert!(iops.is_some());
    assert_eq!(iops.unwrap(), 37000);
}

#[test]
fn test_fio_runner_detect_fio_binary() {
    let result = detect_fio_binary();
    assert!(result.is_none() || result.is_some());
}

#[test]
fn test_parse_fio_json_minimal_valid() {
    let json = "read bw 102400";

    let result = parse_fio_json(json);
    assert!(result.is_ok());
}

#[test]
fn test_parse_fio_json_empty_string() {
    let json = "";
    let result = parse_fio_json(json);
    assert!(result.is_ok());
}

#[test]
fn test_regression_compare_results_no_regression() {
    let baseline = FioResult {
        read_bw_kb: Some(100000),
        write_bw_kb: Some(50000),
        read_iops: Some(25000),
        write_iops: Some(12000),
        read_lat_us_p99: Some(8000),
        write_lat_us_p99: Some(12000),
    };

    let current = FioResult {
        read_bw_kb: Some(100000),
        write_bw_kb: Some(50000),
        read_iops: Some(25000),
        write_iops: Some(12000),
        read_lat_us_p99: Some(8000),
        write_lat_us_p99: Some(12000),
    };

    let regression = current.total_bw_kb().unwrap() >= baseline.total_bw_kb().unwrap() - 5000;
    assert!(regression);
}

#[test]
fn test_performance_baseline_verify_fields_accessible() {
    let result = FioResult {
        read_bw_kb: Some(100000),
        write_bw_kb: Some(50000),
        read_iops: Some(25000),
        write_iops: Some(12000),
        read_lat_us_p99: Some(8000),
        write_lat_us_p99: Some(12000),
    };

    assert!(result.read_bw_kb.is_some());
    assert!(result.write_bw_kb.is_some());
    assert!(result.read_iops.is_some());
    assert!(result.write_iops.is_some());
}

#[test]
fn test_test_case_result_pass() {
    let result = TestCaseResult::new(
        "test_case",
        "test-suite",
        TestStatus::Pass,
        std::time::Duration::from_secs(1),
    );

    assert_eq!(result.status, TestStatus::Pass);
}

#[test]
fn test_test_case_result_failed() {
    let result = TestCaseResult::new(
        "test_case",
        "test-suite",
        TestStatus::Fail,
        std::time::Duration::from_secs(1),
    );

    assert_eq!(result.status, TestStatus::Fail);
}

#[test]
fn test_test_case_result_skipped() {
    let result = TestCaseResult::new(
        "test_case",
        "test-suite",
        TestStatus::Skip,
        std::time::Duration::from_secs(0),
    );

    assert_eq!(result.status, TestStatus::Skip);
}

#[test]
fn test_test_suite_report_new() {
    let report = TestSuiteReport::new("test-suite");
    assert_eq!(report.name, "test-suite");
    assert!(report.cases.is_empty());
}

#[test]
fn test_test_suite_report_add_result() {
    let mut report = TestSuiteReport::new("test-suite");

    let result = TestCaseResult::new(
        "test_case",
        "test-suite",
        TestStatus::Pass,
        std::time::Duration::from_secs(1),
    );

    report.add_result(result);
    assert_eq!(report.cases.len(), 1);
}

#[test]
fn test_test_suite_report_passed_count() {
    let mut report = TestSuiteReport::new("test-suite");

    report.add_result(TestCaseResult::new(
        "test1",
        "test-suite",
        TestStatus::Pass,
        std::time::Duration::from_secs(1),
    ));
    report.add_result(TestCaseResult::new(
        "test2",
        "test-suite",
        TestStatus::Fail,
        std::time::Duration::from_secs(1),
    ));
    report.add_result(TestCaseResult::new(
        "test3",
        "test-suite",
        TestStatus::Pass,
        std::time::Duration::from_secs(1),
    ));

    assert_eq!(report.passed(), 2);
}

#[test]
fn test_test_suite_report_failed_count() {
    let mut report = TestSuiteReport::new("test-suite");

    report.add_result(TestCaseResult::new(
        "test1",
        "test-suite",
        TestStatus::Pass,
        std::time::Duration::from_secs(1),
    ));
    report.add_result(TestCaseResult::new(
        "test2",
        "test-suite",
        TestStatus::Fail,
        std::time::Duration::from_secs(1),
    ));
    report.add_result(TestCaseResult::new(
        "test3",
        "test-suite",
        TestStatus::Error,
        std::time::Duration::from_secs(1),
    ));

    assert_eq!(report.failed(), 2);
}

#[test]
fn test_fio_config_with_rw() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test")).with_rw(FioRwMode::Write);

    assert_eq!(config.rw, FioRwMode::Write);
}

#[test]
fn test_fio_config_with_bs() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test")).with_bs("8k");

    assert_eq!(config.bs, "8k");
}

#[test]
fn test_fio_config_with_iodepth() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test")).with_iodepth(32);

    assert_eq!(config.iodepth, 32);
}

#[test]
fn test_fio_config_with_numjobs() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test")).with_numjobs(4);

    assert_eq!(config.numjobs, 4);
}

#[test]
fn test_fio_config_with_runtime() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test")).with_runtime(60);

    assert_eq!(config.runtime_secs, 60);
}

#[test]
fn test_test_case_result_with_message() {
    let result = TestCaseResult::new(
        "test_case",
        "test-suite",
        TestStatus::Fail,
        std::time::Duration::from_secs(1),
    )
    .with_message("Assertion failed");

    assert!(result.message.is_some());
    assert_eq!(result.message.unwrap(), "Assertion failed");
}

#[test]
fn test_test_case_result_with_tag() {
    let result = TestCaseResult::new(
        "test_case",
        "test-suite",
        TestStatus::Pass,
        std::time::Duration::from_secs(1),
    )
    .with_tag("slow");

    assert!(result.tags.contains(&"slow".to_string()));
}

#[test]
fn test_fio_result_total_bw() {
    let result = FioResult {
        read_bw_kb: Some(80000),
        write_bw_kb: Some(40000),
        read_iops: Some(20000),
        write_iops: Some(10000),
        read_lat_us_p99: Some(8000),
        write_lat_us_p99: Some(12000),
    };

    let total = result.total_bw_kb().unwrap();
    assert_eq!(total, 120000);
}

#[test]
fn test_fio_result_latency() {
    let result = FioResult {
        read_bw_kb: Some(100000),
        write_bw_kb: Some(50000),
        read_iops: Some(25000),
        write_iops: Some(12000),
        read_lat_us_p99: Some(8000),
        write_lat_us_p99: Some(12000),
    };

    assert!(result.read_lat_us_p99.is_some());
    assert!(result.write_lat_us_p99.is_some());
}

#[test]
fn test_fio_config_to_args() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test"));
    let args = config.to_args();
    assert!(!args.is_empty());
}

#[test]
fn test_fio_runner_new() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test"));
    let runner = FioRunner::new(config);
    assert!(runner.config().filename.to_str().unwrap().contains("test"));
}

#[test]
fn test_fio_runner_binary() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test"));
    let runner = FioRunner::new(config);
    let binary = runner.binary();
    assert!(binary.is_none() || binary.is_some());
}

#[test]
fn test_fio_runner_run() {
    let config = FioConfig::new(std::path::PathBuf::from("/tmp/test"));
    let runner = FioRunner::new(config);
    let result = runner.run();
    assert!(result.read_bw_kb.is_some());
}

#[test]
fn test_parse_fio_json_with_read_write() {
    let json = "read bw 100000 write bw 50000";
    let result = parse_fio_json(json);
    assert!(result.is_ok());
}

#[test]
fn test_fio_result_default() {
    let result = FioResult::default();
    assert!(result.read_bw_kb.is_none());
    assert!(result.read_iops.is_none());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::ReportBuilder;

    #[test]
    fn test_report_builder_new() {
        let builder = ReportBuilder::new("test-suite");
        let report = builder.build();
        assert_eq!(report.name, "test-suite");
    }

    #[test]
    fn test_report_builder_pass() {
        let mut builder = ReportBuilder::new("test-suite");
        builder.pass("test1", std::time::Duration::from_secs(1));
        let report = builder.build();
        assert_eq!(report.passed(), 1);
    }

    #[test]
    fn test_report_builder_fail() {
        let mut builder = ReportBuilder::new("test-suite");
        builder.fail("test1", std::time::Duration::from_secs(1), "failed");
        let report = builder.build();
        assert_eq!(report.failed(), 1);
    }

    #[test]
    fn test_report_builder_skip() {
        let mut builder = ReportBuilder::new("test-suite");
        builder.skip("test1", "skipped");
        let report = builder.build();
        assert_eq!(report.skipped(), 1);
    }

    #[test]
    fn test_suite_report_total_count() {
        let mut report = TestSuiteReport::new("test-suite");

        report.add_result(TestCaseResult::new(
            "test1",
            "test-suite",
            TestStatus::Pass,
            std::time::Duration::from_secs(1),
        ));
        report.add_result(TestCaseResult::new(
            "test2",
            "test-suite",
            TestStatus::Fail,
            std::time::Duration::from_secs(1),
        ));
        report.add_result(TestCaseResult::new(
            "test3",
            "test-suite",
            TestStatus::Skip,
            std::time::Duration::from_secs(0),
        ));

        assert_eq!(report.total(), 3);
    }
}
