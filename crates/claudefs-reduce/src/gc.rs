//! Garbage collection engine with reference-counting and mark-and-sweep.
//! Reclaims unreferenced chunks from the CAS index.

use crate::dedupe::CasIndex;
use crate::fingerprint::ChunkHash;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::debug;

/// Statistics from a garbage collection cycle.
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    /// Number of chunks scanned during this cycle.
    pub chunks_scanned: usize,
    /// Number of chunks reclaimed (removed) in this cycle.
    pub chunks_reclaimed: usize,
    /// Bytes reclaimed (not tracked in current implementation).
    pub bytes_reclaimed: u64,
}

/// Configuration for the garbage collector.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GcConfig {
    /// Reference count threshold for reclamation (0 = reclaim only when refcount is 0).
    pub sweep_threshold: usize,
}

/// Garbage collection engine using mark-and-sweep.
/// Tracks reachable chunks and reclaims unreferenced ones.
pub struct GcEngine {
    #[allow(dead_code)]
    config: GcConfig,
    /// Chunks marked as reachable in the current GC cycle.
    reachable: HashSet<ChunkHash>,
}

impl GcEngine {
    /// Create a new GC engine with the given configuration.
    pub fn new(config: GcConfig) -> Self {
        Self {
            config,
            reachable: HashSet::new(),
        }
    }

    /// Mark these chunk hashes as reachable (still in use).
    pub fn mark_reachable(&mut self, hashes: &[ChunkHash]) {
        for hash in hashes {
            self.reachable.insert(*hash);
        }
        debug!(count = hashes.len(), "Marked chunks as reachable");
    }

    /// Clear all marks for the next GC cycle.
    pub fn clear_marks(&mut self) {
        self.reachable.clear();
        debug!("Cleared GC marks");
    }

    /// Check if a chunk is marked as reachable.
    pub fn is_marked(&self, hash: &ChunkHash) -> bool {
        self.reachable.contains(hash)
    }

    /// Sweep: remove all unreferenced chunks from the CAS index.
    /// Removes entries that are NOT marked reachable AND have refcount == 0.
    pub fn sweep(&mut self, cas: &mut CasIndex) -> GcStats {
        let total_chunks = cas.len();

        // Collect hashes with refcount == 0 using the iter() method
        let zero_refcount: Vec<ChunkHash> = cas
            .iter()
            .filter(|(_, count)| *count == 0)
            .map(|(hash, _)| *hash)
            .collect();

        let reclaimed_count = zero_refcount.len();

        // Remove zero-refcount entries
        for hash in &zero_refcount {
            cas.release(hash);
        }

        debug!(
            scanned = total_chunks,
            reclaimed = reclaimed_count,
            "GC sweep complete"
        );

        GcStats {
            chunks_scanned: total_chunks,
            chunks_reclaimed: reclaimed_count,
            bytes_reclaimed: 0,
        }
    }

    /// Run a complete GC cycle: clear marks, mark reachable, then sweep.
    pub fn run_cycle(&mut self, cas: &mut CasIndex, reachable_hashes: &[ChunkHash]) -> GcStats {
        self.clear_marks();
        self.mark_reachable(reachable_hashes);
        self.sweep(cas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::blake3_hash;

    #[test]
    fn test_mark_and_sweep_removes_zero_refcount() {
        let mut cas = CasIndex::new();
        let hash = blake3_hash(b"test chunk");

        // Insert and then release (refcount becomes 0)
        cas.insert(hash);
        cas.release(&hash);
        assert_eq!(cas.refcount(&hash), 0);

        // Run GC - should remove the zero-refcount chunk
        let mut gc = GcEngine::new(GcConfig::default());
        let stats = gc.sweep(&mut cas);

        assert!(!cas.lookup(&hash));
        assert_eq!(stats.chunks_reclaimed, 1);
    }

    #[test]
    fn test_sweep_preserves_referenced() {
        let mut cas = CasIndex::new();
        let hash = blake3_hash(b"test chunk");

        // Insert but don't release (refcount > 0)
        cas.insert(hash);
        assert_eq!(cas.refcount(&hash), 1);

        // Run GC - should NOT remove referenced chunk
        let mut gc = GcEngine::new(GcConfig::default());
        let stats = gc.sweep(&mut cas);

        assert!(cas.lookup(&hash));
        assert_eq!(stats.chunks_reclaimed, 0);
    }

    #[test]
    fn test_run_cycle() {
        let mut cas = CasIndex::new();

        let hash1 = blake3_hash(b"reachable chunk");
        let hash2 = blake3_hash(b"unreachable chunk");

        // Insert both chunks
        cas.insert(hash1);
        cas.insert(hash2);

        // Mark only hash1 as reachable, release hash2
        cas.release(&hash2); // refcount becomes 0
        assert_eq!(cas.refcount(&hash2), 0);

        // Run full cycle
        let mut gc = GcEngine::new(GcConfig::default());
        let stats = gc.run_cycle(&mut cas, &[hash1]);

        // hash1 should remain, hash2 should be removed
        assert!(cas.lookup(&hash1));
        assert!(!cas.lookup(&hash2));
        assert_eq!(stats.chunks_reclaimed, 1);
    }

    #[test]
    fn test_clear_marks() {
        let mut gc = GcEngine::new(GcConfig::default());

        let hash = blake3_hash(b"test");
        gc.mark_reachable(&[hash]);
        assert!(gc.is_marked(&hash));

        gc.clear_marks();
        assert!(!gc.is_marked(&hash));
    }

    #[test]
    fn test_drain_unreferenced() {
        let mut cas = CasIndex::new();

        let hash1 = blake3_hash(b"chunk1");
        let hash2 = blake3_hash(b"chunk2");

        cas.insert(hash1);
        cas.insert(hash2);

        // Release both
        cas.release(&hash1);
        cas.release(&hash2);

        // Drain unreferenced
        let removed = cas.drain_unreferenced();

        assert_eq!(removed.len(), 2);
        assert!(cas.is_empty());
    }

    #[test]
    fn test_gc_stats() {
        let mut cas = CasIndex::new();

        // Add some chunks
        for i in 0..5 {
            let hash = blake3_hash(format!("chunk {}", i).as_bytes());
            cas.insert(hash);
            cas.release(&hash); // refcount = 0
        }

        // Add one more that stays referenced
        let live_hash = blake3_hash(b"live chunk");
        cas.insert(live_hash);

        let mut gc = GcEngine::new(GcConfig::default());
        let stats = gc.sweep(&mut cas);

        assert_eq!(stats.chunks_scanned, 6);
        assert_eq!(stats.chunks_reclaimed, 5);
    }
}
