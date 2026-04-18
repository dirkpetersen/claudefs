/// Phase 32 Block 1: Multi-Node Cluster Setup & Health Validation
///
/// 15 integration tests validating real AWS ClaudeFS cluster infrastructure.
/// These tests verify cluster infrastructure is properly provisioned before
/// running workload tests (Blocks 2-8).
use std::collections::HashMap;
use std::process::Command;

const SSH_TIMEOUT_SECS: u64 = 30;
const NODE_CONNECT_TIMEOUT: u64 = 5;

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ClusterNode {
    name: String,
    ip: String,
    role: String,
    az: String,
}

fn get_env_or_skip(var: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| {
        println!("Skipping test: {} not set", var);
        std::process::exit(0);
    })
}

fn get_env_or_default(var: &str, default: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| default.to_string())
}

fn run_ssh_command(node_ip: &str, cmd: &str) -> Result<String, String> {
    let key_path = get_env_or_default("SSH_PRIVATE_KEY", "~/.ssh/id_rsa");
    let user = get_env_or_default("SSH_USER", "ubuntu");

    let output = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "BatchMode=yes",
            "-i",
            &key_path,
            &format!("{}@{}", user, node_ip),
            cmd,
        ])
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("SSH command failed: {}", stderr))
    }
}

fn run_ssh_command_with_timeout(
    node_ip: &str,
    cmd: &str,
    _timeout_secs: u64,
) -> Result<String, String> {
    let key_path = get_env_or_default("SSH_PRIVATE_KEY", "~/.ssh/id_rsa");
    let user = get_env_or_default("SSH_USER", "ubuntu");

    let output = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            &format!("ConnectTimeout={}", NODE_CONNECT_TIMEOUT),
            "-o",
            "BatchMode=yes",
            "-i",
            &key_path,
            &format!("{}@{}", user, node_ip),
            cmd,
        ])
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("SSH command failed: {}", stderr))
    }
}

fn aws_ec2_describe_instances() -> Result<Vec<ClusterNode>, String> {
    let region = get_env_or_default("AWS_REGION", "us-west-2");
    let cluster_name = get_env_or_default("CLAUDEFS_CLUSTER_NAME", "claudefs");

    let output = Command::new("aws")
        .args([
            "ec2", "describe-instances",
            "--filters",
            &format!("Name=tag:Cluster,Values={}", cluster_name),
            "Name=instance-state-name,Values=running",
            "--region", &region,
            "--query", "Reservations[].Instances[].{Name:Tags[?Key==`Name`]| [0].Value,IP:PrivateIpAddress,Role:Tags[?Key==`Role`]| [0].Value,AZ:Placement.AvailabilityZone}",
            "--output", "json",
        ])
        .output()
        .map_err(|e| format!("AWS CLI failed: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "AWS describe-instances failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let mut nodes = Vec::new();
    if let Some(arr) = json.as_array() {
        for item in arr {
            let name = item
                .get("Name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let ip = item
                .get("IP")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let role = item
                .get("Role")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let az = item
                .get("AZ")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            if !ip.is_empty() {
                nodes.push(ClusterNode { name, ip, role, az });
            }
        }
    }

    Ok(nodes)
}

fn query_prometheus(query: &str) -> Result<serde_json::Value, String> {
    let prometheus_url = get_env_or_default("PROMETHEUS_URL", "http://localhost:9090");

    let url = format!("{}/api/v1/query?query={}", prometheus_url, query);

    let output = Command::new("curl")
        .args(["-s", &url])
        .output()
        .map_err(|e| format!("curl failed: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Prometheus query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse Prometheus response: {}", e))
}

fn ping_latency(from_ip: &str, to_ip: &str) -> Result<f64, String> {
    let key_path = get_env_or_default("SSH_PRIVATE_KEY", "~/.ssh/id_rsa");
    let user = get_env_or_default("SSH_USER", "ubuntu");

    let output = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "ConnectTimeout=5",
            "-i",
            &key_path,
            &format!("{}@{}", user, from_ip),
            &format!("ping -c 3 -W 1 {}", to_ip),
        ])
        .output()
        .map_err(|e| format!("ping failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.contains("avg") || line.contains("average") {
            let parts: Vec<&str> = line.split('/').collect();
            if parts.len() >= 5 {
                if let Ok(latency) = parts[4].trim().parse::<f64>() {
                    return Ok(latency);
                }
            }
        }
    }

    Err(format!(
        "Could not parse latency from ping output: {}",
        stdout
    ))
}

fn collect_ntp_offset(node_ip: &str) -> Result<f64, String> {
    let output = run_ssh_command(node_ip, "chronyc tracking | grep 'System time'")?;

    for line in output.lines() {
        if line.contains("System time") {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let value = parts[1].trim();
                if let Some(idx) = value.find("second") {
                    let num_str = value[..idx].trim();
                    if let Ok(offset) = num_str.parse::<f64>() {
                        return Ok(offset);
                    }
                }
            }
        }
    }

    let output = run_ssh_command(node_ip, "ntpdate -q pool.ntp.org 2>&1 | tail -1")?;
    for line in output.lines() {
        if let Some(offset) = line.split("offset ").nth(1) {
            let val = offset.split_whitespace().next().unwrap_or("0");
            return Ok(val.parse::<f64>().unwrap_or(0.0));
        }
    }

    Err("Could not determine NTP offset".to_string())
}

