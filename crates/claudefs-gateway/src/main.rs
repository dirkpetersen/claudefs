#![warn(missing_docs)]

//! ClaudeFS gateway server binary — serves NFS v3/pNFS, S3-compatible API, and SMB via Samba VFS.

use claudefs_gateway::config::GatewayConfig;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Parse command-line arguments into a GatewayConfig.
fn parse_args() -> GatewayConfig {
    let mut args = std::env::args().skip(1);
    
    let mut export_path = "/".to_string();
    let mut nfs_port: Option<u16> = None;
    let mut s3_port: Option<u16> = None;
    let mut log_level = "info".to_string();
    
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--export" => {
                if let Some(path) = args.next() {
                    export_path = path;
                }
            }
            "--nfs-port" => {
                if let Some(port_str) = args.next() {
                    if let Ok(port) = port_str.parse() {
                        nfs_port = Some(port);
                    }
                }
            }
            "--s3-port" => {
                if let Some(port_str) = args.next() {
                    if let Ok(port) = port_str.parse() {
                        s3_port = Some(port);
                    }
                }
            }
            "--log-level" => {
                if let Some(level) = args.next() {
                    log_level = level;
                }
            }
            "--help" | "-h" => {
                println!("ClaudeFS Gateway Server");
                println!();
                println!("Usage: cfs-gateway [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --export PATH      Export path (default: /)");
                println!("  --nfs-port PORT    NFS server port (default: 2049)");
                println!("  --s3-port PORT     S3 API server port (default: 9000)");
                println!("  --log-level LEVEL  Log level: trace, debug, info, warn, error (default: info)");
                println!("  --help, -h         Show this help message");
                std::process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}. Use --help for usage.", arg);
                std::process::exit(1);
            }
        }
    }
    
    let mut config = GatewayConfig::default_with_export(&export_path);
    config.log_level = log_level;
    
    if let Some(port) = nfs_port {
        config.nfs.bind.port = port;
    }
    if let Some(port) = s3_port {
        config.s3.bind.port = port;
    }
    
    config
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = parse_args();

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive(
            config.log_level.parse().unwrap_or(tracing::Level::INFO).into()
        ))
        .init();

    tracing::info!(
        nfs_enabled = config.enable_nfs,
        s3_enabled = config.enable_s3,
        "ClaudeFS gateway server starting"
    );

    if config.enable_nfs {
        tracing::info!(
            port = config.nfs.bind.port,
            mount_port = config.nfs.mount_bind.port,
            "NFS v3 gateway enabled"
        );
        for export in &config.nfs.exports {
            tracing::info!(path = %export.path, "Export configured");
        }
    }

    if config.enable_s3 {
        tracing::info!(
            port = config.s3.bind.port,
            region = %config.s3.region,
            "S3-compatible API enabled"
        );
    }

    config.validate()?;

    tracing::info!("Gateway configuration valid, ready to serve (stub mode)");
    tracing::info!("Gateway server initialized (stub — no network listeners in Phase 4)");

    Ok(())
}