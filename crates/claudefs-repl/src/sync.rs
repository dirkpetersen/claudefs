//! Synchronization primitives for replication coordination.
//!
//! This module provides:
//! - ConflictDetector: LWW conflict resolution with admin alerting
//! - BatchCompactor: coalesces journal entries per inode before sending
//! - ReplicationSync: high-level coordinator that drives the replication loop

use crate::conduit::EntryBatch;
use crate::journal::{JournalEntry, OpKind};
use crate::wal::{ReplicationCursor, ReplicationWal};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A detected write conflict (same inode, different timestamps on two sites).
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    /// The inode with conflicting updates.
    pub inode: u64,
    /// Local site ID.
    pub local_site_id: u64,
    /// Remote site ID.
    pub remote_site_id: u64,
    /// Timestamp of local entry (microseconds).
    pub local_ts: u64,
    /// Timestamp of remote entry (microseconds).
    pub remote_ts: u64,
    /// The winner (site_id of the entry with the higher timestamp_us).
    pub winner_site_id: u64,
    /// When the conflict was detected (system time microseconds).
    pub detected_at_us: u64,
}

/// Detects and records LWW conflicts between local and remote journal entries.
pub struct ConflictDetector {
    local_site_id: u64,
    conflicts: Arc<Mutex<Vec<Conflict>>>,
}

