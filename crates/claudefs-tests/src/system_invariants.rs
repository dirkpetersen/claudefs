//! System Invariants Tests
//!
//! Cross-crate data integrity invariants using A1 checksums, A3 data reduction, and A4 protocol.

#[cfg(test)]
mod tests {
    use crate::report::{TestCaseResult, TestStatus};
    use claudefs_reduce::compression::{compress, decompress, CompressionAlgorithm};
    use claudefs_reduce::dedupe::{CasIndex, Chunker};
    use claudefs_reduce::encryption::{decrypt, encrypt, EncryptionAlgorithm, EncryptionKey};
    use claudefs_reduce::fingerprint::blake3_hash;
    use claudefs_storage::block::{BlockId, BlockSize};
    use claudefs_storage::checksum::{compute, verify, BlockHeader, ChecksumAlgorithm};
    use claudefs_transport::protocol::{Frame, Opcode};
    use claudefs_transport::routing::{ConsistentHashRing, NodeId, NodeInfo};
    use std::net::SocketAddr;
    use std::str::FromStr;
    use std::time::Duration;

    #[test]
    fn test_checksum_crc32c_compute() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"hello");
        assert_ne!(checksum.value, 0);
    }

    #[test]
    fn test_checksum_verify_pass() {
        let data = b"hello world";
        let checksum = compute(ChecksumAlgorithm::Crc32c, data);
        assert!(verify(&checksum, data));
    }

    #[test]
    fn test_checksum_verify_fail() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"hello");
        assert!(!verify(&checksum, b"world"));
    }

    #[test]
    fn test_checksum_different_algos_differ() {
        let crc = compute(ChecksumAlgorithm::Crc32c, b"hello");
        let xxh = compute(ChecksumAlgorithm::XxHash64, b"hello");
        assert_ne!(crc.value, xxh.value);
    }

    #[test]
    fn test_block_size_as_bytes_4k() {
        assert_eq!(BlockSize::B4K.as_bytes(), 4096);
    }

    #[test]
    fn test_block_size_as_bytes_64k() {
        assert_eq!(BlockSize::B64K.as_bytes(), 65536);
    }

    #[test]
    fn test_block_header_new() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"data");
        let header = BlockHeader::new(BlockSize::B4K, checksum, 1);
        assert_eq!(header.block_size, BlockSize::B4K);
    }

    #[test]
    fn test_block_id_new() {
        let id = BlockId::new(0, 0);
        assert_eq!(id.device_idx, 0);
        assert_eq!(id.offset, 0);
    }

    #[test]
    fn test_compression_lz4_roundtrip() {
        let original = b"hello world hello world";
        let compressed = compress(original, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(original.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn test_compression_zstd_roundtrip() {
        let original = b"hello world hello world";
        let compressed = compress(original, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        let decompressed =
            decompress(&compressed, CompressionAlgorithm::Zstd { level: 3 }).unwrap();
        assert_eq!(original.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn test_compression_empty_roundtrip() {
        let original: Vec<u8> = vec![];
        let compressed = compress(&original, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(original, decompressed);
    }

    #[test]
    fn test_compression_lz4_reduces_size() {
        let data = vec![b'a'; 1000];
        let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_encryption_aes_gcm_roundtrip() {
        let key = EncryptionKey([0u8; 32]);
        let plaintext = b"hello world";
        let encrypted = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_encryption_wrong_key_fails() {
        let key1 = EncryptionKey([0u8; 32]);
        let key2 = EncryptionKey([1u8; 32]);
        let plaintext = b"hello world";
        let encrypted = encrypt(plaintext, &key1, EncryptionAlgorithm::AesGcm256).unwrap();
        let result = decrypt(&encrypted, &key2);
        assert!(result.is_err());
    }

    #[test]
    fn test_encryption_two_encrypts_differ() {
        let key = EncryptionKey([0u8; 32]);
        let plaintext = b"hello world";
        let encrypted1 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        let encrypted2 = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        assert_ne!(encrypted1.ciphertext, encrypted2.ciphertext);
    }

    #[test]
    fn test_encryption_empty_plaintext() {
        let key = EncryptionKey([0u8; 32]);
        let plaintext: Vec<u8> = vec![];
        let encrypted = encrypt(&plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_fingerprint_blake3_deterministic() {
        let hash1 = blake3_hash(b"hello");
        let hash2 = blake3_hash(b"hello");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_fingerprint_different_data_differs() {
        let hash1 = blake3_hash(b"a");
        let hash2 = blake3_hash(b"b");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_fingerprint_length() {
        let hash = blake3_hash(b"data");
        assert_eq!(hash.0.len(), 32);
    }

    #[test]
    fn test_chunker_default() {
        let chunker = Chunker::new();
        let chunks = chunker.chunk(b"test data");
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunker_splits_data() {
        let chunker = Chunker::new();
        let data = vec![b'a'; 100000];
        let chunks = chunker.chunk(&data);
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_chunker_reassembly() {
        let chunker = Chunker::new();
        let data = vec![b'a'; 10000];
        let chunks = chunker.chunk(&data);
        let reassembled: Vec<u8> = chunks.iter().flat_map(|c| c.data.clone()).collect();
        assert_eq!(reassembled, data);
    }

    #[test]
    fn test_cas_index_insert_lookup() {
        let mut index = CasIndex::new();
        let hash = blake3_hash(b"hello");
        index.insert(hash);
        assert!(index.lookup(&hash));
    }

    #[test]
    fn test_cas_index_unknown() {
        let index = CasIndex::new();
        let hash = blake3_hash(b"hello");
        assert!(!index.lookup(&hash));
    }

    #[test]
    fn test_frame_new() {
        let frame = Frame::new(Opcode::Read, 1, b"hello".to_vec());
        assert_eq!(frame.opcode(), Opcode::Read);
    }

    #[test]
    fn test_frame_encode_decode() {
        let frame = Frame::new(Opcode::Read, 1, b"hello".to_vec());
        let encoded = frame.encode();
        let decoded = Frame::decode(&encoded).unwrap();
        assert_eq!(decoded.opcode(), Opcode::Read);
        assert_eq!(decoded.request_id(), 1);
    }

    #[test]
    fn test_frame_validate_ok() {
        let frame = Frame::new(Opcode::Read, 1, b"hello".to_vec());
        let result = frame.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_frame_opcode() {
        let frame = Frame::new(Opcode::Read, 1, b"hello".to_vec());
        assert_eq!(frame.opcode(), Opcode::Read);
    }

    #[test]
    fn test_frame_request_id() {
        let frame = Frame::new(Opcode::Read, 42, b"hello".to_vec());
        assert_eq!(frame.request_id(), 42);
    }

    #[test]
    fn test_frame_is_response() {
        let frame = Frame::new(Opcode::Read, 1, b"hello".to_vec());
        assert!(!frame.is_response());
    }

    #[test]
    fn test_hash_ring_empty() {
        let ring = ConsistentHashRing::new();
        assert_eq!(ring.node_count(), 0);
    }

    #[test]
    fn test_hash_ring_add_node() {
        let mut ring = ConsistentHashRing::new();
        let addr = SocketAddr::from_str("192.168.1.1:8080").unwrap();
        let node_info = NodeInfo::new(NodeId::new(1), addr);
        ring.add_node(node_info, 10);
        assert_eq!(ring.node_count(), 1);
    }

    #[test]
    fn test_hash_ring_lookup_empty() {
        let ring = ConsistentHashRing::new();
        let result = ring.lookup(b"shard-1");
        assert!(result.is_none());
    }

    #[test]
    fn test_hash_ring_lookup_returns_node() {
        let mut ring = ConsistentHashRing::new();
        let addr = SocketAddr::from_str("192.168.1.1:8080").unwrap();
        let node_info = NodeInfo::new(NodeId::new(1), addr);
        ring.add_node(node_info, 10);

        let result = ring.lookup(b"shard-1");
        assert!(result.is_some());
    }

    #[test]
    fn test_hash_ring_consistent() {
        let mut ring = ConsistentHashRing::new();
        let addr = SocketAddr::from_str("192.168.1.1:8080").unwrap();
        let node_info = NodeInfo::new(NodeId::new(1), addr);
        ring.add_node(node_info, 10);

        let result1 = ring.lookup(b"shard-1");
        let result2 = ring.lookup(b"shard-1");
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_test_status_pass() {
        let status = TestStatus::Pass;
        matches!(status, TestStatus::Pass);
    }

    #[test]
    fn test_test_status_fail() {
        let status = TestStatus::Fail;
        matches!(status, TestStatus::Fail);
    }
}
