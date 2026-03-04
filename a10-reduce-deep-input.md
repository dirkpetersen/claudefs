# Task: Write reduce_deep_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-reduce` crate focusing on encryption, key management, dedup oracle attacks, compression bombs, and GC safety.

## File location
`crates/claudefs-security/src/reduce_deep_security_tests.rs`

## Module structure
```rust
//! Deep security tests for claudefs-reduce: encryption, key mgmt, dedup, compression, GC.
//!
//! Part of A10 Phase 6: Reduce deep security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs pub use statements)

```rust
use claudefs_reduce::{
    // Encryption
    EncryptedChunk, EncryptionAlgorithm, EncryptionKey,
    // Key management
    DataKey, KeyManager, KeyManagerConfig, KeyVersion, VersionedKey, WrappedKey,
    // Dedup
    CasIndex, Chunk, Chunker, ChunkerConfig,
    // Fingerprint
    ChunkHash, SuperFeatures,
    // Compression
    CompressionAlgorithm,
    // Checksum
    ChecksumAlgorithm, ChecksummedBlock, DataChecksum,
    // Pipeline
    PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats,
    // GC
    GcConfig, GcEngine, GcStats,
    // Segment
    Segment, SegmentEntry, SegmentPacker, SegmentPackerConfig,
    // Snapshot
    Snapshot, SnapshotConfig, SnapshotInfo, SnapshotManager,
    // Error
    ReduceError,
};
// Module-path imports:
use claudefs_reduce::encryption::{derive_chunk_key, random_nonce, Nonce};
use claudefs_reduce::compression::{compress, decompress, is_compressible};
use claudefs_reduce::checksum::{compute as compute_checksum, verify as verify_checksum};
use claudefs_reduce::fingerprint::{blake3_hash, super_features};
use claudefs_reduce::dedupe::CasIndex;
```

**IMPORTANT**: Not all may be public. If any import fails, remove it. Start with what's most likely available.

## Test categories (25 tests total, 5 per category)

### Category 1: Encryption & Key Management (5 tests)

1. **test_encryption_deterministic_dek** — Call derive_chunk_key with same master key and same chunk_hash twice. Verify derived keys are identical (FINDING: deterministic DEK means same plaintext always encrypts identically when nonce is reused).

2. **test_encryption_different_chunks_different_keys** — Call derive_chunk_key with same master but different chunk_hashes. Verify derived keys are different.

3. **test_key_rotation_preserves_decryption** — Create KeyManager with KEK. Wrap a DEK. Rotate KEK. Verify old wrapped DEK can still be unwrapped (historical keys preserved).

4. **test_key_wrap_tamper_detection** — Create KeyManager, wrap a DEK. Modify one byte of the WrappedKey ciphertext. Try to unwrap. Verify it fails (AEAD authentication).

5. **test_nonce_uniqueness** — Generate 1000 nonces with random_nonce(). Verify all are unique (collision probability should be negligible in 96-bit space).

### Category 2: Dedup & Fingerprint Security (5 tests)

6. **test_cas_refcount_underflow** — Create CasIndex. Insert a hash. Release it (refcount→0). Release again. Verify refcount stays at 0, not wrapping.

7. **test_cas_drain_unreferenced** — Create CasIndex. Insert hash A twice (refcount=2). Insert hash B once (refcount=1). Release B. Drain unreferenced. Verify B is returned but not A.

8. **test_blake3_deterministic** — Hash same data twice with blake3_hash(). Verify identical hashes.

9. **test_super_features_tiny_data** — Call super_features() on data shorter than 4 bytes. Verify returns [0,0,0,0] (FINDING: all tiny chunks produce same features, causing false-positive similarity).

10. **test_chunker_reassembly** — Create Chunker, chunk some data. Concatenate all chunk.data fields. Verify equals original data.

### Category 3: Compression Security (5 tests)

11. **test_compression_roundtrip_lz4** — Compress and decompress with LZ4. Verify data unchanged.

12. **test_compression_roundtrip_zstd** — Compress and decompress with Zstd. Verify data unchanged.

13. **test_compression_none_passthrough** — Compress with CompressionAlgorithm::None. Verify output equals input.

14. **test_compressible_detection** — Call is_compressible on highly repetitive data (e.g., 1000 bytes of 'A'). Verify returns true. Call on random bytes. Verify returns false.

15. **test_compression_empty_data** — Compress empty data with LZ4 and Zstd. Verify roundtrip produces empty output.

### Category 4: Checksum & Integrity (5 tests)

16. **test_checksum_blake3_corruption** — Compute BLAKE3 checksum. Flip one bit in data. Verify verification fails.

17. **test_checksum_crc32c_collision_risk** — Compute CRC32C for two different inputs. Document that CRC32C is non-cryptographic (FINDING: not suitable for malicious tampering detection).

18. **test_checksummed_block_roundtrip** — Create ChecksummedBlock with BLAKE3. Verify verify_integrity() succeeds.

19. **test_checksum_algorithm_downgrade** — Compute checksum with BLAKE3. Verify with CRC32C (different algorithm). Verify mismatch detected (FINDING if algorithms not pinned).

20. **test_checksum_empty_data** — Compute checksums on empty data for all three algorithms. Verify deterministic results.

### Category 5: Pipeline & GC Security (5 tests)

21. **test_pipeline_write_read_roundtrip** — Create ReductionPipeline with compression and NO encryption. Write data. Read back non-duplicate chunks. Verify equals original.

22. **test_pipeline_dedup_detection** — Write same data block twice through pipeline. Verify second write has is_duplicate=true for all chunks.

23. **test_gc_sweep_unreferenced** — Create GcEngine. Create CasIndex with hashes A, B. Mark only A as reachable. Sweep. Verify B is collected, A is not.

24. **test_snapshot_max_limit** — Create SnapshotManager with max_snapshots=2. Create 2 snapshots. Try to create 3rd. Verify error.

25. **test_segment_packing** — Create SegmentPacker. Add multiple entries. Seal. Verify segment contains all entries and verify_integrity() passes.

## Implementation notes
- Use `fn make_xxx()` helper functions
- Mark security findings with `// FINDING-REDUCE-DEEP-XX: description`
- If a type is not public, skip and replace
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
