# A8: Management Subsystem — Complete DuckDB Analytics & Prometheus Export

## Context

**Agent:** A8 (Management) | **Crate:** `claudefs-mgmt` | **Phase:** 1-2 Integration
**Status:** Crate structure complete (~21k lines) with 38 modules. Main gaps: DuckDB queries, Prometheus export.

**Architecture Decisions:**
- D5: S3 Tiering Policy with capacity-triggered eviction
- D8: Metadata-local primary write with EC stripes
- D9: Single binary `cfs` with subcommands
- D10: Embedded KV engine in Rust (not RocksDB)

**Dependencies:**
- Tokio 1.40 — async runtime
- Axum 0.7 — HTTP server
- DuckDB 1.0 — analytics queries
- Clap 4.5 — CLI parsing
- Serde — serialization

## Current State

### Implemented (~21k lines)
- ✅ `metrics.rs` (627 lines) — Gauge, Counter, Histogram types
- ✅ `indexer.rs` (1030 lines) — Parquet namespace indexing with journal ops
- ✅ `api.rs` (670 lines) — Axum HTTP server skeleton with routes
- ✅ `analytics.rs` (833 lines) — Analytics engine structure (stub implementation)
- ✅ `cli.rs` — CLI subcommands (Status, Node, Query, TopUsers, etc.)
- ✅ `config.rs` — Config file loading (TOML/JSON)
- ✅ 30+ other modules — topology, QoS, RBAC, webhooks, tracing, compliance, etc.

### Known TODOs / Gaps
1. **DuckDB Implementation** — analytics.rs has TODO comments for query execution
2. **Prometheus Export** — `/metrics` endpoint not wired to Prometheus text format
3. **Scraper Integration** — Parquet indexer needs to flush and reload
4. **Error Handling** — Several stubs return placeholder results
5. **Tests** — Need integration tests for end-to-end workflows

## Task: Complete A8 Management Subsystem

### 1. DuckDB Query Engine (analytics.rs, ~200 lines)

**File:** `crates/claudefs-mgmt/src/analytics.rs`

Current state: `query()` method has a TODO stub.

**Required Implementation:**
```rust
impl AnalyticsEngine {
    /// Execute a SQL query against the Parquet index
    pub fn query(&self, sql: &str) -> anyhow::Result<Vec<HashMap<String, serde_json::Value>>> {
        // 1. Open DuckDB connection to :memory: or persisted DB
        // 2. Register the Parquet files in index_dir as tables:
        //    - CREATE TABLE metadata AS SELECT * FROM read_parquet('index_dir/metadata*.parquet')
        // 3. Execute the user's SQL query
        // 4. Collect results into Vec<HashMap>
        // 5. Return results or AnalyticsError::QueryFailed
    }

    /// Top N users by storage usage
    pub async fn top_users(&self, limit: usize) -> anyhow::Result<Vec<UserStorageUsage>> {
        let sql = format!(
            "SELECT owner_name, SUM(size_bytes) as total_size_bytes, COUNT(*) as file_count
             FROM metadata
             GROUP BY owner_name
             ORDER BY total_size_bytes DESC
             LIMIT {}",
            limit
        );
        // Parse results into UserStorageUsage structs
    }

    /// Top N directories by storage usage (recursive depth)
    pub async fn top_dirs(&self, depth: usize, limit: usize)
        -> anyhow::Result<Vec<DirStorageUsage>> {
        // Use SUBSTR to extract path prefix, GROUP BY at requested depth
        // ORDER BY size DESC, LIMIT
    }

    /// Reduction stats (logical vs stored bytes)
    pub async fn reduction_stats(&self) -> anyhow::Result<Vec<ReductionStats>> {
        // Query metadata for total_logical_bytes, total_stored_bytes
        // Calculate reduction_ratio = logical / stored
    }

    /// Find files matching pattern (glob or regex)
    pub async fn find_files(&self, pattern: &str) -> anyhow::Result<Vec<MetadataRecord>> {
        // Use LIKE or regex on path column
    }

    /// Stale files (not accessed in N days)
    pub async fn stale_files(&self, days: u64) -> anyhow::Result<Vec<MetadataRecord>> {
        // WHERE (current_time - mtime) > days * 86400
    }
}
```

