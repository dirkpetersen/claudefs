# A8 Phase 3: Block 1-2 Implementation — Query Gateway & Web API

**Goal:** Implement the query gateway (DuckDB connection pool, parameterized queries) and Web API (Axum routes for analytics endpoints).

**Current State:**
- Phase 2 complete: 965 tests, analytics engine with top_users/top_dirs/reduction_stats queries
- Parquet files written to index directory by indexer.rs
- MetadataRecord schema defined in analytics.rs
- Need: Query gateway for DuckDB access + Web API for HTTP endpoints

**Target:** 30+ new tests across both blocks, 1000+ total tests by Phase 3 end

---

## Part 1: query_gateway.rs (10-12 tests)

### Requirements

1. **QueryGateway struct**: Manages persistent DuckDB connection + query cache
   - `new(index_dir: PathBuf) -> Self` — Initialize with Parquet directory
   - `execute_query(query: &str) -> Result<QueryResult>` — Run parameterized query
   - Methods: `set_timeout(duration)`, `clear_cache()`, etc.

2. **Query caching** with 10-minute TTL
   - Cache key: hash of (query string + parameters)
   - LRU eviction for memory efficiency
   - Use `dashmap::DashMap` for thread-safe cache

3. **Parameterized queries** to prevent SQL injection
   - Accept query + parameters struct
   - Substitute parameters safely (bind variables)
   - Example: `execute_query("SELECT * FROM metadata WHERE owner_uid = $1", vec![1000])`

4. **Query timeout support**
   - Default 30 seconds, configurable
   - Use tokio::time::timeout to enforce
   - Return error on timeout: `QueryError::Timeout`

5. **Result streaming** for large datasets
   - Chunk results into batches (e.g., 1000 rows per chunk)
   - StreamedResult enum: `Complete(Vec<Row>)` | `Chunk { rows, has_more }`

6. **Error handling**
   - QueryError enum: DuckDbError(String), Timeout, SqlInjection, IoError(String)
   - Graceful connection recovery

### Tests (12 total)

```rust
#[test]
fn test_query_gateway_new() {
    // Creates QueryGateway with valid index path
    // Verifies internal state initialized
}

#[test]
fn test_query_gateway_execute_simple_query() {
    // Execute: "SELECT COUNT(*) FROM metadata"
    // Verify result is numeric
}

#[test]
fn test_query_gateway_execute_with_parameters() {
    // Execute: "SELECT * FROM metadata WHERE owner_uid = $1" with uid=1000
    // Verify parameter substitution
}

#[test]
fn test_query_gateway_sql_injection_prevention() {
    // Attempt SQL injection: "SELECT * WHERE path = $1" with "$1 OR 1=1"
    // Verify injection is rejected or safely escaped
}

#[test]
fn test_query_gateway_query_timeout() {
    // Set timeout to 100ms, execute slow query
    // Verify QueryError::Timeout returned
}

#[test]
fn test_query_gateway_result_caching() {
    // Execute same query twice
    // Verify second call returns from cache (faster)
    // Inspect cache stats
}

#[test]
fn test_query_gateway_cache_invalidation() {
    // Execute query, verify cached
    // Call clear_cache()
    // Execute same query, verify not from cache
}

#[test]
fn test_query_gateway_streaming_results() {
    // Execute query that returns 5000 rows
    // Verify results returned in chunks
    // Verify has_more flag for pagination
}

#[test]
fn test_query_gateway_error_handling_bad_sql() {
    // Execute malformed SQL
    // Verify QueryError::DuckDbError with message
}

#[test]
fn test_query_gateway_connection_recovery() {
    // Simulate connection loss (mock)
    // Verify automatic reconnection on next query
}

#[test]
fn test_query_gateway_concurrent_queries() {
    // Spawn 10 tokio tasks, each executing a query
    // Verify all succeed, cache is thread-safe
}

#[test]
fn test_query_gateway_performance_cached_vs_uncached() {
    // Time uncached query, then cached query
    // Verify cached is significantly faster
}
```