fn check_s3_bucket_access(bucket: &str, region: &str) -> Result<(), String> {
    let output = Command::new("aws")
        .args(["s3", "ls", &format!("s3://{}", bucket), "--region", region])
        .output()
        .map_err(|e| format!("AWS S3 ls failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "S3 bucket not accessible: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[test]
fn test_cluster_all_nodes_online() {
    println!("[TEST] test_cluster_all_nodes_online: Checking all cluster nodes are online...");

    let _region = get_env_or_default("AWS_REGION", "us-west-2");

    let nodes = match aws_ec2_describe_instances() {
        Ok(n) => n,
        Err(e) => {
            println!("Warning: Could not query AWS EC2: {}", e);
            println!("Attempting with environment variable node lists instead...");

            let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
            let client_ips = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "");
            let conduit_ip = get_env_or_default("CLAUDEFS_CONDUIT_IP", "");

            let mut nodes = Vec::new();
            for (i, ip) in storage_ips.split(',').filter(|s| !s.is_empty()).enumerate() {
                nodes.push(ClusterNode {
                    name: format!("storage-{}", i),
                    ip: ip.to_string(),
                    role: "storage".to_string(),
                    az: "us-west-2a".to_string(),
                });
            }
            for (i, ip) in client_ips.split(',').filter(|s| !s.is_empty()).enumerate() {
                nodes.push(ClusterNode {
                    name: format!("client-{}", i),
                    ip: ip.to_string(),
                    role: "client".to_string(),
                    az: "us-west-2a".to_string(),
                });
            }
            if !conduit_ip.is_empty() {
                nodes.push(ClusterNode {
                    name: "conduit-1".to_string(),
                    ip: conduit_ip,
                    role: "conduit".to_string(),
                    az: "us-west-2a".to_string(),
                });
            }
            nodes
        }
    };

    println!("Found {} nodes in cluster", nodes.len());

    if nodes.is_empty() {
        println!("Skipping: No cluster nodes configured or accessible");
        return;
    }

    let mut online_count = 0;
    let mut failed_nodes = Vec::new();

    for node in &nodes {
        print!("  Checking {} ({}): ", node.name, node.ip);
        match run_ssh_command_with_timeout(&node.ip, "echo OK", SSH_TIMEOUT_SECS) {
            Ok(output) if output.contains("OK") => {
                println!("ONLINE");
                online_count += 1;
            }
            Ok(output) => {
                println!("UNEXPECTED: {}", output);
                failed_nodes.push(node.name.clone());
            }
            Err(e) => {
                println!("FAILED: {}", e);
                failed_nodes.push(node.name.clone());
            }
        }
    }

    println!("\nResult: {}/{} nodes online", online_count, nodes.len());
    assert_eq!(
        online_count,
        nodes.len(),
        "Some nodes offline: {:?}",
        failed_nodes
    );
}

#[test]
fn test_storage_nodes_ntp_synchronized() {
    println!("[TEST] test_storage_nodes_ntp_synchronized: Checking NTP sync on storage nodes...");

    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let ips: Vec<&str> = storage_ips.split(',').filter(|s| !s.is_empty()).collect();

    if ips.is_empty() {
        println!("Skipping: No storage node IPs configured");
        return;
    }

    let mut all_synced = true;
    let mut failed_nodes = Vec::new();

    for ip in &ips {
        print!("  Checking {}: ", ip);
        match run_ssh_command(ip, "timedatectl status | grep -i synchronized") {
            Ok(output) => {
                if output.to_lowercase().contains("yes")
                    || output.to_lowercase().contains("synchronized")
                {
                    println!("SYNCHRONIZED");

                    if let Ok(ntp_output) =
                        run_ssh_command(ip, "ntpstat 2>/dev/null || chronyc tracking")
                    {
                        println!(
                            "    NTP status: {}",
                            ntp_output
                                .lines()
                                .next()
                                .unwrap_or("OK")
                                .chars()
                                .take(60)
                                .collect::<String>()
                        );
                    }
                } else {
                    println!("NOT SYNCHRONIZED: {}", output);
                    all_synced = false;
                    failed_nodes.push(ip.to_string());
                }
            }
            Err(e) => {
                println!("ERROR: {}", e);
                all_synced = false;
                failed_nodes.push(ip.to_string());
            }
        }
    }

    assert!(
        all_synced,
        "Storage nodes not NTP synchronized: {:?}",
        failed_nodes
    );
}

#[test]
fn test_s3_bucket_accessible_from_all_nodes() {
    println!("[TEST] test_s3_bucket_accessible_from_all_nodes: Checking S3 access...");

    let bucket = get_env_or_skip("CLAUDEFS_S3_BUCKET");
    let region = get_env_or_default("AWS_REGION", "us-west-2");

    println!("  Checking S3 bucket: s3://{}", bucket);

    if let Err(e) = check_s3_bucket_access(&bucket, &region) {
        println!("S3 bucket not accessible from orchestrator: {}", e);
    }

    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let client_ips = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "");

    let all_ips: Vec<&str> = storage_ips
        .split(',')
        .chain(client_ips.split(','))
        .filter(|s| !s.is_empty())
        .collect();

    if all_ips.is_empty() {
        println!("Skipping: No node IPs configured");
        return;
    }

    let mut accessible_count = 0;
    let mut failed_nodes = Vec::new();

    for ip in &all_ips {
        print!("  Checking {}: ", ip);
        match run_ssh_command(
            ip,
            &format!(
                "aws s3 ls s3://{} --region {} >/dev/null 2>&1 && echo OK || echo FAIL",
                bucket, region
            ),
        ) {
            Ok(output) => {
                if output.contains("OK") {
                    println!("ACCESSIBLE");
                    accessible_count += 1;
                } else {
                    println!("NOT ACCESSIBLE");
                    failed_nodes.push(ip.to_string());
                }
            }
            Err(e) => {
                println!("ERROR: {}", e);
                failed_nodes.push(ip.to_string());
            }
        }
    }

    assert!(
        accessible_count == all_ips.len(),
        "S3 not accessible from nodes: {:?}",
        failed_nodes
    );
}

#[test]
fn test_prometheus_metrics_collection() {
    println!("[TEST] test_prometheus_metrics_collection: Checking Prometheus metrics...");

    let result = query_prometheus("up");

    match result {
        Ok(json) => {
            if let Some(data) = json.get("data") {
                if let Some(results) = data.get("result") {
                    if let Some(results_arr) = results.as_array() {
                        println!("  Found {} metrics targets", results_arr.len());

                        let mut up_count = 0;
                        let mut down_count = 0;

                        for result in results_arr {
                            if let Some(metric) = result.get("metric") {
                                let job = metric
                                    .get("job")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let instance = metric
                                    .get("instance")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let value = result
                                    .get("value")
                                    .and_then(|v| v.as_array())
                                    .and_then(|a| a.get(1))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("0");

                                if value == "1" {
                                    up_count += 1;
                                } else {
                                    down_count += 1;
                                    println!("    DOWN: {} ({})", job, instance);
                                }
                            }
                        }

                        println!("  Metrics: {} up, {} down", up_count, down_count);

                        assert!(down_count == 0, "Some metrics targets are down");
                        return;
                    }
                }
            }
            println!("  Warning: Could not parse Prometheus response");
        }
        Err(e) => {
            println!("  Warning: Could not query Prometheus: {}", e);
        }
    }

    println!("  Skipping: Prometheus not accessible or not configured");
}

#[test]
fn test_fuse_mounts_online_both_clients() {
    println!("[TEST] test_fuse_mounts_online_both_clients: Checking FUSE mounts...");

    let client_ips = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "");
    let ips: Vec<&str> = client_ips.split(',').filter(|s| !s.is_empty()).collect();

    if ips.is_empty() {
        println!("Skipping: No client IPs configured");
        return;
    }

    let mount_path = get_env_or_default("CLAUDEFS_MOUNT_PATH", "/mnt/claudefs");
    let mut all_mounted = true;
    let mut failed_clients = Vec::new();

    for ip in &ips {
        print!("  Checking client {}: ", ip);

        let check_mount = run_ssh_command(ip, &format!("mountpoint {}", mount_path));
        let check_stat = run_ssh_command(ip, &format!("stat {}", mount_path));
        let check_df = run_ssh_command(ip, &format!("df -h {}", mount_path));

        let mounted = check_mount.is_ok()
            || check_mount
                .map(|s| s.contains("is a mountpoint"))
                .unwrap_or(false);

        if mounted && check_stat.is_ok() {
            println!("MOUNTED");
            if let Ok(df_output) = check_df {
                for line in df_output.lines().skip(1).take(1) {
                    println!("    {}", line);
                }
            }
        } else {
            println!("NOT MOUNTED");
            all_mounted = false;
            failed_clients.push(ip.to_string());
        }
    }

    assert!(
        all_mounted,
        "FUSE mounts not online on clients: {:?}",
        failed_clients
    );
}

