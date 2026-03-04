# A3 Phase 9: Block Map, Journal Segment, Tenant Isolator

You are implementing Phase 9 of the A3 (Data Reduction) agent for ClaudeFS, a distributed
scale-out POSIX file system in Rust. You will directly READ and WRITE files in the codebase.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
591 tests passing across 37 modules. Phase 9 goal: reach ~680 tests by:
1. Adding 3 new modules aligned with the write path and multi-tenancy
2. Expanding test coverage in undertested modules

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/block_map.rs`

Implement logical-to-physical block mapping for inode offset resolution.

In ClaudeFS, files are split into content-defined chunks by FastCDC. Each chunk is stored
in a CAS by its hash. The block map tracks, for each file inode, the mapping from logical
byte ranges to CAS chunk hashes and their position within the storage segment.

Requirements:
- `LogicalRange` struct: `offset: u64`, `length: u64`
  - `fn end(&self) -> u64` → offset + length
  - `fn overlaps(&self, other: &LogicalRange) -> bool`
  - Derive Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize
- `BlockEntry` struct: `range: LogicalRange`, `chunk_hash: [u8; 32]`, `chunk_offset: u64`, `chunk_size: u32`
  - Represents one chunk in the file's block map
  - Derive Debug, Clone, Serialize, Deserialize
- `BlockMap` struct for a single inode: `inode_id: u64`, `entries: Vec<BlockEntry>` (sorted by range.offset)
  - `fn new(inode_id: u64) -> Self`
  - `fn insert(&mut self, entry: BlockEntry)` — insert maintaining sort order by range.offset
  - `fn lookup_range(&self, range: &LogicalRange) -> Vec<&BlockEntry>` — return all entries overlapping the range
  - `fn total_logical_size(&self) -> u64` — sum of all entry range lengths
  - `fn len(&self) -> usize` — number of entries
  - `fn is_empty(&self) -> bool`
  - `fn remove_range(&mut self, range: &LogicalRange)` — remove entries completely covered by range (for truncate/overwrite)
- `BlockMapStore` struct: in-memory map of inode_id → BlockMap
  - `fn new() -> Self`
  - `fn get(&self, inode_id: u64) -> Option<&BlockMap>`
  - `fn get_mut(&mut self, inode_id: u64) -> Option<&mut BlockMap>`
  - `fn get_or_create(&mut self, inode_id: u64) -> &mut BlockMap`
  - `fn remove(&mut self, inode_id: u64) -> Option<BlockMap>`
  - `fn len(&self) -> usize`
  - `fn total_chunks(&self) -> usize` — sum of all BlockMap lengths

Write at least **16 tests**:
1. logical_range_end — offset + length
2. logical_range_overlaps_yes — overlapping ranges
3. logical_range_overlaps_no — non-overlapping ranges
4. logical_range_overlaps_adjacent — touching but not overlapping
5. block_map_new_is_empty
6. block_map_insert_single
7. block_map_insert_maintains_order — insert out of order, verify sorted
8. block_map_lookup_range_single_match
9. block_map_lookup_range_no_match
10. block_map_lookup_range_multiple_matches
11. block_map_total_logical_size
12. block_map_remove_range_partial — only removes entries in range
13. block_map_store_new_is_empty
14. block_map_store_get_or_create — creates on first access
15. block_map_store_total_chunks
16. block_map_store_remove
17. block_map_len_and_is_empty

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/journal_segment.rs`

Implement a write-ahead journal segment for crash-consistent writes (D3).

D3: Write journal — 2x synchronous replication to two nodes before ack to client.
The journal segment packs multiple chunk writes into a sequential log for durability.
On crash recovery, the journal is replayed to reconstruct the write-ahead state.

Requirements:
- `JournalEntry` struct: `sequence: u64`, `inode_id: u64`, `offset: u64`, `chunk_hash: [u8; 32]`, `chunk_size: u32`, `data: Vec<u8>`
  - Derive Debug, Clone, Serialize, Deserialize
- `JournalConfig` struct: `max_entries: usize` (default 4096), `max_bytes: usize` (default 32MB)
  - Derive Debug, Clone, Serialize, Deserialize
- `JournalState` enum: `Open`, `Sealed`, `Checkpointed`
  - Derive Debug, Clone, Copy, PartialEq, Eq
