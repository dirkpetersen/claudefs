# A9 Phase 2: Metadata Module Tests

Write one new test module `meta_phase2_tests.rs` for the `claudefs-tests` crate. It should test A2 (Metadata) Phase 2 modules.

## Public APIs to Test

### 1. `claudefs_meta::locking` — LockManager

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockType { Read, Write }

pub struct LockEntry {
    pub ino: InodeId,
    pub lock_type: LockType,
    pub holder: NodeId,
    pub lock_id: u64,
}

pub struct LockManager { /* private */ }

impl LockManager {
    pub fn new() -> Self;
    pub fn acquire(&self, ino: InodeId, lock_type: LockType, holder: NodeId) -> Result<u64, MetaError>;
    pub fn release(&self, lock_id: u64) -> Result<(), MetaError>;
    pub fn release_all_for_node(&self, node: NodeId) -> Result<usize, MetaError>;
    pub fn is_locked(&self, ino: InodeId) -> Result<bool, MetaError>;
    pub fn locks_on(&self, ino: InodeId) -> Result<Vec<LockEntry>, MetaError>;
}
```

### 2. `claudefs_meta::neg_cache` — NegativeCache

```rust
#[derive(Clone, Debug)]
pub struct NegCacheConfig {
    pub ttl: Duration,
    pub max_entries: usize,
    pub enabled: bool,
}
impl Default for NegCacheConfig;  // ttl=3s, max=8192, enabled=true

pub struct NegEntry {
    pub parent: InodeId,
    pub name: String,
    pub recorded_at: Instant,
}
impl NegEntry {
    pub fn is_expired(&self, ttl: Duration) -> bool;
}

pub struct NegCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub inserts: u64,
    pub invalidations: u64,
    pub expirations: u64,
}

pub struct NegativeCache { /* private */ }

impl NegativeCache {
    pub fn new(config: NegCacheConfig) -> Self;
    pub fn insert(&mut self, parent: InodeId, name: String);
    pub fn is_negative(&mut self, parent: &InodeId, name: &str) -> bool;
    pub fn invalidate(&mut self, parent: &InodeId, name: &str);
    pub fn invalidate_dir(&mut self, parent: &InodeId);
    pub fn entry_count(&self) -> usize;
    pub fn stats(&self) -> &NegCacheStats;
    pub fn hit_ratio(&self) -> f64;
}
```

### 3. `claudefs_meta::pathres` — PathResolver

```rust
pub struct PathCacheEntry {
    pub ino: InodeId,
    pub file_type: FileType,
    pub shard: ShardId,
}

pub struct NegativeCacheEntry {
    pub cached_at: Timestamp,
    pub ttl_secs: u64,
}

pub struct PathResolver { /* private */ }

impl PathResolver {
    pub fn new(num_shards: u16, max_entries: usize, negative_cache_ttl_secs: u64, max_negative_entries: usize) -> Self;
    pub fn parse_path(path: &str) -> Vec<String>;
    pub fn speculative_resolve(&self, path: &str) -> (Vec<PathCacheEntry>, Vec<String>);
    pub fn cache_resolution(&self, parent: InodeId, name: &str, entry: PathCacheEntry);
    pub fn invalidate_parent(&self, parent: InodeId);
    pub fn invalidate_entry(&self, parent: InodeId, name: &str);
    pub fn cache_size(&self) -> usize;
    pub fn clear_cache(&self);
    pub fn cache_negative(&self, parent: InodeId, name: &str);
    pub fn check_negative(&self, parent: InodeId, name: &str) -> bool;
    pub fn invalidate_negative(&self, parent: InodeId, name: &str);
    pub fn invalidate_negative_parent(&self, parent: InodeId);
    pub fn cleanup_expired_negative(&self) -> usize;
    pub fn negative_cache_size(&self) -> usize;
    pub fn resolve_path<F>(&self, path: &str, lookup_fn: F) -> Result<InodeId, MetaError>
        where F: Fn(InodeId, &str) -> Result<InodeId, MetaError>;
}
```

### Types from `claudefs_meta::types`

```rust
pub struct InodeId { /* private u64 */ }
impl InodeId {
    pub fn new(id: u64) -> Self;
    pub fn as_u64(&self) -> u64;
    pub const ROOT_INODE: InodeId;
}

pub struct NodeId { /* private u64 */ }
impl NodeId {
    pub fn new(id: u64) -> Self;
}

pub struct ShardId { /* private u16 */ }
impl ShardId {
    pub fn new(id: u16) -> Self;
    pub fn as_u16(&self) -> u16;
}

pub enum FileType {
    RegularFile, Directory, Symlink, BlockDevice, CharDevice, Fifo, Socket
}

