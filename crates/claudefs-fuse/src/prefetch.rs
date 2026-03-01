use crate::inode::InodeId;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PrefetchConfig {
    pub window_size: usize,
    pub block_size: u64,
    pub max_inflight: usize,
    pub detection_threshold: u32,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            window_size: 8,
            block_size: 65536,
            max_inflight: 4,
            detection_threshold: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrefetchEntry {
    pub ino: InodeId,
    pub offset: u64,
    pub data: Vec<u8>,
    pub ready: bool,
}

#[derive(Debug, Clone)]
pub struct AccessPattern {
    pub last_offset: u64,
    pub sequential_count: u32,
}

impl AccessPattern {
    fn new() -> Self {
        Self {
            last_offset: 0,
            sequential_count: 0,
        }
    }
}

pub struct PrefetchStats {
    pub entries_cached: usize,
    pub inodes_tracked: usize,
    pub sequential_inodes: usize,
}

pub struct PrefetchEngine {
    config: PrefetchConfig,
    patterns: HashMap<InodeId, AccessPattern>,
    buffer: HashMap<(InodeId, u64), PrefetchEntry>,
}

impl PrefetchEngine {
    pub fn new(config: PrefetchConfig) -> Self {
        tracing::debug!(
            "Initializing prefetch engine: window={}, block={}, max_inflight={}",
            config.window_size,
            config.block_size,
            config.max_inflight
        );
        Self {
            config,
            patterns: HashMap::new(),
            buffer: HashMap::new(),
        }
    }

    pub fn record_access(&mut self, ino: InodeId, offset: u64, size: u32) -> u64 {
        let block_offset = self.align_to_block(offset);

        let pattern = self.patterns.entry(ino).or_insert_with(AccessPattern::new);

        if pattern.last_offset > 0 {
            let gap = offset.saturating_sub(pattern.last_offset);

            if gap <= self.config.block_size {
                pattern.sequential_count += 1;
            } else if gap > self.config.block_size * 2 {
                pattern.sequential_count = 0;
                tracing::debug!(
                    "Resetting sequential count for inode {} due to large gap {}",
                    ino,
                    gap
                );
            }
        } else {
            pattern.sequential_count = 1;
        }

        pattern.last_offset = offset + size as u64;

        block_offset
    }

    pub fn is_sequential(&self, ino: InodeId) -> bool {
        self.patterns
            .get(&ino)
            .map(|p| p.sequential_count >= self.config.detection_threshold)
            .unwrap_or(false)
    }

    pub fn compute_prefetch_list(&self, ino: InodeId, current_offset: u64) -> Vec<(InodeId, u64)> {
        if !self.is_sequential(ino) {
            return Vec::new();
        }

        let current_block = self.align_to_block(current_offset);
        let max_range = (self.config.max_inflight as u64) * self.config.block_size;
        let window_size_blocks = self.config.window_size as u64;

        let mut result = Vec::with_capacity(self.config.window_size);

        for i in 1..=window_size_blocks {
            let block_offset = current_block + (i * self.config.block_size);

            if block_offset > current_block + max_range {
                break;
            }

            if !self.buffer.contains_key(&(ino, block_offset)) {
                result.push((ino, block_offset));
            }
        }

        tracing::debug!(
            "Prefetch list for inode {} at offset {}: {} blocks",
            ino,
            current_offset,
            result.len()
        );

        result
    }

    pub fn store_prefetch(&mut self, ino: InodeId, offset: u64, data: Vec<u8>) {
        let aligned = self.align_to_block(offset);
        let entry = PrefetchEntry {
            ino,
            offset: aligned,
            data,
            ready: true,
        };

        self.buffer.insert((ino, aligned), entry);
        tracing::debug!(
            "Stored prefetch entry for inode {} at offset {}",
            ino,
            aligned
        );
    }

    pub fn try_serve(&self, ino: InodeId, offset: u64, size: u32) -> Option<Vec<u8>> {
        let block_offset = self.align_to_block(offset);
        let in_block_offset = offset - block_offset;

        self.buffer.get(&(ino, block_offset)).and_then(|entry| {
            if !entry.ready {
                return None;
            }

            let end_offset = in_block_offset + size as u64;

            if end_offset > entry.data.len() as u64 {
                tracing::debug!(
                    "Partial hit for ino {} block {} offset {} size {}",
                    ino,
                    block_offset,
                    in_block_offset,
                    size
                );
                entry
                    .data
                    .get(in_block_offset as usize..)
                    .map(|d| d.iter().take(size as usize).copied().collect())
            } else {
                tracing::debug!("Full cache hit for ino {} at offset {}", ino, offset);
                entry
                    .data
                    .get(in_block_offset as usize..(in_block_offset + size as u64) as usize)
                    .map(|d| d.to_vec())
            }
        })
    }

    pub fn evict(&mut self, ino: InodeId) {
        let keys: Vec<_> = self
            .buffer
            .keys()
            .filter(|(i, _)| *i == ino)
            .cloned()
            .collect();

        for key in keys {
            self.buffer.remove(&key);
        }

        self.patterns.remove(&ino);

        tracing::debug!("Evicted prefetch state for inode {}", ino);
    }

    pub fn stats(&self) -> PrefetchStats {
        let sequential_count = self
            .patterns
            .values()
            .filter(|p| p.sequential_count >= self.config.detection_threshold)
            .count();

        PrefetchStats {
            entries_cached: self.buffer.len(),
            inodes_tracked: self.patterns.len(),
            sequential_inodes: sequential_count,
        }
    }

    fn align_to_block(&self, offset: u64) -> u64 {
        (offset / self.config.block_size) * self.config.block_size
    }
}

impl Default for PrefetchEngine {
    fn default() -> Self {
        Self::new(PrefetchConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> PrefetchConfig {
        PrefetchConfig {
            window_size: 4,
            block_size: 4096,
            max_inflight: 2,
            detection_threshold: 2,
        }
    }

    #[test]
    fn default_config_has_sensible_values() {
        let config = PrefetchConfig::default();
        assert!(config.window_size > 0, "window_size should be > 0");
        assert!(config.block_size > 0, "block_size should be > 0");
        assert!(config.max_inflight > 0, "max_inflight should be > 0");
        assert!(
            config.detection_threshold > 0,
            "detection_threshold should be > 0"
        );
    }

    #[test]
    fn single_random_access_no_sequential() {
        let mut engine = PrefetchEngine::new(test_config());
        engine.record_access(1, 1000, 512);
        assert!(!engine.is_sequential(1));
    }

    #[test]
    fn two_consecutive_sequential_triggers_detection() {
        let mut engine = PrefetchEngine::new(test_config());
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        assert!(
            engine.is_sequential(1),
            "Two sequential accesses should trigger detection"
        );
    }

    #[test]
    fn three_sequential_returns_window_entries() {
        let mut engine = PrefetchEngine::new(PrefetchConfig {
            window_size: 4,
            block_size: 4096,
            max_inflight: 4,
            detection_threshold: 2,
        });
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        engine.record_access(1, 1024, 512);

        let list = engine.compute_prefetch_list(1, 1024);
        assert_eq!(list.len(), 4, "Should return window_size entries");
    }

    #[test]
    fn prefetch_list_offsets_block_aligned() {
        let mut engine = PrefetchEngine::new(test_config());
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        engine.record_access(1, 1024, 512);

        let list = engine.compute_prefetch_list(1, 1024);
        for (_, offset) in &list {
            assert_eq!(
                *offset % 4096,
                0,
                "Offset {} should be block-aligned",
                offset
            );
        }
    }

    #[test]
    fn store_prefetch_retrievable_by_try_serve() {
        let mut engine = PrefetchEngine::new(test_config());
        let data = vec![1u8; 4096];
        engine.store_prefetch(1, 0, data.clone());

        let result = engine.try_serve(1, 0, 4096);
        assert!(result.is_some(), "Should find cached data");
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn try_serve_returns_none_for_non_cached() {
        let engine = PrefetchEngine::new(test_config());
        let result = engine.try_serve(1, 0, 512);
        assert!(result.is_none(), "Should return None for non-cached offset");
    }

    #[test]
    fn try_serve_returns_correct_data() {
        let mut engine = PrefetchEngine::new(test_config());
        let data = vec![0xAB; 4096];
        engine.store_prefetch(1, 0, data);

        let result = engine.try_serve(1, 0, 4096).unwrap();
        assert!(
            result.iter().all(|&b| b == 0xAB),
            "All bytes should be 0xAB"
        );
    }

    #[test]
    fn evict_removes_all_for_inode() {
        let mut engine = PrefetchEngine::new(test_config());
        engine.store_prefetch(1, 0, vec![1u8; 4096]);
        engine.store_prefetch(1, 4096, vec![2u8; 4096]);
        engine.store_prefetch(2, 0, vec![3u8; 4096]);

        engine.evict(1);

        assert!(engine.try_serve(1, 0, 4096).is_none());
        assert!(
            engine.try_serve(2, 0, 4096).is_some(),
            "Other inode should remain"
        );
    }

    #[test]
    fn stats_reflects_correct_counts() {
        let mut engine = PrefetchEngine::new(test_config());
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        engine.record_access(2, 0, 512);
        engine.store_prefetch(1, 0, vec![1u8; 4096]);
        engine.store_prefetch(2, 0, vec![2u8; 4096]);

        let stats = engine.stats();
        assert_eq!(stats.entries_cached, 2);
        assert_eq!(stats.inodes_tracked, 2);
    }

    #[test]
    fn record_access_on_same_inode_resets_sequential_count_for_large_gap() {
        let mut engine = PrefetchEngine::new(test_config());
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        engine.record_access(1, 1024, 512);
        assert!(engine.is_sequential(1));

        engine.record_access(1, 100000, 512);
        assert!(
            !engine.is_sequential(1),
            "Large gap should reset sequential detection"
        );
    }

    #[test]
    fn large_offset_gap_resets_sequential_detection() {
        let mut engine = PrefetchEngine::new(test_config());
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);

        engine.record_access(1, 20000, 512);

        let pattern = engine.patterns.get(&1).unwrap();
        assert_eq!(pattern.sequential_count, 0);
    }

    #[test]
    fn multiple_inodes_tracked_independently() {
        let mut engine = PrefetchEngine::new(test_config());

        engine.record_access(1, 0, 512);
        engine.record_access(2, 10000, 512);

        assert!(!engine.is_sequential(1));
        assert!(!engine.is_sequential(2));

        engine.record_access(1, 512, 512);

        assert!(engine.is_sequential(1));
        assert!(!engine.is_sequential(2));
    }

    #[test]
    fn prefetch_list_not_exceed_max_inflight_range() {
        let mut engine = PrefetchEngine::new(PrefetchConfig {
            window_size: 8,
            block_size: 4096,
            max_inflight: 2,
            detection_threshold: 2,
        });

        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);

        let list = engine.compute_prefetch_list(1, 512);

        assert!(list.len() <= 2, "Should not exceed max_inflight");
    }

    #[test]
    fn try_serve_partial_sub_block_offset_returns_correct_slice() {
        let mut engine = PrefetchEngine::new(test_config());
        let data: Vec<u8> = (0..4096).map(|i| i as u8).collect();
        engine.store_prefetch(1, 0, data);

        let result = engine.try_serve(1, 100, 200);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.len(), 200);
        assert_eq!(result[0], 100);
        assert_eq!(result[199], 43);
    }

    #[test]
    fn compute_prefetch_list_excludes_already_cached() {
        let mut engine = PrefetchEngine::new(test_config());

        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        engine.record_access(1, 1024, 512);

        engine.store_prefetch(1, 4096, vec![1u8; 4096]);

        let list = engine.compute_prefetch_list(1, 1024);

        assert!(
            !list.iter().any(|(_, o)| *o == 4096),
            "Should exclude cached block"
        );
    }
}
