# A5 FUSE Client — Phase 2 Integration

You are implementing Phase 2 of the ClaudeFS FUSE client (`claudefs-fuse` crate).
Work entirely within the Cargo workspace at `/home/cfs/claudefs/`.

## What you must implement

### Task 1: Fix `FuseError` — add `Busy` and `TooManyInflight` variants

File: `crates/claudefs-fuse/src/error.rs`

Add two missing variants to the `FuseError` enum (currently at line 43):
```rust
    #[error("Resource busy")]
    Busy,

    #[error("Too many in-flight requests: limit {limit}")]
    TooManyInflight { limit: usize },
```

Also update the `to_errno()` match in the `impl FuseError` block:
```rust
    FuseError::Busy => libc::EBUSY,
    FuseError::TooManyInflight { .. } => libc::EAGAIN,
```

Also add these variants to the test at the bottom: in `test_display_messages_non_empty` add:
```rust
    FuseError::Busy,
    FuseError::TooManyInflight { limit: 32 },
```
And add test:
```rust
#[test]
fn test_busy_errno() {
    let err = FuseError::Busy;
    assert_eq!(err.to_errno(), libc::EBUSY);
}

#[test]
fn test_too_many_inflight_errno() {
    let err = FuseError::TooManyInflight { limit: 32 };
    assert_eq!(err.to_errno(), libc::EAGAIN);
}
```

### Task 2: Add missing modules to `lib.rs`

File: `crates/claudefs-fuse/src/lib.rs`

Add AFTER the `pub mod transport;` line:
```rust
/// In-flight I/O router, hot-path dispatch, passthrough/zero-copy/readahead decisions.
pub mod hotpath;
/// Fsync barrier, write ordering, crash-consistent journal entries.
pub mod fsync_barrier;
/// Real metadata backend backed by claudefs-meta MetadataNode (Phase 2).
pub mod backend;
```

Also add to the `pub use` exports at the bottom (after `pub use error::{FuseError, Result};`):
```rust
pub use backend::{LocalMetaBackend, LocalMetaBackendConfig};
```

### Task 3: Update `Cargo.toml` for claudefs-fuse

File: `crates/claudefs-fuse/Cargo.toml`

Add to `[dependencies]` section:
```toml
claudefs-meta = { path = "../claudefs-meta" }
proptest = { version = "1.4", optional = true }
```

Add `[dev-dependencies]` section:
```toml
[dev-dependencies]
proptest = "1.4"
```

### Task 4: Create `backend.rs` — Phase 2 MetadataNode Integration

File: `crates/claudefs-fuse/src/backend.rs`

Create a new module that implements `FuseTransport` (from `crate::transport`) backed
by a real `claudefs_meta::node::MetadataNode`.

The `FuseTransport` trait (from crate::transport) is:
```rust
pub trait FuseTransport: Send + Sync {
    fn lookup(&self, parent: InodeId, name: &str) -> Result<Option<LookupResult>>;
    fn getattr(&self, ino: InodeId) -> Result<Option<LookupResult>>;
    fn read(&self, ino: InodeId, offset: u64, size: u32) -> Result<Vec<u8>>;
    fn write(&self, ino: InodeId, offset: u64, data: &[u8]) -> Result<u32>;
    fn create(&self, parent: InodeId, name: &str, kind: InodeKind, mode: u32, uid: u32, gid: u32) -> Result<InodeId>;
    fn remove(&self, parent: InodeId, name: &str) -> Result<()>;
    fn rename(&self, parent: InodeId, name: &str, newparent: InodeId, newname: &str) -> Result<()>;
    fn is_connected(&self) -> bool;
}
```

Types used in the trait (from `crate::inode`):
- `InodeId = u64` (type alias)
- `InodeKind` enum: `File, Directory, Symlink, BlockDevice, CharDevice, Fifo, Socket`

Types used in `LookupResult` (from `crate::transport`):
```rust
pub struct LookupResult {
    pub ino: InodeId,
    pub kind: InodeKind,
    pub size: u64,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub nlink: u32,
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
}
```

Types available from `claudefs_meta`:
- `claudefs_meta::node::MetadataNode` — the metadata server node
- `claudefs_meta::node::MetadataNodeConfig` — node configuration
- `claudefs_meta::types::{InodeId as MetaInodeId, InodeAttr, FileType, MetaError, Timestamp}`

`MetaInodeId` is a newtype: `MetaInodeId(u64)`. Use `.as_u64()` to get the u64.
`MetaInodeId::new(u64)` to construct it.

