# A3 Data Reduction — Phase 2 Enhancements

You are implementing Phase 2 enhancements for the `claudefs-reduce` crate in the ClaudeFS distributed filesystem project.

## Context

The `claudefs-reduce` crate (at `/home/cfs/claudefs/crates/claudefs-reduce/`) currently has:
- `pipeline.rs` — Core pipeline: chunk → deduplicate → compress → encrypt
- `meta_bridge.rs` — Sync `FingerprintStore` trait + `LocalFingerprintStore` + `NullFingerprintStore`
- `write_path.rs` — `IntegratedWritePath<F: FingerprintStore>` struct
- `dedupe.rs` — FastCDC chunking, CasIndex
- `encryption.rs` — AES-256-GCM, ChaCha20-Poly1305, HKDF
- `compression.rs` — LZ4, Zstd
- `fingerprint.rs` — BLAKE3 hashing, SuperFeatures (MinHash)
- `gc.rs` — Garbage collection engine
- `similarity.rs` — SimilarityIndex, DeltaCompressor
- `segment.rs` — SegmentPacker (2MB segments for erasure coding)
- `background.rs` — Async background processor
- `key_manager.rs` — Envelope encryption, key rotation
- `key_rotation_scheduler.rs` — Async key rotation scheduling
- `metrics.rs` — Prometheus-style metrics
- `recompressor.rs` — Background LZ4→Zstd recompression
- `snapshot.rs` — CoW snapshot management
- `worm_reducer.rs` — WORM/retention policy enforcement
- `error.rs` — ReduceError enum

## Current meta_bridge.rs FingerprintStore trait (SYNC)

```rust
pub trait FingerprintStore: Send + Sync {
    fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation>;
    fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool;
    fn increment_ref(&self, hash: &[u8; 32]) -> bool;
    fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64>;
    fn entry_count(&self) -> usize;
}
```

## What You Need to Implement

### 1. New file: `src/async_meta_bridge.rs`

Implement an **async variant** of the fingerprint store trait for Tokio integration, alongside a concrete async implementation. This is needed for Phase 2 integration with the distributed A2 metadata service which is Tokio-based.

The file must include:

#### `AsyncFingerprintStore` trait
```rust
/// Async version of FingerprintStore for Tokio-based distributed metadata integration.
/// Implementors can delegate to A2's distributed fingerprint index.
#[async_trait::async_trait]
pub trait AsyncFingerprintStore: Send + Sync {
    async fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation>;
    async fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool;
    async fn increment_ref(&self, hash: &[u8; 32]) -> bool;
    async fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64>;
    async fn entry_count(&self) -> usize;
}
```

#### `AsyncLocalFingerprintStore` struct
- Uses `tokio::sync::RwLock<HashMap<[u8; 32], (BlockLocation, u64)>>`
- Implements `AsyncFingerprintStore`
- Also implements `FingerprintStore` (sync trait) using `blocking_read`/`blocking_write`
- Same semantics as `LocalFingerprintStore` but fully async
- Methods: `new()`, `total_deduplicated_bytes() -> u64` (async)

#### `AsyncNullFingerprintStore` struct
- No-op implementation of `AsyncFingerprintStore` for testing
- All operations are no-ops (lookup returns None, insert returns true, refs return false/None, count returns 0)
- Also implements `FingerprintStore` (sync trait)

#### `AsyncIntegratedWritePath<F: AsyncFingerprintStore>` struct
An async version of `IntegratedWritePath` that:
- Takes `Arc<F>` where F: `AsyncFingerprintStore`
- Has `async fn process_write(&mut self, data: &[u8]) -> Result<WritePathResult, ReduceError>`
- Has `async fn flush_segments(&mut self) -> Vec<Segment>`
- Has `fn stats_snapshot(&self) -> WritePathStats`
- Uses the same pipeline and packer internally as `IntegratedWritePath`
- New write path:
  1. Run through `ReductionPipeline::process_write(data)` (sync)
  2. For each chunk, `await` the fingerprint store lookup
  3. If found in distributed store, increment ref (distributed dedup hit)
  4. If not found and not duplicate, pack into segment + insert into store
  5. Return `WritePathResult`

