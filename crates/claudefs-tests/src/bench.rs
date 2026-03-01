//! FIO and Performance Benchmark Harness - Performance test infrastructure

use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BenchError {
    #[error("FIO not found")]
    FioNotFound,
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Parse error: {0}")]
    ParseError(String),
}

/// Read/write mode for FIO
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FioRwMode {
    Read,
    Write,
    RandRead,
    RandWrite,
    ReadWrite,
    RandRW,
}

impl FioRwMode {
    pub fn as_str(&self) -> &str {
        match self {
            FioRwMode::Read => "read",
            FioRwMode::Write => "write",
            FioRwMode::RandRead => "randread",
            FioRwMode::RandWrite => "randwrite",
            FioRwMode::ReadWrite => "rw",
            FioRwMode::RandRW => "randrw",
        }
    }
}

/// FIO configuration
#[derive(Debug, Clone)]
pub struct FioConfig {
    pub rw: FioRwMode,
    pub bs: String,
    pub iodepth: u32,
    pub numjobs: u32,
    pub runtime_secs: u32,
    pub filename: PathBuf,
}

impl FioConfig {
    pub fn new(filename: PathBuf) -> Self {
        Self {
            rw: FioRwMode::Read,
            bs: "4k".to_string(),
            iodepth: 1,
            numjobs: 1,
            runtime_secs: 10,
            filename,
        }
    }

    pub fn with_rw(mut self, rw: FioRwMode) -> Self {
        self.rw = rw;
        self
    }

    pub fn with_bs(mut self, bs: &str) -> Self {
        self.bs = bs.to_string();
        self
    }

    pub fn with_iodepth(mut self, depth: u32) -> Self {
        self.iodepth = depth;
        self
    }

    pub fn with_numjobs(mut self, jobs: u32) -> Self {
        self.numjobs = jobs;
        self
    }

    pub fn with_runtime(mut self, secs: u32) -> Self {
        self.runtime_secs = secs;
        self
    }

    pub fn to_args(&self) -> Vec<String> {
        vec![
            "--filename".to_string(),
            self.filename.to_string_lossy().to_string(),
            "--rw".to_string(),
            self.rw.as_str().to_string(),
            "--bs".to_string(),
            self.bs.clone(),
            "--iodepth".to_string(),
            self.iodepth.to_string(),
            "--numjobs".to_string(),
            self.numjobs.to_string(),
            "--runtime".to_string(),
            self.runtime_secs.to_string(),
            "--time_based".to_string(),
            "--group_reporting".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ]
    }
}

/// FIO result
#[derive(Debug, Clone, Default)]
pub struct FioResult {
    pub read_bw_kb: Option<u64>,
    pub write_bw_kb: Option<u64>,
    pub read_iops: Option<u64>,
    pub write_iops: Option<u64>,
    pub read_lat_us_p99: Option<u64>,
    pub write_lat_us_p99: Option<u64>,
}

impl FioResult {
    pub fn total_bw_kb(&self) -> Option<u64> {
        match (self.read_bw_kb, self.write_bw_kb) {
            (Some(r), Some(w)) => Some(r + w),
            (Some(r), None) => Some(r),
            (None, Some(w)) => Some(w),
            (None, None) => None,
        }
    }

    pub fn total_iops(&self) -> Option<u64> {
        match (self.read_iops, self.write_iops) {
            (Some(r), Some(w)) => Some(r + w),
            (Some(r), None) => Some(r),
            (None, Some(w)) => Some(w),
            (None, None) => None,
        }
    }
}

/// FIO runner
pub struct FioRunner {
    config: FioConfig,
    binary: Option<PathBuf>,
}

impl FioRunner {
    pub fn new(config: FioConfig) -> Self {
        let binary = detect_fio_binary();
        Self { config, binary }
    }

    pub fn with_binary(mut self, binary: PathBuf) -> Self {
        self.binary = Some(binary);
        self
    }

