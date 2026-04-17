[0m

> build · accounts/fireworks/models/minimax-m2p5[0m
[0m✱ [0mGlob "crates/claudefs-transport/src/**/*.rs"[90m 87 matches[0m
[0m→ [0mRead crates/claudefs-transport/Cargo.toml
I'll explore the existing codebase first to understand the current metrics implementation, then use OpenCode to generate the Rust code.
[0m→ [0mRead crates/claudefs-transport/src/metrics.rs
[0m→ [0mRead crates/claudefs-transport/src/lib.rs
[0m→ [0mRead crates/claudefs-transport/src/trace_aggregator.rs[90m [limit=100][0m
[0m→ [0mRead crates/claudefs-transport/src/transport_pooling.rs[90m [limit=100][0m
[0m→ [0mRead crates/claudefs-transport/src/reactive_backpressure.rs[90m [limit=100][0m
[0m✱ [0mGrep "StatsSnapshot|PoolStats"[90m in crates/claudefs-transport · 174 matches[0m
[0m→ [0mRead crates/claudefs-transport/src/transport_pooling.rs[90m [offset=129, limit=100][0m
[0m→ [0mRead crates/claudefs-transport/src/reactive_backpressure.rs[90m [offset=100, limit=80][0m
[0m✱ [0mGrep "QoS|Qos"[90m in crates/claudefs-transport/src · 61 matches[0m
[0m→ [0mRead crates/claudefs-transport/src/trace_aggregator.rs[90m [limit=150, offset=200][0m
Now I have a good understanding of the codebase. Let me write the input.md for OpenCode:
[0m✗ [0mwrite failed
[91m[1mError: [0mYou must read file /home/cfs/claudefs/input.md before overwriting it. Use the Read tool first
[0m✱ [0mGlob "input.md"[90m 3 matches[0m
[0m→ [0mRead input.md
[0m

[0m← [0mWrite transport_prometheus_input.md

Wrote file successfully.[0m[0m$ [0mecho $FIREWORKS_API_KEYfw_J246CQF6HnGPVcHzLDhnRy[0m


[0m$ [0mcd /home/cfs/claudefs && ~/.opencode/bin/opencode run "$(cat transport_prometheus_input.md)" --model fireworks-ai/accounts/fireworks/models/minimax-m2p5 > transport_prometheus_output.md
[0m
[0m
> build · accounts/fireworks/models/minimax-m2p5
[0m
[0m✱ [0mGlob "crates/claudefs-transport/src/**/*.rs"[90m 87 matches[0m
[0m→ [0mRead crates/claudefs-transport/src/lib.rs
[0m→ [0mRead crates/claudefs-transport/src/metrics.rs
[0m→ [0mRead crates/claudefs-transport/src/trace_aggregator.rs
[0m→ [0mRead crates/claudefs-transport/src/transport_pooling.rs
[0m→ [0mRead crates/claudefs-transport/src/reactive_backpressure.rs
[0m→ [0mRead crates/claudefs-transport/src/qos.rs
[0m→ [0mRead crates/claudefs-transport/Cargo.toml
[0m
[0m← [0mWrite crates/claudefs-transport/src/prometheus_exporter.rs
Wrote file successfully.
[0m
[0m← [0mEdit crates/claudefs-transport/Cargo.toml
Index: /home/cfs/claudefs/crates/claudefs-transport/Cargo.toml
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/Cargo.toml
+++ /home/cfs/claudefs/crates/claudefs-transport/Cargo.toml
@@ -7,8 +7,9 @@
 description = "ClaudeFS subsystem: RDMA via libfabric, TCP via io_uring, custom RPC protocol"
 
 [dependencies]
 tokio.workspace = true
+prometheus = "0.13"
 thiserror.workspace = true
 serde.workspace = true
 bincode.workspace = true
 tracing.workspace = true

[0m
[0m← [0mEdit crates/claudefs-transport/src/lib.rs
Index: /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/lib.rs
@@ -238,8 +238,11 @@
     ConnectionState, PooledConnection, ConnectionPool, PoolConfig, PoolStats, PoolStatsSnapshot,
     ConnectionPoolManager,
 };
 
+pub mod prometheus_exporter;
+pub use prometheus_exporter::PrometheusTransportMetrics;
+
 pub mod pipelined_requests;
 pub use pipelined_requests::{
     RequestId, StreamId, RequestState, PipelinedRequest, PipelinedResponse, PipelineConfig,
     InFlightRequest, RequestPipeline, PipelineStats, PipelineStatsSnapshot,

[0m
[0m$ [0mcd /home/cfs/claudefs && cargo build -p claudefs-transport 2>&1
Updating crates.io index
     Locking 2 packages to latest compatible versions
      Adding prometheus v0.13.4 (available: v0.14.0)
      Adding protobuf v2.28.0
 Downloading crates ...
  Downloaded protobuf v2.28.0
  Downloaded prometheus v0.13.4
   Compiling asn1-rs v0.6.2
   Compiling protobuf v2.28.0
   Compiling prometheus v0.13.4
   Compiling tracing-subscriber v0.3.22
   Compiling oid-registry v0.7.1
   Compiling der-parser v9.0.0
   Compiling x509-parser v0.16.0
   Compiling rcgen v0.13.2
   Compiling claudefs-transport v0.1.0 (/home/cfs/claudefs/crates/claudefs-transport)
error[E0061]: this function takes 3 arguments but 2 arguments were supplied
   --> crates/claudefs-transport/src/prometheus_exporter.rs:107:13
    |
107 |             Self::write_qos_stats(output, stats);
    |             ^^^^^^^^^^^^^^^^^^^^^ ------ argument #1 of type `&PrometheusTransportMetrics<'_>` is missing
    |
note: method defined here
   --> crates/claudefs-transport/src/prometheus_exporter.rs:118:8
    |
118 |     fn write_qos_stats(&self, output: &mut String, stats: &QosStats) {
    |        ^^^^^^^^^^^^^^^ -----
help: provide the argument
    |
107 |             Self::write_qos_stats(/* &PrometheusTransportMetrics<'_> */, output, stats);
    |                                   ++++++++++++++++++++++++++++++++++++++

warning: unused variable: `now_ms`
   --> crates/claudefs-transport/src/flow_sched.rs:267:68
    |
267 |     pub fn request_send(&self, flow_id: FlowId, size_bytes: usize, now_ms: u64) -> SendDecision {
    |                                                                    ^^^^^^ help: if this is intentional, prefix it with an underscore: `_now_ms`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `flow`
   --> crates/claudefs-transport/src/flow_sched.rs:277:13
    |
277 |         let flow = match flows.get(&flow_id) {
    |             ^^^^ help: if this is intentional, prefix it with an underscore: `_flow`

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:247:13
    |
247 |         let mut connections = self.connections.lock().unwrap();
    |             ----^^^^^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:267:13
    |
267 |         let mut connections = self.connections.lock().unwrap();
    |             ----^^^^^^^^^^^
    |             |
    |             help: remove this `mut`

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:301:17
    |
301 |             let mut pooled = conn.lock().unwrap();
    |                 ----^^^^^^
    |                 |
    |                 help: remove this `mut`

For more information about this error, try `rustc --explain E0061`.
warning: `claudefs-transport` (lib) generated 5 warnings
error: could not compile `claudefs-transport` (lib) due to 1 previous error; 5 warnings emitted
[0m
[0m← [0mEdit crates/claudefs-transport/src/prometheus_exporter.rs
Index: /home/cfs/claudefs/crates/claudefs-transport/src/prometheus_exporter.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/prometheus_exporter.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/prometheus_exporter.rs
@@ -103,9 +103,9 @@
     let bp_level = self.current_backpressure.map(|l| l.to_numeric()).unwrap_or(0);
     Self::write_gauge(output, "transport_backpressure_level", "Current backpressure level (0=Ok, 1=Slow, 2=Degraded, 3=Overloaded)", bp_level);
 
     if let Some(stats) = self.qos_stats {
-        Self::write_qos_stats(output, stats);
+        self.write_qos_stats(output, stats);
     }
 }
 
 fn write_histograms(&self, output: &mut String) {

[0m
[0m$ [0mcd /home/cfs/claudefs && cargo build -p claudefs-transport 2>&1
Compiling claudefs-transport v0.1.0 (/home/cfs/claudefs/crates/claudefs-transport)
warning: unused variable: `now_ms`
   --> crates/claudefs-transport/src/flow_sched.rs:267:68
    |
267 |     pub fn request_send(&self, flow_id: FlowId, size_bytes: usize, now_ms: u64) -> SendDecision {
    |                                                                    ^^^^^^ help: if this is intentional, prefix it with an underscore: `_now_ms`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `flow`
   --> crates/claudefs-transport/src/flow_sched.rs:277:13
    |
277 |         let flow = match flows.get(&flow_id) {
    |             ^^^^ help: if this is intentional, prefix it with an underscore: `_flow`

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:247:13
    |
247 |         let mut connections = self.connections.lock().unwrap();
    |             ----^^^^^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:267:13
    |
267 |         let mut connections = self.connections.lock().unwrap();
    |             ----^^^^^^^^^^^
    |             |
    |             help: remove this `mut`

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:301:17
    |
301 |             let mut pooled = conn.lock().unwrap();
    |                 ----^^^^^^
    |                 |
    |                 help: remove this `mut`

warning: field `config` is never read
  --> crates/claudefs-transport/src/timer_wheel.rs:80:5
   |
79 | pub struct TimerWheel {
   |            ---------- field in this struct
80 |     config: TimerWheelConfig,
   |     ^^^^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: field `virtual_finish_time` is never read
   --> crates/claudefs-transport/src/flow_sched.rs:178:5
    |
176 | struct FlowState {
    |        --------- field in this struct
177 |     entry: FlowEntry,
178 |     virtual_finish_time: f64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: field `max_burst` is never read
   --> crates/claudefs-transport/src/flow_sched.rs:380:5
    |
377 | struct TokenBucket {
    |        ----------- field in this struct
...
380 |     max_burst: u64,
    |     ^^^^^^^^^

warning: fields `node_id` and `capacity_shards` are never read
   --> crates/claudefs-transport/src/rebalance.rs:194:5
    |
193 | struct NodeInfo {
    |        -------- fields in this struct
194 |     node_id: [u8; 16],
    |     ^^^^^^^
195 |     capacity_shards: usize,
    |     ^^^^^^^^^^^^^^^

warning: fields `plan_in_progress` and `total_shards` are never read
   --> crates/claudefs-transport/src/rebalance.rs:204:5
    |
200 | pub struct RebalanceCoordinator {
    |            -------------------- fields in this struct
...
204 |     plan_in_progress: RwLock<Option<u64>>,
    |     ^^^^^^^^^^^^^^^^
205 |     total_shards: u32,
    |     ^^^^^^^^^^^^

warning: method `set_initial_shards` is never used
   --> crates/claudefs-transport/src/rebalance.rs:552:8
    |
212 | impl RebalanceCoordinator {
    | ------------------------- method in this implementation
...
552 |     fn set_initial_shards(&self, node_id: [u8; 16], shards: Vec<VirtualShard>) {
    |        ^^^^^^^^^^^^^^^^^^

warning: fields `id` and `next_chunk_index` are never read
   --> crates/claudefs-transport/src/snapshot_transfer.rs:215:5
    |
214 | struct Transfer {
    |        -------- fields in this struct
215 |     id: TransferId,
    |     ^^
...
221 |     next_chunk_index: u64,
    |     ^^^^^^^^^^^^^^^^

warning: field `completed` is never read
   --> crates/claudefs-transport/src/trace_aggregator.rs:173:5
    |
169 | struct TraceState {
    |        ---------- field in this struct
...
173 |     completed: bool,
    |     ^^^^^^^^^

warning: field `last_seen_ns` is never read
   --> crates/claudefs-transport/src/adaptive_router.rs:100:5
    |
 94 | struct EndpointState {
    |        ------------- field in this struct
...
100 |     last_seen_ns: u64,
    |     ^^^^^^^^^^^^

warning: field `next_pool_id` is never read
   --> crates/claudefs-transport/src/transport_pooling.rs:355:5
    |
352 | pub struct ConnectionPoolManager {
    |            --------------------- field in this struct
...
355 |     next_pool_id: AtomicU64,
    |     ^^^^^^^^^^^^

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

warning: missing documentation for a method
   --> crates/claudefs-transport/src/enrollment.rs:452:5
    |
452 |     pub fn set_token_expiry(&mut self, token_id: &str, expires_at: u64) -> Option<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:178:5
    |
178 |     pub ops_started: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:179:5
    |
179 |     pub ops_succeeded: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:180:5
    |
180 |     pub ops_failed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:181:5
    |
181 |     pub ops_timed_out: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:182:5
    |
182 |     pub total_targets_sent: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:183:5
    |
183 |     pub total_target_successes: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:184:5
    |
184 |     pub total_target_failures: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:225:5
    |
225 |     pub ops_started: u64,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:226:5
    |
226 |     pub ops_succeeded: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:227:5
    |
227 |     pub ops_failed: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:228:5
    |
228 |     pub ops_timed_out: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:229:5
    |
229 |     pub total_targets_sent: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:230:5
    |
230 |     pub total_target_successes: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:231:5
    |
231 |     pub total_target_failures: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:232:5
    |
232 |     pub in_flight: usize,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:182:5
    |
182 |     pub rounds_started: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:183:5
    |
183 |     pub rounds_achieved: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:184:5
    |
184 |     pub rounds_failed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:185:5
    |
185 |     pub rounds_expired: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:186:5
    |
186 |     pub total_votes: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:187:5
    |
187 |     pub total_approvals: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:188:5
    |
188 |     pub total_rejections: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:229:5
    |
229 |     pub rounds_started: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:230:5
    |
230 |     pub rounds_achieved: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:231:5
    |
231 |     pub rounds_failed: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:232:5
    |
232 |     pub rounds_expired: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:233:5
    |
233 |     pub total_votes: u64,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:234:5
    |
234 |     pub total_approvals: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:235:5
    |
235 |     pub total_rejections: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:236:5
    |
236 |     pub active_rounds: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/node_blacklist.rs:12:1
   |
12 | pub enum BlacklistReason {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/node_blacklist.rs:13:5
   |
13 |     ConnectionFailed,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/node_blacklist.rs:14:5
   |
14 |     ErrorResponse(String),
   |     ^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/node_blacklist.rs:15:5
   |
15 |     LatencyThreshold,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/node_blacklist.rs:16:5
   |
16 |     Manual,
   |     ^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/node_blacklist.rs:20:1
   |
20 | pub struct BlacklistEntry {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:21:5
   |
21 |     pub node_id: [u8; 16],
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:22:5
   |
22 |     pub reason: BlacklistReason,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:23:5
   |
23 |     pub added_at_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:24:5
   |
24 |     pub expires_at_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:25:5
   |
25 |     pub failure_count: u32,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/node_blacklist.rs:29:5
   |
29 |     pub fn is_expired(&self, now_ms: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/node_blacklist.rs:35:1
   |
35 | pub struct BlacklistConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:36:5
   |
36 |     pub base_backoff_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:37:5
   |
37 |     pub max_backoff_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:38:5
   |
38 |     pub exponential: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:39:5
   |
39 |     pub max_entries: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/node_blacklist.rs:53:1
   |
53 | pub struct NodeBlacklist {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-transport/src/node_blacklist.rs:60:5
   |
60 |     pub fn new(config: BlacklistConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/node_blacklist.rs:68:5
   |
68 |     pub fn blacklist(&self, node_id: [u8; 16], reason: BlacklistReason, now_ms: u64) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:103:5
    |
103 |     pub fn remove(&self, node_id: &[u8; 16]) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:110:5
    |
110 |     pub fn is_blacklisted(&self, node_id: &[u8; 16], now_ms: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:123:5
    |
123 |     pub fn expire(&self, now_ms: u64) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:136:5
    |
136 |     pub fn entry(&self, node_id: &[u8; 16], now_ms: u64) -> Option<BlacklistEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:144:5
    |
144 |     pub fn active_entries(&self, now_ms: u64) -> Vec<BlacklistEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:153:5
    |
153 |     pub fn filter_available<'a>(&self, nodes: &'a [[u8; 16]], now_ms: u64) -> Vec<&'a [u8; 16]> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:160:5
    |
160 |     pub fn active_count(&self, now_ms: u64) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:165:5
    |
165 |     pub fn stats(&self) -> Arc<BlacklistStats> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/node_blacklist.rs:170:1
    |
170 | pub struct BlacklistStats {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:171:5
    |
171 |     pub nodes_blacklisted: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:172:5
    |
172 |     pub nodes_removed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:173:5
    |
173 |     pub nodes_expired: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:174:5
    |
174 |     pub blacklist_checks: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:175:5
    |
175 |     pub checks_hit: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/node_blacklist.rs:179:5
    |
179 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:189:5
    |
189 |     pub fn snapshot(&self, active_count: usize) -> BlacklistStatsSnapshot {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/node_blacklist.rs:202:1
    |
202 | pub struct BlacklistStatsSnapshot {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:203:5
    |
203 |     pub nodes_blacklisted: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:204:5
    |
204 |     pub nodes_removed: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:205:5
    |
205 |     pub nodes_expired: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:206:5
    |
206 |     pub blacklist_checks: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:207:5
    |
207 |     pub checks_hit: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:208:5
    |
208 |     pub active_count: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/read_repair.rs:13:1
   |
13 | pub struct RepairId(pub u64);
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/read_repair.rs:16:1
   |
16 | pub enum ShardRepairState {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:17:5
   |
17 |     Fetching,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:18:5
   |
18 |     Fetched,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:19:5
   |
19 |     Failed,
   |     ^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:20:5
   |
20 |     Missing,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:21:5
   |
21 |     Reconstructing,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:22:5
   |
22 |     Repaired,
   |     ^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/read_repair.rs:26:1
   |
26 | pub struct RepairShard {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:27:5
   |
27 |     pub node_id: [u8; 16],
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:28:5
   |
28 |     pub shard_index: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:29:5
   |
29 |     pub state: ShardRepairState,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/read_repair.rs:33:1
   |
33 | pub enum RepairPriority {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:34:5
   |
34 |     Background,
   |     ^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:35:5
   |
35 |     Foreground,
   |     ^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/read_repair.rs:39:1
   |
39 | pub struct ReadRepairConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:40:5
   |
40 |     pub timeout_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:41:5
   |
41 |     pub max_concurrent: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/read_repair.rs:54:1
   |
54 | pub enum RepairError {
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:56:5
   |
56 |     NotFound(RepairId),
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:58:5
   |
58 |     TooManyConcurrent(usize),
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:60:5
   |
60 |     InsufficientShards { available: usize, needed: usize },
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:60:26
   |
60 |     InsufficientShards { available: usize, needed: usize },
   |                          ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:60:44
   |
60 |     InsufficientShards { available: usize, needed: usize },
   |                                            ^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:62:5
   |
62 |     AlreadyCompleted(RepairId),
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/read_repair.rs:66:1
   |
66 | pub enum RepairOpState {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:67:5
   |
67 |     Fetching,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:68:5
   |
68 |     Reconstructing,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:69:5
   |
69 |     WritingBack,
   |     ^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:70:5
   |
70 |     Complete,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:71:5
   |
71 |     Failed,
   |     ^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:72:5
   |
72 |     TimedOut,
   |     ^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/read_repair.rs:75:1
   |
75 | pub struct RepairOp {
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:76:5
   |
76 |     pub id: RepairId,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:77:5
   |
77 |     pub segment_id: u64,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:78:5
   |
78 |     pub priority: RepairPriority,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:79:5
   |
79 |     pub shards: Vec<RepairShard>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:80:5
   |
80 |     pub state: RepairOpState,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:81:5
   |
81 |     pub created_at_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:82:5
   |
82 |     pub ec_data_shards: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:83:5
   |
83 |     pub ec_parity_shards: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-transport/src/read_repair.rs:87:5
   |
87 | /     pub fn new(
88 | |         id: RepairId,
89 | |         segment_id: u64,
90 | |         priority: RepairPriority,
...  |
94 | |         now_ms: u64,
95 | |     ) -> Self {
   | |_____________^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:108:5
    |
108 |     pub fn record_fetch(&mut self, node_id: &[u8; 16], success: bool) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:121:5
    |
121 |     pub fn begin_reconstruct(&mut self) -> Result<(), RepairError> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:137:5
    |
137 |     pub fn begin_writeback(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:141:5
    |
141 |     pub fn complete(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:150:5
    |
150 |     pub fn fail(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:154:5
    |
154 |     pub fn check_timeout(&mut self, now_ms: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:164:5
    |
164 |     pub fn fetched_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:171:5
    |
171 |     pub fn missing_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:178:5
    |
178 |     pub fn can_reconstruct(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/read_repair.rs:183:1
    |
183 | pub struct ReadRepairManager {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/read_repair.rs:191:5
    |
191 |     pub fn new(config: ReadRepairConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:200:5
    |
200 | /     pub fn start_repair(
201 | |         &self,
202 | |         segment_id: u64,
203 | |         priority: RepairPriority,
...   |
207 | |         now_ms: u64,
208 | |     ) -> Result<RepairId, RepairError> {
    | |______________________________________^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:248:5
    |
248 | /     pub fn record_fetch(
249 | |         &self,
250 | |         id: RepairId,
251 | |         node_id: &[u8; 16],
252 | |         success: bool,
253 | |     ) -> Option<RepairOpState> {
    | |______________________________^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:260:5
    |
260 |     pub fn begin_reconstruct(&self, id: RepairId) -> Result<RepairOpState, RepairError> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:275:5
    |
275 |     pub fn complete_repair(&self, id: RepairId) -> Result<(), RepairError> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:290:5
    |
290 |     pub fn check_timeouts(&self, now_ms: u64) -> Vec<RepairId> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:304:5
    |
304 |     pub fn remove(&self, id: RepairId) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:313:5
    |
313 |     pub fn active_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:318:5
    |
318 |     pub fn stats(&self) -> Arc<ReadRepairStats> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/read_repair.rs:323:1
    |
323 | pub struct ReadRepairStats {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:324:5
    |
324 |     pub repairs_started: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:325:5
    |
325 |     pub repairs_completed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:326:5
    |
326 |     pub repairs_failed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:327:5
    |
327 |     pub repairs_timed_out: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:328:5
    |
328 |     pub shards_repaired: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:329:5
    |
329 |     pub foreground_repairs: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:330:5
    |
330 |     pub background_repairs: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/read_repair.rs:334:5
    |
334 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:346:5
    |
346 |     pub fn snapshot(&self, active_repairs: usize) -> ReadRepairStatsSnapshot {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/read_repair.rs:361:1
    |
361 | pub struct ReadRepairStatsSnapshot {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:362:5
    |
362 |     pub repairs_started: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:363:5
    |
363 |     pub repairs_completed: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:364:5
    |
364 |     pub repairs_failed: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:365:5
    |
365 |     pub repairs_timed_out: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:366:5
    |
366 |     pub shards_repaired: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:367:5
    |
367 |     pub foreground_repairs: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:368:5
    |
368 |     pub background_repairs: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:369:5
    |
369 |     pub active_repairs: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/repl_state.rs:11:1
   |
11 | pub struct JournalSeq(pub u64);
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/repl_state.rs:14:5
   |
14 |     pub fn next(self) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/repl_state.rs:18:5
   |
18 |     pub fn is_before(self, other: Self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/repl_state.rs:24:1
   |
24 | pub enum ReplState {
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:25:5
   |
25 |     Idle,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:26:5
   |
26 |     Syncing,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:27:5
   |
27 |     Live,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:28:5
   |
28 |     Disconnected,
   |     ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:29:5
   |
29 |     NeedsResync,
   |     ^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/repl_state.rs:33:1
   |
33 | pub struct JournalEntryRecord {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:34:5
   |
34 |     pub seq: JournalSeq,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:35:5
   |
35 |     pub size_bytes: u32,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:36:5
   |
36 |     pub written_at_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/repl_state.rs:40:1
   |
40 | pub struct ReplStateConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:41:5
   |
41 |     pub max_inflight: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:42:5
   |
42 |     pub max_lag_entries: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:43:5
   |
43 |     pub connection_timeout_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/repl_state.rs:56:1
   |
56 | pub struct JournalReplChannel {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-transport/src/repl_state.rs:68:5
   |
68 |     pub fn new(peer_id: [u8; 16], config: ReplStateConfig, now_ms: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/repl_state.rs:81:5
   |
81 |     pub fn advance_local(&mut self, entry: JournalEntryRecord, now_ms: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:103:5
    |
103 |     pub fn ack(&mut self, seq: JournalSeq, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:120:5
    |
120 |     pub fn check_timeout(&mut self, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:128:5
    |
128 |     pub fn connect(&mut self, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:135:5
    |
135 |     pub fn mark_live(&mut self, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:142:5
    |
142 |     pub fn disconnect(&mut self, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:148:5
    |
148 |     pub fn state(&self) -> ReplState {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:152:5
    |
152 |     pub fn lag(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:156:5
    |
156 |     pub fn inflight_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:160:5
    |
160 |     pub fn is_caught_up(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:164:5
    |
164 |     pub fn peer_id(&self) -> [u8; 16] {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:168:5
    |
168 |     pub fn stats(&self) -> Arc<JournalReplChannelStats> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/repl_state.rs:173:1
    |
173 | pub struct JournalReplChannelStats {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:174:5
    |
174 |     pub entries_sent: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:175:5
    |
175 |     pub entries_acked: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:176:5
    |
176 |     pub ack_timeouts: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:177:5
    |
177 |     pub disconnections: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field

...60855 bytes truncated...

The tool call succeeded but the output was truncated. Full output saved to: /home/cfs/.local/share/opencode/tool-output/tool_d9cfdadfb001TkcrT8e7zWAhmj
Use the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context.
[0m→ [0mRead transport_prometheus_output.md
[0m→ [0mRead crates/claudefs-transport/src/prometheus_exporter.rs
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-transport -- prometheus_exporter 2>&1

Blocking waiting for file lock on artifact directory
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

warning: unused import: `std::sync::atomic::Ordering`
   --> crates/claudefs-transport/src/prometheus_exporter.rs:201:9
    |
201 |     use std::sync::atomic::Ordering;
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^

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

warning: unused variable: `id3`
   --> crates/claudefs-transport/src/fanout.rs:664:13
    |
664 |         let id3 = manager.start(config.clone(), targets.clone(), 0);
    |             ^^^ help: if this is intentional, prefix it with an underscore: `_id3`

warning: unused variable: `id3`
   --> crates/claudefs-transport/src/quorum.rs:611:13
    |
611 |         let id3 = manager.start_round(config.clone(), 0);
    |             ^^^ help: if this is intentional, prefix it with an underscore: `_id3`

warning: unused variable: `backoff2`
   --> crates/claudefs-transport/src/node_blacklist.rs:308:13
    |
308 |         let backoff2 = entry2.expires_at_ms - 2000;
    |             ^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_backoff2`

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/read_repair.rs:646:13
    |
646 |         let mut op = RepairOp::new(
    |             ----^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `receiver`
   --> crates/claudefs-transport/src/stream.rs:776:13
    |
776 |         let receiver = manager.create_receiver(stream_id).unwrap();
    |             ^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_receiver`

warning: unused variable: `manager`
   --> crates/claudefs-transport/src/timeout_budget.rs:539:13
    |
539 |         let manager = TimeoutBudgetManager::new(config);
    |             ^^^^^^^ help: if this is intentional, prefix it with an underscore: `_manager`

warning: unused variable: `id2`
   --> crates/claudefs-transport/src/write_pipeline.rs:423:13
    |
423 |         let id2 = manager.start(2048, 1100);
    |             ^^^ help: if this is intentional, prefix it with an underscore: `_id2`

warning: unused variable: `i`
   --> crates/claudefs-transport/src/zerocopy.rs:425:13
    |
425 |         for i in 0..100 {
    |             ^ help: if this is intentional, prefix it with an underscore: `_i`

warning: unused variable: `now_ms`
   --> crates/claudefs-transport/src/flow_sched.rs:267:68
    |
267 |     pub fn request_send(&self, flow_id: FlowId, size_bytes: usize, now_ms: u64) -> SendDecision {
    |                                                                    ^^^^^^ help: if this is intentional, prefix it with an underscore: `_now_ms`

warning: unused variable: `flow`
   --> crates/claudefs-transport/src/flow_sched.rs:277:13
    |
277 |         let flow = match flows.get(&flow_id) {
    |             ^^^^ help: if this is intentional, prefix it with an underscore: `_flow`

warning: unused variable: `id`
   --> crates/claudefs-transport/src/snapshot_transfer.rs:825:13
    |
825 |         let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();
    |             ^^ help: if this is intentional, prefix it with an underscore: `_id`

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:247:13
    |
247 |         let mut connections = self.connections.lock().unwrap();
    |             ----^^^^^^^^^^^
    |             |
    |             help: remove this `mut`

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:267:13
    |
267 |         let mut connections = self.connections.lock().unwrap();
    |             ----^^^^^^^^^^^
    |             |
    |             help: remove this `mut`

warning: variable does not need to be mutable
   --> crates/claudefs-transport/src/transport_pooling.rs:301:17
    |
301 |             let mut pooled = conn.lock().unwrap();
    |                 ----^^^^^^
    |                 |
    |                 help: remove this `mut`

warning: field `config` is never read
  --> crates/claudefs-transport/src/timer_wheel.rs:80:5
   |
79 | pub struct TimerWheel {
   |            ---------- field in this struct
80 |     config: TimerWheelConfig,
   |     ^^^^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: field `virtual_finish_time` is never read
   --> crates/claudefs-transport/src/flow_sched.rs:178:5
    |
176 | struct FlowState {
    |        --------- field in this struct
177 |     entry: FlowEntry,
178 |     virtual_finish_time: f64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: field `max_burst` is never read
   --> crates/claudefs-transport/src/flow_sched.rs:380:5
    |
377 | struct TokenBucket {
    |        ----------- field in this struct
...
380 |     max_burst: u64,
    |     ^^^^^^^^^

warning: fields `node_id` and `capacity_shards` are never read
   --> crates/claudefs-transport/src/rebalance.rs:194:5
    |
193 | struct NodeInfo {
    |        -------- fields in this struct
194 |     node_id: [u8; 16],
    |     ^^^^^^^
195 |     capacity_shards: usize,
    |     ^^^^^^^^^^^^^^^

warning: fields `plan_in_progress` and `total_shards` are never read
   --> crates/claudefs-transport/src/rebalance.rs:204:5
    |
200 | pub struct RebalanceCoordinator {
    |            -------------------- fields in this struct
...
204 |     plan_in_progress: RwLock<Option<u64>>,
    |     ^^^^^^^^^^^^^^^^
205 |     total_shards: u32,
    |     ^^^^^^^^^^^^

warning: fields `id` and `next_chunk_index` are never read
   --> crates/claudefs-transport/src/snapshot_transfer.rs:215:5
    |
214 | struct Transfer {
    |        -------- fields in this struct
215 |     id: TransferId,
    |     ^^
...
221 |     next_chunk_index: u64,
    |     ^^^^^^^^^^^^^^^^

warning: field `completed` is never read
   --> crates/claudefs-transport/src/trace_aggregator.rs:173:5
    |
169 | struct TraceState {
    |        ---------- field in this struct
...
173 |     completed: bool,
    |     ^^^^^^^^^

warning: field `last_seen_ns` is never read
   --> crates/claudefs-transport/src/adaptive_router.rs:100:5
    |
 94 | struct EndpointState {
    |        ------------- field in this struct
...
100 |     last_seen_ns: u64,
    |     ^^^^^^^^^^^^

warning: field `next_pool_id` is never read
   --> crates/claudefs-transport/src/transport_pooling.rs:355:5
    |
352 | pub struct ConnectionPoolManager {
    |            --------------------- field in this struct
...
355 |     next_pool_id: AtomicU64,
    |     ^^^^^^^^^^^^

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

warning: missing documentation for a method
   --> crates/claudefs-transport/src/enrollment.rs:452:5
    |
452 |     pub fn set_token_expiry(&mut self, token_id: &str, expires_at: u64) -> Option<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:178:5
    |
178 |     pub ops_started: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:179:5
    |
179 |     pub ops_succeeded: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:180:5
    |
180 |     pub ops_failed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:181:5
    |
181 |     pub ops_timed_out: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:182:5
    |
182 |     pub total_targets_sent: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:183:5
    |
183 |     pub total_target_successes: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:184:5
    |
184 |     pub total_target_failures: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:225:5
    |
225 |     pub ops_started: u64,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:226:5
    |
226 |     pub ops_succeeded: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:227:5
    |
227 |     pub ops_failed: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:228:5
    |
228 |     pub ops_timed_out: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:229:5
    |
229 |     pub total_targets_sent: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:230:5
    |
230 |     pub total_target_successes: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:231:5
    |
231 |     pub total_target_failures: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/fanout.rs:232:5
    |
232 |     pub in_flight: usize,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:182:5
    |
182 |     pub rounds_started: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:183:5
    |
183 |     pub rounds_achieved: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:184:5
    |
184 |     pub rounds_failed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:185:5
    |
185 |     pub rounds_expired: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:186:5
    |
186 |     pub total_votes: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:187:5
    |
187 |     pub total_approvals: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:188:5
    |
188 |     pub total_rejections: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:229:5
    |
229 |     pub rounds_started: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:230:5
    |
230 |     pub rounds_achieved: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:231:5
    |
231 |     pub rounds_failed: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:232:5
    |
232 |     pub rounds_expired: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:233:5
    |
233 |     pub total_votes: u64,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:234:5
    |
234 |     pub total_approvals: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:235:5
    |
235 |     pub total_rejections: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/quorum.rs:236:5
    |
236 |     pub active_rounds: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/node_blacklist.rs:12:1
   |
12 | pub enum BlacklistReason {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/node_blacklist.rs:13:5
   |
13 |     ConnectionFailed,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/node_blacklist.rs:14:5
   |
14 |     ErrorResponse(String),
   |     ^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/node_blacklist.rs:15:5
   |
15 |     LatencyThreshold,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/node_blacklist.rs:16:5
   |
16 |     Manual,
   |     ^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/node_blacklist.rs:20:1
   |
20 | pub struct BlacklistEntry {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:21:5
   |
21 |     pub node_id: [u8; 16],
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:22:5
   |
22 |     pub reason: BlacklistReason,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:23:5
   |
23 |     pub added_at_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:24:5
   |
24 |     pub expires_at_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:25:5
   |
25 |     pub failure_count: u32,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/node_blacklist.rs:29:5
   |
29 |     pub fn is_expired(&self, now_ms: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/node_blacklist.rs:35:1
   |
35 | pub struct BlacklistConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:36:5
   |
36 |     pub base_backoff_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:37:5
   |
37 |     pub max_backoff_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:38:5
   |
38 |     pub exponential: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/node_blacklist.rs:39:5
   |
39 |     pub max_entries: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/node_blacklist.rs:53:1
   |
53 | pub struct NodeBlacklist {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-transport/src/node_blacklist.rs:60:5
   |
60 |     pub fn new(config: BlacklistConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/node_blacklist.rs:68:5
   |
68 |     pub fn blacklist(&self, node_id: [u8; 16], reason: BlacklistReason, now_ms: u64) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:103:5
    |
103 |     pub fn remove(&self, node_id: &[u8; 16]) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:110:5
    |
110 |     pub fn is_blacklisted(&self, node_id: &[u8; 16], now_ms: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:123:5
    |
123 |     pub fn expire(&self, now_ms: u64) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:136:5
    |
136 |     pub fn entry(&self, node_id: &[u8; 16], now_ms: u64) -> Option<BlacklistEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:144:5
    |
144 |     pub fn active_entries(&self, now_ms: u64) -> Vec<BlacklistEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:153:5
    |
153 |     pub fn filter_available<'a>(&self, nodes: &'a [[u8; 16]], now_ms: u64) -> Vec<&'a [u8; 16]> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:160:5
    |
160 |     pub fn active_count(&self, now_ms: u64) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:165:5
    |
165 |     pub fn stats(&self) -> Arc<BlacklistStats> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/node_blacklist.rs:170:1
    |
170 | pub struct BlacklistStats {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:171:5
    |
171 |     pub nodes_blacklisted: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:172:5
    |
172 |     pub nodes_removed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:173:5
    |
173 |     pub nodes_expired: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:174:5
    |
174 |     pub blacklist_checks: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:175:5
    |
175 |     pub checks_hit: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/node_blacklist.rs:179:5
    |
179 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/node_blacklist.rs:189:5
    |
189 |     pub fn snapshot(&self, active_count: usize) -> BlacklistStatsSnapshot {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/node_blacklist.rs:202:1
    |
202 | pub struct BlacklistStatsSnapshot {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:203:5
    |
203 |     pub nodes_blacklisted: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:204:5
    |
204 |     pub nodes_removed: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:205:5
    |
205 |     pub nodes_expired: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:206:5
    |
206 |     pub blacklist_checks: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:207:5
    |
207 |     pub checks_hit: u64,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/node_blacklist.rs:208:5
    |
208 |     pub active_count: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/read_repair.rs:13:1
   |
13 | pub struct RepairId(pub u64);
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/read_repair.rs:16:1
   |
16 | pub enum ShardRepairState {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:17:5
   |
17 |     Fetching,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:18:5
   |
18 |     Fetched,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:19:5
   |
19 |     Failed,
   |     ^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:20:5
   |
20 |     Missing,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:21:5
   |
21 |     Reconstructing,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:22:5
   |
22 |     Repaired,
   |     ^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/read_repair.rs:26:1
   |
26 | pub struct RepairShard {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:27:5
   |
27 |     pub node_id: [u8; 16],
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:28:5
   |
28 |     pub shard_index: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:29:5
   |
29 |     pub state: ShardRepairState,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/read_repair.rs:33:1
   |
33 | pub enum RepairPriority {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:34:5
   |
34 |     Background,
   |     ^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:35:5
   |
35 |     Foreground,
   |     ^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/read_repair.rs:39:1
   |
39 | pub struct ReadRepairConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:40:5
   |
40 |     pub timeout_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:41:5
   |
41 |     pub max_concurrent: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/read_repair.rs:54:1
   |
54 | pub enum RepairError {
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:56:5
   |
56 |     NotFound(RepairId),
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:58:5
   |
58 |     TooManyConcurrent(usize),
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:60:5
   |
60 |     InsufficientShards { available: usize, needed: usize },
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:60:26
   |
60 |     InsufficientShards { available: usize, needed: usize },
   |                          ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:60:44
   |
60 |     InsufficientShards { available: usize, needed: usize },
   |                                            ^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:62:5
   |
62 |     AlreadyCompleted(RepairId),
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/read_repair.rs:66:1
   |
66 | pub enum RepairOpState {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:67:5
   |
67 |     Fetching,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:68:5
   |
68 |     Reconstructing,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:69:5
   |
69 |     WritingBack,
   |     ^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:70:5
   |
70 |     Complete,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:71:5
   |
71 |     Failed,
   |     ^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/read_repair.rs:72:5
   |
72 |     TimedOut,
   |     ^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/read_repair.rs:75:1
   |
75 | pub struct RepairOp {
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:76:5
   |
76 |     pub id: RepairId,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:77:5
   |
77 |     pub segment_id: u64,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:78:5
   |
78 |     pub priority: RepairPriority,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:79:5
   |
79 |     pub shards: Vec<RepairShard>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:80:5
   |
80 |     pub state: RepairOpState,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:81:5
   |
81 |     pub created_at_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:82:5
   |
82 |     pub ec_data_shards: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/read_repair.rs:83:5
   |
83 |     pub ec_parity_shards: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-transport/src/read_repair.rs:87:5
   |
87 | /     pub fn new(
88 | |         id: RepairId,
89 | |         segment_id: u64,
90 | |         priority: RepairPriority,
...  |
94 | |         now_ms: u64,
95 | |     ) -> Self {
   | |_____________^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:108:5
    |
108 |     pub fn record_fetch(&mut self, node_id: &[u8; 16], success: bool) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:121:5
    |
121 |     pub fn begin_reconstruct(&mut self) -> Result<(), RepairError> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:137:5
    |
137 |     pub fn begin_writeback(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:141:5
    |
141 |     pub fn complete(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:150:5
    |
150 |     pub fn fail(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:154:5
    |
154 |     pub fn check_timeout(&mut self, now_ms: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:164:5
    |
164 |     pub fn fetched_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:171:5
    |
171 |     pub fn missing_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:178:5
    |
178 |     pub fn can_reconstruct(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/read_repair.rs:183:1
    |
183 | pub struct ReadRepairManager {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/read_repair.rs:191:5
    |
191 |     pub fn new(config: ReadRepairConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:200:5
    |
200 | /     pub fn start_repair(
201 | |         &self,
202 | |         segment_id: u64,
203 | |         priority: RepairPriority,
...   |
207 | |         now_ms: u64,
208 | |     ) -> Result<RepairId, RepairError> {
    | |______________________________________^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:248:5
    |
248 | /     pub fn record_fetch(
249 | |         &self,
250 | |         id: RepairId,
251 | |         node_id: &[u8; 16],
252 | |         success: bool,
253 | |     ) -> Option<RepairOpState> {
    | |______________________________^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:260:5
    |
260 |     pub fn begin_reconstruct(&self, id: RepairId) -> Result<RepairOpState, RepairError> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:275:5
    |
275 |     pub fn complete_repair(&self, id: RepairId) -> Result<(), RepairError> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:290:5
    |
290 |     pub fn check_timeouts(&self, now_ms: u64) -> Vec<RepairId> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:304:5
    |
304 |     pub fn remove(&self, id: RepairId) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:313:5
    |
313 |     pub fn active_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:318:5
    |
318 |     pub fn stats(&self) -> Arc<ReadRepairStats> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/read_repair.rs:323:1
    |
323 | pub struct ReadRepairStats {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:324:5
    |
324 |     pub repairs_started: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:325:5
    |
325 |     pub repairs_completed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:326:5
    |
326 |     pub repairs_failed: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:327:5
    |
327 |     pub repairs_timed_out: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:328:5
    |
328 |     pub shards_repaired: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:329:5
    |
329 |     pub foreground_repairs: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:330:5
    |
330 |     pub background_repairs: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/read_repair.rs:334:5
    |
334 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/read_repair.rs:346:5
    |
346 |     pub fn snapshot(&self, active_repairs: usize) -> ReadRepairStatsSnapshot {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/read_repair.rs:361:1
    |
361 | pub struct ReadRepairStatsSnapshot {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:362:5
    |
362 |     pub repairs_started: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:363:5
    |
363 |     pub repairs_completed: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:364:5
    |
364 |     pub repairs_failed: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:365:5
    |
365 |     pub repairs_timed_out: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:366:5
    |
366 |     pub shards_repaired: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:367:5
    |
367 |     pub foreground_repairs: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:368:5
    |
368 |     pub background_repairs: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/read_repair.rs:369:5
    |
369 |     pub active_repairs: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/repl_state.rs:11:1
   |
11 | pub struct JournalSeq(pub u64);
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/repl_state.rs:14:5
   |
14 |     pub fn next(self) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/repl_state.rs:18:5
   |
18 |     pub fn is_before(self, other: Self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-transport/src/repl_state.rs:24:1
   |
24 | pub enum ReplState {
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:25:5
   |
25 |     Idle,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:26:5
   |
26 |     Syncing,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:27:5
   |
27 |     Live,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:28:5
   |
28 |     Disconnected,
   |     ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-transport/src/repl_state.rs:29:5
   |
29 |     NeedsResync,
   |     ^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/repl_state.rs:33:1
   |
33 | pub struct JournalEntryRecord {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:34:5
   |
34 |     pub seq: JournalSeq,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:35:5
   |
35 |     pub size_bytes: u32,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:36:5
   |
36 |     pub written_at_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/repl_state.rs:40:1
   |
40 | pub struct ReplStateConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:41:5
   |
41 |     pub max_inflight: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:42:5
   |
42 |     pub max_lag_entries: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-transport/src/repl_state.rs:43:5
   |
43 |     pub connection_timeout_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-transport/src/repl_state.rs:56:1
   |
56 | pub struct JournalReplChannel {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-transport/src/repl_state.rs:68:5
   |
68 |     pub fn new(peer_id: [u8; 16], config: ReplStateConfig, now_ms: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-transport/src/repl_state.rs:81:5
   |
81 |     pub fn advance_local(&mut self, entry: JournalEntryRecord, now_ms: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:103:5
    |
103 |     pub fn ack(&mut self, seq: JournalSeq, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:120:5
    |
120 |     pub fn check_timeout(&mut self, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:128:5
    |
128 |     pub fn connect(&mut self, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:135:5
    |
135 |     pub fn mark_live(&mut self, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:142:5
    |
142 |     pub fn disconnect(&mut self, now_ms: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:148:5
    |
148 |     pub fn state(&self) -> ReplState {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:152:5
    |
152 |     pub fn lag(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:156:5
    |
156 |     pub fn inflight_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:160:5
    |
160 |     pub fn is_caught_up(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:164:5
    |
164 |     pub fn peer_id(&self) -> [u8; 16] {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:168:5
    |
168 |     pub fn stats(&self) -> Arc<JournalReplChannelStats> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/repl_state.rs:173:1
    |
173 | pub struct JournalReplChannelStats {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:174:5
    |
174 |     pub entries_sent: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:175:5
    |
175 |     pub entries_acked: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:176:5
    |
176 |     pub ack_timeouts: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:177:5
    |
177 |     pub disconnections: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:178:5
    |
178 |     pub resync_events: AtomicU64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-transport/src/repl_state.rs:182:5
    |
182 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-transport/src/repl_state.rs:192:5
    |
192 | /     pub fn snapshot(
193 | |         &self,
194 | |         lag: u64,
195 | |         inflight: usize,
196 | |         state: ReplState,
197 | |     ) -> JournalReplChannelStatsSnapshot {
    | |________________________________________^

warning: missing documentation for a struct
   --> crates/claudefs-transport/src/repl_state.rs:212:1
    |
212 | pub struct JournalReplChannelStatsSnapshot {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:213:5
    |
213 |     pub entries_sent: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:214:5
    |
214 |     pub entries_acked: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:215:5
    |
215 |     pub ack_timeouts: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:216:5
    |
216 |     pub disconnections: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:217:5
    |
217 |     pub resync_events: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:218:5
    |
218 |     pub current_lag: u64,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-transport/src/repl_state.rs:219:5
    |
219 |     pub inflight_count: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field

...59520 bytes truncated...

The tool call succeeded but the output was truncated. Full output saved to: /home/cfs/.local/share/opencode/tool-output/tool_d9cfdf62b001EXe3ncB3j84stt
Use the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context.
[0m[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-transport -- prometheus_exporter 2>&1 | tail -30
test prometheus_exporter::tests::test_empty_optional_stats ... ok
test prometheus_exporter::tests::test_concurrent_metric_updates ... ok
test prometheus_exporter::tests::test_trace_stats_export ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 1539 filtered out; finished in 0.00s

   Doc-tests claudefs_transport

running 1 test
test crates/claudefs-transport/src/prometheus_exporter.rs - prometheus_exporter::PrometheusTransportMetrics<'a>::scrape (line 51) ... FAILED

failures:

---- crates/claudefs-transport/src/prometheus_exporter.rs - prometheus_exporter::PrometheusTransportMetrics<'a>::scrape (line 51) stdout ----
error: missing `fn` or `struct` for function or struct definition
 --> crates/claudefs-transport/src/prometheus_exporter.rs:52:1
  |
1 | HELP <name> <description>
  | ^^^^

error: aborting due to 1 previous error

Couldn't compile the test.

failures:
    crates/claudefs-transport/src/prometheus_exporter.rs - prometheus_exporter::PrometheusTransportMetrics<'a>::scrape (line 51)

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out; finished in 0.02s

error: doctest failed, to rerun pass `-p claudefs-transport --doc`

[0m[0m← [0mEdit crates/claudefs-transport/src/prometheus_exporter.rsIndex: /home/cfs/claudefs/crates/claudefs-transport/src/prometheus_exporter.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/prometheus_exporter.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/prometheus_exporter.rs
@@ -46,14 +46,13 @@
 }
 
 /// Scrapes all metrics and returns them in Prometheus text format.
 ///
-/// Output follows the Prometheus exposition format:
-/// ```
-/// # HELP <name> <description>
-/// # TYPE <name> <type>
-/// <name>{<labels>} <value>
-/// ```
+/// Output follows the Prometheus exposition format with HELP, TYPE comments
+/// and metric values. Each metric is formatted as:
+/// - `# HELP <name> <description>`
+/// - `# TYPE <name> counter|gauge`
+/// - `<name>{<labels>} <value>`
 pub fn scrape(&self) -> String {
     let mut output = String::new();
 
     self.write_counters(&mut output);
[0m



test prometheus_exporter::tests::test_backpressure_level_export ... ok
test prometheus_exporter::tests::test_display_trait ... ok
test prometheus_exporter::tests::test_metrics_values_correct ... ok
test prometheus_exporter::tests::test_prometheus_transport_metrics_creation ... ok
test prometheus_exporter::tests::test_pool_stats_export ... ok
test prometheus_exporter::tests::test_empty_optional_stats ... ok
test prometheus_exporter::tests::test_scrape_contains_all_expected_metrics ... ok
test prometheus_exporter::tests::test_trace_stats_export ... ok
test prometheus_exporter::tests::test_scrape_output_format ... ok
test prometheus_exporter::tests::test_qos_stats_export ... ok
test prometheus_exporter::tests::test_concurrent_metric_updates ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 1539 filtered out; finished in 0.00s

   Doc-tests claudefs_transport

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; finished in 0.00s[0m

[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-transport -- prometheus_exporter 2>&1 | tail -20All 13 tests pass. Let me verify the final state of all modified files:

[0m→ [0mRead crates/claudefs-transport/Cargo.toml[0m✱ [0mGrep "prometheus_exporter"[90m in crates/claudefs-transport/src · 2 matches[0m
[0m[0m$ [0mrm /home/cfs/claudefs/transport_prometheus_input.md /home/cfs/claudefs/transport_prometheus_output.md

Implementation complete. Here's the summary:

**Created:**
- `crates/claudefs-transport/src/prometheus_exporter.rs` - New module with `PrometheusTransportMetrics` struct that:
  - Wraps references to TransportMetrics, TraceAggregatorStats, PoolStatsSnapshot, BackpressureStatsSnapshot, QosStats
  - Provides `scrape()` method returning Prometheus text format
  - Implements `std::fmt::Display` 
  - Thread-safe with atomic reads (no lock contention)
  - Handles optional stats gracefully
  - 13 comprehensive tests

**Modified:**
- `Cargo.toml` - Added `prometheus = "0.13"` dependency
- `lib.rs` - Added `pub mod prometheus_exporter` and `pub use prometheus_exporter::PrometheusTransportMetrics`

**Exported Metrics:**
- Counters: requests_sent/received, responses_sent/received, errors, retries, timeouts, connections, health checks, backpressure signals
- Gauges: active_connections, bytes_sent/received, pool idle/active/total, backpressure_level, QoS per-class metrics
- Trace aggregator: traces_recorded, active_traces