`Timestamp` has fields: `secs: i64, nanos: u32`.
Convert to `SystemTime` with:
```rust
fn timestamp_to_system_time(ts: &Timestamp) -> SystemTime {
    SystemTime::UNIX_EPOCH
        + std::time::Duration::new(ts.secs.max(0) as u64, ts.nanos)
}
```

`FileType` variants (from meta): `RegularFile, Directory, Symlink, BlockDevice, CharDevice, Fifo, Socket`

Key MetadataNode methods:
- `fn new(config: MetadataNodeConfig) -> Result<Self, MetaError>`
- `fn lookup(&self, parent: MetaInodeId, name: &str) -> Result<DirEntry, MetaError>`
- `fn getattr(&self, ino: MetaInodeId) -> Result<InodeAttr, MetaError>`
- `fn create_file(&self, parent: MetaInodeId, name: &str, uid: u32, gid: u32, mode: u32) -> Result<InodeAttr, MetaError>`
- `fn mkdir(&self, parent: MetaInodeId, name: &str, uid: u32, gid: u32, mode: u32) -> Result<InodeAttr, MetaError>`
- `fn unlink(&self, parent: MetaInodeId, name: &str) -> Result<(), MetaError>`
- `fn rmdir(&self, parent: MetaInodeId, name: &str) -> Result<(), MetaError>`
- `fn rename(&self, parent: MetaInodeId, name: &str, newparent: MetaInodeId, newname: &str) -> Result<(), MetaError>`
- `fn statfs(&self) -> StatFs`

`DirEntry` has: `ino: MetaInodeId`, `name: String`, `file_type: FileType`, `attr: InodeAttr`

`MetaError` variants include:
- `InodeNotFound(MetaInodeId)` → map to `FuseError::NotFound { ino: meta_ino.as_u64() }`  
- `NotDirectory(MetaInodeId)` → map to `FuseError::NotDirectory { ino }`
- `AlreadyExists(String)` → map to `FuseError::AlreadyExists { name: name.to_string() }`
- `PermissionDenied` → map to `FuseError::PermissionDenied { ino, op: "unknown".into() }`
- Other → map to `FuseError::InvalidArgument { msg: e.to_string() }`

Here is the implementation to write in `backend.rs`:

