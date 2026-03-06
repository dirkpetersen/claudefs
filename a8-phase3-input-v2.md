# A8 Phase 3: Query Gateway, Web API, CLI & Dashboards Implementation

## Context

Agent A8 (Management) owns the `claudefs-mgmt` crate. Phase 2 is complete with 965 tests and 43 modules covering:
- Prometheus metrics collection
- Metadata journal consumer (A2 integration)
- DuckDB analytics engine with query methods
- Metadata indexing (Parquet writer and flushing)

Phase 3 builds on this foundation to deliver the Query Gateway, Web API, CLI enhancements, and Grafana dashboard templates.

## Location

All code goes in `/home/cfs/claudefs/crates/claudefs-mgmt/src/`

## Available Dependencies (already in Cargo.toml)

- tokio (async runtime)
- duckdb (for DuckDB connections)
- bb8 (connection pooling)
- dashmap (for query caching)
- thiserror (for error types)
- serde, serde_json (serialization)
- chrono (timestamps)
- axum, tower, hyper, http (HTTP server)
- jsonwebtoken (JWT auth)
- base64 (token decoding)

## Implementation Tasks

### 1. query_gateway.rs (10-12 tests) - NEW MODULE

Create a DuckDB connection pool with query caching and timeouts.

Key types:
```rust
pub struct QueryGateway {
    connections: Arc<DuckDbConnectionPool>,
    cache: Arc<QueryResultCache>,
    query_timeout_secs: u32,
    max_result_rows: usize,
}

pub struct DuckDbConnectionPool {
    available: Arc<tokio::sync::Mutex<Vec<Connection>>>,
    in_use: Arc<std::sync::atomic::AtomicUsize>,
    max_connections: usize,
    index_dir: std::path::PathBuf,
}

pub struct QueryResultCache {
    cache: Arc<DashMap<String, CachedResult>>,
    ttl_secs: u32,
    hits: Arc<std::sync::atomic::AtomicUsize>,
    misses: Arc<std::sync::atomic::AtomicUsize>,
}

pub struct CachedResult {
    rows: Vec<Row>,
    cached_at: std::time::Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub columns: Vec<String>,
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub available: usize,
    pub in_use: usize,
    pub max: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub entries: usize,
}

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("connection pool exhausted")]
    PoolExhausted,
    #[error("query timeout after {0} seconds")]
    Timeout(u32),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("no results")]
    NoResults,
    #[error("DuckDB error: {0}")]
    DuckDbError(String),
}
```

Methods:
```rust
impl QueryGateway {
    pub fn new(index_dir: &str, max_connections: usize, query_timeout_secs: u32, max_result_rows: usize) -> Result<Self, QueryError>
    pub async fn execute_query(&self, query: &str, use_cache: bool) -> Result<Vec<Row>, QueryError>
    pub async fn execute_parameterized(&self, query: &str, params: &[String], use_cache: bool) -> Result<Vec<Row>, QueryError>
    pub fn get_pool_stats(&self) -> PoolStats
    pub fn clear_cache(&self)
    pub fn get_cache_stats(&self) -> CacheStats
}
```

Use spawn_blocking for DuckDB operations like analytics.rs does.

Tests required:
1. test_query_gateway_new_succeeds
2. test_query_gateway_execute_query_succeeds
3. test_query_gateway_execute_query_timeout
4. test_query_gateway_execute_query_with_cache_hit
5. test_query_gateway_execute_query_with_cache_miss
6. test_query_gateway_execute_query_with_cache_ttl_expiry
7. test_query_gateway_parameterized_query_sql_injection_safe
8. test_query_gateway_execute_query_no_results
9. test_query_gateway_connection_pool_exhaustion
10. test_query_gateway_connection_pool_concurrent_requests
11. test_query_gateway_clear_cache
12. test_query_gateway_get_pool_stats

---

### 2. web_api.rs (8-10 tests) - NEW MODULE

Create an Axum HTTP server with REST endpoints for analytics queries.

Key types:
```rust
pub struct ApiServer {
    gateway: Arc<QueryGateway>,
    auth: Arc<crate::web_auth::AuthManager>,
    health: Arc<crate::health::HealthAggregator>,
}

#[derive(serde::Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub timestamp: i64,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self { ... }
    pub fn err(msg: String) -> Self { ... }
}

#[derive(serde::Serialize)]
pub struct ClusterHealthStatus {
    pub status: HealthState,
    pub nodes: usize,
    pub replication_lag_ms: u64,
    pub capacity_used_percent: u32,
    pub last_check_at: i64,
}

#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Healthy,
    Degraded,
    Critical,
}
```

Routes:
- GET /api/v1/health → ClusterHealthStatus
- GET /api/v1/analytics/top-users?limit=20 → Vec<UserStorageUsage>
- GET /api/v1/analytics/top-dirs?limit=20 → Vec<DirStorageUsage>
- GET /api/v1/analytics/stale-files?days=30&limit=100 → Vec<MetadataRecord>
- GET /api/v1/analytics/file-types → HashMap<String, u64>
- GET /api/v1/analytics/reduction-report → Vec<ReductionStats>
- POST /api/v1/analytics/query → requires admin role

