//! Buddy block allocator for NVMe block management.
//!
//! This module implements a buddy allocator that manages blocks at the 4KB
//! granularity level and can split/merge blocks to serve any of the 4 block
//! size classes (4KB, 64KB, 1MB, 64MB).

use std::collections::BTreeSet;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::block::{BlockId, BlockRef, BlockSize};
use crate::error::StorageError;
use crate::error::StorageResult;

/// Configuration for the buddy allocator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocatorConfig {
    /// Device index this allocator manages
    pub device_idx: u16,
    /// Total number of 4KB blocks managed by this allocator
    pub total_blocks_4k: u64,
}

/// Buddy block allocator for NVMe block management.
/// Thread-safe via internal mutex.
pub struct BuddyAllocator {
    inner: Mutex<AllocatorInner>,
}

struct AllocatorInner {
    device_idx: u16,
    total_blocks_4k: u64,
    free_lists: [BTreeSet<u64>; 4],
    total_allocations: u64,
    total_frees: u64,
}

impl AllocatorInner {
    fn new(config: AllocatorConfig) -> StorageResult<Self> {
        let mut free_lists: [BTreeSet<u64>; 4] = [
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        ];

        let mut offset: u64 = 0;
        while offset < config.total_blocks_4k {
            let remaining = config.total_blocks_4k - offset;

            let mut size_class = BlockSize::B4K;
            let mut size_blocks: u64 = 1;

            for check_size in [
                BlockSize::B64M,
                BlockSize::B1M,
                BlockSize::B64K,
                BlockSize::B4K,
            ]
            .iter()
            {
                let blocks = Self::size_to_blocks(*check_size);
                if offset.is_multiple_of(blocks) && remaining >= blocks {
                    size_class = *check_size;
                    size_blocks = blocks;
                    break;
                }
            }

            let size_idx = Self::size_index(size_class);
            free_lists[size_idx].insert(offset);
            offset += size_blocks;
        }

        Ok(Self {
            device_idx: config.device_idx,
            total_blocks_4k: config.total_blocks_4k,
            free_lists,
            total_allocations: 0,
            total_frees: 0,
        })
    }

    fn size_to_blocks(size: BlockSize) -> u64 {
        match size {
            BlockSize::B4K => 1,
            BlockSize::B64K => 16,
            BlockSize::B1M => 256,
            BlockSize::B64M => 16384,
        }
    }

    fn size_index(size: BlockSize) -> usize {
        match size {
            BlockSize::B4K => 0,
            BlockSize::B64K => 1,
            BlockSize::B1M => 2,
            BlockSize::B64M => 3,
        }
    }

    fn allocate(&mut self, size: BlockSize) -> StorageResult<BlockRef> {
        let size_idx = Self::size_index(size);

        if self.free_lists[size_idx].is_empty() {
            self.split_and_allocate(size_idx)?;
        }

        let free_list = &mut self.free_lists[size_idx];
        if let Some(&offset) = free_list.iter().next() {
            free_list.remove(&offset);

            self.total_allocations += 1;

            debug!(
                "Allocated {} at offset {} on device {}",
                size, offset, self.device_idx
            );

            Ok(BlockRef {
                id: BlockId::new(self.device_idx, offset),
                size,
            })
        } else {
            Err(StorageError::OutOfSpace)
        }
    }

    fn split_and_allocate(&mut self, target_idx: usize) -> StorageResult<()> {
        self.split_recursive(target_idx, target_idx + 1)
    }

    fn split_recursive(&mut self, target_idx: usize, current_idx: usize) -> StorageResult<()> {
        if current_idx >= 4 {
            return Err(StorageError::OutOfSpace);
        }

        if !self.free_lists[current_idx].is_empty() {
            let offset = *self.free_lists[current_idx].iter().next().unwrap();
            self.free_lists[current_idx].remove(&offset);

            let current_size = match current_idx {
                1 => BlockSize::B64K,
                2 => BlockSize::B1M,
                3 => BlockSize::B64M,
                _ => unreachable!(),
            };

            let smaller_size = match current_idx {
                1 => BlockSize::B4K,
                2 => BlockSize::B64K,
                3 => BlockSize::B1M,
                _ => unreachable!(),
            };
            let smaller_blocks = Self::size_to_blocks(smaller_size);
            let smaller_idx = current_idx - 1;

            let current_blocks = Self::size_to_blocks(current_size);
            let count = current_blocks / smaller_blocks;

            for i in 0..count {
                self.free_lists[smaller_idx].insert(offset + i * smaller_blocks);
            }

            debug!(
                "Split {} block at offset {} into {} {} blocks",
                current_size, offset, count, smaller_size
            );

            if smaller_idx == target_idx {
                return Ok(());
            }

            return self.split_recursive(target_idx, smaller_idx);
        }

        self.split_recursive(target_idx, current_idx + 1)
    }

