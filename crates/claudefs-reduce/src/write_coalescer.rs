//! Write coalescing to merge adjacent or overlapping writes before the pipeline.
//!
//! Small sequential writes to the same inode at adjacent offsets can be coalesced
//! into a single larger write, improving throughput and dedup effectiveness.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for write coalescing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoalesceConfig {
    /// Maximum gap between writes to still coalesce (0 = only adjacent)
    pub max_gap_bytes: u64,
    /// Maximum size of coalesced write
    pub max_coalesced_bytes: u64,
    /// Time window in milliseconds for coalescing
    pub window_ms: u64,
}

impl Default for CoalesceConfig {
    fn default() -> Self {
        Self {
            max_gap_bytes: 0,
            max_coalesced_bytes: 8 * 1024 * 1024,
            window_ms: 50,
        }
    }
}

/// A single write operation.
#[derive(Debug, Clone)]
pub struct WriteOp {
    /// Target inode ID
    pub inode_id: u64,
    /// Write offset in bytes
    pub offset: u64,
    /// Data to write
    pub data: Vec<u8>,
    /// Timestamp of the write in milliseconds
    pub timestamp_ms: u64,
}

impl WriteOp {
    /// End offset (exclusive) of this write
    pub fn end_offset(&self) -> u64 {
        self.offset + self.data.len() as u64
    }
}

/// A coalesced write combining multiple WriteOps.
#[derive(Debug)]
pub struct CoalescedWrite {
    /// Target inode ID
    pub inode_id: u64,
    /// Starting offset
    pub offset: u64,
    /// Coalesced data
    pub data: Vec<u8>,
    /// Number of source writes combined
    pub source_count: usize,
}

/// Pending writes per inode.
#[derive(Debug, Default)]
struct PendingWrites {
    writes: Vec<WriteOp>,
    earliest_timestamp_ms: u64,
}

/// Write coalescer that buffers and merges adjacent writes.
#[derive(Debug)]
pub struct WriteCoalescer {
    config: CoalesceConfig,
    pending: HashMap<u64, PendingWrites>,
}

impl WriteCoalescer {
    /// Create a new coalescer with the given configuration
    pub fn new(config: CoalesceConfig) -> Self {
        Self {
            config,
            pending: HashMap::new(),
        }
    }

    /// Add a write operation to the buffer
    pub fn add(&mut self, op: WriteOp) {
        let entry = self.pending.entry(op.inode_id).or_default();
        if entry.writes.is_empty() {
            entry.earliest_timestamp_ms = op.timestamp_ms;
        }
        entry.writes.push(op);
    }

    /// Return coalesced writes where the time window has expired
    pub fn flush_ready(&mut self, now_ms: u64) -> Vec<CoalescedWrite> {
        let mut result = Vec::new();
        let inodes: Vec<u64> = self.pending.keys().copied().collect();

        for inode_id in inodes {
            if let Some(pending) = self.pending.get(&inode_id) {
                if now_ms >= pending.earliest_timestamp_ms + self.config.window_ms {
                    if let Some(coalesced) = self.flush_inode(inode_id) {
                        result.push(coalesced);
                    }
                }
            }
        }

        result
    }

    /// Force flush all pending writes for an inode
    pub fn flush_inode(&mut self, inode_id: u64) -> Option<CoalescedWrite> {
        let pending = self.pending.remove(&inode_id)?;
        let mut writes = pending.writes;

        if writes.is_empty() {
            return None;
        }

        writes.sort_by_key(|w| w.offset);
        let mut coalesced = Self::coalesce_sorted(writes, self.config.max_coalesced_bytes);
        if coalesced.is_empty() {
            return None;
        }

        Some(coalesced.remove(0))
    }

    /// Flush all pending writes across all inodes
    pub fn flush_all(&mut self) -> Vec<CoalescedWrite> {
        let mut result = Vec::new();

        let inodes: Vec<u64> = self.pending.keys().copied().collect();
        for inode_id in inodes {
            if let Some(coalesced) = self.flush_inode(inode_id) {
                result.push(coalesced);
            }
        }

        result
    }

    /// Number of pending write operations
    pub fn pending_count(&self) -> usize {
        self.pending.values().map(|p| p.writes.len()).sum()
    }

    fn coalesce_sorted(writes: Vec<WriteOp>, max_bytes: u64) -> Vec<CoalescedWrite> {
        if writes.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();
        let mut current_writes: Vec<WriteOp> = vec![writes[0].clone()];
        let mut current_size = writes[0].data.len() as u64;
        let mut current_end = writes[0].end_offset();

        for write in writes.into_iter().skip(1) {
            let write_len = write.data.len() as u64;
            let write_end = write.end_offset();
            if write.offset == current_end && current_size + write_len <= max_bytes {
                current_writes.push(write);
                current_size += write_len;
                current_end = write_end;
            } else {
                result.push(Self::merge_writes(std::mem::take(&mut current_writes)));
                current_writes = vec![write];
                current_size = write_len;
                current_end = write_end;
            }
        }

        if !current_writes.is_empty() {
            result.push(Self::merge_writes(current_writes));
        }

        result
    }

