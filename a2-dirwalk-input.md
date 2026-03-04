# A2: Implement dir_walk.rs — Recursive Directory Tree Walker

## Context

You are implementing a new module for the `claudefs-meta` crate (`crates/claudefs-meta/`).

The crate uses:
- `thiserror` for errors
- `serde` + `bincode` for serialization
- `bincode` for serialization
- `tracing` for logging
- Tests: standard `#[test]` in `#[cfg(test)] mod tests`

## Existing types you can use

From `types.rs`:
```rust
pub struct InodeId(u64); // newtype with InodeId::new(u64) and as_u64()
pub struct InodeAttr { pub ino: InodeId, pub file_type: FileType, pub mode: u32, pub nlink: u32, pub uid: u32, pub gid: u32, pub size: u64, pub blocks: u64, /* ... */ }
pub enum FileType { Directory, RegularFile, Symlink, /* ... */ }
pub struct DirEntry { pub name: String, pub ino: InodeId, pub file_type: FileType }
pub enum MetaError { NotFound(String), AlreadyExists(String), PermissionDenied, KvError(String), /* ... */ }
pub const ROOT_INODE: InodeId = InodeId(1); // from InodeId::ROOT_INODE
```

Note: `InodeId::ROOT_INODE` is defined as `InodeId::new(1)`.

From `directory.rs`:
```rust
pub struct DirectoryStore;
impl DirectoryStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self;
    pub fn list_dir(&self, dir_ino: InodeId) -> Result<Vec<DirEntry>, MetaError>;
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<Option<InodeId>, MetaError>;
}
```

From `inode.rs`:
```rust
pub struct InodeStore;
impl InodeStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self;
    pub fn get_attr(&self, ino: InodeId) -> Result<InodeAttr, MetaError>;
}
```

## Task: Implement `crates/claudefs-meta/src/dir_walk.rs`

Create a recursive directory tree walker with:

```rust
/// Configuration for directory walks.
pub struct WalkConfig {
    /// Maximum recursion depth (0 = root only, u32::MAX = unlimited).
    pub max_depth: u32,
    /// Whether to follow symlinks during traversal.
    pub follow_symlinks: bool,
    /// Whether to call the visitor on directories before their children (pre-order).
    /// If false, post-order (children visited first).
    pub pre_order: bool,
}

impl Default for WalkConfig {
    fn default() -> Self {
        Self { max_depth: u32::MAX, follow_symlinks: false, pre_order: true }
    }
}

/// A single node visited during a directory walk.
pub struct WalkEntry {
    /// The inode ID.
    pub ino: InodeId,
    /// The name of this entry in its parent directory.
    pub name: String,
    /// The parent inode ID (InodeId::ROOT_INODE for the root).
    pub parent_ino: InodeId,
    /// The file type.
    pub file_type: FileType,
    /// Current depth (0 for the walk root).
    pub depth: u32,
    /// Full path from the walk root (e.g., "subdir/file.txt").
    pub path: String,
}

/// Statistics accumulated during a walk.
#[derive(Clone, Default, Debug)]
pub struct WalkStats {
    /// Total directories visited.
    pub dirs: u64,
    /// Total regular files visited.
    pub files: u64,
    /// Total symlinks visited.
    pub symlinks: u64,
    /// Total other file types visited.
    pub other: u64,
    /// Maximum depth reached.
    pub max_depth_reached: u32,
}

impl WalkStats {
    /// Returns total number of inodes visited.
    pub fn total_inodes(&self) -> u64;
}

/// A directory tree walker.
pub struct DirWalker {
    dir_store: DirectoryStore,
    config: WalkConfig,
}

impl DirWalker {
    /// Creates a new DirWalker with the given directory store and config.
    pub fn new(dir_store: DirectoryStore, config: WalkConfig) -> Self;

    /// Walks the directory tree starting from `root_ino`, calling `visitor`
    /// for each entry (including the root itself if it's a directory).
    ///
    /// The visitor receives `(entry: &WalkEntry)` and can return `WalkControl`.
    /// Walk stops early if visitor returns `WalkControl::Stop`.
    ///
    /// Returns `WalkStats` with aggregate counts.
    pub fn walk<F>(&self, root_ino: InodeId, root_name: &str, visitor: &mut F) -> Result<WalkStats, MetaError>
    where
        F: FnMut(&WalkEntry) -> WalkControl;

    /// Collects all inodes reachable from `root_ino` up to `max_depth`.
    /// Returns (ino, path, file_type) tuples in visit order.
    pub fn collect_all(&self, root_ino: InodeId) -> Result<Vec<WalkEntry>, MetaError>;

    /// Counts all inodes in the subtree rooted at `root_ino` by file type.
    pub fn count_by_type(&self, root_ino: InodeId) -> Result<WalkStats, MetaError>;
}

/// Controls whether walking continues after visiting a node.
#[derive(Debug, PartialEq, Eq)]
pub enum WalkControl {
    /// Continue walking.
    Continue,
    /// Skip the subtree below this directory (only meaningful for directories).
    SkipSubtree,
    /// Stop the entire walk.
    Stop,
}
```

