//! Error types for the storage subsystem.

use thiserror::Error;

use crate::block::BlockId;

/// Result type alias for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

/// Error variants for storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Wraps standard I/O errors.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// The requested block does not exist.
    #[error("Block not found: {block_id:?}")]
    BlockNotFound {
        /// The block ID that was not found.
        block_id: BlockId,
    },

    /// No free blocks available in the allocator.
    #[error("Out of space: no free blocks available")]
    OutOfSpace,

    /// The requested block size is not supported.
    #[error("Invalid block size: requested {requested} bytes, valid sizes: {valid_sizes:?}")]
    InvalidBlockSize {
        /// The requested block size in bytes.
        requested: u64,
        /// List of valid block sizes in bytes.
        valid_sizes: Vec<u64>,
    },

    /// Block allocator internal error.
    #[error("Allocator error: {0}")]
    AllocatorError(String),

    /// NVMe device-level error.
    #[error("Device error on {device}: {reason}")]
    DeviceError {
        /// The device identifier.
        device: String,
        /// Description of the error.
        reason: String,
    },

    /// Data corruption detected: checksum mismatch.
    #[error("Checksum mismatch on block {block_id:?}: expected {expected:#x}, actual {actual:#x}")]
    ChecksumMismatch {
        /// The block ID with the checksum mismatch.
        block_id: BlockId,
        /// The expected checksum value.
        expected: u64,
        /// The actual checksum value.
        actual: u64,
    },

    /// I/O offset is not properly aligned.
    #[error("Not aligned: offset {offset} is not aligned to {alignment}")]
    NotAligned {
        /// The offset that is not aligned.
        offset: u64,
        /// The required alignment.
        alignment: u64,
    },
}
