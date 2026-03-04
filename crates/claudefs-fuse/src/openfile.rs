//! Open file tracking for the FUSE filesystem.
//!
//! This module provides the [`OpenFileTable`] for managing open file handles,
//! tracking read/write modes, offsets, and dirty state for each open file.

use crate::inode::InodeId;
use std::collections::HashMap;

/// Unique identifier for an open file handle.
pub type FileHandle = u64;

/// Access mode flags for an open file.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenFlags {
    /// File opened for read-only access.
    ReadOnly,
    /// File opened for write-only access.
    WriteOnly,
    /// File opened for both reading and writing.
    ReadWrite,
}

impl OpenFlags {
    /// Returns `true` if the file can be read from.
    pub fn is_readable(&self) -> bool {
        match self {
            OpenFlags::ReadOnly | OpenFlags::ReadWrite => true,
            OpenFlags::WriteOnly => false,
        }
    }

    /// Returns `true` if the file can be written to.
    pub fn is_writable(&self) -> bool {
        match self {
            OpenFlags::WriteOnly | OpenFlags::ReadWrite => true,
            OpenFlags::ReadOnly => false,
        }
    }

    /// Converts a libc `O_RDONLY`/`O_WRONLY`/`O_RDWR` value to `OpenFlags`.
    ///
    /// The low two bits of the flags determine the mode:
    /// - `0` → `ReadOnly`
    /// - `1` → `WriteOnly`
    /// - `2` → `ReadWrite`
    pub fn from_libc(flags: i32) -> Self {
        match flags & 0o3 {
            0 => OpenFlags::ReadOnly,
            1 => OpenFlags::WriteOnly,
            2 => OpenFlags::ReadWrite,
            _ => OpenFlags::ReadOnly,
        }
    }
}

/// Entry representing a single open file in the table.
#[derive(Debug, Clone)]
pub struct OpenFileEntry {
    /// The file handle assigned to this open file.
    pub fh: FileHandle,
    /// The inode number of the opened file.
    pub ino: InodeId,
    /// The access mode flags for this open file.
    pub flags: OpenFlags,
    /// The current read/write offset within the file.
    pub offset: u64,
    /// Whether the file has been modified since being opened.
    pub dirty: bool,
}

/// Table tracking all currently open files in the filesystem.
///
/// The table assigns unique file handles, tracks offsets and dirty state,
/// and allows lookup by handle or by inode.
pub struct OpenFileTable {
    next_fh: FileHandle,
    entries: HashMap<FileHandle, OpenFileEntry>,
}

impl OpenFileTable {
    /// Creates a new empty open file table.
    pub fn new() -> Self {
        tracing::debug!("Creating new open file table");
        Self {
            next_fh: 1,
            entries: HashMap::new(),
        }
    }

    /// Opens a file and returns a new file handle.
    ///
    /// The handle is unique and monotonically increasing.
    pub fn open(&mut self, ino: InodeId, flags: OpenFlags) -> FileHandle {
        let fh = self.next_fh;
        self.next_fh += 1;

        let flags_clone = flags.clone();
        let entry = OpenFileEntry {
            fh,
            ino,
            flags,
            offset: 0,
            dirty: false,
        };

        self.entries.insert(fh, entry);

        tracing::debug!(
            "Opened file: ino={}, fh={}, flags={:?}",
            ino,
            fh,
            flags_clone
        );

        fh
    }

    /// Returns a reference to the entry for the given file handle.
    pub fn get(&self, fh: FileHandle) -> Option<&OpenFileEntry> {
        self.entries.get(&fh)
    }

    /// Returns a mutable reference to the entry for the given file handle.
    pub fn get_mut(&mut self, fh: FileHandle) -> Option<&mut OpenFileEntry> {
        self.entries.get_mut(&fh)
    }

    /// Closes the file handle and removes it from the table.
    ///
    /// Returns the removed entry if the handle existed.
    pub fn close(&mut self, fh: FileHandle) -> Option<OpenFileEntry> {
        let entry = self.entries.remove(&fh);

        if entry.is_some() {
            tracing::debug!("Closed file handle: fh={}", fh);
        }

        entry
    }

