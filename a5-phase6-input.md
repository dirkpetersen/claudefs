# A5 FUSE Client — Phase 6: Production Readiness

You are implementing Phase 6 of the ClaudeFS FUSE client crate (`claudefs-fuse`).
Phase 5 is already complete with 235 tests across 17 modules.

## Context: Existing Crate Structure

The crate lives at `crates/claudefs-fuse/src/`. Existing modules:
- `error.rs` — `FuseError`, `Result<T>` (thiserror)
- `inode.rs` — `InodeId = u64`, `InodeKind { File, Dir, Symlink }`, `InodeEntry`
- `attr.rs` — `FuseAttr` wrapper around `fuser::FileAttr`
- `cache.rs` — `MetaCache<K,V>` with TTL
- `operations.rs` — FUSE op dispatch tables
- `filesystem.rs` — `ClaudeFsFilesystem` implementing `fuser::Filesystem`
- `passthrough.rs` — FUSE passthrough mode
- `server.rs` — FUSE server loop
- `mount.rs` — mount/unmount
- `xattr.rs` — extended attributes
- `symlink.rs` — symlink handling
- `datacache.rs` — data block cache
- `transport.rs` — `FuseTransport` trait + `StubTransport`, `TransportConfig`, `RemoteRef`, `LookupResult`
- `session.rs` — session management
- `locking.rs` — POSIX advisory locks
- `mmap.rs` — memory-mapped I/O
- `perf.rs` — performance metrics

Cargo.toml dependencies: `tokio`, `thiserror`, `anyhow`, `serde`, `tracing`, `fuser = "0.15"`, `libc = "0.2"`, `lru = "0.12"`

## Key Types from Existing Code

```rust
// From error.rs
use thiserror::Error;
#[derive(Debug, Error)]
pub enum FuseError {
    #[error("operation not supported: {op}")]
    NotSupported { op: String },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    // ... etc
}
pub type Result<T> = std::result::Result<T, FuseError>;

// From inode.rs
pub type InodeId = u64;
#[derive(Debug, Clone, PartialEq)]
pub enum InodeKind { File, Dir, Symlink }

// From transport.rs
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

## Phase 6: 5 New Modules

Implement the following 5 new production-readiness modules. Each must:
- Be standalone (no cross-module imports beyond `error`, `inode`, `transport`)
- Have `#[cfg(test)] mod tests { ... }` with AT LEAST 12 unit tests per module
- Use `thiserror` for errors (add new variants to local enums, not FuseError)
- No `unwrap()` in non-test code
- No `panic!()` in non-test code
- No `unsafe` except where explicitly specified
- Use `tracing::debug!` / `tracing::warn!` / `tracing::error!` for logging

### Module 1: `prefetch.rs` — Sequential Read-Ahead Prefetch

A read-ahead engine that detects sequential read patterns and prefetches data blocks.

```rust
// Public interface
pub struct PrefetchConfig {
    pub window_size: usize,       // Number of blocks to prefetch ahead (default: 8)
    pub block_size: u64,          // Block size in bytes (default: 65536 = 64KB)
    pub max_inflight: usize,      // Max in-flight prefetch requests (default: 4)
    pub detection_threshold: u32, // Sequential reads to trigger prefetch (default: 2)
}
impl Default for PrefetchConfig { ... }

pub struct PrefetchEntry {
    pub ino: InodeId,
    pub offset: u64,
    pub data: Vec<u8>,
    pub ready: bool,
}

pub struct PrefetchEngine {
    config: PrefetchConfig,
    // Access pattern tracking per inode: last_offset, sequential_count
    patterns: std::collections::HashMap<InodeId, AccessPattern>,
    // Prefetch buffer keyed by (ino, block_start_offset)
    buffer: std::collections::HashMap<(InodeId, u64), PrefetchEntry>,
}

impl PrefetchEngine {
    pub fn new(config: PrefetchConfig) -> Self { ... }

    // Record a read access and return the aligned block offset
    pub fn record_access(&mut self, ino: InodeId, offset: u64, size: u32) -> u64 { ... }

    // Returns true if sequential pattern detected for this inode
    pub fn is_sequential(&self, ino: InodeId) -> bool { ... }

    // Returns list of (ino, offset) pairs that should be prefetched
    pub fn compute_prefetch_list(&self, ino: InodeId, current_offset: u64) -> Vec<(InodeId, u64)> { ... }

    // Store a prefetched block in the buffer
    pub fn store_prefetch(&mut self, ino: InodeId, offset: u64, data: Vec<u8>) { ... }

    // Try to serve a read from the prefetch buffer
    pub fn try_serve(&self, ino: InodeId, offset: u64, size: u32) -> Option<Vec<u8>> { ... }

    // Evict prefetch entries for an inode (e.g., on file close)
    pub fn evict(&mut self, ino: InodeId) { ... }

    // Returns current buffer occupancy stats
    pub fn stats(&self) -> PrefetchStats { ... }
}

pub struct PrefetchStats {
    pub entries_cached: usize,
    pub inodes_tracked: usize,
    pub sequential_inodes: usize,
}
```

