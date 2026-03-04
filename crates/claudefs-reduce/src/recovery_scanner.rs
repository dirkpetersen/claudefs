//! Recovery scanner for rebuilding state from segments after a crash.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// Header metadata for a segment file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentHeader {
    /// Magic bytes to identify valid segments.
    pub magic: [u8; 4],
    /// Unique segment identifier.
    pub segment_id: u64,
    /// Creation timestamp in milliseconds.
    pub created_at_ms: u64,
    /// Number of entries in the segment.
    pub entry_count: u32,
    /// Total size of segment data in bytes.
    pub total_bytes: u64,
    /// Checksum for integrity verification.
    pub checksum: u32,
}

impl SegmentHeader {
    /// Checks if the magic bytes are valid (CFS1).
    pub fn is_valid_magic(&self) -> bool {
        self.magic == [0x43, 0x46, 0x53, 0x31]
    }

    /// Creates a new segment header with the given values.
    pub fn new(
        segment_id: u64,
        created_at_ms: u64,
        entry_count: u32,
        total_bytes: u64,
        checksum: u32,
    ) -> Self {
        Self {
            magic: [0x43, 0x46, 0x53, 0x31],
            segment_id,
            created_at_ms,
            entry_count,
            total_bytes,
            checksum,
        }
    }
}

/// An entry in the recovery log representing a chunk write.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryEntry {
    /// BLAKE3 hash of the chunk data.
    pub chunk_hash: [u8; 32],
    /// Inode identifier of the file.
    pub inode_id: u64,
    /// Logical offset in the file.
    pub logical_offset: u64,
    /// Offset within the segment data.
    pub data_offset: u32,
    /// Size of the data in bytes.
    pub data_size: u32,
}

impl RecoveryEntry {
    /// Creates a new recovery entry with the given values.
    pub fn new(
        chunk_hash: [u8; 32],
        inode_id: u64,
        logical_offset: u64,
        data_offset: u32,
        data_size: u32,
    ) -> Self {
        Self {
            chunk_hash,
            inode_id,
            logical_offset,
            data_offset,
            data_size,
        }
    }
}

/// Report summarizing recovery scan results.
#[derive(Debug, Clone, Default)]
pub struct RecoveryReport {
    /// Number of segments scanned.
    pub segments_scanned: u64,
    /// Number of valid segments.
    pub segments_valid: u64,
    /// Number of corrupt segments.
    pub segments_corrupt: u64,
    /// Number of chunks recovered.
    pub chunks_recovered: u64,
    /// Total bytes recovered.
    pub bytes_recovered: u64,
    /// Number of unique inodes recovered.
    pub inodes_recovered: u64,
}

/// Configuration for the recovery scanner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryScannerConfig {
    /// Stop scanning on first error encountered.
    pub stop_on_first_error: bool,
    /// Verify checksums during scanning.
    pub verify_checksums: bool,
}

impl Default for RecoveryScannerConfig {
    fn default() -> Self {
        Self {
            stop_on_first_error: false,
            verify_checksums: true,
        }
    }
}

/// Errors that can occur during recovery scanning.
#[derive(Error, Debug)]
pub enum RecoveryError {
    /// Segment has invalid magic bytes.
    #[error("invalid segment magic")]
    InvalidMagic,
    /// Segment header is corrupt or truncated.
    #[error("corrupt segment header")]
    CorruptHeader,
    /// Computed checksum does not match stored checksum.
    #[error("checksum mismatch")]
    ChecksumMismatch,
}

/// Scanner for recovering state from segment files after a crash.
pub struct RecoveryScanner {
    config: RecoveryScannerConfig,
}

impl RecoveryScanner {
    /// Creates a new recovery scanner with the given configuration.
    pub fn new(config: RecoveryScannerConfig) -> Self {
        Self { config }
    }

    /// Scans a segment and validates its header and entries.
    pub fn scan_segment(
        &self,
        header: &SegmentHeader,
        entries: &[RecoveryEntry],
    ) -> Result<usize, RecoveryError> {
        if !header.is_valid_magic() {
            return Err(RecoveryError::InvalidMagic);
        }

        if self.config.verify_checksums {
            let computed_checksum = Self::compute_checksum(header, entries);
            if computed_checksum != header.checksum {
                return Err(RecoveryError::ChecksumMismatch);
            }
        }

        if entries.len() != header.entry_count as usize {
            return Err(RecoveryError::CorruptHeader);
        }

        Ok(entries.len())
    }

