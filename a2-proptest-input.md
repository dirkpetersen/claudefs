# Task: Add proptest-based property tests to claudefs-meta

You are working on the `claudefs-meta` crate in a Cargo workspace.
The crate is at `/home/cfs/claudefs/crates/claudefs-meta/`.

## What to do

### 1. Update `crates/claudefs-meta/Cargo.toml`

Add `proptest = "1"` to the `[dev-dependencies]` section.

Current Cargo.toml content:
```toml
[package]
name = "claudefs-meta"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "ClaudeFS subsystem: Distributed metadata, Raft consensus, inode/directory operations"

[[bin]]
name = "cfs-meta"
path = "src/main.rs"

[dependencies]
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
bincode.workspace = true
prost.workspace = true
tonic.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true

[lib]
name = "claudefs_meta"
path = "src/lib.rs"

[dev-dependencies]
tempfile = "3"
```

Replace the `[dev-dependencies]` section with:
```toml
[dev-dependencies]
tempfile = "3"
proptest = "1"
```

### 2. Create `crates/claudefs-meta/src/proptests.rs`

Create this new file with comprehensive property-based tests using proptest.

Here is the full context you need:

**Key types from `src/types.rs`:**
```rust
// InodeId: newtype over u64
pub struct InodeId(u64);
impl InodeId {
    pub const ROOT_INODE: InodeId = InodeId(1);
    pub fn new(id: u64) -> Self;
    pub fn as_u64(&self) -> u64;
    pub fn shard(self, num_shards: u16) -> ShardId;
}

// ShardId: newtype over u16
pub struct ShardId(u16);
impl ShardId {
    pub fn new(id: u16) -> Self;
    pub fn as_u16(&self) -> u16;
}

// NodeId: newtype over u64
pub struct NodeId(u64);

// Term: newtype over u64
pub struct Term(u64);
impl Term {
    pub fn new(t: u64) -> Self;
    pub fn as_u64(&self) -> u64;
}

// LogIndex: newtype over u64
pub struct LogIndex(u64);
impl LogIndex {
    pub const ZERO: LogIndex = LogIndex(0);
    pub fn new(i: u64) -> Self;
    pub fn as_u64(&self) -> u64;
}

// Timestamp: secs/nanos, implements Ord
pub struct Timestamp { pub secs: u64, pub nanos: u32 }
impl Timestamp {
    pub fn now() -> Self;
}

// VectorClock: site_id + sequence, Ord by sequence then site_id
pub struct VectorClock { pub site_id: u64, pub sequence: u64 }
impl VectorClock {
    pub fn new(site_id: u64, sequence: u64) -> Self;
}

// FileType: RegularFile, Directory, Symlink, BlockDevice, CharDevice, Fifo, Socket
pub enum FileType { ... }
impl FileType {
    pub fn mode_bits(&self) -> u32;
}

// InodeAttr: full POSIX inode fields + ClaudeFS extensions
pub struct InodeAttr {
    pub ino: InodeId,
    pub file_type: FileType,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub blocks: u64,
    pub atime: Timestamp,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    pub crtime: Timestamp,
    pub content_hash: Option<[u8; 32]>,
    pub repl_state: ReplicationState,
    pub vector_clock: VectorClock,
    pub generation: u64,
    pub symlink_target: Option<String>,
}
impl InodeAttr {
    pub fn new_file(ino: InodeId, uid: u32, gid: u32, mode: u32, site_id: u64) -> Self;
    pub fn new_directory(ino: InodeId, uid: u32, gid: u32, mode: u32, site_id: u64) -> Self;
    pub fn new_symlink(ino: InodeId, uid: u32, gid: u32, mode: u32, site_id: u64, target: String) -> Self;
}

// DirEntry: name + ino + file_type
pub struct DirEntry { pub name: String, pub ino: InodeId, pub file_type: FileType }

// MetaOp: CreateInode, DeleteInode, SetAttr, CreateEntry, DeleteEntry, Rename, SetXattr, RemoveXattr, Link
pub enum MetaOp { ... }
```

**From `src/inode.rs` (InodeStore):**
```rust
pub struct InodeStore {
    kv: Arc<dyn KvStore>,
    next_inode: AtomicU64,
}
impl InodeStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self;
    pub fn allocate_inode(&self) -> InodeId;
    pub fn create_inode(&self, attr: &InodeAttr) -> Result<(), MetaError>;
    pub fn get_inode(&self, ino: InodeId) -> Result<InodeAttr, MetaError>;
    pub fn set_inode(&self, attr: &InodeAttr) -> Result<(), MetaError>;
    pub fn delete_inode(&self, ino: InodeId) -> Result<(), MetaError>;
    pub fn exists(&self, ino: InodeId) -> Result<bool, MetaError>;
}
```

