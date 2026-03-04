//! Segment compaction: rewrite sparse segments into dense ones.
//!
//! Identifies segments with many dead chunks and rewrites them,
//! reclaiming space without full cluster-wide GC.

use crate::error::ReduceError;
use crate::fingerprint::ChunkHash;
use crate::segment::{Segment, SegmentPacker, SegmentPackerConfig};
use crate::segment_reader::SegmentReader;
use serde::{Deserialize, Serialize};

/// Configuration for the compaction engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Minimum live-chunk fraction to skip compaction (0.0-1.0).
    /// If a segment's live ratio is >= this value, skip it. Default: 0.7
    pub live_ratio_threshold: f64,
    /// Target segment byte size for packed outputs. Default: 2MB
    pub target_segment_bytes: usize,
    /// Starting segment ID for new compacted segments.
    pub next_segment_id: u64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            live_ratio_threshold: 0.7,
            target_segment_bytes: 2 * 1024 * 1024,
            next_segment_id: 0,
        }
    }
}

/// Result from one compaction pass.
#[derive(Debug, Clone, Default)]
pub struct CompactionResult {
    /// Number of input segments that were candidates.
    pub segments_examined: usize,
    /// Number of segments that were too sparse and selected for compaction.
    pub segments_compacted: usize,
    /// Number of new output segments produced.
    pub segments_produced: usize,
    /// Number of live chunks re-packed.
    pub chunks_repacked: usize,
    /// Bytes saved (dead bytes removed from old segments).
    pub bytes_reclaimed: u64,
}

/// Engine that compacts sparse segments.
pub struct CompactionEngine {
    config: CompactionConfig,
}

impl CompactionEngine {
    /// Create a new compaction engine with the given configuration.
    pub fn new(config: CompactionConfig) -> Self {
        Self { config }
    }

    /// Compute the live-chunk ratio for a segment.
    ///
    /// `live_hashes` is the set of chunk hashes that are still referenced.
    /// Returns a value in [0.0, 1.0]. Returns 1.0 if the segment is empty.
    pub fn live_ratio(&self, segment: &Segment, live_hashes: &[ChunkHash]) -> f64 {
        if segment.entries.is_empty() {
            return 1.0;
        }

        let live_count = segment
            .entries
            .iter()
            .filter(|entry| live_hashes.contains(&entry.hash))
            .count();

        live_count as f64 / segment.entries.len() as f64
    }

