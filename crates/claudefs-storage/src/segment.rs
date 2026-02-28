//! Segment packer for building 2MB EC segments from journal entries.
//!
//! Per D1: EC unit is 2MB packed segments (post-dedup, post-compression)
//! Per D3: Write path: client write -> 2x journal replication -> segment packing + EC 4+2 -> journal space reclaimed
//! Per D8: Background segment packer collects journal entries into 2MB segments, applies EC 4+2, distributes stripes

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::block::{BlockRef, PlacementHint};
use crate::checksum::{compute, Checksum, ChecksumAlgorithm};
use crate::error::{StorageError, StorageResult};

/// Default segment size: 2MB (per D1)
pub const SEGMENT_SIZE: usize = 2 * 1024 * 1024;

/// Segment header magic: "CSEG" = 0x43534547
pub const SEGMENT_MAGIC: u32 = 0x43534547;

/// Current segment format version
const SEGMENT_VERSION: u8 = 1;

/// Header stored at the beginning of each segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentHeader {
    /// Magic number (SEGMENT_MAGIC)
    pub magic: u32,
    /// Format version
    pub version: u8,
    /// Unique segment ID
    pub segment_id: u64,
    /// Number of entries packed in this segment
    pub entry_count: u32,
    /// Total data bytes (excluding header and padding)
    pub data_bytes: u64,
    /// Checksum of the segment data
    pub checksum: Checksum,
    /// Timestamp when the segment was sealed
    pub sealed_at_secs: u64,
    /// First journal sequence number in this segment
    pub first_sequence: u64,
    /// Last journal sequence number in this segment
    pub last_sequence: u64,
}

impl SegmentHeader {
    /// Creates a new segment header.
    pub fn new(
        segment_id: u64,
        entry_count: u32,
        data_bytes: u64,
        checksum: Checksum,
        first_sequence: u64,
        last_sequence: u64,
    ) -> Self {
        let sealed_at_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            magic: SEGMENT_MAGIC,
            version: SEGMENT_VERSION,
            segment_id,
            entry_count,
            data_bytes,
            checksum,
            sealed_at_secs,
            first_sequence,
            last_sequence,
        }
    }
}

/// A packed entry within a segment, representing one journal write.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentEntry {
    /// Original journal sequence number
    pub sequence: u64,
    /// Target block reference
    pub block_ref: BlockRef,
    /// Length of data in bytes
    pub data_len: u32,
    /// Offset of data within the segment's data area
    pub data_offset: u32,
    /// Placement hint from original write
    pub placement_hint: PlacementHint,
}

/// Pending entry data with metadata for packing.
#[derive(Debug, Clone)]
struct PendingEntry {
    sequence: u64,
    block_ref: BlockRef,
    data: Vec<u8>,
    placement_hint: PlacementHint,
}

/// Internal state of the segment packer.
struct PackerInner {
    pending_entries: Vec<PendingEntry>,
    current_data: Vec<u8>,
    segment_id: u64,
    stats: SegmentPackerStats,
}

impl Default for PackerInner {
    fn default() -> Self {
        Self {
            pending_entries: Vec::new(),
            current_data: Vec::new(),
            segment_id: 1,
            stats: SegmentPackerStats::default(),
        }
    }
}

/// The complete packed segment ready for EC striping.
#[derive(Debug, Clone)]
pub struct PackedSegment {
    /// Segment header
    pub header: SegmentHeader,
    /// Entry directory (ordered by sequence number)
    pub entries: Vec<SegmentEntry>,
    /// Packed data buffer (up to SEGMENT_SIZE)
    pub data: Vec<u8>,
}

/// Configuration for the segment packer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentPackerConfig {
    /// Target segment size in bytes (default: SEGMENT_SIZE = 2MB)
    pub target_size: usize,
    /// Checksum algorithm for segments
    pub checksum_algorithm: ChecksumAlgorithm,
}

impl Default for SegmentPackerConfig {
    fn default() -> Self {
        Self {
            target_size: SEGMENT_SIZE,
            checksum_algorithm: ChecksumAlgorithm::default(),
        }
    }
}

