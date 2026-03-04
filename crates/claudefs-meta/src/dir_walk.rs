//! Recursive directory tree walker for metadata operations.

use std::collections::HashSet;
use std::sync::Arc;

use tracing::warn;

use crate::directory::DirectoryStore;
use crate::inode::InodeStore;
use crate::types::{FileType, InodeId, MetaError};

/// Configuration for directory walks.
#[derive(Debug, Clone)]
pub struct WalkConfig {
    /// Maximum recursion depth (0 = root only, u32::MAX = unlimited).
    pub max_depth: u32,
    /// Whether to follow symlinks during traversal.
    pub follow_symlinks: bool,
    /// Whether to call the visitor on directories before their children (pre-order).
    /// If false, post-order (children visited first).
    pub pre_order: bool,
}

impl Default for WalkConfig {
    fn default() -> Self {
        Self {
            max_depth: u32::MAX,
            follow_symlinks: false,
            pre_order: true,
        }
    }
}

/// A single node visited during a directory walk.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    /// The inode ID.
    pub ino: InodeId,
    /// The name of this entry in its parent directory.
    pub name: String,
    /// The parent inode ID (InodeId::ROOT_INODE for the root).
    pub parent_ino: InodeId,
    /// The file type.
    pub file_type: FileType,
    /// Current depth (0 for the walk root).
    pub depth: u32,
    /// Full path from the walk root (e.g., "subdir/file.txt").
    pub path: String,
}

/// Statistics accumulated during a walk.
#[derive(Clone, Default, Debug)]
pub struct WalkStats {
    /// Total directories visited.
    pub dirs: u64,
    /// Total regular files visited.
    pub files: u64,
    /// Total symlinks visited.
    pub symlinks: u64,
    /// Total other file types visited.
    pub other: u64,
    /// Maximum depth reached.
    pub max_depth_reached: u32,
}

impl WalkStats {
    /// Returns total number of inodes visited.
    pub fn total_inodes(&self) -> u64 {
        self.dirs + self.files + self.symlinks + self.other
    }
}

/// Controls whether walking continues after visiting a node.
#[derive(Debug, PartialEq, Eq)]
pub enum WalkControl {
    /// Continue walking.
    Continue,
    /// Skip the subtree below this directory (only meaningful for directories).
    SkipSubtree,
    /// Stop the entire walk.
    Stop,
}

/// A directory tree walker.
pub struct DirWalker {
    dir_store: DirectoryStore,
    inode_store: Arc<InodeStore>,
    config: WalkConfig,
}

impl DirWalker {
    /// Creates a new DirWalker with the given directory store and config.
    pub fn new(
        dir_store: DirectoryStore,
        inode_store: Arc<InodeStore>,
        config: WalkConfig,
    ) -> Self {
        Self {
            dir_store,
            inode_store,
            config,
        }
    }

    /// Walks the directory tree starting from `root_ino`, calling `visitor`
    /// for each entry (including the root itself if it's a directory).
    ///
    /// The visitor receives `(entry: &WalkEntry)` and can return `WalkControl`.
    /// Walk stops early if visitor returns `WalkControl::Stop`.
    ///
    /// Returns `WalkStats` with aggregate counts.
    pub fn walk<F>(
        &self,
        root_ino: InodeId,
        root_name: &str,
        visitor: &mut F,
    ) -> Result<WalkStats, MetaError>
    where
        F: FnMut(&WalkEntry) -> WalkControl,
    {
        let mut stats = WalkStats::default();
        let mut visited = HashSet::new();

        let root_attr = self.inode_store.get_inode(root_ino)?;

        if root_attr.file_type != FileType::Directory {
            return Err(MetaError::NotADirectory(root_ino));
        }

        let root_entry = WalkEntry {
            ino: root_ino,
            name: root_name.to_string(),
            parent_ino: InodeId::ROOT_INODE,
            file_type: FileType::Directory,
            depth: 0,
            path: root_name.to_string(),
        };

        self.walk_recursive(&root_entry, &mut stats, &mut visited, visitor)
    }