#### Config and constructors:
```rust
impl<F: AsyncFingerprintStore> AsyncIntegratedWritePath<F> {
    pub fn new(config: WritePathConfig, fingerprint_store: Arc<F>) -> Self { ... }
    pub fn new_with_key(config: WritePathConfig, master_key: EncryptionKey, fingerprint_store: Arc<F>) -> Self { ... }
}
```

#### Comprehensive tests:
- `test_async_basic_write`: Writes 10K bytes through async path, checks chunks and stats
- `test_async_encryption_write`: Encryption path works
- `test_async_flush_segments`: Flush returns partial segments
- `test_async_distributed_dedup`: Two write paths sharing same store, second gets dedup hits
- `test_async_null_store`: Works with no-op store
- `test_async_large_data`: 1MB write works correctly
- `test_async_concurrent_writes`: Uses `tokio::spawn` to run two writers concurrently on a shared store (use `Arc<AsyncLocalFingerprintStore>` split across two `AsyncIntegratedWritePath` instances)

**NOTE**: You need to add `async-trait = "0.1"` to the dependencies in `Cargo.toml`. Do NOT use nightly async trait syntax — use `#[async_trait::async_trait]` macro.

### 2. New file: `src/checksum.rs`

Implement end-to-end data integrity checksums. These are separate from the BLAKE3 CAS hashes — these are for detecting silent data corruption on storage.

The file must include:

#### `ChecksumAlgorithm` enum
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ChecksumAlgorithm {
    #[default]
    Blake3,       // Full BLAKE3 (32 bytes, cryptographically strong, used for CAS)
    Crc32c,       // CRC32C (4 bytes, hardware-accelerated, fast for integrity check)
    Xxhash64,     // xxHash64 (8 bytes, very fast non-crypto hash)
}
```

#### `DataChecksum` struct
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataChecksum {
    pub algorithm: ChecksumAlgorithm,
    pub bytes: Vec<u8>,  // 4 bytes for CRC32C, 8 for xxHash64, 32 for BLAKE3
}
```

#### Functions:
```rust
/// Compute a checksum of the given data
pub fn compute(data: &[u8], algo: ChecksumAlgorithm) -> DataChecksum { ... }

/// Verify data matches the expected checksum.
/// Returns Ok(()) if valid, Err(ReduceError::ChecksumMismatch) if invalid.
pub fn verify(data: &[u8], expected: &DataChecksum) -> Result<(), ReduceError> { ... }
```

#### `ChecksummedBlock` struct
```rust
/// A data block with attached checksum for end-to-end integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksummedBlock {
    pub data: Vec<u8>,
    pub checksum: DataChecksum,
}

impl ChecksummedBlock {
    pub fn new(data: Vec<u8>, algo: ChecksumAlgorithm) -> Self { ... }
    pub fn verify(&self) -> Result<(), ReduceError> { ... }
}
```

#### Implementation notes:
- For `Blake3`: use the `blake3` crate, `blake3::hash(data)`, output is 32 bytes
- For `Crc32c`: implement manually using a simple CRC32C table OR use polynomial `0x82F63B78`
  - Simple implementation: compute CRC32C with Castagnoli polynomial
  - Store as 4 little-endian bytes
- For `Xxhash64`: implement a simple xxHash64 or use a simplified version
  - You can implement a basic xxHash64 using the published algorithm constants:
    - PRIME1 = 11400714785074694791u64
    - PRIME2 = 14029467366897019727u64
    - PRIME3 = 1609587929392839161u64
    - PRIME4 = 9650029242287828579u64
    - PRIME5 = 2870177450012600261u64
  - Store as 8 little-endian bytes

#### `ChecksumMismatch` error:
Add to `error.rs`:
```rust
/// Data integrity checksum mismatch — potential silent corruption
ChecksumMismatch,
```

