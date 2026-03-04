[0m> build · accounts/fireworks/models/glm-5


[0m[0m→ [0mRead crates/claudefs-fuse/src/interrupt.rs
[0m[0m← [0mWrite crates/claudefs-fuse/src/interrupt.rs


[0mWrote file successfully.
[0m$ [0mcargo check -p claudefs-fuse --all-targets 2>&1

[0m
Checking claudefs-fuse v0.1.0 (/home/cfs/claudefs/crates/claudefs-fuse)
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

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/flock.rs:82:18
   |
82 |     WouldBlock { holder_pid: u32 },
   |                  ^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/locking.rs:12:1
   |
12 | pub enum LockType {
   | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/locking.rs:13:5
   |
13 |     Shared,
   |     ^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/locking.rs:14:5
   |
14 |     Exclusive,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/locking.rs:15:5
   |
15 |     Unlock,
   |     ^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/locking.rs:19:1
   |
19 | pub struct LockRecord {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/locking.rs:20:5
   |
20 |     pub lock_type: LockType,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/locking.rs:21:5
   |
21 |     pub owner: u64,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/locking.rs:22:5
   |
22 |     pub pid: u32,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/locking.rs:23:5
   |
23 |     pub start: u64,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/locking.rs:24:5
   |
24 |     pub end: u64,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/locking.rs:27:1
   |
27 | pub struct LockManager {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/locking.rs:36:5
   |
36 |     pub fn new() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/locking.rs:42:5
   |
42 |     pub fn try_lock(&mut self, ino: InodeId, req: LockRecord) -> Result<bool> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/locking.rs:63:5
   |
63 |     pub fn unlock(&mut self, ino: InodeId, owner: u64) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/locking.rs:69:5
   |
69 | /     pub fn has_conflicting_lock(
70 | |         &self,
71 | |         ino: InodeId,
72 | |         lock_type: LockType,
73 | |         start: u64,
74 | |         end: u64,
75 | |     ) -> bool {
   | |_____________^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/locking.rs:79:5
   |
79 | /     pub fn has_conflicting_lock_with_owner(
80 | |         &self,
81 | |         ino: InodeId,
82 | |         lock_type: LockType,
...  |
85 | |         exclude_owner: Option<u64>,
86 | |     ) -> bool {
   | |_____________^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/locking.rs:109:5
    |
109 |     pub fn lock_count(&self, ino: InodeId) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/locking.rs:113:5
    |
113 |     pub fn total_locks(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/locking.rs:117:5
    |
117 |     pub fn clear_inode(&mut self, ino: InodeId) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

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

warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/mount_opts.rs:6:1
  |
6 | pub enum ReadWriteMode {
  | ^^^^^^^^^^^^^^^^^^^^^^
  |
note: the lint level is defined here
 --> crates/claudefs-fuse/src/mount_opts.rs:1:9
  |
1 | #![warn(missing_docs)]
  |         ^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/mount_opts.rs:8:5
  |
8 |     ReadWrite,
  |     ^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/mount_opts.rs:9:5
  |
9 |     ReadOnly,
  |     ^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/mount_opts.rs:13:1
   |
13 | pub enum CacheMode {
   | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/mount_opts.rs:15:5
   |
15 |     None,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/mount_opts.rs:16:5
   |
16 |     Relaxed,
   |     ^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/mount_opts.rs:17:5
   |
17 |     Strict,
   |     ^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/mount_opts.rs:21:1
   |
21 | pub struct MountOptions {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:22:5
   |
22 |     pub source: PathBuf,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:23:5
   |
23 |     pub target: PathBuf,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:24:5
   |
24 |     pub read_only: ReadWriteMode,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:25:5
   |
25 |     pub allow_other: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:26:5
   |
26 |     pub default_permissions: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:27:5
   |
27 |     pub cache_mode: CacheMode,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:28:5
   |
28 |     pub max_background: u32,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:29:5
   |
29 |     pub congestion_threshold: u32,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:30:5
   |
30 |     pub direct_io: bool,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:31:5
   |
31 |     pub kernel_cache: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:32:5
   |
32 |     pub auto_unmount: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/mount_opts.rs:33:5
   |
33 |     pub fd: Option<i32>,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/mount_opts.rs:56:5
   |
56 |     pub fn new(source: PathBuf, target: PathBuf) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/mount_opts.rs:64:5
   |
64 |     pub fn to_fuse_args(&self) -> Vec<String> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a type alias
 --> crates/claudefs-fuse/src/openfile.rs:4:1
  |
4 | pub type FileHandle = u64;
  | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/openfile.rs:7:1
  |
7 | pub enum OpenFlags {
  | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/openfile.rs:8:5
  |
8 |     ReadOnly,
  |     ^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/openfile.rs:9:5
  |
9 |     WriteOnly,
  |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/openfile.rs:10:5
   |
10 |     ReadWrite,
   |     ^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/openfile.rs:14:5
   |
14 |     pub fn is_readable(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/openfile.rs:21:5
   |
21 |     pub fn is_writable(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/openfile.rs:28:5
   |
28 |     pub fn from_libc(flags: i32) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/openfile.rs:39:1
   |
39 | pub struct OpenFileEntry {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/openfile.rs:40:5
   |
40 |     pub fh: FileHandle,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/openfile.rs:41:5
   |
41 |     pub ino: InodeId,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/openfile.rs:42:5
   |
42 |     pub flags: OpenFlags,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/openfile.rs:43:5
   |
43 |     pub offset: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/openfile.rs:44:5
   |
44 |     pub dirty: bool,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/openfile.rs:47:1
   |
47 | pub struct OpenFileTable {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/openfile.rs:53:5
   |
53 |     pub fn new() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/openfile.rs:61:5
   |
61 |     pub fn open(&mut self, ino: InodeId, flags: OpenFlags) -> FileHandle {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/openfile.rs:86:5
   |
86 |     pub fn get(&self, fh: FileHandle) -> Option<&OpenFileEntry> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/openfile.rs:90:5
   |
90 |     pub fn get_mut(&mut self, fh: FileHandle) -> Option<&mut OpenFileEntry> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/openfile.rs:94:5
   |
94 |     pub fn close(&mut self, fh: FileHandle) -> Option<OpenFileEntry> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/openfile.rs:104:5
    |
104 |     pub fn seek(&mut self, fh: FileHandle, offset: u64) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/openfile.rs:118:5
    |
118 |     pub fn mark_dirty(&mut self, fh: FileHandle) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/openfile.rs:132:5
    |
132 |     pub fn mark_clean(&mut self, fh: FileHandle) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/openfile.rs:149:5
    |
149 |     pub fn handles_for_inode(&self, ino: InodeId) -> Vec<FileHandle> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/openfile.rs:157:5
    |
157 |     pub fn count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/openfile.rs:161:5
    |
161 |     pub fn dirty_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-fuse/src/prefetch.rs:5:1
  |
5 | pub struct PrefetchConfig {
  | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/prefetch.rs:6:5
  |
6 |     pub window_size: usize,
  |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/prefetch.rs:7:5
  |
7 |     pub block_size: u64,
  |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/prefetch.rs:8:5
  |
8 |     pub max_inflight: usize,
  |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/prefetch.rs:9:5
  |
9 |     pub detection_threshold: u32,
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/prefetch.rs:24:1
   |
24 | pub struct PrefetchEntry {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:25:5
   |
25 |     pub ino: InodeId,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:26:5
   |
26 |     pub offset: u64,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:27:5
   |
27 |     pub data: Vec<u8>,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:28:5
   |
28 |     pub ready: bool,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/prefetch.rs:32:1
   |
32 | pub struct AccessPattern {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:33:5
   |
33 |     pub last_offset: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:34:5
   |
34 |     pub sequential_count: u32,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/prefetch.rs:46:1
   |
46 | pub struct PrefetchStats {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:47:5
   |
47 |     pub entries_cached: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:48:5
   |
48 |     pub inodes_tracked: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/prefetch.rs:49:5
   |
49 |     pub sequential_inodes: usize,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/prefetch.rs:52:1
   |
52 | pub struct PrefetchEngine {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/prefetch.rs:59:5
   |
59 |     pub fn new(config: PrefetchConfig) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/prefetch.rs:73:5
   |
73 |     pub fn record_access(&mut self, ino: InodeId, offset: u64, size: u32) -> u64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/prefetch.rs:100:5
    |
100 |     pub fn is_sequential(&self, ino: InodeId) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/prefetch.rs:107:5
    |
107 |     pub fn compute_prefetch_list(&self, ino: InodeId, current_offset: u64) -> Vec<(InodeId, u64)> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/prefetch.rs:140:5
    |
140 |     pub fn store_prefetch(&mut self, ino: InodeId, offset: u64, data: Vec<u8>) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/prefetch.rs:157:5
    |
157 |     pub fn try_serve(&self, ino: InodeId, offset: u64, size: u32) -> Option<Vec<u8>> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/prefetch.rs:190:5
    |
190 |     pub fn evict(&mut self, ino: InodeId) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/prefetch.rs:207:5
    |
207 |     pub fn stats(&self) -> PrefetchStats {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-fuse/src/quota_enforce.rs:6:1
  |
6 | pub struct QuotaUsage {
  | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/quota_enforce.rs:7:5
  |
7 |     pub bytes_used: u64,
  |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/quota_enforce.rs:8:5
  |
8 |     pub bytes_soft: u64,
  |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/quota_enforce.rs:9:5
  |
9 |     pub bytes_hard: u64,
  |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/quota_enforce.rs:10:5
   |
10 |     pub inodes_used: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/quota_enforce.rs:11:5
   |
11 |     pub inodes_soft: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/quota_enforce.rs:12:5
   |
12 |     pub inodes_hard: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/quota_enforce.rs:16:5
   |
16 |     pub fn new(bytes_soft: u64, bytes_hard: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/quota_enforce.rs:27:5
   |
27 |     pub fn unlimited() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/quota_enforce.rs:38:5
   |
38 |     pub fn bytes_status(&self) -> QuotaStatus {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/quota_enforce.rs:48:5
   |
48 |     pub fn inodes_status(&self) -> QuotaStatus {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-fuse/src/quota_enforce.rs:60:1
   |
60 | pub enum QuotaStatus {
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/quota_enforce.rs:61:5
   |
61 |     Ok,
   |     ^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/quota_enforce.rs:62:5
   |
62 |     SoftExceeded,
   |     ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-fuse/src/quota_enforce.rs:63:5
   |
63 |     HardExceeded,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/quota_enforce.rs:71:1
   |
71 | pub struct QuotaEnforcer {
   | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/quota_enforce.rs:81:5
   |
81 |     pub fn new(ttl: Duration) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/quota_enforce.rs:92:5
   |
92 |     pub fn with_default_ttl() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/quota_enforce.rs:96:5
   |
96 |     pub fn update_user_quota(&mut self, uid: u32, usage: QuotaUsage) {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:106:5
    |
106 |     pub fn update_group_quota(&mut self, gid: u32, usage: QuotaUsage) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:136:5
    |
136 |     pub fn check_write(&mut self, uid: u32, gid: u32, write_size: u64) -> Result<QuotaStatus> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:178:5
    |
178 |     pub fn check_create(&mut self, uid: u32, gid: u32) -> Result<QuotaStatus> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:220:5
    |
220 |     pub fn invalidate_user(&mut self, uid: u32) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:224:5
    |
224 |     pub fn invalidate_group(&mut self, gid: u32) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:228:5
    |
228 |     pub fn cache_hits(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:232:5
    |
232 |     pub fn check_count(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:236:5
    |
236 |     pub fn denied_count(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/quota_enforce.rs:240:5
    |
240 |     pub fn cache_size(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/ratelimit.rs:2:1
  |
2 | pub enum RateLimitDecision {
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/ratelimit.rs:3:5
  |
3 |     Allow,
  |     ^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/ratelimit.rs:4:5
  |
4 |     Throttle { wait_ms: u64 },
  |     ^^^^^^^^

warning: missing documentation for a struct field
 --> crates/claudefs-fuse/src/ratelimit.rs:4:16
  |
4 |     Throttle { wait_ms: u64 },
  |                ^^^^^^^^^^^^

warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/ratelimit.rs:5:5
  |
5 |     Reject,
  |     ^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-fuse/src/ratelimit.rs:9:1
  |
9 | pub struct RateLimiterConfig {
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/ratelimit.rs:10:5
   |
10 |     pub bytes_per_sec: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/ratelimit.rs:11:5
   |
11 |     pub ops_per_sec: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/ratelimit.rs:12:5
   |
12 |     pub burst_factor: f64,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/ratelimit.rs:13:5
   |
13 |     pub reject_threshold: f64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/ratelimit.rs:27:1
   |
27 | pub struct TokenBucket {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-fuse/src/ratelimit.rs:35:5
   |
35 |     pub fn new(rate_per_sec: u64, burst_factor: f64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/ratelimit.rs:54:5
   |
54 |     pub fn refill(&mut self, now_ms: u64) -> f64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/ratelimit.rs:70:5
   |
70 |     pub fn try_consume(&mut self, amount: f64, now_ms: u64) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-fuse/src/ratelimit.rs:85:5
   |
85 |     pub fn wait_ms_for(&self, amount: f64) -> u64 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/ratelimit.rs:102:5
    |
102 |     pub fn fill_level(&self) -> f64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/ratelimit.rs:110:5
    |
110 |     pub fn is_unlimited(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-fuse/src/ratelimit.rs:115:1
    |
115 | pub struct IoRateLimiter {
    | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-fuse/src/ratelimit.rs:125:5
    |
125 |     pub fn new(config: RateLimiterConfig) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/ratelimit.rs:148:5
    |
148 |     pub fn check_io(&mut self, bytes: u64, now_ms: u64) -> RateLimitDecision {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/ratelimit.rs:185:5
    |
185 |     pub fn check_op(&mut self, now_ms: u64) -> RateLimitDecision {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/ratelimit.rs:206:5
    |
206 |     pub fn total_allowed(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/ratelimit.rs:210:5
    |
210 |     pub fn total_throttled(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/ratelimit.rs:214:5
    |
214 |     pub fn total_rejected(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-fuse/src/ratelimit.rs:218:5
    |
218 |     pub fn is_limited(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/session.rs:18:5
   |
18 |     pub config: FuseServerConfig,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/snapshot.rs:5:1
  |
5 | pub enum SnapshotState {

...13725 bytes truncated...

The tool call succeeded but the output was truncated. Full output saved to: /home/cfs/.local/share/opencode/tool-output/tool_cb7ec8a23001JT3YpV0HRmgMBo
Use the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context.Done. Added module doc and `///` comments to all public items in `interrupt.rs`.