    fn free(&mut self, block_ref: BlockRef) -> StorageResult<()> {
        let size_idx = Self::size_index(block_ref.size);
        let offset = block_ref.id.offset;
        let size_blocks = Self::size_to_blocks(block_ref.size);

        if offset >= self.total_blocks_4k || offset + size_blocks > self.total_blocks_4k {
            return Err(StorageError::AllocatorError(format!(
                "Invalid block offset {} for device with {} blocks",
                offset, self.total_blocks_4k
            )));
        }

        self.free_lists[size_idx].insert(offset);
        self.total_frees += 1;

        debug!(
            "Freed {} at offset {} on device {}",
            block_ref.size, offset, self.device_idx
        );

        self.merge_buddies(size_idx, offset);

        Ok(())
    }

    fn merge_buddies(&mut self, size_idx: usize, offset: u64) {
        if size_idx >= 3 {
            return;
        }

        let size_blocks = match size_idx {
            0 => 1,
            1 => 16,
            2 => 256,
            _ => return,
        };

        let buddy_offset = offset ^ size_blocks;

        if buddy_offset >= self.total_blocks_4k {
            return;
        }

        if self.free_lists[size_idx].contains(&offset)
            && self.free_lists[size_idx].contains(&buddy_offset)
        {
            self.free_lists[size_idx].remove(&offset);
            self.free_lists[size_idx].remove(&buddy_offset);

            let parent_offset = offset & !size_blocks;

            let parent_size = match size_idx {
                0 => BlockSize::B64K,
                1 => BlockSize::B1M,
                2 => BlockSize::B64M,
                _ => return,
            };
            let parent_idx = size_idx + 1;

            self.free_lists[parent_idx].insert(parent_offset);

            debug!(
                "Merged two {} blocks at offsets {} and {} into {} at offset {}",
                BlockSize::B4K,
                offset,
                buddy_offset,
                parent_size,
                parent_offset
            );

            self.merge_buddies(parent_idx, parent_offset);
        }
    }

    fn stats(&self) -> AllocatorStats {
        let mut free_blocks_4k: u64 = 0;
        let mut free_count_per_size = Vec::new();

        for (idx, free_list) in self.free_lists.iter().enumerate() {
            let size = match idx {
                0 => BlockSize::B4K,
                1 => BlockSize::B64K,
                2 => BlockSize::B1M,
                3 => BlockSize::B64M,
                _ => unreachable!(),
            };
            let size_blocks = Self::size_to_blocks(size);
            let count = free_list.len();
            free_blocks_4k += count as u64 * size_blocks;
            free_count_per_size.push((size, count));
        }

        AllocatorStats {
            device_idx: self.device_idx,
            total_blocks_4k: self.total_blocks_4k,
            free_blocks_4k,
            free_count_per_size,
            total_allocations: self.total_allocations,
            total_frees: self.total_frees,
        }
    }

    fn total_capacity_bytes(&self) -> u64 {
        self.total_blocks_4k * 4096
    }

    fn free_capacity_bytes(&self) -> u64 {
        let stats = self.stats();
        stats.free_blocks_4k * 4096
    }
}

impl BuddyAllocator {
    /// Create a new buddy allocator with the given configuration.
    /// Initially all space is carved into the largest possible aligned blocks.
    pub fn new(config: AllocatorConfig) -> StorageResult<Self> {
        let inner = AllocatorInner::new(config)?;
        Ok(Self {
            inner: Mutex::new(inner),
        })
    }

    /// Allocate a block of the given size class.
    /// Returns the allocated BlockRef or OutOfSpace if no blocks available.
    pub fn allocate(&self, size: BlockSize) -> StorageResult<BlockRef> {
        let mut inner = self.inner.lock().unwrap();
        inner.allocate(size)
    }

