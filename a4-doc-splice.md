Add missing Rust doc comments to the splice.rs module for the claudefs-transport crate.

The crate has #![warn(missing_docs)] enabled. The splice.rs file is MOSTLY documented already. The 13 missing_docs warnings are specifically for:
1. The SpliceError enum variants (each needs a `///` doc comment)
2. The named fields within each SpliceError variant (each needs a `///` doc comment)

Here are the specific changes needed:

```rust
// BEFORE:
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

// AFTER (add doc comments to variants and fields):
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum SpliceError {
    /// The transfer endpoint type is not supported for this operation.
    #[error("Unsupported endpoint: {reason}")]
    UnsupportedEndpoint {
        /// Description of why the endpoint is unsupported.
        reason: String
    },

    /// Failed to create the kernel pipe needed for the splice operation.
    #[error("Pipe creation failed: {reason}")]
    PipeCreationFailed {
        /// Description of the pipe creation failure.
        reason: String
    },

    /// The splice() syscall returned an error.
    #[error("Splice failed: {reason} (errno: {errno})")]
    SpliceFailed {
        /// Description of the splice failure.
        reason: String,
        /// The errno value returned by the kernel.
        errno: i32
    },

    /// The requested file offset is beyond the end of the file.
    #[error("Offset {offset} out of range for file size {file_size}")]
    OffsetOutOfRange {
        /// The requested offset that is out of range.
        offset: u64,
        /// The actual size of the file.
        file_size: u64
    },

    /// The transfer completed but fewer bytes were transferred than expected.
    #[error("Transfer incomplete: expected {expected} bytes, got {actual}")]
    TransferIncomplete {
        /// The number of bytes that were expected.
        expected: usize,
        /// The number of bytes that were actually transferred.
        actual: usize
    },
}
```

Please output the COMPLETE splice.rs file with ONLY those doc comments added to SpliceError variants and their fields. Do not change anything else in the file. Output ONLY the Rust source code, no markdown fences.
