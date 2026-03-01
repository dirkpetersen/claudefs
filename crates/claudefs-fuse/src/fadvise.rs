#![allow(dead_code)]

use crate::inode::InodeId;
use std::collections::HashMap;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadviseHint {
    Normal,
    Sequential,
    Random,
    WillNeed,
    DontNeed,
    NoReuse,
}

impl FadviseHint {
    pub const POSIX_FADV_NORMAL: i32 = 0;
    pub const POSIX_FADV_RANDOM: i32 = 1;
    pub const POSIX_FADV_SEQUENTIAL: i32 = 2;
    pub const POSIX_FADV_WILLNEED: i32 = 3;
    pub const POSIX_FADV_DONTNEED: i32 = 4;
    pub const POSIX_FADV_NOREUSE: i32 = 5;

    pub fn from_linux_const(val: i32) -> Option<Self> {
        match val {
            Self::POSIX_FADV_SEQUENTIAL => Some(Self::Sequential),
            Self::POSIX_FADV_RANDOM => Some(Self::Random),
            Self::POSIX_FADV_WILLNEED => Some(Self::WillNeed),
            Self::POSIX_FADV_DONTNEED => Some(Self::DontNeed),
            Self::POSIX_FADV_NOREUSE => Some(Self::NoReuse),
            Self::POSIX_FADV_NORMAL => Some(Self::Normal),
            _ => None,
        }
    }

    pub fn readahead_multiplier(&self) -> u32 {
        match self {
            Self::Normal => 1,
            Self::Sequential => 4,
            Self::Random => 0,
            Self::WillNeed => 2,
            Self::DontNeed => 0,
            Self::NoReuse => 0,
        }
    }

    pub fn suppresses_readahead(&self) -> bool {
        matches!(self, Self::Random | Self::DontNeed | Self::NoReuse)
    }
}

#[derive(Debug, Clone)]
pub struct FileHintState {
    pub ino: InodeId,
    pub hint: FadviseHint,
    pub offset: u64,
    pub len: u64,
}

pub struct HintTracker {
    hints: HashMap<InodeId, FileHintState>,
    max_entries: usize,
}

impl HintTracker {
    pub fn new(max_entries: usize) -> Self {
        Self {
            hints: HashMap::new(),
            max_entries,
        }
    }

    pub fn set_hint(&mut self, ino: InodeId, hint: FadviseHint, offset: u64, len: u64) {
        if self.hints.len() >= self.max_entries && !self.hints.contains_key(&ino) {
            if let Some(first) = self.hints.keys().next().cloned() {
                self.hints.remove(&first);
            }
        }
        self.hints.insert(
            ino,
            FileHintState {
                ino,
                hint,
                offset,
                len,
            },
        );
        debug!("fadvise: set hint {:?} for ino {}", hint, ino);
    }

    pub fn get_hint(&self, ino: InodeId) -> FadviseHint {
        self.hints
            .get(&ino)
            .map(|h| h.hint)
            .unwrap_or(FadviseHint::Normal)
    }

    pub fn clear(&mut self, ino: InodeId) {
        self.hints.remove(&ino);
        debug!("fadvise: cleared hint for ino {}", ino);
    }

    pub fn suggested_readahead(&self, ino: InodeId, base_readahead: u64) -> u64 {
        let mult = self.get_hint(ino).readahead_multiplier();
        base_readahead * mult as u64
    }

    pub fn should_evict_after_read(&self, ino: InodeId) -> bool {
        let hint = self.get_hint(ino);
        matches!(hint, FadviseHint::NoReuse | FadviseHint::DontNeed)
    }

    pub fn should_prefetch_now(&self, ino: InodeId) -> bool {
        matches!(self.get_hint(ino), FadviseHint::WillNeed)
    }

    pub fn len(&self) -> usize {
        self.hints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }
}

#[derive(Debug, Default, Clone)]
pub struct FadviseStats {
    pub hints_received: u64,
    pub sequential_count: u64,
    pub random_count: u64,
    pub willneed_count: u64,
    pub dontneed_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_linux_const_normal() {
        assert_eq!(FadviseHint::from_linux_const(0), Some(FadviseHint::Normal));
    }

    #[test]
    fn test_from_linux_const_random() {
        assert_eq!(FadviseHint::from_linux_const(1), Some(FadviseHint::Random));
    }

    #[test]
    fn test_from_linux_const_sequential() {
        assert_eq!(
            FadviseHint::from_linux_const(2),
            Some(FadviseHint::Sequential)
        );
    }

    #[test]
    fn test_from_linux_const_willneed() {
        assert_eq!(
            FadviseHint::from_linux_const(3),
            Some(FadviseHint::WillNeed)
        );
    }

    #[test]
    fn test_from_linux_const_dontneed() {
        assert_eq!(
            FadviseHint::from_linux_const(4),
            Some(FadviseHint::DontNeed)
        );
    }

    #[test]
    fn test_from_linux_const_noreuse() {
        assert_eq!(FadviseHint::from_linux_const(5), Some(FadviseHint::NoReuse));
    }

    #[test]
    fn test_from_linux_const_unknown_returns_none() {
        assert_eq!(FadviseHint::from_linux_const(99), None);
    }

    #[test]
    fn test_readahead_multiplier_normal() {
        assert_eq!(FadviseHint::Normal.readahead_multiplier(), 1);
    }

