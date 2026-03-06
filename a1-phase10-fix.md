# A1: Storage Engine — Phase 10 Fix: Compilation Errors

## Status
Phase 10 modules were created but have compilation errors that need fixing:
- `request_deduplication.rs` — Use of moved value (key moves on first call)
- `command_queueing.rs` — Check and fix any similar issues
- `device_timeout_handler.rs` — Check and fix any similar issues
- `io_scheduler_fairness.rs` — Unused variable warnings

## Errors to Fix

### 1. request_deduplication.rs

**Issue:** The `read_deduplicated()` function takes ownership of `key: ReadKey`, causing tests to fail when reusing the same key multiple times.

**Current signature (WRONG):**
```rust
pub async fn read_deduplicated<F>(&self, key: ReadKey, mut fetch_fn: F) -> Result<Vec<u8>, String>
```

**Required fix:**
- Change parameter to `&ReadKey` (borrow instead of own)
- Update the DashMap::entry() call to work with `&ReadKey`
- Ensure ReadKey is Copy or that the signature uses references

**Test pattern:**
```rust
let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
// Should be able to call multiple times with same key:
dedup.read_deduplicated(key.clone(), || Ok(vec![1])).await;
dedup.read_deduplicated(key, || Ok(vec![2])).await;
```

### 2. command_queueing.rs

**Check for:**
- CoreId, QueuePairId imports — remove unused imports (NsId, QueueState)
- All test patterns using VecDeque and command structs
- Make sure no moved values in tests

### 3. device_timeout_handler.rs

**Check for:**
- All function signatures are correct
- No moved values in tests

### 4. io_scheduler_fairness.rs

**Check for:**
- Unused imports (Arc at line 4)
- Unused variable `background_count` at line 305 (prefix with `_`)

## Required Changes

**For request_deduplication.rs:**

1. Ensure ReadKey implements Copy + Clone (it does, looks good)

2. Change `read_deduplicated` signature from:
   ```rust
   pub async fn read_deduplicated<F>(&self, key: ReadKey, mut fetch_fn: F) -> Result<Vec<u8>, String>
   ```

   To:
   ```rust
   pub async fn read_deduplicated<F>(&self, key: &ReadKey, mut fetch_fn: F) -> Result<Vec<u8>, String>
   ```

3. In the function body:
   - `self.inflight.entry(key.clone())` or just `self.inflight.entry(*key)` since ReadKey is Copy

4. The `invalidate()` function signature at line around 110 should already be `&ReadKey` — verify

5. All tests should compile without cloning keys when calling read_deduplicated

**For all modules:**
- Fix all unused imports by removing them
- Prefix unused variables with `_`
- Verify all function signatures match test call patterns

## Files to Fix

All in: `crates/claudefs-storage/src/`

1. `request_deduplication.rs` — Fix key parameter to use reference
2. `command_queueing.rs` — Remove unused imports (NsId, QueueState)
3. `device_timeout_handler.rs` — Verify no issues
4. `io_scheduler_fairness.rs` — Remove unused Arc import, prefix background_count with `_`
5. `lib.rs` — Verify all modules are exported

## Success Criteria

After fixes:
- `cargo check -p claudefs-storage` produces zero errors
- All warnings are either allowed unused or justifiable (debug derives, etc.)
- `cargo test -p claudefs-storage --lib` runs without errors
- Tests compile and run (don't need to pass, just compile)

## Notes

- ReadKey is Copy + Clone, so passing by reference should work everywhere
- Use `*key` to dereference when needed for DashMap operations
- Keep existing test structure, just ensure signatures match call patterns
