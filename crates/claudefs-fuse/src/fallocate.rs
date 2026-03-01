use crate::error::{FuseError, Result};
use crate::inode::InodeId;

pub const FALLOC_FL_KEEP_SIZE: u32 = 0x01;
pub const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;
pub const FALLOC_FL_COLLAPSE_RANGE: u32 = 0x08;
pub const FALLOC_FL_ZERO_RANGE: u32 = 0x10;
pub const FALLOC_FL_INSERT_RANGE: u32 = 0x20;

#[derive(Debug, Clone, PartialEq)]
pub enum FallocateOp {
    Allocate {
        keep_size: bool,
    },
    PunchHole {
        offset: u64,
        len: u64,
    },
    ZeroRange {
        offset: u64,
        len: u64,
        keep_size: bool,
    },
    CollapseRange {
        offset: u64,
        len: u64,
    },
    InsertRange {
        offset: u64,
        len: u64,
    },
}

impl FallocateOp {
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

    pub fn is_space_saving(&self) -> bool {
        matches!(
            self,
            FallocateOp::PunchHole { .. } | FallocateOp::CollapseRange { .. }
        )
    }

    pub fn modifies_size(&self) -> bool {
        matches!(
            self,
            FallocateOp::Allocate { keep_size: false }
                | FallocateOp::CollapseRange { .. }
                | FallocateOp::InsertRange { .. }
        )
    }

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

pub struct FallocateRequest {
    pub ino: InodeId,
    pub fh: u64,
    pub op: FallocateOp,
}

pub struct FallocateStats {
    pub total_allocations: u64,
    pub total_punch_holes: u64,
    pub total_zero_ranges: u64,
    pub bytes_allocated: u64,
    pub bytes_punched: u64,
}

impl FallocateStats {
    pub fn new() -> Self {
        FallocateStats {
            total_allocations: 0,
            total_punch_holes: 0,
            total_zero_ranges: 0,
            bytes_allocated: 0,
            bytes_punched: 0,
        }
    }

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
