# A3: Phase 32 Block 1 — Multi-Node Cluster Setup & Health Validation
## OpenCode Implementation Prompt

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Task:** Implement 12-15 integration tests validating real AWS ClaudeFS cluster infrastructure
**Target File:** `crates/claudefs-reduce/tests/cluster_multinode_setup.rs`
**Target LOC:** 600-700 lines of Rust test code
**Target Tests:** 12-15 comprehensive cluster health checks

---

## Context & Dependencies

### Phase 31 Completion
- 2,284 tests passing (all single-machine simulation)
- Phase 31 Block 6 (Soak & Production Simulation) validated:
  - 24-hour stability ✅
  - Memory leak detection ✅
  - Concurrent access patterns ✅
  - Crash recovery scenarios ✅

### Phase 32 Goal
Lift Phase 31 tests from single-machine simulation to **real AWS multi-node cluster** where:
- Real FUSE clients mount the filesystem on actual EC2 client nodes
- Real storage nodes run with genuine io_uring I/O and network communication
- Real S3 backend (AWS S3 API, not mock) for tiering
- Real network partitions and latency (cross-region)
- Real node failures (instance termination, reboot)
- Real monitoring (Prometheus, CloudWatch)

### Block 1 Purpose
Before running workload tests (Blocks 2-8), validate that the **cluster infrastructure is properly provisioned** and all health checks pass. This is a **prerequisite** to ensure test results are meaningful.

### Infrastructure Available (A11 Phase 5 Block 1 Complete)
- **1 Orchestrator node** (c7a.2xlarge) — test coordination, SSH entry point
- **5 Storage nodes** (i4i.2xlarge) — 2 in us-west-2a (site-a), 3 in us-west-2b (site-b)
- **2 FUSE client nodes** (c7a.2xlarge) — for multi-client workload testing
- **1 Conduit node** (c7a.xlarge) — cross-site replication traffic
- **1 Jepsen node** (c7a.xlarge) — optional for future chaos injection
- **Prometheus** — running on orchestrator, collecting metrics from all nodes
- **S3 bucket** — created for tiering tests (e.g., `claudefs-tiering-dev`)
- **Security groups** — allowing controlled network partitions (via AWS API)
- **VPC & Subnets** — 2 AZs, proper routing, 2Gbps+ bandwidth

### Environment & Access
- **Region:** us-west-2
- **SSH Access:** From orchestrator (i-03635f49db3350943) to all nodes via private keys
- **AWS API Access:** Full access to EC2, S3, CloudWatch APIs
- **Prometheus URL:** http://localhost:9090 (on orchestrator) or http://prometheus-ip:9090 (other nodes)
- **Node Discovery:** Query AWS EC2 describe-instances with `Name:cfs-*` tags

---

## Test Specifications (12-15 Tests)

### Test 1: `test_cluster_all_nodes_online`
**Purpose:** Verify all cluster nodes are operational and accessible
**Implementation:**
- Query AWS EC2 API: describe-instances with filters (state=running, tag:Cluster=claudefs)
- Expected: 5 storage + 2 clients + 1 conduit + 1 jepsen = 9 nodes
- For each node: SSH connectivity check (ssh -o ConnectTimeout=5 node-ip 'echo OK')
- Assert: 9/9 nodes online, SSH responding
- Timeout: 60 seconds per node
- Failure action: Report node, IP, and error

### Test 2: `test_storage_nodes_ntp_synchronized`
**Purpose:** Verify NTP time sync across storage nodes (required for dedup coordination)
**Implementation:**
- SSH into each of 5 storage nodes
- Run: `timedatectl status | grep synchronized`
- Parse: NTP synchronized = yes, system clock = UTC
- Assert: All 5 storage nodes have synchronized=yes
- Also check: `ntpstat` or `chronyc tracking` for offset <100ms
- Failure: Report node with largest time offset
- Timeout: 30 seconds per node

### Test 3: `test_s3_bucket_accessible_from_all_nodes`
**Purpose:** Verify S3 connectivity from all nodes (required for tiering)
**Implementation:**
- Query AWS: get environment variable CLAUDEFS_S3_BUCKET (e.g., claudefs-tiering-dev)
- For each of 5 storage nodes:
  - SSH: `aws s3 ls s3://${BUCKET}/ --region us-west-2`
  - Assert: exit code 0 (success)
- For each of 2 client nodes:
  - Same S3 list check
- Timeout: 20 seconds per node
- Failure: Report node and AWS error message

