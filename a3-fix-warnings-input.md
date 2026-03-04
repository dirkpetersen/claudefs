# Fix Compiler Warnings in claudefs-reduce

## Context

You are working on the `claudefs-reduce` crate in a Cargo workspace at `/home/cfs/claudefs`.

This is a data reduction library (dedupe/compress/encrypt) for a distributed filesystem.

## Task

Fix ALL compiler warnings in the `claudefs-reduce` crate. The warnings are:

### 1. `crates/claudefs-reduce/src/write_path.rs` — unused imports in test module

Lines 182–184 in the `#[cfg(test)]` module:
```rust
use crate::compression::compress;           // WARNING: unused
use crate::meta_bridge::{BlockLocation, LocalFingerprintStore, NullFingerprintStore};
//                       ^^^^^^^^^^^^^ WARNING: BlockLocation unused
```

Fix: Remove `compress` from the use statement entirely. Remove `BlockLocation` from the import.
The corrected line should be:
```rust
use crate::meta_bridge::{LocalFingerprintStore, NullFingerprintStore};
```
(remove the entire `use crate::compression::compress;` line and remove `BlockLocation` from the meta_bridge import)

### 2. `crates/claudefs-reduce/src/write_path.rs` line ~248 — unused variable

```rust
let result1 = write_path.process_write(&data).unwrap();
```
Fix: rename to `let _result1 = ...`

### 3. `crates/claudefs-reduce/src/async_meta_bridge.rs` line ~473 — unused variable

```rust
let result1 = write_path.process_write(&data).await.unwrap();
```
Fix: rename to `let _result1 = ...`

### 4. `crates/claudefs-reduce/src/meta_bridge.rs` line ~264 — unused variable

```rust
let location = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
```
Fix: rename to `let _location = ...`

### 5. `crates/claudefs-reduce/src/recompressor.rs` line ~219 — unused variable

```rust
let (improved, stats) = recompressor.recompress_batch(&chunks);
```
Fix: rename to `let (_improved, stats) = ...`

### 6. `crates/claudefs-reduce/src/worm_reducer.rs` — multiple unused `must_use` results

These are calls to `reducer.register(...)` whose `Result` return values are not handled.

Lines 229, 245, 257, 258, and ~267 in the test module.

Fix: Change each bare `reducer.register(...)` call to `let _ = reducer.register(...)`.

## Instructions

For EACH file, read the FULL file content first, then make ONLY the minimal changes needed to fix the warnings. Do NOT refactor, reorganize, or change any logic. Only:
- Remove/fix unused imports
- Rename unused variables by prefixing with `_`
- Add `let _ =` to handle unused `must_use` results

Output the complete corrected content of each modified file.

## Files to modify

1. `/home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs`
2. `/home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs`
3. `/home/cfs/claudefs/crates/claudefs-reduce/src/meta_bridge.rs`
4. `/home/cfs/claudefs/crates/claudefs-reduce/src/recompressor.rs`
5. `/home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs`

For each file, output:
```
=== FILE: <path> ===
<complete file content>
=== END FILE ===
```
