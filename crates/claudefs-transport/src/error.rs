use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("connection refused to {addr}")]
    ConnectionRefused { addr: String },

    #[error("connection timeout after {timeout_ms}ms to {addr}")]
    ConnectionTimeout { addr: String, timeout_ms: u64 },

    #[error("connection reset by peer")]
    ConnectionReset,

    #[error("invalid frame: {reason}")]
    InvalidFrame { reason: String },

    #[error("invalid magic number: expected 0x{expected:08X}, got 0x{got:08X}")]
    InvalidMagic { expected: u32, got: u32 },

    #[error("protocol version mismatch: expected {expected}, got {got}")]
    VersionMismatch { expected: u8, got: u8 },

    #[error("checksum mismatch: expected 0x{expected:08X}, computed 0x{computed:08X}")]
    ChecksumMismatch { expected: u32, computed: u32 },

    #[error("payload too large: {size} bytes (max {max_size})")]
    PayloadTooLarge { size: u32, max_size: u32 },

    #[error("request {request_id} timed out after {timeout_ms}ms")]
    RequestTimeout { request_id: u64, timeout_ms: u64 },

    #[error("unknown opcode: 0x{0:04X}")]
    UnknownOpcode(u16),

    #[error("not connected")]
    NotConnected,

    #[error("RDMA not available: {reason}")]
    RdmaNotAvailable { reason: String },

    #[error("TLS handshake failed: {reason}")]
    TlsError { reason: String },

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TransportError>;
