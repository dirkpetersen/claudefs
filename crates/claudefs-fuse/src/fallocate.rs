//! Fallocate operations for the FUSE filesystem.
//!
//! This module provides support for the Linux `fallocate` system call, which allows
//! applications to preallocate or deallocate blocks for a file. It implements all
//! standard fallocate modes including punch hole, collapse range, insert range,
//! and zero range operations.
//!
//! # Operations
//!
//! - **Allocate**: Preallocate space for a file, optionally keeping the file size unchanged.
//! - **Punch Hole**: Deallocate blocks within a range, creating a "hole" in the file.
//! - **Zero Range**: Zero out blocks within a range without deallocating them.
//! - **Collapse Range**: Remove a range of blocks, shifting subsequent data left.
//! - **Insert Range**: Insert a range of zeros at a specified offset, shifting data right.

use crate::error::{FuseError, Result};
use crate::inode::InodeId;

/// Keep file size unchanged after allocation.
///
/// When set, preallocated blocks are not accounted toward the file size.
/// Required for `PUNCH_HOLE` mode.
pub const FALLOC_FL_KEEP_SIZE: u32 = 0x01;

/// Deallocate blocks within the specified range.
///
/// Creates a "hole" in the file where reads return zeros.
/// Must be combined with `FALLOC_FL_KEEP_SIZE`.
pub const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;

/// Remove a range of blocks, shifting subsequent data left.
///
/// Reduces file size by `len` bytes. The offset and length must be
/// block-aligned for most filesystems.
pub const FALLOC_FL_COLLAPSE_RANGE: u32 = 0x08;

/// Zero out blocks within the specified range.
///
/// Similar to punch hole but blocks remain allocated. Can be combined
/// with `FALLOC_FL_KEEP_SIZE` to avoid changing file size.
pub const FALLOC_FL_ZERO_RANGE: u32 = 0x10;

/// Insert a range of zeros at the specified offset.
///
/// Shifts data after `offset` right by `len` bytes, increasing file size.
/// The offset must be block-aligned for most filesystems.
pub const FALLOC_FL_INSERT_RANGE: u32 = 0x20;

/// Represents a parsed fallocate operation.
///
/// This enum encapsulates all supported fallocate modes, providing
/// type-safe access to operation parameters and validation of flag combinations.
#[derive(Debug, Clone, PartialEq)]
pub enum FallocateOp {
    /// Preallocate blocks for a file.
    ///
    /// If `keep_size` is false, the file size is extended to cover the
    /// allocated range. If true, blocks are allocated but file size unchanged.
    Allocate {
        /// Whether to keep file size unchanged.
        keep_size: bool,
    },

    /// Deallocate blocks within a range, creating a hole.
    ///
    /// Reads within the hole return zeros. The file size is unchanged.
    PunchHole {
        /// Start offset of the hole.
        offset: u64,
        /// Length of the hole in bytes.
        len: u64,
    },

    /// Zero out blocks within a range.
    ///
    /// Blocks remain allocated but contain zeros. If `keep_size` is false
    /// and the range extends beyond EOF, file size is increased.
    ZeroRange {
        /// Start offset of the zeroed range.
        offset: u64,
        /// Length of the zeroed range in bytes.
        len: u64,
        /// Whether to keep file size unchanged.
        keep_size: bool,
    },

    /// Remove a range of blocks, shifting subsequent data left.
    ///
    /// File size is reduced by `len` bytes. Requires block-aligned
    /// offset and length on most filesystems.
    CollapseRange {
        /// Start offset of the range to collapse.
        offset: u64,
        /// Length of the range to collapse in bytes.
        len: u64,
    },

    /// Insert a range of zeros at the specified offset.
    ///
    /// Data after `offset` is shifted right by `len` bytes, increasing
    /// file size. Requires block-aligned offset on most filesystems.
    InsertRange {
        /// Offset at which to insert zeros.
        offset: u64,
        /// Length of the inserted range in bytes.
        len: u64,
    },
}

