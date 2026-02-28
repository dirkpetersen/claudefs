//! High-level metadata service combining all subsystems.
//!
//! MetadataService is the primary entry point for metadata operations.
//! It coordinates between the KV store, inode store, directory store,
//! Raft consensus, and journal to provide a complete POSIX-like API.

use std::sync::Arc;

use crate::consensus::{RaftConfig, RaftNode};
use crate::directory::DirectoryStore;
use crate::inode::InodeStore;
use crate::journal::MetadataJournal;
use crate::kvstore::{KvStore, MemoryKvStore};
use crate::replication::ReplicationTracker;
use crate::types::*;

/// Configuration for the metadata service.
pub struct MetadataServiceConfig {
    /// This node's ID.
    pub node_id: NodeId,
    /// Peer nodes in the cluster.
    pub peers: Vec<NodeId>,
    /// Site ID for replication.
    pub site_id: u64,
    /// Number of virtual shards (default 256, per D4).
    pub num_shards: u16,
    /// Maximum journal entries to keep.
    pub max_journal_entries: usize,
}

impl Default for MetadataServiceConfig {
    fn default() -> Self {
        Self {
            node_id: NodeId::new(1),
            peers: Vec::new(),
            site_id: 1,
            num_shards: 256,
            max_journal_entries: 100_000,
        }
    }
}

/// The metadata service — primary entry point for all metadata operations.
///
/// Combines KV store, inode operations, directory operations, Raft consensus,
/// and journal into a cohesive API for use by FUSE clients, NFS gateways, etc.
pub struct MetadataService {
    #[allow(dead_code)] // used directly in Phase 2 for Raft-driven KV operations
    kv: Arc<dyn KvStore>,
    inodes: Arc<InodeStore>,
    dirs: DirectoryStore,
    #[allow(dead_code)] // used in Phase 2 for consensus-driven metadata mutations
    raft: std::sync::Mutex<RaftNode>,
    journal: Arc<MetadataJournal>,
    replication: ReplicationTracker,
    config: MetadataServiceConfig,
}

impl MetadataService {
    /// Create a new metadata service with in-memory storage (Phase 1).
    pub fn new(config: MetadataServiceConfig) -> Self {
        let kv: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());

        let raft_config = RaftConfig {
            node_id: config.node_id,
            peers: config.peers.clone(),
            ..RaftConfig::default()
        };
        let raft = std::sync::Mutex::new(RaftNode::new(raft_config));

        let journal = Arc::new(MetadataJournal::new(
            config.site_id,
            config.max_journal_entries,
        ));
        let replication = ReplicationTracker::new(journal.clone());

