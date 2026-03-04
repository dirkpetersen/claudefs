//! FUSE operation types and helper functions.
//!
//! This module defines the data structures used to represent FUSE operations
//! and provides utility functions for permission checking and mode handling.

use libc::c_int;
use std::time::SystemTime;

/// Kinds of FUSE operations supported by the filesystem.
pub enum FuseOpKind {
    /// Look up a directory entry by name.
    Lookup,
    /// Get file attributes (metadata).
    GetAttr,
    /// Set file attributes (metadata).
    SetAttr,
    /// Create a directory.
    MkDir,
    /// Remove a directory.
    RmDir,
    /// Create and open a file.
    Create,
    /// Remove a file (unlink).
    Unlink,
    /// Read data from a file.
    Read,
    /// Write data to a file.
    Write,
    /// Read directory entries.
    ReadDir,
    /// Open a file.
    Open,
    /// Release (close) a file.
    Release,
    /// Open a directory.
    OpenDir,
    /// Release (close) a directory.
    ReleaseDir,
    /// Rename a file or directory.
    Rename,
    /// Flush cached data for a file.
    Flush,
    /// Synchronize file data and metadata.
    Fsync,
    /// Get filesystem statistics.
    StatFs,
    /// Check access permissions.
    Access,
    /// Create a hard link.
    Link,
    /// Create a symbolic link.
    Symlink,
    /// Read the target of a symbolic link.
    ReadLink,
    /// Set an extended attribute.
    SetXAttr,
    /// Get an extended attribute.
    GetXAttr,
    /// List extended attributes.
    ListXAttr,
    /// Remove an extended attribute.
    RemoveXAttr,
}

/// Request to set file attributes.
pub struct SetAttrRequest {
    /// Inode number of the file.
    pub ino: u64,
    /// New permission mode bits.
    pub mode: Option<u32>,
    /// New owner user ID.
    pub uid: Option<u32>,
    /// New owner group ID.
    pub gid: Option<u32>,
    /// New file size.
    pub size: Option<u64>,
    /// New access time.
    pub atime: Option<SystemTime>,
    /// New modification time.
    pub mtime: Option<SystemTime>,
    /// File handle for the operation.
    pub fh: Option<u64>,
    /// Operation flags.
    pub flags: Option<u32>,
}

/// Reply for filesystem statistics (statfs).
pub struct StatfsReply {
    /// Total blocks in the filesystem.
    pub blocks: u64,
    /// Free blocks.
    pub bfree: u64,
    /// Free blocks available to non-root users.
    pub bavail: u64,
    /// Total file nodes (inodes).
    pub files: u64,
    /// Free file nodes.
    pub ffree: u64,
    /// Block size.
    pub bsize: u32,
    /// Maximum filename length.
    pub namelen: u32,
    /// Fundamental block size.
    pub frsize: u32,
}

/// Request to create and open a file.
pub struct CreateRequest {
    /// Parent directory inode.
    pub parent: u64,
    /// Name of the new file.
    pub name: String,
    /// Permission mode bits.
    pub mode: u32,
    /// Umask to apply to mode.
    pub umask: u32,
    /// Open flags (e.g., O_RDWR).
    pub flags: i32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
}

/// Request to create a directory.
pub struct MkdirRequest {
    /// Parent directory inode.
    pub parent: u64,
    /// Name of the new directory.
    pub name: String,
    /// Permission mode bits.
    pub mode: u32,
    /// Umask to apply to mode.
    pub umask: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
}

/// Request to rename a file or directory.
pub struct RenameRequest {
    /// Source parent directory inode.
    pub parent: u64,
    /// Source name.
    pub name: String,
    /// Destination parent directory inode.
    pub newparent: u64,
    /// Destination name.
    pub newname: String,
    /// Rename flags (e.g., RENAME_EXCHANGE).
    pub flags: u32,
}

/// A directory entry returned by ReadDir.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Inode number of the entry.
    pub ino: u64,
    /// Offset for pagination.
    pub offset: i64,
    /// File type (regular, directory, symlink, etc.).
    pub kind: fuser::FileType,
    /// Entry name.
    pub name: String,
}

/// Applies umask to file mode bits.
///
/// Strips permissions from the given mode using the process umask.
/// Preserves file type bits (e.g., 0o040000 for directories).
pub fn apply_mode_umask(mode: u32, umask: u32) -> u32 {
    let file_type_bits = mode & 0o170000;
    let perm_bits = mode & 0o777;
    let effective_perm = perm_bits & !umask;
    file_type_bits | effective_perm
}

/// Checks if a user can access a file with given permissions.
///
/// Implements POSIX permission checking for owner, group, and others.
/// Root (uid=0) bypasses most checks but still requires execute bits on directories.
pub fn check_access(
    mode: u32,
    uid: u32,
    gid: u32,
    req_uid: u32,
    req_gid: u32,
    access_mask: c_int,
) -> bool {
    if req_uid == 0 {
        if access_mask & libc::X_OK as c_int != 0 {
            (mode & 0o111) != 0
        } else {
            true
        }
    } else if req_uid == uid {
        let shift = 6;
        ((mode >> shift) & 0o7 & access_mask as u32) != 0
    } else if req_gid == gid {
        let shift = 3;
        ((mode >> shift) & 0o7 & access_mask as u32) != 0
    } else {
        (mode & 0o7 & access_mask as u32) != 0
    }
}