impl FallocateOp {
    /// Parses fallocate flags into a typed operation.
    ///
    /// # Arguments
    ///
    /// * `mode` - The fallocate mode flags from the kernel.
    /// * `offset` - The starting offset for the operation.
    /// * `len` - The length of the range in bytes.
    ///
    /// # Errors
    ///
    /// Returns `FuseError::InvalidArgument` if:
    /// - `PUNCH_HOLE` is specified without `KEEP_SIZE`
    /// - `COLLAPSE_RANGE` and `INSERT_RANGE` are both set
    /// - `PUNCH_HOLE` is combined with `COLLAPSE_RANGE` or `INSERT_RANGE`
    pub fn from_flags(mode: u32, offset: u64, len: u64) -> Result<Self> {
        let punch_hole = mode & FALLOC_FL_PUNCH_HOLE != 0;
        let keep_size = mode & FALLOC_FL_KEEP_SIZE != 0;
        let collapse = mode & FALLOC_FL_COLLAPSE_RANGE != 0;
        let zero = mode & FALLOC_FL_ZERO_RANGE != 0;
        let insert = mode & FALLOC_FL_INSERT_RANGE != 0;

        if punch_hole && !keep_size {
            return Err(FuseError::InvalidArgument {
                msg: "PUNCH_HOLE requires KEEP_SIZE".to_string(),
            });
        }

        if collapse && insert {
            return Err(FuseError::InvalidArgument {
                msg: "COLLAPSE_RANGE and INSERT_RANGE are mutually exclusive".to_string(),
            });
        }

        if punch_hole && (collapse || insert) {
            return Err(FuseError::InvalidArgument {
                msg: "PUNCH_HOLE cannot be combined with COLLAPSE_RANGE or INSERT_RANGE"
                    .to_string(),
            });
        }

        if collapse {
            return Ok(FallocateOp::CollapseRange { offset, len });
        }

        if insert {
            return Ok(FallocateOp::InsertRange { offset, len });
        }

        if punch_hole {
            return Ok(FallocateOp::PunchHole { offset, len });
        }

        if zero {
            return Ok(FallocateOp::ZeroRange {
                offset,
                len,
                keep_size,
            });
        }

        Ok(FallocateOp::Allocate { keep_size })
    }

    /// Returns true if this operation frees storage space.
    ///
    /// `PunchHole` and `CollapseRange` reduce storage consumption.
    /// Other operations either allocate new blocks or keep existing ones.
    pub fn is_space_saving(&self) -> bool {
        matches!(
            self,
            FallocateOp::PunchHole { .. } | FallocateOp::CollapseRange { .. }
        )
    }

    /// Returns true if this operation modifies the file size.
    ///
    /// Size-modifying operations are:
    /// - `Allocate` with `keep_size: false` (extends file)
    /// - `CollapseRange` (shrinks file)
    /// - `InsertRange` (grows file)
    pub fn modifies_size(&self) -> bool {
        matches!(
            self,
            FallocateOp::Allocate { keep_size: false }
                | FallocateOp::CollapseRange { .. }
                | FallocateOp::InsertRange { .. }
        )
    }

    /// Returns the affected byte range for operations that target a specific region.
    ///
    /// Returns `None` for simple allocation operations that don't target a specific
    /// range. Returns `Some((offset, len))` for operations with explicit ranges.
    pub fn affected_range(&self) -> Option<(u64, u64)> {
        match self {
            FallocateOp::Allocate { .. } => None,
            FallocateOp::PunchHole { offset, len } => Some((*offset, *len)),
            FallocateOp::ZeroRange { offset, len, .. } => Some((*offset, *len)),
            FallocateOp::CollapseRange { offset, len } => Some((*offset, *len)),
            FallocateOp::InsertRange { offset, len } => Some((*offset, *len)),
        }
    }
}

/// A fallocate request from the FUSE kernel driver.
///
/// Encapsulates all parameters needed to execute a fallocate operation
/// on a specific file handle.
pub struct FallocateRequest {
    /// The inode number of the target file.
    pub ino: InodeId,
    /// The file handle returned by `open`.
    pub fh: u64,
    /// The parsed fallocate operation.
    pub op: FallocateOp,
}