### Test 4: `test_prometheus_metrics_collection`
**Purpose:** Verify Prometheus is collecting metrics from all nodes
**Implementation:**
- HTTP GET to http://prometheus-ip:9090/api/v1/query?query=up
- Parse JSON response for target status
- Assert: All 9 nodes (storage + client + conduit + jepsen) have job=claudefs_node, state=up
- Also check: `node_exporter` metrics flowing (node_cpu_seconds_total, node_memory_MemAvailable_bytes)
- Timeout: 10 seconds
- Failure: Report which targets are down

### Test 5: `test_fuse_mounts_online_both_clients`
**Purpose:** Verify FUSE clients have mounted the filesystem at /mnt/claudefs
**Implementation:**
- For each of 2 client nodes:
  - SSH: `mountpoint /mnt/claudefs` (exit code 0 if mounted)
  - SSH: `stat /mnt/claudefs` to verify accessible
  - SSH: `df /mnt/claudefs | grep claudefs` (verify mount in df output)
- Assert: Both clients report /mnt/claudefs mounted
- Optional: `df -h /mnt/claudefs` to verify size (should match configured storage)
- Timeout: 20 seconds per node
- Failure: Report mount status for each client

### Test 6: `test_network_connectivity_matrix`
**Purpose:** Verify network connectivity and latency between all node pairs
**Implementation:**
- Create matrix: ping each of 9 nodes from each other node
- Expected latency: <5ms intra-AZ (us-west-2a or us-west-2b), <20ms cross-AZ
- For each node pair:
  - SSH source node: `ping -c 3 target-ip | tail -1` (grab "avg" latency)
  - Parse: extract avg latency
  - Assert: latency meets threshold
- Timeout: 5 seconds per pair (90 pairs total, parallelizable)
- Failure: Report pairs exceeding threshold

### Test 7: `test_security_groups_rules_correct`
**Purpose:** Verify security group rules match expected configuration
**Implementation:**
- Query AWS EC2 security groups: describe-security-groups with tag:Cluster=claudefs
- Expected rules:
  - Storage nodes: inbound 50000-60000/tcp (ClaudeFS RPC), 9100/tcp (node_exporter)
  - Client nodes: inbound 22/tcp (SSH), 9100/tcp (node_exporter)
  - Conduit: inbound 50000-60000/tcp (replication RPC)
  - All: outbound allow-all
- Assert: Each node has expected rules
- Failure: Report unexpected or missing rules

### Test 8: `test_disk_io_baseline_performance`
**Purpose:** Verify NVMe disk I/O capability on storage nodes
**Implementation:**
- For each of 5 storage nodes:
  - SSH: Run `fio --filename=/dev/nvme0n1 --rw=randread --bs=4k --iodepth=32 --numjobs=4 --runtime=10 --output=/tmp/fio-result.txt`
  - Parse: extract IOPS (should see 500K+)
  - Assert: IOPS >= 500_000 (typical for i4i.2xlarge)
- Timeout: 30 seconds per node (fio runtime 10s + overhead)
- Failure: Report node with low IOPS

### Test 9: `test_memory_available_on_all_nodes`
**Purpose:** Verify no memory pressure on any node
**Implementation:**
- For each of 9 nodes:
  - SSH: `free -b | grep Mem | awk '{print $3}'` (used memory bytes)
  - SSH: `free -b | grep Mem | awk '{print $2}'` (total memory bytes)
  - Calculate: used_pct = used / total
  - Assert: used_pct < 50% (no memory pressure)
- Also check: `/proc/pressure/memory` if available (some=0, full=0)
- Timeout: 10 seconds per node
- Failure: Report node with memory pressure

### Test 10: `test_cross_az_latency_acceptable`
**Purpose:** Verify cross-AZ latency within acceptable range (15-30ms typical AWS)
**Implementation:**
- Identify: storage-site-a (3 nodes in us-west-2a), storage-site-b (2 nodes in us-west-2b)
- Ping from storage-site-a to storage-site-b (avg of 3 pings)
- Assert: latency 15-30ms (typical cross-AZ)
- If latency > 30ms: warn but don't fail (may be expected in some AWS configurations)
- If latency < 5ms: likely same-AZ, fail test
- Timeout: 20 seconds

