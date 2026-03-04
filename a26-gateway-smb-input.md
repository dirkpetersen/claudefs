# Task: Create gateway_smb_security_tests.rs for ClaudeFS Security Audit (A10 Phase 26)

## Context

You are writing a Rust security test module for the `claudefs-security` crate. This module audits the SMB3 gateway stub in `claudefs-gateway`.

## Source Under Audit

The file `crates/claudefs-gateway/src/smb.rs` contains:

```rust
//! SMB3 gateway stub.

use crate::error::{GatewayError, Result};

/// SMB session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmbSessionId(pub u64);

/// SMB tree connection identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmbTreeId(pub u32);

/// SMB file handle identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmbFileId(pub u64);

/// SMB authentication information for a session.
#[derive(Debug, Clone)]
pub struct SmbAuthInfo {
    pub session_id: SmbSessionId,
    pub uid: u32,
    pub gid: u32,
    pub supplementary_gids: Vec<u32>,
    pub username: String,
    pub domain: String,
}

/// Flags for opening a file.
#[derive(Debug, Clone, Copy)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub exclusive: bool,
}

impl OpenFlags {
    pub fn new(read: bool, write: bool, create: bool, truncate: bool, exclusive: bool) -> Self {
        Self { read, write, create, truncate, exclusive }
    }
}

/// SMB file metadata.
#[derive(Debug, Clone)]
pub struct SmbFileStat {
    pub size: u64,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub inode: u64,
    pub atime_ns: u64,
    pub mtime_ns: u64,
    pub ctime_ns: u64,
}

/// Directory entry with name and metadata.
#[derive(Debug, Clone)]
pub struct SmbDirEntry {
    pub name: String,
    pub stat: SmbFileStat,
}

/// Virtual filesystem operations for SMB.
pub trait SmbVfsOps: Send + Sync {
    fn smb_open(&self, auth: &SmbAuthInfo, path: &str, flags: OpenFlags) -> Result<SmbFileId>;
    fn smb_close(&self, file_id: SmbFileId) -> Result<()>;
    fn smb_read(&self, file_id: SmbFileId, offset: u64, len: u32) -> Result<Vec<u8>>;
    fn smb_write(&self, file_id: SmbFileId, offset: u64, data: &[u8]) -> Result<u32>;
    fn smb_stat(&self, auth: &SmbAuthInfo, path: &str) -> Result<SmbFileStat>;
    fn smb_mkdir(&self, auth: &SmbAuthInfo, path: &str) -> Result<()>;
    fn smb_unlink(&self, auth: &SmbAuthInfo, path: &str) -> Result<()>;
    fn smb_rename(&self, auth: &SmbAuthInfo, from: &str, to: &str) -> Result<()>;
    fn smb_readdir(&self, auth: &SmbAuthInfo, path: &str) -> Result<Vec<SmbDirEntry>>;
}

/// Stub implementation that returns NotImplemented errors.
pub struct SmbVfsStub;

// ... all methods return Err(GatewayError::NotImplemented { feature: "smb3".to_string() })
```

The `GatewayError` enum (from `crates/claudefs-gateway/src/error.rs`):
```rust
#[derive(Error, Debug)]
pub enum GatewayError {
    Nfs3NoEnt, Nfs3Io, Nfs3Acces, Nfs3Exist, Nfs3NotDir, Nfs3IsDir,
    Nfs3Inval, Nfs3FBig, Nfs3NoSpc, Nfs3ROfs, Nfs3Stale, Nfs3BadHandle,
    Nfs3NotSupp, Nfs3ServerFault,
    S3BucketNotFound { bucket: String },
    S3ObjectNotFound { key: String },
    S3InvalidBucketName { name: String },
    S3AccessDenied,
    XdrDecodeError { reason: String },
    XdrEncodeError { reason: String },
    ProtocolError { reason: String },
    BackendError { reason: String },
    NotImplemented { feature: String },
    IoError(#[from] std::io::Error),
}
```

## Requirements

Create the file `crates/claudefs-security/src/gateway_smb_security_tests.rs` with exactly this structure:

```rust
//! Gateway SMB3 protocol security tests.
//!
//! Part of A10 Phase 26: Gateway SMB3 security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::smb::{
        OpenFlags, SmbAuthInfo, SmbDirEntry, SmbFileId, SmbFileStat,
        SmbSessionId, SmbTreeId, SmbVfsOps, SmbVfsStub,
    };
    use claudefs_gateway::error::GatewayError;

    // ... tests here
}
```

Write **exactly 25 tests** covering these security areas:

### 1. Session ID Security (4 tests)
- Test session ID zero is valid (boundary)
- Test session ID u64::MAX is valid (boundary)
- Test session IDs are hashable (required for session tracking)
- Test tree ID u32::MAX boundary

### 2. Authentication Info Validation (4 tests)
- Test auth with uid 0 (root) — elevated privilege scenario
- Test auth with empty username — identity validation
- Test auth with empty domain — identity validation
- Test auth with very large supplementary_gids list (1000 entries) — resource exhaustion

### 3. Open Flags Security (4 tests)
- Test conflicting flags: create + exclusive + truncate simultaneously
- Test all-true flags combination
- Test all-false flags combination (read-only without read)
- Test write without create (overwrite existing only)

### 4. File Stat Integrity (3 tests)
- Test SmbFileStat with size u64::MAX — boundary
- Test SmbFileStat with all-zero fields — default state
- Test SmbDirEntry with empty name — validation boundary

### 5. VFS Stub Security (5 tests)
- Test all 9 stub operations return NotImplemented (consolidated)
- Test stub is Send + Sync (required by trait)
- Test stub open with path traversal attempt ("../../etc/passwd")
- Test stub open with null byte in path ("/test\0evil")
- Test stub open with very long path (8192 chars)

### 6. Path Input Validation (5 tests)
- Test path with unicode normalization attack (combining characters)
- Test path with Windows-style separators ("C:\\test\\file")
- Test absolute vs relative path handling ("/abs" vs "rel/path")
- Test path with double slashes ("//test//file")
- Test empty path string

## CRITICAL Rules
1. Every test MUST compile and pass. The stub returns NotImplemented for all ops — tests should verify that behavior.
2. Use `#[test]` (not `#[tokio::test]`). No async.
3. Only import from `claudefs_gateway::smb` and `claudefs_gateway::error`.
4. Do NOT use any external test crates (no proptest, quickcheck, etc.).
5. Test names must start with `test_smb_sec_`.
6. Do NOT include `fn main()`.
7. Output ONLY the Rust source file — no markdown fences, no explanation.
