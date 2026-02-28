//! Buffer pool for efficient zero-copy data transfer.
//!
//! This module provides a thread-safe buffer pool that pre-allocates fixed-size
//! buffers and recycles them to avoid allocation overhead on the hot path.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::VecDeque;
use tokio::sync::Mutex;

/// Configuration for the buffer pool.
#[derive(Debug, Clone)]
pub struct BufferPoolConfig {
    /// Size of each buffer in bytes.
    pub buffer_size: usize,
    /// Initial number of pre-allocated buffers.
    pub initial_count: usize,
    /// Maximum number of buffers the pool can grow to.
    pub max_count: usize,
}

/// Standard buffer sizes.
pub const BUFFER_4K: usize = 4 * 1024;
/// 64KB buffer size.
pub const BUFFER_64K: usize = 64 * 1024;
/// 1MB buffer size.
pub const BUFFER_1M: usize = 1024 * 1024;
/// 64MB buffer size.
pub const BUFFER_64M: usize = 64 * 1024 * 1024;

/// Statistics for the buffer pool.
#[derive(Debug, Clone)]
pub struct BufferPoolStats {
    /// Total buffers allocated by this pool.
    pub total_allocated: usize,
    /// Buffers currently available in the pool.
    pub available: usize,
    /// Buffers currently checked out.
    pub in_use: usize,
}

/// A thread-safe pool of reusable byte buffers.
pub struct BufferPool {
    config: BufferPoolConfig,
    buffers: Mutex<VecDeque<Vec<u8>>>,
    total_allocated: AtomicUsize,
    in_use: AtomicUsize,
}

impl BufferPool {
    /// Create a new buffer pool with the given configuration.
    pub fn new(config: BufferPoolConfig) -> Arc<Self> {
        let pool = Arc::new(BufferPool {
            config: config.clone(),
            buffers: Mutex::new(VecDeque::new()),
            total_allocated: AtomicUsize::new(0),
            in_use: AtomicUsize::new(0),
        });

        let pool_clone = pool.clone();
        let config_clone = config.clone();

        // Pre-allocate initial buffers
        tokio::runtime::Handle::current().spawn(async move {
            for _ in 0..config_clone.initial_count {
                let buf = vec![0u8; config_clone.buffer_size];
                let mut buffers = pool_clone.buffers.lock().await;
                buffers.push_back(buf);
                pool_clone.total_allocated.fetch_add(1, Ordering::Relaxed);
            }
        });

        pool
    }

