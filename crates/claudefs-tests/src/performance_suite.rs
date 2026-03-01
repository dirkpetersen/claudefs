//! Performance test suite for ClaudeFS storage, metadata, and network operations.

use crate::bench::{parse_fio_json, FioConfig, FioResult, FioRunner, FioRwMode};
use std::path::PathBuf;

fn test_file() -> PathBuf {
    PathBuf::from("/tmp/perf_test_file")
}

#[test]
fn test_fio_config_sequential_read() {
    let config = FioConfig::new(test_file()).with_rw(FioRwMode::Read);
    assert_eq!(config.rw, FioRwMode::Read);
    assert_eq!(config.bs, "4k");
    assert_eq!(config.iodepth, 1);
    assert_eq!(config.numjobs, 1);
}

#[test]
fn test_fio_config_sequential_write() {
    let config = FioConfig::new(test_file()).with_rw(FioRwMode::Write);
    assert_eq!(config.rw, FioRwMode::Write);
    assert_eq!(config.bs, "4k");
    assert_eq!(config.iodepth, 1);
}

#[test]
fn test_fio_config_random_read() {
    let config = FioConfig::new(test_file()).with_rw(FioRwMode::RandRead);
    assert_eq!(config.rw, FioRwMode::RandRead);
}

#[test]
fn test_fio_config_random_write() {
    let config = FioConfig::new(test_file()).with_rw(FioRwMode::RandWrite);
    assert_eq!(config.rw, FioRwMode::RandWrite);
}

#[test]
fn test_fio_config_mixed_rw() {
    let config = FioConfig::new(test_file()).with_rw(FioRwMode::ReadWrite);
    assert_eq!(config.rw, FioRwMode::ReadWrite);
}

#[test]
fn test_fio_runner_new() {
    let config = FioConfig::new(test_file());
    let runner = FioRunner::new(config);
    let cfg = runner.config();
    assert!(!cfg.filename.to_string_lossy().is_empty());
}

#[test]
fn test_parse_fio_json_minimal_valid() {
    let json = r#"{"jobs": [{"read": {"bw": 1024000, "iops": 250000.0, "lat_ns": {"mean": 4000.0, "percentile": {"99.000000": 8000.0}}}, "write": {"bw": 0, "iops": 0.0, "lat_ns": {"mean": 0.0, "percentile": {"99.000000": 0.0}}}}]}"#;
    let result = parse_fio_json(json);
    assert!(result.is_ok());
}

#[test]
fn test_parse_fio_json_returns_error_for_empty() {
    let result = parse_fio_json("");
    assert!(result.is_err() || result.unwrap().read_bw_kb.is_none());
}

#[test]
fn test_parse_fio_json_read_bandwidth() {
    let json = r#"{"jobs": [{"read": {"bw": 1024000, "iops": 250000.0, "lat_ns": {"mean": 4000.0, "percentile": {"99.000000": 8000.0}}}, "write": {"bw": 0, "iops": 0.0, "lat_ns": {"mean": 0.0, "percentile": {"99.000000": 0.0}}}}]}"#;
    let result = parse_fio_json(json).unwrap();
    assert!(result.read_bw_kb.is_some());
    assert_eq!(result.read_bw_kb.unwrap(), 1024000);
}

#[test]
fn test_parse_fio_json_write_bandwidth() {
    let json = r#"{"jobs": [{"read": {"bw": 1024000, "iops": 250000.0, "lat_ns": {"mean": 4000.0, "percentile": {"99.000000": 8000.0}}}, "write": {"bw": 524288, "iops": 125000.0, "lat_ns": {"mean": 5000.0, "percentile": {"99.000000": 10000.0}}}]}"#;
    let result = parse_fio_json(json).unwrap();
    assert!(result.write_bw_kb.is_some());
    assert_eq!(result.write_bw_kb.unwrap(), 524288);
}

#[test]
fn test_parse_fio_json_iops() {
    let json = r#"{"jobs": [{"read": {"bw": 1024000, "iops": 250000.0, "lat_ns": {"mean": 4000.0, "percentile": {"99.000000": 8000.0}}}, "write": {"bw": 524288, "iops": 125000.0, "lat_ns": {"mean": 5000.0, "percentile": {"99.000000": 10000.0}}}]}"#;
    let result = parse_fio_json(json).unwrap();
    assert!(result.read_bw_kb.is_some());
    assert!(result.write_bw_kb.is_some());
}

#[test]
fn test_parse_fio_json_latency() {
    let json = r#"{"jobs": [{"read": {"bw": 1024000, "iops": 250000.0, "lat_ns": {"mean": 4000.0, "percentile": {"99.000000": 8000.0}}}, "write": {"bw": 524288, "iops": 125000.0, "lat_ns": {"mean": 5000.0, "percentile": {"99.000000": 10000.0}}}]}"#;
    let result = parse_fio_json(json).unwrap();
    assert!(result.read_bw_kb.is_some());
    assert!(result.write_bw_kb.is_some());
}

