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
pub mod cancel;
pub mod circuitbreaker;
pub mod client;
pub mod compress;
pub mod congestion;
pub mod conn_auth;
pub mod connmigrate;
pub mod connection;
pub mod deadline;
pub mod discovery;
pub mod drain;
pub mod enrollment;
pub mod error;
pub use drain::{DrainConfig, DrainController, DrainGuard, DrainListener, DrainState, DrainStats};
pub mod flowcontrol;
pub mod health;
pub mod hedge;
pub mod keepalive;
pub mod loadshed;
pub mod message;
pub mod metrics;
pub mod multipath;
pub mod mux;
pub mod observability;
pub mod pool;
pub mod pipeline;
pub mod priority;
pub mod protocol;
pub mod qos;
pub mod retry;
pub mod routing;
pub mod rdma;
pub mod rpc;
pub mod server;
pub mod splice;
pub mod tcp;
pub mod tenant;
pub mod tls;
pub mod tls_tcp;
pub mod ratelimit;
pub mod request_dedup;
pub mod tracecontext;
pub mod transport;
pub mod version;
pub mod zerocopy;

pub use batch::{
    BatchConfig, BatchCollector, BatchEnvelope, BatchItem, BatchRequest, BatchResponse,
    BatchResult, BatchStats, BatchStatsSnapshot,
};
pub use priority::{
    Priority, PriorityConfig, PriorityScheduler, PriorityStats, PriorityStatsSnapshot,
    PrioritizedRequest, classify_opcode,
};
pub use compress::{
    CompressionAlgorithm, CompressionConfig, CompressedPayload, Compressor,
    CompressionStats, CompressionStatsSnapshot,
};
pub use buffer::{BufferPool, BufferPoolConfig, PooledBuffer, BufferPoolStats};
pub use client::{TransportClient, TransportClientConfig};
pub use deadline::{Deadline, DeadlineContext, encode_deadline, decode_deadline};
pub use discovery::{DiscoveryConfig, DiscoveryStats, MemberInfo, MembershipEvent, MembershipList, NodeState};
pub use error::{TransportError, Result};
pub use flowcontrol::{
    FlowControlConfig, FlowControlState, FlowController, FlowPermit, WindowController,
};
pub use health::{HealthConfig, HealthStatus, HealthStats, ConnectionHealth};
pub use keepalive::{KeepAliveConfig, KeepAliveManager, KeepAliveState, KeepAliveStats, KeepAliveTracker};
pub use message::{serialize_message, deserialize_message};
pub use protocol::{Frame, FrameHeader, Opcode, FrameFlags};
pub use qos::{
    default_qos_config, QosConfig, QosError, QosPermit, QosScheduler, QosStats, WorkloadClass,
};
pub use tls::{TlsConfig, TlsConnector, TlsAcceptor};
pub use tls_tcp::TlsTcpTransport;
pub use tracecontext::{
    TraceContext, TraceFlags, TraceId, TraceParent, TraceState, TRACEPARENT_HEADER,
    TRACESTATE_HEADER,
};
pub use metrics::{MetricsSnapshot, TransportMetrics};
pub use retry::{RetryConfig, RetryExecutor, RetryOutcome, RetryPolicy, is_retryable};
pub use routing::{ConsistentHashRing, NodeId, NodeInfo, RoutingTable, ShardId, ShardRouter};
pub use mux::{Multiplexer, MuxConfig, StreamHandle, StreamState};
pub use ratelimit::{
    CompositeRateLimiter, RateLimitConfig, RateLimitResult, RateLimiter,
};
pub use transport::{Transport, Connection, Listener};
pub use circuitbreaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use server::{ServerConfig, RequestContext, RequestHandler, RpcServer, ServerStats};
pub use cancel::{CancelToken, CancelHandle, CancelReason, CancelRegistry, CancelStats};
pub use hedge::{HedgeConfig, HedgePolicy, HedgeStats, HedgeTracker};
pub use loadshed::{LoadShedConfig, LoadShedStats, LoadShedder};
pub use tenant::{TenantId, TenantConfig, TenantTracker, TenantStats, TenantManager, TenantAdmitResult};
pub use zerocopy::{ZeroCopyConfig, MemoryRegion, RegionId, RegionPool, RegionPoolStats};
pub use adaptive::{
    AdaptiveConfig, AdaptiveStats, AdaptiveStatsSnapshot, AdaptiveTimeout, LatencyHistogram,
    PercentileSnapshot,
};
pub use backpressure::{
    BackpressureConfig, BackpressureMonitor, BackpressureSignal, BackpressureStats,
    BackpressureStatsSnapshot, BackpressureThrottle, PressureLevel, ThrottleConfig,
};
pub use connmigrate::{
    ConnectionId, MigrationConfig, MigrationError, MigrationManager, MigrationReason,
    MigrationRecord, MigrationState, MigrationStats, MigrationStatsSnapshot,
};
pub use pipeline::{
    Pipeline, PipelineConfig, PipelineError, PipelineRequest, PipelineResult,
    PipelineStats, PipelineStatsSnapshot, PipelineDirection, StageAction, StageConfig,
    StageId, StageProcessor, StageResult, PassthroughStage, RejectStage, HeaderStage,
};
pub use observability::{
    ObservabilityConfig, ObservabilityStats, ObservabilityStatsSnapshot,
    SpanId, SpanStatus, SpanEvent, Span, SpanBuilder, SpanCollector,
    EventSeverity, Attribute, AttributeValue,
};
pub use bandwidth::{BandwidthAllocator, BandwidthConfig, BandwidthResult, BandwidthStats, EnforcementMode};
pub use request_dedup::{DedupConfig, DedupEntry, DedupResult, DedupStats, DedupTracker, RequestId};
pub use splice::{
    SpliceConfig, SpliceError, SpliceFlags, SpliceOperation, SplicePipeline, SplicePlan,
    SpliceStats, TransferEndpoint,
};
pub use enrollment::{
    CertificateBundle, ClusterCA, EnrollmentConfig, EnrollmentError, EnrollmentService,
    EnrollmentStats, EnrollmentToken, RevocationEntry, RevocationReason,
};
