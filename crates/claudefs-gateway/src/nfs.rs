//! NFSv3 procedure handlers

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::error::{GatewayError, Result};
use crate::protocol::{
    Entry3, Fattr3, FileHandle3, FsInfoResult, FsStatResult, Ftype3, LookupResult, Nfstime3,
    PathConfResult, ReadDirResult,
};

/// Virtual filesystem backend trait for NFS operations.
/// Provides an abstraction layer for different filesystem implementations.
pub trait VfsBackend: Send + Sync {
    fn getattr(&self, fh: &FileHandle3) -> Result<Fattr3>;
    fn lookup(&self, dir_fh: &FileHandle3, name: &str) -> Result<LookupResult>;
    fn read(&self, fh: &FileHandle3, offset: u64, count: u32) -> Result<(Vec<u8>, bool)>;
    fn write(&self, fh: &FileHandle3, offset: u64, data: &[u8]) -> Result<u32>;
    fn readdir(&self, dir_fh: &FileHandle3, cookie: u64, count: u32) -> Result<ReadDirResult>;
    fn mkdir(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)>;
    fn create(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)>;
    fn remove(&self, dir_fh: &FileHandle3, name: &str) -> Result<()>;
    fn rename(
        &self,
        from_dir: &FileHandle3,
        from_name: &str,
        to_dir: &FileHandle3,
        to_name: &str,
    ) -> Result<()>;
    fn readlink(&self, fh: &FileHandle3) -> Result<String>;
    fn symlink(
        &self,
        dir_fh: &FileHandle3,
        name: &str,
        target: &str,
    ) -> Result<(FileHandle3, Fattr3)>;
    fn fsstat(&self, fh: &FileHandle3) -> Result<FsStatResult>;
    fn fsinfo(&self, fh: &FileHandle3) -> Result<FsInfoResult>;
    fn pathconf(&self, fh: &FileHandle3) -> Result<PathConfResult>;
    fn access(&self, fh: &FileHandle3, uid: u32, gid: u32, access_bits: u32) -> Result<u32>;
}

/// In-memory inode entry representing a file or directory in the virtual filesystem.
#[derive(Debug, Clone)]
pub struct InodeEntry {
    /// File type (regular file, directory, symbolic link, etc.)
    pub ftype: Ftype3,
    /// Unix permission bits (mode)
    pub mode: u32,
    /// File size in bytes
    pub size: u64,
    /// File data content (for regular files)
    pub data: Vec<u8>,
    /// Child entries (for directories) mapping name to inode number
    pub children: HashMap<String, u64>,
    /// Target path for symbolic links
    pub link_target: Option<String>,
    /// Owner user ID
    pub uid: u32,
    /// Owner group ID
    pub gid: u32,
    /// Last access time
    pub atime: Nfstime3,
    /// Last modification time
    pub mtime: Nfstime3,
    /// Last metadata change time
    pub ctime: Nfstime3,
    /// Number of hard links to this inode
    pub nlink: u32,
}

impl InodeEntry {
    fn new_dir(_inode: u64) -> Self {
        let now = Nfstime3::now();
        Self {
            ftype: Ftype3::Dir,
            mode: 0o755,
            size: 4096,
            data: vec![],
            children: HashMap::new(),
            link_target: None,
            uid: 0,
            gid: 0,
            atime: now,
            mtime: now,
            ctime: now,
            nlink: 2,
        }
    }

    fn new_file(_inode: u64) -> Self {
        let now = Nfstime3::now();
        Self {
            ftype: Ftype3::Reg,
            mode: 0o644,
            size: 0,
            data: vec![],
            children: HashMap::new(),
            link_target: None,
            uid: 0,
            gid: 0,
            atime: now,
            mtime: now,
            ctime: now,
            nlink: 1,
        }
    }