    /// Get a buffer from the pool. Creates a new one if pool is empty (up to max).
    pub async fn get(self: &Arc<Self>) -> Option<PooledBuffer> {
        let buf = {
            let mut buffers = self.buffers.lock().await;
            buffers.pop_front()
        };

        match buf {
            Some(buf) => {
                self.in_use.fetch_add(1, Ordering::Relaxed);
                Some(PooledBuffer {
                    buf: Some(buf),
                    pool: Arc::clone(self),
                    len: 0,
                })
            }
            None => {
                // Try to create a new buffer if we haven't hit the limit
                let total = self.total_allocated.load(Ordering::Relaxed);
                if total < self.config.max_count {
                    self.total_allocated.fetch_add(1, Ordering::Relaxed);
                    self.in_use.fetch_add(1, Ordering::Relaxed);
                    Some(PooledBuffer {
                        buf: Some(vec![0u8; self.config.buffer_size]),
                        pool: Arc::clone(self),
                        len: 0,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Return a buffer to the pool.
    pub async fn return_buffer(&self, mut buf: Vec<u8>) {
        // Reset the buffer contents to zero (optional, helps with security)
        buf.resize(self.config.buffer_size, 0);
        
        let mut buffers = self.buffers.lock().await;
        buffers.push_back(buf);
        self.in_use.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get current pool statistics.
    pub async fn stats(&self) -> BufferPoolStats {
        let buffers = self.buffers.lock().await;
        let available = buffers.len();
        let in_use = self.in_use.load(Ordering::Relaxed);
        let total_allocated = self.total_allocated.load(Ordering::Relaxed);

        BufferPoolStats {
            total_allocated,
            available,
            in_use,
        }
    }
}

/// A buffer borrowed from the pool. Automatically returns to pool on drop.
/// Implements Deref<Target=[u8]> and DerefMut for transparent access.
pub struct PooledBuffer {
    buf: Option<Vec<u8>>,
    pool: Arc<BufferPool>,
    /// Actual used length (may be less than buffer capacity).
    len: usize,
}

impl PooledBuffer {
    /// Set the used length of the buffer.
    pub fn set_len(&mut self, len: usize) {
        if let Some(ref buf) = self.buf {
            self.len = len.min(buf.capacity());
        }
    }

    /// Get the used length.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if the buffer has no used data.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the buffer capacity.
    pub fn capacity(&self) -> usize {
        self.buf.as_ref().map(|b| b.capacity()).unwrap_or(0)
    }

    /// Get a slice of the used portion.
    pub fn as_slice(&self) -> &[u8] {
        match &self.buf {
            Some(buf) => &buf[..self.len],
            None => &[],
        }
    }

    /// Get a mutable slice of the used portion.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        match &mut self.buf {
            Some(buf) => &mut buf[..self.len],
            None => &mut [],
        }
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if let Some(buf) = self.buf.take() {
            let pool = self.pool.clone();
            tokio::spawn(async move {
                pool.return_buffer(buf).await;
            });
        }
    }
}

impl Deref for PooledBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_buffer_pool_basic() {
        let config = BufferPoolConfig {
            buffer_size: 4096,
            initial_count: 2,
            max_count: 10,
        };
        let pool = BufferPool::new(config.clone());

        // Wait for initial allocation
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Get a buffer
        let mut buf = pool.get().await.unwrap();
        assert_eq!(buf.capacity(), config.buffer_size);
        assert_eq!(buf.len(), 0);

        // Write some data
        buf.set_len(5);
        buf.as_mut_slice().copy_from_slice(b"hello");

        // Drop the buffer (should return to pool)
        drop(buf);

        // Give time for async return
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Get another buffer - should be the recycled one
        let buf2 = pool.get().await.unwrap();
        assert_eq!(buf2.capacity(), config.buffer_size);

        // Check stats
        let stats = pool.stats().await;
        assert_eq!(stats.total_allocated, 2);
        assert_eq!(stats.in_use, 1);
    }

    #[tokio::test]
    async fn test_buffer_pool_grows() {
        let config = BufferPoolConfig {
            buffer_size: 1024,
            initial_count: 1,
            max_count: 5,
        };
        let pool = BufferPool::new(config.clone());

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Get the only buffer
        let buf1 = pool.get().await.unwrap();
        let stats1 = pool.stats().await;
        assert_eq!(stats1.in_use, 1);
        assert_eq!(stats1.available, 0);

        // Try to get another - should create a new one since we have capacity
        let buf2 = pool.get().await.unwrap();
        let stats2 = pool.stats().await;
        assert_eq!(stats2.in_use, 2);
        assert!(stats2.total_allocated >= 2);

        drop(buf1);
        drop(buf2);
    }

    #[tokio::test]
    async fn test_buffer_pool_max_count() {
        let config = BufferPoolConfig {
            buffer_size: 512,
            initial_count: 1,
            max_count: 2,
        };
        let pool = BufferPool::new(config.clone());

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Get both buffers
        let buf1 = pool.get().await.unwrap();
        let buf2 = pool.get().await.unwrap();

        // Pool is now empty
        let stats = pool.stats().await;
        assert_eq!(stats.in_use, 2);
        assert_eq!(stats.available, 0);

        // Try to get another - should fail since we're at max
        let buf3 = pool.get().await;
        assert!(buf3.is_none());

        drop(buf1);
        drop(buf2);
    }

    #[tokio::test]
    async fn test_buffer_pool_stats() {
        let config = BufferPoolConfig {
            buffer_size: 256,
            initial_count: 3,
            max_count: 10,
        };
        let pool = BufferPool::new(config.clone());

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let buf1 = pool.get().await.unwrap();
        let buf2 = pool.get().await.unwrap();

        let stats = pool.stats().await;
        assert_eq!(stats.total_allocated, 3);
        assert_eq!(stats.in_use, 2);
        assert_eq!(stats.available, 1);

        drop(buf1);
        drop(buf2);

        // Give time for async return to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let stats = pool.stats().await;
        assert_eq!(stats.in_use, 0);
        assert_eq!(stats.available, 3);
    }

    #[tokio::test]
    async fn test_pooled_buffer_auto_return() {
        let config = BufferPoolConfig {
            buffer_size: 128,
            initial_count: 1,
            max_count: 5,
        };
        let pool = BufferPool::new(config.clone());

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Get and drop buffer
        {
            let mut buf = pool.get().await.unwrap();
            buf.set_len(9);
            buf.as_mut_slice().copy_from_slice(b"test data");
        } // buf goes out of scope and should be auto-returned

        // Give time for async return
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let stats = pool.stats().await;
        assert_eq!(stats.in_use, 0);
        assert!(stats.available >= 1);
    }

    #[tokio::test]
    async fn test_different_buffer_sizes() {
        let sizes = [
            (BUFFER_4K, "4K"),
            (BUFFER_64K, "64K"),
            (BUFFER_1M, "1M"),
        ];

        for (size, name) in sizes {
            let config = BufferPoolConfig {
                buffer_size: size,
                initial_count: 1,
                max_count: 2,
            };
            let pool = BufferPool::new(config.clone());

            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            let buf = pool.get().await.unwrap();
            assert_eq!(buf.capacity(), size, "Failed for {}", name);
        }
    }

    #[tokio::test]
    async fn test_pooled_buffer_deref() {
        let config = BufferPoolConfig {
            buffer_size: 256,
            initial_count: 1,
            max_count: 2,
        };
        let pool = BufferPool::new(config.clone());

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let mut buf = pool.get().await.unwrap();
        buf.set_len(10);
        buf.as_mut_slice().copy_from_slice(b"deref test");

        // Test Deref
        let slice: &[u8] = &buf;
        assert_eq!(slice, b"deref test");

        // Test DerefMut
        buf.as_mut_slice()[0] = b'D';
        assert_eq!(&buf[..10], b"Deref test");
    }
}