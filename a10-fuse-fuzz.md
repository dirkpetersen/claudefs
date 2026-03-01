# A10: Create fuzz_fuse Module

## Problem
The claudefs-security crate declares `fuzz_fuse` in lib.rs (line 38, test-only module) but the file doesn't exist. This blocks the build.

## Task
Create crates/claudefs-security/src/fuzz_fuse.rs as a fuzzing harness for the FUSE crate.

## Specifications

### Purpose
Fuzz testing for FUSE protocol operations - similar to existing fuzz_protocol.rs and fuzz_message.rs in the same crate.

### Module Requirements
- File: crates/claudefs-security/src/fuzz_fuse.rs
- #![cfg_attr(test, allow(dead_code))]  (it's a fuzzing harness, not called in normal tests)
- #![warn(missing_docs)]
- Export a fuzzing module/harness compatible with libfuzzer or standard test approach
- Include doc comments for public items

### Implementation Strategy
Minimal stub that compiles and tests pass:
1. Create a `FuseFuzzer` struct or similar
2. Implement `Default` trait
3. Add a test module with at least one basic fuzz test
4. Follow patterns from fuzz_protocol.rs and fuzz_message.rs in same crate
5. Keep code simple and focused on compilation, not full fuzzing coverage yet

### Testing
- Must compile without warnings
- All tests must pass
- Basic doc test example that works

## Output
Create the file at crates/claudefs-security/src/fuzz_fuse.rs. It will be immediately usable to unblock the build.