Tests must cover:
1. Default config has sensible values (window > 0, block_size > 0)
2. Single random access does not trigger sequential detection
3. Two consecutive sequential accesses trigger detection
4. Three sequential accesses: compute_prefetch_list returns `window_size` entries
5. Prefetch list offsets are block-aligned
6. store_prefetch stores data retrievable by try_serve
7. try_serve returns None for non-cached offset
8. try_serve returns Some with correct data for cached block
9. evict removes all entries for that inode but not others
10. stats reflects correct counts
11. record_access on same inode resets sequential count for large gap
12. Large offset gap (> 2x block_size) resets sequential detection
13. Multiple inodes tracked independently
14. Prefetch list does not exceed max_inflight * block_size range
15. try_serve with partial sub-block offset returns correct slice

### Module 2: `writebuf.rs` — Write Coalescing Buffer

A write buffer that coalesces small writes into larger aligned chunks before flushing.

```rust
pub struct WriteBufConfig {
    pub flush_threshold: usize,  // Flush when buffer exceeds this (default: 1MB = 1<<20)
    pub max_coalesce_gap: u64,   // Max gap between writes to coalesce (default: 4096)
    pub dirty_timeout_ms: u64,   // Auto-flush after this many ms (default: 5000)
}
impl Default for WriteBufConfig { ... }

pub struct WriteRange {
    pub offset: u64,
    pub data: Vec<u8>,
}

pub struct WriteBuf {
    config: WriteBufConfig,
    // Per-inode dirty ranges
    dirty: std::collections::HashMap<InodeId, Vec<WriteRange>>,
    // Total buffered bytes
    total_bytes: usize,
}

impl WriteBuf {
    pub fn new(config: WriteBufConfig) -> Self { ... }

    // Buffer a write. Returns true if flush is needed (threshold exceeded)
    pub fn buffer_write(&mut self, ino: InodeId, offset: u64, data: &[u8]) -> bool { ... }

    // Coalesce adjacent/overlapping ranges for an inode
    pub fn coalesce(&mut self, ino: InodeId) { ... }

    // Take all dirty ranges for an inode (clears them from buffer)
    pub fn take_dirty(&mut self, ino: InodeId) -> Vec<WriteRange> { ... }

    // Check if inode has dirty data
    pub fn is_dirty(&self, ino: InodeId) -> bool { ... }

    // List all inodes with dirty data
    pub fn dirty_inodes(&self) -> Vec<InodeId> { ... }

    // Total bytes buffered
    pub fn total_buffered(&self) -> usize { ... }

    // Discard all buffered writes for an inode (e.g., on truncate)
    pub fn discard(&mut self, ino: InodeId) { ... }
}
```

Tests must cover:
1. Default config has valid thresholds
2. Single write buffers correctly (total_buffered = data.len())
3. is_dirty returns true after write
4. take_dirty returns the range and clears it
5. is_dirty returns false after take_dirty
6. buffer_write returns true when threshold exceeded
7. buffer_write returns false when below threshold
8. coalesce merges adjacent ranges (two touching writes → one range)
9. coalesce merges overlapping ranges
10. coalesce leaves non-adjacent ranges separate (gap > max_coalesce_gap)
11. discard removes all data for inode
12. discard does not affect other inodes
13. dirty_inodes returns all inodes with data
14. Multiple writes to same inode accumulate total_buffered
15. Zero-length write is a no-op

### Module 3: `reconnect.rs` — Transport Reconnection Logic

Reconnection and failover logic for handling transport failures.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Reconnecting { attempt: u32 },
    Failed,
}

