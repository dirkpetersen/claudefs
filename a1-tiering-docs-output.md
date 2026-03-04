

[0m> build · accounts/fireworks/models/minimax-m2p5[0m
[0m→ [0mRead crates/claudefs-storage/src/tiering_policy.rs
[0m
[0m← [0mEdit crates/claudefs-storage/src/tiering_policy.rs

Index: /home/cfs/claudefs/crates/claudefs-storage/src/tiering_policy.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/tiering_policy.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/tiering_policy.rs
@@ -1,13 +1,20 @@
+//! Tiering policy engine for intelligent flash/S3 data placement.
+
 use serde::{Deserialize, Serialize};
 use std::collections::HashMap;
 use tracing::{debug, trace};
 
+/// Storage tier classification for a segment.
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
 pub enum TierClass {
+    /// Frequently accessed data, kept on flash.
     Hot,
+    /// Moderately accessed data on flash.
     Warm,
+    /// Infrequently accessed data, candidate for eviction.
     Cold,
+    /// Stale data not accessed within the frozen threshold.
     Frozen,
 }
 
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
[0m
