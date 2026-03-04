# Task: Add Documentation to ClaudeFS FUSE Crate Files

You are working on the `claudefs-fuse` crate (Rust), which has `#![warn(missing_docs)]` in lib.rs.
Your task is to add comprehensive `///` doc comments to ALL public items in each file listed below.

## Requirements
1. Add `//! Module-level documentation` at the top of each file (if not already present)
2. Add `/// doc comment` to every `pub enum`, `pub struct`, `pub type`, `pub fn`, and all `pub` fields
3. Add `/// Variant description` to every enum variant
4. Add `/// Field description` to every pub struct field
5. Do NOT change any code logic, structure, imports, or tests - only add doc comments
6. Keep all existing code exactly as-is
7. Output the COMPLETE file contents for each file

## File 1: operations.rs
Path: `crates/claudefs-fuse/src/operations.rs`

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

/// Applies umask to file mode bits.
///
/// Strips permissions from the given mode using the process umask.
/// Preserves file type bits (e.g., 0o040000 for directories).
pub fn apply_mode_umask(mode: u32, umask: u32) -> u32 {
    let file_type_bits = mode & 0o170000;
    let perm_bits = mode & 0o777;
    let effective_perm = perm_bits & !umask;
    file_type_bits | effective_perm
}

/// Checks if a user can access a file with given permissions.
///
/// Implements POSIX permission checking for owner, group, and others.
/// Root (uid=0) bypasses most checks but still requires execute bits on directories.
pub fn check_access(
    mode: u32,
    uid: u32,
    gid: u32,
    req_uid: u32,
    req_gid: u32,
    access_mask: c_int,
) -> bool {
    if req_uid == 0 {
        if access_mask & libc::X_OK as c_int != 0 {
            (mode & 0o111) != 0
        } else {
            true
        }
    } else if req_uid == uid {
        let shift = 6;
        ((mode >> shift) & 0o7 & access_mask as u32) != 0
    } else if req_gid == gid {
        let shift = 3;
        ((mode >> shift) & 0o7 & access_mask as u32) != 0
    } else {
        (mode & 0o7 & access_mask as u32) != 0
    }
}

/// Extracts file type from mode bits.
///
/// Maps the file type bits (S_IFMT) from a mode value to the FUSE file type enum.
pub fn mode_to_fuser_type(mode: u32) -> fuser::FileType {
    match mode & 0o170000 {
        0o100000 => fuser::FileType::RegularFile,
        0o040000 => fuser::FileType::Directory,
        0o120000 => fuser::FileType::Symlink,
        0o060000 => fuser::FileType::BlockDevice,
        0o020000 => fuser::FileType::CharDevice,
        0o010000 => fuser::FileType::NamedPipe,
        0o140000 => fuser::FileType::Socket,
        _ => fuser::FileType::RegularFile,
    }
}

/// Calculates the number of 512-byte blocks for a given size.
///
/// Rounds up to the nearest block boundary, following POSIX convention.
pub fn blocks_for_size(size: u64) -> u64 {
    size.div_ceil(512)
}
```
(the file also has a #[cfg(test)] mod tests block at the end - keep it identical)

## File 2: perf.rs
Path: `crates/claudefs-fuse/src/perf.rs`

The file has these public types needing docs:
- `OpCounters` struct with fields: lookups, reads, writes, creates, unlinks, mkdirs, rmdirs, renames, getattrs, setattrs, readdirs, errors (all AtomicU64)
- `ByteCounters` struct with fields: bytes_read, bytes_written (all AtomicU64)
- `LatencyHistogram` struct with fields: buckets, total_us, count and methods: record, p50_us, p99_us, mean_us
- `FuseMetrics` struct with fields: ops, bytes and methods: new, inc_lookup, inc_read, inc_write, inc_create, inc_unlink, inc_mkdir, inc_rmdir, inc_rename, inc_getattr, inc_setattr, inc_readdir, inc_error, snapshot
- `MetricsSnapshot` struct with all counter fields
- `OpTimer` struct with methods: new, elapsed_us, elapsed

## File 3: otel_trace.rs
Path: `crates/claudefs-fuse/src/otel_trace.rs`

The file has these public types needing docs:
- `SpanStatus` enum with variants: Ok, Error(String), Unset
- `SpanKind` enum with variants: Internal, Client, Server, Producer, Consumer
- `SpanAttribute` struct with fields: key, value
- `OtelSpan` struct with fields: trace_id, span_id, parent_span_id, operation, service, start_unix_ns, end_unix_ns, status, kind, attributes, and methods: duration_ns, is_error, add_attribute, set_status, finish
- `OtelSpanBuilder` struct with fields: trace_id, parent_span_id, operation, service, start_unix_ns, end_unix_ns, status, kind, attributes, and methods: new, with_parent, with_trace_id, with_kind, with_attribute, build
- `OtelExportBuffer` struct with field: capacity, and methods: new, push, drain, len, is_empty
- `SamplingDecision` enum with variants: RecordAndSample, Drop
- `OtelSampler` struct with methods: new, should_sample, sample_rate

## File 4: crash_recovery.rs
Path: `crates/claudefs-fuse/src/crash_recovery.rs`

The file has these public types needing docs:
- `RecoveryState` enum with variants: Idle, Scanning, Replaying{replayed, total}, Complete{recovered, orphaned}, Failed(String), and methods: is_in_progress, is_complete
- `OpenFileRecord` struct with fields: ino, fd, pid, flags, path_hint, and methods: is_writable, is_append_only
- `PendingWrite` struct with fields: ino, offset, len, dirty_since_secs, and methods: age_secs, is_stale
- `RecoveryJournal` struct with methods: new, add_open_file, add_pending_write, open_file_count, pending_write_count, writable_open_files, stale_pending_writes
- `RecoveryConfig` struct with fields: max_recovery_secs, max_open_files, stale_write_age_secs, and method: default_config
- `CrashRecovery` struct with methods: new, state, begin_scan, record_open_file, record_pending_write, begin_replay, advance_replay, complete, fail, reset, journal

## Output format

For each file, output:
```
=== FILE: crates/claudefs-fuse/src/<filename>.rs ===
<complete file content with ALL original code preserved and docs added>
=== END FILE ===
```

**Critical rules:**
- Every `pub` item (enum, struct, type, field, method, function, variant) MUST have a `///` doc comment
- Do not change any code logic, imports, test code, or structure
- Keep ALL existing `//!` module comments and `///` doc comments that already exist
- Module-level doc `//!` goes at the very top of the file
- Field docs are `/// comment` on the line immediately before the field
- The complete test module must be preserved exactly as-is
