# A10 Phase 3c: Security Tests for Reduce and Repl Crates

## Task

Add two new test modules to the `claudefs-security` crate:
1. `reduce_security_tests.rs` — Security tests for the `claudefs-reduce` crate
2. `repl_security_tests.rs` — Security tests for the `claudefs-repl` crate

Both files go in `crates/claudefs-security/src/`.

Also update `crates/claudefs-security/src/lib.rs` to register both new modules.

## Important Constraints

- All tests must compile and pass with `cargo test -p claudefs-security`
- Use only public APIs from `claudefs-reduce` and `claudefs-repl`
- Tests should document security findings (use eprintln for findings detected)
- Use `#[cfg(test)]` module wrapping
- No unsafe code
- When testing for input validation gaps, DON'T assert failure — instead detect whether the issue exists and log it

## Current lib.rs (last 4 lines to show where to add)

The file ends with:
```rust
#[cfg(test)]
pub mod fuse_security_tests;
#[cfg(test)]
pub mod transport_security_tests;
```

Add after these:
```rust
#[cfg(test)]
pub mod reduce_security_tests;
#[cfg(test)]
pub mod repl_security_tests;
```

## Dependencies already available in Cargo.toml

```toml
[dependencies]
claudefs-reduce = { path = "../claudefs-reduce" }
claudefs-repl = { path = "../claudefs-repl" }
# ... others

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
proptest = "1.4"
bincode.workspace = true
sha2.workspace = true
blake3.workspace = true
zeroize = { version = "1.7", features = ["derive"] }
```

## Public API Reference

### claudefs-reduce

#### GC Engine
```rust
pub struct GcEngine { ... }
impl GcEngine {
    pub fn new(config: GcConfig) -> Self;
    pub fn mark_reachable(&mut self, hashes: &[ChunkHash]);
    pub fn clear_marks(&mut self);
    pub fn is_marked(&self, hash: &ChunkHash) -> bool;
    pub fn sweep(&mut self, cas: &mut CasIndex) -> GcStats;
    pub fn run_cycle(&mut self, cas: &mut CasIndex, reachable_hashes: &[ChunkHash]) -> GcStats;
}
pub struct GcConfig { pub sweep_threshold: usize }
pub struct GcStats { pub chunks_scanned: usize, pub chunks_reclaimed: usize, pub bytes_reclaimed: u64 }
```

#### Key Manager
```rust
pub struct KeyVersion(pub u32);
pub struct DataKey { pub key: [u8; 32] }
pub struct WrappedKey { pub ciphertext: Vec<u8>, pub nonce: [u8; 12], pub kek_version: KeyVersion }
pub struct VersionedKey { pub version: KeyVersion, pub key: EncryptionKey }
pub struct KeyManagerConfig { pub max_key_history: usize }

pub struct KeyManager { ... }
impl KeyManager {
    pub fn new(config: KeyManagerConfig) -> Self;
    pub fn with_initial_key(config: KeyManagerConfig, key: EncryptionKey) -> Self;
    pub fn current_version(&self) -> Option<KeyVersion>;
    pub fn rotate_key(&mut self, new_key: EncryptionKey) -> KeyVersion;
    pub fn generate_dek(&self) -> Result<DataKey, ReduceError>;
    pub fn wrap_dek(&self, dek: &DataKey) -> Result<WrappedKey, ReduceError>;
    pub fn unwrap_dek(&self, wrapped: &WrappedKey) -> Result<DataKey, ReduceError>;
    pub fn rewrap_dek(&mut self, old_wrapped: &WrappedKey) -> Result<WrappedKey, ReduceError>;
    pub fn is_current_version(&self, wrapped: &WrappedKey) -> bool;
    pub fn history_size(&self) -> usize;
    pub fn clear_history(&mut self);
}
```

