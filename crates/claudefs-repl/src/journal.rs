//! Journal entry types and tailer for cross-site replication.
//!
//! The journal records filesystem operations from the metadata service,
//! which are then replicated to remote sites via the conduit.

use serde::{Deserialize, Serialize};

/// Compute CRC32 using the standard IEEE 802.3 polynomial (0xEDB88320).
fn compute_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Filesystem operation kind recorded in the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    /// Create file/dir/symlink.
    Create,
    /// Unlink/rmdir.
    Unlink,
    /// Rename (src, dst).
    Rename,
    /// Write data range.
    Write,
    /// Truncate to length.
    Truncate,
    /// chmod/chown/utimes.
    SetAttr,
    /// Hard link.
    Link,
    /// Symlink target.
    Symlink,
    /// mkdir (distinct for POSIX semantics).
    MkDir,
    /// Extended attribute set.
    SetXattr,
    /// Extended attribute remove.
    RemoveXattr,
}

/// A single journal entry written by the metadata service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number, per-shard.
    pub seq: u64,
    /// Which virtual shard (0..255).
    pub shard_id: u32,
    /// Originating site.
    pub site_id: u64,
    /// Microseconds since Unix epoch (for LWW).
    pub timestamp_us: u64,
    /// Affected inode.
    pub inode: u64,
    /// Operation type.
    pub op: OpKind,
    /// Serialized operation details (bincode).
    pub payload: Vec<u8>,
    /// CRC32 checksum of (seq, shard_id, site_id, timestamp_us, inode, op_discriminant, payload).
    pub crc32: u32,
}

impl JournalEntry {
    /// Compute the CRC32 for this entry (excluding the crc32 field itself).
    pub fn compute_crc(&self) -> u32 {
        let mut data = Vec::new();
        data.extend_from_slice(&self.seq.to_le_bytes());
        data.extend_from_slice(&self.shard_id.to_le_bytes());
        data.extend_from_slice(&self.site_id.to_le_bytes());
        data.extend_from_slice(&self.timestamp_us.to_le_bytes());
        data.extend_from_slice(&self.inode.to_le_bytes());
        data.push(match self.op {
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
        });
        data.extend_from_slice(&self.payload);
        compute_crc32(&data)
    }

    /// Validate the CRC32 of this entry.
    pub fn validate_crc(&self) -> bool {
        self.crc32 == self.compute_crc()
    }

    /// Create a new entry with the CRC computed automatically.
    pub fn new(
        seq: u64,
        shard_id: u32,
        site_id: u64,
        timestamp_us: u64,
        inode: u64,
        op: OpKind,
        payload: Vec<u8>,
    ) -> Self {
        let mut entry = Self {
            seq,
            shard_id,
            site_id,
            timestamp_us,
            inode,
            op,
            payload,
            crc32: 0,
        };
        entry.crc32 = entry.compute_crc();
        entry
    }
}

/// Position within a journal: shard + sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalPosition {
    /// Virtual shard ID.
    pub shard_id: u32,
    /// Sequence number within the shard.
    pub seq: u64,
}

impl JournalPosition {
    /// Create a new journal position.
    pub fn new(shard_id: u32, seq: u64) -> Self {
        Self { shard_id, seq }
    }
}

/// JournalTailer streams entries starting from a given position.
///
/// In production, this reads from the Raft journal (A2). For now,
/// it uses an in-memory buffer for testing.
#[derive(Debug)]
pub struct JournalTailer {
    entries: Vec<JournalEntry>,
    index: usize,
}

impl JournalTailer {
    /// Create a new tailer backed by an in-memory entry buffer.
    pub fn new_in_memory(entries: Vec<JournalEntry>) -> Self {
        let mut tailer = Self { entries, index: 0 };
        tailer.entries.sort_by_key(|e| (e.shard_id, e.seq));
        tailer
    }

    /// Create a new tailer starting from the given position.
    pub fn new_from_position(entries: Vec<JournalEntry>, pos: JournalPosition) -> Self {
        let mut tailer = Self {
            entries,
            index: 0,
        };
        tailer.entries.sort_by_key(|e| (e.shard_id, e.seq));
        tailer.index = tailer
            .entries
            .iter()
            .position(|e| e.shard_id == pos.shard_id && e.seq >= pos.seq)
            .unwrap_or(tailer.entries.len());
        tailer
    }

    /// Return the next entry, or None if the journal is at the tip.
    pub async fn next(&mut self) -> Option<JournalEntry> {
        if self.index < self.entries.len() {
            let entry = self.entries[self.index].clone();
            self.index += 1;
            Some(entry)
        } else {
            None
        }
    }

    /// Return the current read position.
    pub fn position(&self) -> Option<JournalPosition> {
        if self.index < self.entries.len() {
            let e = &self.entries[self.index];
            Some(JournalPosition::new(e.shard_id, e.seq))
        } else if !self.entries.is_empty() {
            let e = &self.entries[self.entries.len() - 1];
            Some(JournalPosition::new(e.shard_id, e.seq + 1))
        } else {
            None
        }
    }

