//! Complete synchronous write pipeline.
//!
//! Provides a single `WritePath` facade that coordinates the write flow end-to-end:
//! client write → write buffer → journal append → segment packing → EC encoding →
//! background scheduler enqueue.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::{debug, info};

use crate::background_scheduler::{BackgroundScheduler, BackgroundTask, BackgroundTaskType};
use crate::block::{BlockId, BlockRef, BlockSize, PlacementHint};
use crate::checksum::{compute, Checksum, ChecksumAlgorithm};
use crate::erasure::{EcConfig, EcProfile, ErasureCodingEngine};
use crate::error::{StorageError, StorageResult};
use crate::segment::{PackedSegment, SegmentEntry, SegmentHeader};
use crate::write_journal::{JournalConfig, JournalOp, SyncMode, WriteJournal};

/// Configuration for the write path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritePathConfig {
    /// Maximum number of pending journal entries before segment packing.
    pub journal_pack_threshold: usize,
    /// EC profile to use (default: 4+2).
    pub ec_data_shards: u8,
    /// EC parity shards.
    pub ec_parity_shards: u8,
    /// Maximum write buffer size in bytes.
    pub max_buffer_bytes: u64,
    /// Sync mode for the journal.
    pub sync_mode: SyncMode,
}

impl Default for WritePathConfig {
    fn default() -> Self {
        Self {
            journal_pack_threshold: 32,
            ec_data_shards: 4,
            ec_parity_shards: 2,
            max_buffer_bytes: 64 * 1024 * 1024,
            sync_mode: SyncMode::Sync,
        }
    }
}

/// Result of a write operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    /// Journal sequence number assigned to this write.
    pub journal_sequence: u64,
    /// Number of bytes written.
    pub bytes_written: u64,
    /// True if a segment was packed and EC-encoded this write.
    pub segment_sealed: bool,
    /// Segment ID if a segment was sealed.
    pub segment_id: Option<u64>,
    /// Write latency in microseconds.
    pub latency_us: u64,
}

/// Aggregate statistics for the write path.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WritePathStats {
    /// Total number of writes.
    pub total_writes: u64,
    /// Total bytes written.
    pub total_bytes_written: u64,
    /// Total segments sealed.
    pub segments_sealed: u64,
    /// Total EC encodes performed.
    pub ec_encodes: u64,
    /// Total write errors.
    pub write_errors: u64,
    /// Rolling average write latency in microseconds.
    pub avg_write_latency_us: u64,
}

/// Errors from the write path.
#[derive(Debug, Error)]
pub enum WritePathError {
    /// Journal error.
    #[error("journal error: {0}")]
    Journal(String),
    /// Segment pack error.
    #[error("segment pack error: {0}")]
    SegmentPack(String),
    /// EC encode error.
    #[error("EC encode error: {0}")]
    EcEncode(String),
    /// Write buffer full.
    #[error("write buffer full: limit {limit_bytes} bytes")]
    BufferFull {
        /// Limit in bytes.
        limit_bytes: u64,
    },
}

/// Pending entry for the current segment.
#[derive(Debug, Clone)]
struct PendingEntry {
    sequence: u64,
    inode_id: u64,
    offset: u64,
    data: Vec<u8>,
}

/// The write path coordinates the complete write flow.
pub struct WritePath {
    config: WritePathConfig,
    journal: WriteJournal,
    pending_entries: VecDeque<PendingEntry>,
    current_segment_data: Vec<u8>,
    current_segment_entries: Vec<SegmentEntry>,
    ec_engine: ErasureCodingEngine,
    scheduler: BackgroundScheduler,
    stats: WritePathStats,
    next_segment_id: u64,
    current_segment_id: u64,
}