#### Key Rotation Scheduler
```rust
pub enum RotationStatus {
    Idle,
    Scheduled { target_version: KeyVersion },
    InProgress { target_version: KeyVersion, rewrapped: usize, total: usize },
    Complete { version: KeyVersion, rewrapped: usize },
    Failed { reason: String },
}
pub struct RotationEntry { pub chunk_id: u64, pub wrapped_key: WrappedKey, pub needs_rotation: bool }
pub struct RotationConfig { pub batch_size: usize }

pub struct KeyRotationScheduler { ... }
impl KeyRotationScheduler {
    pub fn new() -> Self;
    pub fn register_chunk(&mut self, chunk_id: u64, wrapped: WrappedKey);
    pub fn schedule_rotation(&mut self, target_version: KeyVersion) -> Result<(), ReduceError>;
    pub fn mark_needs_rotation(&mut self, old_version: KeyVersion);
    pub fn rewrap_next(&mut self, km: &mut KeyManager) -> Result<Option<u64>, ReduceError>;
    pub fn status(&self) -> &RotationStatus;
    pub fn pending_count(&self) -> usize;
    pub fn total_chunks(&self) -> usize;
    pub fn get_wrapped_key(&self, chunk_id: u64) -> Option<&WrappedKey>;
}
```

#### CAS Index
```rust
pub struct CasIndex { ... }
impl CasIndex {
    pub fn new() -> Self;
    pub fn lookup(&self, hash: &ChunkHash) -> bool;
    pub fn insert(&mut self, hash: ChunkHash);
    pub fn release(&mut self, hash: &ChunkHash) -> bool;
    pub fn refcount(&self, hash: &ChunkHash) -> u64;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn drain_unreferenced(&mut self) -> Vec<ChunkHash>;
    pub fn iter(&self) -> impl Iterator<Item = (&ChunkHash, u64)>;
}
```

#### Encryption
```rust
pub struct EncryptionKey(pub [u8; 32]);
pub struct Nonce(pub [u8; 12]);
pub enum EncryptionAlgorithm { AesGcm256, ChaCha20Poly1305 }
pub struct EncryptedChunk { pub ciphertext: Vec<u8>, pub nonce: Nonce, pub algo: EncryptionAlgorithm }

pub fn derive_chunk_key(master_key: &EncryptionKey, chunk_hash: &[u8; 32]) -> EncryptionKey;
pub fn random_nonce() -> Nonce;
pub fn encrypt(plaintext: &[u8], key: &EncryptionKey, algo: EncryptionAlgorithm) -> Result<EncryptedChunk, ReduceError>;
pub fn decrypt(chunk: &EncryptedChunk, key: &EncryptionKey) -> Result<Vec<u8>, ReduceError>;
```

#### Checksum
```rust
pub enum ChecksumAlgorithm { Blake3, Crc32c, Xxhash64 }
pub struct DataChecksum { pub algorithm: ChecksumAlgorithm, pub bytes: Vec<u8> }
pub struct ChecksummedBlock { pub data: Vec<u8>, pub checksum: DataChecksum }

pub fn compute(data: &[u8], algo: ChecksumAlgorithm) -> DataChecksum;
pub fn verify(data: &[u8], expected: &DataChecksum) -> Result<(), ReduceError>;

impl ChecksummedBlock {
    pub fn new(data: Vec<u8>, algo: ChecksumAlgorithm) -> Self;
    pub fn verify(&self) -> Result<(), ReduceError>;
}
```

#### Fingerprint
```rust
pub struct ChunkHash(pub [u8; 32]);
impl ChunkHash {
    pub fn to_hex(&self) -> String;
    pub fn as_bytes(&self) -> &[u8; 32];
}
pub struct SuperFeatures(pub [u64; 4]);
impl SuperFeatures {
    pub fn similarity(&self, other: &SuperFeatures) -> usize;
    pub fn is_similar(&self, other: &SuperFeatures) -> bool;
}
pub fn blake3_hash(data: &[u8]) -> ChunkHash;
pub fn super_features(data: &[u8]) -> SuperFeatures;
```

#### Compression
```rust
pub enum CompressionAlgorithm { None, Lz4, Zstd { level: i32 } }
pub fn compress(data: &[u8], algo: CompressionAlgorithm) -> Result<Vec<u8>, ReduceError>;
pub fn decompress(data: &[u8], algo: CompressionAlgorithm) -> Result<Vec<u8>, ReduceError>;
pub fn is_compressible(data: &[u8]) -> bool;
```

