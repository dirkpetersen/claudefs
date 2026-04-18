use crate::config::MgmtConfig;
use crate::metrics::ClusterMetrics;
use crate::api::AdminApi;
use crate::health::{HealthAggregator, NodeHealth, HealthStatus, ClusterHealth};
use crate::event_sink::{ExportedEvent, EventSeverity};
use crate::recovery_actions::{RecoveryExecutor, RecoveryLog, RecoveryAction, ActionStatus, RecoveryConfig};
use crate::alerting::{AlertManager, Alert, AlertSeverity, AlertState, AlertRule};
use crate::capacity::{CapacityPlanner, CapacityDataPoint, CapacityProjection, CapacityRecommendation};
use crate::diagnostics::{DiagnosticsRunner, DiagnosticReport, DiagnosticLevel, DiagnosticCheck};
use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "cfs-mgmt")]
#[command(about = "ClaudeFS management CLI", long_about = None)]
pub struct Cli {
    #[arg(short, long, default_value = "http://localhost:8443")]
    pub server: String,

    #[arg(short, long, env = "CFS_ADMIN_TOKEN")]
    pub token: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Status,
    Node {
        #[command(subcommand)]
        cmd: NodeCmd,
    },
    Query {
        sql: String,
    },
    TopUsers {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    TopDirs {
        #[arg(short, long, default_value = "3")]
        depth: usize,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    Find {
        pattern: String,
    },
    Stale {
        #[arg(short, long, default_value = "180")]
        days: u64,
    },
    ReductionReport,
    ReplicationStatus,
    Serve {
        #[arg(short, long, default_value = "/etc/claudefs/mgmt.toml")]
        config: PathBuf,
    },
    Health {
        #[arg(short, long)]
        verbose: bool,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        node: Option<String>,
    },
    Diagnostics {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        csv: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Recovery {
        #[command(subcommand)]
        cmd: RecoveryCmd,
    },
    Capacity {
        #[arg(long, default_value = "30")]
        forecast_days: usize,
        #[arg(long)]
        json: bool,
    },
    Alerts {
        #[arg(long)]
        severity: Option<String>,
        #[arg(long)]
        alert_type: Option<String>,
        #[arg(long)]
        active: bool,
        #[arg(long)]
        resolved: bool,
        #[arg(long, default_value = "50")]
        limit: usize,
        #[arg(long)]
        acknowledge: Option<String>,
        #[arg(long)]
        silence: Option<String>,
        #[arg(long)]
        silence_duration: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    Dashboard {
        #[arg(long, default_value = "all")]
        dashboard: String,
        #[arg(long, default_value = "now-6h")]
        time_from: String,
        #[arg(long, default_value = "now")]
        time_to: String,
        #[arg(long)]
        open: bool,
        #[arg(long, default_value = "http://localhost:3000")]
        grafana_host: String,
    },
}

#[derive(Subcommand, Clone)]
pub enum NodeCmd {
    List,
    Drain {
        node_id: String,
    },
    Show {
        node_id: String,
    },
}

#[derive(Subcommand, Clone)]
pub enum RecoveryCmd {
    Show {
        #[arg(long)]
        action_type: Option<String>,
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "50")]
        limit: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        csv: bool,
    },
    Execute {
        #[arg(long, required = true)]
        action_type: String,
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = "NORMAL")]
        priority: String,
    },
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Command::Status => self.status().await,
            Command::Node { ref cmd } => self.node(cmd).await,
            Command::Query { ref sql } => self.query(sql).await,
            Command::TopUsers { limit } => self.top_users(limit).await,
            Command::TopDirs { depth, limit } => self.top_dirs(depth, limit).await,
            Command::Find { ref pattern } => self.find(pattern).await,
            Command::Stale { days } => self.stale(days).await,
            Command::ReductionReport => self.reduction_report().await,
            Command::ReplicationStatus => self.replication_status().await,
            Command::Serve { ref config } => self.serve(config).await,
            Command::Health { verbose, json, node } => self.health(verbose, json, node.as_deref()).await,
            Command::Diagnostics { json, csv, output } => self.diagnostics(json, csv, output.as_deref()).await,
            Command::Recovery { ref cmd } => self.recovery(cmd).await,
            Command::Capacity { forecast_days, json } => self.capacity(forecast_days, json).await,
            Command::Alerts { severity, alert_type, active, resolved, limit, acknowledge, silence, silence_duration, json } => {
                self.alerts(severity.as_deref(), alert_type.as_deref(), active, resolved, limit, acknowledge.as_deref(), silence.as_deref(), silence_duration, json).await
            },
            Command::Dashboard { dashboard, time_from, time_to, open, grafana_host } => {
                self.dashboard(&dashboard, &time_from, &time_to, open, &grafana_host).await
            },
        }
    }

    async fn status(&self) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/cluster/status", self.server);

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize)]
        struct ClusterStatus {
            total_nodes: usize,
            healthy_nodes: usize,
            degraded_nodes: usize,
            offline_nodes: usize,
            status: String,
        }

        let status: ClusterStatus = response.json().await?;

        println!("Cluster Status: {}", status.status);
        println!("Total Nodes: {}", status.total_nodes);
        println!("Healthy: {}", status.healthy_nodes);
        println!("Degraded: {}", status.degraded_nodes);
        println!("Offline: {}", status.offline_nodes);

        Ok(())
    }

    async fn node(&self, cmd: &NodeCmd) -> Result<()> {
        match cmd {
            NodeCmd::List => self.nodes_list().await,
            NodeCmd::Drain { node_id } => self.node_drain(node_id).await,
            NodeCmd::Show { node_id } => self.node_show(node_id).await,
        }
    }

    async fn nodes_list(&self) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/nodes", self.server);

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize)]
        struct NodeInfo {
            node_id: String,
            addr: String,
            status: String,
            capacity_total: u64,
            capacity_used: u64,
        }

        let nodes: Vec<NodeInfo> = response.json().await?;

        println!("{:<20} {:<20} {:<15} {:>15} {:>15}", "NODE ID", "ADDRESS", "STATUS", "TOTAL", "USED");
        println!("{}", "-".repeat(85));

        for node in nodes {
            println!(
                "{:<20} {:<20} {:<15} {:>15} {:>15}",
                node.node_id,
                node.addr,
                node.status,
                Self::format_bytes(node.capacity_total),
                Self::format_bytes(node.capacity_used)
            );
        }

        Ok(())
    }

    async fn node_drain(&self, node_id: &str) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/nodes/{}/drain", self.server, node_id);

        let mut request = client.post(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize)]
        struct DrainResponse {
            node_id: String,
            status: String,
            message: String,
        }

        let result: DrainResponse = response.json().await?;

        println!("Node: {}", result.node_id);
        println!("Status: {}", result.status);
        println!("Message: {}", result.message);

        Ok(())
    }

    async fn node_show(&self, node_id: &str) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/nodes", self.server);

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize)]
        struct NodeInfo {
            node_id: String,
            addr: String,
            status: String,
            capacity_total: u64,
            capacity_used: u64,
            last_seen: u64,
        }

        let nodes: Vec<NodeInfo> = response.json().await?;

        if let Some(node) = nodes.into_iter().find(|n| n.node_id.as_str() == node_id) {
            println!("Node ID: {}", node.node_id);
            println!("Address: {}", node.addr);
            println!("Status: {}", node.status);
            println!("Capacity Total: {}", Self::format_bytes(node.capacity_total));
            println!("Capacity Used: {}", Self::format_bytes(node.capacity_used));
            println!("Last Seen: {}", node.last_seen);
        } else {
            anyhow::bail!("Node not found: {}", node_id);
        }

        Ok(())
    }

    async fn query(&self, sql: &str) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/query", self.server);

        let mut request = client.post(&url).json(&serde_json::json!({ "sql": sql }));
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        let result: serde_json::Value = response.json().await?;
        println!("{}", serde_json::to_string_pretty(&result)?);

        Ok(())
    }

    async fn top_users(&self, limit: usize) -> Result<()> {
        let _ = limit;
        println!("Top users query not yet implemented (requires DuckDB backend)");
        Ok(())
    }

    async fn top_dirs(&self, depth: usize, limit: usize) -> Result<()> {
        let _ = (depth, limit);
        println!("Top directories query not yet implemented (requires DuckDB backend)");
        Ok(())
    }

    async fn find(&self, pattern: &str) -> Result<()> {
        let _ = pattern;
        println!("Find files query not yet implemented (requires DuckDB backend)");
        Ok(())
    }

    async fn stale(&self, days: u64) -> Result<()> {
        let _ = days;
        println!("Stale files query not yet implemented (requires DuckDB backend)");
        Ok(())
    }

    async fn reduction_report(&self) -> Result<()> {
        println!("Reduction report not yet implemented (requires DuckDB backend)");
        Ok(())
    }

    async fn replication_status(&self) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/replication/status", self.server);

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize)]
        struct ReplicationStatus {
            lag_secs: f64,
            conflicts_total: u64,
            status: String,
        }

        let status: ReplicationStatus = response.json().await?;

        println!("Replication Status: {}", status.status);
        println!("Lag: {:.2} seconds", status.lag_secs);
        println!("Conflicts: {}", status.conflicts_total);

        Ok(())
    }

    async fn serve(&self, config_path: &PathBuf) -> Result<()> {
        let config = if config_path.exists() {
            MgmtConfig::from_file(config_path)?
        } else {
            tracing::warn!("Config file not found, using defaults: {}", config_path.display());
            MgmtConfig::default()
        };

        let metrics = Arc::new(ClusterMetrics::new());
        let config = Arc::new(config);

        let api = AdminApi::new(metrics, config.clone(), config.index_dir.clone());
        api.serve().await
    }

    async fn health(&self, verbose: bool, json: bool, node: Option<&str>) -> Result<()> {
        let client = Client::new();
        let url = if let Some(n) = node {
            format!("{}/api/v1/health?node={}", self.server, n)
        } else {
            format!("{}/api/v1/health", self.server)
        };

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize, Serialize)]
        struct HealthResponse {
            status: String,
            nodes: Option<Vec<NodeHealthStatus>>,
            latency_ms: Option<u64>,
            subsystems: Option<HashMap<String, SubsystemStatus>>,
        }

        #[derive(Deserialize, Serialize)]
        struct NodeHealthStatus {
            node_id: String,
            status: String,
            latency_ms: Option<u64>,
            capacity_used_pct: Option<f64>,
        }

        #[derive(Deserialize, Serialize)]
        struct SubsystemStatus {
            status: String,
            message: Option<String>,
        }

        let health: HealthResponse = response.json().await?;

        if json {
            println!("{}", serde_json::to_string_pretty(&health)?);
            return Ok(());
        }

        println!("Cluster Health: {}", health.status);

        if let Some(nodes) = health.nodes {
            println!("\nNode Status:");
            println!("{:<20} {:<15} {:>12} {:>15}", "NODE ID", "STATUS", "LATENCY", "CAPACITY %");
            println!("{}", "-".repeat(65));

            for n in nodes {
                let latency = n.latency_ms.map(|l| format!("{} ms", l)).unwrap_or_else(|| "N/A".to_string());
                let capacity = n.capacity_used_pct.map(|c| format!("{:.1}%", c)).unwrap_or_else(|| "N/A".to_string());
                println!("{:<20} {:<15} {:>12} {:>15}", n.node_id, n.status, latency, capacity);
            }
        }

        if verbose {
            if let Some(latency) = health.latency_ms {
                println!("\nLatency: {} ms", latency);
            }
            if let Some(subsystems) = health.subsystems {
                println!("\nSubsystems:");
                for (name, sub) in subsystems {
                    let status_str = sub.status;
                    let msg = sub.message.unwrap_or_default();
                    println!("  {}: {} {}", name, status_str, if msg.is_empty() { "" } else { &msg });
                }
            }
        }

        Ok(())
    }

    async fn diagnostics(&self, json: bool, csv: bool, output: Option<&Path>) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/diagnostics", self.server);

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize, Serialize)]
        struct DiagnosticResult {
            check_name: String,
            status: String,
            message: Option<String>,
            details: Option<HashMap<String, serde_json::Value>>,
        }

        let diagnostics: Vec<DiagnosticResult> = response.json().await?;

        if json || csv {
            if let Some(out_path) = output {
                if json {
                    let file = std::fs::File::create(out_path)?;
                    serde_json::to_writer(file, &diagnostics)?;
                    println!("Diagnostics written to {}", out_path.display());
                } else {
                    let file = std::fs::File::create(out_path)?;
                    let mut wtr = csv::Writer::from_writer(file)?;
                    wtr.write_record(&["check_name", "status", "message"])?;
                    for d in &diagnostics {
                        wtr.write_record(&[&d.check_name, &d.status, &d.message.clone().unwrap_or_default()])?;
                    }
                    wtr.flush()?;
                    println!("Diagnostics written to {}", out_path.display());
                }
            } else {
                if json {
                    println!("{}", serde_json::to_string_pretty(&diagnostics)?);
                } else {
                    println!("check_name,status,message");
                    for d in &diagnostics {
                        println!("{},{},{}", d.check_name, d.status, d.message.clone().unwrap_or_default());
                    }
                }
            }
            return Ok(());
        }

        println!("Diagnostic Checks:");
        println!("{:<40} {:<12} {}", "CHECK", "STATUS", "MESSAGE");
        println!("{}", "-".repeat(85));

        for d in diagnostics {
            let msg = d.message.unwrap_or_default();
            println!("{:<40} {:<12} {}", d.check_name, d.status, msg);
        }

        Ok(())
    }

    async fn recovery(&self, cmd: &RecoveryCmd) -> Result<()> {
        match cmd {
            RecoveryCmd::Show { action_type, node, status, limit, json, csv } => {
                self.recovery_show(action_type.as_deref(), node.as_deref(), status.as_deref(), *limit, *json, *csv).await
            },
            RecoveryCmd::Execute { action_type, node, dry_run, force, priority } => {
                self.recovery_execute(action_type, node.as_deref(), *dry_run, *force, priority).await
            },
        }
    }

    async fn recovery_show(&self, action_type: Option<&str>, node: Option<&str>, status: Option<&str>, limit: usize, json: bool, csv: bool) -> Result<()> {
        let client = Client::new();
        let mut url = format!("{}/api/v1/recovery?limit={}", self.server, limit);

        if let Some(at) = action_type {
            url.push_str(&format!("&action_type={}", at));
        }
        if let Some(n) = node {
            url.push_str(&format!("&node={}", n));
        }
        if let Some(s) = status {
            url.push_str(&format!("&status={}", s));
        }

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize, Serialize)]
        struct RecoveryActionInfo {
            action_id: String,
            action_type: String,
            node_id: Option<String>,
            status: String,
            created_at: String,
            message: Option<String>,
        }

        let actions: Vec<RecoveryActionInfo> = response.json().await?;

        if json {
            println!("{}", serde_json::to_string_pretty(&actions)?);
            return Ok(());
        }

        if csv {
            println!("action_id,action_type,node_id,status,created_at,message");
            for a in &actions {
                println!("{},{},{},{},{},{}", a.action_id, a.action_type, a.node_id.clone().unwrap_or_default(), a.status, a.created_at, a.message.clone().unwrap_or_default());
            }
            return Ok(());
        }

        println!("Recovery Actions:");
        println!("{:<36} {:<20} {:<15} {:<12} {}", "ACTION ID", "TYPE", "NODE", "STATUS", "CREATED");
        println!("{}", "-".repeat(100));

        for a in actions {
            println!("{:<36} {:<20} {:<15} {:<12} {}", a.action_id, a.action_type, a.node_id.clone().unwrap_or_default(), a.status, a.created_at);
        }

        Ok(())
    }

    async fn recovery_execute(&self, action_type: &str, node: Option<&str>, dry_run: bool, force: bool, priority: &str) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/recovery/execute", self.server);

        let mut body = json!({
            "action_type": action_type,
            "dry_run": dry_run,
            "force": force,
            "priority": priority,
        });

        if let Some(n) = node {
            body["node"] = json!(n);
        }

        let mut request = client.post(&url).json(&body);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize)]
        struct ExecuteResponse {
            action_id: String,
            status: String,
            message: String,
        }

        let result: ExecuteResponse = response.json().await?;

        println!("Action ID: {}", result.action_id);
        println!("Status: {}", result.status);
        println!("Message: {}", result.message);

        Ok(())
    }

    async fn capacity(&self, forecast_days: usize, json: bool) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/api/v1/capacity?forecast_days={}", self.server, forecast_days);

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize, Serialize)]
        struct CapacityResponse {
            total_bytes: u64,
            used_bytes: u64,
            available_bytes: u64,
            usage_percent: f64,
            projections: Option<Vec<CapacityProjectionInfo>>,
        }

        #[derive(Deserialize, Serialize)]
        struct CapacityProjectionInfo {
            date: String,
            projected_used_bytes: u64,
            projected_percent: f64,
        }

        let capacity: CapacityResponse = response.json().await?;

        if json {
            println!("{}", serde_json::to_string_pretty(&capacity)?);
            return Ok(());
        }

        println!("Capacity Usage:");
        println!("  Total: {}", Self::format_bytes(capacity.total_bytes));
        println!("  Used: {}", Self::format_bytes(capacity.used_bytes));
        println!("  Available: {}", Self::format_bytes(capacity.available_bytes));
        println!("  Usage: {:.1}%", capacity.usage_percent);

        if let Some(projections) = capacity.projections {
            if !projections.is_empty() {
                println!("\nCapacity Projections (next {} days):", forecast_days);
                println!("{:<12} {:>15} {:>15}", "DATE", "PROJECTED", "USAGE %");
                println!("{}", "-".repeat(45));

                for p in projections {
                    println!("{:<12} {:>15} {:>15}", p.date, Self::format_bytes(p.projected_used_bytes), format!("{:.1}%", p.projected_percent));
                }
            }
        }

        Ok(())
    }

    async fn alerts(&self, severity: Option<&str>, alert_type: Option<&str>, active: bool, resolved: bool, limit: usize, acknowledge: Option<&str>, silence: Option<&str>, silence_duration: Option<u64>, json: bool) -> Result<()> {
        if let Some(ack_id) = acknowledge {
            let client = Client::new();
            let url = format!("{}/api/v1/alerts/{}/acknowledge", self.server, ack_id);

            let mut request = client.post(&url);
            if let Some(ref token) = self.token {
                request = request.bearer_auth(token);
            }

            let response = request.send().await?;

            if !response.status().is_success() {
                anyhow::bail!("Request failed: {}", response.status());
            }

            #[derive(Deserialize)]
            struct AckResponse {
                alert_id: String,
                status: String,
            }

            let result: AckResponse = response.json().await?;
            println!("Alert {} acknowledged: {}", result.alert_id, result.status);
            return Ok(());
        }

        if let Some(sil_id) = silence {
            let client = Client::new();
            let url = format!("{}/api/v1/alerts/{}/silence", self.server, sil_id);

            let body = json!({
                "duration_secs": silence_duration.unwrap_or(3600),
            });

            let mut request = client.post(&url).json(&body);
            if let Some(ref token) = self.token {
                request = request.bearer_auth(token);
            }

            let response = request.send().await?;

            if !response.status().is_success() {
                anyhow::bail!("Request failed: {}", response.status());
            }

            #[derive(Deserialize)]
            struct SilenceResponse {
                alert_id: String,
                status: String,
                until: String,
            }

            let result: SilenceResponse = response.json().await?;
            println!("Alert {} silenced until: {}", result.alert_id, result.until);
            return Ok(());
        }

        let client = Client::new();
        let mut url = format!("{}/api/v1/alerts?limit={}&active={}&resolved={}", self.server, limit, active, resolved);

        if let Some(s) = severity {
            url.push_str(&format!("&severity={}", s));
        }
        if let Some(at) = alert_type {
            url.push_str(&format!("&alert_type={}", at));
        }

        let mut request = client.get(&url);
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed: {}", response.status());
        }

        #[derive(Deserialize, Serialize)]
        struct AlertInfo {
            alert_id: String,
            severity: String,
            alert_type: String,
            message: String,
            state: String,
            created_at: String,
            acknowledged_at: Option<String>,
            silenced_until: Option<String>,
        }

        let alerts: Vec<AlertInfo> = response.json().await?;

        if json {
            println!("{}", serde_json::to_string_pretty(&alerts)?);
            return Ok(());
        }

        if alerts.is_empty() {
            println!("No alerts found matching the specified filters.");
            return Ok(());
        }

        println!("Alerts:");
        println!("{:<36} {:<10} {:<15} {:<10} {}", "ALERT ID", "SEVERITY", "TYPE", "STATE", "CREATED");
        println!("{}", "-".repeat(100));

        for a in alerts {
            println!("{:<36} {:<10} {:<15} {:<10} {}", a.alert_id, a.severity, a.alert_type, a.state, a.created_at);
            if !a.message.is_empty() {
                println!("  Message: {}", a.message);
            }
        }

        Ok(())
    }

    async fn dashboard(&self, dashboard: &str, time_from: &str, time_to: &str, open: bool, grafana_host: &str) -> Result<()> {
        let encoded_dashboard = urlencoding::encode(dashboard);
        let url = format!("{}/d-solo/_?orgId=1&panelId=1&from={}&to={}&var-dashboard={}",
            grafana_host, time_from, time_to, encoded_dashboard);

        if open {
            let result = std::process::Command::new("xdg-open")
                .arg(&url)
                .spawn();

            match result {
                Ok(_) => println!("Opened dashboard in browser: {}", url),
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        println!("Could not open browser. Dashboard URL: {}", url);
                    } else {
                        anyhow::bail!("Failed to open browser: {}", e);
                    }
                }
            }
        } else {
            println!("Dashboard URL: {}", url);
        }

        Ok(())
    }

    fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TB", bytes as f64 / TB as f64)
        } else if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_status_subcommand() {
        let cli = Cli::parse_from(&["cfs-mgmt", "status"]);
        match cli.command {
            Command::Status => {}
            _ => panic!("Expected Status command"),
        }
    }

    #[test]
    fn test_cli_node_list_subcommand() {
        let cli = Cli::parse_from(&["cfs-mgmt", "node", "list"]);
        match &cli.command {
            Command::Node { cmd } => match cmd {
                NodeCmd::List => {}
                _ => panic!("Expected List command"),
            },
            _ => panic!("Expected Node command"),
        }
    }

    #[test]
    fn test_cli_query_subcommand() {
        let cli = Cli::parse_from(&["cfs-mgmt", "query", "SELECT 1"]);
        match &cli.command {
            Command::Query { sql } => assert_eq!(sql, "SELECT 1"),
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_top_users_subcommand() {
        let cli = Cli::parse_from(&["cfs-mgmt", "top-users", "--limit", "10"]);
        match &cli.command {
            Command::TopUsers { limit } => assert_eq!(*limit, 10),
            _ => panic!("Expected TopUsers command"),
        }
    }

    #[test]
    fn test_cli_find_subcommand() {
        let cli = Cli::parse_from(&["cfs-mgmt", "find", "*.h5"]);
        match &cli.command {
            Command::Find { pattern } => assert_eq!(pattern, "*.h5"),
            _ => panic!("Expected Find command"),
        }
    }

    #[test]
    fn test_cli_stale_subcommand() {
        let cli = Cli::parse_from(&["cfs-mgmt", "stale", "--days", "90"]);
        match &cli.command {
            Command::Stale { days } => assert_eq!(*days, 90),
            _ => panic!("Expected Stale command"),
        }
    }

    #[test]
    fn test_cli_with_server_flag() {
        let cli = Cli::parse_from(&["cfs-mgmt", "--server", "http://custom:9000", "status"]);
        assert_eq!(cli.server, "http://custom:9000");
    }

    #[test]
    fn test_cli_with_token_flag() {
        let cli = Cli::parse_from(&["cfs-mgmt", "--token", "secret", "status"]);
        assert_eq!(cli.token, Some("secret".to_string()));
    }

    #[test]
    fn test_cli_node_drain_parsing() {
        let cli = Cli::parse_from(&["cfs-mgmt", "node", "drain", "node-1"]);
        match &cli.command {
            Command::Node { cmd } => match cmd {
                NodeCmd::Drain { node_id } => assert_eq!(node_id, "node-1"),
                _ => panic!("Expected Drain command"),
            },
            _ => panic!("Expected Node command"),
        }
    }
}