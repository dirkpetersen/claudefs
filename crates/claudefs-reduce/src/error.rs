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
    /// Data integrity checksum mismatch — silent data corruption detected
    #[error("checksum mismatch — silent data corruption detected")]
    ChecksumMismatch,
    /// No checksum available for integrity verification (segment not yet sealed)
    #[error("checksum missing — segment has no integrity checksum")]
    ChecksumMissing,
    /// Chunk not found in segment or catalog
    #[error("chunk not found: {0}")]
    NotFound(String),
    /// Invalid input (e.g., offset+size out of bounds)
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// Erasure coding: wrong number of shards provided
    #[error("shard count mismatch: expected {expected}, got {got}")]
    ShardCountMismatch {
        /// Expected shard count
        expected: usize,
        /// Actual shard count received
        got: usize,
    },
    /// Erasure coding: recovery failed due to too many missing shards
    #[error("erasure recovery failed: {0}")]
    RecoveryFailed(String),
    /// Memory pressure too high for GC operations
    #[error("Memory pressure too high: {0}%")]
    MemoryPressureHigh(f64),
    /// GC audit failed
    #[error("GC audit failed: {0}")]
    GcAuditFailed(String),
    /// Reference count corruption detected
    #[error("Reference count corruption: block {0} has inconsistent count {1}")]
    RefcountCorruption(String, u64),
    /// GC backpressure stall
    #[error("GC backpressure stall: collection took {0}ms")]
    GcBackpressureStall(u64),
}
