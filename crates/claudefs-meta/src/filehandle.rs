//! Open file handle management for metadata service.
//!
//! Tracks open file descriptors from FUSE clients, supporting lease revocation,
//! lock cleanup, and exclusive open-for-write semantics.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::types::*;

/// Flags for file open mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OpenFlags(u32);

impl OpenFlags {
    /// Read access flag.
    pub const READ: OpenFlags = OpenFlags(0x01);
    /// Write access flag.
    pub const WRITE: OpenFlags = OpenFlags(0x02);
    /// Append mode flag.
    pub const APPEND: OpenFlags = OpenFlags(0x04);
    /// Truncate flag.
    pub const TRUNCATE: OpenFlags = OpenFlags(0x08);
    /// Create flag.
    pub const CREATE: OpenFlags = OpenFlags(0x10);
    /// Exclusive open flag.
    pub const EXCLUSIVE: OpenFlags = OpenFlags(0x20);

    /// Checks if this flags contains another flags.
    pub fn contains(&self, other: OpenFlags) -> bool {
        (self.0 & other.0) != 0
    }

    /// Returns true if the file is readable.
    pub fn is_readable(&self) -> bool {
        self.contains(Self::READ)
    }

    /// Returns true if the file is writable (write or append mode).
    pub fn is_writable(&self) -> bool {
        self.contains(Self::WRITE) || self.contains(Self::APPEND)
    }
}

impl std::ops::BitOr for OpenFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        OpenFlags(self.0 | rhs.0)
    }
}

/// An open file handle.
#[derive(Clone, Debug)]
pub struct FileHandle {
    /// File handle identifier.
    pub fh: u64,
    /// Inode number.
    pub ino: InodeId,
    /// Client node that opened the file.
    pub client: NodeId,
    /// Open flags.
    pub flags: OpenFlags,
    /// Timestamp when the file was opened.
    pub opened_at: Timestamp,
}

/// Manages open file handles.
///
/// Tracks open file descriptors from FUSE clients, supporting lease revocation,
/// lock cleanup, and exclusive open-for-write semantics.
pub struct FileHandleManager {
    next_fh: AtomicU64,
    handles: RwLock<HashMap<u64, FileHandle>>,
    /// Index: inode -> list of fh IDs.
    inode_handles: RwLock<HashMap<InodeId, Vec<u64>>>,
    /// Index: client -> list of fh IDs.
    client_handles: RwLock<HashMap<NodeId, Vec<u64>>>,
}

impl FileHandleManager {
    /// Creates a new FileHandleManager.
    pub fn new() -> Self {
        Self {
            next_fh: AtomicU64::new(1),
            handles: RwLock::new(HashMap::new()),
            inode_handles: RwLock::new(HashMap::new()),
            client_handles: RwLock::new(HashMap::new()),
        }
    }

    /// Opens a file and returns the file handle.
    pub fn open(&self, ino: InodeId, client: NodeId, flags: OpenFlags) -> u64 {
        let fh = self.next_fh.fetch_add(1, Ordering::Relaxed);
        let handle = FileHandle {
            fh,
            ino,
            client,
            flags,
            opened_at: Timestamp::now(),
        };

        let mut handles = self.handles.write().unwrap();
        handles.insert(fh, handle);

        let mut inode_map = self.inode_handles.write().unwrap();
        inode_map.entry(ino).or_default().push(fh);

        let mut client_map = self.client_handles.write().unwrap();
        client_map.entry(client).or_default().push(fh);

        fh
    }

    /// Closes a file handle and returns the handle info.
    ///
    /// Returns an error if the handle does not exist.
    pub fn close(&self, fh: u64) -> Result<FileHandle, MetaError> {
        let mut handles = self.handles.write().unwrap();
        let handle = handles
            .remove(&fh)
            .ok_or_else(|| MetaError::InodeNotFound(InodeId::new(fh)))?;

        {
            let mut inode_map = self.inode_handles.write().unwrap();
            if let Some(fhs) = inode_map.get_mut(&handle.ino) {
                fhs.retain(|&x| x != fh);
                if fhs.is_empty() {
                    inode_map.remove(&handle.ino);
                }
            }
        }

        {
            let mut client_map = self.client_handles.write().unwrap();
            if let Some(fhs) = client_map.get_mut(&handle.client) {
                fhs.retain(|&x| x != fh);
                if fhs.is_empty() {
                    client_map.remove(&handle.client);
                }
            }
        }

        Ok(handle)
    }

    /// Gets handle info for a file handle.
    ///
    /// Returns an error if the handle does not exist.
    pub fn get(&self, fh: u64) -> Result<FileHandle, MetaError> {
        let handles = self.handles.read().unwrap();
        handles
            .get(&fh)
            .cloned()
            .ok_or_else(|| MetaError::InodeNotFound(InodeId::new(fh)))
    }

    /// Checks if any handle is open for this inode.
    pub fn is_open(&self, ino: InodeId) -> bool {
        let inode_map = self.inode_handles.read().unwrap();
        inode_map
            .get(&ino)
            .map(|fhs| !fhs.is_empty())
            .unwrap_or(false)
    }