#### Segment
```rust
pub struct SegmentEntry { pub hash: ChunkHash, pub offset_in_segment: u32, pub payload_size: u32, pub original_size: u32 }
pub struct Segment { pub id: u64, pub entries: Vec<SegmentEntry>, pub payload: Vec<u8>, pub sealed: bool, pub created_at_secs: u64, pub payload_checksum: Option<DataChecksum> }
impl Segment {
    pub fn total_chunks(&self) -> usize;
    pub fn total_payload_bytes(&self) -> usize;
    pub fn verify_integrity(&self) -> Result<(), ReduceError>;
}

pub struct SegmentPackerConfig { pub target_size: usize }
pub struct SegmentPacker { ... }
impl SegmentPacker {
    pub fn new(config: SegmentPackerConfig) -> Self;
    pub fn add_chunk(&mut self, hash: ChunkHash, payload: &[u8], original_size: u32) -> Option<Segment>;
    pub fn flush(&mut self) -> Option<Segment>;
    pub fn current_size(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

#### Snapshot
```rust
pub struct SnapshotInfo { pub id: u64, pub name: String, pub created_at_secs: u64, pub block_count: usize, pub total_bytes: u64 }
pub struct Snapshot { pub info: SnapshotInfo, pub block_hashes: Vec<ChunkHash> }
pub struct SnapshotConfig { pub max_snapshots: usize }

pub struct SnapshotManager { ... }
impl SnapshotManager {
    pub fn new(config: SnapshotConfig) -> Self;
    pub fn create_snapshot(&mut self, name: String, block_hashes: Vec<ChunkHash>, total_bytes: u64) -> Result<SnapshotInfo, ReduceError>;
    pub fn delete_snapshot(&mut self, id: u64) -> Option<Snapshot>;
    pub fn get_snapshot(&self, id: u64) -> Option<&Snapshot>;
    pub fn list_snapshots(&self) -> Vec<&SnapshotInfo>;
    pub fn snapshot_count(&self) -> usize;
    pub fn clone_snapshot(&mut self, source_id: u64, new_name: String) -> Result<SnapshotInfo, ReduceError>;
    pub fn find_by_name(&self, name: &str) -> Option<&Snapshot>;
}
```

#### Error
```rust
pub enum ReduceError {
    CompressionFailed(String),
    DecompressionFailed(String),
    EncryptionFailed(String),
    DecryptionAuthFailed,
    MissingKey,
    MissingChunkData,
    Io(std::io::Error),
    PolicyDowngradeAttempted,
    ChecksumMismatch,
    ChecksumMissing,
}
```

### claudefs-repl

#### Journal
```rust
pub enum OpKind { Create, Unlink, Rename, Write, Truncate, SetAttr, Link, Symlink, MkDir, SetXattr, RemoveXattr }

pub struct JournalEntry {
    pub seq: u64,
    pub shard_id: u32,
    pub site_id: u64,
    pub timestamp_us: u64,
    pub inode: u64,
    pub op: OpKind,
    pub payload: Vec<u8>,
    pub crc32: u32,
}
impl JournalEntry {
    pub fn new(seq: u64, shard_id: u32, site_id: u64, timestamp_us: u64, inode: u64, op: OpKind, payload: Vec<u8>) -> Self;
    pub fn compute_crc(&self) -> u32;
    pub fn validate_crc(&self) -> bool;
}

pub struct JournalPosition { pub shard_id: u32, pub seq: u64 }
impl JournalPosition {
    pub fn new(shard_id: u32, seq: u64) -> Self;
}

pub struct JournalTailer { ... }
impl JournalTailer {
    pub fn new_in_memory(entries: Vec<JournalEntry>) -> Self;
    pub fn new_from_position(entries: Vec<JournalEntry>, pos: JournalPosition) -> Self;
    pub async fn next(&mut self) -> Option<JournalEntry>;
    pub fn position(&self) -> Option<JournalPosition>;
    pub fn append(&mut self, entry: JournalEntry);
    pub fn filter_by_shard(&self, shard_id: u32) -> Vec<&JournalEntry>;
}
```

#### Conduit
```rust
pub struct ConduitTlsConfig {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    pub ca_pem: Vec<u8>,
}
impl ConduitTlsConfig {
    pub fn new(cert_pem: Vec<u8>, key_pem: Vec<u8>, ca_pem: Vec<u8>) -> Self;
}

