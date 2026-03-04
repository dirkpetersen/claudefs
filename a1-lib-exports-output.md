[0m
> build · accounts/fireworks/models/minimax-m2p5
[0m
[0m→ [0mRead crates/claudefs-storage/src/lib.rs
[0m
[0m← [0mEdit crates/claudefs-storage/src/lib.rs
Index: /home/cfs/claudefs/crates/claudefs-storage/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/lib.rs
@@ -37,8 +37,12 @@
 pub mod tiering_policy;
 pub mod write_journal;
 pub mod wear_leveling;
 pub mod zns;
+pub mod erasure;
+pub mod node_rebalance;
+pub mod nvme_passthrough;
+pub mod tracing_storage;
 
 #[cfg(feature = "uring")]
 pub mod uring_engine;
 

[0m
[0m← [0mEdit crates/claudefs-storage/src/lib.rs
Index: /home/cfs/claudefs/crates/claudefs-storage/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/lib.rs
@@ -104,5 +104,10 @@
     TokenBucket, WorkloadClass,
 };
 
 #[cfg(feature = "uring")]
-pub use uring_engine::{UringConfig, UringIoEngine, UringStats};
\ No newline at end of file
+pub use uring_engine::{UringConfig, UringIoEngine, UringStats};
+
+pub use erasure::{EcProfile, EcShard, StripeState, EcStripe, EcConfig, EcStats, EcError, ErasureCodingEngine};
+pub use node_rebalance::{NodeId, RebalanceSegmentId, ShardId, RebalanceState, MigrationDirection, MigrationTask, MigrationTaskState, RebalanceConfig, RebalanceStats, RebalanceEngine};
+pub use nvme_passthrough::{QueuePairId, CoreId, NsId, QueueState, NvmeOpType, SubmissionEntry, CompletionEntry, CompletionStatus, QueuePair, PassthroughConfig, PassthroughStats, PassthroughError, PassthroughManager};
+pub use tracing_storage::{TraceId, SpanId, TraceContext, StorageOp, SpanStatus, StorageSpan, TracingConfig, TracingStats, W3CTraceparent, StorageTracer};
\ No newline at end of file

[0m
[0m→ [0mRead crates/claudefs-storage/src/lib.rs
Done. Added 4 module declarations (lines 41-44) and 4 re-export blocks (lines 110-113).
