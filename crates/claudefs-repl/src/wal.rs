//! Replication WAL (Write-Ahead Log) tracks which journal entries have been
//! successfully replicated to each remote site.

use serde::{Deserialize, Serialize};

/// A site+sequence position representing how far replication has advanced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationCursor {
    /// Remote site we are replicating TO.
    pub site_id: u64,
    /// Virtual shard ID.
    pub shard_id: u32,
    /// Last sequence number successfully replicated to remote.
    pub last_seq: u64,
}

impl ReplicationCursor {
    /// Create a new replication cursor.
    pub fn new(site_id: u64, shard_id: u32, last_seq: u64) -> Self {
        Self {
            site_id,
            shard_id,
            last_seq,
        }
    }
}

/// A single WAL record written when we advance the cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalRecord {
    /// The cursor position after this advance.
    pub cursor: ReplicationCursor,
    /// Timestamp when replication was confirmed (microseconds since epoch).
    pub replicated_at_us: u64,
    /// How many entries this advance covers.
    pub entry_count: u32,
}

/// The replication WAL is an in-memory (later: persisted) log of replication
/// progress. After restart, we resume from the last confirmed cursor.
#[derive(Debug, Default)]
pub struct ReplicationWal {
    cursors: std::collections::HashMap<(u64, u32), u64>,
    history: Vec<WalRecord>,
}

impl ReplicationWal {
    /// Create a new empty replication WAL.
    pub fn new() -> Self {
        Self {
            cursors: std::collections::HashMap::new(),
            history: Vec::new(),
        }
    }

    /// Record that entries up to `seq` have been replicated to `site_id/shard`.
    pub fn advance(
        &mut self,
        site_id: u64,
        shard_id: u32,
        seq: u64,
        replicated_at_us: u64,
        entry_count: u32,
    ) {
        let key = (site_id, shard_id);
        let old_seq = self.cursors.get(&key).copied().unwrap_or(0);
        let count = if seq > old_seq {
            (seq - old_seq) as u32
        } else {
            0
        };

        self.cursors.insert(key, seq);

        self.history.push(WalRecord {
            cursor: ReplicationCursor::new(site_id, shard_id, seq),
            replicated_at_us,
            entry_count: if count > 0 { count } else { entry_count },
        });
    }

    /// Get the current cursor for a (site_id, shard_id) pair. Returns seq=0 if unknown.
    pub fn cursor(&self, site_id: u64, shard_id: u32) -> ReplicationCursor {
        let seq = self.cursors.get(&(site_id, shard_id)).copied().unwrap_or(0);
        ReplicationCursor::new(site_id, shard_id, seq)
    }

    /// Get all cursors (snapshot of current state).
    pub fn all_cursors(&self) -> Vec<ReplicationCursor> {
        let mut cursors: Vec<_> = self
            .cursors
            .iter()
            .map(|((site_id, shard_id), &seq)| ReplicationCursor::new(*site_id, *shard_id, seq))
            .collect();
        cursors.sort_by_key(|c| (c.site_id, c.shard_id));
        cursors
    }

    /// Reset the cursor for a site (used when a site is removed or reset).
    pub fn reset(&mut self, site_id: u64, shard_id: u32) {
        self.cursors.remove(&(site_id, shard_id));
    }

    /// Returns the WAL history as a slice of records (most recent last).
    pub fn history(&self) -> &[WalRecord] {
        &self.history
    }

