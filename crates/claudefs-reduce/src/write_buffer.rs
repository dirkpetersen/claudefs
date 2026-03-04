//! Write buffer that accumulates small writes before passing to the reduction pipeline.
//!
//! In ClaudeFS, FUSE writes may be small (< 4KB). Buffering them into 2MB chunks before
//! running CDC improves dedup effectiveness and reduces metadata overhead.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for the write buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteBufferConfig {
    /// Flush threshold in bytes. Default: 2MB.
    pub flush_threshold_bytes: usize,
    /// Maximum number of pending writes before forcing flush.
    pub max_pending_writes: usize,
}

impl Default for WriteBufferConfig {
    fn default() -> Self {
        Self {
            flush_threshold_bytes: 2 * 1024 * 1024,
            max_pending_writes: 1024,
        }
    }
}

/// A pending write operation waiting to be flushed.
#[derive(Debug, Clone)]
pub struct PendingWrite {
    /// Inode identifier.
    pub inode_id: u64,
    /// Byte offset in the file.
    pub offset: u64,
    /// Data to be written.
    pub data: Vec<u8>,
    /// Timestamp in milliseconds since epoch.
    pub timestamp_ms: u64,
}

/// Reason for flushing pending writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushReason {
    /// Threshold bytes reached.
    ThresholdReached,
    /// Explicit flush requested.
    Explicit,
    /// Timeout expired.
    Timeout,
    /// Inode closed.
    InodeClosed,
}

/// Result of a flush operation.
#[derive(Debug)]
pub struct FlushResult {
    /// Inode that was flushed.
    pub inode_id: u64,
    /// All writes that were flushed.
    pub writes: Vec<PendingWrite>,
    /// Total bytes flushed.
    pub total_bytes: usize,
    /// Why the flush occurred.
    pub reason: FlushReason,
}

/// Write buffer that accumulates small writes before flushing to the reduction pipeline.
pub struct WriteBuffer {
    config: WriteBufferConfig,
    pending: HashMap<u64, Vec<PendingWrite>>,
    pending_bytes: HashMap<u64, usize>,
}

impl WriteBuffer {
    /// Create a new write buffer with the given configuration.
    pub fn new(config: WriteBufferConfig) -> Self {
        Self {
            config,
            pending: HashMap::new(),
            pending_bytes: HashMap::new(),
        }
    }

    /// Buffer a write operation.
    ///
    /// Returns `Some(FlushResult)` if the flush threshold is reached for this inode.
    pub fn buffer_write(&mut self, write: PendingWrite) -> Option<FlushResult> {
        let inode_id = write.inode_id;
        let bytes = write.data.len();

        let writes = self.pending.entry(inode_id).or_default();
        writes.push(write);

        *self.pending_bytes.entry(inode_id).or_insert(0) += bytes;

        if self.pending_bytes.get(&inode_id).copied().unwrap_or(0)
            >= self.config.flush_threshold_bytes
        {
            self.flush(inode_id, FlushReason::ThresholdReached)
        } else {
            None
        }
    }

    /// Flush all pending writes for a specific inode.
    ///
    /// Returns `None` if no pending writes exist for that inode.
    pub fn flush(&mut self, inode_id: u64, reason: FlushReason) -> Option<FlushResult> {
        let writes = self.pending.remove(&inode_id)?;
        let total_bytes = self.pending_bytes.remove(&inode_id).unwrap_or(0);

        Some(FlushResult {
            inode_id,
            writes,
            total_bytes,
            reason,
        })
    }

    /// Flush all pending writes for all inodes.
    pub fn flush_all(&mut self, reason: FlushReason) -> Vec<FlushResult> {
        let inode_ids: Vec<u64> = self.pending.keys().copied().collect();
        inode_ids
            .into_iter()
            .filter_map(|inode_id| self.flush(inode_id, reason))
            .collect()
    }

    /// Number of pending writes for a specific inode.
    pub fn pending_count(&self, inode_id: u64) -> usize {
        self.pending.get(&inode_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Total pending bytes for a specific inode.
    pub fn pending_bytes(&self, inode_id: u64) -> usize {
        self.pending_bytes.get(&inode_id).copied().unwrap_or(0)
    }

    /// Total pending bytes across all inodes.
    pub fn total_pending_bytes(&self) -> usize {
        self.pending_bytes.values().sum()
    }

    /// True if no pending writes exist.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_write(inode_id: u64, offset: u64, size: usize) -> PendingWrite {
        PendingWrite {
            inode_id,
            offset,
            data: vec![0xAB; size],
            timestamp_ms: 0,
        }
    }

    #[test]
    fn default_config_values() {
        let config = WriteBufferConfig::default();
        assert_eq!(config.flush_threshold_bytes, 2 * 1024 * 1024);
        assert_eq!(config.max_pending_writes, 1024);
    }

    #[test]
    fn buffer_single_write() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        let write = make_write(1, 0, 1024);
        buffer.buffer_write(write);
        assert_eq!(buffer.pending_count(1), 1);
        assert_eq!(buffer.pending_bytes(1), 1024);
    }