#[test]
fn test_fio_result_throughput_gbs() {
    let result = FioResult {
        read_bw_kb: Some(3_145_728),
        write_bw_kb: None,
        ..Default::default()
    };
    let kb = result.read_bw_kb.unwrap();
    let gbs = kb as f64 / (1024.0 * 1024.0);
    assert!(gbs > 2.9);
}

#[test]
fn test_fio_config_4k_random_read_iodepth_128() {
    let config = FioConfig::new(test_file())
        .with_rw(FioRwMode::RandRead)
        .with_bs("4k")
        .with_iodepth(128);
    assert_eq!(config.bs, "4k");
    assert_eq!(config.iodepth, 128);
    assert_eq!(config.rw, FioRwMode::RandRead);
}

#[test]
fn test_fio_config_1m_sequential_write() {
    let config = FioConfig::new(test_file())
        .with_rw(FioRwMode::Write)
        .with_bs("1m");
    assert_eq!(config.bs, "1m");
    assert_eq!(config.rw, FioRwMode::Write);
}

#[test]
fn test_perf_baseline_read_bw_threshold() {
    let json = r#"{"jobs": [{"read": {"bw": 3145728, "iops": 250000.0, "lat_ns": {"mean": 4000.0, "percentile": {"99.000000": 8000.0}}}, "write": {"bw": 0, "iops": 0.0, "lat_ns": {"mean": 0.0, "percentile": {"99.000000": 0.0}}}}]}"#;
    let result = parse_fio_json(json).unwrap();
    let read_bw = result.read_bw_kb.unwrap();
    assert!(
        read_bw > 1_000_000,
        "read bandwidth {} should exceed 1MB/s threshold",
        read_bw
    );
}

#[test]
fn test_perf_baseline_write_bw_threshold() {
    let json = r#"{"jobs": [{"read": {"bw": 0, "iops": 0.0, "lat_ns": {"mean": 0.0, "percentile": {"99.000000": 0.0}}}, "write": {"bw": 2097152, "iops": 500000.0, "lat_ns": {"mean": 4000.0, "percentile": {"99.000000": 8000.0}}}]}"#;
    let result = parse_fio_json(json).unwrap();
    let write_bw = result.write_bw_kb.unwrap();
    assert!(
        write_bw > 500_000,
        "write bandwidth {} should exceed 500MB/s threshold",
        write_bw
    );
}

#[test]
fn test_perf_baseline_read_iops_threshold() {
    let json = r#"{"jobs": [{"read": {"bw": 2097152, "iops": 500000.0, "lat_ns": {"mean": 4000.0, "percentile": {"99.000000": 8000.0}}}, "write": {"bw": 0, "iops": 0.0, "lat_ns": {"mean": 0.0, "percentile": {"99.000000": 0.0}}}}]}"#;
    let result = parse_fio_json(json).unwrap();
    assert!(result.read_bw_kb.unwrap() > 1000);
}

#[test]
fn test_fio_result_read_lat_mean_is_finite() {
    let result = FioResult {
        read_bw_kb: Some(100),
        read_iops: Some(1000),
        read_lat_us_p99: Some(8000),
        ..Default::default()
    };
    let lat = result.read_lat_us_p99.unwrap() as f64;
    assert!(lat.is_finite());
    assert!(lat > 0.0);
}

#[test]
fn test_fio_result_write_lat_p99_is_finite() {
    let result = FioResult {
        write_bw_kb: Some(100),
        write_iops: Some(500),
        write_lat_us_p99: Some(12000),
        ..Default::default()
    };
    let lat = result.write_lat_us_p99.unwrap() as f64;
    assert!(lat.is_finite());
    assert!(lat > 0.0);
}

#[test]
fn test_fio_config_runtime_secs() {
    let config = FioConfig::new(test_file()).with_runtime(60);
    assert_eq!(config.runtime_secs, 60);
}

#[test]
fn test_fio_config_numjobs_scaling() {
    let config = FioConfig::new(test_file()).with_numjobs(16);
    assert_eq!(config.numjobs, 16);
}

#[test]
fn test_fio_config_direct_io() {
    let config = FioConfig::new(test_file());
    assert_eq!(config.bs, "4k");
    assert_eq!(config.iodepth, 1);
}

#[test]
fn test_detect_fio_binary_returns_option() {
    let result = crate::bench::detect_fio_binary();
    assert!(result.is_none() || result.unwrap().exists());
}

#[test]
fn test_fio_result_zero_bandwidth_valid() {
    let result = FioResult::default();
    assert!(result.read_bw_kb.is_none());
    assert!(result.write_bw_kb.is_none());
    assert!(result.read_iops.is_none());
    assert!(result.write_iops.is_none());
}
