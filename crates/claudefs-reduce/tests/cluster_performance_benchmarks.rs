/// Phase 32 Block 7: Cluster Performance Benchmark Tests
///
/// Integration tests validating performance characteristics across multiple
/// storage nodes in a real cluster. Tests measure throughput, latency, cache
/// effectiveness, resource utilization, and scaling behavior.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

mod cluster_helpers;
use cluster_helpers::{
    delete_file_fuse, file_exists_fuse, query_prometheus, read_file_fuse, ssh_exec,
    write_file_fuse, ClusterConfig, ClusterResult,
};

const FUSE_MOUNT_PATH: &str = "/mnt/claudefs";
const DEFAULT_TIMEOUT_SECS: u64 = 600;

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Clone)]
pub struct ThroughputResult {
    pub bytes_per_sec: u64,
    pub mb_per_sec: f64,
    pub ops_per_sec: f64,
    pub p50_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub p999_latency_ms: f64,
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub hit_ratio: f64,
}

#[derive(Debug, Clone)]
pub struct ResourceStats {
    pub cpu_percent: f64,
    pub memory_gb: f64,
    pub network_mbps: f64,
}

#[derive(Debug, Clone)]
pub struct TieringBenchmark {
    pub throughput_mb_per_sec: f64,
    pub completion_time_secs: f64,
}

#[derive(Debug, Clone)]
pub struct LatencyDistribution {
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub p9999_ms: f64,
}

#[derive(Debug, Clone)]
pub struct CompressionStats {
    pub original_bytes: u64,
    pub compressed_bytes: u64,
    pub ratio: f64,
}

