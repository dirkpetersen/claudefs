[0m

[0m
> build · accounts/fireworks/models/minimax-m2p5[0m→ [0mRead crates/claudefs-transport/src/multipath.rs[90m [limit=30][0m
[0m✱ [0mGrep "pub enum PathSelectionPolicy"[90m in crates/claudefs-transport/src · 1 match[0m
[0m→ [0mRead crates/claudefs-transport/src/multipath.rs[90m [offset=78, limit=15][0m
[0m
[0m← [0mEdit crates/claudefs-transport/src/multipath.rsIndex: /home/cfs/claudefs/crates/claudefs-transport/src/multipath.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-transport/src/multipath.rs
+++ /home/cfs/claudefs/crates/claudefs-transport/src/multipath.rs
@@ -77,10 +77,10 @@
 }
 
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
 pub enum PathSelectionPolicy {
-    #[default]
     RoundRobin,
+    #[default]
     LowestLatency,
     WeightedRandom,
     Failover,
 }



[0m[0m→ [0mRead crates/claudefs-transport/src/multipath.rs[90m [limit=15, offset=78][0m
Done. The `#[default]` attribute has been moved from `RoundRobin` to `LowestLatency` in `crates/claudefs-transport/src/multipath.rs:82`.