pub struct ConduitConfig {
    pub local_site_id: u64,
    pub remote_site_id: u64,
    pub remote_addrs: Vec<String>,
    pub tls: Option<ConduitTlsConfig>,
    pub max_batch_size: usize,
    pub reconnect_delay_ms: u64,
    pub max_reconnect_delay_ms: u64,
}
impl ConduitConfig {
    pub fn new(local_site_id: u64, remote_site_id: u64) -> Self;
}
impl Default for ConduitConfig { ... }

pub struct EntryBatch {
    pub source_site_id: u64,
    pub entries: Vec<JournalEntry>,
    pub batch_seq: u64,
}
impl EntryBatch {
    pub fn new(source_site_id: u64, entries: Vec<JournalEntry>, batch_seq: u64) -> Self;
}

pub struct ConduitStats { pub batches_sent: u64, pub batches_received: u64, pub entries_sent: u64, pub entries_received: u64, pub send_errors: u64, pub reconnects: u64 }
pub enum ConduitState { Connected, Reconnecting { attempt: u32, delay_ms: u64 }, Shutdown }
```

#### Site Registry
```rust
pub struct SiteId(pub u64);

pub struct SiteRecord {
    pub site_id: u64,
    pub display_name: String,
    pub tls_fingerprint: Option<[u8; 32]>,
    pub addresses: Vec<String>,
    pub added_at_us: u64,
    pub last_seen_us: u64,
}
impl SiteRecord {
    pub fn new(site_id: u64, display_name: &str) -> Self;
}

pub enum SiteRegistryError {
    AlreadyRegistered { site_id: u64 },
    NotFound { site_id: u64 },
    FingerprintMismatch { site_id: u64 },
}

pub struct SiteRegistry { ... }
impl SiteRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, record: SiteRecord) -> Result<(), SiteRegistryError>;
    pub fn unregister(&mut self, site_id: u64) -> Result<SiteRecord, SiteRegistryError>;
    pub fn lookup(&self, site_id: u64) -> Option<&SiteRecord>;
    pub fn verify_source_id(&self, site_id: u64, tls_fingerprint: Option<&[u8; 32]>) -> Result<(), SiteRegistryError>;
    pub fn update_last_seen(&mut self, site_id: u64, timestamp_us: u64) -> Result<(), SiteRegistryError>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn sites(&self) -> impl Iterator<Item = &SiteRecord>;
}
```

#### Conflict Resolver
```rust
// NOTE: This module defines its own SiteId(pub u64) — different from site_registry::SiteId
pub struct SiteId(pub u64);

pub enum ConflictType { LwwResolved, ManualResolutionRequired, SplitBrain }

pub struct ConflictRecord {
    pub conflict_id: u64,
    pub inode: u64,
    pub site_a: SiteId,
    pub site_b: SiteId,
    pub seq_a: u64,
    pub seq_b: u64,
    pub ts_a: u64,
    pub ts_b: u64,
    pub winner: SiteId,
    pub conflict_type: ConflictType,
    pub resolved_at: u64,
}

pub struct ConflictResolver { ... }
impl ConflictResolver {
    pub fn new() -> Self;
    pub fn resolve(&mut self, inode: u64, site_a: SiteId, seq_a: u64, ts_a: u64, site_b: SiteId, seq_b: u64, ts_b: u64) -> ConflictRecord;
    pub fn alert_needed(record: &ConflictRecord) -> bool;
    pub fn conflicts_for_inode(&self, inode: u64) -> Vec<&ConflictRecord>;
    pub fn conflict_count(&self) -> usize;
    pub fn split_brain_count(&self) -> usize;
}
```

#### TLS Policy
```rust
pub enum TlsMode { Required, TestOnly, Disabled }
pub enum TlsPolicyError {
    PlaintextNotAllowed,
    InvalidCertificate { reason: String },
    ModeConflict { msg: String },
}