```rust
//! Phase 2 backend: real MetadataNode integration.
//!
//! `LocalMetaBackend` implements `FuseTransport` using a `MetadataNode`
//! from `claudefs-meta` (A2). File data is stored in an in-memory map until
//! A1 storage is wired in Phase 3.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use claudefs_meta::node::{MetadataNode, MetadataNodeConfig};
use claudefs_meta::types::{FileType, InodeAttr, InodeId as MetaInodeId, MetaError, Timestamp};

use crate::error::{FuseError, Result};
use crate::inode::{InodeId, InodeKind};
use crate::transport::{FuseTransport, LookupResult};

/// Configuration for LocalMetaBackend.
#[derive(Debug, Clone)]
pub struct LocalMetaBackendConfig {
    /// Site ID for metadata replication.
    pub site_id: u64,
    /// Number of virtual shards.
    pub num_shards: u16,
}

impl Default for LocalMetaBackendConfig {
    fn default() -> Self {
        Self {
            site_id: 1,
            num_shards: 16,
        }
    }
}

/// Phase 2 backend: FuseTransport backed by a local MetadataNode.
///
/// Uses `claudefs-meta`'s in-memory MetadataNode for metadata operations.
/// File data is stored in a separate in-memory map until A1 storage integration (Phase 3).
pub struct LocalMetaBackend {
    meta: Arc<MetadataNode>,
    /// In-memory data store: inode → file data bytes.
    /// This is a Phase 2 stub; Phase 3 will wire A1 NVMe storage.
    data: Arc<Mutex<HashMap<InodeId, Vec<u8>>>>,
}

impl LocalMetaBackend {
    /// Create a new LocalMetaBackend with default configuration.
    pub fn new() -> Result<Self> {
        Self::with_config(LocalMetaBackendConfig::default())
    }

    /// Create a new LocalMetaBackend with the given configuration.
    pub fn with_config(config: LocalMetaBackendConfig) -> Result<Self> {
        let meta_config = MetadataNodeConfig {
            site_id: config.site_id,
            num_shards: config.num_shards,
            ..MetadataNodeConfig::default()
        };
        let meta = MetadataNode::new(meta_config)
            .map_err(|e| FuseError::InvalidArgument { msg: e.to_string() })?;
        Ok(Self {
            meta: Arc::new(meta),
            data: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Returns a reference to the underlying MetadataNode.
    pub fn meta_node(&self) -> &Arc<MetadataNode> {
        &self.meta
    }
}

impl Default for LocalMetaBackend {
    fn default() -> Self {
        Self::new().expect("LocalMetaBackend::default() failed to create MetadataNode")
    }
}

// --- type conversion helpers ---

fn meta_ino(ino: InodeId) -> MetaInodeId {
    MetaInodeId::new(ino)
}

fn fuse_ino(ino: &MetaInodeId) -> InodeId {
    ino.as_u64()
}

fn file_type_to_kind(ft: &FileType) -> InodeKind {
    match ft {
        FileType::RegularFile => InodeKind::File,
        FileType::Directory => InodeKind::Directory,
        FileType::Symlink => InodeKind::Symlink,
        FileType::BlockDevice => InodeKind::BlockDevice,
        FileType::CharDevice => InodeKind::CharDevice,
        FileType::Fifo => InodeKind::Fifo,
        FileType::Socket => InodeKind::Socket,
    }
}

fn kind_to_file_type(kind: InodeKind) -> FileType {
    match kind {
        InodeKind::File => FileType::RegularFile,
        InodeKind::Directory => FileType::Directory,
        InodeKind::Symlink => FileType::Symlink,
        InodeKind::BlockDevice => FileType::BlockDevice,
        InodeKind::CharDevice => FileType::CharDevice,
        InodeKind::Fifo => FileType::Fifo,
        InodeKind::Socket => FileType::Socket,
    }
}

fn timestamp_to_system_time(ts: &Timestamp) -> SystemTime {
    SystemTime::UNIX_EPOCH
        + std::time::Duration::new(ts.secs.max(0) as u64, ts.nanos)
}

fn attr_to_lookup_result(attr: &InodeAttr) -> LookupResult {
    LookupResult {
        ino: fuse_ino(&attr.ino),
        kind: file_type_to_kind(&attr.file_type),
        size: attr.size,
        uid: attr.uid,
        gid: attr.gid,
        mode: attr.mode,
        nlink: attr.nlink,
        atime: timestamp_to_system_time(&attr.atime),
        mtime: timestamp_to_system_time(&attr.mtime),
        ctime: timestamp_to_system_time(&attr.ctime),
    }
}

fn meta_err_to_fuse(e: MetaError, ino: InodeId) -> FuseError {
    match e {
        MetaError::InodeNotFound(_) => FuseError::NotFound { ino },
        MetaError::NotDirectory(_) => FuseError::NotDirectory { ino },
        MetaError::AlreadyExists(name) => FuseError::AlreadyExists { name },
        MetaError::PermissionDenied => FuseError::PermissionDenied { ino, op: "unknown".into() },
        other => FuseError::InvalidArgument { msg: other.to_string() },
    }
}

impl FuseTransport for LocalMetaBackend {
    fn lookup(&self, parent: InodeId, name: &str) -> Result<Option<LookupResult>> {
        match self.meta.lookup(meta_ino(parent), name) {
            Ok(entry) => Ok(Some(attr_to_lookup_result(&entry.attr))),
            Err(MetaError::InodeNotFound(_)) => Ok(None),
            Err(e) => Err(meta_err_to_fuse(e, parent)),
        }
    }

    fn getattr(&self, ino: InodeId) -> Result<Option<LookupResult>> {
        match self.meta.getattr(meta_ino(ino)) {
            Ok(attr) => Ok(Some(attr_to_lookup_result(&attr))),
            Err(MetaError::InodeNotFound(_)) => Ok(None),
            Err(e) => Err(meta_err_to_fuse(e, ino)),
        }
    }

    fn read(&self, ino: InodeId, offset: u64, size: u32) -> Result<Vec<u8>> {
        let data_guard = self.data.lock().unwrap();
        let file_data = data_guard.get(&ino).cloned().unwrap_or_default();
        let start = (offset as usize).min(file_data.len());
        let end = (start + size as usize).min(file_data.len());
        Ok(file_data[start..end].to_vec())
    }

    fn write(&self, ino: InodeId, offset: u64, data: &[u8]) -> Result<u32> {
        let mut data_guard = self.data.lock().unwrap();
        let file_data = data_guard.entry(ino).or_default();
        let end = offset as usize + data.len();
        if end > file_data.len() {
            file_data.resize(end, 0);
        }
        file_data[offset as usize..end].copy_from_slice(data);
        Ok(data.len() as u32)
    }

    fn create(&self, parent: InodeId, name: &str, kind: InodeKind, mode: u32, uid: u32, gid: u32) -> Result<InodeId> {
        let attr = match kind {
            InodeKind::Directory => self
                .meta
                .mkdir(meta_ino(parent), name, uid, gid, mode)
                .map_err(|e| meta_err_to_fuse(e, parent))?,
            _ => self
                .meta
                .create_file(meta_ino(parent), name, uid, gid, mode)
                .map_err(|e| meta_err_to_fuse(e, parent))?,
        };
        Ok(fuse_ino(&attr.ino))
    }

    fn remove(&self, parent: InodeId, name: &str) -> Result<()> {
        // Try unlink first (files), then rmdir (directories)
        match self.meta.unlink(meta_ino(parent), name) {
            Ok(()) => {
                // Also clear data store
                // We don't know the ino here, so clean up lazily
                Ok(())
            }
            Err(MetaError::IsDirectory) => {
                self.meta.rmdir(meta_ino(parent), name)
                    .map_err(|e| meta_err_to_fuse(e, parent))
            }
            Err(e) => Err(meta_err_to_fuse(e, parent)),
        }
    }

    fn rename(&self, parent: InodeId, name: &str, newparent: InodeId, newname: &str) -> Result<()> {
        self.meta.rename(meta_ino(parent), name, meta_ino(newparent), newname)
            .map_err(|e| meta_err_to_fuse(e, parent))
    }

    fn is_connected(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::FuseTransport;

    fn make_backend() -> LocalMetaBackend {
        LocalMetaBackend::new().expect("backend creation failed")
    }

    #[test]
    fn test_backend_is_connected() {
        let backend = make_backend();
        assert!(backend.is_connected());
    }

    #[test]
    fn test_getattr_root_exists() {
        let backend = make_backend();
        // Root inode (1) should always exist
        let result = backend.getattr(1).expect("getattr should not error");
        assert!(result.is_some(), "root inode should exist");
        let attr = result.unwrap();
        assert_eq!(attr.ino, 1);
        assert_eq!(attr.kind, InodeKind::Directory);
    }

    #[test]
    fn test_getattr_nonexistent_returns_none() {
        let backend = make_backend();
        let result = backend.getattr(9999).expect("getattr should not error on missing");
        assert!(result.is_none(), "missing inode should return None");
    }

    #[test]
    fn test_lookup_nonexistent_returns_none() {
        let backend = make_backend();
        let result = backend.lookup(1, "no_such_file").expect("lookup should not error");
        assert!(result.is_none());
    }

    #[test]
    fn test_create_file_and_lookup() {
        let backend = make_backend();
        let ino = backend
            .create(1, "hello.txt", InodeKind::File, 0o644, 1000, 1000)
            .expect("create should succeed");
        assert!(ino > 1);

        let result = backend.lookup(1, "hello.txt").expect("lookup should succeed");
        assert!(result.is_some());
        let attr = result.unwrap();
        assert_eq!(attr.ino, ino);
        assert_eq!(attr.kind, InodeKind::File);
    }

    #[test]
    fn test_create_directory_and_lookup() {
        let backend = make_backend();
        let ino = backend
            .create(1, "mydir", InodeKind::Directory, 0o755, 0, 0)
            .expect("create dir should succeed");
        assert!(ino > 1);

        let result = backend.lookup(1, "mydir").expect("lookup dir should succeed");
        assert!(result.is_some());
        let attr = result.unwrap();
        assert_eq!(attr.ino, ino);
        assert_eq!(attr.kind, InodeKind::Directory);
    }

    #[test]
    fn test_write_and_read_data() {
        let backend = make_backend();
        let ino = backend
            .create(1, "data.bin", InodeKind::File, 0o644, 1000, 1000)
            .expect("create failed");

        let data = b"hello world";
        let written = backend.write(ino, 0, data).expect("write failed");
        assert_eq!(written as usize, data.len());

        let read_back = backend.read(ino, 0, data.len() as u32).expect("read failed");
        assert_eq!(read_back, data);
    }

    #[test]
    fn test_read_empty_file_returns_empty() {
        let backend = make_backend();
        let ino = backend
            .create(1, "empty.txt", InodeKind::File, 0o644, 1000, 1000)
            .expect("create failed");

        let data = backend.read(ino, 0, 4096).expect("read failed");
        assert!(data.is_empty());
    }

    #[test]
    fn test_write_at_offset() {
        let backend = make_backend();
        let ino = backend
            .create(1, "offset.bin", InodeKind::File, 0o644, 1000, 1000)
            .expect("create failed");

        // Write at start
        backend.write(ino, 0, b"AAAA").unwrap();
        // Write at offset 4
        backend.write(ino, 4, b"BBBB").unwrap();

        let data = backend.read(ino, 0, 8).unwrap();
        assert_eq!(&data, b"AAAABBBB");
    }

    #[test]
    fn test_read_partial() {
        let backend = make_backend();
        let ino = backend
            .create(1, "partial.txt", InodeKind::File, 0o644, 1000, 1000)
            .expect("create failed");

        backend.write(ino, 0, b"0123456789").unwrap();
        let data = backend.read(ino, 3, 4).unwrap();
        assert_eq!(&data, b"3456");
    }

    #[test]
    fn test_remove_file() {
        let backend = make_backend();
        backend
            .create(1, "todelete.txt", InodeKind::File, 0o644, 1000, 1000)
            .expect("create failed");

        backend.remove(1, "todelete.txt").expect("remove should succeed");

        let result = backend.lookup(1, "todelete.txt").expect("lookup after remove should not error");
        assert!(result.is_none(), "file should not exist after remove");
    }

    #[test]
    fn test_remove_directory() {
        let backend = make_backend();
        backend
            .create(1, "todeldir", InodeKind::Directory, 0o755, 0, 0)
            .expect("create dir failed");

        backend.remove(1, "todeldir").expect("remove dir should succeed");

        let result = backend.lookup(1, "todeldir").expect("lookup after remove should not error");
        assert!(result.is_none(), "dir should not exist after remove");
    }

    #[test]
    fn test_rename_file() {
        let backend = make_backend();
        backend
            .create(1, "original.txt", InodeKind::File, 0o644, 1000, 1000)
            .expect("create failed");

        backend.rename(1, "original.txt", 1, "renamed.txt").expect("rename should succeed");

        let original = backend.lookup(1, "original.txt").expect("lookup failed");
        assert!(original.is_none(), "original name should not exist");

        let renamed = backend.lookup(1, "renamed.txt").expect("lookup after rename should succeed");
        assert!(renamed.is_some(), "renamed file should exist");
    }

    #[test]
    fn test_getattr_returns_correct_mode() {
        let backend = make_backend();
        let ino = backend
            .create(1, "mode_test.txt", InodeKind::File, 0o644, 1000, 1000)
            .expect("create failed");

        let attr = backend.getattr(ino).unwrap().unwrap();
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
        assert_eq!(attr.mode & 0o777, 0o644);
    }

    #[test]
    fn test_type_conversions() {
        assert_eq!(file_type_to_kind(&FileType::RegularFile), InodeKind::File);
        assert_eq!(file_type_to_kind(&FileType::Directory), InodeKind::Directory);
        assert_eq!(file_type_to_kind(&FileType::Symlink), InodeKind::Symlink);
    }

    #[test]
    fn test_backend_config_defaults() {
        let config = LocalMetaBackendConfig::default();
        assert_eq!(config.site_id, 1);
        assert_eq!(config.num_shards, 16);
    }

    #[test]
    fn test_meta_node_accessible() {
        let backend = make_backend();
        let _meta = backend.meta_node();
        // Just verify the Arc is accessible
    }
}
```

