use crate::inode::InodeId;
use std::collections::HashMap;

pub struct WriteBufConfig {
    pub flush_threshold: usize,
    pub max_coalesce_gap: u64,
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

#[derive(Debug, Clone)]
pub struct WriteRange {
    pub offset: u64,
    pub data: Vec<u8>,
}

pub struct WriteBuf {
    config: WriteBufConfig,
    dirty: HashMap<InodeId, Vec<WriteRange>>,
    total_bytes: usize,
}

impl WriteBuf {
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

    pub fn buffer_write(&mut self, ino: InodeId, offset: u64, data: &[u8]) -> bool {
        if data.is_empty() {
            tracing::debug!("Ignoring zero-length write for inode {}", ino);
            return false;
        }

        let ranges = self.dirty.entry(ino).or_insert_with(Vec::new);

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

    pub fn is_dirty(&self, ino: InodeId) -> bool {
        self.dirty.get(&ino).map(|r| !r.is_empty()).unwrap_or(false)
    }

    pub fn dirty_inodes(&self) -> Vec<InodeId> {
        self.dirty
            .iter()
            .filter(|(_, ranges)| !ranges.is_empty())
            .map(|(&ino, _)| ino)
            .collect()
    }

    pub fn total_buffered(&self) -> usize {
        self.total_bytes
    }

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