pub struct ReconnectConfig {
    pub initial_delay_ms: u64,    // First retry delay (default: 100)
    pub max_delay_ms: u64,        // Cap on exponential backoff (default: 30_000)
    pub max_attempts: u32,        // Give up after this many attempts (default: 10)
    pub backoff_multiplier: f64,  // Exponential multiplier (default: 2.0)
    pub jitter_fraction: f64,     // Random jitter 0.0–1.0 (default: 0.1)
}
impl Default for ReconnectConfig { ... }

pub struct ReconnectState {
    pub config: ReconnectConfig,
    pub state: ConnectionState,
    pub attempt: u32,
    pub last_delay_ms: u64,
}

impl ReconnectState {
    pub fn new(config: ReconnectConfig) -> Self { ... }

    // Called when connection succeeds — reset to Connected state
    pub fn on_connected(&mut self) { ... }

    // Called when connection drops — transition to Reconnecting
    pub fn on_disconnected(&mut self) { ... }

    // Compute the next retry delay (with exponential backoff + jitter)
    pub fn next_delay_ms(&mut self) -> u64 { ... }

    // Returns true if we should give up (max attempts exceeded)
    pub fn should_give_up(&self) -> bool { ... }

    // Advance attempt counter; transitions to Failed if max exceeded
    pub fn advance_attempt(&mut self) { ... }

    // Returns true if currently in a state where ops should be retried
    pub fn is_retrying(&self) -> bool { ... }
}

// Retry an operation with backoff, using the reconnect state
pub fn retry_with_backoff<T, E, F>(
    state: &mut ReconnectState,
    op: F,
) -> Result<T, E>
where
    F: Fn() -> std::result::Result<T, E>,
    E: std::fmt::Debug;
```

Tests must cover:
1. Default config has valid ranges (initial < max, multiplier > 1.0)
2. New state is Disconnected
3. on_connected sets state to Connected, resets attempt to 0
4. on_disconnected transitions to Reconnecting
5. next_delay_ms returns initial delay on first attempt
6. next_delay_ms doubles on second attempt (exponential)
7. next_delay_ms is capped at max_delay_ms
8. next_delay_ms with zero jitter is exactly delay
9. should_give_up returns false at zero attempts
10. should_give_up returns true when attempt >= max_attempts
11. advance_attempt increments counter
12. advance_attempt transitions to Failed when max exceeded
13. is_retrying returns true in Reconnecting state
14. is_retrying returns false in Connected state
15. is_retrying returns false in Failed state

### Module 4: `openfile.rs` — Open File Handle Tracking

Track open file handles, their mode flags, and per-handle state.

```rust
use std::collections::HashMap;

