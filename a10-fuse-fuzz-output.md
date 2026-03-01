[0m
> build · accounts/fireworks/models/minimax-m2p5
[0m
[0m→ [0mRead crates/claudefs-security/src/lib.rs
[0m→ [0mRead crates/claudefs-security/src/fuzz_protocol.rs
[0m→ [0mRead crates/claudefs-security/src/fuzz_message.rs
[0m
[0m← [0mWrite crates/claudefs-security/src/fuzz_fuse.rs
Wrote file successfully.
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo build -p claudefs-security 2>&1
Compiling openssl v0.10.75
   Compiling tower-http v0.6.8
   Compiling fuser v0.15.1
   Compiling io-uring v0.7.11
   Compiling claudefs-transport v0.1.0 (/home/cfs/claudefs/crates/claudefs-transport)
   Compiling claudefs-repl v0.1.0 (/home/cfs/claudefs/crates/claudefs-repl)
   Compiling claudefs-reduce v0.1.0 (/home/cfs/claudefs/crates/claudefs-reduce)
   Compiling claudefs-gateway v0.1.0 (/home/cfs/claudefs/crates/claudefs-gateway)
warning: unused import: `rand::rngs::OsRng`
 --> crates/claudefs-gateway/src/nfs_delegation.rs:3:5
  |
3 | use rand::rngs::OsRng;
  |     ^^^^^^^^^^^^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::net::IpAddr`
 --> crates/claudefs-gateway/src/nfs_export.rs:4:5
  |
4 | use std::net::IpAddr;
  |     ^^^^^^^^^^^^^^^^

warning: unused import: `HashSet`
 --> crates/claudefs-gateway/src/nfs_v4_session.rs:8:33
  |
8 | use std::collections::{HashMap, HashSet};
  |                                 ^^^^^^^

   Compiling claudefs-storage v0.1.0 (/home/cfs/claudefs/crates/claudefs-storage)
warning: missing documentation for a method
   --> crates/claudefs-reduce/src/key_manager.rs:216:5
    |
216 |     pub fn clear_history(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
note: the lint level is defined here
   --> crates/claudefs-reduce/src/lib.rs:1:9
    |
  1 | #![warn(missing_docs)]
    |         ^^^^^^^^^^^^

   Compiling claudefs-fuse v0.1.0 (/home/cfs/claudefs/crates/claudefs-fuse)
warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.[0m
Wrote file successfully.
[0m[0m← [0mWrite crates/claudefs-security/src/fuzz_fuse.rs

^
   |
note: the lint level is defined here
  --> crates/claudefs-repl/src/lib.rs:1:9
   |
 1 | #![warn(missing_docs)]
   |         ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:18:16
   |
18 |     Failover { primary: SiteId, standby: SiteId },
   |                ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:18:33
   |
18 |     Failover { primary: SiteId, standby: SiteId },
   |                                 ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:20:16
   |
20 |     Recovery { recovering_site: SiteId },
   |                ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:30:9
   |
30 |         site_id: SiteId,
   |         ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:31:9
   |
31 |         detected_at_ns: u64,
   |         ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:35:9
   |
35 |         site_id: SiteId,
   |         ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:36:9
   |
36 |         detected_at_ns: u64,
   |         ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:39:26
   |
39 |     ReplicationLagHigh { site_id: SiteId, lag_ns: u64 },
   |                          ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:39:43
   |
39 |     ReplicationLagHigh { site_id: SiteId, lag_ns: u64 },
   |                                           ^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:41:22
   |
41 |     ManualFailover { target_primary: SiteId },
   |                      ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-repl/src/site_failover.rs:43:24
   |
43 |     RecoveryComplete { site_id: SiteId },
   |                        ^^^^^^^^^^^^^^^

warning: unused imports: `BlockId` and `BlockSize`
  --> crates/claudefs-storage/src/hot_swap.rs:15:20
   |
15 | use crate::block::{BlockId, BlockRef, BlockSize};
   |                    ^^^^^^^            ^^^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused imports: `FuseError` and `Result`
 --> crates/claudefs-fuse/src/cache_coherence.rs:1:13
  |
1 | use crate::{FuseError, Result};
  |             ^^^^^^^^^  ^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::sync::Arc`
 --> crates/claudefs-fuse/src/cache_coherence.rs:5:5
  |
5 | use std::sync::Arc;
  |     ^^^^^^^^^^^^^^

   Compiling native-tls v0.2.18
warning: unused variable: `token`
   --> crates/claudefs-fuse/src/client_auth.rs:117:17
    |
117 |                 token,
    |                 ^^^^^ help: try ignoring the field: `token: _`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `old_fingerprint`
   --> crates/claudefs-fuse/src/client_auth.rs:174:17
    |
174 |                 old_fingerprint,
    |                 ^^^^^^^^^^^^^^^ help: try ignoring the field: `old_fingerprint: _`

warning: unused variable: `cert_fingerprint`
   --> crates/claudefs-fuse/src/client_auth.rs:204:13
    |
204 |             cert_fingerprint,
    |             ^^^^^^^^^^^^^^^^ help: try ignoring the field: `cert_fingerprint: _`

   Compiling tokio-native-tls v0.3.1
warning: unused variable: `pid`
  --> crates/claudefs-fuse/src/io_priority.rs:88:9
   |
88 |         pid: u32,
   |         ^^^ help: if this is intentional, prefix it with an underscore: `_pid`

warning: unused variable: `uid`
  --> crates/claudefs-fuse/src/io_priority.rs:89:9
   |
89 |         uid: u32,
   |         ^^^ help: if this is intentional, prefix it with an underscore: `_uid`

warning: variable does not need to be mutable
   --> crates/claudefs-fuse/src/io_priority.rs:192:13
    |
192 |         let mut budgets = limits.clone();
    |             ----^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

   Compiling hyper-tls v0.6.0
   Compiling reqwest v0.12.28
warning: unused variable: `now_secs`
  --> crates/claudefs-fuse/src/worm.rs:50:37
   |
50 |     pub fn is_append_allowed(&self, now_secs: u64) -> bool {
   |                                     ^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_now_secs`

warning: variable does not need to be mutable
   --> crates/claudefs-fuse/src/worm.rs:253:13
    |
253 |         let mut hold_inos = inos.clone();
    |             ----^^^^^^^^^
    |             |
    |             help: remove this `mut`

warning: field `protocol` is never read
   --> crates/claudefs-fuse/src/cache_coherence.rs:208:5
    |
205 | pub struct CoherenceManager {
    |            ---------------- field in this struct
...
208 |     protocol: CoherenceProtocol,
    |     ^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: field `cert_dir` is never read
  --> crates/claudefs-fuse/src/client_auth.rs:72:5
   |
68 | pub struct ClientAuthManager {
   |            ----------------- field in this struct
...
72 |     cert_dir: String,
   |     ^^^^^^^^

warning: missing documentation for a module
 --> crates/claudefs-fuse/src/lib.rs:5:1
  |
5 | pub mod attr;
  | ^^^^^^^^^^^^
  |
note: the lint level is defined here
 --> crates/claudefs-fuse/src/lib.rs:1:9
  |
1 | #![warn(missing_docs)]
  |         ^^^^^^^^^^^^

warning: missing documentation for a module
 --> crates/claudefs-fuse/src/lib.rs:7:1
  |
7 | pub mod cache;
  | ^^^^^^^^^^^^^

warning: missing documentation for a module
 --> crates/claudefs-fuse/src/lib.rs:8:1
  |
8 | pub mod cache_coherence;
  | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
 --> crates/claudefs-fuse/src/lib.rs:9:1
  |
9 | pub mod capability;
  | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:11:1
   |
11 | pub mod crash_recovery;
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:12:1
   |
12 | pub mod datacache;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:14:1
   |
14 | pub mod dir_cache;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:15:1
   |
15 | pub mod dirnotify;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:16:1
   |
16 | pub mod error;
   | ^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:17:1
   |
17 | pub mod fallocate;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:18:1
   |
18 | pub mod fadvise;
   | ^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:20:1
   |
20 | pub mod flock;
   | ^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:21:1
   |
21 | pub mod health;
   | ^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:22:1
   |
22 | pub mod idmap;
   | ^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:23:1
   |
23 | pub mod inode;
   | ^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:24:1
   |
24 | pub mod interrupt;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:27:1
   |
27 | pub mod migration;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:31:1
   |
31 | pub mod multipath;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:33:1
   |
33 | pub mod openfile;
   | ^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:34:1
   |
34 | pub mod operations;
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:35:1
   |
35 | pub mod otel_trace;
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:37:1
   |
37 | pub mod path_resolver;
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:39:1
   |
39 | pub mod posix_acl;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:40:1
   |
40 | pub mod prefetch;
   | ^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:41:1
   |
41 | pub mod quota_enforce;
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:42:1
   |
42 | pub mod ratelimit;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:43:1
   |
43 | pub mod reconnect;
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:44:1
   |
44 | pub mod sec_policy;
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:47:1
   |
47 | pub mod snapshot;
   | ^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:48:1
   |
48 | pub mod symlink;
   | ^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:50:1
   |
50 | pub mod tracing_client;
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:52:1
   |
52 | pub mod workload_class;
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:54:1
   |
54 | pub mod writebuf;
   | ^^^^^^^^^^^^^^^^

warning: missing documentation for a module
  --> crates/claudefs-fuse/src/lib.rs:55:1
   |
55 | pub mod xattr;
   | ^^^^^^^^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-fuse/src/attr.rs:5:1
  |
5 | pub struct FileAttr {
  | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/attr.rs:6:5
  |
6 |     pub ino: u64,
  |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/attr.rs:7:5
  |
7 |     pub size: u64,
  |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/attr.rs:8:5
  |
8 |     pub blocks: u64,
  |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/attr.rs:9:5
  |
9 |     pub atime: SystemTime,
  |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:10:5
   |
10 |     pub mtime: SystemTime,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:11:5
   |
11 |     pub ctime: SystemTime,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:12:5
   |
12 |     pub kind: FileType,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:13:5
   |
13 |     pub perm: u16,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:14:5
   |
14 |     pub nlink: u32,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:15:5
   |
15 |     pub uid: u32,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:16:5
   |
16 |     pub gid: u32,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:17:5
   |
17 |     pub rdev: u32,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:18:5
   |
18 |     pub blksize: u32,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/attr.rs:19:5
   |
19 |     pub flags: u32,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/attr.rs:23:1
   |
23 | pub enum FileType {
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/attr.rs:24:5
   |
24 |     RegularFile,
   |     ^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/attr.rs:25:5
   |
25 |     Directory,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/attr.rs:26:5
   |
26 |     Symlink,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/attr.rs:27:5
   |
27 |     BlockDevice,
   |     ^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/attr.rs:28:5
   |
28 |     CharDevice,
   |     ^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/attr.rs:29:5
   |
29 |     NamedPipe,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/attr.rs:30:5
   |
30 |     Socket,
   |     ^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/attr.rs:34:5
   |
34 |     pub fn new_file(ino: u64, size: u64, perm: u16, uid: u32, gid: u32) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/attr.rs:55:5
   |
55 |     pub fn new_dir(ino: u64, perm: u16, uid: u32, gid: u32) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/attr.rs:75:5
   |
75 |     pub fn new_symlink(ino: u64, target_len: u64, uid: u32, gid: u32) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/attr.rs:96:5
   |
96 |     pub fn from_inode(entry: &InodeEntry) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a function
   --> crates/claudefs-fuse/src/attr.rs:139:1
    |
139 | pub fn inode_kind_to_file_type(kind: &InodeKind) -> FileType {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a function
   --> crates/claudefs-fuse/src/attr.rs:151:1
    |
151 | pub fn inode_kind_to_fuser_type(kind: &InodeKind) -> fuser::FileType {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a function
   --> crates/claudefs-fuse/src/attr.rs:163:1
    |
163 | pub fn file_attr_to_fuser(attr: &FileAttr, kind: fuser::FileType) -> fuser::FileAttr {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-fuse/src/cache.rs:7:1
  |
7 | pub struct CacheConfig {
  | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/cache.rs:8:5
  |
8 |     pub capacity: usize,
  |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/cache.rs:9:5
  |
9 |     pub ttl_secs: u64,
  |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache.rs:10:5
   |
10 |     pub negative_ttl_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/cache.rs:23:1
   |
23 | pub struct CacheEntry<V> {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache.rs:24:5
   |
24 |     pub value: V,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache.rs:25:5
   |
25 |     pub inserted_at: Instant,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache.rs:26:5
   |
26 |     pub ttl_secs: u64,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/cache.rs:35:1
   |
35 | pub struct MetadataCache {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/cache.rs:43:1
   |
43 | pub struct CacheStats {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache.rs:44:5
   |
44 |     pub hits: u64,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache.rs:45:5
   |
45 |     pub misses: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache.rs:46:5
   |
46 |     pub evictions: u64,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache.rs:47:5
   |
47 |     pub size: usize,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/cache.rs:51:5
   |
51 |     pub fn new(config: CacheConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache.rs:62:5
   |
62 |     pub fn get_attr(&mut self, ino: u64) -> Option<FileAttr> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache.rs:78:5
   |
78 |     pub fn insert_attr(&mut self, ino: u64, attr: FileAttr) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache.rs:93:5
   |
93 |     pub fn invalidate(&mut self, ino: u64) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache.rs:97:5
   |
97 |     pub fn invalidate_children(&mut self, _parent_ino: u64) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache.rs:101:5
    |
101 |     pub fn insert_negative(&mut self, parent_ino: u64, name: &str) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache.rs:106:5
    |
106 |     pub fn is_negative(&mut self, parent_ino: u64, name: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache.rs:119:5
    |
119 |     pub fn stats(&self) -> CacheStats {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache.rs:128:5
    |
128 |     pub fn clear(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache.rs:133:5
    |
133 |     pub fn len(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache.rs:137:5
    |
137 |     pub fn is_empty(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/cache_coherence.rs:11:1
   |
11 | pub enum CoherenceError {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/cache_coherence.rs:13:5
   |
13 |     LeaseNotFound(u64),
   |     ^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/cache_coherence.rs:15:5
   |
15 |     LeaseExpired(LeaseId),
   |     ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/cache_coherence.rs:17:5
   |
17 |     InvalidLeaseState(LeaseId),
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/cache_coherence.rs:19:5
   |
19 |     InvalidVersion(String),
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a type alias
  --> crates/claudefs-fuse/src/cache_coherence.rs:22:1
   |
22 | pub type CoherenceResult<T> = std::result::Result<T, CoherenceError>;
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/cache_coherence.rs:25:1
   |
25 | pub struct LeaseId(u64);
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/cache_coherence.rs:28:5
   |
28 |     pub fn new(id: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/cache_coherence.rs:40:1
   |
40 | pub enum LeaseState {
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/cache_coherence.rs:41:5
   |
41 |     Active,
   |     ^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/cache_coherence.rs:42:5
   |
42 |     Expired,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/cache_coherence.rs:43:5
   |
43 |     Revoked,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/cache_coherence.rs:44:5
   |
44 |     Renewing,
   |     ^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/cache_coherence.rs:48:1
   |
48 | pub struct CacheLease {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache_coherence.rs:49:5
   |
49 |     pub lease_id: LeaseId,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache_coherence.rs:50:5
   |
50 |     pub inode: u64,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache_coherence.rs:51:5
   |
51 |     pub client_id: u64,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache_coherence.rs:52:5
   |
52 |     pub granted_at: Instant,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache_coherence.rs:53:5
   |
53 |     pub duration: Duration,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/cache_coherence.rs:54:5
   |
54 |     pub state: LeaseState,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/cache_coherence.rs:58:5
   |
58 |     pub fn new(lease_id: LeaseId, inode: u64, client_id: u64, duration: Duration) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache_coherence.rs:69:5
   |
69 |     pub fn is_valid(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache_coherence.rs:73:5
   |
73 |     pub fn is_expired(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache_coherence.rs:80:5
   |
80 |     pub fn time_remaining(&self) -> Duration {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache_coherence.rs:88:5
   |
88 |     pub fn revoke(&mut self) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/cache_coherence.rs:93:5
   |
93 |     pub fn renew(&mut self, new_duration: Duration) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-fuse/src/cache_coherence.rs:106:1
    |
106 | pub enum InvalidationReason {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/cache_coherence.rs:107:5
    |
107 |     LeaseExpired,
    |     ^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/cache_coherence.rs:108:5
    |
108 |     RemoteWrite(u64),
    |     ^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/cache_coherence.rs:109:5
    |
109 |     ConflictDetected,
    |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/cache_coherence.rs:110:5
    |
110 |     ExplicitFlush,
    |     ^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/cache_coherence.rs:111:5
    |
111 |     NodeFailover,
    |     ^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/cache_coherence.rs:115:1
    |
115 | pub struct CacheInvalidation {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-fuse/src/cache_coherence.rs:116:5
    |
116 |     pub inode: u64,
    |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-fuse/src/cache_coherence.rs:117:5
    |
117 |     pub reason: InvalidationReason,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-fuse/src/cache_coherence.rs:118:5
    |
118 |     pub version: u64,
    |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-fuse/src/cache_coherence.rs:119:5
    |
119 |     pub timestamp: SystemTime,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/cache_coherence.rs:123:5
    |
123 |     pub fn new(inode: u64, reason: InvalidationReason, version: u64) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/cache_coherence.rs:134:1
    |
134 | pub struct VersionVector {
    | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/cache_coherence.rs:139:5
    |
139 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:145:5
    |
145 |     pub fn get(&self, inode: u64) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:149:5
    |
149 |     pub fn update(&mut self, inode: u64, version: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:162:5
    |
162 |     pub fn conflicts(&self, other: &VersionVector) -> Vec<u64> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:186:5
    |
186 |     pub fn merge(&mut self, other: &VersionVector) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:192:5
    |
192 |     pub fn len(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-fuse/src/cache_coherence.rs:198:1
    |
198 | pub enum CoherenceProtocol {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/cache_coherence.rs:200:5
    |
200 |     CloseToOpen,
    |     ^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/cache_coherence.rs:201:5
    |
201 |     SessionBased,
    |     ^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/cache_coherence.rs:202:5
    |
202 |     Strict,
    |     ^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/cache_coherence.rs:205:1
    |
205 | pub struct CoherenceManager {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/cache_coherence.rs:214:5
    |
214 |     pub fn new(protocol: CoherenceProtocol) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:224:5
    |
224 |     pub fn grant_lease(&mut self, inode: u64, client_id: u64) -> CacheLease {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:238:5
    |
238 |     pub fn revoke_lease(&mut self, inode: u64) -> Option<CacheInvalidation> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:253:5
    |
253 |     pub fn check_lease(&self, inode: u64) -> Option<&CacheLease> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:257:5
    |
257 |     pub fn invalidate(&mut self, inode: u64, reason: InvalidationReason, version: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:267:5
    |
267 |     pub fn pending_invalidations(&self) -> &[CacheInvalidation] {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:271:5
    |
271 |     pub fn drain_invalidations(&mut self) -> Vec<CacheInvalidation> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:276:5
    |
276 |     pub fn active_lease_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:280:5
    |
280 |     pub fn expire_stale_leases(&mut self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/cache_coherence.rs:295:5
    |
295 |     pub fn is_coherent(&self, inode: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-fuse/src/capability.rs:4:1
  |
4 | pub struct KernelVersion {
  | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/capability.rs:5:5
  |
5 |     pub major: u32,
  |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/capability.rs:6:5
  |
6 |     pub minor: u32,
  |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/capability.rs:7:5
  |
7 |     pub patch: u32,
  |     ^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/capability.rs:11:5
   |
11 |     pub fn new(major: u32, minor: u32, patch: u32) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/capability.rs:19:5
   |
19 |     pub fn parse(s: &str) -> Option<Self> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/capability.rs:40:5
   |
40 |     pub fn at_least(&self, other: &KernelVersion) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a constant
  --> crates/claudefs-fuse/src/capability.rs:51:1
   |
51 | pub const KERNEL_FUSE_PASSTHROUGH: KernelVersion = KernelVersion {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a constant
  --> crates/claudefs-fuse/src/capability.rs:56:1
   |
56 | pub const KERNEL_ATOMIC_WRITES: KernelVersion = KernelVersion {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a constant
  --> crates/claudefs-fuse/src/capability.rs:61:1
   |
61 | pub const KERNEL_DYNAMIC_IORING: KernelVersion = KernelVersion {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/capability.rs:68:1
   |
68 | pub enum PassthroughMode {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/capability.rs:69:5
   |
69 |     Full,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/capability.rs:70:5
   |
70 |     Partial,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/capability.rs:71:5
   |
71 |     None,
   |     ^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/capability.rs:75:1
   |
75 | pub struct NegotiatedCapabilities {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/capability.rs:76:5
   |
76 |     pub passthrough_mode: PassthroughMode,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/capability.rs:77:5
   |
77 |     pub atomic_writes: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/capability.rs:78:5
   |
78 |     pub dynamic_ioring: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/capability.rs:79:5
   |
79 |     pub writeback_cache: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/capability.rs:80:5
   |
80 |     pub async_read: bool,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/capability.rs:84:5
   |
84 |     pub fn for_kernel(version: &KernelVersion) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/capability.rs:102:5
    |
102 |     pub fn best_mode(&self) -> &PassthroughMode {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/capability.rs:106:5
    |
106 |     pub fn supports_passthrough(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/capability.rs:111:1
    |
111 | pub struct CapabilityNegotiator {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/capability.rs:118:5
    |
118 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/capability.rs:126:5
    |
126 |     pub fn negotiate(&mut self, kernel_version: KernelVersion) -> &NegotiatedCapabilities {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/capability.rs:133:5
    |
133 |     pub fn capabilities(&self) -> &NegotiatedCapabilities {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/capability.rs:139:5
    |
139 |     pub fn is_negotiated(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/capability.rs:143:5
    |
143 |     pub fn kernel_version(&self) -> Option<&KernelVersion> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a type alias
  --> crates/claudefs-fuse/src/client_auth.rs:12:1
   |
12 | pub type Result<T> = std::result::Result<T, AuthError>;
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/client_auth.rs:15:1
   |
15 | pub enum AuthState {
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/client_auth.rs:16:5
   |
16 |     Unenrolled,
   |     ^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/client_auth.rs:17:5
   |
17 |     Enrolling {
   |     ^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:18:9
   |
18 |         token: String,
   |         ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:19:9
   |
19 |         started_at_secs: u64,
   |         ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/client_auth.rs:21:5
   |
21 |     Enrolled {
   |     ^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:22:9
   |
22 |         cert_fingerprint: [u8; 32],
   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:23:9
   |
23 |         expires_at_secs: u64,
   |         ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/client_auth.rs:25:5
   |
25 |     Renewing {
   |     ^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:26:9
   |
26 |         old_fingerprint: [u8; 32],
   |         ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:27:9
   |
27 |         started_at_secs: u64,
   |         ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/client_auth.rs:29:5
   |
29 |     Revoked {
   |     ^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:30:9
   |
30 |         reason: String,
   |         ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:31:9
   |
31 |         revoked_at_secs: u64,
   |         ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/client_auth.rs:36:1
   |
36 | pub struct CertRecord {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:37:5
   |
37 |     pub fingerprint: [u8; 32],
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:38:5
   |
38 |     pub subject: String,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:39:5
   |
39 |     pub issued_at_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:40:5
   |
40 |     pub expires_at_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:41:5
   |
41 |     pub cert_pem: String,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:42:5
   |
42 |     pub key_pem: String,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/client_auth.rs:46:5
   |
46 |     pub fn is_expired(&self, now_secs: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/client_auth.rs:50:5
   |
50 |     pub fn needs_renewal(&self, now_secs: u64, renew_before_secs: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/client_auth.rs:55:5
   |
55 |     pub fn days_until_expiry(&self, now_secs: u64) -> i64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/client_auth.rs:62:1
   |
62 | pub struct RevokedCert {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:63:5
   |
63 |     pub fingerprint: [u8; 32],
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:64:5
   |
64 |     pub reason: String,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/client_auth.rs:65:5
   |
65 |     pub revoked_at_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/client_auth.rs:68:1
   |
68 | pub struct ClientAuthManager {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/client_auth.rs:76:5
   |
76 |     pub fn new(cert_dir: &str) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/client_auth.rs:85:5
   |
85 |     pub fn state(&self) -> &AuthState {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/client_auth.rs:89:5
   |
89 |     pub fn cert(&self) -> Option<&CertRecord> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/client_auth.rs:93:5
   |
93 |     pub fn begin_enrollment(&mut self, token: &str, now_secs: u64) -> Result<()> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:109:5
    |
109 | /     pub fn complete_enrollment(
110 | |         &mut self,
111 | |         cert_pem: &str,
112 | |         key_pem: &str,
113 | |         now_secs: u64,
114 | |     ) -> Result<()> {
    | |___________________^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:145:5
    |
145 |     pub fn needs_renewal(&self, now_secs: u64, renew_before_secs: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:153:5
    |
153 |     pub fn begin_renewal(&mut self, now_secs: u64) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:171:5
    |
171 |     pub fn complete_renewal(&mut self, cert_pem: &str, key_pem: &str, now_secs: u64) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:202:5
    |
202 |     pub fn revoke(&mut self, reason: &str, now_secs: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:218:5
    |
218 |     pub fn add_to_crl(&mut self, fingerprint: [u8; 32], reason: &str, revoked_at_secs: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:226:5
    |
226 |     pub fn is_revoked(&self, fingerprint: &[u8; 32]) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:230:5
    |
230 |     pub fn crl_len(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/client_auth.rs:234:5
    |
234 |     pub fn compact_crl(&mut self, now_secs: u64, max_age_secs: u64) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-fuse/src/client_auth.rs:271:1
    |
271 | pub enum AuthError {
    | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/client_auth.rs:273:5
    |
273 |     NotEnrolled,
    |     ^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/client_auth.rs:275:5
    |
275 |     AlreadyEnrolled,
    |     ^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/client_auth.rs:277:5
    |
277 |     EnrollmentInProgress,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/client_auth.rs:279:5
    |
279 |     AlreadyRevoked,
    |     ^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/client_auth.rs:281:5
    |
281 |     InvalidPem(String),
    |     ^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/client_auth.rs:283:5
    |
283 |     CertExpired,
    |     ^^^^^^^^^^^

warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/crash_recovery.rs:5:1
  |
5 | pub enum RecoveryState {
  | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/crash_recovery.rs:6:5
  |
6 |     Idle,
  |     ^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/crash_recovery.rs:7:5
  |
7 |     Scanning,
  |     ^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/crash_recovery.rs:8:5
  |
8 |     Replaying { replayed: u32, total: u32 },
  |     ^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/crash_recovery.rs:8:17
  |
8 |     Replaying { replayed: u32, total: u32 },
  |                 ^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/crash_recovery.rs:8:32
  |
8 |     Replaying { replayed: u32, total: u32 },
  |                                ^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/crash_recovery.rs:9:5
  |
9 |     Complete { recovered: u32, orphaned: u32 },
  |     ^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/crash_recovery.rs:9:16
  |
9 |     Complete { recovered: u32, orphaned: u32 },
  |                ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/crash_recovery.rs:9:32
  |
9 |     Complete { recovered: u32, orphaned: u32 },
  |                                ^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/crash_recovery.rs:10:5
   |
10 |     Failed(String),
   |     ^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:14:5
   |
14 |     pub fn is_in_progress(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:21:5
   |
21 |     pub fn is_complete(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/crash_recovery.rs:30:1
   |
30 | pub struct OpenFileRecord {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/crash_recovery.rs:31:5
   |
31 |     pub ino: InodeId,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/crash_recovery.rs:32:5
   |
32 |     pub fd: u64,
   |     ^^^^^^^^^^^

warning: missing documentation for a struct field

...960329 bytes truncated...

The tool call succeeded but the output was truncated. Full output saved to: /home/cfs/.local/share/opencode/tool-output/tool_ca9f75460001JZlldjlMlO75WS
Use the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context.
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security --lib 2>&1 | head -100
Blocking waiting for file lock on artifact directory
warning: unused imports: `FuseError` and `Result`
 --> crates/claudefs-fuse/src/cache_coherence.rs:1:13
  |
1 | use crate::{FuseError, Result};
  |             ^^^^^^^^^  ^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::sync::Arc`
 --> crates/claudefs-fuse/src/cache_coherence.rs:5:5
  |
5 | use std::sync::Arc;
  |     ^^^^^^^^^^^^^^

warning: unused variable: `token`
   --> crates/claudefs-fuse/src/client_auth.rs:117:17
    |
117 |                 token,
    |                 ^^^^^ help: try ignoring the field: `token: _`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `old_fingerprint`
   --> crates/claudefs-fuse/src/client_auth.rs:174:17
    |
174 |                 old_fingerprint,
    |                 ^^^^^^^^^^^^^^^ help: try ignoring the field: `old_fingerprint: _`

warning: unused variable: `cert_fingerprint`
   --> crates/claudefs-fuse/src/client_auth.rs:204:13
    |
204 |             cert_fingerprint,
    |             ^^^^^^^^^^^^^^^^ help: try ignoring the field: `cert_fingerprint: _`

warning: unused variable: `pid`
  --> crates/claudefs-fuse/src/io_priority.rs:88:9
   |
88 |         pid: u32,
   |         ^^^ help: if this is intentional, prefix it with an underscore: `_pid`

warning: unused variable: `uid`
  --> crates/claudefs-fuse/src/io_priority.rs:89:9
   |
89 |         uid: u32,
   |         ^^^ help: if this is intentional, prefix it with an underscore: `_uid`

warning: variable does not need to be mutable
   --> crates/claudefs-fuse/src/io_priority.rs:192:13
    |
192 |         let mut budgets = limits.clone();
    |             ----^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `now_secs`
  --> crates/claudefs-fuse/src/worm.rs:50:37
   |
50 |     pub fn is_append_allowed(&self, now_secs: u64) -> bool {
   |                                     ^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_now_secs`

warning: variable does not need to be mutable
   --> crates/claudefs-fuse/src/worm.rs:253:13
    |
253 |         let mut hold_inos = inos.clone();
    |             ----^^^^^^^^^
    |             |
    |             help: remove this `mut`

warning: field `protocol` is never read
   --> crates/claudefs-fuse/src/cache_coherence.rs:208:5
    |
205 | pub struct CoherenceManager {
    |            ---------------- field in this struct
...
208 |     protocol: CoherenceProtocol,
    |     ^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: field `cert_dir` is never read
  --> crates/claudefs-fuse/src/client_auth.rs:72:5
   |
68 | pub struct ClientAuthManager {
   |            ----------------- field in this struct
...
72 |     cert_dir: String,
   |     ^^^^^^^^

warning: missing documentation for a module
 --> crates/claudefs-fuse/src/lib.rs:5:1
  |
5 | pub mod attr;
  | ^^^^^^^^^^^^
  |
note: the lint level is defined here
 --> crates/claudefs-fuse/src/lib.rs:1:9
  |
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security --lib fuzz_fuse 2>&1 | tail -30
= note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `total_before`
  --> crates/claudefs-security/src/unsafe_audit.rs:95:17
   |
95 |             let total_before = stats_before.total_regions;
   |                 ^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_total_before`

warning: unused variable: `history_size_before`
  --> crates/claudefs-security/src/crypto_zeroize_audit.rs:73:9
   |
73 |     let history_size_before = km.history_size();
   |         ^^^^^^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_history_size_before`

warning: unused variable: `router`
   --> crates/claudefs-security/src/mgmt_pentest.rs:237:17
    |
237 |             let router = make_api(Some("secret-token"));
    |                 ^^^^^^ help: if this is intentional, prefix it with an underscore: `_router`

warning: unused variable: `api`
   --> crates/claudefs-security/src/mgmt_pentest.rs:306:17
    |
306 |             let api = Arc::new(AdminApi::new(metrics, config));
    |                 ^^^ help: if this is intentional, prefix it with an underscore: `_api`

Some errors have detailed explanations: E0308, E0599.
For more information about an error, try `rustc --explain E0308`.
warning: `claudefs-security` (lib test) generated 17 warnings
error: could not compile `claudefs-security` (lib test) due to 7 previous errors; 17 warnings emitted
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security --lib fuzz_fuse 2>&1 | grep -A3 "^error\[E"
error[E0308]: mismatched types
  --> crates/claudefs-security/src/dep_audit.rs:41:9
   |
39 |       fn get_direct_dependencies() -> serde_json::Value {
--
error[E0599]: no method named `iter` found for enum `Value` in the current scope
   --> crates/claudefs-security/src/dep_audit.rs:161:14
    |
160 |           let rand_pkg = direct_deps
--
error[E0599]: no method named `iter` found for enum `Value` in the current scope
   --> crates/claudefs-security/src/dep_audit.rs:294:14
    |
293 |           let fuser_pkg = direct_deps
--
error[E0599]: no method named `len` found for enum `Value` in the current scope
   --> crates/claudefs-security/src/dep_audit.rs:314:33
    |
314 |         let count = direct_deps.len();
--
error[E0599]: no method named `iter` found for enum `Value` in the current scope
   --> crates/claudefs-security/src/dep_audit.rs:357:38
    |
357 |         let our_crates = direct_deps.iter().filter(|p| {
--
error[E0599]: no method named `iter` found for enum `Value` in the current scope
   --> crates/claudefs-security/src/dep_audit.rs:432:14
    |
431 |           let tokio_pkg = direct_deps
--
error[E0599]: no method named `len` found for enum `Value` in the current scope
   --> crates/claudefs-security/src/dep_audit.rs:469:57
    |
469 |         println!("Direct dependencies: {}", direct_deps.len());
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo build -p claudefs-security 2>&1 | tail -10
warning: missing documentation for a method
   --> crates/claudefs-storage/src/tiering_policy.rs:411:5
    |
411 |     pub fn get_tier(&self, segment_id: u64) -> Option<&TierClass> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `claudefs-storage` (lib) generated 144 warnings (run `cargo fix --lib -p claudefs-storage` to apply 3 suggestions)
   Compiling claudefs-security v0.1.0 (/home/cfs/claudefs/crates/claudefs-security)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.66s
[0m
