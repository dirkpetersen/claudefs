# OpenCode Task: Enhance claudefs-repl main.rs with proper daemon CLI

## Context

You are implementing the main binary for the `claudefs-repl` crate — the cross-site journal replication subsystem for ClaudeFS, a distributed POSIX filesystem.

The crate already has a complete library implementation (35 modules, 742 tests), but the binary entry point `src/main.rs` is nearly empty. Your task is to replace it with a proper replication daemon binary.

## Current main.rs (to replace)

```rust
#![warn(missing_docs)]

//! ClaudeFS replication server

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    tracing::info!("ClaudeFS replication server starting...");

    Ok(())
}
```

## Available Cargo.toml dependencies (already in Cargo.toml, DO NOT modify Cargo.toml)

```toml
[dependencies]
tokio = { version = "1.40", features = ["full"] }  # includes signal support
thiserror = "1.0"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
bincode = "1.3"
prost = "0.13"
tonic = "0.12"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt", "json", "env-filter"] }
lz4_flex = { version = "0.11", features = ["frame"] }
zstd = "0.13"
rand = "0.8"
bytes = "1"
```

**IMPORTANT**: Do NOT add any new dependencies. Use only what's already in Cargo.toml. Do not use clap. Parse arguments manually from `std::env::args()`.

## Library API Available

The library exports these key types (already implemented in src/):

```rust
// From src/engine.rs
pub struct ReplicationEngine {
    pub fn new(config: EngineConfig, topology: ReplicationTopology) -> Self;
    pub async fn start(&self);
    pub async fn stop(&self);
    pub async fn state(&self) -> EngineState;
    pub async fn add_site(&self, info: SiteInfo);
    pub async fn all_site_stats(&self) -> Vec<SiteReplicationStats>;
}

pub struct EngineConfig {
    pub local_site_id: u64,
    pub max_batch_size: usize,  // default: 1000
    pub batch_timeout_ms: u64,  // default: 100
    pub compact_before_send: bool,  // default: true
    pub max_concurrent_sends: usize,  // default: 4
}

// From src/topology.rs
pub struct ReplicationTopology { ... }
impl ReplicationTopology {
    pub fn new(local_site_id: u64) -> Self;
}

pub struct SiteInfo {
    pub site_id: u64,
    pub region: String,
    pub endpoints: Vec<String>,
    pub role: ReplicationRole,
}
impl SiteInfo {
    pub fn new(site_id: u64, region: String, endpoints: Vec<String>, role: ReplicationRole) -> Self;
}

pub enum ReplicationRole {
    Primary,
    Replica { primary_site_id: u64 },
}

// From src/health.rs
// health and metrics tracking

// From src/lag_monitor.rs
pub struct LagMonitor { ... }
```

## Requirements for the new main.rs

Implement a daemon binary that:

### 1. Command-Line Argument Parsing (no clap, use std::env::args)

Support these flags using a simple manual parser over `std::env::args()`:
- `--site-id <N>` — local site ID (u64, required)
- `--peer <id>:<region>:<endpoint>` — add a remote peer (can be specified multiple times), e.g. `--peer 2:us-west-2:grpc://10.0.0.2:50051`
- `--batch-size <N>` — max entries per batch (default: 1000)
- `--batch-timeout-ms <N>` — batch window ms (default: 100)
- `--status-interval-s <N>` — how often to log replication status (default: 30)
- `--help` / `-h` — print usage and exit

### 2. Configuration Structure

Define a `Config` struct that holds all CLI-parsed settings:
```rust
struct Config {
    site_id: u64,
    peers: Vec<PeerSpec>,
    batch_size: usize,
    batch_timeout_ms: u64,
    status_interval_s: u64,
}

struct PeerSpec {
    site_id: u64,
    region: String,
    endpoint: String,
}
```

Parse `--peer 2:us-west-2:grpc://10.0.0.2:50051` by splitting on `:` with exactly 3 parts: `<id>:<region>:<endpoint>`. The endpoint may itself contain colons (grpc://host:port), so split only on the first two colons.

### 3. Startup Sequence

```
1. Parse args → Config (exit with usage if --help or missing required args)
2. Initialize tracing (JSON format if RUST_LOG_JSON=1, else pretty)
3. Create ReplicationEngine with EngineConfig and ReplicationTopology
4. Register all peers with engine.add_site(...)
5. engine.start().await
6. Log "Replication engine started. Local site_id={}, peers={N}"
7. Spawn a status task (periodic logging, see below)
8. Wait for SIGTERM or SIGINT (tokio::signal::unix or ctrl_c)
9. On signal: log "Shutting down replication engine", engine.stop().await
10. Log "Replication engine stopped cleanly"
```

### 4. Status Task

Spawn a `tokio::spawn` background task that every `status_interval_s` seconds:
1. Calls `engine.all_site_stats().await`
2. Logs one tracing::info! per site with: `site_id`, `entries_sent`, `entries_received`, `batches_sent`, `current_lag_entries`, `conflicts_detected`

### 5. Error Handling and Usage

- If `--site-id` is missing, print usage to stderr and `std::process::exit(1)`
- If `--peer` has wrong format, print error to stderr and exit(1)
- If arg parsing fails for numeric values, print error and exit(1)

Print usage like:
```
Usage: cfs-repl --site-id <N> [--peer <id>:<region>:<endpoint>] ...

Options:
  --site-id <N>               Local site ID (required)
  --peer <id>:<region>:<url>  Remote peer (repeatable)
  --batch-size <N>            Max entries per batch (default: 1000)
  --batch-timeout-ms <N>      Batch window in ms (default: 100)
  --status-interval-s <N>     Status log interval in s (default: 30)
  --help, -h                  Show this help
```

### 6. Signal Handling

Use `tokio::signal::ctrl_c()` for cross-platform signal handling. On Linux, also handle SIGTERM:
```rust
#[cfg(unix)]
{
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
    }
}
#[cfg(not(unix))]
{
    tokio::signal::ctrl_c().await?;
}
```

## Code Quality Requirements

1. ALL public items must have `///` doc comments (the crate has `#![warn(missing_docs)]`)
2. No compiler warnings — use `#[allow(dead_code)]` only if truly needed
3. No clippy warnings
4. Use `tracing::{info, warn, error}` for all logging (not println!)
5. Use `anyhow::Result<()>` as the return type for main
6. Follow Rust naming conventions (snake_case functions, CamelCase types)
7. Keep it clean and focused — no unnecessary abstractions

## Important: Library Imports

The binary is in the same crate, so import from `claudefs_repl::`:
```rust
use claudefs_repl::engine::{EngineConfig, ReplicationEngine};
use claudefs_repl::topology::{ReplicationRole, ReplicationTopology, SiteInfo};
```

## File to Write

Write the complete replacement for: `crates/claudefs-repl/src/main.rs`

Show the complete file content with a clear header comment:
```
// File: crates/claudefs-repl/src/main.rs
```

Make it production-quality. The binary name is `cfs-repl` (from Cargo.toml `[[bin]]` section).
