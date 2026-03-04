#![warn(missing_docs)]

//! ClaudeFS transport subsystem: RDMA via libfabric, TCP via io_uring, custom RPC protocol.
//!
//! This crate provides the network transport layer for ClaudeFS, supporting:
//! - Custom binary RPC protocol with frame-based messaging
//! - TCP transport with zero-copy optimizations
//! - RDMA transport via libfabric (when hardware available)
//! - Connection pooling and lifecycle management
//! - Request/response multiplexing
//! - Adaptive load shedding
//! - Request cancellation
//! - Speculative hedging
//! - Multi-tenant traffic isolation
//! - Zero-copy buffer management
//! - Configurable request middleware pipeline
//! - Coordinated backpressure signal propagation
//! - Adaptive timeout tuning
//! - Connection migration
//! - Structured observability with spans and events

pub mod adaptive;
pub mod backpressure;
pub mod bandwidth;
pub mod batch;
pub mod buffer;
pub mod bulk_transfer;
pub mod cancel;
pub mod circuitbreaker;
pub mod client;
pub mod cluster_topology;
pub mod compress;
pub mod congestion;
pub mod conn_auth;
pub mod conn_drain_aware;
pub mod connmigrate;
pub mod connection;
pub mod credit_window;
pub mod deadline;
pub mod discovery;
pub mod drain;
pub mod endpoint_registry;
pub mod enrollment;
pub mod error;
pub use drain::{DrainConfig, DrainController, DrainGuard, DrainListener, DrainState, DrainStats};
pub mod fanout;
pub mod fault_inject;
pub mod flowcontrol;
pub mod gossip;
pub mod health;
pub mod hedge;
pub mod keepalive;
pub mod lease;
pub mod loadshed;
pub mod message;
pub mod metrics;
pub mod multipath;
pub mod mux;
pub mod observability;
pub mod otel;
pub mod pool;
pub mod pipeline;
pub mod priority;
pub mod protocol;
pub mod qos;
pub mod quorum;
pub mod node_blacklist;
pub mod read_repair;
pub mod retry;
pub mod repl_state;
pub mod routing;
pub mod rdma;
pub mod multicast_group;
pub mod rpc;
pub mod session;
pub mod server;
pub mod segment_router;
pub mod shard_map;
pub mod splice;
pub mod splice_queue;
pub mod stream;
pub mod tcp;
pub mod tenant;
pub mod timer_wheel;
pub mod timeout_budget;
pub mod tls;
pub mod tls_tcp;
pub mod ratelimit;
pub mod request_dedup;
pub mod tracecontext;
pub mod transport;
pub mod version;
pub mod wire_diag;
pub mod write_pipeline;
pub mod zerocopy;

pub mod flow_sched;
pub mod rebalance;
pub mod snapshot_transfer;

pub mod ipc;
pub mod repl_channel;
pub mod pnfs_layout;