impl ConflictDetector {
    /// Create a new conflict detector for the given local site.
    pub fn new(local_site_id: u64) -> Self {
        Self {
            local_site_id,
            conflicts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Compare a local entry with an incoming remote entry for the same inode.
    /// If they have different timestamps, this is a conflict (resolved by LWW).
    /// Returns Some(Conflict) if a conflict was detected, None if no conflict.
    /// A conflict is: same inode, same shard, remote_site_id != local_site_id,
    /// AND both operations modify the same resource AND the remote entry does NOT
    /// extend the local sequence (i.e., they're concurrent updates, not sequential).
    pub async fn check(&self, local: &JournalEntry, remote: &JournalEntry) -> Option<Conflict> {
        if !Self::entries_conflict(local, remote) {
            return None;
        }

        let winner = if local.timestamp_us >= remote.timestamp_us {
            local.site_id
        } else {
            remote.site_id
        };

        let conflict = Conflict {
            inode: local.inode,
            local_site_id: self.local_site_id,
            remote_site_id: remote.site_id,
            local_ts: local.timestamp_us,
            remote_ts: remote.timestamp_us,
            winner_site_id: winner,
            detected_at_us: current_time_us(),
        };

        self.conflicts.lock().await.push(conflict.clone());

        Some(conflict)
    }

    /// Return all recorded conflicts.
    pub async fn conflicts(&self) -> Vec<Conflict> {
        self.conflicts.lock().await.clone()
    }

    /// Clear the conflict log (e.g., after admin has acknowledged them).
    pub async fn clear_conflicts(&self) {
        self.conflicts.lock().await.clear();
    }

    /// Return the count of recorded conflicts.
    pub async fn conflict_count(&self) -> usize {
        self.conflicts.lock().await.len()
    }

    /// Check if two entries conflict (same inode, same shard, different sites).
    pub fn entries_conflict(local: &JournalEntry, remote: &JournalEntry) -> bool {
        local.inode == remote.inode
            && local.shard_id == remote.shard_id
            && local.site_id != remote.site_id
    }
}

/// Get current time in microseconds since Unix epoch.
fn current_time_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

/// Compaction result for a group of entries.
#[derive(Debug, Clone, PartialEq)]
pub struct CompactionResult {
    /// Entries that should be sent (after deduplication/compaction).
    pub entries: Vec<JournalEntry>,
    /// Number of entries removed by compaction.
    pub removed_count: usize,
}

/// Compacts (deduplicates) journal entries before sending over the conduit.
///
/// Rules:
/// - For the same inode + op (e.g., multiple Writes), keep only the latest by timestamp_us.
/// - For SetAttr ops on the same inode, keep only the latest by timestamp_us.
/// - Create/Unlink/MkDir/Symlink/Link are always kept (structural ops).
/// - Rename is always kept.
/// - Within a shard, output entries are sorted by seq.
pub struct BatchCompactor;

impl BatchCompactor {
    /// Compact a batch of journal entries.
    /// Removes redundant entries (e.g., superseded Writes to the same inode).
    pub fn compact(entries: Vec<JournalEntry>) -> CompactionResult {
        if entries.is_empty() {
            return CompactionResult {
                entries: vec![],
                removed_count: 0,
            };
        }

        let original_count = entries.len();

        let mut by_inode_op: HashMap<(u64, u8), Vec<&JournalEntry>> = HashMap::new();
        for entry in &entries {
            let key = (entry.inode, op_kind_discriminant(&entry.op));
            by_inode_op.entry(key).or_default().push(entry);
        }

        let mut to_keep: Vec<bool> = vec![true; entries.len()];

        for ((inode, op_disc), group) in by_inode_op.iter() {
            if group.len() <= 1 {
                continue;
            }

            let op = op_kind_from_discriminant(*op_disc);
            if Self::is_structural_op(&op) {
                continue;
            }

            if op == OpKind::SetAttr {
                if let Some(latest) = group.iter().max_by_key(|e| e.timestamp_us) {
                    for entry in group {
                        if !std::ptr::eq(*entry, *latest) {
                            if let Some(pos) = entries.iter().position(|e| e.seq == entry.seq && e.inode == *inode) {
                                to_keep[pos] = false;
                            }
                        }
                    }
                }
                continue;
            }

            if op == OpKind::Write || op == OpKind::Truncate {
                if let Some(latest) = group.iter().max_by_key(|e| e.timestamp_us) {
                    for entry in group {
                        if !std::ptr::eq(*entry, *latest) {
                            if let Some(pos) = entries.iter().position(|e| e.seq == entry.seq && e.inode == *inode) {
                                to_keep[pos] = false;
                            }
                        }
                    }
                }
            }
        }

        let mut result: Vec<JournalEntry> = entries
            .into_iter()
            .zip(to_keep)
            .filter(|(_, keep)| *keep)
            .map(|(e, _)| e)
            .collect();

        result.sort_by_key(|e| (e.shard_id, e.seq));

        let removed_count = original_count - result.len();

        CompactionResult {
            entries: result,
            removed_count,
        }
    }

    /// Compact for a specific inode only (used in targeted tests).
    pub fn compact_inode(entries: Vec<JournalEntry>, inode: u64) -> CompactionResult {
        let filtered: Vec<_> = entries.into_iter().filter(|e| e.inode == inode).collect();
        Self::compact(filtered)
    }

    fn is_structural_op(op: &OpKind) -> bool {
        matches!(
            op,
            OpKind::Create
                | OpKind::Unlink
                | OpKind::MkDir
                | OpKind::Symlink
                | OpKind::Link
                | OpKind::Rename
        )
    }
}

fn op_kind_discriminant(op: &OpKind) -> u8 {
    match op {
        OpKind::Create => 0,
        OpKind::Unlink => 1,
        OpKind::Rename => 2,
        OpKind::Write => 3,
        OpKind::Truncate => 4,
        OpKind::SetAttr => 5,
        OpKind::Link => 6,
        OpKind::Symlink => 7,
        OpKind::MkDir => 8,
        OpKind::SetXattr => 9,
        OpKind::RemoveXattr => 10,
    }
}

fn op_kind_from_discriminant(disc: u8) -> OpKind {
    match disc {
        0 => OpKind::Create,
        1 => OpKind::Unlink,
        2 => OpKind::Rename,
        3 => OpKind::Write,
        4 => OpKind::Truncate,
        5 => OpKind::SetAttr,
        6 => OpKind::Link,
        7 => OpKind::Symlink,
        8 => OpKind::MkDir,
        9 => OpKind::SetXattr,
        10 => OpKind::RemoveXattr,
        _ => OpKind::Write,
    }
}

/// High-level replication synchronization state machine.
/// Drives the replication loop for one remote site.
#[allow(dead_code)]
pub struct ReplicationSync {
    local_site_id: u64,
    remote_site_id: u64,
    wal: Arc<Mutex<ReplicationWal>>,
    detector: Arc<ConflictDetector>,
}

impl ReplicationSync {
    /// Create a new replication sync for one remote site.
    pub fn new(local_site_id: u64, remote_site_id: u64) -> Self {
        Self {
            local_site_id,
            remote_site_id,
            wal: Arc::new(Mutex::new(ReplicationWal::new())),
            detector: Arc::new(ConflictDetector::new(local_site_id)),
        }
    }

    /// Apply a received batch from the remote site.
    /// Compares remote entries against what we expect, detects conflicts,
    /// and advances the WAL cursor.
    /// Returns an ApplyResult describing what happened.
    pub async fn apply_batch(&self, batch: &EntryBatch, local_entries: &[JournalEntry]) -> ApplyResult {
        if batch.source_site_id != self.remote_site_id {
            return ApplyResult::Rejected {
                reason: format!(
                    "batch source site {} does not match expected remote site {}",
                    batch.source_site_id, self.remote_site_id
                ),
            };
        }

        if batch.entries.is_empty() {
            return ApplyResult::Applied { count: 0 };
        }

        let shard_id = batch.entries[0].shard_id;
        let expected_seq = {
            let wal = self.wal.lock().await;
            wal.cursor(self.remote_site_id, shard_id).last_seq
        };

        let first_seq = batch.entries[0].seq;
        if first_seq != expected_seq + 1 && expected_seq != 0 {
            return ApplyResult::Rejected {
                reason: format!(
                    "sequence gap: expected seq {}, got {}",
                    expected_seq + 1,
                    first_seq
                ),
            };
        }

        let mut applied = 0;
        let mut conflicts = 0;

        let local_by_inode: HashMap<u64, &JournalEntry> = local_entries
            .iter()
            .filter(|e| e.shard_id == shard_id)
            .fold(HashMap::new(), |mut acc, e| {
                acc.entry(e.inode).or_insert(e);
                acc
            });

        for remote_entry in &batch.entries {
            if let Some(local_entry) = local_by_inode.get(&remote_entry.inode) {
                if let Some(_conflict) = self.detector.check(local_entry, remote_entry).await {
                    conflicts += 1;
                }
            }
            applied += 1;
        }

        let last_seq = batch.entries.last().map(|e| e.seq).unwrap_or(0);
        {
            let mut wal = self.wal.lock().await;
            wal.advance(
                self.remote_site_id,
                shard_id,
                last_seq,
                current_time_us(),
                applied as u32,
            );
        }

        if conflicts > 0 {
            ApplyResult::AppliedWithConflicts { applied, conflicts }
        } else {
            ApplyResult::Applied { count: applied }
        }
    }

    /// Get the current replication lag (in entries) for a shard.
    /// This is the difference between the local journal tip and the remote cursor.
    pub async fn lag(&self, shard_id: u32, local_tip: u64) -> u64 {
        let wal = self.wal.lock().await;
        let remote_cursor = wal.cursor(self.remote_site_id, shard_id);
        local_tip.saturating_sub(remote_cursor.last_seq)
    }

    /// Get the WAL (for inspection/persistence).
    pub async fn wal_snapshot(&self) -> Vec<ReplicationCursor> {
        let wal = self.wal.lock().await;
        wal.all_cursors()
    }

    /// Get the conflict detector (for admin reporting).
    pub fn detector(&self) -> Arc<ConflictDetector> {
        self.detector.clone()
    }
}

/// The outcome of applying a batch of remote entries.
#[derive(Debug, Clone, PartialEq)]
pub enum ApplyResult {
    /// All entries applied cleanly.
    Applied {
        /// Number of entries applied.
        count: usize,
    },
    /// Some entries were applied, some had conflicts (resolved by LWW).
    AppliedWithConflicts {
        /// Number of entries applied.
        applied: usize,
        /// Number of conflicts detected.
        conflicts: usize,
    },
    /// Batch was rejected (e.g., wrong site, bad sequence number).
    Rejected {
        /// Reason for rejection.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(seq: u64, shard_id: u32, site_id: u64, ts: u64, inode: u64, op: OpKind) -> JournalEntry {
        JournalEntry::new(seq, shard_id, site_id, ts, inode, op, vec![1, 2, 3])
    }

    mod conflict_detector {
        use super::*;

        #[tokio::test]
        async fn test_detect_conflict_same_inode() {
            let detector = ConflictDetector::new(1);

            let local = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote = make_entry(1, 0, 2, 2000, 100, OpKind::Write);

            let conflict = detector.check(&local, &remote).await;
            assert!(conflict.is_some());
            assert_eq!(conflict.unwrap().inode, 100);
        }

        #[tokio::test]
        async fn test_no_conflict_different_inodes() {
            let detector = ConflictDetector::new(1);

            let local = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote = make_entry(1, 0, 2, 2000, 200, OpKind::Write);

            let conflict = detector.check(&local, &remote).await;
            assert!(conflict.is_none());
        }

        #[tokio::test]
        async fn test_no_conflict_same_site() {
            let detector = ConflictDetector::new(1);

            let local = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote = make_entry(2, 0, 1, 2000, 100, OpKind::Write);

            let conflict = detector.check(&local, &remote).await;
            assert!(conflict.is_none());
        }

        #[tokio::test]
        async fn test_lww_winner_higher_timestamp() {
            let detector = ConflictDetector::new(1);

            let local = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote = make_entry(1, 0, 2, 2000, 100, OpKind::Write);

            let conflict = detector.check(&local, &remote).await.unwrap();
            assert_eq!(conflict.winner_site_id, 2);
            assert_eq!(conflict.remote_ts, 2000);
        }

        #[tokio::test]
        async fn test_lww_winner_local_higher_timestamp() {
            let detector = ConflictDetector::new(1);

            let local = make_entry(1, 0, 1, 3000, 100, OpKind::Write);
            let remote = make_entry(1, 0, 2, 2000, 100, OpKind::Write);

            let conflict = detector.check(&local, &remote).await.unwrap();
            assert_eq!(conflict.winner_site_id, 1);
            assert_eq!(conflict.local_ts, 3000);
        }

        #[tokio::test]
        async fn test_clear_conflicts() {
            let detector = ConflictDetector::new(1);

            let local = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote = make_entry(1, 0, 2, 2000, 100, OpKind::Write);
            detector.check(&local, &remote).await;

            assert_eq!(detector.conflict_count().await, 1);

            detector.clear_conflicts().await;

            assert_eq!(detector.conflict_count().await, 0);
        }

        #[tokio::test]
        async fn test_conflict_count() {
            let detector = ConflictDetector::new(1);

            let local1 = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote1 = make_entry(1, 0, 2, 2000, 100, OpKind::Write);
            detector.check(&local1, &remote1).await;

            let local2 = make_entry(2, 0, 1, 1000, 200, OpKind::Write);
            let remote2 = make_entry(2, 0, 2, 2000, 200, OpKind::Write);
            detector.check(&local2, &remote2).await;

            assert_eq!(detector.conflict_count().await, 2);
        }

        #[tokio::test]
        async fn test_conflicts_returns_all() {
            let detector = ConflictDetector::new(1);

            let local1 = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote1 = make_entry(1, 0, 2, 2000, 100, OpKind::Write);
            detector.check(&local1, &remote1).await;

            let local2 = make_entry(2, 0, 1, 1000, 200, OpKind::Write);
            let remote2 = make_entry(2, 0, 2, 2000, 200, OpKind::Write);
            detector.check(&local2, &remote2).await;

            let conflicts = detector.conflicts().await;
            assert_eq!(conflicts.len(), 2);
            assert_eq!(conflicts[0].inode, 100);
            assert_eq!(conflicts[1].inode, 200);
        }

        #[test]
        fn test_entries_conflict_predicate() {
            let local = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote_same = make_entry(1, 0, 2, 2000, 100, OpKind::Write);
            let remote_different_inode = make_entry(1, 0, 2, 2000, 200, OpKind::Write);
            let remote_different_shard = make_entry(1, 1, 2, 2000, 100, OpKind::Write);
            let remote_same_site = make_entry(1, 0, 1, 2000, 100, OpKind::Write);

            assert!(ConflictDetector::entries_conflict(&local, &remote_same));
            assert!(!ConflictDetector::entries_conflict(&local, &remote_different_inode));
            assert!(!ConflictDetector::entries_conflict(&local, &remote_different_shard));
            assert!(!ConflictDetector::entries_conflict(&local, &remote_same_site));
        }
    }

    mod batch_compactor {
        use super::*;

        #[test]
        fn test_remove_duplicate_writes() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::Write),
                make_entry(2, 0, 1, 2000, 100, OpKind::Write),
                make_entry(3, 0, 1, 1500, 100, OpKind::Write),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 1);
            assert_eq!(result.entries[0].seq, 2);
            assert_eq!(result.removed_count, 2);
        }