pub enum MetaError {
    NotFound, PermissionDenied, NotADirectory(InodeId), EntryExists { parent, name },
    EntryNotFound { parent, name }, KvError(String), LockNotFound, ...
}
```

## Write `meta_phase2_tests.rs` with at least 55 tests

### Section 1: LockManager (20 tests)
1. `test_lock_manager_new` — creates without panic
2. `test_is_locked_empty` — new ino is not locked
3. `test_acquire_read_lock` — returns lock_id > 0
4. `test_acquire_write_lock` — returns lock_id > 0
5. `test_is_locked_after_acquire` — is_locked returns true
6. `test_locks_on_returns_entries` — locks_on returns the acquired lock
7. `test_lock_entry_fields` — lock entry has correct ino, lock_type, holder
8. `test_release_lock` — after release, is_locked returns false
9. `test_release_nonexistent_lock` — returns Err(LockNotFound)
10. `test_multiple_readers_allowed` — two read locks on same ino succeed
11. `test_writer_blocks_reader` — read lock fails when write lock held
12. `test_reader_blocks_writer` — write lock fails when read lock held
13. `test_two_writers_fail` — second write lock fails
14. `test_lock_id_increments` — successive locks get different IDs
15. `test_release_all_for_node` — releases all locks for a given node
16. `test_release_all_for_node_count` — returns correct count
17. `test_lock_different_inodes_independent` — lock on ino1 doesn't affect ino2
18. `test_locks_on_empty` — no locks on unlocked ino
19. `test_read_then_release_then_write_ok` — sequential use works
20. `prop_lock_release_leaves_clean` — proptest: acquire/release leaves is_locked=false

### Section 2: NegativeCache (20 tests)
1. `test_neg_cache_config_defaults` — ttl=3s, max=8192, enabled=true
2. `test_neg_cache_new` — creates without panic
3. `test_is_negative_empty` — fresh cache returns false for any entry
4. `test_insert_makes_negative` — after insert, is_negative returns true
5. `test_is_negative_different_parent` — different parent returns false
6. `test_is_negative_different_name` — same parent different name returns false
7. `test_stats_initial_zeros` — all stats are 0 initially
8. `test_insert_increments_inserts` — stats.inserts incremented after insert
9. `test_is_negative_hit_increments_hits` — stats.hits++ after cache hit
10. `test_is_negative_miss_increments_misses` — stats.misses++ after cache miss
11. `test_invalidate_removes_entry` — after invalidate, is_negative returns false
12. `test_invalidate_increments_invalidations` — stats.invalidations++
13. `test_invalidate_dir_removes_all_children` — insert two names, invalidate_dir removes both
14. `test_entry_count_after_insert` — entry_count increments
15. `test_entry_count_after_invalidate` — entry_count decrements
16. `test_hit_ratio_no_lookups` — hit_ratio is 0.0 when no lookups
17. `test_hit_ratio_all_hits` — hit_ratio = 1.0 when all hits
18. `test_hit_ratio_half_hits` — hit_ratio ≈ 0.5 when half hits
19. `test_max_entries_not_exceeded` — inserting beyond max doesn't panic
20. `prop_neg_cache_insert_lookup` — proptest: any inserted entry is found immediately

### Section 3: PathResolver (20 tests)
1. `test_path_resolver_new` — creates without panic
2. `test_parse_path_simple` — "a/b/c" → ["a", "b", "c"]
3. `test_parse_path_root` — "/" or empty → []
4. `test_parse_path_absolute` — "/a/b" → ["a", "b"]
5. `test_parse_path_double_slash` — "a//b" → ["a", "b"] (deduplicate)
6. `test_cache_size_empty` — new resolver has cache_size=0
7. `test_cache_resolution_and_size` — after cache_resolution, cache_size=1
8. `test_speculative_resolve_empty_cache` — unresolved path returns ([], full_segments)
9. `test_speculative_resolve_partial_hit` — one cached, one not cached
10. `test_invalidate_entry_removes` — after invalidate_entry, cache_size decrements
11. `test_invalidate_parent_removes_all` — cache_resolution two children, invalidate_parent removes both
12. `test_clear_cache` — after clear_cache, cache_size=0
13. `test_cache_negative` — check_negative returns false before, true after
14. `test_check_negative_false_initially` — fresh resolver returns false
15. `test_invalidate_negative_removes` — after invalidate_negative, check_negative=false
16. `test_invalidate_negative_parent` — cache_negative two names, invalidate_negative_parent removes both
17. `test_negative_cache_size` — tracks negative cache entries
18. `test_resolve_path_with_lookup_fn_root` — resolves "/" to root inode
19. `test_resolve_path_with_lookup_fn_success` — resolves path using provided lookup fn
20. `prop_parse_path_nonempty_segments` — proptest: non-empty path always produces at least 1 segment

## Imports

```rust
use claudefs_meta::locking::{LockManager, LockType, LockEntry};
use claudefs_meta::neg_cache::{NegCacheConfig, NegCacheStats, NegativeCache};
use claudefs_meta::pathres::{PathCacheEntry, PathResolver};
use claudefs_meta::types::{FileType, InodeId, MetaError, NodeId, ShardId};
use proptest::prelude::*;
```

## Output Format

Output the complete `meta_phase2_tests.rs` file. Organize all tests in `#[cfg(test)]` with two submodules: `tests` and `proptest_tests`. Use `#[test]` for sync tests.

Add helper:
```rust
fn ino(n: u64) -> InodeId { InodeId::new(n) }
fn node(n: u64) -> NodeId { NodeId::new(n) }
fn shard(n: u16) -> ShardId { ShardId::new(n) }
```
