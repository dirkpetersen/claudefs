[0m> build · accounts/fireworks/models/minimax-m2p5


[0m[0m✱ [0mGlob "crates/claudefs-tests/src/**/*.rs"[90m 50 matches[0m
[0m→ [0mRead crates/claudefs-tests/src/crash.rs
[0m→ [0mRead crates/claudefs-tests/src/crash_consistency_tests.rs
Let me also check the lib.rs exports:
[0m✱ [0mGrep "crash"[90m in crates/claudefs-tests/src · 4 matches[0m
[0m
[0m← [0mWrite crates/claudefs-tests/src/crash.rs


Wrote file successfully.[0m[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-tests crash -- --nocapture 2>&1 | head -80[0m

Blocking waiting for file lock on artifact directory
warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/buffer_pool.rs:6:1
  |
6 | pub enum BufferSize {
  | ^^^^^^^^^^^^^^^^^^^
  |
note: the lint level is defined here
 --> crates/claudefs-fuse/src/lib.rs:1:9
  |
1 | #![warn(missing_docs)]
  |         ^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/buffer_pool.rs:7:5
  |
7 |     Page4K,
  |     ^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/buffer_pool.rs:8:5
  |
8 |     Block64K,
  |     ^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/buffer_pool.rs:9:5
  |
9 |     Block1M,
  |     ^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:13:5
   |
13 |     pub fn size_bytes(&self) -> usize {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/buffer_pool.rs:22:1
   |
22 | pub struct Buffer {
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:23:5
   |
23 |     pub data: Vec<u8>,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:24:5
   |
24 |     pub size: BufferSize,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:25:5
   |
25 |     pub id: u64,
   |     ^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:29:5
   |
29 |     pub fn as_slice(&self) -> &[u8] {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:32:5
   |
32 |     pub fn as_mut_slice(&mut self) -> &mut [u8] {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:35:5
   |
35 |     pub fn len(&self) -> usize {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-tests crash -- --nocapture 2>&1 | tail -60
|
382 |         assert!(available >= 0);
    |                 ^^^^^^^^^^^^^^

warning: `claudefs-tests` (lib test) generated 82 warnings (run `cargo fix --lib -p claudefs-tests --tests` to apply 55 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 36.57s
     Running unittests src/lib.rs (target/debug/deps/claudefs_tests-7cfcc00c81bc5c70)

running 48 tests
test crash::tests::test_crash_consistency_test_new ... ok
test crash::tests::test_crash_error_io ... ok
test crash::tests::test_crash_error_simulation ... ok
test crash::tests::test_crash_error_verification ... ok
test crash::tests::test_crash_point_clone ... ok
test crash::tests::test_crash_point_debug ... ok
test crash::tests::test_crash_point_equality ... ok
test crash::tests::test_crash_point_new ... ok
test crash::tests::test_crash_point_various_offsets ... ok
test crash::tests::test_crash_report_clone ... ok
test crash::tests::test_crash_report_debug ... ok
test crash::tests::test_crash_simulator_default ... ok
test crash::tests::test_crash_simulator_new ... ok
test crash::tests::test_crash_simulator_with_crash_point ... ok
test crash::tests::test_crash_simulator_with_crash_points ... ok
test crash::tests::test_crash_test_report_success_rate ... ok
test crash::tests::test_crash_test_report_success_rate_zero ... ok
test crash::tests::test_run_test ... ok
test crash::tests::test_run_test_empty_crash_points ... ok
test crash::tests::test_simulate_crash_at_nonexistent ... ok
test crash_consistency_tests::test_crash_consistency_test_multiple_scenarios ... ok
test crash_consistency_tests::test_clear_crash_point ... ok
test crash::tests::test_simulate_crash_at ... ok
test crash::tests::test_simulate_crash_offset_too_large ... ok
test crash_consistency_tests::test_crash_consistency_test_results_after_run ... ok
test crash_consistency_tests::test_crash_consistency_test_new ... ok
test crash_consistency_tests::test_crash_consistency_test_results_empty_initially ... ok
test crash_consistency_tests::test_crash_consistency_test_run_success ... ok
test crash_consistency_tests::test_crash_point_after_flush ... ok
test crash_consistency_tests::test_crash_error_simulated_crash_debug ... ok
test crash_consistency_tests::test_crash_point_after_replication ... ok
test crash_consistency_tests::test_crash_point_all_variants_can_be_set ... ok
test crash_consistency_tests::test_crash_point_during_flush ... ok
test crash_consistency_tests::test_crash_point_during_replication ... ok
test crash_consistency_tests::test_crash_point_mismatch_no_trigger ... ok
test crash_consistency_tests::test_crash_report_data_consistent_on_success ... ok
test crash_consistency_tests::test_crash_report_debug ... ok
test crash_consistency_tests::test_crash_report_recovery_success_on_clean_path ... ok
test crash_consistency_tests::test_crash_report_repaired_entries_zero_on_no_crash ... ok
test crash_consistency_tests::test_crash_simulator_new_no_crash_point ... ok
test crash_consistency_tests::test_crash_simulator_write_with_data ... ok
test crash_consistency_tests::test_crash_simulator_write_with_empty_data ... ok
test crash_consistency_tests::test_multiple_runs_accumulate_results ... ok
test crash_consistency_tests::test_set_crash_point_after_write ... ok
test crash_consistency_tests::test_set_crash_point_before_write ... ok
test crash_consistency_tests::test_simulate_write_path_no_crash ... ok
test crash_consistency_tests::test_simulate_write_path_recovery_default ... ok
test split_brain_tests::tests::test_process_crash_fault_type ... ok

test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 1528 filtered out; finished in 0.00s

[0mAll 48 tests passed. The rewrite is complete:
- `CrashPoint` is now an enum with `BeforeWrite`, `AfterWrite`, `DuringFlush`, `AfterFlush`, `DuringReplication`, `AfterReplication`, and `Custom { offset, description }` variants
- `CrashError` has the new `SimulatedCrash { at: CrashPoint }` variant
- `CrashSimulator` has `set_crash_point()`, `clear_crash_point()`, `should_crash()`, and `simulate_write_path()` methods
- `CrashReport` is the new per-write struct with `crash_point`, `recovery_success`, `data_consistent`, `repaired_entries` fields
- Old `CrashReport` renamed to `CrashTestReport` for backward compatibility with `run_test()`
- `CrashConsistencyTest::new(simulator)` takes a `CrashSimulator`, with `run()` and `results()` methods
- Backward compatible: `CrashPoint::new(offset, desc)` creates `Custom` variant, existing tests pass
