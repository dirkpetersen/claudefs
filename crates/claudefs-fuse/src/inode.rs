use crate::error::{FuseError, Result};
use std::collections::HashMap;

/// Type alias for inode identifiers (inode numbers).
pub type InodeId = u64;

/// The root inode number in a POSIX filesystem.
pub const ROOT_INODE: InodeId = 1;

/// Type of inode (file classification).
///
/// Corresponds to S_ISREG, S_ISDIR, S_ISLNK, etc. in POSIX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeKind {
    /// Regular file
    File,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
    /// Block device
    BlockDevice,
    /// Character device
    CharDevice,
    /// First-in-first-out (named pipe)
    Fifo,
    /// Unix domain socket
    Socket,
}

/// In-memory representation of an inode and its metadata.
///
/// Stores all POSIX attributes plus internal tracking for directory hierarchy
/// and reference counting.
#[derive(Debug, Clone)]
pub struct InodeEntry {
    /// Inode number (unique within filesystem)
    pub ino: InodeId,
    /// Parent directory inode number
    pub parent: InodeId,
    /// Name of this entry in the parent directory
    pub name: String,
    /// Type of inode
    pub kind: InodeKind,
    /// File size in bytes
    pub size: u64,
    /// Hard link count
    pub nlink: u32,
    /// Owner user ID
    pub uid: u32,
    /// Owner group ID
    pub gid: u32,
    /// File mode (permissions and type bits combined)
    pub mode: u32,
    /// Access time (seconds since epoch)
    pub atime_secs: i64,
    /// Access time (nanoseconds component)
    pub atime_nsecs: u32,
    /// Modification time (seconds since epoch)
    pub mtime_secs: i64,
    /// Modification time (nanoseconds component)
    pub mtime_nsecs: u32,
    /// Change time (seconds since epoch)
    pub ctime_secs: i64,
    /// Change time (nanoseconds component)
    pub ctime_nsecs: u32,
    /// List of child inode numbers (for directories)
    pub children: Vec<InodeId>,
    /// Lookup reference count for kernel cache management
    pub lookup_count: u64,
}

/// In-memory table managing all inode entries.
///
/// Handles inode allocation, hierarchy management, and reference counting.
pub struct InodeTable {
    /// Mapping from inode number to inode entry
    pub(crate) entries: HashMap<InodeId, InodeEntry>,
    /// Next inode number to allocate
    next_ino: InodeId,
}

impl InodeTable {
    /// Creates a new inode table with the root directory pre-populated.
    pub fn new() -> Self {
        let mut table = InodeTable {
            entries: HashMap::new(),
            next_ino: 2,
        };
        table.create_root();
        table
    }

    /// Creates the root directory inode (inode 1) with standard attributes.
    fn create_root(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let entry = InodeEntry {
            ino: ROOT_INODE,
            parent: ROOT_INODE,
            name: String::from("/"),
            kind: InodeKind::Directory,
            size: 4096,
            nlink: 2,
            uid: 0,
            gid: 0,
            mode: 0o755,
            atime_secs: now.as_secs() as i64,
            atime_nsecs: now.subsec_nanos(),
            mtime_secs: now.as_secs() as i64,
            mtime_nsecs: now.subsec_nanos(),
            ctime_secs: now.as_secs() as i64,
            ctime_nsecs: now.subsec_nanos(),
            children: Vec::new(),
            lookup_count: 1,
        };
        self.entries.insert(ROOT_INODE, entry);
    }

