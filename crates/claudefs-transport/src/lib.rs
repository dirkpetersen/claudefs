#![warn(missing_docs)]

//! ClaudeFS transport subsystem: RDMA via libfabric, TCP via io_uring, custom RPC protocol

pub mod connection;
pub mod protocol;
pub mod rdma;
pub mod rpc;
pub mod tcp;
