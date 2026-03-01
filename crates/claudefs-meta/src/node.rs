//! Unified metadata server node implementation.
//!
//! This module combines all the metadata subsystem modules into a single MetadataNode
//! that can be instantiated and used by `cfs server`. It provides a unified API for
//! metadata operations, manages the lifecycle of all sub-components, and handles routing.

use std::path::PathBuf;
use std::sync::Arc;

use crate::fingerprint::FingerprintIndex;
use crate::kvstore::{KvStore, MemoryKvStore};
use crate::raft_log::RaftLogStore;
use crate::service::MetadataServiceConfig;
use crate::*;

#[cfg(test)]
use tempfile::TempDir;

/// Configuration for the MetadataNode.
pub struct MetadataNodeConfig {
    /// This node's unique identifier.
    pub node_id: NodeId,
    /// Number of virtual shards (default 256 per D4).
    pub num_shards: u16,
    /// Raft replication factor (default 3).
    pub replication_factor: usize,
    /// Site ID for cross-site replication.
    pub site_id: u64,
    /// Data directory for persistent storage (None = in-memory).
    pub data_dir: Option<PathBuf>,
    /// Directory shard config.
    pub dir_shard_config: DirShardConfig,
}

impl Default for MetadataNodeConfig {
    fn default() -> Self {
        Self {
            node_id: NodeId::new(1),
            num_shards: 256,
            replication_factor: 3,
            site_id: 1,
            data_dir: None,
            dir_shard_config: DirShardConfig::default(),
        }
    }
}

/// Unified metadata server node.
///
/// Combines KV store, inode operations, directory operations, Raft consensus,
/// and all managers into a cohesive API for use by FUSE clients, NFS gateways, etc.
#[allow(dead_code)]
pub struct MetadataNode {
    config: MetadataNodeConfig,
    kv: Arc<dyn KvStore>,
    service: MetadataService,
    shard_router: ShardRouter,
    lease_mgr: LeaseManager,
    lock_mgr: LockManager,
    fh_mgr: FileHandleManager,
    quota_mgr: QuotaManager,
    metrics: MetricsCollector,
    watch_mgr: WatchManager,
    dir_shard_mgr: DirShardManager,
    xattr_store: XattrStore,
    scaling_mgr: ScalingManager,
    fingerprint_idx: FingerprintIndex,
    worm_mgr: WormManager,
    cdc_stream: CdcStream,
    raft_log: RaftLogStore,
}

impl MetadataNode {
    /// Creates and initializes a MetadataNode with the given configuration.
    ///
    /// If data_dir is Some, uses PersistentKvStore; otherwise MemoryKvStore.
    pub fn new(config: MetadataNodeConfig) -> Result<Self, MetaError> {
        let kv: Arc<dyn KvStore> = if let Some(ref data_dir) = config.data_dir {
            let persistent = PersistentKvStore::open(data_dir)?;
            Arc::new(persistent)
        } else {
            Arc::new(MemoryKvStore::new())
        };

        let service_config = MetadataServiceConfig {
            node_id: config.node_id,
            peers: Vec::new(),
            site_id: config.site_id,
            num_shards: config.num_shards,
            max_journal_entries: 100_000,
        };
        let service = MetadataService::new(service_config);

        service.init_root()?;

        let raft_log = RaftLogStore::new(kv.clone());

        let shard_router = ShardRouter::new(config.num_shards);

        let lease_mgr = LeaseManager::new(30);

        let lock_mgr = LockManager::new();

        let fh_mgr = FileHandleManager::new();

        let quota_mgr = QuotaManager::new();

        let metrics = MetricsCollector::new();

        let watch_mgr = WatchManager::new(1000);

        let dir_shard_mgr = DirShardManager::new(config.dir_shard_config.clone());

        let xattr_store = XattrStore::new(kv.clone());

        let scaling_mgr = ScalingManager::new(config.num_shards, config.replication_factor);

        let fingerprint_idx = FingerprintIndex::new();

        let worm_mgr = WormManager::new();

        let cdc_stream = CdcStream::new(10000);

        Ok(Self {
            config,
            kv,
            service,
            shard_router,
            lease_mgr,
            lock_mgr,
            fh_mgr,
            quota_mgr,
            metrics,
            watch_mgr,
            dir_shard_mgr,
            xattr_store,
            scaling_mgr,
            fingerprint_idx,
            worm_mgr,
            cdc_stream,
            raft_log,
        })
    }

