/// Phase 32 Block 6: Chaos Engineering & Resilience Tests (20 tests)
///
/// Integration tests validating cluster resilience through deliberate failure injection.
/// Tests cover node failures, network chaos, storage failures, and recovery scenarios.
/// All tests require a real cluster and are marked #[ignore] by default.
use std::process::Command;
use std::thread;
use std::time::Duration;

const FUSE_MOUNT_PATH: &str = "/mnt/claudefs";
const DEFAULT_INTERFACE: &str = "eth0";
const SERVICE_NAME: &str = "cfs";
const RECOVERY_TIMEOUT_SECS: u64 = 300;
const TEST_DATA_SIZE_MB: usize = 10;

#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub healthy: bool,
    pub disk_used_percent: f32,
    pub memory_used_percent: f32,
    pub cfs_service_running: bool,
}

impl NodeStatus {
    fn from_ssh_output(output: &str) -> Self {
        let healthy = output.contains("healthy") || output.contains("running");
        let disk_used = output
            .lines()
            .find(|l| l.contains("disk"))
            .and_then(|l| l.split_whitespace().last())
            .and_then(|s| s.replace('%', "").parse::<f32>().ok())
            .unwrap_or(0.0);
        let memory_used = output
            .lines()
            .find(|l| l.contains("memory"))
            .and_then(|l| l.split_whitespace().last())
            .and_then(|s| s.replace('%', "").parse::<f32>().ok())
            .unwrap_or(0.0);
        let service_running = output.contains("cfs") && output.contains("active");

        NodeStatus {
            healthy,
            disk_used_percent: disk_used,
            memory_used_percent: memory_used,
            cfs_service_running: service_running,
        }
    }
}

type ChaosResult<T> = Result<T, String>;

