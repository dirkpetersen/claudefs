# A2: Implement journal_compactor.rs — Metadata Journal Compaction

## Context

Implement journal compaction for the `claudefs-meta` crate to prevent unbounded
journal growth in long-running deployments.

The journal (journal.rs) records metadata operations. Over time, multiple updates
to the same key accumulate. Compaction merges these into a single entry and removes
entries covered by checkpoints.

The crate uses:
- `thiserror` for errors
- `serde` + `bincode` for serialization
- `tracing` for logging

## Existing types

From `types.rs`:
```rust
pub struct LogIndex(u64); // LogIndex::new(u64), as_u64()
pub struct Timestamp(u64); // Timestamp::now()
pub struct ShardId(u8); // ShardId::new(u8)
pub enum MetaError { NotFound(String), KvError(String), /* ... */ }
```

From `kvstore.rs`:
```rust
pub trait KvStore: Send + Sync {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, MetaError>;
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), MetaError>;
    fn delete(&self, key: &[u8]) -> Result<(), MetaError>;
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, MetaError>;
    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), MetaError>;
}
pub struct MemoryKvStore;
pub enum BatchOp { Put { key: Vec<u8>, value: Vec<u8> }, Delete { key: Vec<u8> } }
```

## Task: Implement `crates/claudefs-meta/src/journal_compactor.rs`

```rust
/// A single journal entry to be compacted.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactEntry {
    /// The log index of this entry.
    pub log_index: LogIndex,
    /// The affected key (e.g., inode key, directory entry key).
    pub key: Vec<u8>,
    /// The operation type.
    pub op_type: CompactOpType,
    /// The new value (None for deletes).
    pub value: Option<Vec<u8>>,
}

/// The type of operation in a compact entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactOpType {
    /// Key was set to a new value.
    Put,
    /// Key was deleted.
    Delete,
}

/// Statistics from a compaction run.
#[derive(Clone, Debug, Default)]
pub struct CompactionStats {
    /// Number of entries input to compaction.
    pub input_entries: usize,
    /// Number of entries output after compaction (deduplicated).
    pub output_entries: usize,
    /// Number of entries removed (covered by newer updates).
    pub removed_entries: usize,
    /// Number of entries removed because they are below the checkpoint index.
    pub truncated_entries: usize,
}

impl CompactionStats {
    /// Returns the compaction ratio (input / output). Returns 1.0 if no compaction.
    pub fn ratio(&self) -> f64;
}

/// Journal compactor that deduplicates and truncates journal entries.
pub struct JournalCompactor;

impl JournalCompactor {
    /// Creates a new JournalCompactor.
    pub fn new() -> Self;

    /// Compacts a slice of journal entries.
    ///
    /// Rules:
    /// 1. **Deduplication**: For the same key, only the entry with the highest log_index is kept.
    /// 2. **Delete wins**: If the highest-index entry for a key is a Delete, include it (to signal deletion).
    /// 3. **Checkpoint truncation**: Entries with log_index <= checkpoint_index are removed
    ///    IF the key has a more recent entry (i.e., the older entry is superseded).
    ///    DELETE entries at log_index <= checkpoint_index can be dropped entirely (the deletion is already applied).
    ///
    /// The output is sorted by log_index ascending.
    ///
    /// # Arguments
    ///
    /// * `entries` - Input entries in any order.
    /// * `checkpoint_index` - Log index of the last applied checkpoint. Entries at or below
    ///   this index that are superseded can be removed.
    pub fn compact(&self, entries: Vec<CompactEntry>, checkpoint_index: LogIndex) -> (Vec<CompactEntry>, CompactionStats);

    /// Computes the "hot keys" — keys that have the most journal entries.
    ///
    /// Returns (key, count) pairs sorted by count descending, up to `top_n` entries.
    /// Useful for identifying keys that are being written very frequently.
    pub fn hot_keys(&self, entries: &[CompactEntry], top_n: usize) -> Vec<(Vec<u8>, usize)>;

    /// Estimates the size savings from compaction without applying it.
    ///
    /// Returns (current_entry_count, compacted_entry_count).
    pub fn estimate_savings(&self, entries: &[CompactEntry], checkpoint_index: LogIndex) -> (usize, usize);
}

impl Default for JournalCompactor { ... }
```

## Required Tests (14 tests)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn entry(idx: u64, key: &[u8], op: CompactOpType, value: Option<&[u8]>) -> CompactEntry {
        CompactEntry {
            log_index: LogIndex::new(idx),
            key: key.to_vec(),
            op_type: op,
            value: value.map(|v| v.to_vec()),
        }
    }
    fn put(idx: u64, key: &[u8], val: &[u8]) -> CompactEntry {
        entry(idx, key, CompactOpType::Put, Some(val))
    }
    fn del(idx: u64, key: &[u8]) -> CompactEntry {
        entry(idx, key, CompactOpType::Delete, None)
    }
    fn compactor() -> JournalCompactor { JournalCompactor::new() }
    fn ckpt(idx: u64) -> LogIndex { LogIndex::new(idx) }
}
```

Tests:
1. `test_compact_empty` — empty input, empty output, stats all zero
2. `test_compact_single_entry` — single entry, no dedup needed, output is same entry
3. `test_compact_dedup_same_key` — three puts for same key, only highest index kept
4. `test_compact_dedup_delete_wins` — put at idx=5 then delete at idx=10, delete is kept
5. `test_compact_multiple_keys` — entries for 3 different keys, all kept
6. `test_compact_output_sorted_by_index` — output is sorted ascending by log_index
7. `test_compact_checkpoint_removes_superseded` — entry at idx=3 superseded by idx=7; with checkpoint=5, idx=3 removed
8. `test_compact_checkpoint_keeps_latest` — latest entry for a key kept even if at idx <= checkpoint
9. `test_compact_delete_at_checkpoint_dropped` — delete entry at idx <= checkpoint, no newer entry, dropped entirely
10. `test_compact_stats_correct` — verify CompactionStats fields after compaction
11. `test_compact_ratio` — ratio = input/output (e.g., 10 in, 5 out → ratio=2.0)
12. `test_hot_keys_top_n` — 5 keys with different counts, hot_keys(top_n=3) returns top 3
13. `test_hot_keys_empty` — empty input returns empty
14. `test_estimate_savings` — estimate matches actual compact output count

## Important

- Write the file directly to `crates/claudefs-meta/src/journal_compactor.rs`
- Do NOT modify lib.rs
- No unused imports, no clippy warnings
- All tests must pass
