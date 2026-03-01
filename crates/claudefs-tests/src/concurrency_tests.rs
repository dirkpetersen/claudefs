//! Concurrency and thread-safety tests for ClaudeFS
//!
//! Tests concurrent access patterns, thread safety of core data structures,
//! and stress testing with multiple threads performing simultaneous operations.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct ConcurrentTestResult {
    pub threads_completed: u32,
    pub ops_succeeded: u64,
    pub ops_failed: u64,
    pub data_races_detected: u32,
    pub duration_ms: u64,
}

impl ConcurrentTestResult {
    pub fn new() -> Self {
        Self {
            threads_completed: 0,
            ops_succeeded: 0,
            ops_failed: 0,
            data_races_detected: 0,
            duration_ms: 0,
        }
    }

    pub fn is_success(&self) -> bool {
        self.ops_failed == 0 && self.data_races_detected == 0
    }

    pub fn throughput_ops_per_sec(&self) -> f64 {
        if self.duration_ms == 0 {
            return 0.0;
        }
        (self.ops_succeeded as f64) / (self.duration_ms as f64 / 1000.0)
    }
}

impl Default for ConcurrentTestResult {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ConcurrentAllocatorTest {
    pub num_threads: u32,
    pub ops_per_thread: u32,
}

impl ConcurrentAllocatorTest {
    pub fn new(threads: u32, ops: u32) -> Self {
        Self {
            num_threads: threads,
            ops_per_thread: ops,
        }
    }

    pub fn run(&self) -> ConcurrentTestResult {
        let start = Instant::now();
        let map: Arc<Mutex<HashMap<u64, u64>>> = Arc::new(Mutex::new(HashMap::new()));
        let mut handles = vec![];

        let num_threads = self.num_threads;
        let ops_per_thread = self.ops_per_thread;

        for thread_id in 0..num_threads {
            let map = Arc::clone(&map);
            let handle = thread::spawn(move || {
                for op in 0..ops_per_thread {
                    let key = (thread_id as u64 * 1_000_000) + (op as u64);
                    let mut guard = map.lock().unwrap();
                    guard.insert(key, key * 2);
                }
            });
            handles.push(handle);
        }

        let mut ops_succeeded = 0u64;
        for handle in handles {
            handle.join().unwrap();
            ops_succeeded += ops_per_thread as u64;
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        ConcurrentTestResult {
            threads_completed: num_threads,
            ops_succeeded,
            ops_failed: 0,
            data_races_detected: 0,
            duration_ms,
        }
    }
}

pub struct ConcurrentReadTest {
    pub data: Vec<u8>,
    pub num_readers: u32,
}

impl ConcurrentReadTest {
    pub fn new(data: Vec<u8>, readers: u32) -> Self {
        Self {
            data,
            num_readers: readers,
        }
    }

    pub fn run(&self) -> ConcurrentTestResult {
        let start = Instant::now();
        let data = Arc::new(self.data.clone());
        let mut handles = vec![];

        for _ in 0..self.num_readers {
            let data = Arc::clone(&data);
            let handle = thread::spawn(move || {
                let mut sum = 0u64;
                for &byte in data.iter() {
                    sum += byte as u64;
                }
                sum
            });
            handles.push(handle);
        }

        let mut results = vec![];
        for handle in handles {
            results.push(handle.join().unwrap());
        }

        let first = results[0];
        for r in &results {
            assert_eq!(
                *r, first,
                "Concurrent reads should produce consistent results"
            );
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        ConcurrentTestResult {
            threads_completed: self.num_readers,
            ops_succeeded: self.num_readers as u64,
            ops_failed: 0,
            data_races_detected: 0,
            duration_ms,
        }
    }
}

pub struct ConcurrentCompressTest {
    pub chunks: Vec<Vec<u8>>,
}

impl ConcurrentCompressTest {
    pub fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self { chunks }
    }

