# A9: Fix Flaky Test in claudefs-fuse fsinfo.rs

## Problem
Test `fsinfo::tests::stats_age_secs_reflects_elapsed` is failing intermittently:
- Location: crates/claudefs-fuse/src/fsinfo.rs:403-410
- Issue: Sleep duration is 100ms but assertion expects age_secs >= 1 (1 second)
- The test is timing-sensitive and can fail on slower systems

## Current Code (fsinfo.rs:403-410)
```rust
#[test]
fn stats_age_secs_reflects_elapsed() {
    let mut cache = FsInfoCache::new(FsInfoConfig::default());
    cache.update(make_stats());
    std::thread::sleep(Duration::from_millis(100));
    let age = cache.stats().age_secs;
    assert!(age >= 1);  // Line 409 — PROBLEM: expects 1 second but only slept 100ms
}
```

## Fix
Change the sleep duration from 100ms to at least 1100ms (1.1 seconds) to ensure the assertion passes reliably on all systems.

## Requirements
- Use at least 1100ms sleep duration to guarantee age_secs >= 1
- Preserve all other test logic
- Ensure no other code changes
- File: crates/claudefs-fuse/src/fsinfo.rs
- Test should pass consistently after fix

## Expected Outcome
`cargo test -p claudefs-fuse --lib fsinfo::tests::stats_age_secs_reflects_elapsed` should pass consistently.
