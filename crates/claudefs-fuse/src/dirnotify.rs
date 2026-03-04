//! Directory change notification subsystem.
//!
//! Provides a queuing mechanism for directory change events (create, delete, rename,
//! attribute changes) to support FUSE notify APIs. Events are queued per-directory
//! with configurable limits, and clients can drain events for processing.

use crate::inode::InodeId;
use std::collections::{HashMap, HashSet, VecDeque};

/// Directory change event types for notification.
#[derive(Debug, Clone, PartialEq)]
pub enum DirEvent {
    /// A new entry was created in the directory.
    Created {
        /// Inode number of the created entry.
        ino: InodeId,
        /// Name of the created entry.
        name: String,
    },
    /// An entry was deleted from the directory.
    Deleted {
        /// Inode number of the deleted entry.
        ino: InodeId,
        /// Name of the deleted entry.
        name: String,
    },
    /// An entry was renamed within the directory.
    Renamed {
        /// Original name of the entry.
        old_name: String,
        /// New name of the entry.
        new_name: String,
        /// Inode number of the renamed entry.
        ino: InodeId,
    },
    /// An entry's attributes changed.
    Attrib {
        /// Inode number of the entry with changed attributes.
        ino: InodeId,
    },
}

/// Configuration for the directory notification subsystem.
pub struct NotifyConfig {
    /// Maximum number of events queued per directory before oldest is dropped.
    pub max_queue_per_dir: usize,
    /// Maximum number of directories that can be watched simultaneously.
    pub max_dirs_tracked: usize,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            max_queue_per_dir: 256,
            max_dirs_tracked: 1024,
        }
    }
}

/// Directory change notification manager.
///
/// Tracks watched directories and maintains per-directory event queues.
/// Events are dropped oldest-first when queue limits are exceeded.
pub struct DirNotify {
    config: NotifyConfig,
    queues: HashMap<InodeId, VecDeque<DirEvent>>,
    watched: HashSet<InodeId>,
}

impl DirNotify {
    /// Creates a new directory notification manager with the given configuration.
    pub fn new(config: NotifyConfig) -> Self {
        tracing::debug!(
            "Initializing dir notify: max_queue={}, max_dirs={}",
            config.max_queue_per_dir,
            config.max_dirs_tracked
        );

        Self {
            config,
            queues: HashMap::new(),
            watched: HashSet::new(),
        }
    }

    /// Registers a directory for change notifications.
    ///
    /// Returns `true` if the directory is now watched (was not already watched
    /// and limit not exceeded). Returns `false` if the maximum tracked directories
    /// limit would be exceeded.
    pub fn watch(&mut self, dir_ino: InodeId) -> bool {
        if self.watched.contains(&dir_ino) {
            tracing::debug!("Directory {} already watched", dir_ino);
            return true;
        }

        if self.watched.len() >= self.config.max_dirs_tracked {
            tracing::warn!(
                "Max directories tracked ({}) exceeded, cannot watch {}",
                self.config.max_dirs_tracked,
                dir_ino
            );
            return false;
        }

        self.watched.insert(dir_ino);
        self.queues.entry(dir_ino).or_default();

        tracing::debug!("Watching directory: ino={}", dir_ino);

        true
    }

    /// Removes a directory from the watched set and clears its event queue.
    pub fn unwatch(&mut self, dir_ino: InodeId) {
        self.watched.remove(&dir_ino);
        self.queues.remove(&dir_ino);

        tracing::debug!("Unwatched directory: ino={}", dir_ino);
    }

    /// Posts an event to a directory's notification queue.
    ///
    /// Silently ignored if the directory is not being watched.
    /// Drops the oldest event if the queue is at capacity.
    pub fn post(&mut self, dir_ino: InodeId, event: DirEvent) {
        if !self.watched.contains(&dir_ino) {
            return;
        }

        let queue = match self.queues.get_mut(&dir_ino) {
            Some(q) => q,
            None => return,
        };

        if queue.len() >= self.config.max_queue_per_dir {
            queue.pop_front();
            tracing::debug!("Dropped oldest event for dir {} (queue full)", dir_ino);
        }

        queue.push_back(event);

        tracing::debug!(
            "Posted event to dir {}: queue_size={}",
            dir_ino,
            queue.len()
        );
    }

