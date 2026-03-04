//! Segment read path: extract chunks from sealed segments by BLAKE3 hash.

use crate::error::ReduceError;
use crate::fingerprint::ChunkHash;
use crate::segment::Segment;

/// Reads individual chunks out of a sealed Segment by their BLAKE3 hash.
pub struct SegmentReader<'a> {
    segment: &'a Segment,
}

impl<'a> SegmentReader<'a> {
    /// Create a new reader for the given segment.
    pub fn new(segment: &'a Segment) -> Self {
        Self { segment }
    }

    /// Look up a chunk by its BLAKE3 hash.
    /// Returns a slice of the payload bytes for that chunk.
    /// Returns `ReduceError::NotFound` if no entry with that hash exists.
    /// Returns `ReduceError::InvalidInput` if the entry's offset+size is out of bounds.
    pub fn get_chunk(&self, hash: &ChunkHash) -> Result<&[u8], ReduceError> {
        for entry in &self.segment.entries {
            if &entry.hash == hash {
                let offset = entry.offset_in_segment as usize;
                let size = entry.payload_size as usize;
                let end = offset
                    .checked_add(size)
                    .ok_or_else(|| ReduceError::InvalidInput("offset+size overflow".to_string()))?;
                if end > self.segment.payload.len() {
                    return Err(ReduceError::InvalidInput(format!(
                        "offset {} + size {} exceeds payload length {}",
                        offset,
                        size,
                        self.segment.payload.len()
                    )));
                }
                return Ok(&self.segment.payload[offset..end]);
            }
        }
        Err(ReduceError::NotFound(format!(
            "chunk {} not found in segment {}",
            hash, self.segment.id
        )))
    }

    /// Look up a chunk and copy it into an owned Vec.
    pub fn get_chunk_owned(&self, hash: &ChunkHash) -> Result<Vec<u8>, ReduceError> {
        self.get_chunk(hash).map(|s| s.to_vec())
    }

    /// Check if a chunk exists in this segment.
    pub fn contains(&self, hash: &ChunkHash) -> bool {
        self.segment.entries.iter().any(|e| &e.hash == hash)
    }

    /// Iterate over all (hash, payload) pairs in this segment.
    pub fn iter_chunks(&self) -> impl Iterator<Item = (&ChunkHash, &[u8])> {
        self.segment.entries.iter().filter_map(|entry| {
            let offset = entry.offset_in_segment as usize;
            let size = entry.payload_size as usize;
            let end = offset.saturating_add(size);
            if end <= self.segment.payload.len() {
                Some((&entry.hash, &self.segment.payload[offset..end]))
            } else {
                None
            }
        })
    }

    /// Number of chunks in this segment.
    pub fn len(&self) -> usize {
        self.segment.entries.len()
    }

    /// True if this segment has no chunks.
    pub fn is_empty(&self) -> bool {
        self.segment.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::blake3_hash;
    use crate::segment::{Segment, SegmentEntry};

    fn make_test_segment() -> Segment {
        let data1 = b"hello world".to_vec();
        let data2 = b"foo bar baz".to_vec();
        let data3 = b"quick brown fox".to_vec();

        let hash1 = blake3_hash(&data1);
        let hash2 = blake3_hash(&data2);
        let hash3 = blake3_hash(&data3);

        let mut payload = Vec::new();
        payload.extend_from_slice(&data1);
        payload.extend_from_slice(&data2);
        payload.extend_from_slice(&data3);

        Segment {
            id: 1,
            entries: vec![
                SegmentEntry {
                    hash: hash1,
                    offset_in_segment: 0,
                    payload_size: data1.len() as u32,
                    original_size: data1.len() as u32,
                },
                SegmentEntry {
                    hash: hash2,
                    offset_in_segment: data1.len() as u32,
                    payload_size: data2.len() as u32,
                    original_size: data2.len() as u32,
                },
                SegmentEntry {
                    hash: hash3,
                    offset_in_segment: (data1.len() + data2.len()) as u32,
                    payload_size: data3.len() as u32,
                    original_size: data3.len() as u32,
                },
            ],
            payload,
            sealed: true,
            created_at_secs: 0,
            payload_checksum: None,
        }
    }

    #[test]
    fn test_get_chunk_found() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let data = b"hello world";
        let hash = blake3_hash(data);

        let chunk = reader.get_chunk(&hash).expect("should find chunk");
        assert_eq!(chunk, data);
    }

    #[test]
    fn test_get_chunk_not_found() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let unknown_hash = blake3_hash(b"unknown data");
        let result = reader.get_chunk(&unknown_hash);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ReduceError::NotFound(_)));
    }

    #[test]
    fn test_get_chunk_owned() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let data = b"foo bar baz";
        let hash = blake3_hash(data);

        let chunk = reader.get_chunk_owned(&hash).expect("should find chunk");
        assert_eq!(&chunk[..], data);
    }

    #[test]
    fn test_contains_true() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let hash = blake3_hash(b"hello world");
        assert!(reader.contains(&hash));
    }

    #[test]
    fn test_contains_false() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let hash = blake3_hash(b"not in segment");
        assert!(!reader.contains(&hash));
    }

    #[test]
    fn test_iter_chunks() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let chunks: Vec<_> = reader.iter_chunks().collect();
        assert_eq!(chunks.len(), 3);

        assert_eq!(chunks[0].1, b"hello world");
        assert_eq!(chunks[1].1, b"foo bar baz");
        assert_eq!(chunks[2].1, b"quick brown fox");
    }

    #[test]
    fn test_multiple_chunks() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let hashes = [
            blake3_hash(b"hello world"),
            blake3_hash(b"foo bar baz"),
            blake3_hash(b"quick brown fox"),
        ];

        for (i, hash) in hashes.iter().enumerate() {
            let chunk = reader
                .get_chunk(hash)
                .expect(&format!("chunk {} should exist", i));
            assert!(!chunk.is_empty());
        }
    }

    #[test]
    fn test_len() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);
        assert_eq!(reader.len(), 3);
    }

    #[test]
    fn test_is_empty_true() {
        let segment = Segment {
            id: 0,
            entries: Vec::new(),
            payload: Vec::new(),
            sealed: true,
            created_at_secs: 0,
            payload_checksum: None,
        };
        let reader = SegmentReader::new(&segment);
        assert!(reader.is_empty());
    }

    #[test]
    fn test_is_empty_false() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);
        assert!(!reader.is_empty());
    }

    #[test]
    fn test_get_chunk_correct_slice() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let data = b"quick brown fox";
        let hash = blake3_hash(data);

        let chunk = reader.get_chunk(&hash).expect("should find chunk");
        assert_eq!(chunk.len(), data.len());
        assert_eq!(chunk, data);
    }

    #[test]
    fn test_iter_chunks_count() {
        let segment = make_test_segment();
        let reader = SegmentReader::new(&segment);

        let count = reader.iter_chunks().count();
        assert_eq!(count, reader.len());
    }
}