### Implementation Notes

- Use `duckdb` 1.0 crate already in Cargo.toml
- DuckDB connection opened once, reused for all queries (one per QueryGateway instance)
- Query timeout via `tokio::time::timeout` wrapping `spawn_blocking`
- Cache: `DashMap<String, CachedResult>` where `CachedResult = (ResultData, Instant)`
- TTL check on retrieval: if expired, evict and re-execute

---

## Part 2: parquet_schema.rs (4-6 tests)

### Requirements

1. **ParquetSchema struct** — Central schema definition
   - Defines: inode, path, filename, parent_path, owner_uid, owner_name, group_gid, group_name, size_bytes, blocks_stored, mtime, ctime, file_type, is_replicated
   - Matches InodeState from indexer.rs

2. **Arrow type mappings**
   - u64 inode → arrow::datatypes::DataType::UInt64
   - String path → arrow::datatypes::DataType::Utf8
   - i64 mtime/ctime → arrow::datatypes::DataType::Int64
   - bool is_replicated → arrow::datatypes::DataType::Boolean

3. **Schema validation**
   - Verify Parquet file conforms to expected schema
   - Check column presence and types

4. **Schema versioning**
   - Version field in schema: `schema_version: u32`
   - Support migration from v1 → v2 (future-proofing)

5. **Type conversions**
   - From DuckDB query result → Rust structs (MetadataRecord, etc.)
   - Handle NULL values gracefully

### Tests (6 total)

```rust
#[test]
fn test_parquet_schema_definition() {
    // Create ParquetSchema
    // Verify all 14 fields present
    // Verify field types correct
}

#[test]
fn test_parquet_schema_arrow_types() {
    // Convert to Arrow schema
    // Verify inode is UInt64, path is Utf8, etc.
}

#[test]
fn test_parquet_schema_validation_valid_file() {
    // Load a valid Parquet file
    // Verify schema matches
}

#[test]
fn test_parquet_schema_validation_invalid_file() {
    // Create Parquet with wrong schema
    // Verify validation fails with clear error
}

#[test]
fn test_parquet_schema_row_conversion() {
    // Convert DuckDB row → MetadataRecord
    // Verify all fields mapped correctly
    // Verify NULL handling (e.g., owner_name defaults to "unknown")
}

#[test]
fn test_parquet_schema_versioning() {
    // Read schema_version field
    // Support v1 and v2 formats
    // Verify migration works
}
```

### Implementation Notes

- Reference `arrow-rs` crate for Arrow type definitions
- `DuckDB` has built-in Parquet support, use `duckdb::arrow::RecordBatch` for row access
- NULL values: use Option<T> in conversion layer
- Schema versioning: store version in Parquet metadata or file header

---

## Part 3: web_api.rs (8-10 tests)

### Requirements

1. **Axum HTTP routes** for analytics
   - All routes under `/api/v1/analytics/` or `/api/v1/cluster/`
   - JSON request/response
   - Query parameters for filtering/pagination

2. **Routes to implement:**
   - `GET /api/v1/analytics/top-users?limit=20` → UserStorageUsage[]
   - `GET /api/v1/analytics/top-dirs?depth=2&limit=10` → DirStorageUsage[]
   - `GET /api/v1/analytics/stale-files?days=180` → [{ path, mtime }]
   - `GET /api/v1/analytics/file-types` → [{ ext, count, total_bytes }]
   - `GET /api/v1/analytics/reduction-report?path=/data` → ReductionStats
   - `GET /api/v1/cluster/health` → { node_count, iops, capacity_used % }
   - `POST /api/v1/query` → execute custom SQL (with RBAC check)

3. **Error handling**
   - 400 Bad Request: invalid parameters
   - 404 Not Found: path doesn't exist
   - 504 Gateway Timeout: query timeout
   - 500 Internal Server Error: other failures

4. **Rate limiting (optional but recommended)**
   - Max 100 queries per minute per IP
   - Use tower middleware or simple in-memory counter