    /// Compact history older than `before_us` (keep at least the latest per cursor).
    pub fn compact(&mut self, before_us: u64) {
        let mut indices_to_keep: std::collections::HashSet<usize> =
            std::collections::HashSet::new();

        for (i, record) in self.history.iter().enumerate() {
            if record.replicated_at_us >= before_us {
                indices_to_keep.insert(i);
            }
        }

        if indices_to_keep.is_empty() {
            if !self.history.is_empty() {
                let mut latest_per_cursor: std::collections::HashMap<(u64, u32), usize> =
                    std::collections::HashMap::new();
                for (i, record) in self.history.iter().enumerate() {
                    let key = (record.cursor.site_id, record.cursor.shard_id);
                    latest_per_cursor
                        .entry(key)
                        .and_modify(|existing| *existing = std::cmp::max(*existing, i))
                        .or_insert(i);
                }
                let cursor_count = latest_per_cursor.len();
                if cursor_count > 1 {
                    let mut new_history = Vec::new();
                    for (i, record) in self.history.drain(..).enumerate() {
                        if latest_per_cursor.values().any(|&idx| idx == i) {
                            new_history.push(record);
                        }
                    }
                    self.history = new_history;
                    return;
                }
            }
            self.history.clear();
            return;
        }

        let mut cursor_indices: std::collections::HashMap<(u64, u32), Vec<usize>> =
            std::collections::HashMap::new();
        for (i, record) in self.history.iter().enumerate() {
            let key = (record.cursor.site_id, record.cursor.shard_id);
            cursor_indices.entry(key).or_default().push(i);
        }

        for (_key, indices) in cursor_indices.iter() {
            if indices.is_empty() {
                continue;
            }

            let mut kept_indices_in_chain: Vec<usize> = indices
                .iter()
                .filter(|&&i| indices_to_keep.contains(&i))
                .copied()
                .collect();
            kept_indices_in_chain.sort();

            for &kept_idx in &kept_indices_in_chain {
                if let Some(pos) = indices.iter().position(|&i| i == kept_idx) {
                    if pos > 0 {
                        indices_to_keep.insert(indices[pos - 1]);
                    }
                }
            }
        }

        let mut new_history = Vec::new();
        for (i, record) in self.history.drain(..).enumerate() {
            if indices_to_keep.contains(&i) {
                new_history.push(record);
            }
        }
        self.history = new_history;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advance_and_read_back() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 100, 1700000000000000, 100);

