//! Transport error types for ClaudeFS network layer.

use thiserror::Error;

/// Transport-specific errors that can occur during network operations.
#[derive(Error, Debug)]
pub enum TransportError {
    /// Connection was refused by the remote host.
    #[error("Connection refused to {addr}")]
    ConnectionRefused {
        /// Remote address that refused the connection.
        addr: String,
    },

    /// Connection attempt timed out.
    #[error("Connection to {addr} timed out after {timeout_ms}ms")]
    ConnectionTimeout {
        /// Remote address for the timed-out connection.
        addr: String,
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    /// Connection was reset by the remote host.
    #[error("Connection reset by peer")]
    ConnectionReset,

    /// Received an invalid frame that could not be parsed.
    #[error("Invalid frame: {reason}")]
    InvalidFrame {
        /// Reason why the frame is invalid.
        reason: String,
    },

    /// Frame had invalid magic number.
    #[error("Invalid magic: expected 0x{expected:08X}, got 0x{got:08X}")]
    InvalidMagic {
        /// Expected magic number.
        expected: u32,
        /// Actual magic number received.
        got: u32,
    },

    /// Protocol version mismatch.
    #[error("Version mismatch: expected {expected}, got {got}")]
    VersionMismatch {
        /// Expected protocol version.
        expected: u8,
        /// Actual protocol version received.
        got: u8,
    },

    /// Checksum verification failed.
    #[error("Checksum mismatch: expected 0x{expected:08X}, computed 0x{computed:08X}")]
    ChecksumMismatch {
        /// Expected checksum value.
        expected: u32,
        /// Computed checksum value.
        computed: u32,
    },

    /// Payload exceeds maximum allowed size.
    #[error("Payload too large: {size} bytes (max {max_size})")]
    PayloadTooLarge {
        /// Size of the payload in bytes.
        size: u32,
        /// Maximum allowed payload size in bytes.
        max_size: u32,
    },

    /// Request timed out while waiting for response.
    #[error("Request {request_id} timed out after {timeout_ms}ms")]
    RequestTimeout {
        /// Unique identifier for the request.
        request_id: u64,
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    /// Received an unknown opcode.
    #[error("Unknown opcode: 0x{0:04X}")]
    UnknownOpcode(u16),

    /// Not connected to any peer.
    #[error("Not connected")]
    NotConnected,

    /// RDMA is not available on this system.
    #[error("RDMA not available: {reason}")]
    RdmaNotAvailable {
        /// Reason why RDMA is not available.
        reason: String,
    },

    /// TLS-related error.
    #[error("TLS error: {reason}")]
    TlsError {
        /// Reason for the TLS error.
        reason: String,
    },

    /// Serialization or deserialization error.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// I/O error from the standard library.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Result type alias for transport operations.
pub type Result<T> = std::result::Result<T, TransportError>;