**From `src/kvstore.rs`:**
```rust
pub trait KvStore: Send + Sync { ... }
pub struct MemoryKvStore { ... }
impl MemoryKvStore {
    pub fn new() -> Self;
}
```

**From `src/service.rs`:**
```rust
pub struct MetadataService { ... }
impl MetadataService {
    pub fn new(config: MetadataServiceConfig) -> Self;
    pub fn init_root(&self) -> Result<(), MetaError>;
    pub fn create_file(&self, parent: InodeId, name: &str, uid: u32, gid: u32, mode: u32) -> Result<InodeAttr, MetaError>;
    pub fn mkdir(&self, parent: InodeId, name: &str, uid: u32, gid: u32, mode: u32) -> Result<InodeAttr, MetaError>;
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeAttr, MetaError>;
    pub fn getattr(&self, ino: InodeId) -> Result<InodeAttr, MetaError>;
    pub fn setattr(&self, ino: InodeId, attr: InodeAttr) -> Result<(), MetaError>;
    pub fn readdir(&self, parent: InodeId) -> Result<Vec<DirEntry>, MetaError>;
    pub fn unlink(&self, parent: InodeId, name: &str) -> Result<(), MetaError>;
    pub fn rmdir(&self, parent: InodeId, name: &str) -> Result<(), MetaError>;
    pub fn rename(&self, src_parent: InodeId, src_name: &str, dst_parent: InodeId, dst_name: &str) -> Result<(), MetaError>;
    pub fn symlink(&self, parent: InodeId, name: &str, target: &str, uid: u32, gid: u32) -> Result<InodeAttr, MetaError>;
    pub fn link(&self, parent: InodeId, name: &str, ino: InodeId) -> Result<InodeAttr, MetaError>;
    pub fn readlink(&self, ino: InodeId) -> Result<String, MetaError>;
    pub fn num_shards(&self) -> u16;
    pub fn shard_for_inode(&self, ino: InodeId) -> ShardId;
}

pub struct MetadataServiceConfig {
    pub node_id: NodeId,
    pub peers: Vec<NodeId>,
    pub site_id: u64,
    pub num_shards: u16,
    pub max_journal_entries: usize,
}
impl Default for MetadataServiceConfig { ... } // node_id=1, site_id=1, num_shards=256
```

**From `src/journal.rs`:**
```rust
pub struct MetadataJournal { ... }
impl MetadataJournal {
    pub fn new(site_id: u64, max_entries: usize) -> Self;
    pub fn append(&self, op: MetaOp, log_index: LogIndex) -> Result<u64, MetaError>;
    pub fn read_from(&self, from_sequence: u64, limit: usize) -> Vec<JournalEntry>;
    pub fn latest_sequence(&self) -> Result<u64, MetaError>;
    pub fn compact_before(&self, sequence: u64) -> usize;
}
```

## Requirements for the test file

Write `crates/claudefs-meta/src/proptests.rs` with ALL of the following property-based tests using proptest. The file must be gated with `#[cfg(test)]` at the top level.

```rust
#![cfg(test)]

use proptest::prelude::*;
use std::sync::Arc;

use crate::{
    inode::InodeStore,
    kvstore::MemoryKvStore,
    service::{MetadataService, MetadataServiceConfig},
    types::*,
};
```

### Required tests:

1. **`prop_inode_shard_always_in_range`** — for any inode_id (u64 in 1..=u64::MAX) and num_shards (u16 in 1..=1024), verify that `InodeId::new(ino).shard(shards).as_u16() < shards`.

2. **`prop_inode_shard_deterministic`** — for any inode_id and num_shards, calling shard() twice gives the same result.

3. **`prop_inode_shard_uniform`** — for num_shards in 1..=16u16, for any batch of 1000 sequential inode IDs, each shard value produced is in [0, num_shards).

4. **`prop_inodeattr_bincode_roundtrip`** — for arbitrary (uid, gid, mode, ino_val in 2..=u64::MAX), create an InodeAttr::new_file and verify bincode::serialize -> bincode::deserialize gives equal result.