impl WritePath {
    /// Creates a new WritePath with the given configuration.
    pub fn new(config: WritePathConfig) -> Self {
        let journal_config = JournalConfig {
            sync_mode: config.sync_mode,
            ..Default::default()
        };
        let ec_config = EcConfig {
            default_profile: EcProfile {
                data_shards: config.ec_data_shards,
                parity_shards: config.ec_parity_shards,
            },
            ..Default::default()
        };

        info!(
            journal_pack_threshold = config.journal_pack_threshold,
            ec_profile = ?(config.ec_data_shards, config.ec_parity_shards),
            max_buffer_bytes = config.max_buffer_bytes,
            "creating write path"
        );

        Self {
            journal: WriteJournal::new(journal_config),
            ec_engine: ErasureCodingEngine::new(ec_config),
            scheduler: BackgroundScheduler::new(),
            pending_entries: VecDeque::new(),
            current_segment_data: Vec::new(),
            current_segment_entries: Vec::new(),
            stats: WritePathStats::default(),
            next_segment_id: 1,
            current_segment_id: 1,
            config,
        }
    }

    /// Write data for an inode at offset. Returns WriteResult.
    pub fn write(&mut self, inode_id: u64, offset: u64, data: &[u8]) -> StorageResult<WriteResult> {
        let start_time = SystemTime::now();

        let bytes_written = data.len() as u64;

        if self.current_segment_data.len() as u64 + bytes_written > self.config.max_buffer_bytes {
            self.stats.write_errors += 1;
            return Err(StorageError::AllocatorError(format!(
                "write buffer full: limit {} bytes",
                self.config.max_buffer_bytes
            )));
        }

        let op = JournalOp::Write {
            data: data.to_vec(),
        };
        let sequence = self
            .journal
            .append(op, inode_id, offset)
            .map_err(|e| StorageError::AllocatorError(format!("journal error: {}", e)))?;

        self.pending_entries.push_back(PendingEntry {
            sequence,
            inode_id,
            offset,
            data: data.to_vec(),
        });

        self.current_segment_data.extend_from_slice(data);

        let data_offset = self.current_segment_data.len() as u32 - data.len() as u32;
        self.current_segment_entries.push(SegmentEntry {
            sequence,
            block_ref: BlockRef {
                id: BlockId::new(0, offset),
                size: BlockSize::B4K,
            },
            data_len: data.len() as u32,
            data_offset,
            placement_hint: PlacementHint::HotData,
        });

        let mut segment_sealed = false;
        let mut segment_id = None;

        if self.pending_entries.len() >= self.config.journal_pack_threshold {
            let (sealed, sid) = self.seal_segment()?;
            segment_sealed = sealed;
            segment_id = sid;
        }

        let latency_us = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_micros() as u64;

        self.stats.total_writes += 1;
        self.stats.total_bytes_written += bytes_written;

        let total_latency =
            self.stats.avg_write_latency_us * (self.stats.total_writes - 1) + latency_us;
        self.stats.avg_write_latency_us = total_latency / self.stats.total_writes;

        debug!(
            sequence = sequence,
            inode_id = inode_id,
            offset = offset,
            bytes = bytes_written,
            segment_sealed = segment_sealed,
            "write completed"
        );

        Ok(WriteResult {
            journal_sequence: sequence,
            bytes_written,
            segment_sealed,
            segment_id,
            latency_us,
        })
    }

    /// Force-seal the current segment even if not full.
    /// Returns None if segment has no entries (nothing to seal).
    pub fn flush(&mut self) -> StorageResult<Option<WriteResult>> {
        if self.pending_entries.is_empty() {
            return Ok(None);
        }

        let start_time = SystemTime::now();

        let (sealed, segment_id) = self.seal_segment()?;

        if !sealed {
            return Ok(None);
        }

        let latency_us = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_micros() as u64;

        let bytes_written = self.current_segment_data.len() as u64;

        Ok(Some(WriteResult {
            journal_sequence: self.journal.current_sequence(),
            bytes_written,
            segment_sealed: true,
            segment_id,
            latency_us,
        }))
    }

