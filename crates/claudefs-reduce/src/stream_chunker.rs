//! Async streaming CDC (Content-Defined Chunking) for large files.

use crate::error::ReduceError;
use crate::fingerprint::ChunkHash;
use fastcdc::v2020::FastCDC;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};

/// Configuration for the streaming chunker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunkerConfig {
    /// Minimum chunk size in bytes.
    pub min_chunk_size: usize,
    /// Average (target) chunk size in bytes.
    pub avg_chunk_size: usize,
    /// Maximum chunk size in bytes.
    pub max_chunk_size: usize,
    /// How much to read from the stream at a time.
    pub read_buffer_size: usize,
}

impl Default for StreamChunkerConfig {
    fn default() -> Self {
        Self {
            min_chunk_size: 65536,
            avg_chunk_size: 262144,
            max_chunk_size: 1048576,
            read_buffer_size: 1048576,
        }
    }
}

/// A chunk produced by the streaming chunker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamChunkResult {
    /// BLAKE3 hash of this chunk's data.
    pub hash: ChunkHash,
    /// Raw chunk bytes.
    pub data: Vec<u8>,
    /// Byte offset in the source stream where chunk starts.
    pub offset: u64,
    /// Chunk length in bytes.
    pub length: usize,
}

/// Statistics from a streaming chunk operation.
#[derive(Debug, Clone, Default)]
pub struct StreamingStats {
    /// Number of chunks produced.
    pub chunks_produced: u64,
    /// Total bytes consumed from the stream.
    pub bytes_consumed: u64,
    /// Smallest chunk size observed.
    pub min_chunk_size_seen: usize,
    /// Largest chunk size observed.
    pub max_chunk_size_seen: usize,
}

/// Streaming content-defined chunker.
pub struct StreamChunker {
    config: StreamChunkerConfig,
}

impl StreamChunker {
    /// Create a new streaming chunker with the given configuration.
    pub fn new(config: StreamChunkerConfig) -> Self {
        Self { config }
    }

    /// Chunk all data from the given AsyncRead source.
    ///
    /// Returns a Vec of all chunks with their hashes and offsets.
    /// Uses fastcdc internally.
    pub async fn chunk_stream<R: AsyncRead + Unpin>(
        &self,
        mut reader: R,
    ) -> Result<(Vec<StreamChunkResult>, StreamingStats), ReduceError> {
        let mut buffer = Vec::with_capacity(self.config.read_buffer_size * 2);
        let mut read_buf = vec![0u8; self.config.read_buffer_size];
        let mut results = Vec::new();
        let mut stats = StreamingStats {
            min_chunk_size_seen: usize::MAX,
            max_chunk_size_seen: 0,
            ..Default::default()
        };
        let mut stream_offset: u64 = 0;

        loop {
            let n = reader.read(&mut read_buf).await?;
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&read_buf[..n]);
            stats.bytes_consumed += n as u64;

            // Process the buffer with FastCDC
            if !buffer.is_empty() {
                let chunks: Vec<_> = FastCDC::new(
                    &buffer,
                    self.config.min_chunk_size as u32,
                    self.config.avg_chunk_size as u32,
                    self.config.max_chunk_size as u32,
                )
                .collect();

                let mut consumed = 0usize;
                for chunk in chunks {
                    let start = chunk.offset;
                    let end = start + chunk.length;
                    let chunk_data = buffer[start..end].to_vec();

                    let hash = ChunkHash(*blake3::hash(&chunk_data).as_bytes());

                    results.push(StreamChunkResult {
                        hash,
                        data: chunk_data.clone(),
                        offset: stream_offset + start as u64,
                        length: chunk.length,
                    });

                    stats.chunks_produced += 1;
                    stats.min_chunk_size_seen = stats.min_chunk_size_seen.min(chunk.length);
                    stats.max_chunk_size_seen = stats.max_chunk_size_seen.max(chunk.length);

                    consumed = end;
                }

                // Remove consumed bytes from buffer
                if consumed > 0 {
                    buffer.drain(..consumed);
                    stream_offset += consumed as u64;
                }
            }
        }

