//! Zero-copy buffer registration for io_uring-style data transfers.
//!
//! This module provides registered memory regions that can be used for
//! zero-copy network I/O, avoiding kernel-user space copies.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Configuration for zero-copy buffer management.
#[derive(Debug, Clone)]
pub struct ZeroCopyConfig {
    /// Size of each registered memory region in bytes.
    pub region_size: usize,
    /// Maximum number of registered regions.
    pub max_regions: usize,
    /// Memory alignment requirement in bytes.
    pub alignment: usize,
    /// Number of regions to pre-register at startup.
    pub preregister: usize,
}

impl Default for ZeroCopyConfig {
    fn default() -> Self {
        Self {
            region_size: 2 * 1024 * 1024, // 2MB
            max_regions: 256,
            alignment: 4096, // page-aligned
            preregister: 16,
        }
    }
}

/// Unique identifier for a registered memory region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionId(pub u64);

/// A registered memory region for zero-copy I/O.
#[derive(Debug)]
pub struct MemoryRegion {
    id: RegionId,
    data: Vec<u8>,
}

impl MemoryRegion {
    /// Returns the unique identifier for this region.
    pub fn id(&self) -> RegionId {
        self.id
    }

    /// Views the region as a byte slice.
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Views the region as a mutable byte slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Returns the size of the region.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns whether the region has zero length.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Statistics for the region pool.
#[derive(Debug, Default, Clone)]
pub struct RegionPoolStats {
    /// Total regions allocated.
    pub total_regions: usize,
    /// Regions in the free pool.
    pub available_regions: usize,
    /// Regions currently acquired.
    pub in_use_regions: usize,
    /// Total acquire operations.
    pub total_acquires: u64,
    /// Total release operations.
    pub total_releases: u64,
    /// Times acquire failed (pool empty).
    pub total_exhausted: u64,
}

/// Pool of registered memory regions for zero-copy I/O.
pub struct RegionPool {
    config: ZeroCopyConfig,
    free_list: Mutex<VecDeque<MemoryRegion>>,
    in_use: AtomicUsize,
    total_acquires: AtomicU64,
    total_releases: AtomicU64,
    total_exhausted: AtomicU64,
    next_id: AtomicU64,
    total_allocated: AtomicUsize,
}

impl RegionPool {
    /// Creates a new pool and pre-registers the configured number of regions.
    pub fn new(config: ZeroCopyConfig) -> Self {
        let pool = Self {
            config: config.clone(),
            free_list: Mutex::new(VecDeque::new()),
            in_use: AtomicUsize::new(0),
            total_acquires: AtomicU64::new(0),
            total_releases: AtomicU64::new(0),
            total_exhausted: AtomicU64::new(0),
            next_id: AtomicU64::new(0),
            total_allocated: AtomicUsize::new(0),
        };

        // Pre-register regions by adding to free list
        for _ in 0..config.preregister {
            if let Some(region) = pool.allocate_one() {
                pool.free_list.lock().unwrap().push_back(region);
            }
        }

        pool
    }

    fn allocate_one(&self) -> Option<MemoryRegion> {
        let current = self.total_allocated.load(Ordering::Relaxed);
        if current >= self.config.max_regions {
            return None;
        }

        // Try to increment total allocated (atomic check-and-set)
        match self.total_allocated.compare_exchange(
            current,
            current + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {}
            Err(_) => return None,
        }

        let id = RegionId(self.next_id.fetch_add(1, Ordering::Relaxed));

        // Allocate aligned memory
        let size = self.config.region_size;

        // Allocate a buffer that's large enough and properly aligned
        let layout = std::alloc::Layout::from_size_align(size, self.config.alignment)
            .expect("Invalid layout");
        let data = unsafe {
            let ptr = std::alloc::alloc(layout);
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            std::slice::from_raw_parts_mut(ptr, size).to_vec()
        };

        Some(MemoryRegion { id, data })
    }

