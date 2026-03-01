#![warn(missing_docs)]
//! ClaudeFS FUSE mount daemon
//!
//! Usage: cfs-fuse <mountpoint> [options]

use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn print_usage(prog: &str) {
    eprintln!("Usage: {} <mountpoint> [options]", prog);
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --allow-other      Allow other users to access the mount");
    eprintln!("  --ro               Mount read-only");
    eprintln!("  --direct-io        Bypass page cache for I/O");
    eprintln!("  -h, --help         Show this help message");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = std::env::args().collect();
    let prog = args.first().map(|s| s.as_str()).unwrap_or("cfs-fuse");

    if args.len() < 2 || args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage(prog);
        if args.iter().any(|a| a == "-h" || a == "--help") {
            return Ok(());
        }
        std::process::exit(1);
    }

    let mountpoint = PathBuf::from(&args[1]);
    let allow_other = args.iter().any(|a| a == "--allow-other");
    let ro = args.iter().any(|a| a == "--ro");
    let direct_io = args.iter().any(|a| a == "--direct-io");

    tracing::info!(
        mountpoint = %mountpoint.display(),
        allow_other,
        ro,
        direct_io,
        "ClaudeFS FUSE daemon starting"
    );

    if !mountpoint.exists() {
        anyhow::bail!("Mount point does not exist: {}", mountpoint.display());
    }
    if !mountpoint.is_dir() {
        anyhow::bail!("Mount point is not a directory: {}", mountpoint.display());
    }

    tracing::info!("Mount configuration validated, ready to mount at {}", mountpoint.display());
    tracing::info!("Note: actual FUSE mount requires claudefs-meta (A2) and claudefs-transport (A4) integration");

    Ok(())
}