#### Comprehensive tests:
- `test_blake3_roundtrip`: compute + verify with Blake3 works
- `test_crc32c_roundtrip`: compute + verify with CRC32C works
- `test_xxhash64_roundtrip`: compute + verify with xxHash64 works
- `test_corrupted_data_fails`: flip one byte, verify returns error for all algos
- `test_empty_data`: all algorithms handle empty slice
- `test_checksummed_block`: ChecksummedBlock::new + verify works
- `test_checksummed_block_corruption`: detected correctly
- proptest: `prop_blake3_stable` — same data always produces same checksum
- proptest: `prop_crc32c_stable` — same data always produces same checksum
- proptest: `prop_xxhash64_stable` — same data always produces same checksum

### 3. Updates to `src/lib.rs`

Add these two new modules:
```rust
pub mod async_meta_bridge;
pub mod checksum;
```

And add these public re-exports:
```rust
pub use async_meta_bridge::{
    AsyncFingerprintStore, AsyncIntegratedWritePath, AsyncLocalFingerprintStore, AsyncNullFingerprintStore
};
pub use checksum::{ChecksumAlgorithm, ChecksummedBlock, DataChecksum};
```

### 4. Updates to `src/error.rs`

Add `ChecksumMismatch` variant to `ReduceError`:
```rust
/// Data integrity checksum mismatch — silent data corruption detected
#[error("checksum mismatch — silent data corruption detected")]
ChecksumMismatch,
```

### 5. Updates to `Cargo.toml`

Add dependency:
```toml
async-trait = "0.1"
```

## Existing Code to Reference

### error.rs (current):
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReduceError {
    #[error("compression failed: {0}")]
    CompressionFailed(String),
    #[error("decompression failed: {0}")]
    DecompressionFailed(String),
    #[error("encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("decryption authentication failed")]
    DecryptionAuthFailed,
    #[error("missing encryption key")]
    MissingKey,
    #[error("missing chunk data for read (duplicate chunk not locally available)")]
    MissingChunkData,
    #[error("policy downgrade attempted")]
    PolicyDowngradeAttempted,
}
```

### Cargo.toml (current [dependencies]):
```toml
[dependencies]
tokio.workspace = true
zeroize = { version = "1.7", features = ["derive"] }
thiserror.workspace = true
serde.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
blake3 = "1"
fastcdc = "3"
lz4_flex = { version = "0.11", features = ["frame"] }
zstd = "0.13"
aes-gcm = "0.10"
chacha20poly1305 = "0.10"
hkdf = "0.12"
sha2 = "0.10"
rand = "0.8"
bytes = "1"
```

## Quality Requirements

1. **All files must compile with `cargo build -p claudefs-reduce`**
2. **All tests must pass with `cargo test -p claudefs-reduce`**
3. **Zero clippy warnings (`cargo clippy -p claudefs-reduce`)**
4. **Documentation**: Every public item must have a doc comment (`#![warn(missing_docs)]` is set in lib.rs)
5. **Error handling**: Use `ReduceError` from `crate::error` everywhere
6. **Imports**: Use `crate::` for internal imports, workspace dependencies as defined
7. **No `unsafe` code** — this crate is safe Rust only
8. **Follow existing patterns**: serde Serialize/Deserialize on configs and data types, tracing for logging

## Deliverables

Provide the complete, final content of these files:
1. `/home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs` — complete file
2. `/home/cfs/claudefs/crates/claudefs-reduce/src/checksum.rs` — complete file
3. `/home/cfs/claudefs/crates/claudefs-reduce/src/error.rs` — complete updated file (add ChecksumMismatch)
4. `/home/cfs/claudefs/crates/claudefs-reduce/src/lib.rs` — complete updated file (add new modules + re-exports)
5. `/home/cfs/claudefs/crates/claudefs-reduce/Cargo.toml` — complete updated file (add async-trait)

Format each file as:
```
=== FILE: path/to/file.rs ===
<complete file content>
=== END FILE ===
```