pub use ipc::{IpcConfig, IpcConnection, IpcConnectionState, IpcManager, IpcStats, IpcStatsSnapshot};
pub use repl_channel::{
    InFlightEntry, JournalEntry, ReplAck, ReplChannel, ReplChannelConfig, ReplChannelState,
    ReplChannelStats, ReplChannelStatsSnapshot, ReplError,
};
pub use pnfs_layout::{
    DataLayout, DeviceAddr, DeviceId, IoMode, LayoutCache, LayoutError, LayoutSegment,
    LayoutStateId, LayoutTypeTag, StripePattern,
};
pub use cluster_topology::{
    ClusterTopology, DatacenterId, Proximity, RackId, TopologyLabel, TopologyStatsSnapshot,
};
pub use fault_inject::{
    ConnectAction, FaultConfig, FaultInjector, FaultInjectorStatsSnapshot, FaultKind, FaultSpec,
    RecvAction, SendAction, corrupt_payload,
};
pub use otel::{
    OtlpAttribute, OtlpConfig, OtlpEvent, OtlpExporter, OtlpExporterStatsSnapshot, OtlpSpan,
    OtlpStatusCode, OtlpValue, inject_trace_context, span_to_otlp,
};
pub use multicast_group::{
    BroadcastResult, GroupEvent, GroupId, GroupMember, MulticastError, MulticastGroupConfig,
    MulticastGroupManager, MulticastGroupStats, MulticastGroupStatsSnapshot,
};
pub use wire_diag::{
    InFlightPing, RttSample, RttSeries, RttSeriesSnapshot, TraceHop, TracePath,
    WireDiag, WireDiagConfig, WireDiagStats, WireDiagStatsSnapshot,
};
pub use credit_window::{
    CreditGrant, CreditWindow, CreditWindowConfig, CreditWindowState,
    CreditWindowStats, CreditWindowStatsSnapshot,
};
pub use fanout::{
    FanoutConfig, FanoutId, FanoutManager, FanoutOp, FanoutState, FanoutStats, FanoutStatsSnapshot,
    FanoutTarget, FanoutTargetResult,
};
pub use quorum::{
    QuorumConfig, QuorumError, QuorumManager, QuorumPolicy, QuorumResult, QuorumRound,
    QuorumStats, QuorumStatsSnapshot, Vote,
};
pub use segment_router::{
    EcConfig, SegmentId, SegmentPlacement, SegmentRouter, SegmentRouterConfig,
    SegmentRouterError, SegmentRouterStats, SegmentRouterStatsSnapshot, StripeAssignment,
};
pub use repl_state::{
    JournalEntryRecord, JournalReplChannel, JournalReplChannelStats, JournalReplChannelStatsSnapshot,
    JournalSeq, ReplState, ReplStateConfig,
};
pub use read_repair::{
    ReadRepairConfig, ReadRepairManager, ReadRepairStats, ReadRepairStatsSnapshot,
    RepairError, RepairId, RepairOp, RepairOpState, RepairPriority, RepairShard, ShardRepairState,
};
pub use node_blacklist::{
    BlacklistConfig, BlacklistEntry, BlacklistReason, BlacklistStats, BlacklistStatsSnapshot,
    NodeBlacklist,
};
pub use write_pipeline::{
    WriteError, WriteId, WritePipelineManager, WritePipelineOp, WritePipelineStats,
    WritePipelineStatsSnapshot, WriteStage,
};
pub use splice_queue::{
    SpliceDestination, SpliceEntry, SpliceQueue, SpliceQueueConfig, SpliceQueueError,
    SpliceQueueStats, SpliceQueueStatsSnapshot, SpliceSource, SpliceState,
};
pub use conn_drain_aware::{
    ConnDrainConfig, ConnDrainError, ConnDrainManager, ConnDrainState, ConnDrainStats,
    ConnDrainStatsSnapshot, ConnDrainTracker,
};
pub use lease::{
    Lease, LeaseConfig, LeaseError, LeaseId, LeaseManager, LeaseState, LeaseStats,
    LeaseStatsSnapshot, LeaseType,
};
pub use shard_map::{
    ShardInfo, ShardMap, ShardMapConfig, ShardMapError, ShardMapStats, ShardMapStatsSnapshot,
    ShardReplica, ShardRole, VirtualShard,
};
pub use timeout_budget::{
    TimeoutBudget, TimeoutBudgetConfig, TimeoutBudgetManager, TimeoutBudgetStats,
    TimeoutBudgetStatsSnapshot,
};

pub use flow_sched::{
    FlowEntry, FlowId, FlowSchedConfig, FlowSchedError, FlowSchedStatsSnapshot,
    FlowScheduler, PendingSend, SendDecision,
};
pub use rebalance::{
    MigrationTask, MigrationTaskState, RebalanceConfig, RebalanceCoordinator, RebalanceError,
    RebalancePlan, RebalanceStatsSnapshot, RebalanceTrigger,
};
pub use snapshot_transfer::{
    ChunkTransferState, SnapshotMeta, SnapshotTransferConfig, SnapshotTransferManager,
    SnapshotTransferStatsSnapshot, TransferChunk, TransferError, TransferId, TransferState,
};
