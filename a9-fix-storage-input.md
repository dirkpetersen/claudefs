# Fix: claudefs-storage hot_swap.rs — missing BlockId and BlockSize imports in test module

## Problem

The file `crates/claudefs-storage/src/hot_swap.rs` has a test module that uses
`BlockId::new(device_idx, offset)` and `BlockSize::B4K` at lines 545–546, but
these types are not imported in the test module. The main module only imports
`crate::block::BlockRef`, not `BlockId` or `BlockSize`.

The `use super::*` in the test module does not pull in `BlockId` or `BlockSize`
because they are not re-exported at the module level.

## Compile error

```
error[E0433]: failed to resolve: use of undeclared type `BlockId`
   --> crates/claudefs-storage/src/hot_swap.rs:545:17
    |
545 |             id: BlockId::new(device_idx, offset),
    |                 ^^^^^^^ use of undeclared type `BlockId`

error[E0433]: failed to resolve: use of undeclared type `BlockSize`
   --> crates/claudefs-storage/src/hot_swap.rs:546:19
    |
546 |             size: BlockSize::B4K,
    |                   ^^^^^^^^^ use of undeclared type `BlockSize`
```

## Required fix

In `crates/claudefs-storage/src/hot_swap.rs`, find the `#[cfg(test)]` test
module and add an explicit import for `BlockId` and `BlockSize`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSize};  // <-- ADD THIS LINE
```

## Important constraints

- Only modify the `#[cfg(test)]` test module in `hot_swap.rs`
- Do NOT change any other code
- Do NOT add any new functions or tests
- The fix is exactly one line added inside the `#[cfg(test)] mod tests { ... }` block

## File location

`crates/claudefs-storage/src/hot_swap.rs`

Please output the complete fixed file content with the single import line added.
