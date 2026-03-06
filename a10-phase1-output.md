[0m> build · accounts/fireworks/models/minimax-m2p5[0m


[0m→ [0mRead crates/claudefs-security/src/meta_client_session_security_tests.rs
[0m→ [0mRead crates/claudefs-security/src/transport_trace_aggregator_security_tests.rs
[0m→ [0mRead crates/claudefs-security/src/transport_bandwidth_shaper_security_tests.rs
I'll start by examining the test files with compilation errors and understanding the actual APIs in the transport and meta crates.
[0m✱ [0mGrep "pub struct SessionManager"[90m in crates/claudefs-meta · 2 matches[0m
[0m✱ [0mGrep "pub struct TraceLatencyStats"[90m in crates/claudefs-transport · 1 match[0m
[0m✱ [0mGrep "pub struct TokenBucket"[90m in crates/claudefs-transport · 2 matches[0m
Now I need to explore the actual API definitions in the meta and transport crates to understand what public methods are available.
[0m→ [0mRead crates/claudefs-meta/src/client_session.rs[90m [offset=135, limit=100][0m
[0m→ [0mRead crates/claudefs-transport/src/trace_aggregator.rs[90m [offset=135, limit=60][0m
[0m→ [0mRead crates/claudefs-transport/src/bandwidth_shaper.rs[90m [offset=70, limit=100][0m
[0m✱ [0mGrep "pub struct BandwidthStats"[90m in crates/claudefs-transport · 2 matches[0m
[0m→ [0mRead crates/claudefs-transport/src/bandwidth_shaper.rs[90m [limit=30, offset=230][0m
[0m✱ [0mGrep "pub fn stats"[90m in crates/claudefs-transport/src/bandwidth_shaper.rs · 1 match[0m
[0m→ [0mRead crates/claudefs-transport/src/bandwidth_shaper.rs[90m [limit=30, offset=338][0m
[0m✱ [0mGrep "pub fn"[90m in crates/claudefs-meta/src/client_session.rs · 16 matches[0m
[0m✱ [0mGrep "pub struct SessionMetrics"[90m in crates/claudefs-meta/src/client_session.rs · 1 match[0m
[0m→ [0mRead crates/claudefs-meta/src/client_session.rs[90m [limit=20, offset=112][0m
[0m✱ [0mGrep "pub struct PendingOperation"[90m in crates/claudefs-meta/src/client_session.rs · 1 match[0m
[0m→ [0mRead crates/claudefs-meta/src/client_session.rs[90m [limit=20, offset=67][0m
[0m


Updating crates.io index
     Locking 6 packages to latest compatible versions
      Adding bb8 v0.8.6 (available: v0.9.1)
      Adding futures v0.1.31
      Adding jsonwebtoken v9.3.1 (available: v10.3.0)
      Adding simple_asn1 v0.6.4
      Adding tower-layer v0.1.0 (available: v0.3.3)
      Adding tower-service v0.2.0
 Downloading crates ...
  Downloaded bb8 v0.8.6
  Downloaded tower-service v0.2.0
  Downloaded simple_asn1 v0.6.4
  Downloaded jsonwebtoken v9.3.1
  Downloaded tower-layer v0.1.0
  Downloaded futures v0.1.31
   Compiling tokio-util v0.7.18
   Compiling ring v0.17.14
   Compiling thiserror v2.0.18
   Compiling thiserror-impl v2.0.18
   Compiling futures v0.1.31
   Compiling bb8 v0.8.6
   Compiling claudefs-storage v0.1.0 (/home/cfs/claudefs/crates/claudefs-storage)
warning: unused import: `crate::encryption::EncryptionKey`
 --> crates/claudefs-reduce/src/key_rotation_orchestrator.rs:6:5
  |
6 | use crate::encryption::EncryptionKey;
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `Duration`
  --> crates/claudefs-reduce/src/adaptive_classifier.rs:11:17
   |
11 | use std::time::{Duration, Instant};
   |                 ^^^^^^^^

warning: unused import: `Instant`
  --> crates/claudefs-reduce/src/recovery_enhancer.rs:12:27
   |
12 | use std::time::{Duration, Instant};
   |                           ^^^^^^^

warning: unused import: `std::time::Instant`
  --> crates/claudefs-reduce/src/similarity_tier_stats.rs:10:5
   |
10 | use std::time::Instant;
   |     ^^^^^^^^^^^^^^^^^^

warning: variable does not need to be mutable
   --> crates/claudefs-reduce/src/recovery_enhancer.rs:362:13
    |
362 |         let mut inconsistencies = Vec::new();
    |             ----^^^^^^^^^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> crates/claudefs-reduce/src/recovery_enhancer.rs:366:17
    |
366 |             let mut inconsistency = InconsistencyRecord {
    |                 ----^^^^^^^^^^^^^
    |                 |
    |                 help: remove this `mut`

warning: unused variable: `inconsistency`
   --> crates/claudefs-reduce/src/recovery_enhancer.rs:366:17
    |
366 |             let mut inconsistency = InconsistencyRecord {
    |                 ^^^^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_inconsistency`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `bytes_processed`
   --> crates/claudefs-reduce/src/similarity_tier_stats.rs:139:9
    |
139 |         bytes_processed: u64,
    |         ^^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_bytes_processed`

warning: method `len` is never used
   --> crates/claudefs-reduce/src/similarity_coordinator.rs:201:8
    |
158 | impl<K: Eq + Hash + Clone, V: Clone> LruCache<K, V> {
    | --------------------------------------------------- method in this implementation
...
201 |     fn len(&self) -> usize {
    |        ^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: fields `feature_extractor` and `delta_compressor` are never read
   --> crates/claudefs-reduce/src/similarity_coordinator.rs:223:5
    |
220 | pub struct SimilarityCoordinator {
    |            --------------------- fields in this struct
...
223 |     feature_extractor: FeatureExtractor,
    |     ^^^^^^^^^^^^^^^^^
224 |     delta_compressor: DeltaCompressor,[0m$ [0mcd /home/cfs/claudefs && cargo build --package claudefs-security 2>&1 | head -100[0m
[0m$ [0mcd /home/cfs/claudefs && cargo build --package claudefs-security 2>&1 | tail -60
[0m
|
59 | pub struct ReadAmplificationTracker {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-reduce/src/read_amplification.rs:66:5
   |
66 |     pub fn new(config: ReadAmplificationConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-reduce/src/read_amplification.rs:74:5
   |
74 |     pub fn record(&mut self, event: ReadEvent) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-reduce/src/read_amplification.rs:100:5
    |
100 |     pub fn rolling_avg_amplification(&self) -> f64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-reduce/src/read_amplification.rs:108:5
    |
108 |     pub fn window_max(&self) -> f64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-reduce/src/read_amplification.rs:116:5
    |
116 |     pub fn stats(&self) -> &ReadAmplificationStats {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-reduce/src/read_amplification.rs:119:5
    |
119 |     pub fn window_size(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-storage` (lib) generated 504 warnings (run `cargo fix --lib -p claudefs-storage` to apply 4 suggestions)
warning: `claudefs-reduce` (lib) generated 625 warnings (run `cargo fix --lib -p claudefs-reduce` to apply 8 suggestions)
   Compiling simple_asn1 v0.6.4
   Compiling tower-layer v0.1.0
   Compiling claudefs-fuse v0.1.0 (/home/cfs/claudefs/crates/claudefs-fuse)
error[E0599]: no method named `or_insert_default` found for enum `std::collections::hash_map::Entry<'a, K, V>` in the current scope
   --> crates/claudefs-fuse/src/range_lock.rs:208:46
    |
208 |         let table = self.tables.entry(inode).or_insert_default();
    |                                              ^^^^^^^^^^^^^^^^^
    |
help: there is a method `or_default` with a similar name
    |
208 -         let table = self.tables.entry(inode).or_insert_default();
208 +         let table = self.tables.entry(inode).or_default();
    |

For more information about this error, try `rustc --explain E0599`.
error: could not compile `claudefs-fuse` (lib) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
[0m$ [0mcd /home/cfs/claudefs && cargo check --package claudefs-security 2>&1 | grep -A5 "^error"
error[E0599]: no method named `or_insert_default` found for enum `std::collections::hash_map::Entry<'a, K, V>` in the current scope
   --> crates/claudefs-fuse/src/range_lock.rs:208:46
    |
208 |         let table = self.tables.entry(inode).or_insert_default();
    |                                              ^^^^^^^^^^^^^^^^^
    |
--
error: could not compile `claudefs-fuse` (lib) due to 1 previous error
warning: build failed, waiting for other jobs to finish...

[0m[0m$ [0mcd /home/cfs/claudefs && cargo test --package claudefs-security --no-run 2>&1 | tail -50
|
256 |     pub async fn current_depths(&self) -> Vec<(WorkloadClass, usize)> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-storage/src/io_scheduler_fairness.rs:261:5
    |
261 |     pub async fn reset(&self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-storage/src/io_scheduler_fairness.rs:273:1
    |
273 | pub enum FairnessError {
    | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-storage/src/io_scheduler_fairness.rs:275:5
    |
275 |     Backpressure,
    |     ^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-storage/src/io_scheduler_fairness.rs:277:5
    |
277 |     InvalidClass,
    |     ^^^^^^^^^^^^

   Compiling claudefs-fuse v0.1.0 (/home/cfs/claudefs/crates/claudefs-fuse)
warning: `claudefs-storage` (lib) generated 504 warnings (run `cargo fix --lib -p claudefs-storage` to apply 4 suggestions)
   Compiling rustls v0.23.37
   Compiling h2 v0.4.13
   Compiling tower v0.4.13
error[E0599]: no method named `or_insert_default` found for enum `std::collections::hash_map::Entry<'a, K, V>` in the current scope
   --> crates/claudefs-fuse/src/range_lock.rs:208:46
    |
208 |         let table = self.tables.entry(inode).or_insert_default();
    |                                              ^^^^^^^^^^^^^^^^^
    |
help: there is a method `or_default` with a similar name
    |
208 -         let table = self.tables.entry(inode).or_insert_default();
208 +         let table = self.tables.entry(inode).or_default();
    |

For more information about this error, try `rustc --explain E0599`.
   Compiling x509-parser v0.16.0
   Compiling jsonwebtoken v9.3.1
error: could not compile `claudefs-fuse` (lib) due to 1 previous error
warning: build failed, waiting for other jobs to finish...[0m

[0m→ [0mRead crates/claudefs-fuse/src/range_lock.rs[90m [limit=15, offset=205][0m
[0m[0m$ [0mcd /home/cfs/claudefs && cargo check --package claudefs-fuse 2>&1 | grep -B5 -A5 "or_insert_default"

[0m[0m$ [0mcd /home/cfs/claudefs && cargo check --package claudefs-fuse 2>&1

[0m

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

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:12:5
   |
12 |     pub total_blocks: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:13:5
   |
13 |     pub free_blocks: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:14:5
   |
14 |     pub available_blocks: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:15:5
   |
15 |     pub total_inodes: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:16:5
   |
16 |     pub free_inodes: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:17:5
   |
17 |     pub block_size: u32,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:18:5
   |
18 |     pub max_name_len: u32,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:19:5
   |
19 |     pub last_updated: Option<Instant>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:77:5
   |
77 |     pub ttl_secs: u64,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:78:5
   |
78 |     pub refresh_interval_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:93:5
   |
93 |     pub hits: u64,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:94:5
   |
94 |     pub refreshes: u64,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/fsinfo.rs:95:5
   |
95 |     pub age_secs: u64,
   |     ^^^^^^^^^^^^^^^^^

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
   |
37 |     Failed(String),
   |     ^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/fsync_barrier.rs:40:1
   |
40 | pub struct WriteBarrier {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/fsync_barrier.rs:50:5
   |
50 |     pub fn new(barrier_id: BarrierId, inode: u64, kind: BarrierKind, sequence: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:61:5
   |
61 |     pub fn is_complete(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:68:5
   |
68 |     pub fn is_pending(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:72:5
   |
72 |     pub fn mark_flushing(&mut self) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:76:5
   |
76 |     pub fn mark_committed(&mut self) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:80:5
   |
80 |     pub fn mark_failed(&mut self, reason: &str) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:84:5
   |
84 |     pub fn elapsed_ms(&self) -> u64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:88:5
   |
88 |     pub fn barrier_id(&self) -> BarrierId {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:92:5
   |
92 |     pub fn inode(&self) -> u64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/fsync_barrier.rs:96:5
   |
96 |     pub fn kind(&self) -> BarrierKind {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:100:5
    |
100 |     pub fn state(&self) -> &BarrierState {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:104:5
    |
104 |     pub fn sequence(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-fuse/src/fsync_barrier.rs:110:1
    |
110 | pub enum FsyncMode {
    | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/fsync_barrier.rs:111:5
    |
111 |     Sync,
    |     ^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/fsync_barrier.rs:112:5
    |
112 |     Async,
    |     ^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/fsync_barrier.rs:113:5
    |
113 |     Ordered { max_delay_ms: u64 },
    |     ^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-fuse/src/fsync_barrier.rs:113:15
    |
113 |     Ordered { max_delay_ms: u64 },
    |               ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/fsync_barrier.rs:123:1
    |
123 | pub struct JournalEntry {
    | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/fsync_barrier.rs:132:5
    |
132 |     pub fn new(entry_id: u64, inode: u64, operation: &str, version: u64) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:142:5
    |
142 |     pub fn entry_id(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:146:5
    |
146 |     pub fn inode(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:150:5
    |
150 |     pub fn operation(&self) -> &str {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:154:5
    |
154 |     pub fn version(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:158:5
    |
158 |     pub fn timestamp(&self) -> SystemTime {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-fuse/src/fsync_barrier.rs:164:1
    |
164 | pub enum JournalError {
    | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/fsync_barrier.rs:166:5
    |
166 |     JournalFull,
    |     ^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/fsync_barrier.rs:169:1
    |
169 | pub struct FsyncJournal {
    | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/fsync_barrier.rs:176:5
    |
176 |     pub fn new(max_entries: usize) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:184:5
    |
184 |     pub fn append(&mut self, inode: u64, operation: &str, version: u64) -> Result<u64> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:198:5
    |
198 |     pub fn commit_up_to(&mut self, entry_id: u64) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:208:5
    |
208 |     pub fn pending_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:212:5
    |
212 |     pub fn is_full(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:216:5
    |
216 |     pub fn entries_for_inode(&self, inode: u64) -> Vec<&JournalEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:220:5
    |
220 |     pub fn entries(&self) -> &[JournalEntry] {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/fsync_barrier.rs:225:1
    |
225 | pub struct BarrierManager {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/fsync_barrier.rs:235:5
    |
235 |     pub fn new(mode: FsyncMode) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:245:5
    |
245 |     pub fn create_barrier(&mut self, inode: u64, kind: BarrierKind) -> BarrierId {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:262:5
    |
262 |     pub fn get_barrier(&self, id: &BarrierId) -> Option<&WriteBarrier> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:266:5
    |
266 |     pub fn get_barrier_mut(&mut self, id: &BarrierId) -> Option<&mut WriteBarrier> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:270:5
    |
270 |     pub fn flush_barrier(&mut self, id: &BarrierId) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:283:5
    |
283 |     pub fn commit_barrier(&mut self, id: &BarrierId) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:296:5
    |
296 |     pub fn fail_barrier(&mut self, id: &BarrierId, reason: &str) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:309:5
    |
309 |     pub fn pending_barriers(&self) -> Vec<&WriteBarrier> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:313:5
    |
313 |     pub fn committed_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:320:5
    |
320 |     pub fn failed_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:327:5
    |
327 |     pub fn record_fsync(&mut self, inode: u64, version: u64) -> Result<u64> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:331:5
    |
331 |     pub fn journal(&self) -> &FsyncJournal {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/fsync_barrier.rs:335:5
    |
335 |     pub fn journal_mut(&mut self) -> &mut FsyncJournal {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/lookup_cache.rs:13:5
   |
13 |     RegularFile,
   |     ^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/lookup_cache.rs:14:5
   |
14 |     Directory,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/lookup_cache.rs:15:5
   |
15 |     Symlink,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/lookup_cache.rs:16:5
   |
16 |     Other,
   |     ^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:34:5
   |
34 |     pub child_ino: u64,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:35:5
   |
35 |     pub entry_type: EntryType,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:36:5
   |
36 |     pub generation: u64,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:37:5
   |
37 |     pub cached_at: Instant,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:50:5
   |
50 |     pub capacity: usize,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:51:5
   |
51 |     pub ttl_secs: u64,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:52:5
   |
52 |     pub negative_ttl_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:68:5
   |
68 |     pub hits: u64,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:69:5
   |
69 |     pub misses: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:70:5
   |
70 |     pub negative_hits: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:71:5
   |
71 |     pub evictions: u64,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:72:5
   |
72 |     pub invalidations: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/lookup_cache.rs:73:5
   |
73 |     pub entries: usize,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/mmap.rs:10:1
   |
10 | pub struct MmapProt {
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:11:5
   |
11 |     pub read: bool,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:12:5
   |
12 |     pub write: bool,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:13:5
   |
13 |     pub exec: bool,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/mmap.rs:17:1
   |
17 | pub struct MmapRegion {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:18:5
   |
18 |     pub ino: InodeId,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:19:5
   |
19 |     pub fh: u64,
   |     ^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:20:5
   |
20 |     pub offset: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:21:5
   |
21 |     pub length: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:22:5
   |
22 |     pub prot: MmapProt,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:23:5
   |
23 |     pub flags: u32,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/mmap.rs:26:1
   |
26 | pub struct MmapTracker {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/mmap.rs:33:1
   |
33 | pub struct MmapStats {
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:34:5
   |
34 |     pub total_regions: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:35:5
   |
35 |     pub total_bytes_mapped: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mmap.rs:36:5
   |
36 |     pub active_regions: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/mmap.rs:40:5
   |
40 |     pub fn new() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/mmap.rs:48:5
   |
48 |     pub fn register(&mut self, region: MmapRegion) -> u64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/mmap.rs:60:5
   |
60 |     pub fn unregister(&mut self, region_id: u64) -> Option<MmapRegion> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/mmap.rs:71:5
   |
71 |     pub fn regions_for_inode(&self, ino: InodeId) -> Vec<&MmapRegion> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/mmap.rs:75:5
   |
75 |     pub fn has_writable_mapping(&self, ino: InodeId) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/mmap.rs:79:5
   |
79 |     pub fn stats(&self) -> &MmapStats {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/mmap.rs:83:5
   |
83 |     pub fn total_mapped_bytes(&self) -> u64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/mmap.rs:87:5
   |
87 |     pub fn count(&self) -> usize {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/range_lock.rs:17:5
   |
17 |     Read,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/range_lock.rs:18:5
   |
18 |     Write,
   |     ^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/range_lock.rs:36:5
   |
36 |     pub owner_pid: u32,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/range_lock.rs:37:5
   |
37 |     pub lock_type: LockType,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/range_lock.rs:38:5
   |
38 |     pub start: u64,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/range_lock.rs:39:5
   |
39 |     pub end: u64,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/range_lock.rs:40:5
   |
40 |     pub acquired_at: Instant,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/range_lock.rs:86:5
   |
86 |     pub conflicting_lock: ByteRangeLock,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-fuse` (lib) generated 124 warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s[0m$ [0mcd /home/cfs/claudefs && cargo test --package claudefs-security --no-run 2>&1 | grep "^error"
<bash_metadata>
bash tool terminated command after exceeding timeout 120000 ms
</bash_metadata>

[0m[91m[1mError: [0mUnable to connect. Is the computer able to access the url?
