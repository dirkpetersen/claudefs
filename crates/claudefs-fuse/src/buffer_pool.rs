#![allow(dead_code)]

use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferSize {
    Page4K,
    Block64K,
    Block1M,
}

impl BufferSize {
    pub fn size_bytes(&self) -> usize {
        match self {
            Self::Page4K => 4096,
            Self::Block64K => 65536,
            Self::Block1M => 1048576,
        }
    }
}

pub struct Buffer {
    pub data: Vec<u8>,
    pub size: BufferSize,
    pub id: u64,
}

impl Buffer {
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn clear(&mut self) {
        let n = self.data.len().min(64);
        for b in &mut self.data[..n] {
            *b = 0;
        }
    }
}

#[derive(Debug, Clone)]
pub struct BufferPoolConfig {
    pub max_4k: usize,
    pub max_64k: usize,
    pub max_1m: usize,
}

impl Default for BufferPoolConfig {
    fn default() -> Self {
        Self {
            max_4k: 256,
            max_64k: 64,
            max_1m: 16,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct BufferPoolStats {
    pub alloc_count: u64,
    pub reuse_count: u64,
    pub return_count: u64,
    pub current_4k: usize,
    pub current_64k: usize,
    pub current_1m: usize,
}

impl BufferPoolStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.alloc_count + self.reuse_count;
        if total == 0 {
            0.0
        } else {
            self.reuse_count as f64 / total as f64
        }
    }
}

pub struct BufferPool {
    pool_4k: Vec<Buffer>,
    pool_64k: Vec<Buffer>,
    pool_1m: Vec<Buffer>,
    stats: BufferPoolStats,
    next_id: u64,
    config: BufferPoolConfig,
}

impl BufferPool {
    pub fn new(config: BufferPoolConfig) -> Self {
        Self {
            pool_4k: Vec::new(),
            pool_64k: Vec::new(),
            pool_1m: Vec::new(),
            stats: BufferPoolStats::default(),
            next_id: 0,
            config,
        }
    }

    pub fn acquire(&mut self, size: BufferSize) -> Buffer {
        let pool = match size {
            BufferSize::Page4K => &mut self.pool_4k,
            BufferSize::Block64K => &mut self.pool_64k,
            BufferSize::Block1M => &mut self.pool_1m,
        };
        if let Some(buf) = pool.pop() {
            self.stats.reuse_count += 1;
            self.update_current_stats();
            debug!("buffer_pool: reused buffer id={}", buf.id);
            buf
        } else {
            self.stats.alloc_count += 1;
            let id = self.next_id;
            self.next_id += 1;
            debug!(
                "buffer_pool: allocated new buffer id={} size={:?}",
                id, size
            );
            Buffer {
                data: vec![0u8; size.size_bytes()],
                size,
                id,
            }
        }
    }

    pub fn release(&mut self, buf: Buffer) {
        self.stats.return_count += 1;
        let (pool, max) = match buf.size {
            BufferSize::Page4K => (&mut self.pool_4k, self.config.max_4k),
            BufferSize::Block64K => (&mut self.pool_64k, self.config.max_64k),
            BufferSize::Block1M => (&mut self.pool_1m, self.config.max_1m),
        };
        if pool.len() < max {
            debug!("buffer_pool: returned buffer id={}", buf.id);
            pool.push(buf);
        } else {
            debug!("buffer_pool: dropped buffer id={} (pool full)", buf.id);
        }
        self.update_current_stats();
    }

    fn update_current_stats(&mut self) {
        self.stats.current_4k = self.pool_4k.len();
        self.stats.current_64k = self.pool_64k.len();
        self.stats.current_1m = self.pool_1m.len();
    }

    pub fn stats(&self) -> &BufferPoolStats {
        &self.stats
    }