/// Extracts file type from mode bits.
///
/// Maps the file type bits (S_IFMT) from a mode value to the FUSE file type enum.
pub fn mode_to_fuser_type(mode: u32) -> fuser::FileType {
    match mode & 0o170000 {
        0o100000 => fuser::FileType::RegularFile,
        0o040000 => fuser::FileType::Directory,
        0o120000 => fuser::FileType::Symlink,
        0o060000 => fuser::FileType::BlockDevice,
        0o020000 => fuser::FileType::CharDevice,
        0o010000 => fuser::FileType::NamedPipe,
        0o140000 => fuser::FileType::Socket,
        _ => fuser::FileType::RegularFile,
    }
}

/// Calculates the number of 512-byte blocks for a given size.
///
/// Rounds up to the nearest block boundary, following POSIX convention.
pub fn blocks_for_size(size: u64) -> u64 {
    size.div_ceil(512)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_mode_umask() {
        assert_eq!(apply_mode_umask(0o777, 0o022), 0o755);
        assert_eq!(apply_mode_umask(0o666, 0o077), 0o600);
        assert_eq!(apply_mode_umask(0o755, 0o000), 0o755);
    }

    #[test]
    fn test_check_access_owner_read() {
        let mode = 0o644;
        assert!(check_access(
            mode,
            1000,
            1000,
            1000,
            1000,
            libc::R_OK as c_int
        ));
    }

    #[test]
    fn test_check_access_owner_write() {
        let mode = 0o644;
        assert!(check_access(
            mode,
            1000,
            1000,
            1000,
            1000,
            libc::W_OK as c_int
        ));
    }

    #[test]
    fn test_check_access_owner_execute() {
        // 0o755: rwxr-xr-x — owner has execute
        let mode = 0o755;
        assert!(check_access(
            mode,
            1000,
            1000,
            1000,
            1000,
            libc::X_OK as c_int
        ));
        // 0o644: rw-r--r-- — nobody has execute
        let mode_no_exec = 0o644;
        assert!(!check_access(
            mode_no_exec,
            1000,
            1000,
            1000,
            1000,
            libc::X_OK as c_int
        ));
    }

    #[test]
    fn test_check_access_group() {
        // 0o640: rw- r-- --- — group has read only
        let mode = 0o640;
        assert!(check_access(
            mode,
            1000,
            1000,
            999,
            1000,
            libc::R_OK as c_int
        ));
        assert!(!check_access(
            mode,
            1000,
            1000,
            999,
            1000,
            libc::W_OK as c_int
        ));
        assert!(!check_access(
            mode,
            1000,
            1000,
            999,
            1000,
            libc::X_OK as c_int
        ));
        // 0o660: rw- rw- --- — group has read+write
        let mode_rw = 0o660;
        assert!(check_access(
            mode_rw,
            1000,
            1000,
            999,
            1000,
            libc::R_OK as c_int
        ));
        assert!(check_access(
            mode_rw,
            1000,
            1000,
            999,
            1000,
            libc::W_OK as c_int
        ));
    }

    #[test]
    fn test_check_access_other() {
        let mode = 0o644;
        assert!(check_access(
            mode,
            1000,
            1000,
            999,
            999,
            libc::R_OK as c_int
        ));
        assert!(!check_access(
            mode,
            1000,
            1000,
            999,
            999,
            libc::W_OK as c_int
        ));
        assert!(!check_access(
            mode,
            1000,
            1000,
            999,
            999,
            libc::X_OK as c_int
        ));
    }

    #[test]
    fn test_check_access_root_always_passes_read_write() {
        let mode = 0o000;
        assert!(check_access(mode, 0, 0, 0, 0, libc::R_OK as c_int));
        assert!(check_access(mode, 0, 0, 0, 0, libc::W_OK as c_int));
    }

    #[test]
    fn test_check_access_root_needs_execute_bit() {
        let mode = 0o000;
        assert!(!check_access(mode, 0, 0, 0, 0, libc::X_OK as c_int));

        let mode_with_x = 0o100;
        assert!(check_access(mode_with_x, 0, 0, 0, 0, libc::X_OK as c_int));
    }

    #[test]
    fn test_mode_to_fuser_type_regular_file() {
        let mode = 0o100644;
        assert_eq!(mode_to_fuser_type(mode), fuser::FileType::RegularFile);
    }

    #[test]
    fn test_mode_to_fuser_type_directory() {
        let mode = 0o040755;
        assert_eq!(mode_to_fuser_type(mode), fuser::FileType::Directory);
    }

    #[test]
    fn test_mode_to_fuser_type_symlink() {
        let mode = 0o120777;
        assert_eq!(mode_to_fuser_type(mode), fuser::FileType::Symlink);
    }

    #[test]
    fn test_mode_to_fuser_type_fifo() {
        let mode = 0o010644;
        assert_eq!(mode_to_fuser_type(mode), fuser::FileType::NamedPipe);
    }

    #[test]
    fn test_mode_to_fuser_type_socket() {
        let mode = 0o140777;
        assert_eq!(mode_to_fuser_type(mode), fuser::FileType::Socket);
    }

    #[test]
    fn test_blocks_for_size_zero() {
        assert_eq!(blocks_for_size(0), 0);
    }

    #[test]
    fn test_blocks_for_size_one() {
        assert_eq!(blocks_for_size(1), 1);
    }

    #[test]
    fn test_blocks_for_size_512() {
        assert_eq!(blocks_for_size(512), 1);
    }

    #[test]
    fn test_blocks_for_size_513() {
        assert_eq!(blocks_for_size(513), 2);
    }

    #[test]
    fn test_blocks_for_size_4096() {
        assert_eq!(blocks_for_size(4096), 8);
    }

    #[test]
    fn test_dir_entry_creation() {
        let entry = DirEntry {
            ino: 2,
            offset: 0,
            kind: fuser::FileType::RegularFile,
            name: "test.txt".to_string(),
        };
        assert_eq!(entry.ino, 2);
        assert_eq!(entry.name, "test.txt");
    }
}
