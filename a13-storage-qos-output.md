[0m> build · accounts/fireworks/models/minimax-m2p5


[0m
[0m✱ [0mGlob "crates/claudefs-security/src/**/*.rs"[90m 49 matches[0m[0m✱ [0mGlob "crates/claudefs-storage/src/**/*.rs"[90m 38 matches[0m
[0m→ [0mRead crates/claudefs-storage/src/qos_storage.rs
[0m→ [0mRead crates/claudefs-storage/src/io_scheduler.rs
[0m→ [0mRead crates/claudefs-storage/src/capacity.rs
[0m→ [0mRead crates/claudefs-storage/src/lib.rs
[0m
[0m← [0mWrite crates/claudefs-security/src/storage_qos_security_tests.rs
Wrote file successfully.

[0m[0m→ [0mRead crates/claudefs-security/src/lib.rs
[0m
Index: /home/cfs/claudefs/crates/claudefs-security/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-security/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-security/src/lib.rs
@@ -114,5 +114,8 @@
 #[allow(missing_docs)]
 pub mod fuse_cache_security_tests;
 #[cfg(test)]
 #[allow(missing_docs)]
-pub mod repl_infra_security_tests;
\ No newline at end of file
+pub mod repl_infra_security_tests;
+#[cfg(test)]
+#[allow(missing_docs)]
+pub mod storage_qos_security_tests;
\ No newline at end of file



[0m[0m← [0mEdit crates/claudefs-security/src/lib.rs[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security storage_qos_security_tests 2>&1 | head -100
[0mwarning: field `index_dir` is never read
   --> crates/claudefs-mgmt/src/indexer.rs:338:5
    |
334 | pub struct MetadataIndexer {
    |            --------------- field in this struct
...
338 |     index_dir: PathBuf,
    |     ^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: field `timeout` is never read
  --> crates/claudefs-mgmt/src/scraper.rs:77:5
   |
75 | pub struct NodeScraper {
   |            ----------- field in this struct
76 |     client: reqwest::Client,
77 |     timeout: Duration,
   |     ^^^^^^^

warning: field `capacity` is never read
   --> crates/claudefs-mgmt/src/events.rs:136:5
    |
133 | pub struct EventBus {
    |            -------- field in this struct
...
136 |     capacity: usize,
    |     ^^^^^^^^

warning: field `target_version` is never read
  --> crates/claudefs-mgmt/src/maintenance.rs:50:5
   |
49 | pub struct UpgradeCoordinator {
   |            ------------------ field in this struct
50 |     target_version: String,
   |     ^^^^^^^^^^^^^^
   |
   = note: `UpgradeCoordinator` has derived impls for the traits `Clone` and `Debug`, but these are intentionally ignored during dead code analysis

warning: missing documentation for an enum
 --> crates/claudefs-mgmt/src/alerting.rs:7:1
  |
7 | pub enum AlertError {
  | ^^^^^^^^^^^^^^^^^^^
  |
note: the lint level is defined here
 --> crates/claudefs-mgmt/src/lib.rs:7:9
  |
7 | #![warn(missing_docs)]
  |         ^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-mgmt/src/alerting.rs:9:5
  |
9 |     Evaluation(String),
  |     ^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-mgmt/src/alerting.rs:11:5
   |
11 |     Notification(String),
   |     ^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-mgmt/src/alerting.rs:15:1
   |
15 | pub enum AlertSeverity {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-mgmt/src/alerting.rs:16:5
   |
16 |     Info,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-mgmt/src/alerting.rs:17:5
   |
17 |     Warning,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-mgmt/src/alerting.rs:18:5
   |
18 |     Critical,
   |     ^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-mgmt/src/alerting.rs:22:1
   |
22 | pub enum AlertState {
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-mgmt/src/alerting.rs:23:5
   |
23 |     Ok,
   |     ^^

warning: missing documentation for a variant

[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security storage_qos_security_tests 2>&1 | tail -50


[0m|             ^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_status_at_soft`

warning: variable does not need to be mutable
   --> crates/claudefs-security/src/gateway_protocol_security_tests.rs:478:13
    |
478 |         let mut rule_empty = CorsRule::new();
    |             ----^^^^^^^^^^
    |             |
    |             help: remove this `mut`

warning: variable does not need to be mutable
   --> crates/claudefs-security/src/gateway_infra_security_tests.rs:320:13
    |
320 |         let mut rule_no_actions = LifecycleRule::new("rule-no-actions");
    |             ----^^^^^^^^^^^^^^^
    |             |
    |             help: remove this `mut`

warning: unused variable: `lease`
  --> crates/claudefs-security/src/fuse_cache_security_tests.rs:50:13
   |
50 |         let lease = manager.grant_lease(1, 100);
   |             ^^^^^ help: if this is intentional, prefix it with an underscore: `_lease`

warning: unused variable: `should_flush`
   --> crates/claudefs-security/src/fuse_cache_security_tests.rs:366:13
    |
366 |         let should_flush = buf.buffer_write(make_inode_id(1), 0, b"hello");
    |             ^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_should_flush`

warning: unused variable: `result2`
   --> crates/claudefs-security/src/fuse_cache_security_tests.rs:548:13
    |
548 |         let result2 = cache.insert(make_inode_id(2), vec![0u8; 60], 1);
    |             ^^^^^^^ help: if this is intentional, prefix it with an underscore: `_result2`

error[E0596]: cannot borrow `scheduler` as mutable, as it is not declared as mutable
   --> crates/claudefs-security/src/storage_qos_security_tests.rs:594:26
    |
594 |             let result = scheduler.dequeue();
    |                          ^^^^^^^^^ cannot borrow as mutable
    |
help: consider changing this to be mutable
    |
592 |             let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());
    |                 +++

For more information about this error, try `rustc --explain E0596`.
warning: `claudefs-security` (lib test) generated 150 warnings
error: could not compile `claudefs-security` (lib test) due to 1 previous error; 150 warnings emitted[0m→ [0mRead crates/claudefs-security/src/storage_qos_security_tests.rs[90m [offset=590, limit=10][0m
[0m[0m← [0mEdit crates/claudefs-security/src/storage_qos_security_tests.rs



[0mIndex: /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
+++ /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
@@ -588,9 +588,9 @@
 }
 
 #[test]
 fn test_io_scheduler_empty_dequeue() {
-    let scheduler = IoScheduler::new(IoSchedulerConfig::default());
+    let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());
 
     let result = scheduler.dequeue();
     assert!(
         result.is_none(),


warning: unused `Result` that must be used
   --> crates/claudefs-security/src/dos_resilience.rs:259:17
    |
259 |                 handle_message_type(msg_type);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
    = note: this `Result` may be an `Err` variant, which should be handled
help: use `let _ = ...` to ignore the resulting value
    |
259 |                 let _ = handle_message_type(msg_type);
    |                 +++++++

warning: comparison is useless due to type limits
   --> crates/claudefs-security/src/repl_deep_security_tests_v2.rs:350:17
    |
350 |         assert!(site1.stats().conflicts_resolved >= 0);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(unused_comparisons)]` on by default

warning: comparison is useless due to type limits
   --> crates/claudefs-security/src/repl_deep_security_tests_v2.rs:351:17
    |
351 |         assert!(site2.stats().conflicts_resolved >= 0);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-security` (lib test) generated 181 warnings (run `cargo fix --lib -p claudefs-security --tests` to apply 148 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.86s
     Running unittests src/lib.rs (target/debug/deps/claudefs_security-d966041db421f22c)

running 25 tests
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_normal ... ok
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_eviction_candidates ... ok
test storage_qos_security_tests::tests::config_defaults::test_capacity_tracker_zero_total ... ok
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions ... FAILED
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_eviction_trigger ... FAILED
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_segment_registration ... ok
test storage_qos_security_tests::tests::config_defaults::test_io_scheduler_config_defaults ... ok
test storage_qos_security_tests::tests::config_defaults::test_io_scheduler_empty_dequeue ... ok
test storage_qos_security_tests::tests::config_defaults::test_watermark_config_defaults ... ok
test storage_qos_security_tests::tests::config_defaults::test_qos_enforcer_reset_stats ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_dequeue_priority ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_drain_priority ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_inflight_tracking ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_priority_ordering ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_max_queue_depth ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_allow_within_limits ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_no_policy_rejects ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_remove_policy ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_stats_tracking ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_throttle_when_exceeded ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_bandwidth_tracker_current ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_qos_policy_default ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_token_bucket_consume ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_token_bucket_refill ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_workload_class_display ... ok

failures:

---- storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions stdout ----

thread 'storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions' (478558) panicked at crates/claudefs-security/src/storage_qos_security_tests.rs:418:13:
assertion `left == right` failed
  left: Normal
 right: High

---- storage_qos_security_tests::tests::capacity_watermarks::test_capacity_eviction_trigger stdout ----

thread 'storage_qos_security_tests::tests::capacity_watermarks::test_capacity_eviction_trigger' (478556) panicked at crates/claudefs-security/src/storage_qos_security_tests.rs:442:13:
Above high watermark should trigger eviction
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    storage_qos_security_tests::tests::capacity_watermarks::test_capacity_eviction_trigger
    storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions

test result: FAILED. 23 passed; 2 failed; 0 ignored; 0 measured; 1233 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p claudefs-security --lib`
[0m[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security storage_qos_security_tests 2>&1 | tail -80[0m→ [0mRead crates/claudefs-security/src/storage_qos_security_tests.rs[90m [offset=400, limit=70][0m
[0m

[0m← [0mEdit crates/claudefs-security/src/storage_qos_security_tests.rs
[0m
Index: /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
+++ /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
@@ -410,15 +410,15 @@
     ..Default::default()
 };
 let tracker = CapacityTracker::new(config, 1000);
 
-tracker.update_usage(79);
+tracker.update_usage(790);
 assert_eq!(tracker.level(), CapacityLevel::Normal);
 
-tracker.update_usage(81);
+tracker.update_usage(810);
 assert_eq!(tracker.level(), CapacityLevel::High);
 
-tracker.update_usage(96);
+tracker.update_usage(960);
 assert_eq!(tracker.level(), CapacityLevel::Critical);
 
 tracker.update_usage(1000);
 assert_eq!(tracker.level(), CapacityLevel::Full);
@@ -431,15 +431,15 @@
     ..Default::default()
 };
 let tracker = CapacityTracker::new(config, 1000);
 
-tracker.update_usage(79);
+tracker.update_usage(790);
 assert!(
     !tracker.should_evict(),
     "Below high watermark should not trigger eviction"
 );
 
-tracker.update_usage(81);
+tracker.update_usage(810);
 assert!(
     tracker.should_evict(),
     "Above high watermark should trigger eviction"
 );
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security storage_qos_security_tests 2>&1 | tail -40[0m


test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_segment_registration ... ok
test storage_qos_security_tests::tests::config_defaults::test_capacity_tracker_zero_total ... ok
test storage_qos_security_tests::tests::config_defaults::test_io_scheduler_config_defaults ... ok
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions ... FAILED
test storage_qos_security_tests::tests::config_defaults::test_io_scheduler_empty_dequeue ... ok
test storage_qos_security_tests::tests::config_defaults::test_watermark_config_defaults ... ok
test storage_qos_security_tests::tests::config_defaults::test_qos_enforcer_reset_stats ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_drain_priority ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_dequeue_priority ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_inflight_tracking ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_priority_ordering ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_max_queue_depth ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_no_policy_rejects ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_allow_within_limits ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_remove_policy ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_stats_tracking ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_throttle_when_exceeded ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_bandwidth_tracker_current ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_qos_policy_default ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_token_bucket_consume ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_token_bucket_refill ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_workload_class_display ... ok

failures:

---- storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions stdout ----

thread 'storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions' (479062) panicked at crates/claudefs-security/src/storage_qos_security_tests.rs:415:13:
assertion `left == right` failed
  left: Elevated
 right: Normal
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions

test result: FAILED. 24 passed; 1 failed; 0 ignored; 0 measured; 1233 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p claudefs-security --lib`
[0mIndex: /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
+++ /home/cfs/claudefs/crates/claudefs-security/src/storage_qos_security_tests.rs
@@ -406,15 +406,18 @@
 fn test_capacity_level_transitions() {
     let config = WatermarkConfig {
         high_watermark_pct: 80,
         critical_watermark_pct: 95,
-        ..Default::default()
+        low_watermark_pct: 60,
     };
     let tracker = CapacityTracker::new(config, 1000);
 
-    tracker.update_usage(790);
+    tracker.update_usage(500);
     assert_eq!(tracker.level(), CapacityLevel::Normal);
 
+    tracker.update_usage(700);
+    assert_eq!(tracker.level(), CapacityLevel::Elevated);
+
     tracker.update_usage(810);
     assert_eq!(tracker.level(), CapacityLevel::High);
 
     tracker.update_usage(960);

[0m← [0mEdit crates/claudefs-security/src/storage_qos_security_tests.rs
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security storage_qos_security_tests 2>&1 | tail -35
|                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-security` (lib test) generated 181 warnings (run `cargo fix --lib -p claudefs-security --tests` to apply 148 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.14s
     Running unittests src/lib.rs (target/debug/deps/claudefs_security-d966041db421f22c)

running 25 tests
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_eviction_trigger ... ok
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_normal ... ok
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_eviction_candidates ... ok
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_segment_registration ... ok
test storage_qos_security_tests::tests::capacity_watermarks::test_capacity_level_transitions ... ok
test storage_qos_security_tests::tests::config_defaults::test_capacity_tracker_zero_total ... ok
test storage_qos_security_tests::tests::config_defaults::test_io_scheduler_config_defaults ... ok
test storage_qos_security_tests::tests::config_defaults::test_io_scheduler_empty_dequeue ... ok
test storage_qos_security_tests::tests::config_defaults::test_watermark_config_defaults ... ok
test storage_qos_security_tests::tests::config_defaults::test_qos_enforcer_reset_stats ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_dequeue_priority ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_drain_priority ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_inflight_tracking ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_max_queue_depth ... ok
test storage_qos_security_tests::tests::io_scheduler::test_io_scheduler_priority_ordering ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_allow_within_limits ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_no_policy_rejects ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_remove_policy ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_stats_tracking ... ok
test storage_qos_security_tests::tests::qos_enforcer::test_qos_enforcer_throttle_when_exceeded ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_bandwidth_tracker_current ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_token_bucket_consume ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_qos_policy_default ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_token_bucket_refill ... ok
test storage_qos_security_tests::tests::qos_token_bucket::test_workload_class_display ... ok

test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 1233 filtered out; finished in 0.00s
[0m
[0m→ [0mRead crates/claudefs-security/src/storage_qos_security_tests.rs[90m [limit=30][0m
All 25 tests pass. The file has been created at `crates/claudefs-security/src/storage_qos_security_tests.rs` and is properly registered in `lib.rs`.
