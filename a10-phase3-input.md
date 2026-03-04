# A10 Phase 3: Security Tests for Metadata and Gateway Crates

## Task

Add two new test modules to the `claudefs-security` crate:
1. `meta_security_tests.rs` — Security tests for the `claudefs-meta` crate
2. `gateway_security_tests.rs` — Security tests for the `claudefs-gateway` crate

Both files go in `crates/claudefs-security/src/`.

Also update `crates/claudefs-security/src/lib.rs` to register both new modules.

## Important Constraints

- All tests must compile and pass with `cargo test -p claudefs-security`
- Use only public APIs from `claudefs-meta` and `claudefs-gateway`
- Every test must have a comment explaining the security finding it validates
- Use `#[cfg(test)]` module wrapping
- No unsafe code
- Follow the existing pattern from `phase2_audit.rs` (which I will show below)

## Existing lib.rs (add new modules here)

```rust
// FILE: lib.rs
#![warn(missing_docs)]

//! ClaudeFS security audit crate: fuzzing harnesses, crypto property tests,
//! transport validation, and audit tooling.

pub mod audit;
#[cfg(test)]
pub mod api_security_tests;
#[cfg(test)]
pub mod api_pentest_tests;
#[cfg(test)]
pub mod conduit_auth_tests;
#[cfg(test)]
pub mod crypto_tests;
pub mod fuzz_message;
pub mod fuzz_protocol;
#[cfg(test)]
pub mod gateway_auth_tests;
#[cfg(test)]
pub mod transport_tests;
#[cfg(test)]
pub mod unsafe_review_tests;
#[cfg(test)]
pub mod unsafe_audit;
#[cfg(test)]
pub mod crypto_audit;
#[cfg(test)]
pub mod crypto_zeroize_audit;
#[cfg(test)]
pub mod mgmt_pentest;
#[cfg(test)]
pub mod fuzz_fuse;
#[cfg(test)]
pub mod dep_audit;
#[cfg(test)]
pub mod dos_resilience;
#[cfg(test)]
pub mod supply_chain;
#[cfg(test)]
pub mod operational_security;
#[cfg(test)]
pub mod advanced_fuzzing;
#[cfg(test)]
pub mod phase2_audit;
```

Add at the end (before the closing):
```rust
#[cfg(test)]
pub mod meta_security_tests;
#[cfg(test)]
pub mod gateway_security_tests;
```

## Dependencies available in Cargo.toml

```toml
[dependencies]
claudefs-storage = { path = "../claudefs-storage", features = ["uring"] }
claudefs-transport = { path = "../claudefs-transport" }
claudefs-reduce = { path = "../claudefs-reduce" }
claudefs-repl = { path = "../claudefs-repl" }
claudefs-gateway = { path = "../claudefs-gateway" }
claudefs-mgmt = { path = "../claudefs-mgmt" }
claudefs-fuse = { path = "../claudefs-fuse" }
thiserror.workspace = true
serde.workspace = true
tracing.workspace = true
rand = "0.8"

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
proptest = "1.4"
axum = { version = "0.7", features = ["macros", "json"] }
tower = { version = "0.4", features = ["util", "timeout", "limit"] }
hyper = { version = "1.0", features = ["full"] }
serde_json = "1.0"
urlencoding = "2.1"
bincode.workspace = true
sha2.workspace = true
hkdf.workspace = true
blake3.workspace = true
zeroize = { version = "1.7", features = ["derive"] }
```

NOTE: Also add `claudefs-meta = { path = "../claudefs-meta" }` to the `[dependencies]` section in `Cargo.toml`.

## Key types from claudefs-meta (public API)

