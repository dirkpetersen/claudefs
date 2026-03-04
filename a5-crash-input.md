Add documentation comments to `crates/claudefs-fuse/src/crash_recovery.rs`.

The crate has `#![warn(missing_docs)]`. Add `/// doc comment` to every public item that lacks one.

Rules:
- Add `//!` module-level doc at the top of the file
- Add `/// comment` to every public enum, struct, enum variant, pub field, and all public methods
- Keep ALL existing code, tests, imports exactly the same — only add doc comments
- Write the complete updated file to `crates/claudefs-fuse/src/crash_recovery.rs`