    fn compute_checksum(header: &SegmentHeader, entries: &[RecoveryEntry]) -> u32 {
        let mut hash: u32 = 0;
        hash = hash.wrapping_add(header.segment_id as u32);
        hash = hash.wrapping_add(header.created_at_ms as u32);
        for entry in entries {
            hash = hash.wrapping_add(entry.inode_id as u32);
            hash = hash.wrapping_add(entry.logical_offset as u32);
        }
        hash
    }

    /// Builds a recovery report from scan results.
    pub fn build_report(&self, results: &[(SegmentHeader, Vec<RecoveryEntry>)]) -> RecoveryReport {
        let mut report = RecoveryReport {
            segments_scanned: results.len() as u64,
            ..Default::default()
        };

        let mut all_inodes: HashSet<u64> = HashSet::new();

        for (_header, entries) in results {
            report.segments_valid += 1;
            report.chunks_recovered += entries.len() as u64;
            report.bytes_recovered += entries.iter().map(|e| e.data_size as u64).sum::<u64>();

            for entry in entries {
                all_inodes.insert(entry.inode_id);
            }
        }

        report.inodes_recovered = all_inodes.len() as u64;
        report
    }

    /// Returns the count of unique inodes in the given entries.
    pub fn unique_inodes(entries: &[RecoveryEntry]) -> usize {
        let mut unique = HashSet::new();
        for entry in entries {
            unique.insert(entry.inode_id);
        }
        unique.len()
    }
}

impl Default for RecoveryScanner {
    fn default() -> Self {
        Self::new(RecoveryScannerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_config_default() {
        let config = RecoveryScannerConfig::default();
        assert!(!config.stop_on_first_error);
        assert!(config.verify_checksums);
    }

    #[test]
    fn segment_header_valid_magic() {
        let header = SegmentHeader::new(1, 1000, 10, 4096, 0);
        assert!(header.is_valid_magic());
    }

    #[test]
    fn segment_header_invalid_magic() {
        let header = SegmentHeader {
            magic: [0x00, 0x00, 0x00, 0x00],
            segment_id: 1,
            created_at_ms: 1000,
            entry_count: 10,
            total_bytes: 4096,
            checksum: 0,
        };
        assert!(!header.is_valid_magic());
    }

    #[test]
    fn scan_segment_valid() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig {
            verify_checksums: false,
            ..Default::default()
        });
        let header = SegmentHeader::new(1, 1000, 2, 4096, 0);
        let entries = vec![
            RecoveryEntry::new([0u8; 32], 1, 0, 100, 1024),
            RecoveryEntry::new([1u8; 32], 1, 1024, 200, 1024),
        ];
        let result = scanner.scan_segment(&header, &entries);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
    }

