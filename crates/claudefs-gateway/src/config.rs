//! Gateway configuration

use serde::{Deserialize, Serialize};

use crate::error::{GatewayError, Result};
use crate::mount::ExportEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindAddr {
    pub addr: String,
    pub port: u16,
}

impl BindAddr {
    pub fn new(addr: &str, port: u16) -> Self {
        Self {
            addr: addr.to_string(),
            port,
        }
    }

    pub fn nfs_default() -> Self {
        Self {
            addr: "0.0.0.0".to_string(),
            port: 2049,
        }
    }

    pub fn mount_default() -> Self {
        Self {
            addr: "0.0.0.0".to_string(),
            port: 20048,
        }
    }

    pub fn s3_default() -> Self {
        Self {
            addr: "0.0.0.0".to_string(),
            port: 9000,
        }
    }

    pub fn to_socket_addr_string(&self) -> String {
        format!("{}:{}", self.addr, self.port)
    }
}

impl Default for BindAddr {
    fn default() -> Self {
        Self::nfs_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportConfig {
    pub path: String,
    pub allowed_clients: Vec<String>,
    pub read_only: bool,
    pub root_squash: bool,
    pub anon_uid: u32,
    pub anon_gid: u32,
}

impl ExportConfig {
    pub fn default_rw(path: &str) -> Self {
        Self {
            path: path.to_string(),
            allowed_clients: vec![],
            read_only: false,
            root_squash: true,
            anon_uid: 65534,
            anon_gid: 65534,
        }
    }

    pub fn default_ro(path: &str) -> Self {
        Self {
            path: path.to_string(),
            allowed_clients: vec![],
            read_only: true,
            root_squash: true,
            anon_uid: 65534,
            anon_gid: 65534,
        }
    }

    pub fn to_export_entry(&self) -> ExportEntry {
        ExportEntry {
            dirpath: self.path.clone(),
            groups: self.allowed_clients.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    pub bind: BindAddr,
    pub region: String,
    pub max_object_size: u64,
    pub multipart_chunk_min: u64,
    pub enable_versioning: bool,
}

impl S3Config {
    pub fn new() -> Self {
        Self {
            bind: BindAddr::s3_default(),
            region: "us-east-1".to_string(),
            max_object_size: 5_000_000_000_000u64,
            multipart_chunk_min: 5_242_880,
            enable_versioning: false,
        }
    }
}

impl Default for S3Config {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfsConfig {
    pub bind: BindAddr,
    pub mount_bind: BindAddr,
    pub exports: Vec<ExportConfig>,
    pub fsid: u64,
    pub max_read_size: u32,
    pub max_write_size: u32,
    pub enable_pnfs: bool,
    pub pnfs_data_servers: Vec<String>,
}

impl NfsConfig {
    pub fn default_with_export(path: &str) -> Self {
        Self {
            bind: BindAddr::nfs_default(),
            mount_bind: BindAddr::mount_default(),
            exports: vec![ExportConfig::default_rw(path)],
            fsid: 1,
            max_read_size: 1_048_576,
            max_write_size: 1_048_576,
            enable_pnfs: false,
            pnfs_data_servers: vec![],
        }
    }
}

impl Default for NfsConfig {
    fn default() -> Self {
        Self::default_with_export("/export")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub nfs: NfsConfig,
    pub s3: S3Config,
    pub enable_nfs: bool,
    pub enable_s3: bool,
    pub enable_smb: bool,
    pub log_level: String,
}

impl GatewayConfig {
    pub fn default_with_export(path: &str) -> Self {
        Self {
            nfs: NfsConfig::default_with_export(path),
            s3: S3Config::new(),
            enable_nfs: true,
            enable_s3: true,
            enable_smb: false,
            log_level: "info".to_string(),
        }
    }

    pub fn any_enabled(&self) -> bool {
        self.enable_nfs || self.enable_s3 || self.enable_smb
    }

    pub fn validate(&self) -> Result<()> {
        if !self.any_enabled() {
            return Err(GatewayError::ProtocolError {
                reason: "at least one gateway protocol must be enabled".to_string(),
            });
        }

        if self.nfs.bind.port == self.s3.bind.port && self.enable_nfs && self.enable_s3 {
            return Err(GatewayError::ProtocolError {
                reason: "NFS and S3 cannot use the same port".to_string(),
            });
        }

        if self.nfs.bind.port == self.nfs.mount_bind.port {
            return Err(GatewayError::ProtocolError {
                reason: "NFS and MOUNT cannot use the same port".to_string(),
            });
        }

        if self.nfs.exports.is_empty() {
            return Err(GatewayError::ProtocolError {
                reason: "at least one export must be configured".to_string(),
            });
        }

        for export in &self.nfs.exports {
            if export.path.is_empty() {
                return Err(GatewayError::ProtocolError {
                    reason: "export path cannot be empty".to_string(),
                });
            }
        }

        if self.nfs.enable_pnfs && self.nfs.pnfs_data_servers.is_empty() {
            return Err(GatewayError::ProtocolError {
                reason: "pNFS enabled but no data servers configured".to_string(),
            });
        }

        Ok(())
    }
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self::default_with_export("/export")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_addr_new() {
        let addr = BindAddr::new("192.168.1.1", 8080);
        assert_eq!(addr.addr, "192.168.1.1");
        assert_eq!(addr.port, 8080);
    }

    #[test]
    fn test_bind_addr_nfs_default() {
        let addr = BindAddr::nfs_default();
        assert_eq!(addr.addr, "0.0.0.0");
        assert_eq!(addr.port, 2049);
    }

    #[test]
    fn test_bind_addr_mount_default() {
        let addr = BindAddr::mount_default();
        assert_eq!(addr.addr, "0.0.0.0");
        assert_eq!(addr.port, 20048);
    }

    #[test]
    fn test_bind_addr_s3_default() {
        let addr = BindAddr::s3_default();
        assert_eq!(addr.addr, "0.0.0.0");
        assert_eq!(addr.port, 9000);
    }

    #[test]
    fn test_bind_addr_to_socket_addr_string() {
        let addr = BindAddr::new("192.168.1.1", 8080);
        assert_eq!(addr.to_socket_addr_string(), "192.168.1.1:8080");
    }

    #[test]
    fn test_export_config_default_rw() {
        let export = ExportConfig::default_rw("/data");
        assert_eq!(export.path, "/data");
        assert!(!export.read_only);
        assert!(export.root_squash);
        assert_eq!(export.anon_uid, 65534);
    }

    #[test]
    fn test_export_config_default_ro() {
        let export = ExportConfig::default_ro("/data");
        assert_eq!(export.path, "/data");
        assert!(export.read_only);
    }

    #[test]
    fn test_export_config_to_export_entry() {
        let export = ExportConfig {
            path: "/test".to_string(),
            allowed_clients: vec!["client1".to_string()],
            read_only: false,
            root_squash: true,
            anon_uid: 1000,
            anon_gid: 1000,
        };
        let entry = export.to_export_entry();
        assert_eq!(entry.dirpath, "/test");
        assert_eq!(entry.groups, vec!["client1"]);
    }

    #[test]
    fn test_s3_config_default() {
        let config = S3Config::default();
        assert_eq!(config.bind.port, 9000);
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.max_object_size, 5_000_000_000_000u64);
    }

    #[test]
    fn test_nfs_config_default_with_export() {
        let config = NfsConfig::default_with_export("/data");
        assert_eq!(config.bind.port, 2049);
        assert_eq!(config.mount_bind.port, 20048);
        assert_eq!(config.exports.len(), 1);
        assert_eq!(config.exports[0].path, "/data");
    }

    #[test]
    fn test_gateway_config_default() {
        let config = GatewayConfig::default_with_export("/data");
        assert!(config.enable_nfs);
        assert!(config.enable_s3);
        assert!(!config.enable_smb);
    }

    #[test]
    fn test_gateway_config_any_enabled() {
        let mut config = GatewayConfig::default_with_export("/data");
        config.enable_nfs = false;
        config.enable_s3 = false;
        config.enable_smb = false;
        assert!(!config.any_enabled());

        config.enable_nfs = true;
        assert!(config.any_enabled());
    }

    #[test]
    fn test_gateway_config_validate_ok() {
        let config = GatewayConfig::default_with_export("/data");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_gateway_config_validate_no_protocols() {
        let mut config = GatewayConfig::default_with_export("/data");
        config.enable_nfs = false;
        config.enable_s3 = false;
        config.enable_smb = false;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_gateway_config_validate_port_conflict() {
        let mut config = GatewayConfig::default_with_export("/data");
        config.nfs.bind.port = 9000;
        config.s3.bind.port = 9000;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_gateway_config_validate_nfs_mount_port_conflict() {
        let mut config = GatewayConfig::default_with_export("/data");
        config.nfs.mount_bind.port = config.nfs.bind.port;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_gateway_config_validate_empty_exports() {
        let mut config = GatewayConfig::default_with_export("/data");
        config.nfs.exports = vec![];
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_gateway_config_validate_empty_export_path() {
        let mut config = GatewayConfig::default_with_export("/data");
        config.nfs.exports[0].path = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_gateway_config_validate_pnfs_no_data_servers() {
        let mut config = GatewayConfig::default_with_export("/data");
        config.nfs.enable_pnfs = true;
        config.nfs.pnfs_data_servers = vec![];
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_gateway_config_validate_pnfs_ok() {
        let mut config = GatewayConfig::default_with_export("/data");
        config.nfs.enable_pnfs = true;
        config.nfs.pnfs_data_servers = vec!["192.168.1.1:20020".to_string()];
        assert!(config.validate().is_ok());
    }
}