### Test 11: `test_s3_throughput_baseline`
**Purpose:** Verify S3 throughput capability for tiering
**Implementation:**
- From one storage node:
  - Create 100MB test file: `dd if=/dev/urandom bs=1M count=100 of=/tmp/test-100mb.bin`
  - S3 PUT: `aws s3 cp /tmp/test-100mb.bin s3://${BUCKET}/test-put-baseline.bin --region us-west-2` (time it)
  - Calculate PUT throughput: 100MB / time_seconds
  - S3 GET: `aws s3 cp s3://${BUCKET}/test-put-baseline.bin /tmp/test-get.bin --region us-west-2` (time it)
  - Calculate GET throughput: 100MB / time_seconds
  - Assert: PUT >= 50 MB/s, GET >= 100 MB/s (typical for large objects)
- Timeout: 60 seconds total
- Failure: Report actual throughput

### Test 12: `test_cluster_clock_skew_within_limits`
**Purpose:** Verify clock skew <10ms across all nodes (required for LWW consistency)
**Implementation:**
- From orchestrator: collect NTP offset from each node
- SSH each node: `chronyc tracking | grep 'System time' | awk '{print $NF}'` (offset in microseconds or ms)
- Parse: extract max offset
- Assert: max_offset < 10ms
- Failure: Report node with largest offset

### Test 13: `test_metadata_service_responding`
**Purpose:** Verify metadata RPC endpoints are accessible and responding
**Implementation:**
- Identify metadata service leader (typically storage-node-0 or via Prometheus)
- From orchestrator: SSH to metadata node
- Check: `netstat -tlnp | grep :50000` (ClaudeFS metadata RPC port)
- Assert: process listening on 50000/tcp
- Optional: try connecting with nc (nc -zv metadata-ip 50000), should succeed
- Timeout: 10 seconds

### Test 14: `test_replication_conduit_healthy`
**Purpose:** Verify cross-site replication conduit is operational
**Implementation:**
- Identify conduit node (tag Role=conduit)
- SSH to conduit: check process running (e.g., `ps aux | grep claudefs-conduit`)
- SSH to conduit: `netstat -tlnp | grep :50001` (replication RPC port)
- Check replication lag: query Prometheus metric `replication_lag_seconds` (should be <5s)
- Assert: conduit process running, port listening, lag <5s
- Timeout: 15 seconds

### Test 15: `test_cluster_initial_state_ready_for_workload`
**Purpose:** Summary test: all health checks passed, cluster ready for Phase 32 Blocks 2+
**Implementation:**
- This test runs **only after** tests 1-14 have passed
- Collect results from all 14 tests:
  - nodes_online: bool
  - ntp_synchronized: bool
  - s3_accessible: bool
  - prometheus_metrics: bool
  - fuse_mounts_ok: bool
  - network_latency_ok: bool
  - security_groups_ok: bool
  - disk_io_ok: bool
  - memory_ok: bool
  - cross_az_latency_ok: bool
  - s3_throughput_ok: bool
  - clock_skew_ok: bool
  - metadata_ok: bool
  - conduit_ok: bool
- Assert: all == true
- If all true: print "✅ CLUSTER READY FOR WORKLOAD" and return Ok(())
- If any false: print "❌ CLUSTER NOT READY: <reasons>" and return Err()
- This allows Phase 32 Blocks 2-8 to depend on Block 1 passing

---

## Implementation Guidelines

### Test Structure
```rust
#[cfg(test)]
mod cluster_multinode_setup {
    use std::process::Command;
    use std::time::Duration;

    // Helper functions for SSH, AWS API, HTTP queries

    #[test]
    fn test_cluster_all_nodes_online() {
        // Implementation per spec above
    }

    // ... 14 more tests ...
}
```

### Helper Functions (Create Once, Reuse)
1. `run_ssh_command(node_ip: &str, cmd: &str) -> Result<String, String>`
   - SSH into node, run command, capture output
   - Timeout: 30 seconds (configurable)
   - Return: stdout or error message

2. `aws_ec2_describe_instances() -> Result<Vec<Instance>, String>`
   - Query AWS EC2 API for all claudefs cluster instances
   - Parse state, IP, tags, AZ

3. `query_prometheus(query: &str) -> Result<PrometheusResult, String>`
   - HTTP GET to /api/v1/query
   - Parse JSON response

4. `ping_latency(from_ip: &str, to_ip: &str) -> Result<f64, String>`
   - SSH to from_ip, run ping, parse latency

5. `s3_list_bucket(bucket: &str) -> Result<(), String>`
   - AWS CLI: aws s3 ls
   - Check exit code

6. `collect_ntp_offset(node_ip: &str) -> Result<f64, String>`
   - SSH: chronyc tracking
   - Parse offset in ms

