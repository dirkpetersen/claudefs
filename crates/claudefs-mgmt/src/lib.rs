//! ClaudeFS management subsystem
//!
//! Provides Prometheus metrics export, Parquet-based metadata indexing,
//! DuckDB analytics, an Axum admin API, and CLI tooling for managing
//! a ClaudeFS cluster.

#![warn(missing_docs)]

/// Alerting rules and management
pub mod alerting;
/// DuckDB-based analytics and queries
pub mod analytics;
/// Axum HTTP API for admin operations
pub mod api;
/// CLI subcommands for cfs-mgmt
pub mod cli;
/// Configuration loading and management
pub mod config;
/// Cost tracking and billing
pub mod cost_tracker;
/// Grafana dashboard generation
pub mod grafana;
/// Parquet metadata indexing
pub mod indexer;
/// Prometheus metrics collection
pub mod metrics;
/// Operational metrics aggregation
pub mod ops_metrics;
/// Performance reporting
pub mod perf_report;
/// Storage quotas
pub mod quota;
/// Prometheus metrics scraping
pub mod scraper;
/// Security and authentication
pub mod security;
/// Cluster topology and node management
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

/// Node drain and decommissioning
pub mod drain;
/// Cluster health monitoring
pub mod health;
/// Snapshot management
pub mod snapshot;
/// Tiering policy execution
pub mod tiering;

pub use drain::DrainManager;
pub use health::HealthAggregator;
pub use snapshot::SnapshotCatalog;
pub use tiering::TieringManager;

/// Capacity planning and management
pub mod capacity;
/// Event bus for system notifications
pub mod events;
/// Data migration tools
pub mod migration;
/// Online node scaling
pub mod node_scaling;
/// Quality of Service policies
pub mod qos;
/// Role-based access control
pub mod rbac;
/// Service Level Agreement tracking
pub mod sla;
/// OpenTelemetry tracing integration
pub mod tracing_otel;
/// Webhook notifications
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

/// Audit trail logging
pub mod audit_trail;
/// Compliance and retention policies
pub mod compliance;
/// Configuration synchronization
pub mod config_sync;
/// System diagnostics
pub mod diagnostics;
/// Maintenance and upgrade coordination
pub mod maintenance;
/// Data rebalancing scheduler
pub mod rebalance;
/// Cluster bootstrap and initialization
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

pub use cluster_bootstrap::{BootstrapConfig, BootstrapError, BootstrapManager, BootstrapState, NodeSpec};
pub use compliance::{ComplianceError, ComplianceRegistry, RetentionPolicy, RetentionRecord, RetentionStatus};

/// Live configuration reloading
pub mod live_config;
pub use live_config::{LiveConfigStore, LiveConfigEntry, LiveConfigError, ReloadStatus, ConfigWatcher, validate_json, parse_entry};
pub use ops_metrics::{OpsMetricsAggregator, NodeMetricsSnapshot, ClusterOpsMetrics, ClusterHealthScore, MetricTrend, TrendDirection, OpsMetricsError};
