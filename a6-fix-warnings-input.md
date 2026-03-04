# Fix compile error and warnings in claudefs-repl

You are a Rust expert fixing a compile error and warnings in the `claudefs-repl` crate of the ClaudeFS project.

## Compile Error (MUST FIX)

File: `crates/claudefs-repl/src/journal.rs`, approximately lines 576-577

```
error[E0382]: borrow of partially moved value: `decoded`
   --> crates/claudefs-repl/src/journal.rs:577:26
    |
576 |             prop_assert_eq!(decoded.payload, payload);
    |                             --------------- value partially moved here
577 |             prop_assert!(decoded.validate_crc());
    |                          ^^^^^^^ value borrowed here after partial move
```

The current code looks like:
```rust
prop_assert_eq!(decoded.seq, seq);
prop_assert_eq!(decoded.shard_id, shard_id);
prop_assert_eq!(decoded.site_id, site_id);
prop_assert_eq!(decoded.timestamp_us, timestamp_us);
prop_assert_eq!(decoded.inode, inode);
prop_assert_eq!(decoded.op, op);
prop_assert_eq!(decoded.payload, payload);   // line 576 — moves decoded.payload
prop_assert!(decoded.validate_crc());         // line 577 — ERROR: decoded partially moved
```

**Fix:** Move the `prop_assert!(decoded.validate_crc());` call BEFORE the `prop_assert_eq!(decoded.payload, payload);` line.

So the corrected order should be:
```rust
prop_assert_eq!(decoded.seq, seq);
prop_assert_eq!(decoded.shard_id, shard_id);
prop_assert_eq!(decoded.site_id, site_id);
prop_assert_eq!(decoded.timestamp_us, timestamp_us);
prop_assert_eq!(decoded.inode, inode);
prop_assert_eq!(decoded.op, op);
prop_assert!(decoded.validate_crc());         // moved BEFORE payload comparison
prop_assert_eq!(decoded.payload, payload);
```

## Warnings to Fix

### 1. `batch_auth.rs` line 430: unused `mut`
```
warning: variable does not need to be mutable
   --> crates/claudefs-repl/src/batch_auth.rs:430:13
    |
430 |         let mut a: [u8; 32] = [0x55; 32];
```
**Fix:** Change `let mut a` to `let a`.

### 2. `failover.rs` lines 650, 693, 713: unused `events` variables
```
warning: unused variable: `events`
   --> crates/claudefs-repl/src/failover.rs:650:13
650 |         let events = manager.record_health(100, false).await;
693 |         let events = manager.record_health(100, true).await;
713 |         let events = manager.record_health(100, false).await;
```
**Fix:** Change `let events =` to `let _events =` on all three lines (650, 693, 713).

### 3. `repl_maintenance.rs` line 300: unused `i` in for loop
```
warning: unused variable: `i`
   --> crates/claudefs-repl/src/repl_maintenance.rs:300:13
300 |         for i in 1..=5 {
```
**Fix:** Change `for i in` to `for _i in`.

## Instructions

Please provide the exact content of the modified lines for each file. Show:
1. The file path
2. The exact old line(s) to be replaced
3. The exact new line(s) to replace with

These are all test-code fixes only (inside `#[cfg(test)]` or `#[test]` blocks).
