//! Watch/notify for directory change events
//!
//! Implements an inotify-like mechanism for tracking subscribers to filesystem
//! change events, used by the FUSE client (A5) for directory notifications.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::types::*;

/// Type of filesystem event.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WatchEvent {
    /// A new entry was created in a directory.
    Create {
        /// Parent directory inode
        parent: InodeId,
        /// Name of the new entry
        name: String,
        /// Inode of the new entry
        ino: InodeId,
    },
    /// An entry was deleted from a directory.
    Delete {
        /// Parent directory inode
        parent: InodeId,
        /// Name of the deleted entry
        name: String,
        /// Inode of the deleted entry
        ino: InodeId,
    },
    /// An entry was renamed.
    Rename {
        /// Old parent directory inode
        old_parent: InodeId,
        /// Old name
        old_name: String,
        /// New parent directory inode
        new_parent: InodeId,
        /// New name
        new_name: String,
        /// Inode of the renamed entry
        ino: InodeId,
    },
    /// Inode attributes changed.
    AttrChange {
        /// Inode whose attributes changed
        ino: InodeId,
    },
    /// File data was modified (write, truncate).
    DataChange {
        /// Inode whose data changed
        ino: InodeId,
    },
    /// Extended attributes changed.
    XattrChange {
        /// Inode whose xattrs changed
        ino: InodeId,
    },
    /// Multiple entries were created in a batch operation.
    BatchCreate {
        /// Parent directory inode
        parent: InodeId,
        /// Number of entries created
        count: u32,
    },
}

/// A watch subscription on an inode (directory or file).
#[derive(Clone, Debug)]
pub struct Watch {
    /// Unique watch ID.
    pub id: u64,
    /// The client/node that owns this watch.
    pub client: NodeId,
    /// The inode being watched.
    pub ino: InodeId,
    /// Whether to watch recursively (for directories).
    pub recursive: bool,
}

/// Manages watch subscriptions and event notifications.
pub struct WatchManager {
    /// Counter for generating unique watch IDs.
    next_watch_id: AtomicU64,
    /// Active watches indexed by watch ID.
    watches: RwLock<HashMap<u64, Watch>>,
    /// Index: inode -> watch IDs (for fast lookup when events fire).
    inode_watches: RwLock<HashMap<InodeId, Vec<u64>>>,
    /// Pending events per client.
    pending_events: RwLock<HashMap<NodeId, Vec<WatchEvent>>>,
    /// Maximum pending events per client before dropping.
    max_events_per_client: usize,
}

impl WatchManager {
    /// Creates a new WatchManager with the specified maximum pending events per client.
    pub fn new(max_events_per_client: usize) -> Self {
        Self {
            next_watch_id: AtomicU64::new(1),
            watches: RwLock::new(HashMap::new()),
            inode_watches: RwLock::new(HashMap::new()),
            pending_events: RwLock::new(HashMap::new()),
            max_events_per_client,
        }
    }

    /// Adds a watch and returns the watch ID.
    pub fn add_watch(&self, client: NodeId, ino: InodeId, recursive: bool) -> u64 {
        let watch_id = self.next_watch_id.fetch_add(1, Ordering::Relaxed);
        let watch = Watch {
            id: watch_id,
            client,
            ino,
            recursive,
        };

        // Add to watches map
        self.watches
            .write()
            .expect("lock poisoned")
            .insert(watch_id, watch.clone());

        // Add to inode index
        self.inode_watches
            .write()
            .expect("lock poisoned")
            .entry(ino)
            .or_default()
            .push(watch_id);

        watch_id
    }

    /// Removes a watch, returns true if it existed.
    pub fn remove_watch(&self, watch_id: u64) -> bool {
        let mut watches = self.watches.write().expect("lock poisoned");
        let watch = match watches.remove(&watch_id) {
            Some(w) => w,
            None => return false,
        };

        // Remove from inode index
        let mut inode_watches = self.inode_watches.write().expect("lock poisoned");
        if let Some(watch_ids) = inode_watches.get_mut(&watch.ino) {
            watch_ids.retain(|&id| id != watch_id);
            if watch_ids.is_empty() {
                inode_watches.remove(&watch.ino);
            }
        }

        true
    }

