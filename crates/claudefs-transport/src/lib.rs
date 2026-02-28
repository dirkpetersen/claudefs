#![warn(missing_docs)]

//! ClaudeFS transport subsystem: RDMA via libfabric, TCP via io_uring, custom RPC protocol.
//!
//! This crate provides the network transport layer for ClaudeFS, supporting:
//! - Custom binary RPC protocol with frame-based messaging
//! - TCP transport with zero-copy optimizations
//! - RDMA transport via libfabric (when hardware available)
//! - Connection pooling and lifecycle management
//! - Request/response multiplexing

pub mod buffer;
pub mod connection;
pub mod error;
pub mod message;
pub mod protocol;
pub mod rdma;
pub mod rpc;
pub mod tcp;
pub mod transport;

pub use buffer::{BufferPool, BufferPoolConfig, PooledBuffer, BufferPoolStats};
pub use error::{TransportError, Result};
pub use message::{serialize_message, deserialize_message};
pub use protocol::{Frame, FrameHeader, Opcode, FrameFlags};
pub use transport::{Transport, Connection, Listener};
