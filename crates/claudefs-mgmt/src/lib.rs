#![warn(missing_docs)]

//! ClaudeFS management subsystem: Prometheus exporter, DuckDB analytics, Web UI, CLI, admin API

pub mod metrics;
pub mod analytics;
pub mod cli;
pub mod api;
pub mod config;