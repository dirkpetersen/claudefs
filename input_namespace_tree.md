# Implement namespace_tree.rs for ClaudeFS Reduction Crate

## Working Directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Context
You are implementing Phase 18 of the A3 (Data Reduction) agent for ClaudeFS.
The reduction crate currently has 1303 tests across 64 modules. Goal: ~1390 tests.

## Task
Create NEW FILE: `/home/cfs/claudefs/crates/claudefs-reduce/src/namespace_tree.rs`

Implement a lightweight namespace tree for tracking directory structure in the reduction layer.

The reduction layer needs to understand namespace hierarchy for:
- Per-directory dedup statistics
- Directory-level quota enforcement
- WORM policy inheritance from parent directories

## Requirements

### Types

1. **DirId newtype**: 
```rust
pub struct DirId(pub u64)
```
- Derive: Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize

2. **DirEntry struct**:
```rust
pub struct DirEntry {
    pub id: DirId,
    pub parent: Option<DirId>,
    pub name: String,
    pub child_count: usize,
    pub file_count: usize,
    pub bytes_used: u64,
}
```
- `fn is_root(&self) -> bool` → parent.is_none()
- Derive: Debug, Clone, Serialize, Deserialize

3. **NamespaceTree struct**:
- Internal: Use `HashMap<DirId, DirEntry>` for storage
- Methods:
  - `fn new() -> Self`
  - `fn add_dir(&mut self, id: DirId, parent: Option<DirId>, name: String)` — add directory entry (increment parent's child_count if parent exists)
  - `fn get(&self, id: DirId) -> Option<&DirEntry>`
  - `fn children(&self, parent: DirId) -> Vec<&DirEntry>` — direct children
  - `fn ancestors(&self, id: DirId) -> Vec<&DirEntry>` — path from id to root (not including id itself), in order from immediate parent to root
  - `fn update_usage(&mut self, id: DirId, bytes_delta: i64)` — update bytes_used (clamped to 0, saturating add/sub)
  - `fn record_file(&mut self, dir_id: DirId)` — increment file_count for dir and all ancestors
  - `fn remove_dir(&mut self, id: DirId) -> bool` — remove if no children; return true if removed, false otherwise
  - `fn dir_count(&self) -> usize`
  - `fn total_bytes(&self) -> u64` — sum of all dir bytes_used

## Required Tests (at least 17)

1. `dir_id_equality` — two DirIds with same value are equal
2. `new_tree_empty` — new NamespaceTree has dir_count 0
3. `add_root_dir` — add directory with no parent
4. `add_child_dir` — add child directory, parent's child_count increments
5. `get_dir_found` — get returns entry
6. `get_dir_not_found` — get returns None
7. `children_of_root` — returns direct children
8. `children_empty` — children of leaf directory returns empty
9. `ancestors_of_root` — empty (no ancestors)
10. `ancestors_of_child` — returns parent
11. `ancestors_deep_path` — 3-level path returns 2 ancestors in order (parent, grandparent)
12. `update_usage_positive` — bytes_used increases
13. `update_usage_negative_clamped` — subtracting more than available clamps to 0
14. `record_file_increments_file_count` — increments dir and ancestors
15. `remove_dir_no_children` — succeeds and returns true
16. `remove_dir_has_children_fails` — returns false if child_count > 0
17. `dir_count` — correct count after adds and removes

## Style
- Follow existing crate patterns (see lib.rs for imports style)
- Use `use serde::{Deserialize, Serialize};`
- Use `use std::collections::HashMap;`
- NO COMMENTS in code (per project style)
- Use `#[cfg(test)] mod tests { ... }` pattern

## Validation
After writing the file, verify it compiles:
```bash
cd /home/cfs/claudefs && cargo check -p claudefs-reduce
```
