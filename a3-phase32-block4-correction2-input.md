# A3: Phase 32 Block 4 — Tiering S3 (SECOND CORRECTION)
## OpenCode Follow-up Correction

**Status:** Previous correction still has 11 compilation errors. Apply targeted fixes.

**Remaining Errors (11 total):**
- Value moved in loop (line 934-937)
- Cannot move `client_ip` multiple times
- Multiple ownership/move errors
- Type inference issues

**File:** `crates/claudefs-reduce/tests/cluster_tiering_s3_consistency.rs`

---

## Key Problems & Solutions

### Problem: Moving values in loops
```rust
let client_ip = "192.168.1.1";
for i in 0..5 {
    run_ssh_command(client_ip, "cmd");  // ERROR: moved here, used again next iteration
    //                ^^^^^^^^^
}
```

**Solution: Use reference**
```rust
let client_ip = "192.168.1.1";
for i in 0..5 {
    run_ssh_command(client_ip, "cmd");  // OK: references are Copy
}
```

OR if function takes ownership:
```rust
let client_ip = String::from("192.168.1.1");
for i in 0..5 {
    run_ssh_command(&client_ip, "cmd");  // OK: borrowing
}
```

### Problem: String vs &str
```rust
let value = run_ssh_command(client_ip, ...);
// If client_ip is &str and function expects String, clone it
```

**Solution:**
```rust
let value = run_ssh_command(&client_ip, ...);  // Use reference
// OR
let value = run_ssh_command(&client_ip.to_string(), ...);  // If needs owned String
```

### Problem: Owned values in conditional branches
```rust
let client_ip = get_env_or_default(...);  // Returns String (owned)
if condition {
    use_client_ip(&client_ip);  // OK
}
use_client_ip(&client_ip);  // ERROR: moved in branch above? No, still OK if used with &
```

**Solution:**
- Always use references (`&`) when passing to functions
- Clone only when absolutely necessary
- Prefer `&str` over `String` in most cases

---

## Specific Fixes for cluster_tiering_s3_consistency.rs

1. **Lines 926-932:** Fix temporary lifetime in get_env_or_default
   ```rust
   // Current (wrong):
   let client_ip = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "")
       .split(',')
       .next()
       .unwrap_or("");

   // Fixed:
   let client_ips = get_env_or_default("CLAUDEFS_CLIENT_NODE_IPS", "");
   let client_ip = client_ips.split(',').next().unwrap_or("");
   ```

2. **Lines 934-937:** Use reference in loop
   ```rust
   // Current (wrong):
   let client_ip = ...;
   for i in 0..5 {
       run_ssh_command(client_ip, "cmd");  // Moved each iteration
   }

   // Fixed:
   let client_ip = ...;
   for i in 0..5 {
       run_ssh_command(&client_ip, "cmd");  // Reference (can be reused)
   }
   ```

3. **Helper function signatures must accept references:**
   ```rust
   // Functions should accept &str or &String
   fn run_ssh_command(node: &str, cmd: &str) -> Result<String, String> {
       // ...
   }

   fn fetch_cold_data_and_measure_latency(node: &str, path: &str)
       -> Result<Duration, String> {
       // ...
   }
   ```

4. **All parameters in loops must be references:**
   ```rust
   for i in 0..max_iterations {
       some_function(&param1, &param2)?;  // Use &
   }
   ```

5. **No String clones needed for &str parameters:**
   ```rust
   // Don't do:
   run_ssh_command(&client_ip.clone(), ...);

   // Do:
   run_ssh_command(&client_ip, ...);
   ```

---

## Complete Rewrite Strategy

Rather than partial fixes, REWRITE `cluster_tiering_s3_consistency.rs` with:

### Template Structure
```rust
use std::process::Command;
use std::time::{Duration, Instant};

const FUSE_MOUNT_PATH: &str = "/mnt/claudefs";

fn get_env_or_default(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn get_storage_node() -> String {
    get_env_or_default("CFS_STORAGE_NODE", "storage-node-0")
}

fn run_ssh_command(node: &str, cmd: &str) -> Result<String, String> {
    let output = Command::new("ssh")
        .args(["-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=10", node, cmd])
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Test hot-to-cold transition
#[ignore]
#[tokio::test]
async fn test_cluster_tiering_hot_to_cold_transition() -> Result<(), String> {
    let storage_node = get_storage_node();

    // All uses of storage_node must be references
    run_ssh_command(&storage_node, "command1")?;
    run_ssh_command(&storage_node, "command2")?;

    // Loop example - ALWAYS use references
    for i in 0..3 {
        let cmd = format!("check_{}", i);
        run_ssh_command(&storage_node, &cmd)?;
    }

    Ok(())
}

// All 12-16 tests following same pattern...
```

---

## Compilation Requirements

After rewrite, must pass:
```bash
cargo test -p claudefs-reduce --lib --test cluster_tiering_s3_consistency 2>&1 | grep "^error"
# Should output nothing (no errors)

cargo build -p claudefs-reduce
# Should succeed with 0 errors (warnings OK)
```

---

## Key Points

1. **Variables are owned by default**
   - Once passed to function (not as `&`), they're moved
   - Can't reuse after move

2. **References can be reused**
   - `&variable` creates a reference (non-consuming borrow)
   - Can be used multiple times

3. **Loops need reusable parameters**
   - All parameters in loops should be `&` (reference)
   - Never move variables inside loops

4. **Helper functions should accept `&str`**
   - Cheaper, more flexible than `String`
   - Can accept both `&String` and `&str` literals

5. **Store temporaries in variables**
   - `let x = get_value(); use(&x);` ✅
   - `use(&get_value());` ❌ (temporary freed immediately)

---

## Test Structure Template

```rust
#[ignore]
#[tokio::test]
async fn test_cluster_tiering_SCENARIO() -> Result<(), String> {
    // Setup: Get configuration
    let storage_node = get_storage_node();
    let bucket = get_env_or_default("CFS_S3_BUCKET", "claudefs-test");

    // Execute: Use references for all parameters
    run_ssh_command(&storage_node, "cmd1")?;
    run_ssh_command(&storage_node, "cmd2")?;

    // Verify: Assert results
    let output = run_ssh_command(&storage_node, "verify_cmd")?;
    assert!(!output.is_empty(), "Verification failed");

    Ok(())
}
```

Apply this structure consistently to all 12-16 tests.
