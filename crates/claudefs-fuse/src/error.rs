//! Error types for the FUSE filesystem layer.
//!
//! This module provides [`FuseError`] for all error conditions that can arise
//! during FUSE operations, along with a [`Result`] type alias for convenience.

use thiserror::Error;

/// Errors that can occur during FUSE filesystem operations.
///
/// Each variant maps to an appropriate POSIX errno value via [`Self::to_errno`],
/// allowing the FUSE daemon to return correct error codes to the kernel.
#[derive(Debug, Error)]
pub enum FuseError {
    /// I/O error from underlying system calls or storage operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Mount operation failed at the specified mountpoint.
    #[error("Mount failed at {mountpoint}: {reason}")]
    MountFailed {
        /// The mountpoint path where the mount was attempted.
        mountpoint: String,
        /// The reason for the mount failure.
        reason: String,
    },

    /// Requested inode does not exist in the filesystem.
    #[error("Inode not found: {ino}")]
    NotFound {
        /// The inode number that was not found.
        ino: u64,
    },

    /// Operation not permitted due to insufficient permissions.
    #[error("Permission denied for inode {ino}, operation: {op}")]
    PermissionDenied {
        /// The inode number involved in the denied operation.
        ino: u64,
        /// The operation that was denied (e.g., "read", "write").
        op: String,
    },

    /// Expected a directory but the inode is not a directory.
    #[error("Not a directory: {ino}")]
    NotDirectory {
        /// The inode number that is not a directory.
        ino: u64,
    },

    /// Expected a file but the inode is a directory.
    #[error("Is a directory: {ino}")]
    IsDirectory {
        /// The inode number that is a directory.
        ino: u64,
    },

    /// Attempted to remove a directory that contains entries.
    #[error("Directory not empty: {ino}")]
    NotEmpty {
        /// The inode number of the non-empty directory.
        ino: u64,
    },

    /// Attempted to create an entry that already exists.
    #[error("Name already exists: {name}")]
    AlreadyExists {
        /// The name that already exists in the directory.
        name: String,
    },

    /// Invalid argument provided to an operation.
    #[error("Invalid argument: {msg}")]
    InvalidArgument {
        /// Description of the invalid argument.
        msg: String,
    },

    /// FUSE passthrough mode is not supported on the current kernel.
    #[error("Passthrough mode not supported on this kernel")]
    PassthroughUnsupported,

    /// Running on a kernel version older than the minimum required.
    #[error("Kernel version too old: required {required}, found {found}")]
    KernelVersionTooOld {
        /// The minimum required kernel version.
        required: String,
        /// The detected kernel version.
        found: String,
    },

    /// Metadata cache has reached its capacity limit.
    #[error("Metadata cache is full")]
    CacheOverflow,

    /// Operation is not supported by this filesystem.
    #[error("Operation not supported: {op}")]
    NotSupported {
        /// The name of the unsupported operation.
        op: String,
    },
}

/// A specialized `Result` type for FUSE operations.
pub type Result<T> = std::result::Result<T, FuseError>;

impl FuseError {
    /// Converts the error to a POSIX errno value.
    ///
    /// This mapping allows the FUSE daemon to return appropriate error codes
    /// to the kernel, which then translates them to userspace errno values.
    ///
    /// # Returns
    ///
    /// A POSIX errno integer value (e.g., `ENOENT`, `EACCES`, `EINVAL`).
    pub fn to_errno(&self) -> i32 {
        use libc::*;
        match self {
            FuseError::Io(e) => e.raw_os_error().unwrap_or(EIO),
            FuseError::MountFailed { .. } => ENOENT,
            FuseError::NotFound { .. } => ENOENT,
            FuseError::PermissionDenied { .. } => EACCES,
            FuseError::NotDirectory { .. } => ENOTDIR,
            FuseError::IsDirectory { .. } => EISDIR,
            FuseError::NotEmpty { .. } => ENOTEMPTY,
            FuseError::AlreadyExists { .. } => EEXIST,
            FuseError::InvalidArgument { .. } => EINVAL,
            FuseError::PassthroughUnsupported => ENOSYS,
            FuseError::KernelVersionTooOld { .. } => ENOSYS,
            FuseError::CacheOverflow => ENOMEM,
            FuseError::NotSupported { .. } => ENOSYS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_found_errno() {
        let err = FuseError::NotFound { ino: 42 };
        assert_eq!(err.to_errno(), libc::ENOENT);
    }

    #[test]
    fn test_permission_denied_errno() {
        let err = FuseError::PermissionDenied {
            ino: 1,
            op: "write".to_string(),
        };
        assert_eq!(err.to_errno(), libc::EACCES);
    }

    #[test]
    fn test_not_directory_errno() {
        let err = FuseError::NotDirectory { ino: 5 };
        assert_eq!(err.to_errno(), libc::ENOTDIR);
    }

    #[test]
    fn test_is_directory_errno() {
        let err = FuseError::IsDirectory { ino: 3 };
        assert_eq!(err.to_errno(), libc::EISDIR);
    }

    #[test]
    fn test_not_empty_errno() {
        let err = FuseError::NotEmpty { ino: 10 };
        assert_eq!(err.to_errno(), libc::ENOTEMPTY);
    }

    #[test]
    fn test_already_exists_errno() {
        let err = FuseError::AlreadyExists {
            name: "test".to_string(),
        };
        assert_eq!(err.to_errno(), libc::EEXIST);
    }

    #[test]
    fn test_invalid_argument_errno() {
        let err = FuseError::InvalidArgument {
            msg: "bad".to_string(),
        };
        assert_eq!(err.to_errno(), libc::EINVAL);
    }

    #[test]
    fn test_not_supported_errno() {
        let err = FuseError::NotSupported {
            op: "foo".to_string(),
        };
        assert_eq!(err.to_errno(), libc::ENOSYS);
    }

    #[test]
    fn test_cache_overflow_errno() {
        let err = FuseError::CacheOverflow;
        assert_eq!(err.to_errno(), libc::ENOMEM);
    }

    #[test]
    fn test_io_error_from_not_found() {
        let io_err = std::io::Error::from(std::io::ErrorKind::NotFound);
        let fuse_err = FuseError::Io(io_err);
        assert!(matches!(fuse_err, FuseError::Io(_)));
    }

    #[test]
    fn test_display_messages_non_empty() {
        let errors = [
            FuseError::NotFound { ino: 1 },
            FuseError::PermissionDenied {
                ino: 1,
                op: "read".to_string(),
            },
            FuseError::NotDirectory { ino: 2 },
            FuseError::IsDirectory { ino: 3 },
            FuseError::NotEmpty { ino: 4 },
            FuseError::AlreadyExists {
                name: "foo".to_string(),
            },
            FuseError::InvalidArgument {
                msg: "bad arg".to_string(),
            },
            FuseError::CacheOverflow,
            FuseError::NotSupported {
                op: "op".to_string(),
            },
        ];
        for err in errors {
            let msg = err.to_string();
            assert!(!msg.is_empty(), "Error display should be non-empty");
        }
    }
}