### Error Handling
- Use `Result<(), String>` for tests (simple, clear error messages)
- If environment variable (e.g., CLAUDEFS_CLUSTER_ORCHESTRATOR_IP) not set: skip test with msg
- If node unreachable: fail test with clear message
- All timeout errors should be descriptive (e.g., "node-5 SSH timeout after 60s")

### Assertions
- Use standard `assert!()`, `assert_eq!()` macros
- Always include descriptive messages: `assert!(condition, "Expected X, got Y")`
- For latency: `assert!(latency_ms < 5.0, "latency {:.2}ms exceeds 5ms threshold", latency_ms)`

### Test Execution
- Tests run sequentially (not parallel, to avoid network contention)
- Each test is independent (can run in any order)
- Total runtime target: 5-10 minutes for all 15 tests
- No test should timeout >60 seconds

### Compile & Run
- Should compile with: `cargo test --lib cluster_multinode_setup --no-run`
- Should run with: `cargo test --lib cluster_multinode_setup -- --nocapture`
- All tests should pass with minimal warnings

---

## Dependencies & Imports

```rust
// Standard library
use std::process::Command;
use std::time::Duration;
use std::thread;

// External crates (already in Cargo.toml for tests)
use serde_json::json;  // JSON parsing for Prometheus, AWS APIs

// Suggested but optional for this block:
// - rusoto_ec2 for type-safe AWS API (if available)
// - reqwest for HTTP queries (likely already available)
```

---

## Success Criteria

✅ **All 15 tests compile** without errors
✅ **All 15 tests run** without panic
✅ **All 15 tests pass** when cluster is properly provisioned
✅ **Tests are independent** (can run in any order, can be run individually)
✅ **Error messages are clear** and actionable (operator knows what to fix)
✅ **Runtime** under 10 minutes for full suite
✅ **Zero clippy warnings** in generated code

---

## Notes for OpenCode

1. **Real AWS Infrastructure:** These tests interact with real AWS EC2 instances, S3, Prometheus. Ensure all SSH keys, AWS credentials are available via environment.

2. **Non-Deterministic:** Unlike Phase 31 tests (deterministic mocking), these tests depend on real network, real node states, real metrics. They may occasionally fail due to transient issues (network blips, timing). Build in reasonable retries and timeouts.

3. **Environment Variables:** Tests should read cluster configuration from environment:
   - `CLAUDEFS_CLUSTER_ORCHESTRATOR_IP` — for SSH/Prometheus access
   - `CLAUDEFS_STORAGE_NODE_IPS` — comma-separated list (or auto-discover via AWS)
   - `CLAUDEFS_CLIENT_NODE_IPS` — comma-separated list
   - `CLAUDEFS_S3_BUCKET` — S3 bucket name
   - `AWS_REGION` — default us-west-2
   - `SSH_PRIVATE_KEY` — path to SSH key for orchestrator

4. **Skip Tests When Offline:** If environment variables missing or cluster unreachable, skip with `#[ignore]` or explicit `assert!(false, "cluster not available")`

5. **Logging:** Use `println!()` or `eprintln!()` for progress output. Tests with `--nocapture` flag will show this.

6. **Parallel Execution:** This test module should be thread-safe but tests run sequentially by default. Network queries (ping matrix, SSH commands) can be parallelized within a test if needed.

---

## Files to Generate

- **Output File:** `crates/claudefs-reduce/tests/cluster_multinode_setup.rs` (600-700 LOC)
  - Tests 1-15 as specified
  - Helper functions for SSH, AWS API, Prometheus queries
  - Clear error messages for each failure case

---

## Expected Output

When complete, the operator will run:
```bash
cargo test --lib cluster_multinode_setup -- --nocapture
```

And see:
```
running 15 tests
test cluster_multinode_setup::test_cluster_all_nodes_online ... ok
test cluster_multinode_setup::test_storage_nodes_ntp_synchronized ... ok
test cluster_multinode_setup::test_s3_bucket_accessible_from_all_nodes ... ok
...
test cluster_multinode_setup::test_cluster_initial_state_ready_for_workload ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

---

## References

- **Phase 32 Plan:** docs/A3-PHASE32-PLAN.md
- **Phase 31 Tests:** crates/claudefs-reduce/tests/chaos_failure_modes.rs, etc. (reference patterns)
- **A11 Terraform:** tools/cfs-terraform.sh, tools/terraform/ (infrastructure details)
- **Test Patterns:** crates/claudefs-tests/src/ (POSIX, integration tests — reference for structure)

