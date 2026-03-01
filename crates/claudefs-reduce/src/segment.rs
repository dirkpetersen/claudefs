//! Segment packing for erasure coding.
//! Packs reduced chunks into 2MB segments for EC (4+2 coding).

use crate::fingerprint::ChunkHash;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Default segment size: 2MB for erasure coding (4+2 configuration).
pub const DEFAULT_SEGMENT_SIZE: usize = 2 * 1024 * 1024;

/// Metadata for a single chunk within a segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentEntry {
    /// BLAKE3 hash of the original chunk (for CAS lookup).
    pub hash: ChunkHash,
    /// Byte offset within the segment's payload.
    pub offset_in_segment: u32,
    /// Size of the compressed/encrypted payload in this segment.
    pub payload_size: u32,
    /// Original uncompressed size (for stats).
    pub original_size: u32,
}

/// A 2MB segment containing packed chunk payloads for erasure coding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// Unique segment sequence number.
    pub id: u64,
    /// Chunk metadata entries.
    pub entries: Vec<SegmentEntry>,
    /// Concatenated chunk payloads.
    pub payload: Vec<u8>,
    /// True when full or explicitly sealed.
    pub sealed: bool,
    /// Seconds since UNIX_EPOCH when segment was created.
    pub created_at_secs: u64,
}

impl Segment {
    /// Number of chunks in this segment.
    pub fn total_chunks(&self) -> usize {
        self.entries.len()
    }

    /// Total bytes in the payload.
    pub fn total_payload_bytes(&self) -> usize {
        self.payload.len()
    }
}

/// Configuration for the segment packer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentPackerConfig {
    /// Target segment size in bytes.
    pub target_size: usize,
}

impl Default for SegmentPackerConfig {
    fn default() -> Self {
        Self {
            target_size: DEFAULT_SEGMENT_SIZE,
        }
    }
}

/// Packs reduced chunks into fixed-size segments for erasure coding.
pub struct SegmentPacker {
    config: SegmentPackerConfig,
    next_id: u64,
    current: Option<Segment>,
}

impl Default for SegmentPacker {
    fn default() -> Self {
        Self::new(SegmentPackerConfig::default())
    }
}

impl SegmentPacker {
    /// Create a new segment packer with the given configuration.
    pub fn new(config: SegmentPackerConfig) -> Self {
        Self {
            config,
            next_id: 0,
            current: None,
        }
    }

    /// Add a chunk to the current segment.
    /// Returns a sealed segment if it becomes full (>=
    pub fn add_chunk(
        &mut self,
        hash: ChunkHash,
        payload: &[u8],
        original_size: u32,
    ) -> Option<Segment> {
        // Create current segment if needed
        if self.current.is_none() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            self.current = Some(Segment {
                id: self.next_id,
                entries: Vec::new(),
                payload: Vec::new(),
                sealed: false,
                created_at_secs: now,
            });
            self.next_id += 1;
        }

        let segment = self.current.as_mut().unwrap();
        let offset = segment.payload.len() as u32;
        let payload_len = payload.len() as u32;

        // Add entry
        segment.entries.push(SegmentEntry {
            hash,
            offset_in_segment: offset,
            payload_size: payload_len,
            original_size,
        });

        // Append payload
        segment.payload.extend_from_slice(payload);

        debug!(
            segment_id = segment.id,
            chunk_offset = offset,
            payload_size = payload_len,
            current_size = segment.payload.len(),
            target_size = self.config.target_size,
            "Added chunk to segment"
        );

        // Check if segment is full
        if segment.payload.len() >= self.config.target_size {
            segment.sealed = true;
            let full_segment = self.current.take();
            debug!(
                segment_id = full_segment.as_ref().unwrap().id,
                "Segment sealed (full)"
            );
            return full_segment;
        }

