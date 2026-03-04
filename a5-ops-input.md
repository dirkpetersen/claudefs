Add documentation comments to `crates/claudefs-fuse/src/operations.rs`.

The file has `#![warn(missing_docs)]` in the crate root. Add `/// doc comment` to every public item that lacks one.

Rules:
- Add `//! Module-level doc` at the top
- Add `/// comment` to every `pub enum`, `pub struct`, enum variant, and `pub` struct field
- Keep ALL existing code, tests, imports exactly the same — only add doc comments
- Do not use shell commands; directly write/edit the file

The current file content (add docs to all undocumented pub items):

```rust
use libc::c_int;
use std::time::SystemTime;

pub enum FuseOpKind {
    Lookup,
    GetAttr,
    SetAttr,
    MkDir,
    RmDir,
    Create,
    Unlink,
    Read,
    Write,
    ReadDir,
    Open,
    Release,
    OpenDir,
    ReleaseDir,
    Rename,
    Flush,
    Fsync,
    StatFs,
    Access,
    Link,
    Symlink,
    ReadLink,
    SetXAttr,
    GetXAttr,
    ListXAttr,
    RemoveXAttr,
}

pub struct SetAttrRequest {
    pub ino: u64,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub size: Option<u64>,
    pub atime: Option<SystemTime>,
    pub mtime: Option<SystemTime>,
    pub fh: Option<u64>,
    pub flags: Option<u32>,
}

pub struct StatfsReply {
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
    pub bsize: u32,
    pub namelen: u32,
    pub frsize: u32,
}

pub struct CreateRequest {
    pub parent: u64,
    pub name: String,
    pub mode: u32,
    pub umask: u32,
    pub flags: i32,
    pub uid: u32,
    pub gid: u32,
}

pub struct MkdirRequest {
    pub parent: u64,
    pub name: String,
    pub mode: u32,
    pub umask: u32,
    pub uid: u32,
    pub gid: u32,
}

pub struct RenameRequest {
    pub parent: u64,
    pub name: String,
    pub newparent: u64,
    pub newname: String,
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub ino: u64,
    pub offset: i64,
    pub kind: fuser::FileType,
    pub name: String,
}
```

(The rest of the file has functions that already have doc comments, and a test module — leave them completely unchanged.)

Write the complete updated file to `crates/claudefs-fuse/src/operations.rs`.