    /// Drains all pending events for a directory, returning them in order.
    ///
    /// Returns an empty vector if the directory is not watched or has no events.
    pub fn drain(&mut self, dir_ino: InodeId) -> Vec<DirEvent> {
        let queue = match self.queues.get_mut(&dir_ino) {
            Some(q) => q,
            None => return Vec::new(),
        };

        let events: Vec<DirEvent> = queue.drain(..).collect();

        if !events.is_empty() {
            tracing::debug!("Drained {} events for dir {}", events.len(), dir_ino);
        }

        events
    }

    /// Returns the number of pending events for a specific directory.
    pub fn pending_count(&self, dir_ino: InodeId) -> usize {
        self.queues.get(&dir_ino).map(|q| q.len()).unwrap_or(0)
    }

    /// Returns a list of all currently watched directory inodes.
    pub fn watched_dirs(&self) -> Vec<InodeId> {
        self.watched.iter().cloned().collect()
    }

    /// Returns `true` if the directory is currently being watched.
    pub fn is_watched(&self, dir_ino: InodeId) -> bool {
        self.watched.contains(&dir_ino)
    }

    /// Returns the total number of pending events across all watched directories.
    pub fn total_pending(&self) -> usize {
        self.queues.values().map(VecDeque::len).sum()
    }
}

impl Default for DirNotify {
    fn default() -> Self {
        Self::new(NotifyConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> NotifyConfig {
        NotifyConfig {
            max_queue_per_dir: 10,
            max_dirs_tracked: 5,
        }
    }

    #[test]
    fn new_dirnotify_has_no_watched_dirs() {
        let notify = DirNotify::new(test_config());
        assert!(notify.watched_dirs().is_empty());
    }

    #[test]
    fn watch_returns_true_for_first_watch() {
        let mut notify = DirNotify::new(test_config());

        let result = notify.watch(1);

        assert!(result);
    }

    #[test]
    fn is_watched_returns_true_after_watch() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);

        assert!(notify.is_watched(1));
    }

    #[test]
    fn is_watched_returns_false_before_watch() {
        let notify = DirNotify::new(test_config());

        assert!(!notify.is_watched(1));
    }

    #[test]
    fn unwatch_removes_the_directory() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.unwatch(1);

