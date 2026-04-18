# A3: Phase 32 Block 5 — Multi-Client Workloads Test Generation
## OpenCode Implementation Prompt

**Task:** Implement 14-18 integration tests for multi-client workloads on real cluster
**Target File:** `crates/claudefs-reduce/tests/cluster_multi_client_workloads.rs`
**Target LOC:** 750-900
**Target Tests:** 18

---

## Context

This is ClaudeFS, a distributed POSIX file system. The claudefs-reduce crate handles deduplication, compression, encryption, and data reduction.

Existing test files to follow for conventions:
- `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs` (1132 lines, 20 tests)
- `crates/claudefs-reduce/tests/cluster_helpers.rs` (418 lines, shared utilities)

Test infrastructure:
- Uses SSH to execute commands on remote client/storage nodes
- Prometheus metrics for validation
- FUSE mount at /mnt/claudefs
- Environment variables: CLAUDEFS_STORAGE_NODE_IPS, CLAUDEFS_CLIENT_NODE_IPS, PROMETHEUS_URL

---

## Required Test Functions (18 tests)

1. `test_cluster_two_clients_concurrent_writes` — Both clients write concurrently, no corruption
2. `test_cluster_two_clients_same_file_coordination` — Both write same file, LWW resolution
3. `test_cluster_two_clients_dedup_shared_data` — Same fingerprints shared across clients
4. `test_cluster_two_clients_quota_per_client` — Each client has independent quota
5. `test_cluster_two_clients_cache_coherency_across_clients` — Cache invalidation on peer writes
6. `test_cluster_two_clients_refcount_coordination_concurrent` — Concurrent refcount updates race-free
7. `test_cluster_two_clients_one_fails` — Client 1 crashes, Client 2 continues normally
8. `test_cluster_two_clients_snapshot_consistency` — Snapshot while both clients writing
9. `test_cluster_two_clients_read_after_write_different_client` — Write on C1, read on C2
10. `test_cluster_two_clients_metadata_consistency_reads` — Both see same metadata
11. `test_cluster_two_clients_performance_parallel_writes` — Throughput with 2 clients
12. `test_cluster_two_clients_network_partition_between_clients` — Partition, recovery
13. `test_cluster_two_clients_delete_coordination` — Delete on C1, C2 sees update
14. `test_cluster_two_clients_replication_consistency_cross_site` — Multi-site + multi-client
15. `test_cluster_two_clients_latency_p99_concurrent` — P99 latency under concurrent load
16. `test_cluster_two_clients_mixed_workload_production_like` — Production-like mixed workload
17. `test_cluster_two_clients_10x_throughput` — 2 clients approaching 2x throughput
18. `test_cluster_multi_client_ready_for_chaos` — All tests passed

---

## Required Helper Functions

- `get_client_nodes() -> Vec<String>` — Get client node IPs from env
- `get_client_node(client_id: usize) -> Result<String, String>` — Get specific client IP
- `write_from_client(client_id: usize, path: &str, size_mb: usize) -> Result<(), String>`
- `read_from_client(client_id: usize, path: &str) -> Result<Vec<u8>, String>`
- `delete_from_client(client_id: usize, path: &str) -> Result<(), String>`
- `copy_from_client(client_id: usize, src: &str, dst: &str) -> Result<(), String>`
- `file_exists_on_client(client_id: usize, path: &str) -> Result<bool, String>`
- `get_client_quota(client_id: usize) -> Result<u64, String>`
- `set_client_quota(client_id: usize, bytes: u64) -> Result<(), String>`
- `simulate_client_failure(client_id: usize) -> Result<(), String>`
- `restore_client(client_id: usize) -> Result<(), String>`
- `measure_concurrent_throughput(client_ids: &[usize], duration_secs: u64) -> Result<f64, String>`
- `query_prometheus_metric(metric: &str) -> Result<f64, String>`
- `wait_for_metric(metric: &str, target: f64, timeout_secs: u64) -> Result<(), String>`
- `check_two_clients_available() -> bool`
- `create_snapshot_from_client(client_id: usize, name: &str) -> Result<(), String>`

---

## Code Structure Requirements

