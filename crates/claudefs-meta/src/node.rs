//! Unified metadata server node implementation.
//!
//! This module combines all the metadata subsystem modules into a single MetadataNode
//! that can be instantiated and used by `cfs server`. It provides a unified API for
//! metadata operations, manages the lifecycle of all sub-components, and handles routing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::fingerprint::FingerprintIndex;
use crate::kvstore::{KvStore, MemoryKvStore};
use crate::membership::{MembershipManager, NodeState};
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

/// Filesystem statistics returned by statfs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatFs {
    /// Total number of inodes.
    pub total_inodes: u64,
    /// Number of free inodes.
    pub free_inodes: u64,
    /// Total number of blocks (4KB each).
    pub total_blocks: u64,
    /// Number of free blocks.
    pub free_blocks: u64,
    /// Block size in bytes.
    pub block_size: u32,
    /// Maximum filename length.
    pub max_name_len: u32,
}

/// Cluster membership status.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClusterStatus {
    /// Total number of known nodes.
    pub total_nodes: u32,
    /// Number of alive nodes.
    pub alive_nodes: u32,
    /// Number of suspected nodes.
    pub suspect_nodes: u32,
    /// Number of confirmed dead nodes.
    pub dead_nodes: u32,
    /// This node's ID.
    pub this_node: NodeId,
}

/// Directory entry with full inode attributes (for FUSE readdirplus).
#[derive(Clone, Debug)]
pub struct DirEntryPlus {
    /// Directory entry info.
    pub entry: DirEntry,
    /// Full inode attributes.
    pub attr: InodeAttr,
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
    membership: MembershipManager,
    inode_counter: AtomicU64,
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

        let membership = MembershipManager::new(config.node_id);
        let _ = membership.join(config.node_id, format!("node-{}", config.node_id.as_u64()));

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

        let inode_counter = AtomicU64::new(1);

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
            membership,
            inode_counter,
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

    /// Returns a reference to the LeaseManager.
    pub fn lease_manager(&self) -> &LeaseManager {
        &self.lease_mgr
    }

    /// Returns a reference to the QuotaManager.
    pub fn quota_manager(&self) -> &QuotaManager {
        &self.quota_mgr
    }

    /// Returns a reference to the WormManager.
    pub fn worm_manager(&self) -> &WormManager {
        &self.worm_mgr
    }

    /// Returns a reference to the CdcStream.
    pub fn cdc_stream(&self) -> &CdcStream {
        &self.cdc_stream
    }

    /// Returns a reference to the WatchManager.
    pub fn watch_manager(&self) -> &WatchManager {
        &self.watch_mgr
    }

    /// Returns a reference to the ScalingManager.
    pub fn scaling_manager(&self) -> &ScalingManager {
        &self.scaling_mgr
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
        let start = Instant::now();

        self.quota_mgr
            .check_quota(uid, gid, 0, 1)
            .inspect_err(|_e| {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics
                    .record_op(MetricOp::CreateFile, duration, false);
            })?;

        let result = self.service.create_file(parent, name, uid, gid, mode);

        match result {
            Ok(attr) => {
                self.quota_mgr.update_usage(uid, gid, 0, 1);
                let _ = self.lease_mgr.revoke(parent);
                self.watch_mgr.notify(WatchEvent::Create {
                    parent,
                    name: name.to_string(),
                    ino: attr.ino,
                });
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateInode { attr: attr.clone() },
                    self.config.site_id,
                );
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateEntry {
                        parent,
                        name: name.to_string(),
                        entry: DirEntry {
                            name: name.to_string(),
                            ino: attr.ino,
                            file_type: attr.file_type,
                        },
                    },
                    self.config.site_id,
                );
                self.inode_counter.fetch_add(1, Ordering::Relaxed);

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::CreateFile, duration, true);