    fn seal_segment(&mut self) -> StorageResult<(bool, Option<u64>)> {
        if self.pending_entries.is_empty() {
            return Ok((false, None));
        }

        let segment_id = self.current_segment_id;
        let data = std::mem::take(&mut self.current_segment_data);
        let entries = std::mem::take(&mut self.current_segment_entries);

        if data.is_empty() {
            return Ok((false, None));
        }

        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        let first_sequence = entries.first().map(|e| e.sequence).unwrap_or(0);
        let last_sequence = entries.last().map(|e| e.sequence).unwrap_or(0);

        let header = SegmentHeader::new(
            segment_id,
            entries.len() as u32,
            data.len() as u64,
            checksum,
            first_sequence,
            last_sequence,
        );

        let _segment = PackedSegment {
            header,
            entries,
            data: data.clone(),
        };

        let profile = EcProfile {
            data_shards: self.config.ec_data_shards,
            parity_shards: self.config.ec_parity_shards,
        };

        self.ec_engine
            .encode_segment(segment_id, &data)
            .map_err(|e| StorageError::AllocatorError(format!("EC encode error: {}", e)))?;

        let task = BackgroundTask::new(
            BackgroundTaskType::JournalFlush,
            data.len() as u64,
            format!("Flush segment {}", segment_id),
        );
        self.scheduler.schedule(task);

        self.stats.segments_sealed += 1;
        self.stats.ec_encodes += 1;

        self.current_segment_id = self.next_segment_id;
        self.next_segment_id += 1;
        self.pending_entries.clear();

        debug!(
            segment_id = segment_id,
            entries = _segment.header.entry_count,
            bytes = data.len(),
            "segment sealed and EC-encoded"
        );

        Ok((true, Some(segment_id)))
    }

    /// Returns the write path statistics.
    pub fn stats(&self) -> &WritePathStats {
        &self.stats
    }

    /// Returns the number of pending journal entries not yet packed.
    pub fn pending_entries(&self) -> usize {
        self.pending_entries.len()
    }

