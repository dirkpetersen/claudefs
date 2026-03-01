//! Memory-mapped file support for ClaudeFS FUSE client.
//!
//! Tracks memory-mapped regions to support FUSE read/write from mmap-backed buffers.
//! Actual mmap() is handled by the kernel; this tracks the state for the FUSE daemon.

use crate::inode::InodeId;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct MmapProt {
    pub read: bool,
    pub write: bool,
    pub exec: bool,
}

#[derive(Debug, Clone)]
pub struct MmapRegion {
    pub ino: InodeId,
    pub fh: u64,
    pub offset: u64,
    pub length: u64,
    pub prot: MmapProt,
    pub flags: u32,
}

pub struct MmapTracker {
    regions: HashMap<u64, MmapRegion>,
    next_id: u64,
    stats: MmapStats,
}

#[derive(Debug, Default, Clone)]
pub struct MmapStats {
    pub total_regions: u64,
    pub total_bytes_mapped: u64,
    pub active_regions: usize,
}

impl MmapTracker {
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
            next_id: 1,
            stats: MmapStats::default(),
        }
    }

    pub fn register(&mut self, region: MmapRegion) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.stats.total_regions += 1;
        self.stats.total_bytes_mapped += region.length;
        self.stats.active_regions = self.regions.len() + 1;

        self.regions.insert(id, region);
        id
    }

    pub fn unregister(&mut self, region_id: u64) -> Option<MmapRegion> {
        if let Some(region) = self.regions.remove(&region_id) {
            self.stats.total_bytes_mapped =
                self.stats.total_bytes_mapped.saturating_sub(region.length);
            self.stats.active_regions = self.regions.len();
            Some(region)
        } else {
            None
        }
    }

    pub fn regions_for_inode(&self, ino: InodeId) -> Vec<&MmapRegion> {
        self.regions.values().filter(|r| r.ino == ino).collect()
    }

    pub fn has_writable_mapping(&self, ino: InodeId) -> bool {
        self.regions.values().any(|r| r.ino == ino && r.prot.write)
    }

    pub fn stats(&self) -> &MmapStats {
        &self.stats
    }

    pub fn total_mapped_bytes(&self) -> u64 {
        self.regions.values().map(|r| r.length).sum()
    }

    pub fn count(&self) -> usize {
        self.regions.len()
    }
}

impl Default for MmapTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_region(ino: InodeId, write: bool) -> MmapRegion {
        MmapRegion {
            ino,
            fh: 1,
            offset: 0,
            length: 4096,
            prot: MmapProt {
                read: true,
                write,
                exec: false,
            },
            flags: 0x01,
        }
    }

    #[test]
    fn test_register_and_unregister_region() {
        let mut tracker = MmapTracker::new();
        let region = make_region(100, false);
        let id = tracker.register(region);

        assert_eq!(tracker.count(), 1);

        let removed = tracker.unregister(id);
        assert!(removed.is_some());
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_regions_for_inode_returns_correct_regions() {
        let mut tracker = MmapTracker::new();

        let region1 = make_region(100, false);
        let region2 = make_region(100, false);
        let region3 = make_region(200, false);

        tracker.register(region1);
        tracker.register(region2);
        tracker.register(region3);

        let regions_100 = tracker.regions_for_inode(100);
        let regions_200 = tracker.regions_for_inode(200);

        assert_eq!(regions_100.len(), 2);
        assert_eq!(regions_200.len(), 1);
    }

    #[test]
    fn test_has_writable_mapping_true_for_write_protected_region() {
        let mut tracker = MmapTracker::new();
        let region = make_region(100, true);
        tracker.register(region);

        assert!(tracker.has_writable_mapping(100));
    }

    #[test]
    fn test_has_writable_mapping_false_for_read_only_region() {
        let mut tracker = MmapTracker::new();
        let region = make_region(100, false);
        tracker.register(region);

        assert!(!tracker.has_writable_mapping(100));
    }

    #[test]
    fn test_stats_track_total_regions() {
        let mut tracker = MmapTracker::new();

        let region = make_region(100, false);
        tracker.register(region);

        assert_eq!(tracker.stats().total_regions, 1);
    }

    #[test]
    fn test_total_mapped_bytes_sums_correctly() {
        let mut tracker = MmapTracker::new();

        let region1 = MmapRegion {
            ino: 100,
            fh: 1,
            offset: 0,
            length: 4096,
            prot: MmapProt {
                read: true,
                write: false,
                exec: false,
            },
            flags: 0,
        };
        let region2 = MmapRegion {
            ino: 100,
            fh: 2,
            offset: 4096,
            length: 8192,
            prot: MmapProt {
                read: true,
                write: false,
                exec: false,
            },
            flags: 0,
        };

        tracker.register(region1);
        tracker.register(region2);

        assert_eq!(tracker.total_mapped_bytes(), 4096 + 8192);
    }

    #[test]
    fn test_count_decrements_on_unregister() {
        let mut tracker = MmapTracker::new();

        let id1 = tracker.register(make_region(100, false));
        let id2 = tracker.register(make_region(100, false));

        assert_eq!(tracker.count(), 2);

        tracker.unregister(id1);
        assert_eq!(tracker.count(), 1);

        tracker.unregister(id2);
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_mmap_prot_fields_accessible() {
        let prot = MmapProt {
            read: true,
            write: true,
            exec: false,
        };

        assert!(prot.read);
        assert!(prot.write);
        assert!(!prot.exec);
    }

    #[test]
    fn test_regions_for_inode_returns_empty_for_unknown_inode() {
        let tracker = MmapTracker::new();

        let regions = tracker.regions_for_inode(999);
        assert!(regions.is_empty());
    }

    #[test]
    fn test_unregister_unknown_id_returns_none() {
        let mut tracker = MmapTracker::new();

        let result = tracker.unregister(999);
        assert!(result.is_none());
    }
}