    fn to_fattr(&self, inode: u64, fsid: u64) -> Fattr3 {
        let used = if self.ftype == Ftype3::Reg {
            self.size.div_ceil(4096) * 4096
        } else {
            4096
        };
        Fattr3 {
            ftype: self.ftype,
            mode: self.mode,
            nlink: self.nlink,
            uid: self.uid,
            gid: self.gid,
            size: self.size,
            used,
            rdev: (0, 0),
            fsid,
            fileid: inode,
            atime: self.atime,
            mtime: self.mtime,
            ctime: self.ctime,
        }
    }
}

/// Mock in-memory VFS backend for testing NFS operations.
pub struct MockVfsBackend {
    /// In-memory inode storage
    inodes: Arc<RwLock<HashMap<u64, InodeEntry>>>,
    /// Atomic counter for allocating new inode numbers
    next_inode: std::sync::atomic::AtomicU64,
    /// Filesystem ID for this VFS instance
    fsid: u64,
}

impl MockVfsBackend {
    /// Creates a new mock VFS backend with a root directory.
    pub fn new(fsid: u64) -> Self {
        let inodes: HashMap<u64, InodeEntry> = [(1, InodeEntry::new_dir(1))].into();
        Self {
            inodes: Arc::new(RwLock::new(inodes)),
            next_inode: std::sync::atomic::AtomicU64::new(2),
            fsid,
        }
    }

