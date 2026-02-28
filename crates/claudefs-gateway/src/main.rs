#![warn(missing_docs)]

//! ClaudeFS gateway server (NFS/pNFS/S3/SMB)

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    tracing::info!("ClaudeFS gateway server starting...");

    Ok(())
}