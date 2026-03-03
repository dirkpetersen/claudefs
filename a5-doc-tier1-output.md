[0m> build · accounts/fireworks/models/minimax-m2p5[0m


[0m→ [0mRead crates/claudefs-fuse/src/filesystem.rs
[0m→ [0mRead crates/claudefs-fuse/src/cache_coherence.rs
[0m→ [0mRead crates/claudefs-fuse/src/workload_class.rs
[0m→ [0mRead crates/claudefs-fuse/src/sec_policy.rs
[0m→ [0mRead crates/claudefs-fuse/src/client_auth.rs
I'll start by reading the 5 files to understand their structure and identify all public items that need documentation.
Now I'll add comprehensive documentation to each of the 5 files. Let me start with filesystem.rs:
[0m[0m← [0mEdit crates/claudefs-fuse/src/filesystem.rs



[0mIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
@@ -28,18 +28,31 @@
 use crate::perf::FuseMetrics;
 use crate::symlink::SymlinkStore;
 use crate::xattr::XattrStore;
 
+/// Configuration for the ClaudeFS FUSE filesystem.
+///
+/// Controls caching behavior, user/group identity, permission checking,
+/// and timeout values for attribute and entry caching.
 #[derive(Debug, Clone)]
 pub struct ClaudeFsConfig {
+    /// Metadata cache configuration.
     pub cache: CacheConfig,
+    /// Data cache configuration for file content.
     pub data_cache: DataCacheConfig,
+    /// Default UID for filesystem operations when running as root.
     pub uid: u32,
+    /// Default GID for filesystem operations when running as root.
     pub gid: u32,
+    /// Whether to enforce default POSIX permission checking.
     pub default_permissions: bool,
+    /// Whether to allow other users to access the filesystem.
     pub allow_other: bool,
+    /// How long to cache attribute (getattr) results.
     pub attr_timeout: Duration,
+    /// How long to cache lookup/entry results.
     pub entry_timeout: Duration,
+    /// Whether to use direct I/O (bypass page cache).
     pub direct_io: bool,
 }
 
 impl Default for ClaudeFsConfig {
[0m← [0mEdit crates/claudefs-fuse/src/filesystem.rsIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
@@ -70,12 +70,15 @@
         }
     }
 }
 
+/// Internal handle tracking an open file descriptor.
 #[derive(Debug)]
 #[allow(dead_code)]
 struct OpenHandle {
+    /// Inode number of the opened file.
     ino: u64,
+    /// Flags passed to the open call.
     flags: i32,
 }
 
 struct ClaudeFsState {


[0m
[0m← [0mEdit crates/claudefs-fuse/src/filesystem.rs
Index: /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
@@ -94,15 +94,20 @@
     #[allow(dead_code)]
     data_cache: DataCache,
 }
 
+/// FUSE filesystem implementation for ClaudeFS.
+///
+/// Thread-safe via internal mutex on state. Implements the `fuser::Filesystem`
+/// trait to handle FUSE operations from the kernel.
 pub struct ClaudeFsFilesystem {
     config: ClaudeFsConfig,
     state: Arc<Mutex<ClaudeFsState>>,
     metrics: Arc<FuseMetrics>,
 }
 
 impl ClaudeFsFilesystem {
+    /// Creates a new ClaudeFS filesystem with the given configuration.
     pub fn new(config: ClaudeFsConfig) -> Self {
         let state = ClaudeFsState {
             inodes: InodeTable::new(),
             cache: MetadataCache::new(config.cache.clone()),
@@ -120,12 +125,14 @@
             metrics: Arc::new(FuseMetrics::new()),
         }
     }
 
+    /// Returns a reference to the filesystem configuration.
     pub fn config(&self) -> &ClaudeFsConfig {
         &self.config
     }
 
+    /// Returns a snapshot of current FUSE operation metrics.
     pub fn metrics_snapshot(&self) -> crate::perf::MetricsSnapshot {
         self.metrics.snapshot()
     }
 }


[0m[0m← [0mEdit crates/claudefs-fuse/src/filesystem.rsIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/filesystem.rs
@@ -136,8 +136,9 @@
         self.metrics.snapshot()
     }
 }
 
+/// Converts an internal inode entry to a FUSE file attribute structure.
 fn inode_to_fuser_attr(entry: &InodeEntry) -> fuser::FileAttr {
     let atime = SystemTime::UNIX_EPOCH + Duration::new(entry.atime_secs as u64, entry.atime_nsecs);
     let mtime = SystemTime::UNIX_EPOCH + Duration::new(entry.mtime_secs as u64, entry.mtime_nsecs);
     let ctime = SystemTime::UNIX_EPOCH + Duration::new(entry.ctime_secs as u64, entry.ctime_nsecs);



[0mNow let's document cache_coherence.rs:
[0m← [0mEdit crates/claudefs-fuse/src/cache_coherence.rs

Index: /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
@@ -4,8 +4,9 @@
 use std::time::{Duration, Instant, SystemTime};
 use thiserror::Error;
 use tracing::{debug, trace, warn};
 
+/// Errors that can occur in the cache coherence protocol.
 #[derive(Error, Debug)]
 pub enum CoherenceError {
     #[error("Lease not found for inode {0}")]
     LeaseNotFound(u64),
@@ -16,14 +17,17 @@
     #[error("Invalid version vector: {0}")]
     InvalidVersion(String),
 }
 
+/// Result type for coherence operations.
 pub type CoherenceResult<T> = std::result::Result<T, CoherenceError>;
 
+/// Unique identifier for a cache lease.
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
 pub struct LeaseId(u64);
 
 impl LeaseId {
+    /// Creates a new lease ID from a raw numeric value.
     pub fn new(id: u64) -> Self {
         LeaseId(id)
     }
 }
@@ -33,23 +37,38 @@
         write!(f, "lease:{}", self.0)
     }
 }
 
+/// Current state of a cache lease.
 #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
 pub enum LeaseState {
+    /// Lease is active and valid.
     Active,
+    /// Lease has expired but not yet revoked.
     Expired,
+    /// Lease has been explicitly revoked.
     Revoked,
+    /// Lease is being renewed.
     Renewing,
 }
 
+/// Represents a granted cache lease for a file.
+///
+/// Thread-safe: the lease is immutable after creation, state changes
+/// happen through explicit methods.
 #[derive(Debug, Clone)]
 pub struct CacheLease {
+    /// Unique identifier for this lease.
     pub lease_id: LeaseId,
+    /// Inode number this lease is for.
     pub inode: u64,
+    /// Client ID that holds this lease.
     pub client_id: u64,
+    /// When the lease was granted.
     pub granted_at: Instant,
+    /// Duration the lease is valid for.
     pub duration: Duration,
+    /// Current state of the lease.
     pub state: LeaseState,
 }
 
 impl CacheLease {
[0m
[0m← [0mEdit crates/claudefs-fuse/src/cache_coherence.rs
[0mIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
@@ -71,8 +71,9 @@
     pub state: LeaseState,
 }
 
 impl CacheLease {
+    /// Creates a new cache lease.
     pub fn new(lease_id: LeaseId, inode: u64, client_id: u64, duration: Duration) -> Self {
         CacheLease {
             lease_id,
             inode,
@@ -82,32 +83,37 @@
             state: LeaseState::Active,
         }
     }
 
+    /// Returns true if the lease is currently valid.
     pub fn is_valid(&self) -> bool {
         self.state == LeaseState::Active && !self.is_expired()
     }
 
+    /// Returns true if the lease has expired.
     pub fn is_expired(&self) -> bool {
         if matches!(self.state, LeaseState::Expired | LeaseState::Revoked) {
             return true;
         }
         self.granted_at.elapsed() >= self.duration
     }
 
+    /// Returns the remaining time until the lease expires.
     pub fn time_remaining(&self) -> Duration {
         if self.is_expired() {
             Duration::ZERO
         } else {
             self.duration.saturating_sub(self.granted_at.elapsed())
         }
     }
 
+    /// Revokes this lease, marking it as invalid.
     pub fn revoke(&mut self) {
         debug!("Revoking lease {}", self.lease_id);
         self.state = LeaseState::Revoked;
     }
 
+    /// Renews the lease with a new duration.
     pub fn renew(&mut self, new_duration: Duration) {
         trace!(
             "Renewing lease {} with new duration {:?}",
             self.lease_id,


[0m← [0mEdit crates/claudefs-fuse/src/cache_coherence.rs

Index: /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
@@ -124,26 +124,38 @@
         self.state = LeaseState::Active;
     }
 }
 
+/// Reason why a cache entry was invalidated.
 #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
 pub enum InvalidationReason {
+    /// Lease expired naturally.
     LeaseExpired,
+    /// Remote node wrote to the file.
     RemoteWrite(u64),
+    /// Version conflict detected between caches.
     ConflictDetected,
+    /// Explicit flush requested by client.
     ExplicitFlush,
+    /// Node failover occurred.
     NodeFailover,
 }
 
+/// Represents a cache invalidation event.
 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct CacheInvalidation {
+    /// Inode that was invalidated.
     pub inode: u64,
+    /// Reason for invalidation.
     pub reason: InvalidationReason,
+    /// Version at time of invalidation.
     pub version: u64,
+    /// When the invalidation occurred.
     pub timestamp: SystemTime,
 }
 
 impl CacheInvalidation {
+    /// Creates a new cache invalidation event.
     pub fn new(inode: u64, reason: InvalidationReason, version: u64) -> Self {
         CacheInvalidation {
             inode,
             reason,
[0m
[0m← [0mEdit crates/claudefs-fuse/src/cache_coherence.rs[0m


Index: /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
@@ -164,24 +164,30 @@
         }
     }
 }
 
+/// Vector clock for tracking versions of cached inodes.
+///
+/// Used to detect conflicts in distributed cache coherence.
 #[derive(Debug, Clone, Default)]
 pub struct VersionVector {
     versions: HashMap<u64, u64>,
 }
 
 impl VersionVector {
+    /// Creates a new empty version vector.
     pub fn new() -> Self {
         VersionVector {
             versions: HashMap::new(),
         }
     }
 
+    /// Gets the version for an inode, or 0 if not present.
     pub fn get(&self, inode: u64) -> u64 {
         self.versions.get(&inode).copied().unwrap_or(0)
     }
 
+    /// Updates the version for an inode if the new version is higher.
     pub fn update(&mut self, inode: u64, version: u64) {
         let current = self.versions.get(&inode).copied().unwrap_or(0);
         if version > current {
             trace!(
@@ -193,8 +199,9 @@
             self.versions.insert(inode, version);
         }
     }
 
+    /// Returns inodes that have conflicting versions between two vectors.
     pub fn conflicts(&self, other: &VersionVector) -> Vec<u64> {
         let mut conflicted = Vec::new();
         let mut seen = std::collections::HashSet::new();
         let all_inodes: Vec<u64> = self
@@ -217,18 +224,21 @@
         }
         conflicted
     }
 
+    /// Merges another version vector into this one, taking max versions.
     pub fn merge(&mut self, other: &VersionVector) {
         for (&inode, &version) in &other.versions {
             self.update(inode, version);
         }
     }
 
+    /// Returns the number of tracked inodes.
     pub fn len(&self) -> usize {
         self.versions.len()
     }
 
+    /// Returns true if the vector is empty.
     pub fn is_empty(&self) -> bool {
         self.versions.is_empty()
     }
 }
[0m← [0mEdit crates/claudefs-fuse/src/cache_coherence.rs

[0mIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
@@ -242,16 +242,23 @@
         self.versions.is_empty()
     }
 }
 
+/// Cache coherence protocol to use.
 #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
 pub enum CoherenceProtocol {
+    /// Close-to-open consistency: cache invalidated on open after close.
     #[default]
     CloseToOpen,
+    /// Session-based coherence within a single mount session.
     SessionBased,
+    /// Strict consistency: all writes immediately visible to other clients.
     Strict,
 }
 
+/// Manages cache leases and invalidations for the coherence protocol.
+///
+/// Thread-safe: requires external synchronization for concurrent access.
 pub struct CoherenceManager {
     leases: HashMap<u64, CacheLease>,
     invalidations: Vec<CacheInvalidation>,
     _protocol: CoherenceProtocol,
@@ -259,8 +266,9 @@
     default_lease_duration: Duration,
 }
 
 impl CoherenceManager {
+    /// Creates a new coherence manager with the specified protocol.
     pub fn new(protocol: CoherenceProtocol) -> Self {
         CoherenceManager {
             leases: HashMap::new(),
             invalidations: Vec::new(),
@@ -269,8 +277,9 @@
             default_lease_duration: Duration::from_secs(30),
         }
     }
 
+    /// Grants a cache lease for an inode to a client.
     pub fn grant_lease(&mut self, inode: u64, client_id: u64) -> CacheLease {
         let lease_id = LeaseId::new(self.next_lease_id);
         self.next_lease_id += 1;
 
@@ -283,8 +292,9 @@
         self.leases.insert(inode, lease.clone());
         lease
     }
 
+    /// Revokes the lease for an inode, returning the invalidation event.
     pub fn revoke_lease(&mut self, inode: u64) -> Option<CacheInvalidation> {
         if let Some(lease) = self.leases.get_mut(&inode) {
             lease.revoke();
 
@@ -298,12 +308,14 @@
             None
         }
     }
 
+    /// Checks if a valid lease exists for an inode.
     pub fn check_lease(&self, inode: u64) -> Option<&CacheLease> {
         self.leases.get(&inode).filter(|lease| lease.is_valid())
     }
 
+    /// Invalidates the cache for an inode.
     pub fn invalidate(&mut self, inode: u64, reason: InvalidationReason, version: u64) {
         if let Some(lease) = self.leases.get_mut(&inode) {
             lease.revoke();
         }
@@ -312,21 +324,25 @@
         debug!("Invalidating inode {} with reason {:?}", inode, reason);
         self.invalidations.push(invalidation);
     }
 
+    /// Returns pending invalidations that haven't been processed.
     pub fn pending_invalidations(&self) -> &[CacheInvalidation] {
         &self.invalidations
     }
 
+    /// Drains and returns all pending invalidations.
     pub fn drain_invalidations(&mut self) -> Vec<CacheInvalidation> {
         trace!("Draining {} invalidations", self.invalidations.len());
         std::mem::take(&mut self.invalidations)
     }
 
+    /// Returns the count of currently active leases.
     pub fn active_lease_count(&self) -> usize {
         self.leases.values().filter(|l| l.is_valid()).count()
     }
 
+    /// Expires stale leases and returns the count of expired leases.
     pub fn expire_stale_leases(&mut self) -> usize {
         let mut count = 0;
         for lease in self.leases.values_mut() {
             if lease.is_expired() && lease.state == LeaseState::Active {
@@ -340,8 +356,9 @@
         }
         count
     }
 
+    /// Returns true if an inode has a valid lease (is coherent).
     pub fn is_coherent(&self, inode: u64) -> bool {
         self.leases
             .get(&inode)
             .map(|l| l.is_valid())

[0m← [0mEdit crates/claudefs-fuse/src/workload_class.rs
Index: /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
@@ -1,16 +1,25 @@
 use std::collections::HashMap;
 use tracing::{debug, trace};
 
+/// Classification of I/O workload types.
 #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
 pub enum WorkloadType {
+    /// AI training workloads: large sequential reads, high throughput.
     AiTraining,
+    /// AI inference workloads: low latency, small random reads.
     AiInference,
+    /// Web serving: high IOPS, mixed read/write.
     WebServing,
+    /// Database: random small I/O, transaction-oriented.
     Database,
+    /// Backup/restore: bulk sequential writes.
     Backup,
+    /// Interactive terminal/shell: minimal I/O, latency-sensitive.
     Interactive,
+    /// Streaming media: large sequential reads.
     Streaming,
+    /// Unknown or unclassified workload.
     Unknown,
 }
 
 impl WorkloadType {
[0m

Now let's document workload_class.rs:
[0m← [0mEdit crates/claudefs-fuse/src/workload_class.rs[0m


Index: /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
@@ -22,8 +22,9 @@
     Unknown,
 }
 
 impl WorkloadType {
+    /// Returns true if this workload type is latency-sensitive.
     pub fn is_latency_sensitive(&self) -> bool {
         matches!(
             self,
             WorkloadType::Interactive
@@ -32,15 +33,17 @@
                 | WorkloadType::Database
         )
     }
 
+    /// Returns true if this workload type is throughput-oriented.
     pub fn is_throughput_oriented(&self) -> bool {
         matches!(
             self,
             WorkloadType::AiTraining | WorkloadType::Backup | WorkloadType::Streaming
         )
     }
 
+    /// Returns the suggested read-ahead size in KB for this workload type.
     pub fn suggested_read_ahead_kb(&self) -> u64 {
         match self {
             WorkloadType::AiTraining => 2048,
             WorkloadType::AiInference => 512,
[0m← [0mEdit crates/claudefs-fuse/src/workload_class.rs
[0mIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
@@ -56,20 +56,29 @@
         }
     }
 }
 
+/// Captures the access pattern statistics for a file or client.
 #[derive(Debug, Clone, PartialEq)]
 pub struct AccessProfile {
+    /// Total bytes read.
     pub read_bytes: u64,
+    /// Total bytes written.
     pub write_bytes: u64,
+    /// Number of read operations.
     pub read_ops: u64,
+    /// Number of write operations.
     pub write_ops: u64,
+    /// Number of sequential read operations.
     pub sequential_reads: u64,
+    /// Number of random read operations.
     pub random_reads: u64,
+    /// Average size of read operations in bytes.
     pub avg_read_size_bytes: u64,
 }
 
 impl AccessProfile {
+    /// Creates a new empty access profile.
     pub fn new() -> Self {
         Self {
             read_bytes: 0,
             write_bytes: 0,
@@ -80,8 +89,9 @@
             avg_read_size_bytes: 0,
         }
     }
 
+    /// Records a read operation.
     pub fn record_read(&mut self, bytes: u64, is_sequential: bool) {
         self.read_bytes += bytes;
         self.read_ops += 1;
         if is_sequential {
@@ -93,13 +103,15 @@
             self.avg_read_size_bytes = self.read_bytes / self.read_ops;
         }
     }
 
+    /// Records a write operation.
     pub fn record_write(&mut self, bytes: u64) {
         self.write_bytes += bytes;
         self.write_ops += 1;
     }
 
+    /// Returns the ratio of reads to total I/O (0.0 = all writes, 1.0 = all reads).
     pub fn read_write_ratio(&self) -> f64 {
         let total = self.read_bytes + self.write_bytes;
         if total == 0 {
             1.0
@@ -107,8 +119,9 @@
             self.read_bytes as f64 / total as f64
         }
     }
 
+    /// Returns the ratio of sequential to total reads (0.0 = all random, 1.0 = all sequential).
     pub fn sequential_ratio(&self) -> f64 {
         let total = self.sequential_reads + self.random_reads;
         if total == 0 {
             0.0
@@ -116,12 +129,14 @@
             self.sequential_reads as f64 / total as f64
         }
     }
 
+    /// Returns the total number of I/O operations.
     pub fn total_ops(&self) -> u64 {
         self.read_ops + self.write_ops
     }
 
+    /// Returns true if reads constitute more than 75% of I/O.
     pub fn is_read_heavy(&self) -> bool {
         if self.read_bytes == 0 && self.write_bytes == 0 {
             return false;
         }


[0m← [0mEdit crates/claudefs-fuse/src/workload_class.rs

Index: /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
@@ -149,30 +149,46 @@
         Self::new()
     }
 }
 
+/// Tuning recommendation based on workload classification.
 #[derive(Debug, Clone, PartialEq)]
 pub enum TuningHint {
+    /// Increase kernel read-ahead for this file.
     IncreaseReadAhead,
+    /// Decrease kernel read-ahead.
     DecreaseReadAhead,
+    /// Use direct I/O to bypass page cache.
     UseDirectIo,
+    /// Enable transparent compression.
     EnableCompression,
+    /// Disable transparent compression.
     DisableCompression,
+    /// Optimize for low latency over throughput.
     PrioritizeLatency,
+    /// Optimize for throughput over latency.
     PrioritizeThroughput,
+    /// Increase cache allocation for this file.
     IncreaseCache,
+    /// Reduce cache allocation for this file.
     ReduceCache,
 }
 
+/// Compact signature extracted from an access profile.
 #[derive(Debug, Clone, PartialEq)]
 pub struct WorkloadSignature {
+    /// Read-to-write ratio (0-1).
     pub read_write_ratio: f64,
+    /// Sequential-to-random ratio (0-1).
     pub sequential_ratio: f64,
+    /// Average I/O size in kilobytes.
     pub avg_io_size_kb: f64,
+    /// Operations per second.
     pub ops_per_second: f64,
 }
 
 impl WorkloadSignature {
+    /// Creates a signature from an access profile and observation window.
     pub fn from_profile(profile: &AccessProfile, elapsed_secs: f64) -> Self {
         let total_ops = profile.total_ops();
         let ops_per_second = if elapsed_secs > 0.0 {
             total_ops as f64 / elapsed_secs
@@ -194,16 +210,19 @@
             ops_per_second,
         }
     }
 
+    /// Returns true if the signature matches AI training workloads.
     pub fn matches_ai_training(&self) -> bool {
         self.sequential_ratio > 0.8 && self.avg_io_size_kb >= 256.0
     }
 
+    /// Returns true if the signature matches database workloads.
     pub fn matches_database(&self) -> bool {
         self.sequential_ratio < 0.3 && self.avg_io_size_kb < 16.0 && self.ops_per_second < 500.0
     }
 
+    /// Returns true if the signature matches backup workloads.
     pub fn matches_backup(&self) -> bool {
         self.read_write_ratio < 0.1 && (self.sequential_ratio > 0.9 || self.sequential_ratio == 0.0)
     }
 }
[0m
[0m← [0mEdit crates/claudefs-fuse/src/workload_class.rs
Index: /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
@@ -226,44 +226,54 @@
         self.read_write_ratio < 0.1 && (self.sequential_ratio > 0.9 || self.sequential_ratio == 0.0)
     }
 }
 
+/// Result of workload classification with confidence and tuning hints.
 #[derive(Debug, Clone, PartialEq)]
 pub struct ClassificationResult {
+    /// The classified workload type.
     pub workload_type: WorkloadType,
+    /// Confidence score (0-1) of the classification.
     pub confidence: f64,
+    /// Recommended tuning hints for this workload.
     pub hints: Vec<TuningHint>,
 }
 
 impl ClassificationResult {
+    /// Creates a new classification result.
     pub fn new(workload_type: WorkloadType, confidence: f64) -> Self {
         Self {
             workload_type,
             confidence,
             hints: Vec::new(),
         }
     }
 
+    /// Adds a tuning hint to the result.
     pub fn add_hint(&mut self, hint: TuningHint) {
         self.hints.push(hint);
     }
 
+    /// Returns true if confidence is >= 0.7.
     pub fn is_high_confidence(&self) -> bool {
         self.confidence >= 0.7
     }
 }
 
+/// Classifies I/O workloads based on access patterns.
 pub struct WorkloadClassifier {
     min_ops_for_classification: u64,
 }
 
 impl WorkloadClassifier {
+    /// Creates a new workload classifier with default settings.
     pub fn new() -> Self {
         Self {
             min_ops_for_classification: 100,
         }
     }
 
+    /// Classifies a workload based on its access profile.
     pub fn classify(&self, profile: &AccessProfile, elapsed_secs: f64) -> ClassificationResult {
         let total_ops = profile.total_ops();
         trace!(total_ops, elapsed_secs, "classifying workload");
 


[0m[0m← [0mEdit crates/claudefs-fuse/src/workload_class.rs
[0m
Index: /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/workload_class.rs
@@ -346,16 +346,20 @@
         Self::new()
     }
 }
 
+/// Adaptive tuner that tracks per-inode access patterns and recommends tuning.
+///
+/// Thread-safe: requires external synchronization for concurrent access.
 pub struct AdaptiveTuner {
     policies: HashMap<u64, ClassificationResult>,
     classifier: WorkloadClassifier,
     profiles: HashMap<u64, AccessProfile>,
     window_start: std::time::Instant,
 }
 
 impl AdaptiveTuner {
+    /// Creates a new adaptive tuner.
     pub fn new() -> Self {
         Self {
             policies: HashMap::new(),
             classifier: WorkloadClassifier::new(),
@@ -363,20 +367,23 @@
             window_start: std::time::Instant::now(),
         }
     }
 
+    /// Records a read operation for an inode.
     pub fn record_read(&mut self, inode: u64, bytes: u64, is_sequential: bool) {
         trace!(inode, bytes, is_sequential, "recording read");
         let profile = self.profiles.entry(inode).or_default();
         profile.record_read(bytes, is_sequential);
     }
 
+    /// Records a write operation for an inode.
     pub fn record_write(&mut self, inode: u64, bytes: u64) {
         trace!(inode, bytes, "recording write");
         let profile = self.profiles.entry(inode).or_default();
         profile.record_write(bytes);
     }
 
+    /// Classifies the workload for an inode and returns the result.
     pub fn classify_inode(&mut self, inode: u64) -> &ClassificationResult {
         let elapsed = self.window_start.elapsed().as_secs_f64();
         let profile = match self.profiles.get(&inode) {
             Some(p) => p,
@@ -390,19 +397,22 @@
         trace!(inode, ?result.workload_type, confidence = result.confidence, "classification complete");
         self.policies.entry(inode).or_insert(result)
     }
 
+    /// Gets the suggested read-ahead size in KB for an inode.
     pub fn get_read_ahead_kb(&self, inode: u64) -> u64 {
         self.policies
             .get(&inode)
             .map(|r| r.workload_type.suggested_read_ahead_kb())
             .unwrap_or(128)
     }
 
+    /// Returns the number of tracked inodes.
     pub fn tracked_inodes(&self) -> usize {
         self.profiles.len()
     }
 
+    /// Removes all profile and policy data for an inode.
     pub fn evict_inode(&mut self, inode: u64) {
         trace!(inode, "evicting inode");
         self.policies.remove(&inode);
         self.profiles.remove(&inode);

[0m← [0mEdit crates/claudefs-fuse/src/sec_policy.rs


Index: /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
@@ -3,53 +3,75 @@
 use std::time::{SystemTime, UNIX_EPOCH};
 use thiserror::Error;
 use tracing::{debug, info, warn};
 
+/// Linux capability definitions for fine-grained privilege control.
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
 pub enum Capability {
+    /// System administration (mount, reboot, etc.).
     SysAdmin,
+    /// DAC read search permission.
     DacReadSearch,
+    /// Override DAC restrictions.
     DacOverride,
+    /// Change file ownership.
     Chown,
+    /// Bypass file owner restrictions.
     FOwner,
+    /// Set file mode bits (setuid/setgid).
     FSetId,
+    /// Send signals to arbitrary processes.
     Kill,
+    /// Set process GID.
     SetGid,
+    /// Set process UID.
     SetUid,
+    /// Set process capabilities.
     SetPCap,
+    /// Network administration.
     NetAdmin,
+    /// Change root directory (chroot).
     SysChroot,
+    /// Create special files (mknod).
     Mknod,
+    /// Take file leases.
     Lease,
+    /// Write audit logs.
     AuditWrite,
 }
 
+/// A set of Linux capabilities.
 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct CapabilitySet {
     caps: Vec<Capability>,
 }
 
 impl CapabilitySet {
+    /// Creates a new empty capability set.
     pub fn new() -> Self {
         Self { caps: Vec::new() }
     }
 
+    /// Returns a minimal capability set for FUSE operation.
     pub fn fuse_minimal() -> Self {
         let mut caps = Self::new();
         caps.add(Capability::SysAdmin);
         caps
     }
 
+    /// Returns true if the set contains the given capability.
     pub fn contains(&self, cap: &Capability) -> bool {
         self.caps.contains(cap)
     }
 
+    /// Adds a capability to the set (no-op if already present).
     pub fn add(&mut self, cap: Capability) {
         if !self.caps.contains(&cap) {
             self.caps.push(cap);
         }
     }
 
+    /// Removes a capability from the set.
     pub fn remove(&mut self, cap: Capability) -> bool {
         if let Some(pos) = self.caps.iter().position(|c| c == &cap) {
             self.caps.remove(pos);
             true
@@ -57,12 +79,14 @@
             false
         }
     }
 
+    /// Returns the number of capabilities in the set.
     pub fn len(&self) -> usize {
         self.caps.len()
     }
 
+    /// Returns true if the set is empty.
     pub fn is_empty(&self) -> bool {
         self.caps.is_empty()
     }
 }
[0m[0m← [0mEdit crates/claudefs-fuse/src/sec_policy.rs
[0m
Index: /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
@@ -96,32 +96,39 @@
         Self::new()
     }
 }
 
+/// Seccomp filter mode.
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
 pub enum SeccompMode {
+    /// No seccomp filtering.
     #[default]
     Disabled,
+    /// Log violations but allow syscalls.
     Log,
+    /// Enforce the policy, blocking violations.
     Enforce,
 }
 
+/// Policy controlling which syscalls are allowed or blocked.
 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct SyscallPolicy {
     mode: SeccompMode,
     allowed: Vec<String>,
     blocked: Vec<String>,
 }
 
 impl SyscallPolicy {
+    /// Creates a new empty syscall policy.
     pub fn new() -> Self {
         Self {
             mode: SeccompMode::Disabled,
             allowed: Vec::new(),
             blocked: Vec::new(),
         }
     }
 
+    /// Creates a policy allowing all syscalls needed for FUSE operation.
     pub fn fuse_allowlist() -> Self {
         let allowed = [
             "read",
             "write",

[0m← [0mEdit crates/claudefs-fuse/src/sec_policy.rs

[0m
Index: /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
@@ -459,8 +459,9 @@
         blocked: Vec::new(),
     }
 }
 
+/// Returns true if the syscall is permitted by this policy.
 pub fn is_allowed(&self, syscall: &str) -> bool {
     if self.allowed.is_empty() {
         !self.blocked.contains(&syscall.to_string())
     } else {
@@ -468,20 +469,23 @@
             && !self.blocked.contains(&syscall.to_string())
     }
 }
 
+/// Returns true if the syscall is explicitly blocked.
 pub fn is_blocked(&self, syscall: &str) -> bool {
     if self.blocked.is_empty() {
         false
     } else {
         self.blocked.contains(&syscall.to_string())
     }
 }
 
+/// Returns the seccomp mode for this policy.
 pub fn mode(&self) -> SeccompMode {
     self.mode
 }
 
+/// Returns a new policy with the specified seccomp mode.
 pub fn with_mode(mut self, mode: SeccompMode) -> Self {
     self.mode = mode;
     self
 }
[0m← [0mEdit crates/claudefs-fuse/src/sec_policy.rs


Index: /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
@@ -496,16 +496,18 @@
         Self::new()
     }
 }
 
+/// Represents a Linux mount namespace.
 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct MountNamespace {
     ns_id: u64,
     pid: u32,
     created_at_secs: u64,
 }
 
 impl MountNamespace {
+    /// Creates a new mount namespace from an ID and owning PID.
     pub fn new(ns_id: u64, pid: u32) -> Self {
         let created_at_secs = SystemTime::now()
             .duration_since(UNIX_EPOCH)
             .map(|d| d.as_secs())
@@ -516,20 +518,23 @@
             created_at_secs,
         }
     }
 
+    /// Returns the age of the namespace in seconds.
     pub fn age_secs(&self) -> u64 {
         let now = SystemTime::now()
             .duration_since(UNIX_EPOCH)
             .map(|d| d.as_secs())
             .unwrap_or(0);
         now.saturating_sub(self.created_at_secs)
     }
 
+    /// Returns the namespace ID.
     pub fn ns_id(&self) -> u64 {
         self.ns_id
     }
 
+    /// Returns the PID of the namespace creator.
     pub fn pid(&self) -> u32 {
         self.pid
     }
 }
[0m
[0m
[0m← [0mEdit crates/claudefs-fuse/src/sec_policy.rs
Index: /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
@@ -538,8 +538,9 @@
         self.pid
     }
 }
 
+/// Security profile for a FUSE client or operation.
 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct SecurityProfile {
     capabilities: CapabilitySet,
     syscall_policy: SyscallPolicy,
@@ -547,8 +548,9 @@
     enforce_no_new_privs: bool,
 }
 
 impl SecurityProfile {
+    /// Creates a default permissive security profile.
     pub fn default_profile() -> Self {
         Self {
             capabilities: CapabilitySet::new(),
             syscall_policy: SyscallPolicy::new(),
@@ -556,8 +558,9 @@
             enforce_no_new_privs: false,
         }
     }
 
+    /// Creates a hardened security profile for untrusted environments.
     pub fn hardened() -> Self {
         Self {
             capabilities: CapabilitySet::fuse_minimal(),
             syscall_policy: SyscallPolicy::fuse_allowlist(),
@@ -565,8 +568,9 @@
             enforce_no_new_privs: true,
         }
     }
 
+    /// Creates a profile with the specified capabilities.
     pub fn with_capabilities(caps: CapabilitySet) -> Self {
         Self {
             capabilities: caps,
             syscall_policy: SyscallPolicy::new(),
@@ -574,8 +578,9 @@
             enforce_no_new_privs: false,
         }
     }
 
+    /// Creates a profile with the specified syscall policy.
     pub fn with_syscall_policy(policy: SyscallPolicy) -> Self {
         Self {
             capabilities: CapabilitySet::new(),
             syscall_policy: policy,
@@ -583,30 +588,36 @@
             enforce_no_new_privs: false,
         }
     }
 
+    /// Returns true if the syscall is permitted by this profile.
     pub fn is_syscall_permitted(&self, syscall: &str) -> bool {
         self.syscall_policy.is_allowed(syscall)
     }
 
+    /// Returns the required capabilities for this profile.
     pub fn required_capabilities(&self) -> &CapabilitySet {
         &self.capabilities
     }
 
+    /// Returns a new profile with the specified mount namespace.
     pub fn with_mount_namespace(mut self, ns: MountNamespace) -> Self {
         self.mount_ns = Some(ns);
         self
     }
 
+    /// Returns a new profile with the no_new_privs flag set.
     pub fn with_no_new_privs(mut self, enabled: bool) -> Self {
         self.enforce_no_new_privs = enabled;
         self
     }
 
+    /// Returns the mount namespace if set.
     pub fn mount_ns(&self) -> Option<&MountNamespace> {
         self.mount_ns.as_ref()
     }
 
+    /// Returns true if no_new_privs is enforced.
     pub fn enforce_no_new_privs(&self) -> bool {
         self.enforce_no_new_privs
     }
 }
[0m← [0mEdit crates/claudefs-fuse/src/sec_policy.rs
[0m

Index: /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
@@ -627,8 +627,9 @@
         Self::default_profile()
     }
 }
 
+/// Type of security policy violation.
 #[derive(Debug, Clone, Error, Serialize, Deserialize)]
 pub enum ViolationType {
     #[error("Unauthorized syscall: {0}")]
     UnauthorizedSyscall(String),
@@ -639,32 +640,37 @@
     #[error("Unauthorized mount: {0}")]
     UnauthorizedMount(String),
 }
 
+/// Represents a security policy violation event.
 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct PolicyViolation {
     violation_type: ViolationType,
     details: String,
     timestamp: SystemTime,
 }
 
 impl PolicyViolation {
+    /// Creates a new policy violation.
     pub fn new(vtype: ViolationType, details: &str) -> Self {
         Self {
             violation_type: vtype,
             details: details.to_string(),
             timestamp: SystemTime::now(),
         }
     }
 
+    /// Returns the type of violation.
     pub fn violation_type(&self) -> &ViolationType {
         &self.violation_type
     }
 
+    /// Returns additional details about the violation.
     pub fn details(&self) -> &str {
         &self.details
     }
 
+    /// Returns when the violation occurred.
     pub fn timestamp(&self) -> SystemTime {
         self.timestamp
     }
 }

[0m[0m← [0mEdit crates/claudefs-fuse/src/sec_policy.rs

Index: /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
@@ -674,29 +674,35 @@
         self.timestamp
     }
 }
 
+/// Enforces security policies and tracks violations.
+///
+/// Thread-safe: requires external synchronization for concurrent access.
 #[derive(Debug, Clone)]
 pub struct PolicyEnforcer {
     profile: SecurityProfile,
     violations: Vec<PolicyViolation>,
     max_violations: usize,
 }
 
 impl PolicyEnforcer {
+    /// Creates a new policy enforcer with the given profile.
     pub fn new(profile: SecurityProfile) -> Self {
         Self {
             profile,
             violations: Vec::new(),
             max_violations: 100,
         }
     }
 
+    /// Returns a new enforcer with the specified max violations limit.
     pub fn with_max_violations(mut self, max: usize) -> Self {
         self.max_violations = max;
         self
     }
 
+    /// Checks if a syscall is permitted, recording violations if not.
     pub fn check_syscall(&mut self, syscall: &str) -> Result<()> {
         if self.is_over_limit() {
             warn!("Policy enforcement limit reached, rejecting syscall checks");
             return Err(FuseError::NotSupported {
@@ -717,8 +723,9 @@
         }
         Ok(())
     }
 
+    /// Records a policy violation.
     pub fn record_violation(&mut self, vtype: ViolationType, details: &str) {
         let violation = PolicyViolation::new(vtype, details);
         info!("Policy violation recorded: {}", violation.details());
         self.violations.push(violation);
@@ -727,29 +734,35 @@
             self.violations.remove(0);
         }
     }
 
+    /// Returns the number of recorded violations.
     pub fn violation_count(&self) -> usize {
         self.violations.len()
     }
 
+    /// Returns the n most recent violations.
     pub fn recent_violations(&self, n: usize) -> &[PolicyViolation] {
         let start = self.violations.len().saturating_sub(n);
         &self.violations[start..]
     }
 
+    /// Returns true if the violation limit has been reached.
     pub fn is_over_limit(&self) -> bool {
         self.violation_count() >= self.max_violations
     }
 
+    /// Clears all recorded violations.
     pub fn clear_violations(&mut self) {
         self.violations.clear();
     }
 
+    /// Returns a reference to the security profile.
     pub fn profile(&self) -> &SecurityProfile {
         &self.profile
     }
 
+    /// Returns the maximum allowed violations.
     pub fn max_violations(&self) -> usize {
         self.max_violations
     }
 }
Now let's document client_auth.rs:
[0m← [0mEdit crates/claudefs-fuse/src/client_auth.rs

[0mIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
@@ -8,72 +8,105 @@
 //! - CRL-based revocation
 
 use thiserror::Error;
 
+/// Result type for authentication operations.
 pub type Result<T> = std::result::Result<T, AuthError>;
 
+/// Current authentication state of the client.
 #[derive(Debug, Clone, PartialEq)]
 pub enum AuthState {
+    /// Client has not enrolled with the cluster.
     Unenrolled,
+    /// Enrollment in progress, waiting for certificate.
     Enrolling {
+        /// One-time enrollment token.
         token: String,
+        /// When enrollment started (Unix timestamp).
         started_at_secs: u64,
     },
+    /// Successfully enrolled with a valid certificate.
     Enrolled {
+        /// SHA-256 fingerprint of the certificate.
         cert_fingerprint: [u8; 32],
+        /// When the certificate expires (Unix timestamp).
         expires_at_secs: u64,
     },
+    /// Certificate renewal in progress.
     Renewing {
+        /// Fingerprint of the certificate being replaced.
         old_fingerprint: [u8; 32],
+        /// When renewal started (Unix timestamp).
         started_at_secs: u64,
     },
+    /// Client has been revoked and cannot re-enroll.
     Revoked {
+        /// Reason for revocation.
         reason: String,
+        /// When revocation occurred (Unix timestamp).
         revoked_at_secs: u64,
     },
 }
 
+/// Record of an enrolled certificate.
 #[derive(Debug, Clone)]
 pub struct CertRecord {
+    /// SHA-256 fingerprint of the certificate.
     pub fingerprint: [u8; 32],
+    /// Certificate subject (CN).
     pub subject: String,
+    /// When the certificate was issued (Unix timestamp).
     pub issued_at_secs: u64,
+    /// When the certificate expires (Unix timestamp).
     pub expires_at_secs: u64,
+    /// PEM-encoded certificate.
     pub cert_pem: String,
+    /// PEM-encoded private key.
     pub key_pem: String,
 }
 
 impl CertRecord {
+    /// Returns true if the certificate has expired.
     pub fn is_expired(&self, now_secs: u64) -> bool {
         now_secs >= self.expires_at_secs
     }
 
+    /// Returns true if the certificate should be renewed.
     pub fn needs_renewal(&self, now_secs: u64, renew_before_secs: u64) -> bool {
         let renewal_threshold = self.expires_at_secs.saturating_sub(renew_before_secs);
         now_secs >= renewal_threshold
     }
 
+    /// Returns the number of days until the certificate expires (negative if expired).
     pub fn days_until_expiry(&self, now_secs: u64) -> i64 {
         let secs_until = self.expires_at_secs as i64 - now_secs as i64;
         secs_until / 86400
     }
 }
 
+/// A revoked certificate entry in the CRL.
 #[derive(Debug, Clone)]
 pub struct RevokedCert {
+    /// SHA-256 fingerprint of the revoked certificate.
     pub fingerprint: [u8; 32],
+    /// Reason for revocation.
     pub reason: String,
+    /// When the certificate was revoked (Unix timestamp).
     pub revoked_at_secs: u64,
 }
 
+/// Manages client authentication lifecycle including enrollment, renewal, and revocation.
+///
+/// Thread-safe: requires external synchronization for concurrent access.
 pub struct ClientAuthManager {
     state: AuthState,
     cert: Option<CertRecord>,
     crl: Vec<RevokedCert>,
     _cert_dir: String,
 }
 
 impl ClientAuthManager {
+    /// Creates a new authentication manager.
     pub fn new(cert_dir: &str) -> Self {
         Self {
             state: AuthState::Unenrolled,
             cert: None,
@@ -81,16 +114,19 @@
             _cert_dir: cert_dir.to_string(),
         }
     }
 
+    /// Returns the current authentication state.
     pub fn state(&self) -> &AuthState {
         &self.state
     }
 
+    /// Returns the current certificate record if enrolled.
     pub fn cert(&self) -> Option<&CertRecord> {
         self.cert.as_ref()
     }
 
+    /// Begins the enrollment process with a one-time token.
     pub fn begin_enrollment(&mut self, token: &str, now_secs: u64) -> Result<()> {
         match &self.state {
             AuthState::Unenrolled => {
                 self.state = AuthState::Enrolling {
@@ -105,8 +141,9 @@
             AuthState::Revoked { .. } => Err(AuthError::NotEnrolled),
         }
     }
 
+    /// Completes enrollment with the provided certificate and key.
     pub fn complete_enrollment(
         &mut self,
         cert_pem: &str,
         key_pem: &str,
@@ -141,16 +178,18 @@
             _ => Err(AuthError::EnrollmentInProgress),
         }
     }
 
+    /// Returns true if the current certificate needs renewal.
     pub fn needs_renewal(&self, now_secs: u64, renew_before_secs: u64) -> bool {
         if let Some(cert) = &self.cert {
             cert.needs_renewal(now_secs, renew_before_secs)
         } else {
             false
         }
     }
 
+    /// Begins the certificate renewal process.
     pub fn begin_renewal(&mut self, now_secs: u64) -> Result<()> {
         match &self.state {
             AuthState::Enrolled {
                 cert_fingerprint,
@@ -167,8 +206,9 @@
             _ => Err(AuthError::EnrollmentInProgress),
         }
     }
 
+    /// Completes renewal with the new certificate and key.
     pub fn complete_renewal(&mut self, cert_pem: &str, key_pem: &str, now_secs: u64) -> Result<()> {
         match &self.state {
             AuthState::Renewing {
                 old_fingerprint: _,
@@ -198,8 +238,9 @@
             _ => Err(AuthError::NotEnrolled),
         }
     }
 
+    /// Revokes the client's certificate.
     pub fn revoke(&mut self, reason: &str, now_secs: u64) {
         if let AuthState::Enrolled {
             cert_fingerprint: _,
             expires_at_secs: _,
@@ -214,24 +255,28 @@
             }
         }
     }
 
+    /// Adds a certificate to the CRL.
     pub fn add_to_crl(&mut self, fingerprint: [u8; 32], reason: &str, revoked_at_secs: u64) {
         self.crl.push(RevokedCert {
             fingerprint,
             reason: reason.to_string(),
             revoked_at_secs,
         });
     }
 
+    /// Returns true if the certificate is revoked.
     pub fn is_revoked(&self, fingerprint: &[u8; 32]) -> bool {
         self.crl.iter().any(|r| r.fingerprint == *fingerprint)
     }
 
+    /// Returns the number of entries in the CRL.
     pub fn crl_len(&self) -> usize {
         self.crl.len()
     }
 
+    /// Removes expired entries from the CRL, returns count of removed entries.
     pub fn compact_crl(&mut self, now_secs: u64, max_age_secs: u64) -> usize {
         let old_len = self.crl.len();
         self.crl
             .retain(|entry| now_secs.saturating_sub(entry.revoked_at_secs) < max_age_secs);

[0m← [0mEdit crates/claudefs-fuse/src/client_auth.rs
[0mIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
@@ -283,16 +283,18 @@
         old_len - self.crl.len()
     }
 }
 
+/// Computes a simple fingerprint from PEM content.
 fn compute_fingerprint(pem: &str) -> [u8; 32] {
     let mut hash = [0u8; 32];
     for (i, byte) in pem.bytes().enumerate() {
         hash[i % 32] = hash[i % 32].wrapping_add(byte);
     }
     hash
 }
 
+/// Parses expiration date from PEM certificate.
 fn parse_expiry_from_pem(pem: &str) -> Option<u64> {
     if pem.contains("2030") {
         Some(1893456000)
     } else if pem.contains("2025") {
@@ -301,8 +303,9 @@
         None
     }
 }
 
+/// Parses subject CN from PEM certificate.
 fn parse_subject_from_pem(pem: &str) -> Option<String> {
     if let Some(start) = pem.find("/CN=") {
         let rest = &pem[start + 4..];
         let end = rest.find(['/', '\n', '\r']).unwrap_or(rest.len());
@@ -311,8 +314,9 @@
         None
     }
 }
 
+/// Authentication errors.
 #[derive(Debug, Error)]
 pub enum AuthError {
     #[error("Not enrolled")]
     NotEnrolled,


Now let's verify the documentation is complete by running clippy:
[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "missing_docs|warning:" | head -50
warning: missing documentation for an enum
1 | #![warn(missing_docs)]
warning: missing documentation for a variant
warning: missing documentation for a variant
warning: missing documentation for a variant
warning: missing documentation for a method
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a method
warning: missing documentation for a struct
warning: missing documentation for an associated function
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for an associated function
warning: missing documentation for a method
warning: missing documentation for a method
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "src/(filesystem|cache_coherence|workload_class|sec_policy|client_auth)" | head -50--> crates/claudefs-fuse/src/cache_coherence.rs:12:5
  --> crates/claudefs-fuse/src/cache_coherence.rs:14:5
  --> crates/claudefs-fuse/src/cache_coherence.rs:16:5
  --> crates/claudefs-fuse/src/cache_coherence.rs:18:5
   --> crates/claudefs-fuse/src/client_auth.rs:322:5
   --> crates/claudefs-fuse/src/client_auth.rs:324:5
   --> crates/claudefs-fuse/src/client_auth.rs:326:5
   --> crates/claudefs-fuse/src/client_auth.rs:328:5
   --> crates/claudefs-fuse/src/client_auth.rs:330:5
   --> crates/claudefs-fuse/src/client_auth.rs:332:5
   --> crates/claudefs-fuse/src/sec_policy.rs:635:5
   --> crates/claudefs-fuse/src/sec_policy.rs:637:5
   --> crates/claudefs-fuse/src/sec_policy.rs:639:5
   --> crates/claudefs-fuse/src/sec_policy.rs:641:5[0m


Let me check what specific items need documentation:
[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -B 1 "missing documentation" | grep -E "^(   -->|warning:)" | head -30
warning: missing documentation for an enum
warning: missing documentation for a variant
warning: missing documentation for a variant
warning: missing documentation for a variant
warning: missing documentation for a method
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a method
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a struct field
warning: missing documentation for a method
warning: missing documentation for a struct
warning: missing documentation for an associated function
warning: missing documentation for a method
warning: missing documentation for a method[0m

[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -A 1 "missing documentation for" | head -60[0m


warning: missing documentation for an enum
 --> crates/claudefs-fuse/src/buffer_pool.rs:6:1
--
warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/buffer_pool.rs:7:5
--
warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/buffer_pool.rs:8:5
--
warning: missing documentation for a variant
 --> crates/claudefs-fuse/src/buffer_pool.rs:9:5
--
warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:13:5
--
warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/buffer_pool.rs:22:1
--
warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:23:5
--
warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:24:5
--
warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:25:5
--
warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:29:5
--
warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:32:5
--
warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:35:5
--
warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:38:5
--
warning: missing documentation for a method
  --> crates/claudefs-fuse/src/buffer_pool.rs:41:5
--
warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/buffer_pool.rs:50:1
--
warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:51:5
--
warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:52:5
--
warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:53:5
--
warning: missing documentation for a struct
  --> crates/claudefs-fuse/src/buffer_pool.rs:67:1
--
warning: missing documentation for a struct field
  --> crates/claudefs-fuse/src/buffer_pool.rs:68:5
--Let me specifically check warnings in the 5 target files:
[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "(filesystem\.rs|cache_coherence\.rs|workload_class\.rs|sec_policy\.rs|client_auth\.rs)" | grep "missing documentation"
[0m[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -c "cache_coherence.rs\|client_auth.rs\|sec_policy.rs"14
[0m


[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "cache_coherence\.rs|client_auth\.rs|sec_policy\.rs" | head -20--> crates/claudefs-fuse/src/cache_coherence.rs:12:5
  --> crates/claudefs-fuse/src/cache_coherence.rs:14:5
  --> crates/claudefs-fuse/src/cache_coherence.rs:16:5
  --> crates/claudefs-fuse/src/cache_coherence.rs:18:5
   --> crates/claudefs-fuse/src/client_auth.rs:322:5
   --> crates/claudefs-fuse/src/client_auth.rs:324:5
   --> crates/claudefs-fuse/src/client_auth.rs:326:5
   --> crates/claudefs-fuse/src/client_auth.rs:328:5
   --> crates/claudefs-fuse/src/client_auth.rs:330:5
   --> crates/claudefs-fuse/src/client_auth.rs:332:5
   --> crates/claudefs-fuse/src/sec_policy.rs:635:5
   --> crates/claudefs-fuse/src/sec_policy.rs:637:5
   --> crates/claudefs-fuse/src/sec_policy.rs:639:5
   --> crates/claudefs-fuse/src/sec_policy.rs:641:5[0m


[0m→ [0mRead crates/claudefs-fuse/src/cache_coherence.rs[90m [offset=10, limit=20][0m
[0m[0m← [0mEdit crates/claudefs-fuse/src/cache_coherence.rs

Index: /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/cache_coherence.rs
@@ -7,14 +7,18 @@
 
 /// Errors that can occur in the cache coherence protocol.
 #[derive(Error, Debug)]
 pub enum CoherenceError {
+    /// No lease exists for the specified inode.
     #[error("Lease not found for inode {0}")]
     LeaseNotFound(u64),
+    /// The lease has expired.
     #[error("Lease {0} has expired")]
     LeaseExpired(LeaseId),
+    /// The lease is in an invalid state for the operation.
     #[error("Lease {0} is in invalid state")]
     InvalidLeaseState(LeaseId),
+    /// The version vector is invalid.
     #[error("Invalid version vector: {0}")]
     InvalidVersion(String),
 }
 
[0m

[0m→ [0mRead crates/claudefs-fuse/src/client_auth.rs[90m [limit=20, offset=318][0m
[0m

Index: /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/client_auth.rs
@@ -317,18 +317,24 @@
 
 /// Authentication errors.
 #[derive(Debug, Error)]
 pub enum AuthError {
+    /// Client has not enrolled with the cluster.
     #[error("Not enrolled")]
     NotEnrolled,
+    /// Client is already enrolled.
     #[error("Already enrolled")]
     AlreadyEnrolled,
+    /// Enrollment or renewal is already in progress.
     #[error("Enrollment in progress")]
     EnrollmentInProgress,
+    /// Client has been revoked.
     #[error("Already revoked")]
     AlreadyRevoked,
+    /// The PEM content is invalid.
     #[error("Invalid PEM: {0}")]
     InvalidPem(String),
+    /// The certificate has expired.
     #[error("Certificate expired")]
     CertExpired,
 }
 

[0m
[0m← [0mEdit crates/claudefs-fuse/src/client_auth.rs[0m→ [0mRead crates/claudefs-fuse/src/sec_policy.rs[90m [limit=20, offset=630][0m

[0m← [0mEdit crates/claudefs-fuse/src/sec_policy.rs
[0mIndex: /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-fuse/src/sec_policy.rs
@@ -630,14 +630,18 @@
 
 /// Type of security policy violation.
 #[derive(Debug, Clone, Error, Serialize, Deserialize)]
 pub enum ViolationType {
+    /// A syscall was attempted that is not permitted.
     #[error("Unauthorized syscall: {0}")]
     UnauthorizedSyscall(String),
+    /// Attempt to gain capabilities beyond what is allowed.
     #[error("Capability escalation attempt: {0}")]
     CapabilityEscalation(String),
+    /// Attempt to acquire new privileges.
     #[error("New privileges attempt: {0}")]
     NewPrivilegesAttempt(String),
+    /// Unauthorized mount operation.
     #[error("Unauthorized mount: {0}")]
     UnauthorizedMount(String),
 }
 
[0m

[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "(filesystem\.rs|cache_coherence\.rs|workload_class\.rs|sec_policy\.rs|client_auth\.rs)" | grep "warning:" | wc -l0


[0m[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "filesystem\.rs|cache_coherence\.rs|workload_class\.rs|sec_policy\.rs|client_auth\.rs"
[0m[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-fuse --lib 2>&1 | tail -20

[0m

test xattr::tests::test_clear_inode_removes_all ... ok
test xattr::tests::test_get_non_existent_name_returns_none ... ok
test writebuf::tests::zero_length_write_is_no_op ... ok
test xattr::tests::test_list_returns_sorted_names ... ok
test dir_cache::tests::test_negative_expires ... ok
test xattr::tests::test_list_size_counts_null_terminators ... ok
test xattr::tests::test_multiple_inodes_are_isolated ... ok
test xattr::tests::test_overwrite_existing_xattr_value ... ok
test xattr::tests::test_remove_existing_xattr ... ok
test xattr::tests::test_remove_non_existent_returns_error ... ok
test xattr::tests::test_set_and_get_basic_xattr ... ok
test xattr::tests::test_set_empty_name_returns_error ... ok
test xattr::tests::test_set_name_too_long_returns_error ... ok
test dir_cache::tests::test_ttl_expiry ... ok
test xattr::tests::test_set_value_too_long_returns_error ... ok
test reconnect::tests::retry_with_backoff_retries_on_failure ... ok
test idmap::tests::test_max_entries_limit ... ok

test result: ok. 918 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "error|warning: unused" | wc -l
47
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "filesystem\.rs|cache_coherence\.rs|workload_class\.rs|sec_policy\.rs|client_auth\.rs" | wc -l0
[0m


[0m[0m$ [0mcd /home/cfs/claudefs && cargo clippy -p claudefs-fuse 2>&1 | grep -E "^warning:.*missing documentation" | grep -E "filesystem|cache_coherence|workload_class|sec_policy|client_auth" | wc -l

0Documentation complete. All 5 target files now have comprehensive `///` doc comments:

1. **filesystem.rs** - Documented `ClaudeFsConfig` struct with all fields, `ClaudeFsFilesystem` struct with public methods, and `inode_to_fuser_attr` function
2. **cache_coherence.rs** - Documented `CoherenceError`, `LeaseId`, `LeaseState`, `CacheLease`, `InvalidationReason`, `CacheInvalidation`, `VersionVector`, `CoherenceProtocol`, and `CoherenceManager` with all their public members
3. **workload_class.rs** - Documented `WorkloadType`, `AccessProfile`, `TuningHint`, `WorkloadSignature`, `ClassificationResult`, `WorkloadClassifier`, and `AdaptiveTuner`
4. **sec_policy.rs** - Documented `Capability`, `CapabilitySet`, `SeccompMode`, `SyscallPolicy`, `MountNamespace`, `SecurityProfile`, `ViolationType`, `PolicyViolation`, and `PolicyEnforcer`
5. **client_auth.rs** - Documented `AuthState`, `CertRecord`, `RevokedCert`, `ClientAuthManager`, and `AuthError` with all their variants

**Verification:**
- `cargo clippy -p claudefs-fuse` → 0 warnings in the 5 target files
- `cargo test -p claudefs-fuse --lib` → 918 tests passing
