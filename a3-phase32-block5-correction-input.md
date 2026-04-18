# A3: Phase 32 Block 5 — Multi-Client Workloads (CORRECTION)
## OpenCode Correction Prompt

**Status:** Initial generation has 13 compilation errors. Re-generate with fixes.

**Error Summary:**
- Type mismatches (6x): Result<String> vs Result<()>
- Move errors (4x): client0_ip, test_dir0, client1_ip, test_dir1
- Borrow errors (2x): latencies not mutable, moved values
- Function signature error: wrong argument count
- Missing imports: Duration, EncryptionKey

**File:** `crates/claudefs-reduce/tests/cluster_multi_client_workloads.rs`
**Target:** Fix all 13 errors to compile successfully

---

## Key Fixes Required

### Fix 1: Type Mismatches (Result Types)
All test functions must return `Result<(), String>`, not `Result<String, String>`.

### Fix 2: Move Errors in Loops
**Problem:**
```rust
let client0_ip = ...;
for i in 0..5 {
    use_value(client0_ip);  // Moved each iteration
}
```

**Solution:**
```rust
let client0_ip = ...;
for i in 0..5 {
    use_value(&client0_ip);  // Use reference
}
```

### Fix 3: Mutable Borrow Error
**Problem:**
```rust
let latencies = vec![];
latencies.push(...);  // ERROR: latencies not mut
```

**Solution:**
```rust
let mut latencies = vec![];
latencies.push(...);  // OK
```

### Fix 4: Import Statements
Add missing imports:
```rust
use std::time::{Duration, Instant};
```

---

## Re-Generation Requirements

1. **All test functions:** Return `Result<(), String>`
2. **All parameters in loops:** Use references (&)
3. **All vectors modified:** Declare as `mut`
4. **All imports:** Include Duration, Instant, String utilities
5. **All helpers:** Follow pattern from cluster_single_node_dedup.rs

---

## Reference Template

```rust
use std::process::Command;
use std::time::{Duration, Instant};

fn get_env_or_default(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn ssh_exec(node: &str, cmd: &str) -> Result<String, String> {
    let output = Command::new("ssh")
        .args(["-o", "StrictHostKeyChecking=no", node, cmd])
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[ignore]
#[tokio::test]
async fn test_cluster_two_clients_concurrent_writes() -> Result<(), String> {
    let client0_ip = get_env_or_default("CLAUDEFS_CLIENT0_IP", "client0");
    let client1_ip = get_env_or_default("CLAUDEFS_CLIENT1_IP", "client1");

    // Use references in loops
    for i in 0..3 {
        let file = format!("test_{}.txt", i);
        ssh_exec(&client0_ip, &format!("touch {}", file))?;
        ssh_exec(&client1_ip, &format!("touch {}", file))?;
    }

    // Track mutable state
    let mut success_count = 0;
    for i in 0..5 {
        if ssh_exec(&client0_ip, "ls").is_ok() {
            success_count += 1;
        }
    }

    assert!(success_count > 0, "Expected at least 1 successful exec");
    Ok(())
}
```

---

## All 14-18 Tests Must Be Preserved

- test_cluster_two_clients_concurrent_writes
- test_cluster_two_clients_same_file_coordination
- test_cluster_two_clients_dedup_shared_data
- test_cluster_two_clients_quota_per_client
- test_cluster_two_clients_cache_coherency_across_clients
- test_cluster_two_clients_refcount_coordination_concurrent
- test_cluster_two_clients_one_fails
- test_cluster_two_clients_snapshot_consistency
- test_cluster_two_clients_read_after_write_different_client
- test_cluster_two_clients_metadata_consistency_reads
- test_cluster_two_clients_performance_parallel_writes
- test_cluster_two_clients_network_partition_between_clients
- test_cluster_two_clients_delete_coordination
- test_cluster_two_clients_replication_consistency_cross_site
- test_cluster_two_clients_latency_p99_concurrent
- (and more...)

---

## Success Criteria

✅ Zero compilation errors
✅ All 14-18 tests compile and marked #[ignore]
✅ cargo build -p claudefs-reduce succeeds