    /// Checks if any handle has write access for this inode.
    pub fn is_open_for_write(&self, ino: InodeId) -> bool {
        let inode_map = self.inode_handles.read().unwrap();
        if let Some(fhs) = inode_map.get(&ino) {
            let handles = self.handles.read().unwrap();
            for &fh in fhs {
                if let Some(handle) = handles.get(&fh) {
                    if handle.flags.is_writable() {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Returns all handles for an inode.
    pub fn handles_for_inode(&self, ino: InodeId) -> Vec<FileHandle> {
        let inode_map = self.inode_handles.read().unwrap();
        if let Some(fhs) = inode_map.get(&ino) {
            let handles = self.handles.read().unwrap();
            fhs.iter()
                .filter_map(|&fh| handles.get(&fh).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Returns all handles for a client.
    pub fn handles_for_client(&self, client: NodeId) -> Vec<FileHandle> {
        let client_map = self.client_handles.read().unwrap();
        if let Some(fhs) = client_map.get(&client) {
            let handles = self.handles.read().unwrap();
            fhs.iter()
                .filter_map(|&fh| handles.get(&fh).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Closes all handles for a client (for disconnect cleanup).
    ///
    /// Returns all the closed handles.
    pub fn close_all_for_client(&self, client: NodeId) -> Vec<FileHandle> {
        let mut closed = Vec::new();

        let fhs: Vec<u64> = {
            let client_map = self.client_handles.read().unwrap();
            client_map.get(&client).cloned().unwrap_or_default()
        };

        for fh in fhs {
            if let Ok(handle) = self.close(fh) {
                closed.push(handle);
            }
        }

        closed
    }

    /// Returns the number of open file handles.
    pub fn open_count(&self) -> usize {
        let handles = self.handles.read().unwrap();
        handles.len()
    }
}

impl Default for FileHandleManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_and_close() {
        let mgr = FileHandleManager::new();
        let ino = InodeId::new(100);
        let client = NodeId::new(1);
        let flags = OpenFlags::READ;

        let fh = mgr.open(ino, client, flags);
        assert!(fh > 0);

        let handle = mgr.close(fh).unwrap();
        assert_eq!(handle.fh, fh);
        assert_eq!(handle.ino, ino);
        assert_eq!(handle.client, client);
    }

    #[test]
    fn test_get_handle() {
        let mgr = FileHandleManager::new();
        let ino = InodeId::new(200);
        let client = NodeId::new(2);
        let flags = OpenFlags::READ | OpenFlags::WRITE;

        let fh = mgr.open(ino, client, flags);
        let handle = mgr.get(fh).unwrap();

        assert_eq!(handle.fh, fh);
        assert_eq!(handle.ino, ino);
        assert!(handle.flags.is_readable());
        assert!(handle.flags.is_writable());
    }

    #[test]
    fn test_is_open() {
        let mgr = FileHandleManager::new();
        let ino = InodeId::new(300);

        assert!(!mgr.is_open(ino));

        let fh = mgr.open(ino, NodeId::new(1), OpenFlags::READ);
        assert!(mgr.is_open(ino));

        mgr.close(fh).unwrap();
        assert!(!mgr.is_open(ino));
    }

    #[test]
    fn test_is_open_for_write() {
        let mgr = FileHandleManager::new();
        let ino = InodeId::new(400);

        assert!(!mgr.is_open_for_write(ino));

        mgr.open(ino, NodeId::new(1), OpenFlags::READ);
        assert!(!mgr.is_open_for_write(ino));

        let fh2 = mgr.open(ino, NodeId::new(2), OpenFlags::WRITE);
        assert!(mgr.is_open_for_write(ino));

        mgr.close(fh2).unwrap();
        assert!(!mgr.is_open_for_write(ino));
    }

    #[test]
    fn test_handles_for_inode() {
        let mgr = FileHandleManager::new();
        let ino = InodeId::new(500);

        let fh1 = mgr.open(ino, NodeId::new(1), OpenFlags::READ);
        let fh2 = mgr.open(ino, NodeId::new(2), OpenFlags::WRITE);

        let handles = mgr.handles_for_inode(ino);
        assert_eq!(handles.len(), 2);
        assert!(handles.iter().any(|h| h.fh == fh1));
        assert!(handles.iter().any(|h| h.fh == fh2));
    }

    #[test]
    fn test_handles_for_client() {
        let mgr = FileHandleManager::new();
        let client = NodeId::new(3);
        let ino1 = InodeId::new(600);
        let ino2 = InodeId::new(700);

        mgr.open(ino1, client, OpenFlags::READ);
        mgr.open(ino2, client, OpenFlags::WRITE);

        let handles = mgr.handles_for_client(client);
        assert_eq!(handles.len(), 2);
    }

    #[test]
    fn test_close_all_for_client() {
        let mgr = FileHandleManager::new();
        let client = NodeId::new(4);

        mgr.open(InodeId::new(800), client, OpenFlags::READ);
        mgr.open(InodeId::new(900), client, OpenFlags::WRITE);

        let closed = mgr.close_all_for_client(client);
        assert_eq!(closed.len(), 2);
        assert_eq!(mgr.open_count(), 0);
    }

    #[test]
    fn test_open_count() {
        let mgr = FileHandleManager::new();
        assert_eq!(mgr.open_count(), 0);

        let fh1 = mgr.open(InodeId::new(1000), NodeId::new(1), OpenFlags::READ);
        let fh2 = mgr.open(InodeId::new(1100), NodeId::new(1), OpenFlags::READ);
        assert_eq!(mgr.open_count(), 2);

        mgr.close(fh1).unwrap();
        assert_eq!(mgr.open_count(), 1);

        mgr.close(fh2).unwrap();
        assert_eq!(mgr.open_count(), 0);
    }

    #[test]
    fn test_close_nonexistent() {
        let mgr = FileHandleManager::new();
        let result = mgr.close(9999);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_opens_same_inode() {
        let mgr = FileHandleManager::new();
        let ino = InodeId::new(1200);
        let client1 = NodeId::new(1);
        let client2 = NodeId::new(2);

        let fh1 = mgr.open(ino, client1, OpenFlags::READ);
        let fh2 = mgr.open(ino, client2, OpenFlags::WRITE);

        assert_ne!(fh1, fh2);

        let handles = mgr.handles_for_inode(ino);
        assert_eq!(handles.len(), 2);
    }
}
