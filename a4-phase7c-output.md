[0m
> build · accounts/fireworks/models/glm-5[0m


[0m→ [0mRead crates/claudefs-transport/src/lib.rs[0m→ [0mRead crates/claudefs-transport/src/wire_diag.rs
[0m→ [0mRead crates/claudefs-transport/src/credit_window.rs
I can see the structure of `lib.rs` and the types in both files. I need to:
1. Add `pub mod credit_window;` alphabetically between `connmigrate` and `deadline`
2. Add `pub mod wire_diag;` alphabetically between `version` and `zerocopy`
3. Add `pub use` re-exports at the end
[0m
[0m← [0mEdit crates/claudefs-transport/src/lib.rs
Index: /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
@@ -33,8 +33,9 @@
 pub mod congestion;
 pub mod conn_auth;
 pub mod connmigrate;
 pub mod connection;
+pub mod credit_window;
 pub mod deadline;
 pub mod discovery;
 pub mod drain;
 pub mod endpoint_registry;

[0m
[0m← [0mEdit crates/claudefs-transport/src/lib.rsIndex: /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
@@ -78,8 +78,9 @@
 pub mod request_dedup;
 pub mod tracecontext;
 pub mod transport;
 pub mod version;
+pub mod wire_diag;
 pub mod zerocopy;
 
 pub use batch::{
     BatchConfig, BatchCollector, BatchEnvelope, BatchItem, BatchRequest, BatchResponse,


[0m
[0m← [0mEdit crates/claudefs-transport/src/lib.rsIndex: /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
@@ -213,4 +213,12 @@
 pub use multicast_group::{
     BroadcastResult, GroupEvent, GroupId, GroupMember, MulticastError, MulticastGroupConfig,
     MulticastGroupManager, MulticastGroupStats, MulticastGroupStatsSnapshot,
 };
+pub use wire_diag::{
+    InFlightPing, RttSample, RttSeries, RttSeriesSnapshot, TraceHop, TracePath,
+    WireDiag, WireDiagConfig, WireDiagStats, WireDiagStatsSnapshot,
+};
+pub use credit_window::{
+    CreditGrant, CreditWindow, CreditWindowConfig, CreditWindowState,
+    CreditWindowStats, CreditWindowStatsSnapshot,
+};



[0mNow let me build and test:
[0m$ [0mcargo build -p claudefs-transport 2>&1Blocking waiting for file lock on artifact directory
   Compiling claudefs-transport v0.1.0 (/home/cfs/claudefs/crates/claudefs-transport)
error[E0382]: use of moved value: `config`
   --> crates/claudefs-transport/src/wire_diag.rs:202:51
    |
197 |     pub fn new(config: WireDiagConfig) -> Self {
    |                ------ move occurs because `config` has type `WireDiagConfig`, which does not implement the `Copy` trait
198 |         Self {
199 |             config,
    |             ------ value moved here
...
202 |             rtt_series: Mutex::new(RttSeries::new(config.window_size)),
    |                                                   ^^^^^^^^^^^^^^^^^^ value used here after move
    |
help: consider cloning the value if the performance cost is acceptable
    |
199 |             config: config.clone(),
    |                   ++++++++++++++++

For more information about this error, try `rustc --explain E0382`.
error: could not compile `claudefs-transport` (lib) due to 1 previous error
[0m

There's a compile error in `wire_diag.rs`. I need to fix the move order issue:
[0m← [0mEdit crates/claudefs-transport/src/wire_diag.rsIndex: /home/cfs/claudefs/crates/claudefs-transport/src/wire_diag.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/wire_diag.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/wire_diag.rs
@@ -194,13 +194,14 @@
 use std::collections::HashMap;
 
 impl WireDiag {
     pub fn new(config: WireDiagConfig) -> Self {
+        let rtt_series = Mutex::new(RttSeries::new(config.window_size));
         Self {
             config,
             next_seq: AtomicU64::new(0),
             inflight: Mutex::new(HashMap::new()),
-            rtt_series: Mutex::new(RttSeries::new(config.window_size)),
+            rtt_series,
             stats: Arc::new(WireDiagStats::new()),
         }
     }
 

[0m


[0m$ [0mcargo build -p claudefs-transport 2>&1
Blocking waiting for file lock on artifact directory
   Compiling claudefs-transport v0.1.0 (/home/cfs/claudefs/crates/claudefs-transport)
warning: field `config` is never read
  --> crates/claudefs-transport/src/timer_wheel.rs:80:5
   |
79 | pub struct TimerWheel {
   |            ---------- field in this struct
80 |     config: TimerWheelConfig,
   |     ^^^^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:49:5
   |
49 |     pub grants_issued: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
note: the lint level is defined here
  --> crates/claudefs-transport/src/lib.rs:1:9
   |
 1 | #![warn(missing_docs)]
   |         ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:50:5
   |
50 |     pub grants_denied: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:51:5
   |
51 |     pub credits_granted: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:52:5
   |
52 |     pub credits_returned: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:53:5
   |
53 |     pub throttle_events: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:54:5
   |
54 |     pub exhaustion_events: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/credit_window.rs:69:5
   |
69 | /     pub fn snapshot(
70 | |         &self,
71 | |         available: u64,
72 | |         total: u64,
73 | |         state: CreditWindowState,
74 | |     ) -> CreditWindowStatsSnapshot {
   | |__________________________________^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:92:5
   |
92 |     pub grants_issued: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:93:5
   |
93 |     pub grants_denied: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:94:5
   |
94 |     pub credits_granted: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:95:5
   |
95 |     pub credits_returned: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:96:5
   |
96 |     pub throttle_events: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:97:5
   |
97 |     pub exhaustion_events: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:98:5
   |
98 |     pub available_credits: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:99:5
   |
99 |     pub total_credits: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/credit_window.rs:100:5
    |
100 |     pub state: CreditWindowState,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/credit_window.rs:158:5
    |
158 |     pub fn new(config: CreditWindowConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:123:5
    |
123 |     pub hops: Vec<TraceHop>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:133:5
    |
133 |     pub seq: u64,
    |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:134:5
    |
134 |     pub sent_at_ms: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:135:5
    |
135 |     pub timeout_ms: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:140:5
    |
140 |     pub pings_sent: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:141:5
    |
141 |     pub pongs_received: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:142:5
    |
142 |     pub pings_timed_out: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:143:5
    |
143 |     pub pings_rejected: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/wire_diag.rs:156:5
    |
156 |     pub fn snapshot(&self) -> WireDiagStatsSnapshot {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:169:5
    |
169 |     pub pings_sent: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:170:5
    |
170 |     pub pongs_received: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:171:5
    |
171 |     pub pings_timed_out: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:172:5
    |
172 |     pub pings_rejected: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:178:5
    |
178 |     pub sample_count: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:179:5
    |
179 |     pub min_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:180:5
    |
180 |     pub max_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:181:5
    |
181 |     pub mean_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:182:5
    |
182 |     pub p99_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/wire_diag.rs:197:5
    |
197 |     pub fn new(config: WireDiagConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-transport` (lib) generated 37 warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.74s[0m
Build succeeded (warnings are pre-existing doc issues). Now running tests:
[0m$ [0mcargo test -p claudefs-transport 2>&1

Compiling claudefs-transport v0.1.0 (/home/cfs/claudefs/crates/claudefs-transport)
warning: unused import: `TopologyStatsSnapshot`
   --> crates/claudefs-transport/src/cluster_topology.rs:282:74
    |
282 |         ClusterTopology, DatacenterId, Proximity, RackId, TopologyLabel, TopologyStatsSnapshot,
    |                                                                          ^^^^^^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `FaultInjectorStatsSnapshot`
   --> crates/claudefs-transport/src/fault_inject.rs:342:69
    |
342 |         corrupt_payload, ConnectAction, FaultConfig, FaultInjector, FaultInjectorStatsSnapshot,
    |                                                                     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `SpanStatus`
   --> crates/claudefs-transport/src/otel.rs:391:82
    |
391 |         self, Attribute, AttributeValue, EventSeverity, Span, SpanEvent, SpanId, SpanStatus,
    |                                                                                  ^^^^^^^^^^

warning: unused variable: `expected`
   --> crates/claudefs-transport/src/adaptive.rs:503:13
    |
503 |         let expected = p99 * 1;
    |             ^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_expected`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `min_limit`
   --> crates/claudefs-transport/src/adaptive.rs:504:13
    |
504 |         let min_limit = 200 * 1000;
    |             ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_min_limit`

warning: field `config` is never read
  --> crates/claudefs-transport/src/timer_wheel.rs:80:5
   |
79 | pub struct TimerWheel {
   |            ---------- field in this struct
80 |     config: TimerWheelConfig,
   |     ^^^^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:49:5
   |
49 |     pub grants_issued: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
note: the lint level is defined here
  --> crates/claudefs-transport/src/lib.rs:1:9
   |
 1 | #![warn(missing_docs)]
   |         ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:50:5
   |
50 |     pub grants_denied: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:51:5
   |
51 |     pub credits_granted: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:52:5
   |
52 |     pub credits_returned: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:53:5
   |
53 |     pub throttle_events: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:54:5
   |
54 |     pub exhaustion_events: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/credit_window.rs:69:5
   |
69 | /     pub fn snapshot(
70 | |         &self,
71 | |         available: u64,
72 | |         total: u64,
73 | |         state: CreditWindowState,
74 | |     ) -> CreditWindowStatsSnapshot {
   | |__________________________________^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:92:5
   |
92 |     pub grants_issued: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:93:5
   |
93 |     pub grants_denied: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:94:5
   |
94 |     pub credits_granted: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:95:5
   |
95 |     pub credits_returned: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:96:5
   |
96 |     pub throttle_events: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:97:5
   |
97 |     pub exhaustion_events: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:98:5
   |
98 |     pub available_credits: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:99:5
   |
99 |     pub total_credits: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/credit_window.rs:100:5
    |
100 |     pub state: CreditWindowState,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/credit_window.rs:158:5
    |
158 |     pub fn new(config: CreditWindowConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused variable: `payload`
   --> crates/claudefs-transport/src/compress.rs:477:13
    |
477 |         let payload = compressor.compress(&data);
    |             ^^^^^^^ help: if this is intentional, prefix it with an underscore: `_payload`

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:123:5
    |
123 |     pub hops: Vec<TraceHop>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:133:5
    |
133 |     pub seq: u64,
    |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:134:5
    |
134 |     pub sent_at_ms: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:135:5
    |
135 |     pub timeout_ms: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:140:5
    |
140 |     pub pings_sent: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:141:5
    |
141 |     pub pongs_received: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:142:5
    |
142 |     pub pings_timed_out: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:143:5
    |
143 |     pub pings_rejected: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/wire_diag.rs:156:5
    |
156 |     pub fn snapshot(&self) -> WireDiagStatsSnapshot {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:169:5
    |
169 |     pub pings_sent: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:170:5
    |
170 |     pub pongs_received: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:171:5
    |
171 |     pub pings_timed_out: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:172:5
    |
172 |     pub pings_rejected: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:178:5
    |
178 |     pub sample_count: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:179:5
    |
179 |     pub min_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:180:5
    |
180 |     pub max_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:181:5
    |
181 |     pub mean_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:182:5
    |
182 |     pub p99_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/wire_diag.rs:197:5
    |
197 |     pub fn new(config: WireDiagConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused variable: `receiver`
   --> crates/claudefs-transport/src/stream.rs:776:13
    |
776 |         let receiver = manager.create_receiver(stream_id).unwrap();
    |             ^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_receiver`

warning: unused variable: `i`
   --> crates/claudefs-transport/src/zerocopy.rs:425:13
    |
425 |         for i in 0..100 {
    |             ^ help: if this is intentional, prefix it with an underscore: `_i`

warning: `claudefs-transport` (lib) generated 37 warnings
warning: comparison is useless due to type limits
   --> crates/claudefs-transport/src/server.rs:364:17
    |
364 |         assert!(snap.requests_received >= 0);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(unused_comparisons)]` on by default

warning: comparison is useless due to type limits
   --> crates/claudefs-transport/src/server.rs:365:17
    |
365 |         assert!(snap.responses_sent >= 0);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-transport` (lib test) generated 47 warnings (37 duplicates) (run `cargo fix --lib -p claudefs-transport --tests` to apply 8 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 7.44s
     Running unittests src/lib.rs (target/debug/deps/claudefs_transport-6432c7de32d8aec5)

running 1070 tests
test adaptive::tests::test_adaptive_adjust_decreases_timeout ... ok
test adaptive::tests::test_adaptive_adjust_increases_timeout ... ok
test adaptive::tests::test_adaptive_disabled ... ok
test adaptive::tests::test_adaptive_initial_timeout ... ok
test adaptive::tests::test_adaptive_max_timeout ... ok
test adaptive::tests::test_adaptive_safety_margin ... ok
test adaptive::tests::test_config_default ... ok
test adaptive::tests::test_adaptive_record_latency ... ok
test adaptive::tests::test_histogram_empty ... ok
test adaptive::tests::test_histogram_percentile_p50 ... ok
test adaptive::tests::test_adaptive_record_timeout ... ok
test adaptive::tests::test_histogram_reset ... ok
test adaptive::tests::test_histogram_sample_count ... ok
test adaptive::tests::test_histogram_snapshot ... ok
test adaptive::tests::test_histogram_window_size ... ok
test adaptive::tests::test_percentile_snapshot_ordering ... ok
test backpressure::tests::test_config_default ... ok
test backpressure::tests::test_disabled_monitor ... ok
test backpressure::tests::test_monitor_combined_pressure ... ok
test backpressure::tests::test_monitor_initial_state ... ok
test backpressure::tests::test_monitor_is_overloaded ... ok
test backpressure::tests::test_monitor_level_critical ... ok
test backpressure::tests::test_monitor_level_high ... ok
test backpressure::tests::test_monitor_level_low ... ok
test backpressure::tests::test_monitor_level_medium ... ok
test backpressure::tests::test_monitor_level_none ... ok
test backpressure::tests::test_monitor_memory_pressure ... ok
test backpressure::tests::test_monitor_queue_pressure ... ok
test backpressure::tests::test_monitor_stats ... ok
test backpressure::tests::test_pressure_level_ordering ... ok
test backpressure::tests::test_signal_construction ... ok
test backpressure::tests::test_throttle_decrease_on_pressure ... ok
test backpressure::tests::test_throttle_increase_on_recovery ... ok
test backpressure::tests::test_throttle_initial_rate ... ok
test backpressure::tests::test_throttle_min_rate_floor ... ok
test backpressure::tests::test_throttle_max_rate_ceiling ... ok
test backpressure::tests::test_throttle_reset ... ok
test bandwidth::tests::test_burst_factor ... ok
test adaptive::tests::test_histogram_single_sample ... ok
test bandwidth::tests::test_config_default ... ok
test adaptive::tests::test_histogram_percentile_p99 ... ok
test bandwidth::tests::test_exceed_global_limit ... ok
test bandwidth::tests::test_exceed_tenant_limit_shaping ... ok
test adaptive::tests::test_adaptive_stats ... ok
test bandwidth::tests::test_exceed_tenant_limit_strict ... ok
test bandwidth::tests::test_global_usage_bps ... ok
test bandwidth::tests::test_monitor_mode ... ok
test bandwidth::tests::test_multiple_tenants ... ok
test bandwidth::tests::test_new_tenant_auto_created ... ok
test bandwidth::tests::test_peak_bps_tracking ... ok
test bandwidth::tests::test_set_tenant_limit ... ok
test bandwidth::tests::test_tenant_usage_bps ... ok
test bandwidth::tests::test_window_reset ... ok
test bandwidth::tests::test_within_limit_allowed ... ok
test batch::tests::test_batch_config_default ... ok
test batch::tests::test_batch_envelope_empty ... ok
test bandwidth::tests::test_stats_snapshot ... ok
test batch::tests::test_batch_envelope_request_batch ... ok
test batch::tests::test_batch_envelope_total_bytes ... ok
test batch::tests::test_batch_item_payload_size ... ok
test batch::tests::test_batch_envelope_encode_decode ... ok
test batch::tests::test_batch_request_new ... ok
test batch::tests::test_batch_response_error ... ok
test batch::tests::test_batch_response_new ... ok
test batch::tests::test_batch_envelope_len ... ok
test batch::tests::test_batch_envelope_response_batch ... ok
test batch::tests::test_collector_add_buffered ... ok
test batch::tests::test_collector_add_triggers_ready ... ok
test batch::tests::test_batch_response_success ... ok
test batch::tests::test_batch_stats_snapshot ... ok
test batch::tests::test_collector_flush_empty ... ok
test batch::tests::test_collector_byte_limit ... ok
test batch::tests::test_collector_pending_count ... ok
test batch::tests::test_collector_stats ... ok
test batch::tests::test_collector_disabled ... ok
test batch::tests::test_collector_flush_pending ... ok
test adaptive::tests::test_adaptive_min_timeout ... ok
test backpressure::tests::test_monitor_throughput_pressure ... ok
test bulk_transfer::tests::test_ack_all_chunks_completes_job ... ok
test bulk_transfer::tests::test_ack_completes_chunk ... ok
test bulk_transfer::tests::test_ack_non_in_flight_error ... ok
test bulk_transfer::tests::test_ack_unknown_chunk_error ... ok
test bulk_transfer::tests::test_ack_wrong_job_id ... ok
test bulk_transfer::tests::test_chunk_offsets_contiguous ... ok
test bulk_transfer::tests::test_default_config_values ... ok
test bulk_transfer::tests::test_empty_targets_uses_default_node ... ok
test bulk_transfer::tests::test_fatal_failures_list ... ok
test bulk_transfer::tests::test_has_fatal_failure_false_initially ... ok
test bulk_transfer::tests::test_has_fatal_failure_true_after_max_retries ... ok
test bulk_transfer::tests::test_is_complete_false_initially ... ok
test bulk_transfer::tests::test_is_complete_true_when_all_acked ... ok
test bulk_transfer::tests::test_large_job_1000_chunks ... ok
test bulk_transfer::tests::test_last_chunk_correct_size ... ok
test bulk_transfer::tests::test_max_in_flight_8 ... ok
test bulk_transfer::tests::test_multiple_targets_distribution ... ok
test bulk_transfer::tests::test_nack_max_retries_fails_permanently ... ok
test bulk_transfer::tests::test_nack_on_failed_chunk_returns_error ... ok
test bulk_transfer::tests::test_nack_preserves_retry_count_on_pending ... ok
test bulk_transfer::tests::test_nack_retries ... ok
test bulk_transfer::tests::test_nack_then_resend ... ok
test bulk_transfer::tests::test_nack_unknown_chunk_error ... ok
test bulk_transfer::tests::test_new_job_exact_chunks ... ok
test bulk_transfer::tests::test_new_job_partial_last_chunk ... ok
test bulk_transfer::tests::test_new_job_single_chunk ... ok
test bulk_transfer::tests::test_next_to_send_empty_when_all_in_flight ... ok
test bulk_transfer::tests::test_next_to_send_respects_max_in_flight ... ok
test bulk_transfer::tests::test_next_to_send_returns_pending_only ... ok
test bulk_transfer::tests::test_partial_acknowledgement_stats ... ok
test bulk_transfer::tests::test_retry_flow_end_to_end ... ok
test bulk_transfer::tests::test_round_robin_target_assignment ... ok
test bulk_transfer::tests::test_stats_bytes_acked_increments ... ok
test bulk_transfer::tests::test_stats_bytes_total ... ok
test bulk_transfer::tests::test_stats_chunks_in_flight ... ok
test bulk_transfer::tests::test_stats_failed_chunks ... ok
test bulk_transfer::tests::test_stats_retries_total ... ok
test bulk_transfer::tests::test_stats_total_chunks ... ok
test bulk_transfer::tests::test_zero_byte_transfer ... ok
test cancel::tests::test_cancel_handle_cancels_token ... ok
test cancel::tests::test_cancel_handle_is_cancelled ... ok
test cancel::tests::test_cancel_reason ... ok
test cancel::tests::test_cancel_reason_none_initially ... ok
test cancel::tests::test_cancel_token_clone ... ok
test cancel::tests::test_cancel_token_new ... ok
test cancel::tests::test_cancel_with_each_reason ... ok
test cancel::tests::test_child_token_cancelled_by_parent ... ok
test cancel::tests::test_child_token_cancelled_independently ... ok
test cancel::tests::test_double_cancel ... ok
test cancel::tests::test_multiple_children ... ok
test cancel::tests::test_parent_not_cancelled_by_child ... ok
test cancel::tests::test_registry_cancel ... ok
test cancel::tests::test_registry_cancel_all ... ok
test cancel::tests::test_registry_new ... ok
test cancel::tests::test_registry_register ... ok
test cancel::tests::test_registry_remove ... ok
test cancel::tests::test_registry_stats ... ok
test circuitbreaker::tests::config_default ... ok
test buffer::tests::test_buffer_pool_grows ... ok
test buffer::tests::test_buffer_pool_max_count ... ok
test buffer::tests::test_pooled_buffer_deref ... ok
test circuitbreaker::tests::initial_closed ... ok
test buffer::tests::test_buffer_pool_stats ... ok
test circuitbreaker::tests::reset ... ok
test circuitbreaker::tests::success_resets_failures ... ok
test circuitbreaker::tests::trip_open ... ok
test client::tests::test_client_accessors ... ok
test client::tests::test_client_circuit_breaker_integration ... ok
test client::tests::test_client_debug_format ... ok
test client::tests::test_client_flow_control_blocked ... ok
test client::tests::test_client_health_failed ... ok
test client::tests::test_client_is_available ... ok
test client::tests::test_client_metrics_tracking ... ok
test client::tests::test_client_post_failure ... ok
test client::tests::test_client_post_success ... ok
test client::tests::test_client_pre_request ... ok
test client::tests::test_transport_client_config_default ... ok
test cluster_topology::tests::test_add_remove_node ... ok
test cluster_topology::tests::test_datacenter_id ... ok
test cluster_topology::tests::test_datacenters ... ok
test cluster_topology::tests::test_empty_topology ... ok
test cluster_topology::tests::test_large_topology ... ok
test cluster_topology::tests::test_proximity_node_not_in_topology ... ok
test cluster_topology::tests::test_proximity_ordering ... ok
test cluster_topology::tests::test_proximity_remote_dc ... ok
test cluster_topology::tests::test_proximity_same_dc_different_rack ... ok
test cluster_topology::tests::test_proximity_same_node ... ok
test cluster_topology::tests::test_proximity_same_rack ... ok
test cluster_topology::tests::test_rack_id ... ok
test cluster_topology::tests::test_racks_in_datacenter ... ok
test cluster_topology::tests::test_remote_dc_peers ... ok
test cluster_topology::tests::test_remote_dc_peers_all_same_dc ... ok
test cluster_topology::tests::test_same_dc_cross_rack_peers ... ok
test cluster_topology::tests::test_same_rack_peers ... ok
test cluster_topology::tests::test_same_rack_peers_empty ... ok
test cluster_topology::tests::test_same_rack_peers_unknown_node ... ok
test cluster_topology::tests::test_single_node_topology ... ok
test cluster_topology::tests::test_sorted_by_proximity_deterministic_ties ... ok
test cluster_topology::tests::test_sorted_by_proximity_multi_tier ... ok
test cluster_topology::tests::test_sorted_by_proximity_single_node ... ok
test cluster_topology::tests::test_sorted_by_proximity_unknown_origin ... ok
test cluster_topology::tests::test_stats ... ok
test cluster_topology::tests::test_topology_label ... ok
test compress::tests::test_compress_below_min_size ... ok
test compress::tests::test_compress_disabled ... ok
test compress::tests::test_compress_none ... ok
test compress::tests::test_compressed_payload_encode ... ok
test compress::tests::test_compression_algorithm_values ... ok
test compress::tests::test_compression_config_default ... ok
test compress::tests::test_compressor_stats ... ok
test compress::tests::test_compressor_stats_ratio ... ok
test compress::tests::test_decompress_invalid ... ok
test compress::tests::test_rle_compress_alternating ... ok
test compress::tests::test_rle_compress_empty ... ok
test compress::tests::test_rle_compress_escape_byte ... ok
test compress::tests::test_rle_compress_random ... ok
test compress::tests::test_rle_compress_uniform ... ok
test compress::tests::test_rle_roundtrip ... ok
test congestion::tests::test_aimd_congestion_avoidance ... ok
test congestion::tests::test_aimd_loss_handling ... ok
test congestion::tests::test_available_window ... ok
test congestion::tests::test_bbr_bandwidth_estimation ... ok
test congestion::tests::test_bytes_tracking ... ok
test congestion::tests::test_config_default ... ok
test congestion::tests::test_cubic_loss_handling ... ok
test congestion::tests::test_initial_state ... ok
test congestion::tests::test_loss_events_count ... ok
test congestion::tests::test_recovery_to_avoidance ... ok
test congestion::tests::test_rtt_smoothing ... ok
test congestion::tests::test_slow_start_growth ... ok
test congestion::tests::test_slow_start_to_congestion_avoidance ... ok
test congestion::tests::test_stats_snapshot ... ok
test congestion::tests::test_window_bounds ... ok
test conn_auth::tests::test_allowed_subjects_filter ... ok
test conn_auth::tests::test_auth_level_none_allows_all ... ok
test conn_auth::tests::test_cert_too_old ... ok
test conn_auth::tests::test_cluster_ca_validation ... ok
test conn_auth::tests::test_config_default ... ok
test conn_auth::tests::test_expired_cert_denied ... ok
test conn_auth::tests::test_mutual_tls_allows_valid_cert ... ok
test conn_auth::tests::test_not_yet_valid_cert_denied ... ok
test conn_auth::tests::test_revocation_list_operations ... ok
test conn_auth::tests::test_revoked_fingerprint_denied ... ok
test conn_auth::tests::test_revoked_serial_denied ... ok
test conn_auth::tests::test_stats_tracking ... ok
test conn_auth::tests::test_strict_mode_fingerprint_check ... ok
test connection::tests::test_pool_stats ... ok
test connmigrate::tests::test_active_count ... ok
test connmigrate::tests::test_complete_migration ... ok
test connmigrate::tests::test_complete_removes_from_active ... ok
test connmigrate::tests::test_config_default ... ok
test connmigrate::tests::test_connection_id ... ok
test connmigrate::tests::test_fail_migration ... ok
test connmigrate::tests::test_get_migration ... ok
test connmigrate::tests::test_get_nonexistent_migration ... ok
test connmigrate::tests::test_is_migrating ... ok
test connmigrate::tests::test_manager_initial_state ... ok
test connmigrate::tests::test_migration_reason_values ... ok
test connmigrate::tests::test_migration_record_fields ... ok
test connmigrate::tests::test_migration_state_values ... ok
test connmigrate::tests::test_record_request_failed ... ok
test connmigrate::tests::test_record_request_migrated ... ok
test connmigrate::tests::test_start_migration ... ok
test connmigrate::tests::test_start_migration_already_migrating ... ok
test connmigrate::tests::test_start_migration_disabled ... ok
test connmigrate::tests::test_start_migration_returns_unique_ids ... ok
test connmigrate::tests::test_start_migration_too_many ... ok
test connmigrate::tests::test_stats ... ok
test credit_window::tests::test_multiple_acquisitions_sum ... ok
test credit_window::tests::test_new_default_config ... ok
test credit_window::tests::test_return_credits ... ok
test credit_window::tests::test_return_credits_no_overflow ... ok
test credit_window::tests::test_state_exhausted ... ok
test credit_window::tests::test_state_normal ... ok
test credit_window::tests::test_state_throttled ... ok
test credit_window::tests::test_state_transitions ... FAILED
test credit_window::tests::test_state_warning ... ok
test credit_window::tests::test_stats_counts ... ok
test credit_window::tests::test_try_acquire_exact_total ... ok
test credit_window::tests::test_try_acquire_exhausts_window ... ok
test credit_window::tests::test_try_acquire_explicit_release ... ok
test credit_window::tests::test_try_acquire_max_single_alloc ... ok
test credit_window::tests::test_try_acquire_over_total ... ok
test credit_window::tests::test_try_acquire_returns_on_drop ... ok
test credit_window::tests::test_try_acquire_success ... ok
test credit_window::tests::test_utilization ... ok
test deadline::tests::test_deadline_context_check_expired ... ok
test deadline::tests::test_deadline_context_check_ok ... ok
test deadline::tests::test_deadline_context_no_deadline ... ok
test deadline::tests::test_deadline_context_with_timeout ... ok
test deadline::tests::test_deadline_expired ... ok
test deadline::tests::test_deadline_new ... ok
test buffer::tests::test_different_buffer_sizes ... ok
test deadline::tests::test_encode_decode_no_deadline ... ok
test deadline::tests::test_encode_decode_roundtrip ... ok
test discovery::tests::test_add_duplicate_member ... ok
test discovery::tests::test_add_member ... ok
test discovery::tests::test_alive_count ... ok
test discovery::tests::test_alive_members ... ok
test discovery::tests::test_all_members ... ok
test discovery::tests::test_discovery_config_default ... ok
test discovery::tests::test_event_listener ... ok
test discovery::tests::test_get_member ... ok
test discovery::tests::test_increment_incarnation ... ok
test discovery::tests::test_mark_alive_no_change ... ok
test discovery::tests::test_mark_alive_refutation ... ok
test discovery::tests::test_mark_dead ... ok
test discovery::tests::test_mark_suspected ... ok
test discovery::tests::test_member_count ... ok
test discovery::tests::test_member_info_creation ... ok
test discovery::tests::test_membership_event_variants ... ok
test discovery::tests::test_membership_list_new ... ok
test discovery::tests::test_node_state_values ... ok
test discovery::tests::test_remove_member ... ok
test discovery::tests::test_remove_nonexistent ... ok
test discovery::tests::test_stats ... ok
test drain::tests::test_begin_drain_idempotent ... ok
test drain::tests::test_begin_drain_transitions ... ok
test drain::tests::test_check_drained_with_inflight ... ok
test drain::tests::test_check_drained_with_zero_inflight ... ok
test drain::tests::test_concurrent_acquire_release ... ok
test drain::tests::test_drain_config_default ... ok
test drain::tests::test_drain_from_drained_state ... ok
test drain::tests::test_drain_from_force_closed_state ... ok
test drain::tests::test_drain_guard_decrements ... ok
test drain::tests::test_drain_stats ... ok
test deadline::tests::test_deadline_remaining ... ok
test drain::tests::test_force_close ... ok
test drain::tests::test_inflight_count_tracking ... ok
test drain::tests::test_initial_state_active ... ok
test drain::tests::test_is_accepting_when_active ... ok
test drain::tests::test_is_not_accepting_when_draining ... ok
test drain::tests::test_listener_callbacks ... ok
test drain::tests::test_reset_to_active ... ok
test drain::tests::test_stats_after_force_close ... ok
test drain::tests::test_try_acquire_when_active ... ok
test drain::tests::test_try_acquire_when_draining ... ok
test endpoint_registry::tests::test_concurrent_multiple_nodes ... ok
test endpoint_registry::tests::test_concurrent_register_and_resolve ... ok
test endpoint_registry::tests::test_default_config_values ... ok
test endpoint_registry::tests::test_empty_registry_known_nodes ... ok
test endpoint_registry::tests::test_empty_registry_stats ... ok
test drain::tests::test_elapsed_since_drain ... ok
test endpoint_registry::tests::test_evict_expired_clears_entries ... ok
test endpoint_registry::tests::test_evict_expired_returns_count ... ok
test endpoint_registry::tests::test_gossip_entry_with_future_expiry_resolves ... ok
test endpoint_registry::tests::test_expired_gossip_entry_is_evicted ... ok
test endpoint_registry::tests::test_gossip_overwrites_previous ... ok
test endpoint_registry::tests::test_gossip_preference_defaults_to_tcp_only ... ok
test buffer::tests::test_buffer_pool_basic ... ok
test buffer::tests::test_pooled_buffer_auto_return ... ok
test endpoint_registry::tests::test_max_entries_limit ... ok
test endpoint_registry::tests::test_multiple_addrs_per_node ... ok
test endpoint_registry::tests::test_multiple_addrs_rdma_first ... ok
test endpoint_registry::tests::test_overwrite_static_with_register_static ... ok
test endpoint_registry::tests::test_rdma_first_falls_back_to_tcp ... ok
test endpoint_registry::tests::test_rdma_first_returns_rdma_before_tcp ... ok
test endpoint_registry::tests::test_rdma_first_with_no_rdma_returns_tcp ... ok
test endpoint_registry::tests::test_remove_clears_entry ... ok
test endpoint_registry::tests::test_remove_returns_false_for_unknown ... ok
test endpoint_registry::tests::test_remove_returns_true_for_known ... ok
test endpoint_registry::tests::test_remove_updates_static_entries_count ... ok
test endpoint_registry::tests::test_resolve_all_empty_for_unknown ... ok
test endpoint_registry::tests::test_resolve_all_ordering_with_rdma_first ... ok
test endpoint_registry::tests::test_resolve_unknown_returns_none ... ok
test endpoint_registry::tests::test_gossip_entry_with_past_expiry_returns_none ... ok
test endpoint_registry::tests::test_static_registration_and_resolve ... ok
test endpoint_registry::tests::test_stats_entries ... ok
test endpoint_registry::tests::test_stats_hits ... ok
test endpoint_registry::tests::test_stats_misses ... ok
test circuitbreaker::tests::halfopen_max_requests ... ok
test endpoint_registry::tests::test_stats_static_entries ... ok
test endpoint_registry::tests::test_tcp_only_filters_all_rdma ... ok
test endpoint_registry::tests::test_tcp_only_filters_rdma ... ok
test enrollment::tests::test_ca_and_service_integration ... ok
test enrollment::tests::test_ca_cert_pem_valid ... ok
test enrollment::tests::test_ca_fingerprint_non_empty_hex ... ok
test enrollment::tests::test_certificate_bundle_fields ... ok
test enrollment::tests::test_certificate_bundle_serialization ... ok
test enrollment::tests::test_cluster_ca_creation ... ok
test enrollment::tests::test_config_defaults ... ok
test enrollment::tests::test_config_serialization ... ok
test enrollment::tests::test_different_node_tokens ... ok
test enrollment::tests::test_enroll_with_already_used_token ... ok
test enrollment::tests::test_enroll_with_expired_token ... ok
test enrollment::tests::test_enroll_with_invalid_token ... ok
test enrollment::tests::test_enroll_with_valid_token ... ok
test enrollment::tests::test_enrollment_service_creation ... ok
test circuitbreaker::tests::halfopen_to_closed ... ok
test circuitbreaker::tests::halfopen_to_open ... ok
test enrollment::tests::test_enrollment_token_serialization ... ok
test enrollment::tests::test_generate_enrollment_token ... ok
test enrollment::tests::test_get_crl_entries ... ok
test enrollment::tests::test_is_revoked ... ok
test enrollment::tests::test_enrollment_stats_tracking ... ok
test enrollment::tests::test_max_tokens_per_node_exceeded ... ok
test enrollment::tests::test_issue_client_certificate ... ok
test endpoint_registry::tests::test_known_nodes_excludes_expired ... ok
test circuitbreaker::tests::open_to_halfopen ... ok
test enrollment::tests::test_revocation_entry_serialization ... ok
test enrollment::tests::test_issue_node_certificate ... ok
test enrollment::tests::test_revocation_reason_copy ... ok
test enrollment::tests::test_revocation_reasons ... ok
test endpoint_registry::tests::test_resolve_all_returns_none_when_expired ... ok
test enrollment::tests::test_multiple_enrollments ... ok
test enrollment::tests::test_service_issue_client_cert ... ok
test fault_inject::tests::test_connect_action_equality ... ok
test enrollment::tests::test_token_has_valid_expiry ... ok
test enrollment::tests::test_token_expiry_validation ... ok
test fault_inject::tests::test_corrupt_payload_empty ... ok
test fault_inject::tests::test_corrupt_payload_flips_msb ... ok
test enrollment::tests::test_service_issue_node_cert ... ok
test fault_inject::tests::test_disabled_injector_always_allows ... ok
test fault_inject::tests::test_corrupt_payload_single_byte ... ok
test fault_inject::tests::test_default_config_disabled ... ok
test fault_inject::tests::test_connect_ignores_non_reset_faults ... ok
test fault_inject::tests::test_fault_kind_equality ... ok
test fault_inject::tests::test_fault_spec_fires_always ... ok
test fault_inject::tests::test_fault_spec_fires_never ... ok
test fault_inject::tests::test_fault_spec_fires_partial ... ok
test fault_inject::tests::test_is_enabled ... ok
test fault_inject::tests::test_multiple_fault_specs_first_wins ... ok
test enrollment::tests::test_revoke_certificate ... ok
test fault_inject::tests::test_on_connect_reset_probability_1 ... ok
test fault_inject::tests::test_on_recv_drop_probability_1 ... ok
test fault_inject::tests::test_on_connect_no_faults ... ok
test fault_inject::tests::test_on_recv_no_faults ... ok
test fault_inject::tests::test_on_send_corrupt_probability_1 ... ok
test fault_inject::tests::test_on_send_delay_probability_1 ... ok
test fault_inject::tests::test_on_send_drop_probability_1 ... ok
test fault_inject::tests::test_on_send_no_faults ... ok
test fault_inject::tests::test_recv_action_equality ... ok
test fault_inject::tests::test_recv_ignores_non_drop_faults ... ok
test enrollment::tests::test_token_has_correct_length ... ok
test fault_inject::tests::test_seeded_deterministic ... ok
test fault_inject::tests::test_send_action_equality ... ok
test fault_inject::tests::test_send_drop_rate_zero_sends ... ok
test fault_inject::tests::test_stats_increment ... ok
test fault_inject::tests::test_stats_snapshot_values ... ok
test flowcontrol::tests::backpressure_states ... ok
test flowcontrol::tests::config_default ... ok
test flowcontrol::tests::flow_control_limits ... ok
test fault_inject::tests::test_send_drop_rate_calculation ... ok
test flowcontrol::tests::flow_controller_multiple_release ... ok
test flowcontrol::tests::permit_drop ... ok
test flowcontrol::tests::high_low_watermarks ... ok
test flowcontrol::tests::window_ack ... ok
test flowcontrol::tests::window_advance ... ok
test flowcontrol::tests::window_basic ... ok
test flowcontrol::tests::window_sliding ... ok
test flowcontrol::tests::zero_byte_permit ... ok
test flowcontrol::tests::window_controller_race ... ok
test gossip::tests::test_alive_members_excludes_dead_and_left ... ok
test gossip::tests::test_config_default ... ok
test gossip::tests::test_confirm_dead ... ok
test gossip::tests::test_dead_cleanup ... ok
test gossip::tests::test_gossip_event_propagation ... ok
test gossip::tests::test_incarnation_wins_in_merge ... ok
test gossip::tests::test_join_member ... ok
test gossip::tests::test_leave_member ... ok
test gossip::tests::test_mark_suspect ... ok
test flowcontrol::tests::flow_control_state_serialization ... ok
test gossip::tests::test_new_node_starts_alive ... ok
test gossip::tests::test_probe_count_increment ... ok
test gossip::tests::test_member_lookup ... ok
test gossip::tests::test_process_gossip_merge ... ok
test flowcontrol::tests::flow_control_basic ... ok
test gossip::tests::test_stats_tracking ... ok
test gossip::tests::test_stats_snapshot ... ok
test flowcontrol::tests::config_serialization ... ok
test gossip::tests::test_rejoin ... ok
test gossip::tests::test_suspect_timeout ... ok
test health::tests::test_concurrent_updates ... ok
test endpoint_registry::tests::test_stats_stale_evictions ... ok
test health::tests::test_connection_health_new ... ok
test flowcontrol::tests::concurrent_flow ... ok
test health::tests::test_health_config_default ... ok
test health::tests::test_health_status_conversion ... ok
test health::tests::test_latency_degraded ... ok
test health::tests::test_packet_loss_ratio ... ok
test health::tests::test_record_success ... ok
test health::tests::test_health_status_default ... ok
test health::tests::test_latency_tracking ... ok
test health::tests::test_recovery_from_unhealthy ... ok
test health::tests::test_stats ... ok
test hedge::tests::test_extra_load_budget ... ok
test health::tests::test_reset ... ok
test health::tests::test_unhealthy_threshold ... ok
test hedge::tests::test_hedge_config_default ... ok
test hedge::tests::test_hedge_rate_tracking ... ok
test hedge::tests::test_hedge_policy_new ... ok
test hedge::tests::test_hedge_win_rate_tracking ... ok
test hedge::tests::test_reset_clears_stats ... ok
test hedge::tests::test_should_hedge_writes_when_allowed ... ok
test hedge::tests::test_should_hedge_when_delay_exceeded ... ok
test hedge::tests::test_should_not_hedge_when_disabled ... ok
test hedge::tests::test_should_not_hedge_when_under_delay ... ok
test hedge::tests::test_stats_snapshot ... ok
test hedge::tests::test_should_not_hedge_writes ... ok
test hedge::tests::test_tracker_check_hedge_before_delay ... ok
test hedge::tests::test_tracker_complete_request ... ok
test hedge::tests::test_tracker_new ... ok
test hedge::tests::test_tracker_start_request ... ok
test health::tests::test_record_failure ... ok
test ipc::tests::test_config_default ... ok
test ipc::tests::test_config_custom ... ok
test ipc::tests::test_config_serde ... ok
test ipc::tests::test_connection_idle_ms ... ok
test ipc::tests::test_connection_mark_connected ... ok
test ipc::tests::test_connection_mark_disconnected ... ok
test ipc::tests::test_connection_mark_error ... ok
test ipc::tests::test_connection_new ... ok
test ipc::tests::test_connection_record_recv ... ok
test ipc::tests::test_connection_record_send ... ok
test ipc::tests::test_connection_state_serde ... ok
test ipc::tests::test_manager_active_count ... ok
test ipc::tests::test_manager_add_connection ... ok
test ipc::tests::test_manager_capacity_available ... ok
test ipc::tests::test_manager_get_connection ... ok
test ipc::tests::test_manager_get_mut_connection ... ok
test ipc::tests::test_manager_mark_connected ... ok
test ipc::tests::test_manager_new ... ok
test ipc::tests::test_manager_remove_connected_tracks_disconnects ... ok
test ipc::tests::test_manager_remove_connection ... ok
test ipc::tests::test_manager_remove_error_connection ... ok
test ipc::tests::test_manager_stats ... ok
test ipc::tests::test_manager_stats_snapshot ... ok
test keepalive::tests::test_dead_state ... ok
test keepalive::tests::test_initial_state_active ... ok
test keepalive::tests::test_initial_state_disabled ... ok
test keepalive::tests::test_is_alive_active ... ok
test keepalive::tests::test_is_alive_warning ... ok
test keepalive::tests::test_is_not_alive_dead ... ok
test keepalive::tests::test_keepalive_config_default ... ok
test keepalive::tests::test_average_rtt ... ok
test keepalive::tests::test_manager_add_remove ... ok
test keepalive::tests::test_manager_dead_connections ... ok
test keepalive::tests::test_manager_connection_count ... ok
test keepalive::tests::test_record_received_resets_missed ... ok
test health::proptest_tests::test_latency_tracking_random ... ok
test keepalive::tests::test_record_sent ... ok
test keepalive::tests::test_record_timeout_increments_missed ... ok
test keepalive::tests::test_reset ... ok
test keepalive::tests::test_should_send_initially ... ok
test keepalive::tests::test_stats ... ok
test keepalive::tests::test_warning_state ... ok
test loadshed::tests::test_admit_count_tracking ... ok
test loadshed::tests::test_config_default ... ok
test loadshed::tests::test_disabled_never_sheds ... ok
test loadshed::tests::test_is_overloaded_false_initially ... ok
test loadshed::tests::test_latency_tracking_weighted ... ok
test loadshed::tests::test_multiple_signals_compound ... ok
test loadshed::tests::test_new_load_shedder ... ok
test loadshed::tests::test_not_shedding_when_below_thresholds ... ok
test loadshed::tests::test_reset_clears_state ... ok
test loadshed::tests::test_shed_probability_capped_at_one ... ok
test loadshed::tests::test_shed_probability_increases_with_load ... ok
test loadshed::tests::test_shedding_when_cpu_exceeds ... ok
test loadshed::tests::test_shedding_when_latency_exceeds ... ok
test loadshed::tests::test_shedding_when_queue_depth_exceeds ... ok
test loadshed::tests::test_stats_snapshot ... ok
test message::tests::test_serialize_deserialize_heartbeat_request ... ok
test message::tests::test_rpc_message_serialization ... ok
test message::tests::test_serialize_deserialize_join_cluster_response ... ok
test message::tests::test_serialize_deserialize_lookup_request ... ok
test loadshed::tests::test_is_overloaded_true_when_threshold_exceeded ... ok
test message::tests::test_serialize_deserialize_read_response ... ok
test message::tests::test_serialize_deserialize_readdir_response ... ok
test metrics::tests::test_connection_close_saturating ... ok
test metrics::tests::test_bytes_tracking ... ok
test metrics::tests::test_connection_tracking ... ok
test metrics::tests::test_metrics_new ... ok
test metrics::tests::test_snapshot_is_consistent ... ok
test metrics::tests::test_concurrent_metrics ... ok
test multicast_group::tests::test_broadcast_result_serde ... ok
test metrics::tests::test_inc_counters ... ok
test multicast_group::tests::test_create_group ... ok
test multicast_group::tests::test_create_group_limit ... ok
test multicast_group::tests::test_create_group_duplicate ... ok
test multicast_group::tests::test_default_config ... ok
test multicast_group::tests::test_dissolve_removes_all ... ok
test multicast_group::tests::test_dissolve_unknown_group ... ok
test multicast_group::tests::test_group_id_equality ... ok
test multicast_group::tests::test_group_id_hash ... ok
test multicast_group::tests::test_group_id_new ... ok
test multicast_group::tests::test_group_member_serde ... ok
test multicast_group::tests::test_is_member_false ... ok
test multicast_group::tests::test_is_member_true ... ok
test multicast_group::tests::test_join_duplicate_member ... ok
test multicast_group::tests::test_join_member_limit ... ok
test multicast_group::tests::test_join_success ... ok
test multicast_group::tests::test_join_unknown_group ... ok
test multicast_group::tests::test_leave_not_member ... ok
test multicast_group::tests::test_leave_success ... ok
test multicast_group::tests::test_multiple_groups_independent ... ok
test multicast_group::tests::test_prepare_broadcast_empty_group ... ok
test multicast_group::tests::test_prepare_broadcast_returns_all_members ... ok
test multicast_group::tests::test_stats_counts ... ok
test multipath::tests::test_active_paths_filter ... ok
test multipath::tests::test_add_remove_path ... ok
test multipath::tests::test_config_default ... ok
test multipath::tests::test_failover_selection ... ok
test multipath::tests::test_degraded_detection ... ok
test multipath::tests::test_jitter_calculation ... ok
test multipath::tests::test_latency_ewma_smoothing ... ok
test multipath::tests::test_lowest_latency_selection ... ok
test multipath::tests::test_lowest_latency_tie_breaker_priority ... ok
test multipath::tests::test_mark_failed_and_active ... ok
test multipath::tests::test_min_latency_tracking ... ok
test multipath::tests::test_no_active_paths_returns_none ... ok
test multipath::tests::test_path_id_newtype ... ok
test multipath::tests::test_path_state_transitions ... ok
test multipath::tests::test_record_failure_increments_errors ... ok
test multipath::tests::test_record_success_updates_metrics ... ok
test multipath::tests::test_round_robin_selection ... ok
test multipath::tests::test_skip_failed_in_round_robin ... ok
test multipath::tests::test_stats_snapshot ... ok
test mux::tests::test_active_streams_count ... ok
test multipath::tests::test_weighted_selection ... ok
test mux::tests::test_cancel_stream ... ok
test mux::tests::test_concurrent_dispatch ... ok
test mux::tests::test_dispatch_unknown_stream ... ok
test mux::tests::test_max_concurrent_streams ... ok
test mux::tests::test_mux_config_default ... ok
test mux::tests::test_open_and_dispatch ... ok
test multicast_group::tests::test_group_event_serde ... ok
test observability::tests::test_attribute_bool ... ok
test mux::tests::test_stream_ids_unique ... ok
test observability::tests::test_attribute_int ... ok
test observability::tests::test_attribute_string ... ok
test observability::tests::test_attribute_float ... ok
test observability::tests::test_collector_add_event ... ok
test observability::tests::test_collector_add_event_with_attrs ... ok
test observability::tests::test_collector_attributes_limited_by_max ... ok
test observability::tests::test_collector_disabled ... ok
test observability::tests::test_collector_end_nonexistent_span ... ok
test observability::tests::test_collector_end_span ... ok
test observability::tests::test_collector_drain_completed ... ok
test observability::tests::test_collector_events_limited_by_max ... ok
test observability::tests::test_collector_get_span ... ok
test observability::tests::test_collector_max_spans ... ok
test observability::tests::test_collector_multiple_events ... ok
test observability::tests::test_collector_start_span ... ok
test observability::tests::test_collector_stats ... ok
test observability::tests::test_collector_parent_child ... ok
test observability::tests::test_collector_status_tracking ... ok
test observability::tests::test_config_default ... ok
test observability::tests::test_span_builder_attributes ... ok
test observability::tests::test_span_builder_basic ... ok
test observability::tests::test_span_builder_float_attr ... ok
test observability::tests::test_span_builder_parent ... ok
test observability::tests::test_event_severity_ordering ... ok
test observability::tests::test_span_id ... ok
test observability::tests::test_span_status_values ... ok
test otel::tests::test_inject_trace_context ... ok
test otel::tests::test_otlp_attribute_bool ... ok
test otel::tests::test_otlp_attribute_bytes ... ok
test otel::tests::test_otlp_attribute_clone ... ok
test otel::tests::test_otlp_attribute_double ... ok
test otel::tests::test_otlp_attribute_int ... ok
test otel::tests::test_otlp_attribute_string ... ok
test otel::tests::test_otlp_config_default ... ok
test otel::tests::test_otlp_event_clone ... ok
test observability::tests::test_span_timing ... ok
test otel::tests::test_otlp_event_new ... ok
test otel::tests::test_otlp_event_with_attributes ... ok
test otel::tests::test_otlp_exporter_drain_all ... ok
test otel::tests::test_otlp_exporter_drain_batch_empty ... ok
test otel::tests::test_otlp_exporter_drain_batch_size ... ok
test otel::tests::test_otlp_exporter_enqueue ... ok
test otel::tests::test_otlp_exporter_enqueue_capacity ... ok
test otel::tests::test_otlp_exporter_enqueue_disabled ... ok
test otel::tests::test_otlp_exporter_new ... ok
test otel::tests::test_otlp_span_new ... ok
test otel::tests::test_otlp_value_clone ... ok
test otel::tests::test_otlp_span_clone ... ok
test otel::tests::test_otlp_status_code_values ... ok
test otel::tests::test_roundtrip_enqueue_drain ... ok
test otel::tests::test_span_to_otlp_attributes ... ok
test otel::tests::test_span_to_otlp_basic ... ok
test otel::tests::test_span_to_otlp_status_mapping ... ok
test otel::tests::test_span_to_otlp_events ... ok
test otel::tests::test_stats_snapshot ... ok
test pipeline::tests::test_pipeline_add_stage ... ok
test pipeline::tests::test_pipeline_add_too_many_stages ... ok
test pipeline::tests::test_pipeline_config_default ... ok
test pipeline::tests::test_pipeline_direction_filtering ... ok
test pipeline::tests::test_pipeline_disabled_stage_skipped ... ok
test pipeline::tests::test_pipeline_duplicate_stage_id ... ok
test pipeline::tests::test_pipeline_empty_execute ... ok
test pipeline::tests::test_pipeline_enable_disable ... ok
test pipeline::tests::test_pipeline_fail_open ... ok
test health::proptest_tests::test_consecutive_failures_count ... ok
test pipeline::tests::test_pipeline_header_stage ... ok
test pipeline::tests::test_pipeline_metadata_propagation ... ok
test pipeline::tests::test_pipeline_passthrough ... ok
test pipeline::tests::test_pipeline_payload_too_large ... ok
test pipeline::tests::test_pipeline_reject_stage ... ok
test pipeline::tests::test_pipeline_reject_stage_passes_other_opcodes ... ok
test pipeline::tests::test_pipeline_remove_nonexistent ... ok
test pipeline::tests::test_pipeline_remove_stage ... ok
test pipeline::tests::test_pipeline_stage_ordering ... ok
test pipeline::tests::test_pipeline_stage_timing ... ok
test pipeline::tests::test_pipeline_stats ... ok
test pnfs_layout::tests::test_device_addr_creation ... ok
test pnfs_layout::tests::test_device_id_serde ... ok
test pnfs_layout::tests::test_data_layout_serde ... ok
test pnfs_layout::tests::test_device_addr_serde ... ok
test pnfs_layout::tests::test_device_id_creation ... ok
test pnfs_layout::tests::test_io_mode_serde ... ok
test pnfs_layout::tests::test_layout_cache_is_granted ... ok
test pnfs_layout::tests::test_layout_cache_get_layout ... ok
test pnfs_layout::tests::test_layout_cache_grant ... ok
test pnfs_layout::tests::test_layout_cache_new ... ok
test pnfs_layout::tests::test_layout_cache_recall_all ... ok
test pnfs_layout::tests::test_layout_cache_return_layout ... ok
test pnfs_layout::tests::test_layout_cache_return_nonexistent ... ok
test pnfs_layout::tests::test_layout_cache_return_wrong_stateid ... ok
test pnfs_layout::tests::test_layout_error_display ... ok
test pnfs_layout::tests::test_layout_state_id_new ... ok
test pnfs_layout::tests::test_layout_state_id_bump_seqid ... ok
test pnfs_layout::tests::test_layout_state_id_serde ... ok
test pnfs_layout::tests::test_layout_state_id_wrapping ... ok

...21047 bytes truncated...

The tool call succeeded but the output was truncated. Full output saved to: /home/cfs/.local/share/opencode/tool-output/tool_cb9ab870b001JmM0p4Zasuc5r0
Use the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context.[0m
One test failed. Let me check the failure details:
[0m$ [0mcargo test -p claudefs-transport test_state_transitions 2>&1
Blocking waiting for file lock on artifact directory
warning: unused import: `TopologyStatsSnapshot`
   --> crates/claudefs-transport/src/cluster_topology.rs:282:74
    |
282 |         ClusterTopology, DatacenterId, Proximity, RackId, TopologyLabel, TopologyStatsSnapshot,
    |                                                                          ^^^^^^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `FaultInjectorStatsSnapshot`
   --> crates/claudefs-transport/src/fault_inject.rs:342:69
    |
342 |         corrupt_payload, ConnectAction, FaultConfig, FaultInjector, FaultInjectorStatsSnapshot,
    |                                                                     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `SpanStatus`
   --> crates/claudefs-transport/src/otel.rs:391:82
    |
391 |         self, Attribute, AttributeValue, EventSeverity, Span, SpanEvent, SpanId, SpanStatus,
    |                                                                                  ^^^^^^^^^^

warning: unused variable: `expected`
   --> crates/claudefs-transport/src/adaptive.rs:503:13
    |
503 |         let expected = p99 * 1;
    |             ^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_expected`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `min_limit`
   --> crates/claudefs-transport/src/adaptive.rs:504:13
    |
504 |         let min_limit = 200 * 1000;
    |             ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_min_limit`

warning: unused variable: `payload`
   --> crates/claudefs-transport/src/compress.rs:477:13
    |
477 |         let payload = compressor.compress(&data);
    |             ^^^^^^^ help: if this is intentional, prefix it with an underscore: `_payload`

warning: unused variable: `receiver`
   --> crates/claudefs-transport/src/stream.rs:776:13
    |
776 |         let receiver = manager.create_receiver(stream_id).unwrap();
    |             ^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_receiver`

warning: unused variable: `i`
   --> crates/claudefs-transport/src/zerocopy.rs:425:13
    |
425 |         for i in 0..100 {
    |             ^ help: if this is intentional, prefix it with an underscore: `_i`

warning: field `config` is never read
  --> crates/claudefs-transport/src/timer_wheel.rs:80:5
   |
79 | pub struct TimerWheel {
   |            ---------- field in this struct
80 |     config: TimerWheelConfig,
   |     ^^^^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:49:5
   |
49 |     pub grants_issued: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
note: the lint level is defined here
  --> crates/claudefs-transport/src/lib.rs:1:9
   |
 1 | #![warn(missing_docs)]
   |         ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:50:5
   |
50 |     pub grants_denied: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:51:5
   |
51 |     pub credits_granted: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:52:5
   |
52 |     pub credits_returned: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:53:5
   |
53 |     pub throttle_events: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:54:5
   |
54 |     pub exhaustion_events: AtomicU64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/credit_window.rs:69:5
   |
69 | /     pub fn snapshot(
70 | |         &self,
71 | |         available: u64,
72 | |         total: u64,
73 | |         state: CreditWindowState,
74 | |     ) -> CreditWindowStatsSnapshot {
   | |__________________________________^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:92:5
   |
92 |     pub grants_issued: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:93:5
   |
93 |     pub grants_denied: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:94:5
   |
94 |     pub credits_granted: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:95:5
   |
95 |     pub credits_returned: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:96:5
   |
96 |     pub throttle_events: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:97:5
   |
97 |     pub exhaustion_events: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:98:5
   |
98 |     pub available_credits: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/credit_window.rs:99:5
   |
99 |     pub total_credits: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/credit_window.rs:100:5
    |
100 |     pub state: CreditWindowState,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/credit_window.rs:158:5
    |
158 |     pub fn new(config: CreditWindowConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: comparison is useless due to type limits
   --> crates/claudefs-transport/src/server.rs:364:17
    |
364 |         assert!(snap.requests_received >= 0);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(unused_comparisons)]` on by default

warning: comparison is useless due to type limits
   --> crates/claudefs-transport/src/server.rs:365:17
    |
365 |         assert!(snap.responses_sent >= 0);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:123:5
    |
123 |     pub hops: Vec<TraceHop>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:133:5
    |
133 |     pub seq: u64,
    |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:134:5
    |
134 |     pub sent_at_ms: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:135:5
    |
135 |     pub timeout_ms: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:140:5
    |
140 |     pub pings_sent: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:141:5
    |
141 |     pub pongs_received: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:142:5
    |
142 |     pub pings_timed_out: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:143:5
    |
143 |     pub pings_rejected: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/wire_diag.rs:156:5
    |
156 |     pub fn snapshot(&self) -> WireDiagStatsSnapshot {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:169:5
    |
169 |     pub pings_sent: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:170:5
    |
170 |     pub pongs_received: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:171:5
    |
171 |     pub pings_timed_out: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:172:5
    |
172 |     pub pings_rejected: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:178:5
    |
178 |     pub sample_count: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:179:5
    |
179 |     pub min_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:180:5
    |
180 |     pub max_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:181:5
    |
181 |     pub mean_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/wire_diag.rs:182:5
    |
182 |     pub p99_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/wire_diag.rs:197:5
    |
197 |     pub fn new(config: WireDiagConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-transport` (lib test) generated 47 warnings (run `cargo fix --lib -p claudefs-transport --tests` to apply 8 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.19s
     Running unittests src/lib.rs (target/debug/deps/claudefs_transport-6432c7de32d8aec5)

running 1 test
test credit_window::tests::test_state_transitions ... FAILED

failures:

---- credit_window::tests::test_state_transitions stdout ----

thread 'credit_window::tests::test_state_transitions' (1176217) panicked at crates/claudefs-transport/src/credit_window.rs:506:9:
assertion `left == right` failed
  left: Warning
 right: Throttled
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    credit_window::tests::test_state_transitions

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1069 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p claudefs-transport --lib`[0m

The test has a bug - after consuming 85 credits (15 remaining), ratio is 0.15 which is above throttle_threshold (0.10), so it should be Warning. I need to fix the test to consume 90 credits (10 remaining) to reach Throttled state:
[0m← [0mEdit crates/claudefs-transport/src/credit_window.rsIndex: /home/cfs/claudefs/crates/claudefs-transport/src/credit_window.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/credit_window.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/credit_window.rs
@@ -501,11 +501,11 @@
 
         let _g1 = window.try_acquire(75).unwrap();
         assert_eq!(window.state(), CreditWindowState::Warning);
 
-        let _g2 = window.try_acquire(10).unwrap();
+        let _g2 = window.try_acquire(15).unwrap();
         assert_eq!(window.state(), CreditWindowState::Throttled);
 
-        let _g3 = window.try_acquire(15).unwrap();
+        let _g3 = window.try_acquire(10).unwrap();
         assert_eq!(window.state(), CreditWindowState::Exhausted);
     }
 }



[0m[0m$ [0mcargo test -p claudefs-transport test_state_transitions 2>&1 | tail -20
[0m

--> crates/claudefs-transport/src/wire_diag.rs:182:5
    |
182 |     pub p99_us: Option<u64>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/wire_diag.rs:197:5
    |
197 |     pub fn new(config: WireDiagConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-transport` (lib test) generated 47 warnings (run `cargo fix --lib -p claudefs-transport --tests` to apply 8 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.44s
     Running unittests src/lib.rs (target/debug/deps/claudefs_transport-6432c7de32d8aec5)

running 1 test
test credit_window::tests::test_state_transitions ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1069 filtered out; finished in 0.00sNow let me run all tests to verify everything passes:

test zerocopy::tests::test_pool_concurrent_access ... ok
test zerocopy::tests::test_pool_grow_respects_max ... ok
test zerocopy::tests::test_pool_in_use_tracking ... ok
test zerocopy::tests::test_pool_new ... ok
test zerocopy::tests::test_pool_release ... ok
test zerocopy::tests::test_pool_shrink ... ok
test zerocopy::tests::test_pool_shrink_only_idle ... ok
test zerocopy::tests::test_pool_stats ... ok
test zerocopy::tests::test_region_data_isolation ... ok
test zerocopy::tests::test_region_id ... ok
test rpc::tests::test_rpc_roundtrip ... ok
test protocol::proptest_tests::crc32_deterministic ... ok
test qos::tests::test_token_bucket_rate_limiting ... ok
test retry::tests::test_retry_success_after_failures ... ok
test routing::tests::test_consistent_hash_ring_distribution ... ok
test protocol::tests::test_payload_too_large ... ok

test result: ok. 1070 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.20s

   Doc-tests claudefs_transport

running 5 tests
test crates/claudefs-transport/src/circuitbreaker.rs - circuitbreaker::CircuitBreakerConfig (line 115) ... ok
test crates/claudefs-transport/src/circuitbreaker.rs - circuitbreaker::CircuitBreaker (line 168) ... ok
test crates/claudefs-transport/src/circuitbreaker.rs - circuitbreaker (line 16) ... ok
test crates/claudefs-transport/src/flowcontrol.rs - flowcontrol (line 21) ... ok
test crates/claudefs-transport/src/circuitbreaker.rs - circuitbreaker::CircuitState (line 62) ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s
[0m
[0m$ [0mcargo test -p claudefs-transport 2>&1 | tail -30Done. All 1070 tests pass.

**Summary:**
- Added `pub mod credit_window;` and `pub mod wire_diag;` to `lib.rs`
- Added `pub use` re-exports for all types from both modules
- Fixed compile error in `wire_diag.rs` (config moved before `window_size` was read)
- Fixed test bug in `credit_window.rs` (incorrect state transition expectations - 10→15 credits to reach Throttled threshold)