## Important notes

1. The `claudefs-meta` types use `MetaInodeId(u64)` newtype, not plain `u64`. Always use `.as_u64()` and `MetaInodeId::new(u64)` for conversions.

2. The `MetaError::IsDirectory` variant may or may not exist - if it doesn't, handle it with the catch-all `other => FuseError::InvalidArgument`. The actual variant names from `types.rs` are what matter. Look at what variants of MetaError actually exist and use the correct match arms.

3. In the remove() method, we need to determine if the target is a file or directory. One approach: try `unlink` first, and if it returns `MetaError::IsDirectory` (if that variant exists), fall back to `rmdir`. Otherwise, check for the error type.

4. DO NOT create any other files or modify anything outside what's specified here.

5. After writing the code, the result must compile with:
   ```
   cargo build --package claudefs-fuse
   ```
   and tests must pass with:
   ```
   cd /home/cfs/claudefs && cargo test --package claudefs-fuse
   ```

## File paths to modify/create

1. MODIFY: `crates/claudefs-fuse/src/error.rs` — add Busy + TooManyInflight variants
2. MODIFY: `crates/claudefs-fuse/src/lib.rs` — add pub mod hotpath, fsync_barrier, backend
3. MODIFY: `crates/claudefs-fuse/Cargo.toml` — add claudefs-meta dep
4. CREATE: `crates/claudefs-fuse/src/backend.rs` — LocalMetaBackend impl

Please implement all four tasks. Show the full content of each file you modify or create.