    /// Select which segments to compact.
    ///
    /// Segments whose live ratio < config.live_ratio_threshold are candidates.
    /// Returns indices into the input slice.
    pub fn select_candidates<'a>(
        &self,
        segments: &'a [Segment],
        live_hashes: &[ChunkHash],
    ) -> Vec<usize> {
        segments
            .iter()
            .enumerate()
            .filter(|(_, seg)| self.live_ratio(seg, live_hashes) < self.config.live_ratio_threshold)
            .map(|(i, _)| i)
            .collect()
    }

    /// Compact the given candidate segments into new packed segments.
    ///
    /// Only chunks present in `live_hashes` are copied to the output.
    /// Returns the new (compacted) segments and a CompactionResult.
    pub fn compact(
        &mut self,
        candidates: &[&Segment],
        live_hashes: &[ChunkHash],
    ) -> Result<(Vec<Segment>, CompactionResult), ReduceError> {
        let mut result = CompactionResult {
            segments_examined: candidates.len(),
            ..Default::default()
        };

        if candidates.is_empty() {
            return Ok((Vec::new(), result));
        }

        let live_set: std::collections::HashSet<ChunkHash> = live_hashes.iter().copied().collect();

        let mut live_entries: Vec<(ChunkHash, Vec<u8>, u32)> = Vec::new();
        let mut total_dead_bytes: u64 = 0;

        for segment in candidates {
            let reader = SegmentReader::new(segment);

            let mut segment_live = 0usize;
            for entry in &segment.entries {
                if live_set.contains(&entry.hash) {
                    let payload = reader.get_chunk(&entry.hash)?;
                    live_entries.push((entry.hash, payload.to_vec(), entry.original_size));
                    segment_live += 1;
                } else {
                    total_dead_bytes += entry.payload_size as u64;
                }
            }

            if segment_live > 0 || segment.entries.len() > 0 {
                result.segments_compacted += 1;
            }
        }

        result.bytes_reclaimed = total_dead_bytes;

        if live_entries.is_empty() {
            return Ok((Vec::new(), result));
        }

        let mut packer = SegmentPacker::new(SegmentPackerConfig {
            target_size: self.config.target_segment_bytes,
        });

        let mut output_segments = Vec::new();

        for (hash, payload, original_size) in live_entries {
            if let Some(sealed) = packer.add_chunk(hash, &payload, original_size) {
                output_segments.push(sealed);
                result.segments_produced += 1;
            }
            result.chunks_repacked += 1;
        }

        if let Some(sealed) = packer.flush() {
            output_segments.push(sealed);
            result.segments_produced += 1;
        }

        for seg in &mut output_segments {
            seg.id = self.config.next_segment_id;
            self.config.next_segment_id += 1;
        }

        Ok((output_segments, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::blake3_hash;
    use crate::segment::{Segment, SegmentEntry};

    fn make_segment(id: u64, chunks: &[(&[u8], bool)]) -> Segment {
        let mut entries = Vec::new();
        let mut payload = Vec::new();
        let mut offset = 0u32;

        for (data, _is_live) in chunks {
            let hash = blake3_hash(data);
            entries.push(SegmentEntry {
                hash,
                offset_in_segment: offset,
                payload_size: data.len() as u32,
                original_size: data.len() as u32,
            });
            payload.extend_from_slice(data);
            offset += data.len() as u32;
        }

        Segment {
            id,
            entries,
            payload,
            sealed: true,
            created_at_secs: 0,
            payload_checksum: None,
        }
    }

    fn hashes_from_data(data_slices: &[&[u8]]) -> Vec<ChunkHash> {
        data_slices.iter().map(|d| blake3_hash(d)).collect()
    }

    #[test]
    fn test_default_config() {
        let config = CompactionConfig::default();
        assert!((config.live_ratio_threshold - 0.7).abs() < 1e-10);
        assert_eq!(config.target_segment_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn test_live_ratio_all_live() {
        let engine = CompactionEngine::new(CompactionConfig::default());
        let segment = make_segment(1, &[(b"hello", true), (b"world", true)]);
        let live_hashes = hashes_from_data(&[b"hello", b"world"]);

        let ratio = engine.live_ratio(&segment, &live_hashes);
        assert!((ratio - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_live_ratio_all_dead() {
        let engine = CompactionEngine::new(CompactionConfig::default());
        let segment = make_segment(1, &[(b"hello", false), (b"world", false)]);
        let live_hashes: Vec<ChunkHash> = Vec::new();

        let ratio = engine.live_ratio(&segment, &live_hashes);
        assert!((ratio - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_live_ratio_half() {
        let engine = CompactionEngine::new(CompactionConfig::default());
        let segment = make_segment(1, &[(b"hello", true), (b"world", false)]);
        let live_hashes = hashes_from_data(&[b"hello"]);

        let ratio = engine.live_ratio(&segment, &live_hashes);
        assert!((ratio - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_live_ratio_empty_segment() {
        let engine = CompactionEngine::new(CompactionConfig::default());
        let segment = Segment {
            id: 1,
            entries: Vec::new(),
            payload: Vec::new(),
            sealed: true,
            created_at_secs: 0,
            payload_checksum: None,
        };
        let live_hashes: Vec<ChunkHash> = Vec::new();

        let ratio = engine.live_ratio(&segment, &live_hashes);
        assert!((ratio - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_select_candidates_none() {
        let engine = CompactionEngine::new(CompactionConfig::default());
        let segment = make_segment(1, &[(b"hello", true), (b"world", true)]);
        let live_hashes = hashes_from_data(&[b"hello", b"world"]);

        let candidates = engine.select_candidates(&[segment], &live_hashes);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_select_candidates_some() {
        let engine = CompactionEngine::new(CompactionConfig::default());
        let segment1 = make_segment(1, &[(b"hello", true)]);
        let segment2 = make_segment(2, &[(b"dead", false)]);
        let live_hashes = hashes_from_data(&[b"hello"]);

        let candidates = engine.select_candidates(&[segment1, segment2], &live_hashes);
        assert_eq!(candidates, vec![1]);
    }

    #[test]
    fn test_select_candidates_all() {
        let engine = CompactionEngine::new(CompactionConfig {
            live_ratio_threshold: 0.9,
            ..Default::default()
        });
        let segment1 = make_segment(1, &[(b"a", false), (b"b", false)]);
        let segment2 = make_segment(2, &[(b"c", false), (b"d", true)]);

        let live_hashes = hashes_from_data(&[b"d"]);

        let candidates = engine.select_candidates(&[segment1, segment2], &live_hashes);
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn test_compact_empty_candidates() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let live_hashes: Vec<ChunkHash> = Vec::new();

        let (segments, result) = engine
            .compact(&[], &live_hashes)
            .expect("compact should work");

        assert!(segments.is_empty());
        assert_eq!(result.segments_examined, 0);
        assert_eq!(result.segments_compacted, 0);
        assert_eq!(result.segments_produced, 0);
    }

    #[test]
    fn test_compact_fully_dead_segment() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let segment = make_segment(1, &[(b"dead1", false), (b"dead2", false)]);
        let live_hashes: Vec<ChunkHash> = Vec::new();

        let (segments, result) = engine
            .compact(&[&segment], &live_hashes)
            .expect("compact should work");

        assert!(segments.is_empty());
        assert_eq!(result.chunks_repacked, 0);
        assert_eq!(result.bytes_reclaimed, 10);
    }

    #[test]
    fn test_compact_keeps_live_chunks() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let segment = make_segment(1, &[(b"live1", true), (b"dead", false), (b"live2", true)]);
        let live_hashes = hashes_from_data(&[b"live1", b"live2"]);

        let (segments, result) = engine
            .compact(&[&segment], &live_hashes)
            .expect("compact should work");

        assert_eq!(result.chunks_repacked, 2);
        assert_eq!(segments.len(), 1);

        let reader = SegmentReader::new(&segments[0]);
        assert!(reader.contains(&blake3_hash(b"live1")));
        assert!(reader.contains(&blake3_hash(b"live2")));
    }

    #[test]
    fn test_compact_dead_chunks_dropped() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let segment = make_segment(1, &[(b"live", true), (b"dead", false)]);
        let live_hashes = hashes_from_data(&[b"live"]);

        let (segments, _result) = engine
            .compact(&[&segment], &live_hashes)
            .expect("compact should work");

        assert_eq!(segments.len(), 1);

        let reader = SegmentReader::new(&segments[0]);
        assert!(!reader.contains(&blake3_hash(b"dead")));
    }

    #[test]
    fn test_compact_bytes_reclaimed() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let segment = make_segment(1, &[(b"live", true), (b"dead_data_here", false)]);
        let live_hashes = hashes_from_data(&[b"live"]);

        let (_, result) = engine
            .compact(&[&segment], &live_hashes)
            .expect("compact should work");

        assert_eq!(result.bytes_reclaimed, b"dead_data_here".len() as u64);
    }

    #[test]
    fn test_compact_multiple_segments_into_one() {
        let mut engine = CompactionEngine::new(CompactionConfig {
            target_segment_bytes: 10000,
            ..Default::default()
        });

        let segment1 = make_segment(1, &[(b"live1", true), (b"dead1", false)]);
        let segment2 = make_segment(2, &[(b"live2", true), (b"dead2", false)]);
        let live_hashes = hashes_from_data(&[b"live1", b"live2"]);

        let (segments, result) = engine
            .compact(&[&segment1, &segment2], &live_hashes)
            .expect("compact should work");

        assert_eq!(result.chunks_repacked, 2);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn test_compact_segment_id_increments() {
        let mut engine = CompactionEngine::new(CompactionConfig {
            next_segment_id: 100,
            target_segment_bytes: 100,
            ..Default::default()
        });

        let segment1 = make_segment(1, &[(b"a", true), (b"b", true)]);
        let segment2 = make_segment(2, &[(b"c", true), (b"d", true)]);
        let live_hashes = hashes_from_data(&[b"a", b"b", b"c", b"d"]);

        let (segments, _) = engine
            .compact(&[&segment1, &segment2], &live_hashes)
            .expect("compact should work");

        let ids: Vec<u64> = segments.iter().map(|s| s.id).collect();
        for (i, &id) in ids.iter().enumerate() {
            assert_eq!(id, 100 + i as u64);
        }
    }

    #[test]
    fn test_compact_updates_result_stats() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());

        let segment = make_segment(1, &[(b"live", true), (b"dead", false)]);
        let live_hashes = hashes_from_data(&[b"live"]);

        let (_, result) = engine
            .compact(&[&segment], &live_hashes)
            .expect("compact should work");

        assert_eq!(result.segments_examined, 1);
        assert_eq!(result.segments_compacted, 1);
        assert_eq!(result.segments_produced, 1);
        assert_eq!(result.chunks_repacked, 1);
    }
}
