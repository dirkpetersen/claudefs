//! Error types for ClaudeFS gateway

use thiserror::Error;

/// NFSv3 status: OK (success)
pub const NFS3_OK: u32 = 0;
/// NFSv3 status: not owner
pub const NFS3ERR_PERM: u32 = 1;
/// NFSv3 status: no such file or directory
pub const NFS3ERR_NOENT: u32 = 2;
/// NFSv3 status: I/O error
pub const NFS3ERR_IO: u32 = 5;
/// NFSv3 status: permission denied
pub const NFS3ERR_ACCES: u32 = 13;
/// NFSv3 status: file exists
pub const NFS3ERR_EXIST: u32 = 17;
/// NFSv3 status: not a directory
pub const NFS3ERR_NOTDIR: u32 = 20;
/// NFSv3 status: is a directory
pub const NFS3ERR_ISDIR: u32 = 21;
/// NFSv3 status: invalid argument
pub const NFS3ERR_INVAL: u32 = 22;
/// NFSv3 status: file too large
pub const NFS3ERR_FBIG: u32 = 27;
/// NFSv3 status: no space left on device
pub const NFS3ERR_NOSPC: u32 = 28;
/// NFSv3 status: read-only filesystem
pub const NFS3ERR_ROFS: u32 = 30;
/// NFSv3 status: stale file handle
pub const NFS3ERR_STALE: u32 = 70;
/// NFSv3 status: illegal file handle
pub const NFS3ERR_BADHANDLE: u32 = 10001;
/// NFSv3 status: operation not supported
pub const NFS3ERR_NOTSUPP: u32 = 10004;
/// NFSv3 status: server fault
pub const NFS3ERR_SERVERFAULT: u32 = 10006;

/// Error types for NFS, S3, and protocol-level failures in the gateway.
#[derive(Error, Debug)]
pub enum GatewayError {
    /// NFS: no such file or directory
    #[error("NFS: no such file or directory")]
    Nfs3NoEnt,
    /// NFS: I/O error
    #[error("NFS: I/O error")]
    Nfs3Io,
    /// NFS: permission denied
    #[error("NFS: permission denied")]
    Nfs3Acces,
    /// NFS: already exists
    #[error("NFS: already exists")]
    Nfs3Exist,
    /// NFS: not a directory
    #[error("NFS: not a directory")]
    Nfs3NotDir,
    /// NFS: is a directory
    #[error("NFS: is a directory")]
    Nfs3IsDir,
    /// NFS: invalid argument
    #[error("NFS: invalid argument")]
    Nfs3Inval,
    /// NFS: file too large
    #[error("NFS: file too large")]
    Nfs3FBig,
    /// NFS: no space left
    #[error("NFS: no space left")]
    Nfs3NoSpc,
    /// NFS: read-only filesystem
    #[error("NFS: read-only filesystem")]
    Nfs3ROfs,
    /// NFS: stale file handle
    #[error("NFS: stale file handle")]
    Nfs3Stale,
    /// NFS: bad handle
    #[error("NFS: bad handle")]
    Nfs3BadHandle,
    /// NFS: operation not supported
    #[error("NFS: not supported")]
    Nfs3NotSupp,
    /// NFS: server fault
    #[error("NFS: server fault")]
    Nfs3ServerFault,
    /// S3: bucket not found
    #[error("S3: bucket not found: {bucket}")]
    S3BucketNotFound {
        /// The bucket name that was not found
        bucket: String,
    },
    /// S3: object not found
    #[error("S3: object not found: {key}")]
    S3ObjectNotFound {
        /// The object key that was not found
        key: String,
    },
    /// S3: invalid bucket name
    #[error("S3: invalid bucket name: {name}")]
    S3InvalidBucketName {
        /// The invalid bucket name
        name: String,
    },
    /// S3: access denied
    #[error("S3: access denied")]
    S3AccessDenied,
    /// XDR decode error
    #[error("XDR decode error: {reason}")]
    XdrDecodeError {
        /// The reason for the decode error
        reason: String,
    },
    /// XDR encode error
    #[error("XDR encode error: {reason}")]
    XdrEncodeError {
        /// The reason for the encode error
        reason: String,
    },
    /// Protocol error
    #[error("Protocol error: {reason}")]
    ProtocolError {
        /// The protocol error details
        reason: String,
    },
    /// Backend storage error
    #[error("Backend error: {reason}")]
    BackendError {
        /// The backend error details
        reason: String,
    },
    /// Feature not implemented
    #[error("Not implemented: {feature}")]
    NotImplemented {
        /// The unimplemented feature name
        feature: String,
    },
    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl GatewayError {
    /// Converts the error to an NFSv3 status code for protocol responses.
    pub fn nfs3_status(&self) -> u32 {
        match self {
            GatewayError::Nfs3NoEnt => NFS3ERR_NOENT,
            GatewayError::Nfs3Io => NFS3ERR_IO,
            GatewayError::Nfs3Acces => NFS3ERR_ACCES,
            GatewayError::Nfs3Exist => NFS3ERR_EXIST,
            GatewayError::Nfs3NotDir => NFS3ERR_NOTDIR,
            GatewayError::Nfs3IsDir => NFS3ERR_ISDIR,
            GatewayError::Nfs3Inval => NFS3ERR_INVAL,
            GatewayError::Nfs3FBig => NFS3ERR_FBIG,
            GatewayError::Nfs3NoSpc => NFS3ERR_NOSPC,
            GatewayError::Nfs3ROfs => NFS3ERR_ROFS,
            GatewayError::Nfs3Stale => NFS3ERR_STALE,
            GatewayError::Nfs3BadHandle => NFS3ERR_BADHANDLE,
            GatewayError::Nfs3NotSupp => NFS3ERR_NOTSUPP,
            GatewayError::Nfs3ServerFault => NFS3ERR_SERVERFAULT,
            GatewayError::S3BucketNotFound { .. } => NFS3ERR_NOENT,
            GatewayError::S3ObjectNotFound { .. } => NFS3ERR_NOENT,
            GatewayError::S3InvalidBucketName { .. } => NFS3ERR_INVAL,
            GatewayError::S3AccessDenied => NFS3ERR_ACCES,
            GatewayError::ProtocolError { .. } => NFS3ERR_SERVERFAULT,
            GatewayError::BackendError { .. } => NFS3ERR_IO,
            GatewayError::XdrDecodeError { .. } => NFS3ERR_INVAL,
            GatewayError::XdrEncodeError { .. } => NFS3ERR_INVAL,
            GatewayError::NotImplemented { .. } => NFS3ERR_NOTSUPP,
            GatewayError::IoError(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    NFS3ERR_ACCES
                } else {
                    NFS3ERR_IO
                }
            }
        }
    }
}