    pub fn available(&self, size: BufferSize) -> usize {
        match size {
            BufferSize::Page4K => self.pool_4k.len(),
            BufferSize::Block64K => self.pool_64k.len(),
            BufferSize::Block1M => self.pool_1m.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_size_bytes_4k() {
        assert_eq!(BufferSize::Page4K.size_bytes(), 4096);
    }
    #[test]
    fn test_buffer_size_bytes_64k() {
        assert_eq!(BufferSize::Block64K.size_bytes(), 65536);
    }
    #[test]
    fn test_buffer_size_bytes_1m() {
        assert_eq!(BufferSize::Block1M.size_bytes(), 1048576);
    }
    #[test]
    fn test_acquire_allocates_correct_size() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Page4K);
        assert_eq!(buf.len(), 4096);
    }
    #[test]
    fn test_acquire_64k() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Block64K);
        assert_eq!(buf.len(), 65536);
    }
    #[test]
    fn test_acquire_1m() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Block1M);
        assert_eq!(buf.len(), 1048576);
    }
    #[test]
    fn test_release_and_reuse() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Page4K);
        let id = buf.id;
        pool.release(buf);
        let buf2 = pool.acquire(BufferSize::Page4K);
        assert_eq!(buf2.id, id);
    }
    #[test]
    fn test_stats_alloc_count() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        pool.acquire(BufferSize::Page4K);
        pool.acquire(BufferSize::Page4K);
        assert_eq!(pool.stats().alloc_count, 2);
    }
    #[test]
    fn test_stats_reuse_count() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Page4K);
        pool.release(buf);
        pool.acquire(BufferSize::Page4K);
        assert_eq!(pool.stats().reuse_count, 1);
    }
    #[test]
    fn test_stats_return_count() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Page4K);
        pool.release(buf);
        assert_eq!(pool.stats().return_count, 1);
    }
    #[test]
    fn test_hit_rate_zero_when_no_operations() {
        let pool = BufferPool::new(BufferPoolConfig::default());
        assert_eq!(pool.stats().hit_rate(), 0.0);
    }
    #[test]
    fn test_hit_rate_calculation() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Page4K); // alloc
        pool.release(buf);
        pool.acquire(BufferSize::Page4K); // reuse
                                          // hit_rate = 1 / (1 + 1) = 0.5
        assert!((pool.stats().hit_rate() - 0.5).abs() < 1e-9);
    }
    #[test]
    fn test_max_pool_limit_drops_excess() {
        let mut pool = BufferPool::new(BufferPoolConfig {
            max_4k: 2,
            max_64k: 64,
            max_1m: 16,
        });
        let b1 = pool.acquire(BufferSize::Page4K);
        let b2 = pool.acquire(BufferSize::Page4K);
        let b3 = pool.acquire(BufferSize::Page4K);
        pool.release(b1);
        pool.release(b2);
        pool.release(b3); // should be dropped
        assert_eq!(pool.available(BufferSize::Page4K), 2);
    }
    #[test]
    fn test_buffer_clear() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let mut buf = pool.acquire(BufferSize::Page4K);
        buf.as_mut_slice()[0] = 0xFF;
        buf.as_mut_slice()[63] = 0xAB;
        buf.clear();
        assert_eq!(buf.as_slice()[0], 0);
        assert_eq!(buf.as_slice()[63], 0);
    }
    #[test]
    fn test_available_empty_pool() {
        let pool = BufferPool::new(BufferPoolConfig::default());
        assert_eq!(pool.available(BufferSize::Page4K), 0);
    }
    #[test]
    fn test_available_after_release() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Block64K);
        pool.release(buf);
        assert_eq!(pool.available(BufferSize::Block64K), 1);
    }
    #[test]
    fn test_current_stats_updated() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Page4K);
        pool.release(buf);
        assert_eq!(pool.stats().current_4k, 1);
    }
    #[test]
    fn test_default_config() {
        let config = BufferPoolConfig::default();
        assert_eq!(config.max_4k, 256);
        assert_eq!(config.max_64k, 64);
        assert_eq!(config.max_1m, 16);
    }
    #[test]
    fn test_buffer_as_slice_and_as_mut_slice() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let mut buf = pool.acquire(BufferSize::Page4K);
        buf.as_mut_slice()[100] = 42;
        assert_eq!(buf.as_slice()[100], 42);
    }
    #[test]
    fn test_multiple_size_classes_independent() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let b1 = pool.acquire(BufferSize::Page4K);
        let b2 = pool.acquire(BufferSize::Block64K);
        pool.release(b1);
        pool.release(b2);
        assert_eq!(pool.available(BufferSize::Page4K), 1);
        assert_eq!(pool.available(BufferSize::Block64K), 1);
        assert_eq!(pool.available(BufferSize::Block1M), 0);
    }
    #[test]
    fn test_alloc_count_increments_for_new_buffers() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        pool.acquire(BufferSize::Page4K);
        pool.acquire(BufferSize::Block64K);
        pool.acquire(BufferSize::Block1M);
        assert_eq!(pool.stats().alloc_count, 3);
    }
    #[test]
    fn test_buffer_is_empty_false() {
        let mut pool = BufferPool::new(BufferPoolConfig::default());
        let buf = pool.acquire(BufferSize::Page4K);
        assert!(!buf.is_empty());
    }
}