        assert!(!notify.is_watched(1));
    }

    #[test]
    fn post_on_unwatched_directory_is_silently_ignored() {
        let mut notify = DirNotify::new(test_config());

        notify.post(
            1,
            DirEvent::Created {
                ino: 2,
                name: "test".to_string(),
            },
        );

        assert_eq!(notify.pending_count(1), 0);
    }

    #[test]
    fn post_on_watched_directory_stores_the_event() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.post(
            1,
            DirEvent::Created {
                ino: 2,
                name: "test".to_string(),
            },
        );

        assert_eq!(notify.pending_count(1), 1);
    }

    #[test]
    fn drain_returns_all_events_in_order() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.post(
            1,
            DirEvent::Created {
                ino: 2,
                name: "a".to_string(),
            },
        );
        notify.post(
            1,
            DirEvent::Created {
                ino: 3,
                name: "b".to_string(),
            },
        );

        let events = notify.drain(1);

        assert_eq!(events.len(), 2);
        if let DirEvent::Created { name, .. } = &events[0] {
            assert_eq!(name, "a");
        }
    }

    #[test]
    fn drain_clears_the_queue() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.post(
            1,
            DirEvent::Created {
                ino: 2,
                name: "test".to_string(),
            },
        );

        notify.drain(1);

        assert_eq!(notify.pending_count(1), 0);
    }

    #[test]
    fn pending_count_reflects_queued_events() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.post(
            1,
            DirEvent::Created {
                ino: 2,
                name: "a".to_string(),
            },
        );
        notify.post(
            1,
            DirEvent::Created {
                ino: 3,
                name: "b".to_string(),
            },
        );

        assert_eq!(notify.pending_count(1), 2);
    }

    #[test]
    fn total_pending_sums_across_all_watched_dirs() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.watch(2);
        notify.post(
            1,
            DirEvent::Created {
                ino: 2,
                name: "a".to_string(),
            },
        );
        notify.post(
            2,
            DirEvent::Created {
                ino: 3,
                name: "b".to_string(),
            },
        );

        assert_eq!(notify.total_pending(), 2);
    }

    #[test]
    fn events_exceeding_limit_are_dropped() {
        let mut notify = DirNotify::new(NotifyConfig {
            max_queue_per_dir: 2,
            max_dirs_tracked: 10,
        });

        notify.watch(1);
        notify.post(
            1,
            DirEvent::Created {
                ino: 2,
                name: "a".to_string(),
            },
        );
        notify.post(
            1,
            DirEvent::Created {
                ino: 3,
                name: "b".to_string(),
            },
        );
        notify.post(
            1,
            DirEvent::Created {
                ino: 4,
                name: "c".to_string(),
            },
        );

        assert_eq!(notify.pending_count(1), 2, "Should drop oldest event");
    }

    #[test]
    fn watch_returns_false_when_limit_exceeded() {
        let mut notify = DirNotify::new(NotifyConfig {
            max_queue_per_dir: 10,
            max_dirs_tracked: 2,
        });

        assert!(notify.watch(1));
        assert!(notify.watch(2));
        assert!(!notify.watch(3), "Should fail when limit exceeded");
    }

    #[test]
    fn events_have_correct_fields_created() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.post(
            1,
            DirEvent::Created {
                ino: 42,
                name: "testfile".to_string(),
            },
        );

        let events = notify.drain(1);
        assert_eq!(events.len(), 1);

        if let DirEvent::Created { ino, name } = &events[0] {
            assert_eq!(*ino, 42);
            assert_eq!(name, "testfile");
        } else {
            panic!("Expected Created event");
        }
    }

    #[test]
    fn events_have_correct_fields_deleted() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.post(
            1,
            DirEvent::Deleted {
                ino: 42,
                name: "testfile".to_string(),
            },
        );

        let events = notify.drain(1);
        if let DirEvent::Deleted { name, .. } = &events[0] {
            assert_eq!(name, "testfile");
        } else {
            panic!("Expected Deleted event");
        }
    }

    #[test]
    fn events_have_correct_fields_renamed() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.post(
            1,
            DirEvent::Renamed {
                old_name: "old".to_string(),
                new_name: "new".to_string(),
                ino: 42,
            },
        );

        let events = notify.drain(1);
        if let DirEvent::Renamed {
            old_name, new_name, ..
        } = &events[0]
        {
            assert_eq!(old_name, "old");
            assert_eq!(new_name, "new");
        } else {
            panic!("Expected Renamed event");
        }
    }

    #[test]
    fn events_have_correct_fields_attrib() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.post(1, DirEvent::Attrib { ino: 42 });

        let events = notify.drain(1);
        if let DirEvent::Attrib { ino } = &events[0] {
            assert_eq!(*ino, 42);
        } else {
            panic!("Expected Attrib event");
        }
    }

    #[test]
    fn watched_dirs_returns_all_watched_inodes() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        notify.watch(2);
        notify.watch(3);

        let dirs = notify.watched_dirs();

        assert_eq!(dirs.len(), 3);
        assert!(dirs.contains(&1));
        assert!(dirs.contains(&2));
        assert!(dirs.contains(&3));
    }

    #[test]
    fn post_multiple_events_to_same_dir() {
        let mut notify = DirNotify::new(test_config());

        notify.watch(1);
        for i in 0..5 {
            notify.post(
                1,
                DirEvent::Created {
                    ino: i,
                    name: format!("file{}", i),
                },
            );
        }

        assert_eq!(notify.pending_count(1), 5);
    }

    #[test]
    fn drain_on_unwatched_dir_returns_empty() {
        let mut notify = DirNotify::new(test_config());

        let events = notify.drain(1);

        assert!(events.is_empty());
    }
}