```rust
// types.rs
pub struct InodeId(u64);  // InodeId::new(u64), InodeId::ROOT_INODE, as_u64(), shard()
pub struct NodeId(u64);   // NodeId::new(u64), as_u64()
pub struct ShardId(u16);  // ShardId::new(u16), as_u16()
pub struct Term(u64);     // Term::new(u64)

// From lib.rs re-exports:
pub use kvstore::{KvStore, MemoryKvStore};
pub use locking::{LockManager, LockType};
pub use service::{MetadataService, MetadataServiceConfig};
pub use inode::{InodeStore};  // Uses KvStore
pub use directory::DirectoryStore;
pub use pathres::{PathResolver, PathCacheEntry};
pub use consensus::RaftNode;
pub use cdc::{CdcStream, CdcEvent, CdcCursor};
pub use watch::{WatchManager, WatchEvent};
pub use worm::{WormManager, RetentionPolicy, WormState};
pub use transaction::{TransactionManager, Transaction, TransactionState};
pub use cross_shard::{CrossShardCoordinator, CrossShardResult};
pub use tenant::{TenantManager, TenantConfig, TenantId};
pub use quota::{QuotaManager, QuotaEntry, QuotaLimit, QuotaTarget, QuotaUsage};
pub use membership::{MembershipManager, MemberInfo, NodeState};

// MetaError variants (from types.rs):
pub enum MetaError {
    NotFound,
    AlreadyExists,
    NotDirectory,
    IsDirectory,
    NotEmpty,
    PermissionDenied,
    KvError(String),
    StaleData,
    InvalidArgument(String),
    StorageFull,
    Corrupted(String),
    LockConflict,
    QuotaExceeded,
    WormViolation(String),
    TenantNotFound,
    NotLeader,
    InvalidState(String),
    IoError(String),
    SerializationError(String),
}

// FileType enum:
pub enum FileType { Regular, Directory, Symlink, BlockDevice, CharDevice, Fifo, Socket }

// InodeAttr:
pub struct InodeAttr {
    pub ino: InodeId,
    pub file_type: FileType,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub nlink: u32,
    pub atime: Timestamp,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    pub symlink_target: Option<String>,
    pub site_id: u64,
    pub gen: u64,
    pub blocks: u64,
    pub xattr_count: u32,
}

// LockManager:
impl LockManager {
    pub fn new() -> Self;
    pub fn acquire(&self, ino: InodeId, lock_type: LockType, holder: NodeId) -> Result<u64, MetaError>;
    pub fn release(&self, lock_id: u64) -> Result<(), MetaError>;
    pub fn list_locks(&self) -> Vec<LockEntry>;
}

// MetadataService:
impl MetadataService {
    pub fn new(kv: Arc<dyn KvStore>, config: MetadataServiceConfig) -> Self;
    pub fn create_file(&self, parent: InodeId, name: &str, mode: u32, uid: u32, gid: u32) -> Result<InodeAttr, MetaError>;
    pub fn create_dir(&self, parent: InodeId, name: &str, mode: u32, uid: u32, gid: u32) -> Result<InodeAttr, MetaError>;
    pub fn create_symlink(&self, parent: InodeId, name: &str, target: &str, mode: u32, uid: u32, gid: u32) -> Result<InodeAttr, MetaError>;
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeAttr, MetaError>;
    pub fn unlink(&self, parent: InodeId, name: &str) -> Result<(), MetaError>;
    pub fn rename(&self, src_parent: InodeId, src_name: &str, dst_parent: InodeId, dst_name: &str) -> Result<(), MetaError>;
    pub fn readdir(&self, ino: InodeId) -> Result<Vec<DirEntry>, MetaError>;
    pub fn getattr(&self, ino: InodeId) -> Result<InodeAttr, MetaError>;
    pub fn setattr(&self, ino: InodeId, attr: InodeAttr) -> Result<InodeAttr, MetaError>;
}

// WormManager:
impl WormManager {
    pub fn new() -> Self;
    pub fn register(&self, ino: InodeId, policy: RetentionPolicy, size: u64) -> Result<(), MetaError>;
    pub fn check_modification(&self, ino: InodeId, current_time: u64) -> Result<(), MetaError>;
    pub fn check_deletion(&self, ino: InodeId, current_time: u64) -> Result<(), MetaError>;
}

// RetentionPolicy:
impl RetentionPolicy {
    pub fn immutable_until(timestamp: u64) -> Self;
}

// PathResolver:
impl PathResolver {
    pub fn new() -> Self;
    pub fn populate(&self, parent: InodeId, name: &str, ino: InodeId);
    pub fn speculative_resolve(&self, path: &str) -> (Vec<PathCacheEntry>, Vec<String>);
    pub fn invalidate(&self, parent: InodeId, name: &str);
}

// CdcStream:
impl CdcStream {
    pub fn new(max_events: usize) -> Self;
    pub fn publish(&self, event: CdcEvent);
    pub fn register_consumer(&self, consumer_id: &str);
    pub fn consume(&self, consumer_id: &str, max_count: usize) -> Vec<CdcEvent>;
}

// TransactionManager:
impl TransactionManager {
    pub fn new() -> Self;
    pub fn begin(&self) -> TransactionId;
    pub fn vote_commit(&self, txn_id: TransactionId, shard: ShardId) -> Result<(), MetaError>;
    pub fn abort(&self, txn_id: TransactionId) -> Result<(), MetaError>;
}
```