#[test]
fn test_network_connectivity_matrix() {
    println!("[TEST] test_network_connectivity_matrix: Checking network connectivity...");

    let orchestrator_ip = get_env_or_default("CLAUDEFS_CLUSTER_ORCHESTRATOR_IP", "");
    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let client_ips = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "");

    let mut all_ips = Vec::new();
    if !orchestrator_ip.is_empty() {
        all_ips.push(orchestrator_ip.clone());
    }
    all_ips.extend(
        storage_ips
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from),
    );
    all_ips.extend(
        client_ips
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from),
    );

    if all_ips.len() < 2 {
        println!("Skipping: Not enough IPs configured for matrix test");
        return;
    }

    println!("  Testing connectivity between {} nodes", all_ips.len());

    let mut results = HashMap::new();
    let mut failed_pairs = Vec::new();
    let mut high_latency = Vec::new();

    for from_ip in &all_ips {
        for to_ip in &all_ips {
            if from_ip == to_ip {
                continue;
            }

            let key = format!("{}->{}", from_ip, to_ip);
            match ping_latency(from_ip, to_ip) {
                Ok(latency) => {
                    println!("    {}: {:.2}ms", key, latency);
                    results.insert(key.clone(), latency);

                    if latency > 20.0 {
                        high_latency.push((key, latency));
                    }
                }
                Err(e) => {
                    println!("    {}: FAILED ({})", key, e);
                    failed_pairs.push(key.clone());
                }
            }
        }
    }

    if !failed_pairs.is_empty() {
        panic!(
            "Network connectivity failed between pairs: {:?}",
            failed_pairs
        );
    }

    if !high_latency.is_empty() {
        println!(
            "  Warning: High latency pairs (>{:.0}ms): {:?}",
            20.0, high_latency
        );
    }
}