pub type FileHandle = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum OpenFlags {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

impl OpenFlags {
    pub fn is_readable(&self) -> bool { ... }
    pub fn is_writable(&self) -> bool { ... }
    pub fn from_libc(flags: i32) -> Self { ... }  // O_RDONLY=0, O_WRONLY=1, O_RDWR=2
}

#[derive(Debug, Clone)]
pub struct OpenFileEntry {
    pub fh: FileHandle,
    pub ino: InodeId,
    pub flags: OpenFlags,
    pub offset: u64,       // Current file position
    pub dirty: bool,       // Has unflushed writes
}

pub struct OpenFileTable {
    next_fh: FileHandle,
    entries: HashMap<FileHandle, OpenFileEntry>,
}

impl OpenFileTable {
    pub fn new() -> Self { ... }

    // Register a new open file. Returns the allocated FileHandle.
    pub fn open(&mut self, ino: InodeId, flags: OpenFlags) -> FileHandle { ... }

    // Get a reference to an open file entry
    pub fn get(&self, fh: FileHandle) -> Option<&OpenFileEntry> { ... }

    // Get a mutable reference
    pub fn get_mut(&mut self, fh: FileHandle) -> Option<&mut OpenFileEntry> { ... }

    // Close and remove a file handle
    pub fn close(&mut self, fh: FileHandle) -> Option<OpenFileEntry> { ... }

    // Update file position
    pub fn seek(&mut self, fh: FileHandle, offset: u64) -> bool { ... }

    // Mark a handle as having dirty (unflushed) data
    pub fn mark_dirty(&mut self, fh: FileHandle) -> bool { ... }

    // Mark a handle as clean (after flush)
    pub fn mark_clean(&mut self, fh: FileHandle) -> bool { ... }

    // List all open handles for a given inode
    pub fn handles_for_inode(&self, ino: InodeId) -> Vec<FileHandle> { ... }

    // Count open handles
    pub fn count(&self) -> usize { ... }

    // Count handles with dirty state
    pub fn dirty_count(&self) -> usize { ... }
}

impl Default for OpenFileTable { fn default() -> Self { Self::new() } }
```

Tests must cover:
1. New table has count = 0
2. open() returns distinct handles for each call
3. get() returns Some after open()
4. get() returns None for unknown handle
5. close() returns the entry
6. get() returns None after close()
7. count() reflects open/close lifecycle
8. seek() updates offset
9. seek() returns false for unknown handle
10. mark_dirty() sets dirty = true
11. mark_clean() sets dirty = false
12. dirty_count() counts only dirty handles
13. handles_for_inode() returns all handles for that inode
14. handles_for_inode() returns empty for closed handles
15. OpenFlags::from_libc(0) = ReadOnly, from_libc(1) = WriteOnly, from_libc(2) = ReadWrite
16. is_readable / is_writable correct for all variants

### Module 5: `dirnotify.rs` — Directory Change Notifications

Track directory modification events for kernel inotify/dnotify integration.

```rust
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub enum DirEvent {
    Created { ino: InodeId, name: String },
    Deleted { ino: InodeId, name: String },
    Renamed { old_name: String, new_name: String, ino: InodeId },
    Attrib  { ino: InodeId },   // Attribute changed (chmod, chown, etc.)
}

pub struct NotifyConfig {
    pub max_queue_per_dir: usize,  // Max events per directory queue (default: 256)
    pub max_dirs_tracked: usize,   // Max directories with active watches (default: 1024)
}
impl Default for NotifyConfig { ... }

pub struct DirNotify {
    config: NotifyConfig,
    // Per-directory event queues
    queues: HashMap<InodeId, VecDeque<DirEvent>>,
    // Which directories have listeners
    watched: std::collections::HashSet<InodeId>,
}

impl DirNotify {
    pub fn new(config: NotifyConfig) -> Self { ... }

    // Register a watch on a directory
    pub fn watch(&mut self, dir_ino: InodeId) -> bool { ... }  // false if limit exceeded

    // Remove a watch
    pub fn unwatch(&mut self, dir_ino: InodeId) { ... }

    // Post an event to a directory's queue (no-op if not watched)
    pub fn post(&mut self, dir_ino: InodeId, event: DirEvent) { ... }

    // Drain all pending events for a directory
    pub fn drain(&mut self, dir_ino: InodeId) -> Vec<DirEvent> { ... }

    // Peek at pending event count for a directory
    pub fn pending_count(&self, dir_ino: InodeId) -> usize { ... }

    // List all watched directory inodes
    pub fn watched_dirs(&self) -> Vec<InodeId> { ... }

    // Check if a directory is being watched
    pub fn is_watched(&self, dir_ino: InodeId) -> bool { ... }

    // Total events across all queues
    pub fn total_pending(&self) -> usize { ... }
}
```

Tests must cover:
1. New DirNotify has no watched dirs
2. watch() returns true for first watch
3. is_watched() returns true after watch()
4. is_watched() returns false before watch()
5. unwatch() removes the directory
6. post() on unwatched directory is silently ignored (no events)
7. post() on watched directory stores the event
8. drain() returns all events in order
9. drain() clears the queue
10. pending_count() reflects queued events
11. total_pending() sums across all watched dirs
12. max_queue_per_dir: events exceeding limit are dropped (oldest or newest — your choice, but document it)
13. max_dirs_tracked: watch() returns false when limit exceeded
14. Events have correct fields (Created name, Deleted name, Renamed old/new, Attrib ino)
15. watched_dirs() returns all watched inodes

## Output Requirements

1. Output each file with a clear header: `## FILE: crates/claudefs-fuse/src/prefetch.rs`
2. Each file must be complete and compilable Rust
3. Imports: only from `std`, `crate::error`, `crate::inode`, `crate::transport`
4. No external crate imports except those already in Cargo.toml: `tokio`, `thiserror`, `anyhow`, `serde`, `tracing`, `fuser`, `libc`, `lru`
5. All types must derive `Debug` where sensible
6. Do NOT modify `lib.rs`, `Cargo.toml`, or any existing files
7. Each module must have exactly the functions described (additional helpers are OK)
8. Aim for 12–16 tests per module (total: 60–80 new tests)

## Conventions
- Error handling: use `thiserror` in libs, local error enums per module where needed
- Logging: `tracing::debug!`, `tracing::warn!`, `tracing::error!`
- No panics or unwraps in non-test code
- Structs that own data: impl Default where sensible
