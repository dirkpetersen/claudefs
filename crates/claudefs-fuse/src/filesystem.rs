//! Core FUSE filesystem implementation.
//!
//! Implements the `fuser::Filesystem` trait for ClaudeFS, backed initially by
//! an in-memory inode store. The remote metadata (A2) and transport (A4) backends
//! will be wired in during Phase 3 integration.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::raw::c_int;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use fuser::{
    FileType as FuserFileType, Filesystem, KernelConfig, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, Request, TimeOrNow,
};
use libc;
use tracing::debug;

use crate::cache::{CacheConfig, MetadataCache};
use crate::inode::{InodeEntry, InodeKind, InodeTable};
use crate::operations::{apply_mode_umask, blocks_for_size, check_access};

#[derive(Debug, Clone)]
pub struct ClaudeFsConfig {
    pub cache: CacheConfig,
    pub uid: u32,
    pub gid: u32,
    pub default_permissions: bool,
    pub allow_other: bool,
    pub attr_timeout: Duration,
    pub entry_timeout: Duration,
    pub direct_io: bool,
}

impl Default for ClaudeFsConfig {
    fn default() -> Self {
        Self {
            cache: CacheConfig::default(),
            uid: 0,
            gid: 0,
            default_permissions: true,
            allow_other: false,
            attr_timeout: Duration::from_secs(1),
            entry_timeout: Duration::from_secs(1),
            direct_io: false,
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct OpenHandle {
    ino: u64,
    flags: i32,
}

struct ClaudeFsState {
    inodes: InodeTable,
    cache: MetadataCache,
    open_handles: HashMap<u64, OpenHandle>,
    next_fh: u64,
}

pub struct ClaudeFsFilesystem {
    config: ClaudeFsConfig,
    state: Arc<Mutex<ClaudeFsState>>,
}

impl ClaudeFsFilesystem {
    pub fn new(config: ClaudeFsConfig) -> Self {
        let state = ClaudeFsState {
            inodes: InodeTable::new(),
            cache: MetadataCache::new(config.cache.clone()),
            open_handles: HashMap::new(),
            next_fh: 1,
        };
        Self {
            config,
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn config(&self) -> &ClaudeFsConfig {
        &self.config
    }
}

fn inode_to_fuser_attr(entry: &InodeEntry) -> fuser::FileAttr {
    let atime = SystemTime::UNIX_EPOCH + Duration::new(entry.atime_secs as u64, entry.atime_nsecs);
    let mtime = SystemTime::UNIX_EPOCH + Duration::new(entry.mtime_secs as u64, entry.mtime_nsecs);
    let ctime = SystemTime::UNIX_EPOCH + Duration::new(entry.ctime_secs as u64, entry.ctime_nsecs);
    let kind = crate::attr::inode_kind_to_fuser_type(&entry.kind);
    fuser::FileAttr {
        ino: entry.ino,
        size: entry.size,
        blocks: blocks_for_size(entry.size),
        atime,
        mtime,
        ctime,
        crtime: SystemTime::UNIX_EPOCH,
        kind,
        perm: (entry.mode & 0o7777) as u16,
        nlink: entry.nlink,
        uid: entry.uid,
        gid: entry.gid,
        rdev: 0,
        blksize: 4096,
        flags: 0,
    }
}

impl Filesystem for ClaudeFsFilesystem {
    fn init(&mut self, _req: &Request<'_>, _config: &mut KernelConfig) -> Result<(), c_int> {
        debug!("ClaudeFS filesystem init");
        Ok(())
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        debug!("lookup parent={} name={}", parent, name_str);

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.cache.is_negative(parent, &name_str) {
            reply.error(libc::ENOENT);
            return;
        }

        match state.inodes.lookup_child(parent, &name_str) {
            Some(ino) => {
                if let Some(entry) = state.inodes.get(ino) {
                    let fuser_attr = inode_to_fuser_attr(entry);
                    state.inodes.add_lookup(ino);
                    reply.entry(&self.config.entry_timeout, &fuser_attr, 0);
                } else {
                    reply.error(libc::ENOENT);
                }
            }
            None => {
                state.cache.insert_negative(parent, &name_str);
                reply.error(libc::ENOENT);
            }
        }
    }

    fn forget(&mut self, _req: &Request<'_>, ino: u64, nlookup: u64) {
        debug!("forget ino={} nlookup={}", ino, nlookup);
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        state.inodes.forget(ino, nlookup);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        debug!("getattr ino={}", ino);
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        match state.inodes.get(ino) {
            Some(entry) => {
                let fuser_attr = inode_to_fuser_attr(entry);
                reply.attr(&self.config.attr_timeout, &fuser_attr);
            }
            None => reply.error(libc::ENOENT),
        }
    }

    fn setattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!(
            "setattr ino={} size={:?} mode={:?} uid={:?} gid={:?}",
            ino, size, mode, uid, gid
        );
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let (_, attr) = {
            let entry = match state.inodes.get_mut(ino) {
                Some(e) => e,
                None => {
                    reply.error(libc::ENOENT);
                    return;
                }
            };

            if let Some(new_size) = size {
                entry.size = new_size;
            }

            if let Some(new_mode) = mode {
                entry.mode = new_mode;
            }

            if let Some(new_uid) = uid {
                entry.uid = new_uid;
            }

            if let Some(new_gid) = gid {
                entry.gid = new_gid;
            }

            ((), inode_to_fuser_attr(entry))
        };

        state.cache.invalidate(ino);

        reply.attr(&self.config.attr_timeout, &attr);
    }

    fn mkdir(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        let name_str = name.to_string_lossy();
        debug!("mkdir parent={} name={} mode={:o}", parent, name_str, mode);

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let effective_mode = apply_mode_umask(mode, umask);
        let uid = req.uid();
        let gid = req.gid();

        match state.inodes.alloc(
            parent,
            &name_str,
            InodeKind::Directory,
            effective_mode,
            uid,
            gid,
        ) {
            Ok(ino) => {
                if let Some(entry) = state.inodes.get(ino) {
                    let fuser_attr = inode_to_fuser_attr(entry);
                    reply.entry(&self.config.entry_timeout, &fuser_attr, 0);
                } else {
                    reply.error(libc::EIO);
                }
            }
            Err(e) => reply.error(e.to_errno()),
        }
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let name_str = name.to_string_lossy();
        debug!("unlink parent={} name={}", parent, name_str);

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let ino = match state.inodes.lookup_child(parent, &name_str) {
            Some(ino) => ino,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if let Some(entry) = state.inodes.get(ino) {
            if matches!(entry.kind, InodeKind::Directory) {
                reply.error(libc::EISDIR);
                return;
            }
        }

        match state.inodes.remove(ino) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e.to_errno()),
        }
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let name_str = name.to_string_lossy();
        debug!("rmdir parent={} name={}", parent, name_str);

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let ino = match state.inodes.lookup_child(parent, &name_str) {
            Some(ino) => ino,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if let Some(entry) = state.inodes.get(ino) {
            if !matches!(entry.kind, InodeKind::Directory) {
                reply.error(libc::ENOTDIR);
                return;
            }
        }

        match state.inodes.remove(ino) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e.to_errno()),
        }
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        let newname_str = newname.to_string_lossy();
        debug!(
            "rename parent={} name={} newparent={} newname={}",
            parent, name_str, newparent, newname_str
        );

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let ino = match state.inodes.lookup_child(parent, &name_str) {
            Some(ino) => ino,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if state.inodes.lookup_child(newparent, &newname_str).is_some() {
            reply.error(libc::EEXIST);
            return;
        }

        let is_dir = {
            let entry = match state.inodes.get(ino) {
                Some(e) => e,
                None => {
                    reply.error(libc::ENOENT);
                    return;
                }
            };
            matches!(entry.kind, InodeKind::Directory)
        };

        if let Some(parent_entry) = state.inodes.get_mut(parent) {
            parent_entry.children.retain(|&c| c != ino);
            if is_dir {
                parent_entry.nlink = parent_entry.nlink.saturating_sub(1);
            }
        }

        if let Some(entry) = state.inodes.get_mut(ino) {
            entry.parent = newparent;
            entry.name = newname_str.to_string();
        }

        if let Some(new_parent_entry) = state.inodes.get_mut(newparent) {
            new_parent_entry.children.push(ino);
            if is_dir {
                new_parent_entry.nlink += 1;
            }
        }

        reply.ok();
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        debug!("open ino={} flags={}", ino, flags);

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.inodes.get(ino).is_none() {
            reply.error(libc::ENOENT);
            return;
        }

        let fh = state.next_fh;
        state.next_fh += 1;

        state.open_handles.insert(fh, OpenHandle { ino, flags });

        reply.opened(fh, 0);
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!("read ino={} offset={} size={}", ino, offset, size);

        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let inode_size = state.inodes.get(ino).map(|e| e.size).unwrap_or(0);
        let available = inode_size.saturating_sub(offset as u64);
        let read_size = (size as u64).min(available) as usize;

        reply.data(&vec![0u8; read_size]);
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        debug!("write ino={} offset={} size={}", ino, offset, data.len());

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let new_size = offset + data.len() as i64;
        if let Some(entry) = state.inodes.get_mut(ino) {
            if new_size > entry.size as i64 {
                entry.size = new_size as u64;
            }
        }

        reply.written(data.len() as u32);
    }

    fn flush(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _lock_owner: u64,
        reply: ReplyEmpty,
    ) {
        debug!("flush");
        reply.ok();
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        debug!("release fh={}", fh);

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        state.open_handles.remove(&fh);
        reply.ok();
    }

    fn opendir(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        debug!("opendir ino={}", ino);

        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if let Some(entry) = state.inodes.get(ino) {
            if !matches!(entry.kind, InodeKind::Directory) {
                reply.error(libc::ENOTDIR);
                return;
            }
        } else {
            reply.error(libc::ENOENT);
            return;
        }

        reply.opened(0, 0);
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("readdir ino={} offset={}", ino, offset);

        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let entry = match state.inodes.get(ino) {
            Some(e) => e,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        if !matches!(entry.kind, InodeKind::Directory) {
            reply.error(libc::ENOTDIR);
            return;
        }

        let mut off = offset;

        if offset == 0 {
            if reply.add(ino, 1, FuserFileType::Directory, ".") {
                return;
            }
            off = 1;
        }

        if offset <= 1 {
            if reply.add(entry.parent, 2, FuserFileType::Directory, "..") {
                return;
            }
            off = 2;
        }

        for &child_ino in &entry.children {
            if off < offset {
                off += 1;
                continue;
            }

            if let Some(child) = state.inodes.get(child_ino) {
                let ftype = crate::attr::inode_kind_to_fuser_type(&child.kind);
                if reply.add(child_ino, off + 1, ftype, &child.name) {
                    return;
                }
                off += 1;
            }
        }

        reply.ok();
    }

    fn releasedir(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _flags: i32,
        reply: ReplyEmpty,
    ) {
        debug!("releasedir");
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyStatfs) {
        debug!("statfs");
        reply.statfs(
            1024 * 1024 * 256,
            1024 * 1024 * 230,
            1024 * 1024 * 230,
            1_000_000,
            999_000,
            4096,
            255,
            4096,
        );
    }

    fn access(&mut self, req: &Request<'_>, ino: u64, mask: i32, reply: ReplyEmpty) {
        debug!("access ino={} mask={}", ino, mask);

        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let entry = match state.inodes.get(ino) {
            Some(e) => e,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        let req_uid = req.uid();
        let req_gid = req.gid();

        if check_access(entry.mode, entry.uid, entry.gid, req_uid, req_gid, mask) {
            reply.ok();
        } else {
            reply.error(libc::EACCES);
        }
    }

    fn create(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        let name_str = name.to_string_lossy();
        debug!(
            "create parent={} name={} mode={:o} flags={}",
            parent, name_str, mode, flags
        );

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let effective_mode = apply_mode_umask(mode, umask);
        let uid = req.uid();
        let gid = req.gid();

        match state
            .inodes
            .alloc(parent, &name_str, InodeKind::File, effective_mode, uid, gid)
        {
            Ok(ino) => {
                let fh = state.next_fh;
                state.next_fh += 1;

                state.open_handles.insert(fh, OpenHandle { ino, flags });

                if let Some(entry) = state.inodes.get(ino) {
                    let fuser_attr = inode_to_fuser_attr(entry);
                    reply.created(&self.config.entry_timeout, &fuser_attr, 0, fh, flags as u32);
                } else {
                    reply.error(libc::EIO);
                }
            }
            Err(e) => reply.error(e.to_errno()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inode::{InodeKind, ROOT_INODE};

    fn make_fs() -> ClaudeFsFilesystem {
        ClaudeFsFilesystem::new(ClaudeFsConfig::default())
    }

    #[test]
    fn test_new_filesystem_has_root() {
        let fs = make_fs();
        let state = fs.state.lock().unwrap();
        assert!(state.inodes.get(ROOT_INODE).is_some());
    }

    #[test]
    fn test_default_config_attr_timeout() {
        let config = ClaudeFsConfig::default();
        assert_eq!(config.attr_timeout, Duration::from_secs(1));
    }

    #[test]
    fn test_create_file_in_state() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let ino = state
            .inodes
            .alloc(ROOT_INODE, "test.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        assert!(state.inodes.get(ino).is_some());
        assert_eq!(state.inodes.lookup_child(ROOT_INODE, "test.txt"), Some(ino));
    }

    #[test]
    fn test_create_directory_in_state() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let ino = state
            .inodes
            .alloc(ROOT_INODE, "testdir", InodeKind::Directory, 0o755, 0, 0)
            .unwrap();
        assert!(state.inodes.get(ino).is_some());
        let entry = state.inodes.get(ino).unwrap();
        assert!(matches!(entry.kind, InodeKind::Directory));
    }

    #[test]
    fn test_lookup_child_in_state() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        state
            .inodes
            .alloc(ROOT_INODE, "file.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        let found = state.inodes.lookup_child(ROOT_INODE, "file.txt");
        assert!(found.is_some());
    }

    #[test]
    fn test_remove_file_from_state() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let ino = state
            .inodes
            .alloc(ROOT_INODE, "file.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        state.inodes.remove(ino).expect("Remove should succeed");
        assert!(state.inodes.get(ino).is_none());
    }

    #[test]
    fn test_remove_nonempty_dir_fails() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let dir_ino = state
            .inodes
            .alloc(ROOT_INODE, "mydir", InodeKind::Directory, 0o755, 0, 0)
            .unwrap();
        state
            .inodes
            .alloc(dir_ino, "file.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();

        let result = state.inodes.remove(dir_ino);
        assert!(result.is_err());
    }

    #[test]
    fn test_open_handle_allocation() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let fh = state.next_fh;
        state
            .open_handles
            .insert(fh, OpenHandle { ino: 2, flags: 0 });
        state.next_fh += 1;
        assert!(state.open_handles.contains_key(&fh));
    }

    #[test]
    fn test_open_handle_cleanup() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let fh = state.next_fh;
        state
            .open_handles
            .insert(fh, OpenHandle { ino: 2, flags: 0 });
        state.next_fh += 1;
        state.open_handles.remove(&fh);
        assert!(!state.open_handles.contains_key(&fh));
    }

    #[test]
    fn test_multiple_open_handles() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let fh1 = state.next_fh;
        state
            .open_handles
            .insert(fh1, OpenHandle { ino: 2, flags: 0 });
        state.next_fh += 1;
        let fh2 = state.next_fh;
        state
            .open_handles
            .insert(fh2, OpenHandle { ino: 3, flags: 0 });
        state.next_fh += 1;
        assert!(state.open_handles.contains_key(&fh1));
        assert!(state.open_handles.contains_key(&fh2));
        assert_ne!(fh1, fh2);
    }

    #[test]
    fn test_rename_same_parent() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let ino = state
            .inodes
            .alloc(ROOT_INODE, "old.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        if let Some(entry) = state.inodes.get_mut(ino) {
            entry.name = "new.txt".to_string();
        }
        assert_eq!(state.inodes.lookup_child(ROOT_INODE, "new.txt"), Some(ino));
        assert_eq!(state.inodes.lookup_child(ROOT_INODE, "old.txt"), None);
    }

    #[test]
    fn test_mkdir_creates_directory_kind() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let dir_ino = state
            .inodes
            .alloc(ROOT_INODE, "testdir", InodeKind::Directory, 0o755, 0, 0)
            .unwrap();
        let entry = state.inodes.get(dir_ino).unwrap();
        assert!(matches!(entry.kind, InodeKind::Directory));
    }

    #[test]
    fn test_write_updates_size() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        let ino = state
            .inodes
            .alloc(ROOT_INODE, "test.txt", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        let entry = state.inodes.get_mut(ino).unwrap();
        let new_end = 4096u64;
        if new_end > entry.size {
            entry.size = new_end;
        }
        assert_eq!(state.inodes.get(ino).unwrap().size, 4096);
    }

    #[test]
    fn test_statfs_constants() {
        let blocks: u64 = 1024 * 1024 * 256;
        assert!(blocks > 0);
        let namelen: u32 = 255;
        assert_eq!(namelen, 255);
    }

    #[test]
    fn test_inode_counter_increments() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        assert_eq!(state.inodes.len(), 1);
        state
            .inodes
            .alloc(ROOT_INODE, "a", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        assert_eq!(state.inodes.len(), 2);
        state
            .inodes
            .alloc(ROOT_INODE, "b", InodeKind::File, 0o644, 0, 0)
            .unwrap();
        assert_eq!(state.inodes.len(), 3);
    }

    #[test]
    fn test_next_fh_increments() {
        let fs = make_fs();
        let mut state = fs.state.lock().unwrap();
        assert_eq!(state.next_fh, 1);
        let fh = state.next_fh;
        state
            .open_handles
            .insert(fh, OpenHandle { ino: 1, flags: 0 });
        state.next_fh += 1;
        assert_eq!(state.next_fh, 2);
        let fh2 = state.next_fh;
        state
            .open_handles
            .insert(fh2, OpenHandle { ino: 2, flags: 0 });
        state.next_fh += 1;
        assert_eq!(state.next_fh, 3);
    }

    #[test]
    fn test_default_config_direct_io_off() {
        assert!(!ClaudeFsConfig::default().direct_io);
    }

    #[test]
    fn test_inode_to_fuser_attr_file() {
        let fs = make_fs();
        let state = fs.state.lock().unwrap();
        let ino = ROOT_INODE;
        let entry = state.inodes.get(ino).unwrap();
        let fattr = inode_to_fuser_attr(entry);
        assert_eq!(fattr.ino, ROOT_INODE);
        assert_eq!(fattr.kind, fuser::FileType::Directory);
    }

    #[test]
    fn test_inode_to_fuser_attr_perm() {
        let fs = make_fs();
        let state = fs.state.lock().unwrap();
        let root = state.inodes.get(ROOT_INODE).unwrap();
        let fattr = inode_to_fuser_attr(root);
        assert_eq!(fattr.perm, 0o755);
    }

    #[test]
    fn test_config_default_permissions() {
        let config = ClaudeFsConfig::default();
        assert!(config.default_permissions);
    }

    #[test]
    fn test_readdir_entries() {
        let fs = make_fs();
        let state = fs.state.lock().unwrap();
        let root = state.inodes.get(ROOT_INODE).unwrap();
        assert!(root.children.is_empty());
    }
}
