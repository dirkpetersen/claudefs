//! Journal compaction for preventing unbounded journal growth.
//!
//! Deduplicates and truncates journal entries so that only the most recent
//! update per key is retained, and entries covered by a checkpoint are removed.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::LogIndex;

/// A single journal entry to be compacted.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactEntry {
    /// The log index of this entry.
    pub log_index: LogIndex,
    /// The affected key.
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
    /// Number of entries output after compaction.
    pub output_entries: usize,
    /// Number of entries removed (covered by newer updates).
    pub removed_entries: usize,
    /// Number of entries removed below checkpoint index.
    pub truncated_entries: usize,
}

impl CompactionStats {
    /// Returns the compaction ratio (input / output). Returns 1.0 if no compaction.
    pub fn ratio(&self) -> f64 {
        if self.output_entries == 0 {
            1.0
        } else {
            self.input_entries as f64 / self.output_entries as f64
        }
    }
}

/// Journal compactor that deduplicates and truncates journal entries.
#[derive(Default)]
pub struct JournalCompactor;

impl JournalCompactor {
    /// Creates a new JournalCompactor.
    pub fn new() -> Self {
        Self
    }

    /// Compacts a slice of journal entries.
    ///
    /// Rules:
    /// 1. For the same key, only the entry with the highest log_index is kept.
    /// 2. If highest-index entry for a key is Delete, include it.
    /// 3. Entries with log_index <= checkpoint_index that are superseded are removed.
    /// 4. Delete entries at log_index <= checkpoint_index with no newer entry are dropped.
    ///
    /// Output is sorted by log_index ascending.
    pub fn compact(
        &self,
        entries: Vec<CompactEntry>,
        checkpoint_index: LogIndex,
    ) -> (Vec<CompactEntry>, CompactionStats) {
        let input_count = entries.len();

        // Group by key, keeping only the entry with the highest log_index per key.
        let mut best: HashMap<Vec<u8>, CompactEntry> = HashMap::new();
        for entry in entries {
            let e = best
                .entry(entry.key.clone())
                .or_insert_with(|| entry.clone());
            if entry.log_index > e.log_index {
                *e = entry;
            }
        }

        // Filter out delete entries at or below checkpoint with no newer entry
        // (since we only keep the best entry per key, if it's a delete at <= checkpoint, drop it)
        let mut result: Vec<CompactEntry> = best
            .into_values()
            .filter(|e| {
                // Drop delete entries at or below checkpoint
                !(e.op_type == CompactOpType::Delete && e.log_index <= checkpoint_index)
            })
            .collect();

        // Sort by log_index ascending
        result.sort_by_key(|e| e.log_index);

        let output_count = result.len();
        let removed = input_count.saturating_sub(output_count);

        let stats = CompactionStats {
            input_entries: input_count,
            output_entries: output_count,
            removed_entries: removed,
            truncated_entries: 0, // simplified: merged into removed
        };

        (result, stats)
    }

    /// Computes the "hot keys" — keys with the most journal entries.
    ///
    /// Returns (key, count) pairs sorted by count descending, up to `top_n`.
    pub fn hot_keys(&self, entries: &[CompactEntry], top_n: usize) -> Vec<(Vec<u8>, usize)> {
        let mut counts: HashMap<&[u8], usize> = HashMap::new();
        for entry in entries {
            *counts.entry(entry.key.as_slice()).or_insert(0) += 1;
        }
        let mut pairs: Vec<(Vec<u8>, usize)> =
            counts.into_iter().map(|(k, c)| (k.to_vec(), c)).collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        pairs.truncate(top_n);
        pairs
    }