/// Statistics tracking for fallocate operations.
///
/// Maintains counters for each operation type and total bytes affected.
/// Useful for monitoring storage efficiency and workload patterns.
pub struct FallocateStats {
    /// Total number of allocate operations performed.
    pub total_allocations: u64,
    /// Total number of punch hole operations performed.
    pub total_punch_holes: u64,
    /// Total number of zero range operations performed.
    pub total_zero_ranges: u64,
    /// Total bytes allocated across all allocate operations.
    pub bytes_allocated: u64,
    /// Total bytes deallocated across all punch hole operations.
    pub bytes_punched: u64,
}

impl FallocateStats {
    /// Creates a new statistics instance with all counters zeroed.
    pub fn new() -> Self {
        FallocateStats {
            total_allocations: 0,
            total_punch_holes: 0,
            total_zero_ranges: 0,
            bytes_allocated: 0,
            bytes_punched: 0,
        }
    }

    /// Records a completed fallocate operation.
    ///
    /// Updates the appropriate counters based on the operation type.
    /// Only `Allocate`, `PunchHole`, and `ZeroRange` are tracked.
    ///
    /// # Arguments
    ///
    /// * `op` - The operation that was performed.
    /// * `len` - The length in bytes affected by the operation.
    pub fn record(&mut self, op: &FallocateOp, len: u64) {
        match op {
            FallocateOp::Allocate { .. } => {
                self.total_allocations += 1;
                self.bytes_allocated += len;
            }
            FallocateOp::PunchHole { .. } => {
                self.total_punch_holes += 1;
                self.bytes_punched += len;
            }
            FallocateOp::ZeroRange { .. } => {
                self.total_zero_ranges += 1;
            }
            _ => {}
        }
    }
}

