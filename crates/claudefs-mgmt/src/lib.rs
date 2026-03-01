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
pub mod perf_report;
pub mod quota;
pub mod scraper;
pub mod security;
pub mod topology;

pub use topology::{NodeInfo, NodeRole, NodeStatus, TopologyMap};

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

pub mod audit_trail;
pub mod compliance;
pub mod config_sync;
pub mod diagnostics;
pub mod maintenance;
pub mod rebalance;
pub mod cluster_bootstrap;

pub use audit_trail::{AuditTrail, AuditEvent, AuditEventKind, AuditFilter};
pub use config_sync::{ConfigStore, ConfigEntry, ConfigVersion, SyncStatus, ConfigSyncError};
pub use diagnostics::{
    CheckBuilder, DiagnosticCheck, DiagnosticLevel, DiagnosticReport, DiagnosticsError,
    DiagnosticsRunner,
};
pub use rebalance::{RebalanceScheduler, RebalanceJob, RebalanceJobId, JobState};

pub use maintenance::{
    MaintenanceError, MaintenanceMode, MaintenanceWindow, UpgradeCoordinator, UpgradePhase,
};