    /// Returns the node's ID.
    pub fn node_id(&self) -> NodeId {
        self.config.node_id
    }

    /// Returns the number of shards.
    pub fn num_shards(&self) -> u16 {
        self.config.num_shards
    }

    /// Creates a regular file.
    pub fn create_file(
        &self,
        parent: InodeId,
        name: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<InodeAttr, MetaError> {
        self.service.create_file(parent, name, uid, gid, mode)
    }

    /// Creates a directory.
    pub fn mkdir(
        &self,
        parent: InodeId,
        name: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<InodeAttr, MetaError> {
        self.service.mkdir(parent, name, uid, gid, mode)
    }

    /// Looks up a directory entry.
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<DirEntry, MetaError> {
        let attr = self.service.lookup(parent, name)?;
        Ok(DirEntry {
            name: name.to_string(),
            ino: attr.ino,
            file_type: attr.file_type,
        })
    }

    /// Gets inode attributes.
    pub fn getattr(&self, ino: InodeId) -> Result<InodeAttr, MetaError> {
        self.service.getattr(ino)
    }

    /// Sets inode attributes.
    pub fn setattr(&self, ino: InodeId, attr: InodeAttr) -> Result<(), MetaError> {
        self.service.setattr(ino, attr)
    }

    /// Lists directory entries.
    pub fn readdir(&self, dir: InodeId) -> Result<Vec<DirEntry>, MetaError> {
        self.service.readdir(dir)
    }

    /// Removes a file.
    pub fn unlink(&self, parent: InodeId, name: &str) -> Result<(), MetaError> {
        self.service.unlink(parent, name)
    }

    /// Removes an empty directory.
    pub fn rmdir(&self, parent: InodeId, name: &str) -> Result<(), MetaError> {
        self.service.rmdir(parent, name)
    }

    /// Renames a directory entry.
    pub fn rename(
        &self,
        src_parent: InodeId,
        src_name: &str,
        dst_parent: InodeId,
        dst_name: &str,
    ) -> Result<(), MetaError> {
        self.service
            .rename(src_parent, src_name, dst_parent, dst_name)
    }

    /// Opens a file handle.
    pub fn open(&self, ino: InodeId, client_id: u64, flags: u32) -> Result<u64, MetaError> {
        let open_flags = OpenFlags::from_raw(flags);
        let fh = self.fh_mgr.open(ino, NodeId::new(client_id), open_flags);
        Ok(fh)
    }

    /// Closes a file handle.
    pub fn close(&self, fh: u64) -> Result<(), MetaError> {
        self.fh_mgr.close(fh).map(|_| ())
    }

    /// Returns a snapshot of current metrics.
    pub fn metrics_snapshot(&self) -> MetadataMetrics {
        let active_leases = self.lease_mgr.active_lease_count() as u64;
        let active_watches = self.watch_mgr.watch_count() as u64;
        let active_file_handles = self.fh_mgr.open_count() as u64;
        let inode_count = self.inode_count();

        self.metrics.snapshot(
            active_leases,
            active_watches,
            active_file_handles,
            inode_count,
        )
    }

    /// Routes an inode to its owning shard.
    pub fn route_inode(&self, ino: InodeId) -> ShardId {
        self.shard_router.shard_for_inode(ino)
    }

    /// Returns the total inode count.
    pub fn inode_count(&self) -> u64 {
        let root = self.service.getattr(InodeId::ROOT_INODE);
        match root {
            Ok(_attr) => {
                let entries = self
                    .service
                    .readdir(InodeId::ROOT_INODE)
                    .unwrap_or_default();
                let mut count = 1u64;
                for entry in &entries {
                    if let Ok(eattr) = self.service.getattr(entry.ino) {
                        if eattr.file_type == FileType::Directory {
                            count += 1;
                        }
                    }
                }
                count
            }
            Err(_) => 0,
        }
    }

    /// Returns whether the node is healthy.
    pub fn is_healthy(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_memory() -> MetadataNode {
        let config = MetadataNodeConfig {
            node_id: NodeId::new(1),
            num_shards: 256,
            replication_factor: 3,
            site_id: 1,
            data_dir: None,
            dir_shard_config: DirShardConfig::default(),
        };
        MetadataNode::new(config).unwrap()
    }

    #[test]
    fn test_node_creation_memory() {
        let node = make_node_memory();
        assert_eq!(node.node_id(), NodeId::new(1));
        assert_eq!(node.num_shards(), 256);
    }

    #[test]
    fn test_node_creation_persistent() {
        let temp_dir = TempDir::new().unwrap();
        let config = MetadataNodeConfig {
            node_id: NodeId::new(1),
            num_shards: 256,
            replication_factor: 3,
            site_id: 1,
            data_dir: Some(temp_dir.path().to_path_buf()),
            dir_shard_config: DirShardConfig::default(),
        };
        let node = MetadataNode::new(config).unwrap();
        assert_eq!(node.node_id(), NodeId::new(1));
    }

    #[test]
    fn test_create_file() {
        let node = make_node_memory();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        assert_eq!(attr.file_type, FileType::RegularFile);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.mode, 0o644);
    }

    #[test]
    fn test_mkdir() {
        let node = make_node_memory();
        let attr = node
            .mkdir(InodeId::ROOT_INODE, "testdir", 1000, 1000, 0o755)
            .unwrap();
        assert_eq!(attr.file_type, FileType::Directory);
    }

    #[test]
    fn test_lookup() {
        let node = make_node_memory();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        let entry = node.lookup(InodeId::ROOT_INODE, "test.txt").unwrap();
        assert_eq!(entry.name, "test.txt");
    }

    #[test]
    fn test_readdir() {
        let node = make_node_memory();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "a.txt", 1000, 1000, 0o644)
            .unwrap();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "b.txt", 1000, 1000, 0o644)
            .unwrap();
        let _ = node
            .mkdir(InodeId::ROOT_INODE, "c_dir", 1000, 1000, 0o755)
            .unwrap();

        let entries = node.readdir(InodeId::ROOT_INODE).unwrap();
        assert!(entries.len() >= 3);
    }

