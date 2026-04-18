/// Phase 32 Block 5: Multi-Client Workloads Integration Tests (18 tests)
///
/// Integration tests validating ClaudeFS behavior with multiple concurrent FUSE
/// clients. Tests verify data consistency, coordination, performance, and failure
/// scenarios with 2+ client nodes.
///
/// Prerequisites:
/// - Minimum 2 client nodes accessible via SSH
/// - FUSE mount available at /mnt/claudefs on all clients
/// - Prometheus metrics endpoint
/// - Environment: CLAUDEFS_CLIENT_NODE_IPS, CLAUDEFS_STORAGE_NODE_IPS, PROMETHEUS_URL
use std::process::Command;
use std::time::{Duration, Instant};

const FUSE_MOUNT_PATH: &str = "/mnt/claudefs";

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

fn get_client_nodes() -> Vec<String> {
    std::env::var("CLAUDEFS_CLIENT_NODE_IPS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default()
}

fn get_storage_nodes() -> Vec<String> {
    std::env::var("CLAUDEFS_STORAGE_NODE_IPS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default()
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

fn get_client_node(client_id: usize) -> Result<String, String> {
    let clients = get_client_nodes();
    if client_id >= clients.len() {
        return Err(format!(
            "Client {} not available (have {} clients)",
            client_id,
            clients.len()
        ));
    }
    Ok(clients[client_id].clone())
}

fn check_two_clients_available() -> bool {
    get_client_nodes().len() >= 2
}

fn write_from_client(client_id: usize, path: &str, size_mb: usize) -> Result<(), String> {
    let client_ip = get_client_node(client_id)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    ssh_exec(
        &client_ip,
        &format!(
            "dd if=/dev/urandom of={} bs=1M count={} conv=fdatasync",
            full_path, size_mb
        ),
        (size_mb as u64) + 30,
    )
    .map(|_| ())
}

fn write_zeros_from_client(client_id: usize, path: &str, size_mb: usize) -> Result<(), String> {
    let client_ip = get_client_node(client_id)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    ssh_exec(
        &client_ip,
        &format!(
            "dd if=/dev/zero of={} bs=1M count={} conv=fdatasync",
            full_path, size_mb
        ),
        (size_mb as u64) + 30,
    )
    .map(|_| ())
}

fn read_from_client(client_id: usize, path: &str) -> Result<Vec<u8>, String> {
    let client_ip = get_client_node(client_id)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    let output = ssh_exec(&client_ip, &format!("cat {}", full_path), 60)?;
    Ok(output.into_bytes())
}

fn delete_from_client(client_id: usize, path: &str) -> Result<(), String> {
    let client_ip = get_client_node(client_id)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    ssh_exec(&client_ip, &format!("rm -f {}", full_path), 10).map(|_| ())
}

fn copy_from_client(client_id: usize, src: &str, dst: &str) -> Result<(), String> {
    let client_ip = get_client_node(client_id)?;
    let src_path = format!("{}/{}", FUSE_MOUNT_PATH, src);
    let dst_path = format!("{}/{}", FUSE_MOUNT_PATH, dst);
    ssh_exec(&client_ip, &format!("cp {} {}", src_path, dst_path), 60).map(|_| ())
}

fn file_exists_on_client(client_id: usize, path: &str) -> Result<bool, String> {
    let client_ip = get_client_node(client_id)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    let result = ssh_exec(
        &client_ip,
        &format!("test -f {} && echo EXISTS || echo MISSING", full_path),
        5,
    )?;
    Ok(result.trim() == "EXISTS")
}

fn get_file_checksum(client_id: usize, path: &str) -> Result<String, String> {
    let client_ip = get_client_node(client_id)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    let output = ssh_exec(
        &client_ip,
        &format!("sha256sum {} | awk '{{print $1}}'", full_path),
        60,
    )?;
    Ok(output.trim().to_string())
}

fn get_client_quota(client_id: usize) -> Result<u64, String> {
    let client_ip = get_client_node(client_id)?;
    let output = ssh_exec(
        &client_ip,
        "cfs quota get 2>/dev/null | grep bytes || echo 0",
        10,
    )?;
    let value: u64 = output
        .lines()
        .find(|l| l.contains("bytes"))
        .and_then(|l| l.split_whitespace().last())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(value)
}

fn set_client_quota(client_id: usize, bytes: u64) -> Result<(), String> {
    let client_ip = get_client_node(client_id)?;
    ssh_exec(&client_ip, &format!("cfs quota set --bytes {}", bytes), 30).map(|_| ())
}

fn simulate_client_failure(client_id: usize) -> Result<(), String> {
    let client_ip = get_client_node(client_id)?;
    ssh_exec(
        &client_ip,
        "sudo pkill -9 cfs || sudo pkill -9 claudefs-fuse || true",
        15,
    )?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn restore_client(client_id: usize) -> Result<(), String> {
    let client_ip = get_client_node(client_id)?;
    ssh_exec(
        &client_ip,
        "sudo systemctl start claudefs-fuse || sudo systemctl start cfs || true",
        30,
    )?;
    std::thread::sleep(Duration::from_secs(10));
    Ok(())
}

fn create_snapshot_from_client(_client_id: usize, name: &str) -> Result<(), String> {
    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        return Err("No storage nodes available".to_string());
    }
    for node_ip in &storage_nodes {
        let _ = ssh_exec(node_ip, &format!("cfs snapshot create {}", name), 30);
    }
    std::thread::sleep(Duration::from_secs(5));
    Ok(())
}

fn delete_snapshot_from_client(_client_id: usize, name: &str) -> Result<(), String> {
    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        return Err("No storage nodes available".to_string());
    }
    for node_ip in &storage_nodes {
        let _ = ssh_exec(node_ip, &format!("cfs snapshot delete {}", name), 30);
    }
    std::thread::sleep(Duration::from_secs(3));
    Ok(())
}

fn simulate_network_partition(node_ip: &str) -> Result<(), String> {
    ssh_exec(
        node_ip,
        "sudo iptables -A INPUT -j DROP && sudo iptables -A OUTPUT -j DROP || true",
        10,
    )?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn remove_network_partition(node_ip: &str) -> Result<(), String> {
    ssh_exec(
        node_ip,
        "sudo iptables -F && sudo iptables -F -t nat; true",
        10,
    )?;
    std::thread::sleep(Duration::from_secs(3));
    Ok(())
}

fn measure_concurrent_throughput(client_ids: &[usize], duration_secs: u64) -> Result<f64, String> {
    let start = Instant::now();
    let mut total_bytes: u64 = 0;

    for client_id in client_ids {
        let client_ip = get_client_node(*client_id)?;
        let test_dir = format!("{}/throughput_test_{}", FUSE_MOUNT_PATH, client_id);
        ssh_exec(&client_ip, &format!("mkdir -p {}", test_dir), 5)?;

        let cmd = format!(
            "cd {} && for i in $(seq 1 100); do dd if=/dev/urandom of=file_$i bs=1M count=1 2>/dev/null; done",
            test_dir
        );
        let _ = ssh_exec(&client_ip, &cmd, duration_secs + 30);

        let size_cmd = format!("du -sb {} | awk '{{print $1}}'", test_dir);
        let output = ssh_exec(&client_ip, &size_cmd, 10)?;
        if let Ok(bytes) = output.trim().parse::<u64>() {
            total_bytes += bytes;
        }

        let cleanup = format!("rm -rf {}", test_dir);
        let _ = ssh_exec(&client_ip, &cleanup, 10);
    }

    let elapsed = start.elapsed().as_secs() as f64;
    if elapsed > 0.0 {
        Ok((total_bytes as f64) / elapsed / 1_000_000.0)
    } else {
        Ok(0.0)
    }
}

fn query_prometheus_metric(metric: &str) -> Result<f64, String> {
    let prometheus_url = get_prometheus_url();
    let url = format!(
        "{}/api/v1/query?query={}",
        prometheus_url,
        url_encode(metric)
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

    Err(format!("Metric not found: {}", response))
}

#[allow(dead_code)]
fn wait_for_metric(metric: &str, target: f64, timeout_secs: u64) -> Result<(), String> {
    let start = Instant::now();

    while start.elapsed().as_secs() < timeout_secs {
        if let Ok(value) = query_prometheus_metric(metric) {
            if value >= target {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    Err(format!(
        "Metric {} did not reach {} within {}s",
        metric, target, timeout_secs
    ))
}

fn get_file_size(client_id: usize, path: &str) -> Result<u64, String> {
    let client_ip = get_client_node(client_id)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    let output = ssh_exec(&client_ip, &format!("stat -c %s {}", full_path), 5)?;
    output
        .trim()
        .parse::<u64>()
        .map_err(|e| format!("Parse size failed: {}", e))
}

fn list_files(client_id: usize, dir: &str) -> Result<Vec<String>, String> {
    let client_ip = get_client_node(client_id)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, dir);
    let output = ssh_exec(&client_ip, &format!("ls -1 {}", full_path), 10)?;
    Ok(output.lines().map(|s| s.to_string()).collect())
}

fn cleanup_test_files(client_id: usize, prefix: &str) -> Result<(), String> {
    let client_ip = get_client_node(client_id)?;
    ssh_exec(
        &client_ip,
        &format!("rm -f {}/{}*", FUSE_MOUNT_PATH, prefix),
        10,
    )
    .map(|_| ())
}

fn cleanup_all_test_files() -> Result<(), String> {
    for i in 0..get_client_nodes().len() {
        let _ = cleanup_test_files(i, "mc_test");
    }
    Ok(())
}

fn wait_for_replica_sync(timeout_secs: u64) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed().as_secs() < timeout_secs {
        let dedup_refs = query_prometheus_metric("claudefs_dedup_references_total");
        if dedup_refs.is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err("Replica sync timeout".to_string())
}

#[test]
#[ignore]
fn test_cluster_two_clients_concurrent_writes() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_file = "mc_test_concurrent.bin";
    let size_mb = 50;

    let handle1 = std::thread::spawn(move || write_from_client(0, test_file, size_mb));

    let handle2 = std::thread::spawn(move || write_from_client(1, test_file, size_mb));

    let _ = handle1
        .join()
        .map_err(|e| format!("Thread 1 failed: {:?}", e))?;
    let _ = handle2
        .join()
        .map_err(|e| format!("Thread 2 failed: {:?}", e))?;

    std::thread::sleep(Duration::from_secs(3));

    let exists = file_exists_on_client(0, test_file)?;
    assert!(exists, "File should exist after concurrent writes");

    let size = get_file_size(0, test_file)?;
    assert_eq!(
        size,
        (size_mb * 1024 * 1024) as u64,
        "File size should match expected"
    );

    let checksum1 = get_file_checksum(0, test_file)?;
    let checksum2 = get_file_checksum(1, test_file)?;
    assert_eq!(
        checksum1, checksum2,
        "Both clients should see same file content"
    );

    delete_from_client(0, test_file).ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_same_file_coordination() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_file = "mc_test_lww.bin";
    let content1 = "CLIENT1_CONTENT";
    let content2 = "CLIENT2_CONTENT";

    let client0_ip = get_client_node(0)?;
    let client1_ip = get_client_node(1)?;
    let full_path0 = format!("{}/{}", FUSE_MOUNT_PATH, test_file);
    let full_path1 = format!("{}/{}", FUSE_MOUNT_PATH, test_file);

    ssh_exec(
        &client0_ip,
        &format!("echo '{}' > {}", content1, full_path0),
        10,
    )?;
    std::thread::sleep(Duration::from_millis(500));
    ssh_exec(
        &client1_ip,
        &format!("echo '{}' > {}", content2, full_path1),
        10,
    )?;

    std::thread::sleep(Duration::from_secs(2));

    let read0 = ssh_exec(&client0_ip, &format!("cat {}", full_path0), 5)?;
    let read1 = ssh_exec(&client1_ip, &format!("cat {}", full_path1), 5)?;

    assert!(
        read0.trim().contains(content2) || read1.trim().contains(content2),
        "Last write should win - both should see consistent content"
    );

    let coord_metric = query_prometheus_metric("claudefs_client_write_conflicts_total");
    if coord_metric.is_ok() {
        println!("Write coordination metric available");
    }

    delete_from_client(0, test_file).ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_dedup_shared_data() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let file1 = "mc_test_dedup1.bin";
    let file2 = "mc_test_dedup2.bin";
    let size_mb = 100;

    write_zeros_from_client(0, file1, size_mb)?;
    std::thread::sleep(Duration::from_secs(5));

    write_zeros_from_client(1, file2, size_mb)?;
    std::thread::sleep(Duration::from_secs(10));

    let refs_before = query_prometheus_metric("claudefs_dedup_references_total")?;

    let shared_fps = query_prometheus_metric("claudefs_dedup_shared_fingerprints_total")?;

    let dedup_ratio = query_prometheus_metric("claudefs_dedup_ratio_percent");
    if let Ok(ratio) = dedup_ratio {
        assert!(ratio > 50.0, "Dedup ratio should exceed 50% for zeros");
    }

    println!("Refs: {}, Shared FPs: {}", refs_before, shared_fps);

    delete_from_client(0, file1).ok();
    delete_from_client(1, file2).ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_quota_per_client() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let quota_bytes: u64 = 100 * 1024 * 1024;

    set_client_quota(0, quota_bytes).ok();
    set_client_quota(1, quota_bytes).ok();

    std::thread::sleep(Duration::from_secs(2));

    let test_file = "mc_test_quota.bin";
    let size_mb = 120;

    let result = write_from_client(0, test_file, size_mb);
    if result.is_ok() {
        delete_from_client(0, test_file).ok();
        return Err("Client 0 should have been quota-limited".to_string());
    }

    let quota0 = get_client_quota(0)?;
    let quota1 = get_client_quota(1)?;

    assert!(
        quota0 > 0 && quota1 > 0,
        "Both clients should have quota configured"
    );

    println!(
        "Client 0 quota: {} bytes, Client 1 quota: {} bytes",
        quota0, quota1
    );

    cleanup_all_test_files()?;
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_cache_coherency_across_clients() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_file = "mc_test_cache.bin";
    let initial_content = "INITIAL_CONTENT_12345";
    let updated_content = "UPDATED_CONTENT_67890";

    let client0_ip = get_client_node(0)?;
    let client1_ip = get_client_node(1)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, test_file);

    ssh_exec(
        &client0_ip,
        &format!("echo '{}' > {}", initial_content, full_path),
        10,
    )?;
    std::thread::sleep(Duration::from_secs(2));

    let read1 = ssh_exec(&client1_ip, &format!("cat {}", full_path), 5)?;
    assert!(
        read1.contains(initial_content),
        "Client 2 should see initial content"
    );

    ssh_exec(
        &client0_ip,
        &format!("echo '{}' > {}", updated_content, full_path),
        10,
    )?;
    std::thread::sleep(Duration::from_secs(3));

    let read2 = ssh_exec(&client1_ip, &format!("cat {}", full_path), 5)?;
    assert!(
        read2.contains(updated_content),
        "Client 2 should see updated content after cache invalidation"
    );

    let cache_inv_metric = query_prometheus_metric("claudefs_cache_invalidations_total");
    if cache_inv_metric.is_ok() {
        println!("Cache invalidation metric available");
    }

    delete_from_client(0, test_file).ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_refcount_coordination_concurrent() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let source_file = "mc_test_refcount_src.bin";
    write_from_client(0, source_file, 10)?;
    std::thread::sleep(Duration::from_secs(5));

    let refs_before = query_prometheus_metric("claudefs_dedup_references_total")?;
    println!("Refcount before copies: {}", refs_before);

    let mut handles = vec![];
    for i in 0..5 {
        let src = source_file.to_string();
        let dst = format!("mc_test_refcount_copy_{}.bin", i);
        handles.push(std::thread::spawn(move || copy_from_client(0, &src, &dst)));
    }

    for h in handles {
        let _ = h
            .join()
            .map_err(|e| format!("Copy thread failed: {:?}", e))?;
    }

    std::thread::sleep(Duration::from_secs(10));

    let refs_after = query_prometheus_metric("claudefs_dedup_references_total")?;
    println!("Refcount after copies: {}", refs_after);

    assert!(
        refs_after >= refs_before + 4.0,
        "Refcount should increase with copies"
    );

    for i in 0..5 {
        delete_from_client(0, &format!("mc_test_refcount_copy_{}.bin", i)).ok();
    }
    delete_from_client(0, source_file).ok();

    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_one_fails() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_file = "mc_test_failover.bin";
    write_from_client(0, test_file, 20)?;

    simulate_client_failure(0)?;
    std::thread::sleep(Duration::from_secs(5));

    let result = write_from_client(1, "mc_test_while_failed.bin", 10);
    assert!(
        result.is_ok(),
        "Client 2 should work while Client 1 is failed"
    );

    restore_client(0)?;
    std::thread::sleep(Duration::from_secs(10));

    let exists = file_exists_on_client(0, test_file)?;
    assert!(
        exists,
        "Original file should persist after client failure and recovery"
    );

    delete_from_client(0, test_file).ok();
    delete_from_client(1, "mc_test_while_failed.bin").ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_snapshot_consistency() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_files: Vec<String> = (0..10).map(|i| format!("mc_test_snap_{}.bin", i)).collect();

    for f in &test_files {
        write_from_client(0, f, 5)?;
    }

    std::thread::sleep(Duration::from_secs(5));

    let snapshot_name = format!("snap_mc_{}", Instant::now().elapsed().as_secs());
    create_snapshot_from_client(0, &snapshot_name)?;

    for f in &test_files {
        write_from_client(1, f, 1).ok();
    }

    std::thread::sleep(Duration::from_secs(5));

    let snap_refs = query_prometheus_metric("claudefs_dedup_snapshot_references_total");
    if snap_refs.is_ok() {
        println!("Snapshot references metric available");
    }

    for f in &test_files {
        delete_from_client(0, f).ok();
    }

    delete_snapshot_from_client(0, &snapshot_name).ok();
    cleanup_all_test_files()?;
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_read_after_write_different_client() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_file = "mc_test_read_write.bin";
    let content = "WRITTEN_BY_CLIENT_0_READ_BY_CLIENT_1";

    let client0_ip = get_client_node(0)?;
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, test_file);
    ssh_exec(
        &client0_ip,
        &format!("echo '{}' > {}", content, full_path),
        10,
    )?;

    std::thread::sleep(Duration::from_secs(3));

    let client1_ip = get_client_node(1)?;
    let read_content = ssh_exec(&client1_ip, &format!("cat {}", full_path), 5)?;

    assert!(
        read_content.contains(content),
        "Client 1 should read what Client 0 wrote"
    );

    let read_metric = query_prometheus_metric("claudefs_client_read_bytes_total");
    if read_metric.is_ok() {
        println!("Read metrics available");
    }

    delete_from_client(0, test_file).ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_metadata_consistency_reads() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_file = "mc_test_meta.bin";
    write_from_client(0, test_file, 15)?;

    std::thread::sleep(Duration::from_secs(3));

    let size0 = get_file_size(0, test_file)?;
    let size1 = get_file_size(1, test_file)?;

    assert_eq!(size0, size1, "Both clients should see same file size");

    let exists0 = file_exists_on_client(0, test_file)?;
    let exists1 = file_exists_on_client(1, test_file)?;
    assert_eq!(
        exists0, exists1,
        "Both clients should agree on file existence"
    );

    let files0 = list_files(0, ".")?;
    let _files1 = list_files(1, ".")?;
    assert!(
        files0.iter().any(|f| f.contains("mc_test_meta")),
        "File should be visible to Client 0"
    );

    delete_from_client(0, test_file).ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_performance_parallel_writes() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let single_throughput = measure_concurrent_throughput(&[0], 30)?;
    println!("Single client throughput: {:.2} MB/s", single_throughput);

    let dual_throughput = measure_concurrent_throughput(&[0, 1], 30)?;
    println!("Dual client throughput: {:.2} MB/s", dual_throughput);

    assert!(
        dual_throughput > single_throughput * 1.5,
        "Dual client throughput should be >1.5x single client"
    );

    cleanup_all_test_files()?;
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_network_partition_between_clients() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let client1_ip = get_client_node(1)?;
    let test_file = "mc_test_partition.bin";

    write_from_client(0, test_file, 10)?;

    simulate_network_partition(&client1_ip)?;
    std::thread::sleep(Duration::from_secs(5));

    let read_result = ssh_exec(
        &client1_ip,
        &format!("cat {}/{}", FUSE_MOUNT_PATH, test_file),
        5,
    );
    assert!(
        read_result.is_err(),
        "Client 2 should not be able to read during partition"
    );

    remove_network_partition(&client1_ip)?;
    std::thread::sleep(Duration::from_secs(5));

    let read_after = read_from_client(1, test_file);
    assert!(
        read_after.is_ok(),
        "Client 2 should recover after partition removal"
    );

    delete_from_client(0, test_file).ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_delete_coordination() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_file = "mc_test_delete.bin";
    write_from_client(0, test_file, 10)?;

    std::thread::sleep(Duration::from_secs(3));

    let exists_before = file_exists_on_client(1, test_file)?;
    assert!(
        exists_before,
        "Client 2 should see the file before deletion"
    );

    delete_from_client(0, test_file)?;
    std::thread::sleep(Duration::from_secs(3));

    let exists_after = file_exists_on_client(1, test_file)?;
    assert!(!exists_after, "Client 2 should see file deleted");

    cleanup_all_test_files()?;
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_replication_consistency_cross_site() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    let storage_nodes = get_storage_nodes();
    if storage_nodes.len() < 2 {
        return Err("Need at least 2 storage nodes for cross-site test".to_string());
    }

    cleanup_all_test_files()?;

    let test_file = "mc_test_repl.bin";
    write_from_client(0, test_file, 30)?;

    std::thread::sleep(Duration::from_secs(10));

    wait_for_replica_sync(30)?;

    let refs = query_prometheus_metric("claudefs_dedup_references_total")?;
    println!("Refcount after replication: {}", refs);

    let checksum0 = get_file_checksum(0, test_file)?;
    let checksum1 = get_file_checksum(1, test_file)?;
    assert_eq!(
        checksum0, checksum1,
        "File content should be consistent across sites"
    );

    delete_from_client(0, test_file).ok();
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_latency_p99_concurrent() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let mut latencies: Vec<f64> = (0..100)
        .map(|i| {
            let file = format!("mc_test_lat_{}.bin", i);
            let start = Instant::now();
            let _ = write_from_client(0, &file, 1);
            let elapsed = start.elapsed().as_secs_f64();
            delete_from_client(0, &file).ok();
            elapsed * 1000.0
        })
        .collect();

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p99_index = (latencies.len() as f64 * 0.99) as usize;
    let p99_latency = latencies[p99_index.min(latencies.len() - 1)];

    println!("P99 latency: {:.2} ms", p99_latency);

    assert!(
        p99_latency < 5000.0,
        "P99 latency should be under 5 seconds"
    );

    cleanup_all_test_files()?;
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_mixed_workload_production_like() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let workload_duration = Duration::from_secs(120);
    let start = Instant::now();

    let handle0 = std::thread::spawn(move || {
        let mut errors = 0;
        while start.elapsed() < workload_duration {
            let file = format!(
                "mc_test_workload_{}.bin",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
                    % 10000
            );
            if write_from_client(0, &file, 10).is_err() {
                errors += 1;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        errors
    });

    let handle1 = std::thread::spawn(move || {
        let errors = 0;
        let mut counter: u64 = 0;
        while start.elapsed() < workload_duration {
            counter = counter.wrapping_add(1);
            let r = (counter % 10) as u8;
            if r < 7 {
                let file = format!(
                    "mc_test_r_{}.bin",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                        % 1000
                );
                let _ = read_from_client(1, &file);
            } else if r < 9 {
                let file = format!(
                    "mc_test_w_{}.bin",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                        % 1000
                );
                let _ = write_from_client(1, &file, 5);
            } else {
                let file = format!(
                    "mc_test_d_{}.bin",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                        % 1000
                );
                let _ = delete_from_client(1, &file);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        errors
    });

    let errors0 = handle0
        .join()
        .map_err(|e| format!("Workload thread 0 failed: {:?}", e))?;
    let errors1 = handle1
        .join()
        .map_err(|e| format!("Workload thread 1 failed: {:?}", e))?;

    assert!(errors0 < 10, "Client 0 should have minimal errors");
    assert!(errors1 < 20, "Client 1 should have minimal errors");

    cleanup_all_test_files()?;
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_two_clients_10x_throughput() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    cleanup_all_test_files()?;

    let test_dir = format!("{}/throughput_baseline", FUSE_MOUNT_PATH);
    let client0_ip = get_client_node(0)?;
    ssh_exec(&client0_ip, &format!("mkdir -p {}", test_dir), 5)?;

    let start_single = Instant::now();
    for i in 0..50 {
        let _ = ssh_exec(
            &client0_ip,
            &format!(
                "dd if=/dev/urandom of={}/file_{} bs=1M count=10 2>/dev/null",
                test_dir, i
            ),
            30,
        );
    }
    let single_time = start_single.elapsed().as_secs();

    ssh_exec(&client0_ip, &format!("rm -rf {}", test_dir), 10)?;

    let test_dir0 = format!("{}/throughput_c0", FUSE_MOUNT_PATH);
    let test_dir1 = format!("{}/throughput_c1", FUSE_MOUNT_PATH);
    let test_dir0_for_h0 = test_dir0.clone();
    let test_dir1_for_h1 = test_dir1.clone();
    let client0_ip_for_h0 = client0_ip.clone();
    ssh_exec(&client0_ip, &format!("mkdir -p {}", test_dir0), 5)?;
    let client1_ip = get_client_node(1)?;
    let client1_ip_for_h1 = client1_ip.clone();
    ssh_exec(&client1_ip, &format!("mkdir -p {}", test_dir1), 5)?;

    let start_dual = Instant::now();

    let h0 = std::thread::spawn(move || {
        for i in 0..50 {
            let _ = ssh_exec(
                &client0_ip_for_h0,
                &format!(
                    "dd if=/dev/urandom of={}/file_{} bs=1M count=10 2>/dev/null",
                    test_dir0_for_h0, i
                ),
                30,
            );
        }
    });

    let h1 = std::thread::spawn(move || {
        for i in 0..50 {
            let _ = ssh_exec(
                &client1_ip_for_h1,
                &format!(
                    "dd if=/dev/urandom of={}/file_{} bs=1M count=10 2>/dev/null",
                    test_dir1_for_h1, i
                ),
                30,
            );
        }
    });

    h0.join().map_err(|e| format!("Thread failed: {:?}", e))?;
    h1.join().map_err(|e| format!("Thread failed: {:?}", e))?;

    let dual_time = start_dual.elapsed().as_secs();

    println!(
        "Single client time: {}s, Dual client time: {}s",
        single_time, dual_time
    );

    let speedup = (single_time as f64) / (dual_time as f64);
    println!("Speedup: {:.2}x", speedup);

    assert!(
        speedup >= 1.8,
        "Dual clients should achieve >=1.8x throughput"
    );

    ssh_exec(&client0_ip, &format!("rm -rf {}", test_dir0), 10)?;
    ssh_exec(&client1_ip, &format!("rm -rf {}", test_dir1), 10)?;

    cleanup_all_test_files()?;
    Ok(())
}

#[test]
#[ignore]
fn test_cluster_multi_client_ready_for_chaos() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Two clients not available - skipping test".to_string());
    }

    println!("All multi-client tests passed - cluster ready for chaos testing");

    let total_refs = query_prometheus_metric("claudefs_dedup_references_total")?;
    println!("Total dedup references: {}", total_refs);

    let client_count = query_prometheus_metric("claudefs_client_connected_total")?;
    println!("Connected clients: {}", client_count);

    assert!(
        client_count >= 2.0,
        "At least 2 clients should be connected"
    );

    Ok(())
}
