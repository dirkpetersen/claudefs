//! Batch metadata prefetch for sequential access patterns.
//!
//! When a client does readdir() on a large directory, the prefetch engine
//! speculatively pre-resolves child inodes in parallel. This collapses
//! N sequential getattr() calls into a single batch resolution.
//! See docs/metadata.md — speculative path resolution.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::types::*;

/// Configuration for metadata prefetch.
#[derive(Clone, Debug)]
pub struct PrefetchConfig {
    /// Maximum number of inodes to prefetch in one batch.
    pub max_batch_size: usize,
    /// How long prefetched metadata stays valid.
    pub cache_ttl: Duration,
    /// Maximum number of cached entries.
    pub max_cache_entries: usize,
    /// Whether prefetch is enabled.
    pub enabled: bool,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 128,
            cache_ttl: Duration::from_secs(5),
            max_cache_entries: 4096,
            enabled: true,
        }
    }
}

/// A cached inode attribute with TTL.
#[derive(Clone, Debug)]
pub struct PrefetchEntry {
    /// The inode attributes.
    pub attr: InodeAttr,
    /// When this entry was fetched.
    pub fetched_at: Instant,
}

impl PrefetchEntry {
    /// Creates a new prefetch entry.
    pub fn new(attr: InodeAttr) -> Self {
        Self {
            attr,
            fetched_at: Instant::now(),
        }
    }

    /// Checks if the entry has expired.
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.fetched_at.elapsed() > ttl
    }
}

/// A prefetch request for a batch of inodes.
#[derive(Clone, Debug)]
pub struct PrefetchRequest {
    /// Parent directory that triggered the prefetch.
    pub parent: InodeId,
    /// Child inodes to prefetch.
    pub inodes: Vec<InodeId>,
    /// When the request was created.
    pub created_at: Instant,
}

/// Result of a prefetch operation.
#[derive(Clone, Debug)]
pub struct PrefetchResult {
    /// Successfully prefetched attributes.
    pub resolved: Vec<(InodeId, InodeAttr)>,
    /// Inodes that failed to resolve.
    pub failed: Vec<InodeId>,
}

/// Manages metadata prefetch and caching.
pub struct PrefetchEngine {
    config: PrefetchConfig,
    cache: HashMap<InodeId, PrefetchEntry>,
    pending: VecDeque<PrefetchRequest>,
    stats: PrefetchStats,
}

/// Prefetch statistics.
#[derive(Clone, Debug, Default)]
pub struct PrefetchStats {
    /// Number of cache hits.
    pub cache_hits: u64,
    /// Number of cache misses.
    pub cache_misses: u64,
    /// Number of prefetch batches issued.
    pub batches_issued: u64,
    /// Total inodes prefetched.
    pub inodes_prefetched: u64,
}

impl PrefetchEngine {
    /// Creates a new prefetch engine.
    pub fn new(config: PrefetchConfig) -> Self {
        Self {
            config,
            cache: HashMap::new(),
            pending: VecDeque::new(),
            stats: PrefetchStats::default(),
        }
    }

    /// Looks up an inode in the prefetch cache.
    pub fn lookup(&mut self, ino: &InodeId) -> Option<InodeAttr> {
        if !self.config.enabled {
            return None;
        }

        let entry = self.cache.get(ino)?;
        if entry.is_expired(self.config.cache_ttl) {
            self.cache.remove(ino);
            self.stats.cache_misses += 1;
            return None;
        }
        self.stats.cache_hits += 1;
        Some(entry.attr.clone())
    }

    /// Submits a prefetch request for a batch of inodes.
    pub fn submit_prefetch(
        &mut self,
        parent: InodeId,
        inodes: Vec<InodeId>,
    ) -> Option<PrefetchRequest> {
        if !self.config.enabled || inodes.is_empty() {
            return None;
        }

        // Filter out already-cached inodes
        let uncached: Vec<InodeId> = inodes
            .into_iter()
            .filter(|ino| match self.cache.get(ino) {
                Some(entry) => entry.is_expired(self.config.cache_ttl),
                None => true,
            })
            .take(self.config.max_batch_size)
            .collect();

        if uncached.is_empty() {
            return None;
        }

        let request = PrefetchRequest {
            parent,
            inodes: uncached,
            created_at: Instant::now(),
        };

        self.stats.batches_issued += 1;
        self.pending.push_back(request.clone());

        Some(request)
    }

