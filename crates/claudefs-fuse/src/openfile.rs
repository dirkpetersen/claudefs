use crate::inode::InodeId;
use std::collections::HashMap;

pub type FileHandle = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum OpenFlags {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

impl OpenFlags {
    pub fn is_readable(&self) -> bool {
        match self {
            OpenFlags::ReadOnly | OpenFlags::ReadWrite => true,
            OpenFlags::WriteOnly => false,
        }
    }

    pub fn is_writable(&self) -> bool {
        match self {
            OpenFlags::WriteOnly | OpenFlags::ReadWrite => true,
            OpenFlags::ReadOnly => false,
        }
    }

    pub fn from_libc(flags: i32) -> Self {
        match flags & 0o3 {
            0 => OpenFlags::ReadOnly,
            1 => OpenFlags::WriteOnly,
            2 => OpenFlags::ReadWrite,
            _ => OpenFlags::ReadOnly,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenFileEntry {
    pub fh: FileHandle,
    pub ino: InodeId,
    pub flags: OpenFlags,
    pub offset: u64,
    pub dirty: bool,
}

pub struct OpenFileTable {
    next_fh: FileHandle,
    entries: HashMap<FileHandle, OpenFileEntry>,
}

impl OpenFileTable {
    pub fn new() -> Self {
        tracing::debug!("Creating new open file table");
        Self {
            next_fh: 1,
            entries: HashMap::new(),
        }
    }

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

    pub fn get(&self, fh: FileHandle) -> Option<&OpenFileEntry> {
        self.entries.get(&fh)
    }

    pub fn get_mut(&mut self, fh: FileHandle) -> Option<&mut OpenFileEntry> {
        self.entries.get_mut(&fh)
    }

    pub fn close(&mut self, fh: FileHandle) -> Option<OpenFileEntry> {
        let entry = self.entries.remove(&fh);

        if entry.is_some() {
            tracing::debug!("Closed file handle: fh={}", fh);
        }

        entry
    }

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

    pub fn handles_for_inode(&self, ino: InodeId) -> Vec<FileHandle> {
        self.entries
            .values()
            .filter(|e| e.ino == ino)
            .map(|e| e.fh)
            .collect()
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

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
