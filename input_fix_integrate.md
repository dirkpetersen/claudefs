# Fix dedup_bloom.rs compilation errors and integrate modules into lib.rs

## Working directory
`/home/cfs/claudefs`

## Context
The `claudefs-reduce` crate has 1303 passing tests. Three module files exist:
- `crates/claudefs-reduce/src/dedup_bloom.rs` — has compilation errors, needs fixing
- `crates/claudefs-reduce/src/journal_replay.rs` — ready but not in lib.rs
- `crates/claudefs-reduce/src/namespace_tree.rs` — declared in lib.rs but no pub use exports

## Task 1: Fix crates/claudefs-reduce/src/dedup_bloom.rs

The file has two bugs:

### Bug A: Syntax error in test (~line 249)
Current broken code:
```rust
        for i in 0..50 {
            let hash: [u8; i as u8; 32] = [i; 32];
```
Fix to:
```rust
        for i in 0u8..50u8 {
            let hash: [u8; 32] = [i; 32];
```

### Bug B: Mutability error in may_contain and definitely_absent
The methods `may_contain` and `definitely_absent` take `&self` but try to mutate `self.stats`.
Change them to `&mut self`:

```rust
    pub fn may_contain(&mut self, hash: &[u8; 32]) -> bool {
```

```rust
    pub fn definitely_absent(&mut self, hash: &[u8; 32]) -> bool {
        !self.may_contain(hash)
    }
```

The `stats()` and `estimated_fill_ratio()` methods keep their `&self` signatures.

After fixing, verify the test `may_contain` calls work — all tests already use `let mut bloom`.

## Task 2: Edit crates/claudefs-reduce/src/lib.rs

### 2a: Add module declarations
In the alphabetical list of `pub mod` declarations, add:
```rust
pub mod dedup_bloom;
pub mod journal_replay;
```
Place `dedup_bloom` after the existing `pub mod data_classifier;` line.
Place `journal_replay` after the existing `pub mod journal_segment;` line.

### 2b: Add pub use exports
At the end of lib.rs (after the last `pub use` statement), add:
```rust
pub use dedup_bloom::{BloomConfig, BloomStats, DedupBloom};
pub use journal_replay::{
    InodeReplayState, JournalReplayer, ReplayAction, ReplayConfig, ReplayState, ReplayStats,
};
pub use namespace_tree::{DirEntry, DirId, NamespaceTree};
```

## Important
- Do NOT add doc comments or `#[allow(...)]` attributes
- Keep code minimal and clean
- After writing the files, run:
  ```bash
  cd /home/cfs/claudefs && cargo test -p claudefs-reduce 2>&1 | tail -10
  ```
  to verify the tests pass. Fix any remaining compilation errors.
