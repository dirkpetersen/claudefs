//! ClaudeFS management subsystem
//!
//! Provides Prometheus metrics export, Parquet-based metadata indexing,
//! DuckDB analytics, an Axum admin API, and CLI tooling for managing
//! a ClaudeFS cluster.

#![warn(missing_docs)]

pub mod alerting;
pub mod analytics;
pub mod api;
pub mod cli;
pub mod config;
pub mod grafana;
pub mod indexer;
pub mod metrics;
pub mod quota;
pub mod scraper;

pub use alerting::AlertManager;
pub use analytics::AnalyticsEngine;
pub use api::AdminApi;
pub use config::MgmtConfig;
pub use grafana::{all_dashboards, generate_cluster_overview_dashboard};
pub use indexer::MetadataIndexer;
pub use metrics::ClusterMetrics;
pub use quota::QuotaRegistry;
pub use scraper::ScraperPool;