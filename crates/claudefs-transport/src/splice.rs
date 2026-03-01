//! Zero-copy data transfer using Linux splice/sendfile syscalls.
//!
//! This module provides abstractions for moving data directly from disk to network
//! without copying through userspace, using Linux splice() and sendfile() syscalls.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Configuration for zero-copy transfers via splice/sendfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceConfig {
    /// Maximum size of the pipe buffer for splice operations.
    /// Defaults to 1MB.
    pub max_pipe_size: usize,
    /// Chunk size for each splice operation.
    /// Defaults to 64KB.
    pub splice_chunk_size: usize,
    /// Whether to fallback to sendfile if splice is unsupported.
    /// Defaults to true.
    pub sendfile_fallback: bool,
    /// Whether to use SPLICE_F_MOVE flag (hint to move pages instead of copying).
    /// Defaults to true.
    pub use_splice_move: bool,
    /// Whether to use SPLICE_F_MORE flag (more data coming for batched sends).
    /// Defaults to true.
    pub use_splice_more: bool,
}

impl Default for SpliceConfig {
    fn default() -> Self {
        Self {
            max_pipe_size: 1024 * 1024,   // 1MB
            splice_chunk_size: 64 * 1024, // 64KB
            sendfile_fallback: true,
            use_splice_move: true,
            use_splice_more: true,
        }
    }
}

/// Linux splice() flags as a bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SpliceFlags(u32);

impl SpliceFlags {
    /// SPLICE_F_MOVE - hint to move pages instead of copying
    pub const MOVE: Self = Self(1);
    /// SPLICE_F_NONBLOCK - don't block on pipe operations
    pub const NONBLOCK: Self = Self(2);
    /// SPLICE_F_MORE - more data will be coming
    pub const MORE: Self = Self(4);
    /// SPLICE_F_GIFT - pages are a gift (not implemented in Linux)
    pub const GIFT: Self = Self(8);

    /// Returns the raw bit value.
    pub fn bits(&self) -> u32 {
        self.0
    }

    /// Creates flags from raw bits.
    pub fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Returns whether a specific flag is set.
    pub fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
}

impl std::ops::BitOr for SpliceFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for SpliceFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for SpliceFlags {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// Source or destination of a zero-copy transfer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransferEndpoint {
    /// A file endpoint.
    File {
        /// File path (for debugging/logging).
        path: String,
        /// File descriptor number.
        fd: i32,
    },
    /// A socket endpoint.
    Socket {
        /// Socket address (for debugging/logging).
        addr: String,
        /// Socket file descriptor.
        fd: i32,
    },
    /// A pipe endpoint (for intermediate splice operations).
    Pipe {
        /// Read end file descriptor.
        read_fd: i32,
        /// Write end file descriptor.
        write_fd: i32,
    },
}

impl TransferEndpoint {
    /// Returns true if this is a file endpoint.
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File { .. })
    }

    /// Returns true if this is a socket endpoint.
    pub fn is_socket(&self) -> bool {
        matches!(self, Self::Socket { .. })
    }

    /// Returns true if this is a pipe endpoint.
    pub fn is_pipe(&self) -> bool {
        matches!(self, Self::Pipe { .. })
    }
}

/// Represents a single planned zero-copy transfer operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpliceOperation {
    /// Source endpoint (file or pipe).
    pub source: TransferEndpoint,
    /// Destination endpoint (socket or pipe).
    pub destination: TransferEndpoint,
    /// Offset into the source (for files).
    pub offset: u64,
    /// Number of bytes to transfer.
    pub length: usize,
    /// Splice flags for this operation.
    pub flags: SpliceFlags,
}

/// A planned sequence of splice operations to perform a transfer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SplicePlan {
    /// The sequence of operations to execute.
    pub operations: Vec<SpliceOperation>,
    /// Total bytes to transfer.
    pub total_bytes: usize,
    /// Whether this plan requires a pipe (intermediate buffer).
    pub requires_pipe: bool,
    /// Estimated number of chunks the transfer will be split into.
    pub estimated_chunks: usize,
}

