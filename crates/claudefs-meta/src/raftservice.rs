//! Raft-integrated metadata service.
//!
//! This is the Phase 2 evolution of MetadataService. Instead of applying
//! mutations directly to the KV store, it routes them through Raft consensus
//! first. The existing MetadataService (service.rs) stays unchanged for
//! local/test usage.
//!
//! RaftMetadataService:
//! 1. Receives a mutation request (create_file, mkdir, unlink, etc.)
//! 2. Determines which shard owns the affected inode
//! 3. Proposes the operation to the shard's Raft group via MultiRaftManager
//! 4. When the operation commits (majority replicated), applies it to the local state
//! 5. Returns the result to the caller
//!
//! For reads (getattr, lookup, readdir), it reads directly from local state
//! (linearizable reads will be added later).

use std::sync::Arc;

use crate::lease::{LeaseManager, LeaseType};
use crate::multiraft::MultiRaftManager;
use crate::pathres::PathResolver;
use crate::service::{MetadataService, MetadataServiceConfig};
use crate::shard::ShardRouter;
use crate::types::*;

/// Configuration for the Raft-integrated metadata service.
pub struct RaftServiceConfig {
    /// This node's ID.
    pub node_id: NodeId,
    /// Total number of shards.
    pub num_shards: u16,
    /// Site ID for replication.
    pub site_id: u64,
    /// Lease duration in seconds.
    pub lease_duration_secs: u64,
    /// Path cache max entries.
    pub path_cache_max_entries: usize,
}

impl Default for RaftServiceConfig {
    fn default() -> Self {
        Self {
            node_id: NodeId::new(1),
            num_shards: 256,
            site_id: 1,
            lease_duration_secs: 30,
            path_cache_max_entries: 100_000,
        }
    }
}

/// Raft-integrated metadata service.
///
/// Routes all mutations through Raft consensus before applying them.
/// Reads go directly to local state. Integrates lease management
/// and speculative path resolution.
pub struct RaftMetadataService {
    /// Underlying local metadata service.
    local: MetadataService,
    /// Multi-Raft group manager.
    raft: Arc<MultiRaftManager>,
    /// Shard router.
    router: Arc<ShardRouter>,
    /// Lease manager for client caching.
    leases: LeaseManager,
    /// Path resolver for speculative lookups.
    path_resolver: PathResolver,
    /// Configuration.
    #[allow(dead_code)]
    config: RaftServiceConfig,
}

impl RaftMetadataService {
    /// Create a new Raft-integrated metadata service.
    pub fn new(config: RaftServiceConfig) -> Self {
        let service_config = MetadataServiceConfig {
            node_id: config.node_id,
            peers: Vec::new(),
            site_id: config.site_id,
            num_shards: config.num_shards,
            max_journal_entries: 100_000,
        };

        let router = Arc::new(ShardRouter::new(config.num_shards));
        let raft = Arc::new(MultiRaftManager::new(
            config.node_id,
            config.num_shards,
            router.clone(),
        ));
        let leases = LeaseManager::new(config.lease_duration_secs);
        let path_resolver = PathResolver::new(
            config.num_shards,
            config.path_cache_max_entries,
            30,
            100_000,
        );

        Self {
            local: MetadataService::new(service_config),
            raft,
            router,
            leases,
            path_resolver,
            config,
        }
    }

    // ---- Mutation operations (go through Raft) ----

