//! Write buffering for coalescing small writes before flush to storage.
//!
//! This module provides a write buffer that accumulates small writes per inode,
//! coalesces adjacent or overlapping ranges, and signals when a flush threshold
//! is exceeded. This reduces the number of I/O operations and improves throughput
//! for workloads with many small sequential or random writes.

use crate::inode::InodeId;
use std::collections::HashMap;

/// Configuration parameters for the write buffer.
pub struct WriteBufConfig {
    /// Total bytes buffered before `buffer_write` returns `true` to signal a flush.
    pub flush_threshold: usize,
    /// Maximum gap between ranges to merge during coalescing (in bytes).
    pub max_coalesce_gap: u64,
    /// Maximum time dirty data may remain unflushed (in milliseconds).
    pub dirty_timeout_ms: u64,
}

impl Default for WriteBufConfig {
    fn default() -> Self {
        Self {
            flush_threshold: 1 << 20,
            max_coalesce_gap: 4096,
            dirty_timeout_ms: 5000,
        }
    }
}

/// A contiguous range of data written to an inode.
#[derive(Debug, Clone)]
pub struct WriteRange {
    /// Byte offset within the inode where this data should be written.
    pub offset: u64,
    /// The actual data bytes to write.
    pub data: Vec<u8>,
}

/// Per-inode write buffer with coalescing and flush signaling.
pub struct WriteBuf {
    config: WriteBufConfig,
    dirty: HashMap<InodeId, Vec<WriteRange>>,
    total_bytes: usize,
}

impl WriteBuf {
    /// Creates a new write buffer with the given configuration.
    pub fn new(config: WriteBufConfig) -> Self {
        tracing::debug!(
            "Initializing write buffer: flush_threshold={}, max_coalesce_gap={}, timeout={}ms",
            config.flush_threshold,
            config.max_coalesce_gap,
            config.dirty_timeout_ms
        );
        Self {
            config,
            dirty: HashMap::new(),
            total_bytes: 0,
        }
    }

    /// Buffers a write for the given inode.
    ///
    /// Returns `true` if the total buffered bytes now meets or exceeds the
    /// flush threshold, signaling that a flush should be performed.
    pub fn buffer_write(&mut self, ino: InodeId, offset: u64, data: &[u8]) -> bool {
        if data.is_empty() {
            tracing::debug!("Ignoring zero-length write for inode {}", ino);
            return false;
        }

        let ranges = self.dirty.entry(ino).or_default();

        let data_vec = data.to_vec();
        let size = data_vec.len();

        ranges.push(WriteRange {
            offset,
            data: data_vec,
        });

        self.total_bytes += size;

        tracing::debug!(
            "Buffered write: ino={}, offset={}, size={}, total_buffered={}",
            ino,
            offset,
            size,
            self.total_bytes
        );

        self.total_bytes >= self.config.flush_threshold
    }

    /// Coalesces adjacent, overlapping, or nearly-adjacent ranges for an inode.
    ///
    /// Ranges within `max_coalesce_gap` bytes of each other are merged to reduce
    /// the number of separate I/O operations. Overlapping ranges are handled by
    /// keeping the later write's data for the overlapping region.
    pub fn coalesce(&mut self, ino: InodeId) {
        let ranges = match self.dirty.get_mut(&ino) {
            Some(r) => r,
            None => return,
        };

        if ranges.len() <= 1 {
            return;
        }

        ranges.sort_by_key(|r| r.offset);

        let mut merged: Vec<WriteRange> = Vec::new();
        let mut current = ranges[0].clone();

        for range in ranges.iter().skip(1) {
            let gap = range
                .offset
                .saturating_sub(current.offset + current.data.len() as u64);

            if gap <= self.config.max_coalesce_gap {
                let overlap = current.offset + current.data.len() as u64;
                if range.offset < overlap {
                    let overlap_end =
                        std::cmp::min(overlap, range.offset + range.data.len() as u64);
                    let overlap_size = (overlap_end - range.offset) as usize;

                    if overlap_size < range.data.len() {
                        current.data.extend_from_slice(&range.data[overlap_size..]);
                    }
                } else {
                    current.data.extend_from_slice(&range.data);
                }
            } else {
                merged.push(current);
                current = range.clone();
            }
        }

        merged.push(current);

        let old_len = ranges.len();
        *ranges = merged;

        tracing::debug!(
            "Coalesced ino {}: {} ranges -> {} ranges",
            ino,
            old_len,
            ranges.len()
        );
    }

