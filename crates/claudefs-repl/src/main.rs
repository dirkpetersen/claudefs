#![warn(missing_docs)]

//! ClaudeFS replication daemon binary.
//!
//! This binary runs the cross-site journal replication engine. It connects to
//! remote peer sites and replicates metadata changes asynchronously.

use claudefs_repl::engine::{EngineConfig, ReplicationEngine, SiteReplicationStats};
use claudefs_repl::topology::{ReplicationRole, ReplicationTopology, SiteInfo};
use std::env;
use std::process;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Configuration parsed from command-line arguments.
struct Config {
    site_id: u64,
    peers: Vec<PeerSpec>,
    batch_size: usize,
    batch_timeout_ms: u64,
    status_interval_s: u64,
}

/// A remote peer specification parsed from CLI.
struct PeerSpec {
    site_id: u64,
    region: String,
    endpoint: String,
}

/// Prints usage information to stderr and exits.
fn print_usage() {
    eprintln!(
        "Usage: cfs-repl --site-id <N> [--peer <id>:<region>:<endpoint>] ...\n\
        \n\
        Options:\n\
        --site-id <N>               Local site ID (required)\n\
        --peer <id>:<region>:<url>  Remote peer (repeatable)\n\
        --batch-size <N>            Max entries per batch (default: 1000)\n\
        --batch-timeout-ms <N>      Batch window in ms (default: 100)\n\
        --status-interval-s <N>     Status log interval in s (default: 30)\n\
        --help, -h                  Show this help"
    );
    process::exit(1);
}

/// Prints an error message to stderr and exits.
fn print_error(msg: &str) {
    eprintln!("Error: {}", msg);
    process::exit(1);
}

/// Parses a peer string in format "id:region:endpoint".
/// The endpoint may contain colons (e.g., grpc://host:port), so we split on first two colons.
/// This function never returns on error - it calls process::exit(1).
#[allow(clippy::unnecessary_unwrap)]
fn parse_peer(s: &str) -> PeerSpec {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 3 {
        eprintln!("Error: failed to parse peer '{}': expected at least 3 parts", s);
        process::exit(1);
    }

    let site_id = match parts[0].parse::<u64>() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("Error: failed to parse peer '{}': invalid site_id", s);
            process::exit(1);
        }
    };

    let region = parts[1];

    let endpoint = parts[2..].join(":");
    if endpoint.is_empty() {
        eprintln!("Error: failed to parse peer '{}': missing endpoint", s);
        process::exit(1);
    }

    PeerSpec {
        site_id,
        region: region.to_string(),
        endpoint,
    }
}

/// Parses command-line arguments into a Config struct.
fn parse_args() -> Config {
    let args: Vec<String> = env::args().collect();

    let mut site_id: Option<u64> = None;
    let mut peers: Vec<PeerSpec> = Vec::new();
    let mut batch_size: usize = 1000;
    let mut batch_timeout_ms: u64 = 100;
    let mut status_interval_s: u64 = 30;
    let mut show_help = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                show_help = true;
            }
            "--site-id" => {
                if i + 1 >= args.len() {
                    print_error("--site-id requires a value");
                }
                match args[i + 1].parse::<u64>() {
                    Ok(v) => site_id = Some(v),
                    Err(_) => print_error("--site-id must be a number"),
                }
                i += 1;
            }
            "--peer" => {
                if i + 1 >= args.len() {
                    print_error("--peer requires a value");
                }
                peers.push(parse_peer(&args[i + 1]));
                i += 1;
            }
            "--batch-size" => {
                if i + 1 >= args.len() {
                    print_error("--batch-size requires a value");
                }
                match args[i + 1].parse::<usize>() {
                    Ok(v) => batch_size = v,
                    Err(_) => print_error("--batch-size must be a number"),
                }
                i += 1;
            }
            "--batch-timeout-ms" => {
                if i + 1 >= args.len() {
                    print_error("--batch-timeout-ms requires a value");
                }
                match args[i + 1].parse::<u64>() {
                    Ok(v) => batch_timeout_ms = v,
                    Err(_) => print_error("--batch-timeout-ms must be a number"),
                }
                i += 1;
            }
            "--status-interval-s" => {
                if i + 1 >= args.len() {
                    print_error("--status-interval-s requires a value");
                }
                match args[i + 1].parse::<u64>() {
                    Ok(v) => status_interval_s = v,
                    Err(_) => print_error("--status-interval-s must be a number"),
                }
                i += 1;
            }
            _ => {
                print_error(&format!("unknown argument: {}", args[i]));
            }
        }
        i += 1;
    }

    if show_help {
        print_usage();
    }

    let site_id = match site_id {
        Some(v) => v,
        None => {
            eprintln!("Error: --site-id is required");
            process::exit(1);
        }
    };

    Config {
        site_id,
        peers,
        batch_size,
        batch_timeout_ms,
        status_interval_s,
    }
}

/// Initializes tracing subscriber based on environment.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if std::env::var("RUST_LOG_JSON").is_ok() {
        tracing_subscriber::registry()
            .with(fmt::layer().json())
            .with(filter)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(fmt::layer().pretty())
            .with(filter)
            .init();
    }
}

/// Logs replication stats for a single site.
fn log_site_stats(stats: &SiteReplicationStats) {
    tracing::info!(
        site_id = stats.remote_site_id,
        entries_sent = stats.entries_sent,
        entries_received = stats.entries_received,
        batches_sent = stats.batches_sent,
        current_lag_entries = stats.current_lag_entries,
        conflicts_detected = stats.conflicts_detected,
        "Replication status"
    );
}

/// Spawns the background status logging task.
fn spawn_status_task(engine: Arc<Mutex<ReplicationEngine>>, interval_s: u64) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_s));
        loop {
            interval.tick().await;
            let engine = engine.lock().await;
            let stats = engine.all_site_stats().await;
            drop(engine);
            for s in &stats {
                log_site_stats(s);
            }
            if stats.is_empty() {
                tracing::debug!("No peer sites configured yet");
            }
        }
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = parse_args();

    tracing::info!(
        site_id = config.site_id,
        peers = config.peers.len(),
        "Starting ClaudeFS replication server"
    );

    let engine_config = EngineConfig {
        local_site_id: config.site_id,
        max_batch_size: config.batch_size,
        batch_timeout_ms: config.batch_timeout_ms,
        compact_before_send: true,
        max_concurrent_sends: 4,
    };

    let topology = ReplicationTopology::new(config.site_id);
    let engine = ReplicationEngine::new(engine_config, topology);

    for peer in &config.peers {
        let site_info = SiteInfo::new(
            peer.site_id,
            peer.region.clone(),
            vec![peer.endpoint.clone()],
            ReplicationRole::Primary,
        );
        engine.add_site(site_info).await;
    }

    engine.start().await;

    tracing::info!(
        local_site_id = config.site_id,
        peers = config.peers.len(),
        "Replication engine started"
    );

    let engine = Arc::new(Mutex::new(engine));
    let engine_for_task = Arc::clone(&engine);
    let status_handle = spawn_status_task(engine_for_task, config.status_interval_s);

    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received Ctrl+C, shutting down");
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, shutting down");
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        tracing::info!("Received Ctrl+C, shutting down");
    }

    tracing::info!("Shutting down replication engine");
    
    let engine = engine.lock().await;
    engine.stop().await;
    drop(engine);
    
    status_handle.abort();

    tracing::info!("Replication engine stopped cleanly");

    Ok(())
}