- `JournalSegment` struct:
  - `fn new(config: JournalConfig) -> Self`
  - `fn append(&mut self, entry: JournalEntry) -> Result<(), JournalError>` — append entry if not sealed and under limits; returns `JournalError::Full` if at capacity
  - `fn seal(&mut self)` — mark as Sealed (no more writes)
  - `fn checkpoint(&mut self)` — mark as Checkpointed (replicated and safe)
  - `fn state(&self) -> JournalState`
  - `fn entries(&self) -> &[JournalEntry]`
  - `fn entry_count(&self) -> usize`
  - `fn total_bytes(&self) -> usize` — sum of all entry data sizes
  - `fn is_full(&self) -> bool` — entry_count >= max_entries or total_bytes >= max_bytes
  - `fn replay(&self) -> Vec<&JournalEntry>` — returns entries in sequence order
  - `fn since(&self, sequence: u64) -> Vec<&JournalEntry>` — entries with sequence > given value
- `JournalError` enum with `thiserror::Error`:
  - `Full` — journal at capacity
  - `Sealed` — journal is sealed, cannot append
  - `InvalidSequence` — sequence out of order

Write at least **16 tests**:
1. new_journal_is_open_state
2. new_journal_is_empty
3. append_single_entry
4. append_increments_count
5. seal_changes_state
6. append_after_seal_returns_error
7. checkpoint_changes_state
8. is_full_by_entry_count — max_entries=5, add 5 → full
9. is_full_by_bytes — add entries until bytes full
10. append_when_full_returns_error
11. replay_returns_entries_in_order
12. since_returns_entries_after_sequence
13. since_with_zero_returns_all
14. entry_count_increments
15. total_bytes_sums_data_sizes
16. journal_config_default_values
17. multiple_inodes_in_journal — entries for different inodes coexist

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/tenant_isolator.rs`

Implement multi-tenant data isolation for quota enforcement and data separation.

Requirements:
- `TenantId` newtype: `pub struct TenantId(pub u64)` — Derive Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize
- `TenantPolicy` struct: `tenant_id: TenantId`, `quota_bytes: u64`, `max_iops: u32`, `priority: TenantPriority`
  - Derive Debug, Clone, Serialize, Deserialize
- `TenantPriority` enum: `Low`, `Normal`, `High`, `Critical`
  - Derive Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize
- `TenantUsage` struct: `tenant_id: TenantId`, `bytes_used: u64`, `iops_used: u32`, `chunks_stored: u64`
  - `fn quota_utilization(&self, policy: &TenantPolicy) -> f64` → bytes_used as f64 / quota_bytes as f64
  - `fn is_quota_exceeded(&self, policy: &TenantPolicy) -> bool` → bytes_used > policy.quota_bytes
  - Derive Debug, Clone, Default, Serialize, Deserialize
- `TenantIsolator` struct:
  - `fn new() -> Self`
  - `fn register_tenant(&mut self, policy: TenantPolicy)`
  - `fn get_policy(&self, tenant_id: TenantId) -> Option<&TenantPolicy>`
  - `fn get_usage(&self, tenant_id: TenantId) -> Option<&TenantUsage>`
  - `fn record_write(&mut self, tenant_id: TenantId, bytes: u64) -> Result<(), TenantError>` — returns `TenantError::QuotaExceeded` if over quota; otherwise increments usage
  - `fn record_chunk(&mut self, tenant_id: TenantId, chunk_size: u64) -> Result<(), TenantError>`
  - `fn reset_usage(&mut self, tenant_id: TenantId)` — reset all usage counters to 0
  - `fn list_tenants(&self) -> Vec<TenantId>` — sorted by TenantId
  - `fn tenants_over_quota(&self) -> Vec<TenantId>` — tenants where bytes_used > quota_bytes
- `TenantError` enum with `thiserror::Error`:
  - `QuotaExceeded { tenant_id: TenantId, used: u64, limit: u64 }`
  - `UnknownTenant { tenant_id: TenantId }`
  - Derive Debug

Write at least **16 tests**:
1. tenant_id_equality
2. tenant_priority_ordering — High > Normal > Low
3. register_and_get_policy
4. get_policy_unknown_returns_none
5. record_write_increments_usage
6. record_write_quota_exceeded_returns_error
7. record_chunk_increments_chunks_stored
8. record_write_unknown_tenant_returns_error
9. reset_usage_clears_counters
10. list_tenants_sorted
11. list_tenants_empty
12. tenants_over_quota_none — all within quota
13. tenants_over_quota_some — one over quota
14. quota_utilization_zero — no usage
15. quota_utilization_full — at quota limit
16. is_quota_exceeded_false — under quota
17. is_quota_exceeded_true — over quota
18. multiple_tenants_isolated — usage for one tenant doesn't affect another

---

## EXPAND TESTS in existing modules

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/meta_bridge.rs`
Read the file first (it has 8 tests). Add 10 more tests covering:
`BlockLocation`, `FingerprintStore` trait, `LocalFingerprintStore`, `NullFingerprintStore`