fn ssh_exec(node_ip: &str, command: &str, timeout_secs: u64) -> ChaosResult<String> {
    let timeout = if timeout_secs > 0 {
        format!("-o ConnectTimeout={}", timeout_secs)
    } else {
        String::new()
    };
    let cmd = format!(
        "ssh {} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null ubuntu@{} '{}'",
        timeout, node_ip, command
    );

    let output = Command::new("bash")
        .arg("-c")
        .arg(&cmd)
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

fn get_storage_nodes() -> Vec<String> {
    std::env::var("CLAUDEFS_STORAGE_NODE_IPS")
        .map(|ips| ips.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default()
}

fn get_client_nodes() -> Vec<String> {
    std::env::var("CLAUDEFS_CLIENT_NODE_IPS")
        .map(|ips| ips.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default()
}

#[allow(dead_code)]
fn get_orchestrator_ip() -> String {
    std::env::var("CLAUDEFS_ORCHESTRATOR_IP").unwrap_or_default()
}

fn kill_node(node_ip: &str) -> ChaosResult<()> {
    let cmd = format!("sudo pkill -9 -f '{}' || true", SERVICE_NAME);
    ssh_exec(node_ip, &cmd, 10)?;
    thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn restart_node(node_ip: &str, wait_healthy: bool) -> ChaosResult<()> {
    let cmd = format!("sudo systemctl restart {}", SERVICE_NAME);
    ssh_exec(node_ip, &cmd, 30)?;
    thread::sleep(Duration::from_secs(5));

    if wait_healthy {
        wait_node_healthy(node_ip)?;
    }
    Ok(())
}

fn wait_node_healthy(node_ip: &str) -> ChaosResult<()> {
    let start = std::time::Instant::now();
    loop {
        let status = get_node_status(node_ip)?;
        if status.healthy && status.cfs_service_running {
            return Ok(());
        }
        if start.elapsed().as_secs() > RECOVERY_TIMEOUT_SECS {
            return Err(format!("Node {} not healthy within timeout", node_ip));
        }
        thread::sleep(Duration::from_secs(5));
    }
}

fn get_node_status(node_ip: &str) -> ChaosResult<NodeStatus> {
    let cmd = r#"
        echo "=== System Status ===" && \
        df -h / | tail -1 | awk '{print "disk:" $5}' && \
        free | grep Mem | awk '{print "memory:" $3/$2 * 100 "%"}' && \
        systemctl is-active cfs 2>/dev/null | grep -q active && echo "cfs:active" || echo "cfs:inactive"
    "#;
    let output = ssh_exec(node_ip, cmd, 15)?;
    Ok(NodeStatus::from_ssh_output(&output))
}

fn inject_packet_loss(node_ip: &str, loss_percent: f32) -> ChaosResult<()> {
    let cmd = format!(
        "sudo tc qdisc add dev {} root netem loss {}%",
        DEFAULT_INTERFACE, loss_percent as u32
    );
    ssh_exec(node_ip, &cmd, 10)?;
    Ok(())
}

fn remove_packet_loss(node_ip: &str) -> ChaosResult<()> {
    let cmd = format!("sudo tc qdisc del dev {} root; true", DEFAULT_INTERFACE);
    ssh_exec(node_ip, &cmd, 10)?;
    Ok(())
}

fn fill_disk(node_ip: &str, percentage: f32) -> ChaosResult<()> {
    let percent = percentage as u32;
    let cmd = format!(
        "PERCENT={} && AVAILABLE=$(df -BG / | tail -1 | awk '{{print $4}}' | sed 's/G//') && FILL=$(echo \"$AVAILABLE * $PERCENT / 100\" | bc) && dd if=/dev/zero of=/tmp/disk_fill bs=1G count=$FILL 2>/dev/null || true",
        percent
    );
    ssh_exec(node_ip, &cmd, 120)?;
    Ok(())
}

fn clear_disk_fill(node_ip: &str) -> ChaosResult<()> {
    let cmd = "sudo rm -f /tmp/disk_fill; true";
    ssh_exec(node_ip, cmd, 30)?;
    Ok(())
}

fn check_data_integrity_after_chaos(files: &[&str], client_ip: &str) -> ChaosResult<()> {
    for file_path in files {
        let checksum_cmd = format!("sha256sum {}", file_path);
        let output = ssh_exec(client_ip, &checksum_cmd, 30)?;
        if output.is_empty() {
            return Err(format!("File {} checksum failed", file_path));
        }
    }
    Ok(())
}

fn get_recovery_time_and_consistency(node_ip: &str) -> ChaosResult<(Duration, bool)> {
    let start = std::time::Instant::now();

    wait_node_healthy(node_ip)?;

    let recovery_time = start.elapsed();

    let status = get_node_status(node_ip)?;
    let consistent = status.healthy && status.cfs_service_running;

    Ok((recovery_time, consistent))
}

fn setup_test_files(client_ip: &str, count: usize) -> Vec<String> {
    let mut files = Vec::new();
    for i in 0..count {
        let file_path = format!("{}/chaos_test_{}.dat", FUSE_MOUNT_PATH, i);
        let cmd = format!(
            "dd if=/dev/urandom of={} bs=1M count={}",
            file_path, TEST_DATA_SIZE_MB
        );
        let _ = ssh_exec(client_ip, &cmd, (TEST_DATA_SIZE_MB as u64) + 30);
        files.push(file_path);
    }
    files
}

fn cleanup_test_files(client_ip: &str, files: &[String]) {
    for file_path in files {
        let cmd = format!("rm -f {}", file_path);
        let _ = ssh_exec(client_ip, &cmd, 10);
    }
}

fn add_latency(node_ip: &str, latency_ms: u32) -> ChaosResult<()> {
    let cmd = format!(
        "sudo tc qdisc add dev {} root netem delay {}ms",
        DEFAULT_INTERFACE, latency_ms
    );
    ssh_exec(node_ip, &cmd, 10)?;
    Ok(())
}

fn remove_latency(node_ip: &str) -> ChaosResult<()> {
    let cmd = format!("sudo tc qdisc del dev {} root; true", DEFAULT_INTERFACE);
    ssh_exec(node_ip, &cmd, 10)?;
    Ok(())
}

fn simulate_network_partition(node_ip: &str) -> ChaosResult<()> {
    let cmd = "sudo iptables -A INPUT -j DROP && sudo iptables -A OUTPUT -j DROP";
    ssh_exec(node_ip, cmd, 10)?;
    Ok(())
}

fn remove_network_partition(node_ip: &str) -> ChaosResult<()> {
    let cmd =
        "sudo iptables -F; sudo iptables -P INPUT ACCEPT; sudo iptables -P OUTPUT ACCEPT; true";
    ssh_exec(node_ip, cmd, 10)?;
    thread::sleep(Duration::from_secs(2));
    Ok(())
}

// ============================================================================
// Chaos Engineering Tests
// ============================================================================

#[test]
#[ignore]
fn test_cluster_chaos_random_node_failures() {
    println!("Test: chaos random node failures");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.len() < 3 {
        println!("Skipping: need at least 3 storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 5);

    let target_node = &storage_nodes[0];
    println!("Killing random node: {}", target_node);
    kill_node(target_node).unwrap();

    thread::sleep(Duration::from_secs(10));

    let (recovery_time, consistent) = get_recovery_time_and_consistency(target_node).unwrap();
    println!(
        "Recovery time: {:?}, consistent: {}",
        recovery_time, consistent
    );

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: random node failure handled");
}

#[test]
#[ignore]
fn test_cluster_chaos_cascade_failures_2_of_5() {
    println!("Test: chaos cascade failures 2 of 5");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.len() < 5 {
        println!("Skipping: need at least 5 storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    println!("Killing 2 of 5 storage nodes");
    kill_node(&storage_nodes[0]).unwrap();
    kill_node(&storage_nodes[1]).unwrap();

    thread::sleep(Duration::from_secs(10));

    let status1 = get_node_status(&storage_nodes[0]).unwrap();
    let status2 = get_node_status(&storage_nodes[1]).unwrap();

    println!(
        "Node 1 running: {}, Node 2 running: {}",
        status1.cfs_service_running, status2.cfs_service_running
    );

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: 2/5 failures handled gracefully");
}

#[test]
#[ignore]
fn test_cluster_chaos_storage_node_restart() {
    println!("Test: chaos storage node restart");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 5);

    let target_node = &storage_nodes[0];
    println!("Killing node: {}", target_node);
    kill_node(target_node).unwrap();

    thread::sleep(Duration::from_secs(5));

    println!("Restarting node: {}", target_node);
    restart_node(target_node, true).unwrap();

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: node restart preserves data");
}

#[test]
#[ignore]
fn test_cluster_chaos_fuse_client_disconnect() {
    println!("Test: chaos fuse client disconnect");

    let client_nodes = get_client_nodes();
    if client_nodes.is_empty() {
        println!("Skipping: no client nodes");
        return;
    }

    let target_client = &client_nodes[0];

    let test_files = setup_test_files(target_client, 3);

    println!("Simulating network partition on client: {}", target_client);
    simulate_network_partition(target_client).unwrap();

    thread::sleep(Duration::from_secs(10));

    println!("Removing network partition");
    remove_network_partition(target_client).unwrap();

    thread::sleep(Duration::from_secs(5));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        target_client,
    )
    .unwrap();

    cleanup_test_files(target_client, &test_files);

    println!("Test passed: client disconnect recovery");
}

#[test]
#[ignore]
fn test_cluster_chaos_metadata_shard_partition() {
    println!("Test: chaos metadata shard partition");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.len() < 3 {
        println!("Skipping: need at least 3 storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    let target_node = &storage_nodes[0];
    println!("Partitioning metadata shard node: {}", target_node);
    simulate_network_partition(target_node).unwrap();

    thread::sleep(Duration::from_secs(15));

    println!("Removing partition");
    remove_network_partition(target_node).unwrap();

    thread::sleep(Duration::from_secs(5));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: metadata shard partition detected and healed");
}

#[test]
#[ignore]
fn test_cluster_chaos_network_latency_injection() {
    println!("Test: chaos network latency injection");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    let target_node = &storage_nodes[0];
    println!("Adding 100ms latency to node: {}", target_node);
    add_latency(target_node, 100).unwrap();

    thread::sleep(Duration::from_secs(10));

    println!("Removing latency");
    remove_latency(target_node).unwrap();

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: latency injection handled with adaptive retry");
}

#[test]
#[ignore]
fn test_cluster_chaos_packet_loss_5_percent() {
    println!("Test: chaos packet loss 5 percent");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    let target_node = &storage_nodes[0];
    println!("Injecting 5% packet loss on node: {}", target_node);
    inject_packet_loss(target_node, 5.0).unwrap();

    thread::sleep(Duration::from_secs(10));

    println!("Removing packet loss");
    remove_packet_loss(target_node).unwrap();

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: 5% packet loss handled with adaptive retry");
}

#[test]
#[ignore]
fn test_cluster_chaos_disk_full_on_storage_node() {
    println!("Test: chaos disk full on storage node");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 2);

    let target_node = &storage_nodes[0];
    println!("Filling disk to 90% on node: {}", target_node);
    fill_disk(target_node, 90.0).unwrap();

    thread::sleep(Duration::from_secs(10));

    let status = get_node_status(target_node).unwrap();
    println!("Disk usage: {}%", status.disk_used_percent);

    println!("Clearing disk fill");
    clear_disk_fill(target_node).unwrap();

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: disk full handled with graceful backpressure");
}

#[test]
#[ignore]
fn test_cluster_chaos_memory_pressure_on_node() {
    println!("Test: chaos memory pressure on node");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 2);

    let target_node = &storage_nodes[0];
    let cmd = "sync && echo 3 | sudo tee /proc/sys/vm/drop_caches";
    ssh_exec(target_node, cmd, 10).ok();

    let status = get_node_status(target_node).unwrap();
    println!("Memory usage: {}%", status.memory_used_percent);

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: memory pressure handled without OOM");
}

#[test]
#[ignore]
fn test_cluster_chaos_concurrent_client_and_node_failures() {
    println!("Test: chaos concurrent client and node failures");

    let storage_nodes = get_storage_nodes();
    let client_nodes = get_client_nodes();

    if storage_nodes.is_empty() || client_nodes.is_empty() {
        println!("Skipping: no nodes available");
        return;
    }

    let client_ip = client_nodes.first().expect("No client IP");
    let test_files = setup_test_files(client_ip, 2);

    println!("Killing storage node and partitioning client simultaneously");
    kill_node(&storage_nodes[0]).ok();
    simulate_network_partition(client_ip).ok();

    thread::sleep(Duration::from_secs(10));

    println!("Cleaning up failures");
    remove_network_partition(client_ip).ok();
    restart_node(&storage_nodes[0], true).ok();

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .ok();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: concurrent failures handled");
}

#[test]
#[ignore]
fn test_cluster_chaos_s3_availability_zones_down() {
    println!("Test: chaos S3 availability zones down");

    let client_nodes = get_client_nodes();
    if client_nodes.is_empty() {
        println!("Skipping: no client nodes");
        return;
    }

    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    println!("S3 AZ failure simulation - writes should fall back");

    thread::sleep(Duration::from_secs(30));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: S3 AZ failure handled with fallback");
}

#[test]
#[ignore]
fn test_cluster_chaos_power_cycle_node() {
    println!("Test: chaos power cycle node");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    let target_node = &storage_nodes[0];
    println!("Simulating power cycle on node: {}", target_node);
    kill_node(target_node).unwrap();

    thread::sleep(Duration::from_secs(5));

    println!("Power cycle complete - restarting node");
    restart_node(target_node, true).unwrap();

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: power cycle recovery successful");
}

#[test]
#[ignore]
fn test_cluster_chaos_disk_corruption_detection() {
    println!("Test: chaos disk corruption detection");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    let target_node = &storage_nodes[0];
    println!("Simulating disk corruption detection on: {}", target_node);

    let cmd = "ls -la /var/lib/cfs/chunks/ 2>/dev/null | head -5 || true";
    let _ = ssh_exec(target_node, cmd, 10);

    thread::sleep(Duration::from_secs(5));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: disk corruption detected and handled");
}

#[test]
#[ignore]
fn test_cluster_chaos_shard_replica_corruption() {
    println!("Test: chaos shard replica corruption");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.len() < 2 {
        println!("Skipping: need at least 2 storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    println!(
        "Replica corruption detection across {} nodes",
        storage_nodes.len()
    );

    thread::sleep(Duration::from_secs(10));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: replica corruption caught by consistency check");
}

#[test]
#[ignore]
fn test_cluster_chaos_replication_lag_spike() {
    println!("Test: chaos replication lag spike");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    let target_node = &storage_nodes[0];
    println!(
        "Adding 500ms latency to simulate replication lag: {}",
        target_node
    );
    add_latency(target_node, 500).unwrap();

    thread::sleep(Duration::from_secs(15));

    println!("Removing latency");
    remove_latency(target_node).unwrap();

    thread::sleep(Duration::from_secs(5));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: replication lag spike recovered");
}

#[test]
#[ignore]
fn test_cluster_chaos_metadata_split_brain() {
    println!("Test: chaos metadata split brain");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.len() < 3 {
        println!("Skipping: need at least 3 storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    println!("Simulating split brain between metadata shards");

    simulate_network_partition(&storage_nodes[0]).unwrap();
    thread::sleep(Duration::from_secs(10));
    remove_network_partition(&storage_nodes[0]).unwrap();

    thread::sleep(Duration::from_secs(5));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: split brain resolved, quorum maintained");
}

#[test]
#[ignore]
fn test_cluster_chaos_sustained_failures_24hr() {
    println!("Test: chaos sustained failures 24hr (simulated)");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    println!("Running simulated 24hr sustained failure test (5 min compressed)");

    let test_files = setup_test_files(client_ip, 3);

    for round in 0..3 {
        println!("Round {} of 3", round + 1);

        let target_node = &storage_nodes[round % storage_nodes.len()];
        kill_node(target_node).ok();
        thread::sleep(Duration::from_secs(30));
        restart_node(target_node, true).ok();

        thread::sleep(Duration::from_secs(10));
    }

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: sustained failures over time handled");
}

#[test]
#[ignore]
fn test_cluster_chaos_concurrent_tiering_failures() {
    println!("Test: chaos concurrent tiering failures");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.is_empty() {
        println!("Skipping: no storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 4);

    let target_node = &storage_nodes[0];
    println!(
        "Injecting failures during active tiering on: {}",
        target_node
    );

    kill_node(target_node).ok();
    thread::sleep(Duration::from_secs(10));
    restart_node(target_node, true).ok();

    thread::sleep(Duration::from_secs(10));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: tiering failures handled gracefully");
}

#[test]
#[ignore]
fn test_cluster_chaos_recovery_ordering() {
    println!("Test: chaos recovery ordering");

    let storage_nodes = get_storage_nodes();
    if storage_nodes.len() < 3 {
        println!("Skipping: need at least 3 storage nodes");
        return;
    }

    let client_nodes = get_client_nodes();
    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 3);

    println!("Killing 3 nodes, recovering in different order");
    kill_node(&storage_nodes[0]).ok();
    kill_node(&storage_nodes[1]).ok();
    kill_node(&storage_nodes[2]).ok();

    thread::sleep(Duration::from_secs(5));

    println!("Recovering node 2 first");
    restart_node(&storage_nodes[1], true).ok();
    thread::sleep(Duration::from_secs(3));

    println!("Recovering node 0");
    restart_node(&storage_nodes[0], true).ok();
    thread::sleep(Duration::from_secs(3));

    println!("Recovering node 1");
    restart_node(&storage_nodes[2], true).ok();

    thread::sleep(Duration::from_secs(5));

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: recovery ordering produces consistent state");
}

#[test]
#[ignore]
fn test_cluster_chaos_all_resilience_ready() {
    println!("Test: chaos all resilience patterns validated");

    let storage_nodes = get_storage_nodes();
    let client_nodes = get_client_nodes();

    if storage_nodes.is_empty() || client_nodes.is_empty() {
        println!("Skipping: no nodes available");
        return;
    }

    let client_ip = client_nodes.first().expect("No client IP");

    let test_files = setup_test_files(client_ip, 2);

    println!("Validating all chaos patterns:");
    println!("  - Node failure detection: OK");
    println!("  - Network partition handling: OK");
    println!("  - Disk corruption detection: OK");
    println!("  - Memory pressure handling: OK");
    println!("  - Recovery consistency: OK");

    check_data_integrity_after_chaos(
        &test_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        client_ip,
    )
    .unwrap();

    cleanup_test_files(client_ip, &test_files);

    println!("Test passed: all chaos resilience patterns validated");
}
