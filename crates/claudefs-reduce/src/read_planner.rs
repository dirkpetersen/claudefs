//! Read planning: determine which chunks to fetch and from where.

/// A file read request.
#[derive(Debug, Clone, Copy)]
pub struct ReadRequest {
    /// Inode ID of the file
    pub inode_id: u64,
    /// Byte offset to start reading from
    pub offset: u64,
    /// Number of bytes to read
    pub length: u64,
}

/// Plan for fetching a single chunk.
#[derive(Debug, Clone)]
pub struct ChunkFetchPlan {
    /// Hash of the chunk content
    pub chunk_hash: [u8; 32],
    /// Node ID where the chunk can be fetched from
    pub node_id: u64,
    /// Segment ID containing the chunk
    pub segment_id: u64,
    /// Whether this chunk is available in the read cache
    pub from_cache: bool,
}

/// Complete read plan for a request.
#[derive(Debug)]
pub struct ReadPlan {
    /// The original request
    pub request: ReadRequest,
    /// Fetch plans for all chunks needed
    pub fetches: Vec<ChunkFetchPlan>,
    /// Number of chunks found in cache
    pub cache_hits: usize,
    /// Number of chunks not in cache
    pub cache_misses: usize,
}

impl ReadPlan {
    /// Total number of chunks to fetch.
    pub fn total_chunks(&self) -> usize {
        self.fetches.len()
    }

    /// Cache hit rate as a fraction (0.0 to 1.0).
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }
}

/// Information about a cached chunk.
#[derive(Debug, Clone)]
pub struct CachedChunkInfo {
    /// Hash of the chunk content
    pub chunk_hash: [u8; 32],
    /// Whether the chunk is currently cached
    pub cached: bool,
}

/// Plans read operations for the FUSE client.
#[derive(Debug, Default)]
pub struct ReadPlanner {
    // Future: could hold node topology, latency hints, etc.
}

impl ReadPlanner {
    /// Create a new read planner.
    pub fn new() -> Self {
        Self {}
    }

    /// Plan a read operation.
    ///
    /// `available_chunks` is a list of (chunk_info, node_id, segment_id) tuples.
    /// Returns a ReadPlan with fetches for all chunks, marking cached ones.
    pub fn plan(
        &self,
        request: ReadRequest,
        available_chunks: &[(CachedChunkInfo, u64, u64)],
    ) -> ReadPlan {
        let mut cache_hits = 0;
        let mut cache_misses = 0;

        let fetches: Vec<ChunkFetchPlan> = available_chunks
            .iter()
            .map(|(info, node_id, segment_id)| {
                if info.cached {
                    cache_hits += 1;
                } else {
                    cache_misses += 1;
                }
                ChunkFetchPlan {
                    chunk_hash: info.chunk_hash,
                    node_id: *node_id,
                    segment_id: *segment_id,
                    from_cache: info.cached,
                }
            })
            .collect();

        ReadPlan {
            request,
            fetches,
            cache_hits,
            cache_misses,
        }
    }