1. Module-level documentation header explaining test purpose and prerequisites
2. All imports at top (std::process::Command, std::time::{Duration, Instant}, etc.)
3. Constants: FUSE_MOUNT_PATH = "/mnt/claudefs"
4. Helper functions section (organized logically)
5. Test functions section (all marked #[test] #[ignore])
6. Each test should:
   - Check if two clients available, skip with message if not
   - Use Result<(), String> return type
   - Have descriptive doc comments
   - Use descriptive assertion messages
   - Clean up test files in success and error paths
   - Query Prometheus metrics for validation

---

## Example Test Pattern

```rust
#[test]
#[ignore]
fn test_cluster_two_clients_concurrent_writes() -> Result<(), String> {
    if !check_two_clients_available() {
        return Err("Need 2 client nodes for this test".to_string());
    }
    
    // Write from both clients concurrently
    let file1 = "client1_concurrent.bin";
    let file2 = "client2_concurrent.bin";
    
    write_from_client(0, file1, 50)?;
    write_from_client(1, file2, 50)?;
    
    std::thread::sleep(Duration::from_secs(10));
    
    // Verify both files exist and are readable
    assert!(file_exists_on_client(0, file1)?, "Client 1 file should exist");
    assert!(file_exists_on_client(1, file2)?, "Client 2 file should exist");
    
    // Verify dedup metrics
    let dedup_bytes = query_prometheus_metric("claudefs_dedup_bytes_saved_total")?;
    assert!(dedup_bytes >= 0.0, "Dedup metrics should be available");
    
    // Cleanup
    delete_from_client(0, file1)?;
    delete_from_client(1, file2)?;
    
    Ok(())
}
```

---

## SSH Command Pattern

```rust
fn ssh_exec(node_ip: &str, cmd: &str, timeout_secs: u64) -> Result<String, String> {
    let user = std::env::var("SSH_USER").unwrap_or_else(|_| "ubuntu".to_string());
    let key = std::env::var("SSH_PRIVATE_KEY").unwrap_or_else(|_| "~/.ssh/id_rsa".to_string());
    let sh_cmd = format!(
        "ssh -o ConnectTimeout=10 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i {} {}@{} '{}'",
        key, user, node_ip, cmd
    );
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&sh_cmd)
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;
    
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!("SSH error: {}", String::from_utf8_lossy(&output.stderr)))
    }
}
```

---

## Prometheus Query Pattern

```rust
fn query_prometheus_metric(query: &str) -> Result<f64, String> {
    let prometheus_url = std::env::var("PROMETHEUS_URL")
        .unwrap_or_else(|_| "http://localhost:9090".to_string());
    
    let url = format!("{}/api/v1/query?query={}", prometheus_url, url_encode(query));
    
    let output = Command::new("curl")
        .arg("-s")
        .arg(&url)
        .output()
        .map_err(|e| format!("Prometheus query failed: {}", e))?;
    
    let response = String::from_utf8_lossy(&output.stdout);
    
    // Parse JSON response to extract value
    if let Some(start) = response.find("\"value\"") {
        if let Some(bracket) = response[start..].find('[') {
            if let Some(closing) = response[start + bracket..].find(']') {
                let val_str = &response[start + bracket + 1..start + bracket + closing];
                return val_str.trim().parse::<f64>()
                    .map_err(|e| format!("Parse failed: {}", e));
            }
        }
    }
    
    Err(format!("Metric not found: {}", response))
}
```

---

## File Operations Pattern

```rust
fn write_from_client(client_id: usize, path: &str, size_mb: usize) -> Result<(), String> {
    let clients = get_client_nodes();
    if client_id >= clients.len() {
        return Err(format!("Client {} not available", client_id));
    }
    let client_ip = &clients[client_id];
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    
    let cmd = format!("dd if=/dev/urandom of={} bs=1M count={} conv=fdatasync", full_path, size_mb);
    ssh_exec(client_ip, &cmd, size_mb as u64 + 30)?;
    Ok(())
}

fn read_from_client(client_id: usize, path: &str) -> Result<Vec<u8>, String> {
    let clients = get_client_nodes();
    if client_id >= clients.len() {
        return Err(format!("Client {} not available", client_id));
    }
    let client_ip = &clients[client_id];
    let full_path = format!("{}/{}", FUSE_MOUNT_PATH, path);
    
    let cmd = format!("cat {}", full_path);
    let output = ssh_exec(client_ip, &cmd, 60)?;
    Ok(output.into_bytes())
}
```

---

## Test Scenarios Detail

### test_cluster_two_clients_same_file_coordination
- Both clients write to same file path
- Verify LWW (last-write-wins) semantics
- Check no file corruption
- Prometheus: check coordination metrics

### test_cluster_two_clients_dedup_shared_data
- Client 1 writes file with zeros (high dedup potential)
- Client 2 writes same content to different path
- Verify dedup ratio > 50%
- Check shared fingerprint count increased

### test_cluster_two_clients_cache_coherency_across_clients
- Client 1 writes file
- Client 2 reads same file
- Client 1 overwrites file
- Client 2 reads again, verifies new content
- Check cache invalidation metrics

### test_cluster_two_clients_refcount_coordination_concurrent
- Client 1 creates reference file
- Both clients copy file simultaneously (5-10 copies each)
- Verify refcount is accurate after all copies
- Delete all copies, verify refcount returns to 1

### test_cluster_two_clients_performance_parallel_writes
- Measure single client throughput (baseline)
- Measure both clients writing simultaneously
- Verify combined throughput > 1.5x single client
- Run for 60 seconds, collect metrics

### test_cluster_two_clients_mixed_workload_production_like
- Simulate production: 70% reads, 20% writes, 10% deletes
- Mix of file sizes: 1MB, 10MB, 50MB, 100MB
- Both clients running workload for 2 minutes
- Verify no errors, stable throughput

### test_cluster_two_clients_10x_throughput
- Target: With 2 clients, achieve ~2x throughput of 1 client
- Write 100 files per client (10MB each)
- Measure total time vs single client baseline
- Assert: combined_throughput >= 1.8x single_client

---

## Important Notes

1. ALL tests must be marked with `#[ignore]` — they require real cluster
2. ALL helper functions must handle errors gracefully with Result<(), String>
3. ALL assertions must have descriptive messages
4. ALL tests must cleanup files (use drop guards or explicit cleanup)
5. Use existing cluster_helpers.rs patterns where applicable
6. Target 750-900 lines of code
7. Follow Rust naming conventions: snake_case
8. Target line count: 800-900 lines

---

## Success Criteria

✅ 18 tests implemented with proper #[test] #[ignore] annotations
✅ All helper functions for multi-client operations
✅ Comprehensive doc comments for each test
✅ Result<(), String> error handling throughout
✅ Prometheus metrics integration
✅ SSH-based remote execution
✅ Cleanup in all code paths (success and error)
✅ Compiles without errors or warnings
✅ Follows patterns from cluster_multinode_dedup.rs

---

## Output

Generate a complete, production-ready Rust test file with all 18 tests and helper functions. The file should be fully compilable and ready for integration testing on a real ClaudeFS cluster with 2 FUSE client nodes.
