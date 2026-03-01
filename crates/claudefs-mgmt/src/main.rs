#![warn(missing_docs)]

use anyhow::Result;
use clap::Parser;
use claudefs_mgmt::cli::Cli;
use claudefs_mgmt::{AdminApi, AlertManager, ClusterMetrics, MetadataIndexer, MgmtConfig, ScraperPool};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    tracing::info!("ClaudeFS management CLI starting...");

    let cli = Cli::parse();
    
    match &cli.command {
        claudefs_mgmt::cli::Command::Serve { config } => {
            let config_path = config.clone();
            serve_with_modules(config_path).await
        }
        _ => cli.run().await,
    }
}

async fn serve_with_modules(config_path: PathBuf) -> Result<()> {
    let config = if config_path.exists() {
        MgmtConfig::from_file(&config_path)?
    } else {
        tracing::warn!("Config file not found, using defaults: {}", config_path.display());
        MgmtConfig::default()
    };

    let metrics = Arc::new(ClusterMetrics::new());
    let config = Arc::new(config);
    let indexer = Arc::new(MetadataIndexer::new(
        config.index_dir.clone(),
        config.parquet_flush_interval_secs,
    ));
    
    let mut scraper_pool = ScraperPool::new(config.scrape_interval_secs);
    for (i, addr) in config.node_addrs.iter().enumerate() {
        let node_id = format!("node{}", i + 1);
        let url = format!("http://{}/metrics", addr);
        scraper_pool.add_node(node_id, url);
    }
    let scraper_pool = Arc::new(scraper_pool);
    
    let alert_manager = Arc::new(tokio::sync::Mutex::new(AlertManager::with_default_rules()));
    
    let api = AdminApi::new(metrics.clone(), config.clone());

    let indexer_clone = indexer.clone();
    let indexer_handle = tokio::spawn(async move {
        if let Err(e) = indexer_clone.run_flush_loop().await {
            tracing::error!("Indexer flush loop error: {}", e);
        }
    });

    let scraper_pool_clone = scraper_pool.clone();
    let alert_manager_clone = alert_manager.clone();
    let metrics_clone = metrics.clone();
    let scrape_handle = tokio::spawn(async move {
        loop {
            let results = scraper_pool_clone.scrape_all().await;
            
            let mut alerts = alert_manager_clone.lock().await;
            let mut metrics_map = std::collections::HashMap::new();
            for result in results {
                for (name, value) in result.samples {
                    metrics_map.insert(name, value);
                }
            }
            if !metrics_map.is_empty() {
                alerts.evaluate(&metrics_map);
                let firing = alerts.firing_alerts();
                for alert in firing {
                    if alert.age_secs() >= alert.rule.for_secs {
                        tracing::warn!("Alert firing: {} - {}", alert.rule.name, alert.message);
                    }
                }
            }
            
            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
        }
    });

    let api_handle = tokio::spawn(async move {
        if let Err(e) = api.serve().await {
            tracing::error!("API serve error: {}", e);
        }
    });

    tokio::select! {
        _ = indexer_handle => {}
        _ = scrape_handle => {}
        _ = api_handle => {}
    }

    Ok(())
}