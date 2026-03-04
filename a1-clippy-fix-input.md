# Fix: Remove unused import in nvme_passthrough.rs

## File to fix
`/home/cfs/claudefs/crates/claudefs-storage/src/nvme_passthrough.rs`

## Problem
Clippy reports:
```
warning: unused import: `thiserror::Error`
 --> crates/claudefs-storage/src/nvme_passthrough.rs:5:5
  |
5 | use thiserror::Error;
```

## Fix
Remove the `use thiserror::Error;` line from `nvme_passthrough.rs`.

## Instructions
1. Read `/home/cfs/claudefs/crates/claudefs-storage/src/nvme_passthrough.rs`
2. Find and remove the line `use thiserror::Error;` (around line 5)
3. Write the updated file
4. Do NOT change anything else
