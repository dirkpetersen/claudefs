Add documentation comments to `crates/claudefs-fuse/src/perf.rs`.

The crate has `#![warn(missing_docs)]`. Add `/// doc comment` to every public item that lacks one.

Rules:
- The file already has `//!` module doc at the top — keep it
- Add `/// comment` to every public struct, its fields, and all public methods
- Keep ALL existing code, tests, imports exactly the same — only add doc comments
- Write the complete updated file to `crates/claudefs-fuse/src/perf.rs`
