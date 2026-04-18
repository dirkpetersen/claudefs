# A3: Phase 32 Block 3 — Multi-Node Dedup Coordination (CORRECTION)
## OpenCode Correction Prompt

**Status:** Previous generation has compilation errors. Re-generate with corrections.

**Errors Found:**
1. Type mismatch: returning `Result<String, String>` where `Result<(), String>` expected (line 235-236)
2. Unused variable: `timeout_secs` parameter not used in `ssh_exec()` function (line 52)
3. Unused variable: `failover_count` not used (line 726)

**File:** `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs`
**Target:** Re-generate to fix all compilation errors

---

## Key Fixes Required

### Fix 1: Type Mismatch in Test (around line 235)
**Problem:**
```rust
// Current (wrong):
fn some_test() -> Result<(), String> {
    // ...
    return Ok(format!("message")); // ERROR: returns Result<String, String>
}
```

**Solution:**
```rust
// Correct:
fn some_test() -> Result<(), String> {
    // ...
    return Ok(()); // Return unit type, not String
}

// OR if you need to return info:
fn some_test() -> Result<String, String> { // Change return type
    // ...
    return Ok(format!("message"));
}
```

### Fix 2: Unused `timeout_secs` Parameter
**Problem:**
```rust
fn ssh_exec(node_ip: &str, cmd: &str, timeout_secs: u64) -> Result<String, String> {
    // timeout_secs never used, compiler warning
}
```

**Solution:**
```rust
fn ssh_exec(node_ip: &str, cmd: &str, _timeout_secs: u64) -> Result<String, String> {
    // Prefix with _ to suppress warning if intentionally unused
}

// OR actually use it:
fn ssh_exec(node_ip: &str, cmd: &str, timeout_secs: u64) -> Result<String, String> {
    let timeout = Duration::from_secs(timeout_secs);
    // Use timeout in ssh command...
}
```

### Fix 3: Unused Variable `failover_count`
**Problem:**
```rust
let failover_count = query_prometheus("claudefs_dedup_failover_count_total")?;
// failover_count never used, compiler warning
```

**Solution:**
```rust
let _failover_count = query_prometheus("claudefs_dedup_failover_count_total")?;
// Prefix with _ if intentionally unused

// OR use it in assertion:
let failover_count = query_prometheus("claudefs_dedup_failover_count_total")?;
assert!(failover_count > 0, "Expected at least 1 failover detected");
```

---

## Re-Generation Request

Please re-generate `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs` with:

1. **All functions return `Result<(), String>` as per spec**
   - No `Result<String, String>` returns from test functions
   - All test assertions use `assert!()` directly
   - Helper functions can return strings for logging, but test functions must return `Result<(), String>`

2. **All function parameters used or prefixed with `_`**
   - No unused warnings
   - All parameters used in function body
   - If `timeout_secs` not used, remove it or prefix with `_`

3. **All variables used**
   - No unused variable warnings
   - Remove assignments to values that are never read
   - Use values in assertions, logs, or return statements

4. **Follow existing patterns from:**
   - `crates/claudefs-reduce/tests/cluster_single_node_dedup.rs`
   - `crates/claudefs-reduce/tests/cluster_multinode_setup.rs`
   - These files are examples of correct structure

5. **Preserve all 16-20 tests**
   - test_cluster_two_nodes_same_fingerprint_coordination
   - test_cluster_dedup_shards_distributed_uniformly
   - test_cluster_dedup_shard_leader_routing
   - test_cluster_dedup_shard_replica_consistency
   - test_cluster_dedup_three_node_write_conflict
   - test_cluster_dedup_refcount_coordination_race
   - test_cluster_dedup_cache_coherency_multi_node
   - test_cluster_dedup_gc_coordination_multi_node
   - test_cluster_dedup_tiering_multi_node_consistency
   - test_cluster_dedup_node_failure_shard_failover
   - test_cluster_dedup_network_partition_shard_split
   - test_cluster_dedup_cascade_node_failures
   - test_cluster_dedup_throughput_5_nodes_linear
   - test_cluster_dedup_latency_multinode_p99
   - test_cluster_dedup_cross_node_snapshot_consistency
   - test_cluster_dedup_journal_replay_after_cascade_failure
   - test_cluster_dedup_worm_enforcement_multi_node
   - test_cluster_dedup_tenant_isolation_multi_node
   - test_cluster_dedup_metrics_aggregation
   - test_cluster_multinode_dedup_ready_for_next_blocks

---

## Verification

After generation:
1. Run: `cargo build -p claudefs-reduce`
2. Should compile with zero errors
3. May have warnings (acceptable) but no errors
4. Tests marked `#[ignore]` (require real cluster)

---

## Example Reference Structure

From `cluster_single_node_dedup.rs` (working example):
```rust
#[ignore]
#[tokio::test]
async fn test_cluster_dedup_basic_write_from_fuse_client() -> Result<(), String> {
    let storage_node = get_storage_node();
    let bucket = get_s3_bucket();

    // Call helper functions
    write_file_fuse("test_file.txt", 10).map_err(|e| format!("Write failed: {}", e))?;

    // Verify with SSH
    let output = ssh_exec(&storage_node, "ls -la /path/to/data")
        .map_err(|e| format!("SSH failed: {}", e))?;

    // Assert
    assert!(output.contains("test_file"), "File not found in output");

    Ok(()) // All test functions return Result<(), String>
}
```

Use this structure for all test functions in Block 3.