impl SplicePlan {
    /// Returns the number of operations in this plan.
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Returns whether the plan has no operations.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

/// Orchestrates multi-step zero-copy transfers using splice/sendfile.
pub struct SplicePipeline {
    config: SpliceConfig,
}

impl SplicePipeline {
    /// Creates a new pipeline with the given configuration.
    pub fn new(config: SpliceConfig) -> Self {
        Self { config }
    }

    /// Creates a pipeline with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(SpliceConfig::default())
    }

    /// Plans a file-to-socket zero-copy transfer.
    ///
    /// For file-to-socket transfers, we use a two-stage splice:
    /// 1. splice from file to pipe
    /// 2. splice from pipe to socket
    pub fn plan_file_to_socket(
        &self,
        file_path: &str,
        socket_addr: &str,
        offset: u64,
        length: usize,
    ) -> SplicePlan {
        if length == 0 {
            return SplicePlan {
                operations: vec![],
                total_bytes: 0,
                requires_pipe: true,
                estimated_chunks: 0,
            };
        }

        let chunk_size = if self.config.splice_chunk_size == 0 {
            length
        } else {
            self.config.splice_chunk_size
        };
        let num_chunks = (length + chunk_size - 1) / chunk_size;

        let mut operations = Vec::with_capacity(num_chunks * 2);

        let file_fd = -1; // Placeholder - would be resolved at execution time
        let socket_fd = -1; // Placeholder - would be resolved at execution time

        // Create virtual pipe fds for planning (these would be created at execution)
        let pipe_read_fd = -1;
        let pipe_write_fd = -1;

        // Build operations for each chunk
        for chunk_idx in 0..num_chunks {
            let chunk_offset = offset + (chunk_idx * chunk_size) as u64;
            let current_chunk_size = if chunk_idx == num_chunks - 1 {
                length - (chunk_idx * chunk_size)
            } else {
                chunk_size
            };

            // Stage 1: file -> pipe
            operations.push(SpliceOperation {
                source: TransferEndpoint::File {
                    path: file_path.to_string(),
                    fd: file_fd,
                },
                destination: TransferEndpoint::Pipe {
                    read_fd: pipe_read_fd,
                    write_fd: pipe_write_fd,
                },
                offset: chunk_offset,
                length: current_chunk_size,
                flags: self.build_flags(false),
            });

            // Stage 2: pipe -> socket
            operations.push(SpliceOperation {
                source: TransferEndpoint::Pipe {
                    read_fd: pipe_read_fd,
                    write_fd: pipe_write_fd,
                },
                destination: TransferEndpoint::Socket {
                    addr: socket_addr.to_string(),
                    fd: socket_fd,
                },
                offset: 0, // Pipe doesn't use offset
                length: current_chunk_size,
                flags: self.build_flags(chunk_idx < num_chunks - 1),
            });
        }

        SplicePlan {
            operations,
            total_bytes: length,
            requires_pipe: true,
            estimated_chunks: num_chunks,
        }
    }