New tests:
1. `test_block_location_fields` — verify struct fields set correctly
2. `test_local_store_empty_initially` — entry_count == 0
3. `test_local_store_insert_and_lookup` — insert, then lookup finds it
4. `test_local_store_lookup_missing` — lookup non-existent returns None
5. `test_local_store_increment_ref` — increment_ref increases count
6. `test_local_store_decrement_ref` — decrement_ref decreases count
7. `test_local_store_decrement_to_zero` — returns 0
8. `test_null_store_lookup_always_none` — NullFingerprintStore returns None
9. `test_null_store_insert_returns_true` — NullFingerprintStore insert returns true
10. `test_local_store_entry_count` — increments with inserts

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/key_manager.rs`
Read the file first (9 tests). Add 8 more tests.

New tests covering `DataKey`, `KeyManager`, `KeyManagerConfig`, `KeyVersion`, `VersionedKey`, `WrappedKey`:
1. `test_key_manager_config_default`
2. `test_key_manager_generate_key` — generates a key, verify it's non-zero
3. `test_key_version_ordering` — v2 > v1
4. `test_versioned_key_current_version`
5. `test_key_manager_rotate` — after rotation, new version is incremented
6. `test_data_key_zeroize_on_drop` — DataKey has zeroize behavior
7. `test_wrapped_key_roundtrip` — wrap and unwrap returns same key
8. `test_key_manager_get_current`

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/dedupe.rs`
Read the file first (12 tests). Add 8 more tests.

New tests covering `CasIndex`, `Chunk`, `Chunker`, `ChunkerConfig`:
1. `test_cas_index_insert_twice_same_hash` — refcount becomes 2
2. `test_cas_index_len` — tracks correct length
3. `test_cas_index_is_empty` — true initially
4. `test_chunk_hash_is_deterministic` — same data → same chunk hash
5. `test_chunker_config_default` — verify default values
6. `test_chunker_produces_chunks` — chunk 1MB of data
7. `test_chunker_chunk_sizes_in_range` — all chunks within min/max bounds
8. `test_cas_refcount_multiple_inserts` — 3 inserts → refcount 3

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/segment.rs`
Read the file first (12 tests). Add 8 more tests.

New tests covering `Segment`, `SegmentEntry`, `SegmentPacker`, `SegmentPackerConfig`:
1. `test_segment_entry_fields`
2. `test_segment_packer_config_default`
3. `test_segment_packer_new_is_empty`
4. `test_segment_packer_add_chunk`
5. `test_segment_seals_when_full`
6. `test_sealed_segment_immutable` — adding to sealed segment returns None/does nothing
7. `test_segment_entry_count`
8. `test_segment_total_bytes_sums_correctly`

---

## Implementation instructions

1. READ each existing file before editing it
2. For new files: write complete, compilable Rust with `#![warn(missing_docs)]-compatible` doc comments
3. For test expansions: append to existing `mod tests` blocks
4. Import `use thiserror::Error;` for error types; `use serde::{Serialize, Deserialize};` for structs
5. No async in new modules
6. Do NOT modify Cargo.toml (all deps already present)

## Also update lib.rs

Add to `/home/cfs/claudefs/crates/claudefs-reduce/src/lib.rs`:
- `pub mod block_map;`
- `pub mod journal_segment;`
- `pub mod tenant_isolator;`
- Add re-exports for key types

## Goal
- `cargo build -p claudefs-reduce` compiles with 0 errors, 0 warnings
- `cargo test -p claudefs-reduce` shows ~680+ tests passing
- `cargo clippy -p claudefs-reduce -- -D warnings` passes cleanly