        let cursor = wal.cursor(1, 0);
        assert_eq!(cursor.site_id, 1);
        assert_eq!(cursor.shard_id, 0);
        assert_eq!(cursor.last_seq, 100);
    }

    #[test]
    fn test_advance_multiple_sites() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 50, 1700000000000000, 50);
        wal.advance(2, 0, 75, 1700000000000001, 75);
        wal.advance(3, 0, 100, 1700000000000002, 100);

        assert_eq!(wal.cursor(1, 0).last_seq, 50);
        assert_eq!(wal.cursor(2, 0).last_seq, 75);
        assert_eq!(wal.cursor(3, 0).last_seq, 100);
    }

    #[test]
    fn test_advance_multiple_shards() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 100, 1700000000000000, 100);
        wal.advance(1, 1, 200, 1700000000000001, 200);
        wal.advance(1, 2, 300, 1700000000000002, 300);

        assert_eq!(wal.cursor(1, 0).last_seq, 100);
        assert_eq!(wal.cursor(1, 1).last_seq, 200);
        assert_eq!(wal.cursor(1, 2).last_seq, 300);
    }

    #[test]
    fn test_cursor_unknown_returns_zero() {
        let wal = ReplicationWal::new();
        let cursor = wal.cursor(999, 0);
        assert_eq!(cursor.last_seq, 0);
    }

    #[test]
    fn test_all_cursors() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 100, 1700000000000000, 100);
        wal.advance(2, 0, 200, 1700000000000001, 200);
        wal.advance(1, 1, 150, 1700000000000002, 150);

        let cursors = wal.all_cursors();
        assert_eq!(cursors.len(), 3);
        assert_eq!(cursors[0].site_id, 1);
        assert_eq!(cursors[0].shard_id, 0);
        assert_eq!(cursors[1].site_id, 1);
        assert_eq!(cursors[1].shard_id, 1);
        assert_eq!(cursors[2].site_id, 2);
        assert_eq!(cursors[2].shard_id, 0);
    }

    #[test]
    fn test_history_ordering() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 10, 1000, 10);
        wal.advance(1, 0, 20, 2000, 10);
        wal.advance(1, 0, 30, 3000, 10);

        let history = wal.history();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].cursor.last_seq, 10);
        assert_eq!(history[1].cursor.last_seq, 20);
        assert_eq!(history[2].cursor.last_seq, 30);
    }

    #[test]
    fn test_reset() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 100, 1700000000000000, 100);

        wal.reset(1, 0);

        let cursor = wal.cursor(1, 0);
        assert_eq!(cursor.last_seq, 0);
    }

    #[test]
    fn test_reset_specific_shard() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 100, 1700000000000000, 100);
        wal.advance(1, 1, 200, 1700000000000001, 200);

        wal.reset(1, 0);

        assert_eq!(wal.cursor(1, 0).last_seq, 0);
        assert_eq!(wal.cursor(1, 1).last_seq, 200);
    }

    #[test]
    fn test_compaction_keeps_recent() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 10, 1000, 10);
        wal.advance(1, 0, 20, 2000, 10);
        wal.advance(1, 0, 30, 3000, 10);

        wal.compact(2500);

        let history = wal.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].cursor.last_seq, 20);
        assert_eq!(history[1].cursor.last_seq, 30);
    }

    #[test]
    fn test_compaction_keeps_latest_per_cursor() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 10, 1000, 10);
        wal.advance(1, 1, 15, 1500, 15);
        wal.advance(1, 0, 20, 2000, 10);
        wal.advance(1, 1, 25, 2500, 10);

        wal.compact(3000);

        let history = wal.history();
        assert_eq!(history.len(), 2);
        assert!(history
            .iter()
            .any(|r| r.cursor.shard_id == 0 && r.cursor.last_seq == 20));
        assert!(history
            .iter()
            .any(|r| r.cursor.shard_id == 1 && r.cursor.last_seq == 25));
    }

    #[test]
    fn test_compaction_removes_old() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 10, 1000, 10);
        wal.advance(1, 0, 20, 2000, 10);

        wal.compact(5000);

        assert!(wal.history().is_empty());
    }

    #[test]
    fn test_advance_overwrites() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 50, 1700000000000000, 50);
        wal.advance(1, 0, 100, 1700000000000001, 50);

        let cursor = wal.cursor(1, 0);
        assert_eq!(cursor.last_seq, 100);

        let history = wal.history();
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_advance_same_seq() {
        let mut wal = ReplicationWal::new();
        wal.advance(1, 0, 100, 1700000000000000, 100);
        wal.advance(1, 0, 100, 1700000000000001, 0);

        assert_eq!(wal.cursor(1, 0).last_seq, 100);
    }

    #[test]
    fn test_cursor_struct_equality() {
        let c1 = ReplicationCursor::new(1, 0, 100);
        let c2 = ReplicationCursor::new(1, 0, 100);
        let c3 = ReplicationCursor::new(1, 0, 101);

        assert_eq!(c1, c2);
        assert_ne!(c1, c3);
    }

    #[test]
    fn test_walrecord_struct() {
        let record = WalRecord {
            cursor: ReplicationCursor::new(1, 0, 100),
            replicated_at_us: 1700000000000000,
            entry_count: 50,
        };

        assert_eq!(record.cursor.site_id, 1);
        assert_eq!(record.cursor.shard_id, 0);
        assert_eq!(record.cursor.last_seq, 100);
        assert_eq!(record.replicated_at_us, 1700000000000000);
        assert_eq!(record.entry_count, 50);
    }

    #[test]
    fn test_many_shards() {
        let mut wal = ReplicationWal::new();
        for shard_id in 0..256u32 {
            wal.advance(
                1,
                shard_id,
                shard_id as u64 * 100,
                1700000000000000 + shard_id as u64,
                100,
            );
        }

        let cursors = wal.all_cursors();
        assert_eq!(cursors.len(), 256);
    }

    #[test]
    fn test_all_cursors_empty() {
        let wal = ReplicationWal::new();
        assert!(wal.all_cursors().is_empty());
    }

    #[test]
    fn test_history_empty() {
        let wal = ReplicationWal::new();
        assert!(wal.history().is_empty());
    }

    #[test]
    fn test_new_creates_empty_wal() {
        let wal = ReplicationWal::new();
        assert!(wal.all_cursors().is_empty());
        assert!(wal.history().is_empty());
    }
}