    #[test]
    fn scan_segment_invalid_magic_returns_error() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig::default());
        let header = SegmentHeader {
            magic: [0x00, 0x00, 0x00, 0x00],
            segment_id: 1,
            created_at_ms: 1000,
            entry_count: 1,
            total_bytes: 1024,
            checksum: 0,
        };
        let entries = vec![RecoveryEntry::new([0u8; 32], 1, 0, 100, 1024)];
        let result = scanner.scan_segment(&header, &entries);
        assert!(matches!(result, Err(RecoveryError::InvalidMagic)));
    }

    #[test]
    fn scan_empty_entries() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig {
            verify_checksums: false,
            ..Default::default()
        });
        let header = SegmentHeader::new(1, 1000, 0, 0, 0);
        let result = scanner.scan_segment(&header, &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn build_report_empty() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig::default());
        let results: Vec<(SegmentHeader, Vec<RecoveryEntry>)> = vec![];
        let report = scanner.build_report(&results);
        assert_eq!(report.segments_scanned, 0);
        assert_eq!(report.segments_valid, 0);
        assert_eq!(report.chunks_recovered, 0);
    }

    #[test]
    fn build_report_all_valid() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig::default());
        let results = vec![
            (
                SegmentHeader::new(1, 1000, 2, 2048, 0),
                vec![
                    RecoveryEntry::new([0u8; 32], 1, 0, 100, 1024),
                    RecoveryEntry::new([1u8; 32], 1, 1024, 200, 1024),
                ],
            ),
            (
                SegmentHeader::new(2, 1001, 1, 1024, 0),
                vec![RecoveryEntry::new([2u8; 32], 2, 0, 300, 1024)],
            ),
        ];
        let report = scanner.build_report(&results);
        assert_eq!(report.segments_scanned, 2);
        assert_eq!(report.segments_valid, 2);
        assert_eq!(report.segments_corrupt, 0);
    }

    #[test]
    fn build_report_some_corrupt() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig::default());
        let results: Vec<(SegmentHeader, Vec<RecoveryEntry>)> = vec![];
        let mut report = scanner.build_report(&results);
        report.segments_scanned = 5;
        report.segments_valid = 3;
        report.segments_corrupt = 2;
        assert_eq!(report.segments_corrupt, 2);
    }

    #[test]
    fn chunks_recovered_count() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig::default());
        let results = vec![(
            SegmentHeader::new(1, 1000, 3, 3072, 0),
            vec![
                RecoveryEntry::new([0u8; 32], 1, 0, 100, 1024),
                RecoveryEntry::new([1u8; 32], 1, 1024, 200, 1024),
                RecoveryEntry::new([2u8; 32], 1, 2048, 300, 1024),
            ],
        )];
        let report = scanner.build_report(&results);
        assert_eq!(report.chunks_recovered, 3);
    }

    #[test]
    fn bytes_recovered_sum() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig::default());
        let results = vec![(
            SegmentHeader::new(1, 1000, 2, 2048, 0),
            vec![
                RecoveryEntry::new([0u8; 32], 1, 0, 100, 1024),
                RecoveryEntry::new([1u8; 32], 1, 1024, 200, 2048),
            ],
        )];
        let report = scanner.build_report(&results);
        assert_eq!(report.bytes_recovered, 3072);
    }

    #[test]
    fn inodes_recovered_count() {
        let scanner = RecoveryScanner::new(RecoveryScannerConfig::default());
        let results = vec![(
            SegmentHeader::new(1, 1000, 3, 3072, 0),
            vec![
                RecoveryEntry::new([0u8; 32], 1, 0, 100, 1024),
                RecoveryEntry::new([1u8; 32], 2, 0, 200, 1024),
                RecoveryEntry::new([2u8; 32], 2, 1024, 300, 1024),
            ],
        )];
        let report = scanner.build_report(&results);
        assert_eq!(report.inodes_recovered, 2);
    }

    #[test]
    fn unique_inodes_empty() {
        let entries: Vec<RecoveryEntry> = vec![];
        assert_eq!(RecoveryScanner::unique_inodes(&entries), 0);
    }

    #[test]
    fn unique_inodes_distinct() {
        let entries = vec![
            RecoveryEntry::new([0u8; 32], 1, 0, 100, 1024),
            RecoveryEntry::new([1u8; 32], 2, 0, 200, 1024),
            RecoveryEntry::new([2u8; 32], 3, 0, 300, 1024),
        ];
        assert_eq!(RecoveryScanner::unique_inodes(&entries), 3);
    }

    #[test]
    fn unique_inodes_with_duplicates() {
        let entries = vec![
            RecoveryEntry::new([0u8; 32], 1, 0, 100, 1024),
            RecoveryEntry::new([1u8; 32], 2, 0, 200, 1024),
            RecoveryEntry::new([2u8; 32], 1, 1024, 300, 1024),
            RecoveryEntry::new([3u8; 32], 2, 2048, 400, 1024),
        ];
        assert_eq!(RecoveryScanner::unique_inodes(&entries), 2);
    }

    #[test]
    fn recovery_entry_fields() {
        let entry = RecoveryEntry::new([0xAB; 32], 42, 1000, 500, 2048);
        assert_eq!(entry.chunk_hash[0], 0xAB);
        assert_eq!(entry.inode_id, 42);
        assert_eq!(entry.logical_offset, 1000);
        assert_eq!(entry.data_offset, 500);
        assert_eq!(entry.data_size, 2048);
    }
}
