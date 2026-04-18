use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::time::{Duration, Instant};

use crate::error::ReduceError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcControllerConfig {
    pub high_memory_threshold: f64,
    pub low_memory_threshold: f64,
    pub workload_sample_window_secs: u64,
    pub min_collection_interval_ms: u64,
    pub max_collection_interval_ms: u64,
}

impl Default for GcControllerConfig {
    fn default() -> Self {
        Self {
            high_memory_threshold: 80.0,
            low_memory_threshold: 60.0,
            workload_sample_window_secs: 10,
            min_collection_interval_ms: 100,
            max_collection_interval_ms: 5000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WorkloadType {
    #[default]
    Idle,
    Batch,
    Streaming,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcThresholds {
    pub memory_pressure_percent: f64,
    pub collection_interval_ms: u64,
    pub workload_type: WorkloadType,
    pub backpressure_delay_us: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GcControllerStats {
    pub total_collections: u64,
    pub total_blocks_reclaimed: u64,
    pub current_memory_percent: f64,
    pub current_workload_type: WorkloadType,
    pub last_collection_duration_ms: u64,
}

pub trait GcController: Send + Sync {
    fn should_collect(&self) -> Result<Option<Duration>, ReduceError>;
    fn update_workload_stats(&mut self, batch_size: usize, write_rate_gb_s: f64);
    fn force_collect(&mut self) -> Result<(), ReduceError>;
    fn get_thresholds(&self) -> GcThresholds;
    fn get_stats(&self) -> GcControllerStats;
}

pub struct DynamicGcController {
    config: GcControllerConfig,
    last_collection: Instant,
    last_check: Instant,
    current_interval_ms: u64,
    force_collection: bool,
    workload_samples: Vec<WorkloadSample>,
    stats: GcControllerStats,
    backpressure_delay_us: u64,
}

#[derive(Debug, Clone)]
struct WorkloadSample {
    timestamp: Instant,
    batch_size: usize,
    write_rate_gb_s: f64,
}

impl DynamicGcController {
    pub fn new(config: GcControllerConfig) -> Self {
        let current_interval_ms =
            (config.min_collection_interval_ms + config.max_collection_interval_ms) / 2;
        Self {
            config,
            last_collection: Instant::now(),
            last_check: Instant::now(),
            current_interval_ms,
            force_collection: false,
            workload_samples: Vec::new(),
            stats: GcControllerStats::default(),
            backpressure_delay_us: 0,
        }
    }

    pub fn get_rss_bytes() -> Result<u64, std::io::Error> {
        let mut file = File::open("/proc/self/status")?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        content
            .lines()
            .find(|l| l.starts_with("VmRSS:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "VmRSS not found"))
    }

    pub fn get_memory_percent() -> Result<f64, std::io::Error> {
        let rss_bytes = Self::get_rss_bytes()?;
        let total_bytes = Self::get_total_memory_bytes().unwrap_or(64 * 1024 * 1024 * 1024);
        if rss_bytes == 0 || total_bytes == 0 {
            return Ok(50.0);
        }
        Ok((rss_bytes as f64 / total_bytes as f64) * 100.0)
    }

    pub fn get_total_memory_bytes() -> Result<u64, std::io::Error> {
        let mut file = File::open("/proc/meminfo")?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        content
            .lines()
            .find(|l| l.starts_with("MemTotal:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "MemTotal not found")
            })
    }

    fn classify_workload(&self) -> WorkloadType {
        let recent_samples: Vec<_> = self
            .workload_samples
            .iter()
            .filter(|s| {
                s.timestamp.elapsed() < Duration::from_secs(self.config.workload_sample_window_secs)
            })
            .collect();

        if recent_samples.is_empty() {
            return WorkloadType::Idle;
        }

        let avg_rate: f64 = recent_samples
            .iter()
            .map(|s| s.write_rate_gb_s)
            .sum::<f64>()
            / recent_samples.len() as f64;
        let avg_batch: f64 = recent_samples
            .iter()
            .map(|s| s.batch_size as f64)
            .sum::<f64>()
            / recent_samples.len() as f64;

        if avg_rate > 0.1 || avg_batch > 100.0 {
            WorkloadType::Batch
        } else if avg_rate > 0.0001 || avg_batch > 10.0 {
            WorkloadType::Streaming
        } else {
            WorkloadType::Idle
        }
    }

    fn calculate_adaptive_interval(&self, memory_percent: f64, workload: WorkloadType) -> u64 {
        let base_interval = match workload {
            WorkloadType::Batch => self.config.min_collection_interval_ms,
            WorkloadType::Streaming => {
                (self.config.min_collection_interval_ms + self.config.max_collection_interval_ms)
                    / 2
            }
            WorkloadType::Idle => self.config.max_collection_interval_ms,
        };

        if memory_percent > self.config.high_memory_threshold {
            self.config.min_collection_interval_ms
        } else if memory_percent < self.config.low_memory_threshold {
            base_interval * 2
        } else {
            base_interval
        }
    }
}

impl GcController for DynamicGcController {
    fn should_collect(&self) -> Result<Option<Duration>, ReduceError> {
        if self.force_collection {
            return Ok(Some(Duration::ZERO));
        }

        let memory_percent =
            Self::get_memory_percent().map_err(|e| ReduceError::GcAuditFailed(e.to_string()))?;

        let elapsed = self.last_collection.elapsed();
        let interval = Duration::from_millis(self.current_interval_ms);

        if memory_percent > self.config.high_memory_threshold {
            return Ok(Some(Duration::ZERO));
        }

        if elapsed >= interval {
            Ok(Some(interval))
        } else {
            Ok(None)
        }
    }

    fn update_workload_stats(&mut self, batch_size: usize, write_rate_gb_s: f64) {
        let sample = WorkloadSample {
            timestamp: Instant::now(),
            batch_size,
            write_rate_gb_s,
        };
        self.workload_samples.push(sample);

        let window = Duration::from_secs(self.config.workload_sample_window_secs * 2);
        self.workload_samples
            .retain(|s| s.timestamp.elapsed() < window);

        let memory_percent = Self::get_memory_percent().unwrap_or(50.0);
        let workload = self.classify_workload();
        self.current_interval_ms = self.calculate_adaptive_interval(memory_percent, workload);

        self.stats.current_memory_percent = memory_percent;
        self.stats.current_workload_type = workload;
    }

    fn force_collect(&mut self) -> Result<(), ReduceError> {
        self.force_collection = true;
        Ok(())
    }

    fn get_thresholds(&self) -> GcThresholds {
        let memory_percent = Self::get_memory_percent().unwrap_or(50.0);
        GcThresholds {
            memory_pressure_percent: memory_percent,
            collection_interval_ms: self.current_interval_ms,
            workload_type: self.classify_workload(),
            backpressure_delay_us: self.backpressure_delay_us,
        }
    }

    fn get_stats(&self) -> GcControllerStats {
        self.stats.clone()
    }
}