    fn walk_recursive<F>(
        &self,
        entry: &WalkEntry,
        stats: &mut WalkStats,
        visited: &mut HashSet<u64>,
        visitor: &mut F,
    ) -> Result<WalkStats, MetaError>
    where
        F: FnMut(&WalkEntry) -> WalkControl,
    {
        if visited.contains(&entry.ino.as_u64()) {
            warn!("cycle detected: revisiting inode {}", entry.ino);
            return Ok(stats.clone());
        }
        visited.insert(entry.ino.as_u64());

        match entry.file_type {
            FileType::Directory => stats.dirs += 1,
            FileType::RegularFile => stats.files += 1,
            FileType::Symlink => stats.symlinks += 1,
            _ => stats.other += 1,
        }

        if entry.depth > stats.max_depth_reached {
            stats.max_depth_reached = entry.depth;
        }

        let control = if self.config.pre_order {
            visitor(entry)
        } else {
            WalkControl::Continue
        };

        match control {
            WalkControl::Stop => return Ok(stats.clone()),
            WalkControl::SkipSubtree => return Ok(stats.clone()),
            WalkControl::Continue => {}
        }

        if entry.file_type == FileType::Directory && entry.depth < self.config.max_depth {
            if let Ok(entries) = self.dir_store.list_entries(entry.ino) {
                for dir_entry in entries {
                    let should_recurse = match dir_entry.file_type {
                        FileType::Directory => true,
                        FileType::Symlink => self.config.follow_symlinks,
                        _ => false,
                    };

                    if should_recurse {
                        let child_entry = WalkEntry {
                            ino: dir_entry.ino,
                            name: dir_entry.name.clone(),
                            parent_ino: entry.ino,
                            file_type: dir_entry.file_type,
                            depth: entry.depth + 1,
                            path: format!("{}/{}", entry.path, dir_entry.name),
                        };
                        self.walk_recursive(&child_entry, stats, visited, visitor)?;
                    } else {
                        let non_dir_entry = WalkEntry {
                            ino: dir_entry.ino,
                            name: dir_entry.name.clone(),
                            parent_ino: entry.ino,
                            file_type: dir_entry.file_type,
                            depth: entry.depth + 1,
                            path: format!("{}/{}", entry.path, dir_entry.name),
                        };
                        let control = visitor(&non_dir_entry);
                        if control == WalkControl::Stop {
                            return Ok(stats.clone());
                        }
                        match dir_entry.file_type {
                            FileType::Directory => stats.dirs += 1,
                            FileType::RegularFile => stats.files += 1,
                            FileType::Symlink => stats.symlinks += 1,
                            _ => stats.other += 1,
                        }
                        if non_dir_entry.depth > stats.max_depth_reached {
                            stats.max_depth_reached = non_dir_entry.depth;
                        }
                    }
                }
            }
        }

        if !self.config.pre_order {
            visitor(entry);
        }

        Ok(stats.clone())
    }

    /// Collects all inodes reachable from `root_ino` up to `max_depth`.
    /// Returns (ino, path, file_type) tuples in visit order.
    pub fn collect_all(&self, root_ino: InodeId) -> Result<Vec<WalkEntry>, MetaError> {
        let mut entries = Vec::new();
        let mut visitor = |entry: &WalkEntry| {
            entries.push(entry.clone());
            WalkControl::Continue
        };
        let name = if root_ino == InodeId::ROOT_INODE {
            "root"
        } else {
            ""
        };
        self.walk(root_ino, name, &mut visitor)?;
        Ok(entries)
    }

    /// Counts all inodes in the subtree rooted at `root_ino` by file type.
    pub fn count_by_type(&self, root_ino: InodeId) -> Result<WalkStats, MetaError> {
        let mut stats = WalkStats::default();
        let mut visited = HashSet::new();

        let root_attr = self.inode_store.get_inode(root_ino)?;
        if root_attr.file_type != FileType::Directory {
            return Err(MetaError::NotADirectory(root_ino));
        }

        visited.insert(root_ino.as_u64());

        self.count_recursive(root_ino, 0, &mut stats, &mut visited)
    }