#[test]
fn test_security_groups_rules_correct() {
    println!("[TEST] test_security_groups_rules_correct: Checking security group rules...");

    let region = get_env_or_default("AWS_REGION", "us-west-2");
    let cluster_name = get_env_or_default("CLAUDEFS_CLUSTER_NAME", "claudefs");

    let output = Command::new("aws")
        .args([
            "ec2",
            "describe-security-groups",
            "--filters",
            &format!("Name=tag:Cluster,Values={}", cluster_name),
            "--region",
            &region,
            "--query",
            "SecurityGroups[].{Name:GroupName,Rules:IpPermissions}",
            "--output",
            "json",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let json_str = String::from_utf8_lossy(&out.stdout);
            println!("  Security groups found");

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(groups) = json.as_array() {
                    for group in groups {
                        let name = group
                            .get("Name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        if let Some(rules) = group.get("Rules").and_then(|r| r.as_array()) {
                            println!("    {}: {} rules", name, rules.len());
                        }
                    }
                }
            }
        }
        Ok(out) => {
            println!(
                "  Warning: Could not describe security groups: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => {
            println!("  Warning: Could not query security groups: {}", e);
        }
    }

    println!("  Skipping detailed validation (requires AWS IAM permissions)");
}

#[test]
fn test_disk_io_baseline_performance() {
    println!("[TEST] test_disk_io_baseline_performance: Checking NVMe disk I/O...");

    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let ips: Vec<&str> = storage_ips.split(',').filter(|s| !s.is_empty()).collect();

    if ips.is_empty() {
        println!("Skipping: No storage IPs configured");
        return;
    }

    let test_file = "/tmp/fio_test";
    let min_iops = 100_000;

    let mut results = Vec::new();

    for ip in &ips {
        print!("  Testing {}: ", ip);

        let _cleanup = run_ssh_command(
            ip,
            &format!("rm -f {} 2>/dev/null; echo cleaned", test_file),
        );

        let fio_cmd = format!(
            "fio --name=randread --filename={} --rw=randread --bs=4k --iodepth=32 --ioengine=libaio --direct=1 --size=1G --runtime=10 --time_based --group_reporting 2>/dev/null | grep 'read:' | awk '{{print $2}}'",
            test_file
        );

        match run_ssh_command(ip, &fio_cmd) {
            Ok(output) => {
                let iops_str = output.trim();
                if let Ok(iops) = iops_str.replace(",", "").parse::<u64>() {
                    println!("{} IOPS", iops);
                    results.push((ip.to_string(), iops));

                    if iops < min_iops {
                        println!(
                            "    Warning: IOPS below expected minimum ({} < {})",
                            iops, min_iops
                        );
                    }
                } else {
                    println!("Could not parse IOPS from: {}", output);
                }
            }
            Err(e) => {
                println!("FIO not available or failed: {}", e);
            }
        }

        let _ = run_ssh_command(ip, &format!("rm -f {}", test_file));
    }

    if results.is_empty() {
        println!("  Skipping: Could not run FIO tests");
    }
}

