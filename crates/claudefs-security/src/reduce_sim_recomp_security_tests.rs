//! Reduce similarity index and recompressor security tests.

#[cfg(test)]
mod tests {
    use claudefs_reduce::compression::{compress, decompress, CompressionAlgorithm};
    use claudefs_reduce::fingerprint::{super_features, ChunkHash, SuperFeatures};
    use claudefs_reduce::recompressor::{RecompressionStats, Recompressor, RecompressorConfig};
    use claudefs_reduce::similarity::{DeltaCompressor, SimilarityIndex};
    use std::sync::Arc;
    use std::thread;

    fn make_lz4_data(data: &[u8]) -> Vec<u8> {
        compress(data, CompressionAlgorithm::Lz4).unwrap()
    }

    #[test]
    fn test_reduce_sr_sec_empty_index_find_similar_returns_none() {
        let index = SimilarityIndex::new();
        let features = super_features(b"some query data");
        assert!(index.find_similar(&features).is_none());
    }

    #[test]
    fn test_reduce_sr_sec_insert_then_find_similar_same_features() {
        let index = SimilarityIndex::new();
        let data = b"test data for similarity index lookup";
        let hash = ChunkHash(*blake3::hash(data).as_bytes());
        let features = super_features(data);

        index.insert(hash, features);
        let found = index.find_similar(&features);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), hash);
    }

    #[test]
    fn test_reduce_sr_sec_find_similar_different_features_returns_none() {
        let index = SimilarityIndex::new();
        let data1 = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let data2 = b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

        let hash1 = ChunkHash(*blake3::hash(data1).as_bytes());
        let features1 = super_features(data1);
        let features2 = super_features(data2);

        index.insert(hash1, features1);
        let found = index.find_similar(&features2);
        assert!(found.is_none());
    }

    #[test]
    fn test_reduce_sr_sec_remove_then_find_similar_returns_none() {
        let index = SimilarityIndex::new();
        let data = b"data to be removed";
        let hash = ChunkHash(*blake3::hash(data).as_bytes());
        let features = super_features(data);

        index.insert(hash, features);
        assert!(index.find_similar(&features).is_some());

        index.remove(&hash);
        assert!(index.find_similar(&features).is_none());
    }

    #[test]
    fn test_reduce_sr_sec_entry_count_accurate() {
        let index = SimilarityIndex::new();
        assert_eq!(index.entry_count(), 0);

        let data1 = b"first chunk";
        let data2 = b"second chunk";
        let data3 = b"third chunk";

        let hash1 = ChunkHash(*blake3::hash(data1).as_bytes());
        let hash2 = ChunkHash(*blake3::hash(data2).as_bytes());
        let hash3 = ChunkHash(*blake3::hash(data3).as_bytes());

        index.insert(hash1, super_features(data1));
        assert_eq!(index.entry_count(), 1);

        index.insert(hash2, super_features(data2));
        assert_eq!(index.entry_count(), 2);

        index.insert(hash3, super_features(data3));
        assert_eq!(index.entry_count(), 3);

        index.remove(&hash2);
        assert_eq!(index.entry_count(), 2);
    }

    #[test]
    fn test_reduce_sr_sec_concurrent_inserts() {
        let index = Arc::new(SimilarityIndex::new());
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let idx = Arc::clone(&index);
                thread::spawn(move || {
                    let data = format!("concurrent chunk data {}", i);
                    let hash = ChunkHash(*blake3::hash(data.as_bytes()).as_bytes());
                    let features = super_features(data.as_bytes());
                    idx.insert(hash, features);
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(index.entry_count(), 10);
    }

    #[test]
    fn test_reduce_sr_sec_insert_find_from_different_threads() {
        let index = Arc::new(SimilarityIndex::new());
        let data = b"shared data for thread test";
        let hash = ChunkHash(*blake3::hash(data).as_bytes());
        let features = super_features(data);

        let idx1 = Arc::clone(&index);
        let idx2 = Arc::clone(&index);

        let h1 = thread::spawn(move || {
            idx1.insert(hash, features);
        });

        h1.join().unwrap();

        let h2 = thread::spawn(move || idx2.find_similar(&features));

        let found = h2.join().unwrap();
        assert!(found.is_some());
    }

    #[test]
    fn test_reduce_sr_sec_concurrent_insert_remove_no_panic() {
        let index = Arc::new(SimilarityIndex::new());

        let data = b"data for concurrent ops";
        let hash = ChunkHash(*blake3::hash(data).as_bytes());
        let features = super_features(data);

        for _ in 0..5 {
            index.insert(hash, features);
        }

        let idx1 = Arc::clone(&index);
        let idx2 = Arc::clone(&index);

        let h1 = thread::spawn(move || {
            for i in 0..100 {
                let d = format!("insert data {}", i);
                let h = ChunkHash(*blake3::hash(d.as_bytes()).as_bytes());
                let f = super_features(d.as_bytes());
                idx1.insert(h, f);
            }
        });

        let h2 = thread::spawn(move || {
            for _ in 0..50 {
                idx2.remove(&hash);
            }
        });

        h1.join().unwrap();
        h2.join().unwrap();
    }

    #[test]
    fn test_reduce_sr_sec_compress_delta_empty_reference_error() {
        let data = b"some data to compress";
        let result = DeltaCompressor::compress_delta(data, b"", 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_reduce_sr_sec_decompress_delta_empty_reference_error() {
        let delta = vec![1, 2, 3, 4];
        let result = DeltaCompressor::decompress_delta(&delta, b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_reduce_sr_sec_delta_roundtrip() {
        let original = b"This is the original data for delta compression roundtrip test";
        let reference =
            b"This is reference data that is similar to original data for delta compression test";

        let compressed = DeltaCompressor::compress_delta(original, reference, 3).unwrap();
        let decompressed = DeltaCompressor::decompress_delta(&compressed, reference).unwrap();

        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_reduce_sr_sec_delta_similar_data_smaller() {
        let base: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let mut similar = base.clone();
        for (i, byte) in similar.iter_mut().enumerate() {
            *byte = (*byte).wrapping_add((i % 10) as u8);
        }

        let reference =
            b"reference block for delta compression that has similar structure to the data";

        let original_size = similar.len();
        let compressed = DeltaCompressor::compress_delta(&similar, reference, 3).unwrap();
        let decompressed = DeltaCompressor::decompress_delta(&compressed, reference).unwrap();

        assert_eq!(decompressed, similar);
        assert!(compressed.len() < original_size);
    }

    #[test]
    fn test_reduce_sr_sec_compress_delta_empty_data_succeeds() {
        let reference = b"reference data for empty input";
        let result = DeltaCompressor::compress_delta(b"", reference, 3);
        assert!(result.is_ok());

        let compressed = result.unwrap();
        let decompressed = DeltaCompressor::decompress_delta(&compressed, reference).unwrap();
        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_reduce_sr_sec_default_config_values() {
        let config = RecompressorConfig::default();
        assert_eq!(config.zstd_level, 3);
        assert_eq!(config.min_improvement_pct, 5);
    }

    #[test]
    fn test_reduce_sr_sec_highly_compressible_returns_some() {
        let recompressor = Recompressor::new(RecompressorConfig::default());
        let hash = ChunkHash([0u8; 32]);

        let data: Vec<u8> = vec![0u8; 100_000];
        let lz4_data = make_lz4_data(&data);

        let result = recompressor.recompress_chunk(hash, &lz4_data).unwrap();
        assert!(result.is_some());

        let recompressed = result.unwrap();
        assert!(recompressed.new_zstd_size < recompressed.original_lz4_size);
    }

    #[test]
    fn test_reduce_sr_sec_high_threshold_rejects_marginal() {
        let recompressor = Recompressor::new(RecompressorConfig {
            zstd_level: 3,
            min_improvement_pct: 50,
        });

        let hash = ChunkHash([0u8; 32]);
        let data: Vec<u8> = vec![1u8; 1000];
        let lz4_data = make_lz4_data(&data);

        let result = recompressor.recompress_chunk(hash, &lz4_data).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_reduce_sr_sec_zero_threshold_accepts_any_improvement() {
        let recompressor = Recompressor::new(RecompressorConfig {
            zstd_level: 3,
            min_improvement_pct: 0,
        });

        let hash = ChunkHash([0u8; 32]);
        let data: Vec<u8> = vec![0xAB; 10_000];
        let lz4_data = make_lz4_data(&data);

        let result = recompressor.recompress_chunk(hash, &lz4_data).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_reduce_sr_sec_empty_lz4_roundtrips() {
        let recompressor = Recompressor::new(RecompressorConfig::default());
        let hash = ChunkHash([0u8; 32]);

        let empty_lz4 = make_lz4_data(&[]);
        let result = recompressor.recompress_chunk(hash, &empty_lz4).unwrap();

        if let Some(recompressed) = result {
            let decompressed =
                decompress(&recompressed.data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
            assert!(decompressed.is_empty());
        }
    }

    #[test]
    fn test_reduce_sr_sec_empty_batch_returns_empty() {
        let recompressor = Recompressor::new(RecompressorConfig::default());
        let chunks: Vec<(ChunkHash, Vec<u8>)> = vec![];

        let (improved, stats) = recompressor.recompress_batch(&chunks);

        assert!(improved.is_empty());
        assert_eq!(stats.chunks_processed, 0);
        assert_eq!(stats.bytes_before, 0);
        assert_eq!(stats.bytes_after, 0);
    }

    #[test]
    fn test_reduce_sr_sec_batch_five_chunks_processed() {
        let recompressor = Recompressor::new(RecompressorConfig::default());

        let chunks: Vec<(ChunkHash, Vec<u8>)> = (0..5)
            .map(|i| {
                let data: Vec<u8> = vec![(i % 256) as u8; 10_000];
                let lz4 = make_lz4_data(&data);
                (ChunkHash([i as u8; 32]), lz4)
            })
            .collect();

        let (_, stats) = recompressor.recompress_batch(&chunks);
        assert_eq!(stats.chunks_processed, 5);
    }

    #[test]
    fn test_reduce_sr_sec_batch_bytes_before_positive() {
        let recompressor = Recompressor::new(RecompressorConfig::default());

        let data = b"test data for batch recompression";
        let lz4_data = make_lz4_data(data);
        let chunks = vec![(ChunkHash([1u8; 32]), lz4_data)];

        let (_, stats) = recompressor.recompress_batch(&chunks);
        assert!(stats.bytes_before > 0);
    }

    #[test]
    fn test_reduce_sr_sec_stats_improved_plus_skipped_equals_processed() {
        let recompressor = Recompressor::new(RecompressorConfig::default());

        let chunks: Vec<(ChunkHash, Vec<u8>)> = (0..10)
            .map(|i| {
                let data: Vec<u8> = if i % 2 == 0 {
                    vec![0u8; 5_000]
                } else {
                    (0..5_000).map(|j| ((j * 251) % 256) as u8).collect()
                };
                let lz4 = make_lz4_data(&data);
                (ChunkHash([i as u8; 32]), lz4)
            })
            .collect();

        let (_, stats) = recompressor.recompress_batch(&chunks);
        assert_eq!(
            stats.chunks_improved + stats.chunks_skipped,
            stats.chunks_processed
        );
    }

    #[test]
    fn test_reduce_sr_sec_batch_mixed_compressible_incompressible() {
        let recompressor = Recompressor::new(RecompressorConfig::default());

        let compressible: Vec<u8> = vec![0u8; 10_000];
        let incompressible: Vec<u8> = (0..10_000).map(|i| ((i * 251) % 256) as u8).collect();

        let chunks = vec![
            (ChunkHash([1u8; 32]), make_lz4_data(&compressible)),
            (ChunkHash([2u8; 32]), make_lz4_data(&incompressible)),
            (ChunkHash([3u8; 32]), make_lz4_data(&compressible)),
        ];

        let (improved, stats) = recompressor.recompress_batch(&chunks);

        assert!(stats.chunks_processed == 3);
        assert!(improved.len() <= 3);
        assert!(stats.chunks_improved + stats.chunks_skipped == 3);
    }

    #[test]
    fn test_reduce_sr_sec_default_stats_ratio_1() {
        let stats = RecompressionStats::default();
        assert_eq!(stats.compression_ratio(), 1.0);
    }

    #[test]
    fn test_reduce_sr_sec_bytes_saved_positive() {
        let stats = RecompressionStats {
            chunks_processed: 1,
            chunks_improved: 1,
            chunks_skipped: 0,
            bytes_before: 1000,
            bytes_after: 500,
        };

        assert!(stats.bytes_saved() > 0);
        assert_eq!(stats.bytes_saved(), 500);
    }

    #[test]
    fn test_reduce_sr_sec_bytes_saved_negative() {
        let stats = RecompressionStats {
            chunks_processed: 1,
            chunks_improved: 0,
            chunks_skipped: 1,
            bytes_before: 500,
            bytes_after: 1000,
        };

        assert!(stats.bytes_saved() < 0);
        assert_eq!(stats.bytes_saved(), -500);
    }

    #[test]
    fn test_reduce_sr_sec_compression_ratio_nonzero() {
        let stats = RecompressionStats {
            chunks_processed: 10,
            chunks_improved: 5,
            chunks_skipped: 5,
            bytes_before: 10000,
            bytes_after: 5000,
        };

        let ratio = stats.compression_ratio();
        assert!((ratio - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_reduce_sr_sec_bytes_saved_zero_when_equal() {
        let stats = RecompressionStats {
            chunks_processed: 1,
            chunks_improved: 0,
            chunks_skipped: 1,
            bytes_before: 1000,
            bytes_after: 1000,
        };

        assert_eq!(stats.bytes_saved(), 0);
    }
}
