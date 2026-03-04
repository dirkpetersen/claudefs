# A3 Phase 19: Implement 3 new modules for claudefs-reduce

## Working directory
`/home/cfs/claudefs`

## Context
The `claudefs-reduce` crate has 1349 tests across 67 modules. Goal: ~1415 tests (+66).
Implement 3 new modules and integrate them into lib.rs.

## Task

Create these 3 new files:

### 1. crates/claudefs-reduce/src/hash_ring.rs

Consistent hash ring for shard/node assignment in distributed dedup lookups.

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Config for the hash ring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashRingConfig {
    pub virtual_nodes_per_member: usize,  // default 150
}

impl Default for HashRingConfig { ... }

/// A node/shard in the ring
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingMember {
    pub id: u32,
    pub label: String,
}

/// Stats about ring distribution
#[derive(Debug, Clone, Default)]
pub struct RingStats {
    pub total_members: usize,
    pub total_virtual_nodes: usize,
}

/// Consistent hash ring
pub struct HashRing {
    ring: BTreeMap<u64, RingMember>,
    config: HashRingConfig,
    stats: RingStats,
}

impl HashRing {
    pub fn new(config: HashRingConfig) -> Self { ... }
    /// Add a member to the ring (inserts virtual_nodes_per_member virtual nodes)
    pub fn add_member(&mut self, member: RingMember) { ... }
    /// Remove a member by id
    pub fn remove_member(&mut self, id: u32) { ... }
    /// Find responsible member for a key (hash key, walk ring clockwise)
    pub fn get_member(&self, key: &[u8]) -> Option<&RingMember> { ... }
    /// Get N distinct responsible members for a key (for replication factor)
    pub fn get_members(&self, key: &[u8], count: usize) -> Vec<&RingMember> { ... }
    pub fn member_count(&self) -> usize { ... }
    pub fn stats(&self) -> &RingStats { ... }
}

/// Hash a byte slice to a u64 using a simple mixing function
fn hash_key(data: &[u8]) -> u64 { ... }
```

For virtual node placement: for member with id `m`, virtual node `v` hashes `[m as bytes, v as bytes]`.
For `get_member(key)`: compute `hash_key(key)`, find the next entry >= that value in BTreeMap (wrap around).
For `get_members(key, count)`: collect up to `count` distinct members starting from key position.

**Tests (at least 20):**
1. `hash_ring_config_default` — virtual_nodes_per_member == 150
2. `add_single_member` — member_count() == 1
3. `add_multiple_members` — member_count() == 3
4. `get_member_empty_ring` — returns None
5. `get_member_single_member` — always returns that member
6. `get_member_multiple_members` — returns a valid member
7. `remove_member` — member_count decreases
8. `remove_nonexistent_member` — no panic, count stays
9. `get_members_count_limited` — returns min(count, members)
10. `get_members_empty_ring` — returns empty
11. `get_members_dedup` — all returned members are distinct
12. `stats_total_members` — correct count
13. `stats_total_virtual_nodes` — count == members * virtual_nodes
14. `consistent_hashing_same_key` — same key maps to same member
15. `distribution_reasonable` — 1000 keys across 3 members: each gets >10%
16. `add_remove_member` — add 2, remove 1, count == 1
17. `ring_member_equality` — RingMember with same id/label equals
18. `get_members_returns_ordered` — returned members in ring order
19. `wrap_around` — handles wrap-around in ring
20. `large_ring` — 10 members, 1000 keys → distributes

---

### 2. crates/claudefs-reduce/src/write_journal.rs

Append-only in-memory write journal for ordered write tracking and recovery.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntryData {
    pub seq: u64,
    pub inode_id: u64,
    pub offset: u64,
    pub len: u32,
    pub hash: [u8; 32],
    pub committed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteJournalConfig {
    pub max_entries: usize,  // default 10_000
    pub flush_threshold: usize, // default 1_000
}

impl Default for WriteJournalConfig { ... }

#[derive(Debug, Clone, Default)]
pub struct WriteJournalStats {
    pub entries_appended: u64,
    pub entries_committed: u64,
    pub entries_flushed: u64,
    pub current_seq: u64,
}

pub struct WriteJournal {
    entries: Vec<JournalEntryData>,
    config: WriteJournalConfig,
    stats: WriteJournalStats,
    next_seq: u64,
}

impl WriteJournal {
    pub fn new(config: WriteJournalConfig) -> Self { ... }
    /// Append a write entry, returns its sequence number
    pub fn append(&mut self, inode_id: u64, offset: u64, len: u32, hash: [u8; 32]) -> u64 { ... }
    /// Mark entry as committed (by seq)
    pub fn commit(&mut self, seq: u64) -> bool { ... }
    /// Flush (remove) all committed entries below given seq; returns count flushed
    pub fn flush_committed(&mut self, before_seq: u64) -> usize { ... }
    /// Get uncommitted entries for an inode
    pub fn pending_for_inode(&self, inode_id: u64) -> Vec<&JournalEntryData> { ... }
    /// Total entries in journal
    pub fn len(&self) -> usize { ... }
    pub fn is_empty(&self) -> bool { ... }
    pub fn stats(&self) -> &WriteJournalStats { ... }
    /// Check if journal needs flush (entries >= flush_threshold)
    pub fn needs_flush(&self) -> bool { ... }
}
```

