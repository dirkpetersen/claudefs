You are implementing the `claudefs-storage` crate for the ClaudeFS distributed file system.

## Task: Implement a buddy block allocator in `crates/claudefs-storage/src/allocator.rs`

The allocator manages NVMe block allocation using a buddy allocator system.

### Context: Existing types (already implemented, DO NOT modify)

In `crates/claudefs-storage/src/block.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId {
    pub device_idx: u16,
    pub offset: u64,
}

impl BlockId {
    pub fn new(device_idx: u16, offset: u64) -> Self { ... }
    pub fn byte_offset(&self, size: BlockSize) -> u64 { ... }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum BlockSize {
    B4K = 4096,
    B64K = 65536,
    B1M = 1_048_576,
    B64M = 67_108_864,
}

impl BlockSize {
    pub fn as_bytes(&self) -> u64 { ... }
    pub fn from_bytes(bytes: u64) -> Option<BlockSize> { ... }
    pub fn all() -> &'static [BlockSize] { ... }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockRef {
    pub id: BlockId,
    pub size: BlockSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PlacementHint {
    Metadata = 0,
    HotData = 1,
    WarmData = 2,
    ColdData = 3,
    Snapshot = 4,
    Journal = 5,
}
```

In `crates/claudefs-storage/src/error.rs`:
```rust
pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Error)]
pub enum StorageError {
    IoError(#[from] std::io::Error),
    BlockNotFound { block_id: BlockId },
    OutOfSpace,
    InvalidBlockSize { requested: u64, valid_sizes: Vec<u64> },
    DeviceError { device: String, reason: String },
    AllocatorError(String),
    ChecksumMismatch { block_id: BlockId, expected: u64, actual: u64 },
    NotAligned { offset: u64, alignment: u64 },
}
```

### Requirements for the Buddy Allocator

Design: A buddy allocator that manages blocks at the 4KB granularity level and can split/merge blocks to serve any of the 4 block size classes.

**Key design points:**
1. The allocator manages a contiguous region of blocks on a single device
2. It tracks free lists per size class using a BTreeSet for efficient ordered access
3. Split larger blocks when no blocks of the requested size are available
4. Merge adjacent buddy blocks when freed to prevent fragmentation
5. Thread-safe: wrap the inner state in `std::sync::Mutex` for now (lock-free fast path is Phase 3)
6. Track allocation statistics (total blocks, free blocks per size, allocation/free counts)

**Size class relationships (for splitting/merging):**
- 1 x B64M = 64 x B1M
- 1 x B1M = 16 x B64K
- 1 x B64K = 16 x B4K
- (So 1 x B64M = 64 * 16 * 16 = 16384 x B4K)

**Block address convention:**
- All offsets are in units of 4KB (the smallest block)
- A B4K block at offset N occupies 1 unit starting at offset N
- A B64K block at offset N occupies 16 units starting at N (N must be aligned to 16)
- A B1M block at offset N occupies 256 units starting at N (N must be aligned to 256)
- A B64M block at offset N occupies 16384 units starting at N (N must be aligned to 16384)

### Public API

```rust
/// Configuration for the buddy allocator.
#[derive(Debug, Clone)]
pub struct AllocatorConfig {
    /// Device index this allocator manages
    pub device_idx: u16,
    /// Total number of 4KB blocks managed by this allocator
    pub total_blocks_4k: u64,
}

/// Buddy block allocator for NVMe block management.
/// Thread-safe via internal mutex.
pub struct BuddyAllocator {
    // internal state behind Mutex
}

impl BuddyAllocator {
    /// Create a new buddy allocator with the given configuration.
    /// Initially all space is carved into the largest possible aligned blocks.
    pub fn new(config: AllocatorConfig) -> StorageResult<Self> { ... }

    /// Allocate a block of the given size class.
    /// Returns the allocated BlockRef or OutOfSpace if no blocks available.
    pub fn allocate(&self, size: BlockSize) -> StorageResult<BlockRef> { ... }

    /// Free a previously allocated block.
    /// Merges with buddy blocks if both halves are free.
    pub fn free(&self, block_ref: BlockRef) -> StorageResult<()> { ... }

    /// Returns current allocation statistics.
    pub fn stats(&self) -> AllocatorStats { ... }

    /// Returns the total capacity in bytes.
    pub fn total_capacity_bytes(&self) -> u64 { ... }

    /// Returns the free capacity in bytes.
    pub fn free_capacity_bytes(&self) -> u64 { ... }
}

/// Statistics about the allocator's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocatorStats {
    /// Device index
    pub device_idx: u16,
    /// Total 4KB-equivalent blocks
    pub total_blocks_4k: u64,
    /// Free blocks per size class (4KB units)
    pub free_blocks_4k: u64,
    /// Number of free entries per size class
    pub free_count_per_size: Vec<(BlockSize, usize)>,
    /// Total allocations performed
    pub total_allocations: u64,
    /// Total frees performed
    pub total_frees: u64,
}
```

### Implementation Notes

1. Internally, maintain a `BTreeSet<u64>` per block size class for free block offsets (in 4KB units)
2. When initializing, carve the total space into the largest aligned blocks possible:
   - Walk through the address space, at each position determine the largest block size that:
     a) Fits in the remaining space
     b) Is properly aligned at that offset
   - Add that block to the appropriate free list
3. When allocating:
   - Check the requested size class's free list first
   - If empty, try to split a block from the next larger size class
   - Recursively split up the chain until finding a free block
   - Return StorageError::OutOfSpace if no blocks available at any level
4. When freeing:
   - Add the block back to its size class free list
   - Check if the buddy block is also free; if so, merge them into the next larger size
   - Recursively merge up the chain
5. The "buddy" of a block at offset O with size S (in 4KB units) is at offset O ^ S

### Conventions:
- Use `thiserror` for errors (already have StorageError)
- Use `serde` with Serialize, Deserialize on AllocatorStats and AllocatorConfig
- Add doc comments on every public item
- Use `tracing` for debug-level logging of allocate/free operations

### Tests to include:
- `test_new_allocator` — create with various sizes, verify stats
- `test_allocate_single` — allocate one block of each size
- `test_allocate_until_full` — fill the allocator, get OutOfSpace
- `test_free_and_reallocate` — free a block, allocate again
- `test_buddy_merge` — free two buddies, verify they merge to next size
- `test_split_on_demand` — when no small blocks available, verify splitting works
- `test_alignment` — verify all allocated blocks have correct alignment

### Output format
Write ONLY the complete file `crates/claudefs-storage/src/allocator.rs`.
Output the file content directly, no markdown fences, no explanation.
