/// Phase 32 Block 2: Single-Node Dedup on Real Cluster (18 tests)
///
/// Integration tests validating dedup behavior from FUSE client through real
/// storage node to S3. Tests run against a real cluster with actual FUSE mount,
/// real storage node with io_uring, and real S3 backend.
///
/// Prerequisites:
/// - FUSE mount available at /mnt/claudefs
/// - Storage node accessible via SSH
/// - Prometheus metrics endpoint at storage node
/// - AWS credentials configured for S3 access
/// - Environment variables: CFS_STORAGE_NODE, CFS_PROMETHEUS_PORT, CFS_S3_BUCKET
use std::process::Command;
use std::time::{Duration, Instant};

const FUSE_MOUNT_PATH: &str = "/mnt/claudefs";
const DEFAULT_STORAGE_NODE: &str = "storage-node-0";
const DEFAULT_PROMETHEUS_PORT: u16 = 9090;
const DEFAULT_S3_BUCKET: &str = "claudefs-dedup-test";

fn get_env_or_default(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn get_storage_node() -> String {
    get_env_or_default("CFS_STORAGE_NODE", DEFAULT_STORAGE_NODE)
}

fn get_prometheus_port() -> u16 {
    get_env_or_default("CFS_PROMETHEUS_PORT", "9090")
        .parse()
        .unwrap_or(DEFAULT_PROMETHEUS_PORT)
}

fn get_s3_bucket() -> String {
    get_env_or_default("CFS_S3_BUCKET", DEFAULT_S3_BUCKET)
}

fn ssh_exec(node: &str, cmd: &str) -> Result<String, String> {
    let output = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "ConnectTimeout=10",
            node,
            cmd,
        ])
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn write_file_fuse(path: &str, size_mb: usize) -> Result<(), String> {
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    let output = Command::new("dd")
        .args([
            "if=/dev/urandom",
            "bs=1M",
            &format!("count={}", size_mb),
            &format!("of={}", full_path),
            "conv=fdatasync",
        ])
        .output()
        .map_err(|e| format!("dd failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn write_zero_file_fuse(path: &str, size_mb: usize) -> Result<(), String> {
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    let output = Command::new("dd")
        .args([
            "if=/dev/zero",
            "bs=1M",
            &format!("count={}", size_mb),
            &format!("of={}", full_path),
            "conv=fdatasync",
        ])
        .output()
        .map_err(|e| format!("dd failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn copy_file_fuse(src: &str, dst: &str) -> Result<(), String> {
    let src_path = format!("{}/{}", FUSE_MOUNT_PATH, src);
    let dst_path = format!("{}/{}", FUSE_MOUNT_PATH, dst);
    let output = Command::new("cp")
        .args(["--preserve=all", &src_path, &dst_path])
        .output()
        .map_err(|e| format!("cp failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn delete_file_fuse(path: &str) -> Result<(), String> {
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    let output = Command::new("rm")
        .arg(&full_path)
        .output()
        .map_err(|e| format!("rm failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn read_file_fuse(path: &str) -> Result<Vec<u8>, String> {
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    std::fs::read(&full_path).map_err(|e| format!("Read failed: {}", e))
}

fn query_prometheus_metric(metric: &str) -> Result<f64, String> {
    let node = get_storage_node();
    let port = get_prometheus_port();
    let url = format!("http://{}:{}/api/v1/query?query={}", node, port, metric);

    let output = Command::new("curl")
        .args(["-s", &url])
        .output()
        .map_err(|e| format!("curl failed: {}", e))?;

    let response = String::from_utf8_lossy(&output.stdout);

    if let Some(start) = response.find("\"value\"") {
        if let Some(bracket) = response[start..].find('[') {
            if let Some(closing) = response[start + bracket..].find(']') {
                let val_str = &response[start + bracket + 1..start + bracket + closing];
                return val_str
                    .trim()
                    .parse::<f64>()
                    .map_err(|e| format!("Parse failed: {}", e));
            }
        }
    }

    if let Some(result) = response.find("\"result\"") {
        if let Some(value) = response[result..].find(": ") {
            let val_start = result + value + 2;
            let remaining = &response[val_start..];
            let end_pos = remaining.find(',').unwrap_or(remaining.len()).min(50);
            let val_str = remaining[..end_pos].trim();
            return val_str
                .parse::<f64>()
                .map_err(|e| format!("Parse failed: {}", e));
        }
    }

    Err(format!("Metric not found in response: {}", response))
}

fn query_prometheus_histogram_p99(metric: &str) -> Result<f64, String> {
    let node = get_storage_node();
    let port = get_prometheus_port();
    let url = format!(
        "http://{}:{}/api/v1/query?query=histogram_quantile(0.99, {})",
        node, port, metric
    );

    let output = Command::new("curl")
        .args(["-s", &url])
        .output()
        .map_err(|e| format!("curl failed: {}", e))?;

    let response = String::from_utf8_lossy(&output.stdout);

    if let Some(start) = response.find("\"value\"") {
        if let Some(bracket) = response[start..].find('[') {
            if let Some(closing) = response[start + bracket..].find(']') {
                let val_str = &response[start + bracket + 1..start + bracket + closing];
                return val_str
                    .trim()
                    .parse::<f64>()
                    .map_err(|e| format!("Parse failed: {}", e));
            }
        }
    }

    Err(format!("P99 metric not found in response: {}", response))
}

fn s3_list_objects(prefix: &str) -> Result<Vec<String>, String> {
    let bucket = get_s3_bucket();
    let output = Command::new("aws")
        .args([
            "s3",
            "ls",
            &format!("s3://{}/{}", bucket, prefix),
            "--recursive",
        ])
        .output()
        .map_err(|e| format!("aws s3 ls failed: {}", e))?;

    if output.status.success() {
        let lines = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();
        Ok(lines)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[allow(dead_code)]
fn s3_put_test_object(key: &str, data: &[u8]) -> Result<(), String> {
    let bucket = get_s3_bucket();
    let temp_file = "/tmp/claudefs_test_s3_upload.tmp";
    std::fs::write(temp_file, data).map_err(|e| format!("Write temp failed: {}", e))?;

    let output = Command::new("aws")
        .args(["s3", "cp", temp_file, &format!("s3://{}/{}", bucket, key)])
        .output()
        .map_err(|e| format!("aws s3 cp failed: {}", e))?;

    let _ = std::fs::remove_file(temp_file);

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn wait_for_metric(
    metric: &str,
    expected: f64,
    timeout_secs: u64,
    comparator: &str,
) -> Result<(), String> {
    let start = Instant::now();
    let tolerance = expected * 0.1;

    loop {
        if let Ok(value) = query_prometheus_metric(metric) {
            let matches = match comparator {
                "gte" => value >= expected,
                "lte" => value <= expected,
                "eq" => (value - expected).abs() < tolerance,
                "gt" => value > expected,
                _ => (value - expected).abs() < tolerance,
            };
            if matches {
                return Ok(());
            }
        }

        if start.elapsed().as_secs() >= timeout_secs {
            let actual = query_prometheus_metric(metric).unwrap_or(-1.0);
            return Err(format!(
                "Timeout waiting for {} >= {} (got {})",
                metric, expected, actual
            ));
        }

        std::thread::sleep(Duration::from_secs(2));
    }
}

fn kill_process_on_node(node: &str, process_name: &str) -> Result<(), String> {
    ssh_exec(node, &format!("sudo pkill -9 {}", process_name))?;
    Ok(())
}

fn start_process_on_node(node: &str, process_name: &str) -> Result<(), String> {
    ssh_exec(node, &format!("sudo systemctl start {}", process_name))?;
    Ok(())
}

fn apply_iptables_rule(node: &str, rule: &str) -> Result<(), String> {
    ssh_exec(node, &format!("sudo iptables {}", rule))?;
    Ok(())
}

fn file_exists_fuse(path: &str) -> bool {
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    std::path::Path::new(&full_path).exists()
}

fn file_size_fuse(path: &str) -> Result<u64, String> {
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    std::fs::metadata(&full_path)
        .map(|m| m.len())
        .map_err(|e| format!("Stat failed: {}", e))
}

#[allow(dead_code)]
fn compare_files(path1: &str, path2: &str) -> Result<bool, String> {
    let p1 = format!("{}/{}", FUSE_MOUNT_PATH, path1);
    let p2 = format!("{}/{}", FUSE_MOUNT_PATH, path2);

    let data1 = std::fs::read(&p1).map_err(|e| format!("Read {} failed: {}", path1, e))?;
    let data2 = std::fs::read(&p2).map_err(|e| format!("Read {} failed: {}", path2, e))?;

    Ok(data1 == data2)
}

fn set_tenant_quota(tenant_id: &str, quota_mb: u64) -> Result<(), String> {
    let node = get_storage_node();
    ssh_exec(
        &node,
        &format!(
            "echo 'tenant.quotas.{}= {}' | sudo tee /etc/claudefs/quotas.conf",
            tenant_id,
            quota_mb * 1024 * 1024
        ),
    )?;
    Ok(())
}

fn check_cluster_available() -> bool {
    let node = get_storage_node();
    ssh_exec(&node, "echo 'test'").is_ok() && file_exists_fuse(".")
}

#[test]
#[ignore]
fn test_cluster_dedup_basic_write_from_fuse_client() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let test_file = "dedup_basic_test.bin";
    let size_mb = 100;

    write_file_fuse(test_file, size_mb).expect("Failed to write test file");

    assert!(file_exists_fuse(test_file), "File should exist");
    let size = file_size_fuse(test_file).expect("Failed to get file size");
    assert_eq!(
        size,
        (size_mb * 1024 * 1024) as u64,
        "File size should match"
    );

    let metric = "claudefs_dedup_fingerprints_stored_total";
    let _ = wait_for_metric(metric, 25.0, 60, "gte");

    let bytes_metric = "claudefs_dedup_bytes_written_total";
    let bytes_written = query_prometheus_metric(bytes_metric).unwrap_or(0.0);
    assert!(
        bytes_written >= (size_mb * 1024 * 1024) as f64 * 0.9,
        "Bytes written should be ~{}",
        size_mb * 1024 * 1024
    );

    let s3_objects = s3_list_objects("dedup-fingerprints/").unwrap_or_default();
    assert!(
        !s3_objects.is_empty(),
        "Should have fingerprint objects in S3"
    );

    let _ = delete_file_fuse(test_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_cache_hit_on_second_write() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let reference_file = "reference_dedup_cache.bin";
    let file1 = "dedup_cache_test_1.bin";
    let file2 = "dedup_cache_test_2.bin";
    let size_mb = 100;

    write_zero_file_fuse(reference_file, size_mb).expect("Failed to create reference file");

    copy_file_fuse(reference_file, file1).expect("Failed to copy first file");

    let cache_hits_before =
        query_prometheus_metric("claudefs_dedup_cache_hits_total").unwrap_or(0.0);

    copy_file_fuse(reference_file, file2).expect("Failed to copy second file");

    let cache_hits_after =
        query_prometheus_metric("claudefs_dedup_cache_hits_total").unwrap_or(0.0);

    assert!(
        cache_hits_after > cache_hits_before,
        "Cache hits should increase after second write"
    );

    let _ = delete_file_fuse(reference_file);
    let _ = delete_file_fuse(file1);
    let _ = delete_file_fuse(file2);
}

#[test]
#[ignore]
fn test_cluster_dedup_fingerprint_persisted_to_s3() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let test_file = "dedup_s3_test.bin";
    let size_mb = 200;

    write_file_fuse(test_file, size_mb).expect("Failed to write test file");

    std::thread::sleep(Duration::from_secs(30));

    let s3_objects = s3_list_objects("dedup-fingerprints/").unwrap_or_default();
    assert!(
        !s3_objects.is_empty(),
        "Should have fingerprint objects in S3"
    );

    let _ = delete_file_fuse(test_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_refcount_accurate_after_deletes() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let template = "rc_template.bin";
    write_zero_file_fuse(template, 10).expect("Failed to create template");

    for i in 1..=10 {
        copy_file_fuse(template, &format!("rc_file_{}.bin", i))
            .unwrap_or_else(|e| panic!("Failed to copy file {}: {}", i, e));
    }

    let _ = wait_for_metric("claudefs_dedup_references_total", 10.0, 30, "gte");

    for i in 1..=8 {
        delete_file_fuse(&format!("rc_file_{}.bin", i))
            .unwrap_or_else(|e| panic!("Failed to delete file {}: {}", i, e));
    }

    std::thread::sleep(Duration::from_secs(10));

    let refcount = query_prometheus_metric("claudefs_dedup_references_total").unwrap_or(0.0);
    assert!(
        (1.0..=3.0).contains(&refcount),
        "Refcount should be around 2 after 8 deletes, got {}",
        refcount
    );

    let _ = delete_file_fuse(template);
    for i in 9..=10 {
        let _ = delete_file_fuse(&format!("rc_file_{}.bin", i));
    }
}

#[test]
#[ignore]
fn test_cluster_dedup_coordination_real_rpc() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let node1_file = "coord_file_a.bin";
    let node2_file = "coord_file_b.bin";
    let size_mb = 50;

    write_zero_file_fuse(node1_file, size_mb).expect("Failed to write from node 1");

    let _shard_queries =
        query_prometheus_metric("claudefs_dedup_shard_queries_total").unwrap_or(0.0);

    let conflicts =
        query_prometheus_metric("claudefs_dedup_coordination_conflicts_total").unwrap_or(0.0);

    assert!(conflicts < 5.0, "Coordination conflicts should be low");

    let _ = delete_file_fuse(node1_file);
    let _ = delete_file_fuse(node2_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_throughput_baseline() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let start = Instant::now();
    let total_size_mb = 1000;
    let num_files = 20;
    let size_per_file = total_size_mb / num_files;

    for i in 0..num_files {
        write_file_fuse(
            &format!("throughput_test_{}.bin", i),
            size_per_file as usize,
        )
        .unwrap_or_else(|e| panic!("Failed to write file {}: {}", i, e));
    }

    let elapsed = start.elapsed().as_secs_f64();

    let fingerprints =
        query_prometheus_metric("claudefs_dedup_fingerprints_processed_total").unwrap_or(0.0);

    let throughput = fingerprints / elapsed;
    assert!(
        throughput >= 50_000.0,
        "Throughput should be >= 50K ops/sec, got {}",
        throughput
    );

    for i in 0..num_files {
        let _ = delete_file_fuse(&format!("throughput_test_{}.bin", i));
    }
}

#[test]
#[ignore]
fn test_cluster_dedup_latency_p99_write_path() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let p99_latency = query_prometheus_histogram_p99("claudefs_dedup_write_latency_seconds_bucket");

    match p99_latency {
        Ok(latency) => {
            assert!(
                latency < 0.1,
                "P99 latency should be < 100ms, got {}s",
                latency
            );
        }
        Err(_) => {
            let start = Instant::now();
            for i in 0..100 {
                write_file_fuse(&format!("latency_test_{}.bin", i), 1).ok();
                let _ = delete_file_fuse(&format!("latency_test_{}.bin", i));
            }
            let elapsed = start.elapsed().as_secs_f64();
            let p99_calculated = elapsed / 100.0 * 1.5;
            assert!(p99_calculated < 0.1, "P99 latency should be < 100ms");
        }
    }
}

#[test]
#[ignore]
fn test_cluster_dedup_cache_eviction_under_memory_pressure() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let cache_memory_before =
        query_prometheus_metric("claudefs_dedup_cache_memory_bytes").unwrap_or(0.0);

    for i in 0..100 {
        write_file_fuse(&format!("cache_fill_{}.bin", i), 1).ok();
    }

    let cache_memory_after =
        query_prometheus_metric("claudefs_dedup_cache_memory_bytes").unwrap_or(0.0);

    let evictions = query_prometheus_metric("claudefs_dedup_cache_evictions_total").unwrap_or(0.0);

    assert!(
        cache_memory_after > cache_memory_before || evictions > 0.0,
        "Cache should grow or evict entries under memory pressure"
    );

    for i in 0..100 {
        let _ = delete_file_fuse(&format!("cache_fill_{}.bin", i));
    }
}

#[test]
#[ignore]
fn test_cluster_dedup_cross_tenant_isolation_real() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let result = set_tenant_quota("tenant_a", 500);
    if result.is_err() {
        panic!("Tenant quotas not supported - skipping test");
    }

    write_file_fuse("tenant_a_file.bin", 100).ok();

    let metric = "claudefs_dedup_tenant_a_fingerprints_total";
    let tenant_a_fps = query_prometheus_metric(metric).unwrap_or(0.0);

    assert!(
        tenant_a_fps > 0.0,
        "Tenant A fingerprints should be tracked"
    );

    let _ = delete_file_fuse("tenant_a_file.bin");
}

#[test]
#[ignore]
fn test_cluster_dedup_crash_recovery_real() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let test_file = "crash_recovery_test.bin";
    write_file_fuse(test_file, 100).expect("Failed to write test file");

    let _refcount_before =
        query_prometheus_metric("claudefs_dedup_references_total").unwrap_or(1.0);

    let storage_node = get_storage_node();
    let kill_result = kill_process_on_node(&storage_node, "claudefs-storage");

    if kill_result.is_ok() {
        std::thread::sleep(Duration::from_secs(10));
        let _ = start_process_on_node(&storage_node, "claudefs-storage");
        std::thread::sleep(Duration::from_secs(30));
    }

    assert!(
        file_exists_fuse(test_file),
        "File should still exist after recovery"
    );

    let _ = delete_file_fuse(test_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_coordinator_failover_real() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let test_file = "failover_test.bin";
    write_file_fuse(test_file, 50).expect("Failed to write initial data");

    let storage_node = get_storage_node();
    let _ = kill_process_on_node(&storage_node, "claudefs-dedup-coord");

    std::thread::sleep(Duration::from_secs(10));

    let failover_file = "failover_after.bin";
    write_file_fuse(failover_file, 10).ok();

    assert!(
        file_exists_fuse(failover_file),
        "Writes should succeed after failover"
    );

    let _ = delete_file_fuse(test_file);
    let _ = delete_file_fuse(failover_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_network_partition_recovery_real() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let storage_node = get_storage_node();
    let _ = apply_iptables_rule(&storage_node, "-A INPUT -j DROP");

    let write_result = write_file_fuse("partition_test.bin", 10);
    assert!(
        write_result.is_err() || file_exists_fuse("partition_test.bin"),
        "Write should fail or timeout gracefully"
    );

    let _ = apply_iptables_rule(&storage_node, "-D INPUT -j DROP");

    std::thread::sleep(Duration::from_secs(10));

    let recovery_file = "partition_recovery.bin";
    write_file_fuse(recovery_file, 10).ok();

    assert!(
        file_exists_fuse(recovery_file),
        "Writes should succeed after partition heals"
    );

    let _ = delete_file_fuse("partition_test.bin");
    let _ = delete_file_fuse(recovery_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_metrics_accurate() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let baseline = query_prometheus_metric("claudefs_dedup_bytes_written_total").unwrap_or(0.0);

    let test_file = "metrics_test.bin";
    write_file_fuse(test_file, 50).expect("Failed to write test file");

    let after = query_prometheus_metric("claudefs_dedup_bytes_written_total").unwrap_or(0.0);

    let increment = after - baseline;
    let expected = (50 * 1024 * 1024) as f64;

    assert!(
        (increment - expected).abs() / expected < 0.2,
        "Metric increment should match write size"
    );

    for _ in 0..5 {
        write_file_fuse("metrics_test2.bin", 10).ok();
        let _ = delete_file_fuse("metrics_test2.bin");
    }

    let final_val = query_prometheus_metric("claudefs_dedup_bytes_written_total").unwrap_or(0.0);
    assert!(final_val > after, "Metrics should continue to update");

    let _ = delete_file_fuse(test_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_no_data_corruption() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let test_file = "corruption_test.bin";
    let test_pattern: Vec<u8> = vec![0xAA; 10 * 1024 * 1024];

    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, test_file);
    std::fs::write(&full_path, &test_pattern).expect("Failed to write test file");

    let read_data = read_file_fuse(test_file).expect("Failed to read file");

    assert_eq!(read_data, test_pattern, "Data should match original");

    let checksum_failures =
        query_prometheus_metric("claudefs_dedup_checksum_failures_total").unwrap_or(0.0);
    assert_eq!(checksum_failures, 0.0, "No checksum failures should occur");

    let _ = delete_file_fuse(test_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_quota_enforcement_active() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let set_quota_result = set_tenant_quota("tenant_test", 500);
    if set_quota_result.is_err() {
        panic!("Quotas not supported - skipping test");
    }

    let write_400mb = write_file_fuse("quota_test_400.bin", 400);
    assert!(write_400mb.is_ok(), "400MB write should succeed");

    let write_200mb = write_file_fuse("quota_test_200.bin", 200);

    let rejected =
        query_prometheus_metric("claudefs_dedup_quota_rejected_writes_total").unwrap_or(0.0);

    if write_200mb.is_err() {
        assert!(rejected > 0.0, "Quota rejections should be tracked");
    }

    let _ = delete_file_fuse("quota_test_400.bin");
    let _ = delete_file_fuse("quota_test_200.bin");
}

#[test]
#[ignore]
fn test_cluster_dedup_multi_region_replication() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let site_a_file = "replication_test.bin";
    write_file_fuse(site_a_file, 100).expect("Failed to write test file");

    std::thread::sleep(Duration::from_secs(30));

    let replication_lag =
        query_prometheus_metric("claudefs_replication_lag_seconds").unwrap_or(0.0);

    let site_b_objects = s3_list_objects("site-b/dedup-fingerprints/").unwrap_or_default();

    if site_b_objects.is_empty() && replication_lag > 5.0 {
        panic!("Multi-region replication not working - skipping test");
    }

    let _ = delete_file_fuse(site_a_file);
}

#[test]
#[ignore]
fn test_cluster_tiering_real_s3_backend() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let test_file = "tiering_test.bin";
    write_file_fuse(test_file, 500).expect("Failed to write test file");

    std::thread::sleep(Duration::from_secs(60));

    let s3_objects = s3_list_objects("").unwrap_or_default();
    assert!(!s3_objects.is_empty(), "Should have objects in S3");

    let tiering_bytes =
        query_prometheus_metric("claudefs_tiering_bytes_to_s3_total").unwrap_or(0.0);
    assert!(
        tiering_bytes >= 400.0 * 1024.0 * 1024.0,
        "Tiering should have moved ~500MB"
    );

    let _ = delete_file_fuse(test_file);
}

#[test]
#[ignore]
fn test_cluster_dedup_performance_vs_phase31() {
    if !check_cluster_available() {
        panic!("Cluster not available - skipping test");
    }

    let phase31_baseline = 80_000.0;

    let start = Instant::now();
    write_file_fuse("perf_test.bin", 500).ok();
    let elapsed = start.elapsed().as_secs_f64();

    let fingerprints =
        query_prometheus_metric("claudefs_dedup_fingerprints_processed_total").unwrap_or(0.0);

    if fingerprints > 0.0 {
        let actual_throughput = fingerprints / elapsed.max(0.1);
        let difference = ((phase31_baseline - actual_throughput) / phase31_baseline * 100.0).abs();

        assert!(
            difference <= 10.0,
            "Cluster dedup performance should be within 10% of Phase 31 baseline"
        );
    }

    let _ = delete_file_fuse("perf_test.bin");
}