    /// Estimates the size savings from compaction without applying it.
    ///
    /// Returns (current_entry_count, compacted_entry_count).
    pub fn estimate_savings(
        &self,
        entries: &[CompactEntry],
        checkpoint_index: LogIndex,
    ) -> (usize, usize) {
        let (compacted, _) = self.compact(entries.to_vec(), checkpoint_index);
        (entries.len(), compacted.len())
    }
}

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
    fn compactor() -> JournalCompactor {
        JournalCompactor::new()
    }
    fn ckpt(idx: u64) -> LogIndex {
        LogIndex::new(idx)
    }

    #[test]
    fn test_compact_empty() {
        let (out, stats) = compactor().compact(vec![], ckpt(0));
        assert!(out.is_empty());
        assert_eq!(stats.input_entries, 0);
        assert_eq!(stats.output_entries, 0);
    }

    #[test]
    fn test_compact_single_entry() {
        let e = put(1, b"key", b"val");
        let (out, stats) = compactor().compact(vec![e.clone()], ckpt(0));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], e);
        assert_eq!(stats.input_entries, 1);
        assert_eq!(stats.output_entries, 1);
    }

    #[test]
    fn test_compact_dedup_same_key() {
        let entries = vec![
            put(1, b"key", b"v1"),
            put(2, b"key", b"v2"),
            put(3, b"key", b"v3"),
        ];
        let (out, stats) = compactor().compact(entries, ckpt(0));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].log_index, LogIndex::new(3));
        assert_eq!(out[0].value, Some(b"v3".to_vec()));
        assert_eq!(stats.input_entries, 3);
        assert_eq!(stats.output_entries, 1);
    }

    #[test]
    fn test_compact_dedup_delete_wins() {
        let entries = vec![put(5, b"key", b"v1"), del(10, b"key")];
        let (out, _) = compactor().compact(entries, ckpt(0));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].op_type, CompactOpType::Delete);
        assert_eq!(out[0].log_index, LogIndex::new(10));
    }

    #[test]
    fn test_compact_multiple_keys() {
        let entries = vec![
            put(1, b"k1", b"v1"),
            put(2, b"k2", b"v2"),
            put(3, b"k3", b"v3"),
        ];
        let (out, _) = compactor().compact(entries, ckpt(0));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_compact_output_sorted_by_index() {
        let entries = vec![
            put(3, b"k3", b"v3"),
            put(1, b"k1", b"v1"),
            put(2, b"k2", b"v2"),
        ];
        let (out, _) = compactor().compact(entries, ckpt(0));
        assert_eq!(out[0].log_index, LogIndex::new(1));
        assert_eq!(out[1].log_index, LogIndex::new(2));
        assert_eq!(out[2].log_index, LogIndex::new(3));
    }

    #[test]
    fn test_compact_checkpoint_removes_superseded() {
        let entries = vec![put(3, b"key", b"v1"), put(7, b"key", b"v2")];
        let (out, _) = compactor().compact(entries, ckpt(5));
        // idx=3 superseded by idx=7, and idx=3 <= checkpoint
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].log_index, LogIndex::new(7));
    }

    #[test]
    fn test_compact_checkpoint_keeps_latest() {
        // Only entry for key is at idx=4, which is <= checkpoint=5
        // But it's a Put, so it should be kept (it's the latest)
        let entries = vec![put(4, b"key", b"v1")];
        let (out, _) = compactor().compact(entries, ckpt(5));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].log_index, LogIndex::new(4));
    }

    #[test]
    fn test_compact_delete_at_checkpoint_dropped() {
        // Delete at idx=3, no newer entry, checkpoint=5: drop entirely
        let entries = vec![del(3, b"key")];
        let (out, _) = compactor().compact(entries, ckpt(5));
        assert!(out.is_empty());
    }

    #[test]
    fn test_compact_stats_correct() {
        let entries = vec![
            put(1, b"k1", b"v1"),
            put(2, b"k1", b"v2"),
            put(3, b"k2", b"v3"),
        ];
        let (out, stats) = compactor().compact(entries, ckpt(0));
        assert_eq!(stats.input_entries, 3);
        assert_eq!(stats.output_entries, 2);
        assert_eq!(stats.removed_entries, 1);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn test_compact_ratio() {
        let entries: Vec<CompactEntry> = (1..=10).map(|i| put(i, b"key", b"v")).collect();
        let (_, stats) = compactor().compact(entries, ckpt(0));
        assert_eq!(stats.input_entries, 10);
        assert_eq!(stats.output_entries, 1);
        assert!((stats.ratio() - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_hot_keys_top_n() {
        let entries = vec![
            put(1, b"a", b"v"),
            put(2, b"a", b"v"),
            put(3, b"a", b"v"),
            put(4, b"b", b"v"),
            put(5, b"b", b"v"),
            put(6, b"c", b"v"),
        ];
        let hot = compactor().hot_keys(&entries, 2);
        assert_eq!(hot.len(), 2);
        assert_eq!(hot[0].0, b"a");
        assert_eq!(hot[0].1, 3);
        assert_eq!(hot[1].0, b"b");
        assert_eq!(hot[1].1, 2);
    }

    #[test]
    fn test_hot_keys_empty() {
        let hot = compactor().hot_keys(&[], 5);
        assert!(hot.is_empty());
    }

    #[test]
    fn test_estimate_savings() {
        let entries = vec![
            put(1, b"key1", b"v1"),
            put(2, b"key1", b"v2"),
            put(3, b"key2", b"v3"),
        ];
        let (current, compacted) = compactor().estimate_savings(&entries, ckpt(0));
        assert_eq!(current, 3);
        assert_eq!(compacted, 2);
    }
}