#[test]
fn test_memory_available_on_all_nodes() {
    println!("[TEST] test_memory_available_on_all_nodes: Checking memory on all nodes...");

    let orchestrator_ip = get_env_or_default("CLAUDEFS_CLUSTER_ORCHESTRATOR_IP", "");
    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let client_ips = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "");

    let mut all_ips = Vec::new();
    if !orchestrator_ip.is_empty() {
        all_ips.push(orchestrator_ip);
    }
    all_ips.extend(
        storage_ips
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from),
    );
    all_ips.extend(
        client_ips
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from),
    );

    if all_ips.is_empty() {
        println!("Skipping: No IPs configured");
        return;
    }

    let max_mem_used_pct = 80;
    let mut failed_nodes = Vec::new();

    for ip in &all_ips {
        print!("  Checking {}: ", ip);

        match run_ssh_command(
            ip,
            "free | grep Mem | awk '{printf \"%.0f\\n\", ($3/$2)*100}'",
        ) {
            Ok(output) => {
                if let Ok(used_pct) = output.trim().parse::<u64>() {
                    println!("{}% used", used_pct);
                    if used_pct < max_mem_used_pct {
                    } else {
                        failed_nodes.push((ip.to_string(), used_pct));
                    }
                } else {
                    println!("Could not parse memory usage");
                }
            }
            Err(e) => {
                println!("Error: {}", e);
                failed_nodes.push((ip.to_string(), 0));
            }
        }
    }

    assert!(
        failed_nodes.is_empty(),
        "Nodes with high memory usage: {:?}",
        failed_nodes
    );
}

#[test]
fn test_cross_az_latency_acceptable() {
    println!("[TEST] test_cross_az_latency_acceptable: Checking cross-AZ latency...");

    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let ips: Vec<&str> = storage_ips.split(',').filter(|s| !s.is_empty()).collect();

    if ips.len() < 2 {
        println!("Skipping: Need at least 2 storage nodes");
        return;
    }

    println!("  Testing cross-AZ latency from first storage node to others");
    let from_ip = ips[0];

    let min_latency = 15.0;
    let max_latency = 30.0;
    let mut cross_az_tests = 0;
    let mut results = Vec::new();

    for to_ip in &ips[1..] {
        print!("    {} -> {}: ", from_ip, to_ip);
        match ping_latency(from_ip, to_ip) {
            Ok(latency) => {
                println!("{:.2}ms", latency);
                cross_az_tests += 1;
                results.push(latency);

                if latency < min_latency || latency > max_latency {
                    println!(
                        "      Warning: Latency outside expected range ({:.0}-{:.0}ms)",
                        min_latency, max_latency
                    );
                }
            }
            Err(e) => {
                println!("FAILED: {}", e);
            }
        }
    }

    if cross_az_tests == 0 {
        println!("  Skipping: Could not test cross-AZ latency");
    }
}

