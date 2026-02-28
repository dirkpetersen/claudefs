# Task: Fix types.rs and add Inode attribute types for claudefs-meta

## Context
You are implementing the `claudefs-meta` crate for ClaudeFS, a distributed POSIX file system.
The crate lives at `/home/cfs/claudefs/crates/claudefs-meta/`.

## Current types.rs issues to fix
The current `types.rs` has unnecessary `unsafe impl Send/Sync` for types that already auto-derive
Send and Sync (because all their fields are Send+Sync primitives). Remove ALL the `unsafe impl Send`
and `unsafe impl Sync` lines since they are not needed.

## What to produce

### File 1: `crates/claudefs-meta/src/types.rs`

Rewrite the complete `types.rs` with these types. Keep the existing types but fix them,
and add the new types listed below.

#### Existing types to keep (but remove unnecessary unsafe impls):
- `InodeId(u64)` - with ROOT_INODE constant, Display impl
- `NodeId(u64)` - cluster node identifier
- `ShardId(u16)` - virtual shard identifier (256 default per D4)
- `Timestamp { secs: u64, nanos: u32 }` - with `now()`, Ord/PartialOrd impls
- `VectorClock { site_id: u64, sequence: u64 }` - with `new()`, Ord/PartialOrd impls
- `MetaError` enum - thiserror, keep all existing variants
- `FileType` enum - with `mode_bits()` method
- `ReplicationState` enum

#### New types to add:

1. `Term(u64)` - Raft term number, derive Copy/Clone/Debug/PartialEq/Eq/Hash/PartialOrd/Ord/Serialize/Deserialize, Display impl

2. `LogIndex(u64)` - Raft log index, same derives as Term, Display impl

3. `InodeAttr` struct - The core POSIX inode attributes + ClaudeFS extensions:
```rust
pub struct InodeAttr {
    pub ino: InodeId,
    pub file_type: FileType,
    pub mode: u32,           // permission bits (lower 12 bits)
    pub nlink: u32,          // hard link count
    pub uid: u32,
    pub gid: u32,
    pub size: u64,           // file size in bytes
    pub blocks: u64,         // 512-byte blocks allocated
    pub atime: Timestamp,    // last access
    pub mtime: Timestamp,    // last modification
    pub ctime: Timestamp,    // last status change
    pub crtime: Timestamp,   // creation time (Linux statx)
    // ClaudeFS extensions
    pub content_hash: Option<[u8; 32]>,  // BLAKE3 hash of content
    pub repl_state: ReplicationState,
    pub vector_clock: VectorClock,
    pub generation: u64,     // inode generation number (for NFS handle stability)
}
```
Derive: Clone, Debug, PartialEq, Eq, Serialize, Deserialize

Add an `InodeAttr::new_directory(ino, uid, gid, mode, site_id)` constructor and
`InodeAttr::new_file(ino, uid, gid, mode, site_id)` constructor that set sensible defaults.
Both should set nlink appropriately (2 for dirs, 1 for files), size 0, blocks 0,
timestamps to Timestamp::now(), content_hash None, repl_state Local, vector_clock with site_id and sequence 0, generation 0.

4. `DirEntry` struct:
```rust
pub struct DirEntry {
    pub name: String,
    pub ino: InodeId,
    pub file_type: FileType,
}
```
Derive: Clone, Debug, PartialEq, Eq, Serialize, Deserialize

5. `MetaOp` enum - metadata operations for the journal:
```rust
pub enum MetaOp {
    CreateInode { attr: InodeAttr },
    DeleteInode { ino: InodeId },
    SetAttr { ino: InodeId, attr: InodeAttr },
    CreateEntry { parent: InodeId, name: String, entry: DirEntry },
    DeleteEntry { parent: InodeId, name: String },
    Rename { src_parent: InodeId, src_name: String, dst_parent: InodeId, dst_name: String },
    SetXattr { ino: InodeId, key: String, value: Vec<u8> },
    RemoveXattr { ino: InodeId, key: String },
}
```
Derive: Clone, Debug, Serialize, Deserialize

6. `RaftMessage` enum - messages exchanged between Raft peers:
```rust
pub enum RaftMessage {
    RequestVote {
        term: Term,
        candidate_id: NodeId,
        last_log_index: LogIndex,
        last_log_term: Term,
    },
    RequestVoteResponse {
        term: Term,
        vote_granted: bool,
    },
    AppendEntries {
        term: Term,
        leader_id: NodeId,
        prev_log_index: LogIndex,
        prev_log_term: Term,
        entries: Vec<LogEntry>,
        leader_commit: LogIndex,
    },
    AppendEntriesResponse {
        term: Term,
        success: bool,
        match_index: LogIndex,
    },
}
```
Derive: Clone, Debug, Serialize, Deserialize

7. `LogEntry` struct:
```rust
pub struct LogEntry {
    pub index: LogIndex,
    pub term: Term,
    pub op: MetaOp,
}
```
Derive: Clone, Debug, Serialize, Deserialize

8. `RaftState` enum:
```rust
pub enum RaftState {
    Follower,
    Candidate,
    Leader,
}
```
Derive: Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize

9. Add these methods to InodeId:
- `pub fn new(id: u64) -> Self`
- `pub fn as_u64(&self) -> u64`
- `pub fn shard(self, num_shards: u16) -> ShardId` — computes `ShardId((self.0 % num_shards as u64) as u16)`

10. Add to NodeId: `pub fn new(id: u64) -> Self`, `pub fn as_u64(&self) -> u64`
11. Add to ShardId: `pub fn new(id: u16) -> Self`, `pub fn as_u16(&self) -> u16`
12. Add to Term: `pub fn new(t: u64) -> Self`, `pub fn as_u64(&self) -> u64`
13. Add to LogIndex: `pub fn new(i: u64) -> Self`, `pub fn as_u64(&self) -> u64`, `pub const ZERO: LogIndex = LogIndex(0);`

### File 2: `crates/claudefs-meta/src/lib.rs`

Update lib.rs to include the types module:
```rust
#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

pub mod types;
pub mod consensus;
pub mod directory;
pub mod inode;
pub mod journal;
pub mod kvstore;
pub mod replication;
```

## Conventions
- Use `thiserror` for error types
- Use `serde` with `Serialize, Deserialize` derives
- Use `tracing` for logging
- No `unsafe` code (A2 is safe Rust only)
- All public items must have doc comments (`///`)

## Output format
Output ONLY the file contents, clearly separated with headers like:
```
=== FILE: crates/claudefs-meta/src/types.rs ===
```
and
```
=== FILE: crates/claudefs-meta/src/lib.rs ===
```

Do not include any explanation outside the file contents.