    /// Removes all watches for a client.
    pub fn remove_client_watches(&self, client: NodeId) -> usize {
        let mut watches = self.watches.write().expect("lock poisoned");
        let mut inode_watches = self.inode_watches.write().expect("lock poisoned");

        let watch_ids: Vec<u64> = watches
            .iter()
            .filter(|(_, w)| w.client == client)
            .map(|(&id, _)| id)
            .collect();

        let count = watch_ids.len();

        for watch_id in &watch_ids {
            if let Some(watch) = watches.remove(watch_id) {
                if let Some(watch_ids) = inode_watches.get_mut(&watch.ino) {
                    watch_ids.retain(|&id| id != *watch_id);
                    if watch_ids.is_empty() {
                        inode_watches.remove(&watch.ino);
                    }
                }
            }
        }

        // Clean up pending events for this client
        self.pending_events
            .write()
            .expect("lock poisoned")
            .remove(&client);

        count
    }

    /// Fires an event to all matching watchers.
    /// For Create/Delete/Rename, matches watches on the parent inode.
    /// For AttrChange/DataChange/XattrChange, matches watches on the inode itself.
    pub fn notify(&self, event: WatchEvent) {
        // Determine which inodes to notify based on event type
        let target_inodes: Vec<InodeId> = match &event {
            WatchEvent::Create { parent, .. } => vec![*parent],
            WatchEvent::Delete { parent, .. } => vec![*parent],
            WatchEvent::Rename {
                old_parent,
                new_parent,
                ..
            } => vec![*old_parent, *new_parent],
            WatchEvent::AttrChange { ino } => vec![*ino],
            WatchEvent::DataChange { ino } => vec![*ino],
            WatchEvent::XattrChange { ino } => vec![*ino],
            WatchEvent::BatchCreate { parent, .. } => vec![*parent],
        };

        // Collect all clients to notify
        let mut clients_to_notify: Vec<NodeId> = Vec::new();

        for target_ino in &target_inodes {
            let inode_watches = self.inode_watches.read().expect("lock poisoned");
            if let Some(watch_ids) = inode_watches.get(target_ino) {
                let watches = self.watches.read().expect("lock poisoned");
                for &watch_id in watch_ids {
                    if let Some(watch) = watches.get(&watch_id) {
                        if !clients_to_notify.contains(&watch.client) {
                            clients_to_notify.push(watch.client);
                        }
                    }
                }
            }
        }

        // Queue events to each client
        let mut pending = self.pending_events.write().expect("lock poisoned");
        for client in clients_to_notify {
            let events = pending.entry(client).or_default();
            if events.len() < self.max_events_per_client {
                events.push(event.clone());
            }
        }
    }

    /// Drains and returns all pending events for a client.
    pub fn drain_events(&self, client: NodeId) -> Vec<WatchEvent> {
        self.pending_events
            .write()
            .expect("lock poisoned")
            .remove(&client)
            .unwrap_or_default()
    }

    /// Checks if there are pending events for a client.
    pub fn has_pending_events(&self, client: NodeId) -> bool {
        self.pending_events
            .read()
            .expect("lock poisoned")
            .contains_key(&client)
    }

    /// Returns the total number of active watches.
    pub fn watch_count(&self) -> usize {
        self.watches.read().expect("lock poisoned").len()
    }

