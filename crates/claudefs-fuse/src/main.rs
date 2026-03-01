#![warn(missing_docs)]
//! ClaudeFS FUSE mount daemon

use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    tracing::info!("ClaudeFS FUSE daemon starting...");

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cfs-fuse <mountpoint>");
        std::process::exit(1);
    }

    let mountpoint = PathBuf::from(&args[1]);
    tracing::info!("Mount point: {}", mountpoint.display());

    Ok(())
}