    /// Plans a socket-to-file zero-copy transfer.
    ///
    /// Note: socket-to-file typically uses recv() + write() rather than splice
    /// since Linux doesn't support splice from socket to file directly.
    /// This plan uses a fallback approach with the pipe intermediary.
    pub fn plan_socket_to_file(
        &self,
        socket_addr: &str,
        file_path: &str,
        length: usize,
    ) -> SplicePlan {
        if length == 0 {
            return SplicePlan {
                operations: vec![],
                total_bytes: 0,
                requires_pipe: true,
                estimated_chunks: 0,
            };
        }

        let chunk_size = self.config.splice_chunk_size;
        let num_chunks = (length + chunk_size - 1) / chunk_size;

        let mut operations = Vec::with_capacity(num_chunks * 2);

        let socket_fd = -1; // Placeholder
        let file_fd = -1; // Placeholder

        let pipe_read_fd = -1;
        let pipe_write_fd = -1;

        for chunk_idx in 0..num_chunks {
            let current_chunk_size = if chunk_idx == num_chunks - 1 {
                length - (chunk_idx * chunk_size)
            } else {
                chunk_size
            };

            // Stage 1: socket -> pipe (using splice if possible, fallback to recv)
            operations.push(SpliceOperation {
                source: TransferEndpoint::Socket {
                    addr: socket_addr.to_string(),
                    fd: socket_fd,
                },
                destination: TransferEndpoint::Pipe {
                    read_fd: pipe_read_fd,
                    write_fd: pipe_write_fd,
                },
                offset: 0,
                length: current_chunk_size,
                flags: self.build_flags(false),
            });

            // Stage 2: pipe -> file
            operations.push(SpliceOperation {
                source: TransferEndpoint::Pipe {
                    read_fd: pipe_read_fd,
                    write_fd: pipe_write_fd,
                },
                destination: TransferEndpoint::File {
                    path: file_path.to_string(),
                    fd: file_fd,
                },
                offset: (chunk_idx * chunk_size) as u64,
                length: current_chunk_size,
                flags: self.build_flags(chunk_idx < num_chunks - 1),
            });
        }

        SplicePlan {
            operations,
            total_bytes: length,
            requires_pipe: true,
            estimated_chunks: num_chunks,
        }
    }

    fn build_flags(&self, is_last_chunk: bool) -> SpliceFlags {
        let mut flags = SpliceFlags::default();
        if self.config.use_splice_move {
            flags |= SpliceFlags::MOVE;
        }
        if self.config.use_splice_more && !is_last_chunk {
            flags |= SpliceFlags::MORE;
        }
        flags
    }

    /// Returns the configuration.
    pub fn config(&self) -> &SpliceConfig {
        &self.config
    }
}

/// Statistics for zero-copy transfers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpliceStats {
    /// Total bytes transferred via zero-copy.
    pub total_bytes_transferred: u64,
    /// Total number of transfer operations completed.
    pub total_operations: u64,
    /// Number of splice() syscalls performed.
    pub splice_calls: u64,
    /// Number of sendfile() syscalls performed.
    pub sendfile_calls: u64,
    /// Number of fallback copy operations performed.
    pub fallback_copy_calls: u64,
    /// Bytes saved by avoiding userspace copy (equals total_bytes_transferred).
    pub bytes_saved_from_copy: u64,
}

impl SpliceStats {
    /// Creates new empty statistics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a splice operation.
    pub fn record_splice(&mut self, bytes: u64) {
        self.splice_calls += 1;
        self.total_bytes_transferred += bytes;
        self.bytes_saved_from_copy += bytes;
        self.total_operations += 1;
    }

    /// Records a sendfile operation.
    pub fn record_sendfile(&mut self, bytes: u64) {
        self.sendfile_calls += 1;
        self.total_bytes_transferred += bytes;
        self.bytes_saved_from_copy += bytes;
        self.total_operations += 1;
    }

    /// Records a fallback copy operation.
    pub fn record_fallback(&mut self, bytes: u64) {
        self.fallback_copy_calls += 1;
        self.total_bytes_transferred += bytes;
        self.total_operations += 1;
    }

    /// Returns a snapshot of current statistics.
    pub fn snapshot(&self) -> Self {
        self.clone()
    }

    /// Resets all statistics to zero.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns the zero-copy transfer ratio (splice + sendfile / total).
    pub fn zerocopy_ratio(&self) -> f64 {
        let total = self.splice_calls + self.sendfile_calls + self.fallback_copy_calls;
        if total == 0 {
            return 1.0;
        }
        (self.splice_calls + self.sendfile_calls) as f64 / total as f64
    }
}