    fn alloc_inode(&self) -> u64 {
        self.next_inode
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

impl VfsBackend for MockVfsBackend {
    fn getattr(&self, fh: &FileHandle3) -> Result<Fattr3> {
        let inode = fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let inodes = self.inodes.read().map_err(|_| GatewayError::Nfs3Io)?;
        let entry = inodes.get(&inode).ok_or(GatewayError::Nfs3NoEnt)?;
        Ok(entry.to_fattr(inode, self.fsid))
    }

    fn lookup(&self, dir_fh: &FileHandle3, name: &str) -> Result<LookupResult> {
        let dir_inode = dir_fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let inodes = self.inodes.read().map_err(|_| GatewayError::Nfs3Io)?;
        let dir = inodes.get(&dir_inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if dir.ftype != Ftype3::Dir {
            return Err(GatewayError::Nfs3NotDir);
        }

        if name == "." || name == ".." {
            let target_inode = if name == "." { dir_inode } else { 1 };
            let target = inodes.get(&target_inode).ok_or(GatewayError::Nfs3NoEnt)?;
            let obj_fh = FileHandle3::from_inode(target_inode);
            let obj_attr = Some(target.to_fattr(target_inode, self.fsid));
            let dir_attr = Some(dir.to_fattr(dir_inode, self.fsid));
            return Ok(LookupResult {
                object: obj_fh,
                obj_attributes: obj_attr,
                dir_attributes: dir_attr,
            });
        }

        let child_inode = *dir.children.get(name).ok_or(GatewayError::Nfs3NoEnt)?;
        let child = inodes.get(&child_inode).ok_or(GatewayError::Nfs3NoEnt)?;
        let obj_fh = FileHandle3::from_inode(child_inode);
        let obj_attr = Some(child.to_fattr(child_inode, self.fsid));
        let dir_attr = Some(dir.to_fattr(dir_inode, self.fsid));
        Ok(LookupResult {
            object: obj_fh,
            obj_attributes: obj_attr,
            dir_attributes: dir_attr,
        })
    }

    fn read(&self, fh: &FileHandle3, offset: u64, count: u32) -> Result<(Vec<u8>, bool)> {
        let inode = fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let inodes = self.inodes.read().map_err(|_| GatewayError::Nfs3Io)?;
        let entry = inodes.get(&inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if entry.ftype != Ftype3::Reg {
            return Err(GatewayError::Nfs3Inval);
        }

        let data = &entry.data;
        let eof = offset >= data.len() as u64;
        let start = offset as usize;
        let end = (start + count as usize).min(data.len());
        Ok((data[start..end].to_vec(), eof))
    }

    fn write(&self, fh: &FileHandle3, offset: u64, data: &[u8]) -> Result<u32> {
        let inode = fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let mut inodes = self.inodes.write().map_err(|_| GatewayError::Nfs3Io)?;
        let entry = inodes.get_mut(&inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if entry.ftype != Ftype3::Reg {
            return Err(GatewayError::Nfs3Inval);
        }

        let offset = offset as usize;
        if offset + data.len() > entry.data.len() {
            entry.data.resize(offset + data.len(), 0);
        }
        entry.data[offset..offset + data.len()].copy_from_slice(data);
        entry.size = entry.data.len() as u64;
        entry.mtime = Nfstime3::now();
        entry.ctime = Nfstime3::now();
        Ok(data.len() as u32)
    }

    fn readdir(&self, dir_fh: &FileHandle3, cookie: u64, count: u32) -> Result<ReadDirResult> {
        let dir_inode = dir_fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let inodes = self.inodes.read().map_err(|_| GatewayError::Nfs3Io)?;
        let dir = inodes.get(&dir_inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if dir.ftype != Ftype3::Dir {
            return Err(GatewayError::Nfs3NotDir);
        }

        let mut entries = Vec::new();
        let _cookie = cookie;

        let all_entries = vec![(".".to_string(), dir_inode), ("..".to_string(), 1)]
            .into_iter()
            .chain(dir.children.iter().map(|(k, &v)| (k.clone(), v)));

        for (idx, (name, child_inode)) in all_entries.enumerate() {
            if idx as u64 <= cookie {
                continue;
            }
            if entries.len() as u32 >= count {
                return Ok(ReadDirResult {
                    dir_attributes: Some(dir.to_fattr(dir_inode, self.fsid)),
                    cookieverf: 0,
                    entries,
                    eof: false,
                });
            }
            entries.push(Entry3 {
                fileid: child_inode,
                name,
                cookie: idx as u64 + 1,
            });
        }

        Ok(ReadDirResult {
            dir_attributes: Some(dir.to_fattr(dir_inode, self.fsid)),
            cookieverf: 0,
            entries,
            eof: true,
        })
    }

    fn mkdir(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)> {
        let dir_inode = dir_fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let mut inodes = self.inodes.write().map_err(|_| GatewayError::Nfs3Io)?;
        let dir = inodes.get_mut(&dir_inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if dir.ftype != Ftype3::Dir {
            return Err(GatewayError::Nfs3NotDir);
        }
        if dir.children.contains_key(name) {
            return Err(GatewayError::Nfs3Exist);
        }

        let new_inode = self.alloc_inode();
        dir.children.insert(name.to_string(), new_inode);
        dir.nlink += 1;
        dir.mtime = Nfstime3::now();
        dir.ctime = Nfstime3::now();

        let mut new_entry = InodeEntry::new_dir(new_inode);
        new_entry.mode = mode | 0o111;
        inodes.insert(new_inode, new_entry);

        let fh = FileHandle3::from_inode(new_inode);
        let attr = inodes
            .get(&new_inode)
            .unwrap()
            .to_fattr(new_inode, self.fsid);
        Ok((fh, attr))
    }

    fn create(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)> {
        let dir_inode = dir_fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let mut inodes = self.inodes.write().map_err(|_| GatewayError::Nfs3Io)?;
        let dir = inodes.get_mut(&dir_inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if dir.ftype != Ftype3::Dir {
            return Err(GatewayError::Nfs3NotDir);
        }
        if dir.children.contains_key(name) {
            return Err(GatewayError::Nfs3Exist);
        }

        let new_inode = self.alloc_inode();
        dir.children.insert(name.to_string(), new_inode);
        dir.mtime = Nfstime3::now();
        dir.ctime = Nfstime3::now();

        let mut new_entry = InodeEntry::new_file(new_inode);
        new_entry.mode = mode;
        inodes.insert(new_inode, new_entry);

        let fh = FileHandle3::from_inode(new_inode);
        let attr = inodes
            .get(&new_inode)
            .unwrap()
            .to_fattr(new_inode, self.fsid);
        Ok((fh, attr))
    }

    fn remove(&self, dir_fh: &FileHandle3, name: &str) -> Result<()> {
        let dir_inode = dir_fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let mut inodes = self.inodes.write().map_err(|_| GatewayError::Nfs3Io)?;
        {
            let dir = inodes.get_mut(&dir_inode).ok_or(GatewayError::Nfs3NoEnt)?;
            if dir.ftype != Ftype3::Dir {
                return Err(GatewayError::Nfs3NotDir);
            }
        }

        let child_inode = {
            let dir = inodes.get_mut(&dir_inode).ok_or(GatewayError::Nfs3NoEnt)?;
            dir.children.remove(name).ok_or(GatewayError::Nfs3NoEnt)?
        };

        let is_dir = {
            let child = inodes.get(&child_inode).ok_or(GatewayError::Nfs3NoEnt)?;
            child.ftype == Ftype3::Dir
        };

        inodes.remove(&child_inode);

        let dir = inodes.get_mut(&dir_inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if is_dir {
            dir.nlink -= 1;
        }
        dir.mtime = Nfstime3::now();
        dir.ctime = Nfstime3::now();
        Ok(())
    }

    fn rename(
        &self,
        from_dir: &FileHandle3,
        from_name: &str,
        to_dir: &FileHandle3,
        to_name: &str,
    ) -> Result<()> {
        let from_inode = from_dir.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let to_inode = to_dir.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;

        let mut inodes = self.inodes.write().map_err(|_| GatewayError::Nfs3Io)?;

        let child_inode = {
            let from_dir_entry = inodes.get_mut(&from_inode).ok_or(GatewayError::Nfs3NoEnt)?;
            if from_dir_entry.ftype != Ftype3::Dir {
                return Err(GatewayError::Nfs3NotDir);
            }
            from_dir_entry
                .children
                .remove(from_name)
                .ok_or(GatewayError::Nfs3NoEnt)?
        };

        {
            let to_dir_entry = inodes.get_mut(&to_inode).ok_or(GatewayError::Nfs3NoEnt)?;
            if to_dir_entry.ftype != Ftype3::Dir {
                let _ = to_dir_entry;
                let mut inodes = self.inodes.write().map_err(|_| GatewayError::Nfs3Io)?;
                let from_dir_entry = inodes.get_mut(&from_inode).ok_or(GatewayError::Nfs3NoEnt)?;
                from_dir_entry
                    .children
                    .insert(from_name.to_string(), child_inode);
                return Err(GatewayError::Nfs3NotDir);
            }
            if to_dir_entry.children.contains_key(to_name) {
                let _ = to_dir_entry;
                let mut inodes = self.inodes.write().map_err(|_| GatewayError::Nfs3Io)?;
                let from_dir_entry = inodes.get_mut(&from_inode).ok_or(GatewayError::Nfs3NoEnt)?;
                from_dir_entry
                    .children
                    .insert(from_name.to_string(), child_inode);
                return Err(GatewayError::Nfs3Exist);
            }
        }

        {
            let to_dir_entry = inodes.get_mut(&to_inode).ok_or(GatewayError::Nfs3NoEnt)?;
            to_dir_entry
                .children
                .insert(to_name.to_string(), child_inode);
            to_dir_entry.mtime = Nfstime3::now();
            to_dir_entry.ctime = Nfstime3::now();
        }

        let from_dir_entry = inodes.get_mut(&from_inode).ok_or(GatewayError::Nfs3NoEnt)?;
        from_dir_entry.mtime = Nfstime3::now();
        from_dir_entry.ctime = Nfstime3::now();
        Ok(())
    }

    fn readlink(&self, fh: &FileHandle3) -> Result<String> {
        let inode = fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let inodes = self.inodes.read().map_err(|_| GatewayError::Nfs3Io)?;
        let entry = inodes.get(&inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if let Some(ref target) = entry.link_target {
            Ok(target.clone())
        } else {
            Err(GatewayError::Nfs3Inval)
        }
    }

    fn symlink(
        &self,
        dir_fh: &FileHandle3,
        name: &str,
        target: &str,
    ) -> Result<(FileHandle3, Fattr3)> {
        let dir_inode = dir_fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let mut inodes = self.inodes.write().map_err(|_| GatewayError::Nfs3Io)?;
        let dir = inodes.get_mut(&dir_inode).ok_or(GatewayError::Nfs3NoEnt)?;
        if dir.ftype != Ftype3::Dir {
            return Err(GatewayError::Nfs3NotDir);
        }
        if dir.children.contains_key(name) {
            return Err(GatewayError::Nfs3Exist);
        }

        let new_inode = self.alloc_inode();
        dir.children.insert(name.to_string(), new_inode);
        dir.mtime = Nfstime3::now();
        dir.ctime = Nfstime3::now();

        let mut new_entry = InodeEntry::new_file(new_inode);
        new_entry.ftype = Ftype3::Lnk;
        new_entry.link_target = Some(target.to_string());
        new_entry.size = target.len() as u64;
        inodes.insert(new_inode, new_entry);

        let fh = FileHandle3::from_inode(new_inode);
        let attr = inodes
            .get(&new_inode)
            .unwrap()
            .to_fattr(new_inode, self.fsid);
        Ok((fh, attr))
    }

    fn fsstat(&self, _fh: &FileHandle3) -> Result<FsStatResult> {
        let _inodes = self.inodes.read().map_err(|_| GatewayError::Nfs3Io)?;
        Ok(FsStatResult {
            tbytes: 1_000_000_000_000,
            fbytes: 800_000_000_000,
            abytes: 700_000_000_000,
            tfiles: 1_000_000,
            ffiles: 900_000,
            afiles: 800_000,
            invarsec: 30,
        })
    }

    fn fsinfo(&self, _fh: &FileHandle3) -> Result<FsInfoResult> {
        Ok(FsInfoResult::defaults())
    }

    fn pathconf(&self, _fh: &FileHandle3) -> Result<PathConfResult> {
        Ok(PathConfResult::defaults())
    }

    fn access(&self, fh: &FileHandle3, uid: u32, gid: u32, access_bits: u32) -> Result<u32> {
        let inode = fh.as_inode().ok_or(GatewayError::Nfs3BadHandle)?;
        let inodes = self.inodes.read().map_err(|_| GatewayError::Nfs3Io)?;
        let entry = inodes.get(&inode).ok_or(GatewayError::Nfs3NoEnt)?;

        let mut allowed = 0u32;
        if uid == 0 {
            allowed = access_bits;
        } else if uid == entry.uid {
            if entry.mode & 0o400 != 0 && access_bits & 4 != 0 {
                allowed |= 4;
            }
            if entry.mode & 0o200 != 0 && access_bits & 2 != 0 {
                allowed |= 2;
            }
            if entry.mode & 0o100 != 0 && access_bits & 1 != 0 {
                allowed |= 1;
            }
        } else if gid == entry.gid {
            if entry.mode & 0o040 != 0 && access_bits & 4 != 0 {
                allowed |= 4;
            }
            if entry.mode & 0o020 != 0 && access_bits & 2 != 0 {
                allowed |= 2;
            }
            if entry.mode & 0o010 != 0 && access_bits & 1 != 0 {
                allowed |= 1;
            }
        } else {
            if entry.mode & 0o004 != 0 && access_bits & 4 != 0 {
                allowed |= 4;
            }
            if entry.mode & 0o002 != 0 && access_bits & 2 != 0 {
                allowed |= 2;
            }
            if entry.mode & 0o001 != 0 && access_bits & 1 != 0 {
                allowed |= 1;
            }
        }
        Ok(allowed)
    }
}

/// Result of NFS GETATTR operation.
pub enum Nfs3GetAttrResult {
    /// Success - returns file attributes
    Ok(Fattr3),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS LOOKUP operation.
pub enum Nfs3LookupResult {
    /// Success - returns file handle and attributes
    Ok(LookupResult),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS READ operation.
pub enum Nfs3ReadResult {
    /// Success - returns data buffer and EOF flag
    Ok(Vec<u8>, bool),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS WRITE operation.
pub enum Nfs3WriteResult {
    /// Success - returns bytes written and stable flag
    Ok(u32, u32),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS CREATE/MKDIR/SYMLINK operation.
pub enum Nfs3CreateResult {
    /// Success - returns new file handle and attributes
    Ok(FileHandle3, Fattr3),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS MKDIR operation.
pub enum Nfs3MkdirResult {
    /// Success - returns new directory handle and attributes
    Ok(FileHandle3, Fattr3),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS REMOVE operation.
pub enum Nfs3RemoveResult {
    /// Success
    Ok,
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS RENAME operation.
pub enum Nfs3RenameResult {
    /// Success
    Ok,
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS READDIR operation.
pub enum Nfs3ReadDirResult {
    /// Success - returns directory entries
    Ok(ReadDirResult),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS FSSTAT operation.
pub enum Nfs3FsStatResult {
    /// Success - returns filesystem statistics
    Ok(FsStatResult),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS FSINFO operation.
pub enum Nfs3FsInfoResult {
    /// Success - returns filesystem information
    Ok(FsInfoResult),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS PATHCONF operation.
pub enum Nfs3PathConfResult {
    /// Success - returns path configuration
    Ok(PathConfResult),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS ACCESS operation.
pub enum Nfs3AccessResult {
    /// Success - returns granted access bits
    Ok(u32),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS READLINK operation.
pub enum Nfs3ReadLinkResult {
    /// Success - returns symlink target path
    Ok(String),
    /// Error - returns NFS error code
    Err(u32),
}

/// Result of NFS SYMLINK operation.
pub enum Nfs3SymLinkResult {
    /// Success - returns new symlink handle and attributes
    Ok(FileHandle3, Fattr3),
    /// Error - returns NFS error code
    Err(u32),
}

/// NFSv3 protocol handler dispatching operations to a VFS backend.
pub struct Nfs3Handler<B: VfsBackend> {
    /// VFS backend for file operations
    backend: Arc<B>,
    /// Filesystem ID
    #[allow(dead_code)]
    fsid: u64,
}

impl<B: VfsBackend> Nfs3Handler<B> {
    /// Creates a new NFSv3 handler with the given VFS backend.
    pub fn new(backend: Arc<B>, fsid: u64) -> Self {
        Self { backend, fsid }
    }

    /// Handles NFS GETATTR - retrieves file attributes.
    pub fn handle_getattr(&self, fh: &FileHandle3) -> Nfs3GetAttrResult {
        match self.backend.getattr(fh) {
            Ok(attr) => Nfs3GetAttrResult::Ok(attr),
            Err(e) => Nfs3GetAttrResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS LOOKUP - resolves a filename in a directory to a file handle.
    pub fn handle_lookup(&self, dir_fh: &FileHandle3, name: &str) -> Nfs3LookupResult {
        match self.backend.lookup(dir_fh, name) {
            Ok(result) => Nfs3LookupResult::Ok(result),
            Err(e) => Nfs3LookupResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS READ - reads data from a file.
    pub fn handle_read(&self, fh: &FileHandle3, offset: u64, count: u32) -> Nfs3ReadResult {
        match self.backend.read(fh, offset, count) {
            Ok((data, eof)) => Nfs3ReadResult::Ok(data, eof),
            Err(e) => Nfs3ReadResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS WRITE - writes data to a file.
    pub fn handle_write(
        &self,
        fh: &FileHandle3,
        offset: u64,
        _stable: u32,
        data: &[u8],
    ) -> Nfs3WriteResult {
        match self.backend.write(fh, offset, data) {
            Ok(count) => Nfs3WriteResult::Ok(count, 0),
            Err(e) => Nfs3WriteResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS CREATE - creates a new regular file.
    pub fn handle_create(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Nfs3CreateResult {
        match self.backend.create(dir_fh, name, mode) {
            Ok((fh, attr)) => Nfs3CreateResult::Ok(fh, attr),
            Err(e) => Nfs3CreateResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS MKDIR - creates a new directory.
    pub fn handle_mkdir(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Nfs3MkdirResult {
        match self.backend.mkdir(dir_fh, name, mode) {
            Ok((fh, attr)) => Nfs3MkdirResult::Ok(fh, attr),
            Err(e) => Nfs3MkdirResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS REMOVE - removes a file or empty directory.
    pub fn handle_remove(&self, dir_fh: &FileHandle3, name: &str) -> Nfs3RemoveResult {
        match self.backend.remove(dir_fh, name) {
            Ok(()) => Nfs3RemoveResult::Ok,
            Err(e) => Nfs3RemoveResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS RENAME - renames or moves a file or directory.
    pub fn handle_rename(
        &self,
        from_dir: &FileHandle3,
        from_name: &str,
        to_dir: &FileHandle3,
        to_name: &str,
    ) -> Nfs3RenameResult {
        match self.backend.rename(from_dir, from_name, to_dir, to_name) {
            Ok(()) => Nfs3RenameResult::Ok,
            Err(e) => Nfs3RenameResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS READDIR - lists directory entries.
    pub fn handle_readdir(
        &self,
        dir_fh: &FileHandle3,
        cookie: u64,
        _cookieverf: u64,
        count: u32,
    ) -> Nfs3ReadDirResult {
        match self.backend.readdir(dir_fh, cookie, count) {
            Ok(result) => Nfs3ReadDirResult::Ok(result),
            Err(e) => Nfs3ReadDirResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS FSSTAT - retrieves filesystem statistics.
    pub fn handle_fsstat(&self, fh: &FileHandle3) -> Nfs3FsStatResult {
        match self.backend.fsstat(fh) {
            Ok(result) => Nfs3FsStatResult::Ok(result),
            Err(e) => Nfs3FsStatResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS FSINFO - retrieves filesystem capabilities and limits.
    pub fn handle_fsinfo(&self, fh: &FileHandle3) -> Nfs3FsInfoResult {
        match self.backend.fsinfo(fh) {
            Ok(result) => Nfs3FsInfoResult::Ok(result),
            Err(e) => Nfs3FsInfoResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS PATHCONF - retrieves path-related configuration limits.
    pub fn handle_pathconf(&self, fh: &FileHandle3) -> Nfs3PathConfResult {
        match self.backend.pathconf(fh) {
            Ok(result) => Nfs3PathConfResult::Ok(result),
            Err(e) => Nfs3PathConfResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS ACCESS - checks file access permissions.
    pub fn handle_access(
        &self,
        fh: &FileHandle3,
        uid: u32,
        gid: u32,
        access: u32,
    ) -> Nfs3AccessResult {
        match self.backend.access(fh, uid, gid, access) {
            Ok(result) => Nfs3AccessResult::Ok(result),
            Err(e) => Nfs3AccessResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS READLINK - reads the target of a symbolic link.
    pub fn handle_readlink(&self, fh: &FileHandle3) -> Nfs3ReadLinkResult {
        match self.backend.readlink(fh) {
            Ok(target) => Nfs3ReadLinkResult::Ok(target),
            Err(e) => Nfs3ReadLinkResult::Err(e.nfs3_status()),
        }
    }

    /// Handles NFS SYMLINK - creates a symbolic link.
    pub fn handle_symlink(
        &self,
        dir_fh: &FileHandle3,
        name: &str,
        target: &str,
    ) -> Nfs3SymLinkResult {
        match self.backend.symlink(dir_fh, name, target) {
            Ok((fh, attr)) => Nfs3SymLinkResult::Ok(fh, attr),
            Err(e) => Nfs3SymLinkResult::Err(e.nfs3_status()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (
        Arc<MockVfsBackend>,
        Nfs3Handler<MockVfsBackend>,
        FileHandle3,
    ) {
        let backend = Arc::new(MockVfsBackend::new(1));
        let handler = Nfs3Handler::new(backend.clone(), 1);
        let root_fh = FileHandle3::from_inode(1);
        (backend, handler, root_fh)
    }

    #[test]
    fn test_lookup_root_dot() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_lookup(&root_fh, ".");
        match result {
            Nfs3LookupResult::Ok(lr) => assert_eq!(lr.object.as_inode(), Some(1)),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_lookup_root_dotdot() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_lookup(&root_fh, "..");
        match result {
            Nfs3LookupResult::Ok(lr) => assert_eq!(lr.object.as_inode(), Some(1)),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_mkdir() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_mkdir(&root_fh, "subdir", 0o755);
        match result {
            Nfs3MkdirResult::Ok(fh, _) => assert!(fh.as_inode().is_some()),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_create_file() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_create(&root_fh, "file.txt", 0o644);
        match result {
            Nfs3CreateResult::Ok(fh, _) => assert!(fh.as_inode().is_some()),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_write_and_read() {
        let (_backend, handler, root_fh) = setup();
        let create_result = handler.handle_create(&root_fh, "testfile", 0o644);
        let (fh, _) = match create_result {
            Nfs3CreateResult::Ok(fh, attr) => (fh, attr),
            _ => panic!("expected ok"),
        };

        let write_result = handler.handle_write(&fh, 0, 0, b"hello world");
        match write_result {
            Nfs3WriteResult::Ok(count, _) => assert_eq!(count, 11),
            _ => panic!("expected ok"),
        }

        let read_result = handler.handle_read(&fh, 0, 100);
        match read_result {
            Nfs3ReadResult::Ok(data, _) => assert_eq!(&data[..], b"hello world"),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_remove_file() {
        let (_backend, handler, root_fh) = setup();
        handler.handle_create(&root_fh, "deleteme", 0o644);
        let result = handler.handle_remove(&root_fh, "deleteme");
        match result {
            Nfs3RemoveResult::Ok => (),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_rename_directory() {
        let (_backend, handler, root_fh) = setup();
        handler.handle_mkdir(&root_fh, "oldname", 0o755);
        let result = handler.handle_rename(&root_fh, "oldname", &root_fh, "newname");
        match result {
            Nfs3RenameResult::Ok => (),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_readdir() {
        let (_backend, handler, root_fh) = setup();
        handler.handle_mkdir(&root_fh, "dir1", 0o755);
        handler.handle_create(&root_fh, "file1", 0o644);

        let result = handler.handle_readdir(&root_fh, 0, 0, 100);
        match result {
            Nfs3ReadDirResult::Ok(rdr) => {
                assert!(rdr.eof);
            }
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_fsstat() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_fsstat(&root_fh);
        match result {
            Nfs3FsStatResult::Ok(stat) => {
                assert_eq!(stat.tbytes, 1_000_000_000_000);
            }
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_fsinfo() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_fsinfo(&root_fh);
        match result {
            Nfs3FsInfoResult::Ok(info) => {
                assert_eq!(info.rtmax, 1048576);
            }
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_symlink_readlink() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_symlink(&root_fh, "mylink", "/target/path");
        match result {
            Nfs3SymLinkResult::Ok(fh, _) => {
                let readlink_result = handler.handle_readlink(&fh);
                match readlink_result {
                    Nfs3ReadLinkResult::Ok(target) => assert_eq!(target, "/target/path"),
                    _ => panic!("expected ok"),
                }
            }
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_getattr() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_getattr(&root_fh);
        match result {
            Nfs3GetAttrResult::Ok(attr) => {
                assert_eq!(attr.fileid, 1);
            }
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_access() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_access(&root_fh, 0, 0, 7);
        match result {
            Nfs3AccessResult::Ok(bits) => {
                assert_eq!(bits, 7);
            }
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_pathconf() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_pathconf(&root_fh);
        match result {
            Nfs3PathConfResult::Ok(pc) => {
                assert_eq!(pc.name_max, 255);
            }
            _ => panic!("expected ok"),
        }
    }
}
