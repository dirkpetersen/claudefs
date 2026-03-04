[0m> build · accounts/fireworks/models/minimax-m2p5

[0m
[0m✱ [0mGlob "crates/claudefs-gateway/src/**/*.rs"[90m 56 matches[0m
[0m→ [0mRead crates/claudefs-gateway/src/wire.rs
[0m→ [0mRead crates/claudefs-gateway/src/gateway_audit.rs
[0m→ [0mRead crates/claudefs-gateway/src/access_log.rs
[0m✱ [0mGlob "crates/claudefs-security/**/*.rs"[90m 63 matches[0m
[0m→ [0mRead crates/claudefs-security/src/gateway_wire_audit_security_tests.rs
[0m→ [0mRead crates/claudefs-security/src/lib.rs
Wrote file successfully.


[0m
[0m[0m← [0mWrite crates/claudefs-security/src/gateway_wire_audit_security_tests.rs

warning: unused import: `Duration`
 --> crates/claudefs-fuse/src/fsync_barrier.rs:5:17
  |
5 | use std::time::{Duration, Instant, SystemTime};
  |                 ^^^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: field `fsync_mode` is never read
   --> crates/claudefs-fuse/src/fsync_barrier.rs:230:5
    |
225 | pub struct BarrierManager {
    |            -------------- field in this struct
...
230 |     fsync_mode: FsyncMode,
    |     ^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/flock.rs:82:18
   |
82 |     WouldBlock { holder_pid: u32 },
   |                  ^^^^^^^^^^^^^^^
   |
note: the lint level is defined here
  --> crates/claudefs-fuse/src/lib.rs:1:9
   |
 1 | #![warn(missing_docs)]
   |         ^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/fsync_barrier.rs:10:1
   |
10 | pub struct BarrierId(u64);
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/fsync_barrier.rs:13:5
   |
13 |     pub fn new(id: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/fsync_barrier.rs:25:1
   |
25 | pub enum BarrierKind {
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/fsync_barrier.rs:26:5
   |
26 |     DataOnly,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/fsync_barrier.rs:27:5
   |
27 |     MetadataOnly,
   |     ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/fsync_barrier.rs:28:5
   |
28 |     DataAndMetadata,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/fsync_barrier.rs:29:5
   |
29 |     JournalCommit,
   |     ^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/fsync_barrier.rs:33:1
   |
33 | pub enum BarrierState {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/fsync_barrier.rs:34:5
   |
34 |     Pending,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/fsync_barrier.rs:35:5
   |
35 |     Flushing,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/fsync_barrier.rs:36:5
   |
36 |     Committed,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/fsync_barrier.rs:37:5
   |[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security gateway_wire_audit_security_tests -- --nocapture 2>&1 | head -100[0m

[0m
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security gateway_wire_audit_security_tests 2>&1 | tail -50
|
351 |         assert!(site2.stats().conflicts_resolved >= 0);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-security` (lib test) generated 215 warnings (run `cargo fix --lib -p claudefs-security --tests` to apply 172 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.79s
     Running unittests src/lib.rs (target/debug/deps/claudefs_security-d966041db421f22c)

running 25 tests
test gateway_wire_audit_security_tests::tests::test_access_log_entry_builder ... ok
test gateway_wire_audit_security_tests::tests::test_access_log_filter_protocol_and_client ... ok
test gateway_wire_audit_security_tests::tests::test_access_log_stats_avg_and_rate ... ok
test gateway_wire_audit_security_tests::tests::test_audit_config_defaults ... ok
test gateway_wire_audit_security_tests::tests::test_access_log_stats_tracking ... ok
test gateway_wire_audit_security_tests::tests::test_audit_event_type_severity_mapping ... ok
test gateway_wire_audit_security_tests::tests::test_audit_record_fields ... ok
test gateway_wire_audit_security_tests::tests::test_access_log_ring_buffer ... FAILED
test gateway_wire_audit_security_tests::tests::test_audit_severity_ordering ... ok
test gateway_wire_audit_security_tests::tests::test_audit_ring_buffer_eviction ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_clear ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_disabled ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_min_severity_filter ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_record_and_query ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_id_monotonic ... ok
test gateway_wire_audit_security_tests::tests::test_wire_etag_computation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_iso8601_and_request_id ... ok
test gateway_wire_audit_security_tests::tests::test_wire_mode_format_parse_roundtrip ... ok
test gateway_wire_audit_security_tests::tests::test_wire_nfs_count_validation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_nfs_fh_valid ... ok
test gateway_wire_audit_security_tests::tests::test_wire_nfs_filename_validation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_nfs_path_validation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_part_number_upload_id ... ok
test gateway_wire_audit_security_tests::tests::test_wire_s3_key_validation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_s3_size_validation ... ok

failures:

---- gateway_wire_audit_security_tests::tests::test_access_log_ring_buffer stdout ----

thread 'gateway_wire_audit_security_tests::tests::test_access_log_ring_buffer' (542589) panicked at crates/claudefs-security/src/gateway_wire_audit_security_tests.rs:327:9:
assertion failed: recent.last().unwrap().operation == "D"
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    gateway_wire_audit_security_tests::tests::test_access_log_ring_buffer

test result: FAILED. 24 passed; 1 failed; 0 ignored; 0 measured; 1558 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p claudefs-security --lib`Index: /home/cfs/claudefs/crates/claudefs-security/src/gateway_wire_audit_security_tests.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-security/src/gateway_wire_audit_security_tests.rs
+++ /home/cfs/claudefs/crates/claudefs-security/src/gateway_wire_audit_security_tests.rs
@@ -323,9 +323,9 @@
 
     assert_eq!(log.len(), 3);
     let recent = log.recent(3);
     assert!(!recent.iter().any(|e| e.operation == "A"));
-    assert!(recent.last().unwrap().operation == "D");
+    assert_eq!(recent[0].operation, "D");
 }
 
 #[test]
 fn test_access_log_stats_tracking() {

[0m← [0mEdit crates/claudefs-security/src/gateway_wire_audit_security_tests.rs

[0m[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security gateway_wire_audit_security_tests 2>&1 | tail -30[0m
running 25 tests
test gateway_wire_audit_security_tests::tests::test_access_log_entry_builder ... ok
test gateway_wire_audit_security_tests::tests::test_access_log_filter_protocol_and_client ... ok
test gateway_wire_audit_security_tests::tests::test_access_log_stats_avg_and_rate ... ok
test gateway_wire_audit_security_tests::tests::test_access_log_ring_buffer ... ok
test gateway_wire_audit_security_tests::tests::test_audit_config_defaults ... ok
test gateway_wire_audit_security_tests::tests::test_access_log_stats_tracking ... ok
test gateway_wire_audit_security_tests::tests::test_audit_event_type_severity_mapping ... ok
test gateway_wire_audit_security_tests::tests::test_audit_record_fields ... ok
test gateway_wire_audit_security_tests::tests::test_audit_severity_ordering ... ok
test gateway_wire_audit_security_tests::tests::test_audit_ring_buffer_eviction ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_clear ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_disabled ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_min_severity_filter ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_record_and_query ... ok
test gateway_wire_audit_security_tests::tests::test_audit_trail_id_monotonic ... ok
test gateway_wire_audit_security_tests::tests::test_wire_etag_computation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_iso8601_and_request_id ... ok
test gateway_wire_audit_security_tests::tests::test_wire_mode_format_parse_roundtrip ... ok
test gateway_wire_audit_security_tests::tests::test_wire_nfs_count_validation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_nfs_fh_valid ... ok
test gateway_wire_audit_security_tests::tests::test_wire_nfs_filename_validation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_nfs_path_validation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_part_number_upload_id ... ok
test gateway_wire_audit_security_tests::tests::test_wire_s3_key_validation ... ok
test gateway_wire_audit_security_tests::tests::test_wire_s3_size_validation ... ok

test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 1558 filtered out; finished in 0.00s

[0m→ [0mRead crates/claudefs-security/src/gateway_wire_audit_security_tests.rs
All 25 tests pass. The test file `crates/claudefs-security/src/gateway_wire_audit_security_tests.rs` is complete with:

- **Category 1 (Wire Protocol — NFS):** 5 tests covering file handle validation, filename/path/count validation, and mode format/parse roundtrip
- **Category 2 (Wire Protocol — S3 & Utility):** 5 tests covering S3 key/size validation, part number/upload ID, ETag computation, ISO8601 and request ID generation
- **Category 3 (Audit Trail — Event Recording):** 5 tests covering severity ordering, event type severity mapping, record/query, disabled trail, min severity filter
- **Category 4 (Audit Trail — Ring Buffer & Edge Cases):** 5 tests covering ring buffer eviction, record fields, config defaults, clear, and ID monotonicity
- **Category 5 (Access Log — Ring Buffer & Stats):** 5 tests covering entry builder, ring buffer, stats tracking, filtering, and average/rate calculations
