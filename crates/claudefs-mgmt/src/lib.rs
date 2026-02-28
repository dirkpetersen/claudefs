#![warn(missing_docs)]

//! ClaudeFS management subsystem: Prometheus exporter, DuckDB analytics, Web UI, CLI, admin API

pub mod analytics;
pub mod api;
pub mod cli;
pub mod config;
pub mod metrics;