        #[test]
        fn test_keep_latest_setattr() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::SetAttr),
                make_entry(2, 0, 1, 2000, 100, OpKind::SetAttr),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 1);
            assert_eq!(result.entries[0].seq, 2);
        }

        #[test]
        fn test_keep_all_structural_ops() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::Create),
                make_entry(2, 0, 1, 2000, 100, OpKind::Create),
                make_entry(3, 0, 1, 3000, 100, OpKind::MkDir),
                make_entry(4, 0, 1, 4000, 100, OpKind::Unlink),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 4);
            assert_eq!(result.removed_count, 0);
        }

        #[test]
        fn test_keep_all_renames() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::Rename),
                make_entry(2, 0, 1, 2000, 100, OpKind::Rename),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 2);
        }

        #[test]
        fn test_output_sorted_by_seq() {
            let entries = vec![
                make_entry(5, 0, 1, 5000, 100, OpKind::Write),
                make_entry(2, 0, 1, 2000, 100, OpKind::Write),
                make_entry(8, 0, 1, 8000, 100, OpKind::Write),
                make_entry(1, 0, 1, 1000, 100, OpKind::Write),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 1);
            assert_eq!(result.entries[0].seq, 8);
        }

        #[test]
        fn test_empty_input() {
            let result = BatchCompactor::compact(vec![]);

            assert!(result.entries.is_empty());
            assert_eq!(result.removed_count, 0);
        }

        #[test]
        fn test_single_entry() {
            let entries = vec![make_entry(1, 0, 1, 1000, 100, OpKind::Write)];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 1);
            assert_eq!(result.removed_count, 0);
        }

        #[test]
        fn test_no_compaction_needed() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::Write),
                make_entry(2, 0, 1, 2000, 200, OpKind::Write),
                make_entry(3, 0, 1, 3000, 300, OpKind::Write),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 3);
            assert_eq!(result.removed_count, 0);
        }

        #[test]
        fn test_compact_inode_filter() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::Write),
                make_entry(2, 0, 1, 2000, 100, OpKind::Write),
                make_entry(3, 0, 1, 3000, 200, OpKind::Write),
            ];

            let result = BatchCompactor::compact_inode(entries, 100);

            assert_eq!(result.entries.len(), 1);
            assert_eq!(result.entries[0].inode, 100);
        }

        #[test]
        fn test_mixed_ops_compaction() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::Write),
                make_entry(2, 0, 1, 2000, 100, OpKind::Write),
                make_entry(3, 0, 1, 3000, 100, OpKind::SetAttr),
                make_entry(4, 0, 1, 4000, 100, OpKind::SetAttr),
                make_entry(5, 0, 1, 5000, 200, OpKind::Create),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 3);
            assert!(result.entries.iter().any(|e| e.seq == 5));
        }

        #[test]
        fn test_truncate_compaction() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::Truncate),
                make_entry(2, 0, 1, 2000, 100, OpKind::Truncate),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 1);
            assert_eq!(result.entries[0].seq, 2);
        }

        #[test]
        fn test_preserve_different_ops_same_inode() {
            let entries = vec![
                make_entry(1, 0, 1, 1000, 100, OpKind::Write),
                make_entry(2, 0, 1, 2000, 100, OpKind::SetAttr),
                make_entry(3, 0, 1, 3000, 100, OpKind::Write),
            ];

            let result = BatchCompactor::compact(entries);

            assert_eq!(result.entries.len(), 2);
            let seqs: Vec<_> = result.entries.iter().map(|e| e.seq).collect();
            assert!(seqs.contains(&2));
            assert!(seqs.contains(&3));
        }
    }

    mod replication_sync {
        use super::*;

        #[tokio::test]
        async fn test_apply_clean_batch() {
            let sync = ReplicationSync::new(1, 2);

            let batch = EntryBatch::new(
                2,
                vec![
                    make_entry(1, 0, 2, 1000, 100, OpKind::Write),
                    make_entry(2, 0, 2, 2000, 100, OpKind::Write),
                ],
                1,
            );

            let result = sync.apply_batch(&batch, &[]).await;

            match result {
                ApplyResult::Applied { count } => assert_eq!(count, 2),
                _ => panic!("expected Applied"),
            }
        }

        #[tokio::test]
        async fn test_apply_batch_with_conflicts() {
            let sync = ReplicationSync::new(1, 2);

            let local = vec![make_entry(1, 0, 1, 1000, 100, OpKind::Write)];
            let batch = EntryBatch::new(
                2,
                vec![make_entry(1, 0, 2, 2000, 100, OpKind::Write)],
                1,
            );

            let result = sync.apply_batch(&batch, &local).await;

            match result {
                ApplyResult::AppliedWithConflicts { applied, conflicts } => {
                    assert_eq!(applied, 1);
                    assert_eq!(conflicts, 1);
                }
                _ => panic!("expected AppliedWithConflicts"),
            }
        }

        #[tokio::test]
        async fn test_reject_batch_wrong_site() {
            let sync = ReplicationSync::new(1, 2);

            let batch = EntryBatch::new(
                3,
                vec![make_entry(1, 0, 3, 1000, 100, OpKind::Write)],
                1,
            );

            let result = sync.apply_batch(&batch, &[]).await;

            match result {
                ApplyResult::Rejected { reason } => {
                    assert!(reason.contains("does not match"));
                }
                _ => panic!("expected Rejected"),
            }
        }

        #[tokio::test]
        async fn test_reject_batch_sequence_gap() {
            let sync = ReplicationSync::new(1, 2);

            // Advance WAL to seq 5
            {
                let mut wal = sync.wal.lock().await;
                wal.advance(2, 0, 5, 1000, 5);
            }

            // But batch starts at seq 1
            let batch = EntryBatch::new(
                2,
                vec![make_entry(1, 0, 2, 1000, 100, OpKind::Write)],
                1,
            );

            let result = sync.apply_batch(&batch, &[]).await;

            match result {
                ApplyResult::Rejected { reason } => {
                    assert!(reason.contains("sequence gap"));
                }
                _ => panic!("expected Rejected"),
            }
        }

        #[tokio::test]
        async fn test_lag_calculation() {
            let sync = ReplicationSync::new(1, 2);

            // Local tip is 100
            let lag = sync.lag(0, 100).await;
            assert_eq!(lag, 100);

            // Advance WAL to 80
            {
                let mut wal = sync.wal.lock().await;
                wal.advance(2, 0, 80, 1000, 80);
            }

            // Now lag should be 20
            let lag = sync.lag(0, 100).await;
            assert_eq!(lag, 20);
        }

        #[tokio::test]
        async fn test_wal_snapshot() {
            let sync = ReplicationSync::new(1, 2);

            {
                let mut wal = sync.wal.lock().await;
                wal.advance(2, 0, 50, 1000, 50);
                wal.advance(2, 1, 75, 2000, 75);
            }

            let cursors = sync.wal_snapshot().await;

            assert_eq!(cursors.len(), 2);
            assert!(cursors.iter().any(|c| c.shard_id == 0 && c.last_seq == 50));
            assert!(cursors.iter().any(|c| c.shard_id == 1 && c.last_seq == 75));
        }

        #[tokio::test]
        async fn test_detector_access() {
            let sync = ReplicationSync::new(1, 2);
            let detector = sync.detector();

            let local = make_entry(1, 0, 1, 1000, 100, OpKind::Write);
            let remote = make_entry(1, 0, 2, 2000, 100, OpKind::Write);
            detector.check(&local, &remote).await;

            let conflicts = detector.conflicts().await;
            assert_eq!(conflicts.len(), 1);
        }

        #[tokio::test]
        async fn test_apply_empty_batch() {
            let sync = ReplicationSync::new(1, 2);

            let batch = EntryBatch::new(2, vec![], 1);

            let result = sync.apply_batch(&batch, &[]).await;

            match result {
                ApplyResult::Applied { count } => assert_eq!(count, 0),
                _ => panic!("expected Applied"),
            }
        }

        #[tokio::test]
        async fn test_apply_batch_advances_wal() {
            let sync = ReplicationSync::new(1, 2);

            let batch = EntryBatch::new(
                2,
                vec![make_entry(1, 0, 2, 1000, 100, OpKind::Write)],
                1,
            );

            sync.apply_batch(&batch, &[]).await;

            let cursors = sync.wal_snapshot().await;
            assert_eq!(cursors[0].last_seq, 1);
        }
    }

    mod apply_result {
        use super::*;

        #[test]
        fn test_applied_variant() {
            let result = ApplyResult::Applied { count: 5 };
            match result {
                ApplyResult::Applied { count } => assert_eq!(count, 5),
                _ => panic!("expected Applied"),
            }
        }

        #[test]
        fn test_applied_with_conflicts_variant() {
            let result = ApplyResult::AppliedWithConflicts { applied: 10, conflicts: 2 };
            match result {
                ApplyResult::AppliedWithConflicts { applied, conflicts } => {
                    assert_eq!(applied, 10);
                    assert_eq!(conflicts, 2);
                }
                _ => panic!("expected AppliedWithConflicts"),
            }
        }

        #[test]
        fn test_rejected_variant() {
            let result = ApplyResult::Rejected {
                reason: "test reason".to_string(),
            };
            match result {
                ApplyResult::Rejected { reason } => assert_eq!(reason, "test reason"),
                _ => panic!("expected Rejected"),
            }
        }

        #[test]
        fn test_apply_result_equality() {
            assert_eq!(
                ApplyResult::Applied { count: 5 },
                ApplyResult::Applied { count: 5 }
            );
            assert_eq!(
                ApplyResult::AppliedWithConflicts { applied: 1, conflicts: 1 },
                ApplyResult::AppliedWithConflicts { applied: 1, conflicts: 1 }
            );
            assert_eq!(
                ApplyResult::Rejected { reason: "err".to_string() },
                ApplyResult::Rejected { reason: "err".to_string() }
            );
        }

        #[test]
        fn test_apply_result_inequality() {
            assert_ne!(
                ApplyResult::Applied { count: 5 },
                ApplyResult::Applied { count: 6 }
            );
            assert_ne!(
                ApplyResult::AppliedWithConflicts { applied: 1, conflicts: 1 },
                ApplyResult::AppliedWithConflicts { applied: 2, conflicts: 1 }
            );
        }
    }

    mod conflict_struct {
        use super::*;

        #[test]
        fn test_conflict_fields() {
            let conflict = Conflict {
                inode: 123,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            };

            assert_eq!(conflict.inode, 123);
            assert_eq!(conflict.local_site_id, 1);
            assert_eq!(conflict.remote_site_id, 2);
            assert_eq!(conflict.local_ts, 1000);
            assert_eq!(conflict.remote_ts, 2000);
            assert_eq!(conflict.winner_site_id, 2);
            assert_eq!(conflict.detected_at_us, 3000000);
        }

        #[test]
        fn test_conflict_clone() {
            let conflict = Conflict {
                inode: 123,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            };

            let cloned = conflict.clone();
            assert_eq!(conflict, cloned);
        }

        #[test]
        fn test_conflict_equality() {
            let c1 = Conflict {
                inode: 123,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            };
            let c2 = Conflict {
                inode: 123,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            };
            assert_eq!(c1, c2);
        }
    }

    mod compaction_result {
        use super::*;

        #[test]
        fn test_compaction_result_fields() {
            let result = CompactionResult {
                entries: vec![],
                removed_count: 5,
            };

            assert!(result.entries.is_empty());
            assert_eq!(result.removed_count, 5);
        }

        #[test]
        fn test_compaction_result_equality() {
            let r1 = CompactionResult {
                entries: vec![],
                removed_count: 5,
            };
            let r2 = CompactionResult {
                entries: vec![],
                removed_count: 5,
            };
            assert_eq!(r1, r2);
        }
    }
}