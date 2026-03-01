//! Error types for the claudefs-reduce subsystem

/// All errors that can occur during data reduction operations
#[derive(Debug, thiserror::Error)]
pub enum ReduceError {
    /// Compression operation failed
    #[error("Compression failed: {0}")]
    CompressionFailed(String),
    /// Decompression operation failed
    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),
    /// Encryption operation failed
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    /// Decryption authentication tag mismatch — data may be corrupted or tampered
    #[error("Decryption failed: authentication tag mismatch (data may be corrupted)")]
    DecryptionAuthFailed,
    /// Encryption is enabled but no master key was provided
    #[error("Missing encryption key: encryption is enabled but no master key was set")]
    MissingKey,
    /// Chunk is marked as duplicate but reference data was not provided for read
    #[error("Missing chunk data: chunk is_duplicate=true but reference data not provided")]
    MissingChunkData,
    /// I/O error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Attempted to downgrade WORM retention policy
    #[error("Cannot downgrade WORM retention policy")]
    PolicyDowngradeAttempted,
}