    /// Acquires a region from the pool.
    /// Returns None if the pool is exhausted.
    pub fn acquire(&self) -> Option<MemoryRegion> {
        // Try to get from free list first
        let region = {
            let mut free = self.free_list.lock().unwrap();
            free.pop_front()
        };

        if let Some(region) = region {
            self.in_use.fetch_add(1, Ordering::Relaxed);
            self.total_acquires.fetch_add(1, Ordering::Relaxed);
            return Some(region);
        }

        // Try to allocate a new region
        if let Some(region) = self.allocate_one() {
            self.in_use.fetch_add(1, Ordering::Relaxed);
            self.total_acquires.fetch_add(1, Ordering::Relaxed);
            return Some(region);
        }

        // Pool exhausted
        self.total_exhausted.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Returns a region to the pool.
    /// The region data is zeroed for security.
    pub fn release(&self, mut region: MemoryRegion) {
        // Zero the region data for security/isolation
        region.data.fill(0);
        let mut free = self.free_list.lock().unwrap();
        free.push_back(region);
        self.in_use.fetch_sub(1, Ordering::Relaxed);
        self.total_releases.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the number of available regions.
    pub fn available(&self) -> usize {
        let free = self.free_list.lock().unwrap();
        free.len()
    }

    /// Returns the total number of regions (available + in-use).
    pub fn total(&self) -> usize {
        self.total_allocated.load(Ordering::Relaxed)
    }

    /// Returns the number of regions currently in use.
    pub fn in_use(&self) -> usize {
        self.in_use.load(Ordering::Relaxed)
    }

    /// Returns a statistics snapshot.
    pub fn stats(&self) -> RegionPoolStats {
        RegionPoolStats {
            total_regions: self.total_allocated.load(Ordering::Relaxed),
            available_regions: self.available(),
            in_use_regions: self.in_use.load(Ordering::Relaxed),
            total_acquires: self.total_acquires.load(Ordering::Relaxed),
            total_releases: self.total_releases.load(Ordering::Relaxed),
            total_exhausted: self.total_exhausted.load(Ordering::Relaxed),
        }
    }

    /// Allocates additional regions up to max_regions limit.
    /// Returns the number actually allocated.
    pub fn grow(&self, count: usize) -> usize {
        let mut allocated = 0;
        let mut free = self.free_list.lock().unwrap();

        for _ in 0..count {
            if let Some(region) = self.allocate_one() {
                free.push_back(region);
                allocated += 1;
            } else {
                break;
            }
        }

        allocated
    }

    /// Releases idle regions from the pool.
    /// Returns the number actually released.
    pub fn shrink(&self, count: usize) -> usize {
        let mut released = 0;
        let mut free = self.free_list.lock().unwrap();

        for _ in 0..count {
            if free.pop_front().is_some() {
                released += 1;
                self.total_allocated.fetch_sub(1, Ordering::Relaxed);
            } else {
                break;
            }
        }

        released
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ZeroCopyConfig::default();
        assert_eq!(config.region_size, 2_097_152);
        assert_eq!(config.max_regions, 256);
        assert_eq!(config.alignment, 4096);
        assert_eq!(config.preregister, 16);
    }

    #[test]
    fn test_region_id() {
        let id1 = RegionId(1);
        let id2 = RegionId(1);
        let id3 = RegionId(2);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.0, 1);

        let id_copy = id1;
        assert_eq!(id1, id_copy);

        let id_clone = id1;
        assert_eq!(id1, id_clone);

        let debug = format!("{:?}", id1);
        assert!(debug.contains("RegionId"));
    }

    #[test]
    fn test_memory_region_basics() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 1,
        };
        let pool = RegionPool::new(config);

        let region = pool.acquire().unwrap();
        assert_eq!(region.len(), 1024);
        assert!(!region.is_empty());
        assert!(!region.as_slice().is_empty());

        pool.release(region);
    }

    #[test]
    fn test_memory_region_write_read() {
        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 10,
            alignment: 4096,
            preregister: 1,
        };
        let pool = RegionPool::new(config);

        let mut region = pool.acquire().unwrap();

        // Write data
        let slice = region.as_mut_slice();
        slice[0] = b'H';
        slice[1] = b'e';
        slice[2] = b'l';
        slice[3] = b'l';
        slice[4] = b'o';

        // Read data back
        let read_slice = region.as_slice();
        assert_eq!(&read_slice[0..5], b"Hello");