**Notes:**
- Use `duckdb::Connection` for thread-safe access
- Parse results using `FromRow` or manual HashMap parsing
- Handle Parquet file discovery and registration
- All methods should be async (wrap sync DuckDB calls in `tokio::task::spawn_blocking`)

### 2. Prometheus Metrics Export (metrics.rs + new routes in api.rs, ~150 lines)

**File:** `crates/claudefs-mgmt/src/metrics.rs` (extend) + `api.rs` (add route)

**Required Implementation:**

```rust
// In metrics.rs:
impl ClusterMetrics {
    /// Export metrics in Prometheus text format
    pub fn prometheus_export(&self) -> String {
        // Collect all metrics:
        // - Gauges: capacity_used, capacity_total, node_count, etc.
        // - Counters: operations_total, errors_total, etc.
        // - Histograms: request_latency_seconds, data_reduction_ratio, etc.
        //
        // Format as Prometheus text protocol:
        // # HELP metric_name description
        // # TYPE metric_name gauge|counter|histogram
        // metric_name{label="value"} numeric_value
        //
        // Example:
        // # HELP claudefs_cluster_healthy_nodes Number of healthy nodes
        // # TYPE claudefs_cluster_healthy_nodes gauge
        // claudefs_cluster_healthy_nodes 3
    }
}

// In api.rs, add route:
pub async fn metrics_endpoint(
    State(state): State<Arc<ClusterMetrics>>,
) -> impl IntoResponse {
    let prometheus_text = state.prometheus_export();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        prometheus_text,
    )
}

// Register in Router:
let app = Router::new()
    .route("/metrics", get(metrics_endpoint))
    .with_state(metrics)
```