    #[test]
    fn test_unlink() {
        let node = make_node_memory();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "file.txt", 1000, 1000, 0o644)
            .unwrap();
        node.unlink(InodeId::ROOT_INODE, "file.txt").unwrap();

        let result = node.lookup(InodeId::ROOT_INODE, "file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_rmdir() {
        let node = make_node_memory();
        let _ = node
            .mkdir(InodeId::ROOT_INODE, "empty_dir", 1000, 1000, 0o755)
            .unwrap();
        node.rmdir(InodeId::ROOT_INODE, "empty_dir").unwrap();

        let result = node.lookup(InodeId::ROOT_INODE, "empty_dir");
        assert!(result.is_err());
    }

    #[test]
    fn test_rename() {
        let node = make_node_memory();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "old.txt", 1000, 1000, 0o644)
            .unwrap();
        node.rename(
            InodeId::ROOT_INODE,
            "old.txt",
            InodeId::ROOT_INODE,
            "new.txt",
        )
        .unwrap();

        let result = node.lookup(InodeId::ROOT_INODE, "old.txt");
        assert!(result.is_err());

        let entry = node.lookup(InodeId::ROOT_INODE, "new.txt").unwrap();
        assert_eq!(entry.name, "new.txt");
    }

    #[test]
    fn test_open_close() {
        let node = make_node_memory();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        let fh = node.open(attr.ino, 1, 0x01).unwrap();
        assert!(fh > 0);

        node.close(fh).unwrap();
    }

    #[test]
    fn test_metrics_snapshot() {
        let node = make_node_memory();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        let metrics = node.metrics_snapshot();
        assert!(metrics.inode_count >= 1);
    }

    #[test]
    fn test_route_inode() {
        let node = make_node_memory();
        let shard = node.route_inode(InodeId::new(42));
        assert_eq!(shard, ShardId::new(42));
    }

    #[test]
    fn test_inode_count() {
        let node = make_node_memory();
        let count = node.inode_count();
        assert!(count >= 1);
    }

    #[test]
    fn test_is_healthy() {
        let node = make_node_memory();
        assert!(node.is_healthy());
    }
}
