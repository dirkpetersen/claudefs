Fix ALL 49 clippy warnings in /home/cfs/claudefs/crates/claudefs-reduce.

The warnings are all about missing documentation and one unused variable.

Files to fix:
1. crates/claudefs-reduce/src/worm_reducer.rs - add doc comments to all public items
2. crates/claudefs-reduce/src/key_rotation_scheduler.rs - add doc comments + fix unused var
3. crates/claudefs-reduce/src/lib.rs - add doc comment before `pub mod worm_reducer;`

Rules:
- Add /// doc comments to all public enums, structs, variants, fields, methods
- Add //! module doc at top of files
- In key_rotation_scheduler.rs line 144: change `let total = *total;` to `let _total = *total;`
- In lib.rs: add `/// WORM compliance and retention policy enforcement.` before `pub mod worm_reducer;`
- Do NOT change any logic or tests

Use the Read tool to read the current file contents first.
Then use the Write tool to write the complete fixed versions of each file.

After writing, run: cargo clippy -p claudefs-reduce 2>&1
Verify 0 warnings.
Then run: cargo test -p claudefs-reduce 2>&1
Verify all 166 tests pass.
