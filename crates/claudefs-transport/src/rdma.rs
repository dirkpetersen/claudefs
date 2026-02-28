//! RDMA transport via libfabric.
//!
//! Stub implementation — real RDMA support requires InfiniBand/RoCE hardware.

use crate::error::{TransportError, Result};

/// RDMA transport configuration.
#[derive(Debug, Clone)]
pub struct RdmaConfig {
    /// RDMA device name (e.g., "mlx5_0").
    pub device: String,
    /// Port number.
    pub port: u16,
    /// Maximum memory region size in bytes.
    pub max_mr_size: usize,
}

impl Default for RdmaConfig {
    fn default() -> Self {
        Self { device: "mlx5_0".to_string(), port: 1, max_mr_size: 64 * 1024 * 1024 }
    }
}

/// RDMA transport (stub — not yet implemented).
pub struct RdmaTransport {
    _config: RdmaConfig,
}

impl RdmaTransport {
    /// Create new RDMA transport. Currently always returns an error.
    pub fn new(_config: RdmaConfig) -> Result<Self> {
        Err(TransportError::RdmaNotAvailable {
            reason: "RDMA support not yet implemented".to_string(),
        })
    }

    /// Check if RDMA hardware is available on this system.
    pub fn is_available() -> bool {
        false
    }
}