    /// Free a previously allocated block.
    /// Merges with buddy blocks if both halves are free.
    pub fn free(&self, block_ref: BlockRef) -> StorageResult<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.free(block_ref)
    }

    /// Returns current allocation statistics.
    pub fn stats(&self) -> AllocatorStats {
        let inner = self.inner.lock().unwrap();
        inner.stats()
    }

    /// Returns the total capacity in bytes.
    pub fn total_capacity_bytes(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.total_capacity_bytes()
    }

    /// Returns the free capacity in bytes.
    pub fn free_capacity_bytes(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.free_capacity_bytes()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_allocator() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 16384,
        };
        let alloc = BuddyAllocator::new(config).unwrap();
        let stats = alloc.stats();

        assert_eq!(stats.device_idx, 0);
        assert_eq!(stats.total_blocks_4k, 16384);
        assert_eq!(stats.free_blocks_4k, 16384);
        assert_eq!(stats.total_allocations, 0);
        assert_eq!(stats.total_frees, 0);
    }

    #[test]
    fn test_new_allocator_small() {
        let config = AllocatorConfig {
            device_idx: 1,
            total_blocks_4k: 100,
        };
        let alloc = BuddyAllocator::new(config).unwrap();
        let stats = alloc.stats();

        assert_eq!(stats.device_idx, 1);
        assert_eq!(stats.total_blocks_4k, 100);
        assert_eq!(stats.free_blocks_4k, 100);
    }

    #[test]
    fn test_allocate_single() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 16657,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let b4k = alloc.allocate(BlockSize::B4K).unwrap();
        assert_eq!(b4k.size, BlockSize::B4K);
        assert_eq!(b4k.id.device_idx, 0);

        let b64k = alloc.allocate(BlockSize::B64K).unwrap();
        assert_eq!(b64k.size, BlockSize::B64K);

        let b1m = alloc.allocate(BlockSize::B1M).unwrap();
        assert_eq!(b1m.size, BlockSize::B1M);

        let b64m = alloc.allocate(BlockSize::B64M).unwrap();
        assert_eq!(b64m.size, BlockSize::B64M);
    }

    #[test]
    fn test_allocate_until_full() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 16,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let mut count = 0;
        while alloc.allocate(BlockSize::B4K).is_ok() {
            count += 1;
        }

        assert_eq!(count, 16);

        let result = alloc.allocate(BlockSize::B4K);
        assert!(matches!(result, Err(StorageError::OutOfSpace)));
    }

    #[test]
    fn test_free_and_reallocate() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 16384,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let block = alloc.allocate(BlockSize::B4K).unwrap();

        alloc.free(block).unwrap();

        let block2 = alloc.allocate(BlockSize::B4K);
        assert!(block2.is_ok(), "Should be able to reallocate after free");
    }

    #[test]
    fn test_buddy_merge() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 512,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let block1 = alloc.allocate(BlockSize::B1M).unwrap();
        let block2 = alloc.allocate(BlockSize::B1M).unwrap();

        let offset1 = block1.id.offset;
        let offset2 = block2.id.offset;

        assert_eq!((offset1 ^ offset2), 256);

        alloc.free(block1).unwrap();
        alloc.free(block2).unwrap();

        let stats = alloc.stats();
        let b64m_count = stats
            .free_count_per_size
            .iter()
            .find(|(s, _)| *s == BlockSize::B64M)
            .map(|(_, c)| *c)
            .unwrap_or(0);

        assert_eq!(b64m_count, 1, "Two B1M buddies should merge into one B64M");
    }

    #[test]
    fn test_split_on_demand() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 256,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let b64k = alloc.allocate(BlockSize::B64K).unwrap();
        assert_eq!(b64k.size, BlockSize::B64K);

        for _ in 0..15 {
            let b4k = alloc.allocate(BlockSize::B4K).unwrap();
            assert_eq!(b4k.size, BlockSize::B4K);
        }

        let another_b4k = alloc.allocate(BlockSize::B4K);
        assert!(another_b4k.is_ok(), "Split on demand should work");
    }

    #[test]
    fn test_alignment() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 22404,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        for _ in 0..100 {
            let b4k = alloc.allocate(BlockSize::B4K).unwrap();
            assert!(b4k.id.offset < 22404);
        }

        for _ in 0..50 {
            let b64k = alloc.allocate(BlockSize::B64K).unwrap();
            assert_eq!(b64k.id.offset % 16, 0);
        }

        for _ in 0..20 {
            let b1m = alloc.allocate(BlockSize::B1M).unwrap();
            assert_eq!(b1m.id.offset % 256, 0);
        }

        let b64m = alloc.allocate(BlockSize::B64M).unwrap();
        assert_eq!(b64m.id.offset % 16384, 0);
    }

    #[test]
    fn test_capacity_calculations() {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 16384,
        };
        // Save total_blocks before moving config into BuddyAllocator::new()
        let total_blocks = config.total_blocks_4k;
        let alloc = BuddyAllocator::new(config).unwrap();

        assert_eq!(alloc.total_capacity_bytes(), 16384 * 4096);
        assert_eq!(alloc.free_capacity_bytes(), 16384 * 4096);

        alloc.allocate(BlockSize::B64M).unwrap();

        // BlockSize::B64M = 16384 blocks of 4KB, so after allocation free should be 0
        let blocks_allocated = 16384u64;
        let expected_free = (total_blocks - blocks_allocated) * 4096;
        assert_eq!(alloc.free_capacity_bytes(), expected_free);
    }
}