**Notes:**
- Follow Prometheus text format RFC (https://github.com/prometheus/docs/blob/main/content/docs/instrumenting/exposition_formats.md)
- Include standard labels: `instance`, `job`, `cluster`
- Export both raw metrics and computed aggregates (e.g., reduction_ratio)

### 3. Scraper Pool Integration (scraper.rs, ~100 lines)

**File:** `crates/claudefs-mgmt/src/scraper.rs`

Current: Pool exists but scraping logic is placeholder.

**Required Implementation:**
```rust
pub struct ScraperResult {
    pub node_id: String,
    pub samples: HashMap<String, f64>,
    pub timestamp: i64,
}

impl ScraperPool {
    /// Scrape all registered nodes' /metrics endpoints
    pub async fn scrape_all(&self) -> Vec<ScraperResult> {
        // For each node:
        // 1. GET http://{addr}/metrics
        // 2. Parse Prometheus text format
        // 3. Extract metric samples
        // 4. Return ScraperResult with node_id and samples
    }

    /// Parse Prometheus text format into HashMap<metric_name, value>
    fn parse_prometheus_text(body: &str) -> HashMap<String, f64> {
        // Skip lines starting with #
        // For each metric line: "metric_name{labels} value [timestamp]"
        // Extract metric_name and value (f64)
    }
}
```

**Notes:**
- Use `reqwest` client for HTTP scraping
- Parse the Prometheus text format (simple line-based)
- Handle timeouts and errors gracefully (log, skip node)

### 4. Index Flushing & Reloading (indexer.rs, ~80 lines)

**File:** `crates/claudefs-mgmt/src/indexer.rs`

Current: `run_flush_loop()` exists but doesn't persist/reload Parquet files.

**Required Implementation:**
```rust
impl MetadataIndexer {
    /// Main loop: accumulate journal entries, flush to Parquet on interval
    pub async fn run_flush_loop(&self) -> anyhow::Result<()> {
        loop {
            tokio::time::sleep(Duration::from_secs(self.flush_interval_secs)).await;

            if let Err(e) = self.flush_to_parquet().await {
                tracing::error!("Failed to flush index: {}", e);
            }
        }
    }

    /// Serialize accumulated inodes to Parquet file
    async fn flush_to_parquet(&self) -> anyhow::Result<()> {
        // 1. Lock accumulator
        // 2. Collect all InodeState records
        // 3. Write to file: {index_dir}/metadata_{timestamp}.parquet using Arrow/Parquet
        // 4. Clear accumulator
        // 5. Trigger GC (keep last N files)
    }

    /// Reload Parquet files into DuckDB for queries
    pub async fn reload_tables(&self) -> anyhow::Result<()> {
        // Called by analytics engine before queries
        // DuckDB will handle concurrent reads across multiple Parquet files
    }
}
```

**Notes:**
- Flush periodically (configurable, default 60s)
- Use `parquet` crate to write Arrow RecordBatch to Parquet
- Implement GC to keep recent files (e.g., keep 7 days)

### 5. API Routes Wiring (api.rs, ~100 lines)

**File:** `crates/claudefs-mgmt/src/api.rs`

**Add routes:**
```rust
pub async fn query_endpoint(
    State(analytics): State<Arc<AnalyticsEngine>>,
    Query(params): Query<QueryParams>,
) -> impl IntoResponse {
    match analytics.query(&params.sql).await {
        Ok(results) => (StatusCode::OK, Json(results)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

pub async fn top_users_endpoint(
    State(analytics): State<Arc<AnalyticsEngine>>,
    Query(params): Query<TopUsersParams>,
) -> impl IntoResponse {
    // GET /api/v1/analytics/top-users?limit=20
    match analytics.top_users(params.limit).await {
        Ok(results) => Json(results).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// Similar for: top_dirs, reduction_stats, find_files, stale_files
```

### 6. Integration Test (new file: tests/integration_test.rs, ~150 lines)

**File:** `crates/claudefs-mgmt/tests/integration_test.rs`

```rust
#[tokio::test]
async fn test_end_to_end_analytics() {
    // 1. Create temp index directory
    let temp_dir = tempfile::TempDir::new().unwrap();

    // 2. Create indexer and add journal entries
    let indexer = MetadataIndexer::new(temp_dir.path().to_path_buf(), 1);
    indexer.add_entry(JournalEntry { /* ... */ });

    // 3. Flush to Parquet
    indexer.flush_to_parquet().await.unwrap();

    // 4. Create analytics engine
    let analytics = AnalyticsEngine::new(temp_dir.path().to_path_buf());

    // 5. Query and verify
    let results = analytics.top_users(10).await.unwrap();
    assert!(!results.is_empty());
}

#[tokio::test]
async fn test_prometheus_export() {
    let metrics = ClusterMetrics::new();
    metrics.gauge_healthy_nodes.set(3.0);
    metrics.counter_writes.inc();

    let prometheus_text = metrics.prometheus_export();
    assert!(prometheus_text.contains("claudefs_cluster_healthy_nodes"));
    assert!(prometheus_text.contains("3"));
}
```

## Deliverables

1. ✅ **DuckDB Analytics** — Full query engine with top_users, top_dirs, reduction_stats, find_files, stale_files
2. ✅ **Prometheus Export** — `/metrics` endpoint in Prometheus text format
3. ✅ **Scraper Pool** — Scraping and parsing of upstream node metrics
4. ✅ **Index Flushing** — Parquet write + GC
5. ✅ **API Routes** — /api/v1/analytics/* endpoints wired
6. ✅ **Integration Tests** — End-to-end test suite
7. ✅ **No Compiler Errors** — Full `cargo build -p claudefs-mgmt` passes
8. ✅ **No Clippy Warnings** — `cargo clippy -p claudefs-mgmt` clean

## Files to Modify

1. `crates/claudefs-mgmt/src/analytics.rs` — Implement DuckDB queries
2. `crates/claudefs-mgmt/src/metrics.rs` — Add prometheus_export()
3. `crates/claudefs-mgmt/src/api.rs` — Add analytics routes + metrics endpoint
4. `crates/claudefs-mgmt/src/scraper.rs` — Implement scraping logic
5. `crates/claudefs-mgmt/src/indexer.rs` — Implement Parquet flushing
6. `crates/claudefs-mgmt/tests/integration_test.rs` — New integration tests

## Assumptions

- DuckDB will be embedded (bundled feature already in Cargo.toml)
- Parquet files registered dynamically in DuckDB queries
- Metrics accumulate in memory (in-process, no external store)
- Scraper respects 30-second timeout per node
- Index files retained for 7 days (configurable)

## Deliverable Validation

```bash
cargo build -p claudefs-mgmt  # Must pass
cargo test -p claudefs-mgmt   # Tests must pass
cargo clippy -p claudefs-mgmt # No warnings
```

After implementation:
- `/metrics` endpoint returns Prometheus text format
- `/api/v1/analytics/top-users?limit=20` returns user storage stats
- DuckDB queries work against Parquet index
- All modules compile without warnings