## Implementation Notes

- The walker uses `DirectoryStore::list_dir` to enumerate children
- For each child `DirEntry`, determine if it's a `FileType::Directory`:
  - If yes, recurse (unless depth exceeded or SkipSubtree returned)
  - If no, just call visitor and continue
- Path construction: join parent path + "/" + entry name
- Handle cycles: use a `HashSet<u64>` of visited inode IDs (store as u64 via as_u64())
  - If we revisit an inode, skip it with a tracing::warn
- Pre-order: visit directory before its children
- Post-order: visit directory after its children

## Required Tests (13 tests)

Write these tests in a `#[cfg(test)] mod tests { ... }` block at the bottom of the file.
Tests must use the existing `DirectoryStore` and set up mock directories using
`Arc<MemoryKvStore>` and the existing `DirectoryStore::add_entry` method.

**Setup helper** to use across tests:
```rust
fn make_walker(entries: Vec<(InodeId, Vec<(String, InodeId, FileType)>)>) -> DirWalker {
    // Creates DirectoryStore backed by MemoryKvStore
    // For each (dir_ino, children), calls dir_store.add_entry(dir_ino, DirEntry { name, ino, file_type })
    // Returns DirWalker with default WalkConfig
}
```

To check if `DirectoryStore` has an `add_entry` method, read the source. If it doesn't exist,
use `kv.put(...)` directly with the directory key format used in directory.rs.

Tests:
1. `test_walk_empty_dir` — walk empty directory, stats: dirs=1, files=0
2. `test_walk_single_file` — dir with one file, stats: dirs=1, files=1, total=2
3. `test_walk_nested_dirs` — root → subdir → file, stats: dirs=2, files=1
4. `test_walk_max_depth_zero` — max_depth=0 visits only root dir
5. `test_walk_max_depth_one` — max_depth=1 visits root and direct children
6. `test_walk_skip_subtree` — return SkipSubtree for a subdir, its children not visited
7. `test_walk_stop` — return Stop after first entry, only one entry visited
8. `test_collect_all_returns_entries` — collect_all on tree returns all entries
9. `test_count_by_type_mixed` — mixed files/dirs/symlinks, correct counts
10. `test_walk_symlinks_not_followed` — follow_symlinks=false, symlinks counted but not traversed
11. `test_walk_pre_order` — verify parent dir visited before children
12. `test_walk_post_order` — verify children visited before parent dir
13. `test_walk_stats_max_depth_reached` — check max_depth_reached in stats

## Important

- All tests must pass with `cargo test -p claudefs-meta dir_walk`
- No unused imports, no dead code warnings
- Every public item must have a `///` doc comment
- Do NOT modify lib.rs — I will update it separately
- Write the file directly to `crates/claudefs-meta/src/dir_walk.rs`

## Checking DirectoryStore API first

Before writing the implementation, read `crates/claudefs-meta/src/directory.rs` to see:
1. What methods exist on DirectoryStore
2. The key format for directory entries (to set up tests manually if needed)
3. How DirEntry is constructed

This is critical to get the test setup right.