5. **`prop_direntry_bincode_roundtrip`** — for arbitrary (name as String, ino_val in 1..=u64::MAX), create a DirEntry with FileType::RegularFile and verify bincode roundtrip.

6. **`prop_metaop_rename_bincode_roundtrip`** — for arbitrary (src_parent, dst_parent, src_name, dst_name as Strings), create a MetaOp::Rename and verify bincode roundtrip preserves all fields.

7. **`prop_timestamp_ordering`** — for arbitrary (secs1: u64, nanos1: u32, secs2: u64, nanos2: u32), create two Timestamps and verify Ord is consistent with (secs, nanos) lexicographic order.

8. **`prop_vectorclock_ordering`** — for arbitrary (site1, seq1, site2, seq2: u64), create two VectorClocks and verify Ord is consistent with (sequence, site_id) lexicographic order.

9. **`prop_inode_store_create_get_roundtrip`** — for arbitrary (uid: u32, gid: u32, mode: u32), create a MemoryKvStore + InodeStore, allocate an inode, create an InodeAttr::new_file, store it, get it back, verify all fields match.

10. **`prop_inode_store_set_updates_fields`** — create an inode, then update size and mode via set_inode, verify get_inode returns updated values.

11. **`prop_inode_store_delete_removes`** — create an inode, delete it, verify exists() returns false and get_inode returns InodeNotFound.

12. **`prop_service_create_lookup_roundtrip`** — for a valid filename (ascii alphanumeric, 1..=64 chars), uid, gid, mode: create MetadataService, init_root, create_file in root, then lookup returns same ino and uid.

13. **`prop_service_readdir_count`** — create N files (1..=20) in root, verify readdir returns exactly N entries.

14. **`prop_service_shard_in_range`** — for any ino_val in 1..=10000u64, verify shard_for_inode(InodeId::new(ino_val)).as_u16() < svc.num_shards().

15. **`prop_logindex_ordering`** — for a: u64, b: u64, verify LogIndex::new(a).cmp(&LogIndex::new(b)) matches a.cmp(&b).

16. **`prop_term_ordering`** — for a: u64, b: u64, verify Term::new(a).cmp(&Term::new(b)) matches a.cmp(&b).

17. **`prop_filetype_mode_bits_nonzero`** — for all 7 FileType variants (enumerate them), verify mode_bits() != 0.

18. **`prop_service_unlink_removes_entry`** — create a file, unlink it, verify lookup returns EntryNotFound.

19. **`prop_journal_sequence_monotonic`** — append N ops (1..=50) to a MetadataJournal and verify returned sequence numbers are strictly increasing.

20. **`prop_service_rename_preserves_inode`** — create a file, rename it, verify ino is same after rename, old name is gone.

## Critical: file path and module registration

Output:
- The full content of `crates/claudefs-meta/src/proptests.rs`
- The updated `crates/claudefs-meta/Cargo.toml` with proptest added

## Important notes

- The file starts with `#![cfg(test)]` so it is only compiled in test mode
- Use `proptest!` macro for all property tests
- Use `use proptest::prelude::*;`
- For `prop_service_create_lookup_roundtrip`, generate filenames using `proptest::string::string_regex("[a-z][a-z0-9_]{0,62}").unwrap()` or a fixed strategy `"[a-z][a-z0-9]{0,20}"` using `prop::string::string_regex`
- For inode IDs in InodeStore tests, use values >= 2 (1 is reserved for ROOT_INODE)
- When testing MetadataService, always call `svc.init_root().unwrap()` first
- mode values should be in range 0o000..=0o777 (mask with 0o777 if needed)
- Keep tests fast: use small ranges for counts, small string lengths
- All tests must compile and pass with `cargo test`
- Do not use `#[test]` directly — use `proptest!` macro instead
- Wrap each test in `proptest! { #[test] fn name(...) { ... } }` syntax
- The `prop_service_create_lookup_roundtrip` test: since proptest generates arbitrary strings, use a regex strategy for valid filenames. The simplest approach: use `"[a-z][a-z0-9]{0,15}"` as the strategy.

## Also output: the exact lib.rs addition needed

Add this line to `src/lib.rs` (in the test configuration only, after the last `pub mod`):

```rust
#[cfg(test)]
mod proptests;
```

This must be added to the end of lib.rs, before the `pub use` declarations.

Please output:
1. Complete content of `crates/claudefs-meta/src/proptests.rs`
2. Updated `crates/claudefs-meta/Cargo.toml`
3. The exact edit to make in `src/lib.rs` to register the new module