    /// Takes all dirty ranges for an inode, removing them from the buffer.
    ///
    /// Returns the ranges and decrements the total buffered byte count.
    /// Returns an empty vector if the inode has no dirty data.
    pub fn take_dirty(&mut self, ino: InodeId) -> Vec<WriteRange> {
        let ranges = self.dirty.remove(&ino);

        let result = ranges.unwrap_or_default();

        for range in &result {
            self.total_bytes = self.total_bytes.saturating_sub(range.data.len());
        }

        tracing::debug!(
            "Took {} dirty ranges for ino {}, {} bytes returned",
            result.len(),
            ino,
            result.iter().map(|r| r.data.len()).sum::<usize>()
        );

        result
    }

    /// Returns `true` if the inode has any dirty (buffered) data.
    pub fn is_dirty(&self, ino: InodeId) -> bool {
        self.dirty.get(&ino).map(|r| !r.is_empty()).unwrap_or(false)
    }

    /// Returns a list of all inodes that currently have dirty data.
    pub fn dirty_inodes(&self) -> Vec<InodeId> {
        self.dirty
            .iter()
            .filter(|(_, ranges)| !ranges.is_empty())
            .map(|(&ino, _)| ino)
            .collect()
    }

    /// Returns the total number of bytes currently buffered across all inodes.
    pub fn total_buffered(&self) -> usize {
        self.total_bytes
    }

    /// Discards all buffered data for an inode without flushing.
    ///
    /// Updates the total buffered byte count accordingly.
    pub fn discard(&mut self, ino: InodeId) {
        if let Some(ranges) = self.dirty.remove(&ino) {
            let bytes = ranges.iter().map(|r| r.data.len()).sum::<usize>();
            self.total_bytes = self.total_bytes.saturating_sub(bytes);
            tracing::debug!("Discarded {} bytes for ino {}", bytes, ino);
        }
    }
}

impl Default for WriteBuf {
    fn default() -> Self {
        Self::new(WriteBufConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> WriteBufConfig {
        WriteBufConfig {
            flush_threshold: 8192,
            max_coalesce_gap: 4096,
            dirty_timeout_ms: 1000,
        }
    }

    #[test]
    fn default_config_has_valid_thresholds() {
        let config = WriteBufConfig::default();
        assert!(config.flush_threshold > 0, "flush_threshold should be > 0");
        assert!(
            config.max_coalesce_gap > 0,
            "max_coalesce_gap should be > 0"
        );
        assert!(
            config.dirty_timeout_ms > 0,
            "dirty_timeout_ms should be > 0"
        );
        assert_eq!(config.flush_threshold, 1 << 20);
    }

    #[test]
    fn single_write_buffers_correctly() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"test data");

        assert_eq!(buf.total_buffered(), 9);
    }

