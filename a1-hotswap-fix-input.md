# Fix: Missing imports in hot_swap.rs test module

## File to fix
`/home/cfs/claudefs/crates/claudefs-storage/src/hot_swap.rs`

## Problem
The test module inside `hot_swap.rs` uses `BlockId` and `BlockSize` types but they are not imported.
Compilation fails with:
```
error[E0433]: failed to resolve: use of undeclared type `BlockId`
   --> crates/claudefs-storage/src/hot_swap.rs:545:17
    |
545 |             id: BlockId::new(device_idx, offset),

error[E0433]: failed to resolve: use of undeclared type `BlockSize`
   --> crates/claudefs-storage/src/hot_swap.rs:546:19
    |
546 |             size: BlockSize::B4K,
```

## The module-level imports at top of hot_swap.rs are:
```rust
use crate::block::BlockRef;
use crate::device::DeviceRole;
use crate::error::{StorageError, StorageResult};
```

Only `BlockRef` is imported from `crate::block`, but `BlockId` and `BlockSize` are also in `crate::block`.

## Fix required
The `#[cfg(test)]` mod tests block starts around line 539 with `use super::*;`.
Since `super::*` only re-exports what was imported into the outer module, the test code
can't see `BlockId` or `BlockSize`.

Add `use crate::block::{BlockId, BlockSize};` to the test module's imports, right after `use super::*;`.

Find this exact block in the file:
```rust
#[cfg(test)]
mod tests {
    use super::*;
```

And change it to:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSize};
```

That's the only change needed. Do NOT change anything else in the file.

## Instructions
1. Read the file `/home/cfs/claudefs/crates/claudefs-storage/src/hot_swap.rs`
2. Find the `#[cfg(test)]` mod tests block
3. Add `use crate::block::{BlockId, BlockSize};` after `use super::*;` in that test module
4. Write the complete updated file back
