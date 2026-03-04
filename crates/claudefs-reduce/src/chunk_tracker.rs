use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChunkState {
    Live,
    Orphaned,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRecord {
    pub hash: [u8; 32],
    pub ref_count: u32,
    pub size_bytes: u32,
    pub state: ChunkState,
    pub segment_id: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TrackerStats {
    pub total_chunks: u64,
    pub live_chunks: u64,
    pub orphaned_chunks: u64,
    pub deleted_chunks: u64,
    pub total_bytes: u64,
}

pub struct ChunkTracker {
    chunks: HashMap<[u8; 32], ChunkRecord>,
    stats: TrackerStats,
}

impl Default for ChunkTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkTracker {
    pub fn new() -> Self {
        Self {
            chunks: HashMap::new(),
            stats: TrackerStats::default(),
        }
    }

    pub fn register(&mut self, hash: [u8; 32], size_bytes: u32, segment_id: u64) {
        if let Some(record) = self.chunks.get_mut(&hash) {
            record.ref_count += 1;
        } else {
            self.chunks.insert(
                hash,
                ChunkRecord {
                    hash,
                    ref_count: 1,
                    size_bytes,
                    state: ChunkState::Live,
                    segment_id,
                },
            );
            self.stats.total_chunks += 1;
            self.stats.live_chunks += 1;
            self.stats.total_bytes += size_bytes as u64;
        }
    }

    pub fn inc_ref(&mut self, hash: &[u8; 32]) -> bool {
        if let Some(record) = self.chunks.get_mut(hash) {
            record.ref_count += 1;
            true
        } else {
            false
        }
    }

    pub fn dec_ref(&mut self, hash: &[u8; 32]) -> Option<u32> {
        if let Some(record) = self.chunks.get_mut(hash) {
            if record.ref_count > 0 {
                record.ref_count -= 1;
                let new_count = record.ref_count;

                if new_count == 0 {
                    record.state = ChunkState::Orphaned;
                    if let Some(stats) =
                        self.stats.total_bytes.checked_sub(record.size_bytes as u64)
                    {
                        self.stats.total_bytes = stats;
                    }
                    self.stats.live_chunks = self.stats.live_chunks.saturating_sub(1);
                    self.stats.orphaned_chunks += 1;
                }

                Some(new_count)
            } else {
                Some(0)
            }
        } else {
            None
        }
    }

    pub fn delete_orphaned(&mut self) -> usize {
        let mut count = 0;
        for record in self.chunks.values_mut() {
            if record.state == ChunkState::Orphaned {
                record.state = ChunkState::Deleted;
                self.stats.orphaned_chunks = self.stats.orphaned_chunks.saturating_sub(1);
                self.stats.deleted_chunks += 1;
                count += 1;
            }
        }
        count
    }

    pub fn get(&self, hash: &[u8; 32]) -> Option<&ChunkRecord> {
        self.chunks.get(hash)
    }

    pub fn orphaned_chunks(&self) -> Vec<&ChunkRecord> {
        self.chunks
            .values()
            .filter(|r| r.state == ChunkState::Orphaned)
            .collect()
    }

    pub fn stats(&self) -> TrackerStats {
        self.stats.clone()
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_tracker_new_empty() {
        let tracker = ChunkTracker::new();
        assert!(tracker.is_empty());
    }

    #[test]
    fn register_chunk() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        assert_eq!(tracker.len(), 1);
    }

    #[test]
    fn register_chunk_state_live() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        let record = tracker.get(&hash).unwrap();
        assert_eq!(record.state, ChunkState::Live);
    }

    #[test]
    fn register_chunk_ref_count_one() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        let record = tracker.get(&hash).unwrap();
        assert_eq!(record.ref_count, 1);
    }

    #[test]
    fn inc_ref_existing() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        let result = tracker.inc_ref(&hash);
        assert!(result);
        let record = tracker.get(&hash).unwrap();
        assert_eq!(record.ref_count, 2);
    }

    #[test]
    fn inc_ref_nonexistent() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        let result = tracker.inc_ref(&hash);
        assert!(!result);
    }

    #[test]
    fn dec_ref_to_zero() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        let result = tracker.dec_ref(&hash);
        assert_eq!(result, Some(0));
        let record = tracker.get(&hash).unwrap();
        assert_eq!(record.state, ChunkState::Orphaned);
    }

    #[test]
    fn dec_ref_above_zero() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        tracker.inc_ref(&hash);
        let result = tracker.dec_ref(&hash);
        assert_eq!(result, Some(1));
        let record = tracker.get(&hash).unwrap();
        assert_eq!(record.state, ChunkState::Live);
    }

    #[test]
    fn dec_ref_nonexistent() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        let result = tracker.dec_ref(&hash);
        assert!(result.is_none());
    }

    #[test]
    fn delete_orphaned_clears() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        tracker.dec_ref(&hash);
        tracker.delete_orphaned();
        let record = tracker.get(&hash).unwrap();
        assert_eq!(record.state, ChunkState::Deleted);
    }

    #[test]
    fn delete_orphaned_count() {
        let mut tracker = ChunkTracker::new();
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];
        tracker.register(hash1, 4096, 1);
        tracker.register(hash2, 4096, 1);
        tracker.dec_ref(&hash1);
        tracker.dec_ref(&hash2);
        let count = tracker.delete_orphaned();
        assert_eq!(count, 2);
    }

    #[test]
    fn orphaned_chunks_list() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        tracker.dec_ref(&hash);
        let orphaned = tracker.orphaned_chunks();
        assert_eq!(orphaned.len(), 1);
    }

    #[test]
    fn orphaned_chunks_empty_when_none() {
        let tracker = ChunkTracker::new();
        let orphaned = tracker.orphaned_chunks();
        assert!(orphaned.is_empty());
    }

    #[test]
    fn stats_total_chunks() {
        let mut tracker = ChunkTracker::new();
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];
        tracker.register(hash1, 4096, 1);
        tracker.register(hash2, 4096, 1);
        let stats = tracker.stats();
        assert_eq!(stats.total_chunks, 2);
    }

    #[test]
    fn stats_live_chunks() {
        let mut tracker = ChunkTracker::new();
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];
        tracker.register(hash1, 4096, 1);
        tracker.register(hash2, 4096, 1);
        tracker.dec_ref(&hash1);
        let stats = tracker.stats();
        assert_eq!(stats.live_chunks, 1);
    }

    #[test]
    fn stats_orphaned_chunks() {
        let mut tracker = ChunkTracker::new();
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];
        tracker.register(hash1, 4096, 1);
        tracker.register(hash2, 4096, 1);
        tracker.dec_ref(&hash1);
        tracker.dec_ref(&hash2);
        let stats = tracker.stats();
        assert_eq!(stats.orphaned_chunks, 2);
    }

    #[test]
    fn stats_deleted_chunks() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        tracker.dec_ref(&hash);
        tracker.delete_orphaned();
        let stats = tracker.stats();
        assert_eq!(stats.deleted_chunks, 1);
    }

    #[test]
    fn stats_total_bytes() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        let stats = tracker.stats();
        assert_eq!(stats.total_bytes, 4096);
    }

    #[test]
    fn multiple_chunks_lifecycle() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];

        tracker.register(hash, 4096, 1);
        assert_eq!(tracker.stats().live_chunks, 1);

        tracker.inc_ref(&hash);
        assert_eq!(tracker.get(&hash).unwrap().ref_count, 2);

        tracker.dec_ref(&hash);
        assert_eq!(tracker.get(&hash).unwrap().ref_count, 1);

        tracker.dec_ref(&hash);
        assert_eq!(tracker.get(&hash).unwrap().state, ChunkState::Orphaned);
        assert_eq!(tracker.stats().orphaned_chunks, 1);

        tracker.delete_orphaned();
        assert_eq!(tracker.get(&hash).unwrap().state, ChunkState::Deleted);
    }

    #[test]
    fn register_duplicate_hash() {
        let mut tracker = ChunkTracker::new();
        let hash = [0u8; 32];
        tracker.register(hash, 4096, 1);
        tracker.register(hash, 4096, 1);
        let record = tracker.get(&hash).unwrap();
        assert_eq!(record.ref_count, 2);
        assert_eq!(tracker.len(), 1);
    }

    #[test]
    fn chunk_state_equality() {
        assert_eq!(ChunkState::Live, ChunkState::Live);
        assert_eq!(ChunkState::Orphaned, ChunkState::Orphaned);
        assert_eq!(ChunkState::Deleted, ChunkState::Deleted);
        assert_ne!(ChunkState::Live, ChunkState::Orphaned);
    }
}
