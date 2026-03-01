//! ClaudeFS management subsystem
//!
//! Provides Prometheus metrics export, Parquet-based metadata indexing,
//! DuckDB analytics, an Axum admin API, and CLI tooling for managing
//! a ClaudeFS cluster.

#![warn(missing_docs)]

pub mod analytics;
pub mod api;
pub mod cli;
pub mod config;
pub mod metrics;

pub use analytics::AnalyticsEngine;
pub use api::AdminApi;
pub use config::MgmtConfig;
pub use metrics::ClusterMetrics;