# Fix: Add missing module exports to claudefs-storage/src/lib.rs

## Context
ClaudeFS storage crate (`crates/claudefs-storage/src/lib.rs`) has 4 source files that were
implemented but never declared as `pub mod` or re-exported with `pub use` in lib.rs.

These modules need to be added.

## Modules to add

### 1. erasure.rs — Reed-Solomon erasure coding (D1 architecture decision)
Public types: `EcProfile`, `EcShard`, `StripeState`, `EcStripe`, `EcConfig`, `EcStats`, `EcError`, `ErasureCodingEngine`

### 2. node_rebalance.rs — Online node scaling and segment rebalancing
Public types: `NodeId`, `RebalanceSegmentId`, `ShardId`, `RebalanceState`, `MigrationDirection`, `MigrationTask`, `MigrationTaskState`, `RebalanceConfig`, `RebalanceStats`, `RebalanceEngine`

### 3. nvme_passthrough.rs — NVMe queue pair management
Public types: `QueuePairId`, `CoreId`, `NsId`, `QueueState`, `NvmeOpType`, `SubmissionEntry`, `CompletionEntry`, `CompletionStatus`, `QueuePair`, `PassthroughConfig`, `PassthroughStats`, `PassthroughError`, `PassthroughManager`

### 4. tracing_storage.rs — Distributed tracing (OpenTelemetry-compatible)
Public types: `TraceId`, `SpanId`, `TraceContext`, `StorageOp`, `SpanStatus`, `StorageSpan`, `TracingConfig`, `TracingStats`, `W3CTraceparent`, `StorageTracer`

## Task
Read `/home/cfs/claudefs/crates/claudefs-storage/src/lib.rs`.

1. Add `pub mod erasure;`, `pub mod node_rebalance;`, `pub mod nvme_passthrough;`, `pub mod tracing_storage;` to the module declarations section (alongside the other `pub mod` lines).

2. Add `pub use` re-exports for these modules:
```rust
pub use erasure::{EcProfile, EcShard, StripeState, EcStripe, EcConfig, EcStats, EcError, ErasureCodingEngine};
pub use node_rebalance::{NodeId, RebalanceSegmentId, ShardId, RebalanceState, MigrationDirection, MigrationTask, MigrationTaskState, RebalanceConfig, RebalanceStats, RebalanceEngine};
pub use nvme_passthrough::{QueuePairId, CoreId, NsId, QueueState, NvmeOpType, SubmissionEntry, CompletionEntry, CompletionStatus, QueuePair, PassthroughConfig, PassthroughStats, PassthroughError, PassthroughManager};
pub use tracing_storage::{TraceId, SpanId, TraceContext, StorageOp, SpanStatus, StorageSpan, TracingConfig, TracingStats, W3CTraceparent, StorageTracer};
```

3. Write the complete updated file.

Do NOT remove any existing content. Only add the 4 new module declarations and their re-exports.
