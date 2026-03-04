//! ClusterVfsBackend: NFS VfsBackend implementation wired to A2/A4 transport
//!
//! This module bridges the NFS gateway protocol layer (A7) to the ClaudeFS
//! distributed metadata service (A2) and transport layer (A4).
//!
//! # Phase 2 Status
//!
//! Currently all operations return `NotImplemented` stubs. As A2 and A4 APIs
//! stabilize, operations will be replaced with real RPC calls via `GatewayConnPool`.
//!
//! # Integration Path
//!
//! 1. Operations → `ClusterVfsBackend` → `GatewayConnPool::checkout()` → backend node
//! 2. Backend node → A4 transport → A2 metadata service
//! 3. Response → A4 transport → `GatewayConnPool::checkin()` → NFS handler

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::{debug, instrument, warn};

use crate::error::{GatewayError, Result};
use crate::gateway_conn_pool::{ConnPoolConfig, GatewayConnPool};
use crate::nfs::VfsBackend;
use crate::protocol::{
    Fattr3, FileHandle3, FsInfoResult, FsStatResult, LookupResult, PathConfResult, ReadDirResult,
};

/// Backend node information for connecting to metadata servers.
pub type NodeInfo = crate::gateway_conn_pool::BackendNode;

/// Statistics for cluster backend operations.
#[derive(Debug, Clone, Default)]
pub struct ClusterStats {
    /// Total number of RPC calls dispatched to backend
    pub total_rpc_calls: u64,
    /// Number of successfully completed RPCs
    pub successful_rpcs: u64,
    /// Number of failed RPCs (errors or timeouts)
    pub failed_rpcs: u64,
    /// Total bytes read from backend
    pub backend_bytes_read: u64,
    /// Total bytes written to backend
    pub backend_bytes_written: u64,
    /// Timestamp of last successful operation
    pub last_success: Option<Instant>,
}

/// Inner mutable state for `ClusterVfsBackend`.
struct ClusterState {
    /// Connection pool for metadata/storage node connections
    #[allow(dead_code)]
    pool: GatewayConnPool,
    /// Runtime statistics
    stats: ClusterStats,
}

/// VfsBackend implementation that dispatches to ClaudeFS A2/A4 backend.
///
/// This is the integration point between the NFS gateway protocol layer and
/// the ClaudeFS distributed system. Currently in Phase 2 stub mode.
pub struct ClusterVfsBackend {
    /// Shared mutable state (pool + stats) behind a mutex
    state: Arc<Mutex<ClusterState>>,
    /// Human-readable cluster name for logging
    cluster_name: String,
}

impl ClusterVfsBackend {
    /// Create a new `ClusterVfsBackend` connecting to the given nodes.
    ///
    /// `nodes` is the list of A2 metadata server endpoints.
    /// `config` controls pool sizing and health check parameters.
    pub fn new(nodes: Vec<NodeInfo>, config: ConnPoolConfig) -> Self {
        let pool = GatewayConnPool::new(nodes, config);
        let state = Arc::new(Mutex::new(ClusterState {
            pool,
            stats: ClusterStats::default(),
        }));
        Self {
            state,
            cluster_name: "claudefs-cluster".to_string(),
        }
    }

    /// Set the cluster name used in log messages.
    pub fn with_cluster_name(mut self, name: &str) -> Self {
        self.cluster_name = name.to_string();
        self
    }

    /// Return a snapshot of current operation statistics.
    pub fn stats(&self) -> ClusterStats {
        self.state
            .lock()
            .expect("cluster state lock poisoned")
            .stats
            .clone()
    }

    /// Increment the RPC call counter and optionally record success or failure.
    fn record_rpc(&self, success: bool) {
        let mut state = self.state.lock().expect("cluster state lock poisoned");
        state.stats.total_rpc_calls += 1;
        if success {
            state.stats.successful_rpcs += 1;
            state.stats.last_success = Some(Instant::now());
        } else {
            state.stats.failed_rpcs += 1;
        }
    }

