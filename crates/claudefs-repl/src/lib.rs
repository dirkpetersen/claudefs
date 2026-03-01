#![warn(missing_docs)]

//! ClaudeFS replication subsystem: Cross-site journal replication, cloud conduit (gRPC/mTLS)
//!
//! This module implements asynchronous journal replication across geographically distributed sites.
//! It handles:
//! - Write-ahead logging and cursor tracking (wal, journal)
//! - Cloud conduit communication via gRPC with mTLS (conduit, tls_policy)
//! - Conflict resolution with last-write-wins and split-brain detection (conflict_resolver, split_brain)
//! - Site failover and active-active replication (failover, site_failover, active_active)
//! - Health monitoring and metrics export (health, metrics, otel_repl)
//! - Performance optimization: compression, backpressure, throttling, rate limiting (compression, backpressure, throttle, auth_ratelimit, recv_ratelimit)
//! - Security: UID/GID mapping, batch authentication, TLS policy, audit trail (uidmap, batch_auth, tls_policy, repl_audit)
//! - QoS enforcement and journal garbage collection (repl_qos, journal_gc)
//! - Bootstrap coordination for new replicas (repl_bootstrap)

/// Authentication and rate limiting module.
pub mod auth_ratelimit;
/// Backpressure management for replication pipeline.
pub mod backpressure;
/// Batch authentication processing.
pub mod batch_auth;
/// Checkpoint management for replication state.
pub mod checkpoint;
/// Conflict resolution for diverged metadata.
pub mod conflict_resolver;
/// Journal entry compression.
pub mod compression;
/// gRPC cloud conduit for cross-site communication.
pub mod conduit;
/// Core replication engine orchestration.
pub mod engine;
/// Error types for replication subsystem.
pub mod error;
/// Site failover logic.
pub mod failover;
/// Fan-out writes to multiple sites.
pub mod fanout;
/// Site health monitoring.
pub mod health;
/// Write-ahead journal for metadata changes.
pub mod journal;
/// Prometheus metrics export.
pub mod metrics;
/// Replication lag monitoring and SLA enforcement.
pub mod lag_monitor;
/// Replication pipeline coordination.
pub mod pipeline;
/// Replication status reporting.
pub mod report;
/// Synchronization primitives and cross-site sync.
pub mod sync;
/// Write throttling for peer coordination.
pub mod throttle;
/// Replication topology and site registration.
pub mod topology;
/// UID/GID translation across sites.
pub mod uidmap;
/// Write-ahead log with cursor tracking.
pub mod wal;
/// TLS policy enforcement for inter-site communication.
pub mod tls_policy;
/// Site registry for remote site management.
pub mod site_registry;
/// Site failover coordination.
pub mod site_failover;
/// Receive-side rate limiting.
pub mod recv_ratelimit;
/// Journal garbage collection and retention.
pub mod journal_gc;
/// QoS enforcement for replication traffic.
pub mod repl_qos;
/// Audit trail tracking for replication events.
pub mod repl_audit;
/// Split-brain detection and recovery.
pub mod split_brain;
/// OpenTelemetry instrumentation for replication.
pub mod otel_repl;
/// Bootstrap coordination for new replica sites.
pub mod repl_bootstrap;
/// Maintenance window coordination (future use).
pub mod repl_maintenance;
/// Active-active replication mode.
pub mod active_active;