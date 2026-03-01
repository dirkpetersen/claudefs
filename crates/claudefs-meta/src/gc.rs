#![allow(missing_docs)]

use crate::types::{InodeId, NodeId, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcConfig {
    pub tombstone_ttl_secs: u64,
    pub orphan_scan_interval_secs: u64,
    pub max_items_per_pass: usize,
    pub stale_lock_timeout_secs: u64,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            tombstone_ttl_secs: 86400,
            orphan_scan_interval_secs: 3600,
            max_items_per_pass: 10000,
            stale_lock_timeout_secs: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GcTask {
    RemoveTombstone {
        inode: InodeId,
        deleted_at: Timestamp,
    },
    ReapOrphan {
        inode: InodeId,
    },
    ReapStaleLock {
        inode: InodeId,
        lock_holder: NodeId,
    },
    PurgeExpiredLease {
        inode: InodeId,
    },
    CompactJournal {
        up_to_seq: u64,
    },
}

impl GcTask {
    pub fn describe(&self) -> String {
        match self {
            GcTask::RemoveTombstone { inode, deleted_at } => {
                format!(
                    "RemoveTombstone inode={} deleted_at={}",
                    inode.as_u64(),
                    deleted_at.secs
                )
            }
            GcTask::ReapOrphan { inode } => format!("ReapOrphan inode={}", inode.as_u64()),
            GcTask::ReapStaleLock { inode, lock_holder } => format!(
                "ReapStaleLock inode={} holder={}",
                inode.as_u64(),
                lock_holder.as_u64()
            ),
            GcTask::PurgeExpiredLease { inode } => {
                format!("PurgeExpiredLease inode={}", inode.as_u64())
            }
            GcTask::CompactJournal { up_to_seq } => {
                format!("CompactJournal up_to_seq={}", up_to_seq)
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcStats {
    pub tombstones_removed: u64,
    pub orphans_reaped: u64,
    pub stale_locks_reaped: u64,
    pub expired_leases_purged: u64,
    pub journal_entries_compacted: u64,
    pub errors: u64,
    pub duration_ms: u64,
}

pub struct GcScheduler {
    config: GcConfig,
    pending: VecDeque<GcTask>,
    completed: Vec<GcTask>,
}

impl GcScheduler {
    pub fn new(config: GcConfig) -> Self {
        Self {
            config,
            pending: VecDeque::new(),
            completed: Vec::new(),
        }
    }

    pub fn submit_task(&mut self, task: GcTask) {
        self.pending.push_back(task);
    }

    pub fn submit_tombstone(&mut self, inode: InodeId, deleted_at: Timestamp) {
        self.pending
            .push_back(GcTask::RemoveTombstone { inode, deleted_at });
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn run_pass(&mut self, now: Timestamp) -> GcStats {
        let mut stats = GcStats::default();
        let max_items = self.config.max_items_per_pass;

        while let Some(task) = self.pending.pop_front() {
            let processed = match &task {
                GcTask::RemoveTombstone { deleted_at, .. } => {
                    let age_secs = now.secs.saturating_sub(deleted_at.secs);
                    if age_secs >= self.config.tombstone_ttl_secs {
                        stats.tombstones_removed += 1;
                        true
                    } else {
                        false
                    }
                }
                GcTask::ReapOrphan { .. } => {
                    stats.orphans_reaped += 1;
                    true
                }
                GcTask::ReapStaleLock { .. } => {
                    stats.stale_locks_reaped += 1;
                    true
                }
                GcTask::PurgeExpiredLease { .. } => {
                    stats.expired_leases_purged += 1;
                    true
                }
                GcTask::CompactJournal { .. } => {
                    stats.journal_entries_compacted += 1;
                    true
                }
            };

            if processed {
                self.completed.push(task);
            }

            let processed_count = stats.tombstones_removed
                + stats.orphans_reaped
                + stats.stale_locks_reaped
                + stats.expired_leases_purged
                + stats.journal_entries_compacted;

            if processed_count >= max_items as u64 {
                break;
            }
        }

        stats.duration_ms = 0;
        stats
    }

    pub fn drain_completed(&mut self) -> Vec<GcTask> {
        std::mem::take(&mut self.completed)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

pub struct OrphanDetector {
    inodes: HashSet<InodeId>,
    dir_entries: HashMap<InodeId, HashSet<InodeId>>,
}

impl OrphanDetector {
    pub fn new() -> Self {
        Self {
            inodes: HashSet::new(),
            dir_entries: HashMap::new(),
        }
    }

    pub fn register_inode(&mut self, inode: InodeId) {
        self.inodes.insert(inode);
    }

    pub fn register_dir_entry(&mut self, parent: InodeId, child: InodeId) {
        self.inodes.insert(child);
        self.dir_entries.entry(parent).or_default().insert(child);
    }

    pub fn remove_inode(&mut self, inode: InodeId) {
        self.inodes.remove(&inode);
        for children in self.dir_entries.values_mut() {
            children.remove(&inode);
        }
    }

    pub fn remove_dir_entry(&mut self, parent: InodeId, child: InodeId) {
        if let Some(children) = self.dir_entries.get_mut(&parent) {
            children.remove(&child);
        }
    }

    pub fn find_orphans(&self) -> Vec<InodeId> {
        let mut referenced: HashSet<InodeId> = HashSet::new();
        for children in self.dir_entries.values() {
            referenced.extend(children);
        }

        self.inodes
            .iter()
            .filter(|ino| **ino != InodeId::ROOT_INODE && !referenced.contains(ino))
            .copied()
            .collect()
    }

    pub fn inode_count(&self) -> usize {
        self.inodes.len()
    }
}

impl Default for OrphanDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_timestamp(secs: u64) -> Timestamp {
        Timestamp { secs, nanos: 0 }
    }

    #[test]
    fn test_gc_config_default() {
        let config = GcConfig::default();
        assert_eq!(config.tombstone_ttl_secs, 86400);
        assert_eq!(config.orphan_scan_interval_secs, 3600);
        assert_eq!(config.max_items_per_pass, 10000);
        assert_eq!(config.stale_lock_timeout_secs, 300);
    }

    #[test]
    fn test_gc_task_describe() {
        let task = GcTask::RemoveTombstone {
            inode: InodeId::new(42),
            deleted_at: make_timestamp(1000),
        };
        assert_eq!(task.describe(), "RemoveTombstone inode=42 deleted_at=1000");

        let task = GcTask::ReapOrphan {
            inode: InodeId::new(100),
        };
        assert_eq!(task.describe(), "ReapOrphan inode=100");

        let task = GcTask::ReapStaleLock {
            inode: InodeId::new(50),
            lock_holder: NodeId::new(3),
        };
        assert_eq!(task.describe(), "ReapStaleLock inode=50 holder=3");

        let task = GcTask::PurgeExpiredLease {
            inode: InodeId::new(200),
        };
        assert_eq!(task.describe(), "PurgeExpiredLease inode=200");

        let task = GcTask::CompactJournal { up_to_seq: 5000 };
        assert_eq!(task.describe(), "CompactJournal up_to_seq=5000");
    }

    #[test]
    fn test_gc_stats_initial() {
        let stats = GcStats::default();
        assert_eq!(stats.tombstones_removed, 0);
        assert_eq!(stats.orphans_reaped, 0);
        assert_eq!(stats.stale_locks_reaped, 0);
        assert_eq!(stats.expired_leases_purged, 0);
        assert_eq!(stats.journal_entries_compacted, 0);
        assert_eq!(stats.errors, 0);
        assert_eq!(stats.duration_ms, 0);
    }

    #[test]
    fn test_submit_task() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);
        assert_eq!(scheduler.pending_count(), 0);

        scheduler.submit_task(GcTask::ReapOrphan {
            inode: InodeId::new(1),
        });
        assert_eq!(scheduler.pending_count(), 1);

        scheduler.submit_task(GcTask::ReapOrphan {
            inode: InodeId::new(2),
        });
        assert_eq!(scheduler.pending_count(), 2);
    }

    #[test]
    fn test_submit_tombstone() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        scheduler.submit_tombstone(InodeId::new(100), make_timestamp(1000));
        assert_eq!(scheduler.pending_count(), 1);

        if let Some(GcTask::RemoveTombstone { inode, deleted_at }) = scheduler.pending.front() {
            assert_eq!(*inode, InodeId::new(100));
            assert_eq!(deleted_at.secs, 1000);
        } else {
            panic!("Expected RemoveTombstone task");
        }
    }

    #[test]
    fn test_run_pass_empty() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);
        let stats = scheduler.run_pass(make_timestamp(1000));

        assert_eq!(stats.tombstones_removed, 0);
        assert_eq!(stats.orphans_reaped, 0);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_run_pass_tombstone_expired() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        let deleted_at = make_timestamp(80000);
        scheduler.submit_tombstone(InodeId::new(1), deleted_at);

        let now = make_timestamp(166401);
        let stats = scheduler.run_pass(now);

        assert_eq!(stats.tombstones_removed, 1);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_run_pass_tombstone_not_expired() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        let deleted_at = make_timestamp(80000);
        scheduler.submit_tombstone(InodeId::new(1), deleted_at);

        let now = make_timestamp(166300);
        let stats = scheduler.run_pass(now);

        assert_eq!(stats.tombstones_removed, 0);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_run_pass_orphan() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        scheduler.submit_task(GcTask::ReapOrphan {
            inode: InodeId::new(100),
        });
        let stats = scheduler.run_pass(make_timestamp(1000));

        assert_eq!(stats.orphans_reaped, 1);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_run_pass_stale_lock() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        scheduler.submit_task(GcTask::ReapStaleLock {
            inode: InodeId::new(50),
            lock_holder: NodeId::new(1),
        });
        let stats = scheduler.run_pass(make_timestamp(1000));

        assert_eq!(stats.stale_locks_reaped, 1);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_run_pass_expired_lease() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        scheduler.submit_task(GcTask::PurgeExpiredLease {
            inode: InodeId::new(75),
        });
        let stats = scheduler.run_pass(make_timestamp(1000));

        assert_eq!(stats.expired_leases_purged, 1);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_run_pass_journal_compact() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        scheduler.submit_task(GcTask::CompactJournal { up_to_seq: 1000 });
        let stats = scheduler.run_pass(make_timestamp(1000));

        assert_eq!(stats.journal_entries_compacted, 1);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_run_pass_max_items() {
        let config = GcConfig {
            max_items_per_pass: 3,
            ..Default::default()
        };
        let mut scheduler = GcScheduler::new(config);

        for i in 1..=5 {
            scheduler.submit_task(GcTask::ReapOrphan {
                inode: InodeId::new(i),
            });
        }

        let stats = scheduler.run_pass(make_timestamp(1000));

        assert_eq!(stats.orphans_reaped, 3);
        assert_eq!(scheduler.pending_count(), 2);
    }

    #[test]
    fn test_drain_completed() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        scheduler.submit_task(GcTask::ReapOrphan {
            inode: InodeId::new(1),
        });
        scheduler.submit_task(GcTask::ReapOrphan {
            inode: InodeId::new(2),
        });
        scheduler.run_pass(make_timestamp(1000));

        let completed = scheduler.drain_completed();
        assert_eq!(completed.len(), 2);

        let drained = scheduler.drain_completed();
        assert!(drained.is_empty());
    }

    #[test]
    fn test_is_empty() {
        let config = GcConfig::default();
        let mut scheduler = GcScheduler::new(config);

        assert!(scheduler.is_empty());

        scheduler.submit_task(GcTask::ReapOrphan {
            inode: InodeId::new(1),
        });
        assert!(!scheduler.is_empty());

        scheduler.run_pass(make_timestamp(1000));
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_orphan_detector_empty() {
        let detector = OrphanDetector::new();
        let orphans = detector.find_orphans();
        assert!(orphans.is_empty());
        assert_eq!(detector.inode_count(), 0);
    }

    #[test]
    fn test_orphan_detector_no_orphans() {
        let mut detector = OrphanDetector::new();

        detector.register_inode(InodeId::ROOT_INODE);
        detector.register_inode(InodeId::new(2));
        detector.register_dir_entry(InodeId::ROOT_INODE, InodeId::new(2));

        let orphans = detector.find_orphans();
        assert!(orphans.is_empty());
    }

    #[test]
    fn test_orphan_detector_finds_orphans() {
        let mut detector = OrphanDetector::new();

        detector.register_inode(InodeId::ROOT_INODE);
        detector.register_inode(InodeId::new(2));
        detector.register_inode(InodeId::new(3));
        detector.register_dir_entry(InodeId::ROOT_INODE, InodeId::new(2));

        let orphans = detector.find_orphans();
        assert_eq!(orphans.len(), 1);
        assert!(orphans.contains(&InodeId::new(3)));
    }

    #[test]
    fn test_orphan_detector_root_excluded() {
        let mut detector = OrphanDetector::new();

        detector.register_inode(InodeId::ROOT_INODE);

        let orphans = detector.find_orphans();
        assert!(orphans.is_empty());
    }

    #[test]
    fn test_orphan_detector_remove_entry_creates_orphan() {
        let mut detector = OrphanDetector::new();

        detector.register_dir_entry(InodeId::ROOT_INODE, InodeId::new(2));
        assert!(detector.find_orphans().is_empty());

        detector.remove_dir_entry(InodeId::ROOT_INODE, InodeId::new(2));
        let orphans = detector.find_orphans();
        assert_eq!(orphans.len(), 1);
        assert!(orphans.contains(&InodeId::new(2)));
    }

    #[test]
    fn test_orphan_detector_remove_inode() {
        let mut detector = OrphanDetector::new();

        detector.register_inode(InodeId::new(100));
        assert_eq!(detector.inode_count(), 1);

        detector.remove_inode(InodeId::new(100));
        assert_eq!(detector.inode_count(), 0);
    }
}
