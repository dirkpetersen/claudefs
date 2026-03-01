#![warn(missing_docs)]

//! ClaudeFS transport subsystem: RDMA via libfabric, TCP via io_uring, custom RPC protocol.
//!
//! This crate provides the network transport layer for ClaudeFS, supporting:
//! - Custom binary RPC protocol with frame-based messaging
//! - TCP transport with zero-copy optimizations
//! - RDMA transport via libfabric (when hardware available)
//! - Connection pooling and lifecycle management
//! - Request/response multiplexing

pub mod batch;
pub mod buffer;
pub mod circuitbreaker;
pub mod client;
pub mod connection;
pub mod deadline;
pub mod drain;
pub mod error;
pub use drain::{DrainConfig, DrainController, DrainGuard, DrainListener, DrainState, DrainStats};
pub mod flowcontrol;
pub mod health;
pub mod message;
pub mod metrics;
pub mod mux;
pub mod pool;
pub mod protocol;
pub mod qos;
pub mod retry;
pub mod routing;
pub mod rdma;
pub mod rpc;
pub mod tcp;
pub mod tls;
pub mod tls_tcp;
pub mod ratelimit;
pub mod tracecontext;
pub mod transport;
pub mod version;

pub use batch::{
    BatchConfig, BatchCollector, BatchEnvelope, BatchItem, BatchRequest, BatchResponse,
    BatchResult, BatchStats, BatchStatsSnapshot,
};
pub use buffer::{BufferPool, BufferPoolConfig, PooledBuffer, BufferPoolStats};
pub use client::{TransportClient, TransportClientConfig};
pub use deadline::{Deadline, DeadlineContext, encode_deadline, decode_deadline};
pub use error::{TransportError, Result};
pub use flowcontrol::{
    FlowControlConfig, FlowControlState, FlowController, FlowPermit, WindowController,
};
pub use health::{HealthConfig, HealthStatus, HealthStats, ConnectionHealth};
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
