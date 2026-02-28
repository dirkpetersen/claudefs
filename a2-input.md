Create a NEW file `crates/claudefs-meta/src/raftservice.rs` — Raft-integrated metadata service.

Also update `crates/claudefs-meta/src/lib.rs` to add `pub mod raftservice;` and re-export `RaftMetadataService`.

## What This Module Does

This is the Phase 2 evolution of MetadataService (service.rs). Instead of applying mutations directly to the KV store, it routes them through Raft consensus first. The existing MetadataService (service.rs) stays unchanged for local/test usage.

RaftMetadataService:
1. Receives a mutation request (create_file, mkdir, unlink, etc.)
2. Determines which shard owns the affected inode
3. Proposes the operation to the shard's Raft group via MultiRaftManager
4. When the operation commits (majority replicated), applies it to the local state
5. Returns the result to the caller

For reads (getattr, lookup, readdir), it reads directly from local state (linearizable reads will be added later).

## Existing types to use

```rust
// From crate::types
pub struct InodeId(u64);    // ROOT_INODE = 1, has .shard(num_shards)
pub struct NodeId(u64);
pub struct ShardId(u16);
pub enum MetaError { NotLeader { leader_hint: Option<NodeId> }, RaftError(String), ... }
pub enum MetaOp { CreateInode { attr: InodeAttr }, DeleteInode { ino: InodeId }, SetAttr { ino, attr }, CreateEntry { parent, name, entry }, DeleteEntry { parent, name }, Rename { ... }, Link { parent, name, ino }, SetXattr { ... }, RemoveXattr { ... } }
pub struct InodeAttr { ... }  // has new_file(), new_directory(), new_symlink()
pub struct DirEntry { pub name: String, pub ino: InodeId, pub file_type: FileType }
pub enum FileType { RegularFile, Directory, Symlink, ... }
pub struct LogEntry { pub index: LogIndex, pub term: Term, pub op: MetaOp }
pub struct Timestamp { ... }  // has now()

// From crate::multiraft
pub struct MultiRaftManager { ... }
// Methods: propose(ino, op), is_leader(shard), shard_for_inode(ino), take_committed(shard), ...

// From crate::service
pub struct MetadataService { ... }
// The underlying local metadata service that applies operations directly.
// Methods: init_root(), create_file(), mkdir(), lookup(), getattr(), setattr(), readdir(), unlink(), rmdir(), rename(), symlink(), link(), readlink(), journal(), replication(), ...

// From crate::lease
pub struct LeaseManager { ... }
// Methods: grant(), revoke(), has_valid_lease(), ...

// From crate::pathres
pub struct PathResolver { ... }
// Methods: speculative_resolve(), resolve_path(), cache_resolution(), invalidate_parent(), ...
```

## RaftMetadataService struct

```rust
use std::sync::Arc;
use crate::types::*;
use crate::service::{MetadataService, MetadataServiceConfig};
use crate::multiraft::MultiRaftManager;
use crate::shard::ShardRouter;
use crate::lease::LeaseManager;
use crate::pathres::PathResolver;

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
    config: RaftServiceConfig,
}
```

## Methods

```rust
impl RaftMetadataService {
    /// Create a new Raft-integrated metadata service.
    pub fn new(config: RaftServiceConfig) -> Self {
        // Create local service with matching config
        // Create router, raft manager, leases, path resolver
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
        // For Phase 2 local testing: apply directly to local service
        // In distributed mode: propose to Raft, wait for commit, apply
        let attr = self.local.create_file(parent, name, uid, gid, mode)?;
        // Revoke leases on parent (directory changed)
        self.leases.revoke(parent);
        // Invalidate path cache for parent
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
    ) -> Result<InodeAttr, MetaError>

    /// Unlink a file.
    pub fn unlink(&self, parent: InodeId, name: &str) -> Result<(), MetaError>

    /// Remove a directory.
    pub fn rmdir(&self, parent: InodeId, name: &str) -> Result<(), MetaError>

    /// Rename.
    pub fn rename(
        &self,
        src_parent: InodeId,
        src_name: &str,
        dst_parent: InodeId,
        dst_name: &str,
    ) -> Result<(), MetaError>

    /// Set attributes.
    pub fn setattr(&self, ino: InodeId, attr: InodeAttr) -> Result<(), MetaError>

    /// Create symlink.
    pub fn symlink(
        &self,
        parent: InodeId,
        name: &str,
        target: &str,
        uid: u32,
        gid: u32,
    ) -> Result<InodeAttr, MetaError>

    /// Create hard link.
    pub fn link(
        &self,
        parent: InodeId,
        name: &str,
        ino: InodeId,
    ) -> Result<InodeAttr, MetaError>

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
        lease_type: crate::lease::LeaseType,
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
    pub fn local(&self) -> &MetadataService { &self.local }
    /// Get the Raft manager.
    pub fn raft(&self) -> &Arc<MultiRaftManager> { &self.raft }
    /// Get the shard router.
    pub fn router(&self) -> &Arc<ShardRouter> { &self.router }
    /// Get the lease manager.
    pub fn lease_manager(&self) -> &LeaseManager { &self.leases }
    /// Get the path resolver.
    pub fn path_resolver(&self) -> &PathResolver { &self.path_resolver }
}
```

