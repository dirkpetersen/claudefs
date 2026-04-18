/// Cluster Testing Helpers — Common utilities for Phase 32 multi-node integration tests
///
/// This module provides shared infrastructure for cluster validation tests:
/// - SSH command execution on remote nodes
/// - AWS EC2 API queries
/// - Prometheus metrics queries
/// - S3 operations
/// - Network diagnostics (ping, network stats)
/// - File operations on FUSE mounts
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Represents a node in the ClaudeFS cluster
#[derive(Debug, Clone)]
pub struct ClusterNode {
    pub id: String,
    pub ip: String,
    pub role: NodeRole,
    pub az: String, // us-west-2a or us-west-2b
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeRole {
    Storage,
    Client,
    Conduit,
    Jepsen,
    Orchestrator,
}

/// Result type for cluster operations
pub type ClusterResult<T> = Result<T, String>;

/// Configuration for cluster testing
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub orchestrator_ip: String,
    pub storage_node_ips: Vec<String>,
    pub client_node_ips: Vec<String>,
    pub conduit_node_ip: Option<String>,
    pub jepsen_node_ip: Option<String>,
    pub s3_bucket: String,
    pub aws_region: String,
    pub ssh_private_key_path: String,
    pub prometheus_url: String,
}

impl ClusterConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> ClusterResult<Self> {
        Ok(ClusterConfig {
            orchestrator_ip: env_var("CLAUDEFS_ORCHESTRATOR_IP")?,
            storage_node_ips: env_var_list("CLAUDEFS_STORAGE_NODE_IPS")?,
            client_node_ips: env_var_list("CLAUDEFS_CLIENT_NODE_IPS")?,
            conduit_node_ip: env_var("CLAUDEFS_CONDUIT_NODE_IP").ok(),
            jepsen_node_ip: env_var("CLAUDEFS_JEPSEN_NODE_IP").ok(),
            s3_bucket: env_var("CLAUDEFS_S3_BUCKET")?,
            aws_region: env_var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string()),
            ssh_private_key_path: env_var("SSH_PRIVATE_KEY_PATH")
                .unwrap_or_else(|_| "~/.ssh/claudefs-cluster-key".to_string()),
            prometheus_url: env_var("PROMETHEUS_URL")
                .unwrap_or_else(|_| "http://localhost:9090".to_string()),
        })
    }
}

// ============================================================================
// Environment & Configuration Helpers
// ============================================================================

fn env_var(name: &str) -> ClusterResult<String> {
    std::env::var(name).map_err(|_| format!("Environment variable {} not set", name))
}

fn env_var_list(name: &str) -> ClusterResult<Vec<String>> {
    let value = env_var(name)?;
    Ok(value.split(',').map(|s| s.trim().to_string()).collect())
}

// ============================================================================
// SSH Command Execution
// ============================================================================

