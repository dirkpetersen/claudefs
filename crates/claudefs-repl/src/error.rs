//! Error types for the replication subsystem.

use thiserror::Error;

/// Errors that can occur in the replication subsystem.
#[derive(Debug, Error)]
pub enum ReplError {
    /// Journal read/write error.
    #[error("journal error: {msg}")]
    Journal {
        /// Error message describing the issue.
        msg: String,
    },

    /// WAL data is corrupt.
    #[error("WAL corrupted: {msg}")]
    WalCorrupted {
        /// Error message describing the corruption.
        msg: String,
    },

    /// Unknown site ID.
    #[error("unknown site: {site_id}")]
    SiteUnknown {
        /// The unknown site identifier.
        site_id: u64,
    },

    /// LWW conflict detected between local and remote updates.
    #[error("conflict detected for inode {inode}: local_ts={local_ts}, remote_ts={remote_ts}")]
    ConflictDetected {
        /// The inode that has conflicting updates.
        inode: u64,
        /// Timestamp of the local update (microseconds).
        local_ts: u64,
        /// Timestamp of the remote update (microseconds).
        remote_ts: u64,
    },

    /// Conduit transport error.
    #[error("network error: {msg}")]
    NetworkError {
        /// Error message describing the network issue.
        msg: String,
    },

    /// Serialization/deserialization error.
    #[error("serialization error")]
    Serialization(#[from] bincode::Error),

    /// I/O error.
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    /// Protocol version mismatch.
    #[error("version mismatch: expected {expected}, got {got}")]
    VersionMismatch {
        /// Expected protocol version.
        expected: u32,
        /// Actual protocol version.
        got: u32,
    },

    /// Replication engine was shut down.
    #[error("replication engine shut down")]
    Shutdown,
}