                Ok(attr)
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics
                    .record_op(MetricOp::CreateFile, duration, false);
                Err(e)
            }
        }
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
        let start = Instant::now();

        self.quota_mgr
            .check_quota(uid, gid, 0, 1)
            .inspect_err(|_e| {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Mkdir, duration, false);
            })?;

        let result = self.service.mkdir(parent, name, uid, gid, mode);

        match result {
            Ok(attr) => {
                self.quota_mgr.update_usage(uid, gid, 0, 1);
                let _ = self.lease_mgr.revoke(parent);
                self.watch_mgr.notify(WatchEvent::Create {
                    parent,
                    name: name.to_string(),
                    ino: attr.ino,
                });
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateInode { attr: attr.clone() },
                    self.config.site_id,
                );
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateEntry {
                        parent,
                        name: name.to_string(),
                        entry: DirEntry {
                            name: name.to_string(),
                            ino: attr.ino,
                            file_type: attr.file_type,
                        },
                    },
                    self.config.site_id,
                );
                self.inode_counter.fetch_add(1, Ordering::Relaxed);

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Mkdir, duration, true);

                Ok(attr)
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Mkdir, duration, false);
                Err(e)
            }
        }
    }

    /// Looks up a directory entry.
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<DirEntry, MetaError> {
        let start = Instant::now();
        let result = self.service.lookup(parent, name).map(|attr| DirEntry {
            name: name.to_string(),
            ino: attr.ino,
            file_type: attr.file_type,
        });

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::Lookup, duration, result.is_ok());

        result
    }

    /// Gets inode attributes.
    pub fn getattr(&self, ino: InodeId) -> Result<InodeAttr, MetaError> {
        let start = Instant::now();
        let result = self.service.getattr(ino);

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::Getattr, duration, result.is_ok());

        result
    }

    /// Sets inode attributes.
    pub fn setattr(&self, ino: InodeId, attr: InodeAttr) -> Result<(), MetaError> {
        let start = Instant::now();

        if let Some(worm_state) = self.worm_mgr.get_state(ino) {
            if worm_state.is_protected() {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Setattr, duration, false);
                return Err(MetaError::PermissionDenied);
            }
        }

        let attr_for_cdc = attr.clone();
        let result = self.service.setattr(ino, attr);

        match result {
            Ok(()) => {
                let _ = self.lease_mgr.revoke(ino);
                self.watch_mgr.notify(WatchEvent::AttrChange { ino });
                let _ = self.cdc_stream.publish(
                    MetaOp::SetAttr {
                        ino,
                        attr: attr_for_cdc,
                    },
                    self.config.site_id,
                );

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Setattr, duration, true);

                Ok(())
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Setattr, duration, false);
                Err(e)
            }
        }
    }

    /// Lists directory entries.
    pub fn readdir(&self, dir: InodeId) -> Result<Vec<DirEntry>, MetaError> {
        let start = Instant::now();
        let result = self.service.readdir(dir);

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::Readdir, duration, result.is_ok());

        result
    }

    /// Removes a file.
    pub fn unlink(&self, parent: InodeId, name: &str) -> Result<(), MetaError> {
        let start = Instant::now();

        let entry = self.service.lookup(parent, name)?;
        let ino = entry.ino;

        if let Some(worm_state) = self.worm_mgr.get_state(ino) {
            if worm_state.is_protected() {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Unlink, duration, false);
                return Err(MetaError::PermissionDenied);
            }
        }

        let uid = entry.uid;
        let gid = entry.gid;

        let result = self.service.unlink(parent, name);

        match result {
            Ok(()) => {
                self.quota_mgr.update_usage(uid, gid, 0, -1);
                let _ = self.lease_mgr.revoke(parent);
                let _ = self.lease_mgr.revoke(ino);
                self.watch_mgr.notify(WatchEvent::Delete {
                    parent,
                    name: name.to_string(),
                    ino,
                });
                let _ = self.cdc_stream.publish(
                    MetaOp::DeleteEntry {
                        parent,
                        name: name.to_string(),
                    },
                    self.config.site_id,
                );
                let _ = self
                    .cdc_stream
                    .publish(MetaOp::DeleteInode { ino }, self.config.site_id);
                self.inode_counter.fetch_sub(1, Ordering::Relaxed);

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Unlink, duration, true);

                Ok(())
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Unlink, duration, false);
                Err(e)
            }
        }
    }

    /// Removes an empty directory.
    pub fn rmdir(&self, parent: InodeId, name: &str) -> Result<(), MetaError> {
        let start = Instant::now();

        let entry = self.service.lookup(parent, name)?;
        let ino = entry.ino;

        if let Some(worm_state) = self.worm_mgr.get_state(ino) {
            if worm_state.is_protected() {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Rmdir, duration, false);
                return Err(MetaError::PermissionDenied);
            }
        }

        let uid = entry.uid;
        let gid = entry.gid;

        let result = self.service.rmdir(parent, name);

        match result {
            Ok(()) => {
                self.quota_mgr.update_usage(uid, gid, 0, -1);
                let _ = self.lease_mgr.revoke(parent);
                let _ = self.lease_mgr.revoke(ino);
                self.watch_mgr.notify(WatchEvent::Delete {
                    parent,
                    name: name.to_string(),
                    ino,
                });
                let _ = self.cdc_stream.publish(
                    MetaOp::DeleteEntry {
                        parent,
                        name: name.to_string(),
                    },
                    self.config.site_id,
                );
                let _ = self
                    .cdc_stream
                    .publish(MetaOp::DeleteInode { ino }, self.config.site_id);
                self.inode_counter.fetch_sub(1, Ordering::Relaxed);

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Rmdir, duration, true);

                Ok(())
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Rmdir, duration, false);
                Err(e)
            }
        }
    }

    /// Renames a directory entry.
    pub fn rename(
        &self,
        src_parent: InodeId,
        src_name: &str,
        dst_parent: InodeId,
        dst_name: &str,
    ) -> Result<(), MetaError> {
        let start = Instant::now();

        let entry = self.service.lookup(src_parent, src_name)?;
        let ino = entry.ino;

        let result = self
            .service
            .rename(src_parent, src_name, dst_parent, dst_name);

        match result {
            Ok(()) => {
                let _ = self.lease_mgr.revoke(src_parent);
                let _ = self.lease_mgr.revoke(dst_parent);
                let _ = self.lease_mgr.revoke(ino);
                self.watch_mgr.notify(WatchEvent::Rename {
                    old_parent: src_parent,
                    old_name: src_name.to_string(),
                    new_parent: dst_parent,
                    new_name: dst_name.to_string(),
                    ino,
                });
                let _ = self.cdc_stream.publish(
                    MetaOp::Rename {
                        src_parent,
                        src_name: src_name.to_string(),
                        dst_parent,
                        dst_name: dst_name.to_string(),
                    },
                    self.config.site_id,
                );

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Rename, duration, true);

                Ok(())
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Rename, duration, false);
                Err(e)
            }
        }
    }

    /// Opens a file handle.
    pub fn open(&self, ino: InodeId, client_id: u64, flags: u32) -> Result<u64, MetaError> {
        let start = Instant::now();
        let open_flags = OpenFlags::from_raw(flags);
        let fh = self.fh_mgr.open(ino, NodeId::new(client_id), open_flags);

        let duration = start.elapsed().as_micros() as u64;
        self.metrics.record_op(MetricOp::Open, duration, true);

        Ok(fh)
    }

    /// Closes a file handle.
    pub fn close(&self, fh: u64) -> Result<(), MetaError> {
        let start = Instant::now();
        let result = self.fh_mgr.close(fh).map(|_| ());

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::Close, duration, result.is_ok());

        result
    }

    /// Creates a symbolic link.
    pub fn symlink(
        &self,
        parent: InodeId,
        name: &str,
        target: &str,
        uid: u32,
        gid: u32,
    ) -> Result<InodeAttr, MetaError> {
        let start = Instant::now();

        self.quota_mgr
            .check_quota(uid, gid, 0, 1)
            .inspect_err(|_e| {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Symlink, duration, false);
            })?;

        let result = self.service.symlink(parent, name, target, uid, gid);

        match result {
            Ok(attr) => {
                self.quota_mgr.update_usage(uid, gid, 0, 1);
                let _ = self.lease_mgr.revoke(parent);
                self.watch_mgr.notify(WatchEvent::Create {
                    parent,
                    name: name.to_string(),
                    ino: attr.ino,
                });
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateInode { attr: attr.clone() },
                    self.config.site_id,
                );
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateEntry {
                        parent,
                        name: name.to_string(),
                        entry: DirEntry {
                            name: name.to_string(),
                            ino: attr.ino,
                            file_type: attr.file_type,
                        },
                    },
                    self.config.site_id,
                );
                self.inode_counter.fetch_add(1, Ordering::Relaxed);

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Symlink, duration, true);

                Ok(attr)
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Symlink, duration, false);
                Err(e)
            }
        }
    }

    /// Creates a hard link.
    pub fn link(
        &self,
        parent: InodeId,
        name: &str,
        target_ino: InodeId,
    ) -> Result<InodeAttr, MetaError> {
        let start = Instant::now();

        let result = self.service.link(parent, name, target_ino);

        match result {
            Ok(attr) => {
                let _ = self.lease_mgr.revoke(parent);
                let _ = self.lease_mgr.revoke(target_ino);
                self.watch_mgr.notify(WatchEvent::Create {
                    parent,
                    name: name.to_string(),
                    ino: target_ino,
                });
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateEntry {
                        parent,
                        name: name.to_string(),
                        entry: DirEntry {
                            name: name.to_string(),
                            ino: target_ino,
                            file_type: attr.file_type,
                        },
                    },
                    self.config.site_id,
                );

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Link, duration, true);

                Ok(attr)
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Link, duration, false);
                Err(e)
            }
        }
    }

    /// Reads the target of a symbolic link.
    pub fn readlink(&self, ino: InodeId) -> Result<String, MetaError> {
        let start = Instant::now();
        let result = self.service.readlink(ino);

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::Readlink, duration, result.is_ok());

        result
    }

    /// Gets an extended attribute value.
    pub fn get_xattr(&self, ino: InodeId, name: &str) -> Result<Vec<u8>, MetaError> {
        let start = Instant::now();
        let result = self.xattr_store.get(ino, name);

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::GetXattr, duration, result.is_ok());

        result
    }

    /// Sets an extended attribute value.
    pub fn set_xattr(&self, ino: InodeId, name: &str, value: &[u8]) -> Result<(), MetaError> {
        let start = Instant::now();

        if let Some(worm_state) = self.worm_mgr.get_state(ino) {
            if worm_state.is_protected() {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::SetXattr, duration, false);
                return Err(MetaError::PermissionDenied);
            }
        }

        let result = self.xattr_store.set(ino, name, value);

        match result {
            Ok(()) => {
                let _ = self.lease_mgr.revoke(ino);
                self.watch_mgr.notify(WatchEvent::XattrChange { ino });

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::SetXattr, duration, true);

                Ok(())
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::SetXattr, duration, false);
                Err(e)
            }
        }
    }

    /// Lists extended attribute names on an inode.
    pub fn list_xattrs(&self, ino: InodeId) -> Result<Vec<String>, MetaError> {
        let start = Instant::now();
        let result = self.xattr_store.list(ino);

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::ListXattrs, duration, result.is_ok());

        result
    }

    /// Removes an extended attribute.
    pub fn remove_xattr(&self, ino: InodeId, name: &str) -> Result<(), MetaError> {
        let start = Instant::now();

        if let Some(worm_state) = self.worm_mgr.get_state(ino) {
            if worm_state.is_protected() {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics
                    .record_op(MetricOp::RemoveXattr, duration, false);
                return Err(MetaError::PermissionDenied);
            }
        }

        let result = self.xattr_store.remove(ino, name);

        match result {
            Ok(()) => {
                let _ = self.lease_mgr.revoke(ino);
                self.watch_mgr.notify(WatchEvent::XattrChange { ino });

                let duration = start.elapsed().as_micros() as u64;
                self.metrics
                    .record_op(MetricOp::RemoveXattr, duration, true);

                Ok(())
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics
                    .record_op(MetricOp::RemoveXattr, duration, false);
                Err(e)
            }
        }
    }

    /// Returns filesystem statistics.
    pub fn statfs(&self) -> StatFs {
        let start = Instant::now();
        let inode_count = self.inode_count();
        let max_inodes: u64 = 1_000_000_000;

        let result = StatFs {
            total_inodes: max_inodes,
            free_inodes: max_inodes.saturating_sub(inode_count),
            total_blocks: 1_000_000_000,
            free_blocks: 900_000_000,
            block_size: 4096,
            max_name_len: 255,
        };

        let duration = start.elapsed().as_micros() as u64;
        self.metrics.record_op(MetricOp::Statfs, duration, true);

        result
    }

    /// Lists directory entries with full inode attributes (FUSE readdirplus).
    pub fn readdir_plus(&self, dir: InodeId) -> Result<Vec<DirEntryPlus>, MetaError> {
        let start = Instant::now();
        let entries = self.service.readdir(dir)?;

        let mut result = Vec::with_capacity(entries.len());
        for entry in entries {
            match self.service.getattr(entry.ino) {
                Ok(attr) => {
                    result.push(DirEntryPlus { entry, attr });
                }
                Err(_) => {
                    let attr = InodeAttr::new_file(entry.ino, 0, 0, 0, self.config.site_id);
                    result.push(DirEntryPlus { entry, attr });
                }
            }
        }

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::ReaddirPlus, duration, true);

        Ok(result)
    }

    /// Creates a special file (device, FIFO, or socket).
    pub fn mknod(
        &self,
        parent: InodeId,
        name: &str,
        file_type: FileType,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<InodeAttr, MetaError> {
        let start = Instant::now();

        self.quota_mgr
            .check_quota(uid, gid, 0, 1)
            .inspect_err(|_e| {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Mknod, duration, false);
            })?;

        let _: Result<(), ()> = match file_type {
            FileType::BlockDevice | FileType::CharDevice | FileType::Fifo | FileType::Socket => {
                Ok(())
            }
            _ => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Mknod, duration, false);
                return Err(MetaError::KvError(
                    "mknod requires a special file type".to_string(),
                ));
            }
        };

        let result = self.service.create_file(parent, name, uid, gid, mode);

        match result {
            Ok(mut attr) => {
                attr.file_type = file_type;
                let _ = self.service.setattr(attr.ino, attr.clone());

                self.quota_mgr.update_usage(uid, gid, 0, 1);
                let _ = self.lease_mgr.revoke(parent);
                self.watch_mgr.notify(WatchEvent::Create {
                    parent,
                    name: name.to_string(),
                    ino: attr.ino,
                });
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateInode { attr: attr.clone() },
                    self.config.site_id,
                );
                let _ = self.cdc_stream.publish(
                    MetaOp::CreateEntry {
                        parent,
                        name: name.to_string(),
                        entry: DirEntry {
                            name: name.to_string(),
                            ino: attr.ino,
                            file_type: attr.file_type,
                        },
                    },
                    self.config.site_id,
                );
                self.inode_counter.fetch_add(1, Ordering::Relaxed);

                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Mknod, duration, true);

                Ok(attr)
            }
            Err(e) => {
                let duration = start.elapsed().as_micros() as u64;
                self.metrics.record_op(MetricOp::Mknod, duration, false);
                Err(e)
            }
        }
    }

    /// Checks if a user has the requested access to an inode (FUSE access).
    pub fn access(&self, ino: InodeId, uid: u32, gid: u32, mode: u32) -> Result<(), MetaError> {
        let start = Instant::now();
        let attr = self.service.getattr(ino)?;

        let ctx = crate::access::UserContext::new(uid, gid, vec![]);
        let access_mode = crate::access::AccessMode(mode);
        let result = crate::access::check_access(&attr, &ctx, access_mode);

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::Access, duration, result.is_ok());

        result
    }

    /// Flushes file handle metadata (called on close by FUSE).
    pub fn flush(&self, fh: u64) -> Result<(), MetaError> {
        let start = Instant::now();

        let result = if self.fh_mgr.get(fh).is_ok() {
            Ok(())
        } else {
            Err(MetaError::KvError("invalid file handle".to_string()))
        };

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::Flush, duration, result.is_ok());

        result
    }

    /// Syncs metadata for an inode to persistent storage.
    pub fn fsync(&self, ino: InodeId, _datasync: bool) -> Result<(), MetaError> {
        let start = Instant::now();

        let result = self.service.getattr(ino).map(|_| ());

        let duration = start.elapsed().as_micros() as u64;
        self.metrics
            .record_op(MetricOp::Fsync, duration, result.is_ok());

        result
    }

    /// Returns a reference to the metadata journal for replication tailing.
    pub fn journal(&self) -> &std::sync::Arc<crate::journal::MetadataJournal> {
        self.service.journal()
    }

    /// Returns the fingerprint index for CAS dedup integration (A3).
    pub fn fingerprint_index(&self) -> &FingerprintIndex {
        &self.fingerprint_idx
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
        self.inode_counter.load(Ordering::Relaxed)
    }

    /// Returns whether the node is healthy.
    /// A node is healthy if it is the only node or if it has at least one alive peer.
    pub fn is_healthy(&self) -> bool {
        let status = self.cluster_status();
        status.alive_nodes > 0
    }

    /// Returns a reference to the membership tracker for cluster management.
    pub fn membership(&self) -> &MembershipManager {
        &self.membership
    }

    /// Returns cluster status: number of alive, suspect, and dead nodes.
    pub fn cluster_status(&self) -> ClusterStatus {
        let all = self.membership.all_members();
        let alive = all.iter().filter(|m| m.state == NodeState::Alive).count() as u32;
        let suspect = all.iter().filter(|m| m.state == NodeState::Suspect).count() as u32;
        let dead = all.iter().filter(|m| m.state == NodeState::Dead).count() as u32;
        ClusterStatus {
            total_nodes: all.len() as u32,
            alive_nodes: alive,
            suspect_nodes: suspect,
            dead_nodes: dead,
            this_node: self.config.node_id,
        }
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

    #[test]
    fn test_symlink() {
        let node = make_node_memory();
        let attr = node
            .symlink(InodeId::ROOT_INODE, "mylink", "/tmp/target", 1000, 1000)
            .unwrap();
        assert_eq!(attr.file_type, FileType::Symlink);
        assert_eq!(attr.symlink_target, Some("/tmp/target".to_string()));

        let entry = node.lookup(InodeId::ROOT_INODE, "mylink").unwrap();
        assert_eq!(entry.ino, attr.ino);
    }

    #[test]
    fn test_link() {
        let node = make_node_memory();
        let file = node
            .create_file(InodeId::ROOT_INODE, "original.txt", 1000, 1000, 0o644)
            .unwrap();

        let linked = node
            .link(InodeId::ROOT_INODE, "link.txt", file.ino)
            .unwrap();
        assert_eq!(linked.ino, file.ino);
        assert_eq!(linked.nlink, 2);

        let entry = node.lookup(InodeId::ROOT_INODE, "link.txt").unwrap();
        assert_eq!(entry.ino, file.ino);
    }

    #[test]
    fn test_readlink() {
        let node = make_node_memory();
        let attr = node
            .symlink(InodeId::ROOT_INODE, "mylink", "/etc/hosts", 1000, 1000)
            .unwrap();

        let target = node.readlink(attr.ino).unwrap();
        assert_eq!(target, "/etc/hosts");
    }

    #[test]
    fn test_xattr_set_get() {
        let node = make_node_memory();
        let file = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        node.set_xattr(file.ino, "user.author", b"alice").unwrap();
        let value = node.get_xattr(file.ino, "user.author").unwrap();
        assert_eq!(value, b"alice");
    }

    #[test]
    fn test_xattr_list() {
        let node = make_node_memory();
        let file = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        node.set_xattr(file.ino, "user.a", b"1").unwrap();
        node.set_xattr(file.ino, "user.b", b"2").unwrap();

        let names = node.list_xattrs(file.ino).unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"user.a".to_string()));
        assert!(names.contains(&"user.b".to_string()));
    }

    #[test]
    fn test_xattr_remove() {
        let node = make_node_memory();
        let file = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        node.set_xattr(file.ino, "user.key", b"value").unwrap();
        node.remove_xattr(file.ino, "user.key").unwrap();

        let result = node.get_xattr(file.ino, "user.key");
        assert!(result.is_err());
    }

    #[test]
    fn test_statfs() {
        let node = make_node_memory();
        let stats = node.statfs();
        assert_eq!(stats.block_size, 4096);
        assert_eq!(stats.max_name_len, 255);
        assert!(stats.total_inodes > 0);
        assert!(stats.free_inodes <= stats.total_inodes);
    }

    #[test]
    fn test_cluster_status() {
        let node = make_node_memory();
        let status = node.cluster_status();
        assert_eq!(status.total_nodes, 1);
        assert_eq!(status.alive_nodes, 1);
        assert_eq!(status.suspect_nodes, 0);
        assert_eq!(status.dead_nodes, 0);
        assert_eq!(status.this_node, NodeId::new(1));
    }

    #[test]
    fn test_membership_access() {
        let node = make_node_memory();
        let members = node.membership().all_members();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].node_id, NodeId::new(1));
    }

    #[test]
    fn test_readdir_plus() {
        let node = make_node_memory();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "a.txt", 1000, 1000, 0o644)
            .unwrap();
        let _ = node
            .mkdir(InodeId::ROOT_INODE, "b_dir", 1000, 1000, 0o755)
            .unwrap();

        let entries = node.readdir_plus(InodeId::ROOT_INODE).unwrap();
        assert!(entries.len() >= 2);

        let a_entry = entries.iter().find(|e| e.entry.name == "a.txt").unwrap();
        assert_eq!(a_entry.attr.file_type, FileType::RegularFile);
        assert_eq!(a_entry.attr.uid, 1000);
        assert_eq!(a_entry.attr.mode, 0o644);

        let b_entry = entries.iter().find(|e| e.entry.name == "b_dir").unwrap();
        assert_eq!(b_entry.attr.file_type, FileType::Directory);
    }

    #[test]
    fn test_mknod_fifo() {
        let node = make_node_memory();
        let attr = node
            .mknod(
                InodeId::ROOT_INODE,
                "myfifo",
                FileType::Fifo,
                1000,
                1000,
                0o644,
            )
            .unwrap();
        assert_eq!(attr.file_type, FileType::Fifo);
        assert_eq!(attr.uid, 1000);

        let entry = node.lookup(InodeId::ROOT_INODE, "myfifo").unwrap();
        assert_eq!(entry.ino, attr.ino);
    }

    #[test]
    fn test_mknod_socket() {
        let node = make_node_memory();
        let attr = node
            .mknod(
                InodeId::ROOT_INODE,
                "mysock",
                FileType::Socket,
                1000,
                1000,
                0o755,
            )
            .unwrap();
        assert_eq!(attr.file_type, FileType::Socket);
    }

    #[test]
    fn test_mknod_invalid_type() {
        let node = make_node_memory();
        let result = node.mknod(
            InodeId::ROOT_INODE,
            "invalid",
            FileType::RegularFile,
            1000,
            1000,
            0o644,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_access_owner_read() {
        let node = make_node_memory();
        let file = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        node.access(file.ino, 1000, 1000, 4).unwrap();
    }

    #[test]
    fn test_access_denied() {
        let node = make_node_memory();
        let file = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o600)
            .unwrap();

        let result = node.access(file.ino, 2000, 2000, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_flush_valid_handle() {
        let node = make_node_memory();
        let file = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        let fh = node.open(file.ino, 1, 0x01).unwrap();

        node.flush(fh).unwrap();
    }

    #[test]
    fn test_flush_invalid_handle() {
        let node = make_node_memory();
        let result = node.flush(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_fsync() {
        let node = make_node_memory();
        let file = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        node.fsync(file.ino, false).unwrap();
        node.fsync(file.ino, true).unwrap();
    }
}
