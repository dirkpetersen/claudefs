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

pub mod drain;
pub mod health;
pub mod snapshot;
pub mod tiering;

pub use drain::DrainManager;
pub use health::HealthAggregator;
pub use snapshot::SnapshotCatalog;
pub use tiering::TieringManager;

pub mod capacity;
pub mod events;
pub mod migration;
pub mod node_scaling;
pub mod qos;
pub mod rbac;
pub mod sla;
pub mod tracing_otel;
pub mod webhook;

pub use capacity::CapacityPlanner;
pub use events::EventBus;
pub use migration::MigrationRegistry;
pub use node_scaling::NodeScalingManager;
pub use qos::{QosPriority, QosRegistry, QosPolicy, BandwidthLimit, TokenBucket};
pub use rbac::RbacRegistry;
pub use sla::{SlaChecker, SlaWindow, SlaTarget, SlaReport, SlaMetricKind, compute_percentiles};
pub use tracing_otel::{TracingManager, SpanBuilder, SpanContext, TracePropagator, RateSampler};
pub use webhook::WebhookRegistry;