impl Default for FallocateStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_flags_allocate() {
        let op = FallocateOp::from_flags(0, 0, 1000).unwrap();
        assert_eq!(op, FallocateOp::Allocate { keep_size: false });
    }

    #[test]
    fn test_from_flags_allocate_keep_size() {
        let op = FallocateOp::from_flags(FALLOC_FL_KEEP_SIZE, 0, 1000).unwrap();
        assert_eq!(op, FallocateOp::Allocate { keep_size: true });
    }

    #[test]
    fn test_from_flags_punch_hole_with_keep_size() {
        let op =
            FallocateOp::from_flags(FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE, 0, 1000).unwrap();
        assert_eq!(
            op,
            FallocateOp::PunchHole {
                offset: 0,
                len: 1000
            }
        );
    }

    #[test]
    fn test_from_flags_punch_hole_without_keep_size_error() {
        let result = FallocateOp::from_flags(FALLOC_FL_PUNCH_HOLE, 0, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_flags_zero_range() {
        let op = FallocateOp::from_flags(FALLOC_FL_ZERO_RANGE, 100, 500).unwrap();
        assert_eq!(
            op,
            FallocateOp::ZeroRange {
                offset: 100,
                len: 500,
                keep_size: false
            }
        );
    }

    #[test]
    fn test_from_flags_zero_range_keep_size() {
        let op =
            FallocateOp::from_flags(FALLOC_FL_ZERO_RANGE | FALLOC_FL_KEEP_SIZE, 100, 500).unwrap();
        assert_eq!(
            op,
            FallocateOp::ZeroRange {
                offset: 100,
                len: 500,
                keep_size: true
            }
        );
    }

    #[test]
    fn test_from_flags_collapse_range() {
        let op = FallocateOp::from_flags(FALLOC_FL_COLLAPSE_RANGE, 100, 500).unwrap();
        assert_eq!(
            op,
            FallocateOp::CollapseRange {
                offset: 100,
                len: 500
            }
        );
    }

    #[test]
    fn test_from_flags_insert_range() {
        let op = FallocateOp::from_flags(FALLOC_FL_INSERT_RANGE, 100, 500).unwrap();
        assert_eq!(
            op,
            FallocateOp::InsertRange {
                offset: 100,
                len: 500
            }
        );
    }

    #[test]
    fn test_from_flags_collapse_and_insert_error() {
        let result =
            FallocateOp::from_flags(FALLOC_FL_COLLAPSE_RANGE | FALLOC_FL_INSERT_RANGE, 100, 500);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_flags_punch_hole_and_collapse_error() {
        let result = FallocateOp::from_flags(
            FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE | FALLOC_FL_COLLAPSE_RANGE,
            100,
            500,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_is_space_saving() {
        assert!(!FallocateOp::Allocate { keep_size: false }.is_space_saving());
        assert!(!FallocateOp::Allocate { keep_size: true }.is_space_saving());
        assert!(FallocateOp::PunchHole {
            offset: 0,
            len: 100
        }
        .is_space_saving());
        assert!(!FallocateOp::ZeroRange {
            offset: 0,
            len: 100,
            keep_size: true
        }
        .is_space_saving());
        assert!(FallocateOp::CollapseRange {
            offset: 0,
            len: 100
        }
        .is_space_saving());
        assert!(!FallocateOp::InsertRange {
            offset: 0,
            len: 100
        }
        .is_space_saving());
    }

    #[test]
    fn test_modifies_size() {
        assert!(FallocateOp::Allocate { keep_size: false }.modifies_size());
        assert!(!FallocateOp::Allocate { keep_size: true }.modifies_size());
        assert!(!FallocateOp::PunchHole {
            offset: 0,
            len: 100
        }
        .modifies_size());
        assert!(!FallocateOp::ZeroRange {
            offset: 0,
            len: 100,
            keep_size: true
        }
        .modifies_size());
        assert!(FallocateOp::CollapseRange {
            offset: 0,
            len: 100
        }
        .modifies_size());
        assert!(FallocateOp::InsertRange {
            offset: 0,
            len: 100
        }
        .modifies_size());
    }

    #[test]
    fn test_affected_range_none_for_allocate() {
        let op = FallocateOp::Allocate { keep_size: false };
        assert_eq!(op.affected_range(), None);
    }

    #[test]
    fn test_affected_range_punch_hole() {
        let op = FallocateOp::PunchHole {
            offset: 100,
            len: 500,
        };
        assert_eq!(op.affected_range(), Some((100, 500)));
    }

    #[test]
    fn test_affected_range_zero_range() {
        let op = FallocateOp::ZeroRange {
            offset: 100,
            len: 500,
            keep_size: true,
        };
        assert_eq!(op.affected_range(), Some((100, 500)));
    }

    #[test]
    fn test_affected_range_collapse_range() {
        let op = FallocateOp::CollapseRange {
            offset: 100,
            len: 500,
        };
        assert_eq!(op.affected_range(), Some((100, 500)));
    }

    #[test]
    fn test_affected_range_insert_range() {
        let op = FallocateOp::InsertRange {
            offset: 100,
            len: 500,
        };
        assert_eq!(op.affected_range(), Some((100, 500)));
    }

    #[test]
    fn test_stats_record_allocate() {
        let mut stats = FallocateStats::new();
        stats.record(&FallocateOp::Allocate { keep_size: false }, 1000);

        assert_eq!(stats.total_allocations, 1);
        assert_eq!(stats.bytes_allocated, 1000);
    }

    #[test]
    fn test_stats_record_punch_hole() {
        let mut stats = FallocateStats::new();
        stats.record(
            &FallocateOp::PunchHole {
                offset: 0,
                len: 500,
            },
            500,
        );

        assert_eq!(stats.total_punch_holes, 1);
        assert_eq!(stats.bytes_punched, 500);
    }

    #[test]
    fn test_stats_record_zero_range() {
        let mut stats = FallocateStats::new();
        stats.record(
            &FallocateOp::ZeroRange {
                offset: 0,
                len: 500,
                keep_size: true,
            },
            500,
        );

        assert_eq!(stats.total_zero_ranges, 1);
    }

    #[test]
    fn test_stats_record_collapse_range() {
        let mut stats = FallocateStats::new();
        stats.record(
            &FallocateOp::CollapseRange {
                offset: 0,
                len: 500,
            },
            500,
        );

        assert_eq!(stats.total_allocations, 0);
        assert_eq!(stats.total_punch_holes, 0);
    }

    #[test]
    fn test_stats_record_insert_range() {
        let mut stats = FallocateStats::new();
        stats.record(
            &FallocateOp::InsertRange {
                offset: 0,
                len: 500,
            },
            500,
        );

        assert_eq!(stats.total_allocations, 0);
        assert_eq!(stats.total_punch_holes, 0);
    }
}
