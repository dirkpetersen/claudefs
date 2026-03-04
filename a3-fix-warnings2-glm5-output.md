[0m


[0m> build · accounts/fireworks/models/glm-5[0m→ [0mRead crates/claudefs-reduce/src/async_meta_bridge.rs
[0m→ [0mRead crates/claudefs-reduce/src/segment.rs
[0m→ [0mRead crates/claudefs-reduce/src/write_path.rs
[0m→ [0mRead crates/claudefs-reduce/src/worm_reducer.rs
I'll read all 4 files first, then apply the minimal fixes.
Now I'll apply all the minimal fixes to each file:
[0m[0m← [0mEdit crates/claudefs-reduce/src/async_meta_bridge.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
@@ -458,9 +458,9 @@
     write_path.process_write(&test_data(100)).await.unwrap();
 
     let segments = write_path.flush_segments();
 
-    assert!(segments.len() >= 1);
+    assert!(!segments.is_empty());
 }
 
 #[tokio::test]
 async fn test_async_distributed_dedup() {



[0m[0m← [0mEdit crates/claudefs-reduce/src/async_meta_bridge.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
@@ -491,9 +491,9 @@
 
     let data = test_data(5000);
     let result = write_path.process_write(&data).await.unwrap();
 
-    assert!(result.reduced_chunks.len() > 0);
+    assert!(!result.reduced_chunks.is_empty());
 }
 
 #[tokio::test]
 async fn test_async_large_data() {


[0m[0m← [0mEdit crates/claudefs-reduce/src/async_meta_bridge.rs
[0m
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs
@@ -503,9 +503,9 @@
 
     let data = test_data(1_000_000);
     let result = write_path.process_write(&data).await.unwrap();
 
-    assert!(result.reduced_chunks.len() >= 1);
+    assert!(!result.reduced_chunks.is_empty());
     assert!(result.stats.pipeline.input_bytes == 1_000_000);
 }
 
 #[tokio::test]

[0m← [0mEdit crates/claudefs-reduce/src/segment.rsIndex: /home/cfs/claudefs/crates/claudefs-reduce/src/segment.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/segment.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/segment.rs
@@ -350,9 +350,9 @@
     .map(|i| {
         let (_, payload) = make_chunk(100);
         packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
     })
-    .filter_map(|s| s)
+    .flatten()
     .collect();
 
 assert!(!sealed_segments.is_empty());
 for segment in &sealed_segments {



[0m[0m← [0mEdit crates/claudefs-reduce/src/write_path.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
@@ -231,9 +231,9 @@
     // Flush should return any pending segments
     let segments = write_path.flush_segments();
 
     // At least one segment should be flushed
-    assert!(segments.len() >= 1);
+    assert!(!segments.is_empty());
 }
 
 #[test]
 fn test_distributed_dedup() {


[0m[0m← [0mEdit crates/claudefs-reduce/src/write_path.rsIndex: /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
@@ -268,9 +268,9 @@
     let data = test_data(5000);
     let result = write_path.process_write(&data).unwrap();
 
     // Should work without distributed dedup
-    assert!(result.reduced_chunks.len() > 0);
+    assert!(!result.reduced_chunks.is_empty());
 }
 
 #[test]
 fn test_small_data() {



[0m[0m← [0mEdit crates/claudefs-reduce/src/write_path.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/write_path.rs
@@ -295,8 +295,8 @@
         // Larger data
         let data = test_data(1_000_000);
         let result = write_path.process_write(&data).unwrap();
 
-        assert!(result.reduced_chunks.len() >= 1);
+        assert!(!result.reduced_chunks.is_empty());
         assert!(result.stats.pipeline.input_bytes == 1_000_000);
     }
 }

[0m
[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs
[0m

Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -489,9 +489,9 @@
 
 #[test]
 fn test_policy_clone() {
     let policy = RetentionPolicy::legal_hold();
-    let cloned = policy.clone();
+    let cloned = policy;
     assert!(!cloned.is_expired(0));
 }
 
 #[test]
[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -550,11 +550,11 @@
             RetentionPolicy::none()
         } else if i % 3 == 1 {
             RetentionPolicy::legal_hold()
         } else {
-            RetentionPolicy::immutable_until(i as u64)
+            RetentionPolicy::immutable_until(i)
         };
-        reducer.register(i, policy, i as u64);
+        let _ = reducer.register(i, policy, i);
     }
 
     assert_eq!(reducer.active_count(501), 499);
 }


[0m[0m✗ [0medit failed
[91m[1mError: [0mFound multiple matches for oldString. Provide more surrounding context to make the match unique.

[0mIndex: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -288,11 +288,11 @@
 #[test]
 fn test_gc_removes_all_expired() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::immutable_until(100), 0);
-    reducer.register(2, RetentionPolicy::immutable_until(200), 0);
-    reducer.register(3, RetentionPolicy::immutable_until(300), 0);
+    let _ = reducer.register(1, RetentionPolicy::immutable_until(100), 0);
+    let _ = reducer.register(2, RetentionPolicy::immutable_until(200), 0);
+    let _ = reducer.register(3, RetentionPolicy::immutable_until(300), 0);
 
     let removed = reducer.gc_expired(250);
     assert_eq!(removed, 2);
     assert_eq!(reducer.total_count(), 1);



[0m[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rsIndex: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -302,10 +302,10 @@
 #[test]
 fn test_none_mode_not_counted_as_active() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::none(), 0);
-    reducer.register(2, RetentionPolicy::immutable_until(1000), 0);
+    let _ = reducer.register(1, RetentionPolicy::none(), 0);
+    let _ = reducer.register(2, RetentionPolicy::immutable_until(1000), 0);
 
     assert_eq!(reducer.active_count(500), 1);
 }
 
[0m


Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -332,12 +332,12 @@
 #[test]
 fn test_mixed_policies() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::none(), 0);
-    reducer.register(2, RetentionPolicy::legal_hold(), 0);
-    reducer.register(3, RetentionPolicy::immutable_until(500), 0);
-    reducer.register(4, RetentionPolicy::immutable_until(1000), 0);
+    let _ = reducer.register(1, RetentionPolicy::none(), 0);
+    let _ = reducer.register(2, RetentionPolicy::legal_hold(), 0);
+    let _ = reducer.register(3, RetentionPolicy::immutable_until(500), 0);
+    let _ = reducer.register(4, RetentionPolicy::immutable_until(1000), 0);
 
     assert_eq!(reducer.active_count(600), 2);
     assert_eq!(reducer.active_count(1100), 1);
 }



[0m[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs[0m

Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -355,10 +355,10 @@
 #[test]
 fn test_gc_legal_hold_preserved() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::legal_hold(), 0);
-    reducer.register(2, RetentionPolicy::immutable_until(100), 0);
+    let _ = reducer.register(1, RetentionPolicy::legal_hold(), 0);
+    let _ = reducer.register(2, RetentionPolicy::immutable_until(100), 0);
 
     let removed = reducer.gc_expired(200);
     assert_eq!(removed, 1);
     assert_eq!(reducer.total_count(), 1);

[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -368,12 +368,12 @@
 fn test_concurrent_gc() {
     let mut reducer = WormReducer::new();
 
     for i in 1..=10 {
-        reducer.register(i, RetentionPolicy::immutable_until(i * 100), 0);
+        let _ = reducer.register(i, RetentionPolicy::immutable_until(i * 100), 0);
     }
     // Add one more block that won't expire even at 1001
-    reducer.register(11, RetentionPolicy::immutable_until(2000), 0);
+    let _ = reducer.register(11, RetentionPolicy::immutable_until(2000), 0);
 
     let removed1 = reducer.gc_expired(500);
     assert_eq!(removed1, 4);
 


[0m[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -400,10 +400,10 @@
 #[test]
 fn test_very_large_gc_timestamp() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::immutable_until(1000), 0);
-    reducer.register(2, RetentionPolicy::legal_hold(), 0);
+    let _ = reducer.register(1, RetentionPolicy::immutable_until(1000), 0);
+    let _ = reducer.register(2, RetentionPolicy::legal_hold(), 0);
 
     let removed = reducer.gc_expired(u64::MAX);
     assert_eq!(removed, 1);
 }


[0m[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs
[0mIndex: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -411,10 +411,10 @@
 #[test]
 fn test_register_overwrites() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::none(), 100);
-    reducer.register(1, RetentionPolicy::legal_hold(), 200);
+    let _ = reducer.register(1, RetentionPolicy::none(), 100);
+    let _ = reducer.register(1, RetentionPolicy::legal_hold(), 200);
 
     let (policy, size) = reducer.get(&1).unwrap();
     assert!(matches!(policy.mode, WormMode::LegalHold));
     assert_eq!(*size, 200);


[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs[0m

Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -423,11 +423,11 @@
 #[test]
 fn test_different_hash_sizes() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::legal_hold(), 100);
-    reducer.register(1000000, RetentionPolicy::legal_hold(), 200);
-    reducer.register(u64::MAX, RetentionPolicy::legal_hold(), 300);
+    let _ = reducer.register(1, RetentionPolicy::legal_hold(), 100);
+    let _ = reducer.register(1000000, RetentionPolicy::legal_hold(), 200);
+    let _ = reducer.register(u64::MAX, RetentionPolicy::legal_hold(), 300);
 
     assert_eq!(reducer.active_count(0), 3);
 }
 

[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -452,9 +452,9 @@
 let mut reducer = WormReducer::new();
 
 for i in 1..=10 {
     let ts = i * 100;
-    reducer.register(i, RetentionPolicy::immutable_until(ts), 0);
+    let _ = reducer.register(i, RetentionPolicy::immutable_until(ts), 0);
 }
 
 for check_ts in [0, 100, 250, 500, 750, 1000, 1500] {
     let active = reducer.active_count(check_ts);
[0m

Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -466,10 +466,10 @@
 #[test]
 fn test_gc_idempotent() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::immutable_until(100), 0);
-    reducer.register(2, RetentionPolicy::immutable_until(200), 0);
+    let _ = reducer.register(1, RetentionPolicy::immutable_until(100), 0);
+    let _ = reducer.register(2, RetentionPolicy::immutable_until(200), 0);
 
     let first = reducer.gc_expired(150);
     assert_eq!(first, 1);
 


[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs[0m

[0m
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -521,10 +521,10 @@
 
 #[test]
 fn test_gc_with_only_none_mode() {
     let mut reducer = WormReducer::new();
-    reducer.register(1, RetentionPolicy::none(), 0);
-    reducer.register(2, RetentionPolicy::none(), 0);
+    let _ = reducer.register(1, RetentionPolicy::none(), 0);
+    let _ = reducer.register(2, RetentionPolicy::none(), 0);
 
     let removed = reducer.gc_expired(1000);
     assert_eq!(removed, 2);
     assert_eq!(reducer.total_count(), 0);

[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rsNow I'll add `let _ =` to all the remaining bare `reducer.register()` calls in worm_reducer.rs:
[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -532,10 +532,10 @@
 
 #[test]
 fn test_gc_with_only_legal_hold() {
     let mut reducer = WormReducer::new();
-    reducer.register(1, RetentionPolicy::legal_hold(), 0);
-    reducer.register(2, RetentionPolicy::legal_hold(), 0);
+    let _ = reducer.register(1, RetentionPolicy::legal_hold(), 0);
+    let _ = reducer.register(2, RetentionPolicy::legal_hold(), 0);
 
     let removed = reducer.gc_expired(u64::MAX);
     assert_eq!(removed, 0);
     assert_eq!(reducer.total_count(), 2);

[0m
[0m→ [0mRead crates/claudefs-reduce/src/worm_reducer.rs[90m [offset=278, limit=15][0m


[0m← [0mEdit crates/claudefs-reduce/src/worm_reducer.rs[0m

Index: /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs
@@ -278,10 +278,10 @@
 #[test]
 fn test_legal_hold_never_expires() {
     let mut reducer = WormReducer::new();
 
-    reducer.register(1, RetentionPolicy::legal_hold(), 0);
-    reducer.register(2, RetentionPolicy::immutable_until(100), 0);
+    let _ = reducer.register(1, RetentionPolicy::legal_hold(), 0);
+    let _ = reducer.register(2, RetentionPolicy::immutable_until(100), 0);
 
     assert_eq!(reducer.active_count(200), 1);
 }
 
[0mNow let me verify the changes by running clippy:
[0m$ [0mcd /home/cfs/claudefs && cargo clippy --package claudefs-reduce --tests 2>&1

Checking claudefs-reduce v0.1.0 (/home/cfs/claudefs/crates/claudefs-reduce)
warning: called `map(..).flatten()` on `Iterator`
   --> crates/claudefs-reduce/src/segment.rs:350:14
    |
350 |               .map(|i| {
    |  ______________^
351 | |                 let (_, payload) = make_chunk(100);
352 | |                 packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
353 | |             })
354 | |             .flatten()
    | |______________________^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.93.0/index.html#map_flatten
    = note: `#[warn(clippy::map_flatten)]` on by default
help: try replacing `map` with `filter_map` and remove the `.flatten()`
    |
350 ~             .filter_map(|i| {
351 +                 let (_, payload) = make_chunk(100);
352 +                 packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
353 +             })
    |

warning: `claudefs-reduce` (lib test) generated 1 warning (run `cargo clippy --fix --lib -p claudefs-reduce --tests` to apply 1 suggestion)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.51s[0m
[0m← [0mEdit crates/claudefs-reduce/src/segment.rsIndex: /home/cfs/claudefs/crates/claudefs-reduce/src/segment.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/segment.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/segment.rs
@@ -346,13 +346,12 @@
 fn test_full_segment_has_checksum() {
     let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 1024 });
 
     let sealed_segments: Vec<_> = (0..100)
-        .map(|i| {
+        .filter_map(|i| {
             let (_, payload) = make_chunk(100);
             packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
         })
-        .flatten()
         .collect();
 
     assert!(!sealed_segments.is_empty());
     for segment in &sealed_segments {


[0m
[0m$ [0mcd /home/cfs/claudefs && cargo clippy --package claudefs-reduce --tests 2>&1
Checking claudefs-reduce v0.1.0 (/home/cfs/claudefs/crates/claudefs-reduce)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.90s

[0m[0m→ [0mRead crates/claudefs-reduce/src/async_meta_bridge.rs
[0m→ [0mRead crates/claudefs-reduce/src/segment.rs
[0m→ [0mRead crates/claudefs-reduce/src/write_path.rs
[0m→ [0mRead crates/claudefs-reduce/src/worm_reducer.rs
All 4 files have been fixed. Clippy now passes with 0 warnings.

```
=== FILE: crates/claudefs-reduce/src/async_meta_bridge.rs ===
//! Async fingerprint store bridge for Tokio-based distributed metadata integration.

use crate::encryption::EncryptionKey;
use crate::error::ReduceError;
use crate::meta_bridge::{BlockLocation, FingerprintStore};
use crate::pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats};
use crate::segment::{Segment, SegmentPacker, SegmentPackerConfig};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Async version of FingerprintStore for Tokio-based distributed metadata integration.
/// Implementors can delegate to A2's distributed fingerprint index.
#[async_trait]
pub trait AsyncFingerprintStore: Send + Sync {
    /// Lookup a fingerprint, returning its block location if found.
    async fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation>;

    /// Insert a new fingerprint-location pair.
    /// Returns true if this was a new entry, false if it already existed.
    async fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool;

    /// Increment reference count for an existing entry.
    /// Returns true if the entry existed and was incremented.
    async fn increment_ref(&self, hash: &[u8; 32]) -> bool;

    /// Decrement reference count for an entry.
    /// Returns the new refcount, or None if entry not found.
    async fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64>;

    /// Total number of entries in the store.
    async fn entry_count(&self) -> usize;
}

/// In-memory async fingerprint store using RwLock for thread-safe async access.
pub struct AsyncLocalFingerprintStore {
    entries: RwLock<HashMap<[u8; 32], (BlockLocation, u64)>>,
}

impl AsyncLocalFingerprintStore {
    /// Create a new empty async local fingerprint store.
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Calculate total bytes stored via deduplicated chunks.
    pub async fn total_deduplicated_bytes(&self) -> u64 {
        let entries = self.entries.read().await;
        entries
            .values()
            .filter(|(_, count)| *count > 1)
            .map(|(loc, count)| loc.size * (count - 1))
            .sum()
    }
}

impl Default for AsyncLocalFingerprintStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncFingerprintStore for AsyncLocalFingerprintStore {
    async fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation> {
        let entries = self.entries.read().await;
        entries.get(hash).map(|(loc, _)| *loc)
    }

    async fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool {
        let mut entries = self.entries.write().await;
        match entries.entry(hash) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().1 += 1;
                false
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((location, 1));
                debug!(
                    node_id = location.node_id,
                    offset = location.block_offset,
                    "Inserted new fingerprint"
                );
                true
            }
        }
    }

    async fn increment_ref(&self, hash: &[u8; 32]) -> bool {
        let mut entries = self.entries.write().await;
        if let Some((_, refs)) = entries.get_mut(hash) {
            *refs += 1;
            debug!(hash = ?hash, refs = *refs, "Incremented refcount");
            true
        } else {
            false
        }
    }

    async fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64> {
        let mut entries = self.entries.write().await;
        if let Some((_, refs)) = entries.get_mut(hash) {
            if *refs > 0 {
                *refs -= 1;
                debug!(hash = ?hash, refs = *refs, "Decremented refcount");
                Some(*refs)
            } else {
                None
            }
        } else {
            None
        }
    }

    async fn entry_count(&self) -> usize {
        self.entries.read().await.len()
    }
}

impl FingerprintStore for AsyncLocalFingerprintStore {
    fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation> {
        // Use blocking read for sync compatibility
        let entries = self.entries.blocking_read();
        entries.get(hash).map(|(loc, _)| *loc)
    }

    fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool {
        let mut entries = self.entries.blocking_write();
        match entries.entry(hash) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().1 += 1;
                false
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((location, 1));
                debug!(
                    node_id = location.node_id,
                    offset = location.block_offset,
                    "Inserted new fingerprint"
                );
                true
            }
        }
    }

    fn increment_ref(&self, hash: &[u8; 32]) -> bool {
        let mut entries = self.entries.blocking_write();
        if let Some((_, refs)) = entries.get_mut(hash) {
            *refs += 1;
            debug!(hash = ?hash, refs = *refs, "Incremented refcount");
            true
        } else {
            false
        }
    }

    fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64> {
        let mut entries = self.entries.blocking_write();
        if let Some((_, refs)) = entries.get_mut(hash) {
            if *refs > 0 {
                *refs -= 1;
                debug!(hash = ?hash, refs = *refs, "Decremented refcount");
                Some(*refs)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn entry_count(&self) -> usize {
        self.entries.blocking_read().len()
    }
}

/// No-op async fingerprint store for testing or when distributed dedup is disabled.
pub struct AsyncNullFingerprintStore;

impl AsyncNullFingerprintStore {
    /// Create a new null async fingerprint store.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsyncNullFingerprintStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncFingerprintStore for AsyncNullFingerprintStore {
    async fn lookup(&self, _hash: &[u8; 32]) -> Option<BlockLocation> {
        None
    }

    async fn insert(&self, _hash: [u8; 32], _location: BlockLocation) -> bool {
        true
    }

    async fn increment_ref(&self, _hash: &[u8; 32]) -> bool {
        false
    }

    async fn decrement_ref(&self, _hash: &[u8; 32]) -> Option<u64> {
        None
    }

    async fn entry_count(&self) -> usize {
        0
    }
}

impl FingerprintStore for AsyncNullFingerprintStore {
    fn lookup(&self, _hash: &[u8; 32]) -> Option<BlockLocation> {
        None
    }

    fn insert(&self, _hash: [u8; 32], _location: BlockLocation) -> bool {
        true
    }

    fn increment_ref(&self, _hash: &[u8; 32]) -> bool {
        false
    }

    fn decrement_ref(&self, _hash: &[u8; 32]) -> Option<u64> {
        None
    }

    fn entry_count(&self) -> usize {
        0
    }
}

/// Configuration for the async integrated write path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WritePathConfig {
    /// Reduction pipeline configuration
    pub pipeline: PipelineConfig,
    /// Segment packer configuration
    pub segment: SegmentPackerConfig,
}

/// Statistics from the async integrated write path.
#[derive(Debug, Default, Clone)]
pub struct WritePathStats {
    /// Pipeline statistics
    pub pipeline: ReductionStats,
    /// Number of sealed segments produced
    pub segments_produced: usize,
    /// Hits from distributed deduplication (chunks found in fingerprint store)
    pub distributed_dedup_hits: usize,
}

impl WritePathStats {
    /// Total input bytes processed
    pub fn total_input_bytes(&self) -> u64 {
        self.pipeline.input_bytes
    }

    /// Total bytes stored in segments
    pub fn total_bytes_stored(&self) -> u64 {
        self.pipeline.bytes_after_encryption
    }

    /// Overall reduction ratio (input / stored)
    pub fn overall_reduction_ratio(&self) -> f64 {
        if self.total_bytes_stored() > 0 {
            self.total_input_bytes() as f64 / self.total_bytes_stored() as f64
        } else {
            1.0
        }
    }
}

/// Result from processing a write through the async integrated path.
#[derive(Debug)]
pub struct WritePathResult {
    /// Reduced chunks (for CAS and application use)
    pub reduced_chunks: Vec<ReducedChunk>,
    /// Sealed segments ready for EC/storage
    pub sealed_segments: Vec<Segment>,
    /// Statistics from the operation
    pub stats: WritePathStats,
}

/// Async integrated write path combining pipeline + distributed fingerprint + segment packing.
pub struct AsyncIntegratedWritePath<F: AsyncFingerprintStore> {
    pipeline: ReductionPipeline,
    packer: SegmentPacker,
    fingerprint_store: Arc<F>,
    stats: WritePathStats,
}

impl<F: AsyncFingerprintStore> AsyncIntegratedWritePath<F> {
    /// Create a new async integrated write path without encryption.
    pub fn new(config: WritePathConfig, fingerprint_store: Arc<F>) -> Self {
        Self {
            pipeline: ReductionPipeline::new(config.pipeline),
            packer: SegmentPacker::new(config.segment),
            fingerprint_store,
            stats: WritePathStats::default(),
        }
    }

    /// Create a new async integrated write path with encryption enabled.
    pub fn new_with_key(
        config: WritePathConfig,
        master_key: EncryptionKey,
        fingerprint_store: Arc<F>,
    ) -> Self {
        Self {
            pipeline: ReductionPipeline::with_master_key(config.pipeline, master_key),
            packer: SegmentPacker::new(config.segment),
            fingerprint_store,
            stats: WritePathStats::default(),
        }
    }

    /// Process a write through the full async integrated path:
    /// 1. Run through reduction pipeline (chunk → dedup → compress → encrypt)
    /// 2. Await distributed fingerprint store lookup for each chunk
    /// 3. If found in distributed store, increment ref (distributed dedup hit)
    /// 4. If not found and not duplicate, pack into segment + insert into store
    /// 5. Return WritePathResult
    pub async fn process_write(&mut self, data: &[u8]) -> Result<WritePathResult, ReduceError> {
        // (a) Run through reduction pipeline
        let (chunks, pipeline_stats) = self.pipeline.process_write(data)?;

        // Update stats
        self.stats.pipeline = pipeline_stats;
        let mut sealed_segments = Vec::new();

        // (b) Check distributed fingerprint store and (c) pack new chunks
        for chunk in &chunks {
            // Check if chunk exists in distributed fingerprint store
            if let Some(location) = self.fingerprint_store.lookup(chunk.hash.as_bytes()).await {
                // Distributed dedup hit
                self.stats.distributed_dedup_hits += 1;
                debug!(
                    hash = %chunk.hash.to_hex(),
                    node = location.node_id,
                    "Distributed dedup hit"
                );

                // Increment ref in fingerprint store
                self.fingerprint_store.increment_ref(chunk.hash.as_bytes()).await;
            } else if !chunk.is_duplicate {
                // New chunk - pack into segment
                if let Some(payload) = &chunk.payload {
                    let location = BlockLocation {
                        node_id: 0, // Will be set by actual storage layer
                        block_offset: 0,
                        size: payload.ciphertext.len() as u64,
                    };

                    // Add to segment packer
                    if let Some(segment) = self.packer.add_chunk(
                        chunk.hash,
                        &payload.ciphertext,
                        chunk.original_size as u32,
                    ) {
                        sealed_segments.push(segment);
                        self.stats.segments_produced += 1;
                    }

                    // Insert to distributed fingerprint store
                    self.fingerprint_store
                        .insert(chunk.hash.0, location)
                        .await;
                }
            }
        }

        // (d) Return result
        Ok(WritePathResult {
            reduced_chunks: chunks,
            sealed_segments,
            stats: WritePathStats {
                pipeline: self.stats.pipeline.clone(),
                segments_produced: self.stats.segments_produced,
                distributed_dedup_hits: self.stats.distributed_dedup_hits,
            },
        })
    }

    /// Flush any pending segments.
    /// Returns sealed segments even if not full.
    pub fn flush_segments(&mut self) -> Vec<Segment> {
        let mut segments = Vec::new();
        if let Some(segment) = self.packer.flush() {
            segments.push(segment);
            self.stats.segments_produced += 1;
        }
        segments
    }

    /// Get a snapshot of current statistics.
    pub fn stats_snapshot(&self) -> WritePathStats {
        WritePathStats::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    #[tokio::test]
    async fn test_async_basic_write() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncNullFingerprintStore::new());
        let mut write_path = AsyncIntegratedWritePath::new(config, store);

        let data = test_data(10000);
        let result = write_path.process_write(&data).await.unwrap();

        assert!(!result.reduced_chunks.is_empty());
        assert!(result.stats.pipeline.input_bytes > 0);
    }

    #[tokio::test]
    async fn test_async_encryption_write() {
        let mut config = WritePathConfig::default();
        config.pipeline.encryption_enabled = true;

        let store = Arc::new(AsyncNullFingerprintStore::new());
        let key = EncryptionKey([0x42u8; 32]);
        let mut write_path = AsyncIntegratedWritePath::new_with_key(config, key, store);

        let data = b"secret data for encryption test".to_vec();
        let result = write_path.process_write(&data).await.unwrap();

        assert!(result.reduced_chunks.iter().any(|c| c.payload.is_some()));
    }

    #[tokio::test]
    async fn test_async_flush_segments() {
        let config = WritePathConfig {
            segment: SegmentPackerConfig { target_size: 1000 },
            ..Default::default()
        };

        let store = Arc::new(AsyncNullFingerprintStore::new());
        let mut write_path = AsyncIntegratedWritePath::new(config, store);

        write_path.process_write(&test_data(100)).await.unwrap();

        let segments = write_path.flush_segments();

        assert!(!segments.is_empty());
    }

    #[tokio::test]
    async fn test_async_distributed_dedup() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncLocalFingerprintStore::new());

        let mut write_path = AsyncIntegratedWritePath::new(config, store.clone());

        let data = test_data(100_000);
        let _result1 = write_path.process_write(&data).await.unwrap();

        let config2 = WritePathConfig::default();
        let mut write_path2 = AsyncIntegratedWritePath::new(config2, store);

        let result2 = write_path2.process_write(&data).await.unwrap();

        assert!(
            result2.stats.distributed_dedup_hits > 0,
            "Expected distributed dedup hits"
        );
    }

    #[tokio::test]
    async fn test_async_null_store() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncNullFingerprintStore::new());
        let mut write_path = AsyncIntegratedWritePath::new(config, store);

        let data = test_data(5000);
        let result = write_path.process_write(&data).await.unwrap();

        assert!(!result.reduced_chunks.is_empty());
    }

    #[tokio::test]
    async fn test_async_large_data() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncNullFingerprintStore::new());
        let mut write_path = AsyncIntegratedWritePath::new(config, store);

        let data = test_data(1_000_000);
        let result = write_path.process_write(&data).await.unwrap();

        assert!(!result.reduced_chunks.is_empty());
        assert!(result.stats.pipeline.input_bytes == 1_000_000);
    }

    #[tokio::test]
    async fn test_async_concurrent_writes() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncLocalFingerprintStore::new());

        let store_clone = store.clone();
        let handle1 = tokio::spawn(async move {
            let mut write_path = AsyncIntegratedWritePath::new(config.clone(), store_clone);
            let data = test_data(50_000);
            write_path.process_write(&data).await.unwrap()
        });

        let config2 = WritePathConfig::default();
        let store2 = Arc::new(AsyncLocalFingerprintStore::new());
        let handle2 = tokio::spawn(async move {
            let mut write_path = AsyncIntegratedWritePath::new(config2, store2);
            let data = test_data(50_000);
            write_path.process_write(&data).await.unwrap()
        });

        let _ = handle1.await;
        let _ = handle2.await;

        // Both writes completed without panic
    }

    #[tokio::test]
    async fn test_async_local_store_total_deduplicated_bytes() {
        let store = Arc::new(AsyncLocalFingerprintStore::new());
        let loc1 = BlockLocation {
            node_id: 1,
            block_offset: 100,
            size: 4096,
        };
        let loc2 = BlockLocation {
            node_id: 1,
            block_offset: 200,
            size: 8192,
        };

        AsyncFingerprintStore::insert(&*store, [1u8; 32], loc1).await;
        AsyncFingerprintStore::insert(&*store, [1u8; 32], loc1).await; // refcount now 2

        AsyncFingerprintStore::insert(&*store, [2u8; 32], loc2).await;
        AsyncFingerprintStore::insert(&*store, [2u8; 32], loc2).await;
        AsyncFingerprintStore::insert(&*store, [2u8; 32], loc2).await; // refcount now 3

        assert_eq!(store.total_deduplicated_bytes().await, 4096 + 16384);
    }

    #[tokio::test]
    async fn test_async_local_store_ref_counts() {
        let store = Arc::new(AsyncLocalFingerprintStore::new());
        let hash = [0u8; 32];
        let location = BlockLocation {
            node_id: 1,
            block_offset: 100,
            size: 4096,
        };

        AsyncFingerprintStore::insert(&*store, hash, location).await;

        assert!(AsyncFingerprintStore::increment_ref(&*store, &hash).await);
        assert!(AsyncFingerprintStore::increment_ref(&*store, &hash).await);

        assert_eq!(AsyncFingerprintStore::decrement_ref(&*store, &hash).await, Some(2));
        assert_eq!(AsyncFingerprintStore::decrement_ref(&*store, &hash).await, Some(1));
        assert_eq!(AsyncFingerprintStore::decrement_ref(&*store, &hash).await, Some(0));
        assert_eq!(AsyncFingerprintStore::decrement_ref(&*store, &hash).await, None);
    }

    #[tokio::test]
    async fn test_async_local_store_entry_count() {
        let store = Arc::new(AsyncLocalFingerprintStore::new());
        let loc = BlockLocation {
            node_id: 1,
            block_offset: 100,
            size: 4096,
        };

        assert_eq!(AsyncFingerprintStore::entry_count(&*store).await, 0);

        AsyncFingerprintStore::insert(&*store, [1u8; 32], loc).await;
        assert_eq!(AsyncFingerprintStore::entry_count(&*store).await, 1);

        AsyncFingerprintStore::insert(&*store, [2u8; 32], loc).await;
        assert_eq!(AsyncFingerprintStore::entry_count(&*store).await, 2);

        AsyncFingerprintStore::insert(&*store, [1u8; 32], loc).await;
        assert_eq!(AsyncFingerprintStore::entry_count(&*store).await, 2);
    }
}
=== END FILE ===

=== FILE: crates/claudefs-reduce/src/segment.rs ===
//! Segment packing for erasure coding.
//! Packs reduced chunks into 2MB segments for EC (4+2 coding).

use crate::fingerprint::ChunkHash;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Default segment size: 2MB for erasure coding (4+2 configuration).
pub const DEFAULT_SEGMENT_SIZE: usize = 2 * 1024 * 1024;

/// Metadata for a single chunk within a segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentEntry {
    /// BLAKE3 hash of the original chunk (for CAS lookup).
    pub hash: ChunkHash,
    /// Byte offset within the segment's payload.
    pub offset_in_segment: u32,
    /// Size of the compressed/encrypted payload in this segment.
    pub payload_size: u32,
    /// Original uncompressed size (for stats).
    pub original_size: u32,
}

/// A 2MB segment containing packed chunk payloads for erasure coding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// Unique segment sequence number.
    pub id: u64,
    /// Chunk metadata entries.
    pub entries: Vec<SegmentEntry>,
    /// Concatenated chunk payloads.
    pub payload: Vec<u8>,
    /// True when full or explicitly sealed.
    pub sealed: bool,
    /// Seconds since UNIX_EPOCH when segment was created.
    pub created_at_secs: u64,
    /// CRC32C checksum of the payload bytes (computed when segment is sealed).
    pub payload_checksum: Option<crate::checksum::DataChecksum>,
}

impl Segment {
    /// Number of chunks in this segment.
    pub fn total_chunks(&self) -> usize {
        self.entries.len()
    }

    /// Total bytes in the payload.
    pub fn total_payload_bytes(&self) -> usize {
        self.payload.len()
    }

    /// Verify the integrity of the segment payload against the stored checksum.
    ///
    /// Returns `Ok(())` if valid, `Err(ReduceError::ChecksumMismatch)` if invalid,
    /// or `Err(ReduceError::ChecksumMissing)` if the segment has no checksum.
    pub fn verify_integrity(&self) -> Result<(), crate::error::ReduceError> {
        match &self.payload_checksum {
            Some(checksum) => crate::checksum::verify(&self.payload, checksum),
            None => Err(crate::error::ReduceError::ChecksumMissing),
        }
    }
}

/// Configuration for the segment packer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentPackerConfig {
    /// Target segment size in bytes.
    pub target_size: usize,
}

impl Default for SegmentPackerConfig {
    fn default() -> Self {
        Self {
            target_size: DEFAULT_SEGMENT_SIZE,
        }
    }
}

/// Packs reduced chunks into fixed-size segments for erasure coding.
pub struct SegmentPacker {
    config: SegmentPackerConfig,
    next_id: u64,
    current: Option<Segment>,
}

impl Default for SegmentPacker {
    fn default() -> Self {
        Self::new(SegmentPackerConfig::default())
    }
}

impl SegmentPacker {
    /// Create a new segment packer with the given configuration.
    pub fn new(config: SegmentPackerConfig) -> Self {
        Self {
            config,
            next_id: 0,
            current: None,
        }
    }

    /// Add a chunk to the current segment.
    /// Returns a sealed segment if it becomes full (>=
    pub fn add_chunk(
        &mut self,
        hash: ChunkHash,
        payload: &[u8],
        original_size: u32,
    ) -> Option<Segment> {
        // Create current segment if needed
        if self.current.is_none() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            self.current = Some(Segment {
                id: self.next_id,
                entries: Vec::new(),
                payload: Vec::new(),
                sealed: false,
                created_at_secs: now,
                payload_checksum: None,
            });
            self.next_id += 1;
        }

        let segment = self.current.as_mut().unwrap();
        let offset = segment.payload.len() as u32;
        let payload_len = payload.len() as u32;

        // Add entry
        segment.entries.push(SegmentEntry {
            hash,
            offset_in_segment: offset,
            payload_size: payload_len,
            original_size,
        });

        // Append payload
        segment.payload.extend_from_slice(payload);

        debug!(
            segment_id = segment.id,
            chunk_offset = offset,
            payload_size = payload_len,
            current_size = segment.payload.len(),
            target_size = self.config.target_size,
            "Added chunk to segment"
        );

        // Check if segment is full
        if segment.payload.len() >= self.config.target_size {
            segment.sealed = true;
            segment.payload_checksum = Some(crate::checksum::compute(
                &segment.payload,
                crate::checksum::ChecksumAlgorithm::Crc32c,
            ));
            let full_segment = self.current.take();
            debug!(
                segment_id = full_segment.as_ref().unwrap().id,
                "Segment sealed (full)"
            );
            return full_segment;
        }

        None
    }

    /// Seal and return the current segment, even if not full.
    /// After flushing, current is None.
    pub fn flush(&mut self) -> Option<Segment> {
        if let Some(ref mut segment) = self.current {
            segment.sealed = true;
            segment.payload_checksum = Some(crate::checksum::compute(
                &segment.payload,
                crate::checksum::ChecksumAlgorithm::Crc32c,
            ));
            debug!(segment_id = segment.id, "Segment flushed");
        }
        self.current.take()
    }

    /// Current size in bytes (0 if no current segment).
    pub fn current_size(&self) -> usize {
        self.current.as_ref().map(|s| s.payload.len()).unwrap_or(0)
    }

    /// True if no current segment or it has no chunks.
    pub fn is_empty(&self) -> bool {
        match &self.current {
            Some(segment) => segment.entries.is_empty(),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::blake3_hash;

    fn make_chunk(size: usize) -> (ChunkHash, Vec<u8>) {
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        let hash = blake3_hash(&data);
        (hash, data)
    }

    #[test]
    fn test_add_chunks_returns_segment_when_full() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig {
            target_size: 1024, // Small for testing
        });

        // Add chunks until we exceed target size
        let mut sealed_count = 0;
        for i in 0..100 {
            let (_, payload) = make_chunk(100);
            if let Some(segment) =
                packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
            {
                sealed_count += 1;
                assert!(segment.sealed);
                assert!(segment.payload.len() >= 1024);
            }
        }
        assert!(sealed_count > 0);
    }

    #[test]
    fn test_flush_returns_partial_segment() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        // Add just one small chunk
        let (_, payload) = make_chunk(100);
        packer.add_chunk(blake3_hash(b"test"), &payload, payload.len() as u32);

        // Flush before full
        let segment = packer.flush().expect("should return segment");
        assert!(segment.sealed);
        assert!(segment.entries.len() == 1);
    }

    #[test]
    fn test_flush_on_empty_returns_none() {
        let mut packer: SegmentPacker = SegmentPacker::default();
        let result = packer.flush();
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_entries_correct() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        let (hash1, payload1) = make_chunk(100);
        let (hash2, payload2) = make_chunk(200);

        packer.add_chunk(hash1, &payload1, payload1.len() as u32);
        packer.add_chunk(hash2, &payload2, payload2.len() as u32);

        let segment = packer.flush().unwrap();

        assert_eq!(segment.entries.len(), 2);

        let entry1 = &segment.entries[0];
        assert_eq!(entry1.hash, hash1);
        assert_eq!(entry1.offset_in_segment, 0);
        assert_eq!(entry1.payload_size, 100);
        assert_eq!(entry1.original_size, 100);

        let entry2 = &segment.entries[1];
        assert_eq!(entry2.hash, hash2);
        assert_eq!(entry2.offset_in_segment, 100);
        assert_eq!(entry2.payload_size, 200);
        assert_eq!(entry2.original_size, 200);
    }

    #[test]
    fn test_multiple_segments() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 500 });

        let mut sealed_segments = Vec::new();

        // Add chunks totaling more than 2x target size
        for i in 0..10 {
            let (_, payload) = make_chunk(150);
            if let Some(segment) =
                packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
            {
                sealed_segments.push(segment);
            }
        }

        // Flush remaining
        if let Some(segment) = packer.flush() {
            sealed_segments.push(segment);
        }

        // Should have multiple segments
        assert!(
            sealed_segments.len() >= 2,
            "expected >= 2 segments, got {}",
            sealed_segments.len()
        );

        // Verify segment IDs are sequential
        for (i, segment) in sealed_segments.iter().enumerate() {
            assert_eq!(segment.id, i as u64);
        }
    }

    #[test]
    fn test_segment_id_increments() {
        let mut packer: SegmentPacker = SegmentPacker::default();

        let (_, payload) = make_chunk(100);

        // First segment
        packer.add_chunk(blake3_hash(b"chunk1"), &payload, payload.len() as u32);
        let seg1 = packer.flush().unwrap();

        // Second segment
        packer.add_chunk(blake3_hash(b"chunk2"), &payload, payload.len() as u32);
        let seg2 = packer.flush().unwrap();

        // Third segment
        packer.add_chunk(blake3_hash(b"chunk3"), &payload, payload.len() as u32);
        let seg3 = packer.flush().unwrap();

        assert_eq!(seg1.id, 0);
        assert_eq!(seg2.id, 1);
        assert_eq!(seg3.id, 2);
    }

    #[test]
    fn test_sealed_segment_has_checksum() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        let (_, payload) = make_chunk(100);
        packer.add_chunk(blake3_hash(b"test"), &payload, payload.len() as u32);

        let segment = packer.flush().expect("should return segment");
        assert!(segment.payload_checksum.is_some());
    }

    #[test]
    fn test_full_segment_has_checksum() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 1024 });

        let sealed_segments: Vec<_> = (0..100)
            .filter_map(|i| {
                let (_, payload) = make_chunk(100);
                packer.add_chunk(blake3_hash(&[i]), &payload, payload.len() as u32)
            })
            .collect();

        assert!(!sealed_segments.is_empty());
        for segment in &sealed_segments {
            assert!(segment.payload_checksum.is_some());
        }
    }

    #[test]
    fn test_segment_verify_integrity() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        let (_, payload) = make_chunk(100);
        packer.add_chunk(blake3_hash(b"test"), &payload, payload.len() as u32);

        let segment = packer.flush().expect("should return segment");
        assert!(segment.verify_integrity().is_ok());
    }

    #[test]
    fn test_segment_verify_corruption() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        let (_, payload) = make_chunk(100);
        packer.add_chunk(blake3_hash(b"test"), &payload, payload.len() as u32);

        let mut segment = packer.flush().expect("should return segment");

        segment.payload[0] ^= 0xFF;

        let result = segment.verify_integrity();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::error::ReduceError::ChecksumMismatch
        ));
    }

    #[test]
    fn test_unsealed_no_checksum() {
        let mut packer = SegmentPacker::new(SegmentPackerConfig { target_size: 10000 });

        let (_, payload) = make_chunk(100);
        packer.add_chunk(blake3_hash(b"test"), &payload, payload.len() as u32);

        let segment = packer
            .current
            .as_ref()
            .expect("should have current segment");
        assert!(segment.payload_checksum.is_none());
        assert!(!segment.sealed);
    }

    #[test]
    fn test_verify_missing_checksum() {
        let segment = Segment {
            id: 0,
            entries: Vec::new(),
            payload: vec![1, 2, 3, 4, 5],
            sealed: false,
            created_at_secs: 0,
            payload_checksum: None,
        };

        let result = segment.verify_integrity();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::error::ReduceError::ChecksumMissing
        ));
    }
}
=== END FILE ===

=== FILE: crates/claudefs-reduce/src/write_path.rs ===
//! Integrated write path: reduction pipeline + distributed fingerprint + segment packing.

use crate::encryption::EncryptionKey;
use crate::error::ReduceError;
use crate::meta_bridge::{BlockLocation, FingerprintStore};
use crate::pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats};
use crate::segment::{Segment, SegmentPacker, SegmentPackerConfig};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

/// Configuration for the integrated write path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WritePathConfig {
    /// Reduction pipeline configuration
    pub pipeline: PipelineConfig,
    /// Segment packer configuration
    pub segment: SegmentPackerConfig,
}

/// Statistics from the integrated write path.
#[derive(Debug, Default, Clone)]
pub struct WritePathStats {
    /// Pipeline statistics
    pub pipeline: ReductionStats,
    /// Number of sealed segments produced
    pub segments_produced: usize,
    /// Hits from distributed deduplication (chunks found in fingerprint store)
    pub distributed_dedup_hits: usize,
}

impl WritePathStats {
    /// Total input bytes processed
    pub fn total_input_bytes(&self) -> u64 {
        self.pipeline.input_bytes
    }

    /// Total bytes stored in segments
    pub fn total_bytes_stored(&self) -> u64 {
        self.pipeline.bytes_after_encryption
    }

    /// Overall reduction ratio (input / stored)
    pub fn overall_reduction_ratio(&self) -> f64 {
        if self.total_bytes_stored() > 0 {
            self.total_input_bytes() as f64 / self.total_bytes_stored() as f64
        } else {
            1.0
        }
    }
}

/// Result from processing a write through the integrated path.
#[derive(Debug)]
pub struct WritePathResult {
    /// Reduced chunks (for CAS and application use)
    pub reduced_chunks: Vec<ReducedChunk>,
    /// Sealed segments ready for EC/storage
    pub sealed_segments: Vec<Segment>,
    /// Statistics from the operation
    pub stats: WritePathStats,
}

/// Integrated write path combining pipeline + distributed fingerprint + segment packing.
pub struct IntegratedWritePath<F: FingerprintStore + Send + Sync> {
    pipeline: ReductionPipeline,
    packer: SegmentPacker,
    fingerprint_store: Arc<F>,
    stats: WritePathStats,
}

impl<F: FingerprintStore + Send + Sync> IntegratedWritePath<F> {
    /// Create a new integrated write path without encryption.
    pub fn new(config: WritePathConfig, fingerprint_store: Arc<F>) -> Self {
        Self {
            pipeline: ReductionPipeline::new(config.pipeline),
            packer: SegmentPacker::new(config.segment),
            fingerprint_store,
            stats: WritePathStats::default(),
        }
    }

    /// Create a new integrated write path with encryption enabled.
    pub fn new_with_key(
        config: WritePathConfig,
        master_key: EncryptionKey,
        fingerprint_store: Arc<F>,
    ) -> Self {
        Self {
            pipeline: ReductionPipeline::with_master_key(config.pipeline, master_key),
            packer: SegmentPacker::new(config.segment),
            fingerprint_store,
            stats: WritePathStats::default(),
        }
    }

    /// Process a write through the full integrated path:
    /// 1. Run through reduction pipeline (chunk → dedup → compress → encrypt)
    /// 2. Check distributed fingerprint store for existing chunks
    /// 3. Pack new chunks into segments
    /// 4. Insert new fingerprints to the store
    pub fn process_write(&mut self, data: &[u8]) -> Result<WritePathResult, ReduceError> {
        // (a) Run through reduction pipeline
        let (chunks, pipeline_stats) = self.pipeline.process_write(data)?;

        // Update stats
        self.stats.pipeline = pipeline_stats;
        let mut sealed_segments = Vec::new();

        // (b) Check distributed fingerprint store and (c) pack new chunks
        for chunk in &chunks {
            // Check if chunk exists in distributed fingerprint store
            if let Some(location) = self.fingerprint_store.lookup(chunk.hash.as_bytes()) {
                // Distributed dedup hit
                self.stats.distributed_dedup_hits += 1;
                debug!(
                    hash = %chunk.hash.to_hex(),
                    node = location.node_id,
                    "Distributed dedup hit"
                );

                // Increment ref in fingerprint store
                self.fingerprint_store.increment_ref(chunk.hash.as_bytes());
            } else if !chunk.is_duplicate {
                // New chunk - pack into segment
                if let Some(payload) = &chunk.payload {
                    let location = BlockLocation {
                        node_id: 0, // Will be set by actual storage layer
                        block_offset: 0,
                        size: payload.ciphertext.len() as u64,
                    };

                    // Add to segment packer
                    if let Some(segment) = self.packer.add_chunk(
                        chunk.hash,
                        &payload.ciphertext,
                        chunk.original_size as u32,
                    ) {
                        sealed_segments.push(segment);
                        self.stats.segments_produced += 1;
                    }

                    // Insert to distributed fingerprint store
                    self.fingerprint_store.insert(chunk.hash.0, location);
                }
            }
        }

        // (d) Return result
        Ok(WritePathResult {
            reduced_chunks: chunks,
            sealed_segments,
            stats: WritePathStats {
                pipeline: self.stats.pipeline.clone(),
                segments_produced: self.stats.segments_produced,
                distributed_dedup_hits: self.stats.distributed_dedup_hits,
            },
        })
    }

    /// Flush any pending segments.
    /// Returns sealed segments even if not full.
    pub fn flush_segments(&mut self) -> Vec<Segment> {
        let mut segments = Vec::new();
        if let Some(segment) = self.packer.flush() {
            segments.push(segment);
            self.stats.segments_produced += 1;
        }
        segments
    }

    /// Get a snapshot of current statistics.
    /// Note: Returns default/empty stats as ReductionStats doesn't support cloning well.
    pub fn stats_snapshot(&self) -> WritePathStats {
        WritePathStats::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encryption::EncryptionKey;
    use crate::meta_bridge::{LocalFingerprintStore, NullFingerprintStore};

    fn test_data(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn test_basic_write() {
        let config = WritePathConfig::default();
        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        let data = test_data(10000);
        let result = write_path.process_write(&data).unwrap();

        assert!(!result.reduced_chunks.is_empty());
        assert!(result.stats.pipeline.input_bytes > 0);
    }

    #[test]
    fn test_encryption_write() {
        let mut config = WritePathConfig::default();
        config.pipeline.encryption_enabled = true;

        let store = Arc::new(NullFingerprintStore::new());
        let key = EncryptionKey([0x42u8; 32]);
        let mut write_path = IntegratedWritePath::new_with_key(config, key, store);

        let data = b"secret data for encryption test".to_vec();
        let result = write_path.process_write(&data).unwrap();

        // Should have encrypted chunks
        assert!(result.reduced_chunks.iter().any(|c| c.payload.is_some()));
    }

    #[test]
    fn test_flush_segments() {
        let config = WritePathConfig {
            segment: SegmentPackerConfig { target_size: 1000 },
            ..Default::default()
        };

        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        // Write small data
        write_path.process_write(&test_data(100)).unwrap();

        // Flush should return any pending segments
        let segments = write_path.flush_segments();

        // At least one segment should be flushed
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_distributed_dedup() {
        let config = WritePathConfig::default();
        let store = Arc::new(LocalFingerprintStore::new());

        let mut write_path = IntegratedWritePath::new(config, store.clone());

        // First write - adds fingerprints to store
        let data = test_data(100_000);
        let _result1 = write_path.process_write(&data).unwrap();

        // Create a new write path using the SAME store
        let config2 = WritePathConfig::default();
        let mut write_path2 = IntegratedWritePath::new(config2, store);

        // Second write with same data - should hit distributed dedup
        let result2 = write_path2.process_write(&data).unwrap();

        assert!(
            result2.stats.distributed_dedup_hits > 0,
            "Expected distributed dedup hits"
        );
    }

    #[test]
    fn test_null_fingerprint_store() {
        let config = WritePathConfig::default();
        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        let data = test_data(5000);
        let result = write_path.process_write(&data).unwrap();

        // Should work without distributed dedup
        assert!(!result.reduced_chunks.is_empty());
    }

    #[test]
    fn test_small_data() {
        let config = WritePathConfig::default();
        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        // Very small data
        let data = b"tiny";
        let result = write_path.process_write(data).unwrap();

        assert!(!result.reduced_chunks.is_empty());
        assert_eq!(result.stats.pipeline.input_bytes, 4);
    }

    #[test]
    fn test_large_data() {
        let config = WritePathConfig::default();
        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        // Larger data
        let data = test_data(1_000_000);
        let result = write_path.process_write(&data).unwrap();

        assert!(!result.reduced_chunks.is_empty());
        assert!(result.stats.pipeline.input_bytes == 1_000_000);
    }
}
=== END FILE ===

=== FILE: crates/claudefs-reduce/src/worm_reducer.rs ===
//! WORM (Write Once Read Many) compliance and retention policy enforcement.
//!
//! Provides retention policies for immutable data, legal holds, and
//! time-based expiration enforcement.

use std::collections::HashMap;

/// WORM mode defining the retention type for a data chunk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WormMode {
    /// No retention enforcement - data can be garbage collected.
    None,
    /// Immutable until a specific timestamp.
    Immutable,
    /// Legal hold - never expires until explicitly released.
    LegalHold,
}

/// Retention policy defining when a data chunk can be garbage collected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetentionPolicy {
    /// The WORM mode for this policy.
    pub mode: WormMode,
    /// Unix timestamp (seconds) after which immutable data can be released.
    /// None for legal holds and None-mode policies.
    pub retain_until: Option<u64>,
}

impl RetentionPolicy {
    /// Creates a policy with no retention enforcement.
    pub fn none() -> Self {
        Self {
            mode: WormMode::None,
            retain_until: None,
        }
    }

    /// Creates an immutable policy that retains data until the given timestamp.
    pub fn immutable_until(ts: u64) -> Self {
        Self {
            mode: WormMode::Immutable,
            retain_until: Some(ts),
        }
    }

    /// Creates a legal hold policy that retains data indefinitely.
    pub fn legal_hold() -> Self {
        Self {
            mode: WormMode::LegalHold,
            retain_until: None,
        }
    }

    /// Checks if this policy has expired at the given timestamp.
    pub fn is_expired(&self, now_ts: u64) -> bool {
        match self.mode {
            WormMode::None => true,
            WormMode::LegalHold => false,
            _ => match self.retain_until {
                Some(ts) => now_ts > ts,
                None => false,
            },
        }
    }
}

/// Tracks retention policies for data chunks and manages garbage collection.
pub struct WormReducer {
    records: HashMap<u64, (RetentionPolicy, u64)>,
}

impl WormReducer {
    /// Creates a new empty WORM reducer.
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Registers a chunk with a retention policy.
    pub fn register(
        &mut self,
        hash: u64,
        new_policy: RetentionPolicy,
        size: u64,
    ) -> Result<(), crate::error::ReduceError> {
        if let Some((existing_policy, _)) = self.records.get(&hash) {
            let existing_strength = Self::policy_strength(&existing_policy.mode);
            let new_strength = Self::policy_strength(&new_policy.mode);

            if new_strength < existing_strength {
                return Err(crate::error::ReduceError::PolicyDowngradeAttempted);
            }
        }

        self.records.insert(hash, (new_policy, size));
        Ok(())
    }

    fn policy_strength(mode: &WormMode) -> u32 {
        match mode {
            WormMode::LegalHold => 2,
            WormMode::Immutable => 1,
            WormMode::None => 0,
        }
    }

    /// Gets the retention policy and size for a chunk.
    pub fn get(&self, hash: &u64) -> Option<&(RetentionPolicy, u64)> {
        self.records.get(hash)
    }

    /// Returns the number of non-expired retention policies.
    pub fn active_count(&self, now_ts: u64) -> usize {
        self.records
            .values()
            .filter(|(policy, _)| !policy.is_expired(now_ts))
            .count()
    }

    /// Removes all expired entries and returns the count of removed entries.
    pub fn gc_expired(&mut self, now_ts: u64) -> usize {
        let expired: Vec<_> = self
            .records
            .iter()
            .filter(|(_, (policy, _))| policy.is_expired(now_ts))
            .map(|(hash, _)| *hash)
            .collect();

        for hash in &expired {
            self.records.remove(hash);
        }
        expired.len()
    }

    /// Returns the total number of registered chunks.
    pub fn total_count(&self) -> usize {
        self.records.len()
    }
}

impl Default for WormReducer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(n: u64) -> u64 {
        n
    }

    #[test]
    fn test_retention_none() {
        let policy = RetentionPolicy::none();
        assert!(policy.is_expired(0));
    }

    #[test]
    fn test_retention_immutable() {
        let policy = RetentionPolicy::immutable_until(500);
        assert!(!policy.is_expired(100));
        assert!(!policy.is_expired(500));
        assert!(policy.is_expired(501));
    }

    #[test]
    fn test_retention_legal_hold() {
        let policy = RetentionPolicy::legal_hold();
        assert!(!policy.is_expired(0));
        assert!(!policy.is_expired(u64::MAX));
    }

    #[test]
    fn test_active_count() {
        let mut reducer = WormReducer::new();

        // Hash 1 - legal hold, never expires, counts as active
        reducer
            .register(make_hash(1), RetentionPolicy::legal_hold(), 0)
            .unwrap();
        // Hash 2 - still active at 750 (retain_until > 750)
        reducer
            .register(make_hash(2), RetentionPolicy::immutable_until(1000), 0)
            .unwrap();
        // Immutable active (expires after the test assertion at 750)
        reducer
            .register(make_hash(3), RetentionPolicy::immutable_until(1000), 0)
            .unwrap();

        // 3 is still active at 750
        assert_eq!(reducer.active_count(750), 3);
    }

    #[test]
    fn test_active_records() {
        let mut reducer = WormReducer::new();

        // Hash 1 - legal hold, never expires, counts as active
        reducer
            .register(make_hash(1), RetentionPolicy::legal_hold(), 0)
            .unwrap();
        // Hash 2 - still active at 750 (retain_until > 750)
        reducer
            .register(make_hash(2), RetentionPolicy::immutable_until(1000), 0)
            .unwrap();
        // 3 (still active at 750)
        reducer
            .register(make_hash(3), RetentionPolicy::immutable_until(1000), 0)
            .unwrap();

        assert_eq!(reducer.active_count(750), 3);
    }

    #[test]
    fn test_gc_expired() {
        let mut reducer = WormReducer::new();

        // No None-mode registration — testing time-based expiry only
        reducer
            .register(make_hash(2), RetentionPolicy::immutable_until(500), 0)
            .unwrap();
        reducer
            .register(make_hash(3), RetentionPolicy::immutable_until(1000), 0)
            .unwrap();
        let _ = reducer.register(make_hash(4), RetentionPolicy::legal_hold(), 0);

        let removed = reducer.gc_expired(600);
        assert_eq!(removed, 1);
        assert_eq!(reducer.total_count(), 2);

        // Verify hash 2 is removed
        assert!(reducer.get(&make_hash(2)).is_none());
        // Verify others remain
        assert!(reducer.get(&make_hash(3)).is_some());
        assert!(reducer.get(&make_hash(4)).is_some());
    }

    #[test]
    fn test_register_and_get() {
        let mut reducer = WormReducer::new();
        let _ = reducer.register(100, RetentionPolicy::legal_hold(), 1024);

        let result = reducer.get(&100);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, 1024);
    }

    #[test]
    fn test_total_count() {
        let mut reducer = WormReducer::new();
        assert_eq!(reducer.total_count(), 0);

        let _ = reducer.register(1, RetentionPolicy::none(), 0);
        let _ = reducer.register(2, RetentionPolicy::legal_hold(), 0);
        assert_eq!(reducer.total_count(), 2);
    }

    #[test]
    fn test_multiple_immutable_blocks() {
        let mut reducer = WormReducer::new();

        for i in 1..=5 {
            let _ = reducer.register(
                make_hash(i),
                RetentionPolicy::immutable_until(1000),
                i * 512,
            );
        }

        assert_eq!(reducer.active_count(500), 5);
        assert_eq!(reducer.active_count(1001), 0);
    }

    #[test]
    fn test_legal_hold_never_expires() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::legal_hold(), 0);
        let _ = reducer.register(2, RetentionPolicy::immutable_until(100), 0);

        assert_eq!(reducer.active_count(200), 1);
    }

    #[test]
    fn test_gc_removes_all_expired() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::immutable_until(100), 0);
        let _ = reducer.register(2, RetentionPolicy::immutable_until(200), 0);
        let _ = reducer.register(3, RetentionPolicy::immutable_until(300), 0);

        let removed = reducer.gc_expired(250);
        assert_eq!(removed, 2);
        assert_eq!(reducer.total_count(), 1);
        assert!(reducer.get(&3).is_some());
    }

    #[test]
    fn test_none_mode_not_counted_as_active() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::none(), 0);
        let _ = reducer.register(2, RetentionPolicy::immutable_until(1000), 0);

        assert_eq!(reducer.active_count(500), 1);
    }

    #[test]
    fn test_expired_at_exact_timestamp() {
        let policy = RetentionPolicy::immutable_until(500);
        assert!(!policy.is_expired(500));
        assert!(policy.is_expired(501));
    }

    #[test]
    fn test_gc_empty() {
        let mut reducer = WormReducer::new();
        let removed = reducer.gc_expired(1000);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_active_count_empty() {
        let reducer = WormReducer::new();
        assert_eq!(reducer.active_count(1000), 0);
    }

    #[test]
    fn test_mixed_policies() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::none(), 0);
        let _ = reducer.register(2, RetentionPolicy::legal_hold(), 0);
        let _ = reducer.register(3, RetentionPolicy::immutable_until(500), 0);
        let _ = reducer.register(4, RetentionPolicy::immutable_until(1000), 0);

        assert_eq!(reducer.active_count(600), 2);
        assert_eq!(reducer.active_count(1100), 1);
    }

    #[test]
    fn test_retain_until_none_immutable() {
        let policy = RetentionPolicy {
            mode: WormMode::Immutable,
            retain_until: None,
        };
        assert!(!policy.is_expired(0));
        assert!(!policy.is_expired(u64::MAX));
    }

    #[test]
    fn test_gc_legal_hold_preserved() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::legal_hold(), 0);
        let _ = reducer.register(2, RetentionPolicy::immutable_until(100), 0);

        let removed = reducer.gc_expired(200);
        assert_eq!(removed, 1);
        assert_eq!(reducer.total_count(), 1);
    }

    #[test]
    fn test_concurrent_gc() {
        let mut reducer = WormReducer::new();

        for i in 1..=10 {
            let _ = reducer.register(i, RetentionPolicy::immutable_until(i * 100), 0);
        }
        // Add one more block that won't expire even at 1001
        let _ = reducer.register(11, RetentionPolicy::immutable_until(2000), 0);

        let removed1 = reducer.gc_expired(500);
        assert_eq!(removed1, 4);

        let removed2 = reducer.gc_expired(1001);
        assert_eq!(removed2, 6);

        assert_eq!(reducer.total_count(), 1);
    }

    #[test]
    fn test_zero_timestamp() {
        let policy = RetentionPolicy::immutable_until(0);
        assert!(!policy.is_expired(0));
        assert!(policy.is_expired(1));
    }

    #[test]
    fn test_max_timestamp() {
        let policy = RetentionPolicy::immutable_until(u64::MAX);
        assert!(!policy.is_expired(u64::MAX - 1));
        assert!(!policy.is_expired(u64::MAX));
    }

    #[test]
    fn test_very_large_gc_timestamp() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::immutable_until(1000), 0);
        let _ = reducer.register(2, RetentionPolicy::legal_hold(), 0);

        let removed = reducer.gc_expired(u64::MAX);
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_register_overwrites() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::none(), 100);
        let _ = reducer.register(1, RetentionPolicy::legal_hold(), 200);

        let (policy, size) = reducer.get(&1).unwrap();
        assert!(matches!(policy.mode, WormMode::LegalHold));
        assert_eq!(*size, 200);
    }

    #[test]
    fn test_different_hash_sizes() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::legal_hold(), 100);
        let _ = reducer.register(1000000, RetentionPolicy::legal_hold(), 200);
        let _ = reducer.register(u64::MAX, RetentionPolicy::legal_hold(), 300);

        assert_eq!(reducer.active_count(0), 3);
    }

    #[test]
    fn test_is_expired_edge_cases() {
        let none_policy = RetentionPolicy::none();
        assert!(none_policy.is_expired(0));
        assert!(none_policy.is_expired(u64::MAX));

        let immutable_policy = RetentionPolicy::immutable_until(100);
        assert!(!immutable_policy.is_expired(50));
        assert!(!immutable_policy.is_expired(100));
        assert!(immutable_policy.is_expired(101));

        let legal_hold_policy = RetentionPolicy::legal_hold();
        assert!(!legal_hold_policy.is_expired(0));
        assert!(!legal_hold_policy.is_expired(u64::MAX));
    }

    #[test]
    fn test_active_count_partial_expiry() {
        let mut reducer = WormReducer::new();

        for i in 1..=10 {
            let ts = i * 100;
            let _ = reducer.register(i, RetentionPolicy::immutable_until(ts), 0);
        }

        for check_ts in [0, 100, 250, 500, 750, 1000, 1500] {
            let active = reducer.active_count(check_ts);
            let expected = (1..=10).filter(|&i| i * 100 >= check_ts).count();
            assert_eq!(active, expected, "at timestamp {}", check_ts);
        }
    }

    #[test]
    fn test_gc_idempotent() {
        let mut reducer = WormReducer::new();

        let _ = reducer.register(1, RetentionPolicy::immutable_until(100), 0);
        let _ = reducer.register(2, RetentionPolicy::immutable_until(200), 0);

        let first = reducer.gc_expired(150);
        assert_eq!(first, 1);

        let second = reducer.gc_expired(150);
        assert_eq!(second, 0);

        let third = reducer.gc_expired(250);
        assert_eq!(third, 1);
    }

    #[test]
    fn test_reducer_default() {
        let reducer: WormReducer = Default::default();
        assert_eq!(reducer.total_count(), 0);
        assert_eq!(reducer.active_count(0), 0);
    }

    #[test]
    fn test_policy_clone() {
        let policy = RetentionPolicy::legal_hold();
        let cloned = policy;
        assert!(!cloned.is_expired(0));
    }

    #[test]
    fn test_worm_mode_variants() {
        assert!(matches!(RetentionPolicy::none().mode, WormMode::None));
        assert!(matches!(
            RetentionPolicy::immutable_until(100).mode,
            WormMode::Immutable
        ));
        assert!(matches!(
            RetentionPolicy::legal_hold().mode,
            WormMode::LegalHold
        ));
    }

    #[test]
    fn test_retain_until_values() {
        let p1 = RetentionPolicy::none();
        assert_eq!(p1.retain_until, None);

        let p2 = RetentionPolicy::immutable_until(500);
        assert_eq!(p2.retain_until, Some(500));

        let p3 = RetentionPolicy::legal_hold();
        assert_eq!(p3.retain_until, None);
    }

    #[test]
    fn test_gc_with_only_none_mode() {
        let mut reducer = WormReducer::new();
        let _ = reducer.register(1, RetentionPolicy::none(), 0);
        let _ = reducer.register(2, RetentionPolicy::none(), 0);

        let removed = reducer.gc_expired(1000);
        assert_eq!(removed, 2);
        assert_eq!(reducer.total_count(), 0);
    }

    #[test]
    fn test_gc_with_only_legal_hold() {
        let mut reducer = WormReducer::new();
        let _ = reducer.register(1, RetentionPolicy::legal_hold(), 0);
        let _ = reducer.register(2, RetentionPolicy::legal_hold(), 0);

        let removed = reducer.gc_expired(u64::MAX);
        assert_eq!(removed, 0);
        assert_eq!(reducer.total_count(), 2);
    }

    #[test]
    fn test_large_number_of_records() {
        let mut reducer = WormReducer::new();

        for i in 0..1000 {
            let policy = if i % 3 == 0 {
                RetentionPolicy::none()
            } else if i % 3 == 1 {
                RetentionPolicy::legal_hold()
            } else {
                RetentionPolicy::immutable_until(i)
            };
            let _ = reducer.register(i, policy, i);
        }

        assert_eq!(reducer.active_count(501), 499);
    }

    #[test]
    fn test_empty_hash_space() {
        let reducer = WormReducer::new();
        assert!(reducer.get(&999).is_none());
    }

    #[test]
    fn test_policy_equality() {
        let p1 = RetentionPolicy::immutable_until(100);
        let p2 = RetentionPolicy::immutable_until(100);
        let p3 = RetentionPolicy::immutable_until(200);

        assert_eq!(p1.mode, p2.mode);
        assert_eq!(p1.retain_until, p2.retain_until);
        assert_ne!(p1.retain_until, p3.retain_until);
    }
}
=== END FILE ===
```