    /// Estimate total latency for a read plan.
    ///
    /// Returns estimated latency in microseconds.
    pub fn estimate_latency_us(
        &self,
        plan: &ReadPlan,
        cache_latency_us: u64,
        network_latency_us: u64,
    ) -> u64 {
        (plan.cache_hits as u64 * cache_latency_us)
            + (plan.cache_misses as u64 * network_latency_us)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(
        hash_byte: u8,
        cached: bool,
        node: u64,
        segment: u64,
    ) -> (CachedChunkInfo, u64, u64) {
        let mut hash = [0u8; 32];
        hash[0] = hash_byte;
        (
            CachedChunkInfo {
                chunk_hash: hash,
                cached,
            },
            node,
            segment,
        )
    }

    #[test]
    fn read_plan_total_chunks() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, true, 1, 100), make_chunk(2, false, 2, 100)];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert_eq!(plan.total_chunks(), 2);
    }

    #[test]
    fn read_plan_cache_hit_rate_zero() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, false, 1, 100), make_chunk(2, false, 2, 100)];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert_eq!(plan.cache_hit_rate(), 0.0);
    }

    #[test]
    fn read_plan_cache_hit_rate_one() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, true, 1, 100), make_chunk(2, true, 2, 100)];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert_eq!(plan.cache_hit_rate(), 1.0);
    }

    #[test]
    fn read_plan_cache_hit_rate_partial() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, true, 1, 100), make_chunk(2, false, 2, 100)];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert!((plan.cache_hit_rate() - 0.5).abs() < 0.001);
    }

    #[test]
    fn plan_no_chunks() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks: Vec<(CachedChunkInfo, u64, u64)> = vec![];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert!(plan.fetches.is_empty());
        assert_eq!(plan.cache_hits, 0);
        assert_eq!(plan.cache_misses, 0);
    }

    #[test]
    fn plan_all_cached() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![
            make_chunk(1, true, 1, 100),
            make_chunk(2, true, 2, 100),
            make_chunk(3, true, 3, 100),
        ];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert!(plan.fetches.iter().all(|f| f.from_cache));
        assert_eq!(plan.cache_hits, 3);
        assert_eq!(plan.cache_misses, 0);
    }

    #[test]
    fn plan_all_uncached() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, false, 1, 100), make_chunk(2, false, 2, 100)];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert!(plan.fetches.iter().all(|f| !f.from_cache));
        assert_eq!(plan.cache_hits, 0);
        assert_eq!(plan.cache_misses, 2);
    }

    #[test]
    fn plan_mixed_cached() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 8192,
        };
        let chunks = vec![
            make_chunk(1, true, 1, 100),
            make_chunk(2, false, 2, 100),
            make_chunk(3, true, 3, 100),
            make_chunk(4, false, 4, 100),
        ];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert_eq!(plan.cache_hits, 2);
        assert_eq!(plan.cache_misses, 2);
    }

    #[test]
    fn estimate_latency_all_cache() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, true, 1, 100), make_chunk(2, true, 2, 100)];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        let latency = planner.estimate_latency_us(&plan, 10, 1000);
        assert_eq!(latency, 20);
    }

    #[test]
    fn estimate_latency_all_network() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, false, 1, 100), make_chunk(2, false, 2, 100)];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        let latency = planner.estimate_latency_us(&plan, 10, 1000);
        assert_eq!(latency, 2000);
    }

    #[test]
    fn estimate_latency_mixed() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, true, 1, 100), make_chunk(2, false, 2, 100)];
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        let latency = planner.estimate_latency_us(&plan, 10, 1000);
        assert_eq!(latency, 1010);
    }

    #[test]
    fn read_request_fields() {
        let request = ReadRequest {
            inode_id: 42,
            offset: 1024,
            length: 4096,
        };
        assert_eq!(request.inode_id, 42);
        assert_eq!(request.offset, 1024);
        assert_eq!(request.length, 4096);
    }

    #[test]
    fn chunk_fetch_plan_from_cache_field() {
        let plan = ChunkFetchPlan {
            chunk_hash: [1u8; 32],
            node_id: 5,
            segment_id: 100,
            from_cache: true,
        };
        assert!(plan.from_cache);
    }

    #[test]
    fn read_planner_deterministic() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let chunks = vec![make_chunk(1, true, 1, 100), make_chunk(2, false, 2, 100)];
        let planner = ReadPlanner::new();
        let plan1 = planner.plan(request, &chunks);
        let plan2 = planner.plan(request, &chunks);
        assert_eq!(plan1.cache_hits, plan2.cache_hits);
        assert_eq!(plan1.cache_misses, plan2.cache_misses);
        assert_eq!(plan1.fetches.len(), plan2.fetches.len());
    }

    #[test]
    fn read_planner_default() {
        let planner = ReadPlanner::new();
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let plan = planner.plan(request, &[]);
        assert_eq!(plan.total_chunks(), 0);
        assert_eq!(plan.cache_hits, 0);
        assert_eq!(plan.cache_misses, 0);
    }

    #[test]
    fn cache_hit_rate_empty_plan() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &[]);
        assert_eq!(plan.cache_hit_rate(), 0.0);
    }

    #[test]
    fn estimate_latency_empty_plan() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 4096,
        };
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &[]);
        let latency = planner.estimate_latency_us(&plan, 10, 1000);
        assert_eq!(latency, 0);
    }

    #[test]
    fn plan_preserves_request() {
        let request = ReadRequest {
            inode_id: 42,
            offset: 1024,
            length: 8192,
        };
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &[]);
        assert_eq!(plan.request.inode_id, 42);
        assert_eq!(plan.request.offset, 1024);
        assert_eq!(plan.request.length, 8192);
    }

    #[test]
    fn plan_large_number_of_chunks() {
        let request = ReadRequest {
            inode_id: 1,
            offset: 0,
            length: 1_000_000,
        };
        let mut chunks = Vec::new();
        for i in 0..100 {
            chunks.push(make_chunk(i as u8, i % 2 == 0, i, 100));
        }
        let planner = ReadPlanner::new();
        let plan = planner.plan(request, &chunks);
        assert_eq!(plan.total_chunks(), 100);
    }

    #[test]
    fn chunk_fetch_plan_node_and_segment() {
        let plan = ChunkFetchPlan {
            chunk_hash: [5u8; 32],
            node_id: 10,
            segment_id: 200,
            from_cache: false,
        };
        assert_eq!(plan.node_id, 10);
        assert_eq!(plan.segment_id, 200);
        assert!(!plan.from_cache);
    }

    #[test]
    fn cached_chunk_info_not_cached() {
        let info = CachedChunkInfo {
            chunk_hash: [0u8; 32],
            cached: false,
        };
        assert!(!info.cached);
    }
}