    #[test]
    fn test_readahead_multiplier_sequential() {
        assert_eq!(FadviseHint::Sequential.readahead_multiplier(), 4);
    }

    #[test]
    fn test_readahead_multiplier_random() {
        assert_eq!(FadviseHint::Random.readahead_multiplier(), 0);
    }

    #[test]
    fn test_readahead_multiplier_willneed() {
        assert_eq!(FadviseHint::WillNeed.readahead_multiplier(), 2);
    }

    #[test]
    fn test_readahead_multiplier_dontneed() {
        assert_eq!(FadviseHint::DontNeed.readahead_multiplier(), 0);
    }

    #[test]
    fn test_readahead_multiplier_noreuse() {
        assert_eq!(FadviseHint::NoReuse.readahead_multiplier(), 0);
    }

    #[test]
    fn test_suppresses_readahead_random() {
        assert!(FadviseHint::Random.suppresses_readahead());
    }

    #[test]
    fn test_suppresses_readahead_dontneed() {
        assert!(FadviseHint::DontNeed.suppresses_readahead());
    }

    #[test]
    fn test_suppresses_readahead_noreuse() {
        assert!(FadviseHint::NoReuse.suppresses_readahead());
    }

    #[test]
    fn test_suppresses_readahead_others() {
        assert!(!FadviseHint::Normal.suppresses_readahead());
        assert!(!FadviseHint::Sequential.suppresses_readahead());
        assert!(!FadviseHint::WillNeed.suppresses_readahead());
    }

    #[test]
    fn test_set_and_get_hint() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::Sequential, 0, 0);

        assert_eq!(tracker.get_hint(1), FadviseHint::Sequential);
    }

    #[test]
    fn test_get_hint_default() {
        let tracker = HintTracker::new(100);

        assert_eq!(tracker.get_hint(999), FadviseHint::Normal);
    }

    #[test]
    fn test_clear_hint() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::Sequential, 0, 0);
        tracker.clear(1);

        assert_eq!(tracker.get_hint(1), FadviseHint::Normal);
    }

    #[test]
    fn test_suggested_readahead_sequential() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::Sequential, 0, 0);

        assert_eq!(tracker.suggested_readahead(1, 4096), 16384);
    }

    #[test]
    fn test_suggested_readahead_random() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::Random, 0, 0);

        assert_eq!(tracker.suggested_readahead(1, 4096), 0);
    }

    #[test]
    fn test_suggested_readahead_normal() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::Normal, 0, 0);

        assert_eq!(tracker.suggested_readahead(1, 4096), 4096);
    }

    #[test]
    fn test_suggested_readahead_willneed() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::WillNeed, 0, 0);

        assert_eq!(tracker.suggested_readahead(1, 4096), 8192);
    }

    #[test]
    fn test_should_evict_noreuse() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::NoReuse, 0, 0);

        assert!(tracker.should_evict_after_read(1));
    }

    #[test]
    fn test_should_evict_dontneed() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::DontNeed, 0, 0);

        assert!(tracker.should_evict_after_read(1));
    }

    #[test]
    fn test_should_evict_others() {
        let mut tracker = HintTracker::new(100);

        assert!(!tracker.should_evict_after_read(1));

        tracker.set_hint(2, FadviseHint::Normal, 0, 0);
        assert!(!tracker.should_evict_after_read(2));

        tracker.set_hint(3, FadviseHint::Sequential, 0, 0);
        assert!(!tracker.should_evict_after_read(3));

        tracker.set_hint(4, FadviseHint::WillNeed, 0, 0);
        assert!(!tracker.should_evict_after_read(4));
    }

    #[test]
    fn test_should_prefetch_willneed() {
        let mut tracker = HintTracker::new(100);
        tracker.set_hint(1, FadviseHint::WillNeed, 0, 0);

        assert!(tracker.should_prefetch_now(1));
    }

    #[test]
    fn test_should_prefetch_others() {
        let mut tracker = HintTracker::new(100);

        assert!(!tracker.should_prefetch_now(1));

        tracker.set_hint(2, FadviseHint::Normal, 0, 0);
        assert!(!tracker.should_prefetch_now(2));

        tracker.set_hint(3, FadviseHint::Sequential, 0, 0);
        assert!(!tracker.should_prefetch_now(3));

        tracker.set_hint(4, FadviseHint::Random, 0, 0);
        assert!(!tracker.should_prefetch_now(4));

        tracker.set_hint(5, FadviseHint::DontNeed, 0, 0);
        assert!(!tracker.should_prefetch_now(5));

        tracker.set_hint(6, FadviseHint::NoReuse, 0, 0);
        assert!(!tracker.should_prefetch_now(6));
    }

    #[test]
    fn test_max_entries_eviction() {
        let mut tracker = HintTracker::new(2);
        tracker.set_hint(1, FadviseHint::Normal, 0, 0);
        tracker.set_hint(2, FadviseHint::Normal, 0, 0);
        tracker.set_hint(3, FadviseHint::Normal, 0, 0);

        assert!(tracker.len() <= 2);
    }

    #[test]
    fn test_len() {
        let mut tracker = HintTracker::new(100);
        assert_eq!(tracker.len(), 0);

        tracker.set_hint(1, FadviseHint::Normal, 0, 0);
        assert_eq!(tracker.len(), 1);

        tracker.set_hint(2, FadviseHint::Normal, 0, 0);
        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn test_is_empty() {
        let tracker = HintTracker::new(100);
        assert!(tracker.is_empty());
    }
}