    /// Sets the offset for the given file handle.
    ///
    /// Returns `true` if the handle exists, `false` otherwise.
    pub fn seek(&mut self, fh: FileHandle, offset: u64) -> bool {
        match self.entries.get_mut(&fh) {
            Some(entry) => {
                entry.offset = offset;
                tracing::debug!("Seek: fh={}, offset={}", fh, offset);
                true
            }
            None => {
                tracing::warn!("Seek on unknown handle: fh={}", fh);
                false
            }
        }
    }

    /// Marks the given file handle as dirty (modified).
    ///
    /// Returns `true` if the handle exists, `false` otherwise.
    pub fn mark_dirty(&mut self, fh: FileHandle) -> bool {
        match self.entries.get_mut(&fh) {
            Some(entry) => {
                entry.dirty = true;
                tracing::debug!("Marked dirty: fh={}", fh);
                true
            }
            None => {
                tracing::warn!("Mark dirty on unknown handle: fh={}", fh);
                false
            }
        }
    }

    /// Marks the given file handle as clean (not modified).
    ///
    /// Returns `true` if the handle exists, `false` otherwise.
    pub fn mark_clean(&mut self, fh: FileHandle) -> bool {
        match self.entries.get_mut(&fh) {
            Some(entry) => {
                let was_dirty = entry.dirty;
                entry.dirty = false;
                if was_dirty {
                    tracing::debug!("Marked clean: fh={}", fh);
                }
                true
            }
            None => {
                tracing::warn!("Mark clean on unknown handle: fh={}", fh);
                false
            }
        }
    }

    /// Returns all file handles that reference the given inode.
    pub fn handles_for_inode(&self, ino: InodeId) -> Vec<FileHandle> {
        self.entries
            .values()
            .filter(|e| e.ino == ino)
            .map(|e| e.fh)
            .collect()
    }

    /// Returns the number of currently open file handles.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the number of open file handles that are marked dirty.
    pub fn dirty_count(&self) -> usize {
        self.entries.values().filter(|e| e.dirty).count()
    }
}

