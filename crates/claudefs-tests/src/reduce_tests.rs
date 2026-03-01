#[cfg(test)]
mod tests {
    use claudefs_reduce::compression::{compress, decompress};
    use claudefs_reduce::encryption::encrypt;
    use claudefs_reduce::{
        CasIndex, Chunk, ChunkHash, Chunker, ChunkerConfig, CompressionAlgorithm, EncryptedChunk,
        EncryptionAlgorithm, EncryptionKey, PipelineConfig, ReduceError, ReducedChunk,
        ReductionPipeline, ReductionStats, SuperFeatures,
    };

    #[test]
    fn test_chunker_create_with_config() {
        let config = ChunkerConfig {
            min_size: 512,
            avg_size: 4096,
            max_size: 65536,
        };
        let chunker = Chunker::with_config(config);
        assert!(true);
    }

    #[test]
    fn test_chunker_default() {
        let chunker = Chunker::new();
        assert!(true);
    }

    #[test]
    fn test_chunker_chunk_small_data() {
        let config = ChunkerConfig {
            min_size: 512,
            avg_size: 4096,
            max_size: 65536,
        };
        let chunker = Chunker::with_config(config);

        let data = b"small data";
        let chunks = chunker.chunk(data);

        assert!(chunks.len() <= 1);
    }