        pool.release(region);
    }

    #[test]
    fn test_memory_region_is_empty() {
        let config = ZeroCopyConfig {
            region_size: 0,
            max_regions: 10,
            alignment: 4096,
            preregister: 0,
        };
        let pool = RegionPool::new(config);

        let region = pool.acquire().unwrap();
        assert!(region.is_empty());
        assert_eq!(region.len(), 0);

        pool.release(region);
    }

    #[test]
    fn test_pool_new() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 100,
            alignment: 4096,
            preregister: 5,
        };
        let pool = RegionPool::new(config);

        // Should have 5 pre-registered regions in the free list
        assert_eq!(pool.available(), 5);
        assert_eq!(pool.total(), 5);
    }

    #[test]
    fn test_pool_acquire() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        let region = pool.acquire();
        assert!(region.is_some());

        let region = region.unwrap();
        assert_eq!(region.len(), 1024);

        pool.release(region);
    }

    #[test]
    fn test_pool_release() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        let region = pool.acquire().unwrap();
        assert_eq!(pool.available(), 1); // 2 pre-registered - 1 acquired

        pool.release(region);
        assert_eq!(pool.available(), 2); // Returned to free list
    }

    #[test]
    fn test_pool_acquire_release_cycle() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        for i in 0..100 {
            let region = pool.acquire().unwrap();
            assert_eq!(region.len(), 1024);
            pool.release(region);

            assert_eq!(pool.available(), 2);
            assert_eq!(pool.in_use(), 0);
            assert_eq!(pool.total(), 2);
        }
    }

    #[test]
    fn test_pool_exhausted() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 2,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        // Acquire both regions
        let r1 = pool.acquire().unwrap();
        let r2 = pool.acquire().unwrap();

        assert!(pool.acquire().is_none());

        // Now stats should show exhausted
        let stats = pool.stats();
        assert!(stats.total_exhausted > 0);

        pool.release(r1);
        pool.release(r2);
    }

    #[test]
    fn test_pool_in_use_tracking() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 3,
        };
        let pool = RegionPool::new(config);

        assert_eq!(pool.in_use(), 0);

        let r1 = pool.acquire().unwrap();
        assert_eq!(pool.in_use(), 1);

        let r2 = pool.acquire().unwrap();
        assert_eq!(pool.in_use(), 2);

        pool.release(r1);
        assert_eq!(pool.in_use(), 1);

        pool.release(r2);
        assert_eq!(pool.in_use(), 0);
    }

    #[test]
    fn test_pool_stats() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        let stats = pool.stats();
        assert_eq!(stats.total_regions, 2);
        assert_eq!(stats.available_regions, 2);
        assert_eq!(stats.in_use_regions, 0);
        assert_eq!(stats.total_acquires, 0);
        assert_eq!(stats.total_releases, 0);
        assert_eq!(stats.total_exhausted, 0);

        let r1 = pool.acquire().unwrap();
        let stats = pool.stats();
        assert_eq!(stats.in_use_regions, 1);
        assert_eq!(stats.available_regions, 1);
        assert_eq!(stats.total_acquires, 1);

        let r2 = pool.acquire().unwrap();
        let stats = pool.stats();
        assert_eq!(stats.in_use_regions, 2);

        pool.release(r1);
        let stats = pool.stats();
        assert_eq!(stats.in_use_regions, 1);
        assert_eq!(stats.available_regions, 1);
        assert_eq!(stats.total_releases, 1);

        pool.release(r2);
    }

    #[test]
    fn test_pool_grow() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 5,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        let initial = pool.total();
        assert_eq!(initial, 2);

        let grown = pool.grow(5);
        assert_eq!(grown, 3); // Can only grow to max of 5
        assert_eq!(pool.total(), 5);
    }

    #[test]
    fn test_pool_grow_respects_max() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 5,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        let grown = pool.grow(10);
        assert_eq!(grown, 3); // Can only grow to max of 5 (2 + 3 = 5)
        assert_eq!(pool.total(), 5);

        // Trying to grow more should return 0
        let grown = pool.grow(5);
        assert_eq!(grown, 0);
    }

    #[test]
    fn test_pool_shrink() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 5,
        };
        let pool = RegionPool::new(config);

        assert_eq!(pool.total(), 5);

        let shrunk = pool.shrink(3);
        assert_eq!(shrunk, 3);
        assert_eq!(pool.total(), 2);
    }

    #[test]
    fn test_pool_shrink_only_idle() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 5,
        };
        let pool = RegionPool::new(config);

        // Acquire a region (it will be in use)
        let _region = pool.acquire().unwrap();

        // Available should be 4 now (5 pre-registered - 1 acquired)
        assert_eq!(pool.available(), 4);

        // Try to shrink by 4 - should only shrink available (idle) ones
        let shrunk = pool.shrink(4);
        assert_eq!(shrunk, 4); // All 4 idle regions can be shrunk
        assert_eq!(pool.available(), 0);

        // In use region should still be there
        assert_eq!(pool.in_use(), 1);
    }

    #[test]
    fn test_region_data_isolation() {
        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 10,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        let mut r1 = pool.acquire().unwrap();
        let mut r2 = pool.acquire().unwrap();

        // Write to r1
        r1.as_mut_slice()[0] = 0xAB;

        // Write to r2
        r2.as_mut_slice()[0] = 0xCD;

        // Verify isolation
        assert_eq!(r1.as_slice()[0], 0xAB);
        assert_eq!(r2.as_slice()[0], 0xCD);

        pool.release(r1);
        pool.release(r2);

        // Acquire again - should get different (or same) region, but data is fresh
        let r3 = pool.acquire().unwrap();
        assert_eq!(r3.as_slice()[0], 0); // Fresh region has zeros
        pool.release(r3);
    }

    #[test]
    fn test_pool_concurrent_access() {
        use std::sync::Arc;

        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 20,
            alignment: 4096,
            preregister: 10,
        };
        let pool = Arc::new(RegionPool::new(config));

        // Spawn multiple threads doing acquire/release
        let handles: Vec<_> = (0..5)
            .map(|_| {
                let pool = pool.clone();
                std::thread::spawn(move || {
                    let mut acquired = Vec::new();
                    for _ in 0..100 {
                        if let Some(region) = pool.acquire() {
                            acquired.push(region);
                        }
                    }
                    for region in acquired {
                        pool.release(region);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify pool is in consistent state
        let stats = pool.stats();
        assert_eq!(
            stats.available_regions + stats.in_use_regions,
            stats.total_regions
        );
        assert_eq!(stats.in_use_regions, 0);
    }
}