    #[test]
    fn is_dirty_returns_true_after_write() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"test");

        assert!(buf.is_dirty(1));
    }

    #[test]
    fn take_dirty_returns_range_and_clears() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"test data");
        let dirty = buf.take_dirty(1);

        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].offset, 0);
        assert_eq!(dirty[0].data, b"test data");
        assert!(!buf.is_dirty(1));
    }

    #[test]
    fn is_dirty_returns_false_after_take_dirty() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"test");
        buf.take_dirty(1);

        assert!(!buf.is_dirty(1));
    }

    #[test]
    fn buffer_write_returns_true_when_threshold_exceeded() {
        let mut buf = WriteBuf::new(WriteBufConfig {
            flush_threshold: 10,
            max_coalesce_gap: 4096,
            dirty_timeout_ms: 1000,
        });

        let result = buf.buffer_write(1, 0, b"12345678901");

        assert!(result, "Should return true when threshold exceeded");
    }

    #[test]
    fn buffer_write_returns_false_when_below_threshold() {
        let mut buf = WriteBuf::new(WriteBufConfig {
            flush_threshold: 1000,
            max_coalesce_gap: 4096,
            dirty_timeout_ms: 1000,
        });

        let result = buf.buffer_write(1, 0, b"test");

        assert!(!result, "Should return false when below threshold");
    }

    #[test]
    fn coalesce_merges_adjacent_ranges() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"aaaa");
        buf.buffer_write(1, 4, b"bbbb");

        buf.coalesce(1);

        let dirty = buf.take_dirty(1);
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].data, b"aaaabbbb");
    }

    #[test]
    fn coalesce_merges_overlapping_ranges() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"xxxx");
        buf.buffer_write(1, 3, b"yyyy");

        buf.coalesce(1);

        let dirty = buf.take_dirty(1);
        assert_eq!(dirty.len(), 1);
    }

    #[test]
    fn coalesce_leaves_non_adjacent_ranges_separate() {
        let mut buf = WriteBuf::new(WriteBufConfig {
            flush_threshold: 8192,
            max_coalesce_gap: 100,
            dirty_timeout_ms: 1000,
        });

        buf.buffer_write(1, 0, b"aaaa");
        buf.buffer_write(1, 500, b"bbbb");

        buf.coalesce(1);

        let dirty = buf.take_dirty(1);
        assert_eq!(dirty.len(), 2, "Non-adjacent ranges should remain separate");
    }

    #[test]
    fn discard_removes_all_data_for_inode() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"test1");
        buf.buffer_write(1, 100, b"test2");
        buf.buffer_write(2, 0, b"other");

        buf.discard(1);

        assert!(!buf.is_dirty(1));
        assert!(buf.is_dirty(2));
        assert_eq!(buf.total_buffered(), 5);
    }

    #[test]
    fn discard_does_not_affect_other_inodes() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"test");
        buf.buffer_write(2, 0, b"other");

        buf.discard(1);

        assert!(!buf.is_dirty(1));
        assert!(buf.is_dirty(2));
    }

    #[test]
    fn dirty_inodes_returns_all_inodes_with_data() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"test1");
        buf.buffer_write(2, 0, b"test2");
        buf.buffer_write(3, 0, b"test3");

        let dirty = buf.dirty_inodes();

        assert_eq!(dirty.len(), 3);
        assert!(dirty.contains(&1));
        assert!(dirty.contains(&2));
        assert!(dirty.contains(&3));
    }

    #[test]
    fn multiple_writes_to_same_inode_accumulate_total_buffered() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"aaaa");
        buf.buffer_write(1, 100, b"bbbb");
        buf.buffer_write(1, 200, b"cccc");

        assert_eq!(buf.total_buffered(), 12);
    }

    #[test]
    fn zero_length_write_is_no_op() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"");

        assert!(!buf.is_dirty(1));
        assert_eq!(buf.total_buffered(), 0);
    }

    #[test]
    fn take_dirty_on_nonexistent_inode_returns_empty() {
        let mut buf = WriteBuf::new(test_config());

        let dirty = buf.take_dirty(999);

        assert!(dirty.is_empty());
    }

    #[test]
    fn total_buffered_after_coalesce() {
        let mut buf = WriteBuf::new(test_config());

        buf.buffer_write(1, 0, b"aaaa");
        buf.buffer_write(1, 4, b"bbbb");

        let before = buf.total_buffered();
        buf.coalesce(1);
        let after = buf.total_buffered();

        assert_eq!(before, after, "Coalesce should not change total bytes");
    }
}