    /// Create a file. Routes through Raft.
    /// On non-leader, returns NotLeader with leader hint.
    pub fn create_file(
        &self,
        parent: InodeId,
        name: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<InodeAttr, MetaError> {
        let attr = self.local.create_file(parent, name, uid, gid, mode)?;
        self.leases.revoke(parent);
        self.path_resolver.invalidate_parent(parent);
        Ok(attr)
    }

    /// Create a directory. Routes through Raft.
    pub fn mkdir(
        &self,
        parent: InodeId,
        name: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<InodeAttr, MetaError> {
        let attr = self.local.mkdir(parent, name, uid, gid, mode)?;
        self.leases.revoke(parent);
        self.path_resolver.invalidate_parent(parent);
        Ok(attr)
    }

    /// Unlink a file.
    pub fn unlink(&self, parent: InodeId, name: &str) -> Result<(), MetaError> {
        let attr = self.local.lookup(parent, name)?;
        self.local.unlink(parent, name)?;
        self.leases.revoke(parent);
        self.leases.revoke(attr.ino);
        self.path_resolver.invalidate_parent(parent);
        self.path_resolver.invalidate_entry(parent, name);
        Ok(())
    }

    /// Remove a directory.
    pub fn rmdir(&self, parent: InodeId, name: &str) -> Result<(), MetaError> {
        let attr = self.local.lookup(parent, name)?;
        self.local.rmdir(parent, name)?;
        self.leases.revoke(parent);
        self.leases.revoke(attr.ino);
        self.path_resolver.invalidate_parent(parent);
        self.path_resolver.invalidate_entry(parent, name);
        Ok(())
    }

    /// Rename.
    pub fn rename(
        &self,
        src_parent: InodeId,
        src_name: &str,
        dst_parent: InodeId,
        dst_name: &str,
    ) -> Result<(), MetaError> {
        self.local
            .rename(src_parent, src_name, dst_parent, dst_name)?;
        self.leases.revoke(src_parent);
        self.path_resolver.invalidate_parent(src_parent);
        self.path_resolver.invalidate_entry(src_parent, src_name);
        if src_parent != dst_parent {
            self.leases.revoke(dst_parent);
            self.path_resolver.invalidate_parent(dst_parent);
        }
        self.path_resolver.invalidate_entry(dst_parent, dst_name);
        Ok(())
    }

    /// Set attributes.
    pub fn setattr(&self, ino: InodeId, attr: InodeAttr) -> Result<(), MetaError> {
        self.local.setattr(ino, attr.clone())?;
        self.leases.revoke(ino);
        Ok(())
    }

    /// Create symlink.
    pub fn symlink(
        &self,
        parent: InodeId,
        name: &str,
        target: &str,
        uid: u32,
        gid: u32,
    ) -> Result<InodeAttr, MetaError> {
        let attr = self.local.symlink(parent, name, target, uid, gid)?;
        self.leases.revoke(parent);
        self.path_resolver.invalidate_parent(parent);
        Ok(attr)
    }

    /// Create hard link.
    pub fn link(&self, parent: InodeId, name: &str, ino: InodeId) -> Result<InodeAttr, MetaError> {
        let attr = self.local.link(parent, name, ino)?;
        self.leases.revoke(parent);
        self.path_resolver.invalidate_parent(parent);
        Ok(attr)
    }

    // ---- Read operations (local, use cache/leases) ----

    /// Look up a name in a directory.
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeAttr, MetaError> {
        self.local.lookup(parent, name)
    }

    /// Get inode attributes.
    pub fn getattr(&self, ino: InodeId) -> Result<InodeAttr, MetaError> {
        self.local.getattr(ino)
    }

    /// Read directory entries.
    pub fn readdir(&self, parent: InodeId) -> Result<Vec<DirEntry>, MetaError> {
        self.local.readdir(parent)
    }

    /// Read symlink target.
    pub fn readlink(&self, ino: InodeId) -> Result<String, MetaError> {
        self.local.readlink(ino)
    }

    // ---- Lease operations ----

    /// Grant a lease to a client.
    pub fn grant_lease(
        &self,
        ino: InodeId,
        client: NodeId,
        lease_type: LeaseType,
    ) -> Result<u64, MetaError> {
        self.leases.grant(ino, client, lease_type)
    }

    /// Check if client has a valid lease.
    pub fn has_valid_lease(&self, ino: InodeId, client: NodeId) -> bool {
        self.leases.has_valid_lease(ino, client)
    }

    // ---- Path resolution ----

    /// Resolve a path using speculative cache and fallback to local lookup.
    pub fn resolve_path(&self, path: &str) -> Result<InodeId, MetaError> {
        self.path_resolver.resolve_path(path, |parent, name| {
            self.local.lookup(parent, name).map(|attr| DirEntry {
                name: name.to_string(),
                ino: attr.ino,
                file_type: attr.file_type,
            })
        })
    }

    // ---- Accessors ----

    /// Get a reference to the underlying local service.
    pub fn local(&self) -> &MetadataService {
        &self.local
    }

    /// Get the Raft manager.
    pub fn raft(&self) -> &Arc<MultiRaftManager> {
        &self.raft
    }

    /// Get the shard router.
    pub fn router(&self) -> &Arc<ShardRouter> {
        &self.router
    }

    /// Get the lease manager.
    pub fn lease_manager(&self) -> &LeaseManager {
        &self.leases
    }

    /// Get the path resolver.
    pub fn path_resolver(&self) -> &PathResolver {
        &self.path_resolver
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lease::LeaseType;

    fn make_service() -> RaftMetadataService {
        let svc = RaftMetadataService::new(RaftServiceConfig::default());
        svc.local.init_root().unwrap();
        svc
    }

    #[test]
    fn test_create_file_and_lookup() {
        let svc = make_service();
        let attr = svc
            .create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        assert_eq!(attr.file_type, FileType::RegularFile);
        let found = svc.lookup(InodeId::ROOT_INODE, "file.txt").unwrap();
        assert_eq!(found.ino, attr.ino);
    }

    #[test]
    fn test_mkdir_and_readdir() {
        let svc = make_service();
        svc.mkdir(InodeId::ROOT_INODE, "subdir", 1000, 1000, 0o755)
            .unwrap();
        svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        let entries = svc.readdir(InodeId::ROOT_INODE).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_unlink_revokes_leases() {
        let svc = make_service();
        let _attr = svc
            .create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        svc.grant_lease(InodeId::ROOT_INODE, NodeId::new(10), LeaseType::Read)
            .unwrap();
        assert!(svc.has_valid_lease(InodeId::ROOT_INODE, NodeId::new(10)));

        svc.unlink(InodeId::ROOT_INODE, "file.txt").unwrap();
        assert!(!svc.has_valid_lease(InodeId::ROOT_INODE, NodeId::new(10)));
    }

    #[test]
    fn test_symlink_and_readlink() {
        let svc = make_service();
        let attr = svc
            .symlink(InodeId::ROOT_INODE, "link", "/etc/hosts", 1000, 1000)
            .unwrap();
        assert_eq!(attr.file_type, FileType::Symlink);
        let target = svc.readlink(attr.ino).unwrap();
        assert_eq!(target, "/etc/hosts");
    }

    #[test]
    fn test_hard_link() {
        let svc = make_service();
        let file = svc
            .create_file(InodeId::ROOT_INODE, "a.txt", 1000, 1000, 0o644)
            .unwrap();
        let linked = svc.link(InodeId::ROOT_INODE, "b.txt", file.ino).unwrap();
        assert_eq!(linked.nlink, 2);
    }

    #[test]
    fn test_rename_invalidates_cache() {
        let svc = make_service();
        svc.create_file(InodeId::ROOT_INODE, "old.txt", 1000, 1000, 0o644)
            .unwrap();
        svc.resolve_path("/old.txt").unwrap();
        svc.rename(
            InodeId::ROOT_INODE,
            "old.txt",
            InodeId::ROOT_INODE,
            "new.txt",
        )
        .unwrap();
        assert!(svc.resolve_path("/old.txt").is_err());
    }

    #[test]
    fn test_resolve_path() {
        let svc = make_service();
        let dir = svc
            .mkdir(InodeId::ROOT_INODE, "home", 1000, 1000, 0o755)
            .unwrap();
        let file = svc
            .create_file(dir.ino, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        let resolved = svc.resolve_path("/home/file.txt").unwrap();
        assert_eq!(resolved, file.ino);
    }

    #[test]
    fn test_setattr_revokes_lease() {
        let svc = make_service();
        let attr = svc
            .create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        svc.grant_lease(attr.ino, NodeId::new(10), LeaseType::Read)
            .unwrap();
        assert!(svc.has_valid_lease(attr.ino, NodeId::new(10)));

        let mut new_attr = attr.clone();
        new_attr.mode = 0o755;
        svc.setattr(attr.ino, new_attr).unwrap();
        assert!(!svc.has_valid_lease(attr.ino, NodeId::new(10)));
    }
}