pub struct TlsConfigRef { pub cert_pem: Vec<u8>, pub key_pem: Vec<u8>, pub ca_pem: Vec<u8> }

pub struct TlsValidator { ... }
impl TlsValidator {
    pub fn new(mode: TlsMode) -> Self;
    pub fn mode(&self) -> &TlsMode;
    pub fn is_plaintext_allowed(&self) -> bool;
    pub fn validate_config(&self, tls: &Option<TlsConfigRef>) -> Result<(), TlsPolicyError>;
}

pub struct TlsPolicyBuilder { ... }
impl TlsPolicyBuilder {
    pub fn new() -> Self;
    pub fn mode(mut self, mode: TlsMode) -> Self;
    pub fn build(self) -> TlsValidator;
}

pub fn validate_tls_config(cert_pem: &[u8], key_pem: &[u8], ca_pem: &[u8]) -> Result<(), TlsPolicyError>;
```

#### Batch Auth
```rust
pub struct BatchAuthKey { ... } // derives Zeroize, ZeroizeOnDrop
impl BatchAuthKey {
    pub fn generate() -> Self;
    pub fn from_bytes(bytes: [u8; 32]) -> Self;
    pub fn as_bytes(&self) -> &[u8; 32];
}

pub struct BatchTag { pub bytes: [u8; 32] }
impl BatchTag {
    pub fn new(bytes: [u8; 32]) -> Self;
    pub fn zero() -> Self;
}

pub enum AuthResult { Valid, Invalid { reason: String } }

pub struct BatchAuthenticator { ... }
impl BatchAuthenticator {
    pub fn new(key: BatchAuthKey, local_site_id: u64) -> Self;
    pub fn local_site_id(&self) -> u64;
    pub fn sign_batch(&self, source_site_id: u64, batch_seq: u64, entries: &[JournalEntry]) -> BatchTag;
    pub fn verify_batch(&self, tag: &BatchTag, source_site_id: u64, batch_seq: u64, entries: &[JournalEntry]) -> AuthResult;
}
```

#### Auth Rate Limiter
```rust
pub enum RateLimitResult {
    Allowed,
    Throttled { wait_ms: u64 },
    Blocked { reason: String, until_us: u64 },
}

pub struct RateLimitConfig {
    pub max_auth_attempts_per_minute: u32,
    pub max_batches_per_second: u32,
    pub max_global_bytes_per_second: u64,
    pub lockout_duration_secs: u64,
}
impl Default for RateLimitConfig { ... }

pub struct AuthRateLimiter { ... }
impl AuthRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self;
    pub fn check_auth_attempt(&mut self, site_id: u64, now_us: u64) -> RateLimitResult;
    pub fn check_batch_send(&mut self, site_id: u64, byte_count: u64, now_us: u64) -> RateLimitResult;
    pub fn reset_site(&mut self, site_id: u64);
    pub fn auth_attempt_count(&self, site_id: u64, now_us: u64) -> u32;
    pub fn is_locked_out(&self, site_id: u64, now_us: u64) -> bool;
}
```

#### Failover
```rust
pub enum SiteMode { ActiveReadWrite, StandbyReadOnly, DegradedAcceptWrites, Offline }
pub enum FailoverEvent {
    SitePromoted { site_id: u64, new_mode: SiteMode },
    SiteDemoted { site_id: u64, new_mode: SiteMode, reason: String },
    SiteRecovered { site_id: u64 },
    ConflictRequiresResolution { site_id: u64, inode: u64 },
}

pub struct FailoverConfig {
    pub failure_threshold: u32,
    pub recovery_threshold: u32,
    pub check_interval_ms: u64,
    pub active_active: bool,
}
impl Default for FailoverConfig { ... }

pub struct SiteFailoverState {
    pub site_id: u64,
    pub mode: SiteMode,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub last_check_us: u64,
    pub failover_count: u64,
}
impl SiteFailoverState {
    pub fn new(site_id: u64) -> Self;
    pub fn is_writable(&self) -> bool;
    pub fn is_readable(&self) -> bool;
}

