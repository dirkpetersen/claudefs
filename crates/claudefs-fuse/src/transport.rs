//! Transport integration for ClaudeFS FUSE client.
//!
//! Defines the interface between the FUSE daemon and the remote metadata/storage
//! backends. Currently implemented as an in-memory stub; network RPCs will be
//! wired in Phase 5 when A2 (metadata) and A4 (transport) interfaces stabilize.

use crate::error::{FuseError, Result};
use crate::inode::{InodeId, InodeKind};
use std::time::SystemTime;

/// Reference to a remote file's data.
#[derive(Debug, Clone)]
pub struct RemoteRef {
    /// Inode ID on the remote metadata server
    pub ino: InodeId,
    /// Data version / generation
    pub generation: u64,
    /// File size in bytes
    pub size: u64,
    /// Metadata server shard owning this inode
    pub shard: u32,
}

/// Result of a remote lookup operation
#[derive(Debug, Clone)]
pub struct LookupResult {
    pub ino: InodeId,
    pub kind: InodeKind,
    pub size: u64,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub nlink: u32,
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
}

/// Configuration for the transport backend
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Server addresses (host:port)
    pub servers: Vec<String>,
    /// Connection timeout in milliseconds
    pub connect_timeout_ms: u64,
    /// Request timeout in milliseconds
    pub request_timeout_ms: u64,
    /// Maximum retry count
    pub max_retries: u32,
    /// Enable TLS
    pub tls: bool,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            connect_timeout_ms: 5000,
            request_timeout_ms: 30000,
            max_retries: 3,
            tls: false,
        }
    }
}

/// The transport backend trait — currently an in-memory stub
pub trait FuseTransport: Send + Sync {
    /// Look up an inode by parent inode and name
    fn lookup(&self, parent: InodeId, name: &str) -> Result<Option<LookupResult>>;
    /// Get attributes for an inode
    fn getattr(&self, ino: InodeId) -> Result<Option<LookupResult>>;
    /// Read data bytes from a remote file
    fn read(&self, ino: InodeId, offset: u64, size: u32) -> Result<Vec<u8>>;
    /// Write data bytes to a remote file
    fn write(&self, ino: InodeId, offset: u64, data: &[u8]) -> Result<u32>;
    /// Create a new file/directory
    fn create(
        &self,
        parent: InodeId,
        name: &str,
        kind: InodeKind,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Result<InodeId>;
    /// Remove an inode
    fn remove(&self, parent: InodeId, name: &str) -> Result<()>;
    /// Rename a file
    fn rename(&self, parent: InodeId, name: &str, newparent: InodeId, newname: &str) -> Result<()>;
    /// Check if the transport is connected
    fn is_connected(&self) -> bool;
}

/// In-memory stub implementation of FuseTransport
/// Used for unit testing and development before real backend is wired
#[allow(dead_code)]
pub struct StubTransport {
    connected: bool,
    config: TransportConfig,
}

impl StubTransport {
    /// Create a new stub transport with the given configuration.
    pub fn new(config: TransportConfig) -> Self {
        Self {
            connected: false,
            config,
        }
    }

    /// Creates a connected stub transport.
    pub fn connected(config: TransportConfig) -> Self {
        Self {
            connected: true,
            config,
        }
    }

    /// Creates a disconnected stub transport.
    pub fn disconnected(config: TransportConfig) -> Self {
        Self {
            connected: false,
            config,
        }
    }
}

impl FuseTransport for StubTransport {
    fn lookup(&self, _parent: InodeId, _name: &str) -> Result<Option<LookupResult>> {
        Err(FuseError::NotSupported {
            op: "lookup".into(),
        })
    }

    fn getattr(&self, _ino: InodeId) -> Result<Option<LookupResult>> {
        Err(FuseError::NotSupported {
            op: "getattr".into(),
        })
    }

    fn read(&self, _ino: InodeId, _offset: u64, _size: u32) -> Result<Vec<u8>> {
        Err(FuseError::NotSupported { op: "read".into() })
    }

    fn write(&self, _ino: InodeId, _offset: u64, _data: &[u8]) -> Result<u32> {
        Err(FuseError::NotSupported { op: "write".into() })
    }

    fn create(
        &self,
        _parent: InodeId,
        _name: &str,
        _kind: InodeKind,
        _mode: u32,
        _uid: u32,
        _gid: u32,
    ) -> Result<InodeId> {
        Err(FuseError::NotSupported {
            op: "create".into(),
        })
    }

    fn remove(&self, _parent: InodeId, _name: &str) -> Result<()> {
        Err(FuseError::NotSupported {
            op: "remove".into(),
        })
    }

    fn rename(
        &self,
        _parent: InodeId,
        _name: &str,
        _newparent: InodeId,
        _newname: &str,
    ) -> Result<()> {
        Err(FuseError::NotSupported {
            op: "rename".into(),
        })
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_config_default_has_sensible_values() {
        let config = TransportConfig::default();
        assert_eq!(config.connect_timeout_ms, 5000);
        assert_eq!(config.request_timeout_ms, 30000);
    }

    #[test]
    fn test_transport_config_max_retries_greater_than_zero() {
        let config = TransportConfig::default();
        assert!(config.max_retries > 0);
    }

    #[test]
    fn test_transport_config_tls_false_by_default() {
        let config = TransportConfig::default();
        assert!(!config.tls);
    }

    #[test]
    fn test_stub_transport_connected_is_connected() {
        let config = TransportConfig::default();
        let transport = StubTransport::connected(config);
        assert!(transport.is_connected());
    }

    #[test]
    fn test_stub_transport_disconnected_not_connected() {
        let config = TransportConfig::default();
        let transport = StubTransport::disconnected(config);
        assert!(!transport.is_connected());
    }

    #[test]
    fn test_stub_transport_lookup_returns_not_supported() {
        let config = TransportConfig::default();
        let transport = StubTransport::new(config);
        let result = transport.lookup(1, "test");
        assert!(matches!(result, Err(FuseError::NotSupported { .. })));
    }

    #[test]
    fn test_stub_transport_read_returns_not_supported() {
        let config = TransportConfig::default();
        let transport = StubTransport::new(config);
        let result = transport.read(1, 0, 4096);
        assert!(matches!(result, Err(FuseError::NotSupported { .. })));
    }

    #[test]
    fn test_stub_transport_write_returns_not_supported() {
        let config = TransportConfig::default();
        let transport = StubTransport::new(config);
        let result = transport.write(1, 0, b"data");
        assert!(matches!(result, Err(FuseError::NotSupported { .. })));
    }

    #[test]
    fn test_remote_ref_fields_accessible() {
        let remote = RemoteRef {
            ino: 123,
            generation: 1,
            size: 4096,
            shard: 0,
        };
        assert_eq!(remote.ino, 123);
        assert_eq!(remote.generation, 1);
        assert_eq!(remote.size, 4096);
        assert_eq!(remote.shard, 0);
    }

    #[test]
    fn test_lookup_result_fields_accessible() {
        let now = SystemTime::now();
        let result = LookupResult {
            ino: 42,
            kind: InodeKind::File,
            size: 1024,
            uid: 1000,
            gid: 1000,
            mode: 0o644,
            nlink: 1,
            atime: now,
            mtime: now,
            ctime: now,
        };
        assert_eq!(result.ino, 42);
        assert!(matches!(result.kind, InodeKind::File));
        assert_eq!(result.size, 1024);
        assert_eq!(result.uid, 1000);
    }
}
