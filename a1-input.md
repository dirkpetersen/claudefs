You are implementing the `claudefs-storage` crate for the ClaudeFS distributed file system.
This crate owns local NVMe I/O via io_uring, FDP/ZNS data placement, and the block allocator.

## Task: Implement core types in `block.rs` and error types in a new `error.rs`

### File: `crates/claudefs-storage/src/error.rs`

Create a `StorageError` enum using `thiserror` with these variants:
- `IoError(#[from] std::io::Error)` — wraps std I/O errors
- `BlockNotFound { block_id: BlockId }` — block ID doesn't exist
- `OutOfSpace` — no free blocks available
- `InvalidBlockSize { requested: u64, valid_sizes: Vec<u64> }` — unsupported block size
- `DeviceError { device: String, reason: String }` — NVMe device-level error
- `AllocatorError(String)` — block allocator internal error
- `ChecksumMismatch { block_id: BlockId, expected: u64, actual: u64 }` — data corruption detected
- `NotAligned { offset: u64, alignment: u64 }` — I/O not properly aligned

Also define `pub type StorageResult<T> = Result<T, StorageError>;`

### File: `crates/claudefs-storage/src/block.rs`

Implement the core block types:

```rust
/// Unique identifier for a block on a specific device.
/// Contains device index and block offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId {
    /// Index of the NVMe device (0-based)
    pub device_idx: u16,
    /// Block offset within the device (in units of the block's size class)
    pub offset: u64,
}

/// Supported block size classes for the buddy allocator.
/// Per decisions.md D1: EC unit is 2MB packed segments.
/// The allocator manages these power-of-two block sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum BlockSize {
    /// 4KB — metadata, small inodes, journal entries
    B4K = 4096,
    /// 64KB — small file data, directory entries
    B64K = 65536,
    /// 1MB — medium file chunks, post-reduction data
    B1M = 1_048_576,
    /// 64MB — S3 blob chunks (per architecture: 64MB blob chunks to S3)
    B64M = 67_108_864,
}
```

Add methods to `BlockSize`:
- `fn as_bytes(&self) -> u64` — returns the size in bytes
- `fn from_bytes(bytes: u64) -> Option<BlockSize>` — returns the matching variant or None
- `fn all() -> &'static [BlockSize]` — returns all supported sizes in ascending order

Add methods to `BlockId`:
- `fn new(device_idx: u16, offset: u64) -> Self`
- `fn byte_offset(&self, size: BlockSize) -> u64` — returns `self.offset * size.as_bytes()`

Also define:
```rust
/// Reference to a block with its size class, for the allocator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockRef {
    pub id: BlockId,
    pub size: BlockSize,
}

/// FDP (Flexible Data Placement) hint for NVMe write tagging.
/// Per hardware.md: FDP allows tagging writes with placement hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PlacementHint {
    /// Metadata: journal entries, inode blocks, Raft log
    Metadata = 0,
    /// Hot data: recently written, frequently accessed
    HotData = 1,
    /// Warm data: moderate access frequency
    WarmData = 2,
    /// Cold data: aged-out, candidate for S3 tiering
    ColdData = 3,
    /// Snapshot: CoW snapshot blocks
    Snapshot = 4,
    /// Journal: write-ahead log / write journal entries
    Journal = 5,
}
```

### File: `crates/claudefs-storage/src/lib.rs`

Update to:
```rust
#![warn(missing_docs)]

//! ClaudeFS storage subsystem: Local NVMe I/O via io_uring, FDP/ZNS placement, block allocator
//!
//! This crate provides the foundational block storage layer for ClaudeFS.
//! It manages NVMe devices via io_uring passthrough, handles block allocation
//! using a buddy allocator, and supports FDP/ZNS data placement hints.

pub mod allocator;
pub mod block;
pub mod device;
pub mod error;
pub mod flush;
pub mod io_uring_bridge;
pub mod zns;

pub use block::{BlockId, BlockRef, BlockSize, PlacementHint};
pub use error::{StorageError, StorageResult};
```

### Conventions:
- Use `thiserror` for error types
- Use `serde` with `Serialize, Deserialize` derives on all public types
- Use `tracing` for instrumentation (not in this task yet)
- Add doc comments (`///`) on every public item
- `#![warn(missing_docs)]` is already enabled

### Important:
- Do NOT add any dependencies to Cargo.toml — use only what's already listed (tokio, thiserror, serde, tracing, tracing-subscriber)
- Write complete, compilable files — not snippets
- Output each file with a clear header like `=== FILE: crates/claudefs-storage/src/block.rs ===`
- Add Display impl for BlockId, BlockSize, and PlacementHint
- Add unit tests in each file using `#[cfg(test)] mod tests { ... }` that test the core functionality