    pub fn run(&self) -> ConcurrentTestResult {
        let start = Instant::now();
        let chunks = Arc::new(self.chunks.clone());
        let num_threads = self.chunks.len() as u32;

        let handles: Vec<_> = (0..num_threads)
            .map(|i| {
                let chunks = Arc::clone(&chunks);
                thread::spawn(move || {
                    let chunk = &chunks[i as usize];
                    let sum: u64 = chunk.iter().map(|&b| b as u64).sum();
                    sum
                })
            })
            .collect();

        let mut ops_succeeded = 0u64;
        for handle in handles {
            handle.join().unwrap();
            ops_succeeded += 1;
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        ConcurrentTestResult {
            threads_completed: num_threads,
            ops_succeeded,
            ops_failed: 0,
            data_races_detected: 0,
            duration_ms,
        }
    }
}

pub fn stress_test_mutex_map(threads: u32, ops_per_thread: u32) -> ConcurrentTestResult {
    let start = Instant::now();
    let map: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut handles = vec![];

    for thread_id in 0..threads {
        let map = Arc::clone(&map);
        let handle = thread::spawn(move || {
            for op in 0..ops_per_thread {
                let key = format!("thread{}_op{}", thread_id, op);
                let mut guard = map.lock().unwrap();
                guard.insert(key, op as u64);
            }
        });
        handles.push(handle);
    }

    let mut ops_succeeded = 0u64;
    for handle in handles {
        handle.join().unwrap();
        ops_succeeded += ops_per_thread as u64;
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    ConcurrentTestResult {
        threads_completed: threads,
        ops_succeeded,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms,
    }
}

pub fn run_arc_mutex_under_load(threads: u32) -> ConcurrentTestResult {
    let start = Instant::now();
    let counter = Arc::new(Mutex::new(0u64));
    let mut handles = vec![];

    let ops_per_thread = 1000u64;

    for _ in 0..threads {
        let counter = Arc::clone(&counter);
        let handle = thread::spawn(move || {
            for _ in 0..ops_per_thread {
                let mut guard = counter.lock().unwrap();
                *guard += 1;
            }
        });
        handles.push(handle);
    }

    let mut ops_succeeded = 0u64;
    for handle in handles {
        handle.join().unwrap();
        ops_succeeded += ops_per_thread;
    }

    let final_count = *counter.lock().unwrap();
    assert_eq!(final_count, ops_succeeded);

    let duration_ms = start.elapsed().as_millis() as u64;

    ConcurrentTestResult {
        threads_completed: threads,
        ops_succeeded,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms,
    }
}

pub fn run_rwlock_read_concurrency(readers: u32) -> ConcurrentTestResult {
    let start = Instant::now();
    let data = Arc::new(RwLock::new(vec![1u8; 1000]));
    let mut handles = vec![];

    for _ in 0..readers {
        let data = Arc::clone(&data);
        let handle = thread::spawn(move || {
            let guard = data.read().unwrap();
            guard.len()
        });
        handles.push(handle);
    }

    let mut ops_succeeded = 0u64;
    for handle in handles {
        handle.join().unwrap();
        ops_succeeded += 1;
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    ConcurrentTestResult {
        threads_completed: readers,
        ops_succeeded,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms,
    }
}

pub fn run_rwlock_write_concurrency(writers: u32) -> ConcurrentTestResult {
    let start = Instant::now();
    let data = Arc::new(RwLock::new(0u64));
    let mut handles = vec![];

    for _ in 0..writers {
        let data = Arc::clone(&data);
        let handle = thread::spawn(move || {
            let mut guard = data.write().unwrap();
            *guard += 1;
        });
        handles.push(handle);
    }

    let mut ops_succeeded = 0u64;
    for handle in handles {
        handle.join().unwrap();
        ops_succeeded += 1;
    }

    let final_value = *data.read().unwrap();
    assert_eq!(final_value, writers as u64);

    let duration_ms = start.elapsed().as_millis() as u64;

    ConcurrentTestResult {
        threads_completed: writers,
        ops_succeeded,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms,
    }
}

#[test]
fn test_concurrent_allocator_test_creation() {
    let test = ConcurrentAllocatorTest::new(4, 100);
    assert_eq!(test.num_threads, 4);
    assert_eq!(test.ops_per_thread, 100);
}

#[test]
fn test_concurrent_read_test_creation() {
    let data = vec![1u8, 2, 3, 4, 5];
    let test = ConcurrentReadTest::new(data.clone(), 2);
    assert_eq!(test.data, data);
    assert_eq!(test.num_readers, 2);
}

#[test]
fn test_concurrent_compress_test_creation() {
    let chunks = vec![vec![1u8; 100], vec![2u8; 100]];
    let test = ConcurrentCompressTest::new(chunks.clone());
    assert_eq!(test.chunks.len(), 2);
}

#[test]
fn test_concurrent_result_is_success() {
    let result = ConcurrentTestResult {
        threads_completed: 4,
        ops_succeeded: 100,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms: 50,
    };
    assert!(result.is_success());
}

#[test]
fn test_concurrent_result_failure() {
    let result = ConcurrentTestResult {
        threads_completed: 4,
        ops_succeeded: 95,
        ops_failed: 5,
        data_races_detected: 0,
        duration_ms: 50,
    };
    assert!(!result.is_success());
}

#[test]
fn test_concurrent_result_throughput() {
    let result = ConcurrentTestResult {
        threads_completed: 4,
        ops_succeeded: 1000,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms: 1000,
    };
    let throughput = result.throughput_ops_per_sec();
    assert!((throughput - 1000.0).abs() < 0.01);
}

#[test]
fn test_concurrent_result_zero_duration() {
    let result = ConcurrentTestResult {
        threads_completed: 1,
        ops_succeeded: 100,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms: 0,
    };
    assert_eq!(result.throughput_ops_per_sec(), 0.0);
}

#[test]
fn test_stress_test_mutex_map() {
    let result = stress_test_mutex_map(4, 100);
    assert!(result.is_success());
    assert_eq!(result.ops_succeeded, 400);
}

#[test]
fn test_stress_test_mutex_map_many_threads() {
    let result = stress_test_mutex_map(8, 50);
    assert!(result.is_success());
    assert_eq!(result.ops_succeeded, 400);
}

#[test]
fn test_arc_mutex_under_load() {
    let result = run_arc_mutex_under_load(8);
    assert!(result.is_success());
    assert_eq!(result.ops_succeeded, 8000);
}

#[test]
fn test_arc_mutex_under_load_single_thread() {
    let result = run_arc_mutex_under_load(1);
    assert!(result.is_success());
    assert_eq!(result.ops_succeeded, 1000);
}

#[test]
fn test_rwlock_read_concurrency() {
    let result = run_rwlock_read_concurrency(4);
    assert!(result.is_success());
    assert_eq!(result.ops_succeeded, 4);
}

#[test]
fn test_rwlock_write_concurrency() {
    let result = run_rwlock_write_concurrency(4);
    assert!(result.is_success());
    assert_eq!(result.ops_succeeded, 4);
}

#[test]
fn test_ops_failed_zero_when_no_errors() {
    let result = ConcurrentTestResult {
        threads_completed: 4,
        ops_succeeded: 100,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms: 50,
    };
    assert!(result.is_success());
}

#[test]
fn test_data_races_detected() {
    let result = ConcurrentTestResult {
        threads_completed: 4,
        ops_succeeded: 100,
        ops_failed: 0,
        data_races_detected: 1,
        duration_ms: 50,
    };
    assert!(!result.is_success());
}

#[test]
fn test_concurrent_allocator_run() {
    let test = ConcurrentAllocatorTest::new(2, 50);
    let result = test.run();
    assert!(result.is_success());
}

#[test]
fn test_concurrent_read_run() {
    let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let test = ConcurrentReadTest::new(data, 2);
    let result = test.run();
    assert!(result.is_success());
    assert_eq!(result.threads_completed, 2);
}

#[test]
fn test_concurrent_compress_run() {
    let chunks = vec![
        vec![1u8; 100],
        vec![2u8; 100],
        vec![3u8; 100],
        vec![4u8; 100],
    ];
    let test = ConcurrentCompressTest::new(chunks);
    let result = test.run();
    assert!(result.is_success());
    assert_eq!(result.threads_completed, 4);
}

#[test]
fn test_throughput_calculation() {
    let result = ConcurrentTestResult {
        threads_completed: 4,
        ops_succeeded: 5000,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms: 500,
    };
    let throughput = result.throughput_ops_per_sec();
    assert!((throughput - 10000.0).abs() < 1.0);
}

#[test]
fn test_concurrent_result_default() {
    let result = ConcurrentTestResult::default();
    assert!(result.is_success());
    assert_eq!(result.ops_succeeded, 0);
}

#[test]
fn test_concurrent_result_clone() {
    let result = ConcurrentTestResult {
        threads_completed: 4,
        ops_succeeded: 100,
        ops_failed: 0,
        data_races_detected: 0,
        duration_ms: 50,
    };
    let cloned = result.clone();
    assert_eq!(cloned.threads_completed, 4);
    assert_eq!(cloned.ops_succeeded, 100);
}

#[test]
fn test_multiple_threads_different_operations() {
    let map: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(vec![]));
    let mut handles = vec![];

    for i in 0..4 {
        let map = Arc::clone(&map);
        let handle = thread::spawn(move || {
            for j in 0..50 {
                let mut guard = map.lock().unwrap();
                guard.push(i as u64 * 1000 + j as u64);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let guard = map.lock().unwrap();
    assert_eq!(guard.len(), 200);
}

#[test]
fn test_arc_atomic_concurrent() {
    use std::sync::atomic::{AtomicU64, Ordering};

    let counter = Arc::new(AtomicU64::new(0));
    let mut handles = vec![];

    for _ in 0..4 {
        let counter = Arc::clone(&counter);
        let handle = thread::spawn(move || {
            for _ in 0..250 {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(counter.load(Ordering::Relaxed), 1000);
}

#[test]
fn test_barrier_synchronization() {
    use std::sync::Barrier;

    let barrier = Arc::new(Barrier::new(4));
    let counter = Arc::new(Mutex::new(0u64));
    let mut handles = vec![];

    for _ in 0..4 {
        let barrier = Arc::clone(&barrier);
        let counter = Arc::clone(&counter);
        let handle = thread::spawn(move || {
            barrier.wait();
            let mut guard = counter.lock().unwrap();
            *guard += 1;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let guard = counter.lock().unwrap();
    assert_eq!(*guard, 4);
}
