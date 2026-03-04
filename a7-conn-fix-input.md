# Task: Fix two compile errors in claudefs-gateway

## Error 1: gateway_conn_pool.rs — Instant doesn't implement Serialize/Deserialize

**File:** `crates/claudefs-gateway/src/gateway_conn_pool.rs`

The structs `ConnState` and `PooledConn` have `#[derive(Serialize, Deserialize)]` applied, but they
contain `std::time::Instant` fields. `Instant` does not implement `serde::Serialize` or
`serde::Deserialize`.

**Fix:** Remove `Serialize, Deserialize` from the derives on `ConnState` and `PooledConn`. These types
are runtime state, not serialized to disk, so removing serde is correct. Keep `Debug` and `Clone`.

Specific changes needed:

1. Line 47: Change `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]` on `ConnState`
   to `#[derive(Debug, Clone, PartialEq, Eq)]`

2. Line 58: Change `#[derive(Debug, Clone, Serialize, Deserialize)]` on `PooledConn`
   to `#[derive(Debug, Clone)]`

Also, the imports `info` and `warn` from tracing are unused. Remove them:
- Line 7: Change `use tracing::{debug, info, warn};` to `use tracing::debug;`

## Error 2: nfs_copy_offload.rs — Wrong use of tuple variant constructor

**File:** `crates/claudefs-gateway/src/nfs_copy_offload.rs`

`CopyOffloadError::NotFound` is defined as `NotFound(String)` — a tuple variant requiring a String
argument. The code passes the constructor function itself instead of an instance.

Lines 220 and 239:
```rust
.ok_or(CopyOffloadError::NotFound)?;
```

This is wrong because `CopyOffloadError::NotFound` is a function pointer, not an instance.

**Fix:** Change both occurrences to use `.ok_or_else()` with the copy_id:

Line 220 (in `complete_copy` which takes `copy_id: u64`):
```rust
.ok_or_else(|| CopyOffloadError::NotFound(format!("copy id {}", copy_id)))?;
```

Line 239 (in `fail_copy` which also takes `copy_id: u64`):
```rust
.ok_or_else(|| CopyOffloadError::NotFound(format!("copy id {}", copy_id)))?;
```

## Instructions

1. Read both files completely first
2. Apply the minimal fixes described above
3. Do NOT change any other code, tests, logic, or documentation
4. After fixing, run `cargo check -p claudefs-gateway` to verify the build is clean
5. If there are remaining warnings about unused imports or trivially_copy_pass_by_ref, just note them
   but do not fix them (they are non-critical warnings)
6. The goal is zero compile errors, not zero warnings
