use crate::config::MgmtConfig;
use crate::metrics::ClusterMetrics;
use crate::api::AdminApi;
use anyhow::Result;
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;
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

        let api = AdminApi::new(metrics, config);
        api.serve().await
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