# A3: Phase 32 Block 4 — Tiering with Real S3 Consistency (CORRECTION)
## OpenCode Correction Prompt

**Status:** Previous generation has 19 compilation errors. Re-generate with fixes.

**Error Categories:**
1. **Doc comment errors:** Inner attribute not permitted following outer doc comment
2. **Type mismatch (5x):** Using `?` operator in functions that don't return `Result`
3. **Borrow checker (4x):** Temporary values dropped while borrowed
4. Other type/lifetime issues

**File:** `crates/claudefs-reduce/tests/cluster_tiering_s3_consistency.rs`
**Target:** Re-generate to compile with zero errors

---

## Critical Fixes Required

### Fix 1: Doc Comments
**Problem:**
```rust
/// This is a doc comment
#[ignore]  // ERROR: outer doc followed by inner attribute
async fn test_something() { }
```

**Solution:**
```rust
/// This is a doc comment
#[ignore]
async fn test_something() { }
```

### Fix 2: Using `?` in Non-Result Functions
**Problem:**
```rust
#[tokio::test]
async fn test_something() {  // Returns (), not Result
    let value = some_result_function()?;  // ERROR: can't use ? here
}
```

**Solution:**
```rust
#[tokio::test]
async fn test_something() -> Result<(), String> {  // Return Result
    let value = some_result_function()?;  // Now ? works
    Ok(())
}

// OR without ?:
#[tokio::test]
async fn test_something() {
    let value = match some_result_function() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };
}
```

### Fix 3: Temporary Values & Borrowing
**Problem:**
```rust
let client_ip = get_env_or_default("KEY", "").split(',').next().unwrap_or("");
// temporary from split() is freed, but client_ip borrows from it
if client_ip.is_empty() { }  // ERROR: borrow of freed value
```

**Solution:**
```rust
// Store the temporary first
let client_ips = get_env_or_default("KEY", "");
let client_ip = client_ips.split(',').next().unwrap_or("");
if client_ip.is_empty() { }  // OK: borrows from client_ips which is still alive

// OR use owned types
let client_ip: String = get_env_or_default("KEY", "")
    .split(',')
    .next()
    .unwrap_or("")
    .to_string();
if client_ip.is_empty() { }  // OK: owned String
```

---

## Re-Generation Request

Please re-generate `crates/claudefs-reduce/tests/cluster_tiering_s3_consistency.rs` with:

1. **All test functions return `Result<(), String>`**
   ```rust
   #[ignore]
   #[tokio::test]
   async fn test_something() -> Result<(), String> {
       // ...
       Ok(())
   }
   ```

2. **Proper lifetime/borrow management**
   - Store temporary values in local variables before using them
   - Let bindings keep values alive for the entire scope
   - Use `to_string()` or `String::from()` for owned strings if needed

3. **Doc comments formatted correctly**
   - Outer docs (`///`) on items, then attributes (`#[...]`)
   - No mixing outer docs with attribute errors

4. **All helper functions properly defined**
   - `trigger_tiering_manually() -> Result<(), String>`
   - `check_s3_objects() -> Result<Vec<S3Object>, String>`
   - `fetch_cold_data_and_measure_latency() -> Result<Duration, String>`
   - `simulate_s3_unavailability() -> Result<(), String>`
   - Others as specified

5. **Preserve all 12-16 tests**
   - test_cluster_tiering_hot_to_cold_transition
   - test_cluster_tiering_s3_fetch_on_cold_read
   - test_cluster_tiering_policy_based_movement
   - test_cluster_tiering_s3_failure_resilience
   - test_cluster_tiering_bandwidth_limit_enforcement
   - test_cluster_tiering_concurrent_hot_cold_access
   - test_cluster_tiering_cache_populated_from_s3
   - test_cluster_tiering_metadata_consistency_s3
   - test_cluster_tiering_partial_s3_restore
   - test_cluster_tiering_s3_cleanup_old_chunks
   - test_cluster_tiering_burst_capacity_handling
   - test_cluster_tiering_performance_s3_tier
   - test_cluster_tiering_cross_region_s3
   - test_cluster_tiering_s3_encryption_at_rest
   - test_cluster_tiering_refcount_with_s3_chunks
   - test_cluster_tiering_quota_accounting_with_s3

---

## Reference: Working Example Structure

From `cluster_single_node_dedup.rs` (compiles successfully):

```rust
const FUSE_MOUNT_PATH: &str = "/mnt/claudefs";

fn get_env_or_default(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn ssh_exec(node: &str, cmd: &str) -> Result<String, String> {
    let output = std::process::Command::new("ssh")
        .args(["-o", "StrictHostKeyChecking=no", node, cmd])
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Test S3 fetch on cold read
#[ignore]
#[tokio::test]
async fn test_cluster_tiering_s3_fetch_on_cold_read() -> Result<(), String> {
    let storage_node = get_env_or_default("CFS_STORAGE_NODE", "storage-node-0");

    // Use local variables to keep values alive
    let command = "curl -s http://localhost:9090/metrics | grep 's3_fetch'";
    let metrics_output = ssh_exec(&storage_node, command)?;

    // Assert
    assert!(!metrics_output.is_empty(), "S3 metrics not found");

    Ok(())
}
```

**Key points:**
1. Return type: `Result<(), String>`
2. Attributes in order: `/// doc`, `#[ignore]`, `#[tokio::test]`, `async fn`
3. Store temporaries in local variables
4. Use `?` operator for error propagation
5. Test ends with `Ok(())`

---

## Compilation Check

After re-generation, should pass:
```bash
cargo build -p claudefs-reduce
```

With output:
```
   Compiling claudefs-reduce v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in X.XXs
```

Zero errors (warnings OK).

---

## Common Patterns to Use

### SSH with timeout
```rust
let timeout = Duration::from_secs(30);
let output = ssh_exec(&node, &cmd)?;
// Parse output...
```

### Environment variables
```rust
let s3_bucket = get_env_or_default("CFS_S3_BUCKET", "claudefs-test");
```

### Metrics query
```rust
fn query_s3_metrics() -> Result<Vec<String>, String> {
    // Returns list of metric values
    Ok(vec![])
}
```

### Test assertion
```rust
assert!(condition, "Error message with {}", details);
```

---

## Success Criteria

✅ File compiles: `cargo build -p claudefs-reduce` succeeds
✅ All tests compile: `cargo test --lib -p claudefs-reduce --test cluster_tiering_s3_consistency` compiles
✅ Zero errors, warnings acceptable
✅ All 12-16 tests present and marked #[ignore]
✅ Ready for cluster testing (actual tests run later)