    /// Returns the current segment data size in bytes.
    pub fn current_segment_bytes(&self) -> usize {
        self.current_segment_data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_write_path() -> WritePath {
        WritePath::new(WritePathConfig::default())
    }

    #[test]
    fn test_write_single_small_buffer_succeeds() {
        let mut wp = create_test_write_path();
        let result = wp.write(1, 0, b"hello").unwrap();
        assert_eq!(result.bytes_written, 5);
    }

    #[test]
    fn test_write_result_has_correct_bytes_written() {
        let mut wp = create_test_write_path();
        let data = vec![1u8; 100];
        let result = wp.write(1, 0, &data).unwrap();
        assert_eq!(result.bytes_written, 100);
    }

    #[test]
    fn test_write_result_has_valid_journal_sequence() {
        let mut wp = create_test_write_path();
        let result = wp.write(1, 0, b"test").unwrap();
        assert!(result.journal_sequence > 0);
    }

    #[test]
    fn test_write_to_different_inode_ids() {
        let mut wp = create_test_write_path();
        let r1 = wp.write(1, 0, b"a").unwrap();
        let r2 = wp.write(2, 0, b"b").unwrap();
        assert_ne!(r1.journal_sequence, r2.journal_sequence);
    }

    #[test]
    fn test_write_at_offset_0() {
        let mut wp = create_test_write_path();
        let result = wp.write(1, 0, b"data").unwrap();
        assert_eq!(result.bytes_written, 4);
    }

    #[test]
    fn test_write_at_large_offset() {
        let mut wp = create_test_write_path();
        let large_offset = 1024 * 1024 * 1024 * 10;
        let result = wp.write(1, large_offset, b"data").unwrap();
        assert_eq!(result.bytes_written, 4);
    }

    #[test]
    fn test_multiple_writes_accumulate_in_pending_entries() {
        let mut wp = create_test_write_path();
        wp.write(1, 0, b"a").unwrap();
        wp.write(1, 100, b"b").unwrap();
        wp.write(1, 200, b"c").unwrap();
        assert_eq!(wp.pending_entries(), 3);
    }

    #[test]
    fn test_pending_entries_resets_after_flush() {
        let mut wp = create_test_write_path();
        wp.write(1, 0, b"a").unwrap();
        wp.write(1, 100, b"b").unwrap();
        wp.flush().unwrap();
        assert_eq!(wp.pending_entries(), 0);
    }

    #[test]
    fn test_flush_returns_none_when_no_entries() {
        let mut wp = create_test_write_path();
        let result = wp.flush().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_flush_seals_current_segment() {
        let mut wp = create_test_write_path();
        wp.write(1, 0, b"test data").unwrap();
        let result = wp.flush().unwrap().unwrap();
        assert!(result.segment_sealed);
    }

    #[test]
    fn test_stats_total_writes_increments() {
        let mut wp = create_test_write_path();
        wp.write(1, 0, b"a").unwrap();
        wp.write(1, 100, b"b").unwrap();
        assert_eq!(wp.stats().total_writes, 2);
    }

    #[test]
    fn test_stats_total_bytes_written_accumulates() {
        let mut wp = create_test_write_path();
        wp.write(1, 0, b"aaa").unwrap();
        wp.write(1, 100, b"bb").unwrap();
        assert_eq!(wp.stats().total_bytes_written, 5);
    }

    #[test]
    fn test_segment_sealed_when_pack_threshold_reached() {
        let config = WritePathConfig {
            journal_pack_threshold: 3,
            ..Default::default()
        };
        let mut wp = WritePath::new(config);

        let r1 = wp.write(1, 0, b"a").unwrap();
        assert!(!r1.segment_sealed);

        let r2 = wp.write(1, 100, b"b").unwrap();
        assert!(!r2.segment_sealed);

        let r3 = wp.write(1, 200, b"c").unwrap();
        assert!(r3.segment_sealed);
    }

    #[test]
    fn test_segment_sealed_true_in_result_when_segment_just_sealed() {
        let config = WritePathConfig {
            journal_pack_threshold: 2,
            ..Default::default()
        };
        let mut wp = WritePath::new(config);

        wp.write(1, 0, b"a").unwrap();
        let r2 = wp.write(1, 100, b"b").unwrap();
        assert!(r2.segment_sealed);
    }

    #[test]
    fn test_segment_id_some_when_sealed() {
        let config = WritePathConfig {
            journal_pack_threshold: 1,
            ..Default::default()
        };
        let mut wp = WritePath::new(config);

        let result = wp.write(1, 0, b"data").unwrap();
        assert!(result.segment_id.is_some());
    }

    #[test]
    fn test_stats_segments_sealed_increments_on_seal() {
        let config = WritePathConfig {
            journal_pack_threshold: 1,
            ..Default::default()
        };
        let mut wp = WritePath::new(config);

        wp.write(1, 0, b"a").unwrap();
        assert_eq!(wp.stats().segments_sealed, 1);

        wp.write(1, 100, b"b").unwrap();
        assert_eq!(wp.stats().segments_sealed, 2);
    }

    #[test]
    fn test_stats_ec_encodes_increments_on_seal() {
        let config = WritePathConfig {
            journal_pack_threshold: 1,
            ..Default::default()
        };
        let mut wp = WritePath::new(config);

        wp.write(1, 0, b"data").unwrap();
        assert_eq!(wp.stats().ec_encodes, 1);
    }

    #[test]
    fn test_current_segment_bytes_starts_at_0() {
        let wp = create_test_write_path();
        assert_eq!(wp.current_segment_bytes(), 0);
    }

    #[test]
    fn test_current_segment_bytes_increases_with_writes() {
        let mut wp = create_test_write_path();
        wp.write(1, 0, b"hello").unwrap();
        assert_eq!(wp.current_segment_bytes(), 5);
    }

    #[test]
    fn test_write_path_config_default_has_ec_data_shards_4() {
        let config = WritePathConfig::default();
        assert_eq!(config.ec_data_shards, 4);
    }

    #[test]
    fn test_two_write_path_instances_are_independent() {
        let mut wp1 = create_test_write_path();
        let mut wp2 = create_test_write_path();

        wp1.write(1, 0, b"a").unwrap();
        wp2.write(1, 0, b"bb").unwrap();

        assert_eq!(wp1.stats().total_bytes_written, 1);
        assert_eq!(wp2.stats().total_bytes_written, 2);
    }

    #[test]
    fn test_write_empty_data() {
        let mut wp = create_test_write_path();
        let result = wp.write(1, 0, b"").unwrap();
        assert_eq!(result.bytes_written, 0);
    }
}