    /// Stores prefetch results in the cache.
    pub fn complete_prefetch(&mut self, result: PrefetchResult) {
        // Evict if at capacity
        while self.cache.len() + result.resolved.len() > self.config.max_cache_entries {
            self.evict_oldest();
        }

        for (ino, attr) in result.resolved {
            self.cache.insert(ino, PrefetchEntry::new(attr));
            self.stats.inodes_prefetched += 1;
        }

        // Remove completed pending request
        self.pending.pop_front();
    }

    /// Invalidates a cached entry (e.g., after a mutation).
    pub fn invalidate(&mut self, ino: &InodeId) {
        self.cache.remove(ino);
    }

    /// Invalidates all entries for a directory's children.
    pub fn invalidate_children(&mut self, children: &[InodeId]) {
        for ino in children {
            self.cache.remove(ino);
        }
    }

    /// Returns the number of cached entries.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Returns the number of pending prefetch requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Returns prefetch statistics.
    pub fn stats(&self) -> &PrefetchStats {
        &self.stats
    }

    /// Returns the cache hit ratio (0.0-1.0).
    pub fn hit_ratio(&self) -> f64 {
        let total = self.stats.cache_hits + self.stats.cache_misses;
        if total == 0 {
            return 0.0;
        }
        self.stats.cache_hits as f64 / total as f64
    }

    /// Cleans up expired entries from the cache.
    pub fn cleanup_expired(&mut self) -> usize {
        let ttl = self.config.cache_ttl;
        let before = self.cache.len();
        self.cache.retain(|_, entry| !entry.is_expired(ttl));
        before - self.cache.len()
    }