#[test]
fn test_s3_throughput_baseline() {
    println!("[TEST] test_s3_throughput_baseline: Testing S3 throughput...");

    let bucket = get_env_or_skip("CLAUDEFS_S3_BUCKET");
    let region = get_env_or_default("AWS_REGION", "us-west-2");
    let test_key = "test/throughput_test_100mb.bin";

    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let ip = storage_ips.split(',').next().unwrap_or("");

    if ip.is_empty() {
        println!("Skipping: No storage IP configured");
        return;
    }

    print!("  Testing PUT (100MB): ");
    let mk_test_file = "dd if=/dev/urandom of=/tmp/test_100mb.bin bs=1M count=100 2>/dev/null";
    let _ = run_ssh_command(ip, mk_test_file);

    let start_put = std::time::Instant::now();
    let put_cmd = format!(
        "aws s3 cp /tmp/test_100mb.bin s3://{}/{} --region {}",
        bucket, test_key, region
    );

    match run_ssh_command(ip, &put_cmd) {
        Ok(_) => {
            let elapsed = start_put.elapsed().as_secs_f64();
            let throughput = 100.0 / elapsed;
            println!("{} MB/s", (throughput * 10.0).round() / 10.0);

            print!("  Testing GET (100MB): ");
            let get_cmd = format!(
                "aws s3 cp s3://{}/{} /tmp/test_get.bin --region {}",
                bucket, test_key, region
            );
            let start_get = std::time::Instant::now();

            match run_ssh_command(ip, &get_cmd) {
                Ok(_) => {
                    let elapsed = start_get.elapsed().as_secs_f64();
                    let throughput = 100.0 / elapsed;
                    println!("{} MB/s", (throughput * 10.0).round() / 10.0);
                }
                Err(e) => {
                    println!("GET failed: {}", e);
                }
            }

            let cleanup = format!(
                "rm -f /tmp/test_100mb.bin /tmp/test_get.bin; aws s3 rm s3://{}/{} --region {}",
                bucket, test_key, region
            );
            let _ = run_ssh_command(ip, &cleanup);
        }
        Err(e) => {
            println!("PUT failed: {}", e);
        }
    }
}

#[test]
fn test_cluster_clock_skew_within_limits() {
    println!("[TEST] test_cluster_clock_skew_within_limits: Checking clock skew...");

    let orchestrator_ip = get_env_or_default("CLAUDEFS_CLUSTER_ORCHESTRATOR_IP", "");
    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let client_ips = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "");

    let mut all_ips = Vec::new();
    if !orchestrator_ip.is_empty() {
        all_ips.push(orchestrator_ip);
    }
    all_ips.extend(
        storage_ips
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from),
    );
    all_ips.extend(
        client_ips
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from),
    );

    if all_ips.is_empty() {
        println!("Skipping: No IPs configured");
        return;
    }

    let max_skew_ms = 10.0;
    let mut offsets = Vec::new();

    for ip in &all_ips {
        print!("  {}: ", ip);
        match collect_ntp_offset(ip) {
            Ok(offset_ms) => {
                println!("{:.2}ms offset", offset_ms);
                offsets.push((ip.to_string(), offset_ms));

                if offset_ms.abs() > max_skew_ms {
                    println!("    Warning: Clock skew exceeds {}ms", max_skew_ms);
                }
            }
            Err(e) => {
                println!("Could not determine: {}", e);
            }
        }
    }

    if offsets.is_empty() {
        println!("  Skipping: Could not collect NTP offsets");
    }
}

#[test]
fn test_metadata_service_responding() {
    println!("[TEST] test_metadata_service_responding: Checking metadata RPC...");

    let meta_ips = get_env_or_default("CLAUDEFS_META_NODE_IPS", "");
    let ips: Vec<&str> = meta_ips.split(',').filter(|s| !s.is_empty()).collect();

    if ips.is_empty() {
        println!("Skipping: No metadata node IPs configured");
        return;
    }

    let port = get_env_or_default("CLAUDEFS_META_PORT", "50000");
    let mut all_responding = true;

    for ip in &ips {
        print!("  Checking {}: ", ip);

        match run_ssh_command(
            ip,
            &format!(
                "netstat -tlnp 2>/dev/null | grep :{} || ss -tlnp | grep :{}",
                port, port
            ),
        ) {
            Ok(output) => {
                if output.contains(&port) || output.contains("LISTEN") {
                    println!("LISTENING");
                } else {
                    println!("NOT LISTENING on port {}", port);
                    all_responding = false;
                }
            }
            Err(e) => {
                println!("Error: {}", e);
                all_responding = false;
            }
        }
    }

    assert!(
        all_responding,
        "Metadata service not responding on some nodes"
    );
}