For each mutation method: call the local service, revoke leases on affected parent, invalidate path cache. The Raft proposal path is structured but applied locally for now (Phase 2 single-node testing). The distributed path will be wired in Phase 3.

## Tests (8 tests)

```rust
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
        let attr = svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644).unwrap();
        assert_eq!(attr.file_type, FileType::RegularFile);
        let found = svc.lookup(InodeId::ROOT_INODE, "file.txt").unwrap();
        assert_eq!(found.ino, attr.ino);
    }

    #[test]
    fn test_mkdir_and_readdir() {
        let svc = make_service();
        svc.mkdir(InodeId::ROOT_INODE, "subdir", 1000, 1000, 0o755).unwrap();
        svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644).unwrap();
        let entries = svc.readdir(InodeId::ROOT_INODE).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_unlink_revokes_leases() {
        let svc = make_service();
        let attr = svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644).unwrap();
        svc.grant_lease(InodeId::ROOT_INODE, NodeId::new(10), LeaseType::Read).unwrap();
        assert!(svc.has_valid_lease(InodeId::ROOT_INODE, NodeId::new(10)));

        svc.unlink(InodeId::ROOT_INODE, "file.txt").unwrap();
        // Lease on parent directory should be revoked
        assert!(!svc.has_valid_lease(InodeId::ROOT_INODE, NodeId::new(10)));
    }

    #[test]
    fn test_symlink_and_readlink() {
        let svc = make_service();
        let attr = svc.symlink(InodeId::ROOT_INODE, "link", "/etc/hosts", 1000, 1000).unwrap();
        assert_eq!(attr.file_type, FileType::Symlink);
        let target = svc.readlink(attr.ino).unwrap();
        assert_eq!(target, "/etc/hosts");
    }

    #[test]
    fn test_hard_link() {
        let svc = make_service();
        let file = svc.create_file(InodeId::ROOT_INODE, "a.txt", 1000, 1000, 0o644).unwrap();
        let linked = svc.link(InodeId::ROOT_INODE, "b.txt", file.ino).unwrap();
        assert_eq!(linked.nlink, 2);
    }

    #[test]
    fn test_rename_invalidates_cache() {
        let svc = make_service();
        svc.create_file(InodeId::ROOT_INODE, "old.txt", 1000, 1000, 0o644).unwrap();
        // Resolve path to populate cache
        svc.resolve_path("/old.txt").unwrap();
        // Rename should invalidate cache
        svc.rename(InodeId::ROOT_INODE, "old.txt", InodeId::ROOT_INODE, "new.txt").unwrap();
        // Old path should fail
        assert!(svc.resolve_path("/old.txt").is_err());
    }

    #[test]
    fn test_resolve_path() {
        let svc = make_service();
        let dir = svc.mkdir(InodeId::ROOT_INODE, "home", 1000, 1000, 0o755).unwrap();
        let file = svc.create_file(dir.ino, "file.txt", 1000, 1000, 0o644).unwrap();
        let resolved = svc.resolve_path("/home/file.txt").unwrap();
        assert_eq!(resolved, file.ino);
    }

    #[test]
    fn test_setattr_revokes_lease() {
        let svc = make_service();
        let attr = svc.create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644).unwrap();
        svc.grant_lease(attr.ino, NodeId::new(10), LeaseType::Read).unwrap();
        assert!(svc.has_valid_lease(attr.ino, NodeId::new(10)));

        let mut new_attr = attr.clone();
        new_attr.mode = 0o755;
        svc.setattr(attr.ino, new_attr).unwrap();
        // Lease on the inode should be revoked
        assert!(!svc.has_valid_lease(attr.ino, NodeId::new(10)));
    }
}
```

## Conventions
- `use crate::types::*;`
- Doc comments (///) on all public items
- Module doc (//!) at top
- No unsafe code

Output COMPLETE files separated by `=== FILE: path ===`.