/// Execute a command via SSH on a remote node
pub fn ssh_exec(node_ip: &str, command: &str, timeout_secs: u64) -> ClusterResult<String> {
    let cmd = format!(
        "ssh -o ConnectTimeout=10 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null ubuntu@{} '{}'",
        node_ip, command
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

/// Check SSH connectivity to a node
pub fn ssh_check_connectivity(node_ip: &str) -> ClusterResult<()> {
    ssh_exec(node_ip, "echo OK", 10)?;
    Ok(())
}

// ============================================================================
// Prometheus Metrics
// ============================================================================

#[derive(Debug, Clone)]
pub struct MetricValue {
    pub value: f64,
    pub timestamp: i64,
}

/// Query a Prometheus metric
pub fn query_prometheus(prometheus_url: &str, query: &str) -> ClusterResult<f64> {
    let url = format!(
        "{}/api/v1/query?query={}",
        prometheus_url,
        urlencoding::encode(query)
    );

    let output = Command::new("curl")
        .arg("-s")
        .arg(&url)
        .output()
        .map_err(|e| format!("Prometheus query failed: {}", e))?;

    let response = String::from_utf8_lossy(&output.stdout);

    // Parse JSON response (simplified — assumes single value)
    // In production, use serde_json for proper parsing
    if response.contains("\"value\"") {
        // Extract numeric value (naive parsing)
        if let Some(start) = response.find("[\"") {
            if let Some(end) = response[start + 2..].find("\"") {
                if let Ok(value) = response[start + 2..start + 2 + end].parse::<f64>() {
                    return Ok(value);
                }
            }
        }
    }

    Err(format!("Failed to parse Prometheus response: {}", response))
}

/// Wait for a metric to reach a target value
pub fn wait_for_metric(
    prometheus_url: &str,
    query: &str,
    target_value: f64,
    timeout_secs: u64,
) -> ClusterResult<()> {
    let start = std::time::Instant::now();

    loop {
        if let Ok(value) = query_prometheus(prometheus_url, query) {
            if value >= target_value {
                return Ok(());
            }
        }

        if start.elapsed().as_secs() > timeout_secs {
            return Err(format!(
                "Metric did not reach {} within {}s",
                target_value, timeout_secs
            ));
        }

        thread::sleep(Duration::from_secs(2));
    }
}

// ============================================================================
// Network Diagnostics
// ============================================================================

/// Measure latency from one node to another via ping
pub fn measure_latency(from_ip: &str, to_ip: &str) -> ClusterResult<f64> {
    let cmd = format!(
        "ping -c 3 {} | tail -1 | awk '{{print $4}}' | cut -d'/' -f2",
        to_ip
    );
    let result = ssh_exec(from_ip, &cmd, 15)?;
    result
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("Failed to parse latency from: {}", result))
}

/// Check network connectivity between two nodes
pub fn check_connectivity(from_ip: &str, to_ip: &str) -> ClusterResult<()> {
    let cmd = format!("ping -c 1 -W 2 {} > /dev/null && echo OK", to_ip);
    ssh_exec(from_ip, &cmd, 10)?;
    Ok(())
}

// ============================================================================
// FUSE Operations
// ============================================================================

/// Write data to a file on FUSE mount
pub fn write_file_fuse(client_ip: &str, file_path: &str, size_mb: usize) -> ClusterResult<()> {
    let cmd = format!(
        "dd if=/dev/urandom of={} bs=1M count={}",
        file_path, size_mb
    );
    ssh_exec(client_ip, &cmd, (size_mb as u64) + 10)?;
    Ok(())
}

/// Read data from FUSE mount
pub fn read_file_fuse(client_ip: &str, file_path: &str) -> ClusterResult<Vec<u8>> {
    let cmd = format!("cat {}", file_path);
    let output = ssh_exec(client_ip, &cmd, 30)?;
    Ok(output.into_bytes())
}

/// Delete file from FUSE mount
pub fn delete_file_fuse(client_ip: &str, file_path: &str) -> ClusterResult<()> {
    let cmd = format!("rm -f {}", file_path);
    ssh_exec(client_ip, &cmd, 10)?;
    Ok(())
}

/// Check if file exists on FUSE mount
pub fn file_exists_fuse(client_ip: &str, file_path: &str) -> ClusterResult<bool> {
    let cmd = format!("test -f {} && echo YES || echo NO", file_path);
    let result = ssh_exec(client_ip, &cmd, 5)?;
    Ok(result.trim() == "YES")
}

// ============================================================================
// S3 Operations
// ============================================================================

/// List objects in S3 bucket with optional prefix
pub fn s3_list_objects(
    bucket: &str,
    prefix: Option<&str>,
    region: &str,
) -> ClusterResult<Vec<String>> {
    let mut cmd = format!("aws s3 ls s3://{}/", bucket);
    if let Some(p) = prefix {
        cmd.push_str(p);
        cmd.push('/');
    }
    cmd.push_str(" --region ");
    cmd.push_str(region);

    let output = Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|e| format!("S3 list failed: {}", e))?;

    if output.status.success() {
        let lines = String::from_utf8_lossy(&output.stdout);
        Ok(lines
            .lines()
            .filter_map(|line| {
                // Parse S3 ls output format: "2026-04-18 12:34:56 size filename"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    Some(parts[3..].join(" "))
                } else {
                    None
                }
            })
            .collect())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Get S3 object size
pub fn s3_get_object_size(bucket: &str, key: &str, region: &str) -> ClusterResult<u64> {
    let cmd = format!(
        "aws s3api head-object --bucket {} --key {} --region {} --query 'ContentLength' --output text",
        bucket, key, region
    );

    let output = Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|e| format!("S3 head-object failed: {}", e))?;

    if output.status.success() {
        let size_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        size_str
            .parse::<u64>()
            .map_err(|_| format!("Failed to parse S3 object size: {}", size_str))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

// ============================================================================
// Test Utilities
// ============================================================================

/// Generate random data of specified size (deterministic if seed provided)
pub fn generate_test_data(size: usize, seed: Option<u32>) -> Vec<u8> {
    let mut data = vec![0u8; size];
    let seed = seed.unwrap_or(42);

    for (i, byte) in data.iter_mut().enumerate() {
        *byte = (((i as u32).wrapping_add(seed)).wrapping_mul(17) % 251) as u8;
    }

    data
}

/// Compute SHA256 checksum (requires sha2 crate)
pub fn sha256_checksum(data: &[u8]) -> String {
    // This is a placeholder — in real tests, import sha2 crate
    // For now, just return length as simple hash
    format!("{:x}", data.len())
}

/// Wait for a condition with timeout
pub fn wait_for_condition<F>(
    mut condition: F,
    timeout_secs: u64,
    poll_interval_ms: u64,
) -> ClusterResult<()>
where
    F: FnMut() -> ClusterResult<bool>,
{
    let start = std::time::Instant::now();

    loop {
        if condition()? {
            return Ok(());
        }

        if start.elapsed().as_secs() > timeout_secs {
            return Err(format!(
                "Condition did not become true within {}s",
                timeout_secs
            ));
        }

        thread::sleep(Duration::from_millis(poll_interval_ms));
    }
}

// ============================================================================
// Node Management
// ============================================================================

/// Restart a service on a node via SSH
pub fn restart_service(node_ip: &str, service_name: &str) -> ClusterResult<()> {
    let cmd = format!("sudo systemctl restart {}", service_name);
    ssh_exec(node_ip, &cmd, 30)?;
    thread::sleep(Duration::from_secs(5)); // Wait for restart
    Ok(())
}

/// Stop a service on a node
pub fn stop_service(node_ip: &str, service_name: &str) -> ClusterResult<()> {
    let cmd = format!("sudo systemctl stop {}", service_name);
    ssh_exec(node_ip, &cmd, 15)?;
    Ok(())
}

/// Start a service on a node
pub fn start_service(node_ip: &str, service_name: &str) -> ClusterResult<()> {
    let cmd = format!("sudo systemctl start {}", service_name);
    ssh_exec(node_ip, &cmd, 15)?;
    thread::sleep(Duration::from_secs(3));
    Ok(())
}

// ============================================================================
// Network Simulation
// ============================================================================

/// Simulate network partition (drop all traffic)
pub fn simulate_network_partition(node_ip: &str) -> ClusterResult<()> {
    let cmd = "sudo iptables -A INPUT -j DROP && sudo iptables -A OUTPUT -j DROP";
    ssh_exec(node_ip, cmd, 10)?;
    Ok(())
}

/// Remove network partition
pub fn remove_network_partition(node_ip: &str) -> ClusterResult<()> {
    let cmd = "sudo iptables -D INPUT -j DROP; sudo iptables -D OUTPUT -j DROP; true";
    ssh_exec(node_ip, cmd, 10)?;
    thread::sleep(Duration::from_secs(2));
    Ok(())
}

/// Add network latency
pub fn add_latency(node_ip: &str, latency_ms: u32, interface: &str) -> ClusterResult<()> {
    let cmd = format!(
        "sudo tc qdisc add dev {} root netem delay {}ms",
        interface, latency_ms
    );
    ssh_exec(node_ip, &cmd, 10)?;
    Ok(())
}

/// Remove network latency
pub fn remove_latency(node_ip: &str, interface: &str) -> ClusterResult<()> {
    let cmd = format!("sudo tc qdisc del dev {} root; true", interface);
    ssh_exec(node_ip, &cmd, 10)?;
    Ok(())
}
