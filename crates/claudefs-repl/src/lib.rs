#![warn(missing_docs)]

//! ClaudeFS replication subsystem: Cross-site journal replication, cloud conduit (gRPC/mTLS)

pub mod auth_ratelimit;
pub mod backpressure;
pub mod batch_auth;
pub mod checkpoint;
pub mod compression;
pub mod conduit;
pub mod engine;
pub mod error;
pub mod failover;
pub mod fanout;
pub mod health;
pub mod journal;
pub mod metrics;
pub mod pipeline;
pub mod report;
pub mod sync;
pub mod throttle;
pub mod topology;
pub mod uidmap;
pub mod wal;