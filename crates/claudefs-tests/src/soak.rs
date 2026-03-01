use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SoakConfig {
    pub duration: Duration,
    pub num_workers: u32,
    pub ops_per_sec_target: u64,
    pub verify_data: bool,
    pub seed: u64,
}

impl Default for SoakConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(60),
            num_workers: 4,
            ops_per_sec_target: 1000,
            verify_data: true,
            seed: 12345,
        }
    }
}

#[derive(Debug)]
pub struct SoakStats {
    pub ops_completed: Arc<AtomicU64>,
    pub ops_failed: Arc<AtomicU64>,
    pub bytes_written: Arc<AtomicU64>,
    pub bytes_read: Arc<AtomicU64>,
    pub errors: Arc<Mutex<Vec<String>>>,
}

impl Default for SoakStats {
    fn default() -> Self {
        Self {
            ops_completed: Arc::new(AtomicU64::new(0)),
            ops_failed: Arc::new(AtomicU64::new(0)),
            bytes_written: Arc::new(AtomicU64::new(0)),
            bytes_read: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl SoakStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            ops_completed: Arc::new(AtomicU64::new(0)),
            ops_failed: Arc::new(AtomicU64::new(0)),
            bytes_written: Arc::new(AtomicU64::new(0)),
            bytes_read: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn record_op(&self) {
        self.ops_completed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self, err: String) {
        self.ops_failed.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut errors) = self.errors.lock() {
            errors.push(err);
        }
    }

    pub fn record_write(&self, bytes: u64) {
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_read(&self, bytes: u64) {
        self.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> SoakSnapshot {
        let ops_completed = self.ops_completed.load(Ordering::Relaxed);
        let ops_failed = self.ops_failed.load(Ordering::Relaxed);
        let bytes_written = self.bytes_written.load(Ordering::Relaxed);
        let bytes_read = self.bytes_read.load(Ordering::Relaxed);
        let error_count = self.errors.lock().map(|e| e.len()).unwrap_or(0);

        SoakSnapshot {
            ops_completed,
            ops_failed,
            bytes_written,
            bytes_read,
            error_count,
            elapsed: Duration::default(),
            ops_per_sec: 0.0,
            write_mb_per_sec: 0.0,
            read_mb_per_sec: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SoakSnapshot {
    pub ops_completed: u64,
    pub ops_failed: u64,
    pub bytes_written: u64,
    pub bytes_read: u64,
    pub error_count: usize,
    pub elapsed: Duration,
    pub ops_per_sec: f64,
    pub write_mb_per_sec: f64,
    pub read_mb_per_sec: f64,
}

impl Default for SoakSnapshot {
    fn default() -> Self {
        Self {
            ops_completed: 0,
            ops_failed: 0,
            bytes_written: 0,
            bytes_read: 0,
            error_count: 0,
            elapsed: Duration::default(),
            ops_per_sec: 0.0,
            write_mb_per_sec: 0.0,
            read_mb_per_sec: 0.0,
        }
    }
}

impl SoakSnapshot {
    pub fn calculate_from_stats(stats: &SoakStats, elapsed: Duration) -> Self {
        let ops_completed = stats.ops_completed.load(Ordering::Relaxed);
        let ops_failed = stats.ops_failed.load(Ordering::Relaxed);
        let bytes_written = stats.bytes_written.load(Ordering::Relaxed);
        let bytes_read = stats.bytes_read.load(Ordering::Relaxed);
        let error_count = stats.errors.lock().map(|e| e.len()).unwrap_or(0);

        let secs = elapsed.as_secs_f64();
        let ops_per_sec = if secs > 0.0 {
            ops_completed as f64 / secs
        } else {
            0.0
        };
        let write_mb_per_sec = if secs > 0.0 {
            bytes_written as f64 / (1024.0 * 1024.0 * secs)
        } else {
            0.0
        };
        let read_mb_per_sec = if secs > 0.0 {
            bytes_read as f64 / (1024.0 * 1024.0 * secs)
        } else {
            0.0
        };

        Self {
            ops_completed,
            ops_failed,
            bytes_written,
            bytes_read,
            error_count,
            elapsed,
            ops_per_sec,
            write_mb_per_sec,
            read_mb_per_sec,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerOp {
    Write,
    Read,
    Delete,
    Verify,
}

#[derive(Debug, Clone)]
pub struct WorkerTask {
    pub worker_id: u32,
    pub op: WorkerOp,
    pub size_bytes: u64,
}

pub fn generate_task_sequence(worker_id: u32, seed: u64, count: usize) -> Vec<WorkerTask> {
    let mut tasks = Vec::with_capacity(count);
    let mut state = seed.wrapping_add(worker_id as u64);

    for i in 0..count {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        let op_index = (state % 4) as u32;
        let op = match op_index {
            0 => WorkerOp::Write,
            1 => WorkerOp::Read,
            2 => WorkerOp::Delete,
            _ => WorkerOp::Verify,
        };

        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        let size = ((state % 1024) + 1) as u64;

        tasks.push(WorkerTask {
            worker_id,
            op,
            size_bytes: size,
        });
    }

    tasks
}

pub struct FileSoakTest {
    pub config: SoakConfig,
    pub root: PathBuf,
}

impl FileSoakTest {
    pub fn new(root: PathBuf, config: SoakConfig) -> Self {
        Self { config, root }
    }

    pub fn run_brief(&self) -> SoakSnapshot {
        let stats = SoakStats::new();
        let start = Instant::now();

        let duration = Duration::from_secs(1);
        let end = start + duration;

        while Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }

        let elapsed = start.elapsed();
        SoakSnapshot::calculate_from_stats(&stats, elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soak_config_default() {
        let config = SoakConfig::default();
        assert_eq!(config.duration, Duration::from_secs(60));
        assert_eq!(config.num_workers, 4);
        assert_eq!(config.ops_per_sec_target, 1000);
        assert!(config.verify_data);
    }

    #[test]
    fn test_soak_stats_record_op() {
        let stats = SoakStats::new();
        stats.record_op();
        stats.record_op();
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.ops_completed, 2);
    }

    #[test]
    fn test_soak_stats_record_failure() {
        let stats = SoakStats::new();
        stats.record_failure("error1".to_string());
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.ops_failed, 1);
    }

    #[test]
    fn test_soak_stats_record_write() {
        let stats = SoakStats::new();
        stats.record_write(1024);
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.bytes_written, 1024);
    }

    #[test]
    fn test_soak_stats_record_read() {
        let stats = SoakStats::new();
        stats.record_read(2048);
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.bytes_read, 2048);
    }

    #[test]
    fn test_soak_snapshot_calculations() {
        let stats = SoakStats::new();
        stats.record_op();
        stats.record_op();
        stats.record_write(1024 * 1024);
        stats.record_read(512 * 1024);

        let elapsed = Duration::from_secs(1);
        let snapshot = SoakSnapshot::calculate_from_stats(&stats, elapsed);

        assert_eq!(snapshot.ops_completed, 2);
        assert_eq!(snapshot.bytes_written, 1024 * 1024);
        assert!(snapshot.ops_per_sec > 0.0);
        assert!(snapshot.write_mb_per_sec > 0.0);
    }

    #[test]
    fn test_generate_task_sequence_determinism() {
        let tasks1 = generate_task_sequence(1, 42, 10);
        let tasks2 = generate_task_sequence(1, 42, 10);
        assert_eq!(tasks1.len(), tasks2.len());
        for (a, b) in tasks1.iter().zip(tasks2.iter()) {
            assert_eq!(a.worker_id, b.worker_id);
            assert_eq!(a.op, b.op);
            assert_eq!(a.size_bytes, b.size_bytes);
        }
    }

    #[test]
    fn test_generate_task_sequence_different_seeds() {
        let tasks1 = generate_task_sequence(1, 42, 10);
        let tasks2 = generate_task_sequence(1, 99, 10);
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
    fn test_generate_task_sequence_different_workers() {
        let tasks1 = generate_task_sequence(1, 42, 10);
        let tasks2 = generate_task_sequence(2, 42, 10);
        let mut all_same = true;
        for (a, b) in tasks1.iter().zip(tasks2.iter()) {
            if a.worker_id != b.worker_id || a.op != b.op || a.size_bytes != b.size_bytes {
                all_same = false;
                break;
            }
        }
        assert!(!all_same);
    }

    #[test]
    fn test_worker_task_creation() {
        let task = WorkerTask {
            worker_id: 1,
            op: WorkerOp::Write,
            size_bytes: 1024,
        };
        assert_eq!(task.worker_id, 1);
        assert!(matches!(task.op, WorkerOp::Write));
        assert_eq!(task.size_bytes, 1024);
    }

    #[test]
    fn test_worker_op_variants() {
        assert!(matches!(WorkerOp::Write, WorkerOp::Write));
        assert!(matches!(WorkerOp::Read, WorkerOp::Read));
        assert!(matches!(WorkerOp::Delete, WorkerOp::Delete));
        assert!(matches!(WorkerOp::Verify, WorkerOp::Verify));
    }

    #[test]
    fn test_file_soak_test_new() {
        let config = SoakConfig::default();
        let test = FileSoakTest::new(PathBuf::from("/tmp"), config);
        assert_eq!(test.root, PathBuf::from("/tmp"));
    }

    #[test]
    fn test_file_soak_test_run_brief() {
        let config = SoakConfig::default();
        let test = FileSoakTest::new(PathBuf::from("/tmp"), config);
        let snapshot = test.run_brief();
        assert!(snapshot.elapsed.as_secs() >= 1);
    }

    #[test]
    fn test_soak_stats_snapshot() {
        let stats = SoakStats::new();
        stats.record_op();
        stats.record_failure("test error".to_string());
        stats.record_write(100);
        stats.record_read(200);
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.ops_completed, 1);
        assert_eq!(snapshot.ops_failed, 1);
        assert_eq!(snapshot.bytes_written, 100);
        assert_eq!(snapshot.bytes_read, 200);
        assert_eq!(snapshot.error_count, 1);
    }

    #[test]
    fn test_soak_config_custom() {
        let config = SoakConfig {
            duration: Duration::from_secs(300),
            num_workers: 8,
            ops_per_sec_target: 5000,
            verify_data: false,
            seed: 99999,
        };
        assert_eq!(config.num_workers, 8);
        assert!(!config.verify_data);
    }

    #[test]
    fn test_soak_snapshot_default() {
        let snapshot = SoakSnapshot::default();
        assert_eq!(snapshot.ops_completed, 0);
        assert_eq!(snapshot.ops_failed, 0);
    }

    #[test]
    fn test_generate_task_sequence_count() {
        let tasks = generate_task_sequence(0, 0, 100);
        assert_eq!(tasks.len(), 100);
    }

    #[test]
    fn test_generate_task_sequence_sizes() {
        let tasks = generate_task_sequence(0, 0, 10);
        for task in &tasks {
            assert!(task.size_bytes >= 1);
            assert!(task.size_bytes <= 1024);
        }
    }

    #[test]
    fn test_soak_stats_multiple_operations() {
        let stats = SoakStats::new();
        for _ in 0..100 {
            stats.record_op();
        }
        for _ in 0..5 {
            stats.record_failure("error".to_string());
        }
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.ops_completed, 100);
        assert_eq!(snapshot.ops_failed, 5);
    }
}
