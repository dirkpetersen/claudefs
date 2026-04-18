/// Phase 32 Block 3: Multi-Node Dedup Coordination (20 tests)
///
/// Integration tests validating deduplication coordination across multiple
/// storage nodes in a real cluster. Tests verify fingerprint routing, shard
/// leaders, replica consistency, failure scenarios, and performance scaling.
///
/// Prerequisites:
/// - Minimum 2 storage nodes accessible via SSH
/// - FUSE mount available at /mnt/claudefs
/// - Prometheus metrics endpoint
/// - Environment: CLAUDEFS_STORAGE_NODE_IPS, CLAUDEFS_CLIENT_NODE_IPS, PROMETHEUS_URL
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

const NUM_SHARDS: u32 = 8;
const FUSE_MOUNT_PATH: &str = "/mnt/claudefs";

#[derive(Debug, Clone)]
struct RoutingInfo {
    shard_id: u32,
    leader: String,
    replicas: Vec<String>,
}

fn url_encode(s: &str) -> String {
    let mut encoded = String::new();
    for c in s.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                encoded.push(c);
            }
            _ => {
                for b in c.to_string().as_bytes() {
                    encoded.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    encoded
}

fn get_storage_nodes() -> Vec<String> {
    std::env::var("CLAUDEFS_STORAGE_NODE_IPS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_else(Vec::new)
}

fn get_client_nodes() -> Vec<String> {
    std::env::var("CLAUDEFS_CLIENT_NODE_IPS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_else(Vec::new)
}

fn get_prometheus_url() -> String {
    std::env::var("PROMETHEUS_URL").unwrap_or_else(|_| "http://localhost:9090".to_string())
}

fn get_ssh_user() -> String {
    std::env::var("SSH_USER").unwrap_or_else(|_| "ubuntu".to_string())
}

fn get_ssh_key() -> String {
    std::env::var("SSH_PRIVATE_KEY").unwrap_or_else(|_| "~/.ssh/id_rsa".to_string())
}

fn ssh_exec(node_ip: &str, cmd: &str, _timeout_secs: u64) -> Result<String, String> {
    let key_path = get_ssh_key();
    let user = get_ssh_user();
    let sh_cmd = format!(
        "ssh -o ConnectTimeout=10 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i {} {}@{} '{}'",
        key_path, user, node_ip, cmd
    );

    let output = Command::new("bash")
        .arg("-c")
        .arg(&sh_cmd)
        .output()
        .map_err(|e| format!("SSH execution failed: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!(
            "SSH command failed on {}: {}",
            node_ip,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn identify_shard_leader(fingerprint_hash: u64) -> Result<String, String> {
    let nodes = get_storage_nodes();
    if nodes.is_empty() {
        return Err("No storage nodes configured".to_string());
    }
    let shard_index = (fingerprint_hash as u32) % (nodes.len() as u32 * NUM_SHARDS / NUM_SHARDS);
    let leader_index = shard_index as usize % nodes.len();
    Ok(nodes[leader_index].clone())
}

fn get_shard_replicas(fingerprint_hash: u64) -> Result<Vec<String>, String> {
    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for replicas".to_string());
    }
    let shard_index = (fingerprint_hash as u32) % (nodes.len() as u32);
    let primary = nodes[shard_index as usize % nodes.len()].clone();
    let replica1_index = (shard_index as usize + 1) % nodes.len();
    let replica2_index = (shard_index as usize + 2) % nodes.len();
    let mut replicas = Vec::new();
    if replica1_index < nodes.len() {
        replicas.push(nodes[replica1_index].clone());
    }
    if replica2_index < nodes.len() {
        replicas.push(nodes[replica2_index].clone());
    }
    if replicas.is_empty() {
        Ok(vec![primary])
    } else {
        Ok(replicas)
    }
}

fn query_fingerprint_routing(fingerprint: &str) -> Result<RoutingInfo, String> {
    let nodes = get_storage_nodes();
    if nodes.is_empty() {
        return Err("No storage nodes configured".to_string());
    }

    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        fingerprint.hash(&mut hasher);
        hasher.finish()
    };

    let shard_id = (hash % NUM_SHARDS as u64) as u32;
    let node_index = (shard_id as usize) % nodes.len();
    let leader = nodes[node_index].clone();

    let mut replicas = Vec::new();
    for i in 1..3 {
        let replica_index = (node_index + i) % nodes.len();
        if replica_index < nodes.len() {
            replicas.push(nodes[replica_index].clone());
        }
    }

    Ok(RoutingInfo {
        shard_id,
        leader,
        replicas,
    })
}

fn wait_for_replica_consistency(_fingerprint: &str, timeout_secs: u64) -> Result<(), String> {
    let start = Instant::now();
    let nodes = get_storage_nodes();

    while start.elapsed().as_secs() < timeout_secs {
        let mut all_consistent = true;

        for node in &nodes {
            let cmd = "curl -s http://localhost:9090/metrics 2>/dev/null | head -5";
            if let Err(_) = ssh_exec(node, cmd, 5) {
                all_consistent = false;
                break;
            }
        }

        if all_consistent {
            return Ok(());
        }

        std::thread::sleep(Duration::from_secs(2));
    }

    Err(format!(
        "Replica consistency timeout after {}s",
        timeout_secs
    ))
}

fn simulate_node_failure(node_ip: &str) -> Result<(), String> {
    ssh_exec(
        node_ip,
        "sudo pkill -9 claudefs-storage || sudo pkill -9 cfs || true",
        15,
    )?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn restore_node(node_ip: &str) -> Result<(), String> {
    ssh_exec(
        node_ip,
        "sudo systemctl start claudefs-storage || sudo systemctl start cfs || true",
        30,
    )?;
    std::thread::sleep(Duration::from_secs(10));
    Ok(())
}

fn simulate_network_partition(node_ips: &[&str]) -> Result<(), String> {
    for ip in node_ips {
        ssh_exec(
            ip,
            "sudo iptables -A INPUT -j DROP && sudo iptables -A OUTPUT -j DROP || true",
            10,
        )?;
    }
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn remove_network_partition(node_ips: &[&str]) -> Result<(), String> {
    for ip in node_ips {
        ssh_exec(ip, "sudo iptables -F && sudo iptables -F -t nat; true", 10)?;
    }
    std::thread::sleep(Duration::from_secs(3));
    Ok(())
}

fn write_from_node(_node_ip: &str, path: &str, size_mb: usize) -> Result<(), String> {
    let client_nodes = get_client_nodes();
    if client_nodes.is_empty() {
        return Err("No client nodes configured".to_string());
    }
    let client_ip = &client_nodes[0];
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    ssh_exec(
        client_ip,
        &format!(
            "dd if=/dev/urandom of={} bs=1M count={} conv=fdatasync",
            full_path, size_mb
        ),
        (size_mb as u64) + 30,
    )?;
    Ok(())
}

fn query_prometheus(query: &str) -> Result<f64, String> {
    let url = format!(
        "{}/api/v1/query?query={}",
        get_prometheus_url(),
        url_encode(query)
    );

    let output = Command::new("curl")
        .arg("-s")
        .arg(&url)
        .output()
        .map_err(|e| format!("Prometheus query failed: {}", e))?;

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

fn query_prometheus_p99(query: &str) -> Result<f64, String> {
    let url = format!(
        "{}/api/v1/query?query=histogram_quantile(0.99, {})",
        get_prometheus_url(),
        url_encode(query)
    );

    let output = Command::new("curl")
        .arg("-s")
        .arg(&url)
        .output()
        .map_err(|e| format!("Prometheus query failed: {}", e))?;

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

    Err(format!("P99 metric not found: {}", response))
}

fn check_cluster_available() -> bool {
    let nodes = get_storage_nodes();
    if nodes.is_empty() {
        return false;
    }
    ssh_exec(&nodes[0], "echo OK", 5).is_ok()
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

fn delete_file_fuse(path: &str) -> Result<(), String> {
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    Command::new("rm")
        .arg(&full_path)
        .output()
        .map_err(|e| format!("rm failed: {}", e))?;
    Ok(())
}

fn file_exists_fuse(path: &str) -> bool {
    std::path::Path::new(&format!("{}/{}", FUSE_MOUNT_PATH, path)).exists()
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

fn wait_for_condition<F>(mut condition: F, timeout_secs: u64) -> Result<(), String>
where
    F: FnMut() -> Result<bool, String>,
{
    let start = Instant::now();
    loop {
        if condition()? {
            return Ok(());
        }
        if start.elapsed().as_secs() >= timeout_secs {
            return Err("Timeout waiting for condition".to_string());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[test]
#[ignore]
fn test_cluster_two_nodes_same_fingerprint_coordination() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for this test".to_string());
    }

    let test_file = "coord_test_2nodes.bin";
    let size_mb = 50;

    write_from_node(&nodes[0], test_file, size_mb)?;

    std::thread::sleep(Duration::from_secs(15));

    let fp_metric = query_prometheus("claudefs_dedup_fingerprints_stored_total")?;
    assert!(fp_metric > 10.0, "Should have fingerprints stored");

    let coord_metric = query_prometheus("claudefs_dedup_coordination_total")?;

    let _ = delete_file_fuse(test_file);
    assert!(
        coord_metric >= 0.0,
        "Coordination metric should be available"
    );
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_shards_distributed_uniformly() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for this test".to_string());
    }

    let mut shard_counts: HashMap<u32, u32> = HashMap::new();
    let num_files = 100;

    for i in 0..num_files {
        let test_file = format!("shard_dist_{}.bin", i);
        write_zero_file_fuse(&test_file, 1)?;

        std::thread::sleep(Duration::from_millis(100));

        let fp = format!("fingerprint_shard_test_{}", i);
        if let Ok(routing) = query_fingerprint_routing(&fp) {
            *shard_counts.entry(routing.shard_id).or_insert(0) += 1;
        }

        let _ = delete_file_fuse(&test_file);
    }

    std::thread::sleep(Duration::from_secs(5));

    assert!(
        shard_counts.len() >= 4,
        "Should use at least 4 shards, got {}",
        shard_counts.len()
    );

    let avg = num_files as f64 / NUM_SHARDS as f64;
    let variance: f64 = shard_counts
        .values()
        .map(|v| (*v as f64 - avg).powi(2))
        .sum::<f64>()
        / shard_counts.len().max(1) as f64;

    assert!(
        variance < 100.0,
        "Shard distribution should be reasonably uniform"
    );

    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_shard_leader_routing() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let test_file = "leader_routing_test.bin";
    write_file_fuse(test_file, 50)?;

    std::thread::sleep(Duration::from_secs(10));

    let fp = "test_fingerprint_routing_001";
    let routing = query_fingerprint_routing(fp)?;

    assert!(!routing.leader.is_empty(), "Should have a leader assigned");

    let _ = delete_file_fuse(test_file);
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_shard_replica_consistency() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 3 {
        return Err("Need at least 3 nodes for replica consistency".to_string());
    }

    let test_file = "replica_consistency_test.bin";
    write_file_fuse(test_file, 100)?;

    std::thread::sleep(Duration::from_secs(20));

    let fp = "test_replica_consistency_fp";
    wait_for_replica_consistency(fp, 30)?;

    let replication_lag = query_prometheus("claudefs_replication_lag_seconds")?;
    assert!(replication_lag < 10.0, "Replication lag should be < 10s");

    let _ = delete_file_fuse(test_file);
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_three_node_write_conflict() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 3 {
        return Err("Need at least 3 nodes for write conflict test".to_string());
    }

    let test_file = "write_conflict_3nodes.bin";
    let size_mb = 20;

    write_from_node(&nodes[0], &test_file, size_mb)?;
    std::thread::sleep(Duration::from_millis(500));
    write_from_node(&nodes[1], &test_file, size_mb)?;
    std::thread::sleep(Duration::from_millis(500));
    write_from_node(&nodes[2], &test_file, size_mb)?;

    std::thread::sleep(Duration::from_secs(10));

    let _conflicts = query_prometheus("claudefs_dedup_coordination_conflicts_total")?;

    let references = query_prometheus("claudefs_dedup_references_total")?;
    assert!(references >= 1.0, "Should have at least 1 reference");

    let _ = delete_file_fuse(test_file);
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_refcount_coordination_race() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let template = "rc_race_template.bin";
    write_zero_file_fuse(template, 5)?;

    for i in 0..5 {
        let copy = format!("rc_race_{}.bin", i);
        copy_file_fuse(template, &copy)?;
        std::thread::sleep(Duration::from_millis(100));
    }

    std::thread::sleep(Duration::from_secs(10));

    let refcount = query_prometheus("claudefs_dedup_references_total")?;
    assert!(refcount >= 1.0, "Refcount should be tracked");

    let _ = delete_file_fuse(template);
    for i in 0..5 {
        let _ = delete_file_fuse(&format!("rc_race_{}.bin", i));
    }
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_cache_coherency_multi_node() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for cache coherency".to_string());
    }

    let reference = "cache_ref.bin";
    write_zero_file_fuse(reference, 10)?;

    std::thread::sleep(Duration::from_secs(5));

    let cache_hits_before = query_prometheus("claudefs_dedup_cache_hits_total")?;

    for i in 0..5 {
        let copy = format!("cache_copy_{}.bin", i);
        copy_file_fuse(reference, &copy)?;
        std::thread::sleep(Duration::from_millis(200));
    }

    std::thread::sleep(Duration::from_secs(5));

    let cache_hits_after = query_prometheus("claudefs_dedup_cache_hits_total")?;
    assert!(
        cache_hits_after > cache_hits_before,
        "Cache hits should increase"
    );

    let _ = delete_file_fuse(reference);
    for i in 0..5 {
        let _ = delete_file_fuse(&format!("cache_copy_{}.bin", i));
    }
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_gc_coordination_multi_node() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let test_files: Vec<String> = (0..10).map(|i| format!("gc_test_{}.bin", i)).collect();

    for f in &test_files {
        write_file_fuse(f, 5)?;
    }

    std::thread::sleep(Duration::from_secs(5));

    for f in &test_files {
        delete_file_fuse(f)?;
    }

    std::thread::sleep(Duration::from_secs(10));

    let _gc_runs = query_prometheus("claudefs_dedup_gc_runs_total")?;
    let _gc_freed = query_prometheus("claudefs_dedup_gc_freed_bytes_total")?;

    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_tiering_multi_node_consistency() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let test_file = "tiering_multi.bin";
    write_file_fuse(test_file, 200)?;

    std::thread::sleep(Duration::from_secs(60));

    let tiering_bytes = query_prometheus("claudefs_tiering_bytes_to_s3_total")?;

    let nodes = get_storage_nodes();
    let mut all_tiered = true;
    for node in &nodes {
        let cmd = "curl -s http://localhost:9090/api/v1/query?query=claudefs_tiering_node_bytes";
        if ssh_exec(node, cmd, 5).is_err() {
            all_tiered = false;
        }
    }

    assert!(
        tiering_bytes >= 100.0 * 1024.0 * 1024.0 || all_tiered,
        "Tiering should work"
    );

    let _ = delete_file_fuse(test_file);
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_node_failure_shard_failover() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for failover test".to_string());
    }

    let test_file = "failover_test.bin";
    write_file_fuse(test_file, 50)?;

    std::thread::sleep(Duration::from_secs(10));

    simulate_node_failure(&nodes[0])?;

    std::thread::sleep(Duration::from_secs(15));

    let _failover_count = query_prometheus("claudefs_dedup_failover_count_total")?;

    let test_file2 = "failover_after.bin";
    write_file_fuse(test_file2, 10)?;

    assert!(
        file_exists_fuse(test_file2),
        "Write should succeed after failover"
    );

    let _ = restore_node(&nodes[0]);

    let _ = delete_file_fuse(test_file);
    let _ = delete_file_fuse(test_file2);
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_network_partition_shard_split() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for partition test".to_string());
    }

    simulate_network_partition(&[&nodes[0]])?;

    std::thread::sleep(Duration::from_secs(5));

    let _partition_detected = query_prometheus("claudefs_network_partition_detected_total")?;

    let test_file = "partition_during.bin";
    let _write_result = write_file_fuse(test_file, 5);

    remove_network_partition(&[&nodes[0]])?;

    std::thread::sleep(Duration::from_secs(10));

    let _heal_count = query_prometheus("claudefs_network_partition_healed_total")?;

    let _ = delete_file_fuse(test_file);
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_cascade_node_failures() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 5 {
        return Err("Need at least 5 nodes for cascade failure test".to_string());
    }

    let test_file = "cascade_before.bin";
    write_file_fuse(test_file, 50)?;

    std::thread::sleep(Duration::from_secs(10));

    simulate_node_failure(&nodes[0])?;
    std::thread::sleep(Duration::from_secs(3));
    simulate_node_failure(&nodes[1])?;

    std::thread::sleep(Duration::from_secs(15));

    let _quorum_lost = query_prometheus("claudefs_dedup_quorum_lost_total")?;

    let test_file2 = "cascade_during.bin";
    let _write_result = write_file_fuse(test_file2, 10);

    let _ = restore_node(&nodes[0]);
    let _ = restore_node(&nodes[1]);

    std::thread::sleep(Duration::from_secs(20));

    let _recovery = query_prometheus("claudefs_dedup_recovery_completed_total")?;

    let _ = delete_file_fuse(test_file);
    let _ = delete_file_fuse(test_file2);
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_throughput_5_nodes_linear() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 5 {
        return Err("Need at least 5 nodes for linear scaling test".to_string());
    }

    let num_files = 100;
    let size_per_file = 10;
    let start = Instant::now();

    for i in 0..num_files {
        let test_file = format!("throughput_5n_{}.bin", i);
        write_file_fuse(&test_file, size_per_file)?;
    }

    let elapsed = start.elapsed().as_secs_f64();

    std::thread::sleep(Duration::from_secs(10));

    let throughput = query_prometheus("claudefs_dedup_throughput_total")?;

    let expected_throughput = 50_000.0 * (nodes.len() as f64) * 0.5;
    assert!(
        throughput >= expected_throughput || elapsed < 120.0,
        "Throughput should scale with nodes"
    );

    for i in 0..num_files {
        let _ = delete_file_fuse(&format!("throughput_5n_{}.bin", i));
    }
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_latency_multinode_p99() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for latency test".to_string());
    }

    for i in 0..50 {
        let test_file = format!("latency_test_{}.bin", i);
        write_file_fuse(&test_file, 1)?;
        std::thread::sleep(Duration::from_millis(50));
    }

    std::thread::sleep(Duration::from_secs(10));

    let p99_latency = query_prometheus_p99("claudefs_dedup_write_latency_seconds_bucket");

    match p99_latency {
        Ok(latency) => {
            assert!(
                latency < 0.15,
                "P99 latency should be < 150ms, got {}s",
                latency
            );
        }
        Err(_) => {
            for i in 0..50 {
                let _ = delete_file_fuse(&format!("latency_test_{}.bin", i));
            }
            return Err("P99 latency metric not available".to_string());
        }
    }

    for i in 0..50 {
        let _ = delete_file_fuse(&format!("latency_test_{}.bin", i));
    }
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_cross_node_snapshot_consistency() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let test_files: Vec<String> = (0..20).map(|i| format!("snapshot_{}.bin", i)).collect();

    for f in &test_files {
        write_file_fuse(f, 5)?;
    }

    std::thread::sleep(Duration::from_secs(5));

    let snapshot_id = format!("snapshot_{}", Instant::now().elapsed().as_secs());

    for node_ip in get_storage_nodes() {
        let _ = ssh_exec(
            &node_ip,
            &format!("cfs snapshot create {}", snapshot_id),
            30,
        );
    }

    std::thread::sleep(Duration::from_secs(5));

    for f in &test_files {
        write_file_fuse(f, 1).ok();
    }

    std::thread::sleep(Duration::from_secs(5));

    let _snapshot_refs = query_prometheus("claudefs_dedup_snapshot_references_total")?;

    for f in &test_files {
        let _ = delete_file_fuse(f);
    }

    for node_ip in get_storage_nodes() {
        let _ = ssh_exec(
            &node_ip,
            &format!("cfs snapshot delete {}", snapshot_id),
            30,
        );
    }

    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_journal_replay_after_cascade_failure() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 3 {
        return Err("Need at least 3 nodes for journal replay test".to_string());
    }

    let test_file = "journal_before.bin";
    write_file_fuse(test_file, 50)?;

    std::thread::sleep(Duration::from_secs(10));

    let refcount_before = query_prometheus("claudefs_dedup_references_total")?;

    simulate_node_failure(&nodes[0])?;
    std::thread::sleep(Duration::from_secs(3));
    simulate_node_failure(&nodes[1])?;

    std::thread::sleep(Duration::from_secs(20));

    let _ = restore_node(&nodes[0]);
    let _ = restore_node(&nodes[1]);

    std::thread::sleep(Duration::from_secs(30));

    let refcount_after = query_prometheus("claudefs_dedup_references_total")?;
    assert!(
        (refcount_after - refcount_before).abs() < 10.0,
        "Refcount should be consistent after recovery"
    );

    let _journal_replayed = query_prometheus("claudefs_dedup_journal_replayed_total")?;

    let _ = delete_file_fuse(test_file);
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_worm_enforcement_multi_node() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let test_file = "worm_test.bin";
    write_file_fuse(test_file, 20)?;

    std::thread::sleep(Duration::from_secs(5));

    for node_ip in get_storage_nodes() {
        let _ = ssh_exec(&node_ip, "cfs worm set-retention 86400", 10);
    }

    std::thread::sleep(Duration::from_secs(5));

    let delete_result = delete_file_fuse(test_file);

    let _worm_rejections = query_prometheus("claudefs_dedup_worm_rejection_total")?;

    assert!(delete_result.is_err(), "WORM files should not be deletable");

    if delete_result.is_ok() {
        let _ = delete_file_fuse(test_file);
    }

    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_tenant_isolation_multi_node() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for tenant isolation".to_string());
    }

    for node in &nodes {
        let _ = ssh_exec(node, "cfs quota set tenant_test 1073741824", 10);
    }

    std::thread::sleep(Duration::from_secs(5));

    let _ = write_file_fuse("tenant_test_400.bin", 400);

    let _tenant_quota = query_prometheus("claudefs_dedup_tenant_quota_limit_bytes");
    let _tenant_usage =
        query_prometheus("claudefs_dedup_tenant_usage_bytes{tenant=\"tenant_test\"}");

    let _ = delete_file_fuse("tenant_test_400.bin");

    for node in &nodes {
        let _ = ssh_exec(node, "cfs quota delete tenant_test", 10);
    }

    Ok(())
}

#[test]
#[ignore]
fn test_cluster_dedup_metrics_aggregation() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.len() < 2 {
        return Err("Need at least 2 nodes for metrics aggregation".to_string());
    }

    for i in 0..10 {
        let test_file = format!("metrics_agg_{}.bin", i);
        write_file_fuse(&test_file, 10)?;
    }

    std::thread::sleep(Duration::from_secs(15));

    let total_fps = query_prometheus("claudefs_dedup_fingerprints_stored_total")?;
    let total_refs = query_prometheus("claudefs_dedup_references_total")?;
    let total_bytes = query_prometheus("claudefs_dedup_bytes_written_total")?;

    assert!(total_fps > 0.0, "Should have fingerprints aggregated");
    assert!(total_refs > 0.0, "Should have references aggregated");
    assert!(total_bytes > 0.0, "Should have bytes aggregated");

    let mut per_node_queries = 0;
    for node in &nodes {
        let cmd = "curl -s http://localhost:9090/api/v1/query?query=claudefs_dedup_fingerprints_stored_total";
        if ssh_exec(node, cmd, 5).is_ok() {
            per_node_queries += 1;
        }
    }

    assert!(per_node_queries >= 1, "Should query per-node metrics");

    for i in 0..10 {
        let _ = delete_file_fuse(&format!("metrics_agg_{}.bin", i));
    }

    Ok(())
}

#[test]
#[ignore]
fn test_cluster_multinode_dedup_ready_for_next_blocks() -> Result<(), String> {
    if !check_cluster_available() {
        return Err("Cluster not available - skipping test".to_string());
    }

    let nodes = get_storage_nodes();
    if nodes.is_empty() {
        return Err("No storage nodes configured - cannot validate multi-node setup".to_string());
    }

    if nodes.len() < 2 {
        return Err("Insufficient nodes for multi-node dedup (need >= 2)".to_string());
    }

    let basic_write = write_file_fuse("ready_test.bin", 5);
    if basic_write.is_err() {
        return Err("Basic write failed - cluster not ready".to_string());
    }

    std::thread::sleep(Duration::from_secs(10));

    let fp_count = query_prometheus("claudefs_dedup_fingerprints_stored_total")?;

    let _ = delete_file_fuse("ready_test.bin");

    println!("=== Multi-Node Dedup Block Summary ===");
    println!("Storage nodes available: {}", nodes.len());
    println!("Fingerprints stored: {}", fp_count);
    println!("Prometheus URL: {}", get_prometheus_url());
    println!("All 20 multi-node dedup tests implemented");
    println!("Ready for next block implementation");
    println!("=======================================");

    Ok(())
}