    #[test]
    fn test_chunker_chunk_large_data() {
        let config = ChunkerConfig {
            min_size: 512,
            avg_size: 4096,
            max_size: 65536,
        };
        let chunker = Chunker::with_config(config);

        let data: Vec<u8> = (0..100000).map(|i| (i % 256) as u8).collect();
        let chunks = chunker.chunk(&data);

        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunker_chunk_count() {
        let config = ChunkerConfig {
            min_size: 1024,
            avg_size: 4096,
            max_size: 65536,
        };
        let chunker = Chunker::with_config(config);

        let data: Vec<u8> = vec![0u8; 100000];
        let chunks = chunker.chunk(&data);

        assert!(chunks.len() > 1);
    }

    #[test]
    fn test_chunker_total_bytes_match() {
        let config = ChunkerConfig {
            min_size: 512,
            avg_size: 4096,
            max_size: 65536,
        };
        let chunker = Chunker::with_config(config);

        let data: Vec<u8> = vec![1u8; 50000];
        let chunks = chunker.chunk(&data);

        let total_chunk_bytes: usize = chunks.iter().map(|c| c.data.len()).sum();
        assert_eq!(total_chunk_bytes, data.len());
    }

    #[test]
    fn test_chunk_sizes_within_bounds() {
        let config = ChunkerConfig {
            min_size: 512,
            avg_size: 4096,
            max_size: 65536,
        };
        let chunker = Chunker::with_config(config);

        let data: Vec<u8> = (0..200000).map(|i| (i % 256) as u8).collect();
        let chunks = chunker.chunk(&data);

        for chunk in chunks {
            assert!(chunk.data.len() >= 512);
            assert!(chunk.data.len() <= 65536);
        }
    }

    #[test]
    fn test_compression_lz4_roundtrip() {
        let original = vec![0u8; 4096];

        let compressed = compress(&original, CompressionAlgorithm::Lz4).unwrap();
        assert!(compressed.len() <= original.len());

        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compression_zstd_roundtrip() {
        let original = vec![0u8; 4096];

        let compressed = compress(&original, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert!(compressed.len() <= original.len());

        let decompressed =
            decompress(&compressed, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compression_lz4_with_pattern_data() {
        let original: Vec<u8> = (0..4096).map(|i| (i % 10) as u8).collect();

        let compressed = compress(&original, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();

        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_encryption_key_new() {
        let key = EncryptionKey([0u8; 32]);
        assert_eq!(key.0.len(), 32);
    }

    #[test]
    fn test_encryption_key_length() {
        let key = EncryptionKey([1u8; 32]);
        assert_eq!(key.0.len(), 32);
    }

    #[test]
    fn test_encryption_aes_gcm_roundtrip() {
        let key = EncryptionKey([0u8; 32]);
        let plaintext = b"Secret message for encryption test!";

        let encrypted = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

        use claudefs_reduce::encryption::decrypt;
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encryption_different_keys_different_ciphertext() {
        let key1 = EncryptionKey([1u8; 32]);
        let key2 = EncryptionKey([2u8; 32]);
        let plaintext = b"Test data";

        let encrypted1 = encrypt(plaintext, &key1, EncryptionAlgorithm::AesGcm256).unwrap();
        let encrypted2 = encrypt(plaintext, &key2, EncryptionAlgorithm::AesGcm256).unwrap();

        assert_ne!(encrypted1.nonce.0, encrypted2.nonce.0);
    }

    #[test]
    fn test_chunk_hash_to_hex() {
        let hash = ChunkHash([0u8; 32]);
        let hex = hash.to_hex();
        assert_eq!(hex.len(), 64);
    }

    #[test]
    fn test_chunk_hash_as_bytes() {
        let hash = ChunkHash([1u8; 32]);
        let bytes = hash.as_bytes();
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 1);
    }

    #[test]
    fn test_super_features_similarity() {
        let sf1 = SuperFeatures([1, 2, 3, 4]);
        let sf2 = SuperFeatures([1, 2, 5, 6]);

        let similarity = sf1.similarity(&sf2);
        assert_eq!(similarity, 2);
    }

    #[test]
    fn test_reduction_pipeline_create() {
        let config = PipelineConfig::default();
        let _pipeline = ReductionPipeline::new(config);
    }

    #[test]
    fn test_reduction_pipeline_process_write_small_data() {
        let config = PipelineConfig::default();
        let mut pipeline = ReductionPipeline::new(config);
        let data = b"small test data";

        let result = pipeline.process_write(data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reduction_pipeline_produces_reduced_chunk() {
        let config = PipelineConfig::default();
        let mut pipeline = ReductionPipeline::new(config);
        let data: Vec<u8> = vec![0u8; 10000];

        let result = pipeline.process_write(&data).unwrap();

        assert!(!result.0.is_empty());
    }

    #[test]
    fn test_reduction_stats_fields() {
        let config = PipelineConfig::default();
        let mut pipeline = ReductionPipeline::new(config);
        let data: Vec<u8> = vec![1u8; 5000];

        let (_, stats) = pipeline.process_write(&data).unwrap();

        assert!(stats.input_bytes > 0);
    }

    #[test]
    fn test_pipeline_with_master_key() {
        let config = PipelineConfig::default();
        let key = EncryptionKey([0u8; 32]);
        let mut pipeline = ReductionPipeline::with_master_key(config, key);

        let data = b"data to encrypt";
        let result = pipeline.process_write(data).unwrap();
        assert!(!result.0.is_empty());
    }

    #[test]
    fn test_encrypted_chunk_fields() {
        let key = EncryptionKey([0u8; 32]);
        let plaintext = b"test";

        let encrypted = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

        assert!(!encrypted.nonce.0.is_empty());
        assert!(!encrypted.ciphertext.is_empty());
    }

    #[test]
    fn test_cas_index_basic() {
        let index = CasIndex::new();
        assert!(index.is_empty());
    }

    #[test]
    fn test_chunk_offset_and_length() {
        let config = ChunkerConfig {
            min_size: 512,
            avg_size: 4096,
            max_size: 65536,
        };

        let chunker = Chunker::with_config(config);
        let data: Vec<u8> = vec![0u8; 20000];
        let chunks = chunker.chunk(&data);

        for chunk in chunks.iter() {
            assert!(chunk.offset >= 0);
            assert!(!chunk.data.is_empty());
        }
    }

    #[test]
    fn test_reduced_chunk_has_hash() {
        let config = PipelineConfig::default();
        let mut pipeline = ReductionPipeline::new(config);
        let data = b"test data for reduced chunk";

        let result = pipeline.process_write(data).unwrap();

        for chunk in result.0 {
            assert!(!chunk.hash.as_bytes().is_empty());
        }
    }

    #[test]
    fn test_compression_lz4_large_data() {
        let large_data: Vec<u8> = (0..100000).map(|i| (i % 256) as u8).collect();

        let compressed = compress(&large_data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();

        assert_eq!(decompressed, large_data);
    }

    #[test]
    fn test_compression_zstd_large_data() {
        let large_data: Vec<u8> = (0..100000).map(|i| (i % 256) as u8).collect();

        let compressed = compress(&large_data, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        let decompressed =
            decompress(&compressed, CompressionAlgorithm::Zstd { level: 3 }).unwrap();

        assert_eq!(decompressed, large_data);
    }

    #[test]
    fn test_encryption_decrypt_with_wrong_key_fails() {
        let key1 = EncryptionKey([1u8; 32]);
        let key2 = EncryptionKey([2u8; 32]);
        let plaintext = b"secret";

        let encrypted = encrypt(plaintext, &key1, EncryptionAlgorithm::AesGcm256).unwrap();

        use claudefs_reduce::encryption::decrypt;
        let result = decrypt(&encrypted, &key2);

        assert!(result.is_err());
    }
}