**Tests (at least 20):**
1. `write_journal_config_default` — max_entries == 10_000, flush_threshold == 1_000
2. `write_journal_new_empty` — is_empty() == true
3. `append_single_entry` — len() == 1
4. `append_returns_seq` — first append returns seq 0 or 1 (your choice, be consistent)
5. `append_seq_increments` — each append returns next seq
6. `commit_entry` — commit(seq) returns true, entry.committed == true
7. `commit_nonexistent` — returns false
8. `flush_committed_removes_entries` — committed entries below seq are removed
9. `flush_uncommitted_stays` — uncommitted entries not flushed
10. `pending_for_inode_empty` — returns empty for unknown inode
11. `pending_for_inode_has_entries` — uncommitted entries appear
12. `stats_entries_appended` — increments on append
13. `stats_entries_committed` — increments on commit
14. `stats_entries_flushed` — increments on flush
15. `needs_flush_false_when_empty` — false initially
16. `needs_flush_true_at_threshold` — add flush_threshold entries → needs_flush() == true
17. `flush_then_empty` — flush all → is_empty()
18. `multiple_inodes` — entries for different inodes tracked separately
19. `double_commit_seq` — committing twice is idempotent
20. `flush_before_any_commits` — flush with no committed entries → 0 flushed

---

### 3. crates/claudefs-reduce/src/chunk_tracker.rs

Tracks per-chunk reference counts and lifecycle for GC coordination.

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChunkState {
    Live,        // Referenced
    Orphaned,    // Ref count dropped to 0, pending GC
    Deleted,     // Removed from storage
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRecord {
    pub hash: [u8; 32],
    pub ref_count: u32,
    pub size_bytes: u32,
    pub state: ChunkState,
    pub segment_id: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TrackerStats {
    pub total_chunks: u64,
    pub live_chunks: u64,
    pub orphaned_chunks: u64,
    pub deleted_chunks: u64,
    pub total_bytes: u64,
}

pub struct ChunkTracker {
    chunks: HashMap<[u8; 32], ChunkRecord>,
    stats: TrackerStats,
}

impl Default for ChunkTracker { ... }

impl ChunkTracker {
    pub fn new() -> Self { ... }
    /// Register a new chunk (ref_count starts at 1)
    pub fn register(&mut self, hash: [u8; 32], size_bytes: u32, segment_id: u64) { ... }
    /// Increment ref count (when chunk is referenced by a new file)
    pub fn inc_ref(&mut self, hash: &[u8; 32]) -> bool { ... }
    /// Decrement ref count; if 0, moves to Orphaned state; returns new ref count
    pub fn dec_ref(&mut self, hash: &[u8; 32]) -> Option<u32> { ... }
    /// Mark orphaned chunks as deleted
    pub fn delete_orphaned(&mut self) -> usize { ... }
    /// Get chunk info
    pub fn get(&self, hash: &[u8; 32]) -> Option<&ChunkRecord> { ... }
    /// List all orphaned chunks
    pub fn orphaned_chunks(&self) -> Vec<&ChunkRecord> { ... }
    pub fn stats(&self) -> TrackerStats { ... }
    pub fn len(&self) -> usize { ... }
    pub fn is_empty(&self) -> bool { ... }
}
```

**Tests (at least 20):**
1. `chunk_tracker_new_empty` — is_empty() == true
2. `register_chunk` — len() == 1
3. `register_chunk_state_live` — newly registered chunk is Live
4. `register_chunk_ref_count_one` — ref_count == 1
5. `inc_ref_existing` — returns true, ref_count increases
6. `inc_ref_nonexistent` — returns false
7. `dec_ref_to_zero` — moves to Orphaned state
8. `dec_ref_above_zero` — stays Live
9. `dec_ref_nonexistent` — returns None
10. `delete_orphaned_clears` — orphaned chunks become Deleted
11. `delete_orphaned_count` — returns correct count
12. `orphaned_chunks_list` — returns all Orphaned records
13. `orphaned_chunks_empty_when_none` — empty initially
14. `stats_total_chunks` — correct total
15. `stats_live_chunks` — counts only Live
16. `stats_orphaned_chunks` — counts only Orphaned
17. `stats_deleted_chunks` — counts only Deleted
18. `stats_total_bytes` — sum of sizes (Live + Orphaned only, not Deleted)
19. `multiple_chunks_lifecycle` — register, inc, dec, orphan, delete
20. `register_duplicate_hash` — second register doesn't overwrite (no-op or increments ref)
21. `chunk_state_equality` — ChunkState::Live == ChunkState::Live

---

## After creating the 3 files, edit lib.rs:

Add to the `pub mod` section (alphabetically):
```rust
pub mod chunk_tracker;
pub mod hash_ring;
pub mod write_journal;
```

Add to the `pub use` section at the bottom:
```rust
pub use chunk_tracker::{ChunkRecord, ChunkState, ChunkTracker, TrackerStats};
pub use hash_ring::{HashRing, HashRingConfig, RingMember, RingStats};
pub use write_journal::{
    JournalEntryData, WriteJournal, WriteJournalConfig, WriteJournalStats,
};
```

## Final validation
Run: `cd /home/cfs/claudefs && cargo test -p claudefs-reduce 2>&1 | tail -5`
All tests must pass. Fix any compilation errors.

## Style rules
- No doc comments on private items
- Use `use serde::{Deserialize, Serialize};`
- All test functions in `#[cfg(test)] mod tests { ... }`
- Keep code minimal and clean