5. **CORS support**
   - Allow requests from `http://localhost:3000` (dev) and configured origins

### Tests (10 total)

```rust
#[test]
fn test_web_api_routes_registered() {
    // Build router
    // Send OPTIONS to each endpoint
    // Verify all routes respond (200 or 405)
}

#[test]
async fn test_web_api_top_users_endpoint() {
    // GET /api/v1/analytics/top-users?limit=5
    // Verify response: [ { owner_name, total_size_bytes, file_count }, ... ]
}

#[test]
async fn test_web_api_top_users_default_limit() {
    // GET /api/v1/analytics/top-users (no limit param)
    // Verify default limit=20 applied
}

#[test]
async fn test_web_api_top_dirs_endpoint() {
    // GET /api/v1/analytics/top-dirs?depth=3&limit=10
    // Verify response: dirs aggregated at given depth
}

#[test]
async fn test_web_api_stale_files_endpoint() {
    // GET /api/v1/analytics/stale-files?days=180
    // Verify response: files not accessed in 180 days
}

#[test]
async fn test_web_api_file_types_endpoint() {
    // GET /api/v1/analytics/file-types
    // Verify response: extension distribution
}

#[test]
async fn test_web_api_reduction_report_endpoint() {
    // GET /api/v1/analytics/reduction-report?path=/data
    // Verify response: { logical_bytes, stored_bytes, reduction_ratio }
}

#[test]
async fn test_web_api_cluster_health_endpoint() {
    // GET /api/v1/cluster/health
    // Verify response: { node_count, iops, capacity_pct }
}

#[test]
async fn test_web_api_custom_query_endpoint() {
    // POST /api/v1/query { "sql": "SELECT COUNT(*) FROM metadata" }
    // Verify response: query result
}

#[test]
async fn test_web_api_error_handling_malformed_params() {
    // GET /api/v1/analytics/top-users?limit=invalid
    // Verify 400 Bad Request
}

#[test]
async fn test_web_api_error_handling_query_timeout() {
    // POST /api/v1/query { "sql": "SELECT * FROM metadata CROSS JOIN metadata" }
    // Verify 504 timeout after 30 seconds
}
```

### Implementation Notes

- Use Axum router builder: `Router::new().route("/path", get(handler))`
- Each handler: `async fn handler(...) -> Result<Json<Response>, ApiError>`
- Error responses: wrap in `impl IntoResponse`
- Query gateway: wire QueryGateway into router state via `Extension<Arc<QueryGateway>>`
- CORS: use `tower_http::cors::CorsLayer`

---

## Implementation Order

1. **query_gateway.rs** first (foundation for all queries)
2. **parquet_schema.rs** next (type conversions)
3. **web_api.rs** last (uses both above)

Run tests after each module:
```bash
cargo test -p claudefs-mgmt --lib query_gateway
cargo test -p claudefs-mgmt --lib parquet_schema
cargo test -p claudefs-mgmt --lib web_api
```

---

## Integration with Existing Code

- Use `AnalyticsEngine::query()` method for DuckDB access
- Integrate `QueryGateway` into lib.rs exports
- Web API will be wired into binary's main.rs server setup (not in this PR)

---

## Files to Create/Modify

- **Create:** `src/query_gateway.rs` (150-200 lines)
- **Create:** `src/parquet_schema.rs` (100-150 lines)
- **Create:** `src/web_api.rs` (200-300 lines)
- **Modify:** `src/lib.rs` — add `pub mod query_gateway; pub mod parquet_schema; pub mod web_api;`

---

## External Dependencies Already in Cargo.toml

- tokio 1.40+
- axum 0.7+
- duckdb 1.0+
- serde/serde_json
- thiserror
- chrono
- dashmap (already added in Phase 2)
- uuid (already added in Phase 2)

---

## Success Criteria

- All 30+ new tests pass
- `cargo build -p claudefs-mgmt` succeeds with no errors
- `cargo clippy -p claudefs-mgmt` shows no warnings (for new code)
- Integration tests verify end-to-end query workflow