/// Errors that can occur during splice operations.
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum SpliceError {
    #[error("Unsupported endpoint: {reason}")]
    UnsupportedEndpoint { reason: String },

    #[error("Pipe creation failed: {reason}")]
    PipeCreationFailed { reason: String },

    #[error("Splice failed: {reason} (errno: {errno})")]
    SpliceFailed { reason: String, errno: i32 },

    #[error("Offset {offset} out of range for file size {file_size}")]
    OffsetOutOfRange { offset: u64, file_size: u64 },

    #[error("Transfer incomplete: expected {expected} bytes, got {actual}")]
    TransferIncomplete { expected: usize, actual: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = SpliceConfig::default();
        assert_eq!(config.max_pipe_size, 1024 * 1024);
        assert_eq!(config.splice_chunk_size, 64 * 1024);
        assert!(config.sendfile_fallback);
        assert!(config.use_splice_move);
        assert!(config.use_splice_more);
    }

    #[test]
    fn test_config_serialization() {
        let config = SpliceConfig::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: SpliceConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config.max_pipe_size, deserialized.max_pipe_size);
    }

    #[test]
    fn test_splice_flags_constants() {
        assert_eq!(SpliceFlags::MOVE.bits(), 1);
        assert_eq!(SpliceFlags::NONBLOCK.bits(), 2);
        assert_eq!(SpliceFlags::MORE.bits(), 4);
        assert_eq!(SpliceFlags::GIFT.bits(), 8);
    }

    #[test]
    fn test_splice_flags_bitwise() {
        let flags = SpliceFlags::MOVE | SpliceFlags::MORE;
        assert!(flags.contains(SpliceFlags::MOVE));
        assert!(flags.contains(SpliceFlags::MORE));
        assert!(!flags.contains(SpliceFlags::NONBLOCK));
    }

    #[test]
    fn test_splice_flags_from_bits() {
        let flags = SpliceFlags::from_bits(5); // MOVE | MORE
        assert!(flags.contains(SpliceFlags::MOVE));
        assert!(flags.contains(SpliceFlags::MORE));
    }

    #[test]
    fn test_splice_flags_default() {
        let flags = SpliceFlags::default();
        assert_eq!(flags.bits(), 0);
    }

    #[test]
    fn test_transfer_endpoint_file() {
        let endpoint = TransferEndpoint::File {
            path: "/data/file.bin".to_string(),
            fd: 42,
        };
        assert!(endpoint.is_file());
        assert!(!endpoint.is_socket());
        assert!(!endpoint.is_pipe());
    }

    #[test]
    fn test_transfer_endpoint_socket() {
        let endpoint = TransferEndpoint::Socket {
            addr: "192.168.1.1:8080".to_string(),
            fd: 43,
        };
        assert!(!endpoint.is_file());
        assert!(endpoint.is_socket());
        assert!(!endpoint.is_pipe());
    }

    #[test]
    fn test_transfer_endpoint_pipe() {
        let endpoint = TransferEndpoint::Pipe {
            read_fd: 10,
            write_fd: 11,
        };
        assert!(!endpoint.is_file());
        assert!(!endpoint.is_socket());
        assert!(endpoint.is_pipe());
    }

    #[test]
    fn test_plan_file_to_socket() {
        let pipeline = SplicePipeline::with_defaults();
        let plan = pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 0, 1024);

        assert!(!plan.is_empty());
        assert_eq!(plan.total_bytes, 1024);
        assert!(plan.requires_pipe);
        // 1024 bytes with 64KB chunk = 1 chunk, but we create 2 operations (file->pipe, pipe->socket)
        assert!(plan.estimated_chunks >= 1);
    }

    #[test]
    fn test_plan_file_to_socket_large_transfer() {
        let config = SpliceConfig {
            max_pipe_size: 1024 * 1024,
            splice_chunk_size: 64 * 1024,
            sendfile_fallback: true,
            use_splice_move: true,
            use_splice_more: true,
        };
        let pipeline = SplicePipeline::new(config);

        // 256KB transfer with 64KB chunks = 4 chunks = 8 operations (2 per chunk)
        let plan =
            pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 0, 256 * 1024);

        assert_eq!(plan.estimated_chunks, 4);
        assert_eq!(plan.operations.len(), 8);
        assert_eq!(plan.total_bytes, 256 * 1024);
    }

    #[test]
    fn test_plan_socket_to_file() {
        let pipeline = SplicePipeline::with_defaults();
        let plan = pipeline.plan_socket_to_file("192.168.1.1:8080", "/data/file.bin", 512);

        assert!(!plan.is_empty());
        assert_eq!(plan.total_bytes, 512);
        assert!(plan.requires_pipe);
    }

    #[test]
    fn test_plan_zero_length() {
        let pipeline = SplicePipeline::with_defaults();

        let plan1 = pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 0, 0);
        assert!(plan1.is_empty());
        assert_eq!(plan1.total_bytes, 0);

        let plan2 = pipeline.plan_socket_to_file("192.168.1.1:8080", "/data/file.bin", 0);
        assert!(plan2.is_empty());
        assert_eq!(plan2.total_bytes, 0);
    }

    #[test]
    fn test_plan_with_offset() {
        let pipeline = SplicePipeline::with_defaults();
        let plan =
            pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 1024 * 1024, 4096);

        assert!(!plan.is_empty());
        // First operation should have the offset
        let first_op = &plan.operations[0];
        assert_eq!(first_op.offset, 1024 * 1024);
    }

    #[test]
    fn test_plan_multiple_operations() {
        let config = SpliceConfig {
            max_pipe_size: 1024 * 1024,
            splice_chunk_size: 32 * 1024, // Small chunk to get multiple ops
            sendfile_fallback: true,
            use_splice_move: true,
            use_splice_more: true,
        };
        let pipeline = SplicePipeline::new(config);

        // 128KB with 32KB chunks = 4 chunks = 8 operations
        let plan =
            pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 0, 128 * 1024);

        assert_eq!(plan.operations.len(), 8);
    }

    #[test]
    fn test_splice_stats_record() {
        let mut stats = SpliceStats::new();

        stats.record_splice(100);
        assert_eq!(stats.splice_calls, 1);
        assert_eq!(stats.total_bytes_transferred, 100);
        assert_eq!(stats.bytes_saved_from_copy, 100);

        stats.record_sendfile(200);
        assert_eq!(stats.sendfile_calls, 1);
        assert_eq!(stats.total_bytes_transferred, 300);

        stats.record_fallback(50);
        assert_eq!(stats.fallback_copy_calls, 1);
        assert_eq!(stats.total_bytes_transferred, 350);
    }

    #[test]
    fn test_splice_stats_snapshot() {
        let mut stats = SpliceStats::new();
        stats.record_splice(100);

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.splice_calls, 1);

        // Modifying original doesn't affect snapshot
        stats.record_splice(100);
        assert_eq!(stats.splice_calls, 2);
        assert_eq!(snapshot.splice_calls, 1);
    }

    #[test]
    fn test_splice_stats_reset() {
        let mut stats = SpliceStats::new();
        stats.record_splice(100);
        stats.record_sendfile(200);

        stats.reset();
        assert_eq!(stats.splice_calls, 0);
        assert_eq!(stats.sendfile_calls, 0);
        assert_eq!(stats.total_bytes_transferred, 0);
    }

    #[test]
    fn test_splice_stats_zerocopy_ratio() {
        let mut stats = SpliceStats::new();

        // No operations = 100% zero-copy (ideal)
        assert_eq!(stats.zerocopy_ratio(), 1.0);

        // All splice = 100%
        stats.record_splice(100);
        assert_eq!(stats.zerocopy_ratio(), 1.0);

        // Reset and add fallback
        stats.reset();
        stats.record_fallback(100);
        assert_eq!(stats.zerocopy_ratio(), 0.0);

        // Mixed: 2 zero-copy + 1 fallback = 2/3
        stats.reset();
        stats.record_splice(100);
        stats.record_sendfile(100);
        stats.record_fallback(100);
        assert!((stats.zerocopy_ratio() - 2.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_splice_plan_len() {
        let pipeline = SplicePipeline::with_defaults();
        let plan = pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 0, 64 * 1024);

        assert_eq!(plan.len(), plan.operations.len());
        assert!(!plan.is_empty());
    }

    #[test]
    fn test_splice_plan_empty() {
        let pipeline = SplicePipeline::with_defaults();
        let plan = pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 0, 0);

        assert!(plan.is_empty());
        assert_eq!(plan.len(), 0);
    }

    #[test]
    fn test_pipeline_config_access() {
        let config = SpliceConfig {
            max_pipe_size: 2 * 1024 * 1024,
            splice_chunk_size: 128 * 1024,
            sendfile_fallback: false,
            use_splice_move: false,
            use_splice_more: false,
        };
        let pipeline = SplicePipeline::new(config);

        let accessed_config = pipeline.config();
        assert_eq!(accessed_config.max_pipe_size, 2 * 1024 * 1024);
        assert_eq!(accessed_config.splice_chunk_size, 128 * 1024);
    }

    #[test]
    fn test_pipeline_reuse() {
        let pipeline = SplicePipeline::with_defaults();

        // Reuse the same pipeline for multiple plans
        let _plan1 = pipeline.plan_file_to_socket("/data/file1.bin", "192.168.1.1:8080", 0, 1024);
        let _plan2 = pipeline.plan_file_to_socket("/data/file2.bin", "192.168.1.1:8081", 100, 2048);
        let _plan3 = pipeline.plan_socket_to_file("192.168.1.1:8082", "/data/file3.bin", 512);

        // Pipeline should still be usable - just verify no panics
    }

    #[test]
    fn test_error_unsupported_endpoint() {
        let err = SpliceError::UnsupportedEndpoint {
            reason: "unknown type".to_string(),
        };
        assert!(err.to_string().contains("Unsupported endpoint"));
    }

    #[test]
    fn test_error_pipe_creation_failed() {
        let err = SpliceError::PipeCreationFailed {
            reason: "too many files".to_string(),
        };
        assert!(err.to_string().contains("Pipe creation failed"));
    }

    #[test]
    fn test_error_splice_failed() {
        let err = SpliceError::SpliceFailed {
            reason: "bad file descriptor".to_string(),
            errno: 9,
        };
        assert!(err.to_string().contains("Splice failed"));
    }

    #[test]
    fn test_error_offset_out_of_range() {
        let err = SpliceError::OffsetOutOfRange {
            offset: 1000,
            file_size: 500,
        };
        assert!(err.to_string().contains("Offset"));
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn test_error_transfer_incomplete() {
        let err = SpliceError::TransferIncomplete {
            expected: 1000,
            actual: 500,
        };
        assert!(err.to_string().contains("Transfer incomplete"));
        assert!(err.to_string().contains("expected"));
    }

    #[test]
    fn test_config_validation() {
        // Zero chunk size should work (edge case)
        let config = SpliceConfig {
            max_pipe_size: 1024,
            splice_chunk_size: 0,
            sendfile_fallback: true,
            use_splice_move: true,
            use_splice_more: true,
        };
        let pipeline = SplicePipeline::new(config);
        let plan = pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 0, 100);

        // With 0 chunk size, should still produce operations but may have 1 chunk
        assert!(plan.estimated_chunks >= 1);
    }

    #[test]
    fn test_large_offset_handling() {
        let pipeline = SplicePipeline::with_defaults();

        // Test with a very large offset (simulating large file)
        let plan = pipeline.plan_file_to_socket(
            "/data/large_file.bin",
            "192.168.1.1:8080",
            10 * 1024 * 1024 * 1024, // 10GB offset
            4096,
        );

        assert!(!plan.is_empty());
        let first_op = &plan.operations[0];
        assert_eq!(first_op.offset, 10 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_stats_serialization() {
        let mut stats = SpliceStats::new();
        stats.record_splice(1000);
        stats.record_sendfile(2000);
        stats.record_fallback(300);

        let serialized = serde_json::to_string(&stats).unwrap();
        let deserialized: SpliceStats = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.total_bytes_transferred, 3300);
        assert_eq!(deserialized.splice_calls, 1);
        assert_eq!(deserialized.sendfile_calls, 1);
        assert_eq!(deserialized.fallback_copy_calls, 1);
    }
}
