//! Endurance and stability tests for ClaudeFS sustained operation.

use crate::soak::{
    generate_task_sequence, FileSoakTest, SoakConfig, SoakSnapshot, SoakStats, WorkerOp, WorkerTask,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct EnduranceConfig {
    pub duration_secs: u64,
    pub concurrency: u32,
    pub key_count: u32,
    pub value_size_bytes: u32,
}

impl Default for EnduranceConfig {
    fn default() -> Self {
        Self {
            duration_secs: 60,
            concurrency: 4,
            key_count: 1000,
            value_size_bytes: 4096,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EnduranceOp {
    Read { key: String },
    Write { key: String, data: Vec<u8> },
    Delete { key: String },
    List,
}

#[derive(Debug, Clone)]
pub struct EnduranceTask {
    pub op: EnduranceOp,
    pub timestamp_ms: u64,
    pub latency_us: u64,
    pub success: bool,
}

#[derive(Debug, Clone, Default)]
pub struct EnduranceStats {
    pub total_ops: u64,
    pub successful_ops: u64,
    pub failed_ops: u64,
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub avg_latency_us: f64,
    pub p99_latency_us: f64,
    pub p999_latency_us: f64,
    latencies: Arc<Mutex<Vec<u64>>>,
}

#[derive(Debug)]
pub struct EnduranceSnapshot {
    pub elapsed_ms: u64,
    pub stats: EnduranceStats,
}

#[derive(Debug)]
pub struct EnduranceTest {
    config: EnduranceConfig,
    total_ops: Arc<AtomicU64>,
    successful_ops: Arc<AtomicU64>,
    failed_ops: Arc<AtomicU64>,
    bytes_read: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
    latencies: Arc<Mutex<Vec<u64>>>,
}

impl EnduranceTest {
    pub fn new(config: EnduranceConfig) -> Self {
        Self {
            config,
            total_ops: Arc::new(AtomicU64::new(0)),
            successful_ops: Arc::new(AtomicU64::new(0)),
            failed_ops: Arc::new(AtomicU64::new(0)),
            bytes_read: Arc::new(AtomicU64::new(0)),
            bytes_written: Arc::new(AtomicU64::new(0)),
            latencies: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn record_task(&self, task: EnduranceTask) {
        self.total_ops.fetch_add(1, Ordering::Relaxed);
        if task.success {
            self.successful_ops.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_ops.fetch_add(1, Ordering::Relaxed);
        }
        if let Ok(mut latencies) = self.latencies.lock() {
            latencies.push(task.latency_us);
        }
    }

    pub fn snapshot(&self, elapsed_ms: u64) -> EnduranceSnapshot {
        let stats = self.stats();
        EnduranceSnapshot { elapsed_ms, stats }
    }

    pub fn stats(&self) -> EnduranceStats {
        let total = self.total_ops.load(Ordering::Relaxed);
        let successful = self.successful_ops.load(Ordering::Relaxed);
        let failed = self.failed_ops.load(Ordering::Relaxed);
        let bytes_read = self.bytes_read.load(Ordering::Relaxed);
        let bytes_written = self.bytes_written.load(Ordering::Relaxed);

        let latencies = self
            .latencies
            .lock()
            .map(|l| {
                let mut sorted = l.clone();
                sorted.sort();
                sorted
            })
            .unwrap_or_default();

        let (avg, p99, p999) = if latencies.is_empty() {
            (0.0, 0.0, 0.0)
        } else {
            let sum: u64 = latencies.iter().sum();
            let avg = sum as f64 / latencies.len() as f64;
            let p99_idx = ((latencies.len() as f64) * 0.99) as usize;
            let p999_idx = ((latencies.len() as f64) * 0.999) as usize;
            let p99 = latencies
                .get(p99_idx.min(latencies.len() - 1))
                .copied()
                .unwrap_or(0) as f64;
            let p999 = latencies
                .get(p999_idx.min(latencies.len() - 1))
                .copied()
                .unwrap_or(0) as f64;
            (avg, p99, p999)
        };

        EnduranceStats {
            total_ops: total,
            successful_ops: successful,
            failed_ops: failed,
            total_bytes_read: bytes_read,
            total_bytes_written: bytes_written,
            avg_latency_us: avg,
            p99_latency_us: p99,
            p999_latency_us: p999,
            latencies: self.latencies.clone(),
        }
    }

    pub fn success_rate(&self) -> f64 {
        let total = self.total_ops.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let successful = self.successful_ops.load(Ordering::Relaxed);
        successful as f64 / total as f64
    }

    pub fn is_healthy(&self, min_success_rate: f64) -> bool {
        self.success_rate() >= min_success_rate
    }

    pub fn add_bytes_written(&self, bytes: u64) {
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_bytes_read(&self, bytes: u64) {
        self.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }
}

use std::sync::Mutex;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soak_config_default_values() {
        let config = EnduranceConfig::default();
        assert_eq!(config.duration_secs, 60);
        assert_eq!(config.concurrency, 4);
        assert_eq!(config.key_count, 1000);
        assert_eq!(config.value_size_bytes, 4096);
    }

    #[test]
    fn test_file_soak_test_new() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        assert_eq!(test.success_rate(), 0.0);
    }

    #[test]
    fn test_soak_test_initial_stats_zero() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        let stats = test.stats();
        assert_eq!(stats.total_ops, 0);
        assert_eq!(stats.successful_ops, 0);
    }

    #[test]
    fn test_soak_test_record_success() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        let task = EnduranceTask {
            op: EnduranceOp::Read {
                key: "key1".to_string(),
            },
            timestamp_ms: 1000,
            latency_us: 100,
            success: true,
        };
        test.record_task(task);
        assert_eq!(test.stats().total_ops, 1);
        assert_eq!(test.stats().successful_ops, 1);
    }

    #[test]
    fn test_soak_test_record_failure() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        let task = EnduranceTask {
            op: EnduranceOp::Read {
                key: "key1".to_string(),
            },
            timestamp_ms: 1000,
            latency_us: 100,
            success: false,
        };
        test.record_task(task);
        assert_eq!(test.stats().total_ops, 1);
        assert_eq!(test.stats().failed_ops, 1);
    }

    #[test]
    fn test_soak_test_success_rate_all_success() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        for _ in 0..10 {
            let task = EnduranceTask {
                op: EnduranceOp::Read {
                    key: "key1".to_string(),
                },
                timestamp_ms: 1000,
                latency_us: 100,
                success: true,
            };
            test.record_task(task);
        }
        assert!((test.success_rate() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_soak_test_success_rate_all_fail() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        for _ in 0..10 {
            let task = EnduranceTask {
                op: EnduranceOp::Read {
                    key: "key1".to_string(),
                },
                timestamp_ms: 1000,
                latency_us: 100,
                success: false,
            };
            test.record_task(task);
        }
        assert!((test.success_rate() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_soak_test_success_rate_mixed() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        for _ in 0..3 {
            let task = EnduranceTask {
                op: EnduranceOp::Read {
                    key: "key1".to_string(),
                },
                timestamp_ms: 1000,
                latency_us: 100,
                success: true,
            };
            test.record_task(task);
        }
        let task = EnduranceTask {
            op: EnduranceOp::Read {
                key: "key1".to_string(),
            },
            timestamp_ms: 1000,
            latency_us: 100,
            success: false,
        };
        test.record_task(task);
        assert!((test.success_rate() - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_soak_test_is_healthy_above_threshold() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        for _ in 0..10 {
            let task = EnduranceTask {
                op: EnduranceOp::Read {
                    key: "key1".to_string(),
                },
                timestamp_ms: 1000,
                latency_us: 100,
                success: true,
            };
            test.record_task(task);
        }
        assert!(test.is_healthy(0.99));
    }

    #[test]
    fn test_soak_test_is_healthy_below_threshold() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        for _ in 0..9 {
            let task = EnduranceTask {
                op: EnduranceOp::Read {
                    key: "key1".to_string(),
                },
                timestamp_ms: 1000,
                latency_us: 100,
                success: true,
            };
            test.record_task(task);
        }
        let task = EnduranceTask {
            op: EnduranceOp::Read {
                key: "key1".to_string(),
            },
            timestamp_ms: 1000,
            latency_us: 100,
            success: false,
        };
        test.record_task(task);
        assert!(!test.is_healthy(0.99));
    }

    #[test]
    fn test_soak_test_snapshot() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        let snapshot = test.snapshot(1000);
        assert_eq!(snapshot.elapsed_ms, 1000);
        assert_eq!(snapshot.stats.total_ops, 0);
    }

    #[test]
    fn test_soak_test_bytes_written() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        test.add_bytes_written(1024);
        test.add_bytes_written(2048);
        assert_eq!(test.stats().total_bytes_written, 3072);
    }

    #[test]
    fn test_soak_test_bytes_read() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        test.add_bytes_read(512);
        test.add_bytes_read(1536);
        assert_eq!(test.stats().total_bytes_read, 2048);
    }

    #[test]
    fn test_worker_op_read_create() {
        let op = EnduranceOp::Read {
            key: "test_key".to_string(),
        };
        match op {
            EnduranceOp::Read { key } => assert_eq!(key, "test_key"),
            _ => panic!("Expected Read variant"),
        }
    }

    #[test]
    fn test_worker_op_write_create() {
        let data = vec![1u8, 2, 3, 4];
        let op = EnduranceOp::Write {
            key: "test_key".to_string(),
            data: data.clone(),
        };
        match op {
            EnduranceOp::Write { key, data: d } => {
                assert_eq!(key, "test_key");
                assert_eq!(d, data);
            }
            _ => panic!("Expected Write variant"),
        }
    }

    #[test]
    fn test_worker_op_delete_create() {
        let op = EnduranceOp::Delete {
            key: "test_key".to_string(),
        };
        match op {
            EnduranceOp::Delete { key } => assert_eq!(key, "test_key"),
            _ => panic!("Expected Delete variant"),
        }
    }

    #[test]
    fn test_worker_task_create() {
        let task = EnduranceTask {
            op: EnduranceOp::Read {
                key: "key1".to_string(),
            },
            timestamp_ms: 12345,
            latency_us: 678,
            success: true,
        };
        assert_eq!(task.timestamp_ms, 12345);
        assert_eq!(task.latency_us, 678);
        assert!(task.success);
    }

    #[test]
    fn test_generate_task_sequence_count() {
        let tasks = generate_task_sequence(0, 42, 10);
        assert_eq!(tasks.len(), 10);
    }

    #[test]
    fn test_generate_task_sequence_reproducible() {
        let tasks1 = generate_task_sequence(0, 42, 10);
        let tasks2 = generate_task_sequence(0, 42, 10);
        assert_eq!(tasks1.len(), tasks2.len());
        for (a, b) in tasks1.iter().zip(tasks2.iter()) {
            assert_eq!(a.worker_id, b.worker_id);
            assert_eq!(a.op, b.op);
            assert_eq!(a.size_bytes, b.size_bytes);
        }
    }

    #[test]
    fn test_generate_task_sequence_different_seeds() {
        let tasks1 = generate_task_sequence(0, 42, 10);
        let tasks2 = generate_task_sequence(0, 99, 10);
        let mut all_same = true;
        for (a, b) in tasks1.iter().zip(tasks2.iter()) {
            if a.op != b.op || a.size_bytes != b.size_bytes {
                all_same = false;
                break;
            }
        }
        assert!(!all_same);
    }

    #[test]
    fn test_soak_stats_latency_zero_on_no_ops() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        let stats = test.stats();
        assert!((stats.avg_latency_us - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_soak_stats_latency_after_ops() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        for i in 0..10 {
            let task = EnduranceTask {
                op: EnduranceOp::Read {
                    key: "key1".to_string(),
                },
                timestamp_ms: i * 1000,
                latency_us: 100 + i * 10,
                success: true,
            };
            test.record_task(task);
        }
        let stats = test.stats();
        assert!(stats.avg_latency_us > 0.0);
    }

    #[test]
    fn test_soak_test_many_ops() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        for i in 0..100 {
            let task = EnduranceTask {
                op: EnduranceOp::Read {
                    key: format!("key{}", i),
                },
                timestamp_ms: i as u64 * 1000,
                latency_us: 100,
                success: i % 10 != 0,
            };
            test.record_task(task);
        }
        let stats = test.stats();
        assert_eq!(stats.total_ops, 100);
        assert_eq!(stats.successful_ops, 90);
        assert_eq!(stats.failed_ops, 10);
    }

    #[test]
    fn test_snapshot_elapsed_ms() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        let snapshot = test.snapshot(1000);
        assert_eq!(snapshot.elapsed_ms, 1000);
    }

    #[test]
    fn test_endurance_simulation_10k_ops() {
        let config = EnduranceConfig::default();
        let test = EnduranceTest::new(config);
        let tasks = generate_task_sequence(0, 12345, 10000);
        for task in &tasks {
            let success = match task.op {
                WorkerOp::Write => {
                    test.add_bytes_written(task.size_bytes);
                    true
                }
                WorkerOp::Read => {
                    test.add_bytes_read(task.size_bytes);
                    true
                }
                WorkerOp::Delete => true,
                WorkerOp::Verify => true,
            };
            let endurance_task = EnduranceTask {
                op: match task.op {
                    WorkerOp::Write => EnduranceOp::Write {
                        key: "key".to_string(),
                        data: vec![0u8; task.size_bytes as usize],
                    },
                    WorkerOp::Read => EnduranceOp::Read {
                        key: "key".to_string(),
                    },
                    WorkerOp::Delete => EnduranceOp::Delete {
                        key: "key".to_string(),
                    },
                    WorkerOp::Verify => EnduranceOp::List,
                },
                timestamp_ms: task.worker_id as u64,
                latency_us: task.size_bytes,
                success,
            };
            test.record_task(endurance_task);
        }
        let stats = test.stats();
        assert_eq!(stats.total_ops, 10000);
        assert!(test.is_healthy(0.5));
    }
}