pub struct FailoverManager { ... }
impl FailoverManager {
    pub fn new(config: FailoverConfig, local_site_id: u64) -> Self;
    pub async fn register_site(&self, site_id: u64);
    pub async fn record_health(&self, site_id: u64, healthy: bool) -> Vec<FailoverEvent>;
    pub async fn site_mode(&self, site_id: u64) -> Option<SiteMode>;
    pub async fn writable_sites(&self) -> Vec<u64>;
    pub async fn readable_sites(&self) -> Vec<u64>;
    pub async fn force_mode(&self, site_id: u64, mode: SiteMode) -> Result<(), ReplError>;
    pub async fn drain_events(&self) -> Vec<FailoverEvent>;
    pub async fn all_states(&self) -> Vec<SiteFailoverState>;
    pub async fn failover_counts(&self) -> HashMap<u64, u64>;
}
```

#### WAL
```rust
pub struct ReplicationCursor { pub site_id: u64, pub shard_id: u32, pub last_seq: u64 }
impl ReplicationCursor {
    pub fn new(site_id: u64, shard_id: u32, last_seq: u64) -> Self;
}

pub struct WalRecord { pub cursor: ReplicationCursor, pub replicated_at_us: u64, pub entry_count: u32 }

pub struct ReplicationWal { ... }
impl ReplicationWal {
    pub fn new() -> Self;
    pub fn advance(&mut self, site_id: u64, shard_id: u32, seq: u64, replicated_at_us: u64, entry_count: u32);
    pub fn cursor(&self, site_id: u64, shard_id: u32) -> ReplicationCursor;
    pub fn all_cursors(&self) -> Vec<ReplicationCursor>;
    pub fn reset(&mut self, site_id: u64, shard_id: u32);
    pub fn history(&self) -> &[WalRecord];
    pub fn compact(&mut self, before_us: u64);
}
```

#### Split Brain
```rust
pub struct FencingToken(pub u64);
impl FencingToken {
    pub fn new(v: u64) -> Self;
    pub fn next(&self) -> Self;
    pub fn value(&self) -> u64;
}

pub enum SplitBrainState {
    Normal,
    PartitionSuspected { since_ns: u64, site_id: u64 },
    Confirmed { site_a: u64, site_b: u64, diverged_at_seq: u64 },
    Resolving { fenced_site: u64, active_site: u64, fence_token: FencingToken },
    Healed { at_ns: u64 },
}

pub struct SplitBrainEvidence { pub site_a_last_seq: u64, pub site_b_last_seq: u64, pub site_a_diverge_seq: u64, pub detected_at_ns: u64 }
pub struct SplitBrainStats { pub partitions_detected: u64, pub split_brains_confirmed: u64, pub resolutions_completed: u64, pub fencing_tokens_issued: u64 }
```

#### Site Failover (Active-Active)
```rust
// NOTE: This module defines its own SiteId(pub u64) — different from site_registry::SiteId
pub struct SiteId(pub u64);

pub enum FailoverState { Normal, Degraded { failed_site: SiteId }, Failover { primary: SiteId, standby: SiteId }, Recovery { recovering_site: SiteId }, SplitBrain }
pub enum FailoverEvent {
    SiteDown { site_id: SiteId, detected_at_ns: u64 },
    SiteUp { site_id: SiteId, detected_at_ns: u64 },
    ReplicationLagHigh { site_id: SiteId, lag_ns: u64 },
    ManualFailover { target_primary: SiteId },
    RecoveryComplete { site_id: SiteId },
}
pub struct FailoverStats { pub state_transitions: u64, pub failover_count: u64, pub recovery_count: u64, pub split_brain_count: u64 }

