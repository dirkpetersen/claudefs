#![warn(missing_docs)]

//! ClaudeFS gateway subsystem: NFSv3 gateway, pNFS layouts, S3 API endpoint

/// Access log handling for gateway operations.
pub mod access_log;
/// Authentication and authorization for gateway protocols.
pub mod auth;
/// Configuration types for the gateway.
pub mod config;
/// Error types for the gateway subsystem.
pub mod error;
/// Export manager for NFS/SMB shares.
pub mod export_manager;
/// Audit logging for gateway operations.
pub mod gateway_audit;
/// Circuit breaker for backend fault tolerance and cascading failure prevention.
pub mod gateway_circuit_breaker;
/// Connection pool for backend metadata and transport node connections.
pub mod gateway_conn_pool;
/// Prometheus metrics and counters for gateway operations.
pub mod gateway_metrics;
/// TLS configuration for secure gateway connections.
pub mod gateway_tls;
/// Health check endpoints and monitoring.
pub mod health;
/// MOUNT protocol handling (RFC 1813).
pub mod mount;
/// Mount protocol handling.
pub mod nfs;
/// NFS ACL support.
pub mod nfs_acl;
/// NFS dentry caching.
pub mod nfs_cache;
/// NFSv4.2 server-side copy offload (COPY operation, RFC 7862).
pub mod nfs_copy_offload;
/// NFS delegation support.
pub mod nfs_delegation;
/// NFS export handling.
pub mod nfs_export;
/// NFS READDIRPLUS operations.
pub mod nfs_readdirplus;
/// NFS referral handling.
pub mod nfs_referral;
/// NFSv4 session management.
pub mod nfs_v4_session;
/// NFS write operations.
pub mod nfs_write;
/// Async TCP listener for NFSv3 RPC connections.
pub mod nfs_listener;
/// ClaudeFS cluster VfsBackend: NFS gateway wired to A2 metadata + A4 transport.
pub mod cluster_backend;
/// Performance configuration for gateway.
pub mod perf_config;
/// pNFS layout handling.
pub mod pnfs;
/// pNFS flexible storage layout.
pub mod pnfs_flex;
/// Portmapper (RPCBIND) support.
pub mod portmap;
/// Protocol definitions for NFS/SMB/RPC.
pub mod protocol;
/// Quota management.
pub mod quota;
/// RPC protocol handling.
pub mod rpc;
/// S3 API endpoint implementation.
pub mod s3;
/// S3 bucket policy handling.
pub mod s3_bucket_policy;
/// S3 CORS configuration.
pub mod s3_cors;
/// S3 server-side encryption.
pub mod s3_encryption;
/// S3 bucket lifecycle policies.
pub mod s3_lifecycle;
/// S3 multipart upload support.
pub mod s3_multipart;
/// S3 event notifications.
pub mod s3_notification;
/// S3 object lock (WORM) support.
pub mod s3_object_lock;
/// S3 presigned URL generation.
pub mod s3_presigned;
/// S3 rate limiting.
pub mod s3_ratelimit;
/// S3 cross-region replication configuration and state tracking.
pub mod s3_replication;
/// S3 request routing.
pub mod s3_router;
/// S3 storage class management (Standard, IA, Glacier, Intelligent-Tiering).
pub mod s3_storage_class;
/// S3 versioning support.
pub mod s3_versioning;
/// S3 XML request/response handling.
pub mod s3_xml;
/// Gateway server implementation.
pub mod server;
/// Session management for gateway connections.
pub mod session;
/// SMB protocol support.
pub mod smb;
/// SMB multi-channel support for high-throughput connections.
pub mod smb_multichannel;
/// Gateway statistics and metrics.
pub mod stats;
/// Token-based authentication.
pub mod token_auth;
/// Wire format handling.
pub mod wire;
/// XDR encoding/decoding for RPC.
pub mod xdr;
/// NFSv4 delegation state machine and lifecycle management.
pub mod nfs_delegation_manager;
/// Cross-protocol consistency detection and conflict resolution.
pub mod cross_protocol_consistency;
/// Tiered storage routing (hot NVMe ↔ cold S3).
pub mod tiered_storage_router;
/// OpenTelemetry observability and distributed tracing integration.
pub mod gateway_observability;