Error codes:
- 400: Invalid parameters
- 401: Missing/invalid auth token
- 403: Insufficient RBAC permissions
- 408: Query timeout
- 500: Internal error

Tests required:
1. test_api_server_new_succeeds
2. test_api_server_health_endpoint_returns_cluster_status
3. test_api_server_top_users_endpoint_returns_results
4. test_api_server_top_users_endpoint_validates_parameters
5. test_api_server_query_endpoint_requires_admin_role
6. test_api_server_query_endpoint_timeout_returns_408
7. test_api_server_json_error_response_format
8. test_api_server_stale_files_endpoint
9. test_api_server_file_types_endpoint
10. test_api_server_reduction_report_endpoint

---

### 3. web_auth.rs (5-7 tests) - NEW MODULE

JWT validation and RBAC middleware.

Key types:
```rust
pub struct AuthManager {
    jwt_secret: String,
    role_cache: Arc<DashMap<String, UserRole>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UserRole {
    Admin,
    Operator,
    Viewer,
    TenantAdmin(String),
}

#[derive(Debug, Deserialize)]
pub struct AuthClaim {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub roles: Vec<String>,
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("missing authorization token")]
    MissingToken,
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("token expired")]
    TokenExpired,
    #[error("insufficient permission")]
    InsufficientPermission,
}
```

Methods:
```rust
impl AuthManager {
    pub fn new(jwt_secret: &str) -> Self
    pub async fn verify_token(&self, token: &str) -> Result<AuthClaim, AuthError>
    pub fn has_permission(role: &UserRole, action: &str) -> bool
}
```

Permissions:
- Admin: all operations
- Operator: read, top_users, top_dirs, stale_files, health
- Viewer: read-only
- TenantAdmin(tenant): read + write within tenant

Tests required:
1. test_auth_manager_new_succeeds
2. test_auth_manager_verify_token_valid
3. test_auth_manager_verify_token_expired
4. test_auth_manager_verify_token_invalid_signature
5. test_auth_manager_has_permission_admin
6. test_auth_manager_has_permission_viewer_read_only
7. test_auth_manager_require_role_middleware_blocks_insufficient_role

---

### 4. dashboards.rs (4-5 tests) - NEW MODULE

Pre-built Grafana dashboard JSON templates.

Key types:
```rust
pub struct DashboardGenerator;

#[derive(Debug, Clone)]
pub struct GrafanaDashboard {
    pub title: String,
    pub panels: Vec<GrafanaPanel>,
    pub refresh: String,
}

#[derive(Debug, Clone)]
pub struct GrafanaPanel {
    pub title: String,
    pub datasource: String,
    pub targets: Vec<PrometheusTarget>,
    pub panel_type: String,
}

#[derive(Debug, Clone)]
pub struct PrometheusTarget {
    pub expr: String,
    pub legend: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardTemplate {
    ClusterHealth,
    TopConsumers,
    CapacityTrends,
    ReductionAnalytics,
}
```

Methods:
```rust
impl DashboardGenerator {
    pub fn generate_dashboard(template: DashboardTemplate) -> GrafanaDashboard
    pub fn export_dashboard_json(dashboard: &GrafanaDashboard) -> String
    pub fn generate_all_dashboards() -> Vec<(String, String)>
}
```

Dashboard panels:
1. ClusterHealth: Node status, capacity, replication lag
2. TopConsumers: Bar chart of top users/directories
3. CapacityTrends: Time-series of capacity
4. ReductionAnalytics: Dedup ratio, compression ratio

Tests required:
1. test_dashboard_generator_cluster_health_dashboard
2. test_dashboard_generator_top_consumers_dashboard
3. test_dashboard_generator_export_dashboard_json_valid_format
4. test_dashboard_generator_all_dashboards_generated
5. test_dashboard_generator_grafana_panel_targets_correct_datasource

---

### 5. Integration Tests

Add these tests to an existing test module in claudefs-mgmt:
1. test_e2e_parquet_indexing_to_api_response
2. test_e2e_cli_top_users_invokes_api
3. test_e2e_grafana_dashboard_metrics_available
4. test_e2e_auth_jwt_token_flow
5. test_e2e_query_caching_and_invalidation
6. test_e2e_performance_100k_records_query_completes_in_500ms

---

## Export in lib.rs

Add to lib.rs:
```rust
pub mod query_gateway;
pub mod web_api;
pub mod web_auth;
pub mod dashboards;

pub use query_gateway::{QueryGateway, QueryError, Row, PoolStats, CacheStats};
pub use web_api::{ApiServer, ApiResponse, ClusterHealthStatus, HealthState};
pub use web_auth::{AuthManager, UserRole, AuthClaim, AuthError};
pub use dashboards::{DashboardGenerator, GrafanaDashboard, GrafanaPanel, DashboardTemplate};
```

---

## Requirements

1. All error types use `#[derive(thiserror::Error)]` pattern
2. Use `async/await` with tokio runtime
3. Keep line counts reasonable (< 400 lines per file)
4. Add module-level documentation comments
5. Use existing patterns from analytics.rs for consistency
6. All tests must pass
7. No clippy warnings