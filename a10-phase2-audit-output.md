[0m[0m


> build · accounts/fireworks/models/minimax-m2p5[0m✱ [0mGlob "crates/claudefs-*/src/**/*.rs"[90m 100 matches[0m
[0m✱ [0mGlob "crates/claudefs-security/**/*.rs"[90m 21 matches[0m
[0m✱ [0mGlob "crates/claudefs-reduce/src/**/*.rs"[90m 21 matches[0m
[0m→ [0mRead crates/claudefs-reduce/src/encryption.rs
[0m→ [0mRead crates/claudefs-reduce/src/key_manager.rs
[0m✱ [0mGlob "crates/claudefs-transport/src/**/*.rs"[90m 51 matches[0m
Let me also check the transport and other relevant crates.
[0m→ [0mRead crates/claudefs-transport/src/tls.rs
[0m→ [0mRead crates/claudefs-transport/src/conn_auth.rs
[0m→ [0mRead crates/claudefs-transport/src/zerocopy.rs
[0m✱ [0mGlob "crates/claudefs-repl/src/**/*.rs"[90m 36 matches[0m
[0m→ [0mRead crates/claudefs-repl/src/batch_auth.rs
[0m✱ [0mGlob "crates/claudefs-gateway/src/**/*.rs"[90m 54 matches[0m
[0m→ [0mRead crates/claudefs-gateway/src/auth.rs
[0m→ [0mRead crates/claudefs-gateway/src/rpc.rs[90m [limit=100][0m
[0m✱ [0mGrep "struct OpaqueAuth"[90m in crates/claudefs-gateway · 1 match[0m
[0m→ [0mRead crates/claudefs-security/Cargo.toml
[0m→ [0mRead crates/claudefs-security/src/lib.rs
[0m✱ [0mGrep "struct JournalEntry"[90m in crates/claudefs-repl/src · 1 match[0m
[0m
[0m← [0mWrite crates/claudefs-security/src/phase2_audit.rs
Wrote file successfully.
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo check -p claudefs-security 2>&1

[0m
Blocking waiting for file lock on build directory
    Checking getrandom v0.3.4
    Checking arrow-data v56.2.0
    Checking claudefs-fuse v0.1.0 (/home/cfs/claudefs/crates/claudefs-fuse)
    Checking claudefs-reduce v0.1.0 (/home/cfs/claudefs/crates/claudefs-reduce)
    Checking claudefs-storage v0.1.0 (/home/cfs/claudefs/crates/claudefs-storage)
    Checking claudefs-transport v0.1.0 (/home/cfs/claudefs/crates/claudefs-transport)
    Checking claudefs-repl v0.1.0 (/home/cfs/claudefs/crates/claudefs-repl)
    Checking claudefs-gateway v0.1.0 (/home/cfs/claudefs/crates/claudefs-gateway)
    Checking ahash v0.8.12
    Checking arrow-array v56.2.0
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
  --> crates/claudefs-fuse/src/buffer_pool.rs:38:5
   |
38 |     pub fn is_empty(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:41:5
   |
41 |     pub fn clear(&mut self) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/buffer_pool.rs:50:1
   |
50 | pub struct BufferPoolConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:51:5
   |
51 |     pub max_4k: usize,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:52:5
   |
52 |     pub max_64k: usize,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:53:5
   |
53 |     pub max_1m: usize,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/buffer_pool.rs:67:1
   |
67 | pub struct BufferPoolStats {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:68:5
   |
68 |     pub alloc_count: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:69:5
   |
69 |     pub reuse_count: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:70:5
   |
70 |     pub return_count: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:71:5
   |
71 |     pub current_4k: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:72:5
   |
72 |     pub current_64k: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:73:5
   |
73 |     pub current_1m: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:77:5
   |
77 |     pub fn hit_rate(&self) -> f64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/buffer_pool.rs:87:1
   |
87 | pub struct BufferPool {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/buffer_pool.rs:97:5
   |
97 |     pub fn new(config: BufferPoolConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/buffer_pool.rs:108:5
    |
108 |     pub fn acquire(&mut self, size: BufferSize) -> Buffer {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/buffer_pool.rs:135:5
    |
135 |     pub fn release(&mut self, buf: Buffer) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/buffer_pool.rs:157:5
    |
157 |     pub fn stats(&self) -> &BufferPoolStats {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/buffer_pool.rs:161:5
    |
161 |     pub fn available(&self, size: BufferSize) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

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
  --> crates/claudefs-fuse/src/crash_recovery.rs:33:5
   |
33 |     pub pid: u32,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/crash_recovery.rs:34:5
   |
34 |     pub flags: u32,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/crash_recovery.rs:35:5
   |
35 |     pub path_hint: String,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:39:5
   |
39 |     pub fn is_writable(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:43:5
   |
43 |     pub fn is_append_only(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/crash_recovery.rs:49:1
   |
49 | pub struct PendingWrite {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/crash_recovery.rs:50:5
   |
50 |     pub ino: InodeId,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/crash_recovery.rs:51:5
   |
51 |     pub offset: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/crash_recovery.rs:52:5
   |
52 |     pub len: u64,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/crash_recovery.rs:53:5
   |
53 |     pub dirty_since_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:57:5
   |
57 |     pub fn age_secs(&self, now: u64) -> u64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:61:5
   |
61 |     pub fn is_stale(&self, now: u64, max_age_secs: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/crash_recovery.rs:67:1
   |
67 | pub struct RecoveryJournal {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/crash_recovery.rs:73:5
   |
73 |     pub fn new() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:80:5
   |
80 |     pub fn add_open_file(&mut self, record: OpenFileRecord) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:84:5
   |
84 |     pub fn add_pending_write(&mut self, write: PendingWrite) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:88:5
   |
88 |     pub fn open_file_count(&self) -> usize {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:92:5
   |
92 |     pub fn pending_write_count(&self) -> usize {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/crash_recovery.rs:96:5
   |
96 |     pub fn writable_open_files(&self) -> Vec<&OpenFileRecord> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:100:5
    |
100 |     pub fn stale_pending_writes(&self, now_secs: u64, max_age_secs: u64) -> Vec<&PendingWrite> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/crash_recovery.rs:109:1
    |
109 | pub struct RecoveryConfig {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-fuse/src/crash_recovery.rs:110:5
    |
110 |     pub max_recovery_secs: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-fuse/src/crash_recovery.rs:111:5
    |
111 |     pub max_open_files: usize,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-fuse/src/crash_recovery.rs:112:5
    |
112 |     pub stale_write_age_secs: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/crash_recovery.rs:116:5
    |
116 |     pub fn default_config() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/crash_recovery.rs:125:1
    |
125 | pub struct CrashRecovery {
    | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/crash_recovery.rs:132:5
    |
132 |     pub fn new(config: RecoveryConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:140:5
    |
140 |     pub fn state(&self) -> &RecoveryState {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:144:5
    |
144 |     pub fn begin_scan(&mut self) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:155:5
    |
155 |     pub fn record_open_file(&mut self, record: OpenFileRecord) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:172:5
    |
172 |     pub fn record_pending_write(&mut self, write: PendingWrite) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:183:5
    |
183 |     pub fn begin_replay(&mut self, total: u32) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:194:5
    |
194 |     pub fn advance_replay(&mut self, count: u32) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:200:5
    |
200 |     pub fn complete(&mut self, orphaned: u32) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:219:5
    |
219 |     pub fn fail(&mut self, reason: String) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:223:5
    |
223 |     pub fn reset(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/crash_recovery.rs:228:5
    |
228 |     pub fn journal(&self) -> &RecoveryJournal {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-fuse/src/datacache.rs:6:1
  |
6 | pub struct DataCacheConfig {
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/datacache.rs:7:5
  |
7 |     pub max_files: usize,
  |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/datacache.rs:8:5
  |
8 |     pub max_bytes: u64,
  |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/datacache.rs:9:5
  |
9 |     pub max_file_size: u64,
  |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/datacache.rs:23:1
   |
23 | pub struct CachedData {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/datacache.rs:24:5
   |
24 |     pub data: Vec<u8>,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/datacache.rs:25:5
   |
25 |     pub generation: u64,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/datacache.rs:29:1
   |
29 | pub struct DataCacheStats {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/datacache.rs:30:5
   |
30 |     pub hits: u64,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/datacache.rs:31:5
   |
31 |     pub misses: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/datacache.rs:32:5
   |
32 |     pub evictions: u64,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/datacache.rs:33:5
   |
33 |     pub total_bytes: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/datacache.rs:34:5
   |
34 |     pub files: usize,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/datacache.rs:37:1
   |
37 | pub struct DataCache {
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/datacache.rs:45:5
   |
45 |     pub fn new(config: DataCacheConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/datacache.rs:57:5
   |
57 |     pub fn insert(&mut self, ino: InodeId, data: Vec<u8>, generation: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/datacache.rs:94:5
   |
94 |     pub fn get(&mut self, ino: InodeId) -> Option<&CachedData> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/datacache.rs:104:5
    |
104 |     pub fn invalidate(&mut self, ino: InodeId) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/datacache.rs:112:5
    |
112 |     pub fn invalidate_if_generation(&mut self, ino: InodeId, generation: u64) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/datacache.rs:120:5
    |
120 |     pub fn stats(&self) -> &DataCacheStats {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/datacache.rs:124:5
    |
124 |     pub fn len(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/datacache.rs:128:5
    |
128 |     pub fn is_empty(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/datacache.rs:132:5
    |
132 |     pub fn total_bytes(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/datacache.rs:136:5
    |
136 |     pub fn clear(&mut self) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/deleg.rs:9:1
  |
9 | pub enum DelegType {
  | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/deleg.rs:10:5
   |
10 |     Read,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/deleg.rs:11:5
   |
11 |     Write,
   |     ^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/deleg.rs:15:1
   |
15 | pub enum DelegState {
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/deleg.rs:16:5
   |
16 |     Active,
   |     ^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/deleg.rs:17:5
   |
17 |     Recalled { recalled_at_secs: u64 },
   |     ^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:17:16
   |
17 |     Recalled { recalled_at_secs: u64 },
   |                ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/deleg.rs:18:5
   |
18 |     Returned { returned_at_secs: u64 },
   |     ^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:18:16
   |
18 |     Returned { returned_at_secs: u64 },
   |                ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/deleg.rs:19:5
   |
19 |     Revoked { revoked_at_secs: u64 },
   |     ^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:19:15
   |
19 |     Revoked { revoked_at_secs: u64 },
   |               ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/deleg.rs:23:1
   |
23 | pub struct Delegation {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:24:5
   |
24 |     pub id: u64,
   |     ^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:25:5
   |
25 |     pub ino: u64,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:26:5
   |
26 |     pub deleg_type: DelegType,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:27:5
   |
27 |     pub client_id: u64,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:28:5
   |
28 |     pub granted_at_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:29:5
   |
29 |     pub lease_duration_secs: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/deleg.rs:30:5
   |
30 |     pub state: DelegState,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/deleg.rs:34:5
   |
34 | /     pub fn new(
35 | |         id: u64,
36 | |         ino: u64,
37 | |         deleg_type: DelegType,
...  |
40 | |         lease_secs: u64,
41 | |     ) -> Self {
   | |_____________^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/deleg.rs:53:5
   |
53 |     pub fn is_active(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/deleg.rs:57:5
   |
57 |     pub fn is_returnable(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/deleg.rs:61:5
   |
61 |     pub fn is_expired(&self, now_secs: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/deleg.rs:68:5
   |
68 |     pub fn expires_at(&self) -> u64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/deleg.rs:73:5
   |
73 |     pub fn recall(&mut self, now_secs: u64) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/deleg.rs:81:5
   |
81 |     pub fn returned(&mut self, now_secs: u64) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/deleg.rs:89:5
   |
89 |     pub fn revoke(&mut self, now_secs: u64) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/deleg.rs:95:5
   |
95 |     pub fn time_remaining_secs(&self, now_secs: u64) -> i64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/deleg.rs:101:1
    |
101 | pub struct DelegationManager {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/deleg.rs:109:5
    |
109 |     pub fn new(default_lease_secs: u64) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:118:5
    |
118 | /     pub fn grant(
119 | |         &mut self,
120 | |         ino: u64,
121 | |         deleg_type: DelegType,
122 | |         client_id: u64,
123 | |         now_secs: u64,
124 | |     ) -> std::result::Result<u64, DelegError> {
    | |_____________________________________________^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:155:5
    |
155 |     pub fn get(&self, id: u64) -> Option<&Delegation> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:159:5
    |
159 |     pub fn delegations_for_ino(&self, ino: u64) -> Vec<&Delegation> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:171:5
    |
171 |     pub fn recall_for_ino(&mut self, ino: u64, now_secs: u64) -> Vec<u64> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:187:5
    |
187 |     pub fn return_deleg(&mut self, id: u64, now_secs: u64) -> std::result::Result<(), DelegError> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:201:5
    |
201 |     pub fn revoke_expired(&mut self, now_secs: u64) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:218:5
    |
218 |     pub fn active_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:222:5
    |
222 |     pub fn can_grant_write(&self, ino: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/deleg.rs:226:5
    |
226 |     pub fn can_grant_read(&self, ino: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-fuse/src/deleg.rs:235:1
    |
235 | pub enum DelegError {
    | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/deleg.rs:237:5
    |
237 |     NotFound(u64),
    |     ^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/deleg.rs:239:5
    |
239 |     ConflictingWrite(u64),
    |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/deleg.rs:241:5
    |
241 |     ConflictingRead(u64),
    |     ^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-fuse/src/deleg.rs:243:5
    |
243 |     NotActive,
    |     ^^^^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-fuse/src/dir_cache.rs:9:1
  |
9 | pub struct DirEntry {
  | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:10:5
   |
10 |     pub name: String,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:11:5
   |
11 |     pub ino: InodeId,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:12:5
   |
12 |     pub kind: InodeKind,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/dir_cache.rs:16:1
   |
16 | pub struct ReaddirSnapshot {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:17:5
   |
17 |     pub entries: Vec<DirEntry>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:18:5
   |
18 |     pub inserted_at: Instant,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:19:5
   |
19 |     pub ttl: Duration,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/dir_cache.rs:23:5
   |
23 |     pub fn is_expired(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/dir_cache.rs:27:5
   |
27 |     pub fn len(&self) -> usize {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/dir_cache.rs:31:5
   |
31 |     pub fn is_empty(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/dir_cache.rs:37:1
   |
37 | pub struct DirCacheConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:38:5
   |
38 |     pub max_dirs: usize,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:39:5
   |
39 |     pub ttl: Duration,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:40:5
   |
40 |     pub negative_ttl: Duration,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/dir_cache.rs:54:1
   |
54 | pub struct DirCacheStats {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:55:5
   |
55 |     pub hits: u64,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:56:5
   |
56 |     pub misses: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:57:5
   |
57 |     pub negative_hits: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:58:5
   |
58 |     pub invalidations: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dir_cache.rs:59:5
   |
59 |     pub snapshots_cached: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/dir_cache.rs:62:1
   |
62 | pub struct DirCache {
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/dir_cache.rs:70:5
   |
70 |     pub fn new(config: DirCacheConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/dir_cache.rs:79:5
   |
79 |     pub fn insert_snapshot(&mut self, dir_ino: InodeId, entries: Vec<DirEntry>) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dir_cache.rs:103:5
    |
103 |     pub fn get_snapshot(&mut self, dir_ino: InodeId) -> Option<ReaddirSnapshot> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dir_cache.rs:119:5
    |
119 |     pub fn lookup(&mut self, parent: InodeId, name: &str) -> Option<DirEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dir_cache.rs:133:5
    |
133 |     pub fn insert_negative(&mut self, parent: InodeId, name: &str) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dir_cache.rs:138:5
    |
138 |     pub fn is_negative(&mut self, parent: InodeId, name: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dir_cache.rs:151:5
    |
151 |     pub fn invalidate_dir(&mut self, dir_ino: InodeId) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dir_cache.rs:158:5
    |
158 |     pub fn invalidate_entry(&mut self, parent: InodeId, name: &str) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dir_cache.rs:165:5
    |
165 |     pub fn evict_expired(&mut self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dir_cache.rs:178:5
    |
178 |     pub fn stats(&self) -> &DirCacheStats {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/dirnotify.rs:5:1
  |
5 | pub enum DirEvent {
  | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/dirnotify.rs:6:5
  |
6 |     Created {
  |     ^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/dirnotify.rs:7:9
  |
7 |         ino: InodeId,
  |         ^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/dirnotify.rs:8:9
  |
8 |         name: String,
  |         ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/dirnotify.rs:10:5
   |
10 |     Deleted {
   |     ^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dirnotify.rs:11:9
   |
11 |         ino: InodeId,
   |         ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dirnotify.rs:12:9
   |
12 |         name: String,
   |         ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/dirnotify.rs:14:5
   |
14 |     Renamed {
   |     ^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dirnotify.rs:15:9
   |
15 |         old_name: String,
   |         ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dirnotify.rs:16:9
   |
16 |         new_name: String,
   |         ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dirnotify.rs:17:9
   |
17 |         ino: InodeId,
   |         ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/dirnotify.rs:19:5
   |
19 |     Attrib {
   |     ^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dirnotify.rs:20:9
   |
20 |         ino: InodeId,
   |         ^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/dirnotify.rs:24:1
   |
24 | pub struct NotifyConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dirnotify.rs:25:5
   |
25 |     pub max_queue_per_dir: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/dirnotify.rs:26:5
   |
26 |     pub max_dirs_tracked: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/dirnotify.rs:38:1
   |
38 | pub struct DirNotify {
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/dirnotify.rs:45:5
   |
45 |     pub fn new(config: NotifyConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/dirnotify.rs:59:5
   |
59 |     pub fn watch(&mut self, dir_ino: InodeId) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/dirnotify.rs:82:5
   |
82 |     pub fn unwatch(&mut self, dir_ino: InodeId) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/dirnotify.rs:89:5
   |
89 |     pub fn post(&mut self, dir_ino: InodeId, event: DirEvent) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dirnotify.rs:113:5
    |
113 |     pub fn drain(&mut self, dir_ino: InodeId) -> Vec<DirEvent> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dirnotify.rs:128:5
    |
128 |     pub fn pending_count(&self, dir_ino: InodeId) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dirnotify.rs:132:5
    |
132 |     pub fn watched_dirs(&self) -> Vec<InodeId> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dirnotify.rs:136:5
    |
136 |     pub fn is_watched(&self, dir_ino: InodeId) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/dirnotify.rs:140:5
    |
140 |     pub fn total_pending(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/error.rs:4:1
  |
4 | pub enum FuseError {
  | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/error.rs:6:5
  |
6 |     Io(#[from] std::io::Error),
  |     ^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/error.rs:9:5
  |
9 |     MountFailed { mountpoint: String, reason: String },
  |     ^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/error.rs:9:19
  |
9 |     MountFailed { mountpoint: String, reason: String },
  |                   ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/error.rs:9:39
  |
9 |     MountFailed { mountpoint: String, reason: String },
  |                                       ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/error.rs:12:5
   |
12 |     NotFound { ino: u64 },
   |     ^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/error.rs:12:16
   |
12 |     NotFound { ino: u64 },
   |                ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/error.rs:15:5
   |

...609936 bytes truncated...

The tool call succeeded but the output was truncated. Full output saved to: /home/cfs/.local/share/opencode/tool-output/tool_cb7a1db8e001A9T5F9XI2zMoli
Use the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context.[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security phase2_audit --no-run 2>&1 | tail -30
372 |     let _ = reducer.register(100, RetentionPolicy::legal_hold(), 1024);
    |     +++++++

warning: unused `Result` that must be used
   --> crates/claudefs-security/src/crypto_zeroize_audit.rs:373:5
    |
373 |     reducer.register(200, RetentionPolicy::immutable_until(100), 2048);
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
    = note: this `Result` may be an `Err` variant, which should be handled
help: use `let _ = ...` to ignore the resulting value
    |
373 |     let _ = reducer.register(200, RetentionPolicy::immutable_until(100), 2048);
    |     +++++++

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

warning: `claudefs-security` (lib test) generated 69 warnings (run `cargo fix --lib -p claudefs-security --tests` to apply 44 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 57.44s
  Executable unittests src/lib.rs (target/debug/deps/claudefs_security-b538fb71baa4c100)[0m

[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-security phase2_audit 2>&1 | tail -60

warning: field `0` is never read
    --> crates/claudefs-security/src/advanced_fuzzing.rs:1573:13
     |
1573 |     Replace(u32, u32),
     |     ------- ^^^
     |     |
     |     field in this variant
     |
     = note: `MembershipChange` has a derived impl for the trait `Debug`, but this is intentionally ignored during dead code analysis
help: consider changing the field to be of unit type to suppress this warning while preserving the field numbering, or remove the field
     |
1573 -     Replace(u32, u32),
1573 +     Replace((), u32),
     |

warning: unused `Result` that must be used
   --> crates/claudefs-security/src/crypto_zeroize_audit.rs:372:5
    |
372 |     reducer.register(100, RetentionPolicy::legal_hold(), 1024);
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
    = note: this `Result` may be an `Err` variant, which should be handled
    = note: `#[warn(unused_must_use)]` (part of `#[warn(unused)]`) on by default
help: use `let _ = ...` to ignore the resulting value
    |
372 |     let _ = reducer.register(100, RetentionPolicy::legal_hold(), 1024);
    |     +++++++

warning: unused `Result` that must be used
   --> crates/claudefs-security/src/crypto_zeroize_audit.rs:373:5
    |
373 |     reducer.register(200, RetentionPolicy::immutable_until(100), 2048);
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
    = note: this `Result` may be an `Err` variant, which should be handled
help: use `let _ = ...` to ignore the resulting value
    |
373 |     let _ = reducer.register(200, RetentionPolicy::immutable_until(100), 2048);
    |     +++++++

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

warning: `claudefs-security` (lib test) generated 69 warnings (run `cargo fix --lib -p claudefs-security --tests` to apply 44 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 35.56s
     Running unittests src/lib.rs (target/debug/deps/claudefs_security-b538fb71baa4c100)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 550 filtered out; finished in 0.00s[0m
[0m→ [0mRead crates/claudefs-security/src/lib.rs
[0m
[0m← [0mEdit crates/claudefs-security/src/lib.rsIndex: /home/cfs/claudefs/crates/claudefs-security/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-security/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-security/src/lib.rs
@@ -47,4 +47,6 @@
 #[cfg(test)]
 pub mod operational_security;
 #[cfg(test)]
 pub mod advanced_fuzzing;
+#[cfg(test)]
+pub mod phase2_audit;



[0m