        Self {
            kv,
            inodes,
            dirs,
            raft,
            journal,
            replication,
            config,
        }
    }

    /// Initialize the root directory (inode 1).
    /// Call this once when creating a new filesystem.
    pub fn init_root(&self) -> Result<(), MetaError> {
        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, self.config.site_id);
        self.inodes.create_inode(&root)
    }

    /// Create a file in a directory.
    /// Returns the new file's inode attributes.
    pub fn create_file(
        &self,
        parent: InodeId,
        name: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<InodeAttr, MetaError> {
        let ino = self.inodes.allocate_inode();
        let attr = InodeAttr::new_file(ino, uid, gid, mode, self.config.site_id);

        self.inodes.create_inode(&attr)?;

        let entry = DirEntry {
            name: name.to_string(),
            ino,
            file_type: FileType::RegularFile,
        };
        if let Err(e) = self.dirs.create_entry(parent, &entry) {
            let _ = self.inodes.delete_inode(ino);
            return Err(e);
        }

        let _ = self
            .journal
            .append(MetaOp::CreateInode { attr: attr.clone() }, LogIndex::ZERO);
        let _ = self.journal.append(
            MetaOp::CreateEntry {
                parent,
                name: name.to_string(),
                entry,
            },
            LogIndex::ZERO,
        );

        if let Ok(mut parent_attr) = self.inodes.get_inode(parent) {
            parent_attr.mtime = Timestamp::now();
            parent_attr.ctime = Timestamp::now();
            let _ = self.inodes.set_inode(&parent_attr);
        }

        Ok(attr)
    }

    /// Create a subdirectory.
    /// Returns the new directory's inode attributes.
    pub fn mkdir(
        &self,
        parent: InodeId,
        name: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<InodeAttr, MetaError> {
        let ino = self.inodes.allocate_inode();
        let attr = InodeAttr::new_directory(ino, uid, gid, mode, self.config.site_id);

        self.inodes.create_inode(&attr)?;

        let entry = DirEntry {
            name: name.to_string(),
            ino,
            file_type: FileType::Directory,
        };
        if let Err(e) = self.dirs.create_entry(parent, &entry) {
            let _ = self.inodes.delete_inode(ino);
            return Err(e);
        }

        if let Ok(mut parent_attr) = self.inodes.get_inode(parent) {
            parent_attr.nlink += 1;
            parent_attr.mtime = Timestamp::now();
            parent_attr.ctime = Timestamp::now();
            let _ = self.inodes.set_inode(&parent_attr);
        }

        let _ = self
            .journal
            .append(MetaOp::CreateInode { attr: attr.clone() }, LogIndex::ZERO);

        Ok(attr)
    }

    /// Look up a name in a directory.
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeAttr, MetaError> {
        let entry = self.dirs.lookup(parent, name)?;
        self.inodes.get_inode(entry.ino)
    }

    /// Get inode attributes (stat).
    pub fn getattr(&self, ino: InodeId) -> Result<InodeAttr, MetaError> {
        self.inodes.get_inode(ino)
    }

    /// Set inode attributes (chmod, chown, truncate, utimes).
    pub fn setattr(&self, ino: InodeId, attr: InodeAttr) -> Result<(), MetaError> {
        self.inodes.set_inode(&attr)?;

        let _ = self
            .journal
            .append(MetaOp::SetAttr { ino, attr }, LogIndex::ZERO);

        Ok(())
    }

    /// List directory entries (readdir).
    pub fn readdir(&self, parent: InodeId) -> Result<Vec<DirEntry>, MetaError> {
        self.dirs.list_entries(parent)
    }

    /// Unlink a file from a directory.
    /// If nlink drops to 0, the inode is deleted.
    pub fn unlink(&self, parent: InodeId, name: &str) -> Result<(), MetaError> {
        let entry = self.dirs.delete_entry(parent, name)?;

        let mut attr = self.inodes.get_inode(entry.ino)?;
        attr.nlink = attr.nlink.saturating_sub(1);

        if attr.nlink == 0 {
            self.inodes.delete_inode(entry.ino)?;
            let _ = self
                .journal
                .append(MetaOp::DeleteInode { ino: entry.ino }, LogIndex::ZERO);
        } else {
            attr.ctime = Timestamp::now();
            self.inodes.set_inode(&attr)?;
        }

        let _ = self.journal.append(
            MetaOp::DeleteEntry {
                parent,
                name: name.to_string(),
            },
            LogIndex::ZERO,
        );

        if let Ok(mut parent_attr) = self.inodes.get_inode(parent) {
            parent_attr.mtime = Timestamp::now();
            parent_attr.ctime = Timestamp::now();
            let _ = self.inodes.set_inode(&parent_attr);
        }

        Ok(())
    }

    /// Remove a directory. Must be empty.
    pub fn rmdir(&self, parent: InodeId, name: &str) -> Result<(), MetaError> {
        let entry = self.dirs.lookup(parent, name)?;

        let attr = self.inodes.get_inode(entry.ino)?;
        if attr.file_type != FileType::Directory {
            return Err(MetaError::NotADirectory(entry.ino));
        }

        if !self.dirs.is_empty(entry.ino)? {
            return Err(MetaError::DirectoryNotEmpty(entry.ino));
        }

        self.dirs.delete_entry(parent, name)?;

        self.inodes.delete_inode(entry.ino)?;

        if let Ok(mut parent_attr) = self.inodes.get_inode(parent) {
            parent_attr.nlink = parent_attr.nlink.saturating_sub(1);
            parent_attr.mtime = Timestamp::now();
            parent_attr.ctime = Timestamp::now();
            let _ = self.inodes.set_inode(&parent_attr);
        }

        let _ = self
            .journal
            .append(MetaOp::DeleteInode { ino: entry.ino }, LogIndex::ZERO);

        Ok(())
    }

    /// Rename a file or directory.
    pub fn rename(
        &self,
        src_parent: InodeId,
        src_name: &str,
        dst_parent: InodeId,
        dst_name: &str,
    ) -> Result<(), MetaError> {
        self.dirs
            .rename(src_parent, src_name, dst_parent, dst_name)?;

        let _ = self.journal.append(
            MetaOp::Rename {
                src_parent,
                src_name: src_name.to_string(),
                dst_parent,
                dst_name: dst_name.to_string(),
            },
            LogIndex::ZERO,
        );

        Ok(())
    }

    /// Create a symbolic link pointing to `target`.
    pub fn symlink(
        &self,
        parent: InodeId,
        name: &str,
        target: &str,
        uid: u32,
        gid: u32,
    ) -> Result<InodeAttr, MetaError> {
        let ino = self.inodes.allocate_inode();
        let attr = InodeAttr::new_symlink(
            ino,
            uid,
            gid,
            0o777,
            self.config.site_id,
            target.to_string(),
        );

        self.inodes.create_inode(&attr)?;

        let entry = DirEntry {
            name: name.to_string(),
            ino,
            file_type: FileType::Symlink,
        };
        if let Err(e) = self.dirs.create_entry(parent, &entry) {
            let _ = self.inodes.delete_inode(ino);
            return Err(e);
        }

        let _ = self
            .journal
            .append(MetaOp::CreateInode { attr: attr.clone() }, LogIndex::ZERO);

        if let Ok(mut parent_attr) = self.inodes.get_inode(parent) {
            parent_attr.mtime = Timestamp::now();
            parent_attr.ctime = Timestamp::now();
            let _ = self.inodes.set_inode(&parent_attr);
        }

        Ok(attr)
    }

    /// Create a hard link to an existing inode.
    pub fn link(&self, parent: InodeId, name: &str, ino: InodeId) -> Result<InodeAttr, MetaError> {
        let mut attr = self.inodes.get_inode(ino)?;

        // Cannot hard-link directories (POSIX restriction)
        if attr.file_type == FileType::Directory {
            return Err(MetaError::PermissionDenied);
        }

        let entry = DirEntry {
            name: name.to_string(),
            ino,
            file_type: attr.file_type,
        };
        self.dirs.create_entry(parent, &entry)?;

        attr.nlink += 1;
        attr.ctime = Timestamp::now();
        self.inodes.set_inode(&attr)?;

        let _ = self.journal.append(
            MetaOp::Link {
                parent,
                name: name.to_string(),
                ino,
            },
            LogIndex::ZERO,
        );

        if let Ok(mut parent_attr) = self.inodes.get_inode(parent) {
            parent_attr.mtime = Timestamp::now();
            parent_attr.ctime = Timestamp::now();
            let _ = self.inodes.set_inode(&parent_attr);
        }

        Ok(attr)
    }

    /// Read the target of a symbolic link.
    pub fn readlink(&self, ino: InodeId) -> Result<String, MetaError> {
        let attr = self.inodes.get_inode(ino)?;
        if attr.file_type != FileType::Symlink {
            return Err(MetaError::NotADirectory(ino)); // EINVAL for non-symlink
        }
        attr.symlink_target
            .ok_or_else(|| MetaError::KvError("symlink target missing".to_string()))
    }

    /// Get a reference to the journal for replication.
    pub fn journal(&self) -> &Arc<MetadataJournal> {
        &self.journal
    }

    /// Get a reference to the replication tracker.
    pub fn replication(&self) -> &ReplicationTracker {
        &self.replication
    }

    /// Get the number of shards.
    pub fn num_shards(&self) -> u16 {
        self.config.num_shards
    }

    /// Determine which shard owns a given inode.
    pub fn shard_for_inode(&self, ino: InodeId) -> ShardId {
        ino.shard(self.config.num_shards)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_service() -> MetadataService {
        let svc = MetadataService::new(MetadataServiceConfig::default());
        svc.init_root().unwrap();
        svc
    }

    #[test]
    fn test_init_root() {
        let svc = make_service();
        let root = svc.getattr(InodeId::ROOT_INODE).unwrap();
        assert_eq!(root.file_type, FileType::Directory);
        assert_eq!(root.mode, 0o755);
    }

    #[test]
    fn test_create_file() {
        let svc = make_service();
        let attr = svc
            .create_file(InodeId::ROOT_INODE, "hello.txt", 1000, 1000, 0o644)
            .unwrap();
        assert_eq!(attr.file_type, FileType::RegularFile);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.mode, 0o644);
        assert_eq!(attr.nlink, 1);

        let found = svc.lookup(InodeId::ROOT_INODE, "hello.txt").unwrap();
        assert_eq!(found.ino, attr.ino);
    }

    #[test]
    fn test_mkdir() {
        let svc = make_service();
        let attr = svc
            .mkdir(InodeId::ROOT_INODE, "subdir", 1000, 1000, 0o755)
            .unwrap();
        assert_eq!(attr.file_type, FileType::Directory);
        assert_eq!(attr.nlink, 2);

        let root = svc.getattr(InodeId::ROOT_INODE).unwrap();
        assert_eq!(root.nlink, 3);
    }

    #[test]
    fn test_readdir() {
        let svc = make_service();
        svc.create_file(InodeId::ROOT_INODE, "a.txt", 1000, 1000, 0o644)
            .unwrap();
        svc.create_file(InodeId::ROOT_INODE, "b.txt", 1000, 1000, 0o644)
            .unwrap();
        svc.mkdir(InodeId::ROOT_INODE, "c_dir", 1000, 1000, 0o755)
            .unwrap();

        let entries = svc.readdir(InodeId::ROOT_INODE).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_unlink() {
        let svc = make_service();
        svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        svc.unlink(InodeId::ROOT_INODE, "file.txt").unwrap();

        match svc.lookup(InodeId::ROOT_INODE, "file.txt") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_rmdir() {
        let svc = make_service();
        svc.mkdir(InodeId::ROOT_INODE, "empty_dir", 1000, 1000, 0o755)
            .unwrap();
        svc.rmdir(InodeId::ROOT_INODE, "empty_dir").unwrap();

        match svc.lookup(InodeId::ROOT_INODE, "empty_dir") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_rmdir_not_empty() {
        let svc = make_service();
        let dir = svc
            .mkdir(InodeId::ROOT_INODE, "dir", 1000, 1000, 0o755)
            .unwrap();
        svc.create_file(dir.ino, "file.txt", 1000, 1000, 0o644)
            .unwrap();

        match svc.rmdir(InodeId::ROOT_INODE, "dir") {
            Err(MetaError::DirectoryNotEmpty(_)) => {}
            other => panic!("expected DirectoryNotEmpty, got {:?}", other),
        }
    }

    #[test]
    fn test_rename() {
        let svc = make_service();
        let attr = svc
            .create_file(InodeId::ROOT_INODE, "old.txt", 1000, 1000, 0o644)
            .unwrap();
        svc.rename(
            InodeId::ROOT_INODE,
            "old.txt",
            InodeId::ROOT_INODE,
            "new.txt",
        )
        .unwrap();

        match svc.lookup(InodeId::ROOT_INODE, "old.txt") {
            Err(MetaError::EntryNotFound { .. }) => {}
            other => panic!("expected EntryNotFound, got {:?}", other),
        }

        let found = svc.lookup(InodeId::ROOT_INODE, "new.txt").unwrap();
        assert_eq!(found.ino, attr.ino);
    }

    #[test]
    fn test_setattr() {
        let svc = make_service();
        let attr = svc
            .create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();

        let mut new_attr = attr.clone();
        new_attr.mode = 0o755;
        new_attr.size = 4096;
        svc.setattr(attr.ino, new_attr).unwrap();

        let updated = svc.getattr(attr.ino).unwrap();
        assert_eq!(updated.mode, 0o755);
        assert_eq!(updated.size, 4096);
    }

    #[test]
    fn test_journal_records_operations() {
        let svc = make_service();
        assert_eq!(svc.journal().latest_sequence().unwrap(), 0);

        svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        assert!(svc.journal().latest_sequence().unwrap() >= 2);
    }

    #[test]
    fn test_shard_for_inode() {
        let svc = make_service();
        let shard = svc.shard_for_inode(InodeId::new(42));
        assert_eq!(shard, InodeId::new(42).shard(256));
    }

    #[test]
    fn test_nested_directories() {
        let svc = make_service();

        let d1 = svc
            .mkdir(InodeId::ROOT_INODE, "a", 1000, 1000, 0o755)
            .unwrap();
        let d2 = svc.mkdir(d1.ino, "b", 1000, 1000, 0o755).unwrap();
        let _f = svc
            .create_file(d2.ino, "file.txt", 1000, 1000, 0o644)
            .unwrap();

        let a = svc.lookup(InodeId::ROOT_INODE, "a").unwrap();
        let b = svc.lookup(a.ino, "b").unwrap();
        let f = svc.lookup(b.ino, "file.txt").unwrap();
        assert_eq!(f.file_type, FileType::RegularFile);
    }

    #[test]
    fn test_duplicate_file() {
        let svc = make_service();
        svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();

        match svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644) {
            Err(MetaError::EntryExists { .. }) => {}
            other => panic!("expected EntryExists, got {:?}", other),
        }
    }

    #[test]
    fn test_symlink() {
        let svc = make_service();
        let attr = svc
            .symlink(InodeId::ROOT_INODE, "link", "/tmp/target", 1000, 1000)
            .unwrap();
        assert_eq!(attr.file_type, FileType::Symlink);
        assert_eq!(attr.symlink_target, Some("/tmp/target".to_string()));
        assert_eq!(attr.size, 11); // length of "/tmp/target"
        assert_eq!(attr.mode, 0o777);

        // Look it up
        let found = svc.lookup(InodeId::ROOT_INODE, "link").unwrap();
        assert_eq!(found.ino, attr.ino);
        assert_eq!(found.file_type, FileType::Symlink);
    }

    #[test]
    fn test_readlink() {
        let svc = make_service();
        let attr = svc
            .symlink(InodeId::ROOT_INODE, "link", "/etc/hosts", 1000, 1000)
            .unwrap();
        let target = svc.readlink(attr.ino).unwrap();
        assert_eq!(target, "/etc/hosts");
    }

    #[test]
    fn test_readlink_not_symlink() {
        let svc = make_service();
        let file = svc
            .create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        assert!(svc.readlink(file.ino).is_err());
    }

    #[test]
    fn test_hard_link() {
        let svc = make_service();
        let file = svc
            .create_file(InodeId::ROOT_INODE, "original.txt", 1000, 1000, 0o644)
            .unwrap();
        assert_eq!(file.nlink, 1);

        let linked = svc.link(InodeId::ROOT_INODE, "link.txt", file.ino).unwrap();
        assert_eq!(linked.nlink, 2);
        assert_eq!(linked.ino, file.ino);

        // Both names resolve to same inode
        let orig = svc.lookup(InodeId::ROOT_INODE, "original.txt").unwrap();
        let link = svc.lookup(InodeId::ROOT_INODE, "link.txt").unwrap();
        assert_eq!(orig.ino, link.ino);
    }

    #[test]
    fn test_hard_link_directory_denied() {
        let svc = make_service();
        let dir = svc
            .mkdir(InodeId::ROOT_INODE, "subdir", 1000, 1000, 0o755)
            .unwrap();

        match svc.link(InodeId::ROOT_INODE, "dir_link", dir.ino) {
            Err(MetaError::PermissionDenied) => {}
            other => panic!("expected PermissionDenied, got {:?}", other),
        }
    }

    #[test]
    fn test_unlink_hard_link() {
        let svc = make_service();
        let file = svc
            .create_file(InodeId::ROOT_INODE, "a.txt", 1000, 1000, 0o644)
            .unwrap();
        svc.link(InodeId::ROOT_INODE, "b.txt", file.ino).unwrap();

        // Unlinking one name reduces nlink but doesn't delete inode
        svc.unlink(InodeId::ROOT_INODE, "a.txt").unwrap();
        let remaining = svc.getattr(file.ino).unwrap();
        assert_eq!(remaining.nlink, 1);

        // Unlinking last name deletes the inode
        svc.unlink(InodeId::ROOT_INODE, "b.txt").unwrap();
        assert!(svc.getattr(file.ino).is_err());
    }
}
