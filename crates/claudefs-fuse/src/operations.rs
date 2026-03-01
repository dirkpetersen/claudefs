use libc::c_int;
use std::time::SystemTime;

pub enum FuseOpKind {
    Lookup,
    GetAttr,
    SetAttr,
    MkDir,
    RmDir,
    Create,
    Unlink,
    Read,
    Write,
    ReadDir,
    Open,
    Release,
    OpenDir,
    ReleaseDir,
    Rename,
    Flush,
    Fsync,
    StatFs,
    Access,
    Link,
    Symlink,
    ReadLink,
    SetXAttr,
    GetXAttr,
    ListXAttr,
    RemoveXAttr,
}

pub struct SetAttrRequest {
    pub ino: u64,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub size: Option<u64>,
    pub atime: Option<SystemTime>,
    pub mtime: Option<SystemTime>,
    pub fh: Option<u64>,
    pub flags: Option<u32>,
}

pub struct StatfsReply {
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
    pub bsize: u32,
    pub namelen: u32,
    pub frsize: u32,
}

pub struct CreateRequest {
    pub parent: u64,
    pub name: String,
    pub mode: u32,
    pub umask: u32,
    pub flags: i32,
    pub uid: u32,
    pub gid: u32,
}

pub struct MkdirRequest {
    pub parent: u64,
    pub name: String,
    pub mode: u32,
    pub umask: u32,
    pub uid: u32,
    pub gid: u32,
}

pub struct RenameRequest {
    pub parent: u64,
    pub name: String,
    pub newparent: u64,
    pub newname: String,
    pub flags: u32,
}

pub struct DirEntry {
    pub ino: u64,
    pub offset: i64,
    pub kind: fuser::FileType,
    pub name: String,
}

pub fn apply_mode_umask(mode: u32, umask: u32) -> u32 {
    let file_type_bits = mode & 0o170000;
    let perm_bits = mode & 0o777;
    let effective_perm = perm_bits & !umask;
    file_type_bits | effective_perm
}

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
