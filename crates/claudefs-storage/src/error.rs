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

    /// Superblock is corrupted or invalid.
    #[error("Corrupted superblock: {reason}")]
    CorruptedSuperblock {
        /// Description of the corruption.
        reason: String,
    },

    /// Serialization/deserialization error.
    #[error("Serialization error: {reason}")]
    SerializationError {
        /// Description of the error.
        reason: String,
    },

    /// The requested snapshot does not exist.
    #[error("Snapshot not found: {snapshot_id}")]
    SnapshotNotFound {
        /// The snapshot ID that was not found.
        snapshot_id: u64,
    },

    /// Snapshot is in an invalid state for the requested operation.
    #[error("Invalid snapshot state: {snapshot_id} is {state}")]
    InvalidSnapshotState {
        /// The snapshot ID.
        snapshot_id: u64,
        /// The current state of the snapshot.
        state: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_result_alias() {
        let ok: StorageResult<i32> = Ok(42);
        assert!(ok.is_ok());

        let err: StorageResult<i32> = Err(StorageError::OutOfSpace);
        assert!(err.is_err());
    }

    #[test]
    fn test_io_error_from_std() {
        let std_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let storage_err = StorageError::IoError(std_err);
        matches!(storage_err, StorageError::IoError(_));
    }

    #[test]
    fn test_block_not_found() {
        let id = BlockId::new(0, 42);
        let err = StorageError::BlockNotFound { block_id: id };
        assert!(format!("{}", err).contains("BlockId"));
    }

    #[test]
    fn test_out_of_space() {
        let err = StorageError::OutOfSpace;
        assert_eq!(format!("{}", err), "Out of space: no free blocks available");
    }

    #[test]
    fn test_invalid_block_size() {
        let err = StorageError::InvalidBlockSize {
            requested: 1234,
            valid_sizes: vec![4096, 65536, 1048576, 67108864],
        };
        let msg = format!("{}", err);
        assert!(msg.contains("1234"));
    }

    #[test]
    fn test_device_error() {
        let err = StorageError::DeviceError {
            device: "nvme0n1".to_string(),
            reason: "connection lost".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("nvme0n1"));
        assert!(msg.contains("connection lost"));
    }

    #[test]
    fn test_checksum_mismatch() {
        let id = BlockId::new(1, 100);
        let err = StorageError::ChecksumMismatch {
            block_id: id,
            expected: 0xDEADBEEF,
            actual: 0xCAFEBABE,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("0xdeadbeef"));
        assert!(msg.contains("0xcafebabe"));
    }

    #[test]
    fn test_not_aligned() {
        let err = StorageError::NotAligned {
            offset: 100,
            alignment: 4096,
        };
        assert_eq!(
            format!("{}", err),
            "Not aligned: offset 100 is not aligned to 4096"
        );
    }

    #[test]
    fn test_allocator_error() {
        let err = StorageError::AllocatorError("buddy overflow".to_string());
        assert_eq!(format!("{}", err), "Allocator error: buddy overflow");
    }
}