    fn count_recursive(
        &self,
        dir_ino: InodeId,
        depth: u32,
        stats: &mut WalkStats,
        visited: &mut HashSet<u64>,
    ) -> Result<WalkStats, MetaError> {
        if depth > stats.max_depth_reached {
            stats.max_depth_reached = depth;
        }

        if let Ok(entries) = self.dir_store.list_entries(dir_ino) {
            for entry in entries {
                let entry_ino = entry.ino.as_u64();
                if visited.contains(&entry_ino) {
                    continue;
                }
                visited.insert(entry_ino);

                match entry.file_type {
                    FileType::Directory => stats.dirs += 1,
                    FileType::RegularFile => stats.files += 1,
                    FileType::Symlink => stats.symlinks += 1,
                    _ => stats.other += 1,
                }

                if entry.file_type == FileType::Directory && depth + 1 < self.config.max_depth {
                    self.count_recursive(entry.ino, depth + 1, stats, visited)?;
                }
            }
        }
        Ok(stats.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::directory::DirectoryStore;
    use crate::inode::InodeStore;
    use crate::kvstore::MemoryKvStore;
    use crate::types::{DirEntry, FileType, InodeAttr, InodeId};

    use super::{DirWalker, WalkConfig, WalkControl};

    fn make_walker(entries: Vec<(InodeId, Vec<(String, InodeId, FileType)>)>) -> DirWalker {
        let kv: Arc<dyn crate::kvstore::KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());

        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        inodes.create_inode(&root).unwrap();

        for (parent_ino, children) in entries {
            for (name, child_ino, file_type) in children {
                let attr = match file_type {
                    FileType::Directory => InodeAttr::new_directory(child_ino, 0, 0, 0o755, 1),
                    FileType::RegularFile => InodeAttr::new_file(child_ino, 0, 0, 0o644, 1),
                    FileType::Symlink => {
                        InodeAttr::new_symlink(child_ino, 0, 0, 0o777, 1, "/target".to_string())
                    }
                    _ => InodeAttr::new_file(child_ino, 0, 0, 0o644, 1),
                };
                inodes.create_inode(&attr).unwrap();

                let entry = DirEntry {
                    name,
                    ino: child_ino,
                    file_type,
                };
                dirs.create_entry(parent_ino, &entry).unwrap();
            }
        }

        DirWalker::new(dirs, inodes, WalkConfig::default())
    }

    #[test]
    fn test_walk_empty_dir() {
        let walker = make_walker(vec![]);
        let mut visited = Vec::new();
        let mut visitor = |e: &super::WalkEntry| {
            visited.push((e.ino, e.name.clone()));
            super::WalkControl::Continue
        };
        let stats = walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();
        assert_eq!(stats.dirs, 1);
        assert_eq!(stats.files, 0);
        assert_eq!(stats.total_inodes(), 1);
    }

    #[test]
    fn test_walk_single_file() {
        let walker = make_walker(vec![(
            InodeId::ROOT_INODE,
            vec![(
                "file.txt".to_string(),
                InodeId::new(2),
                FileType::RegularFile,
            )],
        )]);
        let mut visited = Vec::new();
        let mut visitor = |e: &super::WalkEntry| {
            visited.push((e.ino, e.name.clone()));
            super::WalkControl::Continue
        };
        let stats = walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();
        assert_eq!(stats.dirs, 1);
        assert_eq!(stats.files, 1);
        assert_eq!(stats.total_inodes(), 2);
    }

    #[test]
    fn test_walk_nested_dirs() {
        let walker = make_walker(vec![
            (
                InodeId::ROOT_INODE,
                vec![("subdir".to_string(), InodeId::new(2), FileType::Directory)],
            ),
            (
                InodeId::new(2),
                vec![(
                    "file.txt".to_string(),
                    InodeId::new(3),
                    FileType::RegularFile,
                )],
            ),
        ]);
        let mut visited = Vec::new();
        let mut visitor = |e: &super::WalkEntry| {
            visited.push((e.ino, e.name.clone(), e.depth));
            super::WalkControl::Continue
        };
        let stats = walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();
        assert_eq!(stats.dirs, 2);
        assert_eq!(stats.files, 1);
    }

    #[test]
    fn test_walk_max_depth_zero() {
        let mut config = WalkConfig::default();
        config.max_depth = 0;
        let kv: Arc<dyn crate::kvstore::KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());
        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        inodes.create_inode(&root).unwrap();

        let subdir_ino = InodeId::new(2);
        let subdir_attr = InodeAttr::new_directory(subdir_ino, 0, 0, 0o755, 1);
        inodes.create_inode(&subdir_attr).unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "subdir".to_string(),
                ino: subdir_ino,
                file_type: FileType::Directory,
            },
        )
        .unwrap();

        let walker = DirWalker::new(dirs, inodes.clone(), config);

        let mut count = 0;
        let mut visitor = |_e: &super::WalkEntry| {
            count += 1;
            super::WalkControl::Continue
        };
        walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_walk_max_depth_one() {
        let mut config = WalkConfig::default();
        config.max_depth = 1;
        let kv: Arc<dyn crate::kvstore::KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());
        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        inodes.create_inode(&root).unwrap();

        let subdir_ino = InodeId::new(2);
        let subdir_attr = InodeAttr::new_directory(subdir_ino, 0, 0, 0o755, 1);
        inodes.create_inode(&subdir_attr).unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "subdir".to_string(),
                ino: subdir_ino,
                file_type: FileType::Directory,
            },
        )
        .unwrap();

        let file_ino = InodeId::new(3);
        let file_attr = InodeAttr::new_file(file_ino, 0, 0, 0o644, 1);
        inodes.create_inode(&file_attr).unwrap();
        dirs.create_entry(
            subdir_ino,
            &DirEntry {
                name: "file.txt".to_string(),
                ino: file_ino,
                file_type: FileType::RegularFile,
            },
        )
        .unwrap();

        let walker = DirWalker::new(dirs, inodes, config);

        let mut visited = Vec::new();
        let mut visitor = |e: &super::WalkEntry| {
            visited.push((e.name.clone(), e.depth));
            super::WalkControl::Continue
        };
        walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();
        assert!(visited.len() >= 2);
    }

    #[test]
    fn test_walk_skip_subtree() {
        let walker = make_walker(vec![
            (
                InodeId::ROOT_INODE,
                vec![("subdir".to_string(), InodeId::new(2), FileType::Directory)],
            ),
            (
                InodeId::new(2),
                vec![(
                    "file.txt".to_string(),
                    InodeId::new(3),
                    FileType::RegularFile,
                )],
            ),
        ]);

        let mut skip_next = false;
        let mut visited = Vec::new();
        let mut visitor = |e: &super::WalkEntry| {
            visited.push(e.name.clone());
            if skip_next && e.file_type == FileType::Directory {
                skip_next = false;
                return WalkControl::SkipSubtree;
            }
            if e.name == "root" {
                skip_next = true;
            }
            WalkControl::Continue
        };

        walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();
        assert!(!visited.contains(&"file.txt".to_string()));
    }

    #[test]
    fn test_walk_stop() {
        let walker = make_walker(vec![(
            InodeId::ROOT_INODE,
            vec![(
                "file.txt".to_string(),
                InodeId::new(2),
                FileType::RegularFile,
            )],
        )]);

        let mut visited = Vec::new();
        let mut visitor = |e: &super::WalkEntry| {
            visited.push(e.name.clone());
            WalkControl::Stop
        };

        let _stats = walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();
        assert_eq!(visited.len(), 1);
    }

    #[test]
    fn test_collect_all_returns_entries() {
        let walker = make_walker(vec![
            (
                InodeId::ROOT_INODE,
                vec![("subdir".to_string(), InodeId::new(2), FileType::Directory)],
            ),
            (
                InodeId::new(2),
                vec![(
                    "file.txt".to_string(),
                    InodeId::new(3),
                    FileType::RegularFile,
                )],
            ),
        ]);

        let entries = walker.collect_all(InodeId::ROOT_INODE).unwrap();
        assert!(entries.len() >= 3);
        assert!(entries.iter().any(|e| e.name == "root"));
        assert!(entries.iter().any(|e| e.name == "subdir"));
        assert!(entries.iter().any(|e| e.name == "file.txt"));
    }

    #[test]
    fn test_count_by_type_mixed() {
        let walker = make_walker(vec![(
            InodeId::ROOT_INODE,
            vec![
                ("dir1".to_string(), InodeId::new(2), FileType::Directory),
                (
                    "file.txt".to_string(),
                    InodeId::new(3),
                    FileType::RegularFile,
                ),
                ("link".to_string(), InodeId::new(4), FileType::Symlink),
            ],
        )]);

        let stats = walker.count_by_type(InodeId::ROOT_INODE).unwrap();
        assert_eq!(stats.dirs, 1);
        assert_eq!(stats.files, 1);
        assert_eq!(stats.symlinks, 1);
    }

    #[test]
    fn test_walk_symlinks_not_followed() {
        let mut config = WalkConfig::default();
        config.follow_symlinks = false;
        let kv: Arc<dyn crate::kvstore::KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());
        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        inodes.create_inode(&root).unwrap();

        let symlink_ino = InodeId::new(2);
        let symlink_attr =
            InodeAttr::new_symlink(symlink_ino, 0, 0, 0o777, 1, "/target".to_string());
        inodes.create_inode(&symlink_attr).unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "symlink".to_string(),
                ino: symlink_ino,
                file_type: FileType::Symlink,
            },
        )
        .unwrap();

        let walker = DirWalker::new(dirs, inodes, config);

        let stats = walker.count_by_type(InodeId::ROOT_INODE).unwrap();
        assert_eq!(stats.symlinks, 1);
    }

    #[test]
    fn test_walk_pre_order() {
        let mut config = WalkConfig::default();
        config.pre_order = true;
        let kv: Arc<dyn crate::kvstore::KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());
        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        inodes.create_inode(&root).unwrap();

        let subdir_ino = InodeId::new(2);
        let subdir_attr = InodeAttr::new_directory(subdir_ino, 0, 0, 0o755, 1);
        inodes.create_inode(&subdir_attr).unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "subdir".to_string(),
                ino: subdir_ino,
                file_type: FileType::Directory,
            },
        )
        .unwrap();

        let file_ino = InodeId::new(3);
        let file_attr = InodeAttr::new_file(file_ino, 0, 0, 0o644, 1);
        inodes.create_inode(&file_attr).unwrap();
        dirs.create_entry(
            subdir_ino,
            &DirEntry {
                name: "file.txt".to_string(),
                ino: file_ino,
                file_type: FileType::RegularFile,
            },
        )
        .unwrap();

        let walker = DirWalker::new(dirs, inodes, config);

        let mut visit_order = Vec::new();
        let mut visitor = |e: &super::WalkEntry| {
            visit_order.push(e.name.clone());
            super::WalkControl::Continue
        };
        walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();

        let root_idx = visit_order.iter().position(|n| n == "root").unwrap();
        let subdir_idx = visit_order.iter().position(|n| n == "subdir").unwrap();
        let _file_idx = visit_order.iter().position(|n| n == "file.txt").unwrap();

        assert!(root_idx < subdir_idx);
    }

    #[test]
    fn test_walk_post_order() {
        let mut config = WalkConfig::default();
        config.pre_order = false;
        let kv: Arc<dyn crate::kvstore::KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());
        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        inodes.create_inode(&root).unwrap();

        let subdir_ino = InodeId::new(2);
        let subdir_attr = InodeAttr::new_directory(subdir_ino, 0, 0, 0o755, 1);
        inodes.create_inode(&subdir_attr).unwrap();
        dirs.create_entry(
            InodeId::ROOT_INODE,
            &DirEntry {
                name: "subdir".to_string(),
                ino: subdir_ino,
                file_type: FileType::Directory,
            },
        )
        .unwrap();

        let file_ino = InodeId::new(3);
        let file_attr = InodeAttr::new_file(file_ino, 0, 0, 0o644, 1);
        inodes.create_inode(&file_attr).unwrap();
        dirs.create_entry(
            subdir_ino,
            &DirEntry {
                name: "file.txt".to_string(),
                ino: file_ino,
                file_type: FileType::RegularFile,
            },
        )
        .unwrap();

        let walker = DirWalker::new(dirs, inodes, config);

        let mut visit_order = Vec::new();
        let mut visitor = |e: &super::WalkEntry| {
            visit_order.push(e.name.clone());
            super::WalkControl::Continue
        };
        walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();

        let root_idx = visit_order.iter().position(|n| n == "root").unwrap();
        let subdir_idx = visit_order.iter().position(|n| n == "subdir").unwrap();

        assert!(subdir_idx < root_idx);
    }

    #[test]
    fn test_walk_stats_max_depth_reached() {
        let walker = make_walker(vec![
            (
                InodeId::ROOT_INODE,
                vec![("dir1".to_string(), InodeId::new(2), FileType::Directory)],
            ),
            (
                InodeId::new(2),
                vec![("dir2".to_string(), InodeId::new(3), FileType::Directory)],
            ),
            (
                InodeId::new(3),
                vec![(
                    "file.txt".to_string(),
                    InodeId::new(4),
                    FileType::RegularFile,
                )],
            ),
        ]);

        let mut visitor = |_e: &super::WalkEntry| WalkControl::Continue;
        let stats = walker
            .walk(InodeId::ROOT_INODE, "root", &mut visitor)
            .unwrap();
        assert_eq!(stats.max_depth_reached, 3);
    }
}
