use crate::inode::InodeId;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub enum DirEvent {
    Created {
        ino: InodeId,
        name: String,
    },
    Deleted {
        ino: InodeId,
        name: String,
    },
    Renamed {
        old_name: String,
        new_name: String,
        ino: InodeId,
    },
    Attrib {
        ino: InodeId,
    },
}

pub struct NotifyConfig {
    pub max_queue_per_dir: usize,
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

pub struct DirNotify {
    config: NotifyConfig,
    queues: HashMap<InodeId, VecDeque<DirEvent>>,
    watched: HashSet<InodeId>,
}

impl DirNotify {
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

    pub fn unwatch(&mut self, dir_ino: InodeId) {
        self.watched.remove(&dir_ino);
        self.queues.remove(&dir_ino);

        tracing::debug!("Unwatched directory: ino={}", dir_ino);
    }

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

    pub fn pending_count(&self, dir_ino: InodeId) -> usize {
        self.queues.get(&dir_ino).map(|q| q.len()).unwrap_or(0)
    }

    pub fn watched_dirs(&self) -> Vec<InodeId> {
        self.watched.iter().cloned().collect()
    }

    pub fn is_watched(&self, dir_ino: InodeId) -> bool {
        self.watched.contains(&dir_ino)
    }

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
