//! Chunk pool for reusing Vec<u8> allocations in the hot path.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Configuration for the chunk pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Maximum number of buffers to keep in the pool.
    pub max_pooled: usize,
    /// Default chunk size for new allocations.
    pub chunk_size: usize,
    /// Maximum chunk size that can be pooled.
    pub max_chunk_size: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_pooled: 64,
            chunk_size: 16384,
            max_chunk_size: 65536,
        }
    }
}

/// A buffer from the pool with tracked ownership.
#[derive(Debug)]
pub struct PooledBuffer {
    buf: Vec<u8>,
}

impl PooledBuffer {
    /// Returns the buffer data as a slice.
    pub fn data(&self) -> &[u8] {
        &self.buf[..self.buf.len()]
    }

    /// Returns a mutable reference to the underlying Vec.
    pub fn as_mut_slice(&mut self) -> &mut Vec<u8> {
        &mut self.buf
    }
}

/// Statistics for pool performance monitoring.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total allocation requests.
    pub allocations: u64,
    /// Times a buffer was retrieved from the pool.
    pub pool_hits: u64,
    /// Times a new allocation was required.
    pub pool_misses: u64,
    /// Times a buffer was returned to the pool.
    pub returns: u64,
}

impl PoolStats {
    /// Returns the pool hit rate as a fraction (0.0 to 1.0).
    pub fn hit_rate(&self) -> f64 {
        if self.allocations == 0 {
            0.0
        } else {
            self.pool_hits as f64 / self.allocations as f64
        }
    }
}

/// Pool for reusing Vec<u8> allocations to reduce allocations.
pub struct ChunkPool {
    config: PoolConfig,
    pool: VecDeque<Vec<u8>>,
    stats: PoolStats,
}

impl ChunkPool {
    /// Creates a new chunk pool with the given configuration.
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            pool: VecDeque::new(),
            stats: PoolStats::default(),
        }
    }

    /// Acquires a buffer from the pool or allocates a new one.
    pub fn acquire(&mut self, size_hint: usize) -> Vec<u8> {
        self.stats.allocations += 1;

        if size_hint > self.config.max_chunk_size {
            self.stats.pool_misses += 1;
            return vec![0u8; size_hint];
        }

        if let Some(buf) = self.pool.pop_front() {
            self.stats.pool_hits += 1;
            if buf.capacity() >= size_hint {
                buf
            } else {
                vec![0u8; size_hint]
            }
        } else {
            self.stats.pool_misses += 1;
            let capacity = size_hint.max(self.config.chunk_size);
            vec![0u8; capacity]
        }
    }

    /// Releases a buffer back to the pool if space allows.
    pub fn release(&mut self, mut buf: Vec<u8>) -> Option<PooledBuffer> {
        buf.clear();

        if self.pool.len() < self.config.max_pooled {
            self.stats.returns += 1;
            self.pool.push_back(buf);
            None
        } else {
            None
        }
    }

    /// Releases a buffer without returning the PooledBuffer wrapper.
    pub fn release_with_buffer(&mut self, buf: Vec<u8>) {
        let _ = self.release(buf);
    }

    /// Returns the pool statistics.
    pub fn stats(&self) -> &PoolStats {
        &self.stats
    }

    /// Returns the current number of buffers in the pool.
    pub fn pool_size(&self) -> usize {
        self.pool.len()
    }
}

impl Default for ChunkPool {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_pooled, 64);
        assert_eq!(config.chunk_size, 16384);
        assert_eq!(config.max_chunk_size, 65536);
    }

    #[test]
    fn pool_stats_hit_rate_zero() {
        let stats = PoolStats::default();
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn acquire_from_empty_pool() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        let buf = pool.acquire(1024);
        assert!(buf.len() >= 1024);
        assert_eq!(pool.stats().allocations, 1);
    }

    #[test]
    fn release_returns_to_pool() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        let buf = pool.acquire(1024);
        pool.release_with_buffer(buf);
        assert_eq!(pool.pool_size(), 1);
        assert_eq!(pool.stats().returns, 1);
    }

    #[test]
    fn acquire_from_pool_hit() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        let buf1 = pool.acquire(1024);
        pool.release_with_buffer(buf1);
        let _buf2 = pool.acquire(1024);
        assert_eq!(pool.stats().pool_hits, 1);
    }

    #[test]
    fn acquire_stats_miss_count() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        pool.acquire(1024);
        assert_eq!(pool.stats().pool_misses, 1);
    }

    #[test]
    fn acquire_stats_hit_count() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        let buf = pool.acquire(1024);
        pool.release_with_buffer(buf);
        pool.acquire(1024);
        assert_eq!(pool.stats().pool_hits, 1);
    }

    #[test]
    fn release_full_pool_drops() {
        let config = PoolConfig {
            max_pooled: 2,
            chunk_size: 1024,
            max_chunk_size: 4096,
        };
        let mut pool = ChunkPool::new(config);
        let buf1 = pool.acquire(512);
        pool.release_with_buffer(buf1);
        let buf2 = pool.acquire(512);
        pool.release_with_buffer(buf2);
        let third = pool.acquire(512);
        pool.release_with_buffer(third);
        assert_eq!(pool.pool_size(), 1);
    }

    #[test]
    fn pool_size_after_release() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        assert_eq!(pool.pool_size(), 0);
        let buf = pool.acquire(1024);
        pool.release_with_buffer(buf);
        assert_eq!(pool.pool_size(), 1);
    }

    #[test]
    fn pool_size_after_acquire() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        pool.acquire(1024);
        assert_eq!(pool.pool_size(), 0);
    }

    #[test]
    fn acquire_large_size_hint_bypasses_pool() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        let buf = pool.acquire(100000);
        assert_eq!(buf.len(), 100000);
        assert_eq!(pool.stats().pool_misses, 1);
    }

    #[test]
    fn stats_total_allocations() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        pool.acquire(1024);
        pool.acquire(2048);
        pool.acquire(4096);
        assert_eq!(pool.stats().allocations, 3);
    }

    #[test]
    fn stats_returns_count() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        let buf1 = pool.acquire(1024);
        let buf2 = pool.acquire(2048);
        pool.release_with_buffer(buf1);
        pool.release_with_buffer(buf2);
        assert_eq!(pool.stats().returns, 2);
    }

    #[test]
    fn hit_rate_after_hits_and_misses() {
        let mut pool = ChunkPool::new(PoolConfig::default());
        let buf = pool.acquire(1024);
        pool.release_with_buffer(buf);
        pool.acquire(1024);
        pool.acquire(2048);
        pool.acquire(4096);
        assert_eq!(pool.stats().allocations, 4);
        assert_eq!(pool.stats().pool_hits, 1);
        assert!((pool.stats().hit_rate() - 0.25).abs() < 0.001);
    }
}