        // Handle remaining data in buffer (less than min_chunk_size)
        if !buffer.is_empty() {
            let hash = ChunkHash(*blake3::hash(&buffer).as_bytes());
            let len = buffer.len();
            results.push(StreamChunkResult {
                hash,
                data: buffer,
                offset: stream_offset,
                length: len,
            });
            stats.chunks_produced += 1;
            stats.min_chunk_size_seen = stats.min_chunk_size_seen.min(len);
            stats.max_chunk_size_seen = stats.max_chunk_size_seen.max(len);
        }

        // Fix min_chunk_size_seen if no chunks were produced
        if stats.min_chunk_size_seen == usize::MAX {
            stats.min_chunk_size_seen = 0;
        }

        Ok((results, stats))
    }

    /// Chunk data from a byte slice (convenience wrapper).
    ///
    /// This is synchronous and directly uses fastcdc on the slice.
    pub fn chunk_slice(&self, data: &[u8]) -> (Vec<StreamChunkResult>, StreamingStats) {
        let mut results = Vec::new();
        let mut stats = StreamingStats {
            bytes_consumed: data.len() as u64,
            min_chunk_size_seen: usize::MAX,
            max_chunk_size_seen: 0,
            ..Default::default()
        };

        if data.is_empty() {
            stats.min_chunk_size_seen = 0;
            return (results, stats);
        }

        let chunks: Vec<_> = FastCDC::new(
            data,
            self.config.min_chunk_size as u32,
            self.config.avg_chunk_size as u32,
            self.config.max_chunk_size as u32,
        )
        .collect();

        for chunk in chunks {
            let start = chunk.offset;
            let end = start + chunk.length;
            let chunk_data = data[start..end].to_vec();

            let hash = ChunkHash(*blake3::hash(&chunk_data).as_bytes());

            results.push(StreamChunkResult {
                hash,
                data: chunk_data,
                offset: start as u64,
                length: chunk.length,
            });

            stats.chunks_produced += 1;
            stats.min_chunk_size_seen = stats.min_chunk_size_seen.min(chunk.length);
            stats.max_chunk_size_seen = stats.max_chunk_size_seen.max(chunk.length);
        }

        if stats.min_chunk_size_seen == usize::MAX {
            stats.min_chunk_size_seen = 0;
        }

        (results, stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_chunk_empty_slice() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let (results, stats) = chunker.chunk_slice(&[]);
        assert_eq!(results.len(), 0);
        assert_eq!(stats.bytes_consumed, 0);
        assert_eq!(stats.chunks_produced, 0);
    }

    #[test]
    fn test_chunk_small_slice() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data = b"small data".to_vec();
        let (results, stats) = chunker.chunk_slice(&data);
        assert_eq!(results.len(), 1);
        assert!(stats.bytes_consumed > 0);
    }

    #[test]
    fn test_chunk_large_slice() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(4 * 1024 * 1024).collect();
        let (results, stats) = chunker.chunk_slice(&data);
        assert!(results.len() > 1, "Should produce multiple chunks for 4MB");
        assert_eq!(stats.bytes_consumed, data.len() as u64);
    }

    #[test]
    fn test_chunk_offsets_monotonic() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(2 * 1024 * 1024).collect();
        let (results, _) = chunker.chunk_slice(&data);
        let offsets: Vec<u64> = results.iter().map(|r| r.offset).collect();
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1], "Offsets must be strictly increasing");
        }
    }

    #[test]
    fn test_chunk_offsets_contiguous() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(2 * 1024 * 1024).collect();
        let (results, _) = chunker.chunk_slice(&data);
        for i in 1..results.len() {
            let prev_end = results[i - 1].offset + results[i - 1].length as u64;
            assert_eq!(
                results[i].offset, prev_end,
                "Chunks must be contiguous"
            );
        }
    }

    #[test]
    fn test_chunk_total_bytes() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(3 * 1024 * 1024).collect();
        let (results, _) = chunker.chunk_slice(&data);
        let total: usize = results.iter().map(|r| r.length).sum();
        assert_eq!(total, data.len());
    }

    #[test]
    fn test_chunk_hashes_correct() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(1024 * 1024).collect();
        let (results, _) = chunker.chunk_slice(&data);
        for result in &results {
            let expected = ChunkHash(*blake3::hash(&result.data).as_bytes());
            assert_eq!(result.hash, expected);
        }
    }

    #[test]
    fn test_chunk_hashes_unique_for_unique_data() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data1: Vec<u8> = (0u8..=255u8).cycle().take(1024 * 1024).collect();
        let data2: Vec<u8> = (1u8..=255u8).cycle().take(1024 * 1024).collect();
        let (results1, _) = chunker.chunk_slice(&data1);
        let (results2, _) = chunker.chunk_slice(&data2);
        assert_ne!(results1[0].hash, results2[0].hash);
    }

    #[test]
    fn test_stats_bytes_consumed() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(512 * 1024).collect();
        let (_, stats) = chunker.chunk_slice(&data);
        assert_eq!(stats.bytes_consumed, data.len() as u64);
    }

    #[test]
    fn test_stats_chunks_produced() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(512 * 1024).collect();
        let (results, stats) = chunker.chunk_slice(&data);
        assert_eq!(stats.chunks_produced, results.len() as u64);
    }

    #[tokio::test]
    async fn test_stream_matches_slice() {
        let config = StreamChunkerConfig::default();
        let chunker = StreamChunker::new(config.clone());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(2 * 1024 * 1024).collect();

        let (slice_results, _) = chunker.chunk_slice(&data);

        let cursor = Cursor::new(data.clone());
        let (stream_results, _) = chunker.chunk_stream(cursor).await.unwrap();

        assert_eq!(slice_results.len(), stream_results.len());
        for (s, r) in slice_results.iter().zip(stream_results.iter()) {
            assert_eq!(s.hash, r.hash);
            assert_eq!(s.data, r.data);
            assert_eq!(s.offset, r.offset);
            assert_eq!(s.length, r.length);
        }
    }

    #[test]
    fn test_deterministic() {
        let config = StreamChunkerConfig::default();
        let chunker = StreamChunker::new(config.clone());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(1024 * 1024).collect();

        let (results1, _) = chunker.chunk_slice(&data);
        let (results2, _) = chunker.chunk_slice(&data);

        assert_eq!(results1.len(), results2.len());
        for (a, b) in results1.iter().zip(results2.iter()) {
            assert_eq!(a.hash, b.hash);
            assert_eq!(a.data, b.data);
            assert_eq!(a.offset, b.offset);
            assert_eq!(a.length, b.length);
        }
    }

    #[test]
    fn test_custom_config() {
        let config = StreamChunkerConfig {
            min_chunk_size: 4096,
            avg_chunk_size: 8192,
            max_chunk_size: 16384,
            read_buffer_size: 16384,
        };
        let chunker = StreamChunker::new(config);
        let data: Vec<u8> = (0u8..=255u8).cycle().take(64 * 1024).collect();
        let (results, _) = chunker.chunk_slice(&data);

        for chunk in &results {
            assert!(
                chunk.length >= 4096 || chunk == results.last().unwrap(),
                "chunk too small: {}",
                chunk.length
            );
            assert!(
                chunk.length <= 16384,
                "chunk too large: {}",
                chunk.length
            );
        }
    }

    #[tokio::test]
    async fn test_large_file_streaming() {
        let chunker = StreamChunker::new(StreamChunkerConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(8 * 1024 * 1024).collect();

        let cursor = Cursor::new(data.clone());
        let (results, stats) = chunker.chunk_stream(cursor).await.unwrap();

        let total_bytes: usize = results.iter().map(|r| r.length).sum();
        assert_eq!(total_bytes, data.len());
        assert_eq!(stats.bytes_consumed, data.len() as u64);
    }
}