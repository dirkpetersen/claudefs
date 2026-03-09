# A8: query_gateway.rs Implementation

Implement `query_gateway.rs` in `crates/claudefs-mgmt/src/`. This module manages DuckDB queries with caching, timeouts, and result streaming.

## Module: query_gateway.rs

```rust
// PUBLIC API

pub struct QueryGateway {
    // DuckDB connection + query cache
}

impl QueryGateway {
    pub fn new(index_dir: PathBuf) -> Result<Self, QueryError>;
    pub async fn execute_query(&self, query: &str) -> Result<QueryResult, QueryError>;
    pub fn set_timeout(&mut self, duration: Duration);
    pub fn clear_cache(&self);
}

pub enum QueryError {
    DuckDbError(String),
    Timeout,
    SqlInjection,
    IoError(String),
}

pub struct QueryResult {
    pub rows: Vec<HashMap<String, serde_json::Value>>,
}
```

## Requirements

1. Initialize DuckDB connection to Parquet directory
2. Implement 10-minute TTL cache using `DashMap<String, (ResultData, Instant)>`
3. Query timeout: default 30 seconds, configurable via `set_timeout()`
4. LRU eviction: keep cache under 1000 entries
5. Result streaming for large datasets (return batches of 1000 rows max)
6. Thread-safe execution via `tokio::task::spawn_blocking`

## Tests (12 total)

Write 12 unit tests covering:
- `test_query_gateway_new` — initialization
- `test_query_gateway_execute_simple_query` — basic query execution
- `test_query_gateway_execute_with_parameters` — parameterized queries
- `test_query_gateway_sql_injection_prevention` — security
- `test_query_gateway_query_timeout` — timeout enforcement
- `test_query_gateway_result_caching` — cache hit/miss
- `test_query_gateway_cache_invalidation` — cache clearing
- `test_query_gateway_streaming_results` — pagination
- `test_query_gateway_error_handling_bad_sql` — error paths
- `test_query_gateway_connection_recovery` — resilience
- `test_query_gateway_concurrent_queries` — concurrency
- `test_query_gateway_performance_cached_vs_uncached` — performance

## Dependencies

- duckdb 1.0+ (already in Cargo.toml)
- tokio (already in Cargo.toml)
- dashmap 5.5+ (already in Cargo.toml)
- serde_json (already in Cargo.toml)
- thiserror (already in Cargo.toml)

## Implementation Strategy

1. Create QueryError enum with 4 variants
2. Create QueryResult struct with Vec<HashMap<String, Value>>
3. Create QueryGateway struct with connection + cache fields
4. Implement `new()` — open DuckDB connection to Parquet files
5. Implement `execute_query()` — check cache, execute if miss, update cache
6. Implement timeout via `tokio::time::timeout` + `spawn_blocking`
7. Write all 12 tests with assertions

Keep code under 300 lines, well-commented. All tests must pass: `cargo test -p claudefs-mgmt --lib query_gateway`.