    fn merge_writes(writes: Vec<WriteOp>) -> CoalescedWrite {
        if writes.is_empty() {
            panic!("Cannot merge empty writes");
        }

        let inode_id = writes[0].inode_id;
        let offset = writes[0].offset;
        let source_count = writes.len();

        let mut data = Vec::new();
        for write in writes {
            data.extend_from_slice(&write.data);
        }

        CoalescedWrite {
            inode_id,
            offset,
            data,
            source_count,
        }
    }
}

impl Default for WriteCoalescer {
    fn default() -> Self {
        Self::new(CoalesceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_config_default() {
        let config = CoalesceConfig::default();
        assert_eq!(config.max_gap_bytes, 0);
        assert_eq!(config.max_coalesced_bytes, 8 * 1024 * 1024);
        assert_eq!(config.window_ms, 50);
    }

    #[test]
    fn add_single_write() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1, 2, 3],
            timestamp_ms: 100,
        });
        assert_eq!(coalescer.pending_count(), 1);
    }

    #[test]
    fn flush_all_single_write() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1, 2, 3],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_all();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].inode_id, 1);
        assert_eq!(flushed[0].data, vec![1, 2, 3]);
    }

    #[test]
    fn flush_inode_not_found() {
        let mut coalescer = WriteCoalescer::default();
        assert!(coalescer.flush_inode(999).is_none());
    }

    #[test]
    fn coalesce_adjacent_writes() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1, 2, 3],
            timestamp_ms: 100,
        });
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 3,
            data: vec![4, 5],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_all();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].data, vec![1, 2, 3, 4, 5]);
        assert_eq!(flushed[0].source_count, 2);
    }

    #[test]
    fn coalesce_nonadjacent() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1, 2, 3],
            timestamp_ms: 100,
        });
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 10,
            data: vec![4, 5],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_all();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].data.len(), 3);
        assert_eq!(flushed[0].source_count, 1);
    }

    #[test]
    fn flush_all_multiple_inodes() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1],
            timestamp_ms: 100,
        });
        coalescer.add(WriteOp {
            inode_id: 2,
            offset: 0,
            data: vec![2],
            timestamp_ms: 100,
        });

        let mut flushed = coalescer.flush_all();
        assert_eq!(flushed.len(), 2);
        flushed.sort_by_key(|w| w.inode_id);
        assert_eq!(flushed[0].inode_id, 1);
        assert_eq!(flushed[1].inode_id, 2);
    }

    #[test]
    fn coalesced_write_source_count_1() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_all();
        assert_eq!(flushed[0].source_count, 1);
    }

    #[test]
    fn coalesced_write_source_count_2() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1],
            timestamp_ms: 100,
        });
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 1,
            data: vec![2],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_all();
        assert_eq!(flushed[0].source_count, 2);
    }

    #[test]
    fn flush_ready_no_ready_writes() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_ready(50);
        assert!(flushed.is_empty());
        assert_eq!(coalescer.pending_count(), 1);
    }

    #[test]
    fn flush_ready_expired_window() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_ready(200);
        assert_eq!(flushed.len(), 1);
        assert_eq!(coalescer.pending_count(), 0);
    }

    #[test]
    fn pending_count_after_add() {
        let mut coalescer = WriteCoalescer::default();
        assert_eq!(coalescer.pending_count(), 0);

        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1],
            timestamp_ms: 100,
        });
        assert_eq!(coalescer.pending_count(), 1);

        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 1,
            data: vec![2],
            timestamp_ms: 100,
        });
        assert_eq!(coalescer.pending_count(), 2);
    }

    #[test]
    fn pending_count_after_flush() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1],
            timestamp_ms: 100,
        });

        coalescer.flush_all();
        assert_eq!(coalescer.pending_count(), 0);
    }

    #[test]
    fn max_coalesced_size_respected() {
        let config = CoalesceConfig {
            max_gap_bytes: 0,
            max_coalesced_bytes: 5,
            window_ms: 50,
        };
        let mut coalescer = WriteCoalescer::new(config);

        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1, 2, 3],
            timestamp_ms: 100,
        });
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 3,
            data: vec![4, 5],
            timestamp_ms: 100,
        });
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 5,
            data: vec![6, 7],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_all();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].data.len(), 5);
    }

    #[test]
    fn coalesce_three_adjacent_writes() {
        let mut coalescer = WriteCoalescer::default();
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![1],
            timestamp_ms: 100,
        });
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 1,
            data: vec![2],
            timestamp_ms: 100,
        });
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 2,
            data: vec![3],
            timestamp_ms: 100,
        });

        let flushed = coalescer.flush_all();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].data, vec![1, 2, 3]);
        assert_eq!(flushed[0].source_count, 3);
    }
}