## Key types from claudefs-gateway (public API)

```rust
// S3Handler:
impl S3Handler {
    pub fn new() -> Self;
    pub fn create_bucket(&self, bucket: &str) -> Result<()>;
    pub fn list_buckets(&self) -> Result<ListBucketsResult>;
    pub fn delete_bucket(&self, bucket: &str) -> Result<()>;
    pub fn put_object(&self, bucket: &str, key: &str, data: Vec<u8>, content_type: &str) -> Result<ObjectMeta>;
    pub fn get_object(&self, bucket: &str, key: &str) -> Result<(ObjectMeta, Vec<u8>)>;
    pub fn delete_object(&self, bucket: &str, key: &str) -> Result<()>;
    pub fn list_objects(&self, bucket: &str, prefix: &str, delimiter: Option<&str>, max_keys: u32) -> Result<ListObjectsResult>;
    pub fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta>;
    pub fn copy_object(&self, src_bucket: &str, src_key: &str, dst_bucket: &str, dst_key: &str) -> Result<ObjectMeta>;
}

// PnfsLayoutServer:
impl PnfsLayoutServer {
    pub fn new(data_servers: Vec<DataServerLocation>, fsid: u64) -> Self;
    pub fn get_layout(&self, inode: u64, offset: u64, length: u64, iomode: IoMode) -> LayoutGetResult;
    pub fn server_count(&self) -> usize;
    pub fn add_server(&mut self, location: DataServerLocation);
    pub fn remove_server(&mut self, address: &str) -> bool;
}

// DataServerLocation:
pub struct DataServerLocation {
    pub address: String,
    pub device_id: [u8; 16],
}

// IoMode:
pub enum IoMode { Read = 1, ReadWrite = 2, Any = 3 }

// Auth types:
pub struct AuthSysCred { pub uid: u32, pub gid: u32, pub machinename: String, pub gids: Vec<u32> }
pub fn parse_auth_sys(data: &[u8]) -> Result<AuthSysCred>
pub fn apply_squash(cred: &AuthSysCred, policy: SquashPolicy) -> AuthSysCred

// Token auth:
pub struct TokenManager { ... }
impl TokenManager {
    pub fn new() -> Self;
    pub fn create_token(&self, permissions: Vec<String>) -> (String, String); // (token, hash)
    pub fn validate(&self, token_hash: &str) -> Option<Vec<String>>;
    pub fn revoke(&self, token_hash: &str) -> bool;
}

// GatewayError variants include: Nfs3NoEnt, Nfs3Io, Nfs3Acces, Nfs3Exist, Nfs3NotDir, Nfs3IsDir, Nfs3Inval, Nfs3Stale, S3BucketNotFound, S3InvalidBucketName, S3ObjectNotFound, ProtocolError, NotImplemented, BackendError, etc.
```

## meta_security_tests.rs — Required Tests (25+ tests)

Write a module `meta_security_tests` with `#[cfg(test)] mod tests { ... }`.

### Category 1: Input Validation (8 tests)

1. **test_symlink_target_max_length** — Create a symlink with a 4096+ byte target. The system should either reject it or handle it without crashing. Tests FINDING-META-01: No symlink target length validation.

2. **test_directory_entry_name_length** — Try to create a file with a name > 255 bytes. The system should reject it with InvalidArgument or handle correctly. Tests FINDING-META-02: No directory entry name length validation.

3. **test_directory_entry_special_names** — Try to create files named ".", "..", empty string, "\0", "/", "a/b". These should all be rejected. Tests FINDING-META-03: Special name handling.

