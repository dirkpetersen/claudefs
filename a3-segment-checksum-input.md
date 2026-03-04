# A3 Data Reduction — Segment Integrity Checksum Integration

You are implementing a targeted enhancement for the `claudefs-reduce` crate in ClaudeFS.

## Goal

Integrate the `checksum.rs` module into `segment.rs` to add end-to-end data integrity verification for sealed segments. This prevents silent data corruption from going undetected when segments are read back from flash storage or S3.

## Current State

The `Segment` struct in `segment.rs` currently has NO integrity checksum:

```rust
pub struct Segment {
    pub id: u64,
    pub entries: Vec<SegmentEntry>,
    pub payload: Vec<u8>,
    pub sealed: bool,
    pub created_at_secs: u64,
}
```

The `checksum.rs` module has been added to the crate with:
- `ChecksumAlgorithm` enum: `Blake3`, `Crc32c`, `Xxhash64`
- `DataChecksum` struct: `{ algorithm: ChecksumAlgorithm, bytes: Vec<u8> }`
- `ChecksummedBlock` struct: `{ data: Vec<u8>, checksum: DataChecksum }`
- `compute(data: &[u8], algo: ChecksumAlgorithm) -> DataChecksum`
- `verify(data: &[u8], expected: &DataChecksum) -> Result<(), ReduceError>`
- `ChecksumMismatch` error variant in `ReduceError`

## Task

Modify `src/segment.rs` to add payload integrity checksumming:

### 1. Add `payload_checksum` field to `Segment`

```rust
pub struct Segment {
    pub id: u64,
    pub entries: Vec<SegmentEntry>,
    pub payload: Vec<u8>,
    pub sealed: bool,
    pub created_at_secs: u64,
    /// CRC32C checksum of the payload bytes (computed when segment is sealed)
    pub payload_checksum: Option<crate::checksum::DataChecksum>,
}
```

- The field is `Option<DataChecksum>` because unsealed segments don't have a checksum yet
- Use CRC32C as the default algorithm (fast hardware-accelerated check)

### 2. Add `Segment::verify_integrity()` method

```rust
impl Segment {
    /// Verify the integrity of the segment payload against the stored checksum.
    /// Returns Ok(()) if valid, Err(ReduceError::ChecksumMismatch) if invalid,
    /// or Err(ReduceError::ChecksumMissing) if the segment has no checksum.
    pub fn verify_integrity(&self) -> Result<(), crate::error::ReduceError> {
        match &self.payload_checksum {
            Some(checksum) => crate::checksum::verify(&self.payload, checksum),
            None => Err(crate::error::ReduceError::ChecksumMissing),
        }
    }

    // existing methods unchanged: total_chunks(), total_payload_bytes()
}
```

### 3. Update `SegmentPacker` to compute checksum when sealing

In `SegmentPacker::add_chunk()`, when a segment becomes full and is sealed:
```rust
// After: segment.sealed = true;
segment.payload_checksum = Some(crate::checksum::compute(
    &segment.payload,
    crate::checksum::ChecksumAlgorithm::Crc32c,
));
```

In `SegmentPacker::flush()`, compute checksum before returning:
```rust
// After: segment.sealed = true;
segment.payload_checksum = Some(crate::checksum::compute(
    &segment.payload,
    crate::checksum::ChecksumAlgorithm::Crc32c,
));
```

### 4. Add `ChecksumMissing` error variant to `error.rs`

```rust
/// No checksum available for integrity verification (segment not yet sealed)
#[error("checksum missing — segment has no integrity checksum")]
ChecksumMissing,
```

### 5. Update tests in `segment.rs`

Add these tests:
- `test_sealed_segment_has_checksum`: After `flush()`, the segment's `payload_checksum` is `Some`
- `test_full_segment_has_checksum`: After `add_chunk()` returns a sealed segment, it has a checksum
- `test_segment_verify_integrity`: A sealed segment passes `verify_integrity()`
- `test_segment_verify_corruption`: Corrupt the payload by flipping a byte, then `verify_integrity()` returns `Err(ReduceError::ChecksumMismatch)`
- `test_unsealed_no_checksum`: An unsealed segment (no flush) has `payload_checksum = None`
- `test_verify_missing_checksum`: A segment with `payload_checksum = None` returns `Err(ReduceError::ChecksumMissing)`

## Files to Modify

Please provide the complete updated content for:
1. `src/segment.rs` — with `payload_checksum` field, `verify_integrity()`, updated packing
2. `src/error.rs` — add `ChecksumMissing` variant

## Current error.rs Content

```rust
//! Error types for the claudefs-reduce subsystem

/// All errors that can occur during data reduction operations
#[derive(Debug, thiserror::Error)]
pub enum ReduceError {
    /// Compression operation failed
    #[error("Compression failed: {0}")]
    CompressionFailed(String),
    /// Decompression operation failed
    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),
    /// Encryption operation failed
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    /// Decryption authentication tag mismatch — data may be corrupted or tampered
    #[error("Decryption failed: authentication tag mismatch (data may be corrupted)")]
    DecryptionAuthFailed,
    /// Encryption is enabled but no master key was provided
    #[error("Missing encryption key: encryption is enabled but no master key was set")]
    MissingKey,
    /// Chunk is marked as duplicate but reference data was not provided for read
    #[error("Missing chunk data: chunk is_duplicate=true but reference data not provided")]
    MissingChunkData,
    /// I/O error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Attempted to downgrade WORM retention policy
    #[error("Cannot downgrade WORM retention policy")]
    PolicyDowngradeAttempted,
    /// Data integrity checksum mismatch — silent data corruption detected
    #[error("checksum mismatch — silent data corruption detected")]
    ChecksumMismatch,
}
```

## Quality Requirements

1. **Compile with zero errors**: `cargo build -p claudefs-reduce`
2. **All tests pass**: `cargo test -p claudefs-reduce`
3. **Zero clippy warnings**: `cargo clippy -p claudefs-reduce`
4. **Documentation**: Every new public item must have a doc comment
5. **No unsafe code**
6. **Don't break existing tests** - the existing segment tests must still pass

## Deliverables

Format as:
```
=== FILE: crates/claudefs-reduce/src/segment.rs ===
<complete file>
=== END FILE ===

=== FILE: crates/claudefs-reduce/src/error.rs ===
<complete file>
=== END FILE ===
```
