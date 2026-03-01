#![warn(missing_docs)]

use anyhow::Result;
use clap::Parser;
use claudefs_mgmt::cli::Cli;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    tracing::info!("ClaudeFS management CLI starting...");

    let cli = Cli::parse();
    cli.run().await
}