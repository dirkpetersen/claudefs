# Fix Two Failing Tests in claudefs-gateway

You are working on the `claudefs-gateway` crate in the ClaudeFS Rust workspace.
There are 2 failing tests. Fix each one by correcting the logic bug in the
implementation (not in the test — the tests are correct).

## Test 1: `s3_storage_class::tests::test_object_storage_state_non_glacier`

**File:** `crates/claudefs-gateway/src/s3_storage_class.rs`

**Test:**
```rust
#[test]
fn test_object_storage_state_non_glacier() {
    let state = ObjectStorageState::new(StorageClass::Standard);
    assert!(!state.needs_restore());
    assert!(state.is_restored());  // FAILS HERE
}
```

**Current `is_restored()` implementation (around line 158):**
```rust
/// Whether the object is currently accessible (restored and not expired)
pub fn is_restored(&self) -> bool {
    if let Some(expiry) = self.restore_expiry {
        // Check if restore has not expired
        std::time::SystemTime::now() < expiry && !self.is_restoring
    } else {
        false  // BUG: returns false for Standard class, but it should be true
    }
}
```

**Bug:** `is_restored()` returns `false` for non-glacier storage classes (Standard,
StandardIa, etc.) because `restore_expiry` is `None` and the function falls to the
`else false` branch. However, objects in non-glacier storage classes are ALWAYS
accessible — they don't need to be "restored". Only glacier classes (Glacier,
GlacierDeepArchive) need restoration.

**Fix:** Add a check at the start of `is_restored()`: if the storage class does not
require restoration (`!self.current_class.requires_restore()`), return `true` immediately.
The `requires_restore()` method is already defined on `StorageClass`.

**Required fix (replace the `is_restored` function body):**
```rust
/// Whether the object is currently accessible (restored and not expired)
pub fn is_restored(&self) -> bool {
    // Non-glacier classes are always accessible — no restore needed
    if !self.current_class.requires_restore() {
        return true;
    }
    // For glacier classes, check if a restore has been completed and hasn't expired
    if let Some(expiry) = self.restore_expiry {
        std::time::SystemTime::now() < expiry && !self.is_restoring
    } else {
        false
    }
}
```

---

## Test 2: `gateway_conn_pool::tests::test_no_healthy_nodes_returns_none`

**File:** `crates/claudefs-gateway/src/gateway_conn_pool.rs`

**Test:**
```rust
#[test]
fn test_no_healthy_nodes_returns_none() {
    let config = ConnPoolConfig::default();
    let mut gateway = GatewayConnPool::new(vec![make_node("node1")], config);

    // Don't add any connections
    let result = gateway.checkout();
    assert!(result.is_none());  // FAILS: returns Some(...)
}
```

**Bug:** `GatewayConnPool::checkout()` has a second fallback loop (after the main loop)
that calls `pool.checkout()` WITHOUT checking `healthy_count() > 0`. This causes
`NodeConnPool::checkout()` to auto-create a new connection even when the pool has
zero connections. The test expects `None` for an empty pool, which is the correct behavior.

**Current `GatewayConnPool::checkout()` implementation (around lines 344-386):**
```rust
pub fn checkout(&mut self) -> Option<(String, u64)> {
    if self.node_order.is_empty() {
        return None;
    }

    // Try each node in round-robin order starting from current index
    let mut attempts = 0;
    while attempts < self.node_order.len() {
        let idx = self.rr_index % self.node_order.len();
        let node_id = self.node_order[idx].clone();
        self.rr_index = (self.rr_index + 1) % self.node_order.len();
        attempts += 1;

        if let Some(pool) = self.pools.get_mut(&node_id) {
            if pool.healthy_count() > 0 {
                if let Some(conn_id) = pool.checkout() {
                    return Some((node_id, conn_id));
                }
            }
        }
    }

    // If no healthy pool found, try to create new connections
    // Start from current index and try each node once
    let start_idx = self.rr_index;
    loop {
        let idx = self.rr_index % self.node_order.len();
        let node_id = self.node_order[idx].clone();
        self.rr_index = (self.rr_index + 1) % self.node_order.len();

        if let Some(pool) = self.pools.get_mut(&node_id) {
            if let Some(conn_id) = pool.checkout() {  // BUG: no healthy_count() check
                return Some((node_id, conn_id));
            }
        }

        if self.rr_index == start_idx {
            break;
        }
    }

    None
}
```

**Fix:** Remove the second fallback loop entirely. The first loop already handles all
nodes with healthy connections (including creating new ones via `NodeConnPool::checkout()`
when under max capacity). The second loop's only effect is to create connections on
empty nodes (no pre-existing connections), which is wrong.

**Required fix:** Replace the entire `checkout` method body with just the first loop:
```rust
pub fn checkout(&mut self) -> Option<(String, u64)> {
    if self.node_order.is_empty() {
        return None;
    }

    // Try each node in round-robin order starting from current index
    let mut attempts = 0;
    while attempts < self.node_order.len() {
        let idx = self.rr_index % self.node_order.len();
        let node_id = self.node_order[idx].clone();
        self.rr_index = (self.rr_index + 1) % self.node_order.len();
        attempts += 1;

        if let Some(pool) = self.pools.get_mut(&node_id) {
            if pool.healthy_count() > 0 {
                if let Some(conn_id) = pool.checkout() {
                    return Some((node_id, conn_id));
                }
            }
        }
    }

    None
}
```

---

## Instructions

Make only these two minimal changes. Do not change any other code. Show the
modified sections with at least 3 lines of context before and after each change.

After making the changes, verify by running: `cargo test -p claudefs-gateway`