    /// Add entries (used in tests to simulate journal appends).
    pub fn append(&mut self, entry: JournalEntry) {
        self.entries.push(entry);
        self.entries.sort_by_key(|e| (e.shard_id, e.seq));
        if self.index > self.entries.len() {
            self.index = self.entries.len();
        }
    }

    /// Filter entries by shard ID.
    pub fn filter_by_shard(&self, shard_id: u32) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|e| e.shard_id == shard_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialize_entry(entry: &JournalEntry) -> Vec<u8> {
        bincode::serialize(entry).unwrap()
    }

    fn deserialize_entry(data: &[u8]) -> JournalEntry {
        bincode::deserialize(data).unwrap()
    }

    #[test]
    fn test_journal_entry_bincode_roundtrip() {
        let entry = JournalEntry::new(
            100,
            5,
            1,
            1700000000000000,
            12345,
            OpKind::Write,
            vec![1, 2, 3, 4, 5],
        );

        let encoded = serialize_entry(&entry);
        let decoded: JournalEntry = deserialize_entry(&encoded);

        assert_eq!(entry.seq, decoded.seq);
        assert_eq!(entry.shard_id, decoded.shard_id);
        assert_eq!(entry.site_id, decoded.site_id);
        assert_eq!(entry.timestamp_us, decoded.timestamp_us);
        assert_eq!(entry.inode, decoded.inode);
        assert_eq!(entry.op, decoded.op);
        assert_eq!(entry.payload, decoded.payload);
        assert_eq!(entry.crc32, decoded.crc32);
    }

    #[test]
    fn test_journal_entry_all_opkinds() {
        let opkinds = vec![
            OpKind::Create,
            OpKind::Unlink,
            OpKind::Rename,
            OpKind::Write,
            OpKind::Truncate,
            OpKind::SetAttr,
            OpKind::Link,
            OpKind::Symlink,
            OpKind::MkDir,
            OpKind::SetXattr,
            OpKind::RemoveXattr,
        ];

        for (i, op) in opkinds.into_iter().enumerate() {
            let entry = JournalEntry::new(
                i as u64,
                0,
                1,
                1700000000000000,
                100 + i as u64,
                op,
                vec![],
            );

            let encoded = serialize_entry(&entry);
            let decoded: JournalEntry = deserialize_entry(&encoded);
            assert_eq!(entry.op, decoded.op);
        }
    }

    #[test]
    fn test_journal_entry_crc32_validation() {
        let entry = JournalEntry::new(
            42,
            3,
            7,
            1700000000000000,
            999,
            OpKind::Create,
            b"hello world".to_vec(),
        );

        assert!(entry.validate_crc());

        let mut bad_entry = entry.clone();
        bad_entry.crc32 = 0xDEADBEEF;
        assert!(!bad_entry.validate_crc());
    }

    #[test]
    fn test_journal_entry_crc_deterministic() {
        let entry1 = JournalEntry::new(
            1,
            1,
            1,
            1000,
            10,
            OpKind::Write,
            vec![1, 2, 3],
        );
        let entry2 = JournalEntry::new(
            1,
            1,
            1,
            1000,
            10,
            OpKind::Write,
            vec![1, 2, 3],
        );

        assert_eq!(entry1.crc32, entry2.crc32);
    }

    #[test]
    fn test_journal_entry_different_payloads_different_crc() {
        let entry1 = JournalEntry::new(
            1,
            1,
            1,
            1000,
            10,
            OpKind::Write,
            vec![1, 2, 3],
        );
        let entry2 = JournalEntry::new(
            1,
            1,
            1,
            1000,
            10,
            OpKind::Write,
            vec![1, 2, 4],
        );

        assert_ne!(entry1.crc32, entry2.crc32);
    }

    #[tokio::test]
    async fn test_tailer_next_returns_entries_in_order() {
        let entries = vec![
            JournalEntry::new(1, 0, 1, 1000, 10, OpKind::Create, vec![]),
            JournalEntry::new(2, 0, 1, 1001, 10, OpKind::Write, vec![]),
            JournalEntry::new(3, 0, 1, 1002, 10, OpKind::Truncate, vec![]),
        ];
        let mut tailer = JournalTailer::new_in_memory(entries);

        let e1 = tailer.next().await;
        assert!(e1.is_some());
        assert_eq!(e1.unwrap().seq, 1);

        let e2 = tailer.next().await;
        assert!(e2.is_some());
        assert_eq!(e2.unwrap().seq, 2);

        let e3 = tailer.next().await;
        assert!(e3.is_some());
        assert_eq!(e3.unwrap().seq, 3);

        let e4 = tailer.next().await;
        assert!(e4.is_none());
    }

    #[tokio::test]
    async fn test_tailer_new_from_position() {
        let entries = vec![
            JournalEntry::new(1, 0, 1, 1000, 10, OpKind::Create, vec![]),
            JournalEntry::new(2, 0, 1, 1001, 10, OpKind::Write, vec![]),
            JournalEntry::new(3, 0, 1, 1002, 10, OpKind::Truncate, vec![]),
        ];
        let pos = JournalPosition::new(0, 2);
        let mut tailer = JournalTailer::new_from_position(entries, pos);

        let e = tailer.next().await;
        assert!(e.is_some());
        assert_eq!(e.unwrap().seq, 2);
    }