impl Default for OpenFileTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_table_has_count_zero() {
        let table = OpenFileTable::new();
        assert_eq!(table.count(), 0);
    }

    #[test]
    fn open_returns_distinct_handles() {
        let mut table = OpenFileTable::new();

        let fh1 = table.open(1, OpenFlags::ReadOnly);
        let fh2 = table.open(1, OpenFlags::ReadWrite);
        let fh3 = table.open(2, OpenFlags::WriteOnly);

        assert_ne!(fh1, fh2);
        assert_ne!(fh2, fh3);
        assert_ne!(fh1, fh3);
    }

    #[test]
    fn get_returns_some_after_open() {
        let mut table = OpenFileTable::new();

        let fh = table.open(1, OpenFlags::ReadOnly);
        let entry = table.get(fh);

        assert!(entry.is_some());
        assert_eq!(entry.unwrap().ino, 1);
    }

    #[test]
    fn get_returns_none_for_unknown_handle() {
        let table = OpenFileTable::new();

        let entry = table.get(999);

        assert!(entry.is_none());
    }

    #[test]
    fn close_returns_the_entry() {
        let mut table = OpenFileTable::new();

        let fh = table.open(1, OpenFlags::ReadOnly);
        let entry = table.close(fh);

        assert!(entry.is_some());
        assert_eq!(entry.unwrap().ino, 1);
    }

    #[test]
    fn get_returns_none_after_close() {
        let mut table = OpenFileTable::new();

        let fh = table.open(1, OpenFlags::ReadOnly);
        table.close(fh);

        let entry = table.get(fh);

        assert!(entry.is_none());
    }

    #[test]
    fn count_reflects_open_close_lifecycle() {
        let mut table = OpenFileTable::new();

        assert_eq!(table.count(), 0);

        let fh1 = table.open(1, OpenFlags::ReadOnly);
        assert_eq!(table.count(), 1);

        let fh2 = table.open(2, OpenFlags::ReadWrite);
        assert_eq!(table.count(), 2);

        table.close(fh1);
        assert_eq!(table.count(), 1);

        table.close(fh2);
        assert_eq!(table.count(), 0);
    }

    #[test]
    fn seek_updates_offset() {
        let mut table = OpenFileTable::new();

        let fh = table.open(1, OpenFlags::ReadWrite);
        let result = table.seek(fh, 1000);

        assert!(result);

        let entry = table.get(fh).unwrap();
        assert_eq!(entry.offset, 1000);
    }

    #[test]
    fn seek_returns_false_for_unknown_handle() {
        let mut table = OpenFileTable::new();

        let result = table.seek(999, 1000);

        assert!(!result);
    }

    #[test]
    fn mark_dirty_sets_dirty_true() {
        let mut table = OpenFileTable::new();

        let fh = table.open(1, OpenFlags::ReadWrite);
        let result = table.mark_dirty(fh);

        assert!(result);

        let entry = table.get(fh).unwrap();
        assert!(entry.dirty);
    }

    #[test]
    fn mark_clean_sets_dirty_false() {
        let mut table = OpenFileTable::new();

        let fh = table.open(1, OpenFlags::ReadWrite);
        table.mark_dirty(fh);
        let result = table.mark_clean(fh);

        assert!(result);

        let entry = table.get(fh).unwrap();
        assert!(!entry.dirty);
    }

    #[test]
    fn dirty_count_counts_only_dirty_handles() {
        let mut table = OpenFileTable::new();

        let fh1 = table.open(1, OpenFlags::ReadWrite);
        let _fh2 = table.open(2, OpenFlags::ReadWrite);

        table.mark_dirty(fh1);

        assert_eq!(table.dirty_count(), 1);
    }

    #[test]
    fn handles_for_inode_returns_all_handles_for_that_inode() {
        let mut table = OpenFileTable::new();

        let fh1 = table.open(1, OpenFlags::ReadOnly);
        let _ = table.open(2, OpenFlags::ReadOnly);
        let fh3 = table.open(1, OpenFlags::ReadWrite);

        let handles = table.handles_for_inode(1);

        assert_eq!(handles.len(), 2);
        assert!(handles.contains(&fh1));
        assert!(handles.contains(&fh3));
    }

    #[test]
    fn handles_for_inode_returns_empty_for_closed_handles() {
        let mut table = OpenFileTable::new();

        let fh = table.open(1, OpenFlags::ReadOnly);
        table.close(fh);

        let handles = table.handles_for_inode(1);

        assert!(handles.is_empty());
    }

    #[test]
    fn open_flags_from_libc_readonly() {
        assert!(matches!(OpenFlags::from_libc(0), OpenFlags::ReadOnly));
    }

    #[test]
    fn open_flags_from_libc_writeonly() {
        assert!(matches!(OpenFlags::from_libc(1), OpenFlags::WriteOnly));
    }

    #[test]
    fn open_flags_from_libc_readwrite() {
        assert!(matches!(OpenFlags::from_libc(2), OpenFlags::ReadWrite));
    }

    #[test]
    fn is_readable_correct_for_all_variants() {
        assert!(OpenFlags::ReadOnly.is_readable());
        assert!(!OpenFlags::WriteOnly.is_readable());
        assert!(OpenFlags::ReadWrite.is_readable());
    }

    #[test]
    fn is_writable_correct_for_all_variants() {
        assert!(!OpenFlags::ReadOnly.is_writable());
        assert!(OpenFlags::WriteOnly.is_writable());
        assert!(OpenFlags::ReadWrite.is_writable());
    }

    #[test]
    fn get_mut_allows_modification() {
        let mut table = OpenFileTable::new();

        let fh = table.open(1, OpenFlags::ReadWrite);

        if let Some(entry) = table.get_mut(fh) {
            entry.offset = 5000;
        }

        let entry = table.get(fh).unwrap();
        assert_eq!(entry.offset, 5000);
    }

    #[test]
    fn close_unknown_handle_returns_none() {
        let mut table = OpenFileTable::new();

        let result = table.close(999);

        assert!(result.is_none());
    }
}