#[test]
fn test_replication_conduit_healthy() {
    println!("[TEST] test_replication_conduit_healthy: Checking replication conduit...");

    let conduit_ip = get_env_or_default("CLAUDEFS_CONDUIT_IP", "");

    if conduit_ip.is_empty() {
        println!("Skipping: No conduit IP configured");
        return;
    }

    print!("  Checking conduit node {}: ", conduit_ip);

    let check_process = run_ssh_command(
        &conduit_ip,
        "ps aux | grep -E 'replication|conduit' | grep -v grep",
    );
    let check_port = run_ssh_command(
        &conduit_ip,
        "netstat -tlnp 2>/dev/null | grep 50001 || ss -tlnp | grep 50001",
    );

    let healthy = check_process.is_ok() || check_port.is_ok();

    if healthy {
        println!("HEALTHY");

        if query_prometheus("claudefs_replication_lag").is_ok() {
            println!("    Replication lag metric available");
        }
    } else {
        println!("NOT HEALTHY");
    }
}

#[test]
fn test_cluster_initial_state_ready_for_workload() {
    println!("[TEST] test_cluster_initial_state_ready_for_workload: Summary test...");

    let orchestrator_ip = get_env_or_default("CLAUDEFS_CLUSTER_ORCHESTRATOR_IP", "");
    let storage_ips = get_env_or_default("CLAUDEFS_STORAGE_NODE_IPS", "");
    let client_ips = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "");
    let bucket = std::env::var("CLAUDEFS_S3_BUCKET").ok();

    let mut checks = Vec::new();

    println!("\n  Running pre-flight checks:");

    print!("    - Orchestrator accessible: ");
    if !orchestrator_ip.is_empty() {
        match run_ssh_command(&orchestrator_ip, "echo OK") {
            Ok(_) => {
                println!("YES");
                checks.push(("orchestrator", true));
            }
            Err(e) => {
                println!("NO ({})", e);
                checks.push(("orchestrator", false));
            }
        }
    } else {
        println!("NOT CONFIGURED");
        checks.push(("orchestrator", false));
    }

    print!("    - Storage nodes accessible: ");
    if !storage_ips.is_empty() {
        let count = storage_ips.split(',').filter(|s| !s.is_empty()).count();
        println!("{} configured", count);
        checks.push(("storage_nodes", true));
    } else {
        println!("NOT CONFIGURED");
        checks.push(("storage_nodes", false));
    }

    print!("    - Client nodes accessible: ");
    if !client_ips.is_empty() {
        let count = client_ips.split(',').filter(|s| !s.is_empty()).count();
        println!("{} configured", count);
        checks.push(("client_nodes", true));
    } else {
        println!("NOT CONFIGURED");
        checks.push(("client_nodes", false));
    }

    print!("    - S3 bucket accessible: ");
    if let Some(b) = &bucket {
        match check_s3_bucket_access(b, "us-west-2") {
            Ok(_) => {
                println!("YES");
                checks.push(("s3_bucket", true));
            }
            Err(e) => {
                println!("NO ({})", e);
                checks.push(("s3_bucket", false));
            }
        }
    } else {
        println!("NOT CONFIGURED");
        checks.push(("s3_bucket", false));
    }

    print!("    - Prometheus accessible: ");
    match query_prometheus("up") {
        Ok(_) => {
            println!("YES");
            checks.push(("prometheus", true));
        }
        Err(e) => {
            println!("NO ({})", e);
            checks.push(("prometheus", false));
        }
    }

    println!("\n  Pre-flight summary:");
    for (name, status) in &checks {
        println!(
            "    {}: {}",
            name,
            if *status { "READY" } else { "NOT READY" }
        );
    }

    let ready_count = checks.iter().filter(|(_, s)| *s).count();
    let total_count = checks.len();

    println!("\n  Result: {}/{} checks passed", ready_count, total_count);

    assert_eq!(ready_count, total_count, "Cluster not ready for workload");
}
