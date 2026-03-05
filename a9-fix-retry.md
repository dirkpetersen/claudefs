# Quick Fix: Flaky Test Sleep Duration

File: crates/claudefs-fuse/src/fsinfo.rs
Test: fsinfo::tests::stats_age_secs_reflects_elapsed

Current code (line 407):
```rust
std::thread::sleep(Duration::from_millis(100));
```

Should be:
```rust
std::thread::sleep(Duration::from_millis(1100));
```

The assertion at line 409 checks `age >= 1` second, but current sleep is only 100ms.

Simple 1-line change. Duration 100 → 1100.
