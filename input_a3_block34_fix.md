# ClaudeFS Phase 32 Blocks 3-4 Compilation Fixes

## Context
OpenCode previously generated two test files that have compilation errors:
1. `crates/claudefs-reduce/tests/cluster_multinode_dedup.rs` (35 KB, 20 tests)
2. `crates/claudefs-reduce/tests/cluster_tiering_s3_consistency.rs` (34 KB, 16 tests)

Both files compile successfully as libraries, but the test functions have structural issues.

## Issues to Fix

### Block 3 (cluster_multinode_dedup.rs)
**Error:** Type mismatch in `write_from_node` function (line 229)
```
error[E0308]: mismatched types
fn write_from_node(node_ip: &str, path: &str, size_mb: usize) -> Result<(), String>
ssh_exec() returns ClusterResult<String>, not Result<(), String>
```

The function calls `ssh_exec()` which returns `ClusterResult<String>` (defined as `Result<String, ClusterError>`).
The function signature promises `Result<(), String>`.

**Fix Strategy:**
- Change return type to `ClusterResult<String>` for functions that call `ssh_exec()`
- OR convert `ClusterResult<String>` → `Result<(), String>` where needed
- Review all helper functions in Block 3 for similar issues

### Block 4 (cluster_tiering_s3_consistency.rs)
**Errors:** Multiple issues
1. Type mismatch in return type for functions using `?` operator
2. Inner attribute after outer doc comment
3. Temporary value borrowed issues (E0716)

**Example:**
```
error[E0277]: the `?` operator can only be used in a function that returns `Result` or `Option`
fn test_cluster_tiering_metadata_consistency_s3() -> Result<(), Box<dyn std::error::Error>>
```

Functions that use `?` must properly propagate ClusterResult types, not generic Result types.

**Fix Strategy:**
- Import `ClusterResult` and `ClusterError` from `cluster_helpers`
- Change return types to `ClusterResult<()>` instead of `Result<(), Box<dyn std::error::Error>>`
- Fix temporary value lifetime issues (likely missing `.clone()` or ref adjustments)
- Remove conflicting doc comments and attributes

## Required Changes Summary

1. **Imports at top of both files:**
   - Verify `use crate::cluster_helpers::{ClusterResult, ClusterError};` is present
   - Add if missing

2. **Block 3 (multinode_dedup.rs):**
   - Review all helper function signatures
   - Ensure functions using `ssh_exec()` return `ClusterResult<String>` or handle conversion
   - Check `write_from_node`, `read_from_node`, similar I/O functions

3. **Block 4 (tiering_s3_consistency.rs):**
   - Change all test function returns to `ClusterResult<()>`
   - Remove generic `Box<dyn std::error::Error>` returns
   - Fix temporary value borrows by storing in variables when needed
   - Verify attribute comments don't conflict

4. **Both files:**
   - All tests should still be marked `#[ignore]` (cluster-only)
   - Should compile with `cargo test -p claudefs-reduce --test cluster_multinode_dedup`
   - Should compile with `cargo test -p claudefs-reduce --test cluster_tiering_s3_consistency`

## Output Format
Return corrected .rs files that compile cleanly.

## Reference
- `cluster_helpers.rs`: Contains `ClusterResult` type, `ssh_exec()`, `get_client_nodes()`, etc.
- Use pattern: `ssh_exec(...)?` naturally propagates `ClusterResult`
- Test functions: `#[test]` and `#[ignore]` attributes, `fn test_name() -> ClusterResult<()>`
