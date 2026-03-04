# Fix Clippy and Compiler Warnings in claudefs-reduce Test Code

## Context

You are working on the `claudefs-reduce` crate in a Cargo workspace at `/home/cfs/claudefs`.
All 193 tests pass. Clippy on production code shows 0 warnings.
However, `cargo clippy --tests` reveals warnings in test code that need fixing.

## Files to Fix

### 1. `crates/claudefs-reduce/src/async_meta_bridge.rs`

Three `len_zero` warnings in test code. Fix by replacing:

**Line ~462:**
```rust
        assert!(segments.len() >= 1);
```
Change to:
```rust
        assert!(!segments.is_empty());
```

**Line ~495:**
```rust
        assert!(result.reduced_chunks.len() > 0);
```
Change to:
```rust
        assert!(!result.reduced_chunks.is_empty());
```

**Line ~507:**
```rust
        assert!(result.reduced_chunks.len() >= 1);
```
Change to:
```rust
        assert!(!result.reduced_chunks.is_empty());
```

### 2. `crates/claudefs-reduce/src/segment.rs`

One `filter_map_identity` warning in test code. Fix by replacing:

**Line ~354:**
```rust
            .filter_map(|s| s)
```
Change to:
```rust
            .flatten()
```

### 3. `crates/claudefs-reduce/src/write_path.rs`

Three `len_zero` warnings in test code. Fix by replacing:

**Line ~235:**
```rust
        assert!(segments.len() >= 1);
```
Change to:
```rust
        assert!(!segments.is_empty());
```

**Line ~272:**
```rust
        assert!(result.reduced_chunks.len() > 0);
```
Change to:
```rust
        assert!(!result.reduced_chunks.is_empty());
```

**Line ~299:**
```rust
        assert!(result.reduced_chunks.len() >= 1);
```
Change to:
```rust
        assert!(!result.reduced_chunks.is_empty());
```

### 4. `crates/claudefs-reduce/src/worm_reducer.rs`

Many warnings in test code. Apply these changes:

#### A. `clone_on_copy` at line ~493:
```rust
        let cloned = policy.clone();
```
Change to:
```rust
        let cloned = policy;
```

#### B. `unnecessary_cast` at line ~554:
```rust
                RetentionPolicy::immutable_until(i as u64)
```
Change to:
```rust
                RetentionPolicy::immutable_until(i)
```

#### C. `unnecessary_cast` AND `unused_must_use` at line ~556:
```rust
            reducer.register(i, policy, i as u64);
```
Change to:
```rust
            let _ = reducer.register(i, policy, i);
```

#### D. `unused_must_use` — Add `let _ =` to ALL bare `reducer.register(...)` calls in test code that don't already have it.

The following lines need `let _ =` added in front of them (approximate line numbers):

Line ~282: `reducer.register(1, RetentionPolicy::legal_hold(), 0);`
Line ~283: `reducer.register(2, RetentionPolicy::immutable_until(100), 0);`
Line ~292: `reducer.register(1, RetentionPolicy::immutable_until(100), 0);`
Line ~293: `reducer.register(2, RetentionPolicy::immutable_until(200), 0);`
Line ~294: `reducer.register(3, RetentionPolicy::immutable_until(300), 0);`
Line ~306: `reducer.register(1, RetentionPolicy::none(), 0);`
Line ~307: `reducer.register(2, RetentionPolicy::immutable_until(1000), 0);`
Line ~336: `reducer.register(1, RetentionPolicy::none(), 0);`
Line ~337: `reducer.register(2, RetentionPolicy::legal_hold(), 0);`
Line ~338: `reducer.register(3, RetentionPolicy::immutable_until(500), 0);`
Line ~339: `reducer.register(4, RetentionPolicy::immutable_until(1000), 0);`
Line ~359: `reducer.register(1, RetentionPolicy::legal_hold(), 0);`
Line ~360: `reducer.register(2, RetentionPolicy::immutable_until(100), 0);`
Line ~372: `reducer.register(i, RetentionPolicy::immutable_until(i * 100), 0);` (inside loop)
Line ~375: `reducer.register(11, RetentionPolicy::immutable_until(2000), 0);`
Line ~404: `reducer.register(1, RetentionPolicy::immutable_until(1000), 0);`
Line ~405: `reducer.register(2, RetentionPolicy::legal_hold(), 0);`
Line ~415: `reducer.register(1, RetentionPolicy::none(), 100);`
Line ~416: `reducer.register(1, RetentionPolicy::legal_hold(), 200);`
Line ~427: `reducer.register(1, RetentionPolicy::legal_hold(), 100);`
Line ~428: `reducer.register(1000000, RetentionPolicy::legal_hold(), 200);`
Line ~429: `reducer.register(u64::MAX, RetentionPolicy::legal_hold(), 300);`
Line ~456: `reducer.register(i, RetentionPolicy::immutable_until(ts), 0);` (inside loop)
Line ~470: `reducer.register(1, RetentionPolicy::immutable_until(100), 0);`
Line ~471: `reducer.register(2, RetentionPolicy::immutable_until(200), 0);`
Line ~525: `reducer.register(1, RetentionPolicy::none(), 0);`
Line ~526: `reducer.register(2, RetentionPolicy::none(), 0);`
Line ~536: `reducer.register(1, RetentionPolicy::legal_hold(), 0);`
Line ~537: `reducer.register(2, RetentionPolicy::legal_hold(), 0);`

NOTE: Some calls in the test module already have `let _ =` — do NOT add it again to those.
Lines that already have `let _ =` (do NOT change these):
- `let _ = reducer.register(make_hash(4), RetentionPolicy::legal_hold(), 0);` (~229)
- `let _ = reducer.register(100, RetentionPolicy::legal_hold(), 1024);` (~245)
- `let _ = reducer.register(1, RetentionPolicy::none(), 0);` (~257)
- `let _ = reducer.register(2, RetentionPolicy::legal_hold(), 0);` (~258)
- Loop with `let _ = reducer.register(make_hash(i), ...)` (~267-271)

## Instructions

For EACH file, read the FULL file content first, then make ONLY the minimal changes listed above.
Do NOT refactor, reorganize, or change any logic. Do NOT add comments. Do NOT change any test logic.
Only make the exact minimal changes described above.

After making changes, verify no new issues are introduced.

Output the complete corrected content of each modified file in this format:
```
=== FILE: <relative path from repo root> ===
<complete file content>
=== END FILE ===
```

Output all 4 files.
