# Fix: claudefs-gateway — two failing tests

## Bug 1: `s3_storage_class.rs` — `is_restored()` returns false for non-Glacier objects

### Problem

The `ObjectStorageState::is_restored()` method only returns `true` when
`restore_expiry` is set. But for non-Glacier storage classes (Standard, IA,
etc.), objects are always accessible — they don't need a restore operation.
The method should return `true` for objects whose storage class doesn't
`requires_restore()`.

### Failing test

```rust
fn test_object_storage_state_non_glacier() {
    let state = ObjectStorageState::new(StorageClass::Standard);
    assert!(!state.needs_restore());
    assert!(state.is_restored());  // FAILS — returns false
}
```

### Current implementation

```rust
pub fn is_restored(&self) -> bool {
    if let Some(expiry) = self.restore_expiry {
        std::time::SystemTime::now() < expiry && !self.is_restoring
    } else {
        false  // BUG: non-Glacier objects should be "always restored"
    }
}
```

### Required fix

```rust
pub fn is_restored(&self) -> bool {
    // Non-Glacier objects are always accessible — no restore needed
    if !self.current_class.requires_restore() {
        return true;
    }
    if let Some(expiry) = self.restore_expiry {
        std::time::SystemTime::now() < expiry && !self.is_restoring
    } else {
        false
    }
}
```

---

## Bug 2: `gateway_conn_pool.rs` — `checkout()` creates connections lazily instead of returning None

### Problem

`GatewayConnPool::checkout()` has a fallback loop that lazily calls
`NodeConnPool::checkout()` which in turn creates a new connection (since
`self.conns.len() < self.config.max_per_node`). This means checkout always
succeeds even when no connections were ever established.

The test expects that if no connections exist, checkout returns `None`:

### Failing test

```rust
fn test_no_healthy_nodes_returns_none() {
    let config = ConnPoolConfig::default();
    let mut gateway = GatewayConnPool::new(vec![make_node("node1")], config);
    // Don't add any connections
    let result = gateway.checkout();
    assert!(result.is_none());  // FAILS — lazily creates connection
}
```

### Current implementation of `GatewayConnPool::checkout()`

The method has two loops:
1. Round-robin loop that tries pools with `healthy_count() > 0`
2. Fallback loop that calls `pool.checkout()` directly (creating new connections)

The fallback loop #2 is what causes the bug.

### Required fix

Remove the fallback "create new connections" loop from `GatewayConnPool::checkout()`.
The method should only return connections from pools that already have healthy
connections. Connection establishment is a separate operation.

The fixed implementation should look like:

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

    None  // No healthy connections available
}
```

---

## Files to modify

1. `crates/claudefs-gateway/src/s3_storage_class.rs` — fix `is_restored()`
2. `crates/claudefs-gateway/src/gateway_conn_pool.rs` — fix `GatewayConnPool::checkout()`

## Important constraints

- Only make the minimal changes described above
- Do not add or remove any other tests or code
- Make sure all existing tests in these files continue to pass
- Output the complete modified implementations (both `is_restored()` and the
  updated `GatewayConnPool::checkout()` with the fallback loop removed)
