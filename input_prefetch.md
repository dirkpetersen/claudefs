# Task: Implement prefetch_engine.rs module for claudefs-storage

## Location
Create file: `/home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs`

## Purpose
Sequential read-ahead prediction engine. Detects sequential access patterns and pre-fetches blocks before they are requested. Used by the FUSE client layer to reduce read latency for large file workloads.

## Requirements

### Data Structures

```rust
/// Configuration for the prefetch engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchConfig {
    /// Minimum consecutive sequential accesses to trigger prefetch
    pub sequential_threshold: usize,  // default: 3
    /// Number of blocks to prefetch ahead when pattern detected
    pub lookahead_blocks: usize,      // default: 8
    /// Maximum number of streams to track simultaneously
    pub max_streams: usize,           // default: 1024
    /// How many accesses to keep in history window per stream
    pub history_window: usize,        // default: 16
    /// Minimum confidence to issue prefetch (0.0-1.0)
    pub confidence_threshold: f64,    // default: 0.6
}

impl Default for PrefetchConfig { ... }

/// A hint about which block to prefetch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchHint {
    pub offset: u64,
    pub size: u64,
}

/// Statistics about prefetch engine operation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrefetchStats {
    pub streams_tracked: usize,
    pub predictions_issued: u64,
    pub sequential_streams_detected: u64,
    pub random_streams_detected: u64,
    pub streams_cancelled: u64,
}
```

### PrefetchEngine

```rust
/// The main prefetch prediction engine
pub struct PrefetchEngine {
    config: PrefetchConfig,
    // Internal tracking: HashMap<stream_id, StreamState>
    // StreamState includes: history Vec<Access>, confidence f64, is_sequential bool, last_access offset/size
}

impl PrefetchEngine {
    pub fn new(config: PrefetchConfig) -> Self;
    
    /// Record a block access for a stream
    pub fn record_access(&mut self, stream_id: u64, block_offset: u64, size: u64);
    
    /// Get prefetch hints for a stream (blocks to prefetch)
    pub fn get_prefetch_advice(&self, stream_id: u64) -> Vec<PrefetchHint>;
    
    /// Stop tracking a stream (file closed)
    pub fn cancel_stream(&mut self, stream_id: u64);
    
    /// Get statistics
    pub fn stats(&self) -> PrefetchStats;
}
```

### Algorithm Details

1. **Stream Tracking**: Each stream (u64 stream_id) has:
   - Recent access history (circular buffer or Vec capped at `history_window`)
   - Confidence score (0.0-1.0)
   - Flag whether currently detected as sequential
   - Last access offset and size for detecting sequential patterns

2. **Sequential Detection**: 
   - Track last N accesses (history_window)
   - Count consecutive accesses where `new_offset == last_offset + last_size` (assuming size is in bytes/blocks)
   - If consecutive_count >= sequential_threshold, mark stream as sequential

3. **Confidence Scoring**:
   - Start at 0.5 (neutral)
   - Increment by 0.1 (capped at 1.0) for each sequential hit
   - Decrement by 0.2 (floored at 0.0) for each random miss (non-sequential access)
   - Prefetch only issued if confidence >= confidence_threshold

4. **Prefetch Hints**:
   - When stream is sequential and confidence >= threshold:
   - Return `lookahead_blocks` blocks starting from `last_offset + last_size`
   - Each hint has size based on the typical size seen in history (or default 4KB/64KB)

5. **LRU Eviction**:
   - If `streams.len() >= max_streams`, evict the least recently used stream (lowest last_access_time or similar)
   - Use timestamp or access counter for LRU tracking

### Test Coverage (at least 20 unit tests)

Implement these tests in an inline `#[cfg(test)] mod tests` block:

1. Default config values are correct
2. Record accesses for a stream, detect sequential pattern after threshold
3. No prefetch for random accesses (non-sequential offsets)
4. Prefetch hints have correct offsets (next N blocks after current position)
5. Confidence increases on sequential hits, decreases on random
6. Cancel stream removes it from tracking
7. Max streams limit: exceeding max_streams evicts least-recently-used stream
8. Empty history returns no prefetch advice
9. Stats track predictions correctly
10. Single access: no pattern yet, no prefetch advice
11. Two sequential accesses: below threshold, no prefetch
12. Exactly at threshold: prefetch triggered
13. Variable block sizes (4K, 64K, 1M) work correctly
14. Multiple independent streams don't interfere
15. After random miss, confidence drops below threshold → no prefetch
16. Re-detection: after random break, sequential pattern can be re-detected
17. Prefetch hints don't exceed max block offset (u64::MAX safety check)
18. Stats.sequential_streams_detected increments when pattern detected
19. Stats.random_streams_detected increments on first random access
20. Cancelled streams are not counted in streams_tracked

## Style Rules
- All public structs/enums/fns MUST have `///` doc comments
- Use `thiserror` for any errors
- Use `serde` + `bincode` derives: `#[derive(Debug, Clone, Serialize, Deserialize)]`
- Use `tracing` crate: `use tracing::{debug, info, warn, error};`
- Use `std::collections::HashMap` for stream tracking
- No `unwrap()` in production code, use `?` or proper error handling
- Tests use `#[test]` (sync), not async
- Idiomatic Rust: iterators, no manual index loops

## Output
Return the complete Rust code for prefetch_engine.rs with all structs, impls, and tests.