pub struct FailoverController { ... }
impl FailoverController {
    pub fn new(site_a: SiteId, site_b: SiteId) -> Self;
    pub fn process_event(&mut self, event: FailoverEvent) -> FailoverState;
    pub fn state(&self) -> &FailoverState;
    pub fn stats(&self) -> &FailoverStats;
    pub fn is_degraded(&self) -> bool;
    pub fn primary_site(&self) -> SiteId;
}
```

#### Error
```rust
pub enum ReplError {
    Journal { msg: String },
    WalCorrupted { msg: String },
    SiteUnknown { site_id: u64 },
    ConflictDetected { inode: u64, local_ts: u64, remote_ts: u64 },
    NetworkError { msg: String },
    Serialization(bincode::Error),
    Io(std::io::Error),
    VersionMismatch { expected: u32, got: u32 },
    Shutdown,
    Compression(String),
}
```

## reduce_security_tests.rs — Required Tests (20 tests)

### Category 1: GC Safety (5 tests)

1. **test_gc_sweep_with_incomplete_mark** — Mark only some chunks as reachable, sweep CAS index. Verify that unmarked-but-live chunks are reclaimed. FINDING-REDUCE-01: GC can delete live data if mark phase is incomplete.

2. **test_gc_clear_marks_then_sweep** — Mark reachable, then call clear_marks(), then sweep. Verify all chunks reclaimed. Document that clear_marks + sweep deletes everything. FINDING-REDUCE-02: Double-clear mark phase danger.

3. **test_gc_concurrent_insert_during_sweep** — Insert a new hash into CAS after marking but before sweep. Verify the new hash survives or is deleted. FINDING-REDUCE-03: TOCTOU in mark-sweep.

4. **test_cas_refcount_underflow** — Insert hash, release it, release it again. Document behavior — should not underflow or panic. FINDING-REDUCE-04: Refcount underflow.

5. **test_gc_stats_accuracy** — Run a GC cycle and verify GcStats accurately reflects what happened. Correctness test.

### Category 2: Key Management Security (6 tests)

6. **test_key_manager_no_key_generate_dek** — Create KeyManager without initial key, try generate_dek(). Should fail with MissingKey. Correctness.

7. **test_key_manager_wrap_unwrap_roundtrip** — Generate DEK, wrap it, unwrap it. Verify plaintext matches. Correctness.

8. **test_key_manager_unwrap_after_clear_history** — Wrap DEK, rotate key, clear history. Try to unwrap the old wrapped key. Document if it fails. FINDING-REDUCE-05: Key loss on history clear.

9. **test_key_rotation_scheduler_rewrap_without_schedule** — Call rewrap_next without scheduling a rotation first. Document behavior. Correctness.

10. **test_key_rotation_scheduler_double_schedule** — Schedule rotation twice in a row. Document behavior — should fail or overwrite? FINDING-REDUCE-06: Double schedule race.

11. **test_wrapped_key_tampered_ciphertext** — Wrap a DEK, tamper with ciphertext bytes, try to unwrap. Should fail with DecryptionAuthFailed. Correctness — authenticated encryption integrity.

### Category 3: Encryption Security (4 tests)

12. **test_nonce_uniqueness** — Generate 100 nonces with random_nonce(). Verify no duplicates. FINDING-REDUCE-07: Nonce reuse probability.

13. **test_encrypt_empty_plaintext** — Encrypt an empty byte slice. Should succeed (valid operation). Correctness.

14. **test_decrypt_wrong_key** — Encrypt with key A, decrypt with key B. Should fail with DecryptionAuthFailed. Correctness.

15. **test_derive_chunk_key_deterministic** — Derive key from same master_key + chunk_hash twice. Should produce identical keys. Correctness.

### Category 4: Checksum and Segment Security (5 tests)

16. **test_checksum_tampered_data** — Compute checksum, modify one byte of data, verify. Should fail with ChecksumMismatch. Correctness.

17. **test_segment_integrity_tampered_payload** — Create a segment with checksummed payload, modify one byte in payload, call verify_integrity(). Should fail. FINDING-REDUCE-08: Segment tamper detection.

18. **test_snapshot_max_limit** — Create more snapshots than max_snapshots allows. Document behavior (rejection or oldest deleted?). FINDING-REDUCE-09: Snapshot limit enforcement.

19. **test_snapshot_clone_nonexistent** — Clone a snapshot that doesn't exist. Should return error. Correctness.

20. **test_compression_decompression_roundtrip_all_algos** — Compress then decompress with each algorithm (Lz4, Zstd). Verify data matches. Correctness.

## repl_security_tests.rs — Required Tests (20 tests)

### Category 1: Journal Integrity (5 tests)

1. **test_journal_crc_validation** — Create a JournalEntry, validate CRC. Then modify payload, verify CRC is now invalid. Correctness.

2. **test_journal_crc_weak_collision** — Create two entries differing only in site_id. Check if they have different CRCs. FINDING-REPL-01: CRC32 may be insufficient for tampering detection.

3. **test_journal_entry_empty_payload** — Create entry with empty payload vec. Verify CRC still works. Edge case.

4. **test_journal_tailer_position_tracking** — Create tailer, consume entries, verify position() tracks correctly. Correctness.

5. **test_journal_filter_by_nonexistent_shard** — Filter by shard_id that has no entries. Should return empty, not panic. Edge case.

### Category 2: Batch Authentication (5 tests)

6. **test_batch_auth_sign_verify_roundtrip** — Sign a batch, verify it. Should succeed. Correctness.

7. **test_batch_auth_tampered_entry** — Sign a batch, modify one entry, verify. Should return Invalid. Correctness.

8. **test_batch_auth_replay_different_seq** — Sign with batch_seq=1, verify with batch_seq=2. Should fail. FINDING-REPL-02: Replay protection via batch_seq.

9. **test_batch_auth_wrong_key** — Sign with key A, verify with key B. Should return Invalid. Correctness.

10. **test_batch_auth_zero_tag** — Create a BatchTag::zero(), try to verify it against a valid batch. Should return Invalid. FINDING-REPL-03: Zero tag rejection.

### Category 3: Site Identity and TLS (5 tests)

11. **test_site_registry_fingerprint_mismatch** — Register a site with fingerprint, then verify_source_id with different fingerprint. Should fail. Correctness.

12. **test_site_registry_no_fingerprint_bypass** — Register a site with a fingerprint, then verify_source_id with fingerprint=None. Document if this bypasses fingerprint check. FINDING-REPL-04: Optional fingerprint bypass.

13. **test_tls_required_rejects_plaintext** — Create TlsValidator with Required mode, validate with tls=None. Should fail with PlaintextNotAllowed. Correctness.

14. **test_tls_testonly_allows_plaintext** — Create TlsValidator with TestOnly mode, validate with tls=None. Should succeed. Document finding. FINDING-REPL-05: TestOnly allows plaintext.

15. **test_tls_validate_empty_certs** — Validate TLS config with empty cert/key/ca bytes. Document behavior. FINDING-REPL-06: Empty cert validation.

### Category 4: Conflict Resolution and Failover (5 tests)

16. **test_conflict_lww_resolution** — Create two conflicting writes with different timestamps. Verify LWW picks the later one. Correctness.

17. **test_conflict_equal_timestamps** — Two writes with identical timestamps from different sites. Document which wins. FINDING-REPL-07: Tie-breaking policy (site_id as tiebreaker?).

18. **test_fencing_token_monotonic** — Generate multiple fencing tokens via next(). Verify they are strictly increasing. Correctness.

19. **test_wal_reset_loses_cursor** — Advance WAL cursor, then reset. Verify cursor goes back to 0. Correctness — intentional reset vs. accidental data loss.

20. **test_rate_limiter_lockout** — Exceed max_auth_attempts_per_minute. Verify subsequent attempts are Blocked. FINDING-REPL-08: Rate limiting enforcement.

## Output Format

Generate THREE files:

### File 1: Updated `lib.rs`
Full content of the updated `crates/claudefs-security/src/lib.rs` with the two new module declarations added at the end.

### File 2: `reduce_security_tests.rs`
Full content of `crates/claudefs-security/src/reduce_security_tests.rs`.

### File 3: `repl_security_tests.rs`
Full content of `crates/claudefs-security/src/repl_security_tests.rs`.

Mark each file clearly with `// FILE: filename.rs` at the top.