    /// Allocates a new inode and adds it to the table.
    ///
    /// Automatically updates the parent directory's children list and handles
    /// nlink updates for directory entries.
    pub fn alloc(
        &mut self,
        parent: InodeId,
        name: &str,
        kind: InodeKind,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Result<InodeId> {
        let ino = self.next_ino;
        self.next_ino += 1;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();

        let (size, nlink) = match kind {
            InodeKind::Directory => (4096, 2),
            InodeKind::Symlink => (0, 1),
            _ => (0, 1),
        };

        let entry = InodeEntry {
            ino,
            parent,
            name: name.to_string(),
            kind,
            size,
            nlink,
            uid,
            gid,
            mode,
            atime_secs: now.as_secs() as i64,
            atime_nsecs: now.subsec_nanos(),
            mtime_secs: now.as_secs() as i64,
            mtime_nsecs: now.subsec_nanos(),
            ctime_secs: now.as_secs() as i64,
            ctime_nsecs: now.subsec_nanos(),
            children: Vec::new(),
            lookup_count: 1,
        };

        if let Some(parent_entry) = self.entries.get_mut(&parent) {
            if matches!(parent_entry.kind, InodeKind::Directory) {
                parent_entry.children.push(ino);
                // Only subdirectories add a hard link to the parent via ".."
                if matches!(kind, InodeKind::Directory) {
                    parent_entry.nlink += 1;
                }
            }
        }

        self.entries.insert(ino, entry);
        Ok(ino)
    }

    /// Looks up an inode by number.
    pub fn get(&self, ino: InodeId) -> Option<&InodeEntry> {
        self.entries.get(&ino)
    }

    /// Looks up an inode by number for mutable access.
    pub fn get_mut(&mut self, ino: InodeId) -> Option<&mut InodeEntry> {
        self.entries.get_mut(&ino)
    }

    /// Looks up a child entry by name in a directory.
    ///
    /// Returns the child inode number if found, None otherwise.
    pub fn lookup_child(&self, parent: InodeId, name: &str) -> Option<InodeId> {
        self.entries.get(&parent).and_then(|p| {
            if !matches!(p.kind, InodeKind::Directory) {
                return None;
            }
            for &child_ino in &p.children {
                if let Some(child) = self.entries.get(&child_ino) {
                    if child.name == name {
                        return Some(child_ino);
                    }
                }
            }
            None
        })
    }

    /// Removes an inode from the table.
    ///
    /// Fails if the inode is a non-empty directory.
    /// Updates parent directory's children list and nlink.
    pub fn remove(&mut self, ino: InodeId) -> Result<()> {
        let entry = self.entries.get(&ino).ok_or(FuseError::NotFound { ino })?;

        if matches!(entry.kind, InodeKind::Directory) && !entry.children.is_empty() {
            return Err(FuseError::NotEmpty { ino });
        }

        let parent = entry.parent;
        if let Some(parent_entry) = self.entries.get_mut(&parent) {
            parent_entry.children.retain(|&c| c != ino);
            parent_entry.nlink = parent_entry.nlink.saturating_sub(1);
        }

        self.entries.remove(&ino);
        Ok(())
    }

    /// Increments the kernel cache lookup count for an inode.
    pub fn add_lookup(&mut self, ino: InodeId) {
        if let Some(entry) = self.entries.get_mut(&ino) {
            entry.lookup_count += 1;
        }
    }

    /// Decrements the kernel cache lookup count and removes inode if count reaches zero.
    pub fn forget(&mut self, ino: InodeId, n: u64) {
        let should_remove = if let Some(entry) = self.entries.get_mut(&ino) {
            entry.lookup_count = entry.lookup_count.saturating_sub(n);
            entry.lookup_count == 0
        } else {
            false
        };
        if should_remove {
            self.entries.remove(&ino);
        }
    }

    /// Returns the number of inodes in the table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Increments the hard link count of an inode.
    pub fn inc_nlink(&mut self, ino: InodeId) {
        if let Some(entry) = self.entries.get_mut(&ino) {
            entry.nlink += 1;
        }
    }

    /// Adds a child to a directory's children list.
    pub fn add_child(&mut self, parent: InodeId, child: InodeId) {
        if let Some(parent_entry) = self.entries.get_mut(&parent) {
            if matches!(parent_entry.kind, InodeKind::Directory) {
                parent_entry.children.push(child);
            }
        }
    }

    /// Creates a hard link to an existing inode in a new directory.
    ///
    /// Fails if the inode is a directory or if the new parent is not a directory.
    /// Updates both the inode's nlink and the parent's children list.
    pub fn link_to(&mut self, ino: InodeId, newparent: InodeId, name: &str) -> Result<()> {
        let entry = self.entries.get(&ino).ok_or(FuseError::NotFound { ino })?;

        if matches!(entry.kind, InodeKind::Directory) {
            return Err(FuseError::IsDirectory { ino });
        }

        if let Some(parent_entry) = self.entries.get_mut(&newparent) {
            if !matches!(parent_entry.kind, InodeKind::Directory) {
                return Err(FuseError::NotDirectory { ino: newparent });
            }
            parent_entry.children.push(ino);
        } else {
            return Err(FuseError::NotFound { ino: newparent });
        }

        if let Some(entry) = self.entries.get_mut(&ino) {
            entry.nlink += 1;
            entry.parent = newparent;
            entry.name = name.to_string();
        }

        Ok(())
    }
}

impl Default for InodeTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_inode_pre_populated() {
        let table = InodeTable::new();
        let root = table.get(ROOT_INODE).expect("Root should exist");
        assert_eq!(root.ino, ROOT_INODE);
        assert!(matches!(root.kind, InodeKind::Directory));
        assert_eq!(root.mode, 0o755);
        assert_eq!(root.nlink, 2);
    }