/// Result type alias using GatewayError as the error type.
pub type Result<T> = std::result::Result<T, GatewayError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nfs3noent_error() {
        let err = GatewayError::Nfs3NoEnt;
        assert_eq!(err.to_string(), "NFS: no such file or directory");
        assert_eq!(err.nfs3_status(), NFS3ERR_NOENT);
    }

    #[test]
    fn test_nfs3io_error() {
        let err = GatewayError::Nfs3Io;
        assert_eq!(err.to_string(), "NFS: I/O error");
        assert_eq!(err.nfs3_status(), NFS3ERR_IO);
    }

    #[test]
    fn test_nfs3acces_error() {
        let err = GatewayError::Nfs3Acces;
        assert_eq!(err.to_string(), "NFS: permission denied");
        assert_eq!(err.nfs3_status(), NFS3ERR_ACCES);
    }

    #[test]
    fn test_nfs3exist_error() {
        let err = GatewayError::Nfs3Exist;
        assert_eq!(err.to_string(), "NFS: already exists");
        assert_eq!(err.nfs3_status(), NFS3ERR_EXIST);
    }

    #[test]
    fn test_nfs3notdir_error() {
        let err = GatewayError::Nfs3NotDir;
        assert_eq!(err.to_string(), "NFS: not a directory");
        assert_eq!(err.nfs3_status(), NFS3ERR_NOTDIR);
    }

    #[test]
    fn test_nfs3isdir_error() {
        let err = GatewayError::Nfs3IsDir;
        assert_eq!(err.to_string(), "NFS: is a directory");
        assert_eq!(err.nfs3_status(), NFS3ERR_ISDIR);
    }

    #[test]
    fn test_nfs3inval_error() {
        let err = GatewayError::Nfs3Inval;
        assert_eq!(err.to_string(), "NFS: invalid argument");
        assert_eq!(err.nfs3_status(), NFS3ERR_INVAL);
    }

    #[test]
    fn test_nfs3fbig_error() {
        let err = GatewayError::Nfs3FBig;
        assert_eq!(err.to_string(), "NFS: file too large");
        assert_eq!(err.nfs3_status(), NFS3ERR_FBIG);
    }

    #[test]
    fn test_nfs3nospc_error() {
        let err = GatewayError::Nfs3NoSpc;
        assert_eq!(err.to_string(), "NFS: no space left");
        assert_eq!(err.nfs3_status(), NFS3ERR_NOSPC);
    }

    #[test]
    fn test_nfs3rofs_error() {
        let err = GatewayError::Nfs3ROfs;
        assert_eq!(err.to_string(), "NFS: read-only filesystem");
        assert_eq!(err.nfs3_status(), NFS3ERR_ROFS);
    }

    #[test]
    fn test_nfs3stale_error() {
        let err = GatewayError::Nfs3Stale;
        assert_eq!(err.to_string(), "NFS: stale file handle");
        assert_eq!(err.nfs3_status(), NFS3ERR_STALE);
    }

    #[test]
    fn test_nfs3badhandle_error() {
        let err = GatewayError::Nfs3BadHandle;
        assert_eq!(err.to_string(), "NFS: bad handle");
        assert_eq!(err.nfs3_status(), NFS3ERR_BADHANDLE);
    }

    #[test]
    fn test_nfs3notsupp_error() {
        let err = GatewayError::Nfs3NotSupp;
        assert_eq!(err.to_string(), "NFS: not supported");
        assert_eq!(err.nfs3_status(), NFS3ERR_NOTSUPP);
    }

    #[test]
    fn test_nfs3serverfault_error() {
        let err = GatewayError::Nfs3ServerFault;
        assert_eq!(err.to_string(), "NFS: server fault");
        assert_eq!(err.nfs3_status(), NFS3ERR_SERVERFAULT);
    }

    #[test]
    fn test_s3_bucket_not_found_error() {
        let err = GatewayError::S3BucketNotFound {
            bucket: "mybucket".to_string(),
        };
        assert_eq!(err.to_string(), "S3: bucket not found: mybucket");
        assert_eq!(err.nfs3_status(), NFS3ERR_NOENT);
    }
}