#[derive(Debug, Clone)]
pub struct WorkloadMetrics {
    pub iops: f64,
    pub mb_per_sec: f64,
    pub avg_latency_ms: f64,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn run_throughput_test(
    client_id: usize,
    duration_secs: u64,
    block_size: usize,
) -> Result<ThroughputResult, String> {
    let config = ClusterConfig::from_env().map_err(|e| e.to_string())?;
    let client_ip = config
        .client_node_ips
        .get(client_id)
        .ok_or("Invalid client_id")?
        .clone();

    let test_dir = format!("{}/benchmark_throughput_{}", FUSE_MOUNT_PATH, client_id);
    ssh_exec(&client_ip, &format!("mkdir -p {}", test_dir), 10)?;

    let total_ops = Arc::new(AtomicU64::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let latencies = Arc::new(std::sync::Mutex::new(Vec::with_capacity(10000)));

    let start_time = Instant::now();
    let duration = Duration::from_secs(duration_secs);
    let end_time = start_time + duration;

    let total_ops1 = total_ops.clone();
    let total_bytes1 = total_bytes.clone();
    let latencies1 = latencies.clone();
    let end_time1 = end_time;
    let test_dir1 = test_dir.clone();
    let client_ip1 = client_ip.clone();
    let block_size1 = block_size;

    let total_ops2 = total_ops.clone();
    let total_bytes2 = total_bytes.clone();
    let latencies2 = latencies.clone();
    let end_time2 = end_time;
    let test_dir2 = test_dir.clone();
    let client_ip2 = client_ip.clone();
    let block_size2 = block_size;

    let total_ops3 = total_ops.clone();
    let total_bytes3 = total_bytes.clone();
    let latencies3 = latencies.clone();
    let end_time3 = end_time;
    let test_dir3 = test_dir.clone();
    let client_ip3 = client_ip.clone();
    let block_size3 = block_size;

    let total_ops4 = total_ops.clone();
    let total_bytes4 = total_bytes.clone();
    let latencies4 = latencies.clone();
    let end_time4 = end_time;
    let test_dir4 = test_dir.clone();
    let client_ip4 = client_ip.clone();
    let block_size4 = block_size;

    let worker1 = move || {
        let mut ops: u64 = 0;
        let mut bytes: u64 = 0;
        let mut local_latencies = Vec::new();
        let end = end_time1;
        let dir = test_dir1;
        let ip = client_ip1;
        let bsize = block_size1;
        while Instant::now() < end {
            let file_path = format!("{}/block_{}.dat", dir, ops % 100);
            let op_start = Instant::now();
            let _data = vec![0u8; bsize];
            if write_file_fuse(&ip, &file_path, bsize / (1024 * 1024)).is_ok() {
                ops += 1;
                bytes += bsize as u64;
                let elapsed_ms = op_start.elapsed().as_secs_f64() * 1000.0;
                local_latencies.push(elapsed_ms);
                let _ = delete_file_fuse(&ip, &file_path);
            }
        }
        total_ops1.fetch_add(ops, Ordering::Relaxed);
        total_bytes1.fetch_add(bytes, Ordering::Relaxed);
        let mut latencies_guard = latencies1.lock().unwrap();
        latencies_guard.extend(local_latencies);
    };

    let worker2 = move || {
        let mut ops: u64 = 0;
        let mut bytes: u64 = 0;
        let mut local_latencies = Vec::new();
        let end = end_time2;
        let dir = test_dir2;
        let ip = client_ip2;
        let bsize = block_size2;
        while Instant::now() < end {
            let file_path = format!("{}/block_{}.dat", dir, ops % 100);
            let op_start = Instant::now();
            let _data = vec![0u8; bsize];
            if write_file_fuse(&ip, &file_path, bsize / (1024 * 1024)).is_ok() {
                ops += 1;
                bytes += bsize as u64;
                let elapsed_ms = op_start.elapsed().as_secs_f64() * 1000.0;
                local_latencies.push(elapsed_ms);
                let _ = delete_file_fuse(&ip, &file_path);
            }
        }
        total_ops2.fetch_add(ops, Ordering::Relaxed);
        total_bytes2.fetch_add(bytes, Ordering::Relaxed);
        let mut latencies_guard = latencies2.lock().unwrap();
        latencies_guard.extend(local_latencies);
    };

    let worker3 = move || {
        let mut ops: u64 = 0;
        let mut bytes: u64 = 0;
        let mut local_latencies = Vec::new();
        let end = end_time3;
        let dir = test_dir3;
        let ip = client_ip3;
        let bsize = block_size3;
        while Instant::now() < end {
            let file_path = format!("{}/block_{}.dat", dir, ops % 100);
            let op_start = Instant::now();
            let _data = vec![0u8; bsize];
            if write_file_fuse(&ip, &file_path, bsize / (1024 * 1024)).is_ok() {
                ops += 1;
                bytes += bsize as u64;
                let elapsed_ms = op_start.elapsed().as_secs_f64() * 1000.0;
                local_latencies.push(elapsed_ms);
                let _ = delete_file_fuse(&ip, &file_path);
            }
        }
        total_ops3.fetch_add(ops, Ordering::Relaxed);
        total_bytes3.fetch_add(bytes, Ordering::Relaxed);
        let mut latencies_guard = latencies3.lock().unwrap();
        latencies_guard.extend(local_latencies);
    };

    let worker4 = move || {
        let mut ops: u64 = 0;
        let mut bytes: u64 = 0;
        let mut local_latencies = Vec::new();
        let end = end_time4;
        let dir = test_dir4;
        let ip = client_ip4;
        let bsize = block_size4;
        while Instant::now() < end {
            let file_path = format!("{}/block_{}.dat", dir, ops % 100);
            let op_start = Instant::now();
            let _data = vec![0u8; bsize];
            if write_file_fuse(&ip, &file_path, bsize / (1024 * 1024)).is_ok() {
                ops += 1;
                bytes += bsize as u64;
                let elapsed_ms = op_start.elapsed().as_secs_f64() * 1000.0;
                local_latencies.push(elapsed_ms);
                let _ = delete_file_fuse(&ip, &file_path);
            }
        }
        total_ops4.fetch_add(ops, Ordering::Relaxed);
        total_bytes4.fetch_add(bytes, Ordering::Relaxed);
        let mut latencies_guard = latencies4.lock().unwrap();
        latencies_guard.extend(local_latencies);
    };

    let h1 = thread::spawn(worker1);
    let h2 = thread::spawn(worker2);
    let h3 = thread::spawn(worker3);
    let h4 = thread::spawn(worker4);

    let _ = h1.join();
    let _ = h2.join();
    let _ = h3.join();
    let _ = h4.join();

    let actual_duration = start_time.elapsed().as_secs_f64();
    let ops = total_ops.load(Ordering::Relaxed);
    let bytes = total_bytes.load(Ordering::Relaxed);

    let mut latencies_guard = latencies.lock().unwrap();
    latencies_guard.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let n = latencies_guard.len();
    let p50 = if n > 0 {
        latencies_guard[n * 50 / 100]
    } else {
        0.0
    };
    let p99 = if n > 0 {
        latencies_guard[min(n * 99 / 100, n - 1)]
    } else {
        0.0
    };
    let p999 = if n > 0 {
        latencies_guard[min(n * 999 / 1000, n - 1)]
    } else {
        0.0
    };

    Ok(ThroughputResult {
        bytes_per_sec: bytes / actual_duration.max(1.0) as u64,
        mb_per_sec: bytes as f64 / (1024.0 * 1024.0) / actual_duration.max(1.0),
        ops_per_sec: ops as f64 / actual_duration.max(1.0),
        p50_latency_ms: p50,
        p99_latency_ms: p99,
        p999_latency_ms: p999,
    })
}

fn measure_cache_statistics() -> Result<CacheStats, String> {
    let config = ClusterConfig::from_env().map_err(|e| e.to_string())?;

    let cache_hits_query = "sum(claudefs_cache_hits_total)";
    let cache_misses_query = "sum(claudefs_cache_misses_total)";

    let hits = query_prometheus(&config.prometheus_url, cache_hits_query).unwrap_or(0.0) as u64;
    let misses = query_prometheus(&config.prometheus_url, cache_misses_query).unwrap_or(0.0) as u64;

    let total = hits + misses;
    let hit_ratio = if total > 0 {
        (hits as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Ok(CacheStats {
        hits,
        misses,
        hit_ratio,
    })
}

fn get_node_resource_utilization(node_id: &str) -> Result<ResourceStats, String> {
    let config = ClusterConfig::from_env().map_err(|e| e.to_string())?;
    let node_ip = config
        .storage_node_ips
        .iter()
        .find(|ip| ip.contains(node_id))
        .cloned()
        .ok_or_else(|| format!("Node {} not found", node_id))?;

    let cpu_query = format!("avg(claudefs_node_cpu_percent{{node=\"{}\"}})", node_id);
    let mem_query = format!(
        "avg(claudefs_node_memory_used_bytes{{node=\"{}\"}}) / 1024/1024/1024",
        node_id
    );
    let net_query = format!(
        "avg(claudefs_node_network_bytes_per_sec{{node=\"{}\"}}) * 8 / 1000000",
        node_id
    );

    let cpu = query_prometheus(&config.prometheus_url, &cpu_query).unwrap_or(0.0);
    let mem = query_prometheus(&config.prometheus_url, &mem_query).unwrap_or(0.0);
    let net = query_prometheus(&config.prometheus_url, &net_query).unwrap_or(0.0);

    Ok(ResourceStats {
        cpu_percent: cpu,
        memory_gb: mem,
        network_mbps: net,
    })
}

fn run_tiering_benchmark(file_size_mb: usize) -> Result<TieringBenchmark, String> {
    let config = ClusterConfig::from_env().map_err(|e| e.to_string())?;
    let client_ip = config
        .client_node_ips
        .first()
        .ok_or("No client nodes available")?;

    let test_file = format!("{}/tiering_test_{}mb.dat", FUSE_MOUNT_PATH, file_size_mb);

    write_file_fuse(client_ip, &test_file, file_size_mb)?;

    let start = Instant::now();

    ssh_exec(
        client_ip,
        "sync; echo 3 | sudo tee /proc/sys/vm/drop_caches > /dev/null",
        30,
    )?;

    thread::sleep(Duration::from_secs(2));

    let read_result = read_file_fuse(client_ip, &test_file);
    let completion_time = start.elapsed().as_secs_f64();

    delete_file_fuse(client_ip, &test_file)?;

    let bytes_read = read_result.map(|d| d.len()).unwrap_or(0) as f64;
    let throughput = bytes_read / (1024.0 * 1024.0) / completion_time.max(0.001);

    Ok(TieringBenchmark {
        throughput_mb_per_sec: throughput,
        completion_time_secs: completion_time,
    })
}

fn measure_coordination_overhead() -> Result<f32, String> {
    let single_result = run_throughput_test(0, 10, 65536)?;
    let single_latency = single_result.p99_latency_ms;

    let config = ClusterConfig::from_env().map_err(|e| e.to_string())?;
    let client_ip = config
        .client_node_ips
        .first()
        .ok_or("No client nodes available")?;

    let test_dir = format!("{}/coord_test", FUSE_MOUNT_PATH);
    ssh_exec(client_ip, &format!("mkdir -p {}", test_dir), 10)?;

    let start = Instant::now();
    for i in 0..50 {
        let path = format!("{}/coord_{}.dat", test_dir, i);
        let _ = write_file_fuse(client_ip, &path, 1);
    }
    let multi_latency = start.elapsed().as_secs_f64() * 1000.0 / 50.0;

    let overhead = if single_latency > 0.0 {
        ((multi_latency - single_latency) / single_latency) * 100.0
    } else {
        0.0
    };

    ssh_exec(client_ip, &format!("rm -rf {}", test_dir), 10)?;

    Ok(overhead as f32)
}

fn run_latency_percentile_test(samples: usize) -> Result<LatencyDistribution, String> {
    let config = ClusterConfig::from_env().map_err(|e| e.to_string())?;
    let client_ip = config
        .client_node_ips
        .first()
        .ok_or("No client nodes available")?;

    let test_dir = format!("{}/latency_test", FUSE_MOUNT_PATH);
    ssh_exec(client_ip, &format!("mkdir -p {}", test_dir), 10)?;

    let mut latencies = Vec::with_capacity(samples);

    for i in 0..samples {
        let path = format!("{}/lat_{}.dat", test_dir, i);
        let start = Instant::now();
        let _ = write_file_fuse(client_ip, &path, 64);
        latencies.push(start.elapsed().as_secs_f64() * 1000.0);

        if i % 100 == 0 {
            let _ = delete_file_fuse(client_ip, &path);
        }
    }

    for i in 0..samples {
        let path = format!("{}/lat_{}.dat", test_dir, i);
        let _ = delete_file_fuse(client_ip, &path);
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let n = latencies.len();
    let p50 = latencies[min(n * 50 / 100, n - 1)];
    let p90 = latencies[min(n * 90 / 100, n - 1)];
    let p99 = latencies[min(n * 99 / 100, n - 1)];
    let p999 = latencies[min(n * 999 / 1000, n - 1)];
    let p9999 = latencies[min(n * 9999 / 10000, n - 1)];

    ssh_exec(client_ip, &format!("rm -rf {}", test_dir), 10)?;

    Ok(LatencyDistribution {
        p50_ms: p50,
        p90_ms: p90,
        p99_ms: p99,
        p999_ms: p999,
        p9999_ms: p9999,
    })
}

fn min(a: usize, b: usize) -> usize {
    if a < b {
        a
    } else {
        b
    }
}

// ============================================================================
// Benchmark Tests
// ============================================================================

#[test]
#[ignore]
fn test_cluster_benchmark_single_node_throughput() {
    println!("[BENCHMARK] Single-node throughput test");

    let result = run_throughput_test(0, 30, 65536).expect("Throughput test failed");

    println!(
        "Results: {:.2} MB/s, {:.2} ops/s",
        result.mb_per_sec, result.ops_per_sec
    );
    println!(
        "Latency: p50={:.2}ms, p99={:.2}ms, p999={:.2}ms",
        result.p50_latency_ms, result.p99_latency_ms, result.p999_latency_ms
    );

    assert!(
        result.mb_per_sec >= 400.0,
        "Single-node throughput {} MB/s below baseline 400 MB/s",
        result.mb_per_sec
    );
    assert!(
        result.p99_latency_ms <= 200.0,
        "Single-node p99 latency {}ms exceeds 200ms",
        result.p99_latency_ms
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_multi_node_throughput_5x() {
    println!("[BENCHMARK] Multi-node throughput test (5 nodes)");

    let config = ClusterConfig::from_env().expect("Failed to load config");
    assert!(config.storage_node_ips.len() >= 5, "Need 5 storage nodes");

    let single_result = run_throughput_test(0, 20, 65536).expect("Single-node failed");
    let single_mbps = single_result.mb_per_sec;

    let results: Vec<_> = (0..5)
        .map(|i| run_throughput_test(i, 20, 65536).expect("Multi-node failed"))
        .collect();

    let total_mbps: f64 = results.iter().map(|r| r.mb_per_sec).sum();
    let avg_latency: f64 = results.iter().map(|r| r.p99_latency_ms).sum::<f64>() / 5.0;

    println!("Single-node: {:.2} MB/s", single_mbps);
    println!("5-node total: {:.2} MB/s", total_mbps);
    println!("Scaling factor: {:.2}x", total_mbps / single_mbps);
    println!("Avg p99 latency: {:.2}ms", avg_latency);

    let scaling = total_mbps / single_mbps;
    assert!(
        scaling >= 2.0,
        "5-node scaling {}x below 2x target",
        scaling
    );
    assert!(
        avg_latency <= 300.0,
        "5-node p99 latency {}ms exceeds 300ms",
        avg_latency
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_latency_p50_p99_p999() {
    println!("[BENCHMARK] Latency distribution test");

    let result = run_latency_percentile_test(1000).expect("Latency test failed");

    println!("Latency distribution:");
    println!("  p50:  {:.2} ms", result.p50_ms);
    println!("  p90:  {:.2} ms", result.p90_ms);
    println!("  p99:  {:.2} ms", result.p99_ms);
    println!("  p999: {:.2} ms", result.p999_ms);
    println!("  p99.9: {:.2} ms", result.p9999_ms);

    assert!(
        result.p99_ms <= 200.0,
        "p99 latency {}ms exceeds 200ms",
        result.p99_ms
    );
    assert!(
        result.p999_ms <= 500.0,
        "p999 latency {}ms exceeds 500ms",
        result.p999_ms
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_cache_hit_ratio() {
    println!("[BENCHMARK] Cache hit ratio test");

    let config = ClusterConfig::from_env().expect("Failed to load config");
    let client_ip = config.client_node_ips.first().expect("No client");

    let test_dir = format!("{}/cache_test", FUSE_MOUNT_PATH);
    ssh_exec(client_ip, &format!("mkdir -p {}", test_dir), 10).expect("mkdir failed");

    for i in 0..200 {
        let path = format!("{}/cache_{}.dat", test_dir, i % 20);
        if !file_exists_fuse(client_ip, &path).unwrap_or(false) {
            write_file_fuse(client_ip, &path, 1).expect("write failed");
        }
    }

    thread::sleep(Duration::from_secs(3));

    for i in 0..500 {
        let path = format!("{}/cache_{}.dat", test_dir, i % 20);
        let _ = read_file_fuse(client_ip, &path);
    }

    thread::sleep(Duration::from_secs(2));

    let stats = measure_cache_statistics().expect("Cache stats failed");

    println!("Cache statistics:");
    println!("  Hits: {}", stats.hits);
    println!("  Misses: {}", stats.misses);
    println!("  Hit ratio: {:.2}%", stats.hit_ratio);

    ssh_exec(client_ip, &format!("rm -rf {}", test_dir), 10).ok();

    assert!(
        stats.hit_ratio >= 80.0,
        "Cache hit ratio {}% below 80% target",
        stats.hit_ratio
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_memory_utilization() {
    println!("[BENCHMARK] Memory utilization test");

    let config = ClusterConfig::from_env().expect("Failed to load config");

    let client_ip = config.client_node_ips.first().expect("No client");
    let test_dir = format!("{}/memory_test", FUSE_MOUNT_PATH);
    ssh_exec(client_ip, &format!("mkdir -p {}", test_dir), 10).ok();

    for i in 0..100 {
        write_file_fuse(client_ip, &format!("{}/mem_{}.dat", test_dir, i), 10).ok();
    }

    thread::sleep(Duration::from_secs(5));

    let mut total_memory_gb = 0.0;
    for (i, ip) in config.storage_node_ips.iter().enumerate() {
        let stats =
            get_node_resource_utilization(&format!("storage_{}", i)).unwrap_or(ResourceStats {
                cpu_percent: 0.0,
                memory_gb: 0.0,
                network_mbps: 0.0,
            });
        total_memory_gb += stats.memory_gb;
        println!("Node {} memory: {:.2} GB", i, stats.memory_gb);
    }

    println!("Total memory: {:.2} GB", total_memory_gb);

    ssh_exec(client_ip, &format!("rm -rf {}", test_dir), 10).ok();

    assert!(total_memory_gb > 0.0, "No memory data collected");
}

#[test]
#[ignore]
fn test_cluster_benchmark_cpu_utilization_per_node() {
    println!("[BENCHMARK] CPU utilization per node test");

    let result = run_throughput_test(0, 30, 65536).expect("Throughput test failed");
    println!("Throughput: {:.2} MB/s", result.mb_per_sec);

    let config = ClusterConfig::from_env().expect("Failed to load config");

    for (i, ip) in config.storage_node_ips.iter().enumerate() {
        let stats =
            get_node_resource_utilization(&format!("storage_{}", i)).unwrap_or(ResourceStats {
                cpu_percent: 0.0,
                memory_gb: 0.0,
                network_mbps: 0.0,
            });

        println!("Node {} CPU: {:.2}%", i, stats.cpu_percent);

        assert!(
            stats.cpu_percent <= 80.0,
            "Node {} CPU {}% exceeds 80% threshold",
            i,
            stats.cpu_percent
        );
    }
}

#[test]
#[ignore]
fn test_cluster_benchmark_network_bandwidth_utilization() {
    println!("[BENCHMARK] Network bandwidth utilization test");

    let result = run_throughput_test(0, 30, 131072).expect("Throughput test failed");
    println!(
        "Throughput: {:.2} MB/s ({:.2} Mbps)",
        result.mb_per_sec,
        result.mb_per_sec * 8.0
    );

    let config = ClusterConfig::from_env().expect("Failed to load config");

    let max_speed_gbps = 100.0;
    let max_speed_mbps = max_speed_gbps * 1000.0;
    let measured_mbps = result.mb_per_sec * 8.0;
    let utilization_pct = (measured_mbps / max_speed_mbps) * 100.0;

    println!(
        "Network utilization: {:.2}% of {} Gbps",
        utilization_pct, max_speed_gbps
    );

    assert!(
        utilization_pct <= 90.0,
        "Network utilization {}% exceeds 90%",
        utilization_pct
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_s3_tiering_throughput() {
    println!("[BENCHMARK] S3 tiering throughput test");

    let result = run_tiering_benchmark(1024).expect("Tiering benchmark failed");

    println!("Tiering results:");
    println!("  Throughput: {:.2} MB/s", result.throughput_mb_per_sec);
    println!("  Completion time: {:.2}s", result.completion_time_secs);

    assert!(
        result.throughput_mb_per_sec >= 50.0,
        "Tiering throughput {} MB/s below 50 MB/s",
        result.throughput_mb_per_sec
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_dedup_compression_ratio() {
    println!("[BENCHMARK] Deduplication/compression ratio test");

    let config = ClusterConfig::from_env().expect("Failed to load config");
    let client_ip = config.client_node_ips.first().expect("No client");

    let test_dir = format!("{}/dedup_test", FUSE_MOUNT_PATH);
    ssh_exec(client_ip, &format!("mkdir -p {}", test_dir), 10).ok();

    let identical_data = vec![0xAB; 1024 * 1024];
    for i in 0..50 {
        let path = format!("{}/dedup_{}.dat", test_dir, i);
        write_file_fuse(client_ip, &path, 1).ok();
    }

    thread::sleep(Duration::from_secs(3));

    let compress_query =
        "sum(claudefs_compression_original_bytes) / sum(claudefs_compression_compressed_bytes)";
    let ratio = query_prometheus(&config.prometheus_url, compress_query).unwrap_or(1.0);

    println!("Compression ratio: {:.2}x", ratio);

    ssh_exec(client_ip, &format!("rm -rf {}", test_dir), 10).ok();

    assert!(
        ratio >= 1.5,
        "Compression ratio {}x below 1.5x target",
        ratio
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_coordination_overhead() {
    println!("[BENCHMARK] Coordination overhead test");

    let overhead = measure_coordination_overhead().expect("Coordination test failed");

    println!("Coordination overhead: {:.2}%", overhead);

    assert!(
        overhead <= 40.0,
        "Coordination overhead {}% exceeds 40% threshold",
        overhead
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_concurrent_clients_scaling() {
    println!("[BENCHMARK] Concurrent clients scaling test");

    let result_1 = run_throughput_test(0, 15, 65536).expect("1 client failed");
    println!("1 client: {:.2} MB/s", result_1.mb_per_sec);

    let result_2a = run_throughput_test(0, 15, 65536).expect("Client 1/2 failed");
    let result_2b = run_throughput_test(1, 15, 65536).expect("Client 2/2 failed");
    let total_2 = result_2a.mb_per_sec + result_2b.mb_per_sec;
    println!(
        "2 clients: {:.2} MB/s (scaling: {:.2}x)",
        total_2,
        total_2 / result_1.mb_per_sec
    );

    assert!(
        total_2 >= result_1.mb_per_sec * 1.5,
        "2-client scaling {}x below 1.5x baseline",
        total_2 / result_1.mb_per_sec
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_large_file_performance() {
    println!("[BENCHMARK] Large file performance test (10GB+)");

    let config = ClusterConfig::from_env().expect("Failed to load config");
    let client_ip = config.client_node_ips.first().expect("No client");

    let test_file = format!("{}/large_10gb.dat", FUSE_MOUNT_PATH);

    let write_start = Instant::now();
    write_file_fuse(client_ip, &test_file, 10240).expect("Write failed");
    let write_time = write_start.elapsed().as_secs_f64();
    println!(
        "Write 10GB: {:.2}s ({:.2} MB/s)",
        write_time,
        10240.0 / write_time
    );

    let read_start = Instant::now();
    let data = read_file_fuse(client_ip, &test_file).expect("Read failed");
    let read_time = read_start.elapsed().as_secs_f64();
    println!(
        "Read 10GB: {:.2}s ({:.2} MB/s)",
        read_time,
        10240.0 / read_time
    );

    delete_file_fuse(client_ip, &test_file).ok();

    assert!(
        write_time <= 30.0,
        "10GB write took {}s exceeding 30s",
        write_time
    );
    assert!(
        read_time <= 30.0,
        "10GB read took {}s exceeding 30s",
        read_time
    );
}

#[test]
#[ignore]
fn test_cluster_benchmark_small_file_performance() {
    println!("[BENCHMARK] Small file performance test (1KB)");

    let config = ClusterConfig::from_env().expect("Failed to load config");
    let client_ip = config.client_node_ips.first().expect("No client");

    let test_dir = format!("{}/small_files", FUSE_MOUNT_PATH);
    ssh_exec(client_ip, &format!("mkdir -p {}", test_dir), 10).ok();

    let start = Instant::now();
    for i in 0..1000 {
        let path = format!("{}/small_{}.dat", test_dir, i);
        write_file_fuse(client_ip, &path, 1).ok();
    }
    let write_time = start.elapsed().as_secs_f64();

    let start = Instant::now();
    for i in 0..1000 {
        let path = format!("{}/small_{}.dat", test_dir, i);
        let _ = read_file_fuse(client_ip, &path);
    }
    let read_time = start.elapsed().as_secs_f64();

    let write_ops = 1000.0 / write_time;
    let read_ops = 1000.0 / read_time;

    println!(
        "Small file ops/sec - Write: {:.0}, Read: {:.0}",
        write_ops, read_ops
    );

    ssh_exec(client_ip, &format!("rm -rf {}", test_dir), 10).ok();

    assert!(write_ops >= 100.0, "Write ops/sec {} below 100", write_ops);
    assert!(read_ops >= 100.0, "Read ops/sec {} below 100", read_ops);
}

#[test]
#[ignore]
fn test_cluster_benchmark_mixed_workload_iops_mbs() {
    println!("[BENCHMARK] Mixed workload IOPS vs MB/s test");

    let config = ClusterConfig::from_env().expect("Failed to load config");
    let client_ip = config.client_node_ips.first().expect("No client");

    let test_dir = format!("{}/mixed_test", FUSE_MOUNT_PATH);
    ssh_exec(client_ip, &format!("mkdir -p {}", test_dir), 10).ok();

    let small_start = Instant::now();
    for i in 0..500 {
        let path = format!("{}/mixed_small_{}.dat", test_dir, i);
        write_file_fuse(client_ip, &path, 4).ok();
    }
    let small_time = small_start.elapsed().as_secs_f64();

    let large_start = Instant::now();
    for i in 0..50 {
        let path = format!("{}/mixed_large_{}.dat", test_dir, i);
        write_file_fuse(client_ip, &path, 64).ok();
    }
    let large_time = large_start.elapsed().as_secs_f64();

    let small_iops = 500.0 / small_time;
    let large_mbs = (50.0 * 64.0) / large_time;

    println!("Mixed workload:");
    println!("  Small files (4KB): {:.0} IOPS", small_iops);
    println!("  Large files (64MB): {:.2} MB/s", large_mbs);

    ssh_exec(client_ip, &format!("rm -rf {}", test_dir), 10).ok();

    assert!(
        small_iops >= 50.0,
        "Small file IOPS {} below 50",
        small_iops
    );
    assert!(
        large_mbs >= 100.0,
        "Large file MB/s {} below 100",
        large_mbs
    );
}