4. **test_inode_id_zero** — Test that InodeId(0) doesn't cause panics in shard computation and other operations. Tests FINDING-META-04: Edge case inode IDs.

5. **test_inode_id_max** — Test that InodeId(u64::MAX) doesn't overflow in shard computation. Tests FINDING-META-04 cont.

6. **test_setattr_mode_high_bits** — Test setting mode with high bits set (e.g., 0o777777). Tests FINDING-META-05: Mode validation.

7. **test_node_id_boundary_values** — Test NodeId(0), NodeId(u64::MAX) in lock acquisition. Tests FINDING-META-06: Node ID edge cases.

8. **test_shard_id_computation_deterministic** — Verify that shard assignment is deterministic for the same inode. Tests correctness of D4 implementation.

### Category 2: Distributed Locking Security (6 tests)

9. **test_lock_starvation_readers** — Acquire many read locks on one inode, verify write lock is properly denied but won't deadlock. Tests FINDING-META-07: Lock starvation.

10. **test_lock_double_acquire** — Same node acquires write lock twice on same inode. Should fail, not deadlock. Tests FINDING-META-08: Double lock.

11. **test_lock_release_nonexistent** — Release a lock ID that was never acquired. Should return error, not panic. Tests FINDING-META-09: Invalid lock release.

12. **test_lock_id_overflow** — Acquire and release many locks to test lock_id counter near u64::MAX boundary. Tests FINDING-META-10: Counter overflow.

13. **test_concurrent_lock_stress** — Multiple threads concurrently acquire/release locks on overlapping inodes. Tests thread safety. Tests FINDING-META-11: Concurrent lock safety.

14. **test_write_lock_blocks_read** — After acquiring write lock, read lock should fail. Tests basic lock correctness. Tests correctness.

### Category 3: Metadata Service Security (6 tests)

15. **test_create_file_in_nonexistent_parent** — Create a file under a nonexistent parent inode. Should return NotFound. Tests FINDING-META-12: Orphan creation prevention.

16. **test_readdir_on_file_inode** — Call readdir on a regular file inode. Should return NotDirectory or similar error. Tests FINDING-META-13: Type confusion.

17. **test_unlink_nonexistent_file** — Unlink a file that doesn't exist. Should return NotFound. Tests error handling.

18. **test_rename_to_same_location** — Rename a file to itself. Should succeed without corruption. Tests idempotency.

19. **test_create_file_empty_name** — Create a file with empty name. Should fail with appropriate error. Tests input validation.

20. **test_worm_modification_before_retention** — Register a WORM entry and try to modify before retention period expires. Should fail with WormViolation. Tests compliance enforcement.

### Category 4: Cache and CDC Security (5 tests)

21. **test_path_cache_invalidation_on_remove** — Populate cache, invalidate, verify stale entries are gone. Tests FINDING-META-14: TOCTOU.

22. **test_cdc_consumer_isolation** — Two consumers on same stream get independent cursors. Tests FINDING-META-15: Consumer isolation.

23. **test_cdc_empty_consumer** — Consume from empty stream. Should return empty vec, not panic. Tests edge case.

24. **test_path_resolver_empty_path** — Resolve empty path. Should handle gracefully. Tests FINDING-META-16: Empty path handling.

25. **test_path_resolver_deeply_nested** — Resolve path with 100+ components. Tests FINDING-META-17: Stack depth.

## gateway_security_tests.rs — Required Tests (25+ tests)

Write a module `gateway_security_tests` with `#[cfg(test)] mod tests { ... }`.

### Category 1: S3 API Security (10 tests)

1. **test_s3_bucket_name_with_dots** — Bucket name like "my..bucket" should be rejected. Tests DNS label validation.

2. **test_s3_bucket_name_ip_format** — Bucket name "192.168.1.1" should be rejected (AWS S3 rejects IP-like names). Tests AWS compatibility.

3. **test_s3_object_key_path_traversal** — Object key "../../etc/passwd" should either be rejected or safely stored. Tests FINDING-GW-01: Path traversal.

4. **test_s3_object_key_null_bytes** — Object key containing "\0" should be rejected. Tests FINDING-GW-02: Null byte injection.

5. **test_s3_object_key_max_length** — Object key with 1025+ bytes should be rejected (AWS limit is 1024). Tests FINDING-GW-03: Key length validation.