    #[test]
    fn test_alloc_file_under_root() {
        let mut table = InodeTable::new();
        let ino = table
            .alloc(ROOT_INODE, "test.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        assert!(ino > ROOT_INODE);
        let entry = table.get(ino).expect("File should exist");
        assert!(matches!(entry.kind, InodeKind::File));
        assert_eq!(entry.name, "test.txt");
    }

    #[test]
    fn test_alloc_directory_with_children() {
        let mut table = InodeTable::new();
        let dir_ino = table
            .alloc(ROOT_INODE, "mydir", InodeKind::Directory, 0o755, 0, 0)
            .unwrap();
        let file_ino = table
            .alloc(dir_ino, "file.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();

        let dir = table.get(dir_ino).expect("Dir should exist");
        assert!(matches!(dir.kind, InodeKind::Directory));
        assert!(dir.children.contains(&file_ino));
        assert_eq!(dir.nlink, 2);
    }

    #[test]
    fn test_lookup_child_finds_existing() {
        let mut table = InodeTable::new();
        let ino = table
            .alloc(ROOT_INODE, "testfile", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        let found = table.lookup_child(ROOT_INODE, "testfile");
        assert_eq!(found, Some(ino));
    }

    #[test]
    fn test_lookup_child_returns_none_for_missing() {
        let table = InodeTable::new();
        let found = table.lookup_child(ROOT_INODE, "nonexistent");
        assert_eq!(found, None);
    }

    #[test]
    fn test_remove_file_succeeds() {
        let mut table = InodeTable::new();
        let ino = table
            .alloc(ROOT_INODE, "test.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        table.remove(ino).expect("Remove should succeed");
        assert!(table.get(ino).is_none());
    }

    #[test]
    fn test_remove_non_empty_directory_fails() {
        let mut table = InodeTable::new();
        let dir_ino = table
            .alloc(ROOT_INODE, "mydir", InodeKind::Directory, 0o755, 0, 0)
            .unwrap();
        table
            .alloc(dir_ino, "file.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();

        let result = table.remove(dir_ino);
        assert!(matches!(result, Err(FuseError::NotEmpty { .. })));
    }

    #[test]
    fn test_forget_decrements_and_removes() {
        let mut table = InodeTable::new();
        let ino = table
            .alloc(ROOT_INODE, "test.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();

        table.add_lookup(ino);
        table.forget(ino, 1);

        let entry = table.get(ino).expect("Should still exist");
        assert_eq!(entry.lookup_count, 1);

        table.forget(ino, 1);
        assert!(table.get(ino).is_none());
    }

    #[test]
    fn test_len_counts_correctly() {
        let mut table = InodeTable::new();
        assert_eq!(table.len(), 1);

        table
            .alloc(ROOT_INODE, "a", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        table
            .alloc(ROOT_INODE, "b", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        assert_eq!(table.len(), 3);
    }
}
