# A2: Wire dir_walk.rs into lib.rs

## Task

Add the new `dir_walk` module to `crates/claudefs-meta/src/lib.rs`.

## What to add

In the `pub mod` declarations section, add `pub mod dir_walk;` in alphabetical order.
It should come after `pub mod directory;` and before `pub mod dirshard;`.

The current section looks like:
```rust
/// Directory metadata operations (create, rename, readdir, etc.)
pub mod directory;
/// Directory sharding (InifiniFS-style per-directory sharding)
pub mod dirshard;
```

Add after `pub mod directory;`:
```rust
/// Recursive directory tree walker (quota, fsck, backup, snapshot)
pub mod dir_walk;
```

Also add a re-export in the `pub use` section:
```rust
pub use dir_walk::{DirWalker, WalkConfig, WalkControl, WalkEntry, WalkStats};
```

Add this after the existing `pub use directory::...` re-exports.

## Instructions

Read `crates/claudefs-meta/src/lib.rs` first, then make ONLY these minimal insertions:
1. Insert `pub mod dir_walk;` after `pub mod directory;`
2. Insert `pub use dir_walk::{DirWalker, WalkConfig, WalkControl, WalkEntry, WalkStats};` in the pub use section

Do NOT modify any other lines.
