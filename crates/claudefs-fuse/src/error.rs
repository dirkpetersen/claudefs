use thiserror::Error;

#[derive(Debug, Error)]
pub enum FuseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Mount failed at {mountpoint}: {reason}")]
    MountFailed { mountpoint: String, reason: String },

    #[error("Inode not found: {ino}")]
    NotFound { ino: u64 },

    #[error("Permission denied for inode {ino}, operation: {op}")]
    PermissionDenied { ino: u64, op: String },

    #[error("Not a directory: {ino}")]
    NotDirectory { ino: u64 },

    #[error("Is a directory: {ino}")]
    IsDirectory { ino: u64 },

    #[error("Directory not empty: {ino}")]
    NotEmpty { ino: u64 },

    #[error("Name already exists: {name}")]
    AlreadyExists { name: String },

    #[error("Invalid argument: {msg}")]
    InvalidArgument { msg: String },

    #[error("Passthrough mode not supported on this kernel")]
    PassthroughUnsupported,

    #[error("Kernel version too old: required {required}, found {found}")]
    KernelVersionTooOld { required: String, found: String },

    #[error("Metadata cache is full")]
    CacheOverflow,

    #[error("Operation not supported: {op}")]
    NotSupported { op: String },
}

pub type Result<T> = std::result::Result<T, FuseError>;

impl FuseError {
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