    /// Stub implementation: log the attempted operation and return NotImplemented.
    fn not_implemented(&self, op: &str) -> GatewayError {
        warn!(cluster = %self.cluster_name, operation = op, "A2/A4 RPC not yet implemented");
        self.record_rpc(false);
        GatewayError::NotImplemented {
            feature: format!("cluster backend: {}", op),
        }
    }

    /// Check out a backend connection (stub for future A4 integration).
    ///
    /// Returns `None` if no backend nodes are available.
    #[allow(dead_code)]
    fn checkout_conn(&self) -> Option<(String, u64)> {
        let mut state = self.state.lock().expect("cluster state lock poisoned");
        state.pool.checkout()
    }

    /// Return a connection to the pool after use.
    #[allow(dead_code)]
    fn checkin_conn(&self, node_id: &str, conn_id: u64) {
        let mut state = self.state.lock().expect("cluster state lock poisoned");
        state.pool.checkin(node_id, conn_id);
    }
}

impl VfsBackend for ClusterVfsBackend {
    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn getattr(&self, fh: &FileHandle3) -> Result<Fattr3> {
        debug!(fh_len = fh.data.len(), "getattr");
        Err(self.not_implemented("getattr"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn lookup(&self, dir_fh: &FileHandle3, name: &str) -> Result<LookupResult> {
        debug!(dir_fh_len = dir_fh.data.len(), name = name, "lookup");
        Err(self.not_implemented("lookup"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn read(&self, fh: &FileHandle3, offset: u64, count: u32) -> Result<(Vec<u8>, bool)> {
        debug!(
            fh_len = fh.data.len(),
            offset = offset,
            count = count,
            "read"
        );
        Err(self.not_implemented("read"))
    }

    #[instrument(skip(self, data), fields(cluster = %self.cluster_name))]
    fn write(&self, fh: &FileHandle3, offset: u64, data: &[u8]) -> Result<u32> {
        debug!(
            fh_len = fh.data.len(),
            offset = offset,
            bytes = data.len(),
            "write"
        );
        Err(self.not_implemented("write"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn readdir(&self, dir_fh: &FileHandle3, cookie: u64, count: u32) -> Result<ReadDirResult> {
        debug!(
            dir_fh_len = dir_fh.data.len(),
            cookie = cookie,
            count = count,
            "readdir"
        );
        Err(self.not_implemented("readdir"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn mkdir(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)> {
        debug!(
            dir_fh_len = dir_fh.data.len(),
            name = name,
            mode = mode,
            "mkdir"
        );
        Err(self.not_implemented("mkdir"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn create(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)> {
        debug!(
            dir_fh_len = dir_fh.data.len(),
            name = name,
            mode = mode,
            "create"
        );
        Err(self.not_implemented("create"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn remove(&self, dir_fh: &FileHandle3, name: &str) -> Result<()> {
        debug!(dir_fh_len = dir_fh.data.len(), name = name, "remove");
        Err(self.not_implemented("remove"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn rename(
        &self,
        from_dir: &FileHandle3,
        from_name: &str,
        to_dir: &FileHandle3,
        to_name: &str,
    ) -> Result<()> {
        debug!(from = from_name, to = to_name, "rename");
        Err(self.not_implemented("rename"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn readlink(&self, fh: &FileHandle3) -> Result<String> {
        debug!(fh_len = fh.data.len(), "readlink");
        Err(self.not_implemented("readlink"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn symlink(
        &self,
        dir_fh: &FileHandle3,
        name: &str,
        target: &str,
    ) -> Result<(FileHandle3, Fattr3)> {
        debug!(
            dir_fh_len = dir_fh.data.len(),
            name = name,
            target = target,
            "symlink"
        );
        Err(self.not_implemented("symlink"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn fsstat(&self, fh: &FileHandle3) -> Result<FsStatResult> {
        debug!(fh_len = fh.data.len(), "fsstat");
        Err(self.not_implemented("fsstat"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn fsinfo(&self, fh: &FileHandle3) -> Result<FsInfoResult> {
        debug!(fh_len = fh.data.len(), "fsinfo");
        Err(self.not_implemented("fsinfo"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn pathconf(&self, fh: &FileHandle3) -> Result<PathConfResult> {
        debug!(fh_len = fh.data.len(), "pathconf");
        Err(self.not_implemented("pathconf"))
    }

    #[instrument(skip(self), fields(cluster = %self.cluster_name))]
    fn access(&self, fh: &FileHandle3, uid: u32, gid: u32, access_bits: u32) -> Result<u32> {
        debug!(
            fh_len = fh.data.len(),
            uid = uid,
            gid = gid,
            bits = access_bits,
            "access"
        );
        Err(self.not_implemented("access"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_backend() -> ClusterVfsBackend {
        ClusterVfsBackend::new(vec![], ConnPoolConfig::default())
    }

    #[test]
    fn test_cluster_backend_new() {
        let backend = make_backend();
        assert_eq!(backend.cluster_name, "claudefs-cluster");
    }

    #[test]
    fn test_cluster_backend_with_name() {
        let backend = make_backend().with_cluster_name("test-cluster");
        assert_eq!(backend.cluster_name, "test-cluster");
    }

    #[test]
    fn test_stats_initial_zero() {
        let backend = make_backend();
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 0);
        assert_eq!(stats.successful_rpcs, 0);
        assert_eq!(stats.failed_rpcs, 0);
        assert_eq!(stats.backend_bytes_read, 0);
        assert_eq!(stats.backend_bytes_written, 0);
    }

    #[test]
    fn test_getattr_returns_not_implemented() {
        let backend = make_backend();
        let fh = FileHandle3 {
            data: vec![1, 2, 3, 4],
        };
        let result = backend.getattr(&fh);
        assert!(result.is_err());
        match result.unwrap_err() {
            GatewayError::NotImplemented { feature } => {
                assert!(
                    feature.contains("getattr"),
                    "feature should mention getattr, got: {}",
                    feature
                );
            }
            e => panic!("Expected NotImplemented, got: {:?}", e),
        }
        // Stats should reflect the failed RPC
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 1);
        assert_eq!(stats.failed_rpcs, 1);
        assert_eq!(stats.successful_rpcs, 0);
    }

    #[test]
    fn test_all_ops_return_not_implemented() {
        let backend = make_backend();
        let fh = FileHandle3 {
            data: vec![0, 0, 0, 1],
        };
        let dir_fh = FileHandle3 {
            data: vec![0, 0, 0, 2],
        };

        // All operations should return NotImplemented
        assert!(backend.getattr(&fh).is_err());
        assert!(backend.lookup(&dir_fh, "file.txt").is_err());
        assert!(backend.read(&fh, 0, 1024).is_err());
        assert!(backend.write(&fh, 0, b"data").is_err());
        assert!(backend.readdir(&dir_fh, 0, 256).is_err());
        assert!(backend.mkdir(&dir_fh, "newdir", 0o755).is_err());
        assert!(backend.create(&dir_fh, "newfile", 0o644).is_err());
        assert!(backend.remove(&dir_fh, "file.txt").is_err());
        assert!(backend.rename(&dir_fh, "old", &dir_fh, "new").is_err());
        assert!(backend.readlink(&fh).is_err());
        assert!(backend.symlink(&dir_fh, "link", "/target").is_err());
        assert!(backend.fsstat(&fh).is_err());
        assert!(backend.fsinfo(&fh).is_err());
        assert!(backend.pathconf(&fh).is_err());
        assert!(backend.access(&fh, 1000, 1000, 0x7).is_err());

        // All 15 ops should be counted
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 15);
        assert_eq!(stats.failed_rpcs, 15);
    }

    #[test]
    fn test_record_rpc_success() {
        let backend = make_backend();
        backend.record_rpc(true);
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 1);
        assert_eq!(stats.successful_rpcs, 1);
        assert_eq!(stats.failed_rpcs, 0);
        assert!(stats.last_success.is_some());
    }

    #[test]
    fn test_record_rpc_failure() {
        let backend = make_backend();
        backend.record_rpc(false);
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 1);
        assert_eq!(stats.successful_rpcs, 0);
        assert_eq!(stats.failed_rpcs, 1);
        assert!(stats.last_success.is_none());
    }
}