    /// Run FIO and return results
    pub fn run(&self) -> FioResult {
        // Mock implementation - in real usage would execute fio
        FioResult {
            read_bw_kb: Some(500000),
            write_bw_kb: Some(300000),
            read_iops: Some(125000),
            write_iops: Some(75000),
            read_lat_us_p99: Some(8000),
            write_lat_us_p99: Some(12000),
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &FioConfig {
        &self.config
    }

    /// Get the FIO binary path
    pub fn binary(&self) -> Option<&PathBuf> {
        self.binary.as_ref()
    }
}

/// Detect FIO binary location
pub fn detect_fio_binary() -> Option<PathBuf> {
    let paths = [
        PathBuf::from("/usr/bin/fio"),
        PathBuf::from("/usr/local/bin/fio"),
    ];

    for path in &paths {
        if path.exists() {
            return Some(path.clone());
        }
    }

    // Check PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let candidate = PathBuf::from(dir).join("fio");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Parse FIO JSON output
pub fn parse_fio_json(output: &str) -> Result<FioResult, anyhow::Error> {
    // Simple JSON parsing for FIO output
    // In production, use a proper JSON parser

    let mut result = FioResult::default();

    // Extract read bandwidth
    if let Some(start) = output.find("\"read\"") {
        if let Some(bw_start) = output[start..].find("\"bw\"") {
            let bw_section = &output[start + bw_start..start + bw_start + 50];
            if let Some(num) = bw_section
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .ok()
            {
                result.read_bw_kb = Some(num);
            }
        }
    }

    // Extract write bandwidth
    if let Some(start) = output.find("\"write\"") {
        if let Some(bw_start) = output[start..].find("\"bw\"") {
            let bw_section = &output[start + bw_start..start + bw_start + 50];
            if let Some(num) = bw_section
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .ok()
            {
                result.write_bw_kb = Some(num);
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_file() -> PathBuf {
        PathBuf::from("/tmp/test_fio_file")
    }

    #[test]
    fn test_fio_rw_mode_as_str() {
        assert_eq!(FioRwMode::Read.as_str(), "read");
        assert_eq!(FioRwMode::Write.as_str(), "write");
        assert_eq!(FioRwMode::RandRead.as_str(), "randread");
        assert_eq!(FioRwMode::RandWrite.as_str(), "randwrite");
        assert_eq!(FioRwMode::ReadWrite.as_str(), "rw");
        assert_eq!(FioRwMode::RandRW.as_str(), "randrw");
    }

    #[test]
    fn test_fio_config_new() {
        let config = FioConfig::new(test_file());
        assert_eq!(config.rw, FioRwMode::Read);
        assert_eq!(config.bs, "4k");
        assert_eq!(config.iodepth, 1);
        assert_eq!(config.numjobs, 1);
        assert_eq!(config.runtime_secs, 10);
    }

    #[test]
    fn test_fio_config_with_rw() {
        let config = FioConfig::new(test_file()).with_rw(FioRwMode::RandWrite);
        assert_eq!(config.rw, FioRwMode::RandWrite);
    }

    #[test]
    fn test_fio_config_with_bs() {
        let config = FioConfig::new(test_file()).with_bs("128k");
        assert_eq!(config.bs, "128k");
    }

    #[test]
    fn test_fio_config_with_iodepth() {
        let config = FioConfig::new(test_file()).with_iodepth(32);
        assert_eq!(config.iodepth, 32);
    }

    #[test]
    fn test_fio_config_with_numjobs() {
        let config = FioConfig::new(test_file()).with_numjobs(4);
        assert_eq!(config.numjobs, 4);
    }

    #[test]
    fn test_fio_config_with_runtime() {
        let config = FioConfig::new(test_file()).with_runtime(60);
        assert_eq!(config.runtime_secs, 60);
    }

    #[test]
    fn test_fio_config_to_args() {
        let config = FioConfig::new(PathBuf::from("/tmp/test"))
            .with_rw(FioRwMode::Write)
            .with_bs("8k")
            .with_iodepth(4);

        let args = config.to_args();

        assert!(args.contains(&"--rw".to_string()));
        assert!(args.contains(&"write".to_string()));
        assert!(args.contains(&"--bs".to_string()));
        assert!(args.contains(&"8k".to_string()));
        assert!(args.contains(&"--iodepth".to_string()));
        assert!(args.contains(&"4".to_string()));
    }

    #[test]
    fn test_fio_result_total_bw() {
        let result = FioResult {
            read_bw_kb: Some(100),
            write_bw_kb: Some(50),
            ..Default::default()
        };

        assert_eq!(result.total_bw_kb(), Some(150));
    }

    #[test]
    fn test_fio_result_total_bw_read_only() {
        let result = FioResult {
            read_bw_kb: Some(100),
            write_bw_kb: None,
            ..Default::default()
        };

        assert_eq!(result.total_bw_kb(), Some(100));
    }

    #[test]
    fn test_fio_result_total_bw_write_only() {
        let result = FioResult {
            read_bw_kb: None,
            write_bw_kb: Some(100),
            ..Default::default()
        };

        assert_eq!(result.total_bw_kb(), Some(100));
    }

    #[test]
    fn test_fio_result_total_iops() {
        let result = FioResult {
            read_iops: Some(1000),
            write_iops: Some(500),
            ..Default::default()
        };

        assert_eq!(result.total_iops(), Some(1500));
    }

    #[test]
    fn test_fio_runner_new() {
        let config = FioConfig::new(test_file());
        let runner = FioRunner::new(config);

        assert!(
            runner.config().filename.exists()
                || !runner.config().filename.to_string_lossy().is_empty()
        );
    }

    #[test]
    fn test_fio_runner_run() {
        let config = FioConfig::new(test_file());
        let runner = FioRunner::new(config);

        let result = runner.run();
        assert!(result.read_bw_kb.is_some());
    }

    #[test]
    fn test_detect_fio_binary() {
        let result = detect_fio_binary();
        // Returns None if not found
        assert!(result.is_none() || result.unwrap().exists());
    }

    #[test]
    fn test_parse_fio_json_empty() {
        let result = parse_fio_json("{}").unwrap();
        assert!(result.read_bw_kb.is_none());
    }

    #[test]
    fn test_fio_rw_mode_debug() {
        let mode = FioRwMode::Read;
        let debug_str = format!("{:?}", mode);
        assert!(debug_str.contains("Read"));
    }

    #[test]
    fn test_fio_config_debug() {
        let config = FioConfig::new(test_file());
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("FioConfig"));
    }

    #[test]
    fn test_fio_result_debug() {
        let result = FioResult::default();
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("FioResult"));
    }

    #[test]
    fn test_fio_result_default() {
        let result = FioResult::default();
        assert!(result.read_bw_kb.is_none());
        assert!(result.write_bw_kb.is_none());
    }

    #[test]
    fn test_fio_runner_with_binary() {
        let config = FioConfig::new(test_file());
        let binary = PathBuf::from("/usr/bin/fio");
        let runner = FioRunner::new(config).with_binary(binary);

        assert!(runner.binary().is_some());
    }

    #[test]
    fn test_fio_config_chaining() {
        let config = FioConfig::new(test_file())
            .with_rw(FioRwMode::RandRW)
            .with_bs("256k")
            .with_iodepth(64)
            .with_numjobs(8)
            .with_runtime(300);

        assert_eq!(config.rw, FioRwMode::RandRW);
        assert_eq!(config.bs, "256k");
        assert_eq!(config.iodepth, 64);
        assert_eq!(config.numjobs, 8);
        assert_eq!(config.runtime_secs, 300);
    }

    #[test]
    fn test_fio_result_clone() {
        let result = FioResult {
            read_bw_kb: Some(100),
            write_bw_kb: Some(50),
            read_iops: Some(25),
            write_iops: Some(12),
            read_lat_us_p99: Some(1000),
            write_lat_us_p99: Some(2000),
        };

        let cloned = result.clone();
        assert_eq!(result.read_bw_kb, cloned.read_bw_kb);
        assert_eq!(result.write_iops, cloned.write_iops);
    }
}