    /// Returns all watches on a given inode.
    pub fn watches_on(&self, ino: InodeId) -> Vec<Watch> {
        let inode_watches = self.inode_watches.read().expect("lock poisoned");
        let watches = self.watches.read().expect("lock poisoned");

        inode_watches
            .get(&ino)
            .map(|watch_ids| {
                watch_ids
                    .iter()
                    .filter_map(|&id| watches.get(&id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_remove_watch() {
        let manager = WatchManager::new(100);

        let watch_id = manager.add_watch(NodeId::new(1), InodeId::new(100), false);
        assert_eq!(manager.watch_count(), 1);

        assert!(manager.remove_watch(watch_id));
        assert_eq!(manager.watch_count(), 0);

        // Removing non-existent watch should return false
        assert!(!manager.remove_watch(watch_id));
    }

    #[test]
    fn test_notify_create_event() {
        let manager = WatchManager::new(100);

        // Add watch on parent directory
        manager.add_watch(NodeId::new(1), InodeId::new(100), false);

        // Notify create event
        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "test.txt".to_string(),
            ino: InodeId::new(200),
        });

        let events = manager.drain_events(NodeId::new(1));
        assert_eq!(events.len(), 1);
        match &events[0] {
            WatchEvent::Create { parent, name, ino } => {
                assert_eq!(*parent, InodeId::new(100));
                assert_eq!(name, "test.txt");
                assert_eq!(*ino, InodeId::new(200));
            }
            _ => panic!("expected Create event"),
        }
    }

    #[test]
    fn test_notify_delete_event() {
        let manager = WatchManager::new(100);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);

        manager.notify(WatchEvent::Delete {
            parent: InodeId::new(100),
            name: "test.txt".to_string(),
            ino: InodeId::new(200),
        });

        let events = manager.drain_events(NodeId::new(1));
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], WatchEvent::Delete { .. }));
    }

    #[test]
    fn test_notify_rename_event() {
        let manager = WatchManager::new(100);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);

        manager.notify(WatchEvent::Rename {
            old_parent: InodeId::new(100),
            old_name: "old.txt".to_string(),
            new_parent: InodeId::new(100),
            new_name: "new.txt".to_string(),
            ino: InodeId::new(200),
        });

        let events = manager.drain_events(NodeId::new(1));
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], WatchEvent::Rename { .. }));
    }

    #[test]
    fn test_notify_attr_change() {
        let manager = WatchManager::new(100);

        // Watch on the file itself for attr change
        manager.add_watch(NodeId::new(1), InodeId::new(200), false);

        manager.notify(WatchEvent::AttrChange {
            ino: InodeId::new(200),
        });

        let events = manager.drain_events(NodeId::new(1));
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], WatchEvent::AttrChange { .. }));
    }

    #[test]
    fn test_drain_events() {
        let manager = WatchManager::new(100);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);

        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "a.txt".to_string(),
            ino: InodeId::new(1),
        });
        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "b.txt".to_string(),
            ino: InodeId::new(2),
        });

        let events = manager.drain_events(NodeId::new(1));
        assert_eq!(events.len(), 2);

        // Second drain should be empty
        let events = manager.drain_events(NodeId::new(1));
        assert!(events.is_empty());
    }

    #[test]
    fn test_remove_client_watches() {
        let manager = WatchManager::new(100);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);
        manager.add_watch(NodeId::new(1), InodeId::new(200), false);
        manager.add_watch(NodeId::new(2), InodeId::new(300), false);

        let removed = manager.remove_client_watches(NodeId::new(1));
        assert_eq!(removed, 2);
        assert_eq!(manager.watch_count(), 1);
    }

    #[test]
    fn test_max_events_per_client() {
        let manager = WatchManager::new(2); // Max 2 events

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);

        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "a.txt".to_string(),
            ino: InodeId::new(1),
        });
        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "b.txt".to_string(),
            ino: InodeId::new(2),
        });
        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "c.txt".to_string(),
            ino: InodeId::new(3),
        });

        let events = manager.drain_events(NodeId::new(1));
        assert_eq!(events.len(), 2); // Only first 2 kept
    }

    #[test]
    fn test_watches_on_inode() {
        let manager = WatchManager::new(100);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);
        manager.add_watch(NodeId::new(2), InodeId::new(100), false);
        manager.add_watch(NodeId::new(3), InodeId::new(200), false);

        let watches = manager.watches_on(InodeId::new(100));
        assert_eq!(watches.len(), 2);

        let watches = manager.watches_on(InodeId::new(999));
        assert!(watches.is_empty());
    }

    #[test]
    fn test_watch_count() {
        let manager = WatchManager::new(100);

        assert_eq!(manager.watch_count(), 0);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);
        manager.add_watch(NodeId::new(2), InodeId::new(200), false);

        assert_eq!(manager.watch_count(), 2);

        manager.remove_watch(1);
        assert_eq!(manager.watch_count(), 1);
    }
}
