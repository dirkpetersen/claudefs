use crate::inode::{InodeEntry, InodeKind};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct FileAttr {
    pub ino: u64,
    pub size: u64,
    pub blocks: u64,
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
    pub kind: FileType,
    pub perm: u16,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
    pub blksize: u32,
    pub flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    RegularFile,
    Directory,
    Symlink,
    BlockDevice,
    CharDevice,
    NamedPipe,
    Socket,
}

impl FileAttr {
    pub fn new_file(ino: u64, size: u64, perm: u16, uid: u32, gid: u32) -> Self {
        let now = SystemTime::now();
        let blocks = blocks_for_size(size);
        FileAttr {
            ino,
            size,
            blocks,
            atime: now,
            mtime: now,
            ctime: now,
            kind: FileType::RegularFile,
            perm,
            nlink: 1,
            uid,
            gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    pub fn new_dir(ino: u64, perm: u16, uid: u32, gid: u32) -> Self {
        let now = SystemTime::now();
        FileAttr {
            ino,
            size: 4096,
            blocks: 1,
            atime: now,
            mtime: now,
            ctime: now,
            kind: FileType::Directory,
            perm,
            nlink: 2,
            uid,
            gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    pub fn new_symlink(ino: u64, target_len: u64, uid: u32, gid: u32) -> Self {
        let now = SystemTime::now();
        let blocks = blocks_for_size(target_len);
        FileAttr {
            ino,
            size: target_len,
            blocks,
            atime: now,
            mtime: now,
            ctime: now,
            kind: FileType::Symlink,
            perm: 0o777,
            nlink: 1,
            uid,
            gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    pub fn from_inode(entry: &InodeEntry) -> Self {
        let kind = inode_kind_to_file_type(&entry.kind);
        let perm = (entry.mode & 0o777) as u16;

        let atime = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(entry.atime_secs as u64)
            + std::time::Duration::from_nanos(entry.atime_nsecs as u64);
        let mtime = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(entry.mtime_secs as u64)
            + std::time::Duration::from_nanos(entry.mtime_nsecs as u64);
        let ctime = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(entry.ctime_secs as u64)
            + std::time::Duration::from_nanos(entry.ctime_nsecs as u64);

        FileAttr {
            ino: entry.ino,
            size: entry.size,
            blocks: blocks_for_size(entry.size),
            atime,
            mtime,
            ctime,
            kind,
            perm,
            nlink: entry.nlink,
            uid: entry.uid,
            gid: entry.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }
}

fn blocks_for_size(size: u64) -> u64 {
    size.div_ceil(512)
}

impl From<&InodeEntry> for FileAttr {
    fn from(entry: &InodeEntry) -> Self {
        FileAttr::from_inode(entry)
    }
}

pub fn inode_kind_to_file_type(kind: &InodeKind) -> FileType {
    match kind {
        InodeKind::File => FileType::RegularFile,
        InodeKind::Directory => FileType::Directory,
        InodeKind::Symlink => FileType::Symlink,
        InodeKind::BlockDevice => FileType::BlockDevice,
        InodeKind::CharDevice => FileType::CharDevice,
        InodeKind::Fifo => FileType::NamedPipe,
        InodeKind::Socket => FileType::Socket,
    }
}

pub fn inode_kind_to_fuser_type(kind: &InodeKind) -> fuser::FileType {
    match kind {
        InodeKind::File => fuser::FileType::RegularFile,
        InodeKind::Directory => fuser::FileType::Directory,
        InodeKind::Symlink => fuser::FileType::Symlink,
        InodeKind::BlockDevice => fuser::FileType::BlockDevice,
        InodeKind::CharDevice => fuser::FileType::CharDevice,
        InodeKind::Fifo => fuser::FileType::NamedPipe,
        InodeKind::Socket => fuser::FileType::Socket,
    }
}

pub fn file_attr_to_fuser(attr: &FileAttr, kind: fuser::FileType) -> fuser::FileAttr {
    fuser::FileAttr {
        ino: attr.ino,
        size: attr.size,
        blocks: attr.blocks,
        atime: attr.atime,
        mtime: attr.mtime,
        ctime: attr.ctime,
        crtime: SystemTime::UNIX_EPOCH,
        kind,
        perm: attr.perm,
        nlink: attr.nlink,
        uid: attr.uid,
        gid: attr.gid,
        rdev: attr.rdev,
        blksize: attr.blksize,
        flags: attr.flags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inode::{InodeEntry, InodeKind};

    #[test]
    fn test_new_file_sets_kind() {
        let attr = FileAttr::new_file(2, 1000, 0o644, 1000, 1000);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.ino, 2);
        assert_eq!(attr.size, 1000);
        assert_eq!(attr.nlink, 1);
    }

    #[test]
    fn test_new_dir_sets_nlink() {
        let attr = FileAttr::new_dir(3, 0o755, 0, 0);
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.nlink, 2);
    }

    #[test]
    fn test_from_inode_for_file() {
        let entry = InodeEntry {
            ino: 5,
            parent: 1,
            name: "test.txt".to_string(),
            kind: InodeKind::File,
            size: 2048,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            mode: 0o644,
            atime_secs: 0,
            atime_nsecs: 0,
            mtime_secs: 0,
            mtime_nsecs: 0,
            ctime_secs: 0,
            ctime_nsecs: 0,
            children: Vec::new(),
            lookup_count: 1,
        };
        let attr = FileAttr::from_inode(&entry);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.size, 2048);
    }

    #[test]
    fn test_from_inode_for_directory() {
        let entry = InodeEntry {
            ino: 1,
            parent: 1,
            name: "/".to_string(),
            kind: InodeKind::Directory,
            size: 4096,
            nlink: 3,
            uid: 0,
            gid: 0,
            mode: 0o755,
            atime_secs: 0,
            atime_nsecs: 0,
            mtime_secs: 0,
            mtime_nsecs: 0,
            ctime_secs: 0,
            ctime_nsecs: 0,
            children: vec![2, 3],
            lookup_count: 1,
        };
        let attr = FileAttr::from_inode(&entry);
        assert_eq!(attr.kind, FileType::Directory);
    }

    #[test]
    fn test_inode_kind_to_file_type_all_variants() {
        assert_eq!(
            inode_kind_to_file_type(&InodeKind::File),
            FileType::RegularFile
        );
        assert_eq!(
            inode_kind_to_file_type(&InodeKind::Directory),
            FileType::Directory
        );
        assert_eq!(
            inode_kind_to_file_type(&InodeKind::Symlink),
            FileType::Symlink
        );
        assert_eq!(
            inode_kind_to_file_type(&InodeKind::BlockDevice),
            FileType::BlockDevice
        );
        assert_eq!(
            inode_kind_to_file_type(&InodeKind::CharDevice),
            FileType::CharDevice
        );
        assert_eq!(
            inode_kind_to_file_type(&InodeKind::Fifo),
            FileType::NamedPipe
        );
        assert_eq!(
            inode_kind_to_file_type(&InodeKind::Socket),
            FileType::Socket
        );
    }
}