    #[test]
    fn test_tailer_position() {
        let entries = vec![
            JournalEntry::new(1, 0, 1, 1000, 10, OpKind::Create, vec![]),
            JournalEntry::new(2, 0, 1, 1001, 10, OpKind::Write, vec![]),
        ];
        let tailer = JournalTailer::new_in_memory(entries);

        let pos = tailer.position();
        assert!(pos.is_some());
        assert_eq!(pos.unwrap().seq, 1);
    }

    #[tokio::test]
    async fn test_tailer_append() {
        let entries = vec![JournalEntry::new(1, 0, 1, 1000, 10, OpKind::Create, vec![])];
        let mut tailer = JournalTailer::new_in_memory(entries);

        tailer.append(JournalEntry::new(2, 0, 1, 1001, 10, OpKind::Write, vec![]));
        tailer.append(JournalEntry::new(0, 0, 1, 999, 10, OpKind::MkDir, vec![]));

        let e = tailer.next().await;
        assert!(e.is_some());
        assert_eq!(e.unwrap().seq, 0);

        let e = tailer.next().await;
        assert!(e.is_some());
        assert_eq!(e.unwrap().seq, 1);

        let e = tailer.next().await;
        assert!(e.is_some());
        assert_eq!(e.unwrap().seq, 2);
    }

    #[test]
    fn test_tailer_filter_by_shard() {
        let entries = vec![
            JournalEntry::new(1, 0, 1, 1000, 10, OpKind::Create, vec![]),
            JournalEntry::new(2, 1, 1, 1001, 11, OpKind::Write, vec![]),
            JournalEntry::new(3, 0, 1, 1002, 12, OpKind::Truncate, vec![]),
            JournalEntry::new(4, 2, 1, 1003, 13, OpKind::MkDir, vec![]),
        ];
        let tailer = JournalTailer::new_in_memory(entries);

        let shard0 = tailer.filter_by_shard(0);
        assert_eq!(shard0.len(), 2);
        assert!(shard0.iter().all(|e| e.shard_id == 0));

        let shard1 = tailer.filter_by_shard(1);
        assert_eq!(shard1.len(), 1);
        assert!(shard1.iter().all(|e| e.shard_id == 1));
    }

    #[test]
    fn test_journal_position_equality() {
        let pos1 = JournalPosition::new(5, 100);
        let pos2 = JournalPosition::new(5, 100);
        let pos3 = JournalPosition::new(5, 101);
        let pos4 = JournalPosition::new(6, 100);

        assert_eq!(pos1, pos2);
        assert_ne!(pos1, pos3);
        assert_ne!(pos1, pos4);
    }

    #[test]
    fn test_journal_entry_clone() {
        let entry = JournalEntry::new(
            1,
            0,
            1,
            1000,
            10,
            OpKind::Create,
            vec![1, 2, 3],
        );
        let cloned = entry.clone();

        assert_eq!(entry.seq, cloned.seq);
        assert_eq!(entry.shard_id, cloned.shard_id);
        assert_eq!(entry.op, cloned.op);
    }

    #[tokio::test]
    async fn test_tailer_empty() {
        let tailer = JournalTailer::new_in_memory(vec![]);
        assert!(tailer.position().is_none());
    }

    #[tokio::test]
    async fn test_tailer_sorts_by_shard_then_seq() {
        let entries = vec![
            JournalEntry::new(5, 1, 1, 1005, 10, OpKind::Create, vec![]),
            JournalEntry::new(1, 0, 1, 1001, 10, OpKind::Create, vec![]),
            JournalEntry::new(3, 1, 1, 1003, 10, OpKind::Create, vec![]),
            JournalEntry::new(2, 0, 1, 1002, 10, OpKind::Create, vec![]),
        ];
        let mut tailer = JournalTailer::new_in_memory(entries);

        let e1 = tailer.next().await.unwrap();
        assert_eq!(e1.seq, 1);
        assert_eq!(e1.shard_id, 0);

        let e2 = tailer.next().await.unwrap();
        assert_eq!(e2.seq, 2);
        assert_eq!(e2.shard_id, 0);

        let e3 = tailer.next().await.unwrap();
        assert_eq!(e3.seq, 3);
        assert_eq!(e3.shard_id, 1);

        let e4 = tailer.next().await.unwrap();
        assert_eq!(e4.seq, 5);
        assert_eq!(e4.shard_id, 1);
    }

    #[test]
    fn test_large_payload_roundtrip() {
        let payload: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let entry = JournalEntry::new(
            1,
            0,
            1,
            1000,
            10,
            OpKind::Write,
            payload.clone(),
        );

        let encoded = serialize_entry(&entry);
        let decoded: JournalEntry = deserialize_entry(&encoded);

        assert_eq!(decoded.payload, payload);
        assert!(decoded.validate_crc());
    }
}