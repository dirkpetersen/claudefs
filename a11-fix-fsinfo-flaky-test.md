# Fix flaky fsinfo test

## Problem

The test `stats_age_secs_reflects_elapsed` in `crates/claudefs-fuse/src/fsinfo.rs:409` is timing-sensitive:

```
#[test]
fn stats_age_secs_reflects_elapsed() {
    let mut cache = FsInfoCache::new(FsInfoConfig::default());
    cache.update(make_stats());
    std::thread::sleep(Duration::from_millis(1100));
    let age = cache.stats().age_secs;
    assert!(age >= 1);
}
```

**Failure:** On some systems, even after sleeping 1100ms, the measured age is less than 1 second due to rounding or timing precision issues.

**Root cause:** The comparison `age >= 1` is checking integer seconds. If the cache stores microseconds or has rounding issues, the age might round down to 0.

## Solution

Fix the test to be more robust:

1. **Option A (Recommended):** Increase sleep to 1500ms or 2000ms to ensure we're well above the 1-second boundary
2. **Option B:** Change assertion to check `>= 0` if age is counted, OR
3. **Option C:** Check that age is "reasonably close to expected" (e.g., `age >= 1 && age <= 2`)

Also check if `age_secs` is computed correctly in `FsInfoCache::stats()`. It should be `(now - last_update_time).as_secs()`.

## Implementation Requirements

- Make the test reliable on all systems (no timing flakiness)
- Keep the intent of the test (verify that elapsed time is reflected in age_secs)
- All other fsinfo tests should continue passing

## Expected Result

```bash
cargo test -p claudefs-fuse --lib fsinfo::tests::stats_age_secs_reflects_elapsed
# Should show: test result: ok
```

And the entire test suite should pass without flakiness.