        None
    }

    /// Seal and return the current segment, even if not full.
    /// After flushing, current is None.
    pub fn flush(&mut self) -> Option<Segment> {
        if let Some(ref mut segment) = self.current {
            segment.sealed = true;
            debug!(segment_id = segment.id, "Segment flushed");
        }
        self.current.take()
    }

    /// Current size in bytes (0 if no current segment).
    pub fn current_size(&self) -> usize {
        self.current.as_ref().map(|s| s.payload.len()).unwrap_or(0)
    }

    /// True if no current segment or it has no chunks.
    pub fn is_empty(&self) -> bool {
        match &self.current {
            Some(segment) => segment.entries.is_empty(),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::blake3_hash;

    fn make_chunk(size: usize) -> (ChunkHash, Vec<u8>) {
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        let hash = blake3_hash(&data);
        (hash, data)
    }

    #[test]
    fn test_add_chunks_returns_segment_when_full() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig {
            target_size: 1024, // Small for testing
        });

        // Add chunks until we exceed target size
        let mut sealed_count = 0;
        for i in 0..100 {
            let (_, payload) = make_chunk(100);
            if let Some(segment) =
                packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
            {
                sealed_count += 1;
                assert!(segment.sealed);
                assert!(segment.payload.len() >= 1024);
            }
        }
        assert!(sealed_count > 0);
    }

    #[test]
    fn test_flush_returns_partial_segment() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        // Add just one small chunk
        let (_, payload) = make_chunk(100);
        packer.add_chunk(blake3_hash(b"test"), &payload, payload.len() as u32);

        // Flush before full
        let segment = packer.flush().expect("should return segment");
        assert!(segment.sealed);
        assert!(segment.entries.len() == 1);
    }

    #[test]
    fn test_flush_on_empty_returns_none() {
        let mut packer: SegmentPacker = SegmentPacker::default();
        let result = packer.flush();
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_entries_correct() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        let (hash1, payload1) = make_chunk(100);
        let (hash2, payload2) = make_chunk(200);

        packer.add_chunk(hash1, &payload1, payload1.len() as u32);
        packer.add_chunk(hash2, &payload2, payload2.len() as u32);

        let segment = packer.flush().unwrap();

        assert_eq!(segment.entries.len(), 2);

        let entry1 = &segment.entries[0];
        assert_eq!(entry1.hash, hash1);
        assert_eq!(entry1.offset_in_segment, 0);
        assert_eq!(entry1.payload_size, 100);
        assert_eq!(entry1.original_size, 100);

        let entry2 = &segment.entries[1];
        assert_eq!(entry2.hash, hash2);
        assert_eq!(entry2.offset_in_segment, 100);
        assert_eq!(entry2.payload_size, 200);
        assert_eq!(entry2.original_size, 200);
    }

    #[test]
    fn test_multiple_segments() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 500 });

        let mut sealed_segments = Vec::new();

        // Add chunks totaling more than 2x target size
        for i in 0..10 {
            let (_, payload) = make_chunk(150);
            if let Some(segment) =
                packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
            {
                sealed_segments.push(segment);
            }
        }

        // Flush remaining
        if let Some(segment) = packer.flush() {
            sealed_segments.push(segment);
        }

        // Should have multiple segments
        assert!(
            sealed_segments.len() >= 2,
            "expected >= 2 segments, got {}",
            sealed_segments.len()
        );

        // Verify segment IDs are sequential
        for (i, segment) in sealed_segments.iter().enumerate() {
            assert_eq!(segment.id, i as u64);
        }
    }

    #[test]
    fn test_segment_id_increments() {
        let mut packer: SegmentPacker = SegmentPacker::default();

        let (_, payload) = make_chunk(100);

        // First segment
        packer.add_chunk(blake3_hash(b"chunk1"), &payload, payload.len() as u32);
        let seg1 = packer.flush().unwrap();

        // Second segment
        packer.add_chunk(blake3_hash(b"chunk2"), &payload, payload.len() as u32);
        let seg2 = packer.flush().unwrap();

        // Third segment
        packer.add_chunk(blake3_hash(b"chunk3"), &payload, payload.len() as u32);
        let seg3 = packer.flush().unwrap();

        assert_eq!(seg1.id, 0);
        assert_eq!(seg2.id, 1);
        assert_eq!(seg3.id, 2);
    }
}