6. **test_s3_list_objects_max_keys_zero** — ListObjects with max_keys=0 should return empty or error. Tests FINDING-GW-04: Edge case.

7. **test_s3_list_objects_max_keys_overflow** — ListObjects with max_keys=u32::MAX. Should not cause OOM. Tests FINDING-GW-05: Integer overflow.

8. **test_s3_delete_nonexistent_bucket** — Delete a bucket that doesn't exist. Should return BucketNotFound. Tests error handling.

9. **test_s3_put_object_empty_body** — Put an object with empty data. Should succeed (empty objects are valid in S3). Tests correctness.

10. **test_s3_copy_to_nonexistent_bucket** — Copy object to a bucket that doesn't exist. Should fail properly. Tests FINDING-GW-06: Cross-bucket validation.

### Category 2: S3 Bucket Validation (5 tests)

11. **test_s3_bucket_name_too_short** — 2-char bucket name. Should be rejected. Tests length validation.

12. **test_s3_bucket_name_too_long** — 64-char bucket name. Should be rejected. Tests length validation.

13. **test_s3_bucket_name_special_chars** — Bucket names with @, #, $, %, spaces. All should be rejected. Tests character validation.

14. **test_s3_bucket_name_leading_hyphen** — Bucket "-my-bucket". Should be rejected. Tests format validation.

15. **test_s3_bucket_name_valid_examples** — Verify that "my-bucket", "test123", "a-b-c" all succeed. Tests positive cases.

### Category 3: pNFS Layout Security (5 tests)

16. **test_pnfs_stateid_is_inode_based** — Verify stateid contains the inode number (identifies the predictability issue). Tests FINDING-GW-07: Predictable stateids.

17. **test_pnfs_server_selection_modulo** — Verify server selection is simple modulo (identifies the predictability issue). Tests FINDING-GW-08: Predictable server selection.

18. **test_pnfs_empty_server_list** — Get layout with no data servers. Should return empty segments. Tests edge case.

19. **test_pnfs_layout_iomode_validation** — Verify IoMode::from_u32 rejects invalid values (0, 4, u32::MAX). Tests input validation.

20. **test_pnfs_large_inode_no_overflow** — Get layout for inode u64::MAX with 3 servers. Should not overflow or panic. Tests FINDING-GW-09: Integer overflow.

### Category 4: NFS Authentication Security (5 tests)

21. **test_auth_sys_root_squash** — Verify that UID 0 is mapped to 65534 under RootSquash policy. Tests security policy enforcement.

22. **test_auth_sys_all_squash** — Verify that all UIDs are mapped to 65534 under AllSquash. Tests policy enforcement.

23. **test_auth_sys_oversized_machinename** — AUTH_SYS with 256+ byte machine name. Should be rejected. Tests FINDING-GW-10: Input validation.

24. **test_auth_sys_too_many_gids** — AUTH_SYS with 17+ GIDs. Should be rejected. Tests FINDING-GW-11: GID count validation.

25. **test_auth_sys_truncated_payload** — Incomplete AUTH_SYS XDR data (too few bytes). Should return error, not crash. Tests FINDING-GW-12: Truncation handling.

### Category 5: Token Auth Security (3 tests)

26. **test_token_revocation_prevents_access** — Create token, revoke it, verify validation returns None. Tests token lifecycle.

27. **test_token_validate_unknown** — Validate a completely unknown token hash. Should return None. Tests default-deny.

28. **test_token_permissions_preserved** — Create token with specific permissions, validate, verify permissions match. Tests permission integrity.

## Output Format

Generate THREE files:

### File 1: Updated `lib.rs`
Full content of the updated `crates/claudefs-security/src/lib.rs` with the two new module declarations added.

### File 2: `meta_security_tests.rs`
Full content of `crates/claudefs-security/src/meta_security_tests.rs`.

### File 3: `gateway_security_tests.rs`
Full content of `crates/claudefs-security/src/gateway_security_tests.rs`.

### File 4: Updated `Cargo.toml`
Full content of `crates/claudefs-security/Cargo.toml` with `claudefs-meta` added to dependencies.

Mark each file clearly with `// FILE: filename.rs` at the top.