    #[test]
    fn buffer_multiple_writes_same_inode() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        buffer.buffer_write(make_write(1, 0, 1024));
        buffer.buffer_write(make_write(1, 1024, 2048));
        buffer.buffer_write(make_write(1, 3072, 512));
        assert_eq!(buffer.pending_count(1), 3);
        assert_eq!(buffer.pending_bytes(1), 1024 + 2048 + 512);
    }

    #[test]
    fn buffer_writes_different_inodes() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        buffer.buffer_write(make_write(1, 0, 1024));
        buffer.buffer_write(make_write(2, 0, 2048));
        buffer.buffer_write(make_write(3, 0, 512));

        assert_eq!(buffer.pending_count(1), 1);
        assert_eq!(buffer.pending_count(2), 1);
        assert_eq!(buffer.pending_count(3), 1);
        assert_eq!(buffer.total_pending_bytes(), 1024 + 2048 + 512);
    }

    #[test]
    fn buffer_triggers_flush_at_threshold() {
        let config = WriteBufferConfig {
            flush_threshold_bytes: 100,
            max_pending_writes: 1000,
        };
        let mut buffer = WriteBuffer::new(config);

        let result = buffer.buffer_write(make_write(1, 0, 150));
        assert!(result.is_some());
        let flush = result.unwrap();
        assert_eq!(flush.reason, FlushReason::ThresholdReached);
        assert_eq!(flush.total_bytes, 150);
        assert_eq!(buffer.pending_count(1), 0);
    }

    #[test]
    fn flush_returns_all_writes() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        buffer.buffer_write(make_write(1, 0, 1024));
        buffer.buffer_write(make_write(1, 1024, 2048));

        let result = buffer.flush(1, FlushReason::Explicit);
        assert!(result.is_some());
        let flush = result.unwrap();
        assert_eq!(flush.writes.len(), 2);
        assert_eq!(flush.total_bytes, 1024 + 2048);
    }

    #[test]
    fn flush_unknown_inode_returns_none() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        let result = buffer.flush(999, FlushReason::Explicit);
        assert!(result.is_none());
    }

    #[test]
    fn flush_all_returns_all_inodes() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        buffer.buffer_write(make_write(1, 0, 1024));
        buffer.buffer_write(make_write(2, 0, 2048));
        buffer.buffer_write(make_write(3, 0, 512));

        let results = buffer.flush_all(FlushReason::Explicit);
        assert_eq!(results.len(), 3);
        assert!(buffer.is_empty());
    }

    #[test]
    fn flush_clears_pending() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        buffer.buffer_write(make_write(1, 0, 1024));
        buffer.flush(1, FlushReason::Explicit);
        assert_eq!(buffer.pending_count(1), 0);
        assert_eq!(buffer.pending_bytes(1), 0);
    }

    #[test]
    fn pending_count_after_buffer() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        assert_eq!(buffer.pending_count(1), 0);
        buffer.buffer_write(make_write(1, 0, 100));
        assert_eq!(buffer.pending_count(1), 1);
        buffer.buffer_write(make_write(1, 100, 200));
        assert_eq!(buffer.pending_count(1), 2);
    }

    #[test]
    fn pending_bytes_accumulate() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        assert_eq!(buffer.pending_bytes(1), 0);
        buffer.buffer_write(make_write(1, 0, 100));
        assert_eq!(buffer.pending_bytes(1), 100);
        buffer.buffer_write(make_write(1, 100, 200));
        assert_eq!(buffer.pending_bytes(1), 300);
    }

    #[test]
    fn total_pending_bytes() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        assert_eq!(buffer.total_pending_bytes(), 0);
        buffer.buffer_write(make_write(1, 0, 100));
        buffer.buffer_write(make_write(2, 0, 200));
        assert_eq!(buffer.total_pending_bytes(), 300);
    }

    #[test]
    fn is_empty_initially() {
        let buffer = WriteBuffer::new(WriteBufferConfig::default());
        assert!(buffer.is_empty());
    }

    #[test]
    fn is_empty_after_flush() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        buffer.buffer_write(make_write(1, 0, 1024));
        buffer.flush(1, FlushReason::Explicit);
        assert!(buffer.is_empty());
    }

    #[test]
    fn flush_reason_preserved() {
        let mut buffer = WriteBuffer::new(WriteBufferConfig::default());
        buffer.buffer_write(make_write(1, 0, 1024));

        let result = buffer.flush(1, FlushReason::Timeout);
        assert_eq!(result.unwrap().reason, FlushReason::Timeout);
    }

    #[test]
    fn write_buffer_max_pending() {
        let config = WriteBufferConfig {
            flush_threshold_bytes: 10 * 1024 * 1024,
            max_pending_writes: 3,
        };
        let mut buffer = WriteBuffer::new(config);

        buffer.buffer_write(make_write(1, 0, 100));
        buffer.buffer_write(make_write(1, 100, 100));
        buffer.buffer_write(make_write(1, 200, 100));

        assert_eq!(buffer.pending_count(1), 3);
    }
}