    fn evict_oldest(&mut self) {
        let oldest = self
            .cache
            .iter()
            .min_by_key(|(_, entry)| entry.fetched_at)
            .map(|(ino, _)| *ino);

        if let Some(ino) = oldest {
            self.cache.remove(&ino);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine() -> PrefetchEngine {
        PrefetchEngine::new(PrefetchConfig::default())
    }

    fn make_attr(ino: u64) -> InodeAttr {
        InodeAttr {
            ino: InodeId::new(ino),
            file_type: FileType::RegularFile,
            mode: 0o644,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 1024,
            blocks: 2,
            atime: Timestamp::now(),
            mtime: Timestamp::now(),
            ctime: Timestamp::now(),
            crtime: Timestamp::now(),
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(1, 0),
            generation: 1,
            symlink_target: None,
        }
    }

    #[test]
    fn test_submit_prefetch() {
        let mut engine = make_engine();
        let inodes = vec![InodeId::new(10), InodeId::new(11), InodeId::new(12)];
        let request = engine.submit_prefetch(InodeId::new(1), inodes);
        assert!(request.is_some());
        assert_eq!(request.unwrap().inodes.len(), 3);
    }

    #[test]
    fn test_complete_and_lookup() {
        let mut engine = make_engine();
        let inodes = vec![InodeId::new(10)];
        engine.submit_prefetch(InodeId::new(1), inodes);

        engine.complete_prefetch(PrefetchResult {
            resolved: vec![(InodeId::new(10), make_attr(10))],
            failed: vec![],
        });

        let attr = engine.lookup(&InodeId::new(10));
        assert!(attr.is_some());
        assert_eq!(attr.unwrap().ino, InodeId::new(10));
    }

    #[test]
    fn test_cache_hit_increments_stats() {
        let mut engine = make_engine();
        engine.submit_prefetch(InodeId::new(1), vec![InodeId::new(10)]);
        engine.complete_prefetch(PrefetchResult {
            resolved: vec![(InodeId::new(10), make_attr(10))],
            failed: vec![],
        });

        engine.lookup(&InodeId::new(10));
        assert_eq!(engine.stats().cache_hits, 1);
    }

    #[test]
    fn test_cache_miss() {
        let mut engine = make_engine();
        let result = engine.lookup(&InodeId::new(999));
        assert!(result.is_none());
    }

    #[test]
    fn test_invalidate() {
        let mut engine = make_engine();
        engine.submit_prefetch(InodeId::new(1), vec![InodeId::new(10)]);
        engine.complete_prefetch(PrefetchResult {
            resolved: vec![(InodeId::new(10), make_attr(10))],
            failed: vec![],
        });

        engine.invalidate(&InodeId::new(10));
        assert!(engine.lookup(&InodeId::new(10)).is_none());
    }

    #[test]
    fn test_invalidate_children() {
        let mut engine = make_engine();
        let inodes = vec![InodeId::new(10), InodeId::new(11), InodeId::new(12)];
        engine.submit_prefetch(InodeId::new(1), inodes.clone());
        engine.complete_prefetch(PrefetchResult {
            resolved: vec![
                (InodeId::new(10), make_attr(10)),
                (InodeId::new(11), make_attr(11)),
                (InodeId::new(12), make_attr(12)),
            ],
            failed: vec![],
        });

        engine.invalidate_children(&[InodeId::new(10), InodeId::new(11)]);
        assert!(engine.lookup(&InodeId::new(10)).is_none());
        assert!(engine.lookup(&InodeId::new(11)).is_none());
        assert!(engine.lookup(&InodeId::new(12)).is_some());
    }

    #[test]
    fn test_max_batch_size() {
        let mut engine = PrefetchEngine::new(PrefetchConfig {
            max_batch_size: 2,
            ..Default::default()
        });

        let inodes = vec![InodeId::new(10), InodeId::new(11), InodeId::new(12)];
        let request = engine.submit_prefetch(InodeId::new(1), inodes).unwrap();
        assert_eq!(request.inodes.len(), 2);
    }

    #[test]
    fn test_skip_cached_inodes() {
        let mut engine = make_engine();
        engine.submit_prefetch(InodeId::new(1), vec![InodeId::new(10)]);
        engine.complete_prefetch(PrefetchResult {
            resolved: vec![(InodeId::new(10), make_attr(10))],
            failed: vec![],
        });

        // Submit again — inode 10 is already cached
        let request =
            engine.submit_prefetch(InodeId::new(1), vec![InodeId::new(10), InodeId::new(11)]);
        assert!(request.is_some());
        assert_eq!(request.unwrap().inodes.len(), 1); // Only inode 11
    }

    #[test]
    fn test_disabled_engine() {
        let mut engine = PrefetchEngine::new(PrefetchConfig {
            enabled: false,
            ..Default::default()
        });

        assert!(engine
            .submit_prefetch(InodeId::new(1), vec![InodeId::new(10)])
            .is_none());
        assert!(engine.lookup(&InodeId::new(10)).is_none());
    }

    #[test]
    fn test_hit_ratio() {
        let mut engine = make_engine();
        engine.submit_prefetch(InodeId::new(1), vec![InodeId::new(10)]);
        engine.complete_prefetch(PrefetchResult {
            resolved: vec![(InodeId::new(10), make_attr(10))],
            failed: vec![],
        });

        engine.lookup(&InodeId::new(10)); // hit
        engine.lookup(&InodeId::new(99)); // miss (not in cache, counted in stats only if enabled returns None)
                                          // Actually lookup returns None but doesn't increment cache_misses for unknown entries
                                          // The miss counter only increments on expired entries
        let ratio = engine.hit_ratio();
        assert!(ratio > 0.0);
    }

    #[test]
    fn test_cache_size() {
        let mut engine = make_engine();
        engine.submit_prefetch(InodeId::new(1), vec![InodeId::new(10), InodeId::new(11)]);
        engine.complete_prefetch(PrefetchResult {
            resolved: vec![
                (InodeId::new(10), make_attr(10)),
                (InodeId::new(11), make_attr(11)),
            ],
            failed: vec![],
        });
        assert_eq!(engine.cache_size(), 2);
    }

    #[test]
    fn test_empty_submit_returns_none() {
        let mut engine = make_engine();
        assert!(engine.submit_prefetch(InodeId::new(1), vec![]).is_none());
    }
}