/// Statistics for the segment packer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SegmentPackerStats {
    /// Total segments sealed
    pub segments_sealed: u64,
    /// Total entries packed
    pub entries_packed: u64,
    /// Total bytes packed
    pub bytes_packed: u64,
    /// Current pending entries
    pub pending_entries: usize,
    /// Current pending bytes
    pub pending_bytes: usize,
    /// Next segment ID to be assigned
    pub next_segment_id: u64,
}

/// Segment packer that collects journal entries and produces sealed segments.
pub struct SegmentPacker {
    config: SegmentPackerConfig,
    inner: Mutex<PackerInner>,
}

impl SegmentPacker {
    /// Creates a new segment packer with the given configuration.
    pub fn new(config: SegmentPackerConfig) -> Self {
        debug!(
            "SegmentPacker created: target_size={}, algorithm={}",
            config.target_size, config.checksum_algorithm
        );
        Self {
            config,
            inner: Mutex::new(PackerInner::default()),
        }
    }

    /// Adds an entry to the current segment.
    /// If the segment would exceed target_size, seals the current segment and returns it,
    /// then starts a new one with this entry.
    pub fn add_entry(
        &self,
        sequence: u64,
        block_ref: BlockRef,
        data: Vec<u8>,
        hint: PlacementHint,
    ) -> StorageResult<Option<PackedSegment>> {
        let mut inner = self.inner.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire packer lock".to_string())
        })?;

        let new_data_size = inner.current_data.len() + data.len();

        // If adding this entry would exceed target size, seal current segment first
        if new_data_size > self.config.target_size && !inner.pending_entries.is_empty() {
            let sealed = self.seal_inner(&mut inner)?;
            if let Some(segment) = sealed {
                // Now add the new entry to a fresh segment
                inner.current_data.extend_from_slice(&data);
                inner.pending_entries.push(PendingEntry {
                    sequence,
                    block_ref,
                    data,
                    placement_hint: hint,
                });
                inner.stats.pending_bytes = inner.current_data.len();
                inner.stats.pending_entries = inner.pending_entries.len();
                return Ok(Some(segment));
            }
        }

        // Add the entry to current segment
        let _data_offset = inner.current_data.len() as u32;
        inner.current_data.extend_from_slice(&data);
        inner.pending_entries.push(PendingEntry {
            sequence,
            block_ref,
            data,
            placement_hint: hint,
        });

        inner.stats.pending_bytes = inner.current_data.len();
        inner.stats.pending_entries = inner.pending_entries.len();

        debug!(
            "Added entry seq={}, pending={}, bytes={}",
            sequence,
            inner.pending_entries.len(),
            inner.current_data.len()
        );

        Ok(None)
    }

    /// Force-seal the current segment even if not full.
    /// Returns None if no entries.
    pub fn seal(&self) -> StorageResult<Option<PackedSegment>> {
        let mut inner = self.inner.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire packer lock".to_string())
        })?;

        self.seal_inner(&mut inner)
    }

    /// Internal seal implementation.
    fn seal_inner(&self, inner: &mut PackerInner) -> StorageResult<Option<PackedSegment>> {
        if inner.pending_entries.is_empty() {
            return Ok(None);
        }

        // Compute checksum of the data
        let checksum = compute(self.config.checksum_algorithm, &inner.current_data);

        // Build segment entries with correct offsets
        let mut entries = Vec::with_capacity(inner.pending_entries.len());
        let mut current_offset = 0u32;

        for entry in &inner.pending_entries {
            entries.push(SegmentEntry {
                sequence: entry.sequence,
                block_ref: entry.block_ref,
                data_len: entry.data.len() as u32,
                data_offset: current_offset,
                placement_hint: entry.placement_hint,
            });
            current_offset += entry.data.len() as u32;
        }

        // Determine first and last sequence
        let first_sequence = inner
            .pending_entries
            .first()
            .map(|e| e.sequence)
            .unwrap_or(0);
        let last_sequence = inner
            .pending_entries
            .last()
            .map(|e| e.sequence)
            .unwrap_or(0);

        // Create header
        let header = SegmentHeader::new(
            inner.segment_id,
            entries.len() as u32,
            inner.current_data.len() as u64,
            checksum,
            first_sequence,
            last_sequence,
        );

        // Create the packed segment
        let segment = PackedSegment {
            header,
            entries,
            data: std::mem::take(&mut inner.current_data),
        };

        // Update stats
        inner.stats.segments_sealed += 1;
        inner.stats.entries_packed += inner.pending_entries.len() as u64;
        inner.stats.bytes_packed += segment.data.len() as u64;
        inner.stats.next_segment_id = inner.segment_id + 1;

        debug!(
            "Sealed segment id={}, entries={}, bytes={}",
            inner.segment_id,
            segment.entries.len(),
            segment.data.len()
        );

        // Reset for next segment
        inner.segment_id += 1;
        inner.pending_entries.clear();
        inner.current_data = Vec::new();
        inner.stats.pending_entries = 0;
        inner.stats.pending_bytes = 0;

        Ok(Some(segment))
    }

    /// Returns current pending data size
    pub fn pending_bytes(&self) -> usize {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return 0,
        };
        inner.current_data.len()
    }

    /// Returns current pending entry count
    pub fn pending_count(&self) -> usize {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return 0,
        };
        inner.pending_entries.len()
    }

    /// Returns segment packer statistics
    pub fn stats(&self) -> SegmentPackerStats {
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return SegmentPackerStats::default(),
        };

        SegmentPackerStats {
            segments_sealed: inner.stats.segments_sealed,
            entries_packed: inner.stats.entries_packed,
            bytes_packed: inner.stats.bytes_packed,
            pending_entries: inner.pending_entries.len(),
            pending_bytes: inner.current_data.len(),
            next_segment_id: inner.segment_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSize};
    use crate::checksum::verify;

    fn test_block_ref() -> BlockRef {
        BlockRef {
            id: BlockId::new(0, 100),
            size: BlockSize::B4K,
        }
    }

    #[test]
    fn test_packer_creation() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        assert_eq!(packer.pending_bytes(), 0);
        assert_eq!(packer.pending_count(), 0);

        let stats = packer.stats();
        assert_eq!(stats.segments_sealed, 0);
        assert_eq!(stats.entries_packed, 0);
        assert_eq!(stats.next_segment_id, 1);
    }

    #[test]
    fn test_add_single_entry() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        let data = vec![0u8; 4096];
        let result = packer.add_entry(1, test_block_ref(), data, PlacementHint::Journal);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // No segment sealed yet

        assert_eq!(packer.pending_count(), 1);
        assert_eq!(packer.pending_bytes(), 4096);
    }

    #[test]
    fn test_seal_segment() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        // Add a few entries
        for i in 1..=3 {
            let data = vec![i as u8; 1000];
            packer
                .add_entry(i, test_block_ref(), data, PlacementHint::Journal)
                .unwrap();
        }

        assert_eq!(packer.pending_count(), 3);
        assert_eq!(packer.pending_bytes(), 3000);

        // Seal the segment
        let sealed = packer.seal().unwrap();
        assert!(sealed.is_some());

        let segment = sealed.unwrap();
        assert_eq!(segment.header.segment_id, 1);
        assert_eq!(segment.entries.len(), 3);
        assert_eq!(segment.data.len(), 3000);
        assert_eq!(segment.header.first_sequence, 1);
        assert_eq!(segment.header.last_sequence, 3);

        // After seal, pending should be empty
        assert_eq!(packer.pending_count(), 0);
        assert_eq!(packer.pending_bytes(), 0);

        // Stats should be updated
        let stats = packer.stats();
        assert_eq!(stats.segments_sealed, 1);
        assert_eq!(stats.entries_packed, 3);
        assert_eq!(stats.bytes_packed, 3000);
    }

    #[test]
    fn test_auto_seal_on_overflow() {
        // Create a packer with small target size
        let config = SegmentPackerConfig {
            target_size: 1000,
            checksum_algorithm: ChecksumAlgorithm::Crc32c,
        };
        let packer = SegmentPacker::new(config);

        // Add entries that will exceed target size
        let data = vec![1u8; 600];
        let result = packer
            .add_entry(1, test_block_ref(), data.clone(), PlacementHint::Journal)
            .unwrap();

        // Should not seal yet (first entry)
        assert!(result.is_none());
        assert_eq!(packer.pending_count(), 1);

        // Add another entry that will exceed target size
        let result = packer
            .add_entry(2, test_block_ref(), data, PlacementHint::Journal)
            .unwrap();

        // Should auto-seal and return the previous segment
        assert!(result.is_some());

        let segment = result.unwrap();
        assert_eq!(segment.header.segment_id, 1);
        assert_eq!(segment.entries.len(), 1);

        // New segment should have the second entry
        assert_eq!(packer.pending_count(), 1);
        assert_eq!(packer.pending_bytes(), 600);
    }

    #[test]
    fn test_seal_empty() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        let sealed = packer.seal().unwrap();
        assert!(sealed.is_none());

        let stats = packer.stats();
        assert_eq!(stats.segments_sealed, 0);
    }

    #[test]
    fn test_segment_header_fields() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        let data = vec![0u8; 4096];
        packer
            .add_entry(10, test_block_ref(), data, PlacementHint::HotData)
            .unwrap();

        let sealed = packer.seal().unwrap().unwrap();

        assert_eq!(sealed.header.magic, SEGMENT_MAGIC);
        assert_eq!(sealed.header.version, SEGMENT_VERSION);
        assert_eq!(sealed.header.segment_id, 1);
        assert_eq!(sealed.header.entry_count, 1);
        assert_eq!(sealed.header.data_bytes, 4096);
        assert_eq!(sealed.header.first_sequence, 10);
        assert_eq!(sealed.header.last_sequence, 10);
        assert!(sealed.header.sealed_at_secs > 0);
        assert!(sealed.header.checksum.value != 0);
    }

    #[test]
    fn test_entry_data_offsets() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        // Add entries with known sizes
        let data1 = vec![1u8; 100];
        let data2 = vec![2u8; 200];
        let data3 = vec![3u8; 300];

        packer
            .add_entry(1, test_block_ref(), data1, PlacementHint::Journal)
            .unwrap();
        packer
            .add_entry(2, test_block_ref(), data2, PlacementHint::Journal)
            .unwrap();
        packer
            .add_entry(3, test_block_ref(), data3, PlacementHint::Journal)
            .unwrap();

        let sealed = packer.seal().unwrap().unwrap();

        assert_eq!(sealed.entries.len(), 3);
        assert_eq!(sealed.entries[0].data_offset, 0);
        assert_eq!(sealed.entries[0].data_len, 100);
        assert_eq!(sealed.entries[1].data_offset, 100);
        assert_eq!(sealed.entries[1].data_len, 200);
        assert_eq!(sealed.entries[2].data_offset, 300);
        assert_eq!(sealed.entries[2].data_len, 300);

        // Verify data is contiguous in the data buffer
        assert_eq!(sealed.data.len(), 600);
        assert!(sealed.data[..100].iter().all(|&x| x == 1));
        assert!(sealed.data[100..300].iter().all(|&x| x == 2));
        assert!(sealed.data[300..600].iter().all(|&x| x == 3));
    }

    #[test]
    fn test_segment_checksum() {
        let config = SegmentPackerConfig {
            target_size: SEGMENT_SIZE,
            checksum_algorithm: ChecksumAlgorithm::Crc32c,
        };
        let packer = SegmentPacker::new(config);

        let data = b"hello world".to_vec();
        packer
            .add_entry(1, test_block_ref(), data.clone(), PlacementHint::Journal)
            .unwrap();

        let sealed = packer.seal().unwrap().unwrap();

        // Verify checksum
        let expected_checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        assert_eq!(sealed.header.checksum.algorithm, ChecksumAlgorithm::Crc32c);
        assert_eq!(sealed.header.checksum.value, expected_checksum.value);

        // Verify using the verify function
        assert!(verify(&sealed.header.checksum, &sealed.data));
    }

    #[test]
    fn test_packer_stats() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        // Initial stats
        let stats = packer.stats();
        assert_eq!(stats.segments_sealed, 0);
        assert_eq!(stats.entries_packed, 0);
        assert_eq!(stats.bytes_packed, 0);

        // Add entries
        for i in 1..=5 {
            let data = vec![i as u8; 1000];
            packer
                .add_entry(i, test_block_ref(), data, PlacementHint::Journal)
                .unwrap();
        }

        let stats = packer.stats();
        assert_eq!(stats.pending_entries, 5);
        assert_eq!(stats.pending_bytes, 5000);

        // Seal
        packer.seal().unwrap();

        let stats = packer.stats();
        assert_eq!(stats.segments_sealed, 1);
        assert_eq!(stats.entries_packed, 5);
        assert_eq!(stats.bytes_packed, 5000);
        assert_eq!(stats.pending_entries, 0);
        assert_eq!(stats.pending_bytes, 0);
    }

    #[test]
    fn test_multiple_seals() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        // First segment
        packer
            .add_entry(1, test_block_ref(), vec![1u8; 1000], PlacementHint::Journal)
            .unwrap();
        let seg1 = packer.seal().unwrap().unwrap();
        assert_eq!(seg1.header.segment_id, 1);

        // Second segment
        packer
            .add_entry(2, test_block_ref(), vec![2u8; 1000], PlacementHint::Journal)
            .unwrap();
        let seg2 = packer.seal().unwrap().unwrap();
        assert_eq!(seg2.header.segment_id, 2);

        // Third segment
        packer
            .add_entry(3, test_block_ref(), vec![3u8; 1000], PlacementHint::Journal)
            .unwrap();
        let seg3 = packer.seal().unwrap().unwrap();
        assert_eq!(seg3.header.segment_id, 3);

        // Check stats
        let stats = packer.stats();
        assert_eq!(stats.segments_sealed, 3);
        assert_eq!(stats.next_segment_id, 4);
    }

    #[test]
    fn test_segment_header_traits() {
        // Test Debug is implemented
        let header = SegmentHeader::new(
            1,
            10,
            4096,
            Checksum::new(ChecksumAlgorithm::Crc32c, 0x12345678),
            100,
            110,
        );
        let debug_str = format!("{:?}", header);
        assert!(debug_str.contains("SegmentHeader"));
        assert!(debug_str.contains("magic"));
        assert!(debug_str.contains("segment_id"));

        // Test Clone is implemented
        let cloned = header.clone();
        assert_eq!(cloned.segment_id, header.segment_id);

        // Test Serialize/Deserialize derive (compile-time check)
        fn assert_serialize<T: serde::Serialize + serde::de::DeserializeOwned>() {}
        assert_serialize::<SegmentHeader>();
    }

    #[test]
    fn test_placement_hint_preserved() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        packer
            .add_entry(1, test_block_ref(), vec![1u8; 100], PlacementHint::HotData)
            .unwrap();
        packer
            .add_entry(2, test_block_ref(), vec![2u8; 100], PlacementHint::ColdData)
            .unwrap();

        let sealed = packer.seal().unwrap().unwrap();

        assert_eq!(sealed.entries[0].placement_hint, PlacementHint::HotData);
        assert_eq!(sealed.entries[1].placement_hint, PlacementHint::ColdData);
    }

    #[test]
    fn test_block_ref_preserved() {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        let block_ref1 = BlockRef {
            id: BlockId::new(0, 100),
            size: BlockSize::B4K,
        };
        let block_ref2 = BlockRef {
            id: BlockId::new(1, 200),
            size: BlockSize::B64K,
        };

        packer
            .add_entry(1, block_ref1, vec![1u8; 100], PlacementHint::Journal)
            .unwrap();
        packer
            .add_entry(2, block_ref2, vec![2u8; 100], PlacementHint::Journal)
            .unwrap();

        let sealed = packer.seal().unwrap().unwrap();

        assert_eq!(sealed.entries[0].block_ref.id.device_idx, 0);
        assert_eq!(sealed.entries[0].block_ref.id.offset, 100);
        assert_eq!(sealed.entries[0].block_ref.size, BlockSize::B4K);

        assert_eq!(sealed.entries[1].block_ref.id.device_idx, 1);
        assert_eq!(sealed.entries[1].block_ref.id.offset, 200);
        assert_eq!(sealed.entries[1].block_ref.size, BlockSize::B64K);
    }

    #[test]
    fn test_segment_constants() {
        assert_eq!(SEGMENT_SIZE, 2 * 1024 * 1024);
        assert_eq!(SEGMENT_MAGIC, 0x43534547);
    }
}
