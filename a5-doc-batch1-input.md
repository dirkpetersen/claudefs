# A5 FUSE Client Documentation - Batch 1: Core Types

## Task
Add doc comments to all public items in these 7 core modules to fix missing_docs warnings:

1. attr.rs — File attributes
2. error.rs — DONE (reference as style guide)
3. cache.rs — Metadata cache
4. cache_coherence.rs — Cache coherence protocols
5. inode.rs — Inode management
6. openfile.rs — Open file handles
7. operations.rs — Individual operations

## Documentation Style
- Follow error.rs as example (clear, concise)
- Use `///` for doc comments
- Include what, why, and thread-safety where relevant
- Use backticks for code: `inode`, `cache`, `mount`

## Priority Items

### attr.rs
- `FileAttr` struct — represents POSIX file attributes
- Each field (ino, size, blocks, atime, mtime, ctime, kind, perm, nlink, uid, gid, rdev, blksize, flags)
- `FileType` enum — RegularFile, Directory, Symlink, BlockDevice, CharDevice, NamedPipe, Socket
- `impl FileAttr` methods — new_file, new_dir, new_symlink, update_time, etc.

### cache.rs
- `MetadataCache` struct — caches file metadata locally
- Cache fields and methods
- Any public functions

### cache_coherence.rs
- `CacheCoherence` struct or enum — defines coherence protocol
- `CoherenceMode` enum if exists — Close-to-open, Session, Strict, etc.
- Key public methods — acquire_lease, release_lease, invalidate, etc.

### inode.rs
- `InodeEntry` struct — represents in-memory inode
- `InodeTable` struct — manages inode cache/eviction
- Public methods — insert, lookup, remove, get_lru, etc.

### openfile.rs
- `OpenFile` struct — represents open file handle
- `FileHandle` if exists
- Key methods — read, write, close, get_flags, etc.

### operations.rs
- Any public structs/enums representing FUSE operations
- Operation request/response types
- Public handler functions

## Expected Output
- All public types documented
- All public methods documented
- Field documentation where non-obvious
- Zero warnings for these 7 files
- All 918 tests still passing

## Files
All in `crates/claudefs-fuse/src/`:
- attr.rs
- cache.rs
- cache_coherence.rs
- inode.rs
- openfile.rs
- operations.rs
