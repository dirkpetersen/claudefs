# A5: Create Missing FUSE Modules

## Problem
The claudefs-fuse crate declares three modules in lib.rs that don't exist:
- buffer_pool (line 6)
- mount_opts (line 30)
- notify_filter (line 32)

This blocks the entire build. These modules are needed for Phase 3 production readiness.

## Task
Create three stub/placeholder Rust modules for the FUSE crate at:
- crates/claudefs-fuse/src/buffer_pool.rs
- crates/claudefs-fuse/src/mount_opts.rs
- crates/claudefs-fuse/src/notify_filter.rs

## Specifications

### buffer_pool.rs
- Purpose: Buffer pool management for FUSE I/O operations
- Should export a public `BufferPool` struct/type
- Include basic doc comments explaining the buffer pool functionality
- Implement trait: `std::default::Default` with an empty/no-op implementation
- Keep minimal - just enough to compile; can be expanded later
- Add basic tests module with at least one test

### mount_opts.rs
- Purpose: Mount options parsing and configuration
- Should export a public `MountOptions` struct
- Include basic doc comments
- Add common FUSE mount options fields (as_mut_str comments if needed)
- Implement `std::default::Default`
- Keep minimal; can be expanded later
- Add basic tests module

### notify_filter.rs
- Purpose: Directory notification filtering for inotify/fanotify-like events
- Should export a public `NotifyFilter` struct/type or enum
- Include basic doc comments
- Could be a bitflag for filtering different event types
- Implement `std::default::Default`
- Keep minimal; can be expanded later
- Add basic tests module

## Requirements
- All three files should follow the existing code style in the crate
- Each file should have `#![warn(missing_docs)]` at the top
- Include reasonable doc comments for all public items
- Use proper error types (thiserror if needed for errors)
- All code must compile without warnings
- No unsafe code
- Simple unit tests in each module to validate compilation

## Output
Create the three .rs files directly in crates/claudefs-fuse/src/ directory.
They will be immediately usable and should make the build succeed.
