# Task: Create meta_directory_security_tests.rs for ClaudeFS Security Audit (A10 Phase 26)

## Context

You are writing a Rust security test module for the `claudefs-security` crate. This module audits the directory operations in `claudefs-meta`.

## Source Under Audit

The file `crates/claudefs-meta/src/directory.rs` contains:

```rust
use std::sync::Arc;
use crate::inode::InodeStore;
use crate::kvstore::KvStore;
use crate::types::{DirEntry, FileType, InodeId, MetaError};

const DIRENT_PREFIX: &[u8] = b"dirent/";

fn dirent_prefix(parent: InodeId) -> Vec<u8> {
    let mut key = DIRENT_PREFIX.to_vec();
    key.extend_from_slice(&parent.as_u64().to_be_bytes());
    key.push(b'/');
    key
}

fn dirent_key(parent: InodeId, name: &str) -> Vec<u8> {
    let mut key = dirent_prefix(parent);
    key.extend_from_slice(name.as_bytes());
    key
}

pub struct DirectoryStore {
    kv: Arc<dyn KvStore>,
    inodes: Arc<InodeStore>,
}

impl DirectoryStore {
    pub fn new(kv: Arc<dyn KvStore>, inodes: Arc<InodeStore>) -> Self {
        Self { kv, inodes }
    }

    /// Creates a directory entry. Validates parent is directory and entry doesn't exist.
    pub fn create_entry(&self, parent: InodeId, entry: &DirEntry) -> Result<(), MetaError> { ... }

    /// Deletes a directory entry.
    pub fn delete_entry(&self, parent: InodeId, name: &str) -> Result<DirEntry, MetaError> { ... }

    /// Looks up a directory entry by name.
    pub fn lookup(&self, parent: InodeId, name: &str) -> Result<DirEntry, MetaError> { ... }

    /// Lists all entries in a directory.
    pub fn list_entries(&self, parent: InodeId) -> Result<Vec<DirEntry>, MetaError> { ... }

    /// Returns true if directory has any entries.
    pub fn is_empty(&self, parent: InodeId) -> Result<bool, MetaError> { ... }

    /// Renames a directory entry. Supports cross-directory. Uses batch for atomicity.
    pub fn rename(&self, src_parent: InodeId, src_name: &str, dst_parent: InodeId, dst_name: &str) -> Result<(), MetaError> { ... }
}
```

Key types from `crates/claudefs-meta/src/types.rs`:
```rust
pub struct InodeId(u64);
impl InodeId {
    pub const ROOT_INODE: InodeId = InodeId(1);
    pub fn new(id: u64) -> Self { InodeId(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

pub enum FileType { RegularFile, Directory, Symlink, BlockDevice, CharDevice, Fifo, Socket }

pub enum MetaError {
    InodeNotFound(InodeId),
    DirectoryNotFound(InodeId),
    EntryNotFound { parent: InodeId, name: String },
    EntryExists { parent: InodeId, name: String },
    NotADirectory(InodeId),
    DirectoryNotEmpty(InodeId),
    NoSpace,
    PermissionDenied,
    NotLeader { leader_hint: Option<NodeId> },
    RaftError(String),
    KvError(String),
    IoError(#[from] std::io::Error),
}

pub struct DirEntry { pub name: String, pub ino: InodeId, pub file_type: FileType }
pub struct InodeAttr { ... } // has new_directory(), new_file(), new_symlink() constructors
```

KV store from `crates/claudefs-meta/src/kvstore.rs`:
```rust
pub trait KvStore: Send + Sync {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, MetaError>;
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), MetaError>;
    fn delete(&self, key: &[u8]) -> Result<(), MetaError>;
    fn contains_key(&self, key: &[u8]) -> Result<bool, MetaError>;
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, MetaError>;
    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), MetaError>;
}
pub struct MemoryKvStore { ... }
impl MemoryKvStore {
    pub fn new() -> Self { ... }
}
```

Inode store from `crates/claudefs-meta/src/inode.rs`:
```rust
pub struct InodeStore { ... }
impl InodeStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self { ... }
    pub fn allocate_inode(&self) -> InodeId { ... }
    pub fn create_inode(&self, attr: &InodeAttr) -> Result<(), MetaError> { ... }
    pub fn get_inode(&self, ino: InodeId) -> Result<InodeAttr, MetaError> { ... }
}
```

## Requirements

Create the file `crates/claudefs-security/src/meta_directory_security_tests.rs` with exactly this structure:

```rust
//! Metadata directory operations security tests.
//!
//! Part of A10 Phase 26: Meta directory security audit

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use claudefs_meta::kvstore::{KvStore, MemoryKvStore};
    use claudefs_meta::inode::InodeStore;
    use claudefs_meta::directory::DirectoryStore;
    use claudefs_meta::types::{DirEntry, FileType, InodeAttr, InodeId, MetaError};

    fn make_stores() -> (Arc<dyn KvStore>, Arc<InodeStore>, DirectoryStore) {
        let kv: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let inodes = Arc::new(InodeStore::new(kv.clone()));
        let dirs = DirectoryStore::new(kv.clone(), inodes.clone());
        let root = InodeAttr::new_directory(InodeId::ROOT_INODE, 0, 0, 0o755, 1);
        inodes.create_inode(&root).unwrap();
        (kv, inodes, dirs)
    }

    // ... tests here
}
```

Write **exactly 28 tests** covering these security areas:

### 1. Path Traversal & Name Injection (5 tests)
- Test entry name with "/" (directory separator injection)
- Test entry name with null byte "\0" (C string truncation)
- Test entry name with ".." (parent directory traversal)
- Test entry name "." (self-reference)
- Test entry name with very long string (4096 bytes) — resource boundary

### 2. Directory Entry Isolation (4 tests)
- Test entries in different parent directories are isolated (lookup in wrong parent fails)
- Test creating entry with same name in two different directories succeeds
- Test deleting entry from one directory doesn't affect same-name entry in another
- Test listing entries returns only entries from the specified parent

### 3. Rename Security (5 tests)
- Test rename from non-existent parent inode
- Test rename to non-existent destination (should create at destination)
- Test rename self-to-self (same parent, same name) — idempotency
- Test rename chain: A→B, B→C — ensure A is gone, C exists
- Test rename overwrites destination preserving source inode number

### 4. Type Confusion (4 tests)
- Test create entry in non-directory inode (should fail with NotADirectory)
- Test create entry with FileType::Symlink type
- Test create entry with FileType::BlockDevice type
- Test lookup returns correct file_type for each type stored

### 5. Concurrent-Style Safety (3 tests)
- Test creating and immediately deleting same entry
- Test double-delete same entry (second should fail)
- Test create, lookup, delete, re-create same name — check new inode ID

### 6. Boundary & Edge Cases (4 tests)
- Test with InodeId(0) as parent — edge case
- Test with InodeId(u64::MAX) as parent — edge case
- Test empty directory is_empty returns true, non-empty returns false
- Test list_entries on empty directory returns empty vec

### 7. Data Integrity (3 tests)
- Test that stored DirEntry round-trips correctly through KV (name, ino, file_type preserved)
- Test creating many entries (100) and listing returns all
- Test interleaved create/delete operations maintain consistency

## CRITICAL Rules
1. Every test MUST compile and pass.
2. Use `#[test]` (not `#[tokio::test]`). No async.
3. Only import from `claudefs_meta` (types, kvstore, inode, directory modules).
4. Do NOT use any external test crates (no proptest, quickcheck, etc.).
5. Test names must start with `test_dir_sec_`.
6. Do NOT include `fn main()`.
7. Output ONLY the Rust source file — no markdown fences, no explanation.
