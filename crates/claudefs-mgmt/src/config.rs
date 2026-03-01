use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MgmtConfig {
    pub bind_addr: SocketAddr,
    pub index_dir: PathBuf,
    pub duckdb_path: String,
    pub scrape_interval_secs: u64,
    pub parquet_flush_interval_secs: u64,
    pub node_addrs: Vec<String>,
    pub admin_token: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

impl Default for MgmtConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 8443)),
            index_dir: PathBuf::from("/var/lib/claudefs/index"),
            duckdb_path: String::from(":memory:"),
            scrape_interval_secs: 15,
            parquet_flush_interval_secs: 60,
            node_addrs: Vec::new(),
            admin_token: None,
            tls_cert: None,
            tls_key: None,
        }
    }
}

impl MgmtConfig {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();

        match ext.to_lowercase().as_str() {
            "toml" => {
                let config: MgmtConfig = toml::from_str(&contents)?;
                Ok(config)
            }
            "json" => {
                let config: MgmtConfig = serde_json::from_str(&contents)?;
                Ok(config)
            }
            _ => anyhow::bail!("Unsupported config file extension: {}", ext),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_values() {
        let config = MgmtConfig::default();
        assert_eq!(config.bind_addr, SocketAddr::from(([0, 0, 0, 0], 8443)));
        assert_eq!(config.index_dir, PathBuf::from("/var/lib/claudefs/index"));
        assert_eq!(config.duckdb_path, ":memory:");
        assert_eq!(config.scrape_interval_secs, 15);
        assert_eq!(config.parquet_flush_interval_secs, 60);
        assert!(config.node_addrs.is_empty());
        assert!(config.admin_token.is_none());
    }

    #[test]
    fn test_serialization_round_trip() {
        let config = MgmtConfig {
            bind_addr: SocketAddr::from(([192, 168, 1, 1], 9000)),
            index_dir: PathBuf::from("/custom/index"),
            duckdb_path: String::from("/tmp/analytics.db"),
            scrape_interval_secs: 30,
            parquet_flush_interval_secs: 120,
            node_addrs: vec![String::from("node1:9400"), String::from("node2:9400")],
            admin_token: Some(String::from("secret-token")),
            tls_cert: Some(PathBuf::from("/etc/certs/server.crt")),
            tls_key: Some(PathBuf::from("/etc/certs/server.key")),
        };

        let json = serde_json::to_string(&config).unwrap();
        let decoded: MgmtConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.bind_addr, decoded.bind_addr);
        assert_eq!(config.index_dir, decoded.index_dir);
        assert_eq!(config.duckdb_path, decoded.duckdb_path);
        assert_eq!(config.scrape_interval_secs, decoded.scrape_interval_secs);
        assert_eq!(
            config.parquet_flush_interval_secs,
            decoded.parquet_flush_interval_secs
        );
        assert_eq!(config.node_addrs, decoded.node_addrs);
        assert_eq!(config.admin_token, decoded.admin_token);
    }

    #[test]
    fn test_from_file_json() {
        let mut file = NamedTempFile::with_suffix(".json").unwrap();
        writeln!(
            file,
            r#"{{
                "bind_addr": "127.0.0.1:9000",
                "index_dir": "/test/index",
                "duckdb_path": "/test/db",
                "scrape_interval_secs": 20,
                "parquet_flush_interval_secs": 90,
                "node_addrs": ["node1:9400"]
            }}"#
        )
        .unwrap();

        let config = MgmtConfig::from_file(file.path()).unwrap();
        assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 9000)));
        assert_eq!(config.index_dir, PathBuf::from("/test/index"));
        assert_eq!(config.duckdb_path, "/test/db");
        assert_eq!(config.scrape_interval_secs, 20);
        assert_eq!(config.parquet_flush_interval_secs, 90);
        assert_eq!(config.node_addrs, vec!["node1:9400"]);
    }

    #[test]
    fn test_from_file_toml() {
        let mut file = NamedTempFile::with_suffix(".toml").unwrap();
        writeln!(
            file,
            r#"
bind_addr = "10.0.0.1:8080"
index_dir = "/toml/index"
duckdb_path = ":memory:"
scrape_interval_secs = 10
parquet_flush_interval_secs = 45
node_addrs = ["node1:9400", "node2:9400"]
admin_token = "test-token"
            "#
        )
        .unwrap();

        let config = MgmtConfig::from_file(file.path()).unwrap();
        assert_eq!(config.bind_addr, SocketAddr::from(([10, 0, 0, 1], 8080)));
        assert_eq!(config.index_dir, PathBuf::from("/toml/index"));
        assert_eq!(config.duckdb_path, ":memory:");
        assert_eq!(config.scrape_interval_secs, 10);
        assert_eq!(config.parquet_flush_interval_secs, 45);
        assert_eq!(
            config.node_addrs,
            vec!["node1:9400".to_string(), "node2:9400".to_string()]
        );
        assert_eq!(config.admin_token, Some("test-token".to_string()));
    }

    #[test]
    fn test_node_addr_parsing() {
        let config = MgmtConfig {
            node_addrs: vec![
                String::from("192.168.1.10:9400"),
                String::from("node2.internal:9400"),
                String::from("[::1]:9400"),
            ],
            ..MgmtConfig::default()
        };

        assert_eq!(config.node_addrs.len(), 3);
        assert!(config.node_addrs[0].contains("192.168.1.10"));
        assert!(config.node_addrs[1].contains("node2.internal"